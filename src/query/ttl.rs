//! REQ-127 · **查询服务上的 OWL TTL 两视图**（客户视图默认 / 建模视图显式要）。
//!
//! # 一句话
//!
//! Agent 拿 query token 就能下载**整份** OWL（Turtle），而不是只能一问一答；
//! 默认给**客户视图**，建模视图必须显式 `?view=model`（R1）。
//!
//! # 为什么这里没有一行序列化代码（纪律 H）
//!
//! 两视图的字节**只可能**来自既有链路：
//!
//! ```text
//! prepare_ttl_export(…)  ──▶  TtlPrepared（快照 / 投影 / 两份声明）
//!                                │
//!                                ▼
//! console::ops::render_ttl_export(…)  ──▶  pipeline::render(C6 / D9)
//!                                              │
//!                                              ▼
//!                                   domain::ttl 的**唯一**序列化器
//! ```
//!
//! 本模块做三件事：① **请求时**把两视图各渲染**一次**并缓存（版本冻结 ⇒ 内容不变）；
//! ② 从正文算出 sha256 / ETag；③ 按视图取字节。⛔ 不另写序列化器、⛔ 不二次加工正文
//! （否则「与 CLI 逐字节一致」立刻不成立）。
//!
//! # ⭐ REQ-178 · 为什么改成「按需」
//!
//! 改前：`query::serve::prepare_project` 在**装配期**渲染（fail-closed）。客户视图只要
//! 命中一条发布闸（往往只是**一条说明文字**），整个项目就 500 `project.load_failed` ——
//! catalog / describe / sparql / Studio 一起打不开。
//!
//! 改后：装配期只留 [`TtlPlan`]（版本坐标 + 选项 + api 类），**请求时**才渲染
//! （[`TtlCache`]），按 `version_id` 缓存；失败**只**让 `/api/ontology.ttl` 500
//! `project.ttl_failed`（[`TTL_FAILED`]），其余出口不受影响。
//!
//! # 客户视图 = `ontok ontology export`（不带 `--schema` / `--tenancy`）
//!
//! 客户视图**结构上不含列名**，`--schema` 对它唯一的影响是文件头里那句
//! 「类型来源」说明（`TypeSourceMode::CustomerNoColumns` vs `HeuristicOnly`）。
//! 这里固定用 `schema: None` 构造客户视图 ⇒ 与**不带 `--schema`** 的 CLI 导出逐字节相同。
//! 建模视图则用服务端的真实 `--schema` 与租户声明（`--tenancy`）—— 与 CLI 同参数。
//!
//! # 确定性
//!
//! 内容里没有时间戳（REQ-119 的既有约束），输入是冻结版本 ⇒ 同一进程内每次响应逐字节
//! 相同；进程重启后重算也相同（AC-005 的第一半）。ETag = 正文 sha256 的**前 16 位**
//! （R4），带引号（HTTP 的 entity-tag 语法）。
//!
//! # 已知限制
//!
//! - 两视图在**第一次请求**时渲染并常驻内存（客户视图 + 建模视图约 0.9 MB）：
//!   同一个版本只付一次代价 —— [`TtlCache`] 的实例跟着 `QueryService` 走，而服务在两种
//!   模式下都与 `version_id` 一一对应（多项目模式按 `version_id` 缓存服务，单项目模式
//!   进程内只有一版）。换版本 = 换服务 = 换缓存，⛔ 不需要第二套失效机制。
//! - ⭐ REQ-178 · 渲染失败**只影响 TTL 这一个出口**（500 [`TTL_FAILED`]），不再拒绝启动：
//!   病根是「一个词写错不该让整个项目打不开」。
//! - 并发首次请求可能各渲染一次（内容相同、后写入的赢）—— 与 `query::multi` 的
//!   已知边界同一条；⛔ 不为它跨 `await` 持锁（那会把热路径串行化）。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use sqlx::postgres::PgPool;

use crate::domain::model_view::ModelTrackView;
use crate::domain::pipeline::{self, OutletId, OutletRender, Product};
use crate::domain::ttl::{SchemaTypes, TenancyDecl, TtlOptions};
use crate::domain::view::CustomerView;

use super::audit::sha256_hex;

/// ETag 取内容 sha256 的**前 N 位**（R4）。
pub const ETAG_HEX_LEN: usize = 16;

/// 视图名（`?view=` 的取值；审计里也写它）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TtlView {
    /// 默认：客户视图。
    Customer,
    /// 显式 `?view=model`：建模视图。
    Model,
}

impl TtlView {
    pub const fn as_str(self) -> &'static str {
        match self {
            TtlView::Customer => "customer",
            TtlView::Model => "model",
        }
    }

    /// `?view=` 的解析。⛔ 不认识的取值不静默回退成客户视图（那会让「要建模」变成「拿到客户」）。
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim() {
            "customer" => Some(TtlView::Customer),
            "model" => Some(TtlView::Model),
            _ => None,
        }
    }
}

/// 一份**已经渲染好**的 TTL 字节 + 它的摘要。
#[derive(Debug, Clone)]
pub struct TtlBody {
    text: String,
    sha256: String,
    etag: String,
}

impl TtlBody {
    fn new(text: String) -> Self {
        let sha256 = sha256_hex(text.as_bytes());
        let etag = format!("\"{}\"", &sha256[..ETAG_HEX_LEN]);
        Self { text, sha256, etag }
    }

    /// 正文（**管线产物本身**；⛔ 不在这里再加工）。
    pub fn text(&self) -> &str {
        &self.text
    }

    /// 正文字节数（审计里的 `bytes=`）。
    pub fn bytes(&self) -> usize {
        self.text.len()
    }

    /// 正文 sha256（64 位十六进制；与 CLI 文件对拍用的就是它）。
    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    /// HTTP entity-tag（`"<sha256 前 16 位>"`，带引号）。
    pub fn etag(&self) -> &str {
        &self.etag
    }
}

/// ⭐ REQ-178 · **这一版的 TTL 产不出来** —— 只影响 TTL 这一个出口。
///
/// ⛔ 不要退回 `project.load_failed`：那个码的语义是「项目装不起来」（catalog / describe /
/// sparql 也一起挂）。TTL 是**一个出口**的产物 —— 它失败不该让整个项目下线。
pub const TTL_FAILED: &str = "project.ttl_failed";

/// ⭐ REQ-178 · 渲染失败：**原始原因 + 命中的词条**（两个都不许丢）。
///
/// `hits` 来自 [`crate::domain::pipeline::PipelineError::hits`]。REQ-106 的口径是
/// 「命中词条只进 stderr」，但 REQ-178 AC-001 要求这个端点的错误正文**点名**它们：
/// 它们本来就是**客户视图正文里**的那些词（闸门正是从正文里扫出来的），
/// 对调用方不构成新信息。服务端 stderr 仍然照写一份（排障用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TtlFailure {
    /// 出问题的出口（`C6` 客户视图 / `D9` 建模视图；读取阶段失败时为 `None`）。
    pub outlet: Option<&'static str>,
    /// 原始原因（人话；⛔ 不含命中词条 —— 那些在 [`Self::hits`]）。
    pub message: String,
    /// 发布闸命中的词条（非闸门失败时为空）。
    pub hits: Vec<String>,
}

impl TtlFailure {
    /// 读取 / 投影阶段的失败（没有出口、没有命中词条）。
    fn read(message: String) -> Self {
        Self {
            outlet: None,
            message,
            hits: Vec::new(),
        }
    }

    /// 渲染阶段的失败（`PipelineError` 带出口与命中词条）。
    fn from_pipeline(e: crate::domain::pipeline::PipelineError) -> Self {
        Self {
            outlet: Some(e.outlet()),
            message: e.message().to_string(),
            hits: e.hits().to_vec(),
        }
    }
}

impl std::fmt::Display for TtlFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for TtlFailure {}

/// ⭐ REQ-178 · 渲染两视图**需要的东西**（装配期只留这一份，⛔ 不留字节）。
///
/// 与 [`TtlSource`] 的区别只有一个：那份**借**字符串 / 路径（装配期调一次就丢），
/// 这份**拥有**（要活到请求期）。⛔ 不把整个快照搬进来 —— `prepare_ttl_export` 会按
/// `project` / `version` 从元数据库重读快照（与 CLI 同一条读取段，字节才不会漂）。
#[derive(Debug, Clone)]
pub struct TtlPlan {
    pub project: String,
    pub version: i32,
    pub base: String,
    /// `--schema` 的服务端路径（只影响**建模视图**；客户视图结构上不含列名）。
    pub schema: Option<PathBuf>,
    /// `--tenancy` 的服务端路径（只影响**建模视图**）。
    pub tenancy: Option<PathBuf>,
    /// `--access-mapping` 里的 api 类（`None` = 没加载映射 ⇒ 两视图不变，R1）。
    pub api_classes: Option<crate::domain::ttl::ApiClasses>,
}

impl TtlPlan {
    /// 借成 [`TtlSource`]（[`OntologyTtl::prepare`] 的入参）。
    fn source(&self) -> TtlSource<'_> {
        TtlSource {
            project: &self.project,
            version: self.version,
            base: &self.base,
            schema: self.schema.as_deref(),
            tenancy: self.tenancy.as_deref(),
            api_classes: self.api_classes.as_ref(),
        }
    }
}

/// ⭐ REQ-178 · 两视图的**按需**缓存：一个实例 = 一份服务 = 一个 `version_id`。
///
/// # 缓存键为什么"看起来"没有键
///
/// `QueryService` 在两种模式下都与版本一一对应：多项目模式按 `version_id` 缓存服务
/// （`query::multi` 的 `CACHE_CAP`），单项目模式进程内只有启动时钉死的那一版。
/// 于是「换版本」= 换服务 = 换缓存 —— 缓存键**就是**这个实例的归属，
/// ⛔ 不要再存一个键（那会长出第二套失效语义，正是 REQ-174 消灭过的形态）。
///
/// # 失败不缓存
///
/// 版本冻结 ⇒ 成功结果永远不会过期；而失败可能是部署级输入（schema / tenancy 路径）
/// 的问题，下一次请求可能就好了。所以失败**只记次数、不留结果**。
pub struct TtlCache {
    source: CacheSource,
    state: Mutex<Option<Arc<OntologyTtl>>>,
    /// 真渲染次数（失败也算一次）。判据靠它断言「第二次不重渲染」。
    renders: AtomicUsize,
}

enum CacheSource {
    /// 已经渲染好（判据 / 夹具；⛔ 生产路径不走它）。
    Ready,
    /// 生产：拿坐标在请求期渲染。
    OnDemand { meta: PgPool, plan: TtlPlan },
}

impl TtlCache {
    /// 装配期用：只留坐标，**不渲染**（第一次请求才付渲染代价）。
    pub fn lazy(meta: PgPool, plan: TtlPlan) -> Self {
        Self {
            source: CacheSource::OnDemand { meta, plan },
            state: Mutex::new(None),
            renders: AtomicUsize::new(0),
        }
    }

    /// 判据 / 夹具用：已经渲染好的一对字节。
    pub fn preloaded(ttl: OntologyTtl) -> Self {
        Self {
            source: CacheSource::Ready,
            state: Mutex::new(Some(Arc::new(ttl))),
            renders: AtomicUsize::new(0),
        }
    }

    /// 已经渲染好的那一对（没渲染过 / 渲染失败 ⇒ `None`）。
    pub fn ready(&self) -> Option<Arc<OntologyTtl>> {
        lock(&self.state).clone()
    }

    /// 真渲染次数（含失败的尝试；`preloaded` 恒 0）。
    pub fn renders(&self) -> usize {
        self.renders.load(Ordering::SeqCst)
    }

    /// 取两视图：命中直接返回；否则**渲染一次**并放进缓存。
    pub async fn get(&self) -> Result<Arc<OntologyTtl>, TtlFailure> {
        if let Some(ttl) = self.ready() {
            return Ok(ttl);
        }
        let CacheSource::OnDemand { meta, plan } = &self.source else {
            // Ready 却没有字节：只可能是内部状态坏了，⛔ 不编一份字节。
            return Err(TtlFailure::read(
                "两视图缓存状态不一致（标记为已完成，却没有字节）—— 这是 bug".into(),
            ));
        };
        // ⚠️ `TtlSource` 借的是 `plan`（= `self.source`），必须先落到一个具名值上：
        // 直接在闭包里 `&plan.source()` 会借到一个临时值（跨 `await` 不成立）。
        let src = plan.source();
        self.get_or_render_with(move || async move { OntologyTtl::prepare(meta, &src).await })
            .await
    }

    /// **唯一**的缓存点：`render` 只在未命中时调用一次。
    ///
    /// 判据把渲染器换成「计数器 + 夹具」来断言「第二次不重渲染」（AC-003），
    /// 生产路径由 [`Self::get`] 传入 [`OntologyTtl::prepare`] —— 同一段缓存逻辑。
    async fn get_or_render_with<F, Fut>(&self, render: F) -> Result<Arc<OntologyTtl>, TtlFailure>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<OntologyTtl, TtlFailure>>,
    {
        if let Some(ttl) = self.ready() {
            return Ok(ttl);
        }
        self.renders.fetch_add(1, Ordering::SeqCst);
        let ttl = Arc::new(render().await?);
        *lock(&self.state) = Some(Arc::clone(&ttl));
        Ok(ttl)
    }
}

/// 中毒的锁不该让整个端点停摆：缓存坏了最多是「重新渲染一次」。
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match m.lock() {
        Ok(g) => g,
        Err(e) => e.into_inner(),
    }
}

/// 一对**已经渲染好**的两视图字节。
#[derive(Debug)]
pub struct OntologyTtl {
    customer: TtlBody,
    model: TtlBody,
}

/// 启动时构造两视图需要的输入（**路径是服务端路径**，与 CLI 同义）。
pub struct TtlSource<'a> {
    pub project: &'a str,
    pub version: i32,
    pub base: &'a str,
    /// `--schema`（服务端已加载同一份；这里按 CLI 的参数语义再读一次）。
    pub schema: Option<&'a Path>,
    /// `--tenancy`（只在建模视图生效）。
    pub tenancy: Option<&'a Path>,
    /// ⭐ REQ-145 · `--access-mapping` 里的 api 类（`None` = 没加载映射 ⇒ 两视图不变）。
    ///
    /// 只交「哪些类是 api」这一件事；端点 / 参数 / 凭据在这一层**根本拿不到**。
    pub api_classes: Option<&'a crate::domain::ttl::ApiClasses>,
}

impl OntologyTtl {
    /// 由两份**已经渲染完成**的产物构造（唯一构造点）。
    ///
    /// ⚠️ **私有**：`tests/test_req107_source_enumeration.rs::t4` 要求「返回客户产物的公开
    /// 构造器恰好是 `pipeline::render` / `publish_value` 两个」——本函数只是把它们已经
    /// 产出的 `OutletRender` 收拢成一对缓存字节（⛔ 不从 `&str` 造产物），但它是 `pub fn`
    /// 且签名里带 `OutletRender`，会被那条判据按语法认成第三个工厂。构造入口因此留在这里：
    /// 外面只能走 [`Self::prepare`]（生产）或 [`Self::from_views`]（走同一个 `pipeline::render`）。
    fn build(customer: OutletRender, model: OutletRender) -> Self {
        Self {
            customer: TtlBody::new(customer.into_text()),
            model: TtlBody::new(model.into_text()),
        }
    }

    pub fn get(&self, view: TtlView) -> &TtlBody {
        match view {
            TtlView::Customer => &self.customer,
            TtlView::Model => &self.model,
        }
    }

    pub fn customer(&self) -> &TtlBody {
        &self.customer
    }

    pub fn model(&self) -> &TtlBody {
        &self.model
    }

    /// ⭐ REQ-178 · **请求时**构造（装配期不再调用它）：**读/投影**走
    /// [`crate::cli::prepare_ttl_export`]，**渲染**走 [`crate::console::ops::render_ttl_export_raw`]
    /// —— 与 CLI / 控制面同一条链路（同一个 `pipeline::render`）。
    ///
    /// 两次 prepare 而不是「一次读快照、手工投影两份」：多读一次快照换取
    /// 「客户 / 建模两边的投影与声明解析都由同一个函数产出」，不会在请求路径上
    /// 出现第二份投影代码（那正是字节漂移的来源）。
    pub async fn prepare(pool: &PgPool, src: &TtlSource<'_>) -> Result<Self, TtlFailure> {
        let customer = crate::cli::prepare_ttl_export(
            pool,
            crate::cli::TtlExportParams {
                project: src.project,
                version: src.version,
                model_view: false,
                base: src.base,
                // 客户视图结构上不含列名 / 租户标注：固定 None，与**不带** `--schema` /
                // `--tenancy` 的 `ontok ontology export` 逐字节相同（见模块头）。
                tenancy: None,
                schema: None,
            },
        )
        .await
        .map_err(|e| TtlFailure::read(e.message))?;
        // ⭐ REQ-145 · api 源类（`--access-mapping`）在**两视图**都写 `ontok:sourceKind "api"`
        // （这是「数据不在这套本体里」的事实，不涉端点/凭据）。没加载映射 ⇒ 一个字节都不变。
        let customer = match src.api_classes {
            Some(api) => customer.with_api_classes(api.clone()),
            None => customer,
        };
        let customer = crate::console::ops::render_ttl_export_raw(&customer)
            .map_err(TtlFailure::from_pipeline)?;

        let model = crate::cli::prepare_ttl_export(
            pool,
            crate::cli::TtlExportParams {
                project: src.project,
                version: src.version,
                model_view: true,
                base: src.base,
                tenancy: src.tenancy,
                schema: src.schema,
            },
        )
        .await
        .map_err(|e| TtlFailure::read(e.message))?;
        let model = match src.api_classes {
            Some(api) => model.with_api_classes(api.clone()),
            None => model,
        };
        let model = crate::console::ops::render_ttl_export_raw(&model)
            .map_err(TtlFailure::from_pipeline)?;

        Ok(Self::build(customer, model))
    }

    /// 由**已投影**的两份视图构造（现场判据 / 单测用；走同一个 `pipeline::render`）。
    ///
    /// 生产路径用 [`Self::prepare`]（它把读取与投影也交给 `prepare_ttl_export`）；
    /// 这里只是把「已经有视图」的场景接进同一条渲染链路，⛔ 不另写序列化。
    pub fn from_views(
        customer: &CustomerView,
        model: &ModelTrackView,
        meta: TtlMeta<'_>,
    ) -> anyhow::Result<Self> {
        let base = TtlOptions {
            project: meta.project,
            version: meta.version,
            base: meta.base,
            action_type_count: meta.action_type_count,
            tenancy: None,
            schema: None,
            // ⭐ REQ-145 · 这条入口（判据 / 单测）不加载映射 ⇒ 两视图不带 `sourceKind`。
            api_classes: None,
        };
        let customer_rendered = pipeline::render(
            OutletId::C6,
            Product::CustomerTurtle {
                view: customer,
                opts: base,
            },
        )
        .map_err(|e| anyhow::anyhow!("客户视图 TTL 渲染失败（出口 {}）：{}", e.outlet(), e.message()))?;
        let model_opts = TtlOptions {
            tenancy: meta.tenancy,
            schema: meta.schema,
            ..base
        };
        let model_rendered = pipeline::render(
            OutletId::D9,
            Product::ModelTurtle {
                view: model,
                opts: model_opts,
            },
        )
        .map_err(|e| anyhow::anyhow!("建模视图 TTL 渲染失败（出口 {}）：{}", e.outlet(), e.message()))?;
        Ok(Self::build(customer_rendered, model_rendered))
    }
}

/// [`OntologyTtl::from_views`] 的序列化元数据。
pub struct TtlMeta<'a> {
    pub project: &'a str,
    pub version: i32,
    pub base: &'a str,
    pub action_type_count: usize,
    pub schema: Option<&'a SchemaTypes>,
    pub tenancy: Option<&'a TenancyDecl>,
}

// ══════════════════════════════════════════════════════════════
// 测试夹具（同一 crate 的 query 单测共用；⛔ 不连库）
// ══════════════════════════════════════════════════════════════

/// 客户视图夹具：只有业务名，零建模术语。
#[cfg(test)]
pub(crate) fn customer_fixture() -> CustomerView {
    use crate::domain::view::{EntityView, PropertyView, RelationView};
    CustomerView {
        objects: vec![
            EntityView {
                name: "客户".into(),
                note: Some("客户主数据".into()),
                belongs_to: None,
                aliases: vec![],
                properties: vec![
                    PropertyView {
                        name: "客户名".into(),
                        note: None,
                        required: false,
                        sample: None,
                    },
                    PropertyView {
                        name: "状态".into(),
                        note: Some("状态".into()),
                        required: false,
                        sample: Some("正常".into()),
                    },
                ],
            },
            EntityView {
                name: "订单".into(),
                note: Some("客户下的订单".into()),
                belongs_to: None,
                aliases: vec![],
                properties: vec![PropertyView {
                    name: "金额".into(),
                    note: None,
                    required: false,
                    sample: None,
                }],
            },
        ],
        relations: vec![RelationView {
            from: "订单".into(),
            to: "客户".into(),
            how_many: "多对一".into(),
            meaning: "归属于".into(),
        }],
    }
}

/// 两视图夹具：走**同一个** `pipeline::render`（C6 / D9），不连库。
#[cfg(test)]
pub(crate) fn fixture() -> OntologyTtl {
    let (schema, _mapping, model) = crate::query::test_fixture();
    let customer = customer_fixture();
    OntologyTtl::from_views(
        &customer,
        &model,
        TtlMeta {
            project: "demo",
            version: 2,
            base: crate::domain::ttl::DEFAULT_BASE,
            action_type_count: 0,
            schema: Some(&schema),
            tenancy: None,
        },
    )
    .expect("夹具两视图必须能渲染")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ⭐ AC-003 的核心：客户视图 **0** 命中，建模视图至少三类 **> 0**。
    ///
    /// 八个关键词逐个计数；客户视图不是「碰巧为 0」—— 建模视图同一份数据里必须 > 0。
    #[test]
    fn customer_view_has_zero_modeling_terms_and_model_view_has_them() {
        let ttl = fixture();
        let customer = ttl.customer().text();
        let model = ttl.model().text();
        for needle in [
            "ontok:tenantKey",
            "ontok:tenantColumn",
            "ontok:tenantUnannotated",
            "ontok:joinEvidence",
            "ontok:joinVerified",
            "ontok:primaryKeyColumn",
            "ontok:isPrimaryKey",
            "ontok:allowedValue",
        ] {
            assert_eq!(
                customer.matches(needle).count(),
                0,
                "客户视图不该出现 {needle}：\n{customer}"
            );
        }
        for needle in [
            "ontok:joinVerified",
            "ontok:primaryKeyColumn",
            "ontok:allowedValue",
        ] {
            assert!(
                model.matches(needle).count() > 0,
                "建模视图必须有 {needle}：\n{model}"
            );
        }
    }

    /// AC-005：确定性 + ETag = 内容 sha256 前 16 位。
    #[test]
    fn bytes_are_deterministic_and_the_etag_is_the_hash_prefix() {
        let a = fixture();
        let b = fixture();
        assert_eq!(a.customer().text(), b.customer().text());
        assert_eq!(a.model().text(), b.model().text());
        for body in [a.customer(), a.model()] {
            let sha = sha256_hex(body.text().as_bytes());
            assert_eq!(body.sha256(), sha);
            assert_eq!(body.etag(), format!("\"{}\"", &sha[..ETAG_HEX_LEN]));
            assert_eq!(body.bytes(), body.text().len());
        }
        // 两视图不是同一份内容（否则「默认客户视图」就没有意义）。
        assert_ne!(a.customer().sha256(), a.model().sha256());
    }

    /// 视图解析：只认 customer / model，⛔ 不静默回退。
    #[test]
    fn view_parsing_is_exact() {
        assert_eq!(TtlView::parse("model"), Some(TtlView::Model));
        assert_eq!(TtlView::parse(" customer "), Some(TtlView::Customer));
        assert_eq!(TtlView::parse(""), None);
        assert_eq!(TtlView::parse("model "), Some(TtlView::Model));
        assert_eq!(TtlView::parse("Model"), None);
        assert_eq!(TtlView::parse("admin"), None);
    }

    // ══════════════════════════════════════════════════════════
    // ⭐ REQ-178 · 按需渲染 + 按 version 缓存
    // ══════════════════════════════════════════════════════════

    fn lazy_pool() -> PgPool {
        sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgresql://nobody@127.0.0.1:1/none")
            .expect("lazy pool")
    }

    fn plan() -> TtlPlan {
        TtlPlan {
            project: "demo".into(),
            version: 2,
            base: crate::domain::ttl::DEFAULT_BASE.into(),
            schema: None,
            tenancy: None,
            api_classes: None,
        }
    }

    /// ⭐ AC-003 · 同一份缓存只渲染一次；第二次返回**同一个对象**与同一份字节。
    ///
    /// 变异：去掉缓存（每次都调渲染器）⇒ `calls` = 2 ⇒ 红。
    #[tokio::test]
    async fn the_cache_renders_once_and_serves_the_same_bytes_afterwards() {
        let cache = TtlCache::lazy(lazy_pool(), plan());
        let calls = Arc::new(AtomicUsize::new(0));
        assert!(cache.ready().is_none(), "装配期不许已经有字节");

        let c = Arc::clone(&calls);
        let a = cache
            .get_or_render_with(move || async move {
                c.fetch_add(1, Ordering::SeqCst);
                Ok(fixture())
            })
            .await
            .expect("第一次渲染");
        let c = Arc::clone(&calls);
        let b = cache
            .get_or_render_with(move || async move {
                c.fetch_add(1, Ordering::SeqCst);
                Ok(fixture())
            })
            .await
            .expect("第二次应当命中缓存");

        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "渲染器只许被调用一次（AC-003）"
        );
        assert_eq!(cache.renders(), 1, "渲染计数 = 1");
        assert!(Arc::ptr_eq(&a, &b), "第二次必须拿到同一个缓存对象");
        assert_eq!(a.customer().sha256(), b.customer().sha256());
        assert_eq!(a.model().text(), b.model().text());
    }

    /// ⭐ REQ-178 · 失败**不缓存**（下一次还会试），且原始原因 / 命中词条都被保住。
    ///
    /// 变异：把失败也写进缓存 ⇒ 第二次拿到的是错误（或 `ready()` 非空）⇒ 红；
    /// 把 `TtlFailure::from_pipeline` 的 `hits` 丢掉 ⇒ 最后那条断言红。
    #[tokio::test]
    async fn a_failed_render_is_not_cached_and_keeps_the_hits() {
        let cache = TtlCache::lazy(lazy_pool(), plan());
        let e = cache
            .get_or_render_with(|| async {
                Err::<OntologyTtl, TtlFailure>(TtlFailure::read("读不出 schema".into()))
            })
            .await
            .expect_err("第一次必须失败");
        assert!(e.outlet.is_none());
        assert!(e.hits.is_empty());
        assert_eq!(cache.renders(), 1);
        assert!(cache.ready().is_none(), "失败不许留下半份字节");

        let ttl = cache
            .get_or_render_with(|| async { Ok::<_, TtlFailure>(fixture()) })
            .await
            .expect("修好之后下一次应当能渲染");
        assert_eq!(cache.renders(), 2, "失败不缓存 ⇒ 第二次会重试");
        assert!(cache.ready().is_some());
        assert!(!ttl.customer().text().is_empty());

        // 发布闸命中：`PipelineError::hits` 不许在路上丢掉（AC-001 的消息要点名词条）。
        let text = serde_json::json!({ "key": "样例主键", "cardinality": "1:N" }).to_string();
        let leak = crate::domain::pipeline::render(OutletId::C3, Product::Text(text))
            .expect_err("含建模术语的客户轨产物必须被拦");
        let f = TtlFailure::from_pipeline(leak);
        assert_eq!(f.outlet, Some("C3"));
        assert!(
            f.hits.iter().any(|h| h.contains("主键")),
            "命中的词条必须被保住：{:?}",
            f.hits
        );
        assert!(!f.message.is_empty());
    }
}

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
//! 本模块只做三件事：① 启动时把两视图各渲染**一次**并缓存（版本冻结 ⇒ 内容不变）；
//! ② 从正文算出 sha256 / ETag；③ 按视图取字节。⛔ 不另写序列化器、⛔ 不二次加工正文
//! （否则「与 CLI 逐字节一致」立刻不成立）。
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
//! - 两视图在**启动时**渲染并常驻内存（客户视图 + 建模视图约 0.9 MB）：换版本 = 换进程，
//!   与 `query serve` 的既有口径一致（token 表 / schema / 映射也都是启动时读一次）。
//! - 渲染失败（含客户视图命中发布闸）⇒ **拒绝启动**（fail-closed），而不是先起服务、
//!   等请求进来才 500。理由：这个端点的契约就是「与 CLI 逐字节相同」，产不出就不能宣称能产。

use std::path::Path;

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

/// 服务启动时算好的两视图。
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

    /// ⭐ 启动时构造：**读/投影**走 [`crate::cli::prepare_ttl_export`]，
    /// **渲染**走 [`crate::console::ops::render_ttl_export`] —— 与 CLI / 控制面同一条链路。
    ///
    /// 两次 prepare 而不是「一次读快照、手工投影两份」：多读一次快照换取
    /// 「客户 / 建模两边的投影与声明解析都由同一个函数产出」，不会在启动路径上
    /// 出现第二份投影代码（那正是字节漂移的来源）。
    pub async fn prepare(pool: &PgPool, src: &TtlSource<'_>) -> anyhow::Result<Self> {
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
        .await?;
        // ⭐ REQ-145 · api 源类（`--access-mapping`）在**两视图**都写 `ontok:sourceKind "api"`
        // （这是「数据不在这套本体里」的事实，不涉端点/凭据）。没加载映射 ⇒ 一个字节都不变。
        let customer = match src.api_classes {
            Some(api) => customer.with_api_classes(api.clone()),
            None => customer,
        };
        let customer = crate::console::ops::render_ttl_export(&customer)?;

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
        .await?;
        let model = match src.api_classes {
            Some(api) => model.with_api_classes(api.clone()),
            None => model,
        };
        let model = crate::console::ops::render_ttl_export(&model)?;

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
}

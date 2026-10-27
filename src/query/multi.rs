//! ⭐ REQ-159 P1b · **多项目模式** —— 一套部署 · 多套本体。
//!
//! # 为什么要有这个模块
//!
//! REQ-123 起，`query serve` 是「启动即绑死」的：`--project` + `--version` 定死一份快照，
//! 一个进程只能服务一套本体。要给第二个项目开放，就得再起一个进程（多一套端口 / 部署 /
//! 探针）。这个模块把「服务哪一套本体」从**启动参数**挪到**请求期的 key**：
//!
//! ```text
//! Authorization: Bearer <query key>
//!    ↓ token 表命中（sha256 摘要已经算在表里；比较仍是定长的 ct_eq）
//! project = token.project            （多项目模式下**每一条都必填**，启动闸门保证）
//!    ↓ 一次小 SELECT（约 1 ms）
//! active_version_id = projects.active_version_id
//!    ↓ 按 version_id 查内存缓存
//! 命中 ⇒ 用缓存；未命中 ⇒ get_snapshot(version_id) 加载并放入缓存
//! ```
//!
//! # 缓存键为什么是 `version_id` 而不是 `project`
//!
//! `activate` 在**另一个进程**（`ontok console serve`）里执行，`query serve` **收不到
//! 任何通知** —— 没有回调、没有 LISTEN/NOTIFY、没有共享锁。
//!
//! - 按 `project` 做键：指针一变，缓存里那份**看着还有效**，得靠 TTL / 版本号比对 /
//!   跨进程失效去发现 —— 三种都是「另一套机制」，而且每一种是**可以忘**的。
//! - 按 `version_id` 做键：`active_version_id` 一变，查表的键就跟着变 ⇒ **自然落到新
//!   快照**，旧的那份留在缓存里也不会被误用（回滚回去还能直接命中）。
//!
//! 于是「换版本要生效」这件事**不需要**任何跨进程机制：它由**键的定义**保证。
//! 代价是每次请求多一条 `SELECT projects.active_version_id`（约 1 ms）——
//! **这是刻意的**。⛔ 不要为了省它去加 TTL、长缓存或本地副本：那会把「多快生效」
//! 从「下一次请求」退化成「下一次过期」，而 AC-b6 判的正是**下一次请求**。
//!
//! # 缓存的上限与淘汰
//!
//! [`CACHE_CAP`] = **8** 份快照（`Arc<QueryService>`）。理由：本体是元数据
//! （v5 的两视图 TTL 才 210 KB），8 份的量级是几 MB；真按项目数涨到上百个，
//! 也不该无限常驻 —— 淘汰策略是**最久未用**（LRU，逻辑时钟记 `last_used`，
//! 不引入任何依赖、也不用计时器）。
//!
//! ⛔ 缓存里**只装** `Arc<QueryService>`：没有 DSN、没有连接串、没有 token 表。
//! 源库连接池是**一个**、在 [`MultiProject::deploy`] 里（所有项目共用）；token 表在
//! [`MultiProject::tokens`]（身份门用），缓存里的服务拿到的是**空表**
//! （见 [`MultiProject::load`]）。
//!
//! # 已知边界
//!
//! 1. **部署级 schema / 声明**：多项目模式仍是「一套 `--schema` + 一套 `--policy`」
//!    （per-project schema 是后续需求）。→ 纯本体（`--ontology-only`）部署没有这个限制。
//! 2. **并发首次加载**：两个请求同时撞上同一个**未缓存**的版本，会各加载一次，
//!    后写入的赢 —— 两份内容相同（同一版本快照），只是白做一次功。不加锁是刻意的：
//!    跨 `await` 持锁会把这个热路径变成串行。
//! 3. **限流按版本计**：`QueryService` 里的并发/SSE 计数器跟着快照走，换版本会重置
//!    （同一 token 的 `max_rows` / `max_concurrency` 仍来自 token 表，不受影响）。
//! 4. **`/metrics` 是进程级 HTTP 口径**：请求数 / 状态 / 401 原因都由最外层
//!    `metrics::track` 记在**注册表**这一份上；各项目的 `rows` / `ttl_bytes` /
//!    `mapping_joins` 跟着各自的 `QueryService` 走（N 套本体共用一个 gauge 只会互相覆盖，
//!    ⛔ 那比「只给 HTTP 口径」更容易被读错）。按项目拆指标是后续需求。

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use axum::http::StatusCode;
use sqlx::postgres::PgPool;
use uuid::Uuid;

use crate::db::project::PgProjectStore;
use crate::db::snapshot_store::PgStore;

use super::audit::AuditSink;
use super::error::ApiError;
use super::serve::{self, Deployment, ServeArgs};
use super::tokens::{ProjectBinding, TokenEntry, TokenTable};
use super::QueryService;

/// 缓存里最多几份快照（超出按最久未用淘汰）。
pub const CACHE_CAP: usize = 8;

/// 项目还没有生效版本（`projects.active_version_id IS NULL`）—— **503**，可重试。
pub const NO_ACTIVE_VERSION: &str = "project.no_active_version";
/// `active_version_id` 指向的快照取不到 —— **500**（数据不一致，⛔ 不静默 404）。
pub const SNAPSHOT_MISSING: &str = "project.snapshot_missing";
/// token 绑的项目在库里不存在（改名 / 删除 / 抄错）—— **500**。
pub const PROJECT_MISSING: &str = "project.not_found";
/// 快照取到了但装不起来（未冻结 / access 映射对不上 / TTL 产不出）—— **500**。
pub const LOAD_FAILED: &str = "project.load_failed";

/// 多项目模式的运行期状态（一个进程一份；路由层按请求查它）。
pub struct MultiProject {
    /// 元数据库（每请求那一次小 SELECT 用它）。
    pool: PgPool,
    projects: PgProjectStore,
    store: PgStore,
    /// 身份门用的 token 表（**逐条**绑了项目 —— 启动闸门保证）。
    pub tokens: TokenTable,
    /// 部署级输入（声明 / schema / 源库池 / 各种超时）。⛔ 不含 token、不含 DSN。
    pub(crate) deploy: Deployment,
    /// 审计落点（401 那一刻还没有项目，也要能落一行）。
    pub audit: AuditSink,
    /// 进程级指标（`/metrics` 与最外层的 `track` 都用这一份）。
    pub metrics: crate::metrics::Metrics,
    cache: RwLock<Cache>,
}

impl std::fmt::Debug for MultiProject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MultiProject")
            .field("projects", &self.tokens.project_count())
            .field("cached", &self.cached_versions())
            .finish()
    }
}

#[derive(Default)]
struct Cache {
    map: HashMap<Uuid, Cached>,
    /// 逻辑时钟（只在缓存锁里递增；没有定时器、没有系统时间 —— 淘汰是确定的）。
    clock: u64,
}

struct Cached {
    svc: Arc<QueryService>,
    last_used: u64,
}

/// 中毒的锁不该让服务停摆：缓存坏了最多是「重新加载一次」。
fn lock<T>(m: &RwLock<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    match m.write() {
        Ok(g) => g,
        Err(e) => e.into_inner(),
    }
}

impl MultiProject {
    /// 组装一个多项目注册表（生产路径的**唯一**调用点是 [`serve`]；判据也用它）。
    ///
    /// ⛔ 这里**不加载任何快照**：构造函数只收「部署级输入 + 身份表 + 审计落点」，
    /// 快照等第一次请求（见 [`Self::service_for`]）。
    pub fn new(
        pool: PgPool,
        tokens: TokenTable,
        deploy: Deployment,
        audit: AuditSink,
    ) -> Self {
        Self {
            projects: PgProjectStore::new(pool.clone()),
            store: PgStore::new(pool.clone()),
            pool,
            tokens,
            deploy,
            audit,
            metrics: crate::metrics::Metrics::for_query(),
            cache: RwLock::new(Cache::default()),
        }
    }

    /// ⭐ 请求期解析：token → 项目 → `active_version_id` → 内存缓存 / 懒加载。
    ///
    /// 错误语义（AC-b3 / ⑤）：
    ///
    /// | 情形 | 结果 |
    /// | --- | --- |
    /// | token 有效但项目没有 `active_version_id` | 503 `project.no_active_version` |
    /// | `active_version_id` 指向的快照不存在 | 500 `project.snapshot_missing` |
    /// | token 绑的项目不在库里 | 500 `project.not_found`（不是 404：404 会被读成「这个 key 没权限」） |
    /// | 快照在但装不起来 | 500 `project.load_failed`（数据不可信，⛔ 不吞） |
    pub async fn service_for(&self, entry: &TokenEntry) -> Result<Arc<QueryService>, ApiError> {
        // 启动闸门（`TokenTable::parse_multi`）保证每一条都有 project；走到这里的 `None`
        // 只可能是「表被改过还热加载」（本实现不热加载）—— 当 bug 报，⛔ 不猜一个项目。
        let project = entry.project.as_deref().ok_or_else(|| {
            ApiError::with_code(
                StatusCode::INTERNAL_SERVER_ERROR,
                LOAD_FAILED,
                "这张 token 没有绑定项目，但服务是多项目模式 —— 启动闸门漏了（这是 bug）。",
                "重启服务；若仍复现，请把 token 表与这份错误一起交给维护者。",
            )
        })?;

        // ── 一次小 SELECT：项目行 + 生效版本指针 ────────────────
        let found = self
            .projects
            .active_version_by_name(project)
            .await
            .map_err(|e| db_error("读取项目的生效版本", e))?;
        let Some((row, active)) = found else {
            return Err(ApiError::with_code(
                StatusCode::INTERNAL_SERVER_ERROR,
                PROJECT_MISSING,
                format!("这张 token 绑定的项目 {project:?} 不在元数据库里。"),
                "项目可能被改名或删除；请用 `GET /api/projects` 核对项目名，\
                 并让运维把 token 表里的 `project` 改成现存的（改完要重启服务 —— 表不热加载）。",
            ));
        };
        let Some(version_id) = active else {
            // ⛔ 503 而不是 404/200：这不是「路径不对」，也不是「查了没有」——
            // 是「这套本体还没上线」。可重试，且 hint 要说清谁来让它上线。
            return Err(ApiError::with_code(
                StatusCode::SERVICE_UNAVAILABLE,
                NO_ACTIVE_VERSION,
                format!("项目 {:?} 还没有生效版本。", row.name),
                format!(
                    "项目建好但还没有人 `activate` 过任何一版：先上传构建包并 \
                     `POST /api/projects/{}/bundles/<id>/activate`（或 `ontok version freeze` + 激活）。\
                     激活之后**下一次请求**就会生效，不需要重启本服务。",
                    row.name
                ),
            ));
        };

        if let Some(svc) = self.cache_get(version_id) {
            return Ok(svc);
        }
        let svc = self.load(&row.name, version_id).await?;
        Ok(self.cache_put(version_id, svc))
    }

    /// 冷路径：把**一个版本**的快照装成可服务的状态（`version_id` 已由调用方解析）。
    async fn load(&self, project: &str, version_id: Uuid) -> Result<Arc<QueryService>, ApiError> {
        let v = self
            .projects
            .version_by_id(version_id)
            .await
            .map_err(|e| db_error("读取生效版本行", e))?;
        let Some(v) = v else {
            return Err(self.snapshot_missing(project, version_id, "版本行本身也不在了"));
        };
        if v.frozen_at.is_none() {
            return Err(ApiError::with_code(
                StatusCode::INTERNAL_SERVER_ERROR,
                LOAD_FAILED,
                format!(
                    "项目 {project:?} 的 `active_version_id` 指向第 {} 版，但**那一版还没冻结** —— \
                     生效指针只该指向已激活（= 已冻结）的版本，这是数据不一致。",
                    v.seq
                ),
                "别在库里手改 `active_version_id`；用 `activate`（它冻结 + 指指针，同一事务）。",
            ));
        }
        let snap = self
            .store
            .get_snapshot(version_id)
            .await
            .map_err(|e| db_error("读取版本快照", e))?;
        let Some(snap) = snap else {
            return Err(self.snapshot_missing(project, version_id, "`ontology_versions` 里有这一版，但快照表里没有内容"));
        };
        let prepared = serve::prepare_project(&self.pool, &self.deploy, project, v.seq, &snap)
            .await
            .map_err(|e| {
                ApiError::with_code(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    LOAD_FAILED,
                    format!("项目 {project:?} 第 {} 版加载失败（快照在，但装不成可服务状态）。", v.seq),
                    format!(
                        "这一步与单项目模式启动时的装配**同一段代码**（access 映射对账 / \
                         两视图 TTL / R2RML 映射），所以失败原因通常在部署级输入（`--schema` / \
                         `--access-mapping` / `--policy`）与这一版本体不一致。原始错误：{e}"
                    ),
                )
            })?;
        Ok(Arc::new(serve::assemble(
            &self.deploy,
            prepared,
            // ⛔ 缓存里不放 token：身份门用的是 `MultiProject::tokens`，
            // 服务里那张表故意是空的（运行期没有任何分支读它）。
            TokenTable::default(),
            self.audit.clone(),
        )))
    }

    fn snapshot_missing(&self, project: &str, version_id: Uuid, why: &str) -> ApiError {
        ApiError::with_code(
            StatusCode::INTERNAL_SERVER_ERROR,
            SNAPSHOT_MISSING,
            format!(
                "项目 {project:?} 的生效版本（{version_id}）没有可用的快照：{why}。"
            ),
            "这是**数据不一致**（不是「查了没有」，也不是「没权限」）：生效指针指向的版本取不出快照。\
             请用 `GET /api/projects` 核对这一版的状态，并重新 `activate` 一个完整导入的版本。\
             ⛔ 服务端不会退回上一版（那会让你以为看到的是当前生效版本）。",
        )
    }

    /// 缓存命中（顺带把 `last_used` 推到最新 —— LRU 的「用」就是这一次请求）。
    fn cache_get(&self, version_id: Uuid) -> Option<Arc<QueryService>> {
        let mut c = lock(&self.cache);
        c.clock += 1;
        let now = c.clock;
        let e = c.map.get_mut(&version_id)?;
        e.last_used = now;
        Some(Arc::clone(&e.svc))
    }

    /// 放入缓存（超上限就淘汰**最久没用**的那一份）。
    fn cache_put(&self, version_id: Uuid, svc: Arc<QueryService>) -> Arc<QueryService> {
        let mut c = lock(&self.cache);
        c.clock += 1;
        let now = c.clock;
        if !c.map.contains_key(&version_id) && c.map.len() >= CACHE_CAP {
            if let Some(oldest) = c
                .map
                .iter()
                .min_by_key(|(_, v)| v.last_used)
                .map(|(k, _)| *k)
            {
                c.map.remove(&oldest);
            }
        }
        c.map.insert(
            version_id,
            Cached {
                svc: Arc::clone(&svc),
                last_used: now,
            },
        );
        svc
    }

    /// 当前缓存里有几份（判据用；也进 `/metrics` 之外的启动提示）。
    pub fn cached_versions(&self) -> usize {
        lock(&self.cache).map.len()
    }

    /// 缓存里全部版本 id（判据用：AC-b7 要证明两份快照是**不同**的对象）。
    pub fn cached_ids(&self) -> Vec<Uuid> {
        lock(&self.cache).map.keys().copied().collect()
    }

    /// 取一个已缓存的服务（判据用：AC-b7 用指针比较证明不是同一个 `Arc`）。
    pub fn cached_service(&self, version_id: Uuid) -> Option<Arc<QueryService>> {
        lock(&self.cache).map.get(&version_id).map(|c| Arc::clone(&c.svc))
    }
}

fn db_error(what: &str, e: impl std::fmt::Display) -> ApiError {
    ApiError::with_code(
        StatusCode::INTERNAL_SERVER_ERROR,
        LOAD_FAILED,
        format!("{what}失败：{e}"),
        "这是服务端与元数据库之间的问题（连接 / 表结构 / 权限），与你的凭据无关。\
         重试一次；仍失败请把本页的 code 与时间交给运维。",
    )
}

// ══════════════════════════════════════════════════════════════
// 启动
// ══════════════════════════════════════════════════════════════

/// 多项目模式的**启动闸门 + 装配**。
///
/// 与单项目模式的差别只有一处：**一份快照都不加载**。所有「能否服务」的判断都推迟到
/// 请求期（那时才知道是哪个项目、哪一版）—— 于是启动很快，且不需要在启动时就知道
/// 全部项目的存在（`POST /api/projects` 之后新建的项目不必重启）。
///
/// 但**有一条必须是启动时判**：每张 query token 都要能解析出项目
/// （[`TokenTable::parse_multi`]）。理由见那个函数的注释 —— 一张没绑项目的 key
/// 在请求期只能二选一：猜一个项目（串包）或恒 401（发出去就是废的）。
pub async fn serve(pool: &PgPool, args: ServeArgs) -> anyhow::Result<()> {
    let mode = args.mode();

    // ① 绑定闸门（与单项目模式同一段：绑错了根本不该读 token 表 / 连库）
    let (addr, verdict) = serve::bind_gate(&args)?;

    // ② token 表：**逐条必须绑项目**（AC-b4：缺一条就拒绝启动并点名）
    let tokens = serve::load_tokens(&args, mode, ProjectBinding::PerToken)?;

    // ③ 部署级输入（声明 / schema / 源库 / 审计）
    let policy = serve::load_policy(&args)?;
    let (schema, schema_path) = serve::load_schema(&args)?;
    let deploy = Deployment {
        mode,
        schema,
        schema_path,
        policy,
        base: args.base.clone(),
        include_inherit: args.include_inherit,
        access_raw: serve::read_access_mapping(&args)?,
        access_path: args.access_mapping.clone(),
        tenancy: serve::resolve_tenancy(args.policy.as_deref(), args.tenancy.as_deref()),
        timeout_ms: args.timeout_ms,
        sse_idle_timeout_ms: args.sse_idle_timeout_ms,
        sse_max_connections: args.sse_max_connections,
        max_concurrent_queries: args.max_concurrent_queries,
        source: None,
        facts: None,
    };
    let (source, facts) = serve::connect_source(&deploy).await?;
    let deploy = Deployment {
        source,
        facts,
        ..deploy
    };
    let audit = serve::open_audit(&args)?;

    // ④ 提醒（⛔ 不拒绝启动：项目可能**本来就要稍后建**）
    //
    // 与「token 缺 project 就拒绝」不同：那个是**表自身不自洽**（谁也不该等它），
    // 而这个只是「库里暂时还没有这个项目」—— 多项目模式恰恰要为「刚建的项目」
    // 留出空间（建项目 → 传包 → activate 之后**下一次请求**就能用）。
    let projects = PgProjectStore::new(pool.clone());
    for name in tokens.projects() {
        match projects.get_by_name(name).await {
            Ok(Some(_)) => {}
            Ok(None) => eprintln!(
                "⚠ token 表里有 key 绑定到项目 {name:?}，但元数据库里还没有这个项目 —— \
                 拿到那些 key 的调用方现在会得到 500 `{PROJECT_MISSING}`。\
                 项目建好并 activate 之后**下一次请求**即可用（本服务不用重启）。"
            ),
            Err(e) => eprintln!("⚠ 核对项目 {name:?} 是否存在时失败（{e}）；启动继续。"),
        }
    }

    let multi = Arc::new(MultiProject::new(pool.clone(), tokens, deploy, audit));

    eprintln!(
        "{}",
        multi_banner(addr, verdict, &multi, args.timeout_ms, args.max_concurrent_queries)
    );

    let app = super::http::build_router_multi(Arc::clone(&multi));
    let listener = tokio::net::TcpListener::bind(addr).await.map_err(|e| {
        anyhow::anyhow!("绑不上 {addr}：{e}（端口被占 / 地址不属于本机？换一个 --bind 再试）")
    })?;
    axum::serve(listener, app)
        .await
        .map_err(|e| anyhow::anyhow!("查询服务退出：{e}"))
}

/// 多项目模式的启动横幅（**纯函数**：单测逐项对；单项目模式的横幅一个字节都没动）。
#[allow(clippy::too_many_arguments)]
pub fn multi_banner(
    addr: std::net::SocketAddr,
    verdict: serve::BindVerdict,
    multi: &MultiProject,
    timeout_ms: u64,
    max_concurrent_queries: usize,
) -> String {
    let tenants = multi.tokens.tenants();
    let projects = multi.tokens.projects();
    let mut s = String::new();
    s.push_str(&format!(
        "· ontok query serve 就绪（**多项目模式**）：绑定 {addr}（{}） · 项目随 key 解析\
         （key 绑了 {} 个项目：{}） · token {} 个 · 租户 {} 个（{}）\n",
        verdict.label(),
        projects.len(),
        if projects.is_empty() {
            "无".to_string()
        } else {
            projects.join("、")
        },
        multi.tokens.len(),
        tenants.len(),
        if tenants.is_empty() {
            "无".to_string()
        } else {
            tenants.join("、")
        }
    ));
    if let Some(line) = multi.deploy.mode.banner_line() {
        s.push_str(line);
    }
    s.push_str(&format!(
        "· SPARQL 语法 v1 · 超时 {timeout_ms} ms · 单次最多 {} 行（按 token）· \
         全局并发 {max_concurrent_queries}\n",
        multi.tokens.max_rows_cap()
    ));
    s.push_str(
        "· 版本：每次请求按 key → `projects.active_version_id` 解析（⛔ 启动时不加载快照；\
         换版本下一次请求即生效）\n",
    );
    s.push_str(&format!(
        "· 快照缓存：按 version_id 记，上限 {CACHE_CAP} 份，最久未用先淘汰\n"
    ));
    match multi.deploy.facts.as_ref() {
        Some(f) => s.push_str(&format!(
            "· 只读会话（读回）：transaction_read_only = {} · standard_conforming_strings = {}\n",
            f.transaction_read_only, f.standard_conforming_strings
        )),
        None => s.push_str(
            "· 数据源：无（本体服务模式不连业务库；需要库的 SPARQL / sample 恒 503 NO_DATA_SOURCE）\n",
        ),
    }
    s.push_str("· 审计：每次调用一行（token 只记摘要、不记查询原文；stdout 恒写）\n");
    s.push_str(&format!(
        "· 指标（免 token）：http://{addr}{}\n",
        crate::metrics::METRICS_PATH
    ));
    if verdict.needs_warning() {
        s.push_str(&format!(
            "⚠ 警告：绑定的是 {addr}（{}）—— 查询服务可被本机以外访问。\
             token 一旦泄露即可取数；请确认防火墙 / 反向代理 / TLS 边界。\n",
            verdict.label()
        ));
    }
    s
}

// ══════════════════════════════════════════════════════════════
// 判据（纯函数 + 缓存行为；不起 socket、不连库）
// ══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_multi() -> MultiProject {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .acquire_timeout(std::time::Duration::from_millis(200))
            .connect_lazy("postgresql://nobody@127.0.0.1:1/none")
            .expect("lazy pool");
        MultiProject {
            pool: pool.clone(),
            projects: PgProjectStore::new(pool.clone()),
            store: PgStore::new(pool.clone()),
            tokens: TokenTable::default(),
            deploy: Deployment {
                mode: super::super::ServiceMode::OntologyOnly,
                schema: crate::domain::ttl::SchemaTypes::default(),
                schema_path: None,
                policy: crate::sparql::policy::QueryPolicy::parse(
                    r#"{"version":"1.0.0","tenantSubject":"__no_tenant__",
                        "tenantIdColumn":"__no_tenant__","perTable":{},"publicDimensions":[]}"#,
                )
                .expect("占位声明"),
                base: "https://ontok.local/".into(),
                include_inherit: false,
                access_raw: None,
                access_path: None,
                tenancy: None,
                timeout_ms: 1000,
                sse_idle_timeout_ms: 60_000,
                sse_max_connections: 4,
                max_concurrent_queries: 2,
                source: None,
                facts: None,
            },
            audit: AuditSink::stdout_only(),
            metrics: crate::metrics::Metrics::for_query(),
            cache: RwLock::new(Cache::default()),
        }
    }

    /// 缓存**按 version_id 分键**：两个版本各一份、上限 8、最久未用先淘汰。
    ///
    /// 变异：把键换成「恒 0」/只留一份 ⇒ 第二个版本拿到第一个的对象（AC-b7 红）。
    #[tokio::test]
    async fn the_cache_is_keyed_by_version_and_evicts_the_least_recently_used() {
        let multi = empty_multi();
        let svc = Arc::new(super::super::http::tests::test_service());
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        assert!(multi.cached_versions() == 0);

        let put_a = multi.cache_put(a, Arc::clone(&svc));
        let put_b = multi.cache_put(b, Arc::clone(&svc));
        assert_eq!(multi.cached_versions(), 2);
        // 同一个 version_id 命中同**一个**对象；不同 version_id 是**不同**的键。
        assert!(Arc::ptr_eq(&multi.cache_get(a).unwrap(), &put_a));
        assert!(Arc::ptr_eq(&multi.cache_get(b).unwrap(), &put_b));
        assert!(multi.cached_service(a).is_some());

        // 把 a 再「用」一次 ⇒ b 变成最久未用；再塞 7 份（正好填满 8 份）之后的**下一份**
        // 淘汰 b，而刚用过的 a 必须还在。
        multi.cache_get(a);
        for _ in 0..CACHE_CAP - 1 {
            let k = Uuid::new_v4();
            multi.cache_put(k, Arc::clone(&svc));
        }
        assert_eq!(multi.cached_versions(), CACHE_CAP, "正好填满上限");
        assert!(multi.cache_get(a).is_some(), "刚用过的 a 不该被淘汰");
        assert!(multi.cache_get(b).is_none(), "最久未用的 b 应当被淘汰");
        // 再塞一份：淘汰的是现在最久的那个，总数仍不超上限。
        multi.cache_put(Uuid::new_v4(), Arc::clone(&svc));
        assert_eq!(multi.cached_versions(), CACHE_CAP, "缓存不得超过上限");
    }

    /// **AC-b3 / ⑤ 的错误形状**：503 `project.no_active_version`、500 `project.snapshot_missing`
    /// —— 两个码、两个状态，且 hint 非空（错误契约是 `{code,message,hint}`）。
    ///
    /// ⚠️ `snapshot_missing` 在**当前 schema** 下不可达（`projects.active_version_id` 有外键
    /// 指向 `ontology_versions`，指针不可能指向不存在的版本行）—— 它是防御分支。
    /// 这里钉住它的形状，是为了**将来**去掉外键 / 快照行被单独删掉时不会退化成 404。
    ///
    /// 变异：把 `snapshot_missing` 的 500 改成 404 ⇒ 本测试红（⑤ 明令禁止「静默 404」）。
    #[tokio::test]
    async fn snapshot_missing_is_a_500_not_a_404() {
        let multi = empty_multi();
        let e = multi.snapshot_missing("cloud-resource", Uuid::nil(), "测试用");
        assert_eq!(e.code, SNAPSHOT_MISSING);
        assert_eq!(e.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(!e.hint.is_empty(), "hint 不能空（错误契约要求可行动）");
        let body = e.body();
        assert_eq!(body["code"], SNAPSHOT_MISSING);
        assert!(body["message"].as_str().unwrap().contains("cloud-resource"));
        // 三个码各自唯一，且都在契约里写明（AC-b3 / ⑤）。
        assert_eq!(NO_ACTIVE_VERSION, "project.no_active_version");
        assert_eq!(SNAPSHOT_MISSING, "project.snapshot_missing");
        assert_eq!(PROJECT_MISSING, "project.not_found");
        assert_eq!(LOAD_FAILED, "project.load_failed");
    }

    /// 「连不上库」**不是**「这个项目不存在」：前者 500 `project.load_failed`，
    /// 后者才是 `project.not_found`。⛔ 别把两者吞成同一个码（排查时会走错方向）。
    #[tokio::test]
    async fn a_db_failure_is_not_reported_as_a_missing_project() {
        let multi = empty_multi();
        // 夹具池故意连不上（127.0.0.1:1）⇒ 必须**报错**，不是 `Ok(None)`。
        assert!(
            multi.projects.get_by_name("whatever").await.is_err(),
            "连不上库时不能返回「项目不存在」"
        );
        let e = db_error(
            "读取项目",
            crate::db::project::ProjectError::NameTaken("x".into()),
        );
        assert_eq!(e.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(e.code, LOAD_FAILED);
        assert!(!e.hint.is_empty());
    }

    /// ⭐ AC-b6 的**结构前提**：`active_version_id` 必须在**每次请求**上重读
    /// （缓存键是 `version_id`，指针不进缓存）。
    ///
    /// 判据方式：读源码，断言 `service_for` 里确实调了 `active_version_by_name`。
    /// 变异：把指针缓存在注册表里（启动读一次）⇒ 本测试红。
    #[test]
    fn the_active_version_pointer_is_read_on_every_request() {
        let src = include_str!("multi.rs");
        let body = src
            .split("pub async fn service_for")
            .nth(1)
            .expect("service_for 还在");
        let body = body
            .split("async fn load(")
            .next()
            .expect("截到 load 之前");
        assert!(
            body.contains("active_version_by_name"),
            "service_for 必须在每次请求都查一次 `projects.active_version_id`：\n{body}"
        );
        // ⛔ 不许给生效指针加缓存（activate 之后会一直服务旧版本）。
        // 之所以把名字拼起来：判据源码本身也含这些字样，直写会**自己命中自己**（假红）。
        let banned = ["pointer", "_cache"].concat();
        assert!(
            !body.contains(&banned),
            "⛔ 不许给生效指针加缓存（activate 之后会一直服务旧版本）"
        );
    }

    /// 横幅必须说清「多项目模式 / 版本随请求解析 / 缓存上限」，
    /// 且**不许**出现任何连接串或 token 明文。
    #[tokio::test]
    async fn the_multi_banner_says_which_mode_and_how_versions_are_resolved() {
        let multi = empty_multi();
        let addr = serve::parse_bind("127.0.0.1:8800").unwrap();
        let text = multi_banner(addr, serve::check_bind(addr, false).unwrap(), &multi, 10_000, 2);
        for needle in [
            "多项目模式",
            "项目随 key 解析",
            "active_version_id",
            "下一次请求即生效",
            "上限 8 份",
            "127.0.0.1:8800",
        ] {
            assert!(text.contains(needle), "横幅缺 {needle:?}：\n{text}");
        }
        assert!(!text.contains("postgresql://"), "横幅里不许出现连接串：\n{text}");
    }
}

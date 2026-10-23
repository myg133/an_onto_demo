//! REQ-124 · **控制面的领域调用层** —— 三个端点各自的「读状态 → 调既有函数 → 回执」。
//!
//! # 为什么这不是「HTTP 层重写一遍」
//!
//! 每个写操作在**进入**这里之前都先判「可预期的冲突」（已 applied / 已冻结 /
//! 不属于这个项目），然后调**那个**既有函数：
//!
//! | 动作 | 调的函数 | 定义 |
//! | --- | --- | --- |
//! | apply | [`crate::db::apply::PgApplier::apply_proposal`] | `proposal apply` 调的是它 |
//! | freeze | [`crate::db::project::PgProjectStore::freeze`] | `version freeze` 调的是它 |
//!
//! 本模块里**没有**一条写 SQL、没有 `UPDATE proposals` / `UPDATE ontology_versions`：
//! 冲突判断是 `SELECT`（读），写只有上面两个函数。
//!
//! # import / export / r2rml 为什么（mostly）不在这个文件
//!
//! 它们与 CLI 共用的实现放在 [`crate::cli`]（`import_proposal_core` /
//! `prepare_ttl_export` / `r2rml_core`）：
//!
//! - `import_proposal_core` / `prepare_ttl_export` / `r2rml_core` 是**纯领域调用**
//!   （零写出），放在 cli.rs 是因为 CLI handler 与它们共享同一段读取/投影；
//! - TTL 的**渲染**位置有判据钉着：T2/T8 要求「写出的绑定直接来自
//!   `publish_customer(…)`，且 `OutletId::C6/D9` 的管线调用与写点同函数」——
//!   所以 CLI 的渲染必须留在 `ontology_cmd` 体内。控制面这一侧用
//!   [`render_ttl_export`]（同一个 `pipeline::render`、同一份 [`TtlPrepared`]，
//!   命中泄漏返回错误而不是退出进程）。
//!
//! 搬家的方向是：**CLI handler 变薄，共用实现留在 cli.rs**，HTTP 调它。
//! ⛔ 这里没有一条写 SQL（唯一的写是下面两个既有函数）。

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::postgres::PgPool;
use uuid::Uuid;

use crate::db::apply::PgApplier;
use crate::db::pg::StoreError;
use crate::db::project::{PgProjectStore, ProjectError, VersionRow};
use crate::db::proposal::PgProposalStore;
use crate::db::snapshot_store::PgStore;

use super::OpError;

// ══════════════════════════════════════════════════════════════
// GET /api/projects
// ══════════════════════════════════════════════════════════════

/// 一个项目 + 它的全部版本（AC-002）。
#[derive(Debug, Clone, Serialize)]
pub struct ProjectSummary {
    pub project: String,
    pub versions: Vec<VersionSummary>,
}

/// 一版的状态与计数。
#[derive(Debug, Clone, Serialize)]
pub struct VersionSummary {
    pub version: i32,
    /// `ontology_versions.frozen_at IS NOT NULL`。
    pub frozen: bool,
    /// RFC3339（未冻结 ⇒ `null`）。
    pub frozen_at: Option<String>,
    /// 这一版有没有快照行。
    ///
    /// 为 `false` 时 `objects` / `relations` 是 `null` 而不是 `0`：
    /// 「没有快照」与「有快照但为空」是两件事，用 0 会把它说成后者。
    pub has_snapshot: bool,
    /// `object_types` 里这一版的行数（判据：`SELECT count(*) … WHERE version_id=$1`）。
    pub objects: Option<usize>,
    /// `link_types` 里这一版的行数。
    pub relations: Option<usize>,
    pub extracted_by: Option<serde_json::Value>,
}

/// `GET /api/projects` 的实现：**直接读元数据库**，不缓存。
pub async fn projects_summary(pool: &PgPool, project: &str) -> Result<ProjectSummary, OpError> {
    let projects = PgProjectStore::new(pool.clone());
    let p = projects
        .get_by_name(project)
        .await
        .map_err(OpError::db)?
        .ok_or_else(|| {
            OpError::not_found(
                "PROJECT_NOT_FOUND",
                format!("项目 {project:?} 不存在。"),
                "检查 `console serve --project` 的值；项目要先由 `ontok project new` 建出来。",
            )
        })?;
    let versions = projects.list_versions(p.id).await.map_err(OpError::db)?;
    let store = PgStore::new(pool.clone());
    let mut out = Vec::with_capacity(versions.len());
    for v in versions {
        let snap = store.get_snapshot(v.id).await.map_err(OpError::db)?;
        out.push(VersionSummary {
            version: v.seq,
            frozen: v.frozen_at.is_some(),
            frozen_at: v.frozen_at.map(rfc3339),
            has_snapshot: snap.is_some(),
            objects: snap.as_ref().map(|s| s.entity_types.len()),
            relations: snap.as_ref().map(|s| s.link_types.len()),
            extracted_by: snap
                .as_ref()
                .and_then(|s| s.extracted_by.as_ref())
                .and_then(|e| serde_json::to_value(e).ok()),
        });
    }
    Ok(ProjectSummary {
        project: p.name,
        versions: out,
    })
}

fn rfc3339(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

// ══════════════════════════════════════════════════════════════
// POST /api/apply
// ══════════════════════════════════════════════════════════════

/// `POST /api/apply` 的回执。
#[derive(Debug, Clone, Serialize)]
pub struct ApplyReceipt {
    pub project: String,
    /// 提案别名（`p1` / `p2` …，与 CLI 抬头同源）。
    pub proposal: String,
    pub proposal_id: Uuid,
    /// 新版本的 seq（`apply` 产生的那个）。
    pub version: i32,
    pub version_id: Uuid,
    pub status: &'static str,
}

/// `POST /api/apply` 的领域实现：**调 `proposal apply` 的那一个函数**。
///
/// 冲突语义（R5 / AC-004）在**调用之前**判：
/// - 提案不是 `pending` ⇒ 409 `PROPOSAL_NOT_PENDING`（重复 apply 落在这里）；
/// - 基版本已冻结 ⇒ 409 `BASE_VERSION_FROZEN`（**点名是哪一版**）。
///
/// 判完就调 [`PgApplier::apply_proposal`] —— 它自己也会再判一遍（纵深防御：
/// 判完之后、写之前的那一小段里，另一个进程可能把版本冻上）。
pub async fn apply_proposal_core(
    pool: &PgPool,
    project: &str,
    spec: &str,
) -> Result<ApplyReceipt, OpError> {
    let projects = PgProjectStore::new(pool.clone());
    let proposals = PgProposalStore::new(pool.clone());
    let p = projects
        .get_by_name(project)
        .await
        .map_err(OpError::db)?
        .ok_or_else(|| {
            OpError::not_found(
                "PROJECT_NOT_FOUND",
                format!("项目 {project:?} 不存在。"),
                "检查 `console serve --project` 的值。",
            )
        })?;
    let id = crate::cli::resolve_proposal(pool, p.id, spec)
        .await
        .map_err(|e| {
            OpError::not_found(
                "PROPOSAL_NOT_FOUND",
                format!("找不到提案 {spec:?}：{e}"),
                "用别名 `p1`（该项目内最近导入的）或提案 UUID；先 `GET /api/projects` 看项目，\
                 再用 `POST /api/import` 导入一份提案。",
            )
        })?;
    let row = proposals.get(id).await.map_err(OpError::db)?.ok_or_else(|| {
        OpError::not_found(
            "PROPOSAL_NOT_FOUND",
            format!("提案 {spec:?} 不存在。"),
            "别名按项目内创建时间倒序解析；越界时换一个别名或直接给 UUID。",
        )
    })?;
    if row.project_id != p.id {
        return Err(OpError::bad_request(
            "PROPOSAL_NOT_IN_PROJECT",
            format!("提案 {spec:?} 不属于项目 {project:?}。"),
            "别名只在本项目内解析；要给别的项目的提案，请改 `--project` 或直接给 UUID。",
        ));
    }
    let alias = crate::cli::proposal_alias(pool, p.id, id).await;
    if row.status != "pending" {
        return Err(OpError::conflict(
            "PROPOSAL_NOT_PENDING",
            format!("提案 {alias} 的状态是 {}（不是 pending）。", row.status),
            format!(
                "已经 {} 的提案不能再 apply。要改本体请新开一版（`ontok version new`）后重新提案；\
                 要看这一版的结果用 `GET /api/projects`。",
                row.status
            ),
        ));
    }
    // 基版本冻结 ⇒ 409，**点名是哪一版**（AC-004 的判据）。
    let versions = projects.list_versions(p.id).await.map_err(OpError::db)?;
    if let Some(base) = versions.iter().find(|v| v.id == row.base_version_id) {
        if base.frozen_at.is_some() {
            return Err(OpError::conflict(
                "BASE_VERSION_FROZEN",
                format!(
                    "提案 {alias} 基于 v{}，而 v{} 已冻结 —— 不能在冻结的版本上 apply。",
                    base.seq, base.seq
                ),
                "冻结是判据（规格 §9.1）。新开一版再提案：`ontok version new --project …`，\
                 或者在冻结前就把提案应用掉。",
            ));
        }
    }

    let applier = PgApplier::new(pool.clone());
    let version_id = applier.apply_proposal(id).await.map_err(|e| {
        map_apply_error(e, &alias, row.base_version_id, &versions)
    })?;
    let seq = projects
        .latest_version(p.id)
        .await
        .map_err(OpError::db)?
        .map(|v| v.seq)
        .unwrap_or(0);
    Ok(ApplyReceipt {
        project: p.name,
        proposal: alias,
        proposal_id: id,
        version: seq,
        version_id,
        status: "applied",
    })
}

/// `PgApplier::apply_proposal` 的错误 ⇒ 可行动的 HTTP 冲突。
fn map_apply_error(
    e: StoreError,
    alias: &str,
    base_version_id: Uuid,
    versions: &[VersionRow],
) -> OpError {
    match e {
        StoreError::ProposalNotPending(_, status) => OpError::conflict(
            "PROPOSAL_NOT_PENDING",
            format!("提案 {alias} 的状态是 {status}（不是 pending）。"),
            "已经应用过的提案不能再 apply —— 这是幂等性的正面回答，不是失败。",
        ),
        StoreError::BaseVersionFrozen => {
            let seq = versions
                .iter()
                .find(|v| v.id == base_version_id)
                .map(|v| v.seq)
                .unwrap_or(-1);
            OpError::conflict(
                "BASE_VERSION_FROZEN",
                format!("提案 {alias} 基于的 v{seq} 已冻结，不能在其上 apply。"),
                "新开一版再提案（`ontok version new`）；冻结的版本只读。",
            )
        }
        StoreError::ProposalNotPassed { blocked, dangling } => OpError::conflict(
            "PROPOSAL_NOT_PASSED",
            format!(
                "提案 {alias} 未通过审核：被拦 {blocked} 条、悬空关系 {dangling} 条。"
            ),
            "先审核：`GET /api/review` 看条目与建议，`POST /api/decide` 双轨签完再 apply。",
        ),
        StoreError::ItemNotFound { seq } => OpError::bad_request(
            "ITEM_NOT_FOUND",
            format!("提案 {alias} 里没有第 {seq} 个条目。"),
            "条目序号从 1 起；`GET /api/review` 的 `items[].item` 是权威序号。",
        ),
        other => OpError::db(other),
    }
}

// ══════════════════════════════════════════════════════════════
// POST /api/freeze
// ══════════════════════════════════════════════════════════════

/// `POST /api/freeze` 的结果。
#[derive(Debug, Clone, Serialize)]
pub struct FreezeOutcome {
    pub project: String,
    pub version: i32,
    pub version_id: Uuid,
    /// 调用前就已经冻结了？（CLI 的 `version freeze` 是幂等的，照旧打印成功；
    /// HTTP 层把 `true` 翻成 **409** —— 这是 R5 的端点语义，不是领域函数变了。）
    pub already_frozen: bool,
    pub frozen_at: Option<String>,
    pub status: &'static str,
}

/// `POST /api/freeze` 的领域实现：**调 `version freeze` 的那一个函数**
/// （[`PgProjectStore::freeze`]）。重复冻结由 HTTP 层判成 409（见 [`freeze_for_api`]）。
pub async fn freeze_version_core(
    pool: &PgPool,
    project: &str,
    version: i32,
) -> Result<FreezeOutcome, OpError> {
    let projects = PgProjectStore::new(pool.clone());
    let p = projects
        .get_by_name(project)
        .await
        .map_err(OpError::db)?
        .ok_or_else(|| {
            OpError::not_found(
                "PROJECT_NOT_FOUND",
                format!("项目 {project:?} 不存在。"),
                "检查 `console serve --project` 的值。",
            )
        })?;
    let v = projects
        .version_by_seq(p.id, &p.name, version)
        .await
        .map_err(|e| match e {
            ProjectError::VersionNotFound { .. } => OpError::not_found(
                "VERSION_NOT_FOUND",
                format!("项目 {project:?} 没有第 {version} 版。"),
                "`GET /api/projects` 里有每一版的 seq；冻结一个不存在的版本没有意义。",
            ),
            other => OpError::db(other),
        })?;
    if v.frozen_at.is_some() {
        return Ok(FreezeOutcome {
            project: p.name,
            version: v.seq,
            version_id: v.id,
            already_frozen: true,
            frozen_at: v.frozen_at.map(rfc3339),
            status: "frozen",
        });
    }
    let f = projects.freeze(v.id).await.map_err(|e| match e {
        ProjectError::VersionNotFound { .. } => OpError::not_found(
            "VERSION_NOT_FOUND",
            format!("冻结时版本 v{version} 不见了。"),
            "重试；持续失败请检查是不是有人删了版本行。",
        ),
        other => OpError::db(other),
    })?;
    Ok(FreezeOutcome {
        project: p.name,
        version: f.seq,
        version_id: f.id,
        already_frozen: false,
        frozen_at: f.frozen_at.map(rfc3339),
        status: "frozen",
    })
}

/// HTTP 的 freeze：重复冻结 ⇒ **409**（R5 / AC-004）。
pub async fn freeze_for_api(
    pool: &PgPool,
    project: &str,
    version: i32,
) -> Result<FreezeOutcome, OpError> {
    let out = freeze_version_core(pool, project, version).await?;
    if out.already_frozen {
        return Err(OpError::conflict(
            "ALREADY_FROZEN",
            format!("v{version} 已经冻结过了（frozen_at = {}），本次没有改动。",
                out.frozen_at.as_deref().unwrap_or("-")),
            "冻结是**一次性**的显式动作：重复 freeze 不会刷新冻结时间，也不会产生新版本。\
             要改本体请新开一版（`ontok version new`）。",
        ));
    }
    Ok(out)
}

// ══════════════════════════════════════════════════════════════
// TTL 导出：与 `ontonomy export` 共用的**投影** + 控制面的**渲染入口**
// ══════════════════════════════════════════════════════════════

/// ⭐ REQ-124 · 导出**已经准备好的全部输入**（快照 + 投影 + 两份声明）。
///
/// 由 [`crate::cli::prepare_ttl_export`] 构造（CLI 与 HTTP 共用那一段读取/投影），
/// 本类型只负责**持有**它并提供渲染所需的借用。
///
/// # 为什么渲染不在这里收敛成一个函数给两边用
///
/// `tests/test_req107_source_enumeration.rs` 的 T2/T8 把「写出的绑定必须直接来自
/// `publish_customer(…)`」与「`OutletId::C6/D9` 的管线调用与写点同函数」钉在
/// **CLI** 上 —— 所以 CLI 的渲染必须留在 `ontology_cmd` 体内。
/// 能共用的是渲染之前的**一切**（读快照 / 声明解析 / 视图投影 / `TtlOptions`），
/// 那些正是两份实现会漂的地方。控制面这一侧用 [`render_ttl_export`]：
/// 同一个 `domain::pipeline::render`、同一份 `TtlPrepared` ⇒ 字节相同（AC-005）。
pub struct TtlPrepared {
    pub project: String,
    pub version: i32,
    pub base: String,
    pub model_view: bool,
    /// 快照本身（`TtlOptions.action_type_count` 与视图投影都从它来）。
    pub snapshot: crate::domain::snapshot::Snapshot,
    pub(crate) tenancy: Option<crate::domain::ttl::TenancyDecl>,
    pub(crate) schema: Option<crate::domain::ttl::SchemaTypes>,
    pub(crate) model: Option<crate::domain::model_view::ModelTrackView>,
    pub(crate) customer: Option<crate::domain::view::CustomerView>,
}

impl TtlPrepared {
    /// 序列化参数（借本结构里的两份声明；`TtlOptions` 是 `Copy`）。
    pub fn opts(&self) -> crate::domain::ttl::TtlOptions<'_> {
        crate::domain::ttl::TtlOptions {
            project: &self.project,
            version: self.version,
            base: &self.base,
            action_type_count: self.snapshot.action_types.len(),
            tenancy: self.tenancy.as_ref(),
            schema: self.schema.as_ref(),
        }
    }

    pub fn model(&self) -> &crate::domain::model_view::ModelTrackView {
        self.model
            .as_ref()
            .expect("model_view = true 时 prepare 一定投影了建模视图")
    }

    pub fn customer(&self) -> &crate::domain::view::CustomerView {
        self.customer
            .as_ref()
            .expect("客户视图分支一定投影了 CustomerView")
    }

    pub fn tenancy(&self) -> Option<&crate::domain::ttl::TenancyDecl> {
        self.tenancy.as_ref()
    }

    pub fn schema(&self) -> Option<&crate::domain::ttl::SchemaTypes> {
        self.schema.as_ref()
    }

    pub fn object_count(&self) -> usize {
        match (&self.model, &self.customer) {
            (Some(m), _) => m.entity_types.len(),
            (None, Some(c)) => c.objects.len(),
            _ => 0,
        }
    }

    pub fn relation_count(&self) -> usize {
        match (&self.model, &self.customer) {
            (Some(m), _) => m.link_types.len(),
            (None, Some(c)) => c.relations.len(),
            _ => 0,
        }
    }
}

/// `domain::pipeline::render` 的错误 ⇒ [`OpError`]（**不退出进程**）。
fn pipeline_failure(e: crate::domain::pipeline::PipelineError) -> OpError {
    if e.is_leak() {
        OpError::leak(e.outlet(), format!("{:?}", e.hits()))
    } else {
        OpError::render_failed(e.outlet(), e.message())
    }
}

/// ⭐ REQ-124 · **控制面**的 TTL 渲染入口。
///
/// CLI 的渲染入口是 `cli::publish_customer`（它把泄漏映射成
/// `EXIT_LEAKED_MODELING_TERMS`）；控制面不能用它 —— 服务进程不该因为一次导出
/// 请求而退出。两者调的是**同一个** `domain::pipeline::render`，输入是**同一个**
/// [`TtlPrepared`]，所以字节相同（AC-005）。
pub fn render_ttl_export(
    prep: &TtlPrepared,
) -> Result<crate::domain::pipeline::OutletRender, OpError> {
    if prep.model_view {
        crate::domain::pipeline::render(
            crate::domain::pipeline::OutletId::D9,
            crate::domain::pipeline::Product::ModelTurtle {
                view: prep.model(),
                opts: prep.opts(),
            },
        )
        .map_err(pipeline_failure)
    } else {
        crate::domain::pipeline::render(
            crate::domain::pipeline::OutletId::C6,
            crate::domain::pipeline::Product::CustomerTurtle {
                view: prep.customer(),
                opts: prep.opts(),
            },
        )
        .map_err(pipeline_failure)
    }
}

/// 渲染之后的诊断（租户落点 + range 分布）。**CLI 与控制面打同一批**。
///
/// 只收 `range_stats()` 的**只读回执**（不是产物本身）：与 REQ-119 的出口口径一致 ——
/// 判据/回执可以读产物上的分布，⛔ 不能读产物字节再加工。
pub fn report_ttl_export(
    prep: &TtlPrepared,
    stats: Option<&crate::domain::ttl::RangeStats>,
) {
    if prep.model_view {
        let tenancy = crate::domain::ttl::tenancy_report(prep.model(), prep.tenancy());
        if let Some(warning) = tenancy.warning() {
            eprintln!("{warning}");
        }
        crate::cli::eprint_ttl_stats("建模视图", stats, prep.schema().is_some());
    } else {
        crate::cli::eprint_ttl_stats("客户视图", stats, false);
    }
}

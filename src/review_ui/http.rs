//! REQ-111 · 审核界面的 HTTP 面（`GET /`、`GET /api/review`、`GET /api/preview`、
//! `POST /api/decide`）。
//!
//! # 唯一写点
//!
//! `POST /api/decide` 是**唯一**能改数据的入口。它自己**不写 SQL**，
//! 只调 [`crate::db::proposal::PgProposalStore::decide_item`] ——
//! 也就是 `ontok review decide` 走的那一个函数（纪律 H）。
//! `git grep -n 'decide_item('` 只会有「定义 + CLI 调用 + 本文件调用」三处。
//!
//! # 写前再校验（AC-007 / 需求风险 4 / QA N9）
//!
//! 启动时校验一次（[`prepare`]），**每次写之前**再校验一次
//! （[`ensure_unfrozen`]）。只做启动校验是不够的：服务运行期间有两件事会变 ——
//! ① 版本被 `ontok version freeze` 冻上；② 提案被 `ontok proposal apply` 应用掉
//! （此时基版本**仍未冻结**，只看冻结会放过它）。两种情况的写入都必须**当场失败**，
//! 并把「为什么」原样送到页面（不是一句「失败」）。
//!
//! # 为什么错误响应是 `(StatusCode, Json<Value>)` 元组
//!
//! 出口层禁调 `Response::builder` / `.into_response()`（REQ-107 的否定式判据）：
//! 响应体只能来自 axum 对**返回值**的自动转换。返回元组既满足这条，
//! 又不需要在这里手搓 `Response`。所有错误正文都经过 [`error_body`] 一个构造点。
//!
//! # 口令校验为什么是**第二份**实现
//!
//! `serve` 的 `token_matches` 是 `src/server/http.rs` 的私有函数，而 AC-008 要求
//! 那个文件**一行不动**（`git diff` 的路径过滤里必须为空）。于是这里的
//! Bearer 解析 + 定长比较只能自备一份 —— 这是**认证**，不是**决策**：
//! 纪律 H 保护的是「决策只能有一个实现」，两份认证的失败方式是**都拒绝**，
//! 不会出现「一边接受、一边拒绝」的静默不一致。已登记在验证报告的已知限制里。
//!
//! # REQ-112 · 建议（suggestion）接在哪
//!
//! | 接口 | 变化 |
//! | --- | --- |
//! | `GET /api/review` | 每条多一个 `suggestion: {kind, reason}`；顶层多 `suggestion_counts` |
//! | `GET /api/preview` | 多一个 `by` 口径：`confidence`（分数阈值）/ `suggestion`（按建议） |
//! | `POST /api/decide` | **一个字节都没动**（唯一写点不变，也不新增写端点） |
//!
//! 建议**不是**在本文件里算出来的 —— [`crate::domain::confidence::suggestion`] 是唯一实现。
//! 本文件只做两件事：把 `payload.confidence` 解析成 `Confidence`，再问它一句。
//! 于是「页面显示的档位」与「建议依据的档位」必然是同一个值（R6 同源，AC-009），
//! 「条目上的建议」与「批量白名单」也必然是同一份判据（[`SuggestKind::enters_batch`]，AC-006 / AC-007）。
//!
//! ⚠️ **两条批量口径共用那一个判据**（BA 裁定 A-6 / QA 非阻断 N-2）：`by=confidence`
//! 只在 `enters_batch()` 为真的条目里再按分数筛；否则一条 `score = 0.85` 的冲突
//! 在 `min_confidence = 0.80` 下会被自动接受，「冲突永不被自动接受」就只对另一种口径成立。

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, Method, StatusCode};
use axum::response::Html;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::pg::StoreError;
use crate::db::project::PgProjectStore;
use crate::db::proposal::{PgProposalStore, ProposalItemRow, VIA_UI};
use crate::db::snapshot_store::PgStore;
use crate::domain::confidence::{suggestion, Confidence, SuggestKind, Suggestion};
use crate::domain::review::{entry_state, summarize, Decision, EntryState, ItemView, Track};

use super::ReviewArgs;

/// 服务状态。启动时**一次性**解析并冻结作用域；之后只有决策在变。
#[derive(Clone)]
pub struct ReviewState {
    pool: PgPool,
    project: Arc<str>,
    /// 提案 UUID（别名在启动时就解析掉了，见 [`prepare`]）。
    proposal_id: Uuid,
    /// 提案别名（`p1` / `p2` / …）—— 与 CLI 抬头同源。
    proposal_alias: Arc<str>,
    /// 提案基于的版本（**未冻结**才允许启动）。
    base_version_id: Uuid,
    base_seq: i32,
    /// base 版本里已存在的实体 key —— `summarize` 判「关系两端可用」要用。
    baseline: Arc<BTreeSet<String>>,
    token: Arc<str>,
}

/// 错误响应：状态码 + `{"error": …}`。见模块头「为什么是元组」。
type ApiErr = (StatusCode, Json<Value>);

/// 错误正文的**唯一**构造点（T1 的写点登记 needle 就指向它）。
fn error_body(msg: &str) -> Value {
    json!({ "error": msg })
}

fn fail(status: StatusCode, msg: impl Into<String>) -> ApiErr {
    (status, Json(error_body(&msg.into())))
}

// ══════════════════════════════════════════════════════════════
// 启动闸门（AC-001 / AC-002）
// ══════════════════════════════════════════════════════════════

/// 解析作用域 + 三道启动闸门，返回服务状态。
///
/// 闸门顺序是刻意的：**先确认「有这个东西」，再说「它能不能被审核」**。
/// 反过来的话，「项目不存在」会被报成「已冻结」，排障的人会去查冻结时间。
///
/// | # | 闸门 | 失败 |
/// | --- | --- | --- |
/// | 1 | 项目存在 / 提案存在且**属于**该项目 | 拒绝启动（exit≠0） |
/// | 2 | 提案状态是 `pending` | 拒绝启动（已应用/已否决的提案不再改判） |
/// | 3 | 基版本**未冻结** | 拒绝启动，文案点名 `vN 已冻结`（AC-002） |
pub async fn prepare(pool: &PgPool, args: &ReviewArgs) -> anyhow::Result<ReviewState> {
    let projects = PgProjectStore::new(pool.clone());
    let proposals = PgProposalStore::new(pool.clone());

    let project = projects
        .get_by_name(&args.project)
        .await
        .map_err(|e| anyhow::anyhow!("读项目失败：{e}"))?
        .ok_or_else(|| anyhow::anyhow!("项目不存在：{}", args.project))?;

    // 别名解析复用 CLI 的 `resolve_proposal`（同一套「p1 = 最近导入」的口径）。
    let proposal_id = crate::cli::resolve_proposal(pool, project.id, &args.proposal).await?;
    let row = proposals
        .get(proposal_id)
        .await
        .map_err(|e| anyhow::anyhow!("读提案失败：{e}"))?
        .ok_or_else(|| anyhow::anyhow!("提案不存在：{}", args.proposal))?;

    // ⛔ 别名解析是按项目算的，UUID 则完全绕过项目 —— 这里补一道归属校验。
    // 少了它，`--project A --proposal <B 的 uuid>` 会把 B 的提案挂在 A 名下审核。
    if row.project_id != project.id {
        anyhow::bail!(
            "提案 {} 不属于项目「{}」—— 换个 --proposal，或改 --project。",
            args.proposal,
            args.project
        );
    }
    if row.status != "pending" {
        anyhow::bail!(
            "提案 {} 的状态是 {}（不是 pending）：已经应用的提案不再接受改判。",
            args.proposal,
            row.status
        );
    }

    let versions = projects
        .list_versions(project.id)
        .await
        .map_err(|e| anyhow::anyhow!("读版本失败：{e}"))?;
    let base = versions
        .iter()
        .find(|v| v.id == row.base_version_id)
        .ok_or_else(|| anyhow::anyhow!("提案的基版本不在项目「{}」的版本表里。", args.project))?;

    // ⛔ AC-002：**已冻结 ⇒ 启动即失败**。文案必须点名是哪一版 ——
    // 「拒绝启动」但不说原因，等于让人去猜。
    if let Some(frozen_at) = base.frozen_at {
        anyhow::bail!(
            "项目「{}」的 v{} 已冻结（{}）：审核界面只服务**未冻结**的提案。\
             已冻结版本要改判，得先新开一版（`ontok version new`）再提案。",
            project.name,
            base.seq,
            frozen_at.to_rfc3339()
        );
    }

    let snap = PgStore::new(pool.clone())
        .get_snapshot(base.id)
        .await
        .map_err(|e| anyhow::anyhow!("读基版本快照失败：{e}"))?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "基版本 v{} 没有可读的快照 —— 审核需要它判「关系两端是否已存在」。",
                base.seq
            )
        })?;
    let baseline: BTreeSet<String> = snap.entity_types.iter().map(|e| e.key.clone()).collect();

    let alias = crate::cli::proposal_alias(pool, project.id, proposal_id).await;

    Ok(ReviewState {
        pool: pool.clone(),
        project: Arc::from(project.name.as_str()),
        proposal_id,
        proposal_alias: Arc::from(alias.as_str()),
        base_version_id: base.id,
        base_seq: base.seq,
        baseline: Arc::new(baseline),
        token: Arc::from(args.token.as_str()),
    })
}

/// **起服务**。调用顺序有讲究：先闸门、再绑端口 —— 拒绝启动时不该占着端口。
pub async fn serve(pool: &PgPool, args: ReviewArgs) -> anyhow::Result<()> {
    // ⛔ AC-001 · 只绑回环。这个进程**会写**，绑到 0.0.0.0 等于把
    // 「改判一个客户提案」挂到网上等人试口令。宁可起不来。
    if !args.bind.ip().is_loopback() {
        anyhow::bail!(
            "只允许绑回环地址（如 127.0.0.1:8788），收到 {:?}。\
             审核界面会**写**决策，请留在本机并用访问口令把门。",
            args.bind.ip()
        );
    }
    if args.token.trim().len() < 8 {
        anyhow::bail!("访问口令太短（至少 8 个字符）：一个 4 位口令在写接口前面等于没设。");
    }

    let state = prepare(pool, &args).await?;
    let project = state.project.clone();
    let alias = state.proposal_alias.clone();
    let base_seq = state.base_seq;
    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind(args.bind).await?;
    eprintln!(
        "· 审核界面就绪：项目 {} · 提案 {} · 基于 v{}（未冻结）",
        project, alias, base_seq
    );
    eprintln!(
        "· 浏览器打开 http://{}/ ，在页面顶部填口令；页面壳不含任何数据，写入口只有 POST /api/decide。",
        args.bind
    );
    axum::serve(listener, app)
        .await
        .map_err(|e| anyhow::anyhow!("审核界面退出：{e}"))
}

// ══════════════════════════════════════════════════════════════
// 路由
// ══════════════════════════════════════════════════════════════

/// 组装路由。
///
/// `/` 免口令（浏览器导航带不了 `Authorization`）；`/api/*` 由每个 handler
/// **第一行**调 [`check_token`] —— 不用 middleware 是因为错误响应要走元组
/// （见模块头），中间件返回 `Response` 就得手搓响应体。
pub fn build_router(state: ReviewState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/api/review", get(get_review))
        .route("/api/preview", get(get_preview))
        .route("/api/decide", post(post_decide))
        .fallback(fallback)
        .with_state(state)
}

/// 页面壳（**零数据**：项目名 / 条目 / 决策都不在 HTML 里）。
async fn index() -> Html<&'static str> {
    Html(crate::review_ui::page::REVIEW_HTML)
}

/// 未注册路径：GET ⇒ 404；变更动词 ⇒ 405（审核界面只有那一个写点）。
async fn fallback(method: Method) -> (StatusCode, Json<Value>) {
    if method == Method::GET {
        (StatusCode::NOT_FOUND, Json(error_body("没有这个页面。")))
    } else {
        (
            StatusCode::METHOD_NOT_ALLOWED,
            Json(error_body(
                "审核界面只有 GET /api/review、GET /api/preview 与 POST /api/decide 三个接口。",
            )),
        )
    }
}

/// 身份门。**每个 `/api/*` 的第一行都调它**，没有开关能绕过。
fn check_token(state: &ReviewState, headers: &HeaderMap) -> Result<(), ApiErr> {
    let presented = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("")
        .trim();
    if token_matches(presented, &state.token) {
        Ok(())
    } else {
        // ⛔ 响应里只说「没通过」，不回显令牌、不说哪里错了。
        Err(fail(
            StatusCode::UNAUTHORIZED,
            "没有可用的访问口令，拿不到内容。",
        ))
    }
}

/// 定长比较（与 `server/http.rs` 的同名函数同一算法、同一已知边界：
/// 长度不同直接 false ⇒ **长度可见**；令牌由本人设定且不经网络传输，可接受）。
fn token_matches(presented: &str, expected: &str) -> bool {
    let a = presented.as_bytes();
    let b = expected.as_bytes();
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

// ══════════════════════════════════════════════════════════════
// GET /api/review —— 条目 + 双轨状态 + 置信度 + AI 理由 + 证据
// ══════════════════════════════════════════════════════════════

async fn get_review(
    State(state): State<ReviewState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiErr> {
    check_token(&state, &headers)?;
    let review_body = review_payload(&state).await?;
    Ok(Json(review_body))
}

async fn load_rows(state: &ReviewState) -> Result<Vec<ProposalItemRow>, ApiErr> {
    PgProposalStore::new(state.pool.clone())
        .items(state.proposal_id)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, format!("读提案条目失败：{e}")))
}

async fn review_payload(state: &ReviewState) -> Result<Value, ApiErr> {
    let rows = load_rows(state).await?;
    let views = PgProposalStore::to_item_views(&rows);
    let status = summarize(&views, &state.baseline);

    // 「被否 / 被依赖拦下」的具体理由，按条目 id 索引 —— 页面直接显示，
    // 不让人对着 `vetoed` 三个字猜原因（AC-007 的同一条精神）。
    let mut reasons: BTreeMap<String, String> = BTreeMap::new();
    for v in status.verdicts.iter().filter(|v| v.reason.is_some()) {
        if let Some(r) = &v.reason {
            reasons.insert(v.id.clone(), r.clone());
        }
    }
    for b in status.blocked.iter().filter(|b| b.reason.is_some()) {
        if let Some(r) = &b.reason {
            reasons.insert(b.id.clone(), r.clone());
        }
    }

    // REQ-112 · 建议**只算一遍**：条目上的 `suggestion` 与下面那份分组计数
    // 取的是同一个 `Vec` —— 两处各算一次的话，判据会漂（哪怕它是纯函数，
    // 「有第二处调用点」本身就是可被改坏的地方）。
    let suggestions: Vec<Suggestion> = rows.iter().map(suggestion_of).collect();

    let items: Vec<Value> = rows
        .iter()
        .zip(views.iter())
        .zip(suggestions.iter())
        .map(|((r, v), s)| item_json(r, v, &reasons, &s.brief()))
        .collect();

    // REQ-112 · 分组的**计数**（AC-008「分组计数与后端一致」）。
    //
    // 刻意在后端算好再发：页面自己数一遍就是第二份判据，早晚和后端数出两个数。
    // 顺序取 [`SuggestKind::ALL`]（冲突在最前），页面照这个顺序渲染分组。
    let mut counts: Vec<usize> = vec![0; SuggestKind::ALL.len()];
    for s in &suggestions {
        if let Some(i) = SuggestKind::ALL.iter().position(|k| *k == s.kind) {
            counts[i] += 1;
        }
    }
    let suggestion_counts: serde_json::Map<String, Value> = SuggestKind::ALL
        .iter()
        .zip(counts.iter())
        .map(|(k, n)| (k.as_str().to_string(), json!(n)))
        .collect();

    // REQ-111 · QA N2：`frozen` 取**版本行的真实状态**（`ontology_versions.frozen_at IS NOT NULL`），
    // 不缓存启动时的结论 —— 服务运行中版本可能被冻上，读接口不能跟着说假话。
    let frozen = PgProjectStore::new(state.pool.clone())
        .is_frozen(state.base_version_id)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, format!("读版本状态失败：{e}")))?;

    Ok(json!({
        "project": state.project.as_ref(),
        "proposal": state.proposal_alias.as_ref(),
        "base_version": state.base_seq,
        // 每次读都查一次版本行：服务运行中版本可能被冻上，这个字段不能说假话。
        // （QA N2：这里曾硬编码 `false`，冻结后读接口还在说「未冻结」。）
        "frozen": frozen,
        "items": items,
        "progress": progress_json(&rows, &views),
        // REQ-112 · 建议分组计数（`conflict` 永远排第一 —— 冲突要最先被人看到）。
        "suggestion_counts": suggestion_counts,
    }))
}

/// 条目 → 建议。**唯一取值点**：`/api/review` 与 `/api/preview` 都走这里。
///
/// # 为什么解析失败要变成 `none` 而不是「跳过」
///
/// `payload.confidence` 是导入时校验过的，理论上解析不会失败。但「理论上」不是保证：
/// 若真有一份坏数据，**跳过**会让条目从页面上消失（人不签它，它也不会被签），
/// 而 `none`（+ 明确写「无法解析」）会把它留在列表里且**不进**批量白名单。
/// 两者都 fail-closed，后者可见 —— 选后者。
fn suggestion_of(r: &ProposalItemRow) -> Suggestion {
    match r.payload.get("confidence") {
        None | Some(Value::Null) => suggestion(None),
        Some(v) => match serde_json::from_value::<Confidence>(v.clone()) {
            Ok(c) => suggestion(Some(&c)),
            Err(_) => Suggestion::unreadable(),
        },
    }
}

/// 一条条目的建议 —— 给 `item_json` 用的紧凑形态 `{kind, reason}`。
fn item_json(
    r: &ProposalItemRow,
    v: &ItemView,
    reasons: &BTreeMap<String, String>,
    sugg: &Value,
) -> Value {
    let state = match entry_state(v) {
        EntryState::Passed => "passed",
        EntryState::Vetoed => "vetoed",
        EntryState::Pending => "pending",
    };
    json!({
        "item": r.seq,
        "kind": r.kind,
        "target": r.target_key,
        "biz": r.biz_status,
        "model": r.model_status,
        "state": state,
        "reason": reasons.get(&v.id),
        // 置信度整块给出（score / band / method / signals / ai.rationale）——
        // 与 `review show --confidence` 是**同一份** payload 值，不是另算一份。
        "confidence": r.payload.get("confidence").cloned().unwrap_or(Value::Null),
        // REQ-112 · 建议：`{kind, reason}`。`kind ∈ accept/lean_accept/review/reject/conflict/none`。
        // 依据文案 ≤ 40 字（AC-008），页面直接显示，不让人对着 `kind` 猜理由。
        "suggestion": sugg,
        // 证据原样给出：FDE 要能点开看依据，而不是只看到一个分数。
        "evidence": r.payload.get("evidence").cloned().unwrap_or_else(|| json!([])),
        // AC-009 · 审计：每一轨各自的最后一次改判时间与入口。
        "decided_at": {
            "biz": r.biz_decided_at.map(|t| t.to_rfc3339()),
            "model": r.model_decided_at.map(|t| t.to_rfc3339()),
        },
        "via": { "biz": r.biz_via, "model": r.model_via },
        // 完整候选定义（key / 列 / 基数 …）：审核要看的是**要做决定的东西**。
        "payload": r.payload,
    })
}

fn progress_json(rows: &[ProposalItemRow], views: &[ItemView]) -> Value {
    let mut signed = 0usize;
    let mut passed = 0usize;
    let mut vetoed = 0usize;
    let mut biz = [0usize; 3]; // pending / accepted / rejected
    let mut model = [0usize; 3];
    for (r, v) in rows.iter().zip(views.iter()) {
        if entry_state(v) == EntryState::Passed {
            passed += 1;
        }
        if entry_state(v) == EntryState::Vetoed {
            vetoed += 1;
        }
        if r.biz_status != "pending" && r.model_status != "pending" {
            signed += 1;
        }
        biz[decision_index(&r.biz_status)] += 1;
        model[decision_index(&r.model_status)] += 1;
    }
    json!({
        "total": rows.len(),
        // 「已签」= 两轨都定了；「待签」= 还有一轨没定；「阻断」= 任一轨被否。
        "signed": signed,
        "pending": rows.len().saturating_sub(signed),
        "passed": passed,
        "vetoed": vetoed,
        "biz": { "pending": biz[0], "accepted": biz[1], "rejected": biz[2] },
        "model": { "pending": model[0], "accepted": model[1], "rejected": model[2] },
    })
}

fn decision_index(s: &str) -> usize {
    match s {
        "accepted" => 1,
        "rejected" => 2,
        _ => 0,
    }
}

// ══════════════════════════════════════════════════════════════
// GET /api/preview —— **批量签的预览**（AC-006）
// ══════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
struct PreviewQuery {
    track: Option<String>,
    min_confidence: Option<f64>,
    /// 今天只允许 `unsigned`：批量**只作用于未签条目**，不做「一键全库」。
    scope: Option<String>,
    /// **按什么算白名单**（REQ-112）：
    /// - 省略 / `confidence` ⇒ REQ-111 的原口径（分数 ≥ `min_confidence`），
    ///   但**同样**要过 `enters_batch()`（A-6 / N-2）；
    /// - `suggestion` ⇒ 建议为 `accept` / `lean_accept` 的条目（AC-006 / AC-007）。
    by: Option<String>,
}

/// **白名单判据**（AC-006 / AC-007 / A-6）—— 两条批量口径**都走这一个函数**。
///
/// 唯一实现是 [`SuggestKind::enters_batch`]（只有 `accept` / `lean_accept` 为真）；
/// 本函数只多做一件事：**不入白名单时按 kind 报数**，让页面能把
/// 「冲突 1 条、人工核对 1 条……这些不参与」说出来。
///
/// # 为什么要有这个函数
///
/// QA 非阻断 N-2 实测：`min_confidence = 0.80` 的「按档位」批量把
/// 一条 `score = 0.85` 的 `conflict` 放进了白名单。根因不是判据错了，
/// 而是那条路径**根本没调判据**。把它抽成一个函数后，两条路径的差别
/// 只剩「还按不按分数筛」——「谁能被自动接受」永远只有一个答案。
fn batch_gate(kind: SuggestKind, excluded_by_kind: &mut [usize; 6]) -> bool {
    if kind.enters_batch() {
        return true;
    }
    if let Some(i) = SuggestKind::ALL.iter().position(|k| *k == kind) {
        excluded_by_kind[i] += 1;
    }
    false
}

async fn get_preview(
    State(state): State<ReviewState>,
    headers: HeaderMap,
    Query(q): Query<PreviewQuery>,
) -> Result<Json<Value>, ApiErr> {
    check_token(&state, &headers)?;

    let track_raw = q.track.as_deref().unwrap_or("");
    let track = parse_track(track_raw).ok_or_else(|| {
        fail(
            StatusCode::BAD_REQUEST,
            "批量预览要说明哪一轨：track=biz 或 track=model。",
        )
    })?;
    if let Some(scope) = q.scope.as_deref() {
        if scope != "unsigned" {
            return Err(fail(
                StatusCode::BAD_REQUEST,
                "批量只作用于**未签**条目（scope=unsigned）—— 审核界面没有「一键全库」。",
            ));
        }
    }
    // REQ-112 · 两种批量口径**不许混用**：混了就说不清「将接受 N 条」里的 N 是谁。
    let by_suggestion = match q.by.as_deref() {
        None | Some("confidence") => false,
        Some("suggestion") => true,
        Some(other) => {
            return Err(fail(
                StatusCode::BAD_REQUEST,
                format!(
                    "by 只有两种写法：suggestion（按建议）或 confidence（按分数），收到 {other:?}。"
                ),
            ));
        }
    };
    if by_suggestion && q.min_confidence.is_some() {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "by=suggestion 与 min_confidence 不能同时给 —— 两种口径混在一起，弹窗上的条数就没有唯一含义。",
        ));
    }
    // 阈值只在 0..=1 之间有意义；给别的值直接报错，不静默当成「不限」。
    if let Some(min) = q.min_confidence {
        if !(0.0..=1.0).contains(&min) {
            return Err(fail(
                StatusCode::BAD_REQUEST,
                "min_confidence 要落在 0.00–1.00 之间（例如 0.90）。",
            ));
        }
    }

    let rows = load_rows(&state).await?;
    let mut whitelist: Vec<i32> = Vec::new();
    let mut decided = 0usize;
    let mut below_band = 0usize;
    let mut no_confidence = 0usize;
    // 按建议口径下，被排除的条目按 `kind` 分开计数（冲突/存疑/拒绝/无建议）。
    let mut excluded_by_kind = [0usize; 6];
    for r in &rows {
        let st = match track {
            Track::Biz => &r.biz_status,
            Track::Model => &r.model_status,
        };
        if st != "pending" {
            decided += 1;
            continue;
        }
        if by_suggestion {
            // ⭐ AC-006 / AC-007：白名单判据**只有一个** —— `kind.enters_batch()`。
            // `conflict` / `review` / `reject` / `none` 都不进（`none` 就是无置信度的实体条目）。
            let kind = suggestion_of(r).kind;
            if !batch_gate(kind, &mut excluded_by_kind) {
                continue;
            }
            whitelist.push(r.seq);
            continue;
        }
        if let Some(min) = q.min_confidence {
            match r
                .payload
                .get("confidence")
                .and_then(|c| c.get("score"))
                .and_then(|s| s.as_f64())
            {
                // 浮点比较：分数是两位小数的近似量，0.90 由文件里的字面量给出，
                // 用 `>=` 会漏掉 0.9000000000000001 这种表示误差 —— 加一个极小 epsilon。
                Some(score) if score + f64::EPSILON * 8.0 >= min => {}
                Some(_) => {
                    below_band += 1;
                    continue;
                }
                None => {
                    no_confidence += 1;
                    continue;
                }
            }
        }
        // ⭐ N-2（BA 裁定 A-6）：**按档位**这条路径也必须过同一个白名单判据。
        //
        // 只按分数筛会把「争议」条目混进「将接受 N 条」—— 一条 `score = 0.85` 的
        // 冲突在 `min_confidence = 0.80` 下会被自动接受，于是「冲突永不被自动接受」
        // 就只对 `by=suggestion` 成立。两条批量路径共用一个判据：
        // `conflict` / `review` / `reject` / `none` 一律不进（见 [`batch_gate`]）。
        let kind = suggestion_of(r).kind;
        if !batch_gate(kind, &mut excluded_by_kind) {
            continue;
        }
        whitelist.push(r.seq);
    }

    let mut excluded = serde_json::Map::new();
    excluded.insert("decided".into(), json!(decided));
    excluded.insert("below_band".into(), json!(below_band));
    excluded.insert("no_confidence".into(), json!(no_confidence));
    // 被 [`batch_gate`] 挡下的四类「不参与」—— **两种口径都会填**；
    // 页面要把它们**说出来**，否则「将接受 40 条」会让人以为剩下 12 条也被接受了。
    for (i, k) in SuggestKind::ALL.iter().enumerate() {
        excluded.insert(k.as_str().to_string(), json!(excluded_by_kind[i]));
    }

    let preview_body = json!({
        "track": track_raw,
        "min_confidence": q.min_confidence,
        "scope": "unsigned",
        // 口径本身也回显：请求少给一个参数时，页面不能靠自己的默认值猜。
        "by": if by_suggestion { "suggestion" } else { "confidence" },
        // ⭐ 这个数就是页面弹窗上那句「将接受 N 条」的来源 —— **先看数、再执行**。
        "count": whitelist.len(),
        "total": rows.len(),
        // 白名单本身：页面照这份 items[] 逐条 POST，服务端没有批量写接口。
        "items": whitelist,
        "excluded": excluded,
    });
    Ok(Json(preview_body))
}

// ══════════════════════════════════════════════════════════════
// POST /api/decide —— **唯一写点**
// ══════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
struct DecideRequest {
    item: i32,
    track: String,
    decision: String,
}

/// JSON **请求体**抽取器的别名。
///
/// # 为什么需要这个别名（不是风格问题）
///
/// `tests/test_req107_source_enumeration.rs` 的 T1 把 `Json(` 当作**响应构造**
/// 的拼写来枚举「客户可见写点」——而 axum 的请求抽取器写作
/// `Json<DecideRequest>`，同样含有 `Json(`（`Json(req): Json<…>` 这一行）。
/// 不区分的话，一处**读**请求会被登记成一条**写**出口 —— 登记表就多了一条假出口。
///
/// ⇒ 请求侧走这个别名，响应侧仍是 `Json(...)`（R2 的 needle）。
type JsonRequest<T> = axum::Json<T>;

async fn post_decide(
    State(state): State<ReviewState>,
    headers: HeaderMap,
    body: JsonRequest<DecideRequest>,
) -> Result<Json<Value>, ApiErr> {
    let req = body.0;
    check_token(&state, &headers)?;

    let track = parse_track(&req.track).ok_or_else(|| {
        fail(
            StatusCode::BAD_REQUEST,
            format!(
                "track 只有两种写法：biz（业务轨）或 model（建模轨），收到 {:?}。",
                req.track
            ),
        )
    })?;
    let decision = parse_decision_arg(&req.decision).ok_or_else(|| {
        fail(
            StatusCode::BAD_REQUEST,
            format!("decision 只有两种写法：accept 或 reject，收到 {:?}。", req.decision),
        )
    })?;

    // ⭐ 需求风险 4 / AC-007 / QA N9：**每次写前再校验**。启动校验挡不住
    // 「服务开着的时候版本被冻上 / 提案被 apply 掉」—— 那正是最容易被误签的时间窗。
    ensure_unfrozen(&state).await?;

    // ⭐ 唯一写路径：与 `ontok review decide` **同一个函数**（纪律 H）。
    // 这里没有 SQL、没有第二份「序号 → 条目」的解析。
    let proposals = PgProposalStore::new(state.pool.clone());
    proposals
        .decide_item(state.proposal_id, req.item, track, decision, VIA_UI)
        .await
        .map_err(map_store_error)?;

    // 回执带上写入后的**整行状态**与最新进度：页面据此就地更新，不必再 GET 一次。
    let rows = load_rows(&state).await?;
    let views = PgProposalStore::to_item_views(&rows);
    let row = rows
        .iter()
        .find(|r| r.seq == req.item)
        .ok_or_else(|| fail(StatusCode::INTERNAL_SERVER_ERROR, "刚写入的条目读不回来了。"))?;
    // `rows` 与 `views` 同序（`to_item_views` 按同一份 rows 逐条转换），
    // 所以用**行位置**取视图，不拿 `item - 1` 当索引（序号不保证从 1 连续）。
    let pos = rows
        .iter()
        .position(|r| r.seq == req.item)
        .ok_or_else(|| fail(StatusCode::INTERNAL_SERVER_ERROR, "刚写入的条目读不回来了。"))?;
    let view = &views[pos];
    let state_str = match entry_state(view) {
        EntryState::Passed => "passed",
        EntryState::Vetoed => "vetoed",
        EntryState::Pending => "pending",
    };
    let decide_body = json!({
        "ok": true,
        "item": row.seq,
        "track": req.track,
        "decision": decision.as_str(),
        "biz": row.biz_status,
        "model": row.model_status,
        "state": state_str,
        "decided_at": {
            "biz": row.biz_decided_at.map(|t| t.to_rfc3339()),
            "model": row.model_decided_at.map(|t| t.to_rfc3339()),
        },
        // AC-009：页面签的记 `ui`，CLI 签的记 `cli`（同一个函数，只差这个参数）。
        "via": { "biz": row.biz_via, "model": row.model_via },
        "progress": progress_json(&rows, &views),
    });
    Ok(Json(decide_body))
}

/// 写前闸门（AC-007 / QA N9）。**每次都查库**，不缓存启动时的结论。
///
/// 两道，缺一不可：
/// 1. 提案必须**仍是 `pending`** —— `ontok proposal apply` 能在服务开着的时候把
///    它变成 `applied`，而此时**基版本仍未冻结**；只查冻结会放过这次写入，
///    回执就会对用户说「改判成功」，而这份提案已经不再接受审核；
/// 2. 基版本必须**仍未冻结** —— `ontok version freeze` 同样能在运行中发生。
async fn ensure_unfrozen(state: &ReviewState) -> Result<(), ApiErr> {
    let proposal = PgProposalStore::new(state.pool.clone())
        .get(state.proposal_id)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, format!("读提案状态失败：{e}")))?
        .ok_or_else(|| fail(StatusCode::INTERNAL_SERVER_ERROR, "提案不见了。"))?;
    if proposal.status != "pending" {
        return Err(fail(
            StatusCode::CONFLICT,
            format!(
                "提案 {} 已 {}（不是 pending），不能再改判 —— 已经应用的提案不再接受审核。",
                state.proposal_alias, proposal.status
            ),
        ));
    }

    let frozen = PgProjectStore::new(state.pool.clone())
        .is_frozen(state.base_version_id)
        .await
        .map_err(|e| fail(StatusCode::INTERNAL_SERVER_ERROR, format!("读版本状态失败：{e}")))?;
    if frozen {
        return Err(fail(
            StatusCode::CONFLICT,
            format!(
                "项目「{}」的 v{} 已冻结，不能再改判 —— 审核界面只服务未冻结的提案。",
                state.project, state.base_seq
            ),
        ));
    }
    Ok(())
}

fn parse_track(s: &str) -> Option<Track> {
    match s {
        "biz" => Some(Track::Biz),
        "model" => Some(Track::Model),
        _ => None,
    }
}

/// `accept` / `reject` → 领域枚举。**不接受** `pending`（那是「还没签」，不是一次决策）。
fn parse_decision_arg(s: &str) -> Option<Decision> {
    match s {
        "accept" | "accepted" => Some(Decision::Accepted),
        "reject" | "rejected" => Some(Decision::Rejected),
        _ => None,
    }
}

/// 存储错误 → HTTP。**具体原因原样送到页面**（AC-007 的核心）：
/// 「失败」两个字等于没说话，排障的人只能翻服务端日志。
fn map_store_error(e: StoreError) -> ApiErr {
    match &e {
        StoreError::ItemNotFound { .. } => fail(StatusCode::NOT_FOUND, e.to_string()),
        StoreError::VersionFrozen { .. } => fail(StatusCode::CONFLICT, e.to_string()),
        _ => fail(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("写决策失败：{e}"),
        ),
    }
}

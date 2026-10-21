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
//! # 四次「写前再校验」（AC-007 / 需求风险 4）
//!
//! 启动时校验一次（[`prepare`]），**每次写之前**再校验一次
//! （[`ensure_unfrozen`]）。只做启动校验是不够的：版本可能在服务运行期间被
//! `ontok version freeze` 冻上，而页面还开着 —— 那时的写入必须**当场失败**，
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

    let items: Vec<Value> = rows
        .iter()
        .zip(views.iter())
        .map(|(r, v)| item_json(r, v, &reasons))
        .collect();

    Ok(json!({
        "project": state.project.as_ref(),
        "proposal": state.proposal_alias.as_ref(),
        "base_version": state.base_seq,
        // 启动时已确认未冻结；运行中被冻结会在 /api/decide 上当场报 409。
        "frozen": false,
        "items": items,
        "progress": progress_json(&rows, &views),
    }))
}

fn item_json(r: &ProposalItemRow, v: &ItemView, reasons: &BTreeMap<String, String>) -> Value {
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
    for r in &rows {
        let st = match track {
            Track::Biz => &r.biz_status,
            Track::Model => &r.model_status,
        };
        if st != "pending" {
            decided += 1;
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
        whitelist.push(r.seq);
    }

    let preview_body = json!({
        "track": track_raw,
        "min_confidence": q.min_confidence,
        "scope": "unsigned",
        // ⭐ 这个数就是页面弹窗上那句「将接受 N 条」的来源 —— **先看数、再执行**。
        "count": whitelist.len(),
        "total": rows.len(),
        // 白名单本身：页面照这份 items[] 逐条 POST，服务端没有批量写接口。
        "items": whitelist,
        "excluded": {
            "decided": decided,
            "below_band": below_band,
            "no_confidence": no_confidence,
        },
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

    // ⭐ 需求风险 4 / AC-007：**每次写前再校验冻结**。启动校验挡不住
    // 「服务开着的时候版本被冻上」—— 那正是最容易被误签的时间窗。
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

/// 写前冻结校验（AC-007）。**每次都查库**，不缓存启动时的结论。
async fn ensure_unfrozen(state: &ReviewState) -> Result<(), ApiErr> {
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

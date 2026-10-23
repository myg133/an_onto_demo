//! REQ-126 · **客户门户** —— 租户锁死、只能问预置问题。
//!
//! ```text
//! ontok portal serve --bind 127.0.0.1:8900 --tokens portal-tokens.json \
//!     --project <P> --version <V> --schema schema.json \
//!     --policy query-policy.json --questions portal-questions.json
//! ```
//!
//! # 它和内部工作台（REQ-125）是什么关系
//!
//! **复用写法，不复用进程**：同一个仓、同一种页面风格、同一种「口令只进 sessionStorage」
//! 的登录态；但**独立命令、独立凭据（`kind: "portal"`）、独立端口、独立进程**。
//! `console serve` 的 admin token 拿到这里 ⇒ **401**（这里根本没有那套表）；
//! 门户口令拿到 `console` ⇒ **403**（REQ-124 的 kind 校验认得它、不接受它）。
//!
//! # 端点只有四个
//!
//! | 方法 | 路径 | 说明 |
//! | --- | --- | --- |
//! | GET | `/` | 页面壳（**零数据**，免 token） |
//! | GET | `/api/questions` | 问题列表（标题 + 说明，⛔ 不含 SPARQL） |
//! | POST | `/q/<id>` | 执行一个问题 |
//! | GET | `/api/health` | 免 token，只给版本 |
//!
//! ⛔ 没有 `/api/sparql`、没有 `/api/catalog`、没有任何写操作、没有项目/版本切换
//! （进程级固定）。客户能问什么 = `portal-questions.json` 里逐条列出的那些。
//!
//! # 租户只能来自口令 —— 这是**结构**，不是约定
//!
//! | 攻击面 | 结果 |
//! | --- | --- |
//! | 请求体带 `tenant` | **403** `TENANT_FROM_CLIENT`（拒绝，不是忽略） |
//! | URL 带 `tenant`（或任何查询参数） | **400** `QUERY_PARAM_FORBIDDEN` |
//! | 无口令 / 错口令 | **401** |
//! | 别人的口令 | 那是另一张口令 —— 它绑定的租户就是它唯一的身份 |
//!
//! 代码里**没有**「从请求读 tenant」这条路径：[`run_question`] 的租户参数是
//! [`tokens::PortalTokenEntry::tenant`]，类型上就没有第二个来源。
//!
//! # 执行：复用 REQ-122 的 `run_query`，⛔ 不另写查询路径
//!
//! [`run_question`] 只做三件事：拿口令的租户拼 [`PlanInput`] → 调
//! [`crate::sparql::run_query`]（**同一个**规划器 / 注入 / 只读执行）→ 把
//! [`crate::sparql::QueryResult`] 投影成 [`PortalAnswer`]。
//!
//! ## 为什么不用 `query::run_sparql`（REQ-123 的适配器）
//!
//! 它的回执带着 `sql_echo` / `tables` / `columns` / `join_states`（Agent 面要的），
//! 而门户**一个都不能给客户**；它的审计行也没有「问题 id」（AC-009 要）。
//! 拿它再「删字段」是一种**黑名单**投影：`SparqlReceipt` 明天多一个字段，
//! 门户就会默认把它转发出去，而没有任何判据会红。
//!
//! 所以门户用**白名单**投影：[`PortalAnswer`] 逐字段从 [`crate::sparql::QueryResult`]
//! 里取值，`sql_echo` / `tables` / `tenant_applied` **没有对应的字段** ——
//! 「漏出去」在类型层写不出来。租户注入仍然只来自 `run_query`（同一份实现）。
//!
//! # 结果必须解释清楚（AC-004）
//!
//! [`PortalAnswer`] 里三样东西是**必须**有的：
//!
//! 1. `objects[]`：涉及哪些业务对象 —— **业务名**（建模轨的 `display_name`），
//!    ⛔ 不是表名；带上覆盖范围（`complete` / `main_only`）；
//! 2. `warnings[]`：`coverage: main_only` ⇒ 显著提示（页面里是 2px 边框的告警块）；
//! 3. `truncated` + 上限：截断时必须说「只显示前 N 行」。
//!
//! ⛔ 响应里**没有** `sql_echo` / 表名 / 列名 / 属性 IRI。另外还有两道兜底：
//! 启动校验拒绝「投影实例 IRI」与「变量名与列名同名」的问题（见 [`questions`]），
//! 运行期再把**单元格**里出现的本体 IRI 换成 `[内部标识已隐藏]` 并计入告警。
//!
//! # 限额（两道，都来自口令）
//!
//! | 限额 | 来自 | 超了会怎样 |
//! | --- | --- | --- |
//! | 行数 `max_rows` | 口令那一行 | 截断 + 页面写「只显示前 N 行」 |
//! | 超时 `--timeout-ms` | 服务启动参数 | **504 `TIMEOUT`** + 固定话术 |
//! | 并发 `max_concurrency` | 口令那一行 | **429 `TOO_MANY_CONCURRENT`**（[`limit`]） |
//!
//! ⭐ 并发闸与 `query serve`（REQ-123）对齐：**同上限、同隔离粒度（每 token）、
//! 同一条「不排队」纪律** —— 拿不到名额就立刻 429，⛔ 不把请求挂起等到
//! `acquire_timeout`（那会让客户看到「转 10 秒才说忙」）。
//!
//! # 已知限制（⛔ 不写成「已封堵」）
//!
//! - **没有分页**：按口令的 `max_rows` 截断，页面写「只显示前 N 行」；
//! - 并发闸是**进程内**的：多副本部署时每个副本各算一份（要全局口径得上
//!   Redis / PG advisory lock，那是另一条需求）；且它只卡「同时几个」，
//!   **不卡速率**（每秒多少次是另一件事）；
//! - 并发总量没有「全局上限」：上界 = 口令数 × 各自 `max_concurrency`（运维写死）。
//!   源库只读池只有 2 条连接 ⇒ 超过 2 个并发查询会在**池上排队**（不会出错，
//!   但延迟会上去）—— 与 `query serve` 同一条已知边界；
//! - 错误正文是**固定话术**：真原因（含 SQL / 表名）只进 stderr 与审计 code，
//!   客户看不到 —— 代价是「为什么失败」要靠服务方查日志；
//! - 门户只服务**一个已冻结的版本**（与 `query serve` 同一条闸门）：换版本 = 换进程。

pub mod audit;
pub mod http;
pub mod limit;
pub mod page;
pub mod questions;
pub mod serve;
pub mod tokens;

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use axum::http::StatusCode;
use serde::Serialize;
use sqlx::postgres::PgPool;

use crate::sparql::exec::SessionFacts;
use crate::sparql::plan::PlanInput;
use crate::sparql::policy::{QueryPolicy, TableDecision};
use crate::sparql::QueryResult;

use questions::{Question, QuestionCard};
use tokens::PortalTokenEntry;

/// 声明文件的默认落点（与其它命令同一条口径：默认值就是**仓库根**的文件名）。
pub const DEFAULT_POLICY: &str = "query-policy.json";
pub const DEFAULT_SCHEMA: &str = "code/out/schema.json";
pub const DEFAULT_QUESTIONS: &str = "portal-questions.json";

/// 单次问题的默认超时（毫秒）。客户点一下不该等一分钟。
pub const DEFAULT_TIMEOUT_MS: u64 = 10_000;

/// 单元格里的内部标识被换成的文字（AC-004 的兜底）。
pub const HIDDEN_VALUE: &str = "[内部标识已隐藏]";

/// 服务状态（启动时一次性载入；运行期只有审计在变）。
pub struct PortalService {
    /// **源库**只读连接池（与 `query serve` 同一条 `connect_read_only` 路径）。
    pub source: PgPool,
    /// 只读会话的**读回事实**（启动横幅打印它，⛔ 不写死）。
    pub facts: SessionFacts,
    pub mapping: crate::domain::r2rml::R2rmlMapping,
    pub schema: crate::domain::ttl::SchemaTypes,
    pub policy: QueryPolicy,
    pub ontology_iri: String,
    pub project: String,
    pub version: i32,
    pub timeout_ms: u64,
    pub tokens: tokens::PortalTokens,
    pub questions: questions::QuestionSet,
    /// 表 key → **业务名**（启动校验保证：问题涉及的每一张表都在这里）。
    pub names: BTreeMap<String, String>,
    pub audit: audit::AuditLog,
    /// ⭐ **每口令并发闸**（[`limit`]）：`max_concurrency` 在这里真的生效。
    /// 拿不到名额 ⇒ 立刻 429，⛔ 不排队。
    pub limiter: limit::Limiter,
}

/// 构造参数（`serve` 与测试共用**这一个**构造器 —— 不另搭一份接线）。
pub struct PortalParts {
    pub source: PgPool,
    pub facts: SessionFacts,
    pub mapping: crate::domain::r2rml::R2rmlMapping,
    pub schema: crate::domain::ttl::SchemaTypes,
    pub policy: QueryPolicy,
    pub ontology_iri: String,
    pub project: String,
    pub version: i32,
    pub timeout_ms: u64,
    pub tokens: tokens::PortalTokens,
    pub questions: questions::QuestionSet,
    pub names: BTreeMap<String, String>,
    pub audit: audit::AuditLog,
}

impl PortalService {
    pub fn new(p: PortalParts) -> Self {
        Self {
            source: p.source,
            facts: p.facts,
            mapping: p.mapping,
            schema: p.schema,
            policy: p.policy,
            ontology_iri: p.ontology_iri,
            project: p.project,
            version: p.version,
            timeout_ms: p.timeout_ms,
            tokens: p.tokens,
            questions: p.questions,
            names: p.names,
            audit: p.audit,
            limiter: limit::Limiter::default(),
        }
    }
}

// ══════════════════════════════════════════════════════════════
// 对外的回答形状（**白名单**：字段清单就是「允许给客户看的东西」的全集）
// ══════════════════════════════════════════════════════════════

/// 一个参与的业务对象（**业务名** + 覆盖范围）。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ObjectInfo {
    /// 业务名（建模轨的 `display_name`）。⛔ 不是表名。
    pub name: String,
    /// `complete` / `main_only`（机器可读）。
    pub coverage: &'static str,
    /// 人话标签（`完整` / `只覆盖主账号`）。
    pub coverage_label: &'static str,
}

/// 本次实际生效的限额（客户看得出「上限是多少、为什么只显示这么多」）。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AnswerLimits {
    pub max_rows: u64,
    pub timeout_ms: u64,
}

/// 一次问题执行的回答。
///
/// ⚠️ 字段清单就是「允许写给客户的东西」的**全集**：没有 `sql_echo`、没有
/// `tables`、没有 `tenant_applied`、没有 `join_states` —— 将来想加也加不进来
/// （要加就得改类型，而改类型会被 `tests/test_req126_portal.rs` 的否定判据看见）。
#[derive(Debug, Clone, Serialize)]
pub struct PortalAnswer {
    pub question: QuestionCard,
    /// 结果列名（= 问题声明里的变量名 / 聚合别名；启动校验保证它们不与列名同名）。
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Option<String>>>,
    pub row_count: usize,
    pub truncated: bool,
    /// 涉及的业务对象（业务名 + 覆盖范围）。
    pub objects: Vec<ObjectInfo>,
    /// 必须显著展示的说明（覆盖范围不完整 / 截断 / 内部标识已隐藏）。
    pub warnings: Vec<String>,
    pub limits: AnswerLimits,
    pub elapsed_ms: u128,
}

/// 投影所需的只读上下文（抽出来是为了让 [`answer_of`] 不连库也能判）。
pub struct AnswerCtx<'a> {
    pub policy: &'a QueryPolicy,
    pub names: &'a BTreeMap<String, String>,
    pub ontology_iri: &'a str,
    pub max_rows: u64,
    pub timeout_ms: u64,
    pub elapsed_ms: u128,
}

/// [`QueryResult`] → [`PortalAnswer`]（**唯一的投影点**；纯函数）。
///
/// 三件事在这里发生：
/// 1. 表 → 业务名（拿不到业务名 ⇒ 给一个中性占位 + 一条告警，⛔ 绝不退回表名）；
/// 2. `main_only` ⇒ 业务语言的覆盖警告（[`questions::coverage_warning`]）；
/// 3. 单元格里出现本体 IRI ⇒ 换成 [`HIDDEN_VALUE`] 并计入一条告警
///    （兜底：启动校验已经拒绝了「投影 IRI」的问题，这条防的是数据里自带的 IRI）。
pub fn answer_of(q: &Question, r: QueryResult, ctx: &AnswerCtx<'_>) -> PortalAnswer {
    let mut warnings: Vec<String> = Vec::new();
    let mut objects: Vec<ObjectInfo> = Vec::new();
    for t in &r.tables {
        let name = ctx
            .names
            .get(t)
            .cloned()
            .unwrap_or_else(|| "未命名的业务对象".to_string());
        if name == "未命名的业务对象" {
            warnings.push(
                "有一类数据没有业务名，只能模糊地显示为「未命名的业务对象」—— 请联系服务方补齐命名"
                    .to_string(),
            );
        }
        let coverage = match ctx.policy.decide(t) {
            TableDecision::Filter { coverage, .. } => coverage.as_str(),
            TableDecision::Public { .. } => "complete",
            TableDecision::Unregistered => "unknown",
        };
        let note = match ctx.policy.decide(t) {
            TableDecision::Filter { coverage_note, reason, .. } => {
                coverage_note.or(reason)
            }
            _ => None,
        };
        if let Some(w) = questions::coverage_warning(&name, coverage, note.as_deref()) {
            warnings.push(w);
        }
        objects.push(ObjectInfo {
            name,
            coverage,
            coverage_label: questions::coverage_label(coverage),
        });
    }
    // 同一张表可能被规划器列多次（多跳 join），去重时保留第一次的覆盖范围。
    objects.dedup_by(|a, b| a.name == b.name);

    if r.truncated {
        warnings.push(format!(
            "结果超过本次上限（{} 行），只显示前 {} 行 —— 表格下方不是全部数据。\
             要更多请让服务方调大你访问口令的行数上限",
            ctx.max_rows, ctx.max_rows
        ));
    }

    let mut hidden = 0usize;
    let rows: Vec<Vec<Option<String>>> = r
        .rows
        .into_iter()
        .map(|line| {
            line.into_iter()
                .map(|cell| match cell {
                    Some(v) if v.contains(ctx.ontology_iri) => {
                        hidden += 1;
                        Some(HIDDEN_VALUE.to_string())
                    }
                    other => other,
                })
                .collect()
        })
        .collect();
    if hidden > 0 {
        warnings.push(format!(
            "有 {hidden} 个返回值是内部标识（IRI），已隐藏 —— 它们不是业务数据"
        ));
    }

    PortalAnswer {
        question: q.card(),
        columns: r.columns,
        row_count: rows.len(),
        rows,
        truncated: r.truncated,
        objects,
        warnings,
        limits: AnswerLimits {
            max_rows: ctx.max_rows,
            timeout_ms: ctx.timeout_ms,
        },
        elapsed_ms: ctx.elapsed_ms,
    }
}

// ══════════════════════════════════════════════════════════════
// 执行（**唯一**入口：`POST /q/<id>` 只调这里）
// ══════════════════════════════════════════════════════════════

/// 执行一个问题：租户来自**口令**，查询来自**声明**，注入来自 **`run_query`**。
///
/// 顺序是刻意的（最便宜的失败先报，且**每个失败都留一条审计**）：
/// ① 每口令并发名额 → ② 规划 + 注入 + 只读执行 → ③ 投影成回答。
///
/// 失败时返回的 [`crate::query::error::ApiError`] 是**固定话术**（⛔ 不含 SQL /
/// 表名 / 列名）；真原因只进 stderr 与审计的 `code`。
pub async fn run_question(
    svc: &PortalService,
    entry: &PortalTokenEntry,
    q: &Question,
) -> Result<PortalAnswer, crate::query::error::ApiError> {
    let started = Instant::now();

    // ① 并发名额：**拿不到就是 429，不排队**（等 10 秒再告诉你「太忙」比直接说更糟）。
    //    名额按口令隔离 —— A 打满不影响 B。
    let Some(permit) = svc.limiter.acquire(&entry.digest, entry.max_concurrency) else {
        let elapsed = started.elapsed();
        svc.audit
            .record(entry, q.id(), None, elapsed, "TOO_MANY_CONCURRENT");
        return Err(too_many_concurrent(entry.max_concurrency));
    };

    let input = PlanInput {
        mapping: &svc.mapping,
        schema: &svc.schema,
        ontology_iri: &svc.ontology_iri,
        policy: &svc.policy,
        // ⛔ 唯一的租户来源：口令里那一行。
        tenant: &entry.tenant,
        max_rows: entry.max_rows,
    };
    // ⭐ REQ-122 的 `run_query`：同一套解析 / 规划 / 注入 / 只读执行 / 探针截断。
    let out = crate::sparql::run_query(&svc.source, &input, &q.sparql, svc.timeout_ms).await;
    let elapsed: Duration = started.elapsed();
    // `permit` 在这里显式归还：名额只覆盖「查询在跑」的那一段（投影与审计不占名额）。
    drop(permit);

    match out {
        Ok(r) => {
            let ctx = AnswerCtx {
                policy: &svc.policy,
                names: &svc.names,
                ontology_iri: &svc.ontology_iri,
                max_rows: entry.max_rows,
                timeout_ms: svc.timeout_ms,
                elapsed_ms: elapsed.as_millis(),
            };
            let answer = answer_of(q, r, &ctx);
            svc.audit
                .record(entry, q.id(), Some(answer.row_count), elapsed, "OK");
            Ok(answer)
        }
        Err(e) => {
            let (err, code) = portal_error(&e);
            // ⚠️ 真原因只进 stderr（含 SQL / 表名）：客户侧一个字节都不给。
            eprintln!(
                "✗ portal 问题 {} 执行失败（内部诊断，不发给客户）：{}",
                q.id(),
                e.message()
            );
            svc.audit.record(entry, q.id(), None, elapsed, code);
            Err(err)
        }
    }
}

/// 同一口令的并发名额用完了 ⇒ **429**（唯一一处构造）。
///
/// ⛔ 文案里没有表名 / 列名 / SQL / IRI —— 客户只需要知道「太忙，等一下」。
/// ⛔ 不排队：等服务**不会**把这次请求挂起再重试（那会把一次点击变成一次
/// `acquire_timeout`，用户看到的是「转了 10 秒才说忙」）。
fn too_many_concurrent(limit: usize) -> crate::query::error::ApiError {
    crate::query::error::ApiError::with_code(
        StatusCode::TOO_MANY_REQUESTS,
        "TOO_MANY_CONCURRENT",
        "同时在进行的问题太多了，这一次没有执行。",
        format!(
            "同一个访问口令同时最多 {limit} 个问题在跑。等前面的结果出来再点一次（串行最稳）；\
             要更高的并发请让服务方调大你口令的并发上限。"
        ),
    )
}

/// 运行期失败的**对外翻译**（唯一一处）：只分「超时」与「其它」。
///
/// ⛔ 不把 planner / PG 的原话给客户：那些话里带着表名、列名与 SQL。
fn portal_error(e: &crate::sparql::QueryError) -> (crate::query::error::ApiError, &'static str) {
    use crate::query::error::ApiError;
    let msg = e.message();
    if msg.contains("查询超时") || msg.contains("statement timeout") {
        (
            ApiError::with_code(
                StatusCode::GATEWAY_TIMEOUT,
                "TIMEOUT",
                "这个问题这次查询超时了，没能取到结果。",
                "稍后重试一次；如果一直超时，请把时间与问题名称报给服务方。\
                 缩小范围的办法只能由服务方调整问题口径（门户不接受自定义查询）。",
            ),
            "TIMEOUT",
        )
    } else {
        (
            ApiError::with_code(
                StatusCode::INTERNAL_SERVER_ERROR,
                "QUESTION_FAILED",
                "这个问题这次没能取到数据。",
                "稍后重试；若持续出现，请把时间与问题名称报给服务方（技术细节已记录在服务端，\
                 不会显示在这里）。",
            ),
            "QUESTION_FAILED",
        )
    }
}

pub use serve::ServeArgs;

// ══════════════════════════════════════════════════════════════
// 判据（不连库：`answer_of` 是纯函数）
// ══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    const ONT: &str = "https://ontok.local/demo";

    const POLICY: &str = r#"{
      "version": "1.0.0",
      "tenantSubject": "customers",
      "tenantIdColumn": "id",
      "perTable": {
        "orders": {"tenantColumn": "customer_id", "coverage": "main_only",
                   "coverageNote": "main_customer_id 全空（0/4）；子账号产生的订单不在结果内",
                   "reason": "订单归客户"}
      },
      "publicDimensions": [{"table": "package_templates", "reason": "套餐目录"}]
    }"#;

    fn names() -> BTreeMap<String, String> {
        let mut m = BTreeMap::new();
        m.insert("customers".to_string(), "客户".to_string());
        m.insert("orders".to_string(), "订单".to_string());
        m.insert("package_templates".to_string(), "套餐模板".to_string());
        m
    }

    fn question() -> Question {
        questions::test_question()
    }

    fn ctx<'a>(policy: &'a QueryPolicy, names: &'a BTreeMap<String, String>) -> AnswerCtx<'a> {
        AnswerCtx {
            policy,
            names,
            ontology_iri: ONT,
            max_rows: 200,
            timeout_ms: 10_000,
            elapsed_ms: 37,
        }
    }

    fn result(tables: &[&str], rows: Vec<Vec<Option<String>>>, truncated: bool) -> QueryResult {
        QueryResult {
            columns: vec!["amt".into()],
            row_count: rows.len(),
            rows,
            truncated,
            sql_echo: "SELECT 不要出现在回答里".into(),
            tenant_applied: vec![],
            tables: tables.iter().map(|t| t.to_string()).collect(),
            public_dimensions: vec![],
            warnings: vec!["表 orders：租户过滤用 customer_id".into()],
        }
    }

    /// ⭐ AC-004：结果里只有业务名、覆盖警告、行数；⛔ 没有 sql_echo / 表名 / 列名 / IRI。
    #[test]
    fn the_answer_never_carries_sql_tables_or_iris() {
        let q = question();
        let policy = QueryPolicy::parse(POLICY).unwrap();
        let names = names();
        let r = result(
            &["orders"],
            vec![vec![Some("12.3400".into())]],
            false,
        );
        let ans = answer_of(&q, r, &ctx(&policy, &names));
        let json = serde_json::to_string(&ans).unwrap();

        for forbidden in ["sql_echo", "SELECT", " FROM ", "tenant_applied", "https://"] {
            assert!(!json.contains(forbidden), "回答里出现了 {forbidden:?}：{json}");
        }
        for table in ["orders", "customers", "package_templates"] {
            assert!(!json.contains(table), "回答里出现了表名 {table:?}：{json}");
        }
        for col in ["customer_id", "main_customer_id", "amount"] {
            assert!(!json.contains(col), "回答里出现了列名 {col:?}：{json}");
        }
        // 正向：业务名 + 覆盖警告都在。
        assert!(ans.objects.iter().any(|o| o.name == "订单"), "{:?}", ans.objects);
        let warn = ans.warnings.join("\n");
        assert!(warn.contains("订单"), "覆盖警告必须点名业务对象：{warn}");
        assert!(warn.contains("只覆盖主账号"), "{warn}");
        assert!(!warn.contains("main_customer_id"), "列名漏进警告了：{warn}");
        assert_eq!(ans.row_count, 1);
        assert_eq!(ans.limits.max_rows, 200);
        // 规划器自带的 warning（含表名）**不转发**。
        assert!(!warn.contains("租户过滤用"), "{warn}");
    }

    /// 公共维度是 `complete`，不报警；`truncated` 必须报警。
    #[test]
    fn truncation_and_public_dimensions_are_reported_honestly() {
        let q = question();
        let policy = QueryPolicy::parse(POLICY).unwrap();
        let names = names();
        let ans = answer_of(
            &q,
            result(&["package_templates"], vec![], true),
            &ctx(&policy, &names),
        );
        assert!(ans.truncated);
        let warn = ans.warnings.join("\n");
        assert!(warn.contains("只显示前 200 行"), "{warn}");
        assert!(ans.objects.iter().all(|o| o.coverage == "complete"), "{:?}", ans.objects);
        assert!(
            !warn.contains("只覆盖主账号"),
            "complete 的表不该报 main_only 警告：{warn}"
        );
    }

    /// 兜底：数据里自带本体 IRI 的单元格被替换，并计入告警。
    #[test]
    fn a_cell_carrying_an_iri_is_hidden_with_a_visible_warning() {
        let q = question();
        let policy = QueryPolicy::parse(POLICY).unwrap();
        let names = names();
        let leaky = format!("{ONT}#orders/4eea0ba0");
        let ans = answer_of(
            &q,
            result(&["orders"], vec![vec![Some(leaky.clone())]], false),
            &ctx(&policy, &names),
        );
        let json = serde_json::to_string(&ans).unwrap();
        assert!(!json.contains(&leaky), "IRI 漏进回答：{json}");
        assert!(json.contains(HIDDEN_VALUE), "{json}");
        assert!(
            ans.warnings.iter().any(|w| w.contains("内部标识")),
            "{:?}",
            ans.warnings
        );
    }

    /// 没有业务名的表 ⇒ 中性占位 + 告警，⛔ 退回表名。
    #[test]
    fn a_table_without_a_business_name_falls_back_to_a_neutral_label() {
        let q = question();
        let policy = QueryPolicy::parse(POLICY).unwrap();
        let empty = BTreeMap::new();
        let ans = answer_of(&q, result(&["orders"], vec![], false), &ctx(&policy, &empty));
        let json = serde_json::to_string(&ans).unwrap();
        assert!(!json.contains("orders"), "回答里出现了表名：{json}");
        assert!(json.contains("未命名的业务对象"), "{json}");
        assert!(ans.warnings.iter().any(|w| w.contains("没有业务名")), "{:?}", ans.warnings);
    }

    /// ⭐ AC-008（服务端那一半）：对象名里带 `<img onerror>` ⇒ **原样**进 JSON，
    /// 不转义、不裁剪。浏览器侧靠 `textContent`（见 `portal::page` 的单测：
    /// `innerHTML` 0 处），所以「原样」才是正确的 —— 转义会让名字变成乱码，
    /// 而真正的防线是「永远当文本写」。
    #[test]
    fn a_hostile_object_name_is_carried_verbatim_and_never_becomes_html() {
        let q = question();
        let policy = QueryPolicy::parse(POLICY).unwrap();
        let mut names = BTreeMap::new();
        let hostile = "<img src=x onerror=alert(1)>";
        names.insert("orders".to_string(), hostile.to_string());
        let ans = answer_of(&q, result(&["orders"], vec![], false), &ctx(&policy, &names));
        assert_eq!(ans.objects[0].name, hostile, "对象名必须原样带出");
        let json = serde_json::to_string(&ans).unwrap();
        assert!(json.contains(hostile), "JSON 里的名字被改写了：{json}");
        // 页面侧永不把数据当 HTML：这条在 `portal::page` 的判据里（innerHTML = 0）。
        assert!(
            !crate::portal::page::PORTAL_HTML.contains("innerHTML"),
            "页面侧不许用 innerHTML"
        );
    }
}

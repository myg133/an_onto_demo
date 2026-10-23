//! REQ-126 · **门户的启动与绑定闸门** —— 默认只给内网/本机，公网地址要 `--allow-public`。
//!
//! # 启动顺序（每条失败都在**绑 socket 之前**报出来）
//!
//! | # | 步骤 | 为什么在这个位置 |
//! | --- | --- | --- |
//! | ① | 参数闸（`--timeout-ms`） | 最便宜的失败先报 |
//! | ② | 绑定地址 + 闸门（复用 `query::serve` 的判定） | 绑错了根本不该读文件 |
//! | ③ | `portal-tokens.json`（`kind = "portal"`，租户绑在口令上） | 没有凭据表就没有身份 |
//! | ④ | `query-policy.json` | 没有声明 ⇒ 所有表不可查（ADR-006） |
//! | ⑤ | `--schema` | 列存在性 / 「变量名不许与列名同名」只能来自真实 schema |
//! | ⑥ | 元数据库：项目 / 版本 / 快照（**必须已冻结**，与 `query serve` 同一条） | 客户只该看到定稿版本 |
//! | ⑦ | **映射**（`r2rml::build_mapping`，与 `r2rml generate` 同一个函数） | ⛔ 不解析 TTL |
//! | ⑧ | **问题声明**逐题校验（fail-closed） | ⛔ 一条不合格就不启动（AC-003） |
//! | ⑨ | **源库只读**池（`connect_read_only`，读回两个会话值） | 客户资产：只读不成立就一条都不查 |
//! | ⑩ | 审计落点 → 横幅 → 绑 socket | 横幅打印绑定地址 / 项目版本 / 凭据数 / 问题数 |
//!
//! ⑧ 排在 ⑨ 之前是刻意的：**问题声明是纯本地的**，一份写错的口径不该让服务先去
//! 连一次客户库（连接本身也是成本，而且失败信息会盖住真正的原因）。
//!
//! # 绑定闸门
//!
//! 与 `query serve` 逐字同一份（[`crate::query::serve::check_bind`]）：回环 / 内网放行；
//! 通配或公网地址必须 `--allow-public`，且横幅打警告。
//! 为什么门户不学控制面「只给回环」：客户门户**本来就要给客户访问**，
//! 挡在回环上等于「只能在本机看自己的数据」—— 真正的边界是网络策略 + 口令。
//!
//! # 已知限制
//!
//! - 绑定闸门判的是**地址**，不是路由表：多网卡 / NAT / 端口转发仍可能把回环以外的
//!   东西暴露出去；
//! - 没有 TLS：内网明文 HTTP。给公网客户用必须由反向代理加 TLS；
//! - 横幅**不打印租户 id**（打印个数）：门户的受众是客户，运维要的「这台服务能查到谁」
//!   去看 `portal-tokens.json` —— 一行 grep 的事，不值得在日志里留一串客户 id。

use std::net::SocketAddr;
use std::path::PathBuf;

use sqlx::postgres::PgPool;

use crate::query::audit::AuditSink;
use crate::query::serve::{check_bind, is_internal_ip, parse_bind, BindVerdict};
use crate::sparql::policy::QueryPolicy;

use super::audit::AuditLog;
use super::questions::{self, QuestionSet};
use super::tokens::PortalTokens;
use super::{PortalParts, PortalService};

/// `ontok portal serve` 的参数（与 clap 分开：本模块不认识 clap）。
#[derive(Debug, Clone)]
pub struct ServeArgs {
    pub bind: String,
    pub tokens: PathBuf,
    pub project: String,
    pub version: i32,
    pub schema: PathBuf,
    pub policy: PathBuf,
    pub questions: PathBuf,
    /// 绑非内网地址时必须显式给（`0.0.0.0` 也要）。
    pub allow_public: bool,
    pub audit_out: Option<PathBuf>,
    pub timeout_ms: u64,
    /// IRI 基（必须与 `r2rml generate` / `query serve` 用的是同一个）。
    pub base: String,
    /// 把 `joinVerified "inherit"` 的边也生成 join（默认不生成，与 CLI 一致）。
    pub include_inherit: bool,
}

// ══════════════════════════════════════════════════════════════
// 载入（`serve` 与真库判据共用**这一个**函数）
// ══════════════════════════════════════════════════════════════

/// 载入 → 校验 → 一个可以直接起路由的服务状态。
///
/// ⛔ 这里不做任何 HTTP 判断（鉴权 / 限额在 [`super::http`] 与 [`super::run_question`]）；
/// 也**不绑 socket** —— 绑定的地方只有 [`serve`]（判据可以因此不占端口）。
pub async fn load(pool: &PgPool, args: &ServeArgs) -> anyhow::Result<PortalService> {
    // ── ① 参数闸 ────────────────────────────────────────────
    if args.timeout_ms == 0 {
        anyhow::bail!("--timeout-ms 是 0：等于「不许它跑」，那是禁用不是超时。要跑就给 ≥1。");
    }

    // ── ③ 凭据（租户绑在口令上）──────────────────────────────
    let raw = std::fs::read_to_string(&args.tokens).map_err(|e| {
        anyhow::anyhow!(
            "读不出门户凭据表 {:?}：{e}\n  格式：{{\"version\":\"1.0.0\",\"tokens\":[\
             {{\"token\":\"…\",\"kind\":\"portal\",\"tenant\":\"…\",\"max_rows\":200}}]}}\n  \
             示例见 `portal-tokens.example.json`（里面的口令是假值）。\n  \
             ⚠️ `kind` 必须是 \"portal\"：admin / query 凭据**不能**当门户口令用。",
            args.tokens
        )
    })?;
    let tokens = PortalTokens::parse(&raw)
        .map_err(|e| anyhow::anyhow!("门户凭据表 {:?} 不合法：{e}", args.tokens))?;

    // ── ④ 租户声明 ──────────────────────────────────────────
    let raw = std::fs::read_to_string(&args.policy)
        .map_err(|e| anyhow::anyhow!("读不出租户声明 {:?}：{e}", args.policy))?;
    let policy = QueryPolicy::parse(&raw)
        .map_err(|e| anyhow::anyhow!("租户声明 {:?} 不合法：{e}", args.policy))?;

    // ── ⑤ schema（默认值允许向上一级回落：工作树里没有 `code/`）──
    let schema_path = resolve_schema(&args.schema);
    let raw = std::fs::read_to_string(&schema_path).map_err(|e| {
        anyhow::anyhow!(
            "读不出 schema 文件 {:?}：{e}\n  门户用它判「问题里的变量名不与真实列名同名」；\
             ⛔ 没有 schema 就没有这条校验 —— 拒绝启动。\n  \
             写法：`--schema code/out/schema.json`（`ontok-introspect` 的产物）。",
            args.schema
        )
    })?;
    let schema = crate::domain::ttl::SchemaTypes::parse(&raw)
        .map_err(|e| anyhow::anyhow!("schema 文件 {:?} 不合法：{e}", schema_path))?;
    if schema.is_empty() {
        anyhow::bail!(
            "--schema {:?} 里一列都没读到 —— 这份 schema 无法校验（⛔ 不猜列）",
            schema_path
        );
    }

    // ── ⑥ 项目 / 版本 / 快照（必须已冻结）────────────────────
    let projects = crate::db::project::PgProjectStore::new(pool.clone());
    let store = crate::db::snapshot_store::PgStore::new(pool.clone());
    let p = projects
        .get_by_name(&args.project)
        .await?
        .ok_or_else(|| anyhow::anyhow!("项目 {:?} 不存在", args.project))?;
    let v = projects
        .version_by_seq(p.id, &p.name, args.version)
        .await
        .map_err(|e| anyhow::anyhow!("项目 {:?} 没有第 {} 版：{e}", args.project, args.version))?;
    if v.frozen_at.is_none() {
        anyhow::bail!(
            "项目 {:?} 的第 {} 版**尚未冻结**，拒绝对外服务 —— \
             门户只服务定稿版本（与 `serve` / `mcp-server` / `query serve` 同一条闸门）。\n  \
             先执行：`ontok version freeze --project {} --version {}`",
            args.project,
            args.version,
            args.project,
            args.version
        );
    }
    let snap = store
        .get_snapshot(v.id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("项目 {:?} 第 {} 版没有快照", args.project, args.version))?;

    // ── ⑦ 映射（与 r2rml generate 同一个函数）────────────────
    let opts = crate::domain::ttl::TtlOptions {
        project: args.project.as_str(),
        version: args.version,
        base: args.base.as_str(),
        action_type_count: snap.action_types.len(),
        tenancy: None,
        schema: Some(&schema),
    };
    let model = crate::domain::model_view::project_model_track(&snap);
    let mapping = crate::domain::r2rml::build_mapping(
        &model,
        &opts,
        args.include_inherit,
        crate::domain::r2rml::R2rmlAudit::default(),
    )
    .map_err(|e| {
        anyhow::anyhow!(
            "按 R2RML 生成 SQL 的前置对账失败（映射与 schema 不一致，⛔ 一个字节都不产出）：\n{e}"
        )
    })?;
    let ontology_iri = opts.ontology_iri();

    // ── ⑧ 问题声明（逐题 fail-closed）─────────────────────────
    let raw = std::fs::read_to_string(&args.questions).map_err(|e| {
        anyhow::anyhow!(
            "读不出问题声明 {:?}：{e}\n  门户**只能**执行这份文件里列出的问题（⛔ 没有自由查询）。\
             示例见 `portal-questions.json`；`reviewed_by` 必须有人签（生产环境必须真签）。",
            args.questions
        )
    })?;
    let names = questions::business_names(&model);
    let set: QuestionSet = questions::validate(&raw, &mapping, &schema, &policy, &names, &ontology_iri)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    // ── ⑨ 源库（只读；读回校验）──────────────────────────────
    let url = crate::config::require_env(crate::extract::introspect::SOURCE_DATABASE_URL_ENV)
        .map_err(|e| anyhow::anyhow!("{e}（门户查的是源库，不是元数据库）"))?;
    let src = crate::sparql::exec::connect_read_only(&url)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    // ── ⑩ 审计落点 ─────────────────────────────────────────
    let sink = match &args.audit_out {
        Some(path) => AuditSink::open(path).map_err(|e| {
            anyhow::anyhow!(
                "打不开审计文件 {path:?}：{e} —— ⛔ 不静默降级成「只有 stderr」，\
                 请换一个可写的路径或去掉 --audit-out"
            )
        })?,
        None => AuditSink::stderr_only(),
    };

    Ok(PortalService::new(PortalParts {
        source: src.pool,
        facts: src.facts,
        mapping,
        schema,
        policy,
        ontology_iri,
        project: args.project.clone(),
        version: args.version,
        timeout_ms: args.timeout_ms,
        tokens,
        questions: set,
        names,
        audit: AuditLog::new(sink),
    }))
}

/// schema 的落点：默认值 + 相对路径这一种情况下，允许向上一级找。
///
/// 与 `console serve` 同一条规则（工作树里没有 `code/`，从工作树里敲文档那条命令
/// 会在上一级找到它）。⛔ 不猜任意路径：只对**默认值**做回落。
fn resolve_schema(given: &std::path::Path) -> PathBuf {
    if given.is_file() {
        return given.to_path_buf();
    }
    let sibling = std::path::Path::new("..").join(given);
    if given == std::path::Path::new(super::DEFAULT_SCHEMA) && sibling.is_file() {
        eprintln!(
            "· schema 不在当前目录，回落到工作树的上一级：{}",
            sibling.display()
        );
        return sibling;
    }
    given.to_path_buf()
}

// ══════════════════════════════════════════════════════════════
// 横幅（**纯函数**：判据在单测里逐项对）
// ══════════════════════════════════════════════════════════════

/// 启动横幅。
///
/// 必须含：**绑定地址**、**项目 / 版本**、**凭据数 / 租户数**、**问题数**
/// （已逐题签过）、**只读会话读回的事实**、四个端点、绑公网时的**警告**。
pub fn banner(addr: SocketAddr, verdict: BindVerdict, svc: &PortalService) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "· ontok portal serve 就绪：绑定 {addr}（{}） · 项目 {} 第 {} 版\n",
        verdict.label(),
        svc.project,
        svc.version
    ));
    s.push_str(&format!(
        "· 凭据：门户口令 {} 张 / 租户 {} 个 · 每次最多 {} 行 · 超时 {} ms\n",
        svc.tokens.len(),
        svc.tokens.tenant_count(),
        svc.tokens.max_rows_cap(),
        svc.timeout_ms
    ));
    s.push_str(&format!(
        "· 问题：{} 条（逐题校验通过 · 签名 \"{}\"）\n",
        svc.questions.len(),
        svc.questions.reviewed_by()
    ));
    // ⛔ 这两个值来自连接上**读回**的事实，不是字面量（REQ-122 · N3）。
    s.push_str(&format!(
        "· 只读会话（读回）：transaction_read_only = {} · standard_conforming_strings = {}\n",
        svc.facts.transaction_read_only, svc.facts.standard_conforming_strings
    ));
    s.push_str(&format!(
        "· 端点：GET {PAGE}（页面壳，零数据）· GET {Q}（问题列表）· POST /q/<id>（执行）· \
         GET {H}（免 token）\n",
        PAGE = super::http::PAGE_PATH,
        Q = super::http::QUESTIONS_PATH,
        H = super::http::HEALTH_PATH
    ));
    s.push_str(
        "· 隔离：租户**只**来自口令（请求体/URL 带 tenant ⇒ 400/403）· ⛔ 没有 /api/sparql · \
         每次问题执行一行审计（无口令明文、无 SQL）\n",
    );
    if verdict.needs_warning() {
        s.push_str(&format!(
            "⚠ 警告：绑定的是 {addr}（{}）—— 门户可被本机以外访问。\
             拿到客户口令的人可以取到该客户的数据；请确认防火墙 / 反向代理 / TLS 边界。\n",
            verdict.label()
        ));
    }
    s
}

// ══════════════════════════════════════════════════════════════
// 起服务
// ══════════════════════════════════════════════════════════════

/// 载入 → 起服务。
pub async fn serve(pool: &PgPool, args: ServeArgs) -> anyhow::Result<()> {
    // ── ② 绑定 + 闸门（先于一切文件读取）──────────────────────
    let addr = parse_bind(&args.bind).map_err(|e| anyhow::anyhow!("{e}"))?;
    let verdict = check_bind(addr, args.allow_public).map_err(|e| anyhow::anyhow!("{e}"))?;

    let svc = load(pool, &args).await?;
    eprintln!("{}", banner(addr, verdict, &svc));

    let app = super::http::build_router(std::sync::Arc::new(svc));
    let listener = tokio::net::TcpListener::bind(addr).await.map_err(|e| {
        anyhow::anyhow!("绑不上 {addr}：{e}（端口被占 / 地址不属于本机？换一个 --bind 再试）")
    })?;
    axum::serve(listener, app)
        .await
        .map_err(|e| anyhow::anyhow!("门户服务退出：{e}"))
}

/// 判定「这个地址是不是内网」的**唯一**入口再导出（横幅与外部的排障都走它）。
///
/// 存在的理由：防止有人在门户里另写一份 `is_private()` —— 两份判定迟早会漂，
/// 而漂的那一次是「以为绑的是内网，其实绑在公网上」。
pub fn address_is_internal(ip: std::net::IpAddr) -> bool {
    is_internal_ip(ip)
}

// ══════════════════════════════════════════════════════════════
// 判据（纯函数：不连库、不绑 socket）
// ══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bind_gate_matches_the_query_service() {
        // 内网 / 回环：放行且不警告。
        for ok in ["127.0.0.1:8900", "10.0.0.5:8900", "192.168.1.9:8900", "172.16.3.4:8900"] {
            let addr = parse_bind(ok).expect(ok);
            let v = check_bind(addr, false).unwrap_or_else(|e| panic!("{ok} 应当放行：{e}"));
            assert!(v.internal, "{ok}");
            assert!(!v.needs_warning(), "{ok} 是内网/本机，不该警告");
        }
        // 通配 / 公网：没有 flag ⇒ 拒绝；有 flag ⇒ 放行且**必须**警告。
        for bad in ["0.0.0.0:8900", "[::]:8900", "8.8.8.8:8900"] {
            let addr = parse_bind(bad).expect(bad);
            let e = check_bind(addr, false).expect_err(&format!("{bad} 不该放行"));
            assert!(e.contains("--allow-public"), "拒绝时要说清怎么放行：{e}");
            let v = check_bind(addr, true).expect("带 flag 应当放行");
            assert!(v.needs_warning(), "{bad} 绑定必须警告");
        }
        assert!(address_is_internal("127.0.0.1".parse().unwrap()));
        assert!(!address_is_internal("8.8.8.8".parse().unwrap()));
    }

    #[test]
    fn a_bad_bind_string_gets_a_human_error() {
        let e = parse_bind("127.0.0.1").unwrap_err();
        assert!(e.contains(":8800") || e.contains("127.0.0.1:8900") || e.contains("写成"), "{e}");
    }
}

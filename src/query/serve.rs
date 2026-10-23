//! REQ-123 · **启动与绑定闸门** —— 默认只给内网/本机；`0.0.0.0` 要显式 `--allow-public`。
//!
//! # 启动顺序（每一条失败都要**在绑 socket 之前**报出来）
//!
//! | # | 步骤 | 为什么在这个位置 |
//! | --- | --- | --- |
//! | ① | 参数闸（超时 / SSE / 并发） | 最便宜的失败先报 |
//! | ② | 绑定地址 + 闸门 | 绑错了根本不该读库 |
//! | ③ | `tokens.json` | 没有 token 表就没有租户 |
//! | ④ | `query-policy.json` | 没有声明 ⇒ 所有表不可查（ADR-006） |
//! | ⑤ | `schema.json` | 列存在性 / 类型只能来自真实 schema |
//! | ⑥ | 元数据库：项目 / 版本 / 快照（**必须已冻结**，与 `serve` / `mcp-server` 同一条） | 对外服务只服务定稿 |
//! | ⑦ | **映射**（`r2rml::build_mapping`，与 `ontok r2rml generate` 同一个函数） | ⛔ 不解析 TTL |
//! | ⑧ | **源库只读**池（`connect_read_only`，读回两个会话值） | 客户资产：只读不成立就一条都不查 |
//! | ⑨ | 启动横幅 → 绑 socket | 横幅里打印绑定地址 / 项目版本 / token 数 / 租户范围 |
//!
//! # 横幅（AC-008）
//!
//! 必须含：**绑定地址**、**项目 / 版本**、**token 数量**、**租户数量（与 id 列表）**，
//! 绑公网时**警告**。⚠️ 横幅走 **stderr**：stdout 留给将来的管道用法，且与 `serve` 一致。
//!
//! # 已知限制
//!
//! - 绑定闸门判的是**地址**，不是路由表：一台机器有多张网卡时，「内网地址」仍可能被
//!   NAT/端口转发暴露。真正的边界是网络策略 + token 表。
//! - 没有 TLS：内网明文 HTTP（用户已定：只给内网/本机）。放在公网必须由反向代理加 TLS。

use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;

use sqlx::postgres::PgPool;

use crate::sparql::policy::QueryPolicy;

use super::audit::AuditSink;
use super::tokens::TokenTable;
use super::{QueryService, ServiceParts};

/// `ontok query serve` 的参数（与 clap 的定义分开：本模块不认识 clap）。
#[derive(Debug, Clone)]
pub struct ServeArgs {
    pub bind: String,
    pub tokens: PathBuf,
    pub project: String,
    pub version: i32,
    pub schema: PathBuf,
    pub policy: PathBuf,
    /// 绑非内网地址时必须显式给（`0.0.0.0` 也要）。
    pub allow_public: bool,
    pub timeout_ms: u64,
    pub sse_idle_timeout_ms: u64,
    pub sse_max_connections: usize,
    pub max_concurrent_queries: usize,
    pub audit_out: Option<PathBuf>,
    /// IRI 基（必须与 `ontok r2rml generate` 用的是同一个）。
    pub base: String,
    /// 把 `joinVerified "inherit"` 的边也生成 join（默认不生成，与 CLI 一致）。
    pub include_inherit: bool,
}

/// 绑定地址的判定结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BindVerdict {
    /// 回环（127.0.0.1 / ::1）。
    pub loopback: bool,
    /// 内网 / 本机（回环 + RFC1918 + IPv6 ULA / link-local）。
    pub internal: bool,
    /// 通配地址（`0.0.0.0` / `::`）—— 所有网卡。
    pub unspecified: bool,
}

impl BindVerdict {
    /// 横幅里那个人读的标签。
    pub fn label(self) -> &'static str {
        if self.loopback {
            "仅本机"
        } else if self.unspecified {
            "所有网卡"
        } else if self.internal {
            "内网"
        } else {
            "公网地址"
        }
    }

    /// 需要打警告吗（不是「回环/内网」就要）。
    pub fn needs_warning(self) -> bool {
        !self.internal
    }
}

/// 解析 `--bind`。给错格式要报人话（⛔ 不把 socket 的栈倒给用户）。
pub fn parse_bind(raw: &str) -> Result<SocketAddr, String> {
    raw.trim().parse::<SocketAddr>().map_err(|e| {
        format!(
            "监听地址看不懂：{raw:?}（{e}）。要写成 `10.0.0.5:8800` / `127.0.0.1:8800` / `[::1]:8800` 这样的样子。"
        )
    })
}

/// 一个 IP 是不是「内网 / 本机」。
pub fn is_internal_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
        IpAddr::V6(v6) => {
            let seg = v6.segments()[0];
            let ula = (seg & 0xfe00) == 0xfc00; // fc00::/7
            let link_local = (seg & 0xffc0) == 0xfe80; // fe80::/10
            v6.is_loopback() || (!v6.is_unspecified() && (ula || link_local))
        }
    }
}

/// 绑定闸门（**纯函数**，可单测）。
///
/// - 回环 / 内网 ⇒ 放行；
/// - 通配（`0.0.0.0` / `::`）或公网地址 ⇒ **必须** `--allow-public`，否则拒绝启动。
pub fn check_bind(addr: SocketAddr, allow_public: bool) -> Result<BindVerdict, String> {
    let ip = addr.ip();
    let verdict = BindVerdict {
        loopback: ip.is_loopback(),
        internal: is_internal_ip(ip),
        unspecified: ip.is_unspecified(),
    };
    if verdict.internal || allow_public {
        return Ok(verdict);
    }
    Err(format!(
        "拒绝绑定 {addr}：它既不是本机（127.0.0.1 / ::1）也不是内网地址。\n  \
         本服务默认只给内网/本机（用户已定）；要绑通配地址（0.0.0.0 / ::）或公网地址，\
         必须显式加 `--allow-public`，并且自己负责网络边界（防火墙 / 反向代理 / TLS）。\n  \
         内网写法示例：`--bind 10.0.0.5:8800`；只给本机：`--bind 127.0.0.1:8800`。"
    ))
}

/// 启动横幅（**纯函数**：判据在单测里逐项对）。
///
/// ⚠️ 租户 id 会**打印出来**（需求风险 5：「横幅打印绑定地址与租户范围」）。
/// 它不是秘密：知道「这台服务能查到谁」是运维判断暴露面的前提。
pub fn banner(
    addr: SocketAddr,
    verdict: BindVerdict,
    project: &str,
    version: i32,
    token_count: usize,
    tenants: &[&str],
    max_rows_cap: u64,
    timeout_ms: u64,
    max_concurrent_queries: usize,
    facts: &crate::sparql::exec::SessionFacts,
) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "· ontok query serve 就绪：绑定 {addr}（{}） · 项目 {project} 第 {version} 版 · \
         token {token_count} 个 · 租户 {} 个（{}）\n",
        verdict.label(),
        tenants.len(),
        if tenants.is_empty() {
            "无".to_string()
        } else {
            tenants.join("、")
        }
    ));
    s.push_str(&format!(
        "· SPARQL 语法 v1 · 超时 {timeout_ms} ms · 单次最多 {max_rows_cap} 行（按 token）· \
         全局并发 {max_concurrent_queries}\n"
    ));
    // ⛔ 这两个值来自连接上**读回**的事实，不是字面量（REQ-122 · N3）。
    s.push_str(&format!(
        "· 只读会话（读回）：transaction_read_only = {} · standard_conforming_strings = {}\n",
        facts.transaction_read_only, facts.standard_conforming_strings
    ));
    s.push_str("· 审计：每次调用一行（token 只记摘要、不记查询原文）\n");
    if verdict.needs_warning() {
        s.push_str(&format!(
            "⚠ 警告：绑定的是 {addr}（{}）—— 查询服务可被本机以外访问。\
             token 一旦泄露即可取数；请确认防火墙 / 反向代理 / TLS 边界。\n",
            verdict.label()
        ));
    }
    s
}

/// 载入 → 起服务。
pub async fn serve(pool: &PgPool, args: ServeArgs) -> anyhow::Result<()> {
    // ── ① 参数闸 ────────────────────────────────────────────
    if args.timeout_ms == 0 {
        anyhow::bail!("--timeout-ms 是 0：等于「不许它跑」，那是禁用不是超时。要跑就给 ≥1。");
    }
    if args.sse_idle_timeout_ms < 1_000 {
        anyhow::bail!(
            "--sse-idle-timeout-ms 是 {}：小于 1000 ms 的 SSE 流没有使用价值（连接刚建立就会被收尾）。",
            args.sse_idle_timeout_ms
        );
    }
    if args.sse_max_connections == 0 {
        anyhow::bail!("--sse-max-connections 是 0：那等于禁用 GET /mcp。要给 ≥1。");
    }
    if args.max_concurrent_queries == 0 {
        anyhow::bail!("--max-concurrent-queries 是 0：那等于禁用查询。要给 ≥1。");
    }
    if args.max_concurrent_queries > 2 {
        // 源库只读池是 `read_only_pool` 的 2 条连接（REQ-122 起的规定）。
        eprintln!(
            "⚠ --max-concurrent-queries={} 超过源库只读池的连接数 2：\
             多出来的查询会在池上排队（不会出错，但延迟会上去）。",
            args.max_concurrent_queries
        );
    }

    // ── ② 绑定 + 闸门 ───────────────────────────────────────
    let addr = parse_bind(&args.bind).map_err(|e| anyhow::anyhow!("{e}"))?;
    let verdict = check_bind(addr, args.allow_public).map_err(|e| anyhow::anyhow!("{e}"))?;

    // ── ③ token 表 ──────────────────────────────────────────
    let raw = std::fs::read_to_string(&args.tokens).map_err(|e| {
        anyhow::anyhow!(
            "读不出 token 表 {:?}：{e}\n  格式见 `query serve --help`（示例里的 token 是假值，\
             真实 token 请由运维随机生成，≥ 24 字符）。",
            args.tokens
        )
    })?;
    let tokens = TokenTable::parse(&raw)
        .map_err(|e| anyhow::anyhow!("token 表 {:?} 不合法：{e}", args.tokens))?;

    // ── ④ 声明 ──────────────────────────────────────────────
    let raw = std::fs::read_to_string(&args.policy)
        .map_err(|e| anyhow::anyhow!("读不出租户声明 {:?}：{e}", args.policy))?;
    let policy = QueryPolicy::parse(&raw)
        .map_err(|e| anyhow::anyhow!("租户声明 {:?} 不合法：{e}", args.policy))?;

    // ── ⑤ schema ────────────────────────────────────────────
    let raw = std::fs::read_to_string(&args.schema)
        .map_err(|e| anyhow::anyhow!("读不出 schema 文件 {:?}：{e}", args.schema))?;
    let schema = crate::domain::ttl::SchemaTypes::parse(&raw)
        .map_err(|e| anyhow::anyhow!("schema 文件 {:?} 不合法：{e}", args.schema))?;
    if schema.is_empty() {
        anyhow::bail!("--schema 里一列都没读到 —— 这份 schema 无法对账（⛔ 不猜列类型）");
    }

    // ── ⑥ 项目 / 版本 / 快照（必须已冻结）───────────────────
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
             查询服务只服务定稿版本（与 `serve` / `mcp-server` 同一条闸门）。\n  \
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

    // ── ⑧ 源库（只读；读回校验）──────────────────────────────
    let url = crate::config::require_env(crate::extract::introspect::SOURCE_DATABASE_URL_ENV)
        .map_err(|e| anyhow::anyhow!("{e}（查询服务查的是源库，不是元数据库）"))?;
    let src = crate::sparql::exec::connect_read_only(&url)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    // ── ⑨ 审计落点 + 服务状态 ───────────────────────────────
    let audit = match &args.audit_out {
        Some(path) => AuditSink::open(path).map_err(|e| {
            anyhow::anyhow!(
                "打不开审计文件 {path:?}：{e} —— ⛔ 不静默降级成「只有 stderr」，
                 请换一个可写的路径或去掉 --audit-out"
            )
        })?,
        None => AuditSink::stderr_only(),
    };

    let svc = std::sync::Arc::new(QueryService::new(ServiceParts {
        source: src.pool.clone(),
        facts: src.facts.clone(),
        mapping,
        model,
        schema,
        policy,
        ontology_iri,
        project: args.project.clone(),
        version: args.version,
        timeout_ms: args.timeout_ms,
        sse_idle_timeout_ms: args.sse_idle_timeout_ms,
        sse_max_connections: args.sse_max_connections,
        max_concurrent_queries: args.max_concurrent_queries,
        tokens,
        audit,
    }));

    let tenants = svc.tokens.tenants();
    eprintln!(
        "{}",
        banner(
            addr,
            verdict,
            &svc.project,
            svc.version,
            svc.tokens.len(),
            &tenants,
            svc.tokens.max_rows_cap(),
            svc.timeout_ms,
            svc.max_concurrent_queries,
            &svc.facts,
        )
    );

    let app = super::http::build_router(std::sync::Arc::clone(&svc));
    let listener = tokio::net::TcpListener::bind(addr).await.map_err(|e| {
        anyhow::anyhow!("绑不上 {addr}：{e}（端口被占 / 地址不属于本机？换一个 --bind 再试）")
    })?;
    axum::serve(listener, app)
        .await
        .map_err(|e| anyhow::anyhow!("查询服务退出：{e}"))
}

// ══════════════════════════════════════════════════════════════
// 判据（纯函数：不连库、不绑 socket）
// ══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sparql::exec::SessionFacts;

    fn facts_on() -> SessionFacts {
        SessionFacts {
            transaction_read_only: "on".into(),
            standard_conforming_strings: "on".into(),
        }
    }

    /// ⭐ AC-008 的第一半：内网/回环放行，通配/公网**没有 `--allow-public` 就拒绝启动**。
    #[test]
    fn the_bind_gate_only_allows_internal_addresses_without_the_flag() {
        // 放行：回环 + RFC1918 + IPv6 ULA
        for ok in ["127.0.0.1:8800", "10.0.0.5:8800", "192.168.1.9:8800", "172.16.3.4:8800", "[::1]:8800", "[fd00::1]:8800"] {
            let addr = parse_bind(ok).expect(ok);
            let v = check_bind(addr, false).unwrap_or_else(|e| panic!("{ok} 应当放行：{e}"));
            assert!(v.internal, "{ok}");
            assert!(!v.needs_warning(), "{ok} 是内网/本机，不该警告");
        }

        // 拒绝：通配 + 公网（没有 --allow-public）
        for bad in ["0.0.0.0:8800", "[::]:8800", "8.8.8.8:8800"] {
            let addr = parse_bind(bad).expect(bad);
            let e = check_bind(addr, false).expect_err(&format!("{bad} 不该放行"));
            assert!(e.contains("--allow-public"), "拒绝时要说清怎么放行：{e}");
            assert!(e.contains("内网"), "拒绝时要说清默认口径：{e}");
        }

        // 带 --allow-public ⇒ 放行，且**必须**警告
        for pubaddr in ["0.0.0.0:8800", "[::]:8800", "8.8.8.8:8800"] {
            let addr = parse_bind(pubaddr).expect(pubaddr);
            let v = check_bind(addr, true).expect("带 flag 应当放行");
            assert!(v.needs_warning(), "{pubaddr} 绑定必须警告");
        }
        let v = check_bind(parse_bind("0.0.0.0:8800").unwrap(), true).unwrap();
        assert!(v.unspecified, "0.0.0.0 必须被识别成通配地址");
        assert_eq!(v.label(), "所有网卡");
    }

    #[test]
    fn a_bad_bind_string_gets_a_human_error() {
        let e = parse_bind("10.0.0.5").unwrap_err();
        assert!(e.contains("10.0.0.5:8800"), "要给出正确写法：{e}");
        assert!(parse_bind("[::1]:8800").is_ok());
    }

    /// ⭐ AC-008 的第二半：横幅必须打印绑定地址 / 项目版本 / token 数 / 租户数，
    /// 绑公网时**有警告**；只读回执里的值来自入参（写死就会红）。
    #[test]
    fn the_banner_reports_bind_project_counts_and_warns_on_public() {
        let internal = banner(
            parse_bind("10.0.0.5:8800").unwrap(),
            check_bind(parse_bind("10.0.0.5:8800").unwrap(), false).unwrap(),
            "anspire_r2rml_excl",
            2,
            2,
            &["C0001", "C0002"],
            200,
            10_000,
            2,
            &facts_on(),
        );
        for needle in [
            "绑定 10.0.0.5:8800",
            "anspire_r2rml_excl 第 2 版",
            "token 2 个",
            "租户 2 个",
            "C0001",
            "C0002",
            "transaction_read_only = on",
            "standard_conforming_strings = on",
            "不记查询原文",
        ] {
            assert!(internal.contains(needle), "横幅缺 {needle:?}：\n{internal}");
        }
        assert!(!internal.contains("⚠ 警告"), "内网绑定不该警告：\n{internal}");

        // 公网绑定：有警告，且警告里点名地址与后果
        let public = banner(
            parse_bind("0.0.0.0:8800").unwrap(),
            check_bind(parse_bind("0.0.0.0:8800").unwrap(), true).unwrap(),
            "p",
            1,
            1,
            &["C0001"],
            100,
            1000,
            1,
            &facts_on(),
        );
        assert!(public.contains("⚠ 警告"), "{public}");
        assert!(public.contains("0.0.0.0:8800"), "{public}");
        assert!(public.contains("本机以外"), "{public}");

        // 只读回执的值来自入参：喂 off 就必须打印 off（⛔ 不许写死 on）
        let off = banner(
            parse_bind("127.0.0.1:1").unwrap(),
            check_bind(parse_bind("127.0.0.1:1").unwrap(), false).unwrap(),
            "p",
            1,
            1,
            &[],
            1,
            1,
            1,
            &SessionFacts {
                transaction_read_only: "off".into(),
                standard_conforming_strings: "off".into(),
            },
        );
        assert!(off.contains("transaction_read_only = off"), "{off}");
        assert!(!off.contains("= on"), "横幅里出现了写死的 on：\n{off}");
    }
}

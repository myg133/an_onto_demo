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
use super::{QueryService, ServiceMode, ServiceParts};

/// `ontok query serve` 的参数（与 clap 的定义分开：本模块不认识 clap）。
#[derive(Debug, Clone)]
pub struct ServeArgs {
    pub bind: String,
    pub tokens: PathBuf,
    pub project: String,
    pub version: i32,
    /// ⭐ REQ-136 · `None` = 本体服务模式（那里**没有** schema，也不读它）。
    /// 普通模式下**必填**（闸门在 [`serve`] 里：缺 ⇒ 拒绝启动，消息说明为什么）。
    pub schema: Option<PathBuf>,
    /// ⭐ REQ-136 · `None` 允许出现在本体服务模式（没有数据源 ⇒ 租户声明无处生效）；
    /// 普通模式下**必填**。
    pub policy: Option<PathBuf>,
    /// ⭐ REQ-136 · **本体服务模式**：不连源库、不做 schema / R2RML 对账。
    ///
    /// 与 `--schema` **互斥**（R1）：同时给 ⇒ 拒绝启动并说明。
    pub ontology_only: bool,
    /// ⭐ REQ-127 · 租户标注声明（**只在建模视图生效**；与 `ontology export --tenancy` 同一份）。
    ///
    /// 省略时按「`--policy` 同目录的 `tenancy.json`」自动取（取不到则建模视图不带租户标注，
    /// 启动横幅会明说）—— 这样默认启动命令也能与 CLI 的 `--tenancy tenancy.json` 对拍。
    pub tenancy: Option<PathBuf>,
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
    /// ⭐ REQ-145 · **access 映射**（`--access-mapping <file>`）。
    ///
    /// 省略 ⇒ 与基线**逐字节相同**（R1）：没有 `ontok:sourceKind` / `access` / `@pending` /
    /// `complete` / `mapping_version`，api 类照常按库源处理。
    /// 给了 ⇒ api 类进 TTL（两视图）、进 `catalog`/`describe` 的 `access`，
    /// 查询碰到它们时给 `@pending`（jsonld）或 `NOT_DB_SOURCE`（rows）。
    pub access_mapping: Option<PathBuf>,
}

impl ServeArgs {
    /// 本组参数对应哪个模式（**唯一**判据；`serve` 与判据都读它）。
    pub fn mode(&self) -> ServiceMode {
        if self.ontology_only {
            ServiceMode::OntologyOnly
        } else {
            ServiceMode::Query
        }
    }
}

/// ⭐ REQ-136 · 本体服务模式没有 `--policy` 时用的**无租户**声明。
///
/// 为什么需要它：`QueryService` 里有 `policy` 字段（普通模式的规划器要用），而本体模式
/// 一次规划都不会发生。给它一份「每张表都没登记」的空声明，比把字段 `Option` 化改动更小，
/// 且语义诚实：**这个部署没有任何租户口径**（对外说法在 `describe` 的
/// `tenant.mode = not_applicable`，⛔ 不把这份占位声明泄露给调用方）。
const NO_TENANT_POLICY: &str = r#"{"version":"1.0.0","tenantSubject":"__no_tenant__",
    "tenantIdColumn":"__no_tenant__","perTable":{},"publicDimensions":[]}"#;

/// ⭐ REQ-127 · 建模视图的租户声明从哪来。
///
/// 顺序：显式 `--tenancy` → `--policy` 同目录的 `tenancy.json`（存在才用）→ 没有。
///
/// 为什么允许自动取同目录那一份：本项目把两份声明放在一起（`query-policy.json` /
/// `tenancy.json`），而 `query serve` 的默认启动命令里只写了 `--policy`。
/// 若坚持必须显式给 `--tenancy`，默认命令产出的建模视图会**少**租户标注，
/// 与 `ontology export --model-view --tenancy tenancy.json` 对不上（AC-004）。
/// 取不到就**明确警告**，⛔ 不假装已标注。
///
/// ⭐ REQ-136 · `policy` 变成 `Option`（本体模式可以不给）：没有 `--policy` 就没有
/// 「同目录」可推，只剩显式 `--tenancy`。
fn resolve_tenancy(
    policy: Option<&std::path::Path>,
    explicit: Option<&std::path::Path>,
) -> Option<PathBuf> {
    if let Some(path) = explicit {
        return Some(path.to_path_buf());
    }
    let sibling = policy?
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("tenancy.json");
    sibling.is_file().then_some(sibling)
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
///
/// ⭐ REQ-136 · `mode` 决定多印哪一行：
/// - 普通模式：**一个字节都不变**（AC-006 的「零行为变化」连日志也照做），`facts` 必有；
/// - 本体模式：第二行起印 `mode=ontology-only（不接业务库）`（AC-001 的判据），
///   并把「只读会话（读回）」换成「数据源：无」——⛔ 没有会话就**不编**一个 `on`。
#[allow(clippy::too_many_arguments)]
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
    facts: Option<&crate::sparql::exec::SessionFacts>,
    mode: ServiceMode,
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
    // ⭐ REQ-136 · 模式行：普通模式不印（既有横幅不变），本体模式必须印（AC-001）。
    if let Some(line) = mode.banner_line() {
        s.push_str(line);
    }
    s.push_str(&format!(
        "· SPARQL 语法 v1 · 超时 {timeout_ms} ms · 单次最多 {max_rows_cap} 行（按 token）· \
         全局并发 {max_concurrent_queries}\n"
    ));
    match facts {
        // ⛔ 这两个值来自连接上**读回**的事实，不是字面量（REQ-122 · N3）。
        Some(f) => s.push_str(&format!(
            "· 只读会话（读回）：transaction_read_only = {} · standard_conforming_strings = {}\n",
            f.transaction_read_only, f.standard_conforming_strings
        )),
        // ⭐ REQ-136 · 本体模式没有会话可读回 ⇒ 明说「没有数据源」，⛔ 不编一个 on。
        None => s.push_str(
            "· 数据源：无（本体服务模式不连业务库；SPARQL / sample 恒 503 NO_DATA_SOURCE）\n",
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

    // ⭐ REQ-136 · **模式闸**（R1 + 启动约束）。位置在「绑 socket / 读文件 / 连库」之前：
    // 参数自相矛盾时报的是参数问题，不该先看到「token 表读不出来」这种下游错误。
    let mode = args.mode();
    if args.ontology_only && args.schema.is_some() {
        anyhow::bail!(
            "--ontology-only 与 --schema 互斥（R1）：本体服务模式**不做 schema 对账**，\
             给了 --schema 只会让人以为「关系是按列映射出来的」。\n  \
             只看本体：去掉 --schema（与 --policy，若不需要）。\n  \
             要按真实列映射（关系 = join）：去掉 --ontology-only，并给 --schema + --policy。"
        );
    }
    if !args.ontology_only && args.schema.is_none() {
        anyhow::bail!(
            "普通模式缺 --schema：主键 / 列存在性 / 列类型只能来自真实 schema（⛔ 不猜）。\n  \
             没有源库、只想对外提供本体（类 / 关系 / 属性 / TTL）时，请加 `--ontology-only`\
             （那个模式不需要 --schema，也不需要 ONTOK_SOURCE_DATABASE_URL）。"
        );
    }
    if !args.ontology_only && args.policy.is_none() {
        anyhow::bail!(
            "普通模式缺 --policy：租户主体 / 逐表过滤列只能来自声明（ADR-006，⛔ 不许猜一个列名）。\n  \
             本体服务模式（`--ontology-only`）不需要它（那里没有数据可查，也就没有租户口径）。"
        );
    }
    if args.ontology_only && args.include_inherit {
        eprintln!(
            "⚠ --include-inherit 在本体服务模式不生效：本模式不做 R2RML 对账（没有 join 可生成），\
             该开关只影响普通模式的映射生成。"
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
    // ⭐ REQ-137 · token 表**按模式**校验：普通模式 `tenant` 必填（原样）；本体模式
    // `tenant` 必须缺省（填了 ⇒ 这里就拒绝启动，⛔ 不带着假租户起来）。
    let tokens = TokenTable::parse_for_mode(&raw, mode)
        .map_err(|e| anyhow::anyhow!("token 表 {:?} 不合法：{e}", args.tokens))?;

    // ── ④ 声明 ──────────────────────────────────────────────
    //
    // ⭐ REQ-136 · 本体模式可以不给 `--policy`：那份声明的作用是「哪张表按哪一列过滤」，
    // 而本模式没有数据可查。不给时用一份**无租户**声明（每张表都没登记；对外说法见
    // `describe` 的 `tenant.mode = not_applicable`）。
    let policy = match &args.policy {
        Some(path) => {
            let raw = std::fs::read_to_string(path)
                .map_err(|e| anyhow::anyhow!("读不出租户声明 {path:?}：{e}"))?;
            QueryPolicy::parse(&raw)
                .map_err(|e| anyhow::anyhow!("租户声明 {path:?} 不合法：{e}"))?
        }
        None => QueryPolicy::parse(NO_TENANT_POLICY).map_err(|e| {
            anyhow::anyhow!("内部错误：本体服务模式的占位租户声明不合法（{e}）—— 这是 bug")
        })?,
    };

    // ── ⑤ schema ────────────────────────────────────────────
    //
    // ⭐ REQ-136 · 本体模式**不读** schema（`SchemaTypes::default()`：空表 = 没有真实
    // 列类型可用）。空 schema 在这里是**事实**而不是错误：本模式结构上不对账。
    let schema = match &args.schema {
        Some(path) => {
            let raw = std::fs::read_to_string(path)
                .map_err(|e| anyhow::anyhow!("读不出 schema 文件 {path:?}：{e}"))?;
            let schema = crate::domain::ttl::SchemaTypes::parse(&raw)
                .map_err(|e| anyhow::anyhow!("schema 文件 {path:?} 不合法：{e}"))?;
            if schema.is_empty() {
                anyhow::bail!("--schema 里一列都没读到 —— 这份 schema 无法对账（⛔ 不猜列类型）");
            }
            schema
        }
        None => crate::domain::ttl::SchemaTypes::default(),
    };

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

    // ── ⑥.4 ⭐ REQ-145 · access 映射（`--access-mapping`）──
    //
    // 位置在 TTL 之前：两视图都要写 `ontok:sourceKind "api"`，而「哪些类是 api」只在
    // 这份映射里。**加载即对账**（fail-closed）：类必须在这一版本体里存在，否则拒绝启动 ——
    // 一份「一半对不上」的映射最坏的后果是平台以为某类在库、其实在 API。
    //
    // ⚠️ 本段**不读任何凭据 / 端点**，也不执行任何调用：文件里那些字段读进来就丢
    // （见 `query::source_gate` 模块头）。
    let model = crate::domain::model_view::project_model_track(&snap);
    let access = match &args.access_mapping {
        Some(path) => {
            let raw = std::fs::read_to_string(path).map_err(|e| {
                anyhow::anyhow!(
                    "读不出 access 映射 {path:?}：{e}\n  \
                     形状见 `ontok query serve --help` 的 `--access-mapping`；\
                     ⛔ 读不出来就**当作没给**会静默退回「全是库源」，那不是你要的。"
                )
            })?;
            let parsed = crate::query::source_gate::AccessMapping::parse(&raw)
                .map_err(|e| anyhow::anyhow!("access 映射 {path:?} 不合法：{e}"))?;
            Some(parsed.bind(&model).map_err(|e| {
                anyhow::anyhow!("access 映射 {path:?} 与本体 v{} 对不上：{e}", args.version)
            })?)
        }
        None => None,
    };
    match &access {
        Some(a) if a.api_classes().is_empty() => eprintln!(
            "⚠ access 映射 {} 里一条 api 类都没有 ⇒ 全部类按库源处理（这多半不是你要的）",
            args.access_mapping
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default()
        ),
        Some(a) => {
            let mut keys: Vec<&String> = a.api_classes().keys().collect();
            keys.sort();
            // ⚠️ 两个数都报：**边数**（本体里的边）+ **方向数**（前向 + 反向，判定遍历时两个
            // 谓词都要认）。⛔ 只报一个的话，40 条边会被报成 80 —— 排查的人拿它对不上。
            eprintln!(
                "· access 映射：mapping_version={} · api 类 {} 个（{}）· \
                 跨源边 {} 条（{} 个方向）",
                a.mapping_version(),
                keys.len(),
                keys.iter().map(|k| k.as_str()).collect::<Vec<_>>().join("、"),
                a.cross_source_edge_count(),
                a.cross_source_direction_count()
            );
        }
        None => {}
    }

    // ── ⑥.5 ⭐ REQ-127 · 两视图 TTL（启动时算一次；版本冻结 ⇒ 内容不变）──
    //
    // 读/投影走 `cli::prepare_ttl_export`（与 `ontology export` **同一段**），
    // 渲染走 `console::ops::render_ttl_export`（同一个 `pipeline::render`）——
    // 于是 `GET /api/ontology.ttl` 与 CLI 导出同参数时逐字节相同（AC-004）。
    // 产不出就**拒绝启动**：这个端点的契约就是「与 CLI 一致」，产不出不能宣称能产。
    //
    // ⭐ REQ-136 · 本体模式把 `schema` 传 `None`（= CLI 的 `ontology export` 不带 `--schema`）
    // ⇒ 客户视图与 `ontok ontology export --project <p> --version <v>` 逐字节相同（AC-005）。
    //
    // ⭐ REQ-145 · `api_classes` 只有在加载了映射时才是 `Some` —— 没加载 ⇒ 两视图逐字节不变（R1）。
    let tenancy_path = resolve_tenancy(args.policy.as_deref(), args.tenancy.as_deref());
    let ttl = crate::query::ttl::OntologyTtl::prepare(
        pool,
        &crate::query::ttl::TtlSource {
            project: &args.project,
            version: args.version,
            base: &args.base,
            // 客户视图在 `prepare` 里固定用 None（结构上不含列名）；这里只影响建模视图。
            schema: args.schema.as_deref(),
            tenancy: tenancy_path.as_deref(),
            api_classes: access.as_ref().map(|a| a.api_classes()),
        },
    )
    .await
    .map_err(|e| {
        anyhow::anyhow!(
            "启动时无法生成 `/api/ontology.ttl` 的两视图（fail-closed）：{e}\n  \
             这个端点的契约是「与 `ontok ontology export` 同参数逐字节相同」；\
             产不出来就不该起服务（否则调用方会拿到一份与 CLI 不同的本体）。"
        )
    })?;
    match &tenancy_path {
        Some(path) => eprintln!(
            "· TTL 租户标注：{}（只在 `?view=model` 生效；客户视图结构上不含租户信息）",
            path.display()
        ),
        None if mode.is_ontology_only() => eprintln!(
            "· TTL 租户标注：无（本模式没有数据源 ⇒ 也没有租户口径；`?view=model` 不带 ontok:tenant*）"
        ),
        None => eprintln!(
            "⚠ 没有租户声明（`--tenancy` 未给，且 `--policy` 同目录下没有 tenancy.json）：\
             建模视图不含 ontok:tenant*；若要与 `ontology export --model-view --tenancy <file>` \
             逐字节相同，请补 `--tenancy`。"
        ),
    }
    eprintln!(
        "· OWL/TTL：GET /api/ontology.ttl（客户视图，{} 字节，ETag {}）· \
         ?view=model（建模视图，{} 字节，ETag {}）· 与 CLI 同参数逐字节相同",
        ttl.customer().bytes(),
        ttl.customer().etag(),
        ttl.model().bytes(),
        ttl.model().etag(),
    );

    // ── ⑦ 映射（与 r2rml generate 同一个函数）────────────────
    //
    // ⭐ REQ-136 · 本体模式**不建映射**：`build_mapping` 要求 schema（主键 / 列存在性都只能
    // 来自它），而本模式结构上没有 schema。`None` 也正是「不许自动造 schema」的形状。
    let ontology_iri = crate::domain::ttl::TtlOptions {
        project: args.project.as_str(),
        version: args.version,
        base: args.base.as_str(),
        action_type_count: snap.action_types.len(),
        tenancy: None,
        schema: None,
        api_classes: None,
    }
    .ontology_iri();
    let mapping = if mode.is_ontology_only() {
        None
    } else {
        let opts = crate::domain::ttl::TtlOptions {
            project: args.project.as_str(),
            version: args.version,
            base: args.base.as_str(),
            action_type_count: snap.action_types.len(),
            tenancy: None,
            schema: Some(&schema),
            // ⚠️ R2RML 是**库源**的映射：它的生成与 access 映射无关（⛔ api 类不生成
            // TriplesMap）。这里给 `None`，让 R2RML 的字节与没加载 access 时完全一致。
            api_classes: None,
        };
        Some(
            crate::domain::r2rml::build_mapping(
                &model,
                &opts,
                args.include_inherit,
                crate::domain::r2rml::R2rmlAudit::default(),
            )
            .map_err(|e| {
                anyhow::anyhow!(
                    "按 R2RML 生成 SQL 的前置对账失败（映射与 schema 不一致，⛔ 一个字节都不产出）：\n{e}"
                )
            })?,
        )
    };

    // ── ⑧ 源库（只读；读回校验）──────────────────────────────
    //
    // ⭐ REQ-136 · 本体模式**整段跳过**：不读 `ONTOK_SOURCE_DATABASE_URL`、不建连接池、
    // 不读回会话值（R2：「该模式 ⛔ 不许任何请求尝试连源库」—— 启动时也不连）。
    let source = if mode.is_ontology_only() {
        None
    } else {
        let url = crate::config::require_env(crate::extract::introspect::SOURCE_DATABASE_URL_ENV)
            .map_err(|e| anyhow::anyhow!("{e}（查询服务查的是源库，不是元数据库）"))?;
        let src = crate::sparql::exec::connect_read_only(&url)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        Some(src)
    };

    // ── ⑨ 审计落点 + 服务状态 ───────────────────────────────
    let audit = match &args.audit_out {
        Some(path) => AuditSink::open(path).map_err(|e| {
            anyhow::anyhow!(
                "打不开审计文件 {path:?}：{e} —— ⛔ 不静默降级成「只有 stdout」，
                 请换一个可写的路径或去掉 --audit-out"
            )
        })?,
        None => AuditSink::stdout_only(),
    };

    let svc = std::sync::Arc::new(QueryService::new(ServiceParts {
        mode,
        source: source.as_ref().map(|s| s.pool.clone()),
        facts: source.as_ref().map(|s| s.facts.clone()),
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
        ttl,
        // ⭐ REQ-145 · `None` = 没加载映射 ⇒ 运行期一切照旧（R1）。
        access,
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
            svc.facts.as_ref(),
            svc.mode,
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
            Some(&facts_on()),
            ServiceMode::Query,
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
            Some(&facts_on()),
            ServiceMode::Query,
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
            Some(&SessionFacts {
                transaction_read_only: "off".into(),
                standard_conforming_strings: "off".into(),
            }),
            ServiceMode::Query,
        );
        assert!(off.contains("transaction_read_only = off"), "{off}");
        assert!(!off.contains("= on"), "横幅里出现了写死的 on：\n{off}");
    }

    /// ⭐ REQ-127 · 建模视图的租户声明从哪来（AC-004 的建模视图那一半靠它）。
    ///
    /// 三条判据对应三种现实：显式给 / 约定位置有 / 两处都没有。
    /// 变异：把「显式优先」写成「同目录优先」⇒ 第一条红；把「没有就 None」写成
    /// 「造一个默认路径」⇒ 第三条红（会拿一个不存在的文件去开）。
    #[test]
    fn the_model_view_tenancy_resolves_explicit_then_sibling_then_none() {
        let dir = std::env::temp_dir().join(format!(
            "ontok-req127-tenancy-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("建临时目录");
        let policy = dir.join("query-policy.json");
        std::fs::write(&policy, "{}").expect("写 policy");
        let explicit = dir.join("elsewhere.json");

        // ① 显式给 ⇒ 就是它（即使同目录也有 tenancy.json）。
        std::fs::write(dir.join("tenancy.json"), "{}").expect("写同目录 tenancy");
        assert_eq!(
            resolve_tenancy(Some(policy.as_path()), Some(explicit.as_path())),
            Some(explicit.clone()),
            "`--tenancy` 是调用方的明确意图，⛔ 不能被同目录的约定覆盖"
        );

        // ② 没显式给 + 同目录有 ⇒ 用它（这正是默认启动命令能与 CLI 对拍的原因）。
        assert_eq!(
            resolve_tenancy(Some(policy.as_path()), None),
            Some(dir.join("tenancy.json")),
            "`--policy` 同目录的 tenancy.json 应当被自动采用"
        );

        // ③ 两处都没有 ⇒ None（⛔ 不编一个路径出来）。
        std::fs::remove_file(dir.join("tenancy.json")).expect("删同目录 tenancy");
        assert_eq!(
            resolve_tenancy(Some(policy.as_path()), None),
            None,
            "没有声明时必须明说「没有」，不能返回一个不存在的路径"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}

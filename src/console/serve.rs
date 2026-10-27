//! REQ-124 · **控制面的启动与绑定闸门** —— 默认只绑回环，`0.0.0.0` 要 `--allow-public`。
//!
//! # 启动顺序（每条失败都在**绑 socket 之前**报出来）
//!
//! | # | 步骤 | 为什么在这个位置 |
//! | --- | --- | --- |
//! | ① | 绑定地址 + 闸门 | 绑错了根本不该读 token / schema |
//! | ② | `admin-tokens.json`（两套凭据） | 没有 token 表就没有鉴权 |
//! | ③ | `--schema` | 声明校验的「表 / 列必须存在」只能来自它 |
//! | ④ | 三份声明 | **存在性**检查（缺了只警告：GET 会 404，PUT 会新建） |
//! | ⑤ | 元数据库：项目存在 | 服务作用域必须是真项目 |
//! | ⑥ | 审计落点 | 打不开 ⇒ 拒绝启动（⛔ 不静默降级） |
//! | ⑦ | 启动横幅 → 绑 socket | 横幅打印绑定地址 / 项目 / 版本 / token 数 |
//!
//! # R2 比 `query serve` **更严**
//!
//! `query serve` 允许「内网地址」（RFC1918）默认放行；控制面**默认只给回环**：
//! 管理端点能 import / apply / freeze —— 那是改本体的权限，不是取数权限。
//! 任何非回环地址（含 `0.0.0.0` 与内网地址）都要显式 `--allow-public` + 启动警告。
//!
//! # 已知限制
//!
//! - 绑定闸门判的是**地址**，不是路由表：多网卡 / NAT / 端口转发仍可能把回环以外的
//!   东西暴露出去。真正的边界是网络策略 + admin token。
//! - 没有 TLS：本机 / 内网明文 HTTP。放到别处必须由反向代理加 TLS。

use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use sqlx::postgres::PgPool;

use crate::db::project::PgProjectStore;
use crate::domain::ttl::SchemaTypes;
use crate::query::audit::AuditSink;
use crate::query::serve::{is_internal_ip, parse_bind};

use super::audit::AuditLog;
use super::decl::DeclPaths;
use super::tokens::AdminTokens;
use super::{ConsoleParts, ConsoleService};

/// `ontok console serve` 的参数（与 clap 分开：本模块不认识 clap）。
#[derive(Debug, Clone)]
pub struct ServeArgs {
    pub bind: String,
    pub tokens: PathBuf,
    pub project: String,
    pub schema: PathBuf,
    pub exclusions: PathBuf,
    pub policy: PathBuf,
    pub tenancy: PathBuf,
    /// 绑非回环地址时必须显式给。
    pub allow_public: bool,
    pub audit_out: Option<PathBuf>,
    /// IRI 基（必须与 `r2rml generate` / `ontology export` 是同一个）。
    pub base: String,
}

/// 绑定地址的判定结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BindVerdict {
    pub loopback: bool,
    /// 内网 / 本机（仅用于标签：控制面**不**因为它是内网就放行）。
    pub internal: bool,
    pub unspecified: bool,
}

impl BindVerdict {
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

    /// 非回环 ⇒ 横幅必须警告（即使带了 `--allow-public`）。
    pub fn needs_warning(self) -> bool {
        !self.loopback
    }
}

/// 绑定闸门（**纯函数**，可单测）。
///
/// - 回环 ⇒ 放行；
/// - **任何**非回环（`0.0.0.0` / `::` / 内网 / 公网）⇒ 必须 `--allow-public`；
/// - 带了 flag ⇒ 放行，但横幅必须警告。
pub fn check_bind(addr: SocketAddr, allow_public: bool) -> Result<BindVerdict, String> {
    let ip: IpAddr = addr.ip();
    let verdict = BindVerdict {
        loopback: ip.is_loopback(),
        internal: is_internal_ip(ip),
        unspecified: ip.is_unspecified(),
    };
    if verdict.loopback || allow_public {
        return Ok(verdict);
    }
    Err(format!(
        "拒绝绑定 {addr}（{}）：控制面默认**只绑回环**（127.0.0.1 / ::1）。\n  \
         管理端点能 import / apply / freeze —— 那是**改本体**的权限，不是取数权限；\
         所以比 `query serve`（允许内网）更严：内网地址也要显式确认。\n  \
         要绑 {addr} 请加 `--allow-public`，并自己负责网络边界（防火墙 / 反向代理 / TLS）。\n  \
         只给本机：`--bind 127.0.0.1:8899`。",
        verdict.label()
    ))
}

/// 启动横幅（**纯函数**：判据在单测里逐项对）。
pub fn banner(
    addr: SocketAddr,
    verdict: BindVerdict,
    project: &str,
    version: Option<i32>,
    admin_tokens: usize,
    query_tokens: usize,
    exclusions: &std::path::Path,
    policy: &std::path::Path,
    tenancy: &std::path::Path,
) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "· ontok console serve 就绪：绑定 {addr}（{}） · 项目 {project} · {}\n",
        verdict.label(),
        match version {
            Some(v) => format!("当前第 {v} 版"),
            None => "还没有任何版本".to_string(),
        }
    ));
    s.push_str(&format!(
        "· 凭据：admin {admin_tokens} 张 / query {query_tokens} 张（query token 调管理端点 ⇒ 403）\n"
    ));
    s.push_str(&format!(
        "· 声明：exclusions={} · policy={} · tenancy={}（PUT 保存前校验）\n",
        exclusions.display(),
        policy.display(),
        tenancy.display()
    ));
    s.push_str(
        "· 审计：每个写操作一行（含失败），token 只记 sha256 前 12 位；stdout 恒写\
         （--audit-out 只是**追加**第二份，⛔ 不是替代）\n",
    );
    s.push_str(&format!(
        "· 健康检查（免 token）：http://{addr}{HEALTH_PATH}\n",
        HEALTH_PATH = super::http::HEALTH_PATH
    ));
    s.push_str(&format!(
        "· 指标（免 token）：http://{addr}{METRICS_PATH}\n",
        METRICS_PATH = crate::metrics::METRICS_PATH
    ));
    s.push_str(
        "· ⭐ 构建包导入（REQ-159，admin token）：POST /api/projects（发一次性 query key）→ \
         POST /api/projects/:p/bundles（application/octet-stream 原始字节，只校验不解析）→ \
         PUT /api/projects/:p/database + POST …/database/test（需要时才要）→ \
         POST …/bundles/:id/import（单事务产新版本，**不冻结**）→ POST …/activate（冻结 + 生效）\n",
    );
    s.push_str(
        "· DSN 加密列：AES-256-GCM，密钥来自 ONTOK_SECRET_KEY（缺失即拒绝启动）；\
         ⛔ 响应 / 日志 / 审计里都不出现明文\n",
    );
    s.push_str(&format!(
        "· 工作台（页面壳零数据、免 token）：http://{addr}/ —— 口令只存 sessionStorage；\
         7 个视图只调 /api/*，且 ⛔ 不为前端新开端点\n"
    ));
    s.push_str(
        "· 查询台 / 目录浏览调的是 REQ-123 查询服务的 /api/sparql、/api/catalog、/api/describe；\
         页面只发**同源**请求（CSP connect-src 'self'）⇒ 把两个服务放在同一个源后面\
         （反向代理按路径分流；本机调试用 tests/fixtures/req125/dev_proxy.ps1）。\n",
    );
    if verdict.needs_warning() {
        s.push_str(&format!(
            "⚠ 警告：绑定的是 {addr}（{}）—— 控制面可被本机以外访问。\
             拿到 admin token 的人可以 import / apply / freeze（改本体）。\
             请确认防火墙 / 反向代理 / TLS 边界。\n",
            verdict.label()
        ));
    }
    s
}

/// 载入 → 起服务。
pub async fn serve(pool: &PgPool, args: ServeArgs) -> anyhow::Result<()> {
    // ── ① 绑定地址 + 闸门 ───────────────────────────────────
    let addr = parse_bind(&args.bind).map_err(|e| anyhow::anyhow!("{e}"))?;
    let verdict = check_bind(addr, args.allow_public).map_err(|e| anyhow::anyhow!("{e}"))?;

    // ── ② 两套凭据 ──────────────────────────────────────────
    let raw = std::fs::read_to_string(&args.tokens).map_err(|e| {
        anyhow::anyhow!(
            "读不出 token 表 {:?}：{e}\n  格式：{{\"version\":\"1.0.0\",\"tokens\":[\
             {{\"token\":\"…\",\"kind\":\"admin\"}}, {{\"token\":\"…\",\"kind\":\"query\",\"tenant\":\"C0001\"}}]}}\n  \
             示例见 `admin-tokens.example.json`（里面的 token 是假值）。",
            args.tokens
        )
    })?;
    let tokens = AdminTokens::parse(&raw)
        .map_err(|e| anyhow::anyhow!("token 表 {:?} 不合法：{e}", args.tokens))?;

    // ── ②b ⭐ REQ-159 · DSN 加密列的密钥（**缺了就拒绝启动**）──────────
    //
    // 为什么在绑 socket 之前、而且是**无条件**的（哪怕这次启动一个人都不会配 DSN）：
    // `PUT /api/projects/:p/database` 要把 DSN 加密后落库。带着一把坏密钥 / 没有密钥
    // 启动，等于让那个端点在一个**会失败的**状态下对外可用 —— 那时用户已经上传了包、
    // 点开 DSN 弹窗，才收到 500。启动期报错的成本是零，运行期报错的成本是一次联调。
    //
    // ⛔ 没有「本地调试可以先跳过」的开关（与 `ONTOK_VIEW_TOKEN` 同一条纪律）。
    let secret_cipher = crate::console::dsn::SecretCipher::from_env().map_err(|e| {
        anyhow::anyhow!(
            "DSN 加密列的密钥不可用，拒绝启动控制面：\n{e}\n  \
             设置方式（当前会话）：\n    \
             PowerShell : $env:ONTOK_SECRET_KEY = '<64 位十六进制>'\n    \
             bash/zsh   : export ONTOK_SECRET_KEY=<64 位十六进制>\n  \
             ⚠️ 换密钥会让**已有**的 DSN 密文读不出来（v1 不做密钥轮换）：\
             换完要重新 `PUT /api/projects/:p/database` 一次。"
        )
    })?;

    // ── ③ schema（声明校验的唯一事实来源）────────────────────
    // 默认值 `code/out/schema.json` 是**仓库根**的相对路径。工作树（feature-*）
    // 里没有 `code/`，从工作树里敲文档上那条命令时会在上一级找到它 ——
    // 只在「默认值 + 相对路径」这一种情况下回落，⛔ 不猜任意路径。
    let schema_path = if args.schema.is_file() {
        args.schema.clone()
    } else {
        let sibling = std::path::Path::new("..").join(&args.schema);
        if args.schema == std::path::Path::new(crate::console::DEFAULT_SCHEMA) && sibling.is_file() {
            eprintln!(
                "· schema 不在当前目录，回落到工作树的上一级：{}",
                sibling.display()
            );
            sibling
        } else {
            args.schema.clone()
        }
    };
    let raw = std::fs::read_to_string(&schema_path).map_err(|e| {
        anyhow::anyhow!(
            "读不出 schema 文件 {:?}：{e}\n  控制面用它判「声明里的表 / 列是否存在」；\
             ⛔ 没有 schema 就没有 R4 的保存前校验 —— 拒绝启动。\n  \
             写法：`--schema code/out/schema.json`（`ontok-introspect` 的产物；\
             默认路径相对于**仓库根**，工作树里会自动向上一级找）。",
            args.schema
        )
    })?;
    let schema = SchemaTypes::parse(&raw)
        .map_err(|e| anyhow::anyhow!("schema 文件 {:?} 不合法：{e}", schema_path))?;
    if schema.is_empty() {
        anyhow::bail!(
            "schema 文件 {:?} 里一列都没读到 —— 这份 schema 无法校验声明（⛔ 不猜列）",
            schema_path
        );
    }

    // ── ④ 三份声明：只判**存在性**（缺失只警告）───────────────
    let paths = DeclPaths {
        exclusions: args.exclusions.clone(),
        policy: args.policy.clone(),
        tenancy: args.tenancy.clone(),
    };
    for (kind, path) in [
        ("exclusions", &paths.exclusions),
        ("policy", &paths.policy),
        ("tenancy", &paths.tenancy),
    ] {
        if !path.is_file() {
            eprintln!(
                "⚠ 声明 {kind} 不存在：{} —— GET /api/config/{kind} 会 404，PUT 会**新建**它",
                path.display()
            );
        }
    }

    // ── ⑤ 项目必须存在（服务作用域是真项目）───────────────────
    let projects = PgProjectStore::new(pool.clone());
    let proj = projects
        .get_by_name(&args.project)
        .await?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "项目 {:?} 不存在 —— 控制面只服务已存在的项目。\n  先建：\
                 `ontok project new --name {}`",
                args.project,
                args.project
            )
        })?;
    let versions = projects.list_versions(proj.id).await?;
    let latest = versions.last().map(|v| v.seq);

    // ── ⑥ 审计落点 ─────────────────────────────────────────
    let sink = match &args.audit_out {
        Some(path) => AuditSink::open(path).map_err(|e| {
            anyhow::anyhow!(
                "打不开审计文件 {path:?}：{e} —— ⛔ 不静默降级成「只有 stdout」，\
                 请换一个可写的路径或去掉 --audit-out"
            )
        })?,
        None => AuditSink::stdout_only(),
    };

    // ── ⑦ 服务状态 + 横幅 ──────────────────────────────────
    let admin_tokens = tokens.admin_count();
    let query_tokens = tokens.query_count();
    if query_tokens == 0 {
        eprintln!(
            "⚠ token 表里没有 query token：403（「这是查询 token，不是管理凭据」）这条语义\
             在本进程里**不会被触发** —— 401 与 403 就无法区分。"
        );
    }
    let svc = Arc::new(ConsoleService::new(ConsoleParts {
        pool: pool.clone(),
        project: args.project.clone(),
        base: args.base.clone(),
        tokens,
        schema,
        paths,
        audit: AuditLog::new(sink),
        bind: addr,
        allow_public: args.allow_public,
        secret_cipher,
    }));

    eprintln!(
        "{}",
        banner(
            addr,
            verdict,
            &svc.project,
            latest,
            admin_tokens,
            query_tokens,
            &svc.paths.exclusions,
            &svc.paths.policy,
            &svc.paths.tenancy,
        )
    );

    let app = super::http::build_router(Arc::clone(&svc));
    let listener = tokio::net::TcpListener::bind(addr).await.map_err(|e| {
        anyhow::anyhow!("绑不上 {addr}：{e}（端口被占 / 地址不属于本机？换一个 --bind 再试）")
    })?;
    axum::serve(listener, app)
        .await
        .map_err(|e| anyhow::anyhow!("控制面退出：{e}"))
}

// ══════════════════════════════════════════════════════════════
// 判据（纯函数：不连库、不绑 socket）
// ══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_loopback_is_allowed_without_the_flag() {
        for ok in ["127.0.0.1:8899", "[::1]:8899"] {
            let addr = parse_bind(ok).expect(ok);
            let v = check_bind(addr, false).unwrap_or_else(|e| panic!("{ok} 应当放行：{e}"));
            assert!(v.loopback, "{ok}");
            assert!(!v.needs_warning(), "回环不该警告");
        }
        // ⛔ 比 query serve 更严：内网地址也要 flag。
        for bad in ["0.0.0.0:8899", "[::]:8899", "10.0.0.5:8899", "192.168.1.9:8899", "8.8.8.8:8899"] {
            let addr = parse_bind(bad).expect(bad);
            let e = check_bind(addr, false).expect_err(&format!("{bad} 不该放行"));
            assert!(e.contains("--allow-public"), "拒绝时要说清怎么放行：{e}");
            assert!(e.contains("只绑回环"), "拒绝时要说清默认口径：{e}");
        }
        for pubaddr in ["0.0.0.0:8899", "10.0.0.5:8899", "8.8.8.8:8899"] {
            let addr = parse_bind(pubaddr).expect(pubaddr);
            let v = check_bind(addr, true).expect("带 flag 应当放行");
            assert!(v.needs_warning(), "{pubaddr} 绑定必须警告");
        }
        let v = check_bind(parse_bind("0.0.0.0:8899").unwrap(), true).unwrap();
        assert!(v.unspecified);
        assert_eq!(v.label(), "所有网卡");
    }

    #[test]
    fn the_banner_reports_bind_project_version_and_token_counts() {
        let addr = parse_bind("127.0.0.1:8899").unwrap();
        let text = banner(
            addr,
            check_bind(addr, false).unwrap(),
            "anspire_r2rml_excl",
            Some(2),
            1,
            2,
            std::path::Path::new("exclusions.json"),
            std::path::Path::new("query-policy.json"),
            std::path::Path::new("tenancy.json"),
        );
        for needle in [
            "绑定 127.0.0.1:8899",
            "项目 anspire_r2rml_excl",
            "当前第 2 版",
            "admin 1 张",
            "query 2 张",
            "403",
            "exclusions=exclusions.json",
            "/api/health",
        ] {
            assert!(text.contains(needle), "横幅缺 {needle:?}：\n{text}");
        }
        assert!(!text.contains("⚠ 警告"), "回环绑定不该警告：\n{text}");

        let pubaddr = parse_bind("0.0.0.0:8899").unwrap();
        let public = banner(
            pubaddr,
            check_bind(pubaddr, true).unwrap(),
            "p",
            None,
            1,
            1,
            std::path::Path::new("e.json"),
            std::path::Path::new("p.json"),
            std::path::Path::new("t.json"),
        );
        assert!(public.contains("⚠ 警告"), "{public}");
        assert!(public.contains("0.0.0.0:8899"), "{public}");
        assert!(public.contains("改本体"), "{public}");
        assert!(public.contains("还没有任何版本"), "{public}");
    }
}

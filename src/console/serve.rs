//! ⭐ REQ-162 · **本地评审台的启动与绑定闸门**。
//!
//! ```text
//! ontok-builder console serve --dir <工作目录> [--bind 127.0.0.1:8899] [--no-browser]
//! ```
//!
//! # 启动顺序（每条失败都在**绑 socket 之前**报出来）
//!
//! | # | 步骤 | 为什么在这个位置 |
//! | --- | --- | --- |
//! | ① | 绑定地址 + 回环闸 | 非回环地址**直接拒**（没有 `--allow-public`：本机评审台不出本机） |
//! | ② | 打开工作目录 | 目录不存在 / 不是目录 ⇒ 拒绝启动（说清 `--dir` 该给什么） |
//! | ③ | 绑 socket → 拿**真实**端口 | `--bind 127.0.0.1:0` 时横幅要打印内核分配的端口 |
//! | ④ | 启动横幅（stderr） | 打印 URL + 工作目录 + 草稿/定版概况（零业务数据） |
//! | ⑤ | 默认开浏览器 | `--no-browser` 只影响开不开浏览器，不影响起不起来 |
//!
//! # ⛔ 无口令、无项目、无库
//!
//! 不读 `--tokens` / `--project` / `--schema`，也不碰 `ONTOK_SECRET_KEY` ——
//! `ONTOK_DATABASE_URL` 缺了照常起（AC-007；真进程判据见 `tests/test_req162_console.rs`）。
//! 唯一的口令是**回环绑定**本身：只有本机进程能连上。
//!
//! # 回环闸复用旧的那一份
//!
//! [`check_bind`] 是 REQ-124/160 就在用的纯函数（`ontok-builder review open` 也在用），
//! 本单**不写第二份**：非回环 ⇒ 报错 + 说明「本机评审台只绑回环」。
//!
//! # 已知限制
//!
//! - 绑定闸门判的是**地址**，不是路由表：多网卡 / NAT / 端口转发仍可能把回环以外的
//!   东西暴露出去。真正的边界是本机网络策略。
//! - 没有 TLS：本机明文 HTTP。⛔ 放到别处没有意义（那是 Studio 的事）。

use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use crate::query::serve::{is_internal_ip, parse_bind};

use super::local::{self, LocalConsole};

/// `ontok console serve` 的参数（与 clap 分开：本模块不认识 clap）。
#[derive(Debug, Clone)]
pub struct ServeArgs {
    /// 工作目录（**一个工作目录 = 一个项目**；默认 `.`）。
    pub dir: PathBuf,
    /// 监听地址；只许回环。
    pub bind: String,
    /// 只影响开不开浏览器，不影响起不起来。
    pub no_browser: bool,
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

/// 打开工作目录 → 绑 socket → 起本地评审台。
///
/// ⚠️ 顺序：先判地址与目录（失败时不占端口），再绑 socket 拿真实端口，最后打印横幅。
/// `--bind 127.0.0.1:0` 时只有绑完才知道端口 —— 横幅必须打印**真实**地址，
/// 否则 `--no-browser` 的人不知道要打开哪里（`review open` 同一条顺序，理由相同）。
pub async fn serve(args: ServeArgs) -> anyhow::Result<()> {
    // ── ① 绑定地址 + 回环闸（复用旧的那一份 check_bind，⛔ 不写第二份）──
    let addr = parse_bind(&args.bind).map_err(|e| anyhow::anyhow!("{e}"))?;
    // `allow_public = false` 是硬编码的：本机评审台**只绑回环**（AC-001）。
    check_bind(addr, false).map_err(|e| {
        anyhow::anyhow!(
            "{e}\n  本机评审台（console serve）没有 `--allow-public`：\
             它只做「草稿 → 人签 → 定版 → 导出包」，不出本机。"
        )
    })?;

    // ── ② 工作目录（一个工作目录 = 一个项目）───────────────────
    let console = LocalConsole::open(&args.dir).map_err(|e| {
        anyhow::anyhow!("{}\n  改法：{}", e.message, e.hint)
    })?;

    // ── ③ 绑 socket（拿内核分配的真实端口）────────────────────
    let listener = tokio::net::TcpListener::bind(addr).await.map_err(|e| {
        anyhow::anyhow!(
            "绑不上 {addr}：{e}（端口被占 / 地址不属于本机？换一个 --bind 再试）"
        )
    })?;
    let real = listener.local_addr().map_err(|e| {
        anyhow::anyhow!("拿不到实际监听地址：{e}（这不该发生；把这条错误报给维护者）")
    })?;

    // ── ④ 启动横幅（stderr；stdout 留给需要 `| jq` 的场景）──────
    eprintln!("{}", desk_banner(real, &console));

    // ── ⑤ 默认开浏览器（`--no-browser` 只影响这一步）──────────
    if !args.no_browser {
        open_browser(&format!("http://{real}/"));
    }

    let app = local::build_router(Arc::new(console));
    axum::serve(listener, app)
        .await
        .map_err(|e| anyhow::anyhow!("本机评审台退出：{e}"))
}

/// 本地评审台的启动横幅（**纯函数**：判据在单测里逐项对）。
///
/// ⛔ 零业务数据：工作目录 / 项目名 / 计数 / 最近定版号 —— 没有对象名、没有 token
/// （本来也没有 token）。
pub fn desk_banner(addr: SocketAddr, console: &LocalConsole) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "· ontok console serve 就绪：http://{addr}/（仅本机 · 无口令）\n"
    ));
    s.push_str(&format!(
        "· 工作目录 {} · 项目 {}\n",
        crate::console::local::display_path(console.dir()),
        console.project()
    ));
    let draft = console.draft_summary();
    s.push_str(&format!(
        "· 草稿 {}（{} 条，已签 {}）· 最新定版 {} · 构建包 {}\n",
        draft.0, draft.1, draft.2, draft.3, draft.4
    ));
    s.push_str(
        "· 本机评审台只做四件事：读草稿 · 记裁决 · 定版 · 导出包\
         （无登录 / 项目 / 上传 / DSN / 入库 / 激活 / key / 审计）\n",
    );
    s.push_str(
        "· 端点：GET /api/state · GET /api/draft · POST /api/decide · POST /api/finalize · \
         GET /api/versions · POST /api/bundle · GET /api/bundle/download?seq=\n",
    );
    s
}

/// 打开本机浏览器（`--no-browser` 时调用方不走这里）。
///
/// 失败**不**影响服务：打不开浏览器是环境问题（无桌面 / 容器），不是启动失败 ——
/// 横幅已经把 URL 打出来了，复制粘贴即可。所以这里一个错误都不往上抛。
fn open_browser(url: &str) {
    #[cfg(target_os = "windows")]
    let cmd = {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", "", url]);
        c
    };
    #[cfg(target_os = "macos")]
    let cmd = {
        let mut c = std::process::Command::new("open");
        c.arg(url);
        c
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let cmd = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(url);
        c
    };
    let mut cmd = cmd;
    let _ = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
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

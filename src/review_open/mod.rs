//! ⭐ REQ-160 · **统一收口** —— 抽完之后起一个本机审核台，让**人**在页面上审、签。
//!
//! ```text
//! ontok-builder review open --project demo --bind 127.0.0.1:0
//! ```
//!
//! # 为什么是「一条命令」而不是「一堆参数」
//!
//! 构建端有两条路进来（独立 agent 读 skill 自己干 / 人敲命令驱动 ACP 干），
//! 但**出口只有一条**：抽完就起这个审核台。出口一分叉，就一定会出现
//! 「agent 模式跳过了人签」或「acp 模式产物形状不同」两种事故（demand §二）。
//!
//! # 它不是新写的一套 HTTP
//!
//! | 需要的 | 复用的**那一份** |
//! | --- | --- |
//! | 路由表 | [`crate::console::http::build_router`]（控制面唯一一份） |
//! | 服务对象 | [`crate::console::ConsoleService`] / [`crate::console::ConsoleParts`] |
//! | 回环闸 | [`crate::console::serve::check_bind`]（⛔ 不写第二份） |
//! | 启动横幅 | [`crate::console::serve::banner`] |
//! | 声明落点 | [`crate::console::default_paths`] |
//! | 口令表 | [`crate::console::tokens::AdminTokens::parse`]（内存构造，见下） |
//!
//! ⛔ 与 `console::serve::serve` 的唯一区别是**凭据从哪来**：它要一个 token
//! **文件路径**，而本命令的口令是**一次性、只在进程内存里**的东西。
//! ⛔ 所以这里**不**调 `serve()`：那会强迫把口令写进一个临时文件 —— 正是需求
//! §三-S2 明令禁止的事。除了「凭据来源」与「绑 socket 的时机」，其余每一步都走
//! 上面那张表里的**同一个**函数。
//!
//! # 一次性口令（S2）的落点
//!
//! | 规则 | 落在哪 |
//! | --- | --- |
//! | ≥ 32 字符 | [`one_shot_token`]：`otk_` + 两个 UUIDv4 的 32 位十六进制（68 字符），熵来自 `uuid` 已有的 `getrandom` 通道（⛔ 不新增 crate） |
//! | 只在进程内存 | [`one_shot_tokens`]：拼一份 JSON **字符串**喂给 `AdminTokens::parse`，⛔ **一次文件访问都没有**（本文件零 `fs::` 写调用，判据见 `the_one_shot_module_never_touches_the_filesystem`） |
//! | ⛔ 不进 URL | [`review_url`] 只返回 `http://<addr>/`；口令只出现在 [`one_shot_notice`] 的终端正文里 |
//! | ⛔ 不进日志 / 审计原文 | 审计只记 `sha256` 前 12 位（`TokenEntry::audit_digest`，REQ-124 既有行为） |
//! | 退出即失效 | 前台 `axum::serve`；进程退出 = 令牌表随内存消失。⛔ 没有后台/daemon 形态 |
//!
//! # 绑定与端口（S1 / AC-003 / AC-005）
//!
//! ① 解析 `--bind`（复用 `query::serve::parse_bind`）→ ② [`crate::console::serve::check_bind`]
//! 判回环（非回环必须 `--allow-public`）→ ③ **先绑 socket** 拿内核分配的真实端口
//! （`--bind 127.0.0.1:0`）→ ④ 打印**真实** URL → ⑤ 起服务。
//!
//! 顺序与 `console::serve` 有一处**故意不同**：那里先打横幅再绑 socket（横幅里的地址
//! 是人给的），这里必须在打印之后就能让人点开 —— 所以 `:0` 时必须**先**绑。若给的是
//! 一个被占的具体端口，[`bind_listener`] 会**自动顺延**到内核分配（并大声警告），
//! 而不是让整条命令失败（AC-003 说的是「仍能起来」，判据是「打印的端口不是 8899」）。
//!
//! # 为什么**不在** `t5` 出口登记里（BA 裁定，REQ-160）
//!
//! `tests/test_req107_source_enumeration.rs` 的 t5 要求「出口文件里的每个函数都登记用途」。
//! 本模块**刻意不登记为出口**，理由三条（缺一条就该重新裁定）：
//!
//! 1. **只绑回环**：默认 `127.0.0.1:0`，非回环必须显式 `--allow-public`（[`check_listen_addr`]
//!    复用的就是 `console::serve::check_bind`）。出口登记说的是「能被谁读到」，这里的答案恒为
//!    **本机同一个人**。
//! 2. **只打印到本机终端**：唯一的 stdout 是 [`one_shot_notice`]（URL + 一次性口令 + 提示），
//!    它不是客户可见数据 —— 是**凭据**与启动文案；口令本身还**不进 URL / 不落盘 / 不进日志**。
//! 3. **不产生任何客户可见产品**：本模块**没有**自己的序列化出口 —— 没有 TTL、没有 JSON、
//!    没有页面数据。页面那一份在 `console::page`（壳内零数据），数据全走既有的
//!    `console::http` 路由（那一份的出口早已登记）。审核台只是**把既有的控制面绑到本机**
//!    并换了一种凭据来源。
//!
//! ⚠️ 所以 t5 里 `src/cli.rs` 只登记了两个壳函数（`build_options` / `build_cmd`）；
//! 如果将来这里开始**产出客户可见字节**（导出、快照、页面数据），第 3 条就不再成立 ——
//! 那时必须回来登记，⛔ 不能拿这条注释当豁免。
//!
//! # 已知限制（⛔ 不写成「已封堵」）
//!
//! - 没有 TLS：本机明文 HTTP。放到别处必须由反向代理加 TLS。
//! - 绑的是**地址**，不是路由表：多网卡 / NAT 仍可能把回环以外的东西暴露出去。
//! - 口令在进程内存里是明文（与 REQ-123/124 同一条边界）；比较是定长的。
//! - `--schema` 缺省时声明校验**无所对照**（见 [`load_schema`]）：页面照常能审，
//!   但 `PUT /api/config/*` 会被拒绝 —— 这是刻意的 fail-closed，⛔ 不造一个假 schema。

pub mod build_run;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use sqlx::postgres::PgPool;

use crate::console::audit::AuditLog;
use crate::console::decl::{DeclKind, DeclPaths};
use crate::provision::dsn::SecretCipher;
use crate::console::serve::check_bind;
use crate::console::tokens::AdminTokens;
use crate::console::{ConsoleParts, ConsoleService};
use crate::db::project::PgProjectStore;
use crate::domain::ttl::{SchemaTypes, DEFAULT_BASE};
use crate::query::audit::AuditSink;
use crate::query::serve::parse_bind;

/// 默认监听地址：端口 `0` = **内核分配**（AC-003 的起点，冲突不可能发生）。
pub const DEFAULT_BIND: &str = "127.0.0.1:0";

/// 一次性口令的默认有效期（demand §三：`--token-ttl 4h`）。
pub const DEFAULT_TOKEN_TTL: &str = "4h";

/// 一次性口令的最小长度（需求 §三-S2：≥ 32 字符）。
///
/// 与 `query::tokens::MIN_TOKEN_LEN`（24）**不是**同一个数：那条是「文件里的长期口令」，
/// 这条是「本机一次性口令」。两个数字都留着，是因为它们的理由不同（前者要抗离线猜测，
/// 后者只活一个进程），⛔ 不为了统一而把长期口令的闸降到 32 或把这条降到 24。
pub const MIN_ONE_SHOT_TOKEN_LEN: usize = 32;

/// 入参不合法 ⇒ 退出码 **2**（沿用 REQ-159 的口径：`EXIT_BUNDLE_REJECTED` 也是 2）。
///
/// ⚠️ 如实登记：数值 `2` 在 `cli.rs` 里已经有两位住户（`EXIT_LEAKED_MODELING_TERMS`
/// 与 `EXIT_BUNDLE_REJECTED`）。三者的区别在**触发面**，不在数值 —— 派单 §3-5 指定
/// 「入参不合法 ⇒ 2」，这里只落地，不重排既有政策。
pub const EXIT_BAD_INPUT: i32 = 2;

/// **入参不合法**（不是环境错、不是库错）。
///
/// 为什么要有这个类型：`cli::run` 的失败一律让 bin 以 1 退出（`src/bin/ontok-builder.rs`），
/// 而派单要求「项目 / 版本不存在 ⇒ exit 2 并点名」。用 `anyhow` 的 downcast 在
/// **CLI 边界**翻译成退出码，比在业务函数里 `std::process::exit` 好：后者让「拒绝」
/// 与「崩溃」在测试里长得一模一样。
#[derive(Debug, Clone)]
pub struct BadInput(pub String);

impl std::fmt::Display for BadInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for BadInput {}

fn bad(msg: impl Into<String>) -> anyhow::Error {
    anyhow::Error::new(BadInput(msg.into()))
}

/// 一个错误该以哪个退出码收尾（CLI 边界用这一条，⛔ 不在业务函数里 exit）。
pub fn exit_code_for(e: &anyhow::Error) -> i32 {
    if e.downcast_ref::<BadInput>().is_some() {
        EXIT_BAD_INPUT
    } else {
        // 与 `src/bin/ontok-builder.rs` 的历史口径一致：操作性错误 = 1。
        1
    }
}

/// CLI 边界的收尾：打印一行人话，然后按 [`exit_code_for`] 退出。
///
/// 为什么在**边界**退出而不是在业务函数里 `std::process::exit`：
/// 后者会让「入参被拒」与「崩溃」在测试里长得一模一样（都是进程没了）。
/// 业务函数只返回带类型的错误，退出码是这里唯一的一次翻译。
pub fn finish(r: anyhow::Result<()>) -> anyhow::Result<()> {
    match r {
        Ok(()) => Ok(()),
        Err(e) => {
            eprintln!("✗ {e}");
            std::process::exit(exit_code_for(&e));
        }
    }
}

// ══════════════════════════════════════════════════════════════
// 一次性口令（S2）
// ══════════════════════════════════════════════════════════════

/// 生成一枚**一次性 admin 口令**。
///
/// `otk_` + 2 × UUIDv4 的 32 位十六进制 = **68 字符**（≥ [`MIN_ONE_SHOT_TOKEN_LEN`]）。
/// 熵来自 `uuid` crate 已有的 `getrandom` 通道 —— ⛔ 不为它新增 `rand` 之类的 crate
/// （项目纪律：不新增依赖）。
pub fn one_shot_token() -> String {
    format!(
        "otk_{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

/// 把一枚明文口令拼成 `admin-tokens.json` 的**同一份形状**（一个字符串，不落盘）。
///
/// 为什么走 JSON 字符串而不是自己 `TokenEntry { … }` 构造：`AdminTokens` 的校验
/// （kind 必填、长度、重复）**只有一份**，在 `parse` 里。绕开它等于让「一次性的」
/// 这张表不受与文件版同样的检查 —— 那正是「一个 token 两用」重新长出来的地方。
pub fn one_shot_tokens_json(token: &str) -> String {
    serde_json::json!({
        "version": "1.0.0",
        "tokens": [{
            "label": "review-open-one-shot",
            "token": token,
            "kind": "admin",
        }],
    })
    .to_string()
}

/// 内存里的一张**只有一枚 admin** 的口令表（⛔ 不写任何文件）。
pub fn one_shot_tokens(token: &str) -> Result<AdminTokens, String> {
    AdminTokens::parse(&one_shot_tokens_json(token))
}

// ══════════════════════════════════════════════════════════════
// 口令有效期（--token-ttl）
// ══════════════════════════════════════════════════════════════

/// 解析 `--token-ttl`：`4h` / `30m` / `45s`，裸数字按**秒**。
///
/// 为什么不做「1h30m」这种复合写法：这个值的唯一用途是「进程自己关掉的时间点」，
/// 复合写法只会让 `--help` 更难读，而多段解析的边界 bug（`1h30m` = 5400 还是 90？）
/// 一点不便宜。
pub fn parse_token_ttl(raw: &str) -> Result<Duration, String> {
    let t = raw.trim();
    if t.is_empty() {
        return Err("--token-ttl 是空的：要写成 `4h` / `30m` / `45s`（裸数字按秒）".to_string());
    }
    let (digits, unit_secs, unit_name) = if let Some(d) = t.strip_suffix(['h', 'H']) {
        (d, 3600u64, "小时")
    } else if let Some(d) = t.strip_suffix(['m', 'M']) {
        (d, 60u64, "分钟")
    } else if let Some(d) = t.strip_suffix(['s', 'S']) {
        (d, 1u64, "秒")
    } else {
        (t, 1u64, "秒")
    };
    let n: u64 = digits.trim().parse().map_err(|_| {
        format!("--token-ttl 看不懂：{raw:?}。要写成 `4h` / `30m` / `45s`（裸数字按秒）")
    })?;
    if n == 0 {
        return Err(format!(
            "--token-ttl 是 0{unit_name}：0 等于「起来就失效」，那与「起不来」混淆。要跑就给 ≥1。"
        ));
    }
    let secs = n.checked_mul(unit_secs).ok_or_else(|| {
        format!("--token-ttl 溢出：{raw:?} = {n}{unit_name}，算不成秒数")
    })?;
    Ok(Duration::from_secs(secs))
}

// ══════════════════════════════════════════════════════════════
// URL / 终端正文（AC-002：URL 里没有口令）
// ══════════════════════════════════════════════════════════════

/// 审核台的 URL —— **只有**地址，⛔ 没有 query string。
///
/// 为什么不带 `?project=`：页面壳是**零数据**的（REQ-125），它自己会从 `/api/projects`
/// 取项目；而 URL 一旦带参数，就一定会有人顺手把口令也塞进去 —— 页面的
/// `location.search` 是被明令禁用的（demand §1.1 第 5 步）。所以这里连参数都不开这个头。
pub fn review_url(addr: SocketAddr) -> String {
    format!("http://{addr}/")
}

/// 终端正文：URL + **一次性口令** + 有效期 + 「勿外传」提示。
///
/// ⚠️ 口令在这份正文里**恰好出现一次**（判据：`the_notice_carries_the_token_exactly_once`）。
/// 重复打印会把「抄一次」变成「抄错一次」的机会。
pub fn one_shot_notice(
    addr: SocketAddr,
    project: &str,
    version: Option<i32>,
    token: &str,
    ttl: Duration,
) -> String {
    let version_line = match version {
        Some(v) => format!("第 {v} 版"),
        None => "还没有任何版本（页面上只有待审提案）".to_string(),
    };
    let url = review_url(addr);
    let mut s = String::new();
    s.push_str("════════════════════════════════════════════════════════════\n");
    s.push_str(" ontok-builder review open · 本机审核台就绪\n");
    s.push_str(&format!("   URL   : {url}\n"));
    s.push_str(&format!("   项目  : {project}（{version_line}）\n"));
    s.push_str("════════════════════════════════════════════════════════════\n");
    s.push_str(&format!(" 一次性 admin 口令（本进程唯一凭据）: {token}\n"));
    s.push_str(&format!(
        "   · 有效期 {}，**进程退出即失效**（前台运行；Ctrl-C 结束）\n",
        human_ttl(ttl)
    ));
    s.push_str(
        "   · ⛔ 口令不进 URL、不落盘、不写日志 / 审计原文 —— 只在这一行终端输出里出现\n",
    );
    s.push_str(
        "   · ⚠️ 本机一次性口令，勿复制到共享文档 / 聊天工具 / 截图；也不要贴进浏览器地址栏\n",
    );
    s.push_str(
        "   · 用法：浏览器打开上面的 URL，把口令粘贴到页面里的口令框（页面只把它存 sessionStorage）\n",
    );
    s.push_str(&format!(
        "   · 健康检查（免口令）：http://{addr}{}\n",
        crate::console::http::HEALTH_PATH
    ));
    s
}

/// 人读的时长（`4h` / `30m` / `45s`）。
pub fn human_ttl(ttl: Duration) -> String {
    let secs = ttl.as_secs();
    if secs % 3600 == 0 {
        format!("{}h", secs / 3600)
    } else if secs % 60 == 0 {
        format!("{}m", secs / 60)
    } else {
        format!("{secs}s")
    }
}

// ══════════════════════════════════════════════════════════════
// 绑定（S1：只绑回环；AC-003：端口顺延）
// ══════════════════════════════════════════════════════════════

/// 解析 `--bind` + **复用** [`check_bind`] 判回环（⛔ 不写第二份闸）。
///
/// 返回 `(addr, verdict)`：verdict 交给 [`crate::console::serve::banner`] 打印「仅本机 /
/// 所有网卡」与警告，所以这里不自己拼标签。
pub fn check_listen_addr(
    bind: &str,
    allow_public: bool,
) -> Result<(SocketAddr, crate::console::serve::BindVerdict), anyhow::Error> {
    let addr = parse_bind(bind).map_err(bad)?;
    let verdict = check_bind(addr, allow_public).map_err(bad)?;
    Ok((addr, verdict))
}

/// 绑一个监听 socket，并**顺延**端口冲突。
///
/// - 给 `port = 0` ⇒ 内核分配（正常路径）；
/// - 给具体端口但**被占** ⇒ 警告 + 退到 `port = 0`（同一张网卡），⛔ 不让命令直接失败；
/// - 其它错误（地址不属于本机 / 权限）⇒ 原样报出（那是环境问题，不该被顺延掩盖）。
///
/// 返回 `(listener, 真实地址, 是否顺延过)`。真实地址是打印 URL 的**唯一**来源。
pub async fn bind_listener(
    addr: SocketAddr,
) -> std::io::Result<(tokio::net::TcpListener, SocketAddr, bool)> {
    match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => {
            let real = l.local_addr()?;
            Ok((l, real, false))
        }
        Err(e)
            if e.kind() == std::io::ErrorKind::AddrInUse && addr.port() != 0 =>
        {
            let fallback = SocketAddr::new(addr.ip(), 0);
            let l = tokio::net::TcpListener::bind(fallback).await?;
            let real = l.local_addr()?;
            Ok((l, real, true))
        }
        Err(e) => Err(e),
    }
}

// ══════════════════════════════════════════════════════════════
// schema（声明校验的唯一事实来源）
// ══════════════════════════════════════════════════════════════

/// 声明校验用的真实 schema 从哪来。
#[derive(Debug, Clone)]
pub struct LoadedSchema {
    pub schema: SchemaTypes,
    /// 给用户看的一句话（缺省时是**警告**，不是假装成功）。
    pub note: String,
    /// `false` ⇒ 没有 schema，声明校验无所对照。
    pub strict: bool,
}

/// 载入 schema，三条路（顺序是刻意的）：
///
/// 1. `--schema <file>` 给了 ⇒ 必须存在、必须是合法 JSON、且**非空**（否则 `bad`，
///    exit 2）—— 给了却读不动是入参问题，不该悄悄退化成「没有 schema」；
/// 2. 没给 ⇒ 尝试默认落点 `code/out/schema.json`，以及工作树里向上一级找
///    （与 `console::serve` 的默认值回落**同一口径**，因为构建常常在 feature-* 工作树里跑）；
/// 3. 还是找不到 ⇒ [`SchemaTypes::default()`]（**空**）+ 醒目警告。
///
/// ⚠️ 第 3 条为什么不是「拒绝启动」：本命令的主职责是**审**（`GET /api/review`、
/// `POST /api/decide`），那两条端点在鉴权之后**不查 schema**。而空 schema 让
/// `decl::validate` 对任何表/列都说「不存在」⇒ `PUT /api/config/*` 一律 400 ——
/// **fail-closed**，不是「放行一切」。⛔ 绝不凭空调一份假 schema 去骗过校验。
pub fn load_schema(explicit: Option<&Path>) -> Result<LoadedSchema, anyhow::Error> {
    let (path, from_flag) = match explicit {
        Some(p) => (Some(p.to_path_buf()), true),
        None => {
            let def = Path::new(crate::console::DEFAULT_SCHEMA);
            if def.is_file() {
                (Some(def.to_path_buf()), false)
            } else {
                let sibling = Path::new("..").join(def);
                if sibling.is_file() {
                    (Some(sibling), false)
                } else {
                    (None, false)
                }
            }
        }
    };

    let Some(path) = path else {
        return Ok(LoadedSchema {
            schema: SchemaTypes::default(),
            note: format!(
                "⚠ 没有 schema（{} 不存在，也没有给 --schema）：声明校验无所对照 —— \
                 `PUT /api/config/*` 会被**拒绝**（表 / 列无从查起）。\
                 页面照常能审；要改声明请先 `--schema <introspect 产物>`。",
                crate::console::DEFAULT_SCHEMA
            ),
            strict: false,
        });
    };

    let raw = std::fs::read_to_string(&path).map_err(|e| {
        if from_flag {
            bad(format!(
                "读不出 --schema 文件 {}：{e}。\n  它是声明校验的唯一事实来源；\
                 ⛔ 给了就不能悄悄当成「没给」。",
                path.display()
            ))
        } else {
            anyhow::anyhow!("读不出 schema 文件 {}：{e}", path.display())
        }
    })?;
    let schema = SchemaTypes::parse(&raw).map_err(|e| {
        let msg = format!("schema 文件 {} 不合法：{e}", path.display());
        if from_flag {
            bad(msg)
        } else {
            anyhow::anyhow!("{msg}")
        }
    })?;
    if schema.is_empty() {
        let msg = format!(
            "schema 文件 {} 里一列都没读到 —— 这份 schema 无法校验声明（⛔ 不猜列）",
            path.display()
        );
        return Err(if from_flag { bad(msg) } else { anyhow::anyhow!("{msg}") });
    }
    Ok(LoadedSchema {
        schema,
        note: format!("· schema：{}", path.display()),
        strict: true,
    })
}

// ══════════════════════════════════════════════════════════════
// 浏览器（尽力而为：开不出来只警告）
// ══════════════════════════════════════════════════════════════

/// 本平台的「打开默认浏览器」命令（**纯函数**，判据不真开浏览器）。
///
/// | 平台 | 命令 |
/// | --- | --- |
/// | Windows | `cmd /C start "" <url>`（第一个空串是窗口标题位，否则带引号的 URL 会被当成标题） |
/// | macOS | `open <url>` |
/// | 其它 | `xdg-open <url>` |
pub fn browser_command(url: &str) -> (&'static str, Vec<String>) {
    #[cfg(target_os = "windows")]
    {
        (
            "cmd",
            vec![
                "/C".to_string(),
                "start".to_string(),
                String::new(),
                url.to_string(),
            ],
        )
    }
    #[cfg(target_os = "macos")]
    {
        ("open", vec![url.to_string()])
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        ("xdg-open", vec![url.to_string()])
    }
}

/// 尝试打开浏览器：**失败只警告，不影响服务**（SSH / 无桌面环境是常态）。
///
/// 子进程的 stdout/stderr 一律 `/dev/null`（`Stdio::null()`）：它只是个启动器，
/// 拿它的输出去判断成败没有意义，而接管道会让某些环境下的 spawn 变脆。
pub fn try_open_browser(url: &str) -> Result<(), String> {
    let (prog, args) = browser_command(url);
    std::process::Command::new(prog)
        .args(&args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_child| ())
        .map_err(|e| format!("{prog} 启动失败：{e}"))
}

// ══════════════════════════════════════════════════════════════
// 起审核台
// ══════════════════════════════════════════════════════════════

/// `ontok-builder review open` 的参数（已从 clap 拆出来：本模块不认识 clap）。
#[derive(Debug, Clone)]
pub struct OpenArgs {
    pub project: String,
    /// 省略 = 该项目**最新**版本。
    pub version: Option<i32>,
    pub bind: String,
    pub allow_public: bool,
    /// 不给就尝试打开系统浏览器。
    pub no_browser: bool,
    /// `4h` / `30m` / `45s`。
    pub token_ttl: String,
    /// 真实 schema（声明校验用）。省略见 [`load_schema`]。
    pub schema: Option<PathBuf>,
    /// IRI 基；省略用 [`DEFAULT_BASE`]（与 `console serve` 的默认值同一个常量值）。
    pub base: Option<String>,
    /// 审计落点（**追加**；省略 = 只写 stdout，与 `console serve` 同一口径）。
    pub audit_out: Option<PathBuf>,
}

/// 只读地校验「项目 + 版本」存在，返回真实的项目行与版本行。
///
/// ⛔ 只读：**不迁移、不建版本、不写库**（demand §三-1）。
/// 版本省略 ⇒ 最新一版；一版都没有 ⇒ `Ok((proj, None))`（审核台照常起：
/// 还没 apply 过的项目本来就可能没有版本）。
///
/// ⚠️ REQ-163：本条命令**只服务库里的项目**。`build run` 已改成纯本地链（不写库），
/// 所以它**不会**再替本命令「先把项目建出来」—— 要审库里的东西，项目 / 版本得先用
/// `project new` / `version new` / `import` 建出来。
pub async fn resolve_target(
    pool: &PgPool,
    project: &str,
    version: Option<i32>,
) -> Result<
    (
        crate::db::project::ProjectRow,
        Option<crate::db::project::VersionRow>,
    ),
    anyhow::Error,
> {
    let projects = PgProjectStore::new(pool.clone());
    let proj = projects
        .get_by_name(project)
        .await
        .map_err(|e| anyhow::anyhow!("查项目 {project:?} 失败：{e}"))?
        .ok_or_else(|| {
            bad(format!(
                "项目 {project:?} 不存在。\n  控制面只服务**已存在**的项目：先 `ontok-builder \
                 project new --name {project}`（或 `import` 一次把项目建出来）。\n  ⛔ 本命令不建项目 \
                 —— 它只读。\n  （⭐ REQ-163：`build run` 已改成**纯本地**链、不写库，\
                 所以它不会再替你把库里的项目建出来。）"
            ))
        })?;
    let versions = projects
        .list_versions(proj.id)
        .await
        .map_err(|e| anyhow::anyhow!("查项目 {project:?} 的版本失败：{e}"))?;
    let vrow = match version {
        Some(v) => Some(
            versions
                .iter()
                .find(|x| x.seq == v)
                .cloned()
                .ok_or_else(|| {
                    let known = if versions.is_empty() {
                        "该项目还没有任何版本".to_string()
                    } else {
                        format!(
                            "现有：{}",
                            versions
                                .iter()
                                .map(|x| format!("v{}", x.seq))
                                .collect::<Vec<_>>()
                                .join(" / ")
                        )
                    };
                    bad(format!(
                        "版本 v{v} 不存在（项目 {project:?}）。{known}。\n  ⛔ 本命令不建版本 \
                         —— 它只读。要新开一版：`ontok-builder version new --project {project}`。"
                    ))
                })?,
        ),
        None => versions.last().cloned(),
    };
    Ok((proj, vrow))
}

/// `review open` 里**不需要数据库**就能判完的那部分（纯函数）。
///
/// 为什么把它单独拿出来：`--bind 0.0.0.0:8899` 不带 `--allow-public` 是**入参错**（exit 2），
/// 而「元数据库连不上」是**环境错**（exit 1）。若先连库，未配库的机器上永远看不到
/// 那条「地址写错了」——报错会指向错的原因（与 `doc ingest` 在连库之前分派同一条纪律）。
#[derive(Debug, Clone, Copy)]
pub struct Prepared {
    pub addr: SocketAddr,
    pub verdict: crate::console::serve::BindVerdict,
    pub ttl: Duration,
}

/// 判入参（地址 + 回环闸 + TTL）。**零 I/O、零数据库**。
pub fn prepare(args: &OpenArgs) -> Result<Prepared, anyhow::Error> {
    let (addr, verdict) = check_listen_addr(&args.bind, args.allow_public)?;
    let ttl = parse_token_ttl(&args.token_ttl).map_err(bad)?;
    Ok(Prepared { addr, verdict, ttl })
}

/// 起审核台：**先判入参，再连库**（见 [`Prepared`]），然后走 [`open_with`]。
///
/// ⛔ 本命令**不跑迁移**：它只读（项目 / 版本 / 提案）——「读不动就提示先跑一次迁移」，
/// 而不是替用户改库。这也是 demand §三-1「只读，不迁移、不写」的落点。
pub async fn open(args: OpenArgs) -> Result<(), anyhow::Error> {
    prepare(&args)?;
    let url = crate::config::require_env(crate::config::DATABASE_URL_ENV)
        .or_else(|_| crate::config::require_env(crate::config::TEST_DATABASE_URL_ENV))
        .map_err(|_| {
            // ⚠️ 两个名字都点出来：`require_env` 的报错只说**最后一个**名字，
            // 而「设哪个才对」正是这个人此刻要回答的问题。
            anyhow::anyhow!(
                "缺少元数据库连接串：请设置 {}（或兼容名 {}）。\n  \
                 审核台要读元数据库（项目 / 版本 / 提案），没有内存替身。\n  \
                 （⛔ 本命令不连源库：它审的是元数据库里的提案与版本。）",
                crate::config::DATABASE_URL_ENV,
                crate::config::TEST_DATABASE_URL_ENV
            )
        })?;
    let pool = crate::db::pool::connect(&url).await?;
    open_with(&pool, args).await
}

/// 已经拿到连接池的入口（[`open`] 用它：自己连完库再调这里）。
///
/// ⚠️ REQ-163：`build run` **不再**走这里（它已改成纯本地链，尾巴起的是
/// `console::serve`）。本函数保留是因为 [`open`] 的两段式（先判入参、再连库）
/// 需要一个「池已就绪」的入口 —— 测试也用它注入懒连接池。
///
/// 顺序见模块头「绑定与端口」一节；每一步失败都在**绑 socket 之前**报出来
/// （除了绑本身），与 `console::serve` 同一条纪律。
pub async fn open_with(pool: &PgPool, args: OpenArgs) -> Result<(), anyhow::Error> {
    // ── ① 地址 + 回环闸（复用 check_bind）／TTL ────────────────
    let Prepared { addr: want_addr, verdict, ttl } = prepare(&args)?;

    // ── ② 项目 / 版本存在（只读）──────────────────────────────
    let (proj, vrow) = resolve_target(pool, &args.project, args.version).await?;

    // ── ③ 一次性口令（进程内存；⛔ 不落盘）────────────────────
    let token = one_shot_token();
    if token.len() < MIN_ONE_SHOT_TOKEN_LEN {
        // 不可能走到：生成器固定 68 字符。留着是因为「生成器将来被改短」这件事
        // 必须在**启动前**炸掉，而不是让一枚弱口令上岗。
        return Err(anyhow::anyhow!(
            "内部错误：一次性口令只有 {} 字符（要求 ≥ {MIN_ONE_SHOT_TOKEN_LEN}）",
            token.len()
        ));
    }
    let tokens = one_shot_tokens(&token).map_err(|e| anyhow::anyhow!("一次性口令表不合法：{e}"))?;

    // ── ④ schema / 声明 / 审计 / DSN 密钥 ─────────────────────
    let loaded = load_schema(args.schema.as_deref())?;
    let paths: DeclPaths = crate::console::default_paths();
    for kind in [DeclKind::Exclusions, DeclKind::Policy, DeclKind::Tenancy] {
        let p = kind.path(&paths);
        if !p.is_file() {
            eprintln!(
                "⚠ 声明 {kind:?} 不存在：{} —— GET /api/config/{} 会 404，PUT 会**新建**它",
                p.display(),
                kind.as_str()
            );
        }
    }
    let sink = match &args.audit_out {
        Some(p) => AuditSink::open(p).map_err(|e| {
            anyhow::anyhow!(
                "打不开审计文件 {}：{e} —— ⛔ 不静默降级成「只有 stdout」，换个可写路径或去掉 --audit-out",
                p.display()
            )
        })?,
        None => AuditSink::stdout_only(),
    };
    // DSN 加密列的密钥：与 `console serve` 同一条纪律（缺了就拒绝启动）。
    // ⛔ 不回落 `SecretCipher::for_tests()`（那是一把写死的密钥，等于没有加密）。
    let secret_cipher = SecretCipher::from_env().map_err(|e| {
        anyhow::anyhow!(
            "DSN 加密列的密钥不可用，拒绝启动审核台：\n{e}\n  设置方式（当前会话）：\n    \
             PowerShell : $env:ONTOK_SECRET_KEY = '<64 位十六进制>'\n    \
             bash/zsh   : export ONTOK_SECRET_KEY=<64 位十六进制>\n  \
             （审核本身不用 DSN，但控制面的 `ConsoleService` 无条件持有这把加密器 —— \
             与 `console serve` 同一个构造。）"
        )
    })?;

    // ── ⑤ 先绑 socket，拿**真实**端口（AC-003 的判据是打印出来的那个）──
    let (listener, real_addr, shifted) = bind_listener(want_addr).await.map_err(|e| {
        anyhow::anyhow!(
            "绑不上 {want_addr}：{e}（地址不属于本机 / 权限不足？换一个 --bind 再试）"
        )
    })?;
    if shifted {
        eprintln!(
            "⚠ {want_addr} 已被占用 ⇒ **自动顺延**到 {}（端口由内核分配；下面打印的是真实端口）",
            real_addr.port()
        );
    }
    // 真实地址再判一次闸：判的是 IP（顺延只换端口，不换网卡），但这条断言是承重的 ——
    // 将来若有人让 `bind_listener` 换网卡，这里会先炸。
    check_bind(real_addr, args.allow_public).map_err(bad)?;

    // ── ⑥ 服务对象（与 `console serve` 同一个构造器）────────────
    let base = args.base.clone().unwrap_or_else(|| DEFAULT_BASE.to_string());
    let svc = Arc::new(ConsoleService::new(ConsoleParts {
        pool: pool.clone(),
        // 用**库里那一行**的名字（`get_by_name` 是全等匹配，这里只是让「作用域取自
        // 真实项目行」这件事写在代码里，而不是靠调用方传对了字符串）。
        project: proj.name.clone(),
        base: base.clone(),
        tokens,
        schema: loaded.schema,
        paths: paths.clone(),
        audit: AuditLog::new(sink),
        bind: real_addr,
        allow_public: args.allow_public,
        secret_cipher,
    }));

    // ── ⑦ 横幅（复用 console::serve::banner 的文案）+ 一次性口令正文 ──
    eprintln!("· ontok-builder review open（一次性口令模式；⛔ 不写 token 文件）");
    eprintln!("{}", loaded.note);
    eprintln!(
        "{}",
        crate::console::serve::banner(
            real_addr,
            verdict,
            &svc.project,
            vrow.as_ref().map(|v| v.seq),
            svc.tokens.admin_count(),
            svc.tokens.query_count(),
            &svc.paths.exclusions,
            &svc.paths.policy,
            &svc.paths.tenancy,
        )
    );

    // 口令正文走 **stdout**：它是这条命令的**产物**，也是 AC-002 抓的那一处。
    println!(
        "{}",
        one_shot_notice(
            real_addr,
            &svc.project,
            vrow.as_ref().map(|v| v.seq),
            &token,
            ttl,
        )
    );

    // ── ⑧ 浏览器（尽力而为）──────────────────────────────────
    if args.no_browser {
        eprintln!("· 已跳过打开浏览器（--no-browser）：请手动打开上面的 URL");
    } else {
        let url = review_url(real_addr);
        match try_open_browser(&url) {
            Ok(()) => eprintln!("· 已尝试打开默认浏览器：{url}"),
            Err(e) => eprintln!(
                "⚠ 打不开浏览器（只警告，服务照常）：{e}\n  请手动打开：{url}"
            ),
        }
    }

    // ── ⑨ 前台跑：TTL 到点或进程被 Ctrl-C 结束 ────────────────
    let app = crate::console::http::build_router(Arc::clone(&svc));
    let ttl_text = human_ttl(ttl);
    let served = tokio::select! {
        r = axum::serve(listener, app) => Some(r),
        _ = tokio::time::sleep(ttl) => None,
    };
    match served {
        Some(Err(e)) => return Err(anyhow::anyhow!("审核台异常退出：{e}")),
        Some(Ok(())) => eprintln!("· 审核台已停止 —— 一次性口令随本进程失效"),
        None => println!(
            "· 一次性口令已到期（{ttl_text}）：审核台关闭，这枚口令**不再可用**。\
             要接着审就重新 `review open`（会生成一枚新口令）。"
        ),
    }
    // ⚠️ 走到这里 `token` / `tokens` 仍在本栈上，函数返回即随内存消失：
    // 这就是 S2 「进程退出即失效」的全部实现 —— 没有任何清理动作需要写。
    drop(token);
    Ok(())
}

// ══════════════════════════════════════════════════════════════
// 判据（纯函数 / 进程内路由；不连库、不起真 socket）
// ══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{header, Request, StatusCode};
    use tower::ServiceExt;

    /// 本模块源码：用来钉「口令这条路零落盘」这条结构判据。
    const MODULE_SRC: &str = include_str!("mod.rs");

    // ── 测试 1 · 一次性口令 ────────────────────────────────────

    #[test]
    fn the_one_shot_token_is_long_enough_unique_and_has_no_whitespace() {
        let a = one_shot_token();
        let b = one_shot_token();
        for t in [&a, &b] {
            assert!(
                t.len() >= MIN_ONE_SHOT_TOKEN_LEN,
                "口令 {} 只有 {} 字符（要求 ≥ {MIN_ONE_SHOT_TOKEN_LEN}）",
                t,
                t.len()
            );
            // ⚠️ 判据里**不打印**口令本身：失败消息进 CI 日志就等于落盘。
            assert!(
                !t.chars().any(char::is_whitespace),
                "口令里含空白字符（长度 {}）",
                t.len()
            );
            assert!(t.starts_with("otk_"), "口令要有可辨认的前缀（判据 2 靠它扫）");
        }
        assert_ne!(a, b, "两次生成的口令不许相同");
        assert!(a.len() <= 128, "68 字符是设计值，别让它悄悄膨胀");
    }

    #[test]
    fn the_one_shot_token_table_parses_and_holds_exactly_one_admin() {
        let t = one_shot_token();
        let table = one_shot_tokens(&t).expect("一次性口令表必须能过 AdminTokens::parse");
        assert_eq!(table.len(), 1);
        assert_eq!(table.admin_count(), 1, "一次性口令必须是 admin");
        assert_eq!(table.query_count(), 0, "⛔ 不夹带 query 凭据");
        let entry = table.find(&t).expect("刚落进表里的口令必须找得到");
        assert_eq!(
            entry.kind,
            crate::console::tokens::TokenKind::Admin,
            "一次性口令只能是 admin"
        );
        // 审计只写摘要：全长 71 = "sha256:" + 12；⛔ 不含明文。
        let digest = entry.audit_digest();
        assert_eq!(digest.len(), "sha256:".len() + 12);
        assert!(!digest.contains(&t), "审计摘要里不许出现口令明文");
        // 别人的口令不认。
        assert!(table.find("otk_nobody").is_none());
    }

    #[test]
    fn a_short_or_duplicated_one_shot_token_is_rejected_by_the_same_gate() {
        // 走的是 `AdminTokens::parse` 那一份校验：短口令必须被拒（⛔ 不放过）。
        assert!(one_shot_tokens("short").is_err(), "短口令必须被拒");
        assert!(
            one_shot_tokens_json("x").contains("\"kind\":\"admin\""),
            "内存表的形状必须是 admin"
        );
    }

    /// 测试 1 的「不落盘」半边：**模块源码**里没有任何写文件调用。
    ///
    /// 为什么是源码判据而不是「跑完再搜盘」：本仓库的惯例是结构判据
    /// （`tests/test_req107_*` 那一批），它不依赖「这次跑没跑到那条分支」。
    /// 扫的是本文件（`mod.rs`）—— `build_run.rs` 的产物写入是**另一件事**
    /// （断言 / 合并结果进 `--work-dir`），口令路径**不在**那里。
    ///
    /// ⚠️ 探针用 `concat!` 拼：直接把 `fs::write` 写进这个文件会让 `include_str!`
    /// 读到的源码里**恰好含有**探针本身（自指），判据就永远红了。
    #[test]
    fn the_one_shot_module_never_touches_the_filesystem_for_tokens() {
        // 只看**生产代码**：判据模块自己要写临时文件造夹具（schema 用例），
        // 那不是口令路径。切第一个 `#[cfg(test)]` 之前的部分。
        let prod = MODULE_SRC
            .split(concat!("#[cfg", "(test)]"))
            .next()
            .unwrap_or(MODULE_SRC);
        let writes = [
            concat!("fs::", "write"),
            concat!("File::", "create"),
            concat!("Open", "Options"),
            concat!("create", "_new"),
        ];
        for w in writes {
            assert!(
                !prod.contains(w),
                "一次性口令这条路不许落盘，但源码里出现了 `{w}`"
            );
        }
        // 读文件只有两处：schema 与审计（都不是口令）。
        assert!(
            prod.contains(concat!("fs::read_to", "_string")),
            "schema 必须真读（⛔ 不造假 schema）"
        );
    }

    // ── 测试 2 · URL 不含口令 ─────────────────────────────────

    #[test]
    fn the_url_carries_no_token_and_no_query_string() {
        let addr: SocketAddr = "127.0.0.1:54321".parse().unwrap();
        let url = review_url(addr);
        assert_eq!(url, "http://127.0.0.1:54321/");
        assert!(!url.contains('?'), "URL 不带 query string：{url}");
        assert!(!url.contains("token=") && !url.contains("key="), "{url}");

        // IPv6：Display 带方括号，拼出来要能直接点开。
        let v6: SocketAddr = "[::1]:8080".parse().unwrap();
        assert_eq!(review_url(v6), "http://[::1]:8080/");
    }

    #[test]
    fn the_notice_carries_the_token_exactly_once_and_keeps_it_out_of_the_url() {
        let addr: SocketAddr = "127.0.0.1:54321".parse().unwrap();
        let token = one_shot_token();
        let text = one_shot_notice(addr, "demo", Some(3), &token, Duration::from_secs(4 * 3600));

        assert_eq!(
            text.matches(&token).count(),
            1,
            "口令在终端正文里必须**恰好一次**（多一次就多一次抄错的机会）"
        );
        for needle in [
            "http://127.0.0.1:54321/",
            "demo",
            "第 3 版",
            "4h",
            "退出即失效",
            "不落盘",
            "勿复制",
            "/api/health",
        ] {
            assert!(text.contains(needle), "终端正文缺 {needle:?}：\n{text}");
        }
        // URL 那一段不许带口令，全文也不许出现 token= / key=。
        for (i, line) in text.lines().enumerate() {
            if line.contains("http://") {
                assert!(!line.contains(&token), "第 {} 行的 URL 里出现了口令：{line}", i + 1);
            }
        }
        assert!(!text.contains("token=") && !text.contains("key="));

        // 没有版本时说的是人话，不是「第 none 版」。
        let none = one_shot_notice(addr, "demo", None, "otk_x", Duration::from_secs(60));
        assert!(none.contains("还没有任何版本"), "{none}");
        assert!(none.contains("1m"), "60 秒要写成 1m：{none}");
    }

    // ── 测试 4 · 只绑回环（复用 check_bind）───────────────────

    #[test]
    fn only_loopback_binds_pass_without_allow_public() {
        let (addr, verdict) = check_listen_addr("127.0.0.1:0", false).expect("回环必须放行");
        assert_eq!(addr.port(), 0);
        assert!(verdict.loopback);
        assert!(!verdict.needs_warning());

        let e = check_listen_addr("0.0.0.0:8899", false).expect_err("不加 --allow-public 必须拒绝");
        let msg = format!("{e:#}");
        assert!(msg.contains("--allow-public"), "拒绝时要说清怎么放行：{msg}");
        assert!(msg.contains("只绑回环"), "拒绝时要说清默认口径：{msg}");
        assert_eq!(exit_code_for(&e), EXIT_BAD_INPUT, "入参不合法 ⇒ 2");

        let (_, v) = check_listen_addr("0.0.0.0:8899", true).expect("带 flag 必须放行");
        assert!(v.needs_warning(), "非回环绑定必须警告");

        // 格式错也是 2，不是 1。
        let e = check_listen_addr("localhost:8899", false).expect_err("要 SocketAddr 形状");
        assert_eq!(exit_code_for(&e), EXIT_BAD_INPUT);

        // 依赖 `check_bind` 这一份：grep 本文件不许出现第二份判定逻辑的痕迹
        // （探针同样用 `concat!` 避开自指）。
        assert!(
            !MODULE_SRC.contains(concat!("fn ", "check_bind")),
            "⛔ 不许在这里写第二份回环闸（复用 console::serve::check_bind）"
        );
    }

    // ── 测试 3 · 端口顺延 ────────────────────────────────────

    #[tokio::test]
    async fn an_occupied_port_is_shifted_and_the_real_port_is_reported() {
        // 先占住一个具体端口（模拟 `--bind 127.0.0.1:8899` 撞车）。
        let occupied = std::net::TcpListener::bind("127.0.0.1:0").expect("占一个端口");
        let busy: SocketAddr = occupied.local_addr().expect("真实地址");
        assert_ne!(busy.port(), 0);

        let (_listener, real, shifted) = bind_listener(busy).await.expect("必须顺延而不是失败");
        assert!(shifted, "端口被占时必须标记「顺延过」");
        assert_ne!(real.port(), busy.port(), "打印的端口必须是真实端口");
        assert_ne!(real.port(), 0, "内核分配的端口不会是 0");
        assert_eq!(real.ip(), busy.ip(), "顺延只换端口，不换网卡");

        // `:0` 是正常路径：不算顺延，端口由内核给。
        let (_l2, real2, shifted2) = bind_listener("127.0.0.1:0".parse().unwrap())
            .await
            .expect(":0 必须能绑");
        assert!(!shifted2, "port 0 不是「顺延」");
        assert_ne!(real2.port(), 0);
    }

    // ── 测试 5 · 页面壳可匿名取 / 管理端点要口令 ───────────────

    /// 一个**不连库**的服务：懒连接池（故意连不上）+ 一次性口令表。
    ///
    /// 与 `open()` 的构造逐字段同源（`ConsoleParts` 那 10 栏），只是把 schema 换成
    /// 空 schema、把 DSN 密钥换成测试密钥 —— 这两件事 `open()` 在**没有 schema /
    /// 没有 ONTOK_SECRET_KEY** 的机器上会拒绝或警告，而本用例要验的是**路由 + 鉴权**，
    /// 不是那两道启动闸（它们的判据在 [`load_schema`] / `check_listen_addr` 上）。
    fn service_with_one_shot_token(token: &str) -> Arc<ConsoleService> {
        Arc::new(ConsoleService::new(ConsoleParts {
            pool: sqlx::postgres::PgPoolOptions::new()
                .acquire_timeout(Duration::from_millis(200))
                .connect_lazy("postgresql://nobody@127.0.0.1:1/none")
                .expect("lazy pool"),
            project: "demo".into(),
            base: DEFAULT_BASE.into(),
            tokens: one_shot_tokens(token).expect("一次性口令表"),
            schema: SchemaTypes::default(),
            paths: crate::console::default_paths(),
            audit: AuditLog::new(AuditSink::stdout_only()),
            bind: "127.0.0.1:0".parse().unwrap(),
            allow_public: false,
            secret_cipher: SecretCipher::for_tests(),
        }))
    }

    async fn status_of(svc: Arc<ConsoleService>, req: Request<Body>) -> StatusCode {
        crate::console::http::build_router(svc)
            .oneshot(req)
            .await
            .expect("router")
            .status()
    }

    fn get(uri: &str, token: Option<&str>) -> Request<Body> {
        let mut b = Request::builder().method("GET").uri(uri);
        if let Some(t) = token {
            b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
        }
        b.body(Body::empty()).unwrap()
    }

    #[tokio::test]
    async fn the_page_shell_is_anonymous_but_the_api_needs_the_one_shot_token() {
        let token = one_shot_token();
        let svc = service_with_one_shot_token(&token);

        // 页面壳：免口令 200（浏览器导航带不了 Authorization）。
        assert_eq!(
            status_of(Arc::clone(&svc), get("/", None)).await,
            StatusCode::OK
        );
        // 管理端点：没有口令 ⇒ 401。
        assert_eq!(
            status_of(Arc::clone(&svc), get("/api/projects", None)).await,
            StatusCode::UNAUTHORIZED
        );
        // 别人的口令 ⇒ 401（不是 403：它既不是 admin 也不是 query）。
        assert_eq!(
            status_of(Arc::clone(&svc), get("/api/projects", Some("otk_nobody"))).await,
            StatusCode::UNAUTHORIZED
        );
        // 本命令生成的口令 ⇒ **过了鉴权**（懒连接池连不上 ⇒ 500；
        // 这里断言的是「不是 401/403」，也就是令牌表被认出来了，不依赖数据库）。
        let passed = status_of(Arc::clone(&svc), get("/api/projects", Some(&token))).await;
        assert!(
            passed != StatusCode::UNAUTHORIZED && passed != StatusCode::FORBIDDEN,
            "一次性口令必须过鉴权，实际 {passed}"
        );
    }

    // ── 其它纯函数 ───────────────────────────────────────────

    #[test]
    fn the_ttl_parser_accepts_the_documented_forms_and_rejects_the_rest() {
        assert_eq!(parse_token_ttl("4h").unwrap(), Duration::from_secs(4 * 3600));
        assert_eq!(parse_token_ttl("30m").unwrap(), Duration::from_secs(30 * 60));
        assert_eq!(parse_token_ttl("45s").unwrap(), Duration::from_secs(45));
        assert_eq!(parse_token_ttl("90").unwrap(), Duration::from_secs(90));
        assert_eq!(parse_token_ttl(" 2H ").unwrap(), Duration::from_secs(7200));
        for bad_raw in ["", "0", "0h", "4x", "h", "-1h"] {
            let e = parse_token_ttl(bad_raw).expect_err(&format!("{bad_raw:?} 必须被拒"));
            assert!(
                e.contains("--token-ttl"),
                "报错要点名那个参数：{bad_raw:?} ⇒ {e}"
            );
        }
    }

    #[test]
    fn the_human_ttl_reads_like_a_flag_value() {
        assert_eq!(human_ttl(Duration::from_secs(4 * 3600)), "4h");
        assert_eq!(human_ttl(Duration::from_secs(1800)), "30m");
        assert_eq!(human_ttl(Duration::from_secs(45)), "45s");
    }

    #[test]
    fn the_browser_command_differs_per_platform_and_never_shells_the_url_into_a_string() {
        let url = "http://127.0.0.1:8899/";
        let (prog, args) = browser_command(url);
        assert!(!prog.is_empty());
        assert_eq!(args.last().map(String::as_str), Some(url), "URL 必须是独立参数（⛔ 不拼 shell 串）");
        #[cfg(target_os = "windows")]
        {
            assert_eq!(prog, "cmd");
            assert_eq!(&args[..3], &["/C".to_string(), "start".to_string(), String::new()]);
        }
    }

    #[test]
    fn a_missing_schema_degrades_loudly_and_never_invents_one() {
        let dir = std::env::temp_dir().join(format!(
            "ontok-req160-schema-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("临时目录");

        // ① 显式给了却读不出 ⇒ exit 2（入参问题，⛔ 不悄悄退化成「没给」）。
        let missing = dir.join("nope.json");
        let e = load_schema(Some(&missing)).expect_err("读不出必须报错");
        assert_eq!(exit_code_for(&e), EXIT_BAD_INPUT);
        assert!(format!("{e:#}").contains("--schema"), "{e:#}");

        // ② 空 schema 文件 ⇒ 同样拒绝（「一列都没读到」不能被当成「没有 schema」）。
        let empty = dir.join("empty.json");
        std::fs::write(&empty, "{\"tables\":[]}").expect("写空 schema");
        let e = load_schema(Some(&empty)).expect_err("空 schema 必须被拒");
        assert!(format!("{e:#}").contains("一列都没读到"), "{e:#}");

        // ③ 合法 schema ⇒ strict = true，note 里点名文件。
        let ok = dir.join("ok.json");
        std::fs::write(
            &ok,
            r#"{"tables":[{"name":"customers","primary_key":"id",
                "columns":[{"name":"id","data_type":"text","is_primary_key":true}]}]}"#,
        )
        .expect("写 schema");
        let loaded = load_schema(Some(&ok)).expect("合法 schema");
        assert!(loaded.strict);
        assert!(loaded.schema.has_table("customers"));
        assert!(loaded.note.contains("ok.json"), "{}", loaded.note);

        // ④ 缺省且不存在 ⇒ **空** schema + 警告（fail-closed，不是放飞）。
        //    用 `set_current_dir` 太脏；这里只断言「缺省时不会 panic、不会造表」——
        //    具体走哪条分支取决于工作目录里有没有 code/out/schema.json，
        //    所以只判「strict 为 false 时 schema 必为空」这一条不变量。
        if let Ok(loaded) = load_schema(None) {
            if !loaded.strict {
                assert!(loaded.schema.is_empty(), "不 strict 时必须是没有 schema，⛔ 不许造假");
                assert!(loaded.note.contains("没有 schema"), "{}", loaded.note);
            }
        }

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_bad_bind_or_bad_ttl_is_exit_2_not_an_operation_failure() {
        // 环境错（库连不上 / 端口绑不上）才有资格用 1；入参错一律 2。
        let e = anyhow::anyhow!("数据库错误：connection refused");
        assert_eq!(exit_code_for(&e), 1);
        let e = bad("项目 nope 不存在");
        assert_eq!(exit_code_for(&e), EXIT_BAD_INPUT);
    }
}

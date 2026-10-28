//! ⭐ REQ-162 · **console = 本地评审台**（草稿 → 人签 → 定版 → 导出包）。
//!
//! ```text
//! ontok-builder console serve --dir <工作目录> [--bind 127.0.0.1:8899] [--no-browser]
//! ```
//!
//! # 用户口径（唯一依据）
//!
//! > 「纯本地构建，不需要口令。一个工作目录就是一个项目。构建包就是个导出包的功能。
//! >  console 的核心功能是评审。agent 生成的是草稿，人审批过后是定版，导出的是定版。」
//!
//! ⇒ 没有登录、没有项目选择、没有上传 / DSN / 入库 / 激活 / key / 审计。
//! 一条 `--dir` 就是全部作用域；`GET /` 是[一屏两栏的评审台](page)，数据全来自工作目录里的文件。
//!
//! # 本模块的组成
//!
//! | 模块 | 是什么 |
//! | --- | --- |
//! | [`local`] | ⭐ REQ-162 · 本地评审台的领域层 + 6+1 条**纯文件**端点（无库、无口令） |
//! | [`serve`] | 启动与回环闸（复用既有 `check_bind`；⛔ 不写第二份） |
//! | [`page`] | 一屏两栏页面壳（自包含单文件、零外部资源、无 localStorage） |
//! | [`OpError`] | CLI 核心函数与 HTTP 层共用的错误形状（`{code, message, hint}` 的源头） |
//!
//! # 旧「控制面」的那几个模块（诚实登记）
//!
//! `audit` / `decl` / `dsn` / `ops` / `provision` / `provision_api` / `tokens` 与
//! [`http`] 里的库版路由是 REQ-124 / REQ-159 的产物。它们**不再被 `console serve` 挂载**：
//! `console serve` 现在挂的是 [`local::build_router`]。
//!
//! 那批代码仍然编译、仍然被下面这些**本单不可改范围**的调用方使用：
//!
//! - `ontok-builder review open`（REQ-160）复用的是 [`http::build_router`] 与
//!   [`ConsoleService`]（一次性口令审核台，另一条命令、另一套语义）；
//! - `query serve` 的写面复用 [`provision_api`] / [`dsn`] / [`bundle`]（join 正在把它们
//!   搬到 `src/provision/`）；
//! - `cli.rs` 的既有子命令复用 [`ops`]。
//!
//! ⛔ 所以「旧库版路由删干净」在本分支的落点是：**`console serve` 的进程里一条都不挂**
//! （`tests/test_req162_console.rs` 逐条断言 404）；源码里那份路由要等 `review open`
//! 另单收口后才能删 —— 已写进 `BA/.local/dispatch/REQ-162-dev-changelog.md` 的「偏离」一节。
//!
//! # 已知限制
//!
//! - 没有多用户：本机同一个人。**没有任何凭据**，边界就是回环绑定。
//! - 快照的 `version_id` 每次定版新生成（`domain::apply` 的既有行为），
//!   包内/文件里的 `seq` 才是人看的那一栏。
//! - 并发写同一份 `decisions.json` 是后写覆盖（本地单人评审，没有锁 / ETag）。

pub mod audit;
pub mod decl;
pub mod http;
/// ⭐ REQ-162 · 本地评审台（纯文件端点；⛔ 一行 SQL / 一次网络都没有）。
pub mod local;
pub mod ops;
pub mod page;
pub mod provision;
/// ⭐ REQ-159 P1c-1 · 8 条写端点的**共享适配层**（console 与 `query serve` 各挂一份路由，
/// 端点语义只有这一份）。见模块头。
pub mod provision_api;
pub mod serve;
pub mod tokens;

use std::net::SocketAddr;
use std::path::PathBuf;

use axum::http::StatusCode;
use sqlx::postgres::PgPool;

use crate::domain::ttl::SchemaTypes;
use crate::query::error::ApiError;

/// **CLI 核心函数与 HTTP 层共用**的错误形状。
///
/// 为什么不让核心返回 `anyhow::Error`：控制面必须给出 `code` + `hint`（R7），
/// 而 `anyhow` 只有一句话；反过来让 CLI 用 HTTP 的 `ApiError` 又会把 `axum`
/// 拖进「不连库的本地命令」的返回类型。这个类型只带**三件东西**（状态 / 码 / 两句话），
/// 两个前端各自翻译：CLI 走 `Display` + 退出码，HTTP 走 [`ApiError`]。
#[derive(Debug, Clone)]
pub struct OpError {
    pub status: StatusCode,
    /// 机器可读的稳定错误码。
    pub code: &'static str,
    /// 人可读：错在哪。
    pub message: String,
    /// 可行动：下一步怎么办。
    pub hint: String,
    /// 只在「客户视图命中了建模术语」时非空 —— CLI 要按老格式打印**出口 + 词条**，
    /// 并以 [`crate::cli::EXIT_LEAKED_MODELING_TERMS`] 退出（AC-002 的现场判据）。
    pub leak_outlet: Option<&'static str>,
    pub leak_hits: Option<String>,
}

impl OpError {
    pub fn new(
        status: StatusCode,
        code: &'static str,
        message: impl Into<String>,
        hint: impl Into<String>,
    ) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            hint: hint.into(),
            leak_outlet: None,
            leak_hits: None,
        }
    }

    pub fn bad_request(
        code: &'static str,
        message: impl Into<String>,
        hint: impl Into<String>,
    ) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, message, hint)
    }

    pub fn not_found(
        code: &'static str,
        message: impl Into<String>,
        hint: impl Into<String>,
    ) -> Self {
        Self::new(StatusCode::NOT_FOUND, code, message, hint)
    }

    pub fn conflict(
        code: &'static str,
        message: impl Into<String>,
        hint: impl Into<String>,
    ) -> Self {
        Self::new(StatusCode::CONFLICT, code, message, hint)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL",
            message,
            "把出现的时间与这一步的动作报给维护者；这不是请求参数的写法问题。",
        )
    }

    /// 存储层错误 ⇒ 500。**只用于「本来不该发生」的路径**：可预期的冲突
    /// （已 applied / 已冻结 / 不存在）必须在调用点**先**判成 409 / 404，
    /// 不能靠这里把 `StoreError` 一律翻成 500。
    pub fn db(e: impl std::fmt::Display) -> Self {
        Self::internal(format!("数据库错误：{e}"))
    }

    /// 客户视图命中发布闸（AC-002）：一个字节都不产出。
    /// 客户视图命中发布闸（AC-002）：一个字节都不产出。
    ///
    /// ⚠️ **词条必须进 message**：2026-10-28 生产实测，实施人员只收到
    /// 「命中了建模术语（出口 C6）」—— 不知道是哪个词，无法修。
    /// 原来 `leak_hits` 只存不读（全仓零消费者），这里直接拼进 message，HTTP 与 CLI 都能看见。
    pub fn leak(outlet: &'static str, hits: impl Into<String>) -> Self {
        let hits = hits.into();
        let shown = if hits.chars().count() > 200 {
            let cut: String = hits.chars().take(200).collect();
            format!("{cut}…")
        } else {
            hits.clone()
        };
        let mut e = Self::conflict(
            "CUSTOMER_VIEW_LEAKED",
            format!("客户视图命中了建模术语（出口 {outlet}）：{shown}。已拒绝导出。"),
            "这是发布闸，不是 bug：客户视图不能带建模术语。改掉命中的说明文字后重试；\
             ⛔ 不要为了导出而放宽闸门。",
        );
        e.leak_outlet = Some(outlet);
        e.leak_hits = Some(hits);
        e
    }

    /// 出口渲染失败（序列化 / 管线内部错误）。
    pub fn render_failed(outlet: &'static str, message: impl Into<String>) -> Self {
        Self::internal(format!("出口 {outlet} 的产物生成失败：{}", message.into()))
    }

    /// 翻译成 HTTP 的错误契约（**唯一**的翻译点）。
    pub fn into_api(self) -> ApiError {
        ApiError::with_code(self.status, self.code, self.message, self.hint)
    }
}

impl std::fmt::Display for OpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // CLI 侧只打 message：与既有 `anyhow` 路径的可读性一致。
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for OpError {}

/// 服务状态（启动时一次性载入；运行期只有 audit 在变）。
pub struct ConsoleService {
    /// **元数据库**（项目 / 版本 / 提案 / 快照）—— 不是源库：控制面不跑查询。
    pub pool: PgPool,
    /// 本服务只服务这一个项目（与 `query serve` 同一条作用域纪律）。
    pub project: String,
    /// IRI 基（必须与 `r2rml generate` / `ontology export` 用的是同一个）。
    pub base: String,
    pub tokens: tokens::AdminTokens,
    /// 声明校验用的真实 schema（表 / 列只能来自它，⛔ 不猜）。
    pub schema: SchemaTypes,
    pub paths: decl::DeclPaths,
    pub audit: audit::AuditLog,
    /// ⭐ REQ-135 · `/metrics` 的注册表（进程私有；只有 admin 通道会改写它）。
    pub metrics: crate::metrics::Metrics,
    /// 实际绑定地址（横幅与 `/api/health` 之外的诊断用）。
    pub bind: SocketAddr,
    pub allow_public: bool,
    /// ⭐ REQ-159 · DSN 的对称加密器（密钥来自 `ONTOK_DSN_KEY`）。
    ///
    /// ⛔ 它只在这里**持有**：加解密在 [`dsn`]，对外没有任何「打印密钥 / 打印明文」
    /// 的出口（`Debug` 是手写的 `<redacted>`，见 [`crate::provision::dsn::SecretCipher`]）。
    pub secret_cipher: crate::provision::dsn::SecretCipher,
}

/// 构造参数（`serve` 与测试共用**这一个**构造器）。
pub struct ConsoleParts {
    pub pool: PgPool,
    pub project: String,
    pub base: String,
    pub tokens: tokens::AdminTokens,
    pub schema: SchemaTypes,
    pub paths: decl::DeclPaths,
    pub audit: audit::AuditLog,
    pub bind: SocketAddr,
    pub allow_public: bool,
    /// ⭐ REQ-159 · 见 [`ConsoleService::secret_cipher`]。
    pub secret_cipher: crate::provision::dsn::SecretCipher,
}

impl ConsoleService {
    pub fn new(p: ConsoleParts) -> Self {
        Self {
            pool: p.pool,
            project: p.project,
            base: p.base,
            tokens: p.tokens,
            schema: p.schema,
            paths: p.paths,
            audit: p.audit,
            metrics: crate::metrics::Metrics::for_console(),
            bind: p.bind,
            allow_public: p.allow_public,
            secret_cipher: p.secret_cipher,
        }
    }
}

/// 声明文件的默认落点（仓库根；`console serve` 的参数默认值同源）。
///
/// ⚠️ 用 `&'static str` 而不是 `PathBuf`：默认值要同时出现在 clap 的
/// `default_value` 与文档里，两处各写一遍迟早会漂。
pub const DEFAULT_EXCLUSIONS: &str = "exclusions.json";
pub const DEFAULT_POLICY: &str = "query-policy.json";
pub const DEFAULT_TENANCY: &str = "tenancy.json";
pub const DEFAULT_SCHEMA: &str = "code/out/schema.json";

/// 三份声明的路径组。
pub fn default_paths() -> decl::DeclPaths {
    decl::DeclPaths {
        exclusions: PathBuf::from(DEFAULT_EXCLUSIONS),
        policy: PathBuf::from(DEFAULT_POLICY),
        tenancy: PathBuf::from(DEFAULT_TENANCY),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 发布闸的**词条必须进 message**：2026-10-28 生产实测，实施人员只看到
    /// 「命中了建模术语（出口 C6）」，不知道是哪个词 —— `leak_hits` 存了却没人读。
    #[test]
    fn leak_message_names_the_hitting_terms() {
        let e = OpError::leak("C6", "主键、证据");
        assert!(e.message.contains("主键") && e.message.contains("证据"),
                "message 必须点名命中的词条，实际：{}", e.message);
        assert!(e.message.contains("C6"));
        let api = e.into_api();
        assert!(api.message.contains("主键"), "into_api 之后词条不能丢：{}", api.message);
    }

    #[test]
    fn leak_message_truncates_long_term_lists() {
        let long = "主键、".repeat(200);
        let e = OpError::leak("C6", long);
        assert!(e.message.contains("…"), "超长时应截断");
        assert!(e.message.chars().count() < 400, "message 不应无限增长");
    }
}

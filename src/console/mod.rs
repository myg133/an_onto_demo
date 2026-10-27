//! ⭐ REQ-124 · **控制面 API** —— 把管理流水线的每一步 CLI 变成一个端点。
//!
//! ```text
//! ontok console serve --bind 127.0.0.1:8899 --tokens admin-tokens.json \
//!     --project anspire_r2rml_excl --schema code/out/schema.json
//! ```
//!
//! # 它**不是**一条新的写库路径
//!
//! | 端点 | 它调的**既有**实现 |
//! | --- | --- |
//! | `POST /api/import` | [`crate::cli::import_proposal_core`]（`propose import` 的实现搬到这里，两个入口共用） |
//! | `POST /api/apply` | [`crate::db::apply::PgApplier::apply_proposal`]（`proposal apply` 调的同一个函数） |
//! | `POST /api/freeze` | [`crate::db::project::PgProjectStore::freeze`]（`version freeze` 调的同一个函数） |
//! | `GET /api/export/ttl` | [`crate::cli::prepare_ttl_export`]（读取/投影段）+ [`crate::console::ops::render_ttl_export`]（渲染） |
//! | `POST /api/r2rml` | [`crate::cli::r2rml_core`]（`r2rml generate` 的生成段，两个入口共用） |
//! | `GET/PUT /api/config/*` | 三份声明的既有解析器（`Exclusions` / `QueryPolicy` / `TenancyDecl`） |
//! | `GET /api/review` · `POST /api/decide` | [`crate::review_ui::http::review_json`] / [`decide_json`]（`review serve` 的同一份序列化与同一个 `decide_item`） |
//!
//! ⭐ **REQ-159 · 构建包导入（P1a）** 在这张表上新增 7 个端点，它们同样不是新写库路径：
//!
//! | 端点 | 它调的**既有**实现 |
//! | --- | --- |
//! | `POST /api/projects` | [`crate::db::project::PgProjectStore::create_labeled_in`] + [`crate::db::bundle_store::PgBundleStore::create_key_in`]（同一事务） |
//! | `POST /api/projects/:p/bundles` | [`bundle::verify_and_read`]（校验）+ [`crate::db::bundle_store::PgBundleStore::create_bundle`] |
//! | `GET /api/projects/:p/bundles/:id` | [`crate::db::bundle_store::PgBundleStore::get_bundle_in_project`] |
//! | `PUT /api/projects/:p/database` | [`crate::console::dsn::SecretCipher::encrypt`] + [`crate::db::bundle_store::PgBundleStore::upsert_database`] |
//! | `POST /api/projects/:p/database/test` | [`crate::console::dsn::probe`]（真连一次） |
//! | `POST /api/projects/:p/bundles/:id/import` | [`crate::db::apply::begin_new_version`] + [`crate::db::apply::write_snapshot_in`]（**与 `proposal apply` 同一套**，单事务） |
//! | `POST /api/projects/:p/bundles/:id/activate` | [`crate::db::project::PgProjectStore::activate`]（冻结 + 生效指针，同一事务） |
//!
//! ⚠️ **架构变化（要写 ADR）**：这 7 个端点让**控制面**第一次获得「写元数据库里的
//! 本体内容」的能力（`project apply` 之外的第二条产出新版本的路径）。边界是硬的：
//! 只有 **admin token** 能调（`query` token 一律 403），而 `query serve` 仍然**只读**、
//! 仍然一进程一项目（多项目是 P1b，本刀一行没碰）。见 demand §六-4。
//!
//! ⭐ REQ-125 · `GET /` 是**工作台页面壳**（[`page`]）：自包含单文件、零外部依赖、
//! **壳内零数据**、免 token（浏览器导航带不了 `Authorization`）。七个视图只消费上面
//! 这些端点（外加 REQ-123 查询服务的 `/api/sparql` / `/api/catalog` / `/api/describe`，
//! 按**同源**路径调用）—— ⛔ 本模块没有为前端新开端点。
//!
//! ⛔ 本模块**没有**「直接改对象」的端点：只暴露 import → review → apply → freeze
//! 这条既有流程（需求风险 1）。HTTP 层只做「解析 → 调上面那个函数 → 序列化」。
//!
//! # 四道硬闸
//!
//! 1. **token 分离**（R1）：[`tokens`] 一张文件两套凭据；query token 调管理端点 ⇒
//!    **403**（[`http::require_token`]），没有/错 token ⇒ **401**。
//! 2. **绑定**（R2）：默认只绑回环；非回环地址必须 `--allow-public` + 启动警告（[`serve`]）。
//! 3. **声明保存前校验**（R4）：[`decl::validate`] 对照 `--schema` 查表/列；
//!    失败 ⇒ 400 + 点名，**不落盘**（用保存前后 sha256 自证，见 `decl::save` 的调用顺序）。
//! 4. **审计**（R6）：[`audit::AuditLog`] 每个写操作一行（含失败），token 只记摘要。
//!
//! # 已知限制（⛔ 不写成「已封堵」）
//!
//! - 端点里**没有抽取**（`POST /api/extract` 刻意不做）：读源库 + 调模型 + 分钟级长任务
//!   属于任务系统，另开需求。
//! - 只有 `admin` 一个角色（本轮不做多用户 / 角色体系），所以「谁能 freeze」只有
//!   「谁有 admin token」一个答案。
//! - `GET /api/export/ttl` 的 `schema` / `tenancy` 是**服务端路径**（与 CLI 的
//!   `--schema` / `--tenancy` 同义）。拿 admin token 的人本来就能跑 CLI，这不是
//!   新的权限面；但它是「可以读服务端文件」的一条路，已登记在已知限制里。
//! - 声明 PUT 是**整份替换**（不是逐条增删）：校验的是「这份新文件」；
//!   并发两个 PUT 的结果是后写覆盖（没有乐观锁 / ETag）。

pub mod audit;
pub mod bundle;
pub mod decl;
pub mod dsn;
pub mod http;
pub mod ops;
pub mod page;
pub mod provision;
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
    pub fn leak(outlet: &'static str, hits: impl Into<String>) -> Self {
        let hits = hits.into();
        let mut e = Self::conflict(
            "CUSTOMER_VIEW_LEAKED",
            format!("客户视图命中了建模术语（出口 {outlet}），已拒绝导出。"),
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
    /// 的出口（`Debug` 是手写的 `<redacted>`，见 [`dsn::SecretCipher`]）。
    pub secret_cipher: dsn::SecretCipher,
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
    pub secret_cipher: dsn::SecretCipher,
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

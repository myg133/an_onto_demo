//! FDE 本体论平台 · 本体内核
//!
//! 分层纪律（验收 AC-002）：`domain` 模块**零 I/O**。
//! 本文件若出现 `sqlx` / `PgStore` / `db::` 即为违规 ——
//! `grep -rn 'sqlx\|PgStore\|db::' src/domain/` 必须无输出。
//!
//! 另：判定器 AC-002 的 grep 范围是 `src/domain/`，
//! 所以本文件声明存储层时不使用会被误伤的简写。

pub mod cli;
pub mod config;
pub mod db;
pub mod domain;
pub mod extract;

// REQ-027 · MCP 服务器（§9.5「Claude Desktop 经 MCP 连接」）。
// **不放 domain/**：它要连库、起进程，而 domain 的 AC-002 判据是「零 I/O」。
// 理由与上面 agent 模块的同一条，详见 `src/mcp/mod.rs`。
pub mod mcp;

// REQ-093 · **只读** HTTP 服务（把本体图关系摆到浏览器里）。
// **不放 domain/**：它要连库、要绑 socket，理由与上面 mcp 模块的同一条。
pub mod server;

// REQ-111 · **本地审核界面**（在 HTML 里做双轨审核）。
// ⛔ **不放 `server/`**：那一层被「只读」的结构判据钉着
// （`tests/test_graph_server.rs` 扫 `src/server/*.rs` 里不许出现 `set_decision` 等），
// 而审核界面必须**写**决策（唯一写点 `POST /api/decide`）。
// 把它塞进 `server/` 等于让「`serve` 只读」这条守卫为真而事实为假 ——
// 见 `src/review_ui/mod.rs` 模块头。
pub mod review_ui;

// REQ-013 · 抽取 agent 抽象（规格 §7.1 阶段 1「可插拔」）。
// **不放 domain/**：它要跑真实 agent（含将来的协议客户端与 DB 查询），
// 而 `domain` 的 AC-002 判据是「零 I/O」。理由见 `src/agent/mod.rs`。
pub mod agent;

// ⭐ REQ-122 · **SPARQL → SQL 规划与租户注入**（数字员工链路的第三块地基）。
// **不放 domain/**：它要连源库执行 SQL（domain 的 AC-002 判据是「零 I/O」）。
// 纯的那部分（解析 / 规划 / 声明 / SQL 文本）都在 `sparql::*` 里各自单测，不依赖库。
pub mod sparql;

// ⭐ REQ-123 · **查询服务对外**（REST + 远程 MCP）：一个进程、两种协议、**一份鉴权**。
// **不放 domain/**：它要起 socket、连源库、读 token 表。
// ⛔ 也不放进 `server/`：那一层的结构判据是「只读 + 只注册 GET」，而本模块要
// POST /api/sparql 与 POST /mcp，且鉴权模型完全不同（同一张 token 表 → 租户）。
// 见 `src/query/mod.rs` 模块头。
pub mod query;

// ⭐ REQ-124 · **控制面 API**（管理流水线的 HTTP 化）：把每一步 CLI 变成一个端点。
// **不放 domain/**：它要起 socket、连元数据库、读写声明文件。
// ⛔ 也不放进 `server/`（只读）或 `query/`（取数鉴权模型完全不同）：
// 本模块只有 import → review → apply → freeze 这条**既有流程**，
// 没有「直接改对象」的端点（需求风险 1）。见 `src/console/mod.rs` 模块头。
pub mod console;

// ⭐ REQ-126 · **客户门户**（租户锁死、只能问预置问题）。
// **不放 domain/**：它要起 socket、连源库、读三份声明与元数据库。
// ⛔ 也不放进 `console/`：控制面是「admin token + 能写」，门户是
// 「门户口令 + 只读 + 一个问题一个端点」；两者的**凭据、路由、权限面**都不同，
// 共一个模块会让「顺手加一个管理端点」变成一次编辑。见 `src/portal/mod.rs` 模块头。
pub mod portal;

// ⭐ REQ-135 · **生产可观测性**（`GET /metrics` 的 Prometheus 文本格式）。
// 为什么是独立模块而不是塞进三个 http.rs：指标登记表 / 渲染 / 校验只有一个写点，
// 三个服务共用同一份口径；而且它是纯计算（无 I/O、无业务数据），
// 判据（格式合法性、零泄漏、维度爆炸）可以脱离 socket 与数据库跑。
pub mod metrics;


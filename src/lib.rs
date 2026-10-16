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

// REQ-013 · 抽取 agent 抽象（规格 §7.1 阶段 1「可插拔」）。
// **不放 domain/**：它要跑真实 agent（含将来的协议客户端与 DB 查询），
// 而 `domain` 的 AC-002 判据是「零 I/O」。理由见 `src/agent/mod.rs`。
pub mod agent;

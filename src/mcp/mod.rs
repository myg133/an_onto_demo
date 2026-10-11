//! **MCP 服务器**（REQ-027）—— 把冻结快照暴露成标准 MCP 工具。
//!
//! # 目标（§9.5）
//!
//! > 审核通过后，Claude Desktop 经 MCP 连接，能回答至少 5 个客户级业务问题。
//!
//! 所以这里做的是**标准 MCP**，不是自造协议：客户端是真实的 MCP 客户端
//! （Claude Desktop 等），传输用 MCP 的默认 stdio。
//!
//! # 三层，与 ACP 那批同构
//!
//! | 层 | 文件 | 职责 | 依赖 |
//! | --- | --- | --- | --- |
//! | 编解码 | [`protocol`] | JSON-RPC 2.0 + MCP 形状，**纯函数** | 无 |
//! | 工具 | [`tools`] | 参数 → 调既有纯函数 → 结构化数据 + **守门器** | domain |
//! | 外壳 | [`serve`] | 冻结闸门 + stdio 行循环 | db
//!
//! 拆开的理由与 `src/agent/acp/` 一模一样：**协议能不能对，
//! 必须能用假字节判定**，而不是靠「真的起一个客户端」去撞。
//!
//! # 为什么**不进 `src/domain/`**
//!
//! AC-002 判定 `src/domain/` 零 I/O，而这一层要查库、起进程。
//! 把协议编解码塞进 domain 会让那条纪律形同虚设。

pub mod protocol;
pub mod serve;
pub mod tools;

pub use serve::{prepare, serve, ServerArgs};
//! REQ-111 · **本地审核界面** —— 在 HTML 里做双轨审核。
//!
//! # 为什么是一个**独立模块**，而不是 `server/` 里的一个文件
//!
//! `src/server/` 的「只读」不是一句口号，它有三层结构保证（见
//! `src/server/mod.rs` 模块头），其中第二层是**接线层**：
//! `tests/test_graph_server.rs::server_has_no_write_path_to_the_ontology`
//! 逐个扫 `src/server/{mod,http,page}.rs`，断言那里**不出现**
//! `set_decision` / `apply_proposal` / `freeze` / `UPDATE ` 等写入口。
//!
//! 审核界面必须**写**决策。把它的代码放进 `src/server/`，那个守卫要么变红，
//! 要么得把它改成「只扫某几个文件」—— 后者等于**让守卫为真而事实为假**：
//! 「`server/` 只读」这句话就不再成立，只是没人检查而已。
//!
//! ⇒ 独立成 `src/review_ui/`：`serve` 的路由、middleware、`AppState`
//! **一个字节都不动**（AC-008），而「只读的是 `serve`」这句话仍然由结构保证。
//!
//! # 与 `serve` 的分工（需求的第一句就是这张表）
//!
//! | `serve` | `review serve` |
//! | --- | --- |
//! | 只读：任何非 GET ⇒ 405 | 可写：**唯一**写点 `POST /api/decide` |
//! | 面向客户/业务看结果 | 面向 FDE 做审核 |
//! | 只服务**已冻结**版本 | 只服务**未冻结**的提案（已冻结 ⇒ 启动即失败） |
//!
//! # 内容为什么不过客户发布闸
//!
//! 这里返回的是**要被裁决的内容**：建模 `key`、列名、基数、置信度、AI 理由、
//! 证据。客户视图的守门器（`mcp::tools::guard`）把这些词当**泄露**；
//! 在这一轨它们是**审核对象**。这与 `review show`（CLI 的建模轨 D2–D8）
//! 是同一条已裁定的口径 —— 轨道由 [`crate::domain::pipeline::OUTLETS`]
//! 里的 R1–R5 **显式声明**为 `Internal`，不是「因为它没调闸所以它没闸」。
//!
//! # 安全边界（三条，缺一条都不算「关住」）
//!
//! 1. **只绑回环**：非回环地址 ⇒ 启动失败（与 `serve` 同一条闸，理由更重 ——
//!    这个进程**会写**）；
//! 2. **每个 `/api/*` 都要 Bearer 口令**：口令只从 `--token` 或
//!    `ONTOK_VIEW_TOKEN` 来，没有默认值、没有开发模式；
//! 3. **页面壳零数据**：`/` 免口令（浏览器导航带不了 `Authorization`），
//!    壳里没有项目名 / 条目 / 决策 —— 一切数据都走 gated API。
//!
//! # 已知限制（写在这里，不靠「没人发现」）
//!
//! - 单机单人：口令是静态共享令牌，无过期 / 无吊销 / 不区分谁签的（与 `serve` 一致，
//!   完整方案是 REQ-099）；
//! - 复核「同 item+track 连签」的终态靠**后写覆盖**（AC-005 明确要求），
//!   审计记录的是**最后一次**改判的 `decided_at` / `via`，不是完整历史；
//! - 批量接受没有服务端批接口：页面把预览拿到的 `items[]` 逐个 POST ——
//!   这样「一次请求改一批」在 HTTP 面上**不存在**，「一键全库」也就表达不出来。

pub mod http;
pub mod page;

/// 起审核界面所需的参数（`cli` 解析完之后交给 [`http::serve`]）。
pub struct ReviewArgs {
    pub project: String,
    pub proposal: String,
    /// 必须落在回环地址上 —— 见 [`http::serve`] 的闸门。
    pub bind: std::net::SocketAddr,
    /// 访问口令（至少 8 个字符）。
    pub token: String,
}

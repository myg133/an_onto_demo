//! 领域逻辑层 —— 纯函数，**零 I/O**。
//!
//! 验收 AC-002：`grep -rn 'sqlx\|PgStore\|db::' src/domain/` 必须无输出。

pub mod apply;
// REQ-010 · 业务概念组（§7.1 阶段 3「跨组合成」的落地）。纯逻辑，零 I/O。
pub mod concept;
pub mod diff;
pub mod hierarchy;
// REQ-006 · 客户视图 → 单文件 HTML（纯字符串拼装，零 I/O）
pub mod html_view;
// REQ-029 · 源库标识符的**形态判据** —— 页面层与 MCP 层**共用的一份**（纯文本，零 I/O）
pub mod identifier_shape;
// REQ-010 · 业务总览（按概念出总览 + 下钻 + 关系重定向 + 同名属性去重）
pub mod overview;
// REQ-055 · **建模轨投影（FDE 视角）**。与客户视图 `view` **并列**的第二个投影 ——
// 刻意不写进 `view.rs`：那里是建模术语黑名单的所在地，
// 把「保留建模术语的那一份」放进去会让两者的分离变得不可核对。
pub mod model_view;
pub mod review;
// REQ-056 · **提案条目的建模投影（FDE 通道）**。同样**刻意不写进 `view.rs`** ——
// 理由与 `model_view` 一致：那里是建模术语黑名单的所在地。
// 与 `model_view` 的区别在**时序**：那份收已落库的快照，这一份收**裁决时刻的
// 提案条目**（apply 之前），两者不是同一个对象。
pub mod review_view;
pub mod snapshot;
pub mod view;

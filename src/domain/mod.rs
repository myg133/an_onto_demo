//! 领域逻辑层 —— 纯函数，**零 I/O**。
//!
//! # 验收判据（口径已按实测修正，别再写回「必须无输出」）
//!
//! ⚠️ **「`grep -rn 'sqlx\|PgStore\|db::' src/domain/` 必须无输出」这句话
//! 从写下起就不成立。** 基线 `0455523` 实测 **9 处命中，全部在注释里**
//! （`apply.rs` 1 / `snapshot.rs` 2 / `mod.rs` 1 / `review_view.rs` 5）——
//! 那几个文件正是在注释里**描述这条规则本身**。
//!
//! 现在的判据是两条**同时**成立，由 `tests/test_graph_server.rs::domain_layer_stays_io_free` 执行：
//!
//! 1. **剥掉注释后命中 = 0** —— 这才是「零 I/O」本意要表达的事；
//! 2. **连注释一起算命中 = 9 行，不增不减**（基线常量 `BASELINE_COMMENT_HITS`）。
//!
//! 只判第 1 条会漏掉「有人往注释里写了新的存储层引用」；
//! 只判第 2 条会被注释里的既有写法永久卡死。两条缺一不可。

pub mod apply;
// REQ-010 · 业务概念组（§7.1 阶段 3「跨组合成」的落地）。纯逻辑，零 I/O。
pub mod concept;
pub mod diff;
pub mod hierarchy;
// REQ-006 · 客户视图 → 单文件 HTML（纯字符串拼装，零 I/O）
pub mod html_view;
// REQ-093 · 客户视图 → **图结构**（给只读图服务的数据面；纯投影，零 I/O）。
// **刻意不写进 `view.rs`** —— 理由与 `model_view` 同构：那里是客户视图的真身，
// 本模块是它的**第二个消费者**，不是它的改造。
pub mod graph;
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

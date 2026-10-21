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
// REQ-115 · **建议层**：从关系拓扑算候选分组（`concept suggest`）。
// 与 `concept` 分开放，是为了让「权威（人签过的概念组）」与「建议（算法算的）」
// 在**文件层面**也分得开 —— 图上只显示前者（R1）。
// 纯函数：不读库、不读时钟、无随机数 ⇒ 同输入同输出（AC-004）。
pub mod concept_suggest;
// REQ-108 · **证据评分卡 v1**（5 信号 + 8 条 veto + 四档）。
// 刻意是纯函数：AC-005「两次运行分数逐位相同」靠的是**结构**（这里读不到库、读不到时钟）。
pub mod confidence;
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
// REQ-107 · **出口收口**：客户类出口的唯一管线（前处理 → 发布闸 → 可写出的产物）。
// 出口只声明「格式 + 轨道 + 闸粒度」，判据只在本模块里被**调用**（不复制）。
pub mod pipeline;
// REQ-104 · 客户视图的**属性级剔除**（出口层动作；判据在 identifier_shape，不在本文件）
pub mod property_filter;
pub mod review;
// REQ-056 · **提案条目的建模投影（FDE 通道）**。同样**刻意不写进 `view.rs`** ——
// 理由与 `model_view` 一致：那里是建模术语黑名单的所在地。
// 与 `model_view` 的区别在**时序**：那份收已落库的快照，这一份收**裁决时刻的
// 提案条目**（apply 之前），两者不是同一个对象。
pub mod review_view;
pub mod snapshot;
// REQ-110 · **Turtle（OWL）序列化** —— 纯函数、零依赖、确定性。
// 两种投影（客户视图 / 建模视图）各一个入口，共用同一套文本原语；
// 为什么不写进 `view.rs`：那是客户视图真身，这里是它的**第三个消费者**
// （与 `graph` / `model_view` 同一条理由）。
pub mod ttl;
pub mod view;

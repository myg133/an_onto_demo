pub mod cardinality;
// REQ-109 · AI 语义通道（指纹 / 缓存抽象 / 协议校验 / 预算）。
// **不放 domain/**：它要打 HTTP（经 `agent::llm` 的 transport）—— 与 `sample` 同一条理由。
pub mod ai_semantic;
// REQ-116 · 文档摄取与断言抽取（第一步：文档 → **带证据**的断言）。
// **不放 domain/**：它要打 HTTP，理由同上；缓存与传输都是注入的 trait，
// 本模块零磁盘 I/O —— 读写三份产物由 `cli::doc_cmd` 负责。
pub mod doc_ingest;
// REQ-113 · 排除表（**一个实现**：解析 / 校验 / 剔除 / 记录都在这一个文件里）。
// 不放 `domain/`：它判的是「这一轮抽取的作用域」，与 `extract::propose` 同层。
pub mod exclusions;
pub mod introspect;
pub mod names;
pub mod propose;
// REQ-108 · **只读、确定性、不落原值**的关系采样器（AC-002 / AC-003）。
// 与 `introspect` 共用同一条只读池，不另开连接策略。
pub mod sample;

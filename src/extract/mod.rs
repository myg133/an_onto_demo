pub mod cardinality;
// REQ-109 · AI 语义通道（指纹 / 缓存抽象 / 协议校验 / 预算）。
// **不放 domain/**：它要打 HTTP（经 `agent::llm` 的 transport）—— 与 `sample` 同一条理由。
pub mod ai_semantic;
pub mod introspect;
pub mod names;
pub mod propose;
// REQ-108 · **只读、确定性、不落原值**的关系采样器（AC-002 / AC-003）。
// 与 `introspect` 共用同一条只读池，不另开连接策略。
pub mod sample;

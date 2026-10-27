pub mod cardinality;
// REQ-109 · AI 语义通道（指纹 / 缓存抽象 / 协议校验 / 预算）。
// **不放 domain/**：它要打 HTTP（经 `agent::llm` 的 transport）—— 与 `sample` 同一条理由。
pub mod ai_semantic;
// REQ-116 · 文档摄取与断言抽取（第一步：文档 → **带证据**的断言）。
// **不放 domain/**：它要打 HTTP，理由同上；缓存与传输都是注入的 trait，
// 本模块零磁盘 I/O —— 读写三份产物由 `cli::doc_cmd` 负责。
pub mod doc_ingest;
// ⭐ REQ-147 · `doc ingest --agent acp`：把「一块一块地问模型」外包给 ACP 子进程。
// 独立成文件：它是 `doc_ingest` 的**另一个搬运工**（编排 / 证据校验 / 指纹仍只有
// `doc_ingest` 那一份实现），所以不许把判据搬进来。
pub mod doc_acp;
// ⭐ REQ-129 · **文档断言 → 本体提案**的机械合并（`ontok doc merge`）。
// 独立成文件：`doc_ingest` 是**产出方**（本 REQ 不改它的语义），合并规则是
// 消费方的判据；放在一起会让「证据校验的唯一实现」与「合并规则的唯一实现」混成一份。
// 零 I/O：输入是字符串 + `Value`，落盘由 `cli::doc_merge_cmd` 负责。
pub mod doc_merge;
// ⭐ REQ-170 · **库结构摘要**（`schema.json` → 进 prompt 的那一小块 + `maps_to` 的校验面）。
// 独立成文件：`doc_merge` 的 `SchemaIndex` 只服务合并（表 → 列名集合，没有中文名、
// 没有选表打分、也不该有 prompt 文案）；摘要与选表是**产出方**的事，放一起会让
// 「合并规则的唯一实现」和「给模型的摘要」混成一份。
pub mod schema_digest;
// REQ-113 · 排除表（**一个实现**：解析 / 校验 / 剔除 / 记录都在这一个文件里）。
// 不放 `domain/`：它判的是「这一轮抽取的作用域」，与 `extract::propose` 同层。
pub mod exclusions;
// ⭐ REQ-118 · **AI 提案框架**（ADR-005）：提案形状 / 校验 / 指纹缓存 / 预算 / 渲染。
// **三命令共用这一处**（纪律 H）；不放 `domain/`：缓存与传输是注入的 trait，
// 而真正落盘由 `cli` 负责 —— 与 `doc_ingest` 同一条分层理由。
pub mod proposal;
// REQ-118 · 决策点一：中文名提案（`ontok name suggest`）。
pub mod name_suggest;
// REQ-118 · 决策点二：排除提案（`ontok exclude suggest`）。
pub mod exclude_suggest;
// REQ-118 · 决策点三：AI 语义分组（`ontok concept suggest --ai`）。
pub mod ai_group;
pub mod introspect;
pub mod names;
pub mod propose;
// REQ-108 · **只读、确定性、不落原值**的关系采样器（AC-002 / AC-003）。
// 与 `introspect` 共用同一条只读池，不另开连接策略。
pub mod sample;
// ⭐ REQ-120 · **FK 边补全**（候选提名 + 值域重叠判决 + ADR-005 提案 + 签字注入）。
// 为什么独立成文件：REQ-084 用 `tests/retained_names.req084.tsv` 把
// `propose.rs` 里 130 处真实表名的**行号**钉死 —— 在 `propose.rs` 里增删一行，
// 那些行号整批平移、门禁立刻变红（REQ-087 的 `cardinality.rs` 是同一条理由）。
// 采样复用 `sample` 的只读池与两个 SQL 构造器，不另开连接策略。
pub mod fk;

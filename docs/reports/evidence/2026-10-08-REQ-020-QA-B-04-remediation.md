# QA-B · 处置建议（含受影响测试文件清单）

## 5.1 方案成本对比（我 grep 出的真实影响面，不是估计）

前提事实：
- 生产代码里 `PgProposalStore::create` 只有 **2 个**调用点：
  `cli.rs:313`（抽取路径）与 `cli.rs:447`（import 路径）。
- 测试里 `create` 共 **41 处**，分布：
  `tests/test_apply.rs` 25 · `tests/test_proposal.rs` 9 · `tests/test_project.rs` 4 ·
  `tests/test_concept_proposal.rs` 3。
- `extracted_by` 在测试中共 **152 处**，跨 **12 个文件**：
  `test_apply.rs`(55) `test_concept_proposal.rs`(31) `test_snapshot.rs`(19)
  `test_proposal.rs`(15) `test_agent_orchestration.rs`(12) `test_concept_store.rs`(8)
  `test_extraction_agent.rs`(5) `test_extract_cli_req020.rs`(2)
  `test_extract_cli_req019.rs`(2) `test_concept_transition.rs`(1) `test_overview.rs`(1)
  `test_view.rs`(1)

### 方案 (a) 接受现状 + 写文档 —— ✅ 推荐现在做

**代价**：新增一份 ADR（如 `ADR-003-proposal-self-declaration-trust.md`），
在 `cli.rs:119` 与 `propose import --help` 补一句信任边界说明。
**打破的测试**：**0 个**（纯文档；`--help` 文案无测试断言，我已 grep 确认
`propose import` 无 CLI 级测试）。
**局限**：不阻止，只声明。对 `failed_groups` 伪造这种真危害点**没有实际防护**。

### 方案 (b) `extracted_by.source` 留痕 —— ✅ 推荐 RN-2 做

**做法**：给 `domain::snapshot::ExtractedBy`（`snapshot.rs:30-40`）加
`source: Option<ProvenanceSource>`，区分「工具自产」/「外部声明」；
`apply.rs:260-263` 一并落库。

**代价**：
- **打破的测试**：`test_snapshot.rs`（19 处 `ExtractedBy` 字面量构造，
  `snapshot.rs:31` 那个 `ExtractedBy { … }` 结构体字面量会编译失败）、
  `test_apply.rs`（55 处，多数是 `extracted_by: None` 不受影响，
  但 `test_extracted_by_is_not_inherited_on_apply`（`:251`）等断言**逐字节比对**
  `ExtractedBy` 相等，**必须同步改**）、
  `test_concept_proposal.rs`（`:406` `test_extracted_by_follows_proposal_not_base`
  断言 `assert_ne!`/`assert_eq!` 逐字段比，**必须同步改**）、
  `test_proposal.rs:235`（`Some(eb)` 往返）、`test_agent_orchestration.rs`（12 处）。
- 注意 `extract/propose.rs:70` 那个**同名不同型**的 `ExtractedBy` **已有 `source` 字段**
  （`:80`）—— 命名重复本身就是隐患，改名要一并处理。
- 需要 DB 侧回填（`pg.rs:201-204` 已有 `ALTER TABLE … ADD COLUMN IF NOT EXISTS` 先例）。

### 方案 (c) 签名 / 登记哈希 —— ⚠️ 现在做性价比差

**代价**：需要密钥管理、密钥分发、轮换机制；本项目当前**无任何签名基建**。
且 `registry.rs:84-90` 的 `is_vague_version` 只挡 "latest" 这类**模糊**版本，
**挡不住 `"9.9.9-fake"` 这种格式良好但内容虚假的值** —— 我实测的 PoC 正是后者。
⇒ 签名解决的是「谁发的」，**不解决「内容对不对」**，与本问题的失配度高。
建议**推迟**，等真有「多主体异步提案」需求再上。

### 方案 (d) 收窄 `PgProposalStore::create` 可见性 —— ❌ 不要单独做

**代价**：`lib.rs:9` 是 `pub mod db`，收窄成 `pub(crate)` 会**打断全部 41 处测试调用**
（`tests/*.rs` 全部从 crate 外调用）。
更关键：**它挡不住本问题** —— `cli.rs:447` 的 import 路径**在 crate 内**，
收窄可见性对它零效果。

### 我的推荐组合

**RN-2 做 (b)，同时做 (a) 的文档部分；不单独做 (d)。**

**并且补一条 Dev 没提、但证据更强的发现** ↓

## 5.2 ★ 额外发现：同一仓库里已有更好的范式，`propose import` 是唯一漏网的

`concept import`（`cli.rs:278-330`）对同类问题**有防护**：

- 概念声明带 `version` + `history[].content_digest`（`domain/concept.rs:265-290`）
- `decl.verify()`（`:352-370`）**fail-closed** 校验「改内容必须改版本」
- `cli.rs:298-299` 抽取来源取自 `decl.declared_by` + `decl.version`，
  注释明写「**不是「我」** —— 声明是谁做的就记谁」

**实测对照**（我构造伪造 `declared_by` + 瞎编 digest）：

```
$ ontok concept import --file concept_fake.json --project qa-req020b-demo
✗ 改内容必须改版本：当前内容指纹 51b8689d… 与版本 9.9.9 记录的不一致
exit=1                                    ← 被挡住了
```

而同一个「伪造溯源」动作在 `propose import` 上：**exit=0，静默入库**。

⇒ **这才是最强的处置论据**：项目**已经**在同一文件里确立了正确范式
（内容指纹 + fail-closed + 「声明是谁做的就记谁」），
`propose import` 只是**没跟上**。
所以 (b) 不该定位成「新增安全机制」，而该定位成
**「把 `concept import` 已有的纪律补齐到 `propose import`」** —— 这是内部一致性论证，不是外部安全论证。

**但要说清一个诚实的边界**：`content_digest` 挡的是「改了内容不升版本」，
**不是**「声明本身就是假的」。真去重算一个匹配自己内容的 digest 仍然能编。
所以它是**降低误报率、让「随手改」变难**，不是**密码学保证**。
若 BA 要更强的保证，才需要 (c)。

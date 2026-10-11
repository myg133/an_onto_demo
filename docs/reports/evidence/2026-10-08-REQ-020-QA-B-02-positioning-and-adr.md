# QA-B · import 的定位 与 ADR-001 的职责边界

## 2 · `propose import` 存在的意义（找证据，不凭印象）

### 2.1 代码里的定位原文

`src/cli.rs:117-124`：

```rust
#[derive(Subcommand)]
pub enum ProposeCmd {
    /// 从 JSON 文件导入提案      ← 全部文档注释就这一句
    Import { #[arg(long)] file: PathBuf },
}
```

CLI `--help` 原文（实测）：

```
$ ontok propose --help
提案
Commands:
  import  从 JSON 文件导入提案

$ ontok propose import --help
从 JSON 文件导入提案
Usage: ontok propose import --file <FILE>
```

**判定**：定位语只有「从 JSON 文件导入提案」。
- **没有**说「导入外部 agent 离线产出的提案」；
- **没有**说这是受信任输入；
- **没有**任何信任边界 / 真实性声明。
- 对比：同文件 `ConceptCmd::Import`（`cli.rs:60`）反而有一句实质定位
  「把一份**概念声明**导入成提案（走双轨审核，不直接生效）」——
  即 `propose import` 是全 CLI 里**注释最薄**的一条写路径。

### 2.2 规格怎么说

`grep "propose import|导入提案|proposal.json" BA/docs/design/ontology-platform-spec.md` → **零命中**。
规格 §8.4（1132-1190 行）全文只讲 `extracted_by` 的**语义用途**，
**从未提及** `propose import` 这个命令，也从未讨论「谁来保证它不是编的」。

### 2.3 `fixtures/proposal-demo.json` 是给谁用的

`src/extract/propose.rs:114-117` 明确了它是对齐基准：

> 「完整提案 —— **顶层形状与 REQ-002 的 `fixtures/proposal-demo.json` 一致**：
> `project` / `base_version` / `round` / `extracted_by` / `items`。
> ⚠️ 改这里必须同步改那份 fixture，否则 `ontok propose import` 直接报「提案缺 project」。」

⇒ **双向绑定**：`propose::Proposal` 的形状照它，`propose import` 的入参也吃它。
实测：把它改个 project 名就能原样导入成功（6 个条目 + `demo-extractor 0.1.0`）。

**所以 import 的真实定位是：工具自产提案 JSON 的「回灌口」**（pipeline
`introspect → ontok-propose → proposal.json → propose import → 库`），
**不是**为「不可信的外部 agent 产物」设计的入口。

### 2.4 对照实验：接口**在结构上无法**区分真假

| 文件 | `extracted_by.agent` | 来源 | import 结果 |
| --- | --- | --- | --- |
| `fixtures/proposal-demo.json` | `demo-extractor` / `0.1.0` | 仓库里现成的示例 | ✅ 接受 |
| 我构造的 | `totally-not-a-real-agent` / `9.9.9-fake` | 我编的 | ✅ 接受 |

两者在库里长得**一模一样**（实测 SELECT 已证）。
⇒ 该接口只有**结构校验**（`cli.rs:437` 的 `from_value::<ExtractedBy>`），
**没有任何真实性信号**可供判别 —— 这不是「守卫漏了一处」，而是**根本没有守卫**。

## 3 · ADR-001 的职责边界

ADR-001 全文 74 行，我逐行核过。它规定的是**取值口径**：

> 「`apply(proposal) → new_version` 时，`new_version.extracted_by` 原样取提案的 `extracted_by`。
> 提案没有该字段时记 `null`（表示人工修订）。」

后果第 3 条：「`apply` 不得推断、不得回退到 base 的值 —— **不猜**是本决策的核心」。

**ADR-001 有没有回答「谁来保证这个值不是编的」？—— 没有，一个字都没有。**
全文检索：无「谁保证」「可信」「校验」「真实」「来源不可伪造」等任何相关表述。

### 3.1 缺口归谁：ADR-001 的职责范围内 vs 范围外

我的判断：**这不在 ADR-001 的职责范围内，是本就不该由 ADR-001 承担；
但它是一个真实且无人认领的架构缺口。**

理由：

1. **ADR-001 的问题陈述是「取值」，不是「可信性」**。
   它的「背景」三场景（A 人工 / B 新一轮 / C 混合）全部在讨论
   *apply 该抄哪个值*，而不是 *这个值可不可信*。它把 `extracted_by`
   当成一个**给定的输入**。对给定的输入规定输出，这是它的本职，完成得很好。

2. **它明确拒绝做「判断」，而这恰恰是对的**。
   ADR-001 理由表最后一行：「按场景分支」被否，理由是
   「需要判断『这条提案是抽的还是人写的』—— 而**提案自己知道**，不需要我们猜」。
   ⇒ ADR-001 **刻意**把「这条提案声称自己来自谁」当作**提案的自我声明**。
   按它自己的逻辑，import 信任文件内容是**逻辑上自洽的**，
   不是对 ADR-001 的违背。

3. **但 ADR-001 的这句「提案自己知道」隐含了一个它没写的前提**：
   提案的自我声明**是可信的**。这个前提在整个代码库里**无人验证**。
   ADR-001 没写它，规格没写它，import 也没写它 —— 三处都默认了它。
   **这是架构缺口，但缺口的归属是「没人认领」，不是「ADR-001 答错了」。**

⇒ 结论：ADR-001 **不需要修改**（改了反而越界）。
需要的是一份**新的**决策文档，把「提案自我声明的可信度由谁担保」写清楚。

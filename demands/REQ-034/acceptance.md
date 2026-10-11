# 验收标准：REQ-034 RN-5 内置 LLM agent 作为 fallback

> 依据 REQ-013/RN-1 建成的 agent 抽象层 + 2026-10-09 负责人提供可用通道写成。
> 基线：`171d2a0`（develop）。
>
> **这条从本会话开始就一直挂着** —— 之前 `config.yaml` 无 apiKey、`llm-call` 401。
> 通道现已可用。

---

## 背景：现状（BA 已逐项核实，不凭记忆）

| 项 | 现状 |
| --- | --- |
| 抽象层 | `src/agent/mod.rs:325` `pub trait ExtractionAgent` —— **已存在**（REQ-013 建） |
| 工厂 | `src/agent/factory.rs:78` `build_agent(registration, names, round)` 按 `registration.protocol` 分发 —— **已存在** |
| 现有实现 | **两个**：`rule_based.rs`（`inproc`，REQ-013）、`acp/`（`endpoint`，REQ-023/024） |
| LLM 实现 | **零** —— `src/agent/` 下没有任何 LLM 代码 |
| 成本类型 | `mod.rs:275` `MoneyAmount{amount, currency}`；`mod.rs:260` **`used` 单位由协议自己定**（ACP 是 token），类型上禁止「拿 token 上限比请求数」这种错配 |
| 编排闸门 | `max_cost` / `max_used` **两道独立**（REQ-015/022 建） |
| 端点来源 | 工厂从**登记的 `endpoint` 字段**取、不写死（ACP 的既定做法，`factory.rs:100+`） |

⇒ **本需求是「在既有抽象上加第三个实现」，不是「另起一套」。**

---

## ⚠️ 通道实测记录（2026-10-09，BA 实测）

负责人提供了 **OpenAI 兼容**端点。**base_url / key / model 属敏感信息，
只经环境变量传递，未落盘、未回显、未入库、未进 commit message。**

| 检查 | 结果 |
| --- | --- |
| `GET /v1/models` | **HTTP 200，11 ms** |
| `POST /v1/chat/completions`（`max_tokens=300`） | **HTTP 200**，`finish_reason: stop` |

### 🔴 两个必须写进实现的坑（实测，非推断）

1. **该 model 是推理模型，`content` 可能为空。**
   `max_tokens` 给小时得到 `content: null` + `finish_reason: length`。
   实测 `max_tokens=20` 时正文为空、20 个 token 全进 `reasoning_content`；
   `max_tokens=300` 才拿到正文。
   ⇒ **实现必须处理「正文为空」这个形态**，不能假定 `content` 一定有值。

2. **成本必须按 `reasoning_tokens` 算。**
   实测 `reasoning_tokens: 59` / `completion_tokens: 60`
   ⇒ **98% 的 token 花在推理上，正文只有 1 个**。
   若只计 `content` 对应的 token，成本会**严重低估**。
   ⇒ `used` 的单位与算法**必须在实现里写明**，并与 `max_used` 的语义对齐
   （`mod.rs:260` 已规定「`used` 单位由协议自己定，但两边必须声明同一个单位」）。

---

## AC-001 · 作为第三个实现接入既有抽象

- 预期结果：`build_agent` 能按 `protocol = "llm"`（或你选的名字）造出实现，
  **不改** `trait ExtractionAgent` 的既有方法签名
- 类型：架构
- **不要求**改抽象层本身。若你认为必须改，**先说明理由**再动手

## AC-002 · 端点与凭据只从登记/环境变量来，绝不写死

- 预期结果：
  - `endpoint`（base_url）从**登记的 `endpoint` 字段**取，沿用 ACP 的既定做法
  - api key / model 从**环境变量**取，**不入库、不进日志、不进报告、不进 commit**
- 类型：架构
- **理由**：端点在哪、带什么参数是**部署事实**，写死等于「换个环境就跑不起来，
  而报错会指向代码而不是环境」

## AC-003 · 正确处理推理模型的两个坑

- 预期结果：
  1. `content` 为空或 `finish_reason == "length"` 时**不当作成功**
  2. token 计量**包含 `reasoning_tokens`**
- 类型：功能
- **判别力自证（必做）**：构造「正文为空」的响应，验证实现**不当成成功**；
  构造一个 `reasoning_tokens` 占比极高的响应，验证报出的 `used` **包含**它。
  每条给**二进制名 + 运行条数**。

## AC-004 · 成本闸门能拦住

- 预期结果：把 `max_used` 调到低于本次调用的实际用量 ⇒ **该组失败**而非静默超额
- 类型：功能
- ⚠️ 与 `mod.rs:280` 的「两道闸各自独立」语义一致：只设 `max_used` 也是一道闸

## AC-005 · fallback 语义说清楚（**设计判断，Dev 要给理由**）

RN-5 说的是「作为 **fallback**」。**你必须明确回答**：

- 什么条件下走 LLM？主 agent（rule_based / ACP）失败时？还是显式配置？
- 「失败」指传输层 `Err`？还是**内容类缺口**？
  （⚠️ 内容类缺口是**事后核对**出来的，`REQ-025` 已登记「BA 倾向不补追加重试」，
  别顺手把两件事混成一件）
- fallback 会**再花一次钱**（LLM 调用本身要计费）——
  这与成本闸门怎么协同？
- 类型：架构
- **验收判据是「说清楚且自洽」，不是「某个特定设计」**。
  但若你的设计与既有抽象冲突，**指出来**。

## AC-006 · 全量与红线

- 至少 1 轮完整 `cargo test --no-fail-fast`
- 预期结果：**539 passed / 0 failed / 1 ignored**（REQ-048 之后的基线）
- **建议同会话基线对照**；耗时**别声称精度**（共享集群负载浮动）
- 红线：`tests/drop_rate.rs`、`tests/test_fk_naming.rs`、`src/domain/view.rs` 零改动；
  `test_humanize_still_refuses_unknown_columns`（`src/domain/view.rs:1254`）一条断言都没放宽；
  **无测试被删除或 `#[ignore]`**
- 产物 sha **不变**（本需求不碰抽取逻辑）：
  - `stats.json` = `cda6a9a2a007fcabe2be602a9962b66deda84498e0fb1fb8232d0627632cd7ba`
  - `proposal.json`（`--project extracted`）= `1eef95414b8eec084b12b09b8eb89d6347c64f45476447ec243e5dd66c5c5635`
  - 命令实属 **`ontok-propose`**；`--out` 在 `--stats` 之前；**不传 `--names`**

---

## 环境事实

- 共享集群 DB 很慢；本机**没有 `psql`、没有 `psycopg`** ⇒ **测不了 `SELECT 1` p50**
- 报告把「代码失败」与「环境类失败」**分开写**；复现率随负载浮动，**不声称稳定值**
- ⚠️ **`.env` 在仓库根**（各 worktree 内都没有），库名 `notol` 需替换成 `ontol`
- 共享本体库**只读**（仅 `information_schema` / `pg_catalog`，零业务数据行）；
  **禁 DELETE / TRUNCATE / 清空表**
- 口令只走环境变量，**绝不写进代码、文件、日志、报告或 commit**；
  ⚠️ **不要 `echo` 连接串**（本会话已有 Dev 与 QA 各踩过一次口令回显的坑）
- ⚠️ **绝不要 `ln -s` 指向别的 target** —— 符号链接让两个 worktree 共享构建产物，
  本会话已因此连续踩坑两次
- ⚠️ **不要与另一个全量测试并发跑** —— 本会话已发生两次全量互相拖垮
  （一次把对方楔死 2124s，一次把自己中断在 492 条）

## 红线清单

| 文件 / 约束 | 说明 |
| --- | --- |
| `tests/drop_rate.rs` | **禁改** |
| `tests/test_fk_naming.rs` | **禁改** |
| `src/domain/view.rs` | **禁改** |
| `test_humanize_still_refuses_unknown_columns` | 一条断言都不许放宽 |
| 任何测试 | **不许删除、不许 `#[ignore]`** |
| `test_backfill_…` | **不许放松任何一条断言** |
| **既有 agent 抽象的 trait 签名** | **不许改**（除非 AC-001 里先说明理由） |
| `.review-workspace/fix-props/schema.json` | **绝不可删** |

## 交付纪律

- **绝对禁止** `CARGO_TARGET_DIR` / `--release` / 另建 probe crate
- **求和不要用 `paste -sd+`**（逐行输出 ⇒ 内容重复即精确翻倍）
- **量化数字必须标口径与命令**（本会话已 6 次因数字没标口径引发返工）
- **编译失败不算变异执行**；**存活变异必须逐条论证**
- 临时文件只放 `.review-workspace/req034/`；**禁 `/tmp`、禁 home**
- **写含反引号的 Markdown 必须用 edit/write 工具，禁止用 shell**。
  （失败就用带引号 heredoc 兜底并登记偏离，**别反复重试**。）
- 核对文件用**裸 `grep`**。禁 `rm`（本环境已接管成 `mavis-trash`）
- 仓库**无 remote，不要 push**；不 squash、不 force、不改历史
- **BA 分支（`demand`）只由 BA 写** —— 你那个 worktree 的分支上没有 `BA/` 目录，
  **不需要也不应该**去改它；要改就只在报告里给建议文案
- ⚠️ **边做边把结果落盘** —— 本会话已有 **4 次子 agent 会话丢失**的先例

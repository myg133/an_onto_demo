# 验收标准：REQ-109 · AI 语义项接入

> **前置**：REQ-108 已合并（`confidence` / 自证链 / `semantic_fit` 权重位）
> **本机限制**：无 LLM 凭据 ⇒ 所有 AC 必须能用 **scripted transport** 跑通；真实模型只在有 key 时手验

---

## 一 · AC

### AC-001 · AI 关闭时**行为完全不变**

- 默认（不加 `--llm`）⇒ 产物与 REQ-108 **逐字节相同**
- 判据：同一 schema 两次运行，`proposal.json` / `stats.json` / `pending` / `rejected` 四份 sha256 与 REQ-108 基线一致
- ⛔ 不许「顺手改一下权重」——`semantic_fit` 缺失时归一化结果必须与 REQ-108 逐位相同

### AC-002 · 无 key 且开 AI ⇒ **报错退出**

- `--llm` 但缺 `ONTOK_LLM_BASE_URL` / `ONTOK_LLM_API_KEY` / `ONTOK_LLM_MODEL` 任一 ⇒ **exit≠0**
- 文案必须点名缺哪个变量
- ⛔ **不许静默降级为「不跑 AI 继续抽」**——那会让产物看起来正常但其实没跑

### AC-003 · 缓存命中 ⇒ **不调用模型**

- 用 scripted transport（计数器）跑两次同一输入
- 判据：第二次调用次数 **= 0**，且 `ai.cache_hit == true`
- 判据：删除 `ai_cache` 表内容后第三次运行 ⇒ 调用次数 **= 1**（缓存真在起作用）

### AC-004 · 幂等：同输入两次运行产物逐字节相同

- 两次 CLI 运行（第二次命中缓存）⇒ 四份产物 sha256 相同
- 判据：`git diff --no-index` 无输出

### AC-005 · 改 prompt ⇒ 指纹变 ⇒ 必须重新调用

- 修改提示词模板（或 `--refresh`）⇒ `prompt_sha256` 变 ⇒ 缓存不命中 ⇒ 调用次数 +1
- 判据：故意把 prompt 改一个字，断言 `cache_hit == false` 且调用次数递增

### AC-006 · 预算闸

- AI 开启时 `--budget-tokens` **必填且 > 0**；缺省 ⇒ exit≠0（报错说明要用预算）
- 超预算 ⇒ **停止**并报告「已用 / 上限 / 停在第几组」
- 判据：把预算设成 1 token ⇒ 第一次调用后即停，且不产出半份可导入的提案（文件要么不存在，要么带 `importable: false`）

### AC-007 · `semantic_fit` **不能翻转 veto**

- 构造：0 命中 + AI 给 1.0 ⇒ 分数仍 `≤ 0.20`
- 构造：类型不兼容 + AI 给 1.0 ⇒ 仍 `≤ 0.15`
- 判据：这两条必须落「拒绝」

### AC-008 · `cites` 必须是真实坐标

- AI 返回的 `cites` 里每个 `表.列` 必须存在于本次 schema 中
- 编造一个 ⇒ 该条 AI 判断作废（记 `semantic_fit: absent`），并在报告里计数
- 判据：构造 scripted 返回含 `nonexistent.col` ⇒ 断言被丢弃且计数为 1

### AC-009 · `rationale` 必填且简短

- `rationale` 缺失 / 空 / 超 60 字 ⇒ 该条 AI 判断作废
- 判据：构造三种坏输入，断言全部作废且不污染分数

### AC-010 · 客户视图零泄漏

- `ontology show`（JSON/HTML）不得出现 `ai` / `semantic_fit` / `model` / `cache_hit` 任何字段
- `--model-view` 与审核界面**必须**有
- 判据：客户视图 grep = 0；model-view 里每条被 AI 判断过的关系都有

### AC-011 · 无 key 也能全绿

- 所有测试用 scripted transport；`cargo test --lib` / 相关 `--test` 在无任何 LLM 环境变量下全绿
- 判据：清空 `ONTOK_LLM_*` 后跑测试，0 failed

### AC-012 · 红线与不退化

| 项 | 约束 |
| --- | --- |
| ⛔ 禁改 | 四红线 + `tools.rs` + `guard()` + `property_filter.rs` G1–G4 |
| ⛔ 禁 | 删测试、`#[ignore]`、放宽断言、**新增依赖**（HTTP 走既有 transport） |
| 不许退化 | REQ-108 的 226 lib / 4 doc / 31 套件 300 passed |

---

## 二 · 纪律

- **G** 变异三元组；⛔ 编译失败不算执行
- **H** 不许写第二份判据（复用 `confidence.rs` 与 `agent/llm` 通道）
- **I** 每条命令自带 `cd` 或绝对路径
- **S** AC 自己也会是错的 —— 发现不成立直接说
- **密钥** ⛔ 任何文件、日志、错误信息里不得出现 API key

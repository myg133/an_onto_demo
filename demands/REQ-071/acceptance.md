# 验收标准：REQ-071 转义守卫不完备（`.clone()` 等黑名单外写法可绕过）

> 依据 REQ-069 的 QA 验收（判 **PASS 带 5 项非阻断**）写成。
> 基线：REQ-069 合并后的 develop（`c3ddcd5`）。
> **本条是 REQ-069 的核心守卫的一个洞。**

---

## 问题：守卫只认「白名单写法」，`blocked[].target` 改成 `.clone()` 就绕过

REQ-069 加了一条守卫
`test_req069_every_payload_string_read_goes_through_the_escape`：
枚举 payload 读取点，断言每个 `blocked[]` 项的 `.target` **是 `escape_payload_text(...)` 形态**。

**QA 的 EXP-A 证明它确实会红**（改 `.map()` 为裸 `to_string()` ⇒ 2 红）。

**但 QA 的下一个实验构造出绕过**：

把 `.target` 改成 **`.clone()`** —— 这是**黑名单之外的写法** ⇒
**7 个测试二进制、100 条测试全绿**，
而 QA 的探针同时证明 **注入确实被重新打开**（`STRUCTURAL` 由 false 变 true）。

⚠️ 依据本会话反复出现的形态：**恒假 ⇒ 永远绿 ⇒ 看起来有防护其实没有** ——
这里更隐蔽：守卫**不是恒真**，但**它的判别面是一个封闭的写法枚举**。

---

## AC-001 · 守卫必须判「**转义发生**」，不是判「**写法像**」

- 预期结果：守卫的判据改为「**该读取点的输出确实过了转义**」
  （例如：喂一个含 `\n` 的字符串进去，断言**返回值里没有裸 `\n`**）
- 类型：测试有效性
- ⚠️ **不要**再扩写法枚举 —— 那是 REQ-066 已经证明的
  **「有限枚举不可能穷尽」**，本项目已为此开了三条需求。
  **必须换判据维度。**

## AC-002 · 已转义的字段位必须有换行断言

QA 的实测：**18 个已接上转义的字段位里，有 7 位没有任何换行断言**
（`state` / `provenance.agent` / `evidence.table` / `evidence.column` /
`model_lines.bound` / `model_lines.mapping` / `extract.actor`）。

⇒ 那 7 位一旦被改回未转义，**测试不会响**。

- 预期结果：这 7 位**逐一**加换行断言
- ⚠️ **M2 是灵魂**：把其中**任意一位**改回未转义 ⇒ **必须变红**

### ⛔ 2026-10-11 撤回：BA 上面的 7 位名单**是错的**（Dev 否定，BA 认账）

BA 写的 `state` / `provenance.agent` / `evidence.table` / `evidence.column` /
`model_lines.bound` / `model_lines.mapping` / `extract.actor` ——
**基数对、成员错 4 / 漏 2 / 多 1 个不存在**。

Dev 的更正（待 QA 独立裁决）：
- `provenance.agent` **已有**换行断言；`extract.actor` 是它的**重复**
- `evidence.table` 语料**已有**
- `evidence.column` 与 `table` **共用一条转义调用点、不可分别变异**
- `model_lines.mapping` = B/C **两位都有**
- `model_lines.bound` 一名两指、指向了**已覆盖**的 I/J
- `state` 在本分支**不存在**这样的载荷字段（**是枚举**）
- 而 **S / T 完全没出现**

⇒ **正确答案是 L / M / N / O / P / S / T。**
⚠️ **本条由此产生的教训**：BA 手抄 QA 报告里的位号而**没有回原表核对** ——
「基数对、成员错」比「全错」更危险，因为它**看起来对**。

### ⛔ BA 的候选乙也被否定

BA 写过「或让守卫**与判据共用同一份实现**」。
Dev 指出那是**循环论证**：那会让测试拿 `escape_payload_text` 算期望值，
转义函数一被摘掉**两边一起错** ⇒ **守卫恰好在防护消失那一刻变绿**。

---

## ⛔ BA 的范围判断：本条**不做**

1. **不扩写法枚举**（见 AC-001）
2. **不动 `escape_payload_text` 本身** —— 它已被 REQ-069 验证正确
   （18 位全中、REQ-058 判据 15/15、本条 M1~M4 全杀）
3. **不修「真实二进制未端到端」** —— 那是独立的 **REQ-072**
4. 不碰红线；不加 `#[ignore]`；不删测试

## ⚠️ 派单坑

1. 改 `src/domain/review_view.rs` 与 `tests/test_review_view_req069.rs`
   —— 与 REQ-065/068/070 同一族文件，**必须等前面的合并完成后再派**，
   **不要并行开分支改同一文件**
2. 临时文件/报告写**仓库根** `.review-workspace/req071d/`
   （worktree 内 `.review-workspace/` 不被 gitignore）
3. **本条的价值在 AC-002，不在 AC-001。**
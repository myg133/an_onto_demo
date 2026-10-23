# 验收标准：REQ-128 · `--names` 必填

> **一句话**：不传翻译表就**不许跑**；传了占位表要**被提醒**。

## 一 · AC

### AC-001 · 不传 `--names` ⇒ 失败

- `ontok-propose` 不带 `--names` ⇒ **退出码非 0**
- stderr 里有可行动信息，且**必须包含**：
  - 「必须显式提供翻译表」
  - 「`table_names.json` 是脱敏占位样例」
  - 生成方式（`ontok-translate-names` 或 `ontok name suggest --as-names`）
- 判据：跑一次，贴退出码与 stderr 原文

### AC-002 · 显式传占位表 ⇒ 警告但不拒绝

- 传 `src/extract/table_names.json`（91 条 `t001→对象 001`）⇒ **继续运行**，但 stderr 有警告
- 检测规则：key 形如 `t\d{3}` 且 value 形如 `对象 \d{3}` 的比例 > 80% 时告警
- 判据：既验证「有警告」，也验证「确实跑完了」（不能因为警告而中止）

### AC-003 · 真实表 ⇒ 无警告

- 传一份正常的翻译表 ⇒ **无**占位警告
- 判据：stderr 不含该警告

### AC-004 · 既有测试不被放宽

- 依赖默认值的测试改为**显式传 `--names`**（用脱敏 fixture 或临时文件）
- ⛔ 不许 `#[ignore]`、不许删测试、不许放宽断言
- 判据：`cargo test --lib` 与相关集成测试全绿；失败的只剩基线既有的那几条

### AC-005 · 文档同步

- `ontok-propose --help` 里 `--names` 标注「必填」且说明为什么
- README / runbook 的示例命令都带 `--names`
- 判据：grep 检查

### AC-006 · 不退化 / 零依赖

| 项 | 约束 |
| --- | --- |
| ⛔ 禁改 | 四红线 + `tools.rs` + `guard()` + `property_filter.rs` |
| ⛔ 禁 | 新增依赖 |
| 基线 | 以同机 `develop` 干净检出实测为准 |

## 二 · 纪律

- **H** 参数解析只改一处（`ontok-propose.rs` 的默认值 + 校验）
- **G** 变异：把默认值加回去 / 去掉占位检测 ⇒ 每条必须被抓
- **S** AC 也会错，发现直接说

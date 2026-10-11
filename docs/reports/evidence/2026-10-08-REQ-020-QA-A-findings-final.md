# QA REQ-020a — 缺口 2 + 产物 + 红线 验证记录
worktree: feature-REQ-020 @ e5717a8（父 23485c0），worktree 全程干净

## 二、缺口 2 · 基版本冻结提前失败

### 1. latest_version 四个调用方 —— 属实（2 处行号是父提交口径）
| # | 父提交 | HEAD 实际 | 语义 | Dev 说法 |
|---|---|---|---|---|
| 1 | ontok-propose.rs:355 | **:376** | 本 REQ 加冻结校验处 | ✅（行号为父提交） |
| 2 | cli.rs:573 | :573 | apply 回显取 max seq | ✅ 精确 |
| 3 | test_project.rs:114 | :114 | `test_version_latest_picks_highest_seq` 断言 `l.seq == 3` | ✅ 精确 |
| 4 | test_extraction_agent.rs:1210 | **:1148** | 存在性检查 | ✅（行号为父提交） |

`src/db/project.rs:165` SQL：`ORDER BY seq DESC LIMIT 1`，WHERE 仅 `project_id`，
不过滤 `frozen_at` —— 冻结缺口客观存在。
（细节：#3 该测试本身未冻结任何版本，故加 `AND frozen_at IS NULL` 并不会**字面**改红它；
 Dev「语义是 max-seq、不是可提案性」的论证方向正确，只是那条测试不是必然被改红的那一个。）

### 2. 实测：确实省掉白跑（QA 独立复现，走真实二进制）
场景全部由 CLI 搭：`ontok project new` → `version new` ×2 → `version freeze --version 2`
（v2 = latest，已冻结）；输入 = fix-props/schema.json + 仓库内 src/extract/table_names.json。

| 场景 | rc | 耗时 | 产物 | 输出 |
|---|---|---|---|---|
| latest 已冻结，`--round 2` | **1** | **1.09s** | **无 proposal.json / stats.json** | stderr 一段中文说明 |
| 未冻结，`--round 2`（对照） | 0 | **2.39s** | 207630 B proposal + 298 B stats | items 142 条（实体 91 · 关系 51） |

→ 冻结场景在**载入基版本那一刻**就停，未进入抽取（对照组要 2.39s 产出 142 条）。
→ 实测省下约 1.3s ≈ 抽取阶段全部开销。**真正的价值不在秒数**（本地 CPU 计算很便宜），
  而在「失败发生在第一步 + 不留下看起来像跑成功的产物」。
  Dev 文档里「实测 6.9s 量级」与本机 2.39s 不一致（口径/硬件/规模差异），机制结论不受影响。

### 3. 绝不退回空清单 —— 守住 ✅
`load_existing_types`（src/bin/ontok-propose.rs:360-437）全路径：
- `round <= 1` → `Ok((DEFAULT_BASE_VERSION, Vec::new()))` ← **唯一**的空清单出口，且是设计内的（第 1 轮不比对基版本）
- `round >= 2`：缺 project → bail；无版本 → bail；**`base.frozen_at.is_some()` → bail**；
  之后 `get_snapshot` 失败 → `ok_or_else` 报错
- **冻结命中时：报错退出（rc=1），existing_types 根本没有被构造**。
  实测 stdout 为空、不含「已有类型」字样 —— 不存在「悄悄给空清单」的形状。
  三种违规（ExistingKeyDeclaredAsNew / ExistingDisplayNameChanged / ForbiddenTerm）
  在冻结场景下是「显式失败」，不是「静默不可达」。

### 4. 失败信息说人话 ✅（实测 stderr 原文见下）
要素齐：哪一版（v2）+ 哪个 project（带项目名）+ 为什么（冻结版是判据 AC-006，不接受变更）
+ 怎么办（三条按常见程度排，各带**可直接粘贴的命令** `ontok version new/list`）
+ 明说「本步刻意不替你猜是哪一版，也不悄悄改去查上一版」。

## 三、产物与红线
### 产物 sha（输入 = fix-props/schema.json + src/extract/table_names.json，--project extracted）
| 文件 | 实测 sha256 | 期望 | 结论 |
|---|---|---|---|
| stats.json | 6b62a5137656...a029d6 | 同 | ✅ 完全一致 |
| proposal.json | b269f3436eff...3e95 | 同 | ✅ 完全一致 |
输入 sha：schema.json b71c2afd…f2019 / table_names.json f52fea02…5fa5f9（均未变）

### 红线
| 文件 | 状态 |
|---|---|
| tests/drop_rate.rs | 未动 ✅ sha 5da9272b…4363，与父提交逐字节相同 |
| tests/test_fk_naming.rs | 未动 ✅ sha c28a5bb6…2831 |
| src/domain/view.rs | 未动 ✅ sha c0c2df2c…d4ad |
| .review-workspace/fix-props/schema.json | 未删 ✅ 219688 B，10月7日 23:55 原样 |
三者均不在 `git diff --name-only 23485c0..e5717a8` 的 6 文件内。

## 四、Dev 报的两项未修 —— 代码现状与描述相符
1. `src/cli.rs:436` `let eb = doc.get("extracted_by")...serde_json::from_value::<ExtractedBy>(v.clone())`
   → 原样从 JSON 取，仅做格式反序列化，**无任何来源核验**；:449 直接塞进 NewProposal。✅ 相符
2. `src/db/proposal.rs:108` `pub async fn create(...)` 为 `pub`，`NewProposal.extracted_by`
   直接 `.bind()` 进 INSERT（:118-125），**无白名单/能力闸门**。✅ 相符
（严重度与处置不在本轮评估范围，按派单要求仅确认事实。）

## 五、纪律自证
- 只用 worktree 自带 `feature-REQ-020/target`，未设 CARGO_TARGET_DIR、未用 --release、未建 probe crate
- 唯一临时源文件 `tests/qa_probe_visibility_tmp.rs` 已删，`git status --porcelain` 与 `git clean -nd` 均空
- 全部产物落在 `.review-workspace/qa-req020a/`；/tmp 与 home 无残留
- 共享本体库：只 INSERT（project new / version new / version freeze），无 DELETE/TRUNCATE

# 状态：REQ-107 · 出口收口

## 当前状态

**待验证（第 2 轮）**（2026-10-20 · Dev 已修 5 条阻断，QA 复验中）

## 状态流转

| 时间 | 状态 | 依据 / 动作 |
| --- | --- | --- |
| 2026-10-17 | 草稿 | 从 REQ-106 拆出，登记于 `dispatch/req-registry.md`；**当时 AC 未写**，理由是「AC 待 REQ-106 落地后再写」（纪律 S） |
| 2026-10-20 | 已就绪 | REQ-106 已验证并合入 `develop` `b714f48` ⇒ 写 AC 的前置条件满足；用户批准开单 |
| 2026-10-20 | 进行中 | 建 `feature-REQ-107/`（分支 `feature/REQ-107`，base `develop` `b714f48`）；派 explore 枚举出口 |

## 已完成的准备

- [x] `demands/REQ-107/demand.md` —— 范围 / 不做 / 三个卡点 / 红线纪律 / 交付物
- [x] `.feature/manifest.json` —— worktree 分配登记
- [x] `demands/REQ-107/acceptance.md` —— **基于两份独立摸底清单**（HTTP/MCP 一份、CLI/HTML 一份），非凭印象
- [x] QA 生成测试用例（29 条，P0=17；覆盖矩阵 6 条如实标注「无法机判」）
- [x] 派 Dev 编码 —— 已交付（6 提交，8 个新判据文件，13/13 出口接线）

## 摸底结论（2026-10-20，两份独立 explore）

| 项 | 结果 |
| --- | --- |
| 出口计数 | **33**（全仓口径：CLI 22 + HTTP 4 + MCP 7）。旧「25」按 A/B/C/D 任一可复现口径都对不上，**已作废** |
| 客户类 | 13 条：CLI 5（全过闸，均在写出前）+ HTTP 3 + MCP 5（serve 层统一过闸） |
| **零闸数据出口** | **1 条**：`http.rs:222-230` 把项目名裸拼进 400 正文（无 guard / 无 safe_error / 无转义）⇒ AC-006 |
| 静态零闸 | `tools/list`（`serve.rs:256`）、`initialize` 模板正文（`serve.rs:245`）、`INDEX_HTML`（`page.rs:30-259`）⇒ AC-008 |
| 建模豁免 | 8 条，豁免**成立**（类型隔离）但依赖「客户类无一例外调闸」的人肉前提 ⇒ AC-007 |
| 结构根因 | `emit_domain_*` 只保证「写出去 == 判据看到」，**不决定要不要判**；G-5 反向守卫有 owner 白名单 + 「实参提到生产者」判据，**放过 14 个出口** |
| 测试现状 | 56 个测试文件、13 个提到守门器，**没有一个**断言「所有出口都接管线」 |


## 阻塞 / 风险

- **无外部阻塞**（不依赖 D1/D2，不依赖客户判读）
- 风险：三个卡点（口径 / 粒度 / 报告）任一个没裁定，收口就会静默改输出 ⇒ AC 必须先把「零可见变化」或「逐条变化」写死

## ⚠ 本机环境限制（2026-10-20 实测，派单必须带上）

| 项 | 实测结果 |
| --- | --- |
| `cargo check --lib` | ✅ 通过（0.93s） |
| `cargo check --all-targets` | ❌ **失败**：`src/bin/ontok-gate-preflight.rs` 用了 `std::os::unix::fs::MetadataExt` / `symlink`（587/588/1514/1544 行）⇒ **Windows 上编不过** |
| 归因 | 该文件是 develop **首个提交**（`a93abd3`）就带进来的，**不是本需求引入**，属既有 unix-only 代码 |
| 后果 | **本机无法跑全量 `cargo test`**（62 suite / 846 passed 的基线无法在此复核）；单个 `--test <name>` 与 `--lib` 可跑 |
| 派单要求 | Dev **不许伪造全量数字**（纪律 E / L）。必须在交付物里写明「全量基线本机不可复核」并给出它能跑的那部分实测结果 |


## 用例与 AC 的互校（QA 回来后补）

- QA 报了 **4 个 AC 缺陷**，已全部裁定并写入 `acceptance.md` §七 A1–A7：
  ① 计数不闭合（34 vs 33，差 Z4，Z4 不是出口）；② AC-005 与 AC-006 冲突（Z1 是唯一允许的可见变化）；
  ③ 既有 `test_property_filter.rs:341` 必红 ⇒ **改写不许删**；④ `property_filter.rs:139` 与 REQ-103 的边界（两套否定式断言各管一段）。
- QA 指出三条「最可能被做成假绿」：TC-016（golden 用改后代码生成 ⇒ diff 恒空）、TC-010（M3 盲区）、TC-023（`description` 今天恰好干净 ⇒ `assert!(!is_empty())` 零判别力）。已进 Dev briefing。

## Dev 交付后新增的待办

- [x] QA 代码审核 —— **FAIL，3 阻断**（见下）
- [ ] 清理 `.review-workspace/req107/base`（Dev 为 golden 取证建的 detach worktree）
- [ ] 合并后回收 `feature-REQ-107`

## QA 审核结论（2026-10-20 · `qa-review.md`）

**FAIL · 3 条阻断**。QA 在独立 worktree 做了 **12 次变异**（Dev 是 6 次），核心结论：
**AC-003 的承诺只对 CLI 的 4 个登记站点成立，对 HTTP 新增出口完全不成成立。**

### 阻断项

| # | 违反的 AC | 事实 | 证据 |
| --- | --- | --- | --- |
| B1 | **AC-003** | 「新增客户类出口但不接管线」—— 这条 AC **逐字点名**的场景，实测 **0 红** | 新增 `/api/raw` 路由、零闸、不进 `OUTLETS` ⇒ **12/12 全绿** |
| B2 | **AC-001** | `OutletSpec.track` 是 **pub 字段** ⇒ 出口一行 `spec.track = Track::Internal` 就能关闸 | 全绿 |
| B3 | **A9 #2** | 锚点迁到管线后，**管线内部**顺序仍被钉住（M6 红），但**出口侧「先写后判」完全失守**（M7 全绿） | 把「未过闸字节先写 stdout」放回出口 ⇒ 全绿 |

### 五种绕过硬（挡不住）

1. 自造 `fn local_ok` 且**真的改变写出**（保留管线调用）
2. 公开构造器**换个名字** `pub fn craft(String)`（doctest 全绿、12 判据全绿）
3. **新增 `/api/raw` 客户路由**，零闸、不进 `OUTLETS`
4. 出口把 `spec.track` 改成 `Internal` 即关闸
5. 把「未过闸字节先写 stdout」放回出口

### 挡得住的三种（不是全盘失败）

摘掉 H3 的管线调用（红 2 条）· 三字段全改 `pub`（doctest 报 "compiled successfully, but marked compile_fail" + 源码判据红）· 公开构造器**叫 `new`**（但只靠 `pub fn new(` 这个字符串）

### QA 确认「真做对了」的部分

13 条出口**确实全部接线**（QA 逐条读 `path:line`）· 出口层零判据直调 · 五红线 / `guard()` / G1–G4 前 145 行逐字节未动 · 无 `#[ignore]` / 无删测试 · golden 来源可独立复现（QA 自写脚本重算 6/13 份全 MATCH）· **Z1/Z2/Z3 修复有效**（塞「外键」进 `description` 与 `instructions` 模板正文都红）· Dev 报告的本机数字与 QA 实测**逐条一致（没编数）**

### 根因（QA 的判定，我认同）

`OUTLETS` 是**手工登记表**，只做「表 → 源码」**单向** needle 对账；**没有任何判据枚举源码里的客户写出点**。
⇒ Dev 的 M-2 变异用的是**已登记的名字**，所以「能红」，但这掩盖了「换个名字就绿」。

### 预算说明

`src/bin/ontok-gate-preflight.rs` 的 A8 绕行经 QA 两次独立复验，sha256 `C7CF95E2…322D` 前后一致。
⚠️ **QA 新发现**：`tests/test_retained_names_req084.rs` 用 `include_str!` 引用该 bin ⇒ **bin 挪出期间它编不过**（Dev 报告未写）。A8 绕行只对**不含该 `include_str!` 的目标**成立 —— 记入 A10。

## 第 2 轮修复（2026-10-20 · commit `4bbd8ac`）

| 阻断 | 修法 | 位置 |
| --- | --- | --- |
| B1 反向枚举 | 递归扫 `src/` 全目录写点（剥注释 + 抹字符串），每个必须落在 `OUTLETS.call_site` 或 22 条非客户白名单（带期望命中数） | `tests/test_req107_source_enumeration.rs:296/364/486` |
| B2 track 不可赋值 | 出口只传 `OutletId`（enum），轨道由 `const fn spec_of(id)` 查表；全字段私有 + 运行时 `pairing_ok` | `pipeline.rs:243/291/636/727/713` |
| B3 出口侧时序 | 写出函数只收 `OutletRender`（裸 `&str` **编不过**） | `cli.rs:645/650`；判据 `source_enumeration.rs:546` |
| B4 构造器全枚举 | 枚举全仓 `pub fn` 返回类型（含 `-> Self`），要求恰好 `render`/`publish_value` 两个；删掉公开工厂 `publish_error_text` | `source_enumeration.rs:840/893`；`http.rs:251-266` |
| B5 自造判据 | 出口文件逐函数登记（43 条）+ 全仓 mod 登记（52 条）+ 禁止产物转手 | `:981/:1093/:546` |

**5 种攻击实测**：全红（其中 4a/5a 是**编译期挡住**，Dev 如实标注「不算执行结果」，并用 4b/5b/5c 等价可编译形态补测）。

**Dev 自陈未封口三项**（已入 AC §A12 作第 3 轮靶子）：闭包式本地判据 · handler 返回裸 `String` · `NON_OUTLET_FILES` 整文件豁免。

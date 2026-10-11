# 验收标准：REQ-046 消除 `read_source` 的编译期路径依赖

> 依据 REQ-045 **合并后全量实测 30 条失败**带出。基线：`c98a631`（develop）。
>
> **这是 REQ-045 引入的回归，不是既有缺陷。** 改动前 `include_str!` 的二进制
> **自包含**（内容随二进制走），**没有**这个依赖。

---

## 背景：事故是怎么发生的

REQ-045 把判据从 `include_str!`（编译期内嵌）改成
`read_source`（运行时 `read_to_string` + `env!("CARGO_MANIFEST_DIR")`）。

**合并后第一次全量：506 passed / 30 failed**，形态集中：

```
panicked at tests/test_acp_extraction.rs:378
读不到 /home/…/verify-REQ-044/src/agent/acp/mod.rs：No such file or directory
```

**根因（已用 `strings` 坐实）**：被复用的二进制其 `CARGO_MANIFEST_DIR`
指向 **`/home/…/verify-REQ-044`** —— **一个早已回收的 worktree**。

| | 改动前 `include_str!` | 改动后 `read_source` |
| --- | --- | --- |
| 二进制是否自包含 | **是**（内容随二进制走） | **否**（依赖编译时的目录路径） |

⇒ REQ-045 之前**不存在**这个依赖。**这是新引入的脆弱性。**

> ⚠️ REQ-045 的 Dev 报告里提到过「`CARGO_MANIFEST_DIR` 是编译期常量，
> 编译后搬走仓库会指向旧路径」，但**归类为「诚实说明两点」**。
> BA 定性：**一个一定会咬人的「已知限制」是缺陷，不是说明。**
> 尤其当它**没有解法**、而原方案**本来就没这个问题**时。

---

## AC-001 · 判据不依赖编译期目录

- 预期结果：判据**每次运行都定位到「当前实际的那个 crate 根」**，
  而不是编译时那个目录
- 类型：架构
- **候选实现（由 Dev 判断取舍，可组合）**：
  1. 运行时从 `std::env::current_exe()` 往上回溯定位 crate 根（**不依赖编译期常量**）
  2. 保留 `CARGO_MANIFEST_DIR`，但**读之前先校验 cwd 下是否存在同名文件**；
     不存在时**明确报**「该二进制在 X 目录编译、你在 Y 跑的」，
     而不是笼统的 `No such file or directory`
  3. 二进制内同时记录构建时的 `git rev-parse HEAD`，不匹配即拒绝运行

> **不强制具体方案**，但要求：**二进制被搬到别的目录后，要么仍能工作，
> 要么给出让人一看就懂「这是目录不对、不是文件丢了」的报错。**
> 现在这个报错把「目录不对」说成「文件不存在」，**会把人引向错误方向**。

## AC-002 · 必须复现并消除原事故

- 测试步骤：
  1. 在某个 worktree 编译出判据二进制
  2. **删除/回收那个 worktree**
  3. 把二进制**复制到另一个目录**并直跑
- 预期结果：**不出现 30 条那种「读不到 X 路径」的集中失败**；
  要么正常工作，要么按 AC-001 给出**指明目录不符**的明确报错
- 类型：功能
- **判别力自证（必做）**：
  - 修复前：该场景**必须复现失败**（否则你的修法没对准问题）
  - 修复后：同一场景**必须变好**
  - 报告须给出**两组证据**（二进制名 + 运行条数）

> ⚠️ **别用「cargo 会重编译」当理由跳过这一步** ——
> 本次事故的根因就是 **cargo 误判新鲜而复用了旧二进制**。
> 判别力自证**必须直跑二进制、不经 cargo**（REQ-045 已确立这个口径）。

## AC-003 · fail-closed 保持

REQ-045 已补的 fail-closed（读不到文件 ⇒ panic + 打印三层信息）**不能退化**。
若采用 AC-001 方案 2/3，**报错信息要能区分两类失败**：
- **文件真的不存在**（该报）
- **二进制与当前目录不匹配**（该明确报「目录不对」）

把两者混成一条 `No such file or directory` 就是这次事故里最有害的部分。

## AC-004 · 全量与红线

- 至少 1 轮完整 `cargo test --no-fail-fast`
- 预期结果：**539 passed / 0 failed / 1 ignored**
  （⚠️ **基线由 536 变为 539，原因：新增 G-1/G-2/G-3 三条守卫**（REQ-047）。
  原文的「536 / 6m20s」是 REQ-045 强制重编译后的**当时实测**，作历史记录保留）
- **建议同会话基线对照**；耗时**别声称精度**（共享集群负载浮动）
- 红线：`tests/drop_rate.rs`、`tests/test_fk_naming.rs`、`src/domain/view.rs`、`src/` 零改动；
  `test_humanize_still_refuses_unknown_columns` 一条断言都没放宽；
  **无测试被删除或 `#[ignore]`**；判据文件内测试条数不变
- 产物 sha 不变：
  - `stats.json` = `cda6a9a2a007fcabe2be602a9962b66deda84498e0fb1fb8232d0627632cd7ba`
  - `proposal.json`（`--project extracted`）= `1eef95414b8eec084b12b09b8eb89d6347c64f45476447ec243e5dd66c5c5635`
  - 命令实属 **`ontok-propose`**；`--out` 在 `--stats` 之前；**不传 `--names`**

---

## 环境事实

- 共享集群 DB 很慢：`SELECT 1` p50 实测 **133~161 ms**；总连接常驻 **32~38 / 100**
- **先探延迟再判断测试慢不慢**；复现率随负载浮动，**不声称稳定值**
- 报告把「代码失败」与「环境类失败」**分开写**
- ⚠️ **运行期判据与环境类失败严格二分** —— 环境失败绝不能冒充「杀死」
- ⚠️ **`.env` 里库名就是 `notol`**，忘了替换成 `ontol` 会让 DB 测试秒败、
  且症状像「集群故障」—— 本会话已有 Dev 与 QA 各踩过一次
- 本机**没有 `psql`、没有 psycopg**
- 共享本体库**禁 DELETE / TRUNCATE / 清空表**；源库只读
- 口令只走环境变量，**绝不写进报告**；⚠️ **不要 `echo` 连接串**
- ⚠️ **测量二进制里的 `CARGO_MANIFEST_DIR` 用 `strings` 时，pattern 要带 `/src/` 之类才算精确** ——
  裸 `verify-REQ-044` 会误命中判据文件里记录 REQ-044 封口历史的注释（本会话已误报一次）

## 红线清单

| 文件 / 约束 | 说明 |
| --- | --- |
| `tests/drop_rate.rs` | 禁改 |
| `tests/test_fk_naming.rs` | 禁改 |
| `src/domain/view.rs` | 禁改 |
| `test_humanize_still_refuses_unknown_columns` | 一条断言都不许放宽 |
| 任何测试 | **不许删除、不许 `#[ignore]`** |
| `test_backfill_…` | **不许放松任何一条断言** |
| **重试行为参数** | **不许改**（本需求只碰判据） |
| `.review-workspace/fix-props/schema.json` | **绝不可删**（门禁输入） |

## 交付纪律

- **绝对禁止** `CARGO_TARGET_DIR` / `--release` / 另建 probe crate
- **绝对禁止** `ln -s` 指向别的 target（符号链接会让两个 worktree 共享构建产物，
  本会话已因此连续踩坑两次）
- **量化数字必须标口径与命令**（本会话已 6 次因数字没标口径引发返工）
- **编译失败不算变异执行**
- **存活变异必须逐条论证**
- 临时文件只放 `.review-workspace/req046/`；**禁 `/tmp`、禁 home**
- **写含反引号的 Markdown 必须用 edit/write 工具，禁止用 shell**。
  （失败就用带引号 heredoc 兜底并登记偏离，**别反复重试**。）
- 核对文件用**裸 `grep`**。禁 `rm`（本环境已接管成 `mavis-trash`）
- 仓库**无 remote，不要 push**；不 squash、不 force、不改历史
- ⚠️ **边做边落盘**，不要攒到最后 —— 本会话已有 **4 次子 agent 会话丢失**的先例

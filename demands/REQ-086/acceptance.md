# 验收标准：REQ-086 MCP 关系输出与 HTML/总览说同一句话

> 立项依据：2026-10-11 explore 摸底（80 条关系实际形态）+ **负责人裁定「存储子→主，消费主→子」** + BA 复核
> 基线：本地 develop `f8c7240`（已含 REQ-084 / REQ-087）

---

## 一句话背景

本体平台把关系暴露给客户，客户问「一个客户有几条订阅」。
HTML 页面**已经会说人话**（「多个订阅」），**MCP 不会**——它把裸的 `N:1` 直接透给客户，
客户得自己判断那是「我有 N 个」还是「它只有 1 个我」。

本需求：**不设计新方向**，把 HTML 侧早已确立的约定接到 MCP 上。

---

## 已确立的约定（不要重新发明）

**存储方向 = 子表 → 主表**（`客户订阅 —N:1→ 客户`），explore 已实测证实，负责人裁定保留。
「子→父 + `belongs_to` + `N:1`」与「父→子 + `has` + `1:N`」是**同一句话的两种写法**
⇒ **反向重述语义收益为零**，且会把 80 条审核历史断成 160 条 Removed/Added。⛔ 不要做。

**数量词按查询端重述**，`src/domain/html_view.rs:115` 是全项目**唯一**出口：

```rust
pub fn count_phrase(how_many: &str, self_is_from: bool) -> &'static str
// ("多对一", true)  => "1 个"   // 一条日志指向一个客户
// ("多对一", false) => "多个"   // 一个客户有几条日志 ← 人自然想问的
```

`src/domain/overview.rs:307` **已复用它**。注释里写着：
「两处各写一份的话，页面说「多个」而总览说「1 个」，判读人会当场发现两处对不上。」

⚠️ **`MCP` 是第三条路径，必须复用同一个函数。**

---

## 要改的地方

`src/mcp/tools.rs` 的 **`list_relations`**（约 `:510-534`）：

```rust
// 现状
.json!({ "from": r.from, "relation": r.meaning, "to": r.to, "how_many": r.how_many })
//                                        ↑ 裸串，方向取决于调用方从哪端看
```

`describe_object` 若也回关系，一并检查。

---

## AC-001 · 复用 `count_phrase`，**不得另写一份映射**

- ⛔ **不许在 `tools.rs` 里新建「基数 → 人话」的对照表**
- ✅ 必须调用 `html_view::count_phrase`
- 若它是私有（`fn` 非 `pub`），收窄可见性即可，**⛔ 不要复制它的逻辑**

## AC-002 · 有过滤时按查询端重述

`list_relations(object=X)` 时，每条关系的 `self_is_from = (r.from == X)`。

**必须实测证明**：传「客户」得到「多个」方向、传「客户订阅」得到「1 个」方向。

## AC-003 · 无过滤时必须说明视角（**本需求最容易漏的一条**）

`list_relations()` 不带 `object` 时返回**全部** 80 条关系——**没有单一视角**。
此时若仍只给裸 `N:1`，客户困惑照旧。

**请你设计并说明**该怎么做（三种可选，你选一种并给理由）：
- (a) 只输出裸 `how_many` + 一句说明「这是从 from 端读的」
- (b) 同时输出 `from_end_count` / `to_end_count` 两列
- (c) 其他（说明）

⚠️ 不接受「静默沿用一个视角」——那等于把歧义藏起来。

## AC-004 · 与既有出口的一致性守卫

加一条测试：**同一个关系，MCP 的输出与 HTML/总览的输出必须一致**。
不是「各自看起来对」，而是**同一句话**。参考 `count_phrase` 注释里那句
「页面说『多个』而总览说『1 个』，判读人会当场发现两处对不上」。

## AC-005 · 判别力自证

给**二进制名 + 运行条数**：

| 变异 | 期望 |
| --- | --- |
| 把 MCP 的 `self_is_from` 取反 | 至少一条测试红 |
| 让 MCP **绕过** `count_phrase` 改用裸串 | AC-004 的一致性测试必须红 |
| 改 `count_phrase` 的某个 match 分支 | MCP 与 HTML **同时**变（证明真的共用） |

⚠️ **编译失败不算变异执行。**
⚠️ 回滚若用 `cp -p` / `shutil.copy2`（保留 mtime）⇒ cargo 不重编、跑陈旧二进制的假绿假红。**回滚后必须 `touch` 强制重编。**

## AC-006 · 向后兼容

- MCP 是对外接口，**新增字段可以**、**删除/改名字段不行**
- 若你选择保留 `how_many` 又新增 `count`，必须在字段说明里写清两者关系
- `src/mcp/` 的既有测试（`tests/test_mcp_server.rs` 31 条）**不得放宽**

## AC-007 · 全量与红线

- 至少 1 轮完整 `cargo test --no-fail-fast`，基线 **780 passed / 0 failed / 1 ignored / 59 组**
- 红线三文件 `git diff --numstat` **无输出**
- 无测试删除、无新增 `#[ignore]`、无断言放宽
- ⚠️ 若某个 target 红了，**先确认是不是 REQ-079 的连接抖动**（共享集群偶发 `ConnectionReset`），重跑确认再判定
- **产物零变化**：`stats.json` / `proposal.json` 的 sha 不得变（本需求只改消费侧）

---

## ⚠️ 环境坑

**坑 0（QA 在 REQ-087 实测踩过，最容易误判）：worktree 的 `src/extract/table_names.json` 是脱敏版 `t001…`，真版要手动拷。**

漏拷时**不会测试红**，而是**产物静默降级**（`entities_with_cn_name` 掉到 3、
`props_via_relation` 61→0）⇒ 极易被误读成「规则没接线」而错判。

```bash
set -a && . <仓库根>/.env && set +a        # ⛔ 不要 echo 连接串
export ONTOK_SCHEMA_JSON="<仓库根>/.review-workspace/fix-props/schema.json"
cp <仓库根>/.review-workspace/table_names.orig.json src/extract/table_names.json
```

⛔ **不要 commit 这次覆盖**，交回前 `git checkout HEAD -- src/extract/table_names.json`。

测试库是**远端共享集群**，不是本地。冷编译约 6 分钟。
⛔ 禁 `CARGO_TARGET_DIR` / `--release` / 另建 probe crate / `ln -s`。

---

## 硬约束

| 项 | 说明 |
| --- | --- |
| 红线文件 | `tests/drop_rate.rs` / `tests/test_fk_naming.rs` / `src/domain/view.rs` **禁改** |
| `count_phrase` | 可收窄可见性，**⛔ 不得改它的既有分支逻辑** |
| `propose.rs` | **禁改**（REQ-084 登记表钉死了 130 处行号+列号，加一行即门禁变红） |
| 任何测试 | 不许删除、不许 `#[ignore]`、不许放宽断言 |
| 临时文件 | 只放 `.review-workspace/req086/`；⛔ 禁 `/tmp`、禁 home |
| 删除文件 | 禁 `rm`（本环境接管成 `mavis-trash`） |
| 仓库 | **无 remote，不要 push**；不 squash、不 force、不改历史 |
| BA 分支 | 只由 BA 写 |
| 中文长文 | 用 edit/write 工具，**不要 heredoc** |
| 量化数字 | **必须标口径与命令**；⛔ 禁 `\b` 统计中文语料；⛔ `git ls-files` 扫中文路径须 `-z` + `-c core.quotePath=false` 且 `-c` 在**子命令之前** |
| 口令 | 只走环境变量，不入库/不进文件/不进日志/不进 commit；**不 echo 连接串** |

---

## 交付

1. 改动摘要（文件:行号 + 改了什么）
2. **AC-003 你选了哪种方案、为什么**
3. AC-005 三条变异的证据（二进制名 + 运行条数 + 结果）
4. AC-004 一致性测试怎么写的（贴关键断言）
5. 有过滤 / 无过滤两种情况下的**实际输出样例**（贴 JSON）
6. 全量结果
7. 红线 `git diff --numstat` 原始输出
8. 产物零变化的证据（两个 sha）
9. `table_names.json` 已还原的确认
10. 对本 AC 的异议（无则写「无」）

## 你可以否定 BA

本项目 Dev 否定 BA 被采纳 18 次。特别是 AC-003 的三个选项——
**如果你认为我列的三个都不对，给出更好的方案并说明**。
另外：**若你认为「按查询端重述」这个约定本身该重新讨论**（而不只是接上去），
请说，但**不要擅自动存储方向或 `count_phrase` 的逻辑**。

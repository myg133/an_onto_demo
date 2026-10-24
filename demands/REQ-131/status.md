# 状态：REQ-131 · 测试夹具缺失时的「响亮跳过」

## 当前状态

**已完成**（2026-10-24 · 已合并 `develop` `5163cfb`）

## 结果

| 项 | 值 |
| --- | --- |
| 合并 | `develop` **`5163cfb`**（2 个提交）；`src/` **零改动**，只改 `tests/` 下 9 个文件 |
| 默认（缺夹具） | 打 `*** SKIP: <用例> -- 缺 <夹具> -- 这条判据没有执行，不是通过 ***`，**exit 0** |
| `ONTOK_REQUIRE_FIXTURES=1` | 打 `*** FAIL: … CI 要求夹具必须存在 ***` 并 panic，**exit 101** |
| 取值清单 | 开：`1`/`true`/`yes`/`on`；关：未设/空/`0`/`false`/`no`/`off`/其它（只认白名单，不做模糊匹配） |
| 跳过可数 | 21 条用例 / 21 行标记，**1:1** |
| 夹具齐全 | 断言**一行未改**（`git diff` 里 `+/-` 的 assert 行与 `#[ignore]` 全为空） |

## 价值：假红清零后，真红露出来了

改之前全量里有 **21 条假红**（缺夹具却失败）。清完之后：

| 红 | 数量 | 性质 |
| --- | --- | --- |
| `test_review_show_e2e` | 9 | 测试自身 flake（PID 复用撞名），重跑即绿 |
| `drop_rate` / `test_fk_naming` | 2 | **fail-closed 门禁**（故意失败，不修） |
| `test_agent_orchestration` | 1 | **真红**：抽取少产出 1 条 `add_link_type` |
| `test_retained_names_req084` | 2 | **真红**：保留名登记表过期（行漂移 + 计数 9≠5） |

⇒ 已建 [`BA/dispatch/baseline-reds.md`](../../dispatch/baseline-reds.md) 台账，**每条红都有归属**；并登记 REQ-132/133/134。

**顺带修掉的 2 条既有红**（不属本需求范围）：`test_req128_names_required.rs:42` 与 `test_req129_doc_merge.rs:820` 的裸 `env!("CARGO_MANIFEST_DIR")` ⇒ `common::crate_root()`，`test_anchor_guard` 由红转绿（3/3）。

## 关键裁定

| # | 裁定 |
| --- | --- |
| R1 | 缺夹具默认**跳过不失败**，但必须打印可 grep 的标记 |
| R2 | `ONTOK_REQUIRE_FIXTURES=1` ⇒ **失败**（防「跳过 = 永远不跑」） |
| R3 | 只改测试基础设施；⛔ 产品代码零改动 |
| R4 | 结构判据**按文件名排除自己**（它自带反向样本，否则必假红）——接受 |
| R5 | `ONTOK_REQUIRE_FIXTURES` 接受 `1/true/yes/on`（AC 写 `=1` 是举例）——接受，但**必须写进模块头** |

## 一条要记住的边界

**fail-closed 门禁 ≠ 可跳过夹具**：

| 类型 | 缺件时 | 例 |
| --- | --- | --- |
| 可跳过夹具 | **响亮跳过**（可强制失败） | `ONTOK_TEST_DATABASE_URL`、租户 id |
| **fail-closed 门禁** | **必须失败** | `drop_rate` / `test_fk_naming` 的脱敏 schema |

后者若改成跳过，「丢弃率超线」就再也没人检查。

# REQ-055 · Dev 结果（落盘）

commit: d3ebea8 · 分支 feature/REQ-055 · 基线 7343bb0 · 未 push

## 改动面（`git diff --name-only 7343bb0 HEAD`）

src/cli.rs · src/domain/mod.rs · src/domain/model_view.rs（新）· tests/test_model_view.rs（新）
红线三件套（view.rs / drop_rate.rs / test_fk_naming.rs）**零改动**。

## AC-001 判别力（同一份 fixture 快照，同一条 `scan_modeling_terms`）

| 侧 | 判决 | 二进制 | 条数 |
|---|---|---|---|
| 建模轨输出 | **命中**（含来自**数据**的 `1:N` 形态命中）| `test_model_view` | 10 passed / 0 failed / 0 ignored |
| 客户视图输出 | **空** | 同上（同一二进制内）| 同上 |

关键测试：`test_model_view_same_snapshot_terms_land_in_model_track_only`

## 变异执行（全部编译通过后才算执行）

| 变异 | 结果 |
|---|---|
| M-A 抽掉 `column`（建模轨丢列映射）| **被杀** ×3 |
| M-B 基数/语义翻成人话（照抄客户视图取舍）| **被杀** ×1 |
| M-C 建模轨整体照抄客户视图取舍 | **被杀** ×4，**含判别力测试本身** |

存活论证：M-A/M-B 下 `test_model_view_same_snapshot_terms_land_in_model_track_only` 存活，
因为建模轨的**字段名本身**（`key`/`evidence`/`extracted_by`）就是建模术语 ——
于是据此把该测试加固为「命中里必须有一条来自**数据**（`1:N` 形态），
字段名触发不了形态守门」，加固后 M-C 将其杀掉。

## AC-002 既有守门零退化

- 全量：583 passed / 1 failed / 1 ignored（584 可运行 = 574 基线 + 10 新增），42 条 result 行
- 隔离复跑：`test_model_view` 10/0/0 · `test_view` 26/0/0 · `test_mcp_server` 31/0/1
- 全量耗时 446s（7m26s）

## AC-003 红线

- `stats.json` = `cda6a9a2a007fcabe2be602a9962b66deda84498e0fb1fb8232d0627632cd7ba` **不变**
- `proposal.json`（`--project extracted`）= `1eef95414b8eec084b12b09b8eb89d6347c64f45476447ec243e5dd66c5c5635` **不变**
- 输入 `schema.json` 跑前跑后 sha/mtime/size 三项一致，未删未改

## 未修的问题（已报 BA）

1. **P1 结构性**：`review decide --track model` 裁决的是**提案条目**，
   本投影读的是**已落库快照** —— 两者不是同一个对象。
   提案侧的建模轨视图是下一个缺口。
2. **P2**：快照层 `Property` 没有独立 `pk` 位，建模轨不伪造，只给 `column` + `nullable`。
3. **P3**：`english_term_hit` 只返回**首犯**一个词（`Option<&'static str>`），
   守门 fail-closed 不受影响，但报告口径是「只报第一个」。

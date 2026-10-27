
## QA 结论（2026-10-27 15:48）—— **PASS（功能面）· 阻断 1 条（覆盖）**

报告 `BA/.local/qa/REQ-162/REPORT.md`。

| 组 | 结果 |
| --- | --- |
| **AC-007 真无库** | ✅ 清空三个 env 起服务走完全流程 + `bundle verify` exit 0；**反证**：`ONTOK_DATABASE_URL` 指向坏库仍通过；静态 grep 4 处命中**全在注释** |
| 定版/导出/只读 | ✅ `DRAFT_NOT_READY` 400 / `NO_VERSION` 400 / stale 后旧 v1 sha 不变且 finalize 409 / 包内快照与定版**逐字节相同** / `out/proposal.json` sha 前后不变 |
| 旧路由 404 | ✅ **33 条探测全 404**（0 条非 404、0 条 401），并写明「不再挂载 ≠ 源码删除」 |
| 真浏览器 | ✅ 21/21（复用我 12 条 + QA 自写 3 条） |
| 变异 | ✅ 5 个全被抓住（门禁/逐字节/content-type/草稿只读/stale） |
| **§E 覆盖审计** | ⛔ **阻断 1** |

### BLOCK-1 · 「25 条退场」的理由不成立

QA 实测：`console::http::build_router` **还在**、**仍被 `review open` 挂载**（`review_open/mod.rs:753`，`test_req160_review_open` 9 条绿），
那 25 条是**进程内**直接调 `build_router(service(pool))` ⇒ **被测对象没消失**。
而用来"接住"的 `test_req159c_write_plane`（9 条）实测只接住约 5 项，**约 10 项无替代**。

⇒ 已退回 Dev：恢复 25 条并修到带库 25/25；补回 2 处被弱化的 page 判据；更正 changelog「其余测试一个没删」的不实表述；「全选接受/拒绝」加二次确认。

### 另一条要记的（非阻断）

`page.rs` 内嵌单测 **19 → 0**：11 条因视图删除合理移除、6 条有等价替代、**2 条曾被弱化**（端点集合冻结→子串；guarded→只断言 `confirm(`）⇒ 已要求补回强度。

# 状态：REQ-170 · 构建链带上库结构

## 当前

**已退回**（QA 判阻断 1 条：只读账号下主键全 null ⇒ r2rml 0 个 TriplesMap；Dev 在修 F-1/F-2/F-3）

## 我复核过的（lion 亲跑）

| 判据 | 结果 |
| --- | --- |
| `cargo check --lib` | 绿 |
| `--lib extract::` | **257 / 257**（带真库；不设 `ONTOK_TEST_DATABASE_URL` 时 1 条环境性失败） |
| 全量 `--lib` | **968 / 968**（带真库 `req170_lion`） |
| `--test test_req170_schema_chain` | **6 / 6** |
| `--test test_req163_local_chain` | 8 / 8 |
| `--test test_req147_doc_acp` | 13 / 13 |

## Dev 交付的关键证据

| 项 | 值 |
| --- | --- |
| 无库零回归 | 7 个产物 + stdout **逐文件 sha256 全 SAME**（含 `.cache.json` 与自动空提案），stderr 仅内核端口不同 |
| 有库 `--dsn` | `schema.json 2890B`；`maps_to 落地 1 条 · 丢弃 0`；边 key `cloud_disks_server_id__servers` |
| R2RML | `2 个 TriplesMap / 7 个谓词-宾语映射 · schema 对账 0 失败`，`mapping.ttl 3200B` |
| `--schema` 与 `--dsn` | 产物 **SAME**（证明两条入口等价） |
| 反例能红 | `maps-bad`（列不存在）⇒ `maps_to 落地 0 · 丢弃 1（查不到列 1）`，关系整条不进 JSONL |
| 大库闸 | 3 张表只带 2 张（`audit_logs` 与文档无关）；单测用 96 张表证"只带 1 张、⛔ 不全塞" |

## 我裁定的四条（Dev §6）

| # | 他报的 | 裁定 |
| --- | --- | --- |
| ① | `--dsn` 调 `introspect_with_keys`（非裸 `introspect`） | ✅ **批准**（r2rml 要主键；未新增 DB 代码） |
| ② | **判据 3（TTL 被服务面解析）未按字面验** | ⚠️ **我的口径错了，判据作废** —— 产品没有"读入 R2RML Turtle"的入口，服务面是生成方向。**产品缺口**，不是本单缺陷 |
| ③ | podman 走 WSL 地址 `172.17.0.87:5432` | ✅ 接受（记进 runbook，不改代码） |
| ④ | 摘要只带匹配表、不补满 N | ✅ **批准，且比简报更好**（凑数会诱导模型照无关表名编 `maps_to`） |

## 待办

- [x] Dev 补 changelog §7「已知缺口」（已补：7.1 R2RML TTL 无导入入口 · 7.2 schema.json 无外键明细）
- [ ] QA（重点：**零回归的 sha256 证据能否复现**、maps_to 校验的 4 类丢弃码、大库闸）
- [ ] 合并（待用户）

## QA 结论（2026-10-27 22:12）—— **不可以合，阻断 1 条**

报告 `BA\.local\qa\REQ-170\REPORT.md`。

| 组 | 结果 |
| --- | --- |
| **A 零回归** | QA **自己从 `git archive 7323382` 现编基线**，8 项产物 sha256 **全 SAME**；stderr 归一化端口后长度/sha 相同 ⇒ 「只有端口不同」成立 |
| B `maps_to` 4 类 | QA **自写可控假 ACP**：形状错 / 查不到表 / 对上多张 / 查不到列 —— **逐类红**，计数与屏幕分类同向 |
| C 大库闸 | C1 60 表只带 2 ✅ · C2 零匹配带 0 并明说"不要给 maps_to" ✅ · **C3 单表 3001 列 ⇒ 60478 字符（上限 12000）❌ F-2** · C4 覆盖生效 ✅ |
| D 只读/禁令 | 禁区 0 处；**无新增 DB 代码**（唯一接触点 `build_run.rs:327`）；只读角色跑通、写入三种证据被拒 |
| E 回归 | `extract::` 257 · `review_open::` 28 · req170 6 · req163 8 · req147 13 · req159 38 · req107 10 · anchor 3 · req160 9 · 全量 lib **968/968** |

### 🔴 F-1（阻断）只读账号 ⇒ 主键全 null ⇒ r2rml **0 个 TriplesMap**

| 账号 | `schema.json` | r2rml |
| --- | --- | --- |
| owner（`postgres`，**Dev 自证用的**） | `primary_key: "id"` | 2 TriplesMap / 2952B |
| **只读 `qa_ro`**（本需求的形态） | `primary_key: null` | **0 TriplesMap / 2056B** |

而且**下发给模型的摘要本身是错的**（"表 servers 主键：无"，而它明明有 `id`）。
根因：`introspect.rs:418-446 / 535-547` 用 `information_schema.table_constraints`，PG 只对属主可见；QA 钉死对照：**`pg_index` 对只读账号可见（66 行）**。
**失效还安静**：`build run` 正常、TTL 照样写出 2056B。

⇒ **已退回 Dev**（F-1 修 + 只读角色判据 + F-2 字符闸 + F-3 exit code）。

### 一条方法论（记进 rules）

Dev 自证全用 owner，**而需求的形态是只读账号** ⇒ 自证里不可能红。
**自证要用目标形态，不是"能跑通的最方便形态"。**


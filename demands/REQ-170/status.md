# 状态：REQ-170 · 构建链带上库结构

## 当前

**待验证**（Dev 交付 `d7f2321`，未 push；待补 changelog §7 后派 QA）

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

- [ ] Dev 补 changelog §7「已知缺口」（R2RML TTL 无导入入口 · schema.json 无外键明细）
- [ ] QA（重点：**零回归的 sha256 证据能否复现**、maps_to 校验的 4 类丢弃码、大库闸）
- [ ] 合并（待用户）

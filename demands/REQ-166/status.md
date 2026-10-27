# 状态：REQ-166 · AM 契约补齐

## 当前

**已完成**（合并 `0c47cb4`）（Dev 2 commits：`c6e0449` 第一轮 + `de78945` R1/R2 整改；未 push；待 QA）

## 我复核过的（lion 亲跑）

| 判据 | 结果 |
| --- | --- |
| `cargo check --lib` | 绿 |
| `--lib access::` | **70 / 70**（66 + R1 1 + R2 3） |
| `--test test_req144_access` | 9 / 9 |
| `--test test_req144_network` | **27 / 27**（含真调用端到端：host `127.0.0.1`，`Authorization` 里是 `/cvm/tc3_request`） |
| 真 AM 端到端（`.local` 副本补 8 个 service） | `access_fetch.py --check` 的 **8 条 service 错误全部消失**，只剩 `contract_version=2 只认 1`（join 待修） |

## 两处我拍板的取舍

| # | 取舍 | 裁定 |
| --- | --- | --- |
| 1 | R1 只镜像 `max_limit`，其它扁平分页键不往返 | **接受**（`RequestSpec` 既有形态，扩到全量会改序列化输出） |
| 2 | `opts.service` 优先于 AM | **接受**（生产 CLI 从不设它 ⇒ 实际生效的是 AM；它是测试/自检的后门） |

## 待办

- [ ] QA（重点：R2 的变异是否真红、4 处既有测试改动有无放宽）
- [ ] join 修 `contract_version=2` 与 `AM-FIELDS.md:21` 后，本地 AM 落回并复跑 `--check`
- [ ] 合并（待用户）


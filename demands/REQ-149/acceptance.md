# 验收标准：REQ-149


## 二 · 裁定（Dev 交付后）

### R-A · R1 与 D2 的边界：**诊断输出可以新增「提醒」，不许新增「错误」**

Dev 问：旧 mapping（不写任何新字段）现在 `validate` 会多出 `fixture_note` + `FIXTURE_LOOKS_DERIVED`（真实产物 13 条），
这算不算破了 R1「旧 mapping 行为逐字节不变」？

**裁定：不算破。但要精确划线。**

| 输出类别 | 允不允许变 |
| --- | --- |
| **产物**（`fetch` 的 JSON-LD / `publish` 的 access.jsonld / `assertions`） | ⛔ **一个字节都不许变** |
| `validate` 的 **`errors`** 与 **exit code** | ⛔ **不许新增**（0 仍 0、1 仍 1） |
| `validate` 的 **`warnings`**（诊断提醒） | ✅ **可以新增** |

**理由**：`validate` 是**体检报告**，不是产物。它的职责就是"把看不到的问题说出来"。
把"这 13 份夹具疑似反向生成"藏起来，才是真的破约——**那正是 D 项要治的病**。

**附加要求**：
1. headline 必须能一眼看出「N 条**未做字段路径校准**」/「N 条**疑似反向生成**」（Dev 已实现）
2. 若有人嫌吵，可后续加 `--quiet-notes`；⛔ 但**默认必须说**

### R-B · `paging.page` 与 `offset_base` 的语义：**接受 Dev 的裁定**

它选了 `page`（页码，推荐）+ `offset_base`（兼容写法），并明确「`offset` + `offset_base:1` 解释为**页码**，不是条数偏移」。

**接受**，理由：① 与 monitor 的真实语义一致 ② 在 `--help` 里写清了"1 基条数偏移不支持" ③ 没有为某个接口偷偷加分支。

### R-C · 它报的映射问题：**确认，去改 mapping，不改代码**

| # | Dev 报的 | 处置 |
| --- | --- | --- |
| 1 | `SslCertificate.capability.local_only` 含 `status`，template 无 `CertificateStatus` ⇒ 永远无法下推整数数组 | **改 mapping**（把 status 挪到 pushdown 并加模板），这是映射作者的错，不是执行器的 |
| 2 | `request.paging.max_limit` 存在但解析器不读（实际读实体级） | **记小债**：两处 `max_limit` 同名不同位，将来要收敛 |
| 3 | 13 个 synthetic fixture 被点名"疑似反向生成" | ✅ **这正是 D2 想要的可见性** |

### R-D · 本轮测试数据的可信度

全量 `passed=1874 / failed=60 / SKIP=3`，其中 **27 条 DB 超时是 VPN 断线所致**（不是被测代码，也不是我们压垮了集群）。

⇒ **可以采信的**：A/B/C 三形状、三个变异、零回归 8/8、D 判据、定向测试（`access` 56 / `req144_access` 9 / `req144_network` 26 / `req149_generic` 7）。
⇒ **未完成的**：全量里那 27 条需要 VPN 恢复后重跑。

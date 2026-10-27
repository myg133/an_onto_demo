# 状态：REQ-149 · 分页与过滤绑定的通用性

## 当前状态

**已完成**（2026-10-26 合并 `develop` **`9dfb8e7`** · Dev `cb2aecc` · QA **PASS（带非阻断）**）

> ⚠️ 本文件曾长期停在「已就绪（未开工）」、登记表写「进行中」，都是错的。
> 2026-10-27 由 lion（抽取端 BA）按 develop 提交与 QA 报告就地纠正。

## 为什么现在开

生成 AM（14 entity / 441 属性）时实测出三个缺口，**其中两个会直接挡住真调用**：

| # | 缺口 | 后果 |
| --- | --- | --- |
| **A** | 分页参数**类型**不可控（固定写数字） | VPC 三接口 `Offset/Limit` 文档是 String ⇒ 真调用可能 `InvalidParameter` |
| **B** | 分页**起点**不可控（固定 0 起） | monitor `PageNumber` 从 1 开始 ⇒ 第一页发 0 |
| **C** | 过滤值只能字符串 | `Array of Integer` 筛选（SSL/CDB）无法下推 |

**都是通用问题**（1-based 分页、字符串型参数、整数数组筛选在很多 API 里都有），不是腾讯特例。

## 落地结果（四个能力，均被 QA 用变异证明「撤销即红」）

| 能力 | 落地 | QA 证据 |
| --- | --- | --- |
| A · 分页值类型 `value_type` | ✅ | 服务端收到 `{"Limit":"100","Offset":"0"}`；变异后退回数字并红 |
| B · 页码起点（`page` / `offset_base: 1`） | ✅ | 第 1 页 `PageNumber=1`、第 2 页 `=2`；变异后退回 0 并红 |
| C · 绑定 integer + array | ✅ | 收到 `"CertificateStatus":[1]`（整数），⛔ 不是 `["1"]`；变异后变字符串数组并红 |
| D · `fixture` 可选（本需求灵魂） | ✅ | 四场景：exit 0 + 点名警告 + **无** `field_coverage` 键；真调用不受影响 |

## 证据链

| 项 | 位置 |
| --- | --- |
| Dev 实现 | `cb2aecc`（`[Access] REQ-149`） |
| 合并 | `9dfb8e7` |
| QA 报告（独立契约 / 独立假端点 / 独立变异） | [`qa-review.md`](qa-review.md)（PASS 带非阻断） |
| BA 裁定（产物只约束 fetch/publish/assertions；`validate` 的 warnings 可新增） | `df36eaf` |
| 部署 | `da62cb3`（uat-ops360 挂 14 类 AM · 镜像 0.1.7 含本能力） |

## 遗留（非阻断）

| # | 项 | 处置 |
| --- | --- | --- |
| 1 | `SslCertificate` 的筛选下推问题**改 mapping 不改代码** | BA 已裁定，属映射侧 |
| 2 | REQ-144 阶段 1（真 API + TC3 签名）**仍未开工** | 当前产品面已由 REQ-152 把执行器移出（`ontok-access-verify`），该阶段是否还做需重新裁定 |

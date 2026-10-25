# 需求：REQ-144 · access mapping 阶段 0（契约 + 校验 + 离线 fixture）

## 基本信息

| 项 | 值 |
| --- | --- |
| 需求编号 | REQ-144 |
| 优先级 | **P1**（"现场"路径的地基；不阻塞当前 demo） |
| 状态 | 进行中 |
| 基线 | `develop`（等 REQ-143 合入后开工） |
| 依据 | `BA/design/2026-10-25-access-mapping-contract.md`（草案 v1） |

## 要解决的问题

我们只有 R2RML（关系库）。**API 源没有映射格式** ⇒ 数字员工无法"按契约自己去调云 API"。

裁定（用户）：**给可执行描述 + 一个参考执行器**；执行者是客户侧。

## 本阶段只做三件（⛔ 不碰真 API）

| # | 产物 | 说明 |
| --- | --- | --- |
| 1 | **契约形状** | 按设计稿 v1：`sources` / `entities` / `relations`，含 `capability` / `freshness` / `identity` |
| 2 | **`ontok access validate`** | 离线自洽检查：字段路径存在、join 两端在、`iri_template` 与本体一致、`pushdown`/`local_only` 不重叠、必填项齐全 |
| 3 | **`ontok access fetch --dry-run-fixture`** | 用 fixture 跑完整取数流程（含分页、本地过滤、JSON-LD 组装），**⛔ 不发网络请求** |

## 明确不做（本阶段）

- ⛔ 不调真 API（无凭据、有费用）
- ⛔ 不做 TC3 签名（那是阶段 1）
- ⛔ 不做跨源 join（v1 边界）
- ⛔ 不引入 HTTP 客户端依赖（本阶段只需读文件；`ureq` 已在依赖里，阶段 1 再用）

## 关键裁定

| # | 裁定 |
| --- | --- |
| R1 | **执行器是 `ontok` 的子命令**（`ontok access fetch`），⛔ 不做独立二进制 |
| R2 | 输出形状与 `query serve` 的 JSON-LD **一致**（`@context`/`@graph`/`meta`），`meta` 必须带 `filtered_locally` / `freshness` / `capability_warnings` |
| R3 | `iri_template` 必须与 TTL 的 `ontok:instanceIri` **一致**（validate 必须查这条） |
| R4 | `capability` 里 `pushdown` 与 `local_only` **不许重叠**（同一字段两边都写 ⇒ validate 报错） |
| R5 | 超 `max_scan_rows` ⇒ **拒绝**（`SCAN_LIMIT_EXCEEDED`），⛔ 不许静默全量 |
| R6 | 每个 `entity` 有 `reviewed_by`；**未签的 entity，`fetch` 必须拒绝**（`UNREVIEWED_MAPPING`）——AI 提案未经人签不许取数 |
| R7 | 自环/空 join、`from`/`to` 不在 `entities` 里 ⇒ validate 报错 |

## 风险

| # | 风险 | 缓解 |
| --- | --- | --- |
| 1 | 契约形状设计过度，后期推翻 | 只在阶段 0 定**最小必要字段**；扩展靠 `contract_version` |
| 2 | validate 只查自洽，查不了"腾讯云真支持这个参数" | 明确写进文档：**能力声明需要人签**（R6 就是为此） |
| 3 | fixture 与真实响应漂移 | fixture 是脱敏真实响应；validate 检查"字段在 fixture 里存在" |

## 交付物

1. 契约 v1 的 Rust 结构 + 一份可跑的示例 `access.json`
2. `validate`：R3/R4/R7 等检查 + 可行动报错
3. `fetch --dry-run-fixture`：分页、本地过滤、JSON-LD 组装、`meta` 三项
4. 一个 fixture（可用腾讯云官方文档示例，注明来源）
5. 已知限制

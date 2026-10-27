# REQ-167 · 收掉取数脚本最后一处「猜」：分页参数兜底

- 状态：**已就绪**（P2）
- 关联：REQ-165（通用化）、REQ-166（AM 补齐）

## 一 · 问题（实测代码，不是推测）

`access_fetch.py:862-865`：

```python
offset_param = paging.get("offset_param") or req_paging.get("offset") or req_paging.get("page") or "Offset"
limit_param  = paging.get("limit_param")  or req_paging.get("limit")  or "Limit"
max_limit    = paging.get("max_limit")    or req_paging.get("max_limit") or 100
```

**"猜"的是最后那三个字面量**：`"Offset"` / `"Limit"` / `100`。
AM 没声明时，脚本会按某个厂商的命名去拼请求体 —— 与"service 从 host 猜"是同一类问题。

## 二 · AM 的两层（**都已存在**，实测）

| 层 | 键 | 真实样例 |
| --- | --- | --- |
| 实体级 `entity.paging` | `offset_param` / `limit_param` / `max_limit` / `total_path` | `{"offset_param":"Offset","limit_param":"Limit","max_limit":100,"total_path":"$.Response.TotalCount"}` |
| 请求级 `entity.request.paging` | `offset` / `page` / `limit` / `max_limit` / `value_type` / `offset_base` | `{"offset":"Offset","limit":"Limit","max_limit":100,"value_type":"string"}` |

## 三 · 要做的

| # | 改动 |
| --- | --- |
| 1 | **实体级缺 `offset_param`/`limit_param` 且请求级缺 `offset`/`page`/`limit`** ⇒ `am.invalid`，点名缺哪层哪个键 |
| 2 | **`max_limit` 缺失不再默认 100** ⇒ `am.invalid` |
| 3 | 分页为 `null` / 缺省（不分页的 entity）⇒ **保持现状**（单次取数），不报错 |
| 4 | 错误消息给"补法"示例（两层各一行） |

## 四 · 验收

| # | 判据 |
| --- | --- |
| AC-001 | 只写实体级 ⇒ 通过；只写请求级 ⇒ 通过；两层都缺 ⇒ `am.invalid` + 退出码 2 且点名 |
| AC-002 | `max_limit` 缺失 ⇒ `am.invalid`（不再静默 100） |
| AC-003 | `paging: null` 的 entity（如 `SecurityGroupRule`）⇒ 仍单次取数、不报错 |
| AC-004 | `--selftest` 全绿（含既有 86 条） |
| AC-005 | 现有 `am-tencent.json` 两层都写了 ⇒ `--check` 在分页这一项上不新增错误 |

**红绿自证**：把兜底改回去 ⇒ AC-001/AC-002 红。

## 五 · 跨面依赖

REQ-166（AM 生成器）要给**两层**都写显式值；只写一层仍会被本单拒。

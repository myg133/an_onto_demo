# 状态：REQ-127 · 查询服务提供 OWL TTL 下载

## 当前状态

**进行中**（2026-10-23 开单）

## 端点

```
GET /api/ontology.ttl                 # 客户视图（默认）
GET /api/ontology.ttl?view=model      # 建模视图（显式要）
Authorization: Bearer <QUERY_TOKEN>
```

| 项 | 约定 |
| --- | --- |
| Content-Type | `text/turtle; charset=utf-8` |
| 认证 | 只认 query token；admin token ⇒ 401（两套凭证不通用） |
| 租户 | **不注入**——本体是共享的，不含客户数据 |
| 客户视图 | ⛔ 无 tenant* / joinEvidence / primaryKey / allowedValue |
| 建模视图 | 含上述；需 `?view=model` |
| 一致性 | 与 `ontok ontology export` **sha256 相等** |
| 缓存 | ETag + `If-None-Match` ⇒ 304 |

## 待办

- [ ] Dev 实现（复用 REQ-119 序列化器 + REQ-124 导出准备）
- [ ] QA（重点：客户视图 grep 0 命中、sha256 对拍、401 语义）

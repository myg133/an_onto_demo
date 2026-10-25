# 状态：REQ-145 · 平台侧支持 API 源

## 当前状态

**进行中**（2026-10-25 开单 · P0）

## 依据

[两平台交互契约 v1](../design/2026-10-25-interaction-contract.md)

## 要解决的三个现状缺口

| 现状 | 后果 |
| --- | --- |
| 查询打到 API 类 ⇒ **空结果** | ⛔ Agent 以为"没有数据" |
| TTL 不标 `sourceKind` | Agent 写查询前不知道这类查了也没数 |
| catalog 不说走哪个入口 | Agent 会拿 SPARQL 反复试 |

## 四件事

1. TTL 加 `ontok:sourceKind "api"`（**仅 api 类**）
2. JSON-LD 加 `@pending` + `meta.complete` + `meta.mapping_version`；rows 模式加 `meta.complete`（无 pending 明细）
3. `query serve --access-mapping`：catalog/describe 报 `access`；SPARQL 打 api 类 ⇒ jsonld 给 pending / rows 报错
4. 错误码 `NOT_DB_SOURCE` / `CROSS_SOURCE_UNSUPPORTED`

## 六条裁定

R1 无映射零变化 · R2 complete 与 pending 一致 · R3 pending 不进 graph · R4 跨源边不下发 get · R5 rows 报错不返回空表 · R6 客户部署不加载映射

## 待办

- [ ] Dev
- [ ] QA
- [ ] 与 REQ-144（映射侧）会合后做一次端到端（映射 → pending → Agent 补数）

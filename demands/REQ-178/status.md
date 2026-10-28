# 状态：REQ-178 · TTL 按需生成

**已就绪**（2026-10-29 客户实测 `yiyao` 触发）

## 一句话
装配期不再渲染 TTL；`/api/ontology.ttl` 请求时渲染（按 version_id 缓存），失败只挡这一个端点
（新码 `project.ttl_failed`）⇒ 别的出口不再陪葬，"坏项目"也能继续传包修。

## 影响面
`src/query/serve.rs`（`prepare_project`）· `src/query/http.rs`（ttl 端点）· 可能 `src/query/multi.rs`（缓存）· 测试

# 状态：REQ-174 · 多项目各带各的 AM

**已合入 develop `46c9a47`**（2026-10-28）· BA 亲验 test_req174_version_am 7/7 + 回归全绿

## 一句话
包里的 AM 现在**被丢掉**（导入只读 `ontology/snapshot.json`），而运行时 AM 是**进程级**一份
⇒ 多项目没法各带各的。改成：AM 随**版本**入库，`QueryService` 按版本加载。

## 影响面
`src/db/pg.rs`（建表）· 新 `src/db/access_store.rs`（唯一读写入口）·
`src/console/provision.rs`（导入时验+存）· `src/query/serve.rs` + `multi.rs`（按版本取 AM）· 测试

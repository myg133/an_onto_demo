# develop

主开发分支，CI 构建。`feature/*` 从这里切出，PR 合并回这里。

**当前尚无代码** —— 待抽取可信度 spike 通过后初始化 Rust 骨架。

## 测试库（⚠️ 必须是专用库）

```bash
export ONTOK_TEST_DATABASE_URL=postgresql://postgres:postgres123@172.16.120.20:30432/ontok_test_scratch
```

**库名必须含 `test` / `scratch` / `tmp` / `ci` / `dev`** —— 否则 `config::test_database_url()`
的防污染闸会**直接拒绝**（`SharedDatabaseRefused`）。

> 为什么：本机能连到的库只有 UAT 那台，历史上 `ONTOK_TEST_DATABASE_URL` 常年指向
> **UAT 业务库**，测试每跑一次就塞十几个 `t_req…` 项目（实测把 Studio 塞满打不开的项目）。
> 专用库 `ontok_test_scratch` 与业务库在同一台 PG 上，互不影响。

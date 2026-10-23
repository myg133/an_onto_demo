# 状态：REQ-127 · 查询服务提供 OWL TTL 下载

## 当前状态

**已完成**（2026-10-23 · 已合并 `develop` `2eff4ce`）

## 端点

```
GET /api/ontology.ttl                 → 客户视图（默认）
GET /api/ontology.ttl?view=model      → 建模视图（显式）
Authorization: Bearer <QUERY_TOKEN>
```

## 最终结果

| 项 | 值 |
| --- | --- |
| 合并 | `develop` **`2eff4ce`**；禁改面零 diff |
| 客户视图 | 293,814 B · sha256 `a420bbce…cc070`（**与 CLI 相等**）· 建模关键词 **7/7 全 0** |
| 建模视图 | 371,356 B · sha256 `2382cd9a…fd0d0`（**与 CLI 相等**）· tenantKey 54 / joinEvidence 156 / joinVerified 157 / primaryKey 84 / allowedValue 104 |
| 认证 | 无 token / 错 token / **admin token** ⇒ 401；query token 200（**双向不通用**，QA 用真凭据验过） |
| ETag | = sha256 前 16 位；`If-None-Match` 命中 ⇒ 304 空体；**重启进程后字节不变** |
| 审计 | 18 请求 18 行，含 `view=`/`bytes=`，无明文 |
| MCP | 选 **resource**（`ontok://ontology/{customer,model}.ttl`），内容 sha256 与 HTTP 相等 |
| 测试 | lib **651**（同机基线 639，**+12**）；`test_req127_live` 2 |
| 变异 | M1 默认视图改建模 ⇒ 3 红；M2 ETag 恒定 ⇒ 2 红；M3 拆身份门 ⇒ 1 红 |

## 关键裁定

| # | 裁定 |
| --- | --- |
| **P-1** | AC-004 以**不带 `--schema`** 为准（客户视图结构上没有列名，`--schema` 只改文件头一句）。QA 按此口径验，两支 sha256 相等 |
| **P-2** | `tenantUnannotated = 0` **正确**：它是「声明缺失/未签」告警，与已签的 `tenantKey` 互斥 |
| **P-3** | 基线数字以**同机实测**为准（639，不是 645；部分 lib 测试依赖 `ONTOK_TEST_DATABASE_URL`） |

## 5 条非阻断（登记）

1. 未支持 gzip（demand 写「更好」非必须）
2. `?view=bogus`（400）与未知 resource 的审计行回退记 `view=customer`，不够精确
3. **`--tenancy` 变可选**：缺省取 `--policy` 同目录的 `tenancy.json`，取不到只 stderr 警告 ⇒ 部署漏带时 `?view=model` 会**静默少** `ontok:tenant*`。**已要求写进 runbook**
4. 两视图启动时渲染常驻 ~0.9 MB；换版本需重启（与既有 serve 语义一致）
5. `git diff develop..HEAD` 显示 Dockerfile「被删除」是分支图假象（开 PR 时 base=develop 即可）

## 附带修的两处

| 项 | 内容 |
| --- | --- |
| `pub fn build` → 私有 | 前一个 Dev 的 `pub fn build` 让 `test_req107_source_enumeration::t4` 变红（签名被判成第三个出口工厂） |
| **MSRV 修正** | `rust-version` 1.86 → **1.88**（home 0.5.12 / icu 2.3 的硬下限）；镜像工具链 1.86 → **1.96**（与本机一致） |

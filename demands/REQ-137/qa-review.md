# QA 审核：REQ-137 · 本体模式下 token 不强制租户

> Pre-merge 审核 · P1（审计诚实性）
> 审核对象：`feature/REQ-137` @ **`eab763e`**（1 个提交）· 基线 `develop` **`d433108`**
> 审核方式：独立构建（`CARGO_TARGET_DIR=D:\qa-req137-target\{feature,base,mut}`）+ 真服务实测 + 独立 worktree 变异（`qa-REQ-137-mut`，已回收）
> 审核时间：2026-10-24
> ⛔ 未改动 feature 分支（`git status` 干净，HEAD 仍是 `eab763e`）· 未 push

---

## 1 · 结论

**PASS 带非阻断（0 阻断 / 4 条非阻断）**

| 维度 | 结果 |
| --- | --- |
| AC-002 拒绝语义 | ✅ 四类值（UUID / 普通串 / 空串 / `null`）+ 数值型 ⇒ **全部 exit=1**，三段文案齐全；多 token 的第 1 / 第 2 个位置都能抓 |
| AC-002 变异（拒绝→忽略） | ✅ 单测 **红**，且变异二进制**真的起来了**并写出 `tenant=00000000-…`（现状复现，3 处命中） |
| AC-005 审计零假值 | ✅ catalog / TTL / SPARQL(503) / MCP 的审计行全部 `tenant=-`；`00000000-0000-0000-0000-000000000000` 在 stdout + stderr + 审计 + 响应体 + `/metrics` **0 命中** |
| AC-003 报错逐字未变 | ✅ 与基线二进制 **sha256 完全相同**（缺 / 空串 / `null` 三种写法各比一次） |
| AC-004 普通模式零回归 | ✅ **12 个端点逐字节相同**；84 / 952 / 70 / excluded 8；TTL 293,814 B `a420bbce…c070`；SPARQL 5 行 |
| 边界 | ✅ 多 token 的任一位置、`tenant: null`、`tenant: 12345` 都拒绝 |
| `unwrap_or_default` 可达性 | **生产不可达**（判定见 §7）⇒ 非阻断，建议改成显式 fail-closed |
| 不退化 | ✅ lib **+5 / 0 删除**（715 vs 710）；四红线 / `tools.rs` / `guard()` / `property_filter.rs` / `Cargo.*` 零 diff；HTTP 出口 7 = 7，无新增 |

> **一句话**：假租户在本体模式**进不来**（启动即拒绝）、也**写不进**审计（`tenant=-`）；普通模式一个字节都没动。剩下的都是可读性/防未来退化建议。

---

## 2 · AC-002 拒绝语义（四类值 + 变异）

### 2.1 四类值（+ 1 个额外）

同一个 token（`QUERY-qa137-ontology-aaaaaaaaaaaaaaaa`）在本体模式下只改 `tenant` 的值：

| # | `tenant` 的值 | exit | 文案三段齐全 |
| --- | --- | --- | --- |
| 1 | `"00000000-0000-0000-0000-000000000000"` | **1** | ✅ |
| 2 | `"C0001"`（随机/普通串） | **1** | ✅ |
| 3 | `""`（空串） | **1** | ✅ |
| 4 | `null` | **1** | ✅ |
| 5 | `12345`（**额外**：非字符串） | **1** | ✅ |

四类值的输出**逐字相同**（只有路径不同），原文：

```text
✗ token 表 "D:\\qa-req137-artifacts\\onto-uuid.json" 不合法：第 1 个 token（label=Some("onto")）带了 tenant，但这是本体服务模式（--ontology-only）。
  本体模式**没有业务库、没有租户概念**，SPARQL 恒 503 —— 这里的 tenant 不表示任何数据范围。
  ⛔ 不要填假租户（例如全 0 UUID）：它会让审计日志里出现一个"看起来真实"的租户。
  请从 token 里删掉 tenant 字段。
```

三段逐条对：

| AC-002 要求 | 命中文本 |
| --- | --- |
| ① 本体模式没有租户概念 | `本体服务模式（--ontology-only）` + `没有租户概念` + `SPARQL 恒 503` |
| ② ⛔ 不要填假租户（会污染审计） | `⛔ 不要填假租户` + `看起来真实` |
| ③ 请删掉该字段 | `请从 token 里删掉 tenant 字段` |

- 退出发生在 **token 表解析阶段**（`serve.rs` ③），早于 policy / schema / 项目查询 / 连库 ⇒ 拒绝路径本身不依赖数据库（本次实测环境里库是通的，但顺序保证了失败先报）。
- `null` 被拒绝是关键：`OntologyTokenSpec` 用自定义 `present_tenant` 取 `serde_json::Value`，**键出现即 `Some`**，不会像 `Option<String>` 那样把 `null` 读成「没写」。

### 2.2 变异（纪律 G）：把「拒绝」改成「忽略」

独立 worktree `qa-REQ-137-mut`（detached @ `eab763e`）+ `CARGO_TARGET_DIR=D:\qa-req137-target\mut`：

```rust
// src/query/tokens.rs · validate()
-        if mode.is_ontology_only() {
-            if let Some((i, t)) = tokens.iter().enumerate().find(|(_, t)| t.tenant.is_some()) {
-                return Err(format!(/* 三段文案 */));
-            }
-        }
+        let _ = mode;   // ☠ M1：拒绝 → 忽略（tenant 顺着原来的路流进 TokenEntry）
```

**判据 A：测试必须红** ✅

```text
---- query::tokens::tests::ontology_mode_rejects_any_tenant_field_with_an_actionable_message stdout ----
thread '...' panicked at src\query\tokens.rs:548:18:
带了 "tenant":"00000000-0000-0000-0000-000000000000", 必须拒绝:
TokenTable { entries: [TokenEntry { token: "FAKE-TOKEN-…", digest: "561d…",
  tenant: Some("00000000-0000-0000-0000-000000000000"), max_rows: 200, … }] }
test result: FAILED. 0 passed; 1 failed
```

（panic 原文里 `tenant: Some("0000…")` 就是「忽略 = 值活着」的直接证据。）

**判据 B：变异版真的能起来并写出假租户** ✅

```
· ontok query serve 就绪：绑定 127.0.0.1:18814 · 项目 ops360 第 2 版 · token 1 个 ·
  租户 1 个（00000000-0000-0000-0000-000000000000）        ← 这就是"现状"：审计里那个"看起来真实"的租户
catalog -> 200 · ttl -> 200 · sparql -> 503

audit … event=catalog … tenant=00000000-0000-0000-0000-000000000000 … rows=77 code=OK
audit … event=ttl     … tenant=00000000-0000-0000-0000-000000000000 … view=customer bytes=21335
audit … event=sparql  … tenant=00000000-0000-0000-0000-000000000000 … code=NO_DATA_SOURCE
grep 假 UUID 命中 = 3
```

✅ 变异被抓；而且「忽略」这条路**确实**会把假租户带进审计 —— 需求里「为什么不是忽略」的论证被实测证实。

### 2.3 额外变异 M2（审计诚实性反向验证）

把 `audit_call` 的租户改成「没有就编一个占位 UUID」：

```rust
- tenant: caller.token.tenant.clone(),
+ tenant: caller.token.tenant.clone().or_else(|| Some("00000000-0000-0000-0000-000000000000".to_string())),
```

`ontology_mode_audit_has_dash_tenant_and_no_placeholder_uuid` **立刻红**：

```
本体模式的审计行必须是 tenant=-（⛔ 不是空串、不是占位租户）：
audit … event=catalog … tenant=00000000-0000-0000-0000-000000000000 …
```

⇒ AC-005 的那条测试不是空转，它真能拦住「编一个看起来像真的租户」。

---

## 3 · AC-005 审计零假值（本需求的核心）

本体模式（token **不带** `tenant`）实测一次 health + catalog + TTL + SPARQL(503)：

```
audit ts=2026-10-24T09:35:26.449Z event=health  proto=rest token=-                  tenant=- q=-                   rows=-  elapsed_ms=0 code=OK
audit ts=2026-10-24T09:35:26.574Z event=catalog proto=rest token=sha256:207b4633a3b7 tenant=- q=-                   rows=77 elapsed_ms=0 code=OK
audit ts=2026-10-24T09:35:26.600Z event=ttl     proto=rest token=sha256:207b4633a3b7 tenant=- q=-                   rows=-  elapsed_ms=0 code=OK view=customer bytes=21335
audit ts=2026-10-24T09:35:26.612Z event=sparql  proto=rest token=sha256:207b4633a3b7 tenant=- q=sha256:7585c2fbaf5f86de rows=- elapsed_ms=0 code=NO_DATA_SOURCE
```

MCP 出口同样（另起一次实例）：

```
audit … event=mcp     proto=mcp token=sha256:207b4633a3b7 tenant=- q=-   rows=-  code=OK
audit … event=sparql  proto=mcp token=sha256:207b4633a3b7 tenant=- q=…   rows=-  code=NO_DATA_SOURCE
audit … event=catalog proto=mcp token=sha256:207b4633a3b7 tenant=- q=-   rows=77 code=OK
```

`tenant=-` 的来源：`AuditRecord::line()` 对 `None` 统一写 `-`（`audit.rs:180-182`，本需求未改该文件），而 `TokenEntry.tenant` 在本体模式恒 `None`。

**假 UUID 全局 grep（`-SimpleMatch`，对文本与二进制响应体都做）**

| 范围 | `00000000-0000-0000-0000-000000000000` |
| --- | --- |
| 启动 stdout（`onto.out.log`，4 行审计） | **0** |
| 启动 stderr（`onto.err.log`，10 行横幅/警告） | **0** |
| 审计文件（`onto-audit.log`） | **0** |
| health / catalog / TTL / SPARQL 响应体 | **0** |
| MCP `tools/list` / `catalog` / `sparql_query` 响应体 | **0** |
| `/metrics` | **0** |
| 合计 | **0 命中** ✅ |

附带事实：启动横幅 `· 租户 0 个（无）` —— 不是 `租户 0 个（）`、也不是凭空一个全 0 UUID。

---

## 4 · AC-003 普通模式缺 tenant 仍拒绝，且报错逐字未变

用**同一个绝对路径**的 token 文件、同一组参数，分别跑基线（`d433108`）与 feature（`eab763e`）二进制，比 stderr 的 **sha256**（不是"看起来一样"）：

| 用例 | feature exit | base exit | sha256（两侧相同） | `fc /b` |
| --- | --- | --- | --- | --- |
| 缺 `tenant` | 1 | 1 | `B799FF154F4E4B14297160C54586D6D994BE250F88FFBEB2367747522675CA77` | no differences |
| `tenant: ""` | 1 | 1 | 字节相同 | no differences |
| `tenant: null` | 1 | 1 | 字节相同 | no differences |

原文（两侧一字不差）：

```text
✗ token 表 "D:\\qa-req137-artifacts\\normal-missing.json" 不合法：tokens.json 不是合法形状：missing field `tenant` at line 1 column 125
```

```text
✗ token 表 "…\\normal-empty.json" 不合法：tokens.json 有 1 处不合法：
  - 第 1 条（label=Some("onto")） 的 tenant 是空的 —— ⛔ 没有租户就没有隔离（不会退化成「查全部」）
```

```text
✗ token 表 "…\\normal-null.json" 不合法：tokens.json 不是合法形状：invalid type: null, expected a string at line 1 column 106
```

> 实现上这是结构保证：普通模式仍走 `TokenFile` / `TokenSpec{ tenant: String }`（serde 在解析期就要求），本体模式才走 `OntologyTokenFile`。两条路各一份 Serde 形状，普通模式那条一个字节都没动。

---

## 5 · AC-004 普通模式回归（12 端点逐字节对拍）

同机同时起 feature（`:18812`）与基线（`:18813`），同一份 `schema.json` / `query-policy.json` / token / 源库，逐请求比 **状态码 + 响应体 sha256**：

| # | 端点 | 状态 A/B | 字节 | 逐字节 | sha256 |
| --- | --- | --- | --- | --- | --- |
| 1 | `/api/health` | 200/200 | 124 | ✅ | `4fc14554…3c819f89` |
| 2 | `/api/catalog?page=1&page_size=200` | 200/200 | 31,671 | ✅ | `f2cd2fb8…e4bf739` |
| 3 | `/api/catalog?page=2&page_size=200` | 200/200 | 31,980 | ✅ | `d2ee13ba…442e9e7b` |
| 4 | `/api/describe?name=:activity` | 200/200 | 2,207 | ✅ | `6423876a…4dfe49de2` |
| 5 | `/api/describe?name=:activity__id` | 200/200 | 159 | ✅ | `e29236af…afbe8e0ef` |
| 6 | `/api/sparql`（`SELECT ?m … LIMIT 5`） | 200/200 | 1,437 | ✅ | `005e4940…0abc0046` |
| 7 | `/api/sparql`（`COUNT` 聚合） | 200/200 | 948 | ✅ | `994c02a1…f7388dc` |
| 8 | `/api/ontology.ttl`（客户视图） | 200/200 | **293,814** | ✅ | **`a420bbcefb04074618710ffa8b760de9ed415ce7ed58e7e479c0e35c899cc070`** |
| 9 | `/api/ontology.ttl?view=model` | 200/200 | 371,356 | ✅ | `2382cd9a…7fd0d0` |
| 10 | MCP `tools/list` | 200/200 | 2,719 | ✅ | `93abef89…25eb00` |
| 11 | MCP `tools/call catalog` | 200/200 | 67,940 | ✅ | `c4a54471…b2d20a7` |
| 12 | MCP `tools/call sparql_query` | 200/200 | 3,066 | ✅ | `6902c2b6…6be8bb` |

**不匹配数 = 0。**

> 证据完整性说明：对拍窗口（09:36:06–09:36:07Z）之后，另有一个**非本次审核**的客户端访问过 `:18813`（基线实例的 `normal-base.out.log` 里有几条 `rows=100` / `UNAUTHORIZED` 记录不是本次探针发的）。这些请求发生在**对拍完成之后**，且被测端点是无状态的，12 个响应体的 sha256 已在窗口内固化（`normal-compare.json` + `cmp-*.bin`）。

业务判据：

| 项 | 实测 | 期望 | |
| --- | --- | --- | --- |
| catalog classes | **84** | 84 | ✅ |
| catalog properties | **952** | 952 | ✅ |
| catalog relations | **70** | 70 | ✅ |
| `excluded_relations` | **8** | 8 | ✅ |
| `skipped_classes` | 0 | 0 | ✅ |
| TTL 客户视图 | **293,814 B** / `a420bbce…c070` | 293,814 B / `a420bbce…` | ✅ |
| SPARQL `SELECT ?m … LIMIT 5` | **5 行**（列 `m`） | 5 行 | ✅ |

→ **普通模式零回归，且是逐字节级**（不是"数字对得上"）。

普通模式审计仍写**真实租户**（与基线同形）：`tenant=7d491821-27c3-4bf8-8c31-19ee5acd936d`；启动横幅两侧都是 `租户 1 个（7d491821-…）`。

---

## 6 · 边界：`tenant` 出现在不同位置

| 场景 | token 表 | 结果 |
| --- | --- | --- |
| 第 1 个有、第 2 个没有 | `[{tenant: 0000…}, {}]` | exit=1，文案指 `第 1 个 token（label=Some("a")）` ✅ |
| 第 1 个没有、第 2 个有 | `[{}, {tenant: "C0009"}]` | exit=1，文案指 `第 2 个 token（label=Some("b")）` ✅ |

- 实现是 `tokens.iter().enumerate().find(|(_, t)| t.tenant.is_some())` ⇒ 扫**全部** token，不是只看第一个；且报的是真正违规的那一条的序号。
- `tenant: null`（键在、值为 null）单独验过（§2.1 第 4 行）⇒ 拒绝。判据与实现一致：**键出现即算"填了"**。
- 本体模式合法表（无 `tenant`）随便多几个 token 也正常起 —— 只拒绝"填了"的那些。

---

## 7 · `unwrap_or_default()` 可达性判定（Dev 提的软点）

代码：`src/query/mod.rs:904`（请求体 tenant 与凭证 tenant 比对）、`:963`（`PlanInput.tenant`）。

### 判定：**生产二进制不可达**（⇒ 非阻断），但有两条「防未来退化」理由建议改成显式 fail-closed。

推理链（每条都可在代码里核对）：

1. **唯一生产构造点**是 `serve.rs:336`：`TokenTable::parse_for_mode(&raw, mode)`，与 `ServiceMode` 用的是**同一个** `mode` 变量（`serve.rs:293`）。没有第二个生产入口读 token 表。
2. 普通模式走 `TokenFile` / `TokenSpec`，其中 `tenant: String`（**不是** `Option`）⇒ serde 在解析期就要求这个字段；随后 `validate` 再拒 `""` / 纯空白。所以**普通模式的 `TokenEntry.tenant` 恒 `Some(非空)`**。
3. `Caller.token` 只来自 `svc.tokens.find(presented)`（`http.rs:205-207`）⇒ 凭证与 token 表同源，不存在"另一条路塞一个空租户进来"。
4. 本体模式在 `run_sparql_event` 的 **⓪** 就 `return Err(no_data_source)`（`mod.rs:892-896`），位置在 903/963 之前 ⇒ 那两行在本体模式**结构上**走不到。
5. 要走到 963，必须 `svc.mapping` 与 `svc.source` 同时为 `Some`；这两者只在**非本体模式**分支被填（`serve.rs:467` / `497`）⇒ 与「本体模式 + `tenant=None`」互斥。

### 但有两处值得改（非阻断）

- **lib API 层面可构造**：`ServiceParts` 与 `QueryService::new` 都是 `pub`（`query/mod.rs:566/588`，`query` 是 `pub mod`）。今天只有测试这么用；将来若有人在 `mode: Query` 下塞一张按本体模式解析的 token 表（`tenant=None`）+ mapping/source，就会带着 `tenant=""` 进入规划。**后果不是跨租户泄漏**：租户值最终经 `sparql/sql.rs:143 tenant_sql()`，空租户直接报错（`sql.rs:145-149`，测试 `an_empty_tenant_is_rejected` 覆盖）—— 也就是 fail-closed，但审计会写 `tenant=-`（普通模式里这是**另一个方向的**不诚实）。建议 `QueryService::new` 里加不变量检查（`debug_assert` 或返回 `Result`）。
- **显式 fail-closed 更好读**：两处 `unwrap_or_default()` 让"这里一定 `Some`"只写在注释里。建议改成
  ```rust
  let Some(bound) = caller.token.tenant.as_deref() else {
      // 结构上不该发生：普通模式解析期已强制 tenant
      return Err(ApiError::no_data_source());   // 或专用的内部错误码
  };
  ```
  一处绑定、两处复用，就不需要"注释即证明"。

---

## 8 · 不退化

| 项 | 结果 |
| --- | --- |
| 改动文件 | **5 个**：`src/cli.rs`(+4/-0)、`src/query/http.rs`(+68/-5，**全部在 `mod tests`**)、`src/query/mod.rs`(+13/-6)、`src/query/serve.rs`(+3/-1)、`src/query/tokens.rs`(+239/-20) |
| 四红线（`tests/drop_rate.rs`、`tests/test_fk_naming.rs`、`src/domain/view.rs`、`src/domain/review_view.rs`） | **零 diff** |
| `src/mcp/tools.rs`（含 `guard()`） | **零 diff** |
| `src/domain/property_filter.rs` | **零 diff** |
| `src/domain/pipeline.rs`（`OUTLETS`） | **零 diff** |
| `Cargo.toml` / `Cargo.lock` | **零 diff**（无新依赖） |
| HTTP 出口 | `.route(` 计数 **7 = 7**（无新增端点）；未新增需登记的出口 |
| lib 测试 | feature **715 总数 / 713 通过**；基线 **710 / 708** ⇒ **+5、0 删除**（用 `--list` 名单做集合比对，无「只在基线里」的条目） |
| 新增的 5 条 | `ontology_mode_rejects_any_tenant_field_with_an_actionable_message`、`ontology_mode_accepts_a_token_table_without_tenant`、`ontology_mode_keeps_the_other_strict_checks`、`query_mode_still_requires_tenant_with_the_same_serde_error`、`ontology_mode_audit_has_dash_tenant_and_no_placeholder_uuid` |
| `query::` 子集 | feature **102 通过 / 0 失败**；基线 **97 / 0** ⇒ +5，全绿 |
| 两处失败 | `db::apply::tests::write_snapshot_in_maps_unique_conflict_to_db_not_key_conflict`、`extract::introspect::tests::read_only_pool_really_reads_back_on`（都因为**没设** `ONTOK_TEST_DATABASE_URL`），**两侧同名同因** ⇒ 与本次改动无关 |

---

## 9 · 阻断项 / 非阻断项

### 阻断项

**无。**

### 非阻断项

| # | 项 | 说明 / 建议 |
| --- | --- | --- |
| N-1 | `unwrap_or_default()`（`mod.rs:904/963`）生产不可达，但只靠注释保证 | 建议改成显式 `let-else` fail-closed（§7 给了写法），并在 `QueryService::new` 加 mode↔token 形状的不变量检查（`pub` API 今天允许构造不匹配组合） |
| N-2 | 报错里的 label 是 Rust `Debug` 形状：`第 1 个 token（label=Some("onto")）` | 对运维略粗糙（`Some(…)` 是代码泄漏到文案）。建议 `label="onto"`，或 `None` 时整段省略。不影响任何判据 |
| N-3 | `TokenTable::parse()` 仍是 `pub` 且硬编码 `ServiceMode::Query` | 今天只有测试用。将来若有人在本体模式误用这个入口，会退回「强制 tenant」。建议标注为测试/兼容入口（`#[doc(hidden)]`），或在文档里写明必须用 `parse_for_mode` |
| N-4 | **残留风险（范围外）**：普通模式仍允许 `tenant` 填成全 0 UUID | 本需求只保证「本体模式填了就拒绝」，普通模式无法分辨真假租户。若部署侧把假 UUID 写进**普通模式**的 secret，审计里照样出现假租户 —— 建议在运维文档里写明「普通模式的 tenant 必须来自真实租户清单」 |

### 观察（不计入非阻断）

- `tokens.example.json` / `README.md` 未加本体模式示例（新规则只写在 `--help` 与模块注释里）。因为拒绝文案本身「能照做」，风险很低；建议在交付文档里补一段「本体模式的 token 长什么样」。
- 需求文档里 AC-002 的示例文案是 `第 1 个 token 带了 tenant`，实现多印了 `（label=…）`。多信息不是错，且更好定位。

---

## 附 · 复现

```powershell
# 独立构建
$env:CARGO_TARGET_DIR='D:\qa-req137-target\feature'; cd D:\MyCodes\AI\an_onto_demo\feature-REQ-137; cargo build --bin ontok
$env:CARGO_TARGET_DIR='D:\qa-req137-target\base';    cd D:\MyCodes\AI\an_onto_demo\code;          cargo build --bin ontok   # 基线 d433108

# 本体模式（不设源库）
$env:ONTOK_DATABASE_URL='postgresql://postgres:postgres@172.17.0.87:5432/anspire_real'
Remove-Item Env:ONTOK_SOURCE_DATABASE_URL
ontok query serve --bind 127.0.0.1:18815 --tokens D:\qa-req137-artifacts\onto-ok.json `
  --project ops360 --version 2 --ontology-only --audit-out D:\qa-req137-artifacts\onto-audit.log

# 普通模式（两侧各一次，端口 18812 / 18813）
$env:ONTOK_SOURCE_DATABASE_URL='postgresql://postgres:postgres123@172.16.120.20:30432/anspire_open_uat'
ontok query serve --bind 127.0.0.1:18812 --tokens local-run/query-tokens.json `
  --project anspire_r2rml_excl --version 2 --schema code\out\schema.json --policy code\query-policy.json
pwsh -NoProfile -File D:\qa-req137-artifacts\compare137.ps1
```

**原始证据**（`D:\qa-req137-artifacts\`）：
`reject-onto-{uuid,str,empty,null,num}.err.txt`、`reject-onto-multi-{1st,2nd}.err.txt`、`onto.out.log` / `onto.err.log` / `onto-audit.log`、`onto2-*` / `onto3-*`（MCP + metrics）、`normal-missing-{feature,base}.err.txt`、`normal-{empty,null}-{feature,base}.err.txt`、`cmp-{A,B}-*.bin` + `normal-compare.json`（12 端点）、`mut1-{out,err,audit}.log`（变异版假租户复现）、`tests-{feature,base}.txt`（测试名单）。

变异在独立 worktree `qa-REQ-137-mut`（detached @ `eab763e`）+ 独立 `CARGO_TARGET_DIR=D:\qa-req137-target\mut` 完成，**已回收**；feature 分支未改动、未 push。

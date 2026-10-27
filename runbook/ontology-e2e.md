# 端到端 Runbook · 从源库到数字员工可查

> 目标：**一条命令接一条命令**，把一个业务库变成「Agent 能用 SPARQL 查、且租户隔离」的本体服务。
> 全部命令已在 `anspire_open_uat`（96 表）上实跑通过；产物与判据见各 REQ 的 status。

---

## 0 · 前置

| 项 | 说明 |
| --- | --- |
| 元数据库 | PostgreSQL，存本体/提案/快照（本机 podman `172.17.0.87:5432`） |
| 源库 | **只读**账号（本机用 `172.16.120.20:30432`） |
| LLM | 可选；带 `--llm` 时需 `ONTOK_LLM_*` |

```powershell
cd D:\MyCodes\AI\an_onto_demo\code
cargo build -q                      # 一次编译，后面用 target\debug\ontok.exe

$env:ONTOK_DATABASE_URL      = "postgresql://postgres:postgres@172.17.0.87:5432/anspire_real"   # 元数据库
$env:ONTOK_SOURCE_DATABASE_URL = "postgresql://postgres:postgres123@172.16.120.20:30432/anspire_open_uat"  # 源库（只读）

$env:ONTOK_LLM_BASE_URL = "https://ollama.apps.anspire.cn/v1"
$env:ONTOK_LLM_API_KEY  = "<你的 key>"
$env:ONTOK_LLM_MODEL    = "qwen36-27B"
```

---

## 1 · 勘查源库（只读）

```powershell
.\target\debug\ontok-introspect.exe --out out\schema.json
```

**看什么**：表/列数、有注释比例、**外键数（0 说明关系全靠推断）**、单列主键比例。

---

## 2 · AI 提案（三个决策点，全部只产提案、不落库）

> ADR-005：决策点由 AI 先提案，**人签后才生效**。

```powershell
# ① 表名/属性名
.\target\debug\ontok.exe name suggest --schema out\schema.json `
    --out out\names.proposal.json --as-names out\table_names.json --budget-tokens 60000

# ② 哪些表不进本体
.\target\debug\ontok.exe exclude suggest --schema out\schema.json `
    --out out\excl.proposal.json --budget-tokens 60000

# ③ 业务概念分组（可选）
.\target\debug\ontok.exe concept suggest --ai --project <项目名> --version 1 `
    --out out\groups.proposal.json --budget-tokens 20000
```

| 产物 | 怎么用 | 注意 |
| --- | --- | --- |
| `names.proposal.json` | 人审后用 `--as-names` 的产物喂 `--names` | 提案带 `claim/evidence/confidence/reason` |
| `excl.proposal.json` | **需转成 REQ-113 的 exclusions 形状**（`exclude:[{table,category,reason,decided_by}]`）才能喂 `--exclude` | 默认多排除，宁多不少 |
| `groups.proposal.json` | `concept import` 走双轨审核 | `source: ai` vs 算法版 `rule` |

**第二遍同输入 = 0 token**（指纹缓存）。

---

## 3 · 抽取 + 关系证据（候选 → 签字 → 注入）

### 3.1 先出 FK 候选（未签）

```powershell
.\target\debug\ontok-propose.exe --schema out\schema.json --names out\table_names.json `
    --exclude exclusions.json --tenancy tenancy.json `
    --src $env:ONTOK_SOURCE_DATABASE_URL `
    --project <项目名> --out out\proposal-draft.json --stats out\stats-draft.json `
    --fk-out out\fk-proposals.json
```

产出：`fk-proposals.json`（每条带 `claim/evidence/confidence/reason/facts`，`reviewed_by: null`）+ `fk-unresolved.json`（没解析出来的列 + 原因）。

### 3.2 签字（**这一步必须人工**）

```powershell
# 人工评审 fk-proposals.json 后，把要采纳的条目 reviewed_by 改成签字人
# ⚠️ 下面是「示例」，生产里必须真的有人看过 evidence 再签
$p = Get-Content out\fk-proposals.json -Raw | ConvertFrom-Json
$p.proposals | Where-Object { $_.band -eq 'strong' } | ForEach-Object { $_.reviewed_by = "张三@2026-10-23" }
$p | ConvertTo-Json -Depth 12 | Set-Content out\fk-signed.json -Encoding utf8
```

| 规则 | 说明 |
| --- | --- |
| `band = strong` | 值域重叠 ≥95%，可签 |
| `band = inherit` | 同名列继承，**租户键列禁止签**（会被拒） |
| `band = medium` | 需更强理由 |
| `reviewed_by = null` | **不会进快照**（P2） |

### 3.3 正式抽取（带签字边）

```powershell
.\target\debug\ontok-propose.exe --schema out\schema.json --names out\table_names.json `
    --exclude exclusions.json --tenancy tenancy.json `
    --src $env:ONTOK_SOURCE_DATABASE_URL `
    --fk-sign out\fk-signed.json `
    --project <项目名> --out out\proposal.json --stats out\stats.json `
    --pending-out out\proposal-pending.json --rejected-out out\proposal-rejected.json `
    --llm --budget-tokens 200000
```

**看什么**：

```
表：勘查 96 − 排除 12 = 抽取 84（闭合必须是 true）
关系：高/中/低/拒绝 四档；主提案 = 高+中
token：首次实付；第二次 0（缓存）
```

---

## 4 · 入库 → 审核 → 应用 → 冻结

```powershell
.\target\debug\ontok.exe project new --name <项目名>
.\target\debug\ontok.exe version new --project <项目名>

.\target\debug\ontok.exe propose import --file out\proposal.json
.\target\debug\ontok.exe review decide --proposal p1 --project <项目名> --accept-all
.\target\debug\ontok.exe proposal apply --proposal p1 --project <项目名>
.\target\debug\ontok.exe version freeze --project <项目名> --version 2
```

> ⚠️ 提案别名 **最新在前**：刚导入的是 `p1`，本体提案退为 `p2`。

**逐条审**（不 `--accept-all` 时）：

```powershell
.\target\debug\ontok.exe review show --proposal p1 --project <项目名>
.\target\debug\ontok.exe review decide --item 4 --track biz --accept --project <项目名>
```

**批量按建议**（页面/CLI 均由 `enters_batch()` 决定）：

```powershell
# 冲突/无建议/拒绝档永远不进批量
.\target\debug\ontok.exe review decide --proposal p1 --project <项目名> --accept-all
```

---

## 5 · 导出本体（TTL）

```powershell
# 客户视图（给客户看）
.\target\debug\ontok.exe ontology export --project <项目名> --version 2 `
    --out out\customer.ttl

# 建模视图（给数字员工/映射用：含主键、租户列、枚举值、真实类型）
.\target\debug\ontok.exe ontology export --project <项目名> --version 2 --model-view `
    --schema out\schema.json --tenancy tenancy.json `
    --out out\model.ttl

# 业务总览（人看）
.\target\debug\ontok.exe ontology overview --project <项目名> --version 2
```

**验收**：

| 检查 | 期望 |
| --- | --- |
| `owl:inverseOf` | = 关系数 × 1（每条边都有反向） |
| `joinVerified "inherit"` | 少量；没有 `true` 的边不该是多数 |
| `allow` 空 | 客户视图里 `tenant*` 出现 **0** 次 |
| 两次导出 | sha256 相同 |

---

## 6 · 生成 R2RML 映射

```powershell
.\target\debug\ontok.exe r2rml generate --project <项目名> --version 2 `
    --schema out\schema.json --exclude exclusions.json `
    --out out\mapping.ttl
```

**看什么**（回执里）：

```
84 个 TriplesMap / 70 joinCondition / 0 张已排除的表 / schema 对账 0 失败
```

| 现象 | 含义 |
| --- | --- |
| `false` 边出现在 join 里 | ❌ 严重缺陷（应永不生成） |
| 被排除的表出现在映射里 | ❌ 不应发生 |
| `schema 对账 N 失败` | 列名写错，映射不可用 |

---

## 7 · 起查询服务

### 7.1 两份声明

**`tenancy.json`**（租户列，`--tenancy` 用）：

```json
{ "version": "1.0.0", "columns": ["customer_id", "main_customer_id"], "reviewed_by": "张三@2026-10-23" }
```

**`query-policy.json`**（**查询期强制口径**，ADR-006）：

```json
{
  "version": "1.0.0",
  "tenantSubject": "customers",
  "tenantIdColumn": "id",
  "perTable": {
    "deductions": { "tenantColumn": "customer_id", "coverage": "main_only",
                    "coverageNote": "main_customer_id 全空（0/4）；子账号扣费不在结果内，金额可能偏低",
                    "reason": "数据事实与取舍…" }
  },
  "publicDimensions": [
    { "table": "package_templates", "reason": "套餐定义为产品目录，对所有客户相同" }
  ],
  "reviewed_by": "张三@2026-10-23"
}
```

> `perTable` 未登记、又不在 `publicDimensions` 的表 ⇒ **不可查**（fail-closed）。

**`tokens.json`**（凭证 → 租户，**唯一租户来源**）：

```json
{ "tokens": [
    { "token": "REPLACE_ME_A", "tenant": "<客户A的 customers.id>", "max_rows": 200, "max_concurrency": 4 }
] }
```

### 7.2 启动

```powershell
.\target\debug\ontok.exe query serve --bind 127.0.0.1:8800 `
    --tokens tokens.json `
    --project <项目名> --version 2 `
    --schema out\schema.json --policy query-policy.json
```

启动横幅会打印：绑定地址、项目/版本、token 数、租户 id、**只读会话读回值**。
⚠️ 绑 `0.0.0.0` 需 `--allow-public`，否则拒绝启动。

---

## 7.3 · 工作台前端（REQ-125）与「两服务同源」

`ontok-builder console serve` 自带页面（`GET /`），但它**只含管理端点**；页面里的「查询台 / 目录浏览」要调的是
**REQ-123 的 `query serve`**（另一进程、另一套 query token）。

```
浏览器 ──▶ 同源反代 ──┬─▶ console serve  :8899   （管理 API + 页面）
                      └─▶ query   serve  :8898   （/api/sparql、/api/catalog、/api/describe）
```

**只起 `console serve` 时，查询台会如实报「查询服务未接入」**（这是设计，不是 bug）——它不会假装空结果。

最小示例（本机开发用）：`tests/fixtures/req125/dev_proxy.ps1`

```powershell
# 一个进程起两个服务 + 反代
pwsh -File testsixtures
eq125\dev_proxy.ps1
# 浏览器打开 http://127.0.0.1:8897/ （代理端口）
```

生产部署：由网关/反代把两个服务挂到同一源下（`/api/sparql|/api/catalog|/api/describe` → query serve，其余 → console）。

> ⛔ 不要把查询端点挂到 admin 端口——那会把 query token 的面带到管理服务上，也会破坏「不为前端新开端点」的纪律。

---

## 8 · 查询（REST 与 MCP）

### 8.1 REST

```powershell
$H = @{ Authorization = "Bearer REPLACE_ME_A"; "Content-Type" = "application/json" }

# 能问什么
Invoke-RestMethod http://127.0.0.1:8800/api/catalog -Headers $H | ConvertTo-Json -Depth 4

# 查数据
$body = @{ query = 'SELECT ?d ?amt WHERE { ?d :deductions__deduction_amount ?amt }' } | ConvertTo-Json
# ⚠️ 默认回执是 JSON-LD 图；下面这条要的是**表格**，所以显式 ?format=rows
Invoke-RestMethod 'http://127.0.0.1:8800/api/sparql?format=rows' -Method Post -Headers $H -Body $body | ConvertTo-Json -Depth 5
```

### 8.2 MCP（JSON-RPC，同一 token）

```powershell
$body = @{
  jsonrpc = "2.0"; id = 1; method = "tools/call"
  params = @{ name = "sparql_query"
              arguments = @{ query = 'SELECT ?d ?amt WHERE { ?d :deductions__deduction_amount ?amt }' } }
} | ConvertTo-Json -Depth 6

Invoke-RestMethod http://127.0.0.1:8800/mcp -Method Post -Headers $H -Body $body | ConvertTo-Json -Depth 6
```

> 同一查询经 REST 与 MCP，回执**逐字节相同**（REQ-123 AC-004，8 组 sha256 已验）。

### 8.3 回执里必须读的四样

| 字段 | 为什么 |
| --- | --- |
| `warnings` | **漏数据的唯一提示**（`coverage: main_only`） |
| `coverage[]` | 哪些表的过滤不完整 |
| `join_states[]` + `used` | 结论依赖的边是实测/继承/仅名字推断；`used:false` = 涉及但没用到 |
| `truncated` | 结果被截断，别当全量 |

---

## 9 · 验证（跑完必做）

### 9.1 租户隔离（最重要）

```powershell
# 用 A 的 token 查，结果里不得出现 B 的任何行
# 要逐行比主键 ⇒ 必须显式 format=rows（默认是图，没有 rows）
$A = Invoke-RestMethod 'http://127.0.0.1:8800/api/sparql?format=rows' -Method Post -Headers $H -Body $bodyA
$B = Invoke-RestMethod 'http://127.0.0.1:8800/api/sparql?format=rows' -Method Post -Headers $HB -Body $bodyB
# 逐行比对两边的主键集合，交集必须为空
```

### 9.2 反向攻击

```powershell
# ① body 带别人的 tenant ⇒ 必须 403 TENANT_MISMATCH
# ② token 放 query string ⇒ 必须 401
Invoke-RestMethod "http://127.0.0.1:8800/api/sparql?token=REPLACE_ME_A" -Method Post
```

### 9.3 声明生效

```powershell
# 把 query-policy.json 里 deductions.tenantColumn 改成另一列 ⇒ 重启后 sql_echo 的 WHERE 必须跟着变
# 把 coverage 改成 complete ⇒ warnings 必须消失
```

---

## 10 · 清场

```powershell
Remove-Item Env:ONTOK_LLM_API_KEY    # 别留在会话里
# 元数据库里的临时项目：只删本轮的，别动生产
```

---

## 11 · 常见故障

| 现象 | 原因 | 处理 |
| --- | --- | --- |
| `租户过滤无法确定：表 X` | `query-policy.json` 没登记该表 | 加进 `perTable` 或 `publicDimensions` |
| `TENANT_MISMATCH` | body 里的 tenant ≠ token 绑定 | 删掉 body 的 tenant，只靠 token |
| `UNKNOWN_PROPERTY` | 属性名写错 | 看 `hint` 里的可用属性，或 `GET /api/describe` |
| `SPARQL_UNSUPPORTED` | 用了 OPTIONAL/UNION 等 | v1 不支持，改写成多跳 BGP |
| 查询返回 0 行但确信有数据 | 枚举值猜错 / `coverage: main_only` 漏子账号 | 先 `sample` 看真实值；再看 `warnings` |
| 映射里出现被排除的表 | 用了未排除的快照 | 快照必须带 `--exclude` 重建 |
| 第二次抽取仍花钱 | `ai_cache` 在**元数据库**里，换库即冷启动 | 同一元数据库内才会命中 |

---

## 12 · 这套流程的边界（别越过）

| 限制 | 说明 |
| --- | --- |
| 关系语义 | 79 条边里 70 条有数据证据；`false` 的 7 条**永不进映射** |
| 扣费/调用表数据稀少 | UAT 里 `deductions` 4 行、`api_call_logs` 0 行 ⇒ 部分边无法验证 |
| 子账号 | 主账号能看到子账号数据**取决于声明**；`main_only` 会漏且报警 |
| SPARQL | 只支持 BGP/FILTER/LIMIT/ORDER BY 单列/聚合；⛔ 不支持 OPTIONAL/UNION/子查询 |
| 一对多 | 一表一租户列；多列租户（如 `from_customer_id`/`to_customer_id`）需拆查询 |


---

## 12.5 · 客户门户（REQ-126）

### 与工作台的根本区别

| | 工作台 `console serve` | **门户 `portal serve`** |
| --- | --- | --- |
| 凭证 | admin token（可跨项目调试） | **portal token（绑定一个租户）** |
| 能切项目/版本 | 可以 | **不能**（进程级固定） |
| 写操作 | 有 | **完全没有** |
| 写 SPARQL | 可以 | **不能**，只给预置问题 |
| 看别人数据 | 调试时（审计） | **结构上不可能** |

### 启动

```powershell
\target\debug\ontok.exe portal serve --bind 127.0.0.1:8900 `
    --tokens portal-tokens.json `
    --project <项目名> --version 2 `
    --schema out\schema.json --policy query-policy.json `
    --questions portal-questions.json --audit-out portal-audit.log
```

**端点只有四个**：`GET /`（页面壳，零数据）、`GET /api/questions`（标题+说明，不含 SPARQL）、`POST /q/<id>`、`GET /api/health`。
⛔ 没有 `/api/sparql` —— 客户不能自由查询。

### 问题声明 `portal-questions.json`

```json
{ "version": "1.0.0", "reviewed_by": "业务负责人@2026-10-23",
  "questions": [
    { "id": "charges_by_currency",
      "title": "我的扣费明细（按币种汇总）",
      "sparql": "SELECT ?cur (SUM(?amt) AS ?total) WHERE { ?d :deductions__currency ?cur . ?d :deductions__deduction_amount ?amt . } GROUP BY ?cur",
      "note": "按币种汇总；覆盖范围见结果页警告" } ] }
```

**启动时逐题校验（fail-closed）**，任一条不合格 ⇒ **拒绝启动**并点名：

| 校验 | 为什么 |
| --- | --- |
| `reviewed_by` 非空 | 问题本身就是业务口径 |
| 涉及的表在 policy 里登记为租户表或公共维度 | 否则无法注入租户 |
| 属性存在、语法可规划 | 别等客户点开才 500 |
| **不投影实例 IRI**（`#表名/uuid`） | 会同时泄漏项目名与表名 |
| **id 不含表名** | id 会出现在 URL / 问题列表 / 审计行 |
| 结果列名不与真实列名同名 | 同名等于泄漏列名 |
| 涉及的表必须有业务名（`display_name`） | 否则只能显示表名 |

⚠️ **反例见 `BA/demands/REQ-126/portal-questions.draft.json`**——那份草稿正是被启动校验拦下的写法。

### 页面只给业务信息

- 结果列 = 业务对象名（如「扣费明细」），**不是**表名
- 覆盖警告：`main_only` 的表 ⇒ 「只覆盖主账号：子账号产生的部分不在结果里，金额可能偏低」
- `truncated` ⇒ 「只显示前 N 行，还有更多没有显示」
- ⛔ 响应体里没有 `sql_echo` / 表名 / 列名 / IRI（**类型层就没有这些字段**，不是运行时删除）

### 限额（两条，都来自凭证）

| 字段 | 默认 | 上限 | 生效方式 |
| --- | --- | --- | --- |
| `max_rows` | 200 | 10000 | 截断 + 可见警告 |
| `max_concurrency` | 2 | 64 | 超限 **立刻 429**（⛔ 不排队），按 token 隔离 |

### 验证（跑完必做）

```powershell
# ① 租户锁死：body 带 tenant ⇒ 403；URL 带 tenant ⇒ 400
# ② 两租户对拍 ⇒ 交集必须为空
# ③ 结果体 grep 表名/SELECT/https:// ⇒ 0 命中
# ④ 并发：同 token 三连击（cap=2）⇒ 2×200 + 1×429，且 429 的审计 elapsed_ms ≈ 0（证明没排队）
```

---

## 13 · 三个界面的分工（别混用）

| 界面 | 命令 | 给谁 | 能做什么 |
| --- | --- | --- | --- |
| 客户视图 | `ontok serve` | 业务/客户 | 看图、看对象、看总览；导出 TTL |
| 内部工作台 | `ontok-builder console serve` | FDE/实施 | 项目、评审、导出、声明编辑、审计、查询台（+反代） |
| 客户门户 | `ontok portal serve`（REQ-126） | 客户 | 只能点**预审过的问题**，租户锁死，无自由查询；结果只给业务名+覆盖警告 |

**三套凭证、三个端口、互不复用。**

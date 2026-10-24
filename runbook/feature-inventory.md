# 功能清单（`develop` `5163cfb`，2026-10-24）

> 一份**按能力**而不是按需求的清单：做什么、入口在哪、边界是什么。
> 需求编号只是溯源，不是重点。

---

## 一 · 全景

```
输入            加工                      产物                  消费
─────────────────────────────────────────────────────────────────────
关系库（只读）→ 勘查 → 提案 → 人签 → 冻结 → TTL / R2RML  → 查询服务 → 数字员工
业务文档      → 摄取 → 合并 → 签字  ↗                    → 工作台   → FDE
                                                          → 门户     → 客户
```

---

## 二 · 摄取与建模

| 能力 | 入口 | 说明 | 边界 |
| --- | --- | --- | --- |
| **源库勘查** | `ontok-introspect --out schema.json` | 只读 pg_catalog：表/列/类型/主键/**唯一列**/注释 | 不搬数据 |
| **表名提案** | `ontok name suggest` | AI 给表/属性中文名，带证据与置信度 | 提案不权威，需人签 |
| **排除提案** | `ontok exclude suggest` | AI 判哪些表不进本体 | 同上 |
| **概念分组** | `ontok concept suggest [--ai]` / `import` / `digest` | 业务概念分组（算法版 `rule` / AI 版 `ai`） | 算法版不许冒充 AI |
| **关系抽取** | `ontok-propose --fk-out` | 对 `*_id` 列做**值域重叠**实测，产出关系提案 | 阈值：≥95% strong / 50–95% medium / <50% 否决 |
| **文档摄取** | `ontok doc ingest --input docs/` | `.md`/`.txt` → **带逐字证据**的断言 JSONL | ⛔ 不支持 PDF/Word；指纹缓存，二次 0 token |
| **文档合并** | `ontok doc merge` | 断言 + 提案 → 合并提案（机械对齐） | ⛔ **不猜语义**；无映射进 `doc_unmapped` |
| **签字** | `ontok doc sign --by <人>` | 给 doc 条目签 `reviewed_by` + `signed_at` | 不许匿名；幂等；不就地改 |
| **成本闸** | `--budget-tokens` | 超预算即停并上报 | 不产出半份文件 |

---

## 三 · 评审与版本

| 能力 | 入口 | 说明 |
| --- | --- | --- |
| **提案导入** | `ontok propose import --file` | 提案入库，得到别名（最新在前） |
| **双轨评审** | `ontok review decide --track biz\|data` | 业务/数据两轨各自接受或拒绝 |
| **批量按建议** | `ontok review decide --accept-all` | 冲突/无建议/拒绝档**永不进批量** |
| **评审页面** | `ontok review serve` | 浏览器逐条审（回环） |
| **应用** | `ontok proposal apply` | 写入本体；`applicable:false` 的条目**拒绝并点名** |
| **版本冻结** | `ontok version freeze` | 不可变快照；冻结后再 apply ⇒ 409 |
| **置信度校准** | `ontok confidence calibrate` | REQ-108 的分档校准 |

---

## 四 · 产物

| 产物 | 入口 | 内容 |
| --- | --- | --- |
| **OWL/TTL（客户视图）** | `ontok ontology export` | 类/属性/关系/反向边；⛔ 无列名/无租户列/无主键 |
| **OWL/TTL（建模视图）** | `ontology export --model-view --schema --tenancy` | 加上主键、枚举值域、租户列、**join 证据等级** |
| **R2RML 映射** | `ontok r2rml generate` | 类→表、属性→列、**只有 `true` 边生成 join** |
| **总览** | `ontok ontology overview` | 人读的统计 |
| **差异** | `ontok ontology diff` | 两版对比 |

**证据等级（TTL 里可查）**：

| 值 | 含义 | 进 R2RML？ |
| --- | --- | --- |
| `true` | 值域重叠实测过 | ✅ |
| `inherit` | 同名列在别处验过 | ❌ 默认不生成 |
| `false` | 仅名字推断 | ⛔ 永不生成 |

---

## 五 · 服务（三个入口，三套凭证）

| 服务 | 端口 | 凭证 | 能力 |
| --- | --- | --- | --- |
| **query serve** | 8800 | query token（绑租户） | `POST /api/sparql` · `GET /api/catalog` · `/api/describe` · **`/api/ontology.ttl[?view=model]`** · `/mcp`（JSON-RPC + SSE） |
| **console serve** | 8899 | admin token | 项目/评审/应用/冻结/导出 TTL/生成 R2RML/**声明读写（保存前校验）**/审计/评审页/查询台 |
| **portal serve** | 8900 | portal token（绑租户） | 只给**预置问题**；无 `/api/sparql`；结果带覆盖警告 |

**三套 token 的 `kind` 互斥**：拿 admin token 调 query 服务 ⇒ 401，反之亦然。

### 查询服务的硬约束

| 约束 | 实现 |
| --- | --- |
| 租户**只来自 token** | 请求体/URL 带 tenant ⇒ 403/400 |
| 注入**可见** | `sql_echo` 就是执行的 SQL（`pg_stat_activity` 对拍过） |
| 漏数据**要报警** | `coverage: main_only` ⇒ `warnings[]` |
| 关系可信度**要透明** | 回执带 `join_states[]` + `used` |
| 不支持就**报错** | 26 条拒绝用例（OPTIONAL/UNION/子查询/路径/REGEX…） |

---

## 六 · 前端

| 页面 | 地址 | 特点 |
| --- | --- | --- |
| **内部工作台** | `console serve` 的 `/`（59,988 B 单文件） | 壳**零数据**、token 只进 `sessionStorage`、查询台显示 warnings/coverage/used=false/truncated |
| **客户门户** | `portal serve` 的 `/`（13,692 B） | 租户锁死、只点预置问题、结果只给业务名+覆盖警告、⛔ 不露表名/列名/SQL |
| **本体视图** | `ontok serve`（REQ-093） | 只读看图：拖拽、碰撞、折叠、导出 |

前端**零外部依赖**（无 CDN、无构建链），CSP + nosniff + no-referrer。

---

## 七 · MCP

| 形态 | 入口 | 工具/资源 |
| --- | --- | --- |
| **stdio** | `ontok mcp-server` | 暴露冻结快照（本体结构） |
| **HTTP/SSE** | `query serve` 的 `/mcp` | `catalog` / `describe` / **`sparql_query`** / `sample` + **resources**（`ontok://ontology/{customer,model}.ttl`） |

---

## 八 · 工程与运维

| 能力 | 位置 |
| --- | --- |
| 镜像 | `code/Dockerfile`（alpine + rust 1.96 + bun + pi/pi-acp）：四个二进制 + 抽取 agent |
| 单机编排 | `Deploy/compose/`（db + query + console + portal） |
| K8s | `Deploy/k8s/`（kustomize：base + overlays/dev + prod），非 root / 只读根 / 丢能力 |
| 加新项目 | `Deploy/PROJECT-ONBOARDING.md`（六步 + 文档挂载） |
| 构建 | `Deploy/BUILD.md` |
| 环境坑 | `Deploy/LOCAL-ENV-ISSUES.md`（cgroup、libstdc++、pi shebang） |
| 操作手册 | `BA/runbook/`：`ontology-e2e.md` / `pipeline-flow.md` / `doc-to-ontology.md` |
| 红灯台账 | `BA/dispatch/baseline-reds.md` |

---

## 九 · 已知边界（明确不做的）

| # | 边界 | 原因 |
| --- | --- | --- |
| 1 | 文档只吃 `.md`/`.txt` | PDF/Word 需外部转换 |
| 2 | 文档↔表的**语义映射**不做自动猜测 | 是业务判断，猜错会"能跑、不报错、结果错" |
| 3 | SPARQL v1 不支持 OPTIONAL/UNION/子查询/路径/REGEX | 支持集明确，不支持**报错**而非静默 |
| 4 | 一套服务 = 一个项目 + 一个版本 | 隔离靠部署边界（最硬）；>10 项目应改架构 |
| 5 | 门户不做自由查询 | 能力/安全/诚实三条 |
| 6 | 子账号用户需先解析成主账号 | ADR-006 Limitation |
| 7 | `deductions` 4 行 / `api_call_logs` 0 行 | UAT 数据稀疏，相关边无法验证 |
| 8 | 检索/向量/图推理 | 未做 |

---

## 十 · 代码里能查到的事实（不是承诺）

```
src/                      ~80 个模块
lib 测试                  689 passed
集成测试                  ~40 个 target（含真库 live 套件，缺夹具响亮 SKIP）
OUTLETS 登记              62 条（客户 17 / 建模 10 / 内部 32 / 静态 2 / 错误路径 1）
TTL                      客户视图 293,814 B / 建模视图 371,356 B（84 类 / 78 关系）
R2RML                    84 TriplesMap / 70 joinCondition
```

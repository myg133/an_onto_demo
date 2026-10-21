# 状态：REQ-111 · 本地审核界面

## 当前状态

**已完成**（2026-10-21 · 已合并 `develop` `4178a4e`，真实数据演示通过）

## 前置已满足

REQ-110 已合并（`develop` `f624c1a`）：页面可达（`/` 免 token、零数据壳）、`/api/*` gated、TTL 导出内置。
本需求直接基于 `feature-REQ-111`（base `f624c1a`）。

## 为什么必须是独立命令

`serve` 的硬承诺：**只接受 GET，任何改动类请求一律 405，没有可写的地方**（REQ-093 判据）。
给它加「审核模式开关」会把**结构保证**降级成**配置保证**——本仓库反复栽的坑。

⇒ `ontok review serve`：只回环、只服务**未冻结**提案、**唯一写点** `POST /api/decide`。

## 痛点数字

142 条目 × 2 轨 = **284 次进程启动**（REQ-003 验收原话：「E2E 根本跑不完，15 分钟才走一半」）。

## 关键裁定

| # | 裁定 |
| --- | --- |
| R1 | 写接口必须 Bearer token，只绑 `127.0.0.1`/`::1` |
| R2 | 同一 `item+track` 再签 = 改判（幂等覆盖） |
| R3 | **复用 `review decide` 的领域函数**，⛔ 不许 HTTP 层直写 SQL |
| R4 | 独立 HTML，不复用 `serve` 页面 |
| R5 | 失败必须让页面看到具体原因 |

## 待办

- [ ] Dev 实现（`review serve` / 两个端点 / 页面 / 审计）
- [ ] QA
- [ ] 实测「142 条目逐条签」的耗时对比

## Dev 交付摘要

| 项 | 结果 |
| --- | --- |
| `review serve` | `ReviewCmd::Serve`，默认 `127.0.0.1:8788`，非回环 ⇒ exit 1 |
| 决策复用 | **唯一实现** `db/proposal.rs:317 decide_item`，两个调用点（CLI / UI）；**唯一 UPDATE** `set_decision_via` |
| `review_ui` 里的 SQL | **0 命中**（UPDATE/INSERT/DELETE/sqlx 全无） |
| `serve` 是否被动 | `git diff develop -- src/server/http.rs` = **0 行**；POST 仍 405 |
| 冻结拒绝 | 启动 exit 1；运行中提交 ⇒ **409 + 具体原因**，且不写入 |
| 审计 | `via = "ui"` / `"cli"`，`decided_at` 有值 |
| 耗时 | CLI **59.59 s** vs 页面路径 **7.69 s**（**7.75×**；口径=270 个 HTTP 请求墙钟，不含渲染与人手点击） |
| 测试 | lib 245 / doc 4 / 32 基线套件 313 / 新套件 12 ⇒ 36 套件 **349** |

**待裁定项**：`src/review_ui/` 未进 `OUTLET_FILES`（T5/T9 不扫）⇒ 见 `acceptance.md` A-1（已接受，附条件）。

## QA 结果与合并

| 轮次 | 结论 | 关键 |
| --- | --- | --- |
| 首轮 | **PASS 带 9 条非阻断** | 决策唯一性经变异验证（M1 写死 `via` ⇒ 2 条红；M2 直写 SQL ⇒ 1 条红）；`review_ui` 的 customer-track 调用 **0**（A-1 前提成立）；`serve` 与 develop **blob 相同** |
| BA 改判 | **N9 改判阻断** | `apply` 之后仍在运行的服务返回 200「改判成功」——**回执失信** |
| 修复 | `ab42894 → 00cfa1b` | `ensure_unfrozen` 增查 `proposal.status == pending`；`frozen` 字段改真实值；两处文档订正 |
| 复审 | 14 passed（含 2 条新回归） | 409 + 库逐行不变 + 对照组 200 |

| 合并 | `develop` **`4178a4e`**（`--no-ff`）；红线 + `src/server/http.rs` 相对 `f624c1a` **numstat 空**；合并后 `cargo check --lib` ✅ |

## 真实数据演示（`review_demo` / 提案 p1 / 135 条目）

| 步骤 | 实测 |
| --- | --- |
| 页面壳 `GET /` | 200 · 15.3 KB · **不含项目名** |
| `GET /api/review` | 135 条目 · `frozen=false` · 待签 biz/model 各 135 |
| 预览 `min_confidence=0.9` | **count=39**（`below_band=13` / `decided=1` / `no_confidence=82`） |
| 预览 `min_confidence=0.6`（model） | **count=52** |
| 参数校验 | `min_confidence=1.5` / `scope=all` / `track=xx` ⇒ **全部 400** |
| `POST /api/decide` item4 | **200** · `biz:"accepted"` · `via.biz:"ui"` · 时间戳有值 |
| 读回 | `item4: biz=accepted via=ui`；进度 `accepted=1 / pending=134` |

> ⚠️ 演示时我自己踩过一个坑：默认参数名是 **`min_confidence`**，我写成 `min_score`，被静默忽略（不过滤=全量 135）。
> **不是实现 bug，但说明「拼错参数名 ⇒ 静默降级为全量」是个真实的误用风险**，已建议下一轮对未知 query 参数报 400。

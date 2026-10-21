# 状态：REQ-110 · 内置 TTL 导出 + 页面查看与下载

## 当前状态

**已完成**（2026-10-21 · 已合并 `develop` `f624c1a`，冒烟通过）

## 状态流转

| 时间 | 状态 | 依据 |
| --- | --- | --- |
| 2026-10-21 | 草稿 | 两个障碍：TTL 靠外部脚本、页面浏览器打不开 |
| 2026-10-21 | 进行中 | 建 `feature-REQ-110`，派 Dev |
| 2026-10-21 | 待验证 | Dev 交付（1 提交），ADR-004 已落库 |

## 交付摘要

| 项 | 结果 |
| --- | --- |
| `ttl.rs` | 1002 行纯序列化，9 条单测 |
| CLI | `ontok ontology export`（默认客户视图，`--model-view` 可选） |
| HTTP | `GET /api/ontology.ttl`（gated，`text/turtle` + attachment） |
| 页面 | 「导出 TTL」按钮；`/` 免 token 但**零数据壳** |
| 门禁 | `require_token` 收窄到 `/api/*`；未注册 `/api/xxx` 仍 401 |
| ADR | `BA/decisions/ADR-004-页面可达性与门禁.md` |

**导出计数（anspire_ai v2）**：建模视图 **83 类 / 866 数据属性 / 52 对象属性 / 5953 三元组**；客户视图 843 数据属性（差 23 条 = 投影 11 + 剔除 11 + TTL 同名合并 1；前两项与其它出口同源，第三项是 TTL 特有）。

**AC-007 实测**：CLI 与 HTTP 导出 sha256 `47C2F166…39D98` **完全相同**。

**测试**：lib 245（+15）/ doc 4 / 32 基线套件 313（一条不少）+ 新增 14。

## 待办

- [ ] QA 审核（重点：AC-002 的扫描范围、H1 golden 重锚是否合规、`/` 零数据壳）
- [ ] 合并后开 REQ-111

## QA 两轮结果

| 轮次 | 结论 | 关键 |
| --- | --- | --- |
| 首轮 | **FAIL**（1 阻断 + 8 非阻断） | 阻断：`src/bin/ontok-gate-preflight.rs` 被 `git add -A` 误删（**BA 事故**） |
| 复审 | **B-1 真修好**，但 **A-7 断言未落地** ⇒ 仍未通过 | QA 用**独立克隆**验证：同一 blob OID、fresh clone 编译产出 exe |
| 补测后 | **15 passed / 0 failed** | 解析两个 IRI + `assert_eq!` + **防空转反证**（改一字节必须判不等） |

### QA 抓到的三处 BA 错误（都已订正）

| # | 我写的 | 实际 |
| --- | --- | --- |
| 1 | 「头部是编译期常量」 | 头部 `@prefix` 由项目名派生，另有 `action_type_count` |
| 2 | 「23 条 = 12 投影 + 11 剔除」 | **11 + 11 + 1**（第 23 条是 TTL 同名属性合并，`/api/graph` 844 vs TTL 843） |
| 3 | 「头部 IRI 与正文 IRI 逐字相同」 | 头部比正文多一个 `#`（Turtle 片段分隔符）；正确表述是「前缀 = 本体 IRI + `#`」 |

### 事故教训（已写进 `dispatch/rules.md`）

> 提交前必须看 `git status --short` 的 `D` 行；挪文件的绕行必须在 `finally` 挪回，并同时打印 `sha256` 与 `Test-Path`。

## 关键实测

| 项 | 结果 |
| --- | --- |
| 导出计数 | 建模 **83 / 866 / 52 / 5953**；客户视图 843 数据属性 |
| CLI vs HTTP | **同 sha256**（`47C2F166…539D98`），三类响应头逐项通过 |
| 守门器 | 正文含「主键」⇒ CLI exit=2 / HTTP 500，`--out` 不落盘 |
| `/` 零数据壳 | `secretproj`/v7：项目名/版本/对象名 **0 命中**；`/api/*` 含未注册路径与 POST 全 401 |
| H1 golden 重锚 | QA 独立确认：新 golden 与实测 `GET /` 逐字节一致，PROVENANCE 说明原因，其余 12 份未动 |
| 变异 | 4 组（关闸/扫整份/摘身份门/HTTP 多一字节）分别被 5/7/6/1 条判据抓住 |

## 合并与冒烟（2026-10-21）

| 项 | 结果 |
| --- | --- |
| 合并 | `develop` **`f624c1a`**（`--no-ff`，21 文件 / +3202 / −758）；合并后 `cargo check --lib` ✅ |
| 红线 | 相对 `abf1c9f`，四红线 + `tools.rs` **numstat 空** |
| bin | `src/bin/ontok-gate-preflight.rs` 在合并后仍存在 ✅ |
| 回收 | feature worktree + 分支 + `qa/REQ-110` + 外部 QA 克隆（`_qa2_req110`）全部清理 |

**冒烟（用户最初的两个诉求）**：

```
① 产品内置导出：
   ontok ontology export → 客户视图 215.5 KB（83 对象 / 52 关系）
   ontok ontology export --model-view → 226.5 KB（83 类 / 52 关系）
② 页面可达：
   GET /                    = 200 · 12.2 KB（项目名 0 命中、对象名 0 命中）
   /api/{graph,scope,ontology.ttl,nonexistent} 无 token = **全部 401**
   /api/ontology.ttl +token = 200 · 215.5 KB · text/turtle · attachment; filename="anspire_ai-v2.ttl"
```

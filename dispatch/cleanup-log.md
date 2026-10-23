# Worktree 回收与巡检日志

## 已知规范偏离（避免巡检误判为违规）

| 日期 | 偏离项 | 理由 | 决策人 |
| --- | --- | --- | --- |
| 2026-09-28 | workspace 分支除 `README.md` / `.gitignore` 外，还跟踪 `AGENTS.md` 与 `.agents/` | 二者是**跨 worktree 共享的工具层**；放进 `BA/`（demand 分支）会导致 feature worktree 无法读取。不属业务代码，不违反「workspace 禁止业务代码」的核心约束 | 项目负责人 |
| 2026-09-28 | 规范文档的 `git rm -rf .` 改为 `git read-tree --empty` | 前者会**真删工作区文件**，初始化时会误删业务内容 | BA Agent |
| 2026-09-28 | 规范 `.gitignore` 的 `/*/` 改为 `/*` + 白名单 | `/*/` 会无差别忽略第一层子目录，`.agents/` 会被吃掉 | BA Agent |

## 操作规约（本次初始化得出）

> **根 worktree 恒定停留在 `workspace` 分支。**
>
> 任何其他分支的操作都通过 `git worktree add` 建独立 worktree 完成，**不要在根上 `git checkout`**。
> 原因：根上切分支会让工具层（`AGENTS.md` / `.agents/`）消失或冲突 —— 它们只存在于 `workspace` 分支。

## 初始化记录

| 时间 | 动作 | 结果 |
| --- | --- | --- |
| 2026-09-28 | `git init` + 建 `workspace` 分支，扩展白名单 `.gitignore` | 19 个文件，零业务内容 |
| 2026-09-28 | 建 `main`（orphan）/ `develop`（派生自 main） | 占位 |
| 2026-09-28 | 建 `demand`（orphan）/ `deploy`（orphan） | 骨架 |
| 2026-09-28 | 平铺建 `BA/` `code/` `Deploy/` 三个 worktree | 完成 |
| 2026-09-28 | `docs/` `input/` `archive/` 迁入 `BA/` | 完成 |

## 回收日志

| 时间 | worktree | 分支 | 原因 |
| --- | --- | --- | --- |
| 2026-10-09 | `verify-REQ-038` | `verify/REQ-038` | REQ-038 QA 验收完成。树已核实为 develop 祖先（`696aa9f`），安全删除 |
| 2026-10-09 | `feature-REQ-038` | `feature/REQ-038` | REQ-038 已快进合并进 develop（`896c43c`）。**树哈希与 develop 完全相同**（`09ed0ab9d64985055d99c642bc9f940f023db83f`），确认无残留内容 |
| 2026-10-09 | —（分支清理） | `feature/REQ-010,011,013,014,015,019,020,022,023,024,025,026,027,028,029,030,032,033` 共 18 个 | **巡检发现的历史遗留分支** —— worktree 当时已回收，但分支本身漏删，违反登记表「绝不留历史 worktree（完成即清）」。逐个核实均为 develop 祖先后删除。**这是本会话第一次真正巡检出遗留** |
| 2026-10-09 | `verify-REQ-039`、`feature-REQ-039` | `verify/REQ-039`、`feature/REQ-039` | REQ-039 合并并通过合并后全量（523 passed / 1 环境类 / 0 死锁）。`feature/REQ-039` 树哈希与 develop **完全相同**（`8e8de88419fdd330d106402e652ecadbd23dff24`），`verify/REQ-039` 经核实为 develop 祖先，安全删除 |

### 巡检附带发现（未改历史，记此备查）

提交 `d16a060` 的 commit message 标注为 `REQ-025`，但其内容（register 并发改 `ON CONFLICT` +
`migrate()` 锁覆盖全部 DDL + 删「≥3 实现」假许诺）**实为 REQ-016** 的工作，
且已被 REQ-016 的合并提交 `361a163` 吸收。

- **REQ-025（内容类缺口追加重试）至今未实现**，登记表「已就绪」状态正确 —— 是提交信息编号错，不是状态漏更新
- 按纪律「不改历史」，不重写该提交；在此登记以免后续 `git log --grep=REQ-025` 误判

# 2026-09-30 17:57 — 2026-09-30

- 认领 REQ-002（本体内核 MVP Rust 实施）
- 范围确认，10 条暂缓项标注补回时机
- 完成需求文档（demand / acceptance / status）
- 完成实施计划（11 个任务，24.6KB）

## 2026-10-10 — REQ-061 回收 + REQ-062 开单

**回收（延迟一轮的账）**：`feature-REQ-061` / `verify-REQ-061` 两个 worktree
在前一轮被记为「已回收」，**实况是仍在** —— 巡检发现后补做。

- 回收前逐项核实：`merge-base --is-ancestor <branch> develop` = YES（两个都是）、
  `git diff develop..<branch>` = **0 行**、两个 worktree `git status --short -uall` = **0 处**
- 删除分支时 `git branch -d` 报「没有完全合并」—— 原因是它拿**当前分支**（workspace `1d02ad0`）
  做合并判定，而这两个分支合入的是 **develop**。核实 tip 均为 `8aea8ad`（== develop）后用 `-D` 删除，
  **提交全部保存在 develop，零数据丢失**
- ⚠️ **教训**：`git branch -d` 的合并判定基准是 HEAD，不是目标分支。
  跨 worktree 回收时它会给出误导性报错，**先 `merge-base --is-ancestor` 对目标分支核实再删**

**开单**：建 `feature-REQ-062` worktree（基于 develop `8aea8ad`），分支 `feature/REQ-062`。

**基线备份**：`.review-workspace/fix-props/schema.json`（sha256 `b71c2afd…`）
复制到 `.review-workspace/baseline-backup/schema.baseline.json`。
起因见 `demands/REQ-062/acceptance.md` 的 AC-000 ——
**门禁现拉模式会把该文件覆盖成待审对象**，且该文件未被 git 跟踪，无 git 兜底。

## 2026-10-20 — 本仓库首次本地初始化（新克隆）+ 巡检

**背景**：远程已有完整五分支历史（`demand` tip `d39be85`），本地只有仓库根（`workspace`）。
本次补齐本地 workspace 内容。

**建齐 worktree**（在仓库根执行，均跟踪对应远程分支）：

| 目录 | 分支 | tip |
| --- | --- | --- |
| `BA/` | `demand` | `d39be85` |
| `code/` | `develop` | `b714f48` |
| `Deploy/` | `deploy` | `233fa8f` |

根 worktree 仍在 `workspace`（`aa35cc9`），符合「根恒定停留 workspace」规约。

**补齐缺失结构**：
- `BA/backlog/inbox/`、`BA/backlog/refined/` —— **此前不存在**，但 `README.md` / `AGENTS.md` 都把 `BA/backlog/inbox/` 当需求入口
- 各 worktree 的 `.local/{discussions,analysis,drafts,references}/`（不跟踪）
- `BA/.ba/session.md`（不跟踪）

**巡检结果**：
- `feature-*` / `hotfix-*` worktree：**0 个**；远程也无 `feature/*` `hotfix/*` 分支 ⇒ 无需回收
- 根 `git status` 干净；`git ls-files` = 19 个文件，全部属已登记偏离（README / AGENTS / .gitignore / .agents/）
- `agent-registry.md`：`ba-bogon-112089` 由 `online` 降为 `stale`（最后活动 2026-10-02，已 18 天）；新增本会话 `online`
- ⚠️ **发现**：`demand` 历史是脱敏快照同步，**全部提交不含 `[session: …]` 尾注** ⇒ 规范的
  `git log --grep` 追溯在本仓库无效，registry 时间戳是唯一依据（已写入 `agent-registry.md`）

**未做（留待负责人拍板）**：
- `BA/sprint/current.md` 仍停在 Sprint 1 / REQ-002（`demand` 已到 REQ-106）
- 仓库根 `README.md` 的「当前状态」停在 2026-10-04

## 2026-10-21 — REQ-107 收尾：清理 5 个 review worktree

**背景**：QA/Dev 在 `.review-workspace/req107/` 下留了 5 个 detach worktree 做变异复验（`base` @`b714f48`、`qa2` @`4bbd8ac`、`qa3` @`867969b`、`qa4` @`757f88c`、`qa-mut` @`b55d07c`），合计约 **16.8 GB**（每个都带 `target/`）。

**回收前核对**：
- 5 个 worktree 逐个 `git status --porcelain`；只有 `base` 脏（3 个 `??` 未跟踪文件）
- 其中 `tests/qa_golden_recompute.rs` 是 **QA 独立复算 golden 的脚本**（`qa-review.md` 引用它作证据）⇒ **先复制到 `demands/REQ-107/evidence/`**，再删
- 另两个 `??`（`tests/common/req107.rs`、`tests/test_req107_zero_visible_change.rs`）与 feature 分支同名文件 **sha256 逐字节相同**（`7682E9A9…` / `60F480EA…`）⇒ 可安全丢弃

**执行**：`git worktree remove --force` ×5 → `git worktree prune`

**保留**：`.review-workspace/req107/{dev3,dev4,*-logs,*-patches,mutations}`（约 10 MB）—— Dev/QА 报告 §11.6 / §12 引用了这些三元组证据，**不删**

**结果**：`git worktree list` 只剩 5 个正常 worktree（仓库根 / BA / code / Deploy / feature-REQ-107）

## 2026-10-21 — REQ-107 合并后回收

- 合并：`git merge --no-ff feature/REQ-107` → `develop` `4573269`（合并前 `is-ancestor` = YES）
- 合并后 `cargo check --lib` ✅；五红线 + `mcp/tools.rs` 的 `numstat` 相对 `b714f48` **为空**
- 回收：`git worktree remove feature-REQ-107`（exit 0）→ `git push origin --delete feature/REQ-107` → `git branch -D feature/REQ-107`
- ⚠️ **`git branch -d` 二次误报**：判定基准是 HEAD（`workspace`）不是 `develop`，报「not fully merged」。
  与 **2026-10-10** 那条教训同因。⇒ 规程固定为「先 `merge-base --is-ancestor <branch> <目标分支>` 核实，再用 `-D`」
- 状态：REQ-107 `已验证` → **`已完成`**

## 2026-10-21 — REQ-108 合并后回收

- 合并：`git merge --no-ff feature/REQ-108` → `develop` `a8a51ed`（11 提交；合并前 `is-ancestor` = YES）
- 合并后 `cargo check --lib` ✅；四红线 + `mcp/tools.rs` 相对 `4573269` 的 `numstat` **为空**
- 远程 `feature/REQ-108` **不存在**（Dev 全程未 push）⇒ `push --delete` 报 "remote ref does not exist"，属预期
- 回收：`git worktree remove feature-REQ-108` → `git branch -D`（先 `is-ancestor` 核实）
- 另清 QA 留下的 3 个 review worktree（`.review-workspace/req108/{qa,qa2,qa3}`，各 ~3.8MB，其中 2 个脏但只含 QA 临时脚本）
- 状态：REQ-108 `已验证` → **`已完成`**

**本轮新增的一条流程教训**：QA 与 Dev 各自报「31 套件全绿 / 300 passed」时，实测差 5 条——**整体性断言失真，因为清单与断言对不上**。
⇒ 规程：**报告必须列出回归集名单，数字要能逐项对上**（已写进 REQ-108 的 A-14）。

## 2026-10-21 — REQ-109 合并后回收 + 真实业务运行

- 合并：`git merge --no-ff feature/REQ-109` → `develop` `5737ecf`（合并前 `is-ancestor`=YES，合并后 `cargo check --lib` ✅）
- 红线：相对 `a8a51ed`，四红线 + `mcp/tools.rs` numstat **为空**
- 回收：`git worktree remove feature-REQ-109` + `git branch -D`（远程分支从未推过）
- 真实运行（`anspire_open_uat` + `qwen36-27B`）：83 实体 / 81 关系 / 61 条读到样本；
  分档 **AI 开 高39·中13·低19·拒绝10 / AI 关 高43·中9·低16·拒绝13**；**token 消耗 0（全部命中缓存）**
- ⚠️ **使用陷阱**：REQ-108 后 schema 产物新增 `primary_key` / `unique_columns`。
  用旧 schema 跑新 propose ⇒ 采样读不到、全部 `data_evidence=none`、AI 不判。**必须先重新 introspect**。

## 2026-10-21 — REQ-110 合并后回收 + 冒烟

- 合并：`git merge --no-ff feature/REQ-110` → `develop` `f624c1a`（21 文件 / +3202 / −758）
- 合并后 `cargo check --lib` ✅；四红线 + `mcp/tools.rs` 相对 `abf1c9f` numstat **空**
- 回收：`feature-REQ-110` worktree + 分支 + QA 的 `qa/REQ-110` 分支 + 外部克隆 `D:\MyCodes\AI\_qa2_req110`
- 冒烟：**产品内置导出**（客户 215.5 KB / 建模 226.5 KB）+ **页面可达**（`GET /` 200 零数据壳；`/api/*` 全 401；带 token 导出 200 `text/turtle`）

### 本轮事故（已写进 `dispatch/rules.md`）

`git add -A` 把「Dev 测试绕行挪走的 `src/bin/ontok-gate-preflight.rs`」当成删除提交（2758 行）。
**本地全绿、CI 编不过**——只有在 fresh checkout 上才暴露。
⇒ 规程：提交前看 `D` 行；挪文件必须 `finally` 挪回，并同时打印 `sha256` 与 `Test-Path`。

## 2026-10-21 — REQ-111 合并后回收 + 真实数据演示

- 合并：`git merge --no-ff feature/REQ-111` → `develop` `4178a4e`；合并后 `cargo check --lib` ✅
- 红线 + `src/server/http.rs` + `Cargo.*` 相对 `f624c1a` **numstat 空**
- 回收：feature worktree + 本地分支
- 演示（`review_demo` / 135 条目）：页面壳 200 无数据；`/api/review` 135；预览 `≥0.90` **39 条**、`≥0.60` **52 条**；参数越界全 **400**；`POST` 签一条 ⇒ `via=ui` + 时间戳；读回一致
- ⚠️ 演示踩坑（已记入 status）：预览参数名是 **`min_confidence`**；拼成 `min_score` 会被**静默忽略**（变全量）。建议下一轮对未知 query 参数报 400。

## 2026-10-21 — REQ-112 合并后回收

- 合并：`git merge --no-ff feature/REQ-112` → `develop` `e9755e8`；合并后 `cargo check --lib` ✅
- 禁改面（`server/http.rs` / `tools.rs` / `view.rs` / `review_view.rs` / `property_filter.rs` / 两红线 / `Cargo.*`）相对 `4178a4e` **numstat 空**
- 回收：feature worktree + 本地分支
- 清理 Dev 与 QA 的临时项目：`dev112fix` / `dev112c` / `dev112b` / `t112` / `t112_low`（库里只剩 `anspire_real`）
- 本轮教训：**BA 的规格自相矛盾**（AC-002 有 `band=high` 约束、AC-003 没有）导致冲突规则不可达 —— QA 用生产评分函数算出 `fit=0.0 ⇒ 0.85/middle` 才暴露

## 2026-10-22 — REQ-113 + REQ-114 合并与回收

- 合并：`feature/REQ-113` → `develop` `af45c0c`；`feature/REQ-114` → `develop` **`71ae770`**
- 冲突：两需求都改 `.feature/verification-report.md` 与 `.feature/status.md`（Dev 报告文件），保留两段并加分隔
- 红线/禁改面：相对 `e9755e8` numstat 空；合并后 `cargo check --lib` ✅
- 回收：两个 feature worktree + 分支；另清掉 QA 留下的 4 个 `.review-workspace` worktree（req113/baseline、req113/qa、req114/qa-baseline、req114/qa-mut）
- 真实库复跑（`anspire_excl`）：**勘查 96 → 排除 12 → 抽取 84（闭合 true）**；实体 84 / 关系 82；token **1140**（缓存命中 99%）；审核全签 → v2 冻结 → TTL 83 对象/52 关系
- `proposals.inputs.exclusions.counts` 落库正确：`{seen:96, excluded:12, extracted:84, stale:0, flagged:11}`

## 2026-10-22 — REQ-115 合并与回收

- 合并：`feature/REQ-115` → `develop` **`a85383a`**（4 个提交：`7718061` → `cc7e44a` → `efa2239`）
- 禁改面：`http.rs` 仅分组管道（79/9 行，路由与鉴权未动）；`tools.rs` / `property_filter.rs` / `Cargo.*` 零 diff
- 合并后 `cargo check --lib` ✅；golden H1 `c8d5e148…`（49936B）与 `INDEX_HTML` 逐字节一致
- 回收：feature worktree + 分支；QA 的 2 个 worktree 已由 QA 自行清理
- 真实数据：新建 `anspire_grouped`（96→排除 12→84 对象），本体 v2 + 8 组 v3（冻结）
- 验证：`GET /api/graph` 200 / 83 节点 / **8 组** / 未分组 0 / 响应无字面 `"key"`；`ontology overview` **exit 0**
- 声明改动留痕：`code/out/concept-declaration-ai-v2.json`（v1.0.1，digest `04ffce330779807d`，「未持 Key」→「未持凭证」）

## 2026-10-22 — REQ-116 合并与回收

- 合并：`feature/REQ-116` → `develop` **`5f6636f`**（2 个提交：`31838ef` + `90c6e71`）
- 禁改面（`http.rs` / `tools.rs` / `property_filter.rs` / `Cargo.*`）相对 `a85383a` **零 diff**；合并后 `cargo check --lib` ✅
- 回收：feature worktree + 分支
- 机制验证：101 条断言 / 101 证据字节级命中 / 37 个谓词 / 原始 belongs_to 0/51 / 第三次运行 0 token
- ⚠️ **真实业务文档仍未跑**（4 份语料经 QA 判定过度贴合提示词，只能当冒烟夹具）
- 新规程：`Copy-Item` 还原保留 mtime ⇒ cargo 不重编 ⇒ 可能跑变异版产物（Dev 自报踩到）。还原后必须 `touch` / `cargo clean -p`


## 2026-10-22 — REQ-118 合并与回收

- 合并：`feature/REQ-118` → `develop` **`2b55624`**（2 个提交：`b0a814f` + `959611d`）
- 禁改面零 diff；合并后 `cargo check --lib` ✅
- 回收：feature worktree + 分支
- 真实库三份提案：name 96 / exclude 96 判定（排 21）/ concept 9 组；缓存二次 **0 token**
- **清理测试污染**：`anspire_real` 里 3242 条 proposal / 2613 条 item / 3000+ `t_*` 项目全部删除，现只剩 4 个真实项目
- 新规程两条（写进 `dispatch/rules.md`）：
  1. **失败路径三态**：complete / partial(exit≠0) / all-failed(不产产物)，⛔ 禁止结论式提示
  2. **测试库必须独立**：测试不得指向演示库

## 2026-10-22 — REQ-119 合并与回收

- 合并：`feature/REQ-119` → `develop` **`bf5f89f`**（3 个提交）
- `http.rs` +6 行（测试夹具），四红线 / `tools.rs` / `property_filter.rs` / `Cargo.*` 零 diff
- 回收：feature worktree + 分支
- 最终能力：**104 对象属性（52 正 + 52 反，方向已修正）** / **952 条 xsd range 与真实列类型全等** / **84 主键** / **104 枚举值域（QA 逐条反查 0 编造）** / **54 租户标注**
- ⚠️ 规程订正：**「挪开 preflight 再跑测试」在本仓不成立**（`retained_names` 对它 `include_str!`），改用 `#[cfg]` shim 或跳过并写明。这条错误规程被 Dev/QA 各自重新发明 ≥4 次
- 后续：REQ-120（FK 边补全 + 1:1 基数修正）→ REQ-121（R2RML）→ REQ-122（SPARQL 端点 + 租户注入）→ REQ-123（Agent 契约）


## 2026-10-22 — REQ-120 合并与回收

- 合并：`feature/REQ-120` → `develop` **`e73d5e9`**（5 个提交）
- 禁改面零 diff；合并后 `cargo check --lib` ✅
- 回收：feature worktree + 分支；清理临时项目 **1308 条**（`anspire_fk120*` / `g1demo` 等），库里只剩 4 个真实项目
- 结果：关系 52 → **79**；三态 true 144 / inherit 2 / false 12；`joinEvidence` 158/158；1:1 共 8 对
- **新规程**：安全约束必须**全路径 fail-closed**（生成 / 签字 / 注入三层）——「能被文本绕过的不算规则」


## 2026-10-23 — REQ-121 合并与回收

- 合并：`feature/REQ-121` → `develop` **`e8f21c8`**（5 个提交：3c229fe → bf252b7 → 28199ca → d19fa13 → 595711e）
- 禁改面零 diff；合并后 `cargo check --lib` ✅
- 回收：feature worktree + 分支
- 结果：84 TriplesMap / 70 join；true→join、inherit 默认不生成、false 永不成 join；类集双向差集 0；中文 IRI 0 处
- 新修：AC-008 三条 guard 补夹具 + 逐条变异证明有牙；`.scratch/`（1.4 MB 真名 TTL）进 `.gitignore`


## 2026-10-23 — REQ-122 合并与回收

- 合并：`feature/REQ-122` → `develop` **`e3da616`**（6 个提交）
- 禁改面零 diff；合并后 `cargo check --lib` ✅
- 回收：feature worktree + 分支
- 结果：84/952/70 与映射逐数相同；泄漏对拍交集 0（含「公共维度当驱动表」攻击）；echo 与 `pg_stat_activity` 逐字相同；26 条拒绝用例；库内泄漏测试只需测试库
- **两条新规程**：① 派单前查 `list_agents`（我制造过一次双写冲突）② 「声称必须有判据」（5 轮里 5 次同类问题）


## 2026-10-23 — REQ-123 合并与回收

- 合并：`feature/REQ-123` → `develop` **`fbae80a`**（2 个提交：`5ecca46` + `09526b0`）
- 禁改面零 diff；合并后 `cargo check --lib` ✅
- 回收：feature worktree + 分支
- 结果：REST 四端点 + MCP(JSON-RPC/SSE)；**两协议 8 组 sha256 全等**；租户攻击 6 项全挡；`futures-util` 无新 crate（260→260）；审计 111 行无明文
- **新规程**：提交前必须验「分支上真的有东西」（REQ-123 首轮分支 0 commit，合并会丢功能）


## 2026-10-23 — REQ-124 合并与回收

- 合并：`feature/REQ-124` → `develop` **`eaa7c96`**（1 个提交 `881bcfd`）
- 禁改面零 diff；合并后 `cargo check --lib` ✅
- 回收：feature worktree + 分支
- 结果：11 端点；token 双向隔离；四组 sha256 与 CLI 全等；9 类声明非法全 400 且不落盘；无后门端点
- 合并后立即开 REQ-125（内部工作台前端），worktree 已建


## 2026-10-23 — REQ-125 合并与回收

- 合并：`feature/REQ-125` → `develop` **`d54c130`**（3 个提交：`302bba5` → `37b6526`（L-2 CSP 注释）→ `cbfed33`（B-1/B-2））
- 禁改面零 diff；合并后 `cargo check --lib` ✅
- 回收：feature worktree + 分支；合并后立即开 REQ-126 worktree
- 结果：页面 59,988 B / 壳零数据（沙箱验证：投毒项目名 0 命中）/ token 仅 sessionStorage / 查询台显示 warnings、coverage、used=false、truncated
- 两轮 QA：首轮 FAIL（查询台 404/403 误导 + `--help` 缺失），修复后通过
- **新记录**：`dev_proxy.ps1` 成为「两服务同源」的最小示例，已写入 `BA/runbook/ontology-e2e.md §7.3`


## 2026-10-23 — REQ-126 合并与回收

- 合并：`feature/REQ-126` → `develop` **`5e73813`**（3 个提交）
- 禁改面零 diff；合并后 `cargo check --lib` ✅
- 回收：feature worktree + 分支；另清理遗留的 `qa-REQ-125`（detached，QA 变异用）
- 结果：4 端点 / 租户只来自口令 / 启动 8 条 fail-closed 校验 / 响应类型层无技术标识 / 并发闸立刻 429
- 三轮：主体 → 并发闸 → QA 三条必修（N-1 假红 / N-2 壳豁免 / N-3 单测无计时）
- **本日三条需求全部合并**：REQ-124 `eaa7c96` · REQ-125 `d54c130` · REQ-126 `5e73813`


## 2026-10-24 — REQ-128 合并与回收

- 合并：`feature/REQ-128` → `develop` **`2573429`**（1 个提交 `15a2227`）
- 禁改面零 diff；合并后 `cargo check --lib` ✅
- 回收：feature worktree + 分支
- 结果：`--names` 必填（不传退出码 1、产物不生成）；占位表只警告不拒绝；阈值严格 `>80%`
- **QA 推翻了我两个说法**（「有一批测试依赖默认值」→ 实际 0 处；「19 调用点」→ 基线 18），已更正进 demand

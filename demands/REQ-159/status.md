# 状态：REQ-159 · 构建包导入契约 v1

## 当前状态

**P1a + P1b 已合入 develop（`1c418ce`）· P1b-2 待派**（2026-10-27 12:50）

## 决策（全部已定）

| # | 决策 | 结论 |
| --- | --- | --- |
| 1 | 包签名 | **v1 不做**（只 sha256） |
| 2 | key 与项目 | **一项目一把**（query key，绑项目） |
| 3 | DSN 存储 | **元数据库加密列** |
| 4 | 生效方式 | **解析后人工激活** |
| 5 | 安全强度 | 保留最小鉴权与串包防护，放弃签名/限流/密钥托管 |

## 三刀

| 刀 | 内容 | 状态 |
| --- | --- | --- |
| **P1a** | 表结构 + 7 端点 + 包校验（8 错误码）+ 解析入库 + 人工激活 | 🟡 已派 Dev（worktree `feature-REQ-159`） |
| P1b | `query serve` 多项目（按 key 反查 → active_version → 按 version_id 缓存） | ✅ **已合 `1c418ce`**（8/8，干净态回归全绿） |
| **P1c-1** | 8 条端点从 console **搬到 `query serve`** + `kind: admin` + `ONTOK_SECRET_KEY` fail-fast | 🟡 本次派单 |
| **P1c-2** | **Studio UI**：＋ 建项目 / 上传 / 解析 / 激活 / 查看 key（形态见 `BA/design/bundle-prototype`） | ⬜ 等 P1c-1 |
| **P1c-3** | 版本三端点（列/看/激活回滚）+ `GET /api/projects/:p` + `key/rotate` + AM 落库 | ⬜ 最后 |
| P1c | 前端页面（**由 lion 的面**：`src/console/page.rs`，他已完成，等我端点合完联调） | 🟡 他那侧完成 |

## 现存表（已确认，不用新建）

`projects`(id,name,created_at) · `ontology_versions`(id,project_id,seq,frozen_at,created_at)

**要新增**：`project_keys` · `project_bundles` · `project_databases`
**要加列**：`projects.label` · `projects.active_version_id`

## 消费方

- **数字员工**用 `query` key（一 key 绑一项目），服务端从 key 反查项目与生效版本
- **FDE** 用 `admin` key 做建项目/上传/配 DSN/激活

## 关联

REQ-124（控制面 API）· REQ-157（本体 v5）· ADR-012（服务面首次获得写能力，待写）

## P1b 交付要点（2026-10-27 · BA 亲验）

| 项 | 值 |
| --- | --- |
| commit | `5925e01`（基线 `a4faf2d`）· 12 文件 · 未碰 `page.rs`/`console/bundle.rs`/`Cargo.toml`/`console::serve` |
| 新判据 | `tests/test_req159b_multi.rs` **8/8**（BA 亲跑，真库） |
| 单项目零变化 | 与 `a4faf2d` 基线二进制比 **10 个请求 status+body sha256 全等** |
| 缓存 | 键 = `version_id`（activate 在另一进程 ⇒ 键变自然生效，无需跨进程失效）；上限 8、LRU；**每请求一条 SELECT**（刻意不加 TTL） |
| 三条偏差 | 全部接受：① `project` 落在两张 token 表（各自落实）② 多项目 `--schema/--policy` 仍部署级（已知边界）③ `/metrics` 只进程级口径（N 套共用 gauge 会互相覆盖，比缺更误导） |

**合并顺序（我定）**：先合「4 条红守卫修复」→ 再合 P1b。理由：合并后全量必须干净，否则分不清谁的红。

## 部署（待拍板）

见 [`deploy-plan.md`](deploy-plan.md)：**A（只上 console）→ B（query serve 切多项目）** 两段走。
关键前提：UAT **现在没有 console 进程**，所以 REQ-159 的能力在 UAT 上**完全不可用**（只在本地跑通）。

# 状态：REQ-159 · 构建包导入契约 v1

## 当前状态

**已就绪 · P1a 已派单**（2026-10-27 10:10）

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
| P1b | `query serve` 多项目（按 key 反查 → active_version → 懒加载 registry） | ⬜ 等 P1a |
| P1c | Studio ＋ 建项目 / 上传向导（前端） | ⬜ 等 P1a |

## 现存表（已确认，不用新建）

`projects`(id,name,created_at) · `ontology_versions`(id,project_id,seq,frozen_at,created_at)

**要新增**：`project_keys` · `project_bundles` · `project_databases`
**要加列**：`projects.label` · `projects.active_version_id`

## 消费方

- **数字员工**用 `query` key（一 key 绑一项目），服务端从 key 反查项目与生效版本
- **FDE** 用 `admin` key 做建项目/上传/配 DSN/激活

## 关联

REQ-124（控制面 API）· REQ-157（本体 v5）· ADR-012（服务面首次获得写能力，待写）

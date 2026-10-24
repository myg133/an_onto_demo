# 状态：REQ-139 · health 报出项目与本体版本

## 当前状态

**已完成**（2026-10-24 · 合并 `develop` `600ae76` · 已部署为镜像 `0.1.3`）

## 缺口

```
GET /api/health
{ "service": "ontok-query", "version": "0.1.0", "mode": "ontology-only", ... }
                              ↑ 产品版本，不是本体版本
没有 project、没有 ontology_version
```

⇒ Agent 想知道"我面对的是哪套本体"，**必须先下 21 KB TTL** 读 `ontok:sourceProject`。

## 要做

加两个字段：`project`、`ontology_version`；保留 `version`（产品版本）并在文档里**说清两者区别**。

## 五条裁定

| # | 裁定 |
| --- | --- |
| R1 | 免 token |
| R2 | 只给项目名（已在 TTL/catalog 公开）；⛔ 不加租户 id / 连接串 / 表名 |
| R3 | 本体模式与普通模式**都有** |
| R4 | 与 `catalog` 的 `project`/`version` **必须是同一个数**（不许两处各算） |
| R5 | 现有字段一个不少（有客户端按 `mode` 判断） |

## 待办

- [ ] Dev
- [ ] QA
- [ ] 合入后：更新 skill（第 0 步可同时确认"我在跟谁说话"）+ 重新部署 0.1.3

## 上线结果（集群实测）

```json
GET https://an-notol.mic-scfw.cloudns.org/api/health
{ "project": "ops360", "ontology_version": 2, "version": "0.1.0",
  "mode": "ontology-only", "service": "ontok-query", ... }
```

| 项 | 结果 |
| --- | --- |
| 合并 | `develop` **`600ae76`**；QA PASS（16/16 数据面 sha256 相同、变异有牙） |
| 与 catalog 对账 | ✅ 同值同源（`ops360` / `2`） |
| 旧字段 | ✅ 一个不少，`mode` 仍只在本体模式 |
| 零泄漏 | ✅ 租户 uuid / 连接串 / 96 张表名 全 0 |
| 部署 | 镜像 `0.1.3` 已推 ACR 并滚动（`2/2 Running`） |

## 附带发现（未处理）

镜像 tag（`0.1.3`）与 `health.version`（`0.1.0`，Cargo 版本）是两套编号 ⇒ 排查时可能误判"跑的是旧镜像"。
建议后续注入 git sha（`build` 字段）。已记，未开需求。

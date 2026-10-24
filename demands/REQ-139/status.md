# 状态：REQ-139 · health 报出项目与本体版本

## 当前状态

**进行中**（2026-10-24 开单 · P2）

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

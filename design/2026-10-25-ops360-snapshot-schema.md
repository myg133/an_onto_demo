# Ops360 运维快照 · schema 草案

> 状态：**草案，待评审**。2026-10-25。
> 目标：一套表，**两种写入口**——① 快照（稳定、可造）② 现场采集（腾讯云 API）。

---

## 一 · 先看一个反推结果（它改变了设计）

我拿 ops360 本体（20 类 / 47 属性）反推 DDL，结果：

| 类 | 属性数 | 属性是什么 |
| --- | --- | --- |
| 费用概览 | 7 | 日均费用、可用余额、欠费金额、待续费资源数……**口径** |
| 备份策略 | 5 | 备份覆盖率……**口径** |
| 告警 | 4 | 告警总数、严重告警数、未恢复告警数……**计数** |
| 业务口径 | 10 | 全是口径定义 |
| **云资源** | **2** | 资源总数、异常状态资源数……**也是计数** |
| 告警策略 / 地域 / 安全组规则 / 快照 / 数据库账号 | **0** | 文档没给字段 |

**结论**：

> 这套本体描述的是「**有哪些东西**」和「**怎么算**」，
> **不是**「一张表长什么样」——它没有 `id` / `name` / `region` / `status` 这类**身份字段**。

⇒ DDL 必须由两部分拼成：

```
身份字段（本体没有，要补）  +  口径属性（本体有，照搬）
```

---

## 二 · 一个必须补的缺口：**「应用 / 归属」**

demo 剧本的核心是「**这条告警影响谁**」。而"影响谁"靠的是：

```
告警 → 资源 → 归属哪个应用 → 同应用还有哪些资源 → 谁负责
```

**但本体里没有「应用」这个类**（20 类里没有）。文档也没提。

⇒ 两种可能：

| 可能 | 处置 |
| --- | --- |
| 客户有 CMDB / 应用台账 | 采集时 join 进来，落成 `app` 表 |
| 没有，靠命名约定 | 从资源名/标签前缀推断 —— ⚠️ **这是猜测，必须标记为 `inferred`** |

这条要你定，它直接决定 demo 里"影响面"能不能成立。

---

## 三 · DDL 草案（7 张表）

```sql
-- ① 采集批次（每次巡检 / 每次拉取一次）
CREATE TABLE ops_snapshot (
  id            uuid PRIMARY KEY,
  provider      text NOT NULL,              -- tencent / synthetic
  collected_at  timestamptz NOT NULL,
  collector     text NOT NULL,              -- 采集器标识（谁采的）
  status        text NOT NULL               -- ok / partial / failed
);

-- ② 云账号
CREATE TABLE cloud_account (
  id          text PRIMARY KEY,             -- 账号 UIN
  name        text NOT NULL,
  provider    text NOT NULL,
  account_type text                        -- 主账号 / 子账号
);

-- ③ 云资源（CVM / CDB / OSS / CDN 统一一张，用 resource_type 区分）
CREATE TABLE cloud_resource (
  id             text PRIMARY KEY,          -- 实例 ID（i-xxx / cdb-xxx）
  name           text NOT NULL,
  resource_type  text NOT NULL,             -- cvm / cdb / cos / cdn / ssl
  region         text,
  zone           text,
  status         text,                      -- RUNNING / STOPPED / ...
  account_id     text REFERENCES cloud_account(id),
  snapshot_id    uuid REFERENCES ops_snapshot(id),
  -- ⚠️ 归属：来自 CMDB 或命名推断（推断时必须标 inferred）
  app_code       text,
  app_source     text                       -- cmdb / inferred / unknown
);

-- ④ 告警（对应本体「告警」类）
CREATE TABLE alarm (
  id            text PRIMARY KEY,
  name          text NOT NULL,
  level         text NOT NULL,              -- critical / warning / info
  status        text NOT NULL,              -- firing / recovered
  resource_id   text REFERENCES cloud_resource(id),
  metric        text,
  threshold     text,
  current_value text,
  fired_at      timestamptz NOT NULL,
  recovered_at  timestamptz,
  snapshot_id   uuid REFERENCES ops_snapshot(id)
);

-- ⑤ 备份策略（对应本体「备份策略」类）
CREATE TABLE backup_policy (
  id               text PRIMARY KEY,
  name             text NOT NULL,
  resource_id      text REFERENCES cloud_resource(id),
  schedule         text,                    -- daily / weekly / cron
  retention_days   integer,
  last_success_at  timestamptz,             -- NULL = 从未成功（关键信号）
  enabled          boolean NOT NULL DEFAULT true,
  snapshot_id      uuid REFERENCES ops_snapshot(id)
);

-- ⑥ 费用快照（对应本体「费用概览」类）
CREATE TABLE cost_daily (
  id           uuid PRIMARY KEY,
  account_id   text REFERENCES cloud_account(id),
  bill_date    date NOT NULL,
  product      text,                        -- cvm / cdb / cos ...
  amount       numeric(14,2) NOT NULL,
  currency     text NOT NULL DEFAULT 'CNY',
  snapshot_id  uuid REFERENCES ops_snapshot(id)
);

-- ⑦ 风险项（对应本体「风险项」类；采集或规则产出）
CREATE TABLE risk_item (
  id           uuid PRIMARY KEY,
  level        text NOT NULL,
  title        text NOT NULL,
  suggestion   text,
  resource_id  text REFERENCES cloud_resource(id),
  dimension    text,                        -- 巡检维度
  persistent   boolean NOT NULL DEFAULT false,
  snapshot_id  uuid REFERENCES ops_snapshot(id)
);
```

**没有单独建 `app` 表**——`app_code` 直接放在资源上，因为 demo 只需要"同一 app 下有谁"。

---

## 四 · 与本体对齐（哪些类有表、哪些没表）

| 本体类 | 对应表 | 说明 |
| --- | --- | --- |
| 云资源 | `cloud_resource` | ✅ |
| 关系型数据库实例 | `cloud_resource`（`resource_type='cdb'`） | 合并 |
| 告警 | `alarm` | ✅ |
| 备份策略 | `backup_policy` | ✅ |
| 费用概览 | `cost_daily` | 概览 → 按日明细 |
| 风险项 | `risk_item` | ✅ |
| 巡检快照 | `ops_snapshot` | ✅ |
| 云凭据 / 告警策略 / 地域 / 安全组 / 安全组规则 / 数据库账号 / 快照 / 巡检配置 / 采集结果 / 巡检报告 / SSL 证书 / 用量指标序列 | **暂无表** | demo 用不到；后续按需 |
| **业务口径（10 条）** | **不建表** | 它是**计算规则**，不是数据 |

> 「业务口径不建表」这条很重要：口径（如"备份覆盖率 = 已备份/应备份"）
> **要么在查询时算，要么在采集时物化**。demo 建议**查询时算**（保持口径单一来源）。

---

## 五 · 两种写入口

| 方式 | 谁写 | 用于 |
| --- | --- | --- |
| **快照** | 脚本灌入（合成或采集产物） | demo 稳定可重复 |
| **现场** | 采集器调腾讯云 API（`Describe*`） | 证明真实 |

**关键**：两者写**同一套表**，所以本体/映射/查询**一个字都不用改**。

```sql
-- 现场采集后，只需换 snapshot_id 与 provider
UPDATE ... -- 不需要；直接 INSERT 一批新的，查询取最新 batch
```

---

## 六 · 三条要你定的

| # | 问题 | 为什么必须定 |
| --- | --- | --- |
| **1** | **「应用归属」从哪来**？（CMDB / 命名推断 / 手工表） | demo 的"影响面"完全依赖它；推断必须标 `inferred` |
| **2** | 云资源**要不要分表**（cvm/cdb 各一张）还是统一 `cloud_resource`？ | 分表更贴真实 API；统一更简单。我倾向**统一 + `resource_type`** |
| **3** | 快照数据造多少？（建议：资源 200 / 告警 40 / 风险 15，覆盖"有备份 vs 无备份""同应用多实例"两类对比） | 决定 demo 剧情密度 |

---

## 七 · demo 查询会是什么样（先给个预览）

```sparql
SELECT ?alarm ?res ?app ?peer ?backup WHERE {
  ?alarm  a :alarm ;
          :alarm__resource ?res .
  ?res    :cloud_resource__app ?app .
  ?app    :app__resources ?peer .
  OPTIONAL { ?res :cloud_resource__backup ?backup }
}
```

**这段现在跑不了**（v1 不支持 `OPTIONAL`）——demo 要么改成两段查询，要么把 `backup` 做成必填关系。

**这是个真约束，得在写剧本前定下来。**

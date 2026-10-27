# REQ-159 部署方案（UAT / 生产）· 待拍板

- 日期：2026-10-27
- 状态：**草案 · 待用户拍板**
- 负责：join（服务端 / Deploy）

---

## 一 · 现在的部署是什么样

```
UAT: ontok-query（唯一进程）
       ontok query serve --project cloud-resource --version 5 --ontology-only
       └─ 单项目模式（启动参数绑死项目与版本）
     没有 console（控制面）—— overlay 里那个 Deployment 被 $patch: delete 掉了
```

**后果**：`POST /api/projects`、上传包、配 DSN、解析、激活 —— **一个都调不到**。
所以 REQ-159 的东西**在 UAT 上目前完全不可用**（只在本地跑通）。

---

## 二 · 要上新能力，最少需要动三样

| # | 要什么 | 为什么 | 风险 |
| --- | --- | --- | --- |
| 1 | **部署 console**（`ontok-builder console serve`） | 构建包导入的所有写端点都在控制面 | 它是**写库 + 收包**的进程，暴露面比现在的只读 query 大 |
| 2 | **新增密钥 `ONTOK_SECRET_KEY`**（32 字节 hex） | DSN 与 query key 的加密列；**缺了控制面直接拒绝启动** | 密钥丢失 ⇒ 已存的 DSN/key 密文解不开（需重新配置） |
| 3 | **决定 query serve 用哪种模式** | 单项目（现状）vs 多项目（新能力） | 多项目模式才能"一套部署多套本体" |

---

## 三 · 三个选项

### 选项 A · 只上 console，query serve 保持单项目（**最保守**）

```
ontok-query     : 单项目模式（--project cloud-resource --version 5）   ← 不变
ontok-console   : 新增 Deployment，仅集群内可达（ClusterIP）
```

| 项 | 值 |
| --- | --- |
| 能做什么 | 建项目 / 上传包 / 配 DSN / 解析 / 激活 / 查看 key（全在 console 上） |
| 不能做什么 | 新项目的**对外查询**（query serve 仍只服务 cloud-resource v5） |
| 暴露面 | console **只绑 ClusterIP / 回环**，不对外；FDE 用 `kubectl port-forward` 访问 |
| 适合 | **先验证整条链路**在生产库上没问题 |

### 选项 B · 上 console + query serve 切多项目（**目标形态**）

```
ontok-query     : 多项目模式（不给 --project/--version）→ 按 key 解析项目与生效版本
ontok-console   : 新增（同 A）
```

| 项 | 值 |
| --- | --- |
| 能做什么 | 「一套部署多套本体」完整可用：新建项目 → 传包 → 激活 → 数字员工用新 key 就能查 |
| 风险 | ① 现有 token 表是 **v1**（没有 `project`）⇒ **多项目模式启动会失败**，必须先把 token 升 v2 并给每把 query key 补 `project`；② 单项目模式的 10 个请求 sha256 等价是**本地验的**，生产镜像上还没跑过 |
| 适合 | 目标形态，但要先解决 token 表 |

### 选项 C · 先不动部署，只在本地/单机上用

| 项 | 值 |
| --- | --- |
| 能做什么 | 你或 FDE 在本机起 console 连生产库（就像我今天做的 e2e） |
| 风险 | 生产库被本机直接写；无审计集中；不可持续 |

---

## 四 · 我的建议：**A → B 两段走**

| 段 | 做什么 | 判据（可验） |
| --- | --- | --- |
| **第一段（A）** | 部署 console（ClusterIP）+ 注入 `ONTOK_SECRET_KEY`；query serve 不动 | ① console 起来且 `/api/health` 通；② `POST /api/projects` 能建一个测试项目（建完删掉不用）；③ **query serve 的 10 个响应与部署前逐字节相同** |
| **第二段（B）** | token 表升 v2（给现有 query key 补 `project`）→ query serve 切多项目 | ① 用旧 key 查 cloud-resource 结果与切之前逐字节相同；② 新建项目的 key 能查到**新项目**（不是 cloud-resource） |

**为什么分两段**：A 段只**新增**一个进程（不动在跑的服务）⇒ 出问题回滚就是删掉 Deployment。
B 段动的是**对外服务** ⇒ 必须有一段"稳定态"做对照。

---

## 五 · 拍板前必须确认的三件事

| # | 问题 | 我的建议 |
| --- | --- | --- |
| 1 | **console 要不要对外暴露**？ | **不对外**（ClusterIP + port-forward）。它是写库的进程，暴露面越小越好 |
| 2 | `ONTOK_SECRET_KEY` 怎么产生与保管？ | 32 字节随机（`openssl rand -hex 32`），存 k8s Secret；**记进密码管理**，丢了 DSN/key 要重配 |
| 3 | UAT 的 token 表现状 | 我需要看：是 v1 还是已混用；给现有 query key 补 `project` 属于**数据变更**，要用户点头 |

---

## 六 · 不在本方案内

| 项 | 为什么 |
| --- | --- |
| AM 落库（P1b-2） | 做了再改部署也不迟；它不影响进程拓扑 |
| 版本三端点（P1b-2） | 纯 console 端点，随 console 一起上 |
| 生产环境 | 先把 UAT 走通 |
| 多副本 / 高可用 | 单副本够用；多副本要考虑 `active_version_id` 的读一致与缓存，另立单 |

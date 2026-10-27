# REQ-157 · P3 · v4 关系动词命名提案（**待签，未落库**）

- 日期：2026-10-27
- 状态：**提案就绪 · `reviewed_by: null` · ⛔ 未冻结、未写库**
- 输入：线上 v3 的 34 条关系（`/api/catalog` 实测）+ `.local/tencent-docs` 语料
- 产出：`proposal-v4-naming.json`（34 条：**改 5 · 保 29**）

---

## 一 · 结论

**34 条关系里只有 5 条需要动**，其余 29 条的动词已在字典内且无泛词问题。

| 关系 | 原动词 | 新动词 | 动作 | 依据（逐字） |
| --- | --- | --- | --- | --- |
| `cbs_disk__cvm_instance` | `attaches_to` | **`attached_to`** | 别名归一 | `cbs.md:141`「云硬盘挂载的云主机ID」 |
| `clb_listener__clb_backend` | `binds` | **`binds_to`** | 改名 | `clb.md:67`「监听器绑定的…绑定的后端服务的端口随监听器端口」 |
| `clb_listener__ssl_certificate` | **`uses`**（泛词） | **`binds_to`** | 改名 | `clb.md:828`「**监听器绑定的证书信息**」 |
| `clb_target_group__clb_backend` | **`contains`**（泛词） | **`binds_to`** | 改名 | `clb.md:1584`「TARGETGROUP 表示绑定目标组」 |
| `clb_target_group__clb_rule` | **`associated_with`**（泛词） | **`references`** | 改名 | `clb.md:39`「关联到的转发规则ID」 |

**泛词清零：3 → 0**（`uses` / `contains` / `associated_with` 全部有文档依据地替换掉）。
**同义不并存**：`attaches_to` 消失（归一为 `attached_to`），`binds` 消失（归一为 `binds_to`）。

---

## 二 · 证据是怎么验的（**不是"我看了觉得对"**）

对每条改动，脚本做两件事：

1. **quote 必须在该文件里逐字存在**
2. **quote 必须就是所引行号那一行**（`lines[line-1].strip() == quote`）

```
✅ cbs_disk__cvm_instance          cbs.md:141
✅ clb_listener__clb_backend       clb.md:67
✅ clb_listener__ssl_certificate   clb.md:828
✅ clb_target_group__clb_backend   clb.md:1584
✅ clb_target_group__clb_rule      clb.md:39

逐字不匹配: 0 条
```

> ⚠️ 过程中我**自己踩过一次**：第一版 `cbs_disk` 的 quote 是我概括的「云硬盘挂载于实例」，
> 不是原文；脚本判为"不可定位"，我改成 `cbs.md:141` 的原句才过。
> **这正是"证据必须逐字"的意义——概括很容易就变成编造。**

---

## 三 · 需要你拍板的两件事

### ① `binds_to` 现在覆盖三种不同的对

| 关系 | 语义 |
| --- | --- |
| `clb_listener → clb_backend` | 监听器绑定后端 |
| `clb_listener → ssl_certificate` | 监听器绑定证书 |
| `clb_target_group → clb_backend` | 目标组绑定后端 |

文档对这三处用的都是「**绑定**」，所以**有据可依的选择就是 `binds_to`**。

**但**如果你觉得"绑定后端"和"绑定证书"应该分开，那就需要给字典加词（例如 `serves_traffic_of` / `presents_certificate`）——**那是新的业务判断，要你定**，我不擅自加。

**我的建议**：**先按 `binds_to` 收**。理由：三处文档用词一致，硬造区分反而失去依据。将来真出现歧义再加。

### ② 是否直接进 v4

| 选项 | 说明 |
| --- | --- |
| **A（建议）** | 把这 5 条改动作为 **v4 的唯一变更**，你签 `reviewed_by` 后我冻结 v4 |
| B | 等 REQ-157 的字典/校验落地后，**连同其它治理项一起**出 v4 |

**我倾向 A**：改动面已经很小（5 条、全有证据），**早一天让客户看到「监听器绑定的证书」比「使用」有用**。

---

## 四 · 这次**没有**做的事（明确边界）

| 没做 | 为什么 |
| --- | --- |
| ⛔ 写库 / 冻结 v4 | **要人签 `reviewed_by`**，我不代签 |
| ⛔ 改 v3 | 版本不可变 |
| ⛔ 重抽全部 34 条 | 29 条已经是对的，**重抽只会引入新的不确定性** |
| ⛔ 给另外 29 条编 evidance | 它们本来就在字典内，无需依据（保留原样即正确） |

---

## 五 · 复现

```bash
# 1) 取线上 v3 的关系（含 semantic）
curl -s "$BASE/api/catalog?page=5&page_size=200" -H "Authorization: Bearer $TOK" > rel.json

# 2) 生成提案（脚本在 .local/_gen_proposal.py）
python .local/_gen_proposal.py

# 3) 证据逐字校验（脚本在 .local/_verify_ev.py）
python .local/_verify_ev.py     # 期望：不可定位的证据 = 0

# 4) 字典校验（REQ-157 P1 的命令落地后）
ontok-builder relation verbs check --proposal proposal-v4-naming.json   # 期望 exit 0
```


---

## 六 · 用字典校验提案（**已跑，全绿**）

拿 REQ-157 P1 的字典草稿（16 主键 / 13 别名 / 6 禁用）对提案跑一遍：

```
命中禁用: 0    未知词: 0    需归一: 0
```

**提案落地后的动词分布**：

| 动词 | 条数 | 变化 |
| --- | ---: | --- |
| `belongs_to` | 16 | 不变 |
| `binds_to` | **5** | 2 不变 + **3 从泛词/别名改来** |
| `references` | **5** | 4 不变 + **1 从 `associated_with` 改来** |
| `attached_to` | **2** | 1 不变 + **1 从 `attaches_to` 归一** |
| `notifies_via` / `applies_to` / `targets` / `bills` / `based_on` / `deployed_to` | 各 1 | 不变 |

| 指标 | 改前（v3） | 改后（v4 提案） |
| --- | ---: | ---: |
| 动词种类 | **15** | **10** |
| 泛词条数 | **3** | **0** |
| 同义并存 | `attaches_to`+`attached_to` · `binds`+`binds_to` | **无** |

> 15 → 10 不是"丢信息"：**消失的 5 种是别名和泛词**，语义都落到有证据的主键上了。


---

## 七 · 已落地（2026-10-27）· **实际是 v5，不是 v4**

用户签了（`reviewed_by = michael@2026-10-27`）后执行：

| 步骤 | 命令 | 结果 |
| --- | --- | --- |
| 建版本 | `version new` | v4（**空草稿**） |
| 导入 | `import --file <修正版 v3 提案>` | 提案 `p1`，64 条 |
| 审核 | `review decide --p1 --accept-all` | 64 条 × 2 轨 accepted |
| 应用 | `proposal apply --p1` | **→ v5**（apply 在 base 之上**再建一版**） |
| 校验 | `relation verbs check --version 5` | **exit 0**，34/34 干净 |
| 冻结 | `version freeze --version 5` | **已冻结**（跑过发布闸） |
| 部署 | `ONTOK_VERSION 3 → 5` | 线上 `ontology_version=5` |

### 落地后的实测

```
relation 34 · 中文标签 34 · ASCII 标签 0
泛词：uses=0  associated_with=0  contains=0
标签分布：归属于 16 · 绑定到 5 · 关联到 5 · 挂载于 2 · 通知到 1
         · 适用于 1 · 指向 1 · 计费到 1 · 基于 1 · 部署于 1
TTL 210,610 B（v3 是 209,429）
```

### 我用来证明"改动面只有这 5 条"的方法

导出 v3 / v5 的**建模视图 TTL**，逐行 diff：

```
24 行差异 = 版本头 4 行（label + ontok:version）
          + 5 条语义 × 4 行（ontok:semantic ±  /  rdfs:comment 语义 ±）
非语义差异: 4 行（只有版本头）
```

⇒ **v5 = v3 + 恰好那 5 条**，没有顺手改到别的东西。

### 过程中的三个坑（记下来）

| # | 坑 | 教训 |
| --- | --- | --- |
| 1 | `base_version` 必须是**未冻结**的草稿（填 3 被拒：「v3 已冻结，不能在其上提案」） | 提案是"在某版**之上**改"，不是"改某版" |
| 2 | `apply` **不是填进交进去的那版**，而是**再建一版** | 所以 v4 成了空草稿，真身是 v5 |
| 3 | 本地 `ontok-builder` 是**旧二进制**（比 REQ-157 早），`relation` 子命令不存在 | 清 target 缓存后必须重建，否则拿旧工具验新代码 |

### 一个仍然存在的缺口（建议单独立单）

**"签字"在流水线里没有被强制**：

```rust
review decide --proposal p1 --accept-all      // 没有 --by
```

`apply` 的门是"**已决策**"，不是"**有人签过**"。而我签的那份命名提案
（`reviewed_by = michael@…`）**并不参与**这条机器路径 —— 它是给人看的依据文件。

⇒ 建议：`review decide` 加 `--by <人>` 写进 `decided_by/reviewed_by`，`apply` 校验非空。
否则"AI 自己 accept-all 再 apply"与"人签过"在库里**长得一模一样**。

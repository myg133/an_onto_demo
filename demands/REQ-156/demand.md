# REQ-156 · 把关系语义动词透出到 API 与 Studio

- 状态：**已就绪**
- 提出：2026-10-27
- 类型：**透出**（数据已存在，只是没往外递）
- 关联：REQ-150（关系语义）、REQ-155（Studio 显示）、REQ-157（动词治理，后续）

---

## 一 · 一句话

本体里**已经有** `semantic`（34 条 / 15 种动词），但 `/api/catalog` 不输出它 ⇒ Studio 只能显示 `from__to`。本需求把它透出来，并给中文标签。

---

## 二 · 事实（已核实）

| 层 | 有没有 semantic | 证据 |
| --- | --- | --- |
| 提案 | ✅ | `"payload": {"key": "cvm_instance_belongs_to_subnet", "semantic": "belongs_to"}` |
| 本体库 `link_types.semantic` | ✅ | `apply.rs:422` 读它，缺省 `"has"` |
| 建模视图 TTL | ✅ | `ontok:semantic` **34 次**，15 种动词 |
| 客户视图 TTL | 🟡 降级为 `rdfs:comment` | `语义：归属于（多对一）` |
| **`/api/catalog` 关系项** | ❌ **没有** | 只有 `name`/`from`/`to`/`cardinality`/`evidence_level` |
| Studio 显示 | ❌ 只能 `name`（=`from__to`） | —— |

**现有 15 种动词**（英文 key，中文只有 6 个有映射）：

```
belongs_to 16 · references 4 · binds_to 2 · attached_to 1 · attaches_to 1
notifies_via 1 · applies_to 1 · targets 1 · binds 1 · uses 1 · contains 1
associated_with 1 · bills 1 · based_on 1 · deployed_to 1
```

---

## 三 · 要做的

| # | 改哪 | 做什么 |
| --- | --- | --- |
| ① | `src/query/catalog.rs` 的 `RelationItem` | 加 `semantic`（原样英文 key）+ `semantic_label`（中文，走 `semantic_cn`） |
| ② | `src/domain/view.rs::semantic_cn` | **补齐映射**：现有 15 个词全部有中文（含 `attached_to`/`attaches_to` 同义归一） |
| ③ | Studio（`src/studio/studio.js`） | 边标签优先 `semantic_label`；抽屉关系行显示 `标签 + 英文 key` |
| ④ | 客户视图 TTL | 关系上加 `skos:altLabel "归属于"`（现在只有中文 `rdfs:comment`，机器读不到） |

### ② 的映射（B 方案：英文 key + 中文标签）

| key | 标签 | 备注 |
| --- | --- | --- |
| `belongs_to` | 归属于 | 已有 |
| `references` | 关联到 | 已有 |
| `has` | 拥有 | 已有 |
| `depends_on` | 依赖 | 已有 |
| `records` | 记录了 | 已有 |
| `derived_from` | 算自 | 已有 |
| `binds_to` / `binds` | 绑定到 | **同一件事，归一到 `binds_to`** |
| `attached_to` / `attaches_to` | 挂载于 | **同一件事，归一到 `attached_to`** |
| `notifies_via` | 通知到 | 新增 |
| `applies_to` | 适用于 | 新增 |
| `targets` | 指向 | 新增 |
| `uses` | ⚠️ 使用 | **泛词**，REQ-157 必清 |
| `contains` | ⚠️ 包含 | **泛词**，REQ-157 必清 |
| `associated_with` | ⚠️ 关联 | **泛词**，REQ-157 必清 |
| `bills` | 计费到 | 新增 |
| `based_on` | 基于 | 新增 |
| `deployed_to` | 部署于 | 新增 |

---

## 四 · 验收

| # | 判据 |
| --- | --- |
| AC-001 | `/api/catalog` 的 relation 项含 `semantic` 与 `semantic_label`；34 条**全部**有值 |
| AC-002 | `semantic_label` **不含任何 ASCII 字母**（复用 `untranslated_semantic_hit` 的形态判据） |
| AC-003 | `attached_to` 与 `attaches_to` 的 `semantic_label` **相同**（都是「挂载于」） |
| AC-004 | 未知动词（造一个 `xyz_unknown`）⇒ `semantic_label` 为 **null**，且**不 swallowed**：catalog 正常返回 + 该条 `semantic` 原样保留 |
| AC-005 | Studio 边标签显示中文；无标签时退回英文 key（⛔ 不许显示空白） |
| AC-006 | 8 case：**02 首屏不变**（分页口径：默认 50 条第 1 页没有 relation）· **03 describe +408 B** · **07 TTL +1163 B** · 其余 5 个逐字节不变 |
| AC-006b | **新增 case 02b**：`?page_size=200&page=5`（relation 页）⇒ relation 项含 `semantic`/`semantic_label`，且**其余字段逐字节不变** |
| AC-006c | 07 的 +1163 B 要能**拆开解释**：变中文的 `语义：` = 34 行 · 新增 `skos:altLabel` = 34 行 · 两者能对上字节数 |
| AC-007 | `get` 键数不变（v1=4 / v2=10）；`meta.contract_version` 不变 |
| AC-008 | ⛔ 不改 `semantic` 的存储值（只加映射与透出）；⛔ 不重写 v3 |

---

## 五 · 不做（留给 REQ-157）

| 不做 | 为什么 |
| --- | --- |
| 删/改 v3 里的英文动词 | 那要重抽出新版本，是数据治理 |
| 禁泛动词（`uses`/`contains`/`associated_with`） | 同上 |
| 动词字典文件 + 校验（拒绝字典外动词） | REQ-157 的 P1 |
| 改 `parse.rs` 的 schema 示例 | REQ-157 的 P2（与 skill 一起改） |


---

## 六 · 红线例外（已裁定 · 2026-10-27）

`tests/test_req107_red_lines.rs` 把 `src/domain/view.rs` 列为**整文件禁改**，与本单 §三 ② 冲突。

**裁定：批准为"有条件的例外"，⛔ 不是整文件白名单。**

| # | 要求 |
| --- | --- |
| A1 | 在 `red_lines` 里**点名** `src/domain/view.rs`，注释写明"REQ-156 授权：只允许**追加** `semantic_cn` 映射分支"；其余三个红线文件**仍整文件禁改** |
| A2 | **新增专属守卫**证明"只追加了"：① 原 6 条映射逐字未变 ② 兜底仍是 `_ => s.to_string()` ③ 每个分支输出不含 ASCII 字母 ④ `untranslated_semantic_hit` 判据未被改写 |

**理由**：白名单只放行文件，不放行"改了什么"。**"允许追加"必须由机器证明只追加了**，不能靠人看 diff。

守卫必须能红：
- 把 `belongs_to → 归属于` 改成「所属」⇒ ① 红
- 把 `_ =>` 改成 `_ => "未知"` ⇒ ② 红

---

## 七 · 顺带记录的一条独立 bug（待确认性质）

实测发现：**v3 的客户视图 TTL（`/api/ontology.ttl`）里有 13 个英文动词漏出**（`applies_to` / `attached_to` / `uses` …），
而 `untranslated_semantic_hit` 的判据是"meaning 里出现任何 ASCII 字母就抓"。

**两种可能，性质完全不同**：

| 可能 | 性质 | 处置 |
| --- | --- | --- |
| `/api/ontology.ttl` 的渲染路径**不经过**该守卫 | **fail-open bug**（客户能拿到未翻译的内部标识符） | **单独立单** |
| 路径经过，只是 v3 冻结在守卫之前 | 历史数据问题 | 记录即可，重冻结即修复 |

**已要求 Dev 用 `path:line` 确认是哪一种。**

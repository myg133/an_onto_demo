# REQ-173 · 让取数配方**自足**：`@pending.get` 带上 `identity` 与 `iri_template`

- 状态：**已就绪**（P1）
- 触发：数字员工平台真实对接（2026-10-28 `decision-trace-5018c7e4`）
- 关联：REQ-169（配方带 `auth.service`）、REQ-164（取数脚本）

## 一 · 事实（线上实测）

`@pending[0].get` 现在的键：

```
['auth','class','fields','filter','iterator','operation','pushdown','ref','request','source']
```

**没有 `identity` / `iri_template`** —— 而 AM 里明明有：

```json
{ "identity": {"key_path": "InstanceId"}, "iri_template": "ontok:cvm_instance/{id}" }
```

## 二 · 后果（实测的坏体验）

取数脚本要把一行变成节点，必须有 `@id` ⇒ 需要 `identity`。配方里没有，于是：

1. 脚本要求调用方传 `--id-property <fields 里的属性名>`
2. 平台上的 agent **只能猜**：先猜 `instance_id`（本体属性名），失败了才改猜对
3. rows 模式下漏传 ⇒ 每行被 `skipped_no_identity` 丢掉 ⇒ **0 行且不报错**

⇒ 配方"看起来完整"，实际**少了把自己变成图的那一环**。

## 三 · 要做的

`Recipe` 增加两个字段（从 AM 的 `entities[]` 原样取，⛔ 不推导）：

| 字段 | 来源 |
| --- | --- |
| `identity` | `entities[].identity`（至少含 `key_path`） |
| `iri_template` | `entities[].iri_template` |

- 两者都 `skip_serializing_if = "Option::is_none"`（v1 契约下不出现，AC-103 不回退）
- `build_recipe()` 原样填；AM 没写 ⇒ 键不出现（⛔ 不塞空串）

## 四 · 验收

| # | 判据 |
| --- | --- |
| AC-001 | 真进程 + 真库：`@pending[].get.identity.key_path` == AM 里该 entity 的 `identity.key_path`（用**与 fields 属性名不同**的值，如 `InstanceId`，防"猜对了也算"） |
| AC-002 | `get.iri_template` == AM 的 `iri_template` |
| AC-003 | v1 AM ⇒ `@pending.get` 仍恰好 4 键（AC-103 不回退） |
| AC-004 | AM 里没写 `identity` ⇒ 配方里**无** `identity` 键（不塞空） |
| AC-005 | 端到端：线上回执原样喂 `access_fetch.py --recipe`，**不传 `--id-property`** 也能出 jsonld（脚本侧同步改） |

**红绿自证**：把 `build_recipe` 里填 `identity` 的两行去掉 ⇒ AC-001/AC-005 红。

## 五 · 脚本侧（同步改，见 REQ-164 的后续）

- `--id-property` 改为：**配方自带 `identity` 时不需要**（优先级：`--id-property` > 配方 `identity` > 报错）
- `--entity` 在配方只有 1 条 entity 时**可省**
- `--recipe` 接受**整段回执**（`@pending` 数组 / 整条 entry / `get` / 裸配方）四种形状

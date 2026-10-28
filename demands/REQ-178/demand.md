# REQ-178 · TTL 两视图**改成按需生成**：一个词写错不该让整个项目打不开

- 优先级：**P1**
- 状态：已就绪
- 触发：2026-10-29 客户实测 —— `yiyao` 的客户视图命中发布闸 ⇒ `/api/ontology.ttl` 产不出来
  ⇒ **装配期 fail-closed** ⇒ **整个项目 500 `project.load_failed`**（catalog / describe / sparql 全挂，Studio 切不过去）
  ⇒ 连"重新传一个修好的包"都只能靠 API 绕（Studio 进不去）。

## 一 · 病根

`query::serve::prepare_project` 在**装配期**就把 TTL 两视图渲染出来了（fail-closed 设计）。
后果是「一个出口的失败，拖垮整个项目的所有出口」：

| 出口 | 受影响 |
| --- | --- |
| `/api/catalog` `/api/describe` `/api/sparql` `/api/node` MCP | ❌ **本不该受影响**，现在一起 500 |
| `/api/ontology.ttl` | ✅ 该失败（它就是要产 TTL 的那个） |

而 TTL 渲染失败的原因往往是**一条说明文字**（如实体别名 `protocol` 被误判，或某个词命中词表）——
也就是说：**改一行文字成就的失败，让整个项目下线**。

## 二 · 要做的

1. 装配期**不再**渲染 TTL 两视图：只把「渲染所需的东西」（快照 + 选项）存进 `QueryService`
2. `/api/ontology.ttl` **请求时**渲染，**按 version_id 缓存**（同版本第二次不重渲染）
3. 渲染失败 ⇒ **只这个端点** 500，错误码用**新的** `project.ttl_failed`（保留原始原因与命中的词条）；
   ⛔ 不再用 `project.load_failed`（那个语义是"项目装不起来"）
4. 其余出口行为**逐字节不变**（catalog / describe / sparql / TTL 成功时字节完全相同）
5. MCP 若有 ttl 工具，同样改为按需（失败时返回同样的错误码，不影响其他 MCP 工具）

## 三 · 验收

| # | 判据 |
| --- | --- |
| AC-001 | 客户视图命中闸门的项目：`catalog` / `describe` / `sparql` **200**；`/api/ontology.ttl` **500 `project.ttl_failed`**（消息里带命中词条） |
| AC-002 | 正常项目：TTL 字节与改前**逐字节相同**（拿真库比 sha256） |
| AC-003 | 同一 `version_id` 连续两次请求 TTL ⇒ 第二次**不重渲染**（用计数/耗时断言，别只看时间） |
| AC-004 | 换版本（activate 新版本）⇒ 下一个 TTL 请求用**新版本**（缓存键是 version_id） |
| AC-005 | 单项目模式同样生效（`--project/--version` 钉死时也只在请求期渲染） |
| AC-006 | **写面不受影响**：坏项目也能 `POST bundles` / `import` / `activate`（这条现在就是对的，别改坏） |

**红绿自证**：把"按需渲染"改回装配期 ⇒ AC-001 红（catalog 也 500）。

## 四 · 不在本次范围

- 发布闸本身（词表 / 形态判据）不动
- Studio 页面的交互改动（在 lion 那侧；本单只保证"读端点不再整体 500"）

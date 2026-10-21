# 设计概要：REQ-110 · 内置 TTL 导出 + 页面查看与下载

> 一句话：**把「我用手写脚本 + 拖文件」这两步，收成产品的一个命令 + 页面上的一个按钮。**

---

## 一 · 两条路径，一个函数

```
CLI:  ontok ontology export --format ttl
              │
              ├──▶ ttl::serialize(&view, opts) ──▶ 写文件
              │
HTTP: GET /api/ontology.ttl (Bearer)
              │
              └──▶ ttl::serialize(&state.view, opts) ──▶ 200 text/turtle
```

**同一条序列化路径**（纪律 H：不许第二份实现）。页面按钮只是带 token 拉这个端点。

## 二 · 序列化器（`src/domain/ttl.rs`，纯函数）

### 2.1 映射（照规格 §5.5）

| 本平台 | Turtle |
| --- | --- |
| `EntityType` | `owl:Class` + `rdfs:label` / `rdfs:comment` |
| `EntityType.parent` | `rdfs:subClassOf` |
| `disjoint_with` | `owl:disjointWith` |
| 属性 | `owl:DatatypeProperty` + `rdfs:domain` |
| `LinkType` | `owl:ObjectProperty` + `rdfs:domain` / `rdfs:range` |
| `cardinality` 1:1 / 1:N | `owl:FunctionalProperty` |
| `cardinality` N:1 / 1:1 | `owl:InverseFunctionalProperty` |
| `semantic` | **降级为 `rdfs:comment`**（有损） |
| `ActionType` | **不导出**，头部注释声明数量（有损） |
| 溯源 | `prov:wasGeneratedBy` |

### 2.2 确定性

- 类按 **key 字典序**、属性按 **(类 key, 列名)**、关系按 **key** 排序
- 前缀表固定顺序输出
- 时间戳**不写进正文**（写进头部注释会破坏逐字节一致 ⇒ 用固定占位或省略）
  - 裁定：**省略生成时间**；版本信息写在 `ontok:version` 字面量里

### 2.3 转义

- 字符串：`\` `"` 换行/制表 必须转义
- 中文按 UTF-8 原样输出（不加 `\u` 转义），但文件名/`@zh` 语言标签保留

## 三 · 门禁顺序（客户视图）

```
CustomerView ──▶ serialize ──▶ scan_modeling_terms(ttl_text)
                                    │
                          命中 ─────┴──▶ 拒绝导出（exit≠0 / HTTP 500 + 词条）
```

**为什么在序列化后扫**：TTL 是最终字节，扫它才覆盖「序列化过程自己引入的文本」（例如前缀名、注解）。只扫结构字段会漏。

## 四 · 页面可达性（本需求唯一的安全取舍）

| 路由 | token | 理由 |
| --- | --- | --- |
| `GET /` | **免** | 静态壳，**不含任何本体数据**（项目名/版本/图都不是服务端渲染进去的） |
| `GET /api/graph` / `/api/node` / `/api/ontology.ttl` | **必须** | 全部数据 |

实现：`require_token` 从「全局 layer」改为**只挂 `/api/*` 子路由**。

### 代价（写进 ADR）

原来「未登录连路径都探测不到」变成「能探测到有个服务，但拿不到任何数据」。
判据改写为：`/` = 200 且响应体不含数据；`/api/*` = 401。**测试改写，不删除。**

## 五 · 页面改动

```html
<button id="export">导出 TTL</button>
```
- 用页内 token `fetch('api/ontology.ttl', {headers:{Authorization:'Bearer '+TOKEN}})`
- `blob` + `URL.createObjectURL` 触发下载，文件名从 `Content-Disposition` 里取
- 失败时把错误显示在状态栏（401 ⇒ 「口令不对」）

图渲染**不动**：现有 `/api/graph` + SVG 已够用。

## 六 · 代码落点

| 文件 | 职责 |
| --- | --- |
| `src/domain/ttl.rs`（新） | 纯序列化 + 有损声明 + 确定性排序 |
| `src/cli.rs`（改） | `ontology export` 子命令 |
| `src/server/http.rs`（改） | `/api/ontology.ttl` 路由；middleware 收窄到 `/api/*` |
| `src/server/page.rs`（改） | 导出按钮 + 下载逻辑 |
| `tests/test_req110_*.rs`（新） | AC-001~010 |

## 七 · 已知限制

| # | 限制 | 影响 |
| --- | --- | --- |
| G1 | 只导出 TBox（类/属性/关系），不含实例（ABox） | 需要实例得另做，属独立需求 |
| G2 | `ActionType` 不可导出 | OWL 无对应，头部声明有损 |
| G3 | 不导 `SHACL` 形状（nullable/必填） | 后续可加，本轮不做 |
| G4 | 页面壳免 token 后，服务存在性可被探测 | ADR 记录并接受 |

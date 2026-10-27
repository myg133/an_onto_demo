# 需求：REQ-161 · Console 换皮（设计样机 → 产品页）

## 基本信息

| 项 | 值 |
| --- | --- |
| 需求编号 | REQ-161 |
| 优先级 | **P1**（FDE 每天用的工具，现在还是一屏 6 个 tab 挤在顶栏的旧壳） |
| 状态 | **已就绪** |
| 基线 | `develop` `b2e6b8e` |
| 触发 | 用户 2026-10-27 13:1x：看产品 console（8899）后问「怎么不是新版的 console 页面」，随即拍板「**换皮要做的**」 |
| 关联 | REQ-125（现有页面壳）、REQ-155（UI 重组：P1+P2 已做，**P3 Console 换皮一直是「待做」**）、ADR-010（Studio 挂 query serve）、ADR-013（REQ-159 端点迁 Studio） |

---

## 一 · 一句话

> 把 `BA/design/console-prototype/`（FDE 版工作台样机）落成**产品页**，
> **视图不变、端点不变、逻辑不变**，变的是壳与信息密度。

**现状**：产品里跑的是 REQ-125 的旧壳（顶栏 8 个 tab）；新设计**只存在于样机**（`BA/design/console-prototype/`，README 自己写着「这是设计稿，不是产品代码」）。
登记表里 REQ-155「P3 Console 换皮」一直是**待做**；今天所有 P3 提交都是 **Studio** 的修复，没有一行落在 `page.rs`。

---

## 二 · 设计与现有实现的**结构差异**（决定了这不是「套 CSS」）

| 维度 | 现状（REQ-125 壳） | 样机（目标） |
| --- | --- | --- |
| 导航 | **顶栏 tab**（8 个并列，`#tabs`） | **左侧窄导航 + 单一主区**（能常驻显示连接状态、token 是否有效） |
| 密度 | 普通表格 | **行高 32px + 表头 sticky**、一屏 20+ 行 |
| key / IRI | 普通字体 | **一律等宽**（`.key` 类），靠视觉模式识别 |
| 详情 | 无 | **右侧抽屉**（目录浏览点一行看详情） |
| 分区 | 各 tab 各自为政 | 主区内**分块面板**（每块一个动作 + 一句「为什么」） |

---

## 三 · 视图 ↔ 端点 ↔ 必须保留的逻辑（**这张表是验收的骨架**）

| # | 视图 | 端点（**一个都不许新增**） | 必须原样保留的逻辑 |
| --- | --- | --- | --- |
| 1 | 项目与版本 | `GET /api/projects` · `POST /api/apply` · `POST /api/freeze` | apply 的 409 语义、freeze 的 `guarded` 二次确认（写明目标+后果） |
| 2 | 评审 | `GET /api/review` · `POST /api/decide` | **两轨裁决**、`progress.pending/vetoed` 汇总口径、建议分档 |
| 3 | 导出 | `GET /api/export/ttl` · `POST /api/r2rml` | 与 CLI 产物**逐字节相同**（页面不加工） |
| 4 | 声明编辑 | `GET/PUT /api/config/{exclusions,policy,tenancy}` | 保存前**按真实 schema 校验**、失败不落盘、整份替换 |
| 5 | 审计 | `GET /api/audit` | token 只显示 sha256 前 12、失败也记 |
| 6 | 查询台 | `POST /api/sparql`（query serve，同源） | **默认 JSON-LD**；显式 `format:"rows"` 才给表格；四段元信息（warnings/coverage/join_states/truncated）必须显示 |
| 7 | 目录浏览 | `GET /api/catalog` · `GET /api/describe` | 分页、`access.kind` 展示、右侧抽屉看详情 |
| 8 | 构建包 | REQ-159 的 8 条（现挂在 console） | **ADR-013 的「已迁到 Studio」提示条要保留**；`progress.pending` 门禁、`already_imported` 当成功、404 降级不登出、`reveal_key` 审计提示、`dsnFlag` 缺失显示 `—` |

**壳的纪律（一条都不能松）**：

| # | 纪律 | 判据 |
| --- | --- | --- |
| S1 | 壳内**零数据**（无项目名/版本号/对象名/示例 token） | `page.rs` 单测既有断言 |
| S2 | **禁** `localStorage` / `indexedDB` / `document.cookie` / `location.search` | `test_req125_page.rs::ac003` |
| S3 | 口令只进 `sessionStorage`；页面壳免 token、`/api/*` 要 token | `ac001` |
| S4 | 零外部依赖（无 CDN/字体/图标）、CSP 自包含 | `ac002` |
| S5 | 危险动作（apply / freeze / 声明保存 / 激活 / 换 key）**必须**走 `guarded` 且写「目标 + 后果」 | `page.rs` 断言（现为 5 处） |
| S6 | 单文件体积 **≤ 300 KB**（新设计比旧的大；旧壳 96 KB，样机 18 KB HTML+CSS+JS 说明有富余） | 新增断言 |

---

## 四 · 不做

| ⛔ | 理由 |
| --- | --- |
| 新增/修改任何 `/api/*` 端点 | 换皮只动壳；端点变更属 REQ-159/REQ-161 之外的线 |
| 改业务逻辑（门禁、幂等、降级、审计） | 那些是判据，不是装饰 |
| 把 Studio 的视觉（能量球/轨道/辉光）搬进 console | Console 是**工具**，密度优先；两套共用 token 但密度不同（样机 README §一） |
| 删掉「构建包」视图 | ADR-013 只把它标为「已迁到 Studio」，代码保留作参考实现 |

---

## 五 · 验收标准

### AC-001 · 结构到位
- 左下导航含 **8 个视图入口**（文字与顺序见 §三），**顶栏 `#tabs` 不再存在**
- 主区单一滚动容器；目录浏览有**右侧抽屉**（默认收起，点行展开）
- 判据：DOM 断言（`#nav` / `.view` / `#drawer`）+ 真浏览器可点

### AC-002 · 视觉规范可判
- **key/IRI 一律等宽**：抽查 `.key` 类覆盖 目录浏览/审计/评审 三处
- **表头 sticky**、行高 32px：CSS 断言（`position: sticky` + `height: 32px`）
- 色板与 Studio **同一套 token**：`--fg/--mut/--acc/--ok/--warn/--bad/--line` 同名同值
- 判据：CSS 源码断言 + 真浏览器截图人看

### AC-003 · 逻辑零退化（**逐条**）
§三 表里「必须保留」的每一项各一条断言；⛔ 不许删弱既有断言（`ac001`~`ac004` 原样保留）。

### AC-004 · 端点零变化
- `grep` 页面里出现的 `/api/` 路径集合 **与本单之前的集合完全相等**
- 判据：集合相减为空（脚本化，写在测试里）

### AC-005 · 真浏览器（BA 的 Playwright harness）
现有 23 项全过 + 新增 4 项：左导航切换、抽屉开合、sticky 表头、等宽 key 生效。

### AC-006 · 体积与依赖
单文件 ≤ 300 KB、零外部资源引用（`http(s)://` 出现在 HTML 里即红，除注释）。

---

## 六 · 风险

| # | 风险 | 缓解 |
| --- | --- | --- |
| 1 | `page.rs` 是 96 KB 单文件 + 内嵌测试，大改容易顺手删弱断言 | **先把断言清单冻结**（§三 表 + S1~S6），Dev 每步跑 `cargo test --lib console::page` + `test_req125_page` |
| 2 | 换皮时把「构建包」那 8 条端点顺手改成 Studio 的 | AC-004 的集合相等判据专治这个 |
| 3 | 样机是静态的（`snapshot.js` 假数据），照搬会把假数据带进产品 | 样机只作**布局与视觉**参考；数据一律来自 `/api/*`（S1 零数据断言） |
| 4 | 一次改完再测 ⇒ 崩了全丢 | 3 个检查点：壳+导航 → 7 视图重排 → 抽屉/等宽/sticky + 测试收口 |

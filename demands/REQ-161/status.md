# 状态：REQ-161 · Console 换皮（样机 → 产品页）

## 当前状态

**进行中**（2026-10-27 · 已认领 · 已派 Dev · worktree `feature-REQ-161`）

## 一句话

> 视图不变、端点不变、逻辑不变 —— **变的是壳与信息密度**（顶栏 tab → 左导航；普通表格 → 32px 行高 + sticky 表头；key/IRI 等宽；目录浏览加右侧抽屉）。

## 为什么现在做

| 事实 | 证据 |
| --- | --- |
| 产品跑的是 REQ-125 旧壳（顶栏 8 tab） | `src/console/page.rs` |
| 新设计**只存在于样机**，README 自称「设计稿，不是产品代码」 | `BA/design/console-prototype/README.md` 第一行 |
| REQ-155 P3「Console 换皮」**长期挂「待做」**，今天所有 P3 提交都是 Studio 修复 | `req-registry.md` + `22ab671`/`4be3b90`/`50bd66c` 全是 studio |
| 用户 13:1x 拍板 | 「换皮要做的」 |

## 三个不变（验收骨架）

1. **视图不变**：8 个视图（项目与版本 / 评审 / 导出 / 声明编辑 / 审计 / 查询台 / 目录浏览 / 构建包）
2. **端点不变**：`/api/*` 路径集合**逐字相等**（⛔ 不许借换皮改接口）
3. **逻辑不变**：§三 表里「必须保留」的每一项各一条断言（门禁 / 幂等 / 404 降级 / 审计提示 / dsnFlag / 两轨裁决 / 四段元信息）

## 壳的六条纪律（不许松）

S1 零数据 · S2 禁 localStorage/IndexedDB/cookie/location.search · S3 口令只进 sessionStorage · S4 零外部依赖 · S5 危险动作 5 处 guarded（写明目标+后果） · S6 ≤ 300 KB

## 派单

`BA/.local/dispatch/REQ-161-dev-brief.md`（3 个检查点：壳+导航 → 7 视图重排 → 抽屉/等宽/sticky + 测试收口）

## 待办

- [ ] Dev 三个检查点
- [ ] 我用**真浏览器**跑 23 项 + 新增 4 项（左导航 / 抽屉 / sticky / 等宽）
- [ ] QA（重点：端点集合相等、断言未被删弱、样机假数据没被搬进产品）
- [ ] 合并（待用户点头）

## 交付与我的复核（2026-10-27 13:35）

Dev 4 个 commit（`8e02355` → … → `a996cc7`），只改 `src/console/page.rs`（+796/−257）。

| 判据 | 结果 |
| --- | --- |
| `cargo check --lib` | 绿 |
| `cargo test --lib console::page` | **18 / 18**（原 11 + 新增 7） |
| `cargo test --test test_req125_page` | **8 / 8**（**该文件一行未改**） |
| `page.rs` / `CONSOLE_HTML` | 149.5 KB / **113.0 KB**（S6 ≤300 KB、ac002 <200 KB 都过） |
| `/api/*` 集合 | **12 条，本单前后逐字相等**（提取规则已固化成单测 `the_api_path_set_is_frozen`） |

**我的真浏览器复核（两套，23 + 12）**：

| 套 | 结果 | 覆盖 |
| --- | --- | --- |
| `ui-check.js`（功能 23 项，API 全 stub） | **23 / 23** | 建项目 / 上传 / 组件计数与 `requires_dsn` / DSN 先存后测 / 解析 / 待确认门禁 / 激活确认框 / P1b 警示 / 查看 key / `reveal_key` 审计 / 0 JS 异常 |
| `ui-check-structure.js`（结构 12 项） | **12 / 12** | `#nav` 存在、`#tabs` 已移除、**8 个入口**、视图切换、**点行开抽屉 / Esc 关**、**表头 sticky**、**行高 32px**、**`.key` 等宽**、0 JS 异常 |

⇒ 「视图/端点/逻辑三不变」在**真浏览器**下成立，不只是单测。

## Dev 报的 8 处「与样机偏离」（我逐条认可）

| # | 偏离 | 我的裁定 |
| --- | --- | --- |
| 1 | 样机 7 视图，产品 8 个（构建包按 ADR-013 保留） | ✅ 对 |
| 2 | 顶栏不做「项目/版本下拉」（样机是快照数据） | ✅ 零数据优先；项目/版本仍在视图内 |
| 3 | 不造统计卡片（`/api/projects` 无类/属性/关系聚合） | ✅ 造卡片就得在页面另算 = 新增加工 |
| 4 | 评审保留卡片而非 64 行表格 | ✅ 数据形状是「两轨+建议+置信度+payload」，卡片更贴 |
| 5 | 查询台不加三个样本、目录不加本地过滤 | ✅ 前者会引入示例数据（违反零数据），后者是新逻辑 |
| 6 | 抽屉内属性/关系用紧凑列表 | ✅ 460px 装 7 列每列只剩几十像素 |
| 7 | 沿用 7 个 token 名、值取 Studio 同套色板 | ✅ AC-002 点名的就是这套 |
| 8 | 深色 only（去掉浅色分支） | ✅ 与样机/Studio 一致 |

## 待办

- [ ] QA（重点：端点集合、断言未被删弱、零数据、真浏览器两套）
- [ ] 合并（待用户点头）

## QA 结论（2026-10-27 13:45）

**PASS，0 阻断**（P0=0 / P1=0）。报告 `BA/.local/qa/REQ-161/REPORT.md`。

| 组 | 结果 |
| --- | --- |
| **端点集合**（QA 自抽，未复用 Dev 测试） | 基线 12 = 现在 12，逐条相同；`bundlePath` 的 7 个动态拼接两侧也相同。⚠️ 唯一差异是**可见端点标签**让字面 `/api/` 多 2 条 —— AC-004 若写成「字面集合相等」会误红，Dev 的口径（`api/…` 归一）才对 |
| 端点变异 | 加死链 `/api/nonexistent` ⇒ 红；改 `describe`→`describes` ⇒ 红；撤回 ⇒ 绿 |
| **删除行审计** | 257 行删除里 **0 行**含 `assert`/`#[test]`/`fn`，全部在 `CONSOLE_HTML` 内；测试函数 11→18、`removed=[]`；`tests/test_req125_page.rs` **blob 前后相同、diff 0 行** |
| 纪律计数 | guarded 5→5（5 段**逐字节相同**）、`progress.pending` 8→8、`already_imported` 4、`reveal_key` 7、`dsnFlag` 2、ADR-013 提示 1、`localStorage/indexedDB/cookie/location.search/innerHTML` 全 0、`#tabs` 1→0、`nav-item` 0→8 |
| 真浏览器 | 复用 `ui-check.js` **23/23**、`ui-check-structure.js` **12/12**；QA 自写：评审 **10/10**、导出 8/9、四视图+抽屉 16/16 |

## 一处既有缺陷 → **裁定：并入本单修掉**

**`POST /api/r2rml` 前端解析错类型**：服务端回 `text/turtle`，页面走 `post()`→`json()`→`JSON.parse` ⇒ FDE 点「生成 R2RML」**必报 `BAD_JSON`**，错误文案还甩锅服务端。
QA 确认**基线同错**（基线 L922 与新 L1268 逐字相同），但它在本次刚重排的「导出」视图里，合并后会被误认为换皮换坏的。

| 项 | 决定 |
| --- | --- |
| 开新单？ | **不**（一行级别的修，且在同一个文件、同一个视图） |
| 怎么修 | 加 `postText()`（取 `r.text`），`exportR2rml` 改用它；⛔ 不动 `request()` 的返回契约、⛔ 不动 `/api/export/ttl` |
| 判据 | 结构断言（`postText(` 在、`post("api/r2rml"` 不在）+ 真浏览器（stub 回 `text/turtle` ⇒ 显示正文且无 `BAD_JSON`） |

## 另一个既有缺口（本单不修，记录在案）

`access.kind` 在基线里**也是 0 次出现**（目录浏览没展示它）—— 既有缺口，**不是本单删的**，验收时别记成回归。

## r2rml 修复的复核（2026-10-27 13:52）

Dev 追加 `492f927`（只动 `page.rs`，+62/−1）：新增 `postText()`，`exportR2rml` 改用它。

| 判据 | 结果 |
| --- | --- |
| `cargo check --lib` | 绿 |
| `cargo test --lib console::page` | **19 / 19**（18 → 19，新增 `r2rml_is_read_as_text_not_parsed_as_json`） |
| `cargo test --test test_req125_page` | **8 / 8** |
| **真浏览器 · 导出视图（我写的）** | **6 / 6** —— 确实 POST 一次；显示「R2RML 已生成」；**显示 Turtle 正文**（`@prefix rr:`）；**无 `BAD_JSON`**；无「服务端返回的不是 JSON」；无 JS 异常 |
| 回归（修复没碰坏别处） | 功能 **23/23**、结构 **12/12** |

⚠️ 一处容易误判，记下来：**DOM 里出现 `BAD_JSON` 字面量是正常的**（页面 JS 源码里就有这个错误码），判据要看**结果区**（`#e2-out`），不能全页 grep。

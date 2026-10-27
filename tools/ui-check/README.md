# BA/tools/ui-check · 控制台页面的**真浏览器**验证

> 为什么要有它：`src/console/page.rs` 的测试（`cargo test --lib console::page` / `--test test_req125_page`）
> 只能保证「壳里有什么字符串」和「HTTP 层要口令」；**点得通不通**它验不到。
> 假 DOM 探针（Dev 写的）也只覆盖逻辑，不覆盖真渲染与事件绑定。

## 怎么跑

```powershell
# 1) 从源码抽出页面（raw string 的内联 HTML）
$src = "..\..\code\src\console\page.rs"
$t = [IO.File]::ReadAllText($src)
$s = $t.IndexOf('r####"'); $e = $t.IndexOf('"####;', $s)
New-Item -ItemType Directory -Force .\ | Out-Null
[IO.File]::WriteAllText(".\console.html", $t.Substring($s + 6, $e - ($s + 6)), (New-Object System.Text.UTF8Encoding($false)))

# 2) 跑（本机 Playwright 与浏览器路径见下）
$env:NODE_PATH = "D:\ToolCaches\npm-cache\_npx\aa1f6563a672b75d\node_modules"
$env:EXE = "$env:LOCALAPPDATA\ms-playwright\chromium_headless_shell-1248\chrome-headless-shell-win64\chrome-headless-shell.exe"
node .\ui-check.js
```

## 本机环境（2026-10-27 实测）

| 项 | 值 |
| --- | --- |
| Playwright 包 | `D:\ToolCaches\npm-cache\_npx\aa1f6563a672b75d\node_modules\playwright`（npx 缓存） |
| 浏览器 | `%LOCALAPPDATA%\ms-playwright\chromium_headless_shell-1248\...\chrome-headless-shell.exe`（v156） |
| 直接 `chromium.launch()` 会失败 | 包默认找 `chromium_headless_shell-1217`，本机没装 ⇒ **必须传 `executablePath`** |

## 它怎么做到「不连库也能点」

1. 起一个**同源**静态服务（`127.0.0.1:<随机端口>`）只吐 `console.html` —— 满足页面 CSP 的 `connect-src 'self'`
2. `page.route('**/api/**')` 拦掉全部 API，返回**与 P1a 真实回执同形**的假数据（见脚本里 `apiResponse()`）
3. 用真 `setInputFiles` 传一个假 `.ontokbundle`，走完：登录 → 构建包 tab → 新建项目 → 上传 → DSN（**先存后测**）→ 解析 → 待确认门禁 → 激活（确认弹窗）→ 查看 key

⚠️ **顺序有讲究**：页面的 `saveDsn()` 会把 `dsnOk` 置 `false`（换连接即失效）⇒ 必须**先保存再测连通**，否则「解析入库」按钮一直是灰的（这不是 bug）。

## 已知局限（别当成「UI 全验了」）

| 没覆盖 | 说明 |
| --- | --- |
| 真服务端 | 全部 API 是假数据；真联调要起有库的 `console serve` |
| 视觉/CSS | 只验可交互与文本，不验样式、布局、截断 |
| 多浏览器 | 只跑 chromium headless（没有 firefox/webkit 的自动化覆盖） |

## 2026-10-27 首跑结果

**18/19 通过**，唯一 FAIL 是**页面真缺陷**：

```
组件计数：ontology — ｜ am — ｜ db —
```

收据里明明是 `components.ontology.entity_types/link_types/properties`、`components.am.entities/sources`
（契约 §五-2 的形状），但页面的 `countOf()` 只认 `count` / `item_count` / `total` / `items` / `entries`
⇒ FDE 传完包看到一列「—」，像是 manifest 里没写计数。

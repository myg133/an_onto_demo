//! REQ-155 · **Studio 页面壳** —— 服务面（`ontok query serve`）上的三条静态资源路由。
//!
//! ```text
//! GET /studio                → index.html   (text/html; charset=utf-8)
//! GET /studio/studio.css     → studio.css   (text/css; charset=utf-8)
//! GET /studio/studio.js      → studio.js    (application/javascript; charset=utf-8)
//! ```
//!
//! # 为什么挂 `query serve` 而不是 `ontok serve`（ADR-010-A）
//!
//! `ontok serve` 只接受 GET（那是它最强的保证），而 Studio 的查询台要
//! `POST /api/sparql` —— 放进去就破了它。`query serve` 已经有 `health` /
//! `catalog` / `describe` / `sparql` / `ontology.ttl`，**且已公开、已 token 鉴权**，
//! 所以这里只加**静态资源**：零新增接口、零新增攻击面。
//!
//! # 页面壳**零数据**（AC-008b）
//!
//! 三个文件是**编译期常量**（`include_str!`）：没有类名、没有关系名、没有属性名、
//! 没有项目名、没有版本号。数据一律经 `/api/*` 取（同源相对路径 ⇒ 不需要 CORS）。
//! 判据在下面 `tests`：拿五个真实类名 + `ONTOK_SNAPSHOT` 去 grep，**必须 0 命中**。
//!
//! ⛔ 设计稿里的 `snapshot.js`（离线快照）**不进产品**：它是 460 KB 的真实数据，
//! 进了仓库就等于把本体写死在页面里。
//!
//! # 只读（S1）
//!
//! 三条路由都只注册 `get`。`POST/PUT/DELETE /studio` 由 axum 的 `MethodRouter`
//! 直接回 **405**（不需要在 handler 里判方法）—— 判据见 `query::http` 的测试。
//!
//! # 鉴权（S6：`/api/*` 一个字节不变）
//!
//! `/studio` **不在** `query::http::needs_auth` 的前缀里（它只看 `/mcp` 与 `/api`），
//! 所以页面壳可匿名取得 —— 与 `console` 的 `GET /` 同口径。壳里既然零数据，
//! 「匿名拿壳」就等于「拿到一张空表格」。
//!
//! # 安全响应头（照 `console::page` 的做法）
//!
//! CSP / `nosniff` / `no-referrer` 三个头在 [`SECURITY_HEADERS`] 里**只有一份**，
//! 由 [`studio_response`] 统一叠上去。
//!
//! ⚠️ 与 console 的**唯一**区别：这里**没有** `'unsafe-inline'`。console 是单文件内联页
//! （`<style>` / `<script>` 全在 HTML 里），而 Studio 本来就是三个文件 ——
//! 于是 `script-src 'self'` / `style-src 'self'` 可以真的生效。
//! 代价是页面**不许**用 HTML 里的 `style` 属性（CSP 会忽略它），
//! 所以 `studio.js` 一律走 CSSOM（`el.style.cssText`），
//! `index.html` 里也一个 `style=` 都没有。

use axum::http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};

/// 页面壳（HTML）。**零数据**：类名 / 关系名 / 属性名一个都不在这里。
pub const INDEX_HTML: &str = include_str!("index.html");

/// 样式（设计稿 `BA/design/ui-prototype/studio.css` 的**原样**副本，逐字节相同）。
pub const STUDIO_CSS: &str = include_str!("studio.css");

/// 页面脚本（从设计稿改造：`fetch` 真实端点 + 口令进 `sessionStorage`）。
pub const STUDIO_JS: &str = include_str!("studio.js");

/// 页面入口（浏览器打开的就是它）。
pub const STUDIO_PATH: &str = "/studio";

/// 带尾斜杠的同一页（别让 `/studio/` 掉进 404）。
pub const STUDIO_INDEX_PATH: &str = "/studio/";

/// 样式表。
pub const CSS_PATH: &str = "/studio/studio.css";

/// 脚本。
pub const JS_PATH: &str = "/studio/studio.js";

/// 三个资源的 `Content-Type`（AC-004 的判据：必须带 `charset=utf-8`）。
pub const HTML_CONTENT_TYPE: &str = "text/html; charset=utf-8";
pub const CSS_CONTENT_TYPE: &str = "text/css; charset=utf-8";
pub const JS_CONTENT_TYPE: &str = "application/javascript; charset=utf-8";

/// 页面 CSP。
///
/// 与 `console::page::CSP` 的差别只有一处：**没有 `'unsafe-inline'`**（见模块头）。
/// `connect-src 'self'` 把「页面被改成打第三方接口」变成一条会当场失败的请求，
/// 也是 `/api/*` 必须与页面**同源**的技术理由。
pub const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self'; \
connect-src 'self'; img-src 'self' data:; base-uri 'none'; form-action 'none'; \
frame-ancestors 'none'";

/// 响应必须带的头。
///
/// 写成常量数组而不是就地拼 `HeaderMap`：调用点只有 [`studio_response`] 一处，
/// 「漏一个头」在 `tests` 里逐项红。
///
/// ⚠️ 第 4 个是 **`cache-control: no-cache`**，不是安全头，是**运维头**：
/// 没有它时 Cloudflare/浏览器按 `max-age=14400` 缓存 JS/CSS 四小时，
/// 于是「修了线上却还是老样子」——踩过一次，别再踩。
pub const SECURITY_HEADERS: [(&str, &str); 4] = [
    ("content-security-policy", CSP),
    ("x-content-type-options", "nosniff"),
    ("referrer-policy", "no-referrer"),
    ("cache-control", "no-cache"),
];

/// 把 [`SECURITY_HEADERS`] 变成 axum 的头部数组（类型转换只在这一处）。
pub fn security_headers() -> [(HeaderName, HeaderValue); 4] {
    [
        (
            HeaderName::from_static(SECURITY_HEADERS[0].0),
            HeaderValue::from_static(SECURITY_HEADERS[0].1),
        ),
        (
            HeaderName::from_static(SECURITY_HEADERS[1].0),
            HeaderValue::from_static(SECURITY_HEADERS[1].1),
        ),
        (
            HeaderName::from_static(SECURITY_HEADERS[2].0),
            HeaderValue::from_static(SECURITY_HEADERS[2].1),
        ),
        (
            HeaderName::from_static(SECURITY_HEADERS[3].0),
            HeaderValue::from_static(SECURITY_HEADERS[3].1),
        ),
    ]
}

/// 三条静态资源**唯一**的响应构造点（出口登记 `I32` 钉的就是本函数）。
///
/// 正文永远是 `include_str!` 的常量 —— ⛔ 这里不接受任何运行期拼出来的字符串，
/// 也就不存在「页面壳里混进一条数据」的路径（AC-008b 的结构保证，不只是 grep）。
fn studio_response(content_type: &'static str, body: &'static str) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    // 壳是编译期常量：每次都是同一份字节，但**不许**中间层缓存（发布新镜像就该换页面）。
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    for (name, value) in security_headers() {
        headers.insert(name, value);
    }
    (StatusCode::OK, headers, body).into_response()
}

/// `GET /studio` —— 页面壳（免口令、零数据）。
pub async fn index() -> Response {
    studio_response(HTML_CONTENT_TYPE, INDEX_HTML)
}

/// `GET /studio/studio.css` —— 样式表。
pub async fn css() -> Response {
    studio_response(CSS_CONTENT_TYPE, STUDIO_CSS)
}

/// `GET /studio/studio.js` —— 页面脚本。
pub async fn js() -> Response {
    studio_response(JS_CONTENT_TYPE, STUDIO_JS)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 五个**真实**类名（来自设计稿那份快照）+ 快照全局变量名。
    ///
    /// 判据是 **0 命中**：壳里只要出现一个，就说明有人把数据写死在页面里了（AC-008b）。
    const REAL_CLASS_NAMES: [&str; 5] = [
        "cvm_instance",
        "cbs_disk",
        "vpc",
        "subnet",
        "security_group",
    ];

    fn hits(hay: &str, needle: &str) -> usize {
        hay.matches(needle).count()
    }

    /// ⭐ AC-008b · 页面壳**零数据**：三个文件里五个真实类名 + `ONTOK_SNAPSHOT` 全 0。
    ///
    /// 变异（红）：往 `index.html` 里插一句 `<b>cvm_instance</b>` ⇒ 本用例红；
    /// 撤掉复绿。同一变异也能被 `query::http` 的 AC-008b 用例（打真路由）抓到。
    #[test]
    fn the_shell_carries_no_ontology_data() {
        for (name, body) in [
            ("index.html", INDEX_HTML),
            ("studio.css", STUDIO_CSS),
            ("studio.js", STUDIO_JS),
        ] {
            for needle in REAL_CLASS_NAMES.iter().copied().chain(["ONTOK_SNAPSHOT"]) {
                assert_eq!(
                    hits(body, needle),
                    0,
                    "{name} 里出现了 {needle:?} —— 页面壳必须零数据（AC-008b）"
                );
            }
        }
    }

    /// ⭐ 设计稿的离线快照**不许**进产品：它不在 `include_str!` 里，也没有 `window.` 全局。
    #[test]
    fn the_offline_snapshot_never_ships() {
        assert!(!STUDIO_JS.contains("window.ONTOK_SNAPSHOT"));
        assert!(!STUDIO_JS.contains("snapshot.js"));
    }

    /// 端口 / 路径常量与 HTML 里的引用必须**对得上**（改名不改引用 = 静态资源 404）。
    #[test]
    fn the_shell_links_the_two_assets_at_their_served_paths() {
        assert!(INDEX_HTML.contains(&format!("src=\"{JS_PATH}\"")), "缺 studio.js 的 script 标签");
        assert!(
            INDEX_HTML.contains(&format!("href=\"{CSS_PATH}\"")),
            "缺 studio.css 的 link 标签"
        );
        // 绝对路径（不是相对路径）：页面挂在 /studio（无尾斜杠）时相对路径会解析到 /studio.css。
        assert!(INDEX_HTML.contains("src=\"/studio/studio.js\""));
        assert!(INDEX_HTML.contains("href=\"/studio/studio.css\""));
    }

    /// 壳体**零外部依赖**：不引任何第三方源、不内联 `<style>` / `<script>` / `style=`。
    ///
    /// 后两条正是 CSP（无 `'unsafe-inline'`）能成立的前提：被忽略的内联样式会让页面「看起来坏了」。
    #[test]
    fn the_shell_is_self_contained() {
        for needle in ["http://", "https://", "<style", "style=\"", "@import"] {
            assert_eq!(hits(INDEX_HTML, needle), 0, "index.html 里出现了 {needle:?}");
        }
        // 仅有的两个 `src=` / `href=` 就是那两条同源资源。
        assert_eq!(hits(INDEX_HTML, "src="), 1, "index.html 只该有一个 src=");
        assert_eq!(hits(INDEX_HTML, "href="), 1, "index.html 只该有一个 href=");
    }

    /// 页面 JS **不用** `innerHTML`（与 console 同一条 XSS 纪律），也不用 `eval`。
    #[test]
    fn the_script_never_uses_innerhtml_or_eval() {
        for needle in [
            "innerHTML",
            "outerHTML",
            "insertAdjacentHTML",
            "document.write",
            "eval(",
        ] {
            assert_eq!(hits(STUDIO_JS, needle), 0, "studio.js 里出现了 {needle:?}");
        }
    }

    /// 口令**只进** `sessionStorage`：⛔ `localStorage`、⛔ URL 参数、⛔ cookie。
    #[test]
    fn the_token_only_lives_in_session_storage() {
        assert!(STUDIO_JS.contains("sessionStorage.setItem(TOKEN_KEY"));
        assert!(STUDIO_JS.contains("sessionStorage.removeItem(TOKEN_KEY)"));
        assert!(
            !STUDIO_JS.contains("localStorage"),
            "口令⛔ 进 localStorage（ADR-010-A S3）"
        );
        assert!(!STUDIO_JS.contains("document.cookie"), "口令⛔ 进 cookie");
        // 口令⛔ 进 URL：页面自己也从不把 token 拼进 query string。
        assert!(!STUDIO_JS.contains("?token=") && !STUDIO_JS.contains("&token="));
    }

    /// 页面**只**调既有端点 —— 把这条写成常量表，改一处就红。
    ///
    /// ⭐ REQ-159 P1c-2 起 `/api/projects` **是**真端点了（ADR-013：8 条写面 + 这条
    /// `GET` 挂在 `query serve` 上，Studio 调）。于是它从「⛔ 不许出现」挪到了
    /// 「✅ 必须出现」，并补上 8 条写端点的拼装尾巴 —— 少一条就红。
    #[test]
    fn the_script_only_calls_the_existing_endpoints() {
        for path in [
            "/api/health",
            "/api/catalog",
            "/api/describe",
            "/api/sparql",
            /* ⭐ REQ-159 P1c-2 · 顶栏项目下拉的数据源（GET，admin 才 200） */
            "/api/projects",
        ] {
            assert!(STUDIO_JS.contains(path), "页面没有用到 {path}");
        }
        /* 8 条写端点：`:p` 是运行时拼的，所以钉「前缀 + 尾巴」这一段拼装 */
        for tail in [
            "/bundles",
            "/import",
            "/activate",
            "/database",
            "/database/test",
            "/key",
        ] {
            assert!(
                STUDIO_JS.contains(tail),
                "页面没有拼出写端点 {tail}（ADR-013 的 8 条里少了一条）"
            );
        }
        // ⛔ 没有 /api/graph 一类的新端点（ADR-010-A：图由前端从 catalog 自己算）。
        for absent in ["/api/graph", "/api/portal"] {
            assert!(!STUDIO_JS.contains(absent), "页面引用了不存在的端点 {absent}");
        }
    }

    /// ⭐ REQ-159 P1c-2 · **AC-d8 的静态那一半**：四个管理入口在壳里就是 `disabled` 的
    /// 初始态 —— 页面**先**渲染出「存在但不可用」，admin 判定通过后才由 JS 打开。
    ///
    /// 为什么这条不能只靠 JS 保证：`index.html` 是首屏。如果按钮在 HTML 里是可点的、
    /// 只等 JS 去禁用，那么「JS 还没跑完 / 报错」那一瞬间它就是**可点**的，
    /// 而点下去会发一个必然 403 的请求。
    ///
    /// 变异（红）：把任一 `disabled` 从壳里删掉 ⇒ 本用例红。
    #[test]
    fn the_ops_entrypoints_are_disabled_in_the_shell_until_admin() {
        for id in ["proj-pick", "proj-add", "bundle-open", "key-view"] {
            let tag = INDEX_HTML
                .lines()
                .find(|l| l.contains(&format!("id=\"{id}\"")))
                .unwrap_or_else(|| panic!("壳里没有 #{id}（顶栏入口丢了？）"));
            assert!(
                tag.contains("disabled"),
                "#{id} 在壳里必须是 disabled（JS 还没判定 admin 之前不许点）：`{}`",
                tag.trim()
            );
        }
        /* 反空转：JS 里确实有「打开」的那一步（否则 disabled 永远不会解除 = 功能没做） */
        assert!(
            STUDIO_JS.contains("function applyOpsGate"),
            "applyOpsGate 不见了：按钮永远不会被打开"
        );
        assert!(
            STUDIO_JS.contains("setBtnDisabled(\"proj-add\", !on)"),
            "applyOpsGate 没有管 proj-add"
        );
    }

    /// ⭐ REQ-159 P1c-2 · **上传必须是原始字节**（AC-d4）：octet-stream + `arrayBuffer()`，
    /// ⛔ 没有 `FormData` / `multipart`（那会让服务端在 zip 那一步报一个看不懂的错）。
    #[test]
    fn the_bundle_upload_sends_raw_bytes_not_multipart() {
        assert!(
            STUDIO_JS.contains("application/octet-stream"),
            "上传没有用 octet-stream"
        );
        assert!(
            STUDIO_JS.contains("arrayBuffer"),
            "上传没有读文件的原始字节（File.arrayBuffer）"
        );
        for bad in ["FormData", "multipart/form-data"] {
            assert!(!STUDIO_JS.contains(bad), "上传里出现了 {bad}（契约要求原始字节）");
        }
    }

    /// ⭐ REQ-159 P1c-2 · **AC-d5 / d6 / d7 的三句话必须在页面上**（文案即判据）。
    ///
    /// - 解析按钮的门：`requiresDb && !dbOk` ⇒ disabled（少一个变量这道门就漏）
    /// - 重复解析：`already_imported` 走**成功**文案（「幂等成功」）
    /// - 激活二次确认：写明「会影响对外服务」
    ///
    /// 变异（红）：把 `W.requiresDb && !W.dbOk` 改成 `false` ⇒ 本用例红。
    #[test]
    fn the_irreversible_steps_carry_their_warning_texts() {
        /* ⚠️ 这里钉的是**按钮那一行的完整表达式**，不是散落的 `W.requiresDb && !W.dbOk`
           —— 后者在别处（doImport 的保险 + 提示文案）也出现，用松散的子串会让
           「门禁被去掉」照样绿（实测过一次：去掉 disabled 上的门禁，松散断言仍通过）。 */
        assert!(
            STUDIO_JS.contains("btn.disabled = !!W.importing || (W.requiresDb && !W.dbOk)"),
            "解析按钮没有再受 requires_db × 测通 的双门控（AC-d5）"
        );
        assert!(
            STUDIO_JS.contains("already_imported") && STUDIO_JS.contains("幂等成功"),
            "重复解析没有按「幂等成功」渲染（AC-d6）"
        );
        assert!(
            STUDIO_JS.contains("会影响对外服务"),
            "激活的二次确认没有写明「会影响对外服务」（AC-d7）"
        );
        assert!(
            STUDIO_JS.contains("本次查看会记录一条审计"),
            "查看 key 的固定提示丢了（每次查看都写审计）"
        );
        /* 反空转：这三句话必须真的渲染进 DOM（h(…text…) / alertBox），不是只躺在注释里 */
        assert!(
            STUDIO_JS.contains("alertBox(\"ok\""),
            "没有成功提示的渲染点（幂等成功那句话会无处可去）"
        );
        assert!(
            STUDIO_JS.contains("wiz-confirm"),
            "二次确认的容器不在（确认框可能只在注释里）"
        );
    }

    /// 三个文件都是**真内容**（空文件 / 被截断会让上面那些 grep 判据变成空转）。
    #[test]
    fn the_three_assets_are_not_empty() {
        assert!(INDEX_HTML.len() > 2_000, "index.html 太小：{}", INDEX_HTML.len());
        assert!(STUDIO_CSS.len() > 10_000, "studio.css 太小：{}", STUDIO_CSS.len());
        assert!(STUDIO_JS.len() > 20_000, "studio.js 太小：{}", STUDIO_JS.len());
        // 三份都是 UTF-8（include_str! 已经保证，这里只要它们真的含中文/样式/逻辑）。
        assert!(INDEX_HTML.contains("元方本体平台"));
        assert!(STUDIO_CSS.contains("--accent"));
        assert!(STUDIO_JS.contains("sessionStorage"));
    }

    /// CSP 里**没有** `'unsafe-inline'`：Studio 是三个文件，内联没有存在的理由（AC-011 的下限）。
    #[test]
    fn the_csp_has_no_unsafe_inline() {
        assert!(!CSP.contains("unsafe-inline"), "{CSP}");
        for needle in [
            "default-src 'self'",
            "script-src 'self'",
            "style-src 'self'",
            "connect-src 'self'",
        ] {
            assert!(CSP.contains(needle), "CSP 缺 {needle:?}：{CSP}");
        }
        assert_eq!(SECURITY_HEADERS.len(), 4);
        assert_eq!(SECURITY_HEADERS[1].1, "nosniff");
        assert_eq!(SECURITY_HEADERS[2].1, "no-referrer");
        /* 第 4 个是运维头：没有它，浏览器/CF 会把 JS/CSS 缓存 4 小时 ⇒「修了还是老样子」 */
        assert_eq!(SECURITY_HEADERS[3].0, "cache-control");
        assert_eq!(SECURITY_HEADERS[3].1, "no-cache");
    }

    /// ⭐ 回归守卫：**hover 不许改几何**。
    ///
    /// 真事故（v0.2.2 线上）：`.node-g:hover { transform: scale(1.06) }`。
    /// 节点用 `cx/cy` 定位、`<g>` 没有自己的 transform ⇒ CSS 的 `transform`
    /// 以 **SVG 原点**为中心缩放，鼠标一进节点它朝原点跳一下；跳走之后
    /// `mouseleave` 撤掉 hover 又跳回来 ⇒ 用户看到的是"节点总在躲鼠标"。
    ///
    /// 判据：`.node-g` 的任何状态里都不许出现 `transform`（位置只能靠 cx/cy 更新）。
    #[test]
    fn css_hover_must_not_change_node_geometry() {
        for (i, line) in STUDIO_CSS.lines().enumerate() {
            let l = line.trim();
            if !l.starts_with(".node-g") {
                continue;
            }
            assert!(
                !l.contains("transform"),
                "第 {} 行给 .node-g 加了 transform：`{l}`\n\
                 ⛔ 节点位置只能由 cx/cy 更新；SVG 的 CSS transform 以 viewBox 原点为基准，\
                 会让节点在 hover 时跳走（v0.2.2 的真实事故）。",
                i + 1
            );
        }
    }

    /// ⭐ 回归守卫：**SVG 里的自转动画不许用 `rotate()`**。
    ///
    /// 真事故（v0.2.2 线上）：`.node-orbit` 用 `transform: rotate(360deg)` 做自转。
    /// SVG 元素的 CSS transform 以 **viewBox 原点**为基准，`transform-box: fill-box`
    /// 一旦不生效（浏览器差异 / bbox 计算异常），虚线环就绕原点甩出一大段弧
    /// ⇒ 用户看到的"开了动效就跳得老远"。
    /// 现在改成 `stroke-dashoffset`（只改描边相位，几何一点不动）。
    #[test]
    fn orbit_animation_must_not_rotate_svg() {
        /* 只看 `orbitSpin` 这个 keyframe —— `orbSpin`（logo）是 HTML 元素，
           它对 rotate 是安全的，不能一起禁。 */
        let i = STUDIO_CSS.find("@keyframes orbitSpin").expect("orbitSpin 必须存在");
        let rest = &STUDIO_CSS[i..];
        let end = rest.find('}').map(|k| k + 1).unwrap_or(rest.len());
        let block = &rest[..end];
        assert!(
            !block.contains("rotate"),
            "orbitSpin 里又用了 rotate：`{block}`\n\
             ⛔ SVG 的 CSS transform 以 viewBox 原点为基准，偏离原点会甩出大弧\
             （线上表现为「开了动效节点跳得老远」）。请用 stroke-dashoffset。"
        );
        assert!(
            STUDIO_CSS.contains("stroke-dashoffset"),
            "orbitSpin 应该用 stroke-dashoffset 做流动"
        );
    }

    /// ⭐ 回归守卫：**边的透明度只有一个写者**（`syncEdges`）。
    ///
    /// 之前 `focusNode` 和 `syncEdges` 都写 `o.line.opacity`，互相覆盖 ⇒
    /// 「弱证据 / 关系名」这类开关点了没反应。
    #[test]
    fn edge_opacity_has_exactly_one_writer() {
        let n = STUDIO_JS.matches("o.line.setAttribute(\"opacity\"").count();
        assert_eq!(
            n, 1,
            "写 `o.line.opacity` 的地方有 {n} 处，应当只有 1 处（syncEdges）。\
             多个写者会互相覆盖，界面上就表现为「开关点了没反应」。"
        );
    }

    /// ⭐ 回归守卫：**「关系名」不被「弱证据」门控**。
    ///
    /// 本套 34 条关系全是 `unverified`；旧逻辑里 `show = labels && (weak || !isWeak)`，
    /// 于是"关系名"默认打开却一条也不显示 ⇒ 用户以为按钮坏了。
    #[test]
    fn labels_are_not_gated_by_the_weak_layer() {
        assert!(
            !STUDIO_JS.contains("(ui.layers.weak || !isWeak("),
            "关系名的可见性又被「弱证据」门控了：34 条关系全 unverified 时会一条都不显示"
        );
    }

    /// ⭐ REQ-156 · 边标签优先中文 `semantic_label`；没有标签就退回英文 key
    /// （`name` / `semantic`），⛔ 不许留白。
    #[test]
    fn relation_labels_prefer_the_chinese_semantic_label() {
        assert!(
            STUDIO_JS.contains("e.r.semantic_label"),
            "边标签没有使用 catalog 的 semantic_label"
        );
        assert!(
            STUDIO_JS.contains("|| String(e.r.name"),
            "没有语义标签时应当退回关系真名（英文 key）"
        );
        // 抽屉的关系行：标签 + 小字英文 key；未知动词只有 key。
        assert!(
            STUDIO_JS.contains("r.semantic_label"),
            "抽屉关系行没有使用 semantic_label"
        );
        assert!(
            STUDIO_JS.contains("r.semantic"),
            "抽屉关系行没有保留英文 key（无标签时要显示它）"
        );
        // 两条路都不许留白：标签 / key 都空时还要有占位。
        assert!(
            STUDIO_JS.contains("|| \"关系\""),
            "边标签在 label/name/semantic 都空时会留白"
        );
    }

    /// ⭐ 回归守卫：**CSS 不许把边的 opacity 写死**。
    ///
    /// 真事故（v0.2.2 线上）：
    ///  · `.edge-line { animation: edgeIn … both }` —— `both` 让动画结束后
    ///    opacity **永久**停在 0.55，JS 再改 `opacity` 属性都不生效
    ///    ⇒「弱证据」点了没反应。
    ///  · `.edge-label { opacity: 0 }` —— CSS 的 opacity 优先级高于 SVG 属性，
    ///    把 syncEdges 设的值整体压掉 ⇒ **关系名永远不显示**。
    ///
    /// 判据：`.edge-line` / `.edge-label` 的规则里不许出现 `opacity`，
    /// 也不许出现 `fill-mode: both` 这类会把终值锁死的动画。
    #[test]
    fn css_must_not_hardcode_edge_opacity() {
        for (i, line) in STUDIO_CSS.lines().enumerate() {
            let l = line.trim();
            let is_edge_rule = l.starts_with(".edge-line")
                || l.starts_with(".edge-label")
                || l.starts_with(".edge-label-bg");
            /* 只看**声明** `opacity:`，别把 `transition: opacity …` 也算进来 */
            if is_edge_rule && l.contains("opacity:") {
                panic!(
                    "第 {} 行给边/标签写死了 opacity：`{l}`\n\
                     ⛔ CSS 的 opacity 比 SVG 属性优先，会把 syncEdges 设的值压掉。\n\
                     透明度只能由 JS 的属性来控制。",
                    i + 1
                );
            }
        }
        /* ⚠️ 只查**声明形态**：注释里可能出现同样的文字（本文件就写过一次），
           直接 `contains("animation: edgeIn")` 会被注释绊倒。 */
        assert!(
            !STUDIO_CSS.contains(".edge-line { animation:"),
            "`.edge-line` 上不要挂入场动画（`both` 会把 opacity 锁死）"
        );
    }

    /* ══════════════════════════════════════════════════════════════
       ⭐ REQ-159 P1c-2b · 节点着色的**两种模式**（可查性 / 中心度）
       ══════════════════════════════════════════════════════════════ */

    /// 从 `studio.js` 里抠出一个函数体（含首尾大括号）。
    ///
    /// 判据只看**这个函数内部**在写什么 —— AC-e4 要的正是「可查性的图例函数里
    /// 不许出现中心度的词」这种**局部**约束；拿整份 JS 做 `contains` 会被另一段
    /// 代码满足，等于没判（REQ-144 的「判据写在两处」教训）。
    fn body_of(src: &'static str, name: &str) -> &'static str {
        let start = src
            .find(&format!("function {name}("))
            .unwrap_or_else(|| panic!("studio.js 里找不到 `function {name}(`"));
        let rest = &src[start..];
        /* 函数体在文件里以「两空格缩进 + } + 换行」收尾；嵌套闭包的缩进更深，
           不会撞上这个标记。 */
        let end = rest
            .find("\n  }\n")
            .unwrap_or_else(|| panic!("`function {name}(` 没有以两空格缩进的 }} 收尾"))
            + "\n  }\n".len();
        &rest[..end]
    }

    /// ⭐ **AC-e1** · 着色模式**只进** `sessionStorage`：⛔ URL、⛔ 长期存储。
    ///
    /// 为什么不能进 URL：URL 会进访问日志、收藏、截图、别人贴过来的链接 ——
    /// 那是一个**显示偏好**，没有理由出现在任何人的日志里（与口令同一条纪律，
    /// 只是后果轻一些）。所以这里连 `searchParams` / `pushState` 都一并禁掉。
    ///
    /// 变异（红）：把 `sessionStorage.setItem(COLOR_KEY` 改成 `location.search` ⇒ 本用例红。
    #[test]
    fn the_color_mode_lives_only_in_session_storage_and_never_in_the_url() {
        assert!(
            STUDIO_JS.contains("var COLOR_KEY = \"ontok_studio_color_mode\";"),
            "着色模式的 storage key 不见了"
        );
        assert!(
            STUDIO_JS.contains("sessionStorage.setItem(COLOR_KEY"),
            "切模式没有写 sessionStorage ⇒ 刷新后记不住（AC-e1）"
        );
        assert!(
            STUDIO_JS.contains("sessionStorage.getItem(COLOR_KEY)"),
            "启动时没有读 sessionStorage ⇒ 刷新后回不到上次的选择（AC-e1）"
        );
        for bad in ["?color=", "&color=", "searchParams", "pushState", "replaceState"] {
            assert!(
                !STUDIO_JS.contains(bad),
                "着色模式⛔ 进 URL：页面里出现了 `{bad}`"
            );
        }

        /* 壳：两项、默认是第一项「可查性」 */
        assert!(INDEX_HTML.contains("id=\"color-mode\""), "顶栏没有「着色」下拉（#color-mode）");
        let a = INDEX_HTML.find("value=\"verifiability\"").expect("缺「可查性」选项");
        let b = INDEX_HTML.find("value=\"degree\"").expect("缺「中心度」选项");
        assert!(a < b, "「可查性」必须是**第一个** option（= 默认值）");
        assert!(
            INDEX_HTML.contains(">可查性<") && INDEX_HTML.contains(">中心度<"),
            "两项的文案必须是「可查性 / 中心度」"
        );
        assert_eq!(INDEX_HTML.matches("<option").count(), 2, "着色下拉只该有两项");
    }

    /// ⭐ **AC-e3** · 四档必须用**分位数**（P50/P80/P95），⛔ 绝对阈值。
    ///
    /// 「≥5 条边算枢纽」在 500 个类上会全红、在 10 个类上会全灰；分位数保证
    /// 「不管多少类，四档都有人」。这条守卫的两半：
    /// · `degreeTier` 里的每一条比较都只能跟 `cuts.pXX` 比（出现数字 ⇒ 又回到绝对阈值）；
    /// · 分位数是「排序后取第 k 个」（最近秩法），不是插值 —— 图上没有 3.5 条边。
    ///
    /// 变异（红）：把 `deg >= cuts.p95` 改成 `deg >= 5` ⇒ 本用例红。
    #[test]
    fn the_degree_tiers_are_quantiles_not_absolute_thresholds() {
        assert!(STUDIO_JS.contains("var DEGREE_TIERS = ["), "四档颜色表（DEGREE_TIERS）不见了");
        for (label, color) in [
            ("枢纽", "#ef7a8a"),
            ("中心", "#e6b455"),
            ("一般", "#6ec8ff"),
            ("边缘", "#7d8aa3"),
        ] {
            assert!(
                STUDIO_JS.contains(&format!("label: \"{label}\", color: \"{color}\"")),
                "DEGREE_TIERS 里的「{label}」不是占位色 {color}（颜色表被改散了？）"
            );
        }
        assert!(
            STUDIO_JS.contains("var DEGREE_PCTS = { p50: 50, p80: 80, p95: 95 };"),
            "分位点常量被改了（四档 = P50/P80/P95）"
        );
        assert!(
            STUDIO_JS.contains("Math.ceil(p / 100 * n)"),
            "分位数不再是「排序后取第 ceil(p/100×n) 个」—— 换个算法边界值就对不上了"
        );

        let tier = body_of(STUDIO_JS, "degreeTier");
        let mut idx = 0usize;
        let mut seen = 0usize;
        while let Some(p) = tier[idx..].find("deg >=") {
            let at = idx + p + "deg >=".len();
            let rest = tier[at..].trim_start();
            assert!(
                rest.starts_with("cuts.p"),
                "degreeTier 里出现了**绝对阈值**（`deg >=` 后面不是分位切点）：\n{tier}"
            );
            seen += 1;
            idx = at;
        }
        assert_eq!(seen, 3, "四档的判定应当正好三条 `deg >= cuts.pXX`：\n{tier}");

        /* 颜色表集中一处：球面/泛光渐变从 DEGREE_TIERS 派生，⛔ 不再写第二份色值 */
        let defs = body_of(STUDIO_JS, "injectDefs");
        assert!(defs.contains("DEGREE_TIERS.forEach"), "四档的渐变不是从 DEGREE_TIERS 派生的");
        assert!(
            !defs.contains("#ef7a8a"),
            "injectDefs 里又写了一份枢纽色 ⇒ 颜色表不再集中一处（用户改一处不生效）"
        );
    }

    /// ⭐ **AC-e4** 的静态那一半 · 图例文案跟着模式走，⛔ 两种模式的词不许串味。
    ///
    /// 这条守卫防的是**颜色撒谎**：图例说「琥珀 = 需取数」而实际琥珀 = 枢纽。
    /// 所以判据不是「图例里有中心度的词」，而是「**可查性的图例函数里没有**中心度的词」
    /// 且「中心度的图例函数里没有**可查性**的词」——局部断言，串味必红。
    ///
    /// 动态那一半（真跑 studio.js、切模式、读图例）在
    /// `tests/fixtures/req159e/color_probe.js`。
    ///
    /// 变异（红）：让 `buildLegend` 永远走 `verifiabilityLegendRows()` ⇒ 本用例红 +
    /// 探针的 AC-e4 四条红。
    #[test]
    fn the_legend_text_follows_the_color_mode() {
        assert!(
            STUDIO_JS.contains(
                "(colorMode === \"degree\") ? degreeLegendRows() : verifiabilityLegendRows()"
            ),
            "图例不再按模式分派 ⇒ 切了模式图例不变，颜色就在撒谎（AC-e4）"
        );

        let ver = body_of(STUDIO_JS, "verifiabilityLegendRows");
        assert!(ver.contains("API 源") && ver.contains("库源"), "可查性图例少了那两句");
        for bad in ["需取数", "不可查", "枢纽", "中心度", "边缘"] {
            assert!(
                !ver.contains(bad),
                "可查性图例里出现了中心度的词「{bad}」\n{ver}"
            );
        }

        let deg = body_of(STUDIO_JS, "degreeLegendRows");
        assert!(deg.contains("中心度着色"), "中心度图例没有自己的标题（用户分不清现在看的是哪套）");
        assert!(deg.contains("大小与颜色同源"), "中心度图例没有说明「大小与颜色同源」");
        for bad in ["需取数", "不可查", "API 源", "库源"] {
            assert!(
                !deg.contains(bad),
                "中心度图例里出现了可查性的词「{bad}」⇒ 颜色在撒谎\n{deg}"
            );
        }
        /* 阈值必须**现算**并显示出来（⛔ 写死一个数字：换个本体阈值就变了） */
        assert!(deg.contains("tierRange(t, cuts)"), "中心度图例没有显示实际阈值");
        assert!(deg.contains("degreeCuts()"), "中心度图例的阈值不是现算的");
    }

    /// ⭐ **AC-e7** · 模式 A「可查性」的三态、三色、判据**逐字未变**。
    ///
    /// 模式 A 是**已验收**的语义（REQ-155/156 那套），这次只加第二种模式，
    /// ⛔ 不许"顺手优化"它。这条用例把返回值、色调表达式、三个判据一起钉住：
    /// 改任何一个字（包括色值）就红。
    ///
    /// 变异（红）：把 `"#7d8aa3"` 改成 `"#606a80"` ⇒ 本用例红。
    #[test]
    fn the_verifiability_three_states_and_colors_are_unchanged() {
        assert!(STUDIO_JS.contains(r#"key: "api", label: "需取数", tone: "warn","#));
        assert!(STUDIO_JS.contains(r#"key: "none", label: "不可查", tone: "dim","#));
        assert!(STUDIO_JS.contains(
            r#"return { key: "ok", label: "可查", tone: "ok", why: "库源，可走 SPARQL" };"#
        ));
        assert!(
            STUDIO_JS.contains(
                r##"v.key === "api" ? "#e6b455" : v.key === "ok" ? "#6ec8ff" : "#7d8aa3""##
            ),
            "模式 A 的三色映射被改了（AC-e7 钉的就是这个表达式：琥珀 / 蓝 / 灰）"
        );
        assert!(
            STUDIO_JS.contains(
                r#"grad: v.key === "api" ? "warn" : v.key === "ok" ? "ok" : "dim""#
            ),
            "模式 A 的三个渐变键被改了（glow-warn / glow-ok / glow-dim）"
        );

        let vf = body_of(STUDIO_JS, "verifiability");
        for needle in [
            "var mode = (S.health || {}).mode;",
            "if (isApi(c)) {",
            "if (mode === \"ontology-only\") {",
        ] {
            assert!(vf.contains(needle), "verifiability 的判据变了：`{needle}`");
        }
    }

    /// ⭐ **AC-e5** 的静态那一半 · 切换着色**只改属性**：⛔ 不重建图、⛔ 不重跑布局、⛔ 不刷新。
    ///
    /// 布局一跳，用户就以为页面在闪 —— 这条守的是「点了下拉，图**原地**换色」。
    /// 静态这半盯调用点（建图/布局/刷新一个都不许出现），动态那半由探针数
    /// 「g-root 新建 0 / node-g 新建 0 / reload 0 / cx 偏移 0」。
    ///
    /// 变异（红）：在 `setColorMode` 里加一句 `buildGraph()` ⇒ 本用例红。
    #[test]
    fn the_color_switch_only_repaints_existing_nodes() {
        let repaint = body_of(STUDIO_JS, "applyColorMode");
        assert!(
            !repaint.contains("buildGraph") && !repaint.contains("simulate"),
            "applyColorMode 里调了建图/布局 ⇒ 布局会跳（AC-e5）：\n{repaint}"
        );
        assert!(
            !repaint.contains("createElement") && !repaint.contains("appendChild"),
            "applyColorMode 里在造元素 ⇒ 那不是「只重绘颜色」"
        );
        assert!(!repaint.contains("reload"), "applyColorMode 里刷新了页面");
        assert!(
            repaint.contains("setAttribute(\"stroke\"") && repaint.contains("setAttribute(\"fill\""),
            "没有真的改描边/填充色"
        );

        let setmode = body_of(STUDIO_JS, "setColorMode");
        for bad in ["buildGraph", "simulate", "renderAll", "reload", "centerAt1", "applyView"] {
            assert!(
                !setmode.contains(bad),
                "setColorMode 里出现了 `{bad}`（切换会重建/跳布局）"
            );
        }
        assert!(setmode.contains("buildLegend()"), "setColorMode 没有换图例（AC-e4）");

        /* 首屏不能「先按默认色画一遍再改」= 一次肉眼可见的闪 */
        let start = body_of(STUDIO_JS, "start");
        let read_at = start.find("colorMode = readColorMode();").expect("start() 没有先读模式");
        let boot_at = start.find("boot();").expect("start() 里没有 boot()");
        assert!(read_at < boot_at, "先画图后读模式 ⇒ 首屏会闪一下");
    }
}

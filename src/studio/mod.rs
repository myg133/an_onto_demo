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

/// 三个响应必须带的头（与 console 同一组）。
///
/// 写成常量数组而不是就地拼 `HeaderMap`：调用点只有 [`studio_response`] 一处，
/// 「漏一个头」在 `tests` 里逐项红。
pub const SECURITY_HEADERS: [(&str, &str); 3] = [
    ("content-security-policy", CSP),
    ("x-content-type-options", "nosniff"),
    ("referrer-policy", "no-referrer"),
];

/// 把 [`SECURITY_HEADERS`] 变成 axum 的头部数组（类型转换只在这一处）。
pub fn security_headers() -> [(HeaderName, HeaderValue); 3] {
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

    /// 页面**只**调既有端点（零新增接口）——把这条写成常量表，改一处就红。
    #[test]
    fn the_script_only_calls_the_existing_endpoints() {
        for path in [
            "/api/health",
            "/api/catalog",
            "/api/describe",
            "/api/sparql",
        ] {
            assert!(STUDIO_JS.contains(path), "页面没有用到 {path}");
        }
        // ⛔ 没有 /api/graph 一类的新端点（ADR-010-A：图由前端从 catalog 自己算）。
        for absent in ["/api/graph", "/api/portal", "/api/projects"] {
            assert!(!STUDIO_JS.contains(absent), "页面引用了不存在的端点 {absent}");
        }
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
        assert_eq!(SECURITY_HEADERS.len(), 3);
        assert_eq!(SECURITY_HEADERS[1].1, "nosniff");
        assert_eq!(SECURITY_HEADERS[2].1, "no-referrer");
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
}

//! REQ-135 · **生产可观测性** —— 手写 Prometheus 文本格式（`/metrics`）。
//!
//! # 三件事，各自只有一个写点
//!
//! - **指标清单**：[`METRICS`] 是**唯一**的一张静态表（名字 / 类型 / HELP / 标签名）。
//!   渲染、校验、测试都从这里出发 —— 不在别处再抄一份名字；
//! - **取值**：[`Metrics`] 是每个服务进程**私有的**注册表（一个 `Mutex<BTreeMap<_>>`），
//!   服务在 `new()` 里建它 ⇒ 测试各自一份，断言不必和并行用例抢全局状态；
//! - **响应体**：[`Metrics::http_response`] 是 `/metrics` 的**唯一**响应构造点
//!   （三个服务共用；`Content-Type` 也在这里）。
//!
//! # 为什么手写、不引 prometheus crate
//!
//! 需求 R2 明写「⛔ 不引依赖」。本模块只用到 `std` + 已在依赖树里的 `axum`：
//! 六个指标、十来个标签、一个直方图，手写文本比多一个 crate 更容易被审计。
//!
//! # 零业务数据（R1 / R4）
//!
//! 标签值全部是 `&'static str`：`service` / `endpoint`（路由**模式**，不是真实路径）/
//! `code`（HTTP 状态码） / `reason`（代码里写死的拒绝原因） / `view`（`customer|model`）。
//! 类型层就**传不进**租户 id、表名、SQL 或查询原文 —— 那些都是运行时 `String`。
//! 端点标签用 axum 的 [`MatchedPath`]（`/q/:id` 这类**模式**），不是原始 URL。
//!
//! # 已知限制
//!
//! - 计数器 / 直方图是**进程内**的：重启归零（Prometheus 的 `rate()` 本来就是增量语义）；
//! - `code` 只保留有限集合（见 [`status_label`]），其它状态归到 `other` —— 维度不爆炸；
//! - `ontok_rows_returned_total` 只数**真的返回给调用方**的行（截断前）：
//!   已被 `max_rows` 截断的语义由回执的 `truncated` 表达，这里不重复表达。

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};

/// Prometheus 文本格式的端点路径。
pub const METRICS_PATH: &str = "/metrics";

/// Prometheus 0.0.4 的 `Content-Type`（AC-001 逐字要求）。
pub const CONTENT_TYPE: &str = "text/plain; version=0.0.4";

/// 三个服务在 `service` 标签里的名字（与 `/api/health` 的 `service` 字段同源）。
pub const SERVICE_QUERY: &str = "ontok-query";
pub const SERVICE_CONSOLE: &str = "ontok-console";
pub const SERVICE_PORTAL: &str = "ontok-portal";

/// 租户拒绝的四个原因（标签值的**全集**，⛔ 不接受运行时字符串）。
pub const REASON_TENANT_MISMATCH: &str = "TENANT_MISMATCH";
pub const REASON_TENANT_UNRESOLVED: &str = "TENANT_UNRESOLVED";
pub const REASON_TOKEN_KIND_FORBIDDEN: &str = "TOKEN_KIND_FORBIDDEN";
pub const REASON_TENANT_FROM_CLIENT: &str = "TENANT_FROM_CLIENT";

/// TTL 视图标签值。
pub const VIEW_CUSTOMER: &str = "customer";
pub const VIEW_MODEL: &str = "model";

/// 直方图的桶（秒）。Prometheus 的 `_bucket` 是**累积**计数，末尾必有一条 `+Inf`。
const BUCKETS: &[f64] = &[
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
];

// ══════════════════════════════════════════════════════════════
// 唯一的指标登记表（纪律 H）
// ══════════════════════════════════════════════════════════════

/// 指标类型（只认 Prometheus 的三种，够用且最小）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Counter,
    Gauge,
    Histogram,
}

impl Kind {
    /// `# TYPE` 行里的那个词。
    pub const fn as_str(self) -> &'static str {
        match self {
            Kind::Counter => "counter",
            Kind::Gauge => "gauge",
            Kind::Histogram => "histogram",
        }
    }
}

/// 一条指标的登记项。**字段全是 `&'static str`** —— 指标清单是常量，不是运行时数据。
#[derive(Debug, Clone, Copy)]
pub struct MetricSpec {
    pub name: &'static str,
    pub kind: Kind,
    pub help: &'static str,
    /// 标签名，**顺序即输出顺序**。
    pub labels: &'static [&'static str],
}

/// ⭐ 六个指标（需求 §2 的最小可用集）。新增指标 = 在这里加一行，
/// 渲染 / 校验 / 测试会**自动**把它纳入（少写一处就会在 `the_six_metrics_are_exactly_this_set` 红）。
pub const METRICS: &[MetricSpec] = &[
    MetricSpec {
        name: "ontok_requests_total",
        kind: Kind::Counter,
        help: "HTTP requests handled, by service, endpoint and status code.",
        labels: &["service", "endpoint", "code"],
    },
    MetricSpec {
        name: "ontok_request_duration_seconds",
        kind: Kind::Histogram,
        help: "HTTP request latency in seconds, by service and endpoint.",
        labels: &["service", "endpoint"],
    },
    MetricSpec {
        name: "ontok_rows_returned_total",
        kind: Kind::Counter,
        help: "Result rows returned to callers, by service.",
        labels: &["service"],
    },
    MetricSpec {
        name: "ontok_tenant_denied_total",
        kind: Kind::Counter,
        help: "Cross-tenant or wrong-token-kind rejections, by service and reason.",
        labels: &["service", "reason"],
    },
    MetricSpec {
        name: "ontok_ttl_bytes",
        kind: Kind::Gauge,
        help: "Size in bytes of the served ontology TTL document, by view.",
        labels: &["view"],
    },
    MetricSpec {
        name: "ontok_mapping_joins",
        kind: Kind::Gauge,
        help: "Number of R2RML join conditions in the loaded mapping.",
        labels: &[],
    },
];

// ══════════════════════════════════════════════════════════════
// 固定的小集合（端点 / 状态码 / 拒绝原因）
// ══════════════════════════════════════════════════════════════

/// 查询服务的**路由模式**（不是真实路径；`MatchedPath` 给的就是模式）。
pub const QUERY_ENDPOINTS: &[&str] = &[
    "/api/health",
    "/api/sparql",
    "/api/catalog",
    "/api/describe",
    "/api/ontology.ttl",
    "/mcp",
    METRICS_PATH,
];

/// 控制面的路由模式。
pub const CONSOLE_ENDPOINTS: &[&str] = &[
    "/",
    "/api/health",
    "/api/projects",
    "/api/import",
    "/api/apply",
    "/api/freeze",
    "/api/export/ttl",
    "/api/r2rml",
    "/api/config/:kind",
    "/api/audit",
    "/api/review",
    "/api/decide",
    METRICS_PATH,
];

/// 门户的路由模式。
pub const PORTAL_ENDPOINTS: &[&str] = &[
    "/",
    "/api/health",
    "/api/questions",
    "/q/:id",
    METRICS_PATH,
];

/// 查询服务会产的拒绝原因。
pub const QUERY_DENIED_REASONS: &[&str] = &[REASON_TENANT_MISMATCH, REASON_TENANT_UNRESOLVED];
/// 控制面会产的拒绝原因。
pub const CONSOLE_DENIED_REASONS: &[&str] = &[REASON_TOKEN_KIND_FORBIDDEN];
/// 门户会产的拒绝原因。
pub const PORTAL_DENIED_REASONS: &[&str] = &[REASON_TENANT_FROM_CLIENT];

/// 路径 → **固定的**端点标签值（路由模式的等价物）。
///
/// 为什么不用 axum 的 `MatchedPath`：它要打开 `matched-path` feature，
/// 而本需求的纪律是 **`Cargo.toml` 零改动**（AC-008）。这里用**同一张静态表**
/// 做匹配：已知路由原样返回，带路径参数的两条（`/q/<id>` / `/api/config/<kind>`）
/// 只认**单段**并折叠成模式；其余一律 `other`。
///
/// ⛔ 绝不把原始 URL 当标签：`/q/abc` 与 `/q/def` 会各自长出一条时间序列
/// （维度爆炸 + 业务 id 泄漏）。未知路径（含探测 `/wp-admin`）都只落一行 `other`。
pub fn endpoint_label(path: &str) -> &'static str {
    // 先把 query string / fragment 切掉（它们不参与路由，也不该进标签）。
    let p = path.split(['?', '#']).next().unwrap_or("");
    match p {
        "/" => "/",
        "/api/health" => "/api/health",
        "/api/sparql" => "/api/sparql",
        "/api/catalog" => "/api/catalog",
        "/api/describe" => "/api/describe",
        "/api/ontology.ttl" => "/api/ontology.ttl",
        "/mcp" => "/mcp",
        "/api/projects" => "/api/projects",
        "/api/import" => "/api/import",
        "/api/apply" => "/api/apply",
        "/api/freeze" => "/api/freeze",
        "/api/export/ttl" => "/api/export/ttl",
        "/api/r2rml" => "/api/r2rml",
        "/api/audit" => "/api/audit",
        "/api/review" => "/api/review",
        "/api/decide" => "/api/decide",
        "/api/questions" => "/api/questions",
        METRICS_PATH => METRICS_PATH,
        other => {
            if single_segment_after(other, "/q/") {
                "/q/:id"
            } else if single_segment_after(other, "/api/config/") {
                "/api/config/:kind"
            } else {
                "other"
            }
        }
    }
}

/// `path` 是不是 `prefix` 后面**恰好一段**（`/q/abc` 是，`/q/a/b` 与 `/q/` 不是）。
fn single_segment_after(path: &str, prefix: &str) -> bool {
    let Some(rest) = path.strip_prefix(prefix) else {
        return false;
    };
    !rest.is_empty() && !rest.contains('/')
}

/// HTTP 状态码 → **有限集合**的 `code` 标签。
///
/// 只保留本 crate 会产生的那些（`ApiError` 的 status 全集 + 405 + 默认 200）；
/// 其余一律 `other` —— 不把任意状态码变成新维度。
pub fn status_label(status: u16) -> &'static str {
    match status {
        200 => "200",
        201 => "201",
        202 => "202",
        204 => "204",
        304 => "304",
        400 => "400",
        401 => "401",
        403 => "403",
        404 => "404",
        405 => "405",
        409 => "409",
        422 => "422",
        429 => "429",
        500 => "500",
        502 => "502",
        503 => "503",
        504 => "504",
        _ => "other",
    }
}

/// 错误码 → 拒绝原因（**唯一**的映射；只认四个安全码，别的一律 `None`）。
///
/// 为什么用错误码而不在调用点各写一遍：`TENANT_UNRESOLVED` 是查询面在
/// `ApiError::from_query_error` 里造出来的，调用点拿不到「为什么」，
/// 只剩 `code` 这个稳定事实 —— 在**一个**地方翻译，别处不会再漂。
pub fn denied_reason(code: &str) -> Option<&'static str> {
    match code {
        REASON_TENANT_MISMATCH => Some(REASON_TENANT_MISMATCH),
        REASON_TENANT_UNRESOLVED => Some(REASON_TENANT_UNRESOLVED),
        REASON_TOKEN_KIND_FORBIDDEN => Some(REASON_TOKEN_KIND_FORBIDDEN),
        REASON_TENANT_FROM_CLIENT => Some(REASON_TENANT_FROM_CLIENT),
        _ => None,
    }
}

// ══════════════════════════════════════════════════════════════
// 注册表
// ══════════════════════════════════════════════════════════════

/// 一条时间序列的键：`(指标名, 有序标签值)`。
type Key = (&'static str, Vec<(&'static str, &'static str)>);

#[derive(Debug, Clone, Default)]
struct Histogram {
    /// **累积**计数（与 `_bucket` 的语义一致）。
    buckets: Vec<u64>,
    sum: f64,
    count: u64,
}

impl Histogram {
    fn new() -> Self {
        Self {
            buckets: vec![0; BUCKETS.len()],
            sum: 0.0,
            count: 0,
        }
    }
}

#[derive(Debug, Clone)]
enum Sample {
    Counter(u64),
    Gauge(u64),
    Histogram(Histogram),
}

#[derive(Debug, Default)]
struct Inner {
    series: Mutex<BTreeMap<Key, Sample>>,
}

/// 一个服务进程的指标注册表（`Arc` 内部共享；`clone()` 只是加一个引用）。
#[derive(Clone)]
pub struct Metrics {
    service: &'static str,
    inner: Arc<Inner>,
}

impl std::fmt::Debug for Metrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Metrics")
            .field("service", &self.service)
            .finish()
    }
}

impl Metrics {
    /// 通用构造点：登记 `service`，并为**已知**端点 / 拒绝原因 / 视图各建一条 0 值序列。
    ///
    /// 为什么要预置 0 值：Prometheus 的 `rate()` / `increase()` 需要序列**在事件发生前**
    /// 就存在（否则第一次事件会被当成「从 0 开始」，面板上出现假的尖峰）。
    /// 预置的标签值全在 [`METRICS`] / [`QUERY_ENDPOINTS`] 这类**静态表**里 ⇒ 维度不涨。
    pub fn new(
        service: &'static str,
        denied_reasons: &'static [&'static str],
        endpoints: &'static [&'static str],
    ) -> Self {
        let this = Self {
            service,
            inner: Arc::new(Inner::default()),
        };
        // 计数器：返回行数 0（每个服务一条）。
        this.add(
            "ontok_rows_returned_total",
            vec![("service", this.service)],
            0,
        );
        // 计数器：拒绝原因（只预置本服务**真的会产**的那几条）。
        for reason in denied_reasons {
            this.add(
                "ontok_tenant_denied_total",
                vec![("service", this.service), ("reason", reason)],
                0,
            );
        }
        // 直方图：每个已知端点一条空直方图（`_bucket` / `_sum` / `_count` 从 0 起）。
        for endpoint in endpoints {
            this.seed_histogram(endpoint);
        }
        // 仪表：TTL 字节（两视图）与映射 join 数。
        for view in [VIEW_CUSTOMER, VIEW_MODEL] {
            this.set("ontok_ttl_bytes", vec![("view", view)], 0);
        }
        this.set("ontok_mapping_joins", Vec::new(), 0);
        this
    }

    /// 查询服务（`ontok query serve`）。
    pub fn for_query() -> Self {
        Self::new(SERVICE_QUERY, QUERY_DENIED_REASONS, QUERY_ENDPOINTS)
    }

    /// 控制面（`ontok console serve`）。
    pub fn for_console() -> Self {
        Self::new(SERVICE_CONSOLE, CONSOLE_DENIED_REASONS, CONSOLE_ENDPOINTS)
    }

    /// 客户门户（`ontok portal serve`）。
    pub fn for_portal() -> Self {
        Self::new(SERVICE_PORTAL, PORTAL_DENIED_REASONS, PORTAL_ENDPOINTS)
    }

    /// 本服务在 `service` 标签里的名字。
    pub fn service(&self) -> &'static str {
        self.service
    }

    // ── 放宽的取锁（毒化不是失败原因：指标不该让一次请求变 500）──
    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<Key, Sample>> {
        self.inner
            .series
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    fn add(&self, metric: &'static str, labels: Vec<(&'static str, &'static str)>, delta: u64) {
        let mut map = self.lock();
        match map
            .entry((metric, labels))
            .or_insert(Sample::Counter(0))
        {
            Sample::Counter(v) => *v = v.saturating_add(delta),
            other => *other = Sample::Counter(delta),
        }
    }

    fn set(&self, metric: &'static str, labels: Vec<(&'static str, &'static str)>, value: u64) {
        let mut map = self.lock();
        map.insert((metric, labels), Sample::Gauge(value));
    }

    /// 预置一条**空**直方图（全 0；`_count` 也是 0）。
    fn seed_histogram(&self, endpoint: &'static str) {
        let labels = vec![("service", self.service), ("endpoint", endpoint)];
        self.lock()
            .insert(("ontok_request_duration_seconds", labels), Sample::Histogram(Histogram::new()));
    }

    fn observe_seconds(&self, endpoint: &'static str, seconds: f64) {
        let labels = vec![("service", self.service), ("endpoint", endpoint)];
        let mut map = self.lock();
        let entry = map
            .entry(("ontok_request_duration_seconds", labels))
            .or_insert_with(|| Sample::Histogram(Histogram::new()));
        let Sample::Histogram(h) = entry else {
            return;
        };
        h.buckets.resize(BUCKETS.len(), 0);
        for (i, upper) in BUCKETS.iter().enumerate() {
            if seconds <= *upper {
                h.buckets[i] += 1;
            }
        }
        h.sum += seconds;
        h.count += 1;
    }

    /// 记一次 HTTP 请求：`requests_total{service,endpoint,code}` +1，并观察耗时。
    pub fn observe_request(&self, endpoint: &'static str, code: &'static str, elapsed: Duration) {
        self.add(
            "ontok_requests_total",
            vec![
                ("service", self.service),
                ("endpoint", endpoint),
                ("code", code),
            ],
            1,
        );
        self.observe_seconds(endpoint, elapsed.as_secs_f64());
    }

    /// 记返回给调用方的行数（查询 / 门户各在**成功分支**调一次）。
    pub fn rows(&self, rows: u64) {
        self.add(
            "ontok_rows_returned_total",
            vec![("service", self.service)],
            rows,
        );
    }

    /// 记一次租户拒绝（`reason` 必须是 [`denied_reason`] 认得的常量）。
    pub fn tenant_denied(&self, reason: &'static str) {
        self.add(
            "ontok_tenant_denied_total",
            vec![("service", self.service), ("reason", reason)],
            1,
        );
    }

    /// 按错误码记拒绝（不认得的码不计数 —— ⛔ 不把任意错误变成安全信号）。
    pub fn note_denial_code(&self, code: &str) {
        if let Some(reason) = denied_reason(code) {
            self.tenant_denied(reason);
        }
    }

    /// 设置 TTL 字节数（启动时 / 导出成功时各调一次）。
    pub fn set_ttl_bytes(&self, view: &'static str, bytes: u64) {
        self.set("ontok_ttl_bytes", vec![("view", view)], bytes);
    }

    /// 设置映射 join 数（R2RML 的 `joinCondition` 条数）。
    pub fn set_mapping_joins(&self, joins: u64) {
        self.set("ontok_mapping_joins", Vec::new(), joins);
    }

    // ══════════════════════════════════════════════════════════
    // 渲染（Prometheus 文本格式 0.0.4）
    // ══════════════════════════════════════════════════════════

    /// 渲染整份 exposition 文本。
    ///
    /// 顺序：按 [`METRICS`] 的登记顺序；同一指标内按 `(指标名, 标签值)` 字典序 ——
    /// 于是同样的状态**逐字节相同**（判据可以整段对拍）。
    pub fn render(&self) -> String {
        let map = self.lock();
        let mut out = String::new();
        for spec in METRICS {
            out.push_str("# HELP ");
            out.push_str(spec.name);
            out.push(' ');
            out.push_str(spec.help);
            out.push('\n');
            out.push_str("# TYPE ");
            out.push_str(spec.name);
            out.push(' ');
            out.push_str(spec.kind.as_str());
            out.push('\n');
            for (key, sample) in map.iter().filter(|(k, _)| k.0 == spec.name) {
                match (spec.kind, sample) {
                    (Kind::Counter, Sample::Counter(v)) | (Kind::Gauge, Sample::Gauge(v)) => {
                        write_sample(&mut out, spec.name, &key.1, &v.to_string());
                    }
                    (Kind::Gauge, Sample::Counter(v)) | (Kind::Counter, Sample::Gauge(v)) => {
                        // 类型对不上（只可能来自写错的调用点）：按值写出去，别 panic。
                        write_sample(&mut out, spec.name, &key.1, &v.to_string());
                    }
                    (Kind::Histogram, Sample::Histogram(h)) => {
                        for (i, upper) in BUCKETS.iter().enumerate() {
                            let mut labels = key.1.clone();
                            let le = bucket_label(*upper);
                            labels.push(("le", le.as_str()));
                            write_sample(
                                &mut out,
                                &format!("{}_bucket", spec.name),
                                &labels,
                                &h.buckets.get(i).copied().unwrap_or(0).to_string(),
                            );
                        }
                        let mut labels = key.1.clone();
                        labels.push(("le", "+Inf"));
                        write_sample(
                            &mut out,
                            &format!("{}_bucket", spec.name),
                            &labels,
                            &h.count.to_string(),
                        );
                        write_sample(
                            &mut out,
                            &format!("{}_sum", spec.name),
                            &key.1,
                            &float_label(h.sum),
                        );
                        write_sample(
                            &mut out,
                            &format!("{}_count", spec.name),
                            &key.1,
                            &h.count.to_string(),
                        );
                    }
                    _ => {}
                }
            }
        }
        out
    }

    /// `/metrics` 的**唯一**响应构造点（三个服务共用）。
    pub fn http_response(&self) -> Response {
        (
            StatusCode::OK,
            [(header::CONTENT_TYPE, HeaderValue::from_static(CONTENT_TYPE))],
            self.render(),
        )
            .into_response()
    }
}

/// 桶上界 → `le` 标签值（`0.005` / `1` / `10`；与 Prometheus 的惯例一致）。
fn bucket_label(upper: f64) -> String {
    if upper.fract() == 0.0 {
        format!("{}", upper as u64)
    } else {
        format!("{upper}")
    }
}

/// 浮点值 → 文本（Rust 的默认格式已是合法 Prometheus float；`0` 不写成 `0.0`）。
fn float_label(v: f64) -> String {
    if v == 0.0 {
        "0".to_string()
    } else {
        format!("{v}")
    }
}

/// 一条样本行：`name{k="v",...} value`。
fn write_sample(out: &mut String, name: &str, labels: &[(&str, &str)], value: &str) {
    out.push_str(name);
    if !labels.is_empty() {
        out.push('{');
        for (i, (k, v)) in labels.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(k);
            out.push_str("=\"");
            escape_label_into(out, v);
            out.push('"');
        }
        out.push('}');
    }
    out.push(' ');
    out.push_str(value);
    out.push('\n');
}

/// 标签值转义（Prometheus：`\` → `\\`、`"` → `\"`、换行 → `\n`）。
///
/// 我们的标签值全是静态 ASCII 短语，转义只是**纵深防御**：将来有人不小心
/// 把一段运行时字符串塞进来，也不会让整份 exposition 解析失败。
fn escape_label_into(out: &mut String, value: &str) {
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            other => out.push(other),
        }
    }
}

// ══════════════════════════════════════════════════════════════
// 中间件：每个请求一行计数 + 一次耗时观察
// ══════════════════════════════════════════════════════════════

/// 整张路由的最外层中间件（三个服务各挂一次）。
///
/// 端点取自 [`endpoint_label`]（**路由模式**，如 `/q/:id`），不是原始 URL；
/// 未知路径 ⇒ `other`。状态码经 [`status_label`] 收口。
pub async fn track(
    axum::extract::State(metrics): axum::extract::State<Metrics>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let endpoint = endpoint_label(req.uri().path());
    let started = Instant::now();
    let res = next.run(req).await;
    metrics.observe_request(endpoint, status_label(res.status().as_u16()), started.elapsed());
    res
}

// ══════════════════════════════════════════════════════════════
// 格式校验器（AC-002：没有 promtool 也不许跳过）
// ══════════════════════════════════════════════════════════════

/// 逐条校验一段 Prometheus exposition 文本。
///
/// 判据（与 AC-002 一一对应）：
/// 1. `# HELP` / `# TYPE` 各指标**恰好一条**，且名字都在 [`METRICS`] 里；
/// 2. 每个样本行形如 `name{label="v",...} value`，值是可解析的有限浮点；
/// 3. 样本的标签名与该指标的登记标签**逐字相等**（直方图的 `_bucket` 多一个 `le`）；
/// 4. 直方图三件套齐全：`_bucket`（以 `+Inf` 收尾、累积不减）/ `_sum` / `_count`，
///    且 `+Inf` 桶 == `_count`；
/// 5. 同一 `(name, labels)` 不许重复出现（重复 = 同一时刻两个值，解析器会拒）。
///
/// 返回**全部**错误（不是第一个）：一次就能看清坏了几处。
pub fn validate_exposition(text: &str) -> Result<(), Vec<String>> {
    let mut errs: Vec<String> = Vec::new();
    let mut type_seen: BTreeMap<String, &'static str> = BTreeMap::new();
    let mut help_seen: BTreeMap<String, usize> = BTreeMap::new();
    // (name, labels) → (行号, 取值)；行号用来查重，取值用来做直方图对账。
    let mut samples: BTreeMap<(String, Vec<(String, String)>), (usize, f64)> = BTreeMap::new();

    // 样本 → 所属指标（长名优先：`ontok_x_bucket` 先试 `ontok_x`）。
    let spec_of_sample = |name: &str| -> Option<&'static MetricSpec> {
        METRICS.iter().find(|s| {
            name == s.name
                || matches!(s.kind, Kind::Histogram)
                    && (name == format!("{}_bucket", s.name)
                        || name == format!("{}_sum", s.name)
                        || name == format!("{}_count", s.name))
        })
    };

    for (idx, raw) in text.lines().enumerate() {
        let n = idx + 1;
        if raw.is_empty() {
            continue;
        }
        if let Some(rest) = raw.strip_prefix("# HELP ") {
            let Some((name, help)) = rest.split_once(' ') else {
                errs.push(format!("第 {n} 行：# HELP 缺说明文本"));
                continue;
            };
            if METRICS.iter().all(|s| s.name != name) {
                errs.push(format!("第 {n} 行：# HELP 的名字 {name:?} 不在登记表里"));
            }
            if help.is_empty() {
                errs.push(format!("第 {n} 行：# HELP {name} 的说明是空的"));
            }
            *help_seen.entry(name.to_string()).or_insert(0) += 1;
            continue;
        }
        if let Some(rest) = raw.strip_prefix("# TYPE ") {
            let Some((name, kind)) = rest.split_once(' ') else {
                errs.push(format!("第 {n} 行：# TYPE 缺类型"));
                continue;
            };
            let Some(spec) = METRICS.iter().find(|s| s.name == name) else {
                errs.push(format!("第 {n} 行：# TYPE 的名字 {name:?} 不在登记表里"));
                continue;
            };
            if kind != spec.kind.as_str() {
                errs.push(format!(
                    "第 {n} 行：{name} 的类型是 {kind:?}，登记表要求 {:?}",
                    spec.kind.as_str()
                ));
            }
            if type_seen.insert(name.to_string(), spec.kind.as_str()).is_some() {
                errs.push(format!("第 {n} 行：{name} 出现了第二条 # TYPE"));
            }
            continue;
        }
        if raw.starts_with('#') {
            errs.push(format!("第 {n} 行：不认识的注释行 {raw:?}"));
            continue;
        }

        // ── 样本行 ──
        let Some((head, value)) = raw.rsplit_once(' ') else {
            errs.push(format!("第 {n} 行：样本行缺取值：{raw:?}"));
            continue;
        };
        let Ok(v) = value.parse::<f64>() else {
            errs.push(format!("第 {n} 行：取值 {value:?} 不是数字"));
            continue;
        };
        if !v.is_finite() {
            errs.push(format!("第 {n} 行：取值 {value:?} 不是有限数"));
        }
        let (name, labels) = match head.split_once('{') {
            None => (head, Vec::new()),
            Some((name, rest)) => {
                let Some(body) = rest.strip_suffix('}') else {
                    errs.push(format!("第 {n} 行：标签块没有闭合：{raw:?}"));
                    continue;
                };
                match parse_labels(body) {
                    Ok(l) => (name, l),
                    Err(e) => {
                        errs.push(format!("第 {n} 行：{e}"));
                        continue;
                    }
                }
            }
        };
        let Some(spec) = spec_of_sample(name) else {
            errs.push(format!("第 {n} 行：样本 {name:?} 不属于任何已登记指标"));
            continue;
        };
        // 该指标必须已经声明过 TYPE（否则解析器不知道它是 counter 还是 histogram）。
        if !type_seen.contains_key(spec.name) {
            errs.push(format!(
                "第 {n} 行：样本 {name:?} 出现在 # TYPE {} 之前",
                spec.name
            ));
        }
        // 标签名逐字对账（直方图 `_bucket` 多一个 `le`，且 `le` 只许在那里出现）。
        let mut want: Vec<&str> = spec.labels.to_vec();
        let is_bucket = name.strip_suffix("_bucket") == Some(spec.name);
        if is_bucket {
            want.push("le");
        }
        if spec.kind == Kind::Histogram && name != spec.name && !is_bucket
            && name != format!("{}_sum", spec.name)
            && name != format!("{}_count", spec.name)
        {
            errs.push(format!("第 {n} 行：直方图样本名 {name:?} 不合法"));
        }
        if spec.kind != Kind::Histogram && name != spec.name {
            errs.push(format!("第 {n} 行：{name:?} 不该带后缀"));
        }
        let got: Vec<&str> = labels.iter().map(|(k, _)| k.as_str()).collect();
        if got != want {
            errs.push(format!(
                "第 {n} 行：{name} 的标签 {got:?} 与登记 {want:?} 不一致"
            ));
        }
        if labels.iter().any(|(_, v)| v.is_empty() || v.contains('\n')) {
            errs.push(format!("第 {n} 行：{name} 有空标签值或换行"));
        }
        if samples
            .insert((name.to_string(), labels.clone()), (n, v))
            .is_some()
        {
            errs.push(format!(
                "第 {n} 行：{name} 的同一组标签出现了两次（同一时刻两个值）"
            ));
        }
    }

    // 每个登记指标：HELP / TYPE 恰好一条（缺一条 = 解析器不知道该按什么类型读）。
    for spec in METRICS {
        if type_seen.get(spec.name).is_none() {
            errs.push(format!("{}：缺 # TYPE 行", spec.name));
        }
        match help_seen.get(spec.name).copied().unwrap_or(0) {
            1 => {}
            0 => errs.push(format!("{}：缺 # HELP 行", spec.name)),
            k => errs.push(format!("{}：出现了 {k} 条 # HELP（只许一条）", spec.name)),
        }
    }

    // 直方图三件套：按「去掉 le 的标签组」聚合，逐组对账。
    for spec in METRICS.iter().filter(|s| s.kind == Kind::Histogram) {
        let bucket_name = format!("{}_bucket", spec.name);
        let sum_name = format!("{}_sum", spec.name);
        let count_name = format!("{}_count", spec.name);
        let mut buckets_by_series: BTreeMap<Vec<(String, String)>, Vec<(String, f64)>> =
            BTreeMap::new();
        let mut sums: BTreeMap<Vec<(String, String)>, f64> = BTreeMap::new();
        let mut counts: BTreeMap<Vec<(String, String)>, f64> = BTreeMap::new();
        for ((name, labels), (_, value)) in samples.iter() {
            if name == &bucket_name {
                let Some(le) = labels.iter().find(|(k, _)| k == "le").map(|(_, v)| v.clone()) else {
                    continue; // 缺 le 的 bucket 已由标签对账报过
                };
                let rest: Vec<(String, String)> = labels
                    .iter()
                    .filter(|(k, _)| k != "le")
                    .cloned()
                    .collect();
                buckets_by_series
                    .entry(rest)
                    .or_default()
                    .push((le, *value));
            } else if name == &sum_name {
                sums.insert(labels.clone(), *value);
            } else if name == &count_name {
                counts.insert(labels.clone(), *value);
            }
        }
        for (labels, mut buckets) in buckets_by_series.clone() {
            if buckets.len() != BUCKETS.len() + 1 {
                errs.push(format!(
                    "{}：标签 {labels:?} 的桶数 {} != {}（含 +Inf）",
                    spec.name,
                    buckets.len(),
                    BUCKETS.len() + 1
                ));
            }
            buckets.sort_by(|a, b| {
                parse_bound(&a.0)
                    .partial_cmp(&parse_bound(&b.0))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            if buckets.last().map(|(le, _)| le.as_str()) != Some("+Inf") {
                errs.push(format!("{}：标签 {labels:?} 的最后一个桶不是 +Inf", spec.name));
            }
            let mut prev = 0.0;
            for (le, v) in &buckets {
                if *v < prev {
                    errs.push(format!(
                        "{}：标签 {labels:?} 的桶 le={le} 比上一个桶小（累积计数必须不减）",
                        spec.name
                    ));
                }
                prev = *v;
            }
            let inf = buckets
                .iter()
                .find(|(le, _)| le == "+Inf")
                .map(|(_, v)| *v);
            match (inf, counts.get(&labels).copied()) {
                (Some(inf), Some(count)) if inf != count => errs.push(format!(
                    "{}：+Inf 桶 {inf} 与 _count {count} 不相等",
                    spec.name
                )),
                (_, None) => errs.push(format!("{}：标签 {labels:?} 缺 _count 行", spec.name)),
                _ => {}
            }
        }
        // 每组都要有 _sum 与 _count（不能只有桶）。
        for labels in buckets_by_series.keys() {
            if !sums.contains_key(labels) {
                errs.push(format!("{}：标签 {labels:?} 缺 _sum 行", spec.name));
            }
            if !counts.contains_key(labels) {
                errs.push(format!("{}：标签 {labels:?} 缺 _count 行", spec.name));
            }
        }
        for labels in sums.keys() {
            if !counts.contains_key(labels) {
                errs.push(format!("{}：标签 {labels:?} 有 _sum 却没有 _count", spec.name));
            }
        }
        for labels in counts.keys() {
            if !sums.contains_key(labels) {
                errs.push(format!("{}：标签 {labels:?} 有 _count 却没有 _sum", spec.name));
            }
        }
    }

    if errs.is_empty() {
        Ok(())
    } else {
        Err(errs)
    }
}

/// `le` → 数值（`+Inf` 用无穷大）。
fn parse_bound(le: &str) -> f64 {
    if le == "+Inf" {
        f64::INFINITY
    } else {
        le.parse::<f64>().unwrap_or(f64::NAN)
    }
}

/// 解析 `k="v",k2="v2"`（⛔ 不认转义：我们的标签值是静态短语；见到 `\` 就报错）。
fn parse_labels(body: &str) -> Result<Vec<(String, String)>, String> {
    if body.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for part in body.split(',') {
        let Some((k, v)) = part.split_once('=') else {
            return Err(format!("标签 {part:?} 不是 k=v"));
        };
        if k.is_empty()
            || !k
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return Err(format!("标签名 {k:?} 不合法"));
        }
        let Some(v) = v.strip_prefix('"').and_then(|r| r.strip_suffix('"')) else {
            return Err(format!("标签 {k} 的值没有用双引号括起来"));
        };
        if v.contains('"') || v.contains('\\') {
            return Err(format!("标签 {k} 的值含未转义的引号或反斜杠"));
        }
        out.push((k.to_string(), v.to_string()));
    }
    Ok(out)
}

// ══════════════════════════════════════════════════════════════
// 判据
// ══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn exercised() -> Metrics {
        let m = Metrics::for_query();
        m.set_mapping_joins(7);
        m.set_ttl_bytes(VIEW_CUSTOMER, 4_321);
        m.set_ttl_bytes(VIEW_MODEL, 12_345);
        m.rows(5);
        m.note_denial_code(REASON_TENANT_MISMATCH);
        m.note_denial_code(REASON_TENANT_UNRESOLVED);
        m.note_denial_code("OK"); // 不该计数
        m.observe_request("/api/sparql", "200", Duration::from_millis(3));
        m.observe_request("/api/sparql", "200", Duration::from_millis(37));
        m.observe_request("/api/catalog", "401", Duration::from_millis(1));
        // 一条明确落在 0.1 ~ 0.25 之间的耗时（直方图的桶边界不是摆设）。
        m.observe_request("/api/describe", "200", Duration::from_millis(120));
        m
    }

    #[test]
    fn the_six_metrics_are_exactly_this_set() {
        let names: Vec<&str> = METRICS.iter().map(|s| s.name).collect();
        assert_eq!(
            names,
            vec![
                "ontok_requests_total",
                "ontok_request_duration_seconds",
                "ontok_rows_returned_total",
                "ontok_tenant_denied_total",
                "ontok_ttl_bytes",
                "ontok_mapping_joins",
            ],
            "登记表就是需求 §2 的六个指标（改这里 = 改验收口径）"
        );
        // 标签集合固定（R4）：只许这四个维度 + 空的 mapping_joins。
        assert_eq!(METRICS[0].labels, &["service", "endpoint", "code"]);
        assert_eq!(METRICS[1].labels, &["service", "endpoint"]);
        assert_eq!(METRICS[2].labels, &["service"]);
        assert_eq!(METRICS[3].labels, &["service", "reason"]);
        assert_eq!(METRICS[4].labels, &["view"]);
        assert!(METRICS[5].labels.is_empty());
    }

    #[test]
    fn every_metric_has_help_type_and_a_valid_rendered_body() {
        let text = exercised().render();
        assert!(text.contains("# HELP ontok_requests_total "), "{text}");
        assert!(text.contains("# TYPE ontok_requests_total counter"), "{text}");
        assert!(
            text.contains("# TYPE ontok_request_duration_seconds histogram"),
            "{text}"
        );
        assert!(text.contains("# TYPE ontok_ttl_bytes gauge"), "{text}");
        assert!(
            text.contains("ontok_tenant_denied_total{service=\"ontok-query\",reason=\"TENANT_MISMATCH\"} 1"),
            "{text}"
        );
        assert!(
            text.contains("ontok_rows_returned_total{service=\"ontok-query\"} 5"),
            "{text}"
        );
        assert!(
            text.contains("ontok_ttl_bytes{view=\"customer\"} 4321"),
            "{text}"
        );
        assert!(text.contains("ontok_mapping_joins 7"), "{text}");
        assert!(
            text.contains("ontok_requests_total{service=\"ontok-query\",endpoint=\"/api/sparql\",code=\"200\"} 2"),
            "{text}"
        );
        if let Err(errs) = validate_exposition(&text) {
            panic!("渲染出的 exposition 不合法：\n{}", errs.join("\n") + "\n\n" + &text);
        }
    }

    #[test]
    fn a_histogram_is_cumulative_and_ends_with_inf() {
        let text = exercised().render();
        // 3 ms 与 37 ms 都 <= 0.05 ⇒ 0.05 桶是 2；两者也都 <= 0.005 / 0.01 / 0.025？3 ms 只 <= 0.005。
        assert!(
            text.contains("ontok_request_duration_seconds_bucket{service=\"ontok-query\",endpoint=\"/api/sparql\",le=\"0.005\"} 1"),
            "{text}"
        );
        assert!(
            text.contains("ontok_request_duration_seconds_bucket{service=\"ontok-query\",endpoint=\"/api/sparql\",le=\"0.05\"} 2"),
            "{text}"
        );
        assert!(
            text.contains("ontok_request_duration_seconds_bucket{service=\"ontok-query\",endpoint=\"/api/sparql\",le=\"+Inf\"} 2"),
            "{text}"
        );
        assert!(
            text.contains("ontok_request_duration_seconds_count{service=\"ontok-query\",endpoint=\"/api/sparql\"} 2"),
            "{text}"
        );
        // 120 ms 落进 0.25 桶（而 0.1 桶仍是 0）：桶边界真的被用上了。
        assert!(
            text.contains("ontok_request_duration_seconds_bucket{service=\"ontok-query\",endpoint=\"/api/describe\",le=\"0.1\"} 0"),
            "{text}"
        );
        assert!(
            text.contains("ontok_request_duration_seconds_bucket{service=\"ontok-query\",endpoint=\"/api/describe\",le=\"0.25\"} 1"),
            "{text}"
        );
    }

    #[test]
    fn counters_only_grow() {
        let m = Metrics::for_query();
        m.rows(2);
        let first = m.render();
        m.rows(3);
        let second = m.render();
        let take = |text: &str| -> f64 {
            text.lines()
                .find(|l| l.starts_with("ontok_rows_returned_total{service=\"ontok-query\"}"))
                .unwrap()
                .rsplit(' ')
                .next()
                .unwrap()
                .parse()
                .unwrap()
        };
        assert_eq!(take(&first), 2.0);
        assert_eq!(take(&second), 5.0, "counter 必须单调不减");
    }

    #[test]
    fn the_body_carries_no_business_data() {
        let m = exercised();
        let text = m.render();
        // 真实租户 uuid / 表名 / SQL 片段，一个都不许出现在 /metrics 里（R1 / R4）。
        for forbidden in [
            "c51dc886-6c91-43d7-a8fb-a0b18af67b81",
            "C0001",
            "deductions",
            "SELECT",
            "FROM ",
            "customers",
        ] {
            assert!(
                !text.contains(forbidden),
                "/metrics 里出现了业务数据 {forbidden:?}：\n{text}"
            );
        }
        // 端点标签是**路由模式**，不是真实 URL。
        assert_eq!(endpoint_label("/q/abc-123"), "/q/:id");
        assert_eq!(endpoint_label("/q/abc-123?x=1"), "/q/:id");
        assert_eq!(endpoint_label("/api/config/policy"), "/api/config/:kind");
        assert_eq!(endpoint_label("/q/a/b"), "other");
        assert_eq!(endpoint_label("/wp-admin"), "other");
        assert_eq!(endpoint_label("/api/sparql"), "/api/sparql");
        assert_eq!(endpoint_label("/metrics"), "/metrics");
        // 状态码收口：不认识的落到 other，不新增维度。
        assert_eq!(status_label(418), "other");
        assert_eq!(status_label(403), "403");
    }

    #[test]
    fn the_label_set_stays_small() {
        let m = Metrics::for_query();
        for _ in 0..100 {
            m.observe_request("/api/sparql", "200", Duration::from_micros(1));
            m.observe_request("/nope/whatever", "other", Duration::from_micros(1));
        }
        let text = m.render();
        let series: Vec<&str> = text
            .lines()
            .filter(|l| l.starts_with("ontok_requests_total{"))
            .collect();
        assert!(
            series.len() <= QUERY_ENDPOINTS.len() + 2,
            "序列数不该随请求数增长（维度爆炸）：{series:#?}"
        );
    }

    #[test]
    fn the_validator_rejects_broken_exposition() {
        // 反空转：每一种「手写格式会犯的错」都必须被认出来，否则 AC-002 是空转。
        let good = exercised().render();
        assert!(validate_exposition(&good).is_ok());
        let cases = [
            (good.replace("# TYPE ontok_rows_returned_total counter\n", ""), "# TYPE"),
            (good.replace("counter", "counterz"), "类型"),
            (good.replace("ontok_ttl_bytes{view=\"model\"} 12345", "ontok_ttl_bytes{view=\"model\"} abc"), "不是数字"),
            (good.replace("code=\"401\"", "code=401"), "双引号"),
            (good.replace("ontok_mapping_joins 7", "ontok_mapping_joins{tenant=\"x\"} 7"), "标签"),
            (good.replace("ontok_mapping_joins 7", "ontok_mapping_joins 7\nontok_mapping_joins 8"), "两次"),
        ];
        for (broken, needle) in cases {
            let errs = validate_exposition(&broken)
                .expect_err(&format!("这种坏格式必须被拒（{needle}）：\n{broken}"));
            assert!(
                errs.iter().any(|e| e.contains(needle)),
                "错误里应含 {needle:?}：{errs:#?}"
            );
        }
    }

    #[test]
    fn the_http_response_is_the_prometheus_content_type() {
        let res = exercised().http_response();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers()[header::CONTENT_TYPE], CONTENT_TYPE);
    }
}

//! ⭐ REQ-144 阶段 1 · **真调用通路**（TC3 签名 → HTTP 传输 → 错误分类）。
//!
//! 阶段 0 只有「读 fixture」一条通路（[`super::fetch::dry_run`]）；本模块是第二条，
//! 它**必须**由 `--allow-network` 显式打开（R1）。两条通路共用同一套
//! `Meta` / JSON-LD 形状 / R6 闸门 / R5 预算，差别只在于「行是从哪来的」。
//!
//! # 一条请求的完整形状（缺一样服务端就报签名失败）
//!
//! ```text
//! POST https://api.example.com/
//! Content-Type: application/json; charset=utf-8      ← 签名里用的就是这串
//! Host: api.example.com                       ← ureq 按 URI 自动加，与签名同值
//! X-TC-Action: SomeAction
//! X-TC-Version: 2017-03-12                            ← 契约 operation.version
//! X-TC-Timestamp: 1551113065
//! X-TC-Region: ap-guangzhou                           ← 契约 defaults.Region
//! Authorization: TC3-HMAC-SHA256 Credential=…/…/cvm/tc3_request, SignedHeaders=…, Signature=…
//!
//! {"Limit":100,"Offset":0}                            ← 签名的 payload 与发送的 body 是**同一个字符串**
//! ```
//!
//! # 凭据纪律（R3）
//!
//! | 规则 | 落点 |
//! | --- | --- |
//! | 值**只**从环境变量读 | [`Credentials::resolve`] 的 `lookup` 是唯一入口；测试注入假 lookup，⛔ 不读全局环境 |
//! | 契约里只有**变量名** | 变量名来自 `sources[].auth.credential_env`；没写就用官方标准名 |
//! | ⛔ 不进日志 / 错误 / 契约 | [`Credentials`] 的手写 `Debug` 打 `<redacted>`；所有 `AccessError` 只回显**变量名**与状态码 |
//!
//! # 五种失败各报各的码（R4）
//!
//! | 场景 | 码 | 重试？ | 依据 |
//! | --- | --- | --- | --- |
//! | 环境变量缺 | `AUTH_MISSING` | ⛔ | **发请求之前**就判（⛔ 不浪费一次配额去换 401） |
//! | HTTP 401 / 403 或 `AuthFailure.*` | `AUTH_REJECTED` | ⛔ | 重试只会得到同样的拒绝 |
//! | HTTP 429 或 `RequestLimitExceeded` | `RATE_LIMITED` | ✅ 退避、有上限 | 唯一值得重试的一类 |
//! | 响应里没有 `iterator` 指到的节点 | `FIELD_PATH_MISSING` | ⛔ | 契约错了，报给我们（不是数据空） |
//! | 超 `max_scan_rows` | `SCAN_LIMIT_EXCEEDED` | ⛔ | 阶段 0 同一条预算，**取页前**判 |
//!
//! 另外三码（同样是可行动的，不是一个 `request failed` 了事）：
//! `UPSTREAM_TIMEOUT`（明确说超时，⛔ 不静默）、`UPSTREAM_UNREACHABLE`（连不上）、
//! `API_ERROR`（服务端业务错误，带它的 `Code`/`Message`）。
//!
//! ⚠️ **「连不上」与「等太久」的边界**（复审点名的重点 —— 码名错了，改法就错了）：
//!
//! | 底层错误 | 码 | 运维该干什么 |
//! | --- | --- | --- |
//! | `Timeout(Connect)` / `Timeout(Resolve)`（**连接都没建立**） | `UPSTREAM_UNREACHABLE` | 查安全组 / 端口 / DNS / 出网白名单 |
//! | `Io(ConnectionRefused / …)`（被拒、DNS 失败、路由不通） | `UPSTREAM_UNREACHABLE` | 同上 |
//! | `Timeout(Global / RecvResponse / SendRequest / …)`（连上了但等太久） | `UPSTREAM_TIMEOUT` | 查链路质量 / 调大 `--timeout-secs` / 加窄过滤 |
//!
//! 把「连接超时」写成 `UPSTREAM_TIMEOUT` 会让运维去调 `timeout` —— 而真正该做的是查端口。
//!
//! # 为什么「200 里的错误信封」必须单独判
//!
//! 腾讯云大量错误是 **HTTP 200 + `{"Response":{"Error":{"Code":…,"Message":…}}}`**。
//! 只看状态码会把「签名被拒」「被限流」全部当成成功，然后在「解析行」那一步
//! 报成 `FIELD_PATH_MISSING` —— 那正好是把「凭据/限流问题」误诊成「契约问题」。
//!
//! # 已知限制（⛔ 不写成「已封堵」）
//!
//! - **没有做过任何真实 API 调用**：判据来自「官方向量 + 本地假端点」。
//!   请求体形状由契约的 `request.template` 逐字决定（作者写什么就发什么），
//!   ⛔ 执行器不替你猜「这个接口的参数该放在哪」—— 猜出来的东西没法审。
//! - 只支持 POST + JSON（签名模块同一条边界）；非 POST / 非 JSON **报不支持并点名**。
//! - 区域性接口才需要 `X-TC-Region`；契约没给 `defaults.Region` 时本实现**不发**这个头，
//!   并在 `meta.capability_warnings` 里说明（⛔ 不瞎填一个）。
//! - 分页只给**值**，不做算术变换（偏移 → 页码 这类映射写不出来）；
//!   若某接口的 `TotalCount` 与实际不一致，以服务端实际返回的行数为准。

use std::collections::BTreeMap;
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Map, Value};

use super::fetch::{self, classify_filters, FilterValue, FreshnessReceipt, JsonLdDoc, Meta};
use super::path;
use super::tc3::{self, SignInput};
use super::{
    expand_iri_template, placeholders, AccessError, BindingSource, Entity, Mapping, Ontology,
    PagingValue, RequestSpec, Source, CODE_API_ERROR, CODE_AUTH_MISSING,
    CODE_AUTH_MODE_UNSUPPORTED, CODE_AUTH_REJECTED, CODE_ENTITY_UNKNOWN, CODE_FIELD_PATH,
    CODE_KEY_MISSING, CODE_RATE_LIMITED, CODE_REQUEST_BINDING_MISSING, CODE_REQUEST_SHAPE,
    CODE_SCAN_LIMIT, CODE_SCHEMA, CODE_UNREVIEWED, CODE_UPSTREAM_TIMEOUT,
    CODE_UPSTREAM_UNREACHABLE,
};

/// 腾讯云官方文档里用的标准环境变量名（契约没声明 `credential_env` 时用它）。
pub const CANONICAL_SECRET_ID_ENV: &str = "TENCENTCLOUD_SECRET_ID";
/// 同上，密钥。
pub const CANONICAL_SECRET_KEY_ENV: &str = "TENCENTCLOUD_SECRET_KEY";

// ══════════════════════════════════════════════════════════════
// 凭据（值只从环境变量来）
// ══════════════════════════════════════════════════════════════

/// 一对云 API 凭据。**手写 `Debug`，不打印任何值**。
#[derive(Clone)]
pub struct Credentials {
    secret_id: String,
    secret_key: String,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // derive 的 Debug 会把 SecretKey 打进日志 / panic / 测试失败输出。
        write!(
            f,
            "Credentials {{ secret_id: \"<redacted>\", secret_key: \"<redacted>\" }}"
        )
    }
}

impl Credentials {
    /// 直接构造（**只给测试与将来的自检**；命令行一律走 [`Credentials::from_env`]）。
    pub fn new(secret_id: impl Into<String>, secret_key: impl Into<String>) -> Self {
        Self {
            secret_id: secret_id.into(),
            secret_key: secret_key.into(),
        }
    }

    /// 从环境变量读一对凭据。
    pub fn from_env(source: &Source) -> Result<Self, AccessError> {
        Self::resolve(source, &|name| std::env::var(name).ok())
    }

    /// 从**注入的**取值函数读凭据（单测用：⛔ 不污染进程环境，也就不会互相干扰）。
    ///
    /// 变量名优先级：契约 `auth.credential_env` 里声明的名字 → 官方标准名。
    /// 两边都没有（或都是空白）⇒ [`CODE_AUTH_MISSING`]，**在发请求之前**。
    pub fn resolve(
        source: &Source,
        lookup: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Self, AccessError> {
        let (mode, declared) = match &source.auth {
            Some(a) => (a.mode.as_str(), a.credential_env.clone()),
            None => ("none", Vec::new()),
        };
        if !mode.eq_ignore_ascii_case("tc3-hmac-sha256") {
            return Err(AccessError::new(
                CODE_AUTH_MODE_UNSUPPORTED,
                format!(
                    "source `{}` 的 `auth.mode` 是 `{mode}`，阶段 1 只实现 `tc3-hmac-sha256`。\n\
                     改法：把 `auth.mode` 写成 `tc3-hmac-sha256`（腾讯云 API 3.0 的签名方法），\
                     或在契约里声明正确的 `credential_env`。⛔ 本实现不会假装支持别的签名方式。",
                    source.id
                ),
            ));
        }

        let mut id_names: Vec<&str> = Vec::new();
        let mut key_names: Vec<&str> = Vec::new();
        if let Some(n) = declared.first() {
            id_names.push(n.as_str());
        }
        id_names.push(CANONICAL_SECRET_ID_ENV);
        if let Some(n) = declared.get(1) {
            key_names.push(n.as_str());
        }
        key_names.push(CANONICAL_SECRET_KEY_ENV);

        let secret_id = pick(lookup, &id_names);
        let secret_key = pick(lookup, &key_names);
        match (secret_id, secret_key) {
            (Some(id), Some(key)) => Ok(Self {
                secret_id: id,
                secret_key: key,
            }),
            (id, key) => {
                // ⚠️ 只点名**变量名**，⛔ 不回显任何值（含「已设置但为空」这种情况的原文）。
                let missing: Vec<String> = [
                    id.is_none().then(|| id_names.join(" / ")),
                    key.is_none().then(|| key_names.join(" / ")),
                ]
                .into_iter()
                .flatten()
                .collect();
                Err(AccessError::new(
                    CODE_AUTH_MISSING,
                    format!(
                        "缺少凭据：环境变量 {} 没有设置（或为空）。\n\
                         这一步在**发请求之前**判定 —— ⛔ 不先打一次接口再拿 401 当答案。\n\
                         改法：在**进程环境**里设置只读子账号的 SecretId / SecretKey\
                         （PowerShell：`$env:{}=…`；bash：`export {}=…`），\
                         然后再跑同一条命令。\n\
                         ⛔ 凭据不要写进 `access.json`、不要贴在命令行参数里、不要提交进仓库。\n\
                         没有凭据时请改用 `--dry-run-fixture` 走离线夹具（阶段 0 那条通路）。",
                        missing.join("、"),
                        CANONICAL_SECRET_ID_ENV,
                        CANONICAL_SECRET_KEY_ENV
                    ),
                ))
            }
        }
    }

    /// 凭据标识（进 `Authorization` 的 `Credential=`）。
    pub fn secret_id(&self) -> &str {
        &self.secret_id
    }

    /// 凭据密钥（只给签名用；`pub(crate)` ⇒ 本 crate 之外拿不到）。
    pub(crate) fn secret_key(&self) -> &str {
        &self.secret_key
    }
}

/// 依次试多个变量名；空白字符串视为「没设」。
fn pick(lookup: &dyn Fn(&str) -> Option<String>, names: &[&str]) -> Option<String> {
    for n in names {
        if let Some(v) = lookup(n) {
            if !v.trim().is_empty() {
                return Some(v);
            }
        }
    }
    None
}

// ══════════════════════════════════════════════════════════════
// 超时 / 重试策略
// ══════════════════════════════════════════════════════════════

/// 连接与整体超时（分开设：连不上要快点报，整体要给够）。
#[derive(Debug, Clone)]
pub struct HttpTimeouts {
    pub connect: Duration,
    pub global: Duration,
}

impl Default for HttpTimeouts {
    fn default() -> Self {
        Self {
            connect: Duration::from_secs(5),
            global: Duration::from_secs(30),
        }
    }
}

/// 退避重试策略。**只有可重试的错误**才用它（见模块头那张表）。
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    /// 总尝试次数（= 1 次首次 + `max_attempts-1` 次重试）。
    pub max_attempts: u32,
    /// 第一次退避时长，之后每次 ×2。
    pub base_delay: Duration,
    /// 单次退避上限。
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(200),
            max_delay: Duration::from_secs(2),
        }
    }
}

impl RetryPolicy {
    fn delay(&self, retry_index: u32) -> Duration {
        let factor = 1u32 << retry_index.min(16);
        (self.base_delay * factor).min(self.max_delay)
    }
}

// ══════════════════════════════════════════════════════════════
// 传输（`ureq`，阻塞式 —— 与本 crate 的同步纪律一致）
// ══════════════════════════════════════════════════════════════

/// 一次 HTTP 往返的结果（状态码 + 原文 + 花了几次尝试）。
#[derive(Debug, Clone)]
pub struct RawResponse {
    pub status: u16,
    pub body: String,
    /// 这次调用一共尝试了几次（1 = 没重试）。
    pub attempts: u32,
}

/// 极薄的 HTTP 客户端：**只做**「带头的 POST + 读回文本」。
pub struct Transport {
    agent: ureq::Agent,
    retry: RetryPolicy,
}

impl std::fmt::Debug for Transport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Transport")
            .field("retry", &self.retry)
            .finish()
    }
}

impl Transport {
    pub fn new(timeouts: HttpTimeouts, retry: RetryPolicy) -> Self {
        // ⚠️ `http_status_as_error(false)`：401 / 429 也要能读到**响应体**，
        // 否则「AuthFailure.SignatureFailure」「RequestLimitExceeded」这些
        // 决定分类与重试的字符串全被 ureq 的 Err 吞掉，只剩一个状态码。
        let cfg = ureq::Agent::config_builder()
            .timeout_connect(Some(timeouts.connect))
            .timeout_global(Some(timeouts.global))
            .http_status_as_error(false)
            .build();
        Self {
            agent: cfg.into(),
            retry,
        }
    }

    /// POST 一段 JSON；按 [`RetryPolicy`] 只对**可重试**的错误退避重试。
    pub fn post_json(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: &str,
    ) -> Result<RawResponse, AccessError> {
        let mut attempt = 1u32;
        loop {
            let outcome = self.attempt(url, headers, body);
            match outcome {
                Ok(resp) => match classify_http(resp.status, &resp.body) {
                    Verdict::Pass => {
                        return Ok(RawResponse {
                            attempts: attempt,
                            ..resp
                        })
                    }
                    Verdict::Fatal(e) => return Err(e),
                    Verdict::Retry(e) => {
                        if attempt >= self.retry.max_attempts {
                            return Err(retry_exhausted(e, attempt));
                        }
                        std::thread::sleep(self.retry.delay(attempt - 1));
                        attempt += 1;
                    }
                },
                Err((retryable, e)) => {
                    if !retryable || attempt >= self.retry.max_attempts {
                        return Err(if retryable {
                            retry_exhausted(e, attempt)
                        } else {
                            e
                        });
                    }
                    std::thread::sleep(self.retry.delay(attempt - 1));
                    attempt += 1;
                }
            }
        }
    }

    /// 单次尝试。`Err((是否可重试, 错误))`。
    fn attempt(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: &str,
    ) -> Result<RawResponse, (bool, AccessError)> {
        let mut req = self.agent.post(url);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let resp = req.send(body).map_err(|e| transport_error(e, url))?;
        let status = resp.status().as_u16();
        let text = resp
            .into_body()
            .read_to_string()
            .map_err(|e| transport_error(e, url))?;
        Ok(RawResponse {
            status,
            body: text,
            attempts: 1,
        })
    }
}

/// 传输层错误 → 码 + 可读原因（**⛔ 不回显请求体 / Authorization / 凭据**）。
fn transport_error(e: ureq::Error, url: &str) -> (bool, AccessError) {
    let host = host_of(url);
    match &e {
        // ⚠️ **连接阶段**的超时 ⇒ 根本没连上 ⇒ 「不可达」而不是「超时」。
        // 两者运维动作完全不同：前者查安全组/端口/DNS，后者查链路质量/调大 timeout。
        // （复审点名的边界：把它归成 TIMEOUT，运维就会去调一个治不好病的旋钮。）
        ureq::Error::Timeout(ureq::Timeout::Connect | ureq::Timeout::Resolve) => (
            true,
            AccessError::new(
                CODE_UPSTREAM_UNREACHABLE,
                format!(
                    "连不上 endpoint `{host}`：连接阶段超时（{e}）。\n\
                     ⚠️ 这是**连接没建立**，不是「服务端慢」——调大 `--timeout-secs` 不会让它变通。\n\
                     这不是「没数据」：⛔ 不返回空结果。\n\
                     改法：查安全组 / 端口是否开放、DNS 是否解析到该主机、出网白名单；\
                     只想验映射逻辑请用 `--dry-run-fixture`。"
                ),
            ),
        ),
        ureq::Error::Timeout(kind) => (
            true,
            AccessError::new(
                CODE_UPSTREAM_TIMEOUT,
                format!(
                    "请求超时（endpoint `{host}`，超时类型 {kind:?}）。\n\
                     连接是通的，是**等待**超了：⛔ 不返回空结果、⛔ 不静默成「没数据」。\n\
                     改法：确认到 `{host}` 的链路质量 / 代理；服务端慢时把 `--timeout-secs` 调大，\
                     或加更窄的过滤减少单页数据量。"
                ),
            ),
        ),
        _ => (
            true,
            AccessError::new(
                CODE_UPSTREAM_UNREACHABLE,
                format!(
                    "连不上 endpoint `{host}`：{e}。\n\
                     这不是「没数据」：⛔ 不返回空结果。\n\
                     改法：查 DNS / 端口是否开放 / 安全组与出网白名单 / 代理；\
                     只想验映射逻辑请用 `--dry-run-fixture`。"
                ),
            ),
        ),
    }
}

/// 重试用尽时把「试了几次」写进消息（⛔ 不静默地少试一次）。
fn retry_exhausted(e: AccessError, attempts: u32) -> AccessError {
    AccessError::new(
        e.code,
        format!(
            "{}（已尝试 {attempts} 次，退避上限已用完 —— ⛔ 不会无限重试）",
            e.message
        ),
    )
}

/// 一次响应的三种去向。
enum Verdict {
    /// 可以继续处理（可能是业务错误，由上层再判一次信封）。
    Pass,
    /// 值得退避重试。
    Retry(AccessError),
    /// 再来一次也一样。
    Fatal(AccessError),
}

/// 只看状态码 + 响应体信封，决定「过 / 重试 / 放弃」。
fn classify_http(status: u16, body: &str) -> Verdict {
    // ① 先看信封：腾讯云大量错误是 HTTP 200 + Response.Error。
    if let Some((code, message)) = api_error(body) {
        return classify_api_error_code(status, &code, &message);
    }
    match status {
        200..=299 => Verdict::Pass,
        401 | 403 => Verdict::Fatal(auth_rejected(
            status,
            "（响应体里没有 Response.Error，只有状态码）",
        )),
        429 => Verdict::Retry(rate_limited(
            status,
            "（响应体里没有 Response.Error，只有状态码）",
        )),
        500..=599 => Verdict::Retry(AccessError::new(
            CODE_API_ERROR,
            format!(
                "服务端 {status}（5xx）—— 这类通常可重试，但重试也有上限。\n\
                 若持续 5xx：这是服务端问题，⛔ 不要改契约去迁就它。"
            ),
        )),
        400..=499 => Verdict::Fatal(AccessError::new(
            CODE_API_ERROR,
            format!(
                "服务端 {status}（4xx）—— 请求本身不合法（参数名 / 参数值 / 版本）。\n\
                 改法：对照接口文档检查 `access.json` 的 `operation` 与 `request.from_filter`；\
                 ⛔ 重试不会让它变合法。\n\
                 响应体前 300 字（已截断）：{}",
                clip(body)
            ),
        )),
        other => Verdict::Fatal(AccessError::new(
            CODE_API_ERROR,
            format!(
                "服务端返回了不认识的状态码 {other}。响应体前 300 字：{}",
                clip(body)
            ),
        )),
    }
}

/// 从响应体里取 `Response.Error.Code` / `Message`。
fn api_error(body: &str) -> Option<(String, String)> {
    let v: Value = serde_json::from_str(body).ok()?;
    let err = v.get("Response")?.get("Error")?;
    let code = err.get("Code")?.as_str()?.to_string();
    let message = err
        .get("Message")
        .and_then(|m| m.as_str())
        .unwrap_or("")
        .to_string();
    Some((code, message))
}

/// 业务错误码 → 分类（这是「同一件事只有一个判定点」的地方）。
fn classify_api_error_code(status: u16, code: &str, message: &str) -> Verdict {
    let detail = format!("服务端返回码 `{code}`：{message}");
    let auth_failure = code.starts_with("AuthFailure")
        || code.contains("AuthFailure")
        || code == "InvalidCredential"
        || code.starts_with("UnauthorizedOperation");
    if auth_failure || status == 401 || status == 403 {
        return Verdict::Fatal(auth_rejected(status, &detail));
    }
    if code == "RequestLimitExceeded" || code.starts_with("RequestLimit") {
        return Verdict::Retry(rate_limited(status, &detail));
    }
    Verdict::Fatal(AccessError::new(
        CODE_API_ERROR,
        format!(
            "{detail}（HTTP {status}）。\n\
             这是**业务错误**，不是签名问题：按这个码改请求（参数 / 资源 id / 权限面），\
             ⛔ 重试同一个请求不会好。"
        ),
    ))
}

fn auth_rejected(status: u16, detail: &str) -> AccessError {
    AccessError::new(
        CODE_AUTH_REJECTED,
        format!(
            "凭据被服务端拒绝（HTTP {status}）。{detail}\n\
             这是**签名 / 凭据**问题，⛔ 不重试（重试只会得到同样的拒绝）。逐项检查：\n\
             1. `TENCENTCLOUD_SECRET_ID` / `TENCENTCLOUD_SECRET_KEY` 是不是**同一对**；\n\
             2. 本机时钟与标准时间是否相差超过 5 分钟（文档：超过必失败）；\n\
             3. 该子账号有没有这个接口的读权限（只读策略是否覆盖这个 Action）；\n\
             4. 契约 `operation.version` 与服务端要求的版本是否一致。"
        ),
    )
}

fn rate_limited(status: u16, detail: &str) -> AccessError {
    AccessError::new(
        CODE_RATE_LIMITED,
        format!(
            "被服务端限流（HTTP {status}）。{detail}\n\
             本命令会**退避重试**（有上限），这次是重试用尽后的结论。\n\
             改法：① 隔一会儿再跑；② 把 `--limit` 调小 / 加更窄的过滤，减少请求次数；\
             ③ 确认没有并发跑多个 `access fetch`。"
        ),
    )
}

/// 响应体截断（错误信息里不带整份响应；也避免把大响应塞进日志）。
fn clip(body: &str) -> String {
    body.chars()
        .take(300)
        .collect::<String>()
        .replace('\n', " ")
}

// ══════════════════════════════════════════════════════════════
// 取数（真调用）
// ══════════════════════════════════════════════════════════════

/// 真调用的选项（与 [`FetchOptions`] 同名同义的 `filter` / `limit`，另加网络侧旋钮）。
#[derive(Debug, Clone)]
pub struct NetworkOptions {
    pub filter: BTreeMap<String, FilterValue>,
    pub limit: usize,
    /// 固定时间戳（测试对拍用）。`None` ⇒ 本机时钟。
    ///
    /// ⚠️ 生产路径**不**允许命令行指定它（否则签名有效期变成可伪造的），
    /// 只有测试与将来的「签名自检」会钉死它。
    pub timestamp: Option<i64>,
    /// 覆盖 service（默认从 host 推：`api.example.com` → `cvm`）。
    /// 测试用假 endpoint 时钉成 `cvm`，好让签名可被独立复算。
    pub service: Option<String>,
    pub retry: RetryPolicy,
    pub timeouts: HttpTimeouts,
}

impl Default for NetworkOptions {
    fn default() -> Self {
        Self {
            filter: BTreeMap::new(),
            limit: 100,
            timestamp: None,
            service: None,
            retry: RetryPolicy::default(),
            timeouts: HttpTimeouts::default(),
        }
    }
}

/// 真调用的回执（只在网络通路出现；`Meta::network` 是 `Option`，
/// 阶段 0 的 `--dry-run-fixture` 输出**不出现这个字段** ⇒ 逐字节不变）。
#[derive(Debug, Clone, Serialize)]
pub struct NetworkReceipt {
    /// 请求域名（不含路径，不含 query）。
    pub host: String,
    /// 签名用的 service（`cvm` / `monitor` …）。
    pub service: String,
    /// 所有页累计的 HTTP 尝试次数（含重试）。
    pub attempts: u32,
    /// 其中重试了几次（= attempts − 页数）。
    pub retries: u32,
    /// 最后一次响应的 HTTP 状态码。
    pub http_status: u16,
    /// 实际发出的参与签名的头（可复核「签的与发的是同一套」）。
    pub signed_headers: String,
}

/// 按映射**真调**一次 API，把行组装成与 `query serve` 同形状的 JSON-LD。
///
/// 与 [`super::fetch::dry_run`] 的关系：
///
/// | 步骤 | dry_run | 本函数 |
/// | --- | --- | --- |
/// | R6 未签闸门 | 一样 | 一样 |
/// | 过滤分类 | 一样（同一个 [`classify_filters`]） | 一样 |
/// | 行从哪来 | fixture 文件 | HTTP（分页真取） |
/// | R5 预算 | 取页前判 | 取页前判（同一句话、同一个码） |
/// | `filtered_locally` / `freshness` / `capability_warnings` | 同三栏 | 同三栏 + `network` |
pub fn fetch_network(
    mapping: &Mapping,
    ontology_iri: &str,
    entity_id: &str,
    opts: &NetworkOptions,
    creds: &Credentials,
) -> Result<JsonLdDoc, AccessError> {
    // 不需要原始响应体时走这条（把 sink 丢掉）。
    let mut discard: Option<String> = None;
    fetch_network_capturing(mapping, ontology_iri, entity_id, opts, creds, &mut discard)
}

/// 同 [`fetch_network`]，但把**第一页的原始响应体**（逐字节、未裁剪）交给调用方。
///
/// 用途：`ontok access fetch --allow-network --dump-raw <path>` ——
/// 拿一份**真实响应**去替换合成的 fixture（`fixtures/README.md` 里欠的那件事）。
///
/// 为什么是**第一页**：同一份响应的各页结构完全相同，第一页就是「结构样本」；
/// 而且它是**最小**的那份（含 `RequestId` / `TotalCount` / `InstanceSet[]` 全部字段，⛔ 不裁剪）。
/// 要多几行就把 `--limit` 调大后重跑。
///
/// ⚠️ 这里只**交出**原文，**不写盘**：写文件仍走 CLI 那一个登记过的写点（出口纪律）。
pub fn fetch_network_capturing(
    mapping: &Mapping,
    ontology_iri: &str,
    entity_id: &str,
    opts: &NetworkOptions,
    creds: &Credentials,
    raw_first_page: &mut Option<String>,
) -> Result<JsonLdDoc, AccessError> {
    let entity = mapping.entity(entity_id).ok_or_else(|| {
        AccessError::new(
            CODE_ENTITY_UNKNOWN,
            format!(
                "`{entity_id}` 不在这份契约里（有：{}）。",
                mapping
                    .entities
                    .iter()
                    .map(|e| e.id.as_str())
                    .collect::<Vec<_>>()
                    .join(" / ")
            ),
        )
    })?;

    // ── 1 · R6：未签的映射不许取数（与 dry_run 同一句话）──────────
    if !entity.reviewed() {
        return Err(AccessError::new(
            CODE_UNREVIEWED,
            format!(
                "entity `{}` 的 `reviewed_by` 为空 —— 未签的映射不许取数（R6 / ADR-005）。\n\
                 AI 生成的映射也是**提案**：先让人把 `reviewed_by` 签上\
                 （`\"张三@2026-10-25\"`），再跑取数。",
                entity.id
            ),
        ));
    }

    if opts.limit == 0 {
        return Err(AccessError::new(
            CODE_SCHEMA,
            "`--limit` 必须 ≥ 1（0 行没有意义，而回执会看起来像「查了但什么都没有」）。"
                .to_string(),
        ));
    }

    let source = mapping.source(&entity.source).ok_or_else(|| {
        AccessError::new(
            CODE_SCHEMA,
            format!(
                "entity `{}` 引用的 source `{}` 不存在（`sources[].id` 里没有它）。",
                entity.id, entity.source
            ),
        )
    })?;

    let base_url = source.base_url.trim().trim_end_matches('/');
    if base_url.is_empty() {
        return Err(AccessError::new(
            CODE_SCHEMA,
            format!(
                "source `{}` 的 `base_url` 是空的 —— 没有地址就没法调用。",
                source.id
            ),
        ));
    }
    let host = host_of(base_url);
    let url = format!("{base_url}/");
    let service = opts.service.clone().unwrap_or_else(|| service_of(&host));
    let timestamp = opts
        .timestamp
        .unwrap_or_else(|| chrono::Utc::now().timestamp());

    // ── 2 · 过滤分类（与 fixture 通路共用同一份逻辑）──────────────
    let plan = classify_filters(entity, &opts.filter)?;
    let mut warnings = plan.warnings;

    // ── 3 · 逐页真取 ─────────────────────────────────────────
    let transport = Transport::new(opts.timeouts.clone(), opts.retry.clone());
    let page_size = entity
        .paging
        .as_ref()
        .map(|p| p.max_limit)
        .filter(|n| *n > 0)
        .unwrap_or(100);
    let content_type = entity.operation.content_type.clone();
    let mut offset = 0usize;
    let mut rows_scanned = 0usize;
    let mut pages = 0usize;
    let mut matched = 0usize;
    let mut kept: Vec<Value> = Vec::new();
    let mut attempts_total = 0u32;
    // 只统计**传输层的重试**（= 每次调用的尝试次数 − 1 之和），与翻页次数无关。
    let mut retries_total = 0u32;
    let mut last_status = 0u16;
    let mut total: Option<usize> = None;
    let mut last_page_rows = 0usize;

    loop {
        if kept.len() >= opts.limit {
            break;
        }
        let need = opts.limit - kept.len();
        let page_limit = page_size.min(need.max(1));

        // ⭐ R5：**取页前**判预算（与 dry_run 同一条纪律：⛔ 不拉完再判）
        if rows_scanned as u64 + page_limit as u64 > entity.capability.max_scan_rows {
            return Err(AccessError::new(
                CODE_SCAN_LIMIT,
                format!(
                    "继续取下一页就会超过 `max_scan_rows`：已扫 {rows_scanned} 行（{pages} 页），\
                     下一页要 {page_limit} 行 —— 合计会到 {}，预算只有 {}。\n\
                     ⛔ 拒绝返回部分结果。请**改查询**（加更窄的过滤 / 调小 --limit），\
                     ⛔ 不要重试 —— 重试只会再撞同一面墙。",
                    rows_scanned + page_limit,
                    entity.capability.max_scan_rows
                ),
            ));
        }

        let (payload, region) =
            build_payload(entity, source, &plan.pushdown_filters, offset, page_limit)?;
        let signed = tc3::sign(&SignInput {
            secret_id: creds.secret_id(),
            secret_key: creds.secret_key(),
            service: &service,
            host: &host,
            action: &entity.operation.action,
            timestamp,
            payload: &payload,
            content_type: &content_type,
            method: "POST",
            canonical_uri: "/",
            canonical_query: "",
        })?;

        // 头必须与签名里那三个**逐字一致**（content-type / host / x-tc-action）。
        let ts_text = timestamp.to_string();
        let mut headers: Vec<(&str, &str)> = vec![
            ("Content-Type", content_type.as_str()),
            ("X-TC-Action", entity.operation.action.as_str()),
            ("X-TC-Timestamp", ts_text.as_str()),
            ("Authorization", signed.authorization.as_str()),
        ];
        if let Some(v) = entity.operation.version.as_deref() {
            headers.push(("X-TC-Version", v));
        } else {
            warnings.push(
                "契约没写 `operation.version` ⇒ 本次**没发** `X-TC-Version` 头。\
                 腾讯云 API 3.0 要求它，请在 `access.json` 里补上（如 `2017-03-12`）。"
                    .to_string(),
            );
        }
        if let Some(r) = region.as_deref() {
            headers.push(("X-TC-Region", r));
        } else {
            warnings.push(
                "契约的 `sources[].defaults` 里没有 `Region` ⇒ 本次**没发** `X-TC-Region` 头。\
                 区域性接口会因此失败；可把它写进 defaults（如 `{\"Region\":\"ap-guangzhou\"}`）。"
                    .to_string(),
            );
        }

        let resp = transport.post_json(&url, &headers, &payload)?;
        attempts_total += resp.attempts;
        retries_total += resp.attempts.saturating_sub(1);
        last_status = resp.status;

        // ⭐ `--dump-raw`：把**第一页**的原始响应体（逐字节、未裁剪）交出去。
        // 只交不改：落盘由 CLI 的登记写点做。
        if pages == 0 {
            *raw_first_page = Some(resp.body.clone());
        }

        let doc: Value = serde_json::from_str(&resp.body).map_err(|e| {
            AccessError::new(
                CODE_API_ERROR,
                format!(
                    "服务端返回的不是 JSON（HTTP {}）：{e}。响应体前 300 字：{}",
                    resp.status,
                    clip(&resp.body)
                ),
            )
        })?;
        // 双保险：HTTP 200 + 错误信封（见模块头）
        if let Some((code, message)) = api_error(&resp.body) {
            let e = match classify_http(200, &resp.body) {
                Verdict::Pass => AccessError::new(
                    CODE_API_ERROR,
                    format!("服务端返回错误 `{code}`：{message}"),
                ),
                Verdict::Fatal(e) | Verdict::Retry(e) => e,
            };
            return Err(e);
        }

        // 页数在**这一页真的取回来了**之后才 +1（哪怕它 0 行）：
        // 回执要如实说「发了几页请求」，「0 条结果」不等于「没发请求」。
        let first_page = pages == 0;
        if first_page {
            total = total_from(&doc, entity);
        }
        pages += 1;
        let rows = path::iterate(&doc, &entity.iterator);
        last_page_rows = rows.len();
        if rows.is_empty() {
            if first_page {
                // 第 1 页就选不到节点：三种情况**都**是契约错了，不是「数据空」。
                //   ① 容器路径整个不存在（`$.Response.InstanceSet` 没有）
                //   ② 容器在、但不是数组（结构变了）
                //   ③ 容器是空数组，但 `total_path` 说还有行（自相矛盾）
                let container = path::select(&doc, path::strip_iter(&entity.iterator));
                let container_missing = container.is_none();
                let not_array = container.map(|c| !c.is_array()).unwrap_or(false);
                let declared_total = total.unwrap_or(0);
                if container_missing || not_array || declared_total > 0 {
                    let why = if container_missing {
                        "响应里连这个路径都没有"
                    } else if not_array {
                        "这个路径在响应里不是数组"
                    } else {
                        "这个数组是空的，但 total_path 说有行"
                    };
                    return Err(AccessError::new(
                        CODE_FIELD_PATH,
                        format!(
                            "响应里没有 `iterator` 指到的节点：`iterator` = `{}`\
                             （`total_path` = {:?}，读到 {}；{why}）。\n\
                             这是**契约写错了**（或服务端响应结构变了），不是「没有数据」。\n\
                             改法：拿这份真实响应校准 `access.json` 的 `iterator` / `total_path` / `fields.*.path`。\n\
                             ⛔ 不要重试 —— 重试还是同一个路径。\n\
                             响应前 300 字：{}",
                            entity.iterator,
                            entity.paging.as_ref().and_then(|p| p.total_path.as_deref()),
                            declared_total,
                            clip(&resp.body)
                        ),
                    ));
                }
            }
            break; // 真到头了（或本来就是 0 条）
        }

        for row in rows.iter().copied() {
            if !path::exists(row, &entity.key_path) {
                return Err(AccessError::new(
                    CODE_KEY_MISSING,
                    format!(
                        "第 {pages} 页有一行取不到键 `{}` —— 拼不出实例 IRI。\n\
                         这是契约写错了（键路径不是这个数组元素上的字段），不是数据问题。",
                        entity.key_path
                    ),
                ));
            }
            let hit = plan.local_filters.iter().all(|(k, want)| {
                match fetch::local_path(entity, k) {
                    Some(p) => want.matches(path::select(row, &p)),
                    // 与 fixture 通路同口径：本地过滤的字段在响应里没有 ⇒ 该行不命中
                    None => false,
                }
            });
            if hit {
                matched += 1;
                if kept.len() < opts.limit {
                    kept.push(row.clone());
                }
            }
        }
        rows_scanned += rows.len();
        offset += rows.len();

        if last_page_rows < page_limit {
            break; // 服务端给不满一页 ⇒ 没有下一页
        }
        if let Some(t) = total {
            if offset >= t {
                break;
            }
        }
        // 防死循环的安全阀：预算已经由 R5 兜住，这里是「预算写成了极大值」时的兜底。
        if pages >= 100_000 {
            warnings.push(
                "翻页次数到 100000 页上限 —— 已停止继续取（多半是服务端 offset 没生效）。"
                    .to_string(),
            );
            break;
        }
    }

    // 截断判定：拿满了 --limit、而服务端**可能**还有
    let truncated = kept.len() >= opts.limit
        && match total {
            Some(t) => offset < t || last_page_rows == page_size,
            None => last_page_rows == page_size,
        };

    // ── 4 · 组装（与 dry_run 同一套：@context / @graph / summary / meta）────
    let class_local = Ontology::local_name(&entity.class).to_string();
    let template = expand_iri_template(&entity.iri_template, ontology_iri);
    let row_refs: Vec<&Value> = kept.iter().collect();
    let graph = fetch::assemble_graph(&row_refs, entity, &class_local, &template, &mut warnings);

    let mut context: BTreeMap<&'static str, String> = BTreeMap::new();
    context.insert(
        crate::query::jsonld::ONTOK_PREFIX,
        format!("{ontology_iri}#"),
    );
    context.insert("xsd", crate::query::jsonld::XSD_NS.to_string());

    let mut summary = Map::new();
    summary.insert("nodes".to_string(), json!(graph.len()));
    summary.insert("row_count".to_string(), json!(rows_scanned));

    let mut pushdown_applied: Vec<String> = plan
        .pushdown_filters
        .iter()
        .map(|(k, _)| k.clone())
        .collect();
    pushdown_applied.sort();

    let meta = Meta {
        rows_scanned,
        pages,
        rows_matched: matched,
        rows_returned: graph.len(),
        filtered_locally: plan.local_keys,
        freshness: FreshnessReceipt {
            mode: entity.freshness.mode.clone(),
            note: entity.freshness.note.clone(),
            fetched_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            // ⚠️ 与 fixture 通路**必须**可区分：这是现场数据。
            source: "api".to_string(),
        },
        capability_warnings: warnings,
        truncated,
        limit: opts.limit,
        max_scan_rows: entity.capability.max_scan_rows,
        // 真调用没有「模拟下推」这回事：声明的下推都进了请求参数。
        unsimulated_pushdown: Vec::new(),
        pushdown_applied,
        mapping_ref: format!("access#{}@v{}", entity.id, mapping.mapping_version),
        entity: entity.id.clone(),
        class: entity.class.clone(),
        network: Some(NetworkReceipt {
            host: host.clone(),
            service,
            attempts: attempts_total,
            retries: retries_total,
            http_status: last_status,
            signed_headers: tc3::SIGNED_HEADERS.to_string(),
        }),
    };

    Ok(JsonLdDoc {
        context,
        graph,
        summary: Value::Object(summary),
        meta,
    })
}

/// 拼一页的请求体 + 该发的 `X-TC-Region`。
///
/// # 请求体从哪来（**模板优先**）
///
/// | 契约里有什么 | 请求体怎么来 |
/// | --- | --- |
/// | `request.template` | **按模板逐字渲染**：`{name}` 按 `bindings[name]` 取值替换（只替换字符串值，⛔ 不改键名） |
/// | 只有旧写法 `request.from_filter` | 解析时已内部转成模板（`{"zone":"Zone"}` ⇒ `{"Zone":"{zone}"}`） |
/// | 两者都没有 | 旧口径：`defaults`（除 Region）+ 每个下推过滤条件按**过滤名**做顶层参数 + 分页参数 |
///
/// # 为什么是「模板」而不是「参数表」
///
/// 请求体形状至少有三类真实存在：**顶层标量**、**数组里放对象**、**深层嵌套**
/// （`{"Cond":{"Rules":[{"Field":"m","Value":"v"}]}}`）。参数表只能表达第一类。
/// 模板把形状交给作者写，代码只做替换 ⇒ **零厂商/接口名词**，也不需要为哪个接口加分支。
///
/// # 缺值剪枝（`--filter` 没给该条件时）
///
/// | 占位符位置 | 没取到值时 |
/// | --- | --- |
/// | 对象的成员（`"K":"{x}"`） | 丢掉 `K` |
/// | 数组元素（该元素子树里有一处被剪） | 丢掉整个元素；数组变空则连这个键一起丢 |
/// | 字符串里**嵌着**占位符（`"a-{x}"`） | **报错**（⛔ 不写半截字符串） |
///
/// # 分页
///
/// `paging.offset` / `.limit` 是**路径**（`Offset` 或 `Page.Number`），渲染后按路径写入
/// （中间层不存在就建对象）。⛔ 不硬编码位置，也不做「偏移 → 页码」这类算术变换
/// （契约只能给值，不能给表达式 —— 见模块头「已知限制」）。
fn build_payload(
    entity: &Entity,
    source: &Source,
    pushdown: &[(String, FilterValue)],
    offset: usize,
    page_limit: usize,
) -> Result<(String, Option<String>), AccessError> {
    ensure_json_post(entity)?;

    let offsets = PagingPaths {
        offset: entity
            .request
            .paging_offset
            .clone()
            .or_else(|| entity.paging.as_ref().map(|p| p.offset_param.clone()))
            .unwrap_or_else(|| "Offset".to_string()),
        limit: entity
            .request
            .paging_limit
            .clone()
            .or_else(|| entity.paging.as_ref().map(|p| p.limit_param.clone()))
            .unwrap_or_else(|| "Limit".to_string()),
    };

    // `X-TC-Region`：只来自 `sources[].defaults.Region`（大小写不敏感）；
    // 没有 default 时，退让给「名叫 region 的下推过滤条件」（保持旧行为）。
    let mut region: Option<String> = source
        .defaults
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("Region"))
        .and_then(|(_, v)| v.as_str().map(|s| s.to_string()));
    if region.is_none() {
        if let Some((_, v)) = pushdown
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("region"))
        {
            region = Some(value_as_text(v)?);
        }
    }

    let body: Value = match &entity.request.template {
        Some(template) => {
            let ctx = RenderCtx {
                defaults: &source.defaults,
                pushdown,
                spec: &entity.request,
                offset,
                limit: page_limit,
            };
            // 必填绑定先对账（缺 ⇒ REQUEST_BINDING_MISSING，而不是发出一个空值）
            for name in &entity.request.required {
                if ctx.resolve(name)?.is_none() {
                    return Err(AccessError::new(
                        CODE_REQUEST_BINDING_MISSING,
                        format!(
                            "请求模板的必填绑定 `{name}` 取不到值 —— 模板里有 `{{{name}}}`，\
                             契约把它列进了 `request.required`，但这次请求没有对应的值。\n\
                             改法：① 在 `--filter` 里给它（`--filter '{{\"{name}\":\"…\"}}'`）；\
                             ② 或让 `sources[].defaults` 提供它；\
                             ③ 或把它从 `request.required` 里去掉（那就变成可选，缺了会被剪掉）。",
                        ),
                    ));
                }
            }
            let (rendered, _pruned) = render_node(template, &ctx)?;
            rendered.unwrap_or_else(|| Value::Object(Map::new()))
        }
        None => {
            // 旧口径：顶层参数 + 分页参数（没有模板时的最小可用路径）
            let mut body = Map::new();
            for (k, v) in &source.defaults {
                if k.eq_ignore_ascii_case("Region") {
                    continue;
                }
                body.insert(k.clone(), v.clone());
            }
            for (name, value) in pushdown {
                if name.eq_ignore_ascii_case("region") {
                    continue; // 已进头
                }
                body.insert(name.clone(), filter_value_json(value));
            }
            Value::Object(body)
        }
    };

    // 分页值按**路径**写进去（模板里没有这条路就建出来）
    let mut body = body;
    set_path(&mut body, &offsets.offset, json!(offset))?;
    set_path(&mut body, &offsets.limit, json!(page_limit))?;

    // ⚠️ 这里序列化出来的**就是**被签名的 payload（⛔ 不发第二份）。
    let text = serde_json::to_string(&body).unwrap_or_else(|_| "{}".to_string());
    Ok((text, region))
}

/// 分页值要写到的两个路径。
struct PagingPaths {
    offset: String,
    limit: String,
}

/// 请求体形状闸：本实现只签/只发 **POST + JSON**。
///
/// 非 JSON 形态（XML body / 表单编码 / GET query 签名）⇒ **报不支持并点名**，
/// ⛔ 不偷偷适配、也不静默降级。
fn ensure_json_post(entity: &Entity) -> Result<(), AccessError> {
    if !entity.operation.method.eq_ignore_ascii_case("POST") {
        return Err(AccessError::new(
            CODE_REQUEST_SHAPE,
            format!(
                "不支持的请求形状：契约写的 `operation.method` 是 `{}`。\n\
                 本实现只签/只发 **POST + JSON 请求体**。GET 的 query 签名\
                 （CanonicalQueryString + RFC3986 编码）**没有实现** —— ⛔ 不假装支持。\n\
                 改法：把 `operation.method` 改成 `POST`（绝大多数查询接口都支持 POST+JSON）。",
                entity.operation.method
            ),
        ));
    }
    if !entity
        .operation
        .content_type
        .to_ascii_lowercase()
        .contains("json")
    {
        return Err(AccessError::new(
            CODE_REQUEST_SHAPE,
            format!(
                "不支持的请求体类型：契约写的 `operation.content_type` 是 `{}`。\n\
                 本实现只签/只发 **JSON** 请求体；XML / multipart / \
                 application/x-www-form-urlencoded 都**没有实现** —— ⛔ 不偷偷适配。",
                entity.operation.content_type
            ),
        ));
    }
    Ok(())
}

/// 一页请求的渲染上下文（模板 / 默认值 / 过滤值 / 分页值）。
struct RenderCtx<'a> {
    defaults: &'a BTreeMap<String, Value>,
    pushdown: &'a [(String, FilterValue)],
    spec: &'a RequestSpec,
    offset: usize,
    limit: usize,
}

impl RenderCtx<'_> {
    /// 取一个占位符的值。`None` ⇒ 这次没有值（交给剪枝规则处理）。
    fn resolve(&self, name: &str) -> Result<Option<Value>, AccessError> {
        let Some(binding) = self.spec.bindings.get(name) else {
            // 解析阶段已经对过账；走到这里说明是内部不一致，仍然给出可行动信息。
            return Err(AccessError::new(
                CODE_REQUEST_BINDING_MISSING,
                format!("模板占位符 `{{{name}}}` 没有绑定（内部不一致，请报缺陷）。"),
            ));
        };
        match binding.source {
            BindingSource::Default => Ok(self.defaults.get(name).cloned().or_else(|| {
                self.defaults
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case(name))
                    .map(|(_, v)| v.clone())
            })),
            BindingSource::Filter => Ok(self
                .pushdown
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| match v {
                    FilterValue::One(s) => Value::String(s.clone()),
                    FilterValue::Many(items) => {
                        Value::Array(items.iter().map(|s| Value::String(s.clone())).collect())
                    }
                })),
            BindingSource::Paging(PagingValue::Offset) => Ok(Some(json!(self.offset))),
            BindingSource::Paging(PagingValue::Limit) => Ok(Some(json!(self.limit))),
        }
    }
}

/// 渲染一个模板节点。返回 `(渲染结果, 是否发生了剪枝)`。
fn render_node(v: &Value, ctx: &RenderCtx) -> Result<(Option<Value>, bool), AccessError> {
    match v {
        Value::String(s) => render_string(s, ctx),
        Value::Array(items) => {
            let mut out = Vec::new();
            let mut pruned = false;
            for item in items {
                // ⭐ 整个字符串就是一个占位符、且值是数组 ⇒ **展开**进数组
                //   （`"Values": ["{ids}"]` + 多个 id ⇒ `["a","b"]`，而不是 `[["a","b"]]`）
                if let Value::String(s) = item {
                    if let Some(name) = whole_placeholder(s) {
                        match ctx.resolve(&name)? {
                            Some(Value::Array(vs)) => {
                                out.extend(vs);
                                continue;
                            }
                            Some(other) => {
                                out.push(other);
                                continue;
                            }
                            None => {
                                pruned = true;
                                continue;
                            }
                        }
                    }
                }
                let (rendered, had) = render_node(item, ctx)?;
                pruned |= had;
                if had {
                    // 数组元素子树里有剪枝 ⇒ 丢掉整个元素（规则见 `build_payload` 的文档）
                    continue;
                }
                out.push(rendered.unwrap_or(Value::Null));
            }
            if out.is_empty() && pruned {
                return Ok((None, true));
            }
            Ok((Some(Value::Array(out)), pruned))
        }
        Value::Object(map) => {
            let mut out = Map::new();
            let mut pruned = false;
            for (k, child) in map {
                let (rendered, had) = render_node(child, ctx)?;
                pruned |= had;
                if let Some(x) = rendered {
                    out.insert(k.clone(), x);
                }
            }
            if out.is_empty() && pruned {
                return Ok((None, true));
            }
            Ok((Some(Value::Object(out)), pruned))
        }
        other => Ok((Some(other.clone()), false)),
    }
}

/// 渲染一个字符串值（整个值是一个占位符 ⇒ 保持原类型；嵌在文本里 ⇒ 只是替换文字）。
fn render_string(s: &str, ctx: &RenderCtx) -> Result<(Option<Value>, bool), AccessError> {
    let names = placeholders(s);
    if names.is_empty() {
        return Ok((Some(Value::String(s.to_string())), false));
    }
    if let Some(name) = whole_placeholder(s) {
        // 整值占位符：保留原类型（字符串 / 数组 / 数字 / 布尔）
        return match ctx.resolve(&name)? {
            Some(v) => Ok((Some(v), false)),
            None => Ok((None, true)),
        };
    }
    // 嵌在文本里：只能替换成**文本**（多值 / 数组 ⇒ 报错，⛔ 不拼逗号猜）
    let mut out = s.to_string();
    for name in names {
        let Some(v) = ctx.resolve(&name)? else {
            return Err(AccessError::new(
                CODE_REQUEST_BINDING_MISSING,
                format!(
                    "模板里的 `{s}` 把占位符 `{{{name}}}` **嵌在文本里**，而这次没有值。\n\
                     改法：① 让它取到值；② 或把整个字符串改成 `{{{name}}}`\
                     （整值占位符，缺值时会被整段剪掉）。\n\
                     ⛔ 本实现不会把「缺值」写成半截字符串（那会发出一个看起来像数据的空值）。"
                ),
            ));
        };
        let text = match v {
            Value::String(x) => x,
            Value::Number(n) => n.to_string(),
            Value::Bool(b) => b.to_string(),
            other => {
                return Err(AccessError::new(
                    CODE_REQUEST_BINDING_MISSING,
                    format!(
                        "占位符 `{{{name}}}` 嵌在文本 `{s}` 里，但它的值是 {other}（数组/对象）——\
                         嵌不进去。改法：把它挪成**整值占位符**（整个字符串就是 `{{{name}}}`）。"
                    ),
                ))
            }
        };
        out = out.replace(&format!("{{{name}}}"), &text);
    }
    Ok((Some(Value::String(out)), false))
}

/// 字符串**整体**就是一个占位符时返回名字（`"{zone}"` ⇒ `Some("zone")`）。
fn whole_placeholder(s: &str) -> Option<String> {
    let t = s.trim();
    if !(t.starts_with('{') && t.ends_with('}')) {
        return None;
    }
    let inner = &t[1..t.len() - 1];
    if inner.is_empty() || inner.contains('{') || inner.contains('}') {
        return None;
    }
    Some(inner.to_string())
}

/// 按**点分路径**把值写进 JSON（中间层不存在就建对象；存在但不是对象 ⇒ 契约错）。
fn set_path(body: &mut Value, path: &str, value: Value) -> Result<(), AccessError> {
    let segs: Vec<&str> = path.split('.').filter(|s| !s.is_empty()).collect();
    if segs.is_empty() {
        return Err(AccessError::new(
            CODE_SCHEMA,
            "分页路径是空的（`paging.offset` / `.limit` 要写成 `Offset` 或 `Page.Number`）。"
                .to_string(),
        ));
    }
    if !body.is_object() {
        *body = Value::Object(Map::new());
    }
    let mut cur = body;
    for seg in &segs[..segs.len() - 1] {
        let obj = cur.as_object_mut().expect("上面已保证是对象");
        let entry = obj.entry(seg.to_string()).or_insert_with(|| Value::Object(Map::new()));
        if !entry.is_object() {
            return Err(AccessError::new(
                CODE_SCHEMA,
                format!(
                    "分页路径 `{path}` 的中间段 `{seg}` 在模板里已经是一个非对象值 —— \
                     分页值写不进去。改法：把该路径换成模板里空着的位置，或调整 `paging` 路径。"
                ),
            ));
        }
        cur = entry;
    }
    let last = segs[segs.len() - 1];
    cur.as_object_mut()
        .expect("上面已保证是对象")
        .insert(last.to_string(), value);
    Ok(())
}

/// 过滤值 → **标量**文本（多值 ⇒ 契约错，⛔ 不静默 join、不挑第一个）。
fn value_as_text(v: &FilterValue) -> Result<String, AccessError> {
    match v {
        FilterValue::One(s) => Ok(s.clone()),
        FilterValue::Many(items) => Err(AccessError::new(
            CODE_SCHEMA,
            format!(
                "过滤值 {items:?} 是多值，但契约把它映射到了**标量**位置\
                 （`X-TC-Region` 头，或嵌在文本里的占位符）。\n\
                 改法：① 过滤值改传单值；② 或把它写成**整值占位符**\
                 （`\"{{name}}\"`，多值会渲染成数组）。⛔ 本实现不会替你挑一个值。"
            ),
        )),
    }
}

/// 旧口径（没有模板时）的过滤值 → 顶层参数 JSON（单值 ⇒ 字符串，多值 ⇒ 数组）。
fn filter_value_json(v: &FilterValue) -> Value {
    match v {
        FilterValue::One(s) => json!(s),
        FilterValue::Many(items) => json!(items),
    }
}

/// 总行数（`paging.total_path` 指到的数字）。
fn total_from(doc: &Value, entity: &Entity) -> Option<usize> {
    entity
        .paging
        .as_ref()
        .and_then(|p| p.total_path.as_ref())
        .and_then(|tp| path::select(doc, tp))
        .and_then(|v| v.as_u64())
        .map(|n| n as usize)
}

/// `https://api.example.com/x` → `api.example.com`（含非默认端口）。
fn host_of(url: &str) -> String {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    rest.split('/').next().unwrap_or(rest).to_string()
}

/// `api.example.com` → `cvm`（文档：service 必须与产品域名一致）。
fn service_of(host: &str) -> String {
    host.split('.').next().unwrap_or(host).to_string()
}

// ══════════════════════════════════════════════════════════════
// 传输层错误 → 码 的映射（复审点名的边界）
// ══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://api.example.com/";

    /// ⭐ **连接阶段**超时 = 根本没连上 ⇒ `UPSTREAM_UNREACHABLE`（⛔ 不许报成 TIMEOUT）。
    ///
    /// 判据的理由是**运维动作**：看到 TIMEOUT 会去调 `--timeout-secs`（治不好端口没开）；
    /// 看到 UNREACHABLE 才会去查安全组 / 端口 / DNS。
    #[test]
    fn connect_stage_timeout_is_unreachable_not_timeout() {
        for kind in [ureq::Timeout::Connect, ureq::Timeout::Resolve] {
            let (retryable, e) = transport_error(ureq::Error::Timeout(kind), URL);
            assert!(retryable, "连接类失败可重试（可能是瞬时抖动）");
            assert_eq!(e.code, CODE_UPSTREAM_UNREACHABLE, "{kind:?} 应当是不可达：{e}");
            assert!(e.message.contains("连接没建立"), "要把话说死：{e}");
            assert!(e.message.contains("安全组"), "要给出该查什么：{e}");
            assert!(e.message.contains("--dry-run-fixture"), "要给出不用网络的那条路：{e}");
        }
    }

    /// 连上了但**等太久** ⇒ `UPSTREAM_TIMEOUT`。
    #[test]
    fn wait_stage_timeout_is_timeout() {
        for kind in [
            ureq::Timeout::Global,
            ureq::Timeout::RecvResponse,
            ureq::Timeout::SendRequest,
            ureq::Timeout::RecvBody,
        ] {
            let (retryable, e) = transport_error(ureq::Error::Timeout(kind), URL);
            assert!(retryable, "超时可重试（有上限）");
            assert_eq!(e.code, CODE_UPSTREAM_TIMEOUT, "{kind:?} 应当是超时：{e}");
            assert!(e.message.contains("--timeout-secs"), "要给出可调的旋钮：{e}");
        }
    }

    /// 连接被拒 / DNS 失败等 IO 错误 ⇒ `UPSTREAM_UNREACHABLE`。
    #[test]
    fn io_failures_are_unreachable() {
        let io = std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "connection refused");
        let (retryable, e) = transport_error(ureq::Error::Io(io), URL);
        assert!(retryable);
        assert_eq!(e.code, CODE_UPSTREAM_UNREACHABLE, "{e}");
        assert!(e.message.contains("端口"), "要给出该查什么：{e}");
        assert!(!e.message.contains("--timeout-secs"), "⛔ 不可达不该建议调超时：{e}");
    }

    /// 凭据值**不进**错误信息（只回显变量名）—— 这条在 `Credentials` 侧也测了，
    /// 这里再钉一次传输层：任何 `AccessError` 的 message 都不该带出 `secret_key`。
    #[test]
    fn transport_errors_never_echo_credentials() {
        let io = std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused");
        let (_, e) = transport_error(ureq::Error::Io(io), URL);
        assert!(!e.message.contains("SECRET_KEY="), "{}", e.message);
    }
}

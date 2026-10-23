//! REQ-126 · **客户门户的凭据表** —— 一张文件、一种身份：`kind = "portal"`。
//!
//! ```json
//! {
//!   "version": "1.0.0",
//!   "tokens": [
//!     {"label": "customer-a", "token": "REPLACE-WITH-YOUR-OWN-PORTAL-TOKEN",
//!      "kind": "portal", "tenant": "c51dc886-…", "max_rows": 200, "max_concurrency": 2}
//!   ]
//! }
//! ```
//!
//! # 为什么和 `admin-tokens.json` 不共用一个文件
//!
//! 门户是**第三个**服务（`portal serve`），凭据来源也必须是第三份：
//! 客户的访问口令一旦与 `admin` / `query` 共用一张表，任何一次「顺手把客户口令加进
//! 运维表」都会把客户凭据变成管理凭据的邻居 —— 而邻居就是这个仓库里所有
//! 「同一张表里两种身份」事故的起点（REQ-124 的 R1 正是为此）。
//!
//! # `kind` 的互斥是**加载期**判的，不是运行期
//!
//! | 情形 | 结果 |
//! | --- | --- |
//! | `kind = "portal"` 且 `tenant` 非空 | 接受 |
//! | `kind = "admin"` / `"query"` | **整份拒绝**：这是管理 / 查询凭据，不能当门户口令用 |
//! | `kind` 是别的值 | **整份拒绝**：不认识的 kind 不许默认成 portal |
//! | 同一个 token 出现两次（不论 kind） | **整份拒绝**：一个 token 只能有一个身份 |
//! | 缺 `kind` | **整份拒绝**：`kind` 没有默认值 |
//!
//! 这与 REQ-124 的 `console::tokens` 是**同一条纪律的两份实现**（不是复制粘贴）：
//! 那边一张表装两种身份、用 `TokenKind` 分开；这边一张表只装一种身份，
//! 于是「混进来」只能报错。⛔ 本模块不 import `console::tokens` ——
//! 两个服务共用一个 token 解析器，会让「门户读到管理表的字段」变成一个可编译的写法。
//!
//! # 两个限额字段（都真的生效）
//!
//! | 字段 | 缺省 | 作用 | 超了会怎样 |
//! | --- | --- | --- | --- |
//! | `max_rows` | **200**（上限 10000） | 单次问题最多几行 | 截断 + 页面写「只显示前 N 行」 |
//! | `max_concurrency` | **2**（上限 64） | 同一口令同时最多几个问题在跑 | **429 `TOO_MANY_CONCURRENT`**（[`crate::portal::limit`]） |
//!
//! 两者都是**口令级硬上限**：请求里没有对应字段（带了 ⇒ 400「不认识的字段」），
//! 所以调用方**抬不高**。缺省值与 REQ-123 的 `tokens.json` 逐字一致。
//!
//! # 已知边界（⛔ 不写成「已封堵」）
//!
//! - 表是**启动时读一次**的（不热加载）：改口令要重启（与 REQ-123/124 同一条口径）；
//! - 比较是定长的（[`PortalTokens::find`]），但进程内存里有明文（同一条边界）；
//! - **跨文件**的互斥（同一个字符串既在 `admin-tokens.json` 又在
//!   `portal-tokens.json`）本模块看不见：两份文件由两次启动分别加载。
//!   真正的边界是「两份文件由不同的人保管 + 不同的服务端口」，已登记为已知限制。

use serde::Deserialize;

use crate::query::audit::{digest_prefix, sha256_hex};
use crate::query::tokens::{ct_eq, MAX_CONCURRENCY_CEILING, MAX_ROWS_CEILING, MIN_TOKEN_LEN};

/// 门户凭据的唯一合法 `kind`。
pub const PORTAL_KIND: &str = "portal";

/// `max_rows` 的默认值（与 REQ-123 的 `tokens.json` 同一个默认）。
fn default_max_rows() -> u64 {
    200
}

/// `max_concurrency` 的默认值。
fn default_max_concurrency() -> usize {
    2
}

/// `portal-tokens.json` 的顶层形状（`deny_unknown_fields`：拼错即报，不静默）。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct TokenFile {
    version: String,
    tokens: Vec<TokenSpec>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct TokenSpec {
    token: String,
    /// ⛔ **必填**。缺了它 serde 会报 `missing field kind` —— 那正是我们要的：
    /// 「这张口令是干什么的」不许靠猜（与 REQ-124 同一条）。
    kind: String,
    #[serde(default)]
    tenant: Option<String>,
    #[serde(default = "default_max_rows")]
    max_rows: u64,
    #[serde(default = "default_max_concurrency")]
    max_concurrency: usize,
    #[serde(default)]
    label: Option<String>,
}

/// 一条**已校验**的门户口令。
#[derive(Debug, Clone)]
pub struct PortalTokenEntry {
    pub token: String,
    /// `sha256(token)` 全长（审计只用前缀）。
    pub digest: String,
    /// 这张口令**唯一**能看到的租户 —— 门户身份的全部来源。
    pub tenant: String,
    /// 每次查询的行数上限（口令级；调用方抬不高）。
    pub max_rows: u64,
    /// **同一口令同时最多几个问题在跑**（口令级；不填 ⇒ 默认 2）。
    ///
    /// 由 [`crate::portal::limit`] 执行：名额用完 ⇒ 第 N+1 个请求**立刻** 429，
    /// ⛔ 不排队。名额按口令隔离（A 打满不影响 B）。
    pub max_concurrency: usize,
    pub label: Option<String>,
}

impl PortalTokenEntry {
    /// 审计里写的口令摘要（`sha256:` + 12 位十六进制；与 REQ-123/124 同一个形状）。
    pub fn audit_digest(&self) -> String {
        format!("sha256:{}", digest_prefix(&self.digest, 12))
    }
}

/// 已加载的门户口令表。
#[derive(Debug, Clone, Default)]
pub struct PortalTokens {
    entries: Vec<PortalTokenEntry>,
}

impl PortalTokens {
    /// 解析 + 校验。任何一条不合格 ⇒ **整份拒绝**（fail-closed）。
    pub fn parse(json: &str) -> Result<Self, String> {
        let doc: TokenFile = serde_json::from_str(json).map_err(|e| {
            format!(
                "portal-tokens.json 不是合法形状：{e}\n  期望 \
                 {{version, tokens:[{{token, kind:\"portal\", tenant, max_rows?, max_concurrency?, label?}}]}}"
            )
        })?;

        let major = doc.version.trim().split('.').next().unwrap_or("");
        if major != "1" {
            return Err(format!(
                "portal-tokens.json 的 version = {:?}：本实现只认 1.x（升级格式要配一次迁移）",
                doc.version
            ));
        }
        if doc.tokens.is_empty() {
            return Err(
                "portal-tokens.json 里一条口令都没有 —— 没有人能通过鉴权，服务没有意义".to_string(),
            );
        }

        let mut entries: Vec<PortalTokenEntry> = Vec::with_capacity(doc.tokens.len());
        let mut problems: Vec<String> = Vec::new();
        for (i, t) in doc.tokens.iter().enumerate() {
            let at = match t.label.as_deref() {
                Some(l) if !l.trim().is_empty() => format!("第 {} 条（label={l:?}）", i + 1),
                _ => format!("第 {} 条（没有 label）", i + 1),
            };
            let token = t.token.trim();
            // ── kind：三种拒绝各有各的话（运维要能一眼看出该改哪行）──
            match t.kind.trim() {
                PORTAL_KIND => {}
                other @ ("admin" | "query") => problems.push(format!(
                    "{at} 的 kind = {other:?} —— ⛔ 管理 / 查询凭据**不能**当门户口令用。\
                     门户只认 kind = \"portal\"；同一个 token 不许两用（两套凭据 = 两套文件）"
                )),
                other => problems.push(format!(
                    "{at} 的 kind = {other:?} 不认识 —— 只认 \"portal\"（⛔ 没有默认值）"
                )),
            }
            if token.len() < MIN_TOKEN_LEN {
                problems.push(format!(
                    "{at} 的 token 只有 {} 字符，少于 {MIN_TOKEN_LEN} —— 低熵口令等于没有鉴权",
                    token.len()
                ));
            }
            if token.chars().any(char::is_whitespace) {
                problems.push(format!("{at} 的 token 里含空白字符"));
            }
            let tenant = t
                .tenant
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            if tenant.is_none() {
                problems.push(format!(
                    "{at} 没有 tenant —— ⛔ 门户的租户只能来自口令；空租户等于「没有隔离」"
                ));
            }
            if t.max_rows == 0 {
                problems.push(format!("{at} 的 max_rows 是 0：0 行等于「什么都没查」"));
            }
            if t.max_rows > MAX_ROWS_CEILING {
                problems.push(format!(
                    "{at} 的 max_rows = {} 超过硬上限 {MAX_ROWS_CEILING}",
                    t.max_rows
                ));
            }
            if t.max_concurrency == 0 {
                problems.push(format!("{at} 的 max_concurrency 是 0：那等于禁止调用"));
            }
            if t.max_concurrency > MAX_CONCURRENCY_CEILING {
                problems.push(format!(
                    "{at} 的 max_concurrency = {} 超过硬上限 {MAX_CONCURRENCY_CEILING}",
                    t.max_concurrency
                ));
            }
            // ⛔ 同一个 token 不许出现两次（不论 kind）—— 「一 token 一身份」。
            if entries.iter().any(|e| e.token == token) {
                problems.push(format!(
                    "{at} 的 token 与前面的重复 —— ⛔ 同一个 token 不许两用（一个 token 一个身份）"
                ));
            }
            if let Some(tenant) = tenant {
                entries.push(PortalTokenEntry {
                    token: token.to_string(),
                    digest: sha256_hex(token.as_bytes()),
                    tenant,
                    max_rows: t.max_rows,
                    max_concurrency: t.max_concurrency,
                    label: t.label.clone(),
                });
            }
        }
        if !problems.is_empty() {
            return Err(format!(
                "portal-tokens.json 有 {} 处不合法：\n  - {}",
                problems.len(),
                problems.join("\n  - ")
            ));
        }
        Ok(Self { entries })
    }

    /// 按来源取一条（**只认调用方给出的字符串**；本模块不读 query string / cookie）。
    ///
    /// 定长比较、不早退（与 REQ-123/124 同一算法、同一条已知边界：长度不同会立刻返回）。
    pub fn find(&self, presented: &str) -> Option<&PortalTokenEntry> {
        let presented = presented.trim();
        if presented.is_empty() {
            return None;
        }
        let mut hit: Option<usize> = None;
        for (i, e) in self.entries.iter().enumerate() {
            if ct_eq(presented.as_bytes(), e.token.as_bytes()) {
                hit = Some(i);
            }
        }
        hit.map(|i| &self.entries[i])
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 口令表覆盖的租户个数（启动横幅用 —— ⛔ 不打印租户 id 本身）。
    pub fn tenant_count(&self) -> usize {
        let mut ts: Vec<&str> = self.entries.iter().map(|e| e.tenant.as_str()).collect();
        ts.sort_unstable();
        ts.dedup();
        ts.len()
    }

    /// 表里最大的 `max_rows`（横幅用；每次调用仍按**各自**口令的上限执行）。
    pub fn max_rows_cap(&self) -> u64 {
        self.entries.iter().map(|e| e.max_rows).max().unwrap_or(0)
    }

    /// 表里最大的 `max_concurrency`（横幅用；并发名额仍按**各自**口令的上限算）。
    pub fn max_concurrency_cap(&self) -> usize {
        self.entries
            .iter()
            .map(|e| e.max_concurrency)
            .max()
            .unwrap_or(0)
    }
}

// ══════════════════════════════════════════════════════════════
// 判据
// ══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "FAKE-PORTAL-TOKEN-aaaaaaaaaaaaaaaa";
    const B: &str = "FAKE-PORTAL-TOKEN-bbbbbbbbbbbbbbbbbbbb";

    fn good() -> String {
        format!(
            r#"{{"version":"1.0.0","tokens":[
                {{"label":"cust-a","token":"{A}","kind":"portal","tenant":"T-A","max_rows":200}},
                {{"label":"cust-b","token":"{B}","kind":"portal","tenant":"T-B"}}
            ]}}"#
        )
    }

    #[test]
    fn a_valid_table_parses_and_finds_by_token_only() {
        let t = PortalTokens::parse(&good()).expect("合法表");
        assert_eq!(t.len(), 2);
        assert_eq!(t.tenant_count(), 2);
        assert_eq!(t.max_rows_cap(), 200);

        let a = t.find(A).expect("A 必须找得到");
        assert_eq!(a.tenant, "T-A");
        assert_eq!(a.max_rows, 200);
        assert_eq!(a.audit_digest().len(), "sha256:".len() + 12);
        // 默认值：B 没写 max_rows / max_concurrency。
        let b = t.find(B).expect("B 必须找得到");
        assert_eq!(b.max_rows, default_max_rows());
        assert_eq!(b.max_concurrency, default_max_concurrency());

        assert!(t.find("").is_none());
        assert!(t.find("FAKE-PORTAL-TOKEN-cccccccccccccccc").is_none());
    }

    /// ⭐ 与 `admin` / `query` 的**加载期**互斥（AC-006）。
    #[test]
    fn admin_and_query_kinds_are_rejected_by_name() {
        for kind in ["admin", "query"] {
            let raw = good().replace("\"kind\":\"portal\"", &format!("\"kind\":\"{kind}\""));
            let e = PortalTokens::parse(&raw).unwrap_err();
            assert!(
                e.contains("不能") && e.contains("两用"),
                "kind={kind} 必须点名「不能两用」：{e}"
            );
        }
        // 不认识的 kind 不许默认成 portal。
        let raw = good().replace("\"kind\":\"portal\"", "\"kind\":\"root\"");
        let e = PortalTokens::parse(&raw).unwrap_err();
        assert!(e.contains("不认识") && e.contains("portal"), "{e}");
    }

    #[test]
    fn a_missing_kind_is_an_error_not_a_portal_default() {
        let raw = format!(
            r#"{{"version":"1.0.0","tokens":[{{"token":"{A}","tenant":"T-A"}}]}}"#
        );
        let e = PortalTokens::parse(&raw).unwrap_err();
        assert!(e.contains("kind"), "缺 kind 必须点名：{e}");
    }

    #[test]
    fn one_token_cannot_be_registered_twice() {
        let raw = format!(
            r#"{{"version":"1.0.0","tokens":[
                {{"token":"{A}","kind":"portal","tenant":"T-A"}},
                {{"token":"{A}","kind":"portal","tenant":"T-B"}}
            ]}}"#
        );
        let e = PortalTokens::parse(&raw).unwrap_err();
        assert!(e.contains("两用") && e.contains("重复"), "{e}");
    }

    #[test]
    fn a_token_without_a_tenant_is_rejected() {
        let raw = format!(
            r#"{{"version":"1.0.0","tokens":[{{"token":"{A}","kind":"portal"}}]}}"#
        );
        let e = PortalTokens::parse(&raw).unwrap_err();
        assert!(e.contains("tenant") && e.contains("只能来自口令"), "{e}");

        let raw = format!(
            r#"{{"version":"1.0.0","tokens":[{{"token":"{A}","kind":"portal","tenant":"   "}}]}}"#
        );
        assert!(PortalTokens::parse(&raw).unwrap_err().contains("tenant"));
    }

    #[test]
    fn short_tokens_and_absurd_limits_are_rejected() {
        let raw = format!(r#"{{"version":"1.0.0","tokens":[{{"token":"short","kind":"portal","tenant":"T"}}]}}"#);
        assert!(PortalTokens::parse(&raw).unwrap_err().contains("少于 24"));

        let raw = good().replace("\"max_rows\":200", "\"max_rows\":0");
        assert!(PortalTokens::parse(&raw).unwrap_err().contains("max_rows 是 0"));

        let raw = good().replace("\"max_rows\":200", "\"max_rows\":1000001");
        assert!(PortalTokens::parse(&raw).unwrap_err().contains("硬上限"));

        let raw = good().replace("\"max_rows\":200", "\"max_rows\":200,\"max_concurrency\":0");
        assert!(PortalTokens::parse(&raw).unwrap_err().contains("禁止调用"));
    }

    #[test]
    fn an_unknown_field_is_rejected_not_ignored() {
        let raw = good().replace("\"label\":\"cust-a\"", "\"label\":\"cust-a\",\"admin\":true");
        let e = PortalTokens::parse(&raw).unwrap_err();
        assert!(e.contains("admin"), "{e}");
    }

    #[test]
    fn wrong_version_or_empty_table_is_rejected() {
        let raw = good().replace("\"1.0.0\"", "\"2.0.0\"");
        assert!(PortalTokens::parse(&raw).unwrap_err().contains("只认 1.x"));

        let raw = r#"{"version":"1.0.0","tokens":[]}"#;
        assert!(PortalTokens::parse(raw).unwrap_err().contains("一条口令都没有"));
    }
}

//! REQ-124 · **控制面的 token 表** —— 一张文件装**两套**凭据：`admin` 与 `query`。
//!
//! # R1 的原文与它的落点
//!
//! > `admin` 与 `query` **分开**；query token 调管理端点 ⇒ **403**。⛔ 不许一个 token 两用。
//!
//! 「两个文件」本身回答不了 403：控制面读到一张**不认识的** token 时，只有两种可能 ——
//! 它是一张 query token（⇒ 403），或者它什么都不是（⇒ 401）。要区分这两者，
//! 控制面**必须**知道 query 集合是什么。所以本表把两套写在**同一个文件**里，用
//! [`TokenKind`] 分开：
//!
//! ```json
//! {
//!   "version": "1.0.0",
//!   "tokens": [
//!     {"label": "ops-laptop", "token": "REPLACE-WITH-YOUR-OWN-ADMIN-TOKEN", "kind": "admin"},
//!     {"label": "agent-a", "token": "REPLACE-WITH-YOUR-OWN-QUERY-TOKEN", "kind": "query", "tenant": "C0001"}
//!   ]
//! }
//! ```
//!
//! - `kind` **必填**（没有默认值）：默认成 `admin` 会让一张查数 token 悄悄拿到管理权；
//! - 同一个 token 出现两次（**不论 kind**）⇒ 整份拒绝：那正是「一个 token 两用」；
//! - `kind = query` 必须带非空 `tenant`（与 REQ-123 的 `tokens.json` 同一条要求）；
//! - token ≥ 24 字符（与 REQ-123 的 `MIN_TOKEN_LEN` 同一条理由：低熵口令在摘要面前守不住）。
//!
//! # 已知边界
//!
//! - 表是**启动时读一次**的（不热加载）：改 token 要重启 —— 与 REQ-123 同一条口径；
//! - 比较是定长的（[`AdminTokens::find`]），但进程内存里有明文（与 REQ-123 同一条边界）；
//! - 本表里的 `query` 条目**只用于识别 403**，控制面从不拿它做任何查询 ——
//!   真正的查询服务读的是它自己的 `tokens.json`（两份文件、两套发放）。

use serde::Deserialize;

use crate::query::audit::{digest_prefix, sha256_hex};
use crate::query::tokens::MIN_TOKEN_LEN;

/// 一条 token 的**用途**。⛔ 没有「两者皆是」这个取值。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenKind {
    /// 管理端点。
    Admin,
    /// 查询端点（控制面只认得它、**不接受**它）。
    Query,
}

impl TokenKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            TokenKind::Admin => "admin",
            TokenKind::Query => "query",
        }
    }
}

/// `admin-tokens.json` 的顶层形状（`deny_unknown_fields`：拼错即报，不静默）。
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
    /// ⛔ 必填。缺了它 serde 会报 `missing field kind` —— 那正是我们要的：
    /// 「这张 token 是干什么的」不许靠猜。
    kind: TokenKind,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    tenant: Option<String>,
}

/// 一条**已校验**的 token。
#[derive(Debug, Clone)]
pub struct TokenEntry {
    pub token: String,
    /// `sha256(token)` 全长（审计只用前缀）。
    pub digest: String,
    pub kind: TokenKind,
    pub label: Option<String>,
    pub tenant: Option<String>,
}

impl TokenEntry {
    /// 审计里写的 token 摘要（`sha256:` + 12 位十六进制；与 REQ-123 同一个形状）。
    pub fn audit_digest(&self) -> String {
        format!("sha256:{}", digest_prefix(&self.digest, 12))
    }
}

/// 已加载的两套 token。
#[derive(Debug, Clone, Default)]
pub struct AdminTokens {
    entries: Vec<TokenEntry>,
}

impl AdminTokens {
    /// 解析 + 校验。任何一条不合格 ⇒ **整份拒绝**（fail-closed）。
    pub fn parse(json: &str) -> Result<Self, String> {
        let doc: TokenFile = serde_json::from_str(json).map_err(|e| {
            format!(
                "admin-tokens.json 不是合法形状：{e}\n  期望 \
                 {{version, tokens:[{{token, kind:\"admin\"|\"query\", label?, tenant?}}]}}"
            )
        })?;

        let major = doc.version.trim().split('.').next().unwrap_or("");
        if major != "1" {
            return Err(format!(
                "admin-tokens.json 的 version = {:?}：本实现只认 1.x（升级格式要配一次迁移）",
                doc.version
            ));
        }
        if doc.tokens.is_empty() {
            return Err("admin-tokens.json 里一条 token 都没有 —— 没有人能通过鉴权".to_string());
        }

        let mut entries: Vec<TokenEntry> = Vec::with_capacity(doc.tokens.len());
        let mut problems: Vec<String> = Vec::new();
        let mut has_admin = false;
        for (i, t) in doc.tokens.iter().enumerate() {
            let at = format!("第 {} 条（label={:?}）", i + 1, t.label);
            let token = t.token.trim();
            if token.len() < MIN_TOKEN_LEN {
                problems.push(format!(
                    "{at} 的 token 只有 {} 字符，少于 {MIN_TOKEN_LEN} —— 低熵口令等于没有鉴权",
                    token.len()
                ));
            }
            if token.chars().any(char::is_whitespace) {
                problems.push(format!("{at} 的 token 里含空白字符"));
            }
            // ⛔ R1：一个 token 只能有一个身份。重复（含跨 kind）⇒ 整份拒绝。
            if entries.iter().any(|e| e.token == token) {
                problems.push(format!(
                    "{at} 的 token 与前面的重复 —— ⛔ 同一个 token 不许两用（admin / query 必须分开）"
                ));
            }
            let tenant = t
                .tenant
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            if t.kind == TokenKind::Query && tenant.is_none() {
                problems.push(format!(
                    "{at} 的 kind = query 却没有 tenant —— 查询 token 必须绑定一个租户"
                ));
            }
            if t.kind == TokenKind::Admin {
                has_admin = true;
            }
            if !token.is_empty() {
                entries.push(TokenEntry {
                    token: token.to_string(),
                    digest: sha256_hex(token.as_bytes()),
                    kind: t.kind,
                    label: t.label.clone(),
                    tenant,
                });
            }
        }
        if !has_admin {
            problems.push(
                "admin-tokens.json 里一张 admin token 都没有 —— 管理端点将无人可用".to_string(),
            );
        }
        if !problems.is_empty() {
            return Err(format!(
                "admin-tokens.json 有 {} 处不合法：\n  - {}",
                problems.len(),
                problems.join("\n  - ")
            ));
        }
        Ok(Self { entries })
    }

    /// 按来源取一条（**只认调用方给出的字符串**；本模块不读 query string / cookie）。
    ///
    /// 定长比较、不早退（与 REQ-123 的 `TokenTable::find` 同一算法、同一条已知边界：
    /// 长度不同会立刻返回，所以**长度**是可见的）。
    pub fn find(&self, presented: &str) -> Option<&TokenEntry> {
        let presented = presented.trim();
        if presented.is_empty() {
            return None;
        }
        let mut hit: Option<usize> = None;
        for (i, e) in self.entries.iter().enumerate() {
            if crate::query::tokens::ct_eq(presented.as_bytes(), e.token.as_bytes()) {
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

    pub fn admin_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.kind == TokenKind::Admin)
            .count()
    }

    pub fn query_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.kind == TokenKind::Query)
            .count()
    }
}

// ══════════════════════════════════════════════════════════════
// 判据
// ══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    const ADMIN: &str = "FAKE-ADMIN-TOKEN-aaaaaaaaaaaaaaaa";
    const QUERY: &str = "FAKE-QUERY-TOKEN-bbbbbbbbbbbbbbbbbbbb";

    fn good() -> String {
        format!(
            r#"{{"version":"1.0.0","tokens":[
                {{"label":"ops","token":"{ADMIN}","kind":"admin"}},
                {{"label":"agent-a","token":"{QUERY}","kind":"query","tenant":"C0001"}}
            ]}}"#
        )
    }

    #[test]
    fn the_two_sets_are_recognized_separately() {
        let t = AdminTokens::parse(&good()).expect("合法表");
        assert_eq!(t.len(), 2);
        assert_eq!(t.admin_count(), 1);
        assert_eq!(t.query_count(), 1);

        let a = t.find(ADMIN).expect("admin 必须找得到");
        assert_eq!(a.kind, TokenKind::Admin);
        let q = t.find(QUERY).expect("query 必须找得到（用于区分 403）");
        assert_eq!(q.kind, TokenKind::Query);
        assert_eq!(q.tenant.as_deref(), Some("C0001"));
        assert_eq!(a.audit_digest().len(), "sha256:".len() + 12);
        // 不认识的字符串 ⇒ None（401），不是 403。
        assert!(t.find("FAKE-NOBODY-TOKEN-cccccccccccccccc").is_none());
    }

    #[test]
    fn a_missing_kind_is_an_error_not_an_admin_default() {
        let raw = format!(r#"{{"version":"1.0.0","tokens":[{{"token":"{ADMIN}"}}]}}"#);
        let e = AdminTokens::parse(&raw).unwrap_err();
        assert!(e.contains("kind"), "缺 kind 必须点名：{e}");
    }

    #[test]
    fn one_token_cannot_be_both_admin_and_query() {
        let raw = format!(
            r#"{{"version":"1.0.0","tokens":[
                {{"token":"{ADMIN}","kind":"admin"}},
                {{"token":"{ADMIN}","kind":"query","tenant":"C0001"}}
            ]}}"#
        );
        let e = AdminTokens::parse(&raw).unwrap_err();
        assert!(e.contains("两用") || e.contains("重复"), "{e}");
    }

    #[test]
    fn a_query_token_without_a_tenant_is_rejected() {
        let raw = format!(
            r#"{{"version":"1.0.0","tokens":[
                {{"token":"{ADMIN}","kind":"admin"}},
                {{"token":"{QUERY}","kind":"query"}}
            ]}}"#
        );
        let e = AdminTokens::parse(&raw).unwrap_err();
        assert!(e.contains("tenant"), "{e}");
    }

    #[test]
    fn a_short_or_missing_admin_token_is_rejected() {
        let raw = format!(r#"{{"version":"1.0.0","tokens":[{{"token":"short","kind":"admin"}}]}}"#);
        assert!(AdminTokens::parse(&raw).unwrap_err().contains("少于 24"));

        let raw = format!(
            r#"{{"version":"1.0.0","tokens":[{{"token":"{QUERY}","kind":"query","tenant":"C1"}}]}}"#
        );
        assert!(
            AdminTokens::parse(&raw)
                .unwrap_err()
                .contains("一张 admin token 都没有"),
            "没有 admin 的表必须拒绝"
        );
    }

    #[test]
    fn an_unknown_kind_or_field_is_rejected() {
        let raw = format!(
            r#"{{"version":"1.0.0","tokens":[{{"token":"{ADMIN}","kind":"root"}}]}}"#
        );
        let e = AdminTokens::parse(&raw).unwrap_err();
        assert!(e.contains("unknown variant") || e.contains("kind"), "{e}");

        let raw = format!(
            r#"{{"version":"1.0.0","tokens":[{{"token":"{ADMIN}","kind":"admin","admin":true}}]}}"#
        );
        assert!(AdminTokens::parse(&raw).unwrap_err().contains("admin"));
    }

    #[test]
    fn a_wrong_version_is_rejected() {
        let raw = good().replace("\"1.0.0\"", "\"2.0.0\"");
        assert!(AdminTokens::parse(&raw).unwrap_err().contains("只认 1.x"));
    }
}

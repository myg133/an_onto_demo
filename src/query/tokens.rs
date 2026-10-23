//! REQ-123 · **静态 token 表** —— `token → {tenant, max_rows, 并发上限}`。
//!
//! # 为什么租户只能从这张表来（R1）
//!
//! 请求体是**调用方可写**的，token 表是**运维可写**的。把「我是谁」放在请求体里，
//! 等于让调用方自己声明租户 —— 那不是隔离，是「请自便」。
//! 所以：`Authorization: Bearer <token>` → 本表 → `{tenant, 限额}`，
//! 请求体里的 `tenant` 只用于**比对**（不一致 ⇒ 403 `TENANT_MISMATCH`）。
//!
//! # 文件形状（`tokens.json`）
//!
//! ```json
//! {
//!   "version": "1.0.0",
//!   "tokens": [
//!     {
//!       "label": "agent-a",
//!       "token": "REPLACE-WITH-YOUR-OWN-RANDOM-TOKEN",
//!       "tenant": "C0001",
//!       "max_rows": 200,
//!       "max_concurrency": 2
//!     }
//!   ]
//! }
//! ```
//!
//! - `token` **≥ 24 字符**（低熵口令在审计摘要面前也守不住，见 [`super::audit`]）；
//! - `tenant` 非空（空租户 = 没有隔离）；
//! - `max_rows` 默认 200、上限 10000（这是**调用方永远抬不高**的那条线）；
//! - `max_concurrency` 默认 2、上限 64。
//!
//! # 严格解析（`deny_unknown_fields`）
//!
//! 拼错一个字段名（`"max_row"`）如果被宽容忽略，那份表就会「看起来配了限额、
//! 实际用的是默认值」—— 与 REQ-122 的声明同一个理由：**猜错 = 限额不存在，且不报错**。
//!
//! # 已知边界
//!
//! - 表是**启动时读一次**的（不热加载）：改 token 要重启。这是刻意的 ——
//!   热加载意味着「正在服务的进程」与「你读到的文件」可能不是同一份。
//! - 比较是**定长**的（[`TokenTable::find`]），但不做哈希存储：进程内存里有明文。
//!   换掉它需要 KMS/散列前缀索引，那是另一条需求。

use serde::Deserialize;

use super::audit::{digest_prefix, sha256_hex};

/// token 的最小长度。短于此值直接拒绝加载。
pub const MIN_TOKEN_LEN: usize = 24;
/// `max_rows` 的硬上限（防止一张表把「限额」写成 100 万）。
pub const MAX_ROWS_CEILING: u64 = 10_000;
/// `max_concurrency` 的硬上限。
pub const MAX_CONCURRENCY_CEILING: usize = 64;

/// `tokens.json` 的顶层形状。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokenFile {
    pub version: String,
    pub tokens: Vec<TokenSpec>,
}

/// 一条 token 登记。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokenSpec {
    /// **假值示例**见模块头；真实 token 由运维生成（≥ 24 字符）。
    pub token: String,
    pub tenant: String,
    #[serde(default = "default_max_rows")]
    pub max_rows: u64,
    #[serde(default = "default_max_concurrency")]
    pub max_concurrency: usize,
    /// 人读的标签（进审计**不**出现原文；只进启动横幅的计数）。
    #[serde(default)]
    pub label: Option<String>,
}

fn default_max_rows() -> u64 {
    200
}

fn default_max_concurrency() -> usize {
    2
}

/// 一条**已校验**的 token（服务端内存里的形状）。
#[derive(Debug, Clone)]
pub struct TokenEntry {
    pub token: String,
    /// `sha256(token)` 全长（审计只用前缀；这里留全长是为了单测能对账）。
    pub digest: String,
    pub tenant: String,
    pub max_rows: u64,
    pub max_concurrency: usize,
    pub label: Option<String>,
}

impl TokenEntry {
    /// 审计里写的 token 摘要（`sha256:` + 12 位十六进制）。
    pub fn audit_digest(&self) -> String {
        format!("sha256:{}", digest_prefix(&self.digest, 12))
    }
}

/// 已加载的 token 表。
#[derive(Debug, Clone, Default)]
pub struct TokenTable {
    entries: Vec<TokenEntry>,
}

impl TokenTable {
    /// 解析 + 校验。任何一条不合格 ⇒ **整份拒绝**（fail-closed）。
    pub fn parse(json: &str) -> Result<Self, String> {
        let doc: TokenFile = serde_json::from_str(json)
            .map_err(|e| format!("tokens.json 不是合法形状：{e}"))?;

        let major = doc.version.trim().split('.').next().unwrap_or("");
        if major != "1" {
            return Err(format!(
                "tokens.json 的 version = {:?}：本实现只认 1.x（升级格式要配一次迁移）",
                doc.version
            ));
        }
        if doc.tokens.is_empty() {
            return Err(
                "tokens.json 里一条 token 都没有 —— 没有人能通过鉴权，服务没有意义".to_string(),
            );
        }

        let mut entries: Vec<TokenEntry> = Vec::with_capacity(doc.tokens.len());
        let mut problems: Vec<String> = Vec::new();
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
            if t.tenant.trim().is_empty() {
                problems.push(format!(
                    "{at} 的 tenant 是空的 —— ⛔ 没有租户就没有隔离（不会退化成「查全部」）"
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
            if entries.iter().any(|e| e.token == token) {
                problems.push(format!("{at} 的 token 与前面的重复 —— 两条登记会互相覆盖限额"));
            }
            if !token.is_empty() {
                entries.push(TokenEntry {
                    token: token.to_string(),
                    digest: sha256_hex(token.as_bytes()),
                    tenant: t.tenant.trim().to_string(),
                    max_rows: t.max_rows,
                    max_concurrency: t.max_concurrency,
                    label: t.label.clone(),
                });
            }
        }
        if !problems.is_empty() {
            return Err(format!(
                "tokens.json 有 {} 处不合法：\n  - {}",
                problems.len(),
                problems.join("\n  - ")
            ));
        }
        Ok(Self { entries })
    }

    /// 按来源取一条（**只认调用方给出的字符串**；本模块不读 query string / cookie）。
    ///
    /// 定长比较、不早退：逐个比完再决定，避免按字节时序把 token 前缀猜出来。
    /// （已知边界：长度不同会立刻返回 false，所以**长度**是可见的。）
    pub fn find(&self, presented: &str) -> Option<&TokenEntry> {
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

    /// 不同租户的个数（启动横幅用；⛔ 不打印租户 id 本身之外的任何数据）。
    pub fn tenant_count(&self) -> usize {
        let mut ts: Vec<&str> = self.entries.iter().map(|e| e.tenant.as_str()).collect();
        ts.sort_unstable();
        ts.dedup();
        ts.len()
    }

    /// 全部租户 id（**启动横幅**打印它们：租户集合不是秘密，是「这台服务能查到谁」）。
    pub fn tenants(&self) -> Vec<&str> {
        let mut ts: Vec<&str> = self.entries.iter().map(|e| e.tenant.as_str()).collect();
        ts.sort_unstable();
        ts.dedup();
        ts
    }

    /// 表里最大的 `max_rows`（横幅用；每次调用仍按**各自** token 的上限执行）。
    pub fn max_rows_cap(&self) -> u64 {
        self.entries.iter().map(|e| e.max_rows).max().unwrap_or(0)
    }
}

/// 定长字节比较。
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

// ══════════════════════════════════════════════════════════════
// 判据
// ══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn good() -> String {
        r#"{
          "version": "1.0.0",
          "tokens": [
            {"label":"a","token":"FAKE-TOKEN-aaaaaaaaaaaaaaaaaaaa","tenant":"C0001",
             "max_rows":100,"max_concurrency":2},
            {"label":"b","token":"FAKE-TOKEN-bbbbbbbbbbbbbbbbbbbb","tenant":"C0002",
             "max_rows":50,"max_concurrency":1}
          ]
        }"#
        .to_string()
    }

    #[test]
    fn a_valid_table_parses_and_finds_by_token_only() {
        let t = TokenTable::parse(&good()).expect("合法表");
        assert_eq!(t.len(), 2);
        assert_eq!(t.tenant_count(), 2);
        assert_eq!(t.max_rows_cap(), 100);
        assert_eq!(t.tenants(), vec!["C0001", "C0002"]);

        let e = t.find("FAKE-TOKEN-aaaaaaaaaaaaaaaaaaaa").expect("必须找得到");
        assert_eq!(e.tenant, "C0001");
        assert_eq!(e.max_rows, 100);
        assert_eq!(e.max_concurrency, 2);
        assert_eq!(e.audit_digest().len(), "sha256:".len() + 12);

        // 非表内的字符串一律 None（query string / cookie 里的 token 不会有第二条路）。
        assert!(t.find("FAKE-TOKEN-cccccccccccccccccccc").is_none());
        assert!(t.find("").is_none());
        assert!(t.find("  ").is_none());
        // 大小写敏感（token 不是用户名）
        assert!(t.find("fake-token-aaaaaaaaaaaaaaaaaaaa").is_none());
    }

    #[test]
    fn defaults_are_applied_but_not_invented() {
        let t = TokenTable::parse(
            r#"{"version":"1.0.0","tokens":[
                {"token":"FAKE-TOKEN-cccccccccccccccccccc","tenant":"C0003"}]}"#,
        )
        .expect("合法表");
        let e = t.find("FAKE-TOKEN-cccccccccccccccccccc").unwrap();
        assert_eq!(e.max_rows, default_max_rows());
        assert_eq!(e.max_concurrency, default_max_concurrency());
        assert!(e.label.is_none());
    }

    #[test]
    fn a_short_token_is_rejected() {
        let raw = good().replace(
            "FAKE-TOKEN-bbbbbbbbbbbbbbbbbbbb",
            "short",
        );
        let err = TokenTable::parse(&raw).unwrap_err();
        assert!(err.contains("少于 24"), "{err}");
    }

    #[test]
    fn an_empty_tenant_is_rejected() {
        let raw = good().replace("\"tenant\":\"C0002\"", "\"tenant\":\"  \"");
        let err = TokenTable::parse(&raw).unwrap_err();
        assert!(err.contains("tenant 是空的"), "{err}");
    }

    #[test]
    fn a_duplicate_token_is_rejected() {
        let raw = good().replace("FAKE-TOKEN-bbbbbbbbbbbbbbbbbbbb", "FAKE-TOKEN-aaaaaaaaaaaaaaaaaaaa");
        let err = TokenTable::parse(&raw).unwrap_err();
        assert!(err.contains("重复"), "{err}");
    }

    #[test]
    fn a_typo_in_a_field_name_is_an_error_not_a_default() {
        let raw = good().replace("\"max_rows\":100", "\"max_row\":100");
        let err = TokenTable::parse(&raw).unwrap_err();
        assert!(err.contains("max_row"), "{err}");
        assert!(err.contains("tokens.json"), "{err}");
    }

    #[test]
    fn a_zero_or_absurd_limit_is_rejected() {
        let raw = good().replace("\"max_rows\":100", "\"max_rows\":0");
        assert!(TokenTable::parse(&raw).unwrap_err().contains("max_rows 是 0"));
        let raw = good().replace("\"max_rows\":100", "\"max_rows\":1000001");
        assert!(TokenTable::parse(&raw).unwrap_err().contains("硬上限"));
        let raw = good().replace("\"max_concurrency\":2", "\"max_concurrency\":0");
        assert!(TokenTable::parse(&raw).unwrap_err().contains("禁止调用"));
    }

    #[test]
    fn an_empty_or_wrong_version_table_is_rejected() {
        let raw = good().replace("\"version\": \"1.0.0\"", "\"version\": \"2.0.0\"");
        assert!(TokenTable::parse(&raw).unwrap_err().contains("只认 1.x"));

        let empty = r#"{"version":"1.0.0","tokens":[]}"#;
        assert!(TokenTable::parse(empty).unwrap_err().contains("一条 token 都没有"));
    }

    #[test]
    fn a_poisoned_field_is_not_silently_ignored() {
        // `deny_unknown_fields` 的反向自检：多一个不认识的字段就必须报错（不是忽略）。
        let raw = good().replace(
            "\"max_rows\":100",
            "\"max_rows\":100,\"admin\":true",
        );
        let err = TokenTable::parse(&raw).unwrap_err();
        assert!(err.contains("admin"), "{err}");
    }
}

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
//! - `tenant` **普通模式必填**（空租户 = 没有隔离）；
//! - ⭐ REQ-137 · **本体服务模式（`--ontology-only`）必须不填 `tenant`**；填了（任何值，
//!   含 `null` / 空串）⇒ **拒绝启动**。那个模式没有业务库、没有租户概念，
//!   `SPARQL` 恒 503（**需要库的查询**；纯 api 类给 `@pending`，REQ-151）——
//!   填一个假租户（如全 0 UUID）只会让审计里出现一个**看起来真实**的租户，所以不忽略、
//!   直接拒绝（见 [`TokenTable::parse_for_mode`]）；
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
use super::ServiceMode;

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

/// 一条 token 登记（**普通模式**的形状：`tenant` 必填）。
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

/// ⭐ REQ-137 · **本体服务模式**的 token 登记：`tenant` **必须缺省**。
///
/// 为什么单独一份形状，而不是给 [`TokenSpec`] 的 `tenant` 加 `Option`：
/// 普通模式缺 `tenant` 时必须**逐字保留**原来的报错（serde 的
/// ``missing field `tenant` ``，AC-003 拿它与基线比对）。两种模式各用一份 Serde 形状，
/// 普通模式那条路就一个字节都没动。
///
/// 为什么用自定义反序列化，而不是 `Option<String>`：serde 会把 `"tenant": null` 解成
/// `None` —— 那会让「写了一个 tenant 键」看起来像「没写」。这里要的是
/// **只要出现这个键就拒绝**（`null` / 空串 / 任何值），所以先取 `serde_json::Value`
/// 再折成 `Some(String)`（存在的证据），值本身只用于报错时说明。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct OntologyTokenSpec {
    token: String,
    #[serde(default = "default_max_rows")]
    max_rows: u64,
    #[serde(default = "default_max_concurrency")]
    max_concurrency: usize,
    #[serde(default)]
    label: Option<String>,
    #[serde(default, deserialize_with = "present_tenant")]
    tenant: Option<String>,
}

/// 本体模式的 `tokens.json` 顶层形状（与 [`TokenFile`] 只差在 spec）。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct OntologyTokenFile {
    version: String,
    tokens: Vec<OntologyTokenSpec>,
}

/// 「这个 token 里**写了** `tenant`」—— 存在即 `Some`（`null` 也算写了）。
fn present_tenant<'de, D>(d: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = serde_json::Value::deserialize(d)?;
    Ok(Some(match v {
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::String(s) => s,
        other => other.to_string(),
    }))
}

/// 两种 JSON 形状在**校验之前**的统一形状。
///
/// `tenant = None` 只可能来自本体模式（普通模式的 [`TokenSpec`] 里它是 `String`，
/// serde 在解析阶段就要求它有值）。这样一个校验循环就能服务两种模式。
///
/// ⚠️ **REQ-152 · 名字是承重的**：原来叫 `Raw` + `Spec`，而 QG 的
/// 「产品路径零厂商 token」判据里有一条**不区分大小写**的三字母厂商缩写 ——
/// 那两个词首尾相接就**恰好命中**（前者的尾巴 + 后者的头），
/// 于是 grep 把一行正经代码判成「厂商名」。
/// ⛔ 别改回去：任何让那三个字母连起来的拼写都会让 AC-002 的 grep 变红。
struct RawTokenSpec {
    token: String,
    tenant: Option<String>,
    max_rows: u64,
    max_concurrency: usize,
    label: Option<String>,
}

impl From<TokenSpec> for RawTokenSpec {
    fn from(t: TokenSpec) -> Self {
        Self {
            token: t.token,
            tenant: Some(t.tenant.trim().to_string()),
            max_rows: t.max_rows,
            max_concurrency: t.max_concurrency,
            label: t.label,
        }
    }
}

impl From<OntologyTokenSpec> for RawTokenSpec {
    fn from(t: OntologyTokenSpec) -> Self {
        Self {
            token: t.token,
            tenant: t.tenant,
            max_rows: t.max_rows,
            max_concurrency: t.max_concurrency,
            label: t.label,
        }
    }
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
    /// ⭐ REQ-137 · 租户：**普通模式恒 `Some`**（缺失在解析期就拒绝）；**本体模式恒 `None`**
    /// —— 那个部署没有租户概念。审计据此写 `tenant=-`（⛔ 不编空串、⛔ 不用全 0 UUID 顶替）。
    pub tenant: Option<String>,
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
    /// 解析 + 校验（**普通模式**的既有入口）。任何一条不合格 ⇒ **整份拒绝**（fail-closed）。
    ///
    /// ⚠️ 行为与 REQ-123 逐字一致（`tenant` 必填，缺失时是 serde 的
    /// ``missing field `tenant` ``）。要按服务模式校验用 [`Self::parse_for_mode`]。
    pub fn parse(json: &str) -> Result<Self, String> {
        Self::parse_for_mode(json, ServiceMode::Query)
    }

    /// ⭐ REQ-137 · 按**服务模式**解析 token 表。
    ///
    /// | 模式 | `tenant` | 结果 |
    /// | --- | --- | --- |
    /// | [`ServiceMode::Query`] | 必填 | 与 REQ-123 完全一致（缺 ⇒ serde 报 missing field） |
    /// | [`ServiceMode::OntologyOnly`] | **必须缺省** | 出现这个键（任何值）⇒ **整份拒绝** |
    ///
    /// 为什么本体模式「填了就拒绝」而不是忽略：忽略会留下一个**声明了数据范围、
    /// 但那个范围根本不存在**的凭证，而审计里会多出一个看起来真实的租户 —— 那正是
    /// 本需求要防的「假数据冒充事实」。拒绝会逼人在 token 里写实话（R2 / R4）。
    pub fn parse_for_mode(json: &str, mode: ServiceMode) -> Result<Self, String> {
        if mode.is_ontology_only() {
            let doc: OntologyTokenFile = serde_json::from_str(json)
                .map_err(|e| format!("tokens.json 不是合法形状：{e}"))?;
            let specs: Vec<RawTokenSpec> = doc.tokens.into_iter().map(RawTokenSpec::from).collect();
            return Self::validate(&doc.version, specs, mode);
        }
        let doc: TokenFile =
            serde_json::from_str(json).map_err(|e| format!("tokens.json 不是合法形状：{e}"))?;
        let specs: Vec<RawTokenSpec> = doc.tokens.into_iter().map(RawTokenSpec::from).collect();
        Self::validate(&doc.version, specs, mode)
    }

    fn validate(version: &str, tokens: Vec<RawTokenSpec>, mode: ServiceMode) -> Result<Self, String> {
        let major = version.trim().split('.').next().unwrap_or("");
        if major != "1" {
            return Err(format!(
                "tokens.json 的 version = {version:?}：本实现只认 1.x（升级格式要配一次迁移）"
            ));
        }
        if tokens.is_empty() {
            return Err(
                "tokens.json 里一条 token 都没有 —— 没有人能通过鉴权，服务没有意义".to_string(),
            );
        }
        // ⭐ REQ-137 · 本体模式：`tenant` 出现即拒绝（fail-closed，⛔ 不是忽略）。
        // 报错的三件事必须都在：① 这是本体模式、没有租户概念；② ⛔ 不要填假租户；
        // ③ 请删掉该字段（R4）。
        if mode.is_ontology_only() {
            if let Some((i, t)) = tokens.iter().enumerate().find(|(_, t)| t.tenant.is_some()) {
                return Err(format!(
                    "第 {} 个 token（label={:?}）带了 tenant，但这是本体服务模式（--ontology-only）。\n  \
                     本体模式**没有业务库、没有租户概念**，SPARQL 恒 503（需要库的查询；\
                     纯 api 类走 @pending，REQ-151）—— 这里的 tenant 不表示任何数据范围。\n  \
                     ⛔ 不要填假租户（例如全 0 UUID）：它会让审计日志里出现一个\"看起来真实\"的租户。\n  \
                     请从 token 里删掉 tenant 字段。",
                    i + 1,
                    t.label
                ));
            }
        }

        let mut entries: Vec<TokenEntry> = Vec::with_capacity(tokens.len());
        let mut problems: Vec<String> = Vec::new();
        for (i, t) in tokens.iter().enumerate() {
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
            // 普通模式：空租户仍然拒绝（原行为）。本体模式到这里 `tenant` 必然缺省，
            // 不参与这条判断（它没有租户，而不是「租户写空了」）。
            if !mode.is_ontology_only() && t.tenant.as_deref().is_none_or(|v| v.trim().is_empty()) {
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
                    // 本体模式：`None`（没有租户 ≠ 租户为空串）；普通模式：必有值。
                    tenant: t.tenant.clone(),
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
    ///
    /// ⭐ REQ-137 · 本体模式恒 0：那里的凭证**没有租户**（审计写 `tenant=-`）。
    pub fn tenant_count(&self) -> usize {
        self.tenants().len()
    }

    /// 全部租户 id（**启动横幅**打印它们：租户集合不是秘密，是「这台服务能查到谁」）。
    ///
    /// ⭐ REQ-137 · 本体模式没有租户 ⇒ 返回空表（⛔ 不用空串/占位 UUID 凑一个）。
    pub fn tenants(&self) -> Vec<&str> {
        let mut ts: Vec<&str> = self
            .entries
            .iter()
            .filter_map(|e| e.tenant.as_deref())
            .collect();
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
        assert_eq!(e.tenant.as_deref(), Some("C0001"));
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

    // ══════════════════════════════════════════════════════════
    // ⭐ REQ-137 · `tenant` 按模式必填
    // ══════════════════════════════════════════════════════════

    /// ⭐ AC-003 · 普通模式缺 `tenant` **仍拒绝**，且报错就是基线的
    /// serde ``missing field `tenant` ``（零行为变化的判据）。
    #[test]
    fn query_mode_still_requires_tenant_with_the_same_serde_error() {
        let raw = r#"{"version":"1.0.0","tokens":[
            {"label":"a","token":"FAKE-TOKEN-aaaaaaaaaaaaaaaaaaaa"}]}"#;
        let err = TokenTable::parse(raw).unwrap_err();
        assert!(err.contains("missing field `tenant`"), "{err}");
        // `parse` 与 `parse_for_mode(.., Query)` 是同一条路（不是两套实现）。
        assert_eq!(
            err,
            TokenTable::parse_for_mode(raw, ServiceMode::Query).unwrap_err()
        );
    }

    /// ⭐ AC-001 · 本体模式 + 无 `tenant` ⇒ 解析通过，并且**真的没有租户**
    /// （`tenants()` 空 / `tenant_count() == 0` / entry 的 `tenant` 是 `None`）。
    #[test]
    fn ontology_mode_accepts_a_token_table_without_tenant() {
        let raw = r#"{"version":"1.0.0","tokens":[
            {"label":"t","token":"FAKE-TOKEN-aaaaaaaaaaaaaaaaaaaa",
             "max_rows":200,"max_concurrency":4}]}"#;
        let t = TokenTable::parse_for_mode(raw, ServiceMode::OntologyOnly).expect("本体模式合法表");
        let e = t.find("FAKE-TOKEN-aaaaaaaaaaaaaaaaaaaa").expect("必须找得到");
        assert!(e.tenant.is_none(), "本体模式的凭证没有租户：{:?}", e.tenant);
        assert_eq!(e.max_rows, 200);
        assert_eq!(e.max_concurrency, 4);
        assert!(t.tenants().is_empty(), "⛔ 不许造出租户：{:?}", t.tenants());
        assert_eq!(t.tenant_count(), 0);
    }

    /// ⭐ AC-002 · 本体模式 + 带 `tenant`（任何值：UUID / 普通串 / 空串 / null）⇒ 拒绝，
    /// 且报错三件事都在：① 本体模式没有租户概念 ② ⛔ 不要填假租户 ③ 请删掉该字段。
    ///
    /// 变异（纪律 G）：把「填了就拒绝」改成「忽略」⇒ 本测试立刻红。
    #[test]
    fn ontology_mode_rejects_any_tenant_field_with_an_actionable_message() {
        for tenant in [
            r#""tenant":"00000000-0000-0000-0000-000000000000","#,
            r#""tenant":"C0001","#,
            r#""tenant":"","#,
            r#""tenant":null,"#,
        ] {
            let raw = format!(
                r#"{{"version":"1.0.0","tokens":[
                    {{"label":"t",{tenant}"token":"FAKE-TOKEN-aaaaaaaaaaaaaaaaaaaa"}}]}}"#
            );
            let err = TokenTable::parse_for_mode(&raw, ServiceMode::OntologyOnly)
                .expect_err(&format!("带了 {tenant} 必须拒绝"));
            // ① 这是本体模式、没有租户概念、SPARQL 恒 503
            assert!(err.contains("本体服务模式（--ontology-only）"), "{err}");
            assert!(err.contains("没有租户概念"), "{err}");
            assert!(err.contains("SPARQL 恒 503"), "{err}");
            // ② ⛔ 不要填假租户（它会污染审计）
            assert!(err.contains("不要填假租户"), "{err}");
            assert!(err.contains("看起来真实"), "{err}");
            // ③ 动作：删掉这个字段
            assert!(err.contains("请从 token 里删掉 tenant 字段"), "{err}");
        }
    }

    /// ⭐ REQ-137 · 本体模式**不放宽**其它校验（拼错字段名 / 上限仍在）。
    #[test]
    fn ontology_mode_keeps_the_other_strict_checks() {
        let typo = r#"{"version":"1.0.0","tokens":[
            {"token":"FAKE-TOKEN-aaaaaaaaaaaaaaaaaaaa","max_row":200}]}"#;
        assert!(TokenTable::parse_for_mode(typo, ServiceMode::OntologyOnly)
            .unwrap_err()
            .contains("max_row"));

        let absurd = r#"{"version":"1.0.0","tokens":[
            {"token":"FAKE-TOKEN-aaaaaaaaaaaaaaaaaaaa","max_rows":1000001}]}"#;
        assert!(TokenTable::parse_for_mode(absurd, ServiceMode::OntologyOnly)
            .unwrap_err()
            .contains("硬上限"));
    }
}

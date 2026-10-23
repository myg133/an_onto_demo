//! REQ-126 · **门户审计** —— 每次问题执行一行，⛔ 无口令明文、⛔ 无 SQL、⛔ 无查询原文。
//!
//! # 一行长什么样（唯一形状，[`AuditEntry::line`]）
//!
//! ```text
//! audit ts=2026-10-23T09:41:02.480Z event=portal token=sha256:3f2a1b0c9d8e tenant=c51dc886-… qid=deduction_by_currency rows=1 elapsed_ms=37 code=OK
//! ```
//!
//! AC-009 的七项逐项对应：`ts` / `token`（摘要）/ `qid`（问题 id）/ `tenant` /
//! `rows` / `elapsed_ms` / `code`。
//!
//! # 落点复用查询面的那一个
//!
//! 行在这里拼，写出去走 [`crate::query::audit::AuditSink::record_line`]：
//! **stderr 恒写 + 可选文件追加**。⛔ 不另开第二个 sink —— 审计的唯一写点是
//! `query/audit.rs` 的 `writeln!`（OUTLETS 的 I17）。
//!
//! # 字段清单就是白名单
//!
//! [`AuditEntry`] 里**没有** `sql`、没有 `token`（原文）、没有 `sparql` 三个字段，
//! 所以将来「顺手把查询打进去」这件事在类型层写不出来（与 REQ-123/124 同一条设计）。
//!
//! # `qid` 是**客户端可控输入**，所以先过滤再进日志
//!
//! `POST /q/<id>` 的 id 来自 URL。直接把它拼进审计行等于允许换行注入
//! （`/q/x%0Aaudit ...` —— 一行变两行，日志就骗人了）。[`safe_qid`] 只放行
//! `[a-z0-9_-]{1,64}`，其余一律记成 `-`。
//!
//! # 已知边界（⛔ 不写成「已封堵」）
//!
//! - ⚠️ **租户 id 是明文**（不是摘要）：AC-009 要求每行记「是哪个租户」，而租户 id
//!   正是对账时要看的东西（「这张口令那天查了几次」要能落到客户身上）。代价是
//!   审计文件/日志一旦外流，**谁是谁**是直读的。要脱敏得另开需求（换成散列前缀
//!   会同时失去按租户对账的能力，那不是这一版能顺手做的取舍）。
//! - 摘要**不加盐**（与 REQ-123 同一条）：低熵口令仍可能被离线比对 ⇒
//!   `portal-tokens.json` 要求 ≥ 24 字符；
//! - 审计写**文件**时没有轮转/限额：由外部（logrotate / 磁盘）负责。

use std::time::Instant;

use serde::Serialize;

use crate::query::audit::{AuditRecord, AuditSink};

use super::tokens::PortalTokenEntry;

/// 一条门户审计记录。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AuditEntry {
    pub ts: String,
    /// `sha256:` + 12 位；没有身份时 `None`。
    pub token: Option<String>,
    /// 口令绑定的租户（没有身份时 `None`）。
    pub tenant: Option<String>,
    /// 问题 id（已过滤；没有问题时 `-`）。
    pub qid: String,
    /// 返回行数（失败时 `None`）。
    pub rows: Option<usize>,
    pub elapsed_ms: u128,
    /// `OK` 或错误码。
    pub code: String,
}

impl AuditEntry {
    /// 唯一的行形状（判据 `the_line_has_the_agreed_seven_fields`）。
    pub fn line(&self) -> String {
        let dash = "-";
        format!(
            "audit ts={} event=portal token={} tenant={} qid={} rows={} elapsed_ms={} code={}",
            self.ts,
            self.token.as_deref().unwrap_or(dash),
            self.tenant.as_deref().unwrap_or(dash),
            self.qid,
            self.rows.map(|r| r.to_string()).unwrap_or_else(|| dash.to_string()),
            self.elapsed_ms,
            self.code
        )
    }
}

/// 只放行 `[a-z0-9_-]{1,64}`；其余记 `-`（⛔ 不让客户端输入改写日志形状）。
pub fn safe_qid(raw: &str) -> String {
    let t = raw.trim();
    if t.is_empty() || t.len() > 64 {
        return "-".to_string();
    }
    if t.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
    {
        t.to_string()
    } else {
        "-".to_string()
    }
}

/// 审计出口：唯一落点（stderr 恒写；文件可选追加）。
#[derive(Debug)]
pub struct AuditLog {
    sink: AuditSink,
}

impl AuditLog {
    pub fn new(sink: AuditSink) -> Self {
        Self { sink }
    }

    /// 记一次**有身份**的调用（成功与失败都记）。
    pub fn record(
        &self,
        entry: &PortalTokenEntry,
        qid: &str,
        rows: Option<usize>,
        elapsed: std::time::Duration,
        code: &'static str,
    ) {
        self.push(AuditEntry {
            ts: AuditRecord::now_ts(),
            token: Some(entry.audit_digest()),
            tenant: Some(entry.tenant.clone()),
            qid: safe_qid(qid),
            rows,
            elapsed_ms: elapsed.as_millis(),
            code: code.to_string(),
        });
    }

    /// 记一次**没有身份**的事件（401 —— 此刻没有可用的门户口令）。
    ///
    /// 字段里没有 token / tenant / qid：它们**此刻不存在**。事件名仍然记录，
    /// 于是「401 有多少次」是一个能数出来的事实，而不是一段沉默。
    pub fn record_anonymous(&self, code: &'static str, elapsed: std::time::Duration) {
        self.push(AuditEntry {
            ts: AuditRecord::now_ts(),
            token: None,
            tenant: None,
            qid: "-".to_string(),
            rows: None,
            elapsed_ms: elapsed.as_millis(),
            code: code.to_string(),
        });
    }

    fn push(&self, entry: AuditEntry) {
        self.sink.record_line(&entry.line());
    }
}

/// 没有身份的那一条（`record_anonymous` 的糖，调用点少写一个 `Instant::now()`）。
pub fn anonymize(log: &AuditLog, code: &'static str, started: Instant) {
    log.record_anonymous(code, started.elapsed());
}

// ══════════════════════════════════════════════════════════════
// 判据
// ══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> AuditEntry {
        AuditEntry {
            ts: "2026-10-23T09:41:02.480Z".into(),
            token: Some("sha256:3f2a1b0c9d8e".into()),
            tenant: Some("c51dc886-6c91-43d7-a8fb-a0b18af67b81".into()),
            qid: "deduction_by_currency".into(),
            rows: Some(1),
            elapsed_ms: 37,
            code: "OK".into(),
        }
    }

    #[test]
    fn the_line_has_the_agreed_seven_fields() {
        let line = entry().line();
        for needle in [
            "audit ts=2026-10-23T09:41:02.480Z",
            "event=portal",
            "token=sha256:3f2a1b0c9d8e",
            "tenant=c51dc886-6c91-43d7-a8fb-a0b18af67b81",
            "qid=deduction_by_currency",
            "rows=1",
            "elapsed_ms=37",
            "code=OK",
        ] {
            assert!(line.contains(needle), "审计行缺 {needle:?}：{line}");
        }
        // 失败：rows 是 `-`，code 是错误码。
        let bad = AuditEntry {
            rows: None,
            code: "QUESTION_FAILED".into(),
            ..entry()
        };
        assert!(bad.line().contains("rows=- "), "{}", bad.line());
        assert!(bad.line().ends_with("code=QUESTION_FAILED"), "{}", bad.line());
    }

    /// ⛔ 无口令明文、⛔ 无 SQL / 查询原文（AC-009 的两条否定判据）。
    #[test]
    fn the_line_carries_no_token_plaintext_and_no_sql() {
        let token = "FAKE-PORTAL-TOKEN-0123456789abcdef";
        let log = AuditLog::new(AuditSink::stderr_only());
        let entry = PortalTokenEntry {
            token: token.into(),
            digest: crate::query::audit::sha256_hex(token.as_bytes()),
            tenant: "T-1".into(),
            max_rows: 10,
            max_concurrency: 1,
            label: None,
        };
        let rec = AuditEntry {
            ts: AuditRecord::now_ts(),
            token: Some(entry.audit_digest()),
            tenant: Some(entry.tenant.clone()),
            qid: "q1".into(),
            rows: Some(2),
            elapsed_ms: 5,
            code: "OK".into(),
        };
        let line = rec.line();
        assert!(!line.contains(token), "审计里出现了口令明文：{line}");
        assert!(!line.to_ascii_lowercase().contains("select"), "{line}");
        assert!(!line.contains("FROM "), "{line}");
        // `push` 不 panic、不返回错误（审计失败不该把一次成功的查询变成 500）。
        log.push(rec);
    }

    /// ⛔ 客户端可控的 id 不许改写日志形状（换行注入）。
    #[test]
    fn a_hostile_question_id_cannot_forge_an_audit_line() {
        assert_eq!(safe_qid("ok_id-1"), "ok_id-1");
        assert_eq!(safe_qid("x\naudit ts=2026"), "-");
        assert_eq!(safe_qid("UPPER"), "-");
        assert_eq!(safe_qid(""), "-");
        assert_eq!(safe_qid(&"a".repeat(65)), "-");
        assert_eq!(safe_qid(&"a".repeat(64)), "a".repeat(64));
    }

    #[test]
    fn an_anonymous_event_has_no_identity_fields() {
        let line = AuditEntry {
            ts: "t".into(),
            token: None,
            tenant: None,
            qid: "-".into(),
            rows: None,
            elapsed_ms: 1,
            code: "UNAUTHORIZED".into(),
        }
        .line();
        assert!(line.contains("token=- tenant=- qid=- rows=- "), "{line}");
        assert!(line.ends_with("code=UNAUTHORIZED"), "{line}");
    }
}

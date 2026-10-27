//! REQ-124 · **控制面审计** —— 每个写操作一行（**含失败**），⛔ 不含 token 明文。
//!
//! # 一行长什么样（唯一形状，[`AuditEntry::line`]）
//!
//! ```text
//! audit ts=2026-10-23T07:12:01.480Z action=import token=sha256:3f2a1b0c9d8e target=project=p1 source=/tmp/p.json result=OK elapsed_ms=41
//! ```
//!
//! R6 要的六项逐项对应：`ts` / `token`（摘要）/ `action` / `target` / `result` / `elapsed_ms`。
//!
//! # 落点复用查询面的那一个
//!
//! 行在这里拼，写出去走 [`crate::query::audit::AuditSink::record_line`]：
//! **stdout 恒写 + 可选文件追加**。⛔ 不另开第二个 sink —— 那会出现
//! 「一个文件两种行、其中一种忘了 append」这类问题（OUTLETS 的 I17 只有一个写点）。
//!
//! # 为什么要一个内存环
//!
//! `GET /api/audit` 要能读到「刚发生的那几行」。审计文件是**追加**的、
//! 由外部轮转，服务端不该为了这个端点去 tail 文件（tail 读到的是别人的格式）。
//! 环里只留最近 [`RING_CAP`] 条：它不是审计的**存储**，是**检索窗口**。
//!
//! ⛔ 否定判据（判据见 `tests/test_req124_console.rs` 与 `console::audit` 的单测）：
//! 行的字段里**没有** token 原文，也没有请求体内容；`target` 是端点自己写死的
//! 短标签（项目 / 版本 / 提案别名 / 声明名 / 文件路径），不是请求体转储。

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Instant;

use serde::Serialize;

use crate::query::audit::{AuditRecord, AuditSink};

use super::tokens::TokenEntry;

/// 内存里保留的最近条数。
pub const RING_CAP: usize = 1000;

/// 一条控制面审计记录。
///
/// ⚠️ 字段清单就是「允许写出去的东西」的**全集**（与 REQ-123 的 `AuditRecord`
/// 同一条设计）：没有 `token` 原文、没有 `body`。将来想加也加不进来。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AuditEntry {
    pub ts: String,
    /// `sha256:` + 12 位；没有身份时 `null`。
    pub token: Option<String>,
    /// `import` / `apply` / `freeze` / `export` / `r2rml` / `config` / `decide` / `auth`。
    pub action: &'static str,
    /// 目标（端点写死的短标签，不含请求体）。
    pub target: String,
    /// `OK` 或错误码。
    pub result: String,
    pub elapsed_ms: u128,
}

impl AuditEntry {
    /// 唯一的行形状（判据 `line_has_the_agreed_shape`）。
    pub fn line(&self) -> String {
        format!(
            "audit ts={} action={} token={} target={} result={} elapsed_ms={}",
            self.ts,
            self.action,
            self.token.as_deref().unwrap_or("-"),
            self.target,
            self.result,
            self.elapsed_ms
        )
    }
}

/// 审计出口：内存环 + 唯一落点。
#[derive(Debug)]
pub struct AuditLog {
    ring: Mutex<VecDeque<AuditEntry>>,
    sink: AuditSink,
}

impl AuditLog {
    pub fn new(sink: AuditSink) -> Self {
        Self {
            ring: Mutex::new(VecDeque::with_capacity(64)),
            sink,
        }
    }

    /// 记一条**有身份**的操作。
    pub fn record(
        &self,
        auth: &TokenEntry,
        action: &'static str,
        target: impl Into<String>,
        result: impl Into<String>,
        started: Instant,
    ) {
        self.record_digest(Some(&auth.audit_digest()), action, target, result, started);
    }

    /// ⭐ REQ-159 P1c-1 · 记一条**按摘要**的操作（不要求 `console::tokens::TokenEntry`）。
    ///
    /// 为什么需要它：这 8 条写端点现在挂在**两个面**上（console 与 `query serve`），
    /// 而两个面的 token 是**两个类型**（`console::tokens::TokenEntry` /
    /// `query::tokens::TokenEntry`）—— 但审计行的**形状只有一种**（本模块）。
    /// 于是 `record` 变成「先算摘要，再走这一条」的薄包装：控制面与查询面写出来的行
    /// 逐字同形，⛔ 不是两套实现。（判据：`tests/test_req159_bundles.rs` 的 AC-012
    /// 数的是 `action=reveal_key` 的行数，两边的写法必须都能被它数到。）
    ///
    /// `token = None` ⇒ 行里写 `token=-`（与 [`Self::record_anonymous`] 同一个事实：
    /// 此刻没有可用的身份）。
    pub fn record_digest(
        &self,
        token: Option<&str>,
        action: &'static str,
        target: impl Into<String>,
        result: impl Into<String>,
        started: Instant,
    ) {
        self.push(AuditEntry {
            ts: AuditRecord::now_ts(),
            token: token.map(str::to_string),
            action,
            target: target.into(),
            result: result.into(),
            elapsed_ms: started.elapsed().as_millis(),
        });
    }

    /// 记一条**没有身份**的事件（401 / 403 —— 此刻没有可用的 admin 身份）。
    pub fn record_anonymous(
        &self,
        action: &'static str,
        target: impl Into<String>,
        result: impl Into<String>,
        started: Instant,
    ) {
        self.push(AuditEntry {
            ts: AuditRecord::now_ts(),
            token: None,
            action,
            target: target.into(),
            result: result.into(),
            elapsed_ms: started.elapsed().as_millis(),
        });
    }

    fn push(&self, entry: AuditEntry) {
        self.sink.record_line(&entry.line());
        let mut ring = match self.ring.lock() {
            Ok(g) => g,
            Err(e) => e.into_inner(),
        };
        ring.push_back(entry);
        while ring.len() > RING_CAP {
            ring.pop_front();
        }
    }

    /// 最近 `limit` 条（**按发生顺序**：旧 → 新；`limit` 只截尾部）。
    pub fn recent(&self, limit: usize) -> Vec<AuditEntry> {
        let ring = match self.ring.lock() {
            Ok(g) => g,
            Err(e) => e.into_inner(),
        };
        let skip = ring.len().saturating_sub(limit);
        ring.iter().skip(skip).cloned().collect()
    }

    pub fn len(&self) -> usize {
        match self.ring.lock() {
            Ok(g) => g.len(),
            Err(e) => e.into_inner().len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> AuditEntry {
        AuditEntry {
            ts: "2026-10-23T07:12:01.480Z".into(),
            token: Some("sha256:3f2a1b0c9d8e".into()),
            action: "import",
            target: "project=demo source=/tmp/p.json".into(),
            result: "OK".into(),
            elapsed_ms: 41,
        }
    }

    #[test]
    fn line_has_the_agreed_shape_and_the_six_fields() {
        assert_eq!(
            entry().line(),
            "audit ts=2026-10-23T07:12:01.480Z action=import token=sha256:3f2a1b0c9d8e \
             target=project=demo source=/tmp/p.json result=OK elapsed_ms=41"
        );
        // 失败：result 是错误码（不是 OK）；没有身份时 token 是 `-`。
        let bad = AuditEntry {
            token: None,
            result: "PROPOSAL_NOT_PENDING".into(),
            ..entry()
        };
        let line = bad.line();
        assert!(line.contains("token=- "), "{line}");
        assert!(line.ends_with("result=PROPOSAL_NOT_PENDING elapsed_ms=41"), "{line}");
    }

    #[test]
    fn the_line_never_carries_a_token_plaintext() {
        let token = "FAKE-ADMIN-TOKEN-0123456789abcdef";
        let rec = AuditEntry {
            token: Some(format!(
                "sha256:{}",
                crate::query::audit::digest_prefix(
                    &crate::query::audit::sha256_hex(token.as_bytes()),
                    12
                )
            )),
            ..entry()
        };
        assert!(!rec.line().contains(token), "审计里出现了 token 明文：{}", rec.line());
    }

    #[test]
    fn the_ring_is_bounded_and_recent_keeps_the_tail_in_order() {
        let log = AuditLog::new(AuditSink::stdout_only());
        let started = Instant::now();
        for i in 0..(RING_CAP + 3) {
            log.push(AuditEntry {
                action: "import",
                target: format!("n={i}"),
                ..entry()
            });
        }
        assert_eq!(log.len(), RING_CAP, "环必须有上限（否则长跑的服务会吃光内存）");
        let got = log.recent(2);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].target, format!("n={}", RING_CAP + 1));
        assert_eq!(got[1].target, format!("n={}", RING_CAP + 2));
        let _ = started;
    }
}

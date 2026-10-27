//! REQ-123 · **审计与摘要** —— 每次调用一行，⛔ 不含 token 明文、⛔ 不含查询原文。
//!
//! # 一行长什么样（唯一形状，`AuditRecord::line`）
//!
//! ```text
//! audit ts=2026-10-23T05:31:12.480Z event=sparql proto=rest token=sha256:3f2a… tenant=C0001 q=sha256:9c1d… rows=12 elapsed_ms=37 code=OK
//! ```
//!
//! - `token` 只写**摘要**（[`sha256_hex`] 前 12 位）—— 拿到日志的人无法据此取数；
//! - `q` 只写**查询哈希**（前 16 位）—— 能对「同一条查询」做计数与比对，
//!   但**读不出查询原文**（AC-007 的两条否定判据）；
//! - 失败也记（`code` ≠ `OK`），`rows=-`。
//!
//! # 为什么自己写 sha256（而不是引依赖）
//!
//! 服务端审计只要「稳定、定长、不可逆」的摘要，不需要加密强度以外的任何东西；
//! 而本 crate 的依赖面是**需求级被管着的**（AC-011：不引新依赖）。
//! sha256 是 60 行确定性代码，附两条 FIPS 测试向量（见文末单测）——
//! 比多一个依赖更容易被审计。
//!
//! # 已知边界（⛔ 不写成「已封堵」）
//!
//! - 摘要**不是**加盐的：拿一份已知 token 的候选表可以离线比对（[`AuditRecord`] 的
//!   `token` 字段只给 12 位十六进制，穷举面是 2^48，但**低熵口令**仍可能被猜中）。
//!   ⇒ token 必须由运维随机生成（`tokens.json` 解析时要求 ≥ 24 字符）。
//! - 审计写**文件**时没有轮转/限额：写满由外部（logrotate / 磁盘）负责。

use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};

// ══════════════════════════════════════════════════════════════
// sha256（FIPS 180-4）
// ══════════════════════════════════════════════════════════════

/// sha256 轮常量。
const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
    0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
    0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
    0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
    0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
    0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
    0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
    0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
    0xc67178f2,
];

/// 摘要的十六进制写法（小写，64 字符）。
///
/// 判据：两条 FIPS 已知向量（空串 / `abc`）+ 一条「不同输入不同摘要」——
/// 写错常量或漏掉填充都会在那两条上红。
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    // 填充：0x80 + 若干 0 + 64 位大端比特长度。
    let bit_len = (bytes.len() as u64).wrapping_mul(8);
    let mut msg = Vec::with_capacity(bytes.len() + 72);
    msg.extend_from_slice(bytes);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, word) in w.iter_mut().take(16).enumerate() {
            *word = u32::from_be_bytes([
                chunk[4 * i],
                chunk[4 * i + 1],
                chunk[4 * i + 2],
                chunk[4 * i + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut out = String::with_capacity(64);
    for word in h {
        out.push_str(&format!("{word:08x}"));
    }
    out
}

/// 摘要的前 `n` 位（审计里只放前缀：够比对，不够反推）。
///
/// `n` 超过 64 时自动截到 64（⛔ 不 panic —— 审计细节不该让服务 500）。
pub fn digest_prefix(hex: &str, n: usize) -> &str {
    let end = n.min(hex.len());
    &hex[..end]
}

// ══════════════════════════════════════════════════════════════
// 一条审计记录
// ══════════════════════════════════════════════════════════════

/// 一次调用的审计记录。
///
/// ⚠️ 字段设计就是「允许写出去的东西」的**全集**：没有 `query`、没有 `token` 原文
/// 这两个字段，所以将来有人想加也加不进来（要加就得改类型）。
#[derive(Debug, Clone)]
pub struct AuditRecord {
    /// RFC3339（UTC，毫秒）。
    pub ts: String,
    /// `sparql` / `catalog` / `describe` / `sample` / `sse` / `auth`。
    pub event: &'static str,
    /// `rest` / `mcp`。
    pub proto: &'static str,
    /// token 的 sha256 前缀（没有 token 时 `None`）。
    pub token_digest: Option<String>,
    pub tenant: Option<String>,
    /// 查询的 sha256 前缀（catalog/describe 没有查询时 `None`）。
    pub query_digest: Option<String>,
    /// 返回行数（失败时 `None`）。
    pub rows: Option<usize>,
    pub elapsed_ms: u128,
    /// `OK` 或错误码。
    pub code: &'static str,
    /// ⭐ REQ-127 · TTL 视图（`customer` / `model`）；其它出口 `None`。
    ///
    /// 只放**代码里写死的两个短语**（`TtlView::as_str`），不是调用方给的字符串。
    pub view: Option<&'static str>,
    /// ⭐ REQ-127 · 响应正文字节数（只有 TTL 出口用它；401/304 记 `0`）。
    pub bytes: Option<usize>,
    /// 附加事实（**只许是代码里写死的短语**：`idle_timeout` / `client_gone`）。
    pub note: Option<&'static str>,
    /// ⭐ REQ-145 · 这次调用**只返回了一半**（`@pending` 非空 / 行回执被拒）。
    ///
    /// ⚠️ 为什么是**独立字段**而不是第二个错误码：`code` 表达「客户端收到了什么」
    /// （`OK` / `NOT_DB_SOURCE` / …），`pending` 表达「这份结果完整吗」。
    /// 用一个 `code=API_PENDING` 混着说两件事，看审计的人就会以为客户端收到了一个
    /// 叫 `API_PENDING` 的错误 —— 那正是 R-2 要求改掉的东西。
    pub pending: bool,
}

impl AuditRecord {
    /// 现在（UTC，毫秒）。
    pub fn now_ts() -> String {
        chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    }

    /// 唯一的行形状。判据：单测 [`tests::the_line_has_the_agreed_shape`]。
    pub fn line(&self) -> String {
        let dash = "-";
        let token = self.token_digest.as_deref().unwrap_or(dash);
        let tenant = self.tenant.as_deref().unwrap_or(dash);
        let query = self.query_digest.as_deref().unwrap_or(dash);
        let rows = self
            .rows
            .map(|r| r.to_string())
            .unwrap_or_else(|| dash.to_string());
        let note = self
            .note
            .map(|n| format!(" note={n}"))
            .unwrap_or_default();
        let view = self
            .view
            .map(|v| format!(" view={v}"))
            .unwrap_or_default();
        let bytes = self
            .bytes
            .map(|b| format!(" bytes={b}"))
            .unwrap_or_default();
        // ⭐ REQ-145 · `pending=true` 只在**真是半个答案**时出现（既有行一个字节都不变）。
        let pending = if self.pending { " pending=true" } else { "" };
        format!(
            "audit ts={} event={} proto={} token={} tenant={} q={} rows={} elapsed_ms={} code={}{}{}{}{}",
            self.ts,
            self.event,
            self.proto,
            token,
            tenant,
            query,
            rows,
            self.elapsed_ms,
            self.code,
            pending,
            view,
            bytes,
            note
        )
    }
}

// ══════════════════════════════════════════════════════════════
// 出点
// ══════════════════════════════════════════════════════════════

/// 审计的落点：**stdout 恒写**，可选再追加一份文件。
///
/// 为什么 stdout 恒写（REQ-135）：k8s / systemd / docker 的标准做法是「进程写 stdout，
/// 集群负责采集」。写 `emptyDir` 等于把合规证据交给 Pod 生命周期（重启即丢）。
/// 「忘了配 `--audit-out` 就没有审计」不是一个可接受的安全默认 —— 文件是**可选**的
/// 第二落点，**不是**替代品：给了 `--audit-out` 也照样先写 stdout（AC-005）。
///
/// ⭐ REQ-159 P1b · `file` 是 `Arc<Mutex<File>>`、整个落点 `Clone`：
/// 多项目模式下**每一份项目服务**都要能写同**一个**审计落点（一个进程一份审计，
/// ⛔ 不是每个项目一份文件）。克隆共享同一个 `File`，写入仍由那把锁串行 ——
/// 单项目模式只用到一个 clone，输出一个字节都不变。
#[derive(Debug, Clone)]
pub struct AuditSink {
    file: Option<Arc<Mutex<std::fs::File>>>,
}

impl AuditSink {
    /// 只写 stdout（不给 `--audit-out` 时的默认落点）。
    pub fn stdout_only() -> Self {
        Self { file: None }
    }

    /// 追加写（不截断历史）。打不开 ⇒ `Err`（调用方拒绝启动，⛔ 不静默降级）。
    pub fn open(path: &Path) -> std::io::Result<Self> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        Ok(Self {
            file: Some(Arc::new(Mutex::new(file))),
        })
    }

    /// 写一条。**永不 panic、永不返回错误** —— 审计失败不该让一次已经成功的查询变成 500；
    /// 但写失败会再往 stderr 喊一句（`✗`），运维能看见。
    pub fn record(&self, rec: &AuditRecord) {
        self.record_line(&rec.line());
    }

    /// ⭐ REQ-124 · **已成形的一行**（控制面审计复用同一个落点）。
    ///
    /// 为什么不让控制面另开一个 sink：`record_line` 里那一次 stdout 写出是审计的**唯一**
    /// 落点（OUTLETS 的 I17）。控制面条目多两个字段（`action` / `target`），
    /// 但落点、追加语义、「stdout 恒写」这三件事与查询面**必须**是同一份 ——
    /// 两个 sink 意味着「一个文件里两种行、其中一种忘了 flush」这类问题。
    ///
    /// 行的**形状**由调用方给（控制面在 `console::audit` 里定义它的字段），
    /// 本函数只负责写出。
    ///
    /// # 落点（REQ-135）
    ///
    /// ① **stdout 恒写**（容器日志 → 集群采集）；
    /// ② 给了 `--audit-out` 再**追加**同一行到文件。⛔ 不存在「只写文件」的分支。
    pub fn record_line(&self, line: &str) {
        // 用 `Stdout` 的 `Write` 而不是 `println!`：管线登记表（OUTLETS）的 needle
        // 里不能出现 stdout 宏的字面量 —— `domain/pipeline.rs` 有一条
        // 「管线模块自己不许往 stdout 写」的判据（T7），那条判据扫的是**源码文本**，
        // 会把登记表里的 needle 字面量也算进去。这里写成 `writeln!` 既保持
        // 「stdout 恒写」，又不给那条判据制造假阳性（它自己另有一处真 `eprintln!`）。
        if let Err(e) = writeln!(std::io::stdout(), "{line}") {
            eprintln!("✗ 审计 stdout 写入失败：{e}");
        }
        let Some(lock) = &self.file else { return };
        let mut f = match lock.lock() {
            Ok(f) => f,
            Err(e) => e.into_inner(),
        };
        if let Err(e) = writeln!(f, "{line}") {
            eprintln!("✗ 审计文件写入失败：{e}");
        }
    }
}

// ══════════════════════════════════════════════════════════════
// 判据
// ══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    /// FIPS 180-4 的两条已知向量（常量 / 填充 / 大端长度任何一处写错都红）。
    #[test]
    fn sha256_matches_the_fips_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        // 摘要是**内容**的函数：差一个字节就完全不同（防「截断/去尾」的实现）。
        assert_ne!(sha256_hex(b"SELECT ?s"), sha256_hex(b"SELECT ?s "));
        assert_eq!(sha256_hex(b"x").len(), 64);
    }

    #[test]
    fn digest_prefix_is_bounded_and_never_panics() {
        let d = sha256_hex(b"abc");
        assert_eq!(digest_prefix(&d, 4), &d[..4]);
        assert_eq!(digest_prefix(&d, 64), d);
        assert_eq!(digest_prefix(&d, 999), d, "越界只截到全长，不 panic");
    }

    /// 行形状：字段齐全 + 顺序固定（运维 grep 的口径）。
    #[test]
    fn the_line_has_the_agreed_shape() {
        let rec = AuditRecord {
            ts: "2026-10-23T05:31:12.480Z".into(),
            event: "sparql",
            proto: "rest",
            token_digest: Some("sha256:3f2a1b0c9d8e".into()),
            tenant: Some("C0001".into()),
            query_digest: Some("sha256:9c1d2e3f4a5b".into()),
            rows: Some(12),
            elapsed_ms: 37,
            code: "OK",
            view: None,
            bytes: None,
            note: None,
            // ⭐ REQ-145 · 不是半个答案（那个事实由 audit_pending 写 pending=true）。
            pending: false,
        };
        assert_eq!(
            rec.line(),
            "audit ts=2026-10-23T05:31:12.480Z event=sparql proto=rest \
             token=sha256:3f2a1b0c9d8e tenant=C0001 q=sha256:9c1d2e3f4a5b rows=12 \
             elapsed_ms=37 code=OK"
        );

        // 失败：rows 用 `-`，code 是错误码（不是 OK）。
        let bad = AuditRecord {
            rows: None,
            code: "UNKNOWN_PROPERTY",
            ..rec.clone()
        };
        let line = bad.line();
        assert!(line.contains("rows=- "), "{line}");
        assert!(line.ends_with("code=UNKNOWN_PROPERTY"), "{line}");

        // note 只在有值时出现（静态短语，不是自由文本）。
        let sse = AuditRecord {
            event: "sse",
            note: Some("idle_timeout"),
            ..rec
        };
        assert!(sse.line().ends_with("code=OK note=idle_timeout"), "{}", sse.line());
    }

    /// ⭐ REQ-145 · R-2：`pending=true` 是**独立字段**，⛔ 不许拿 `code` 表达两件事。
    ///
    /// 判据三条：
    /// ① 不是半个答案时**一个字节都不变**（既有行形状不受影响）；
    /// ② 是半个答案时多一个 ` pending=true`，且位置固定在 `code=` 之后；
    /// ③ `code` 仍然是「客户端实际收到的那个」（`OK` / `NOT_DB_SOURCE`），
    ///   ⛔ 不是内部状态名（变异：写 `code="API_PENDING"` ⇒ ③ 红）。
    #[test]
    fn the_pending_marker_is_a_field_not_a_code() {
        let base = AuditRecord {
            ts: "2026-10-25T06:00:00.000Z".into(),
            event: "sparql",
            proto: "rest",
            token_digest: Some("sha256:3f2a1b0c9d8e".into()),
            tenant: Some("C0001".into()),
            query_digest: Some("sha256:9c1d2e3f4a5b".into()),
            rows: Some(0),
            elapsed_ms: 1,
            code: "OK",
            view: None,
            bytes: None,
            note: None,
            pending: false,
        };
        assert!(!base.line().contains("pending"), "{}", base.line());

        let half = AuditRecord {
            pending: true,
            ..base.clone()
        };
        assert!(
            half.line().ends_with("code=OK pending=true"),
            "半个答案要标 pending=true：{}",
            half.line()
        );

        // 客户端收到的是错误 ⇒ `code` 就是那个错误码（不是 `API_PENDING`）。
        let rejected = AuditRecord {
            pending: true,
            code: "NOT_DB_SOURCE",
            rows: None,
            ..base
        };
        let line = rejected.line();
        assert!(line.contains("code=NOT_DB_SOURCE"), "{line}");
        assert!(line.contains("pending=true"), "{line}");
        assert!(
            !line.contains("API_PENDING"),
            "⛔ code 不许是内部状态名（R-2）：{line}"
        );
    }

    /// ⭐ REQ-127 · TTL 出口的审计行：`view=` / `bytes=` 只在有值时出现，
    /// 且位置固定（`code=` 之后、`note=` 之前）—— 运维 grep 的口径不变。
    #[test]
    fn the_ttl_line_carries_view_and_bytes() {
        let rec = AuditRecord {
            ts: "2026-10-23T05:31:12.480Z".into(),
            event: "ttl",
            proto: "rest",
            token_digest: Some("sha256:3f2a1b0c9d8e".into()),
            tenant: Some("C0001".into()),
            query_digest: None,
            rows: None,
            elapsed_ms: 3,
            code: "OK",
            view: Some("model"),
            bytes: Some(432_101),
            note: None,
            // ⭐ REQ-145 · 不是半个答案（那个事实由 audit_pending 写 pending=true）。
            pending: false,
        };
        let line = rec.line();
        assert!(
            line.ends_with("code=OK view=model bytes=432101"),
            "TTL 审计行的形状不对：{line}"
        );
        // 304：bytes=0；code 仍是可数的事实。
        let not_modified = AuditRecord {
            code: "NOT_MODIFIED",
            bytes: Some(0),
            ..rec.clone()
        };
        assert!(
            not_modified.line().ends_with("code=NOT_MODIFIED view=model bytes=0"),
            "{}",
            not_modified.line()
        );
        // 其它出口不带这两个字段时，行形状与 REQ-123 完全一致（⛔ 不破坏既有 grep）。
        let plain = AuditRecord {
            view: None,
            bytes: None,
            ..rec
        };
        assert!(!plain.line().contains("view="), "{}", plain.line());
        assert!(!plain.line().contains("bytes="), "{}", plain.line());
    }

    /// ⭐ AC-007 的两条否定判据：行里**不许**出现 token 原文与查询原文。
    ///
    /// 变异：把 `token_digest` 换成 `Some(token.clone())` / 把 `query_digest`
    /// 换成查询原文 ⇒ 本测试红。
    #[test]
    fn the_line_never_carries_the_token_or_the_query_text() {
        let token = "sk-REAL-TOKEN-0123456789abcdef";
        let query = "SELECT ?s WHERE { ?s :customers__id ?v }";
        let rec = AuditRecord {
            ts: "2026-10-23T05:31:12.480Z".into(),
            event: "sparql",
            proto: "mcp",
            token_digest: Some(format!("sha256:{}", digest_prefix(&sha256_hex(token.as_bytes()), 12))),
            tenant: Some("C0001".into()),
            query_digest: Some(format!("sha256:{}", digest_prefix(&sha256_hex(query.as_bytes()), 16))),
            rows: Some(3),
            elapsed_ms: 5,
            code: "OK",
            view: None,
            bytes: None,
            note: None,
            // ⭐ REQ-145 · 不是半个答案（那个事实由 audit_pending 写 pending=true）。
            pending: false,
        };
        let line = rec.line();
        assert!(!line.contains(token), "⛔ 审计里出现了 token 明文：{line}");
        assert!(!line.contains("customers__id"), "⛔ 审计里出现了查询原文：{line}");
        assert!(line.contains("token=sha256:"), "{line}");
        assert!(line.contains("q=sha256:"), "{line}");
    }

    /// 文件落点：真的写进去了，且**追加**（不覆盖历史）。
    #[test]
    fn the_file_sink_appends_instead_of_truncating() {
        let dir = std::env::temp_dir().join(format!(
            "ontok-req123-audit-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("建临时目录");
        let path = dir.join("audit.log");

        let rec = AuditRecord {
            ts: "t".into(),
            event: "catalog",
            proto: "rest",
            token_digest: None,
            tenant: None,
            query_digest: None,
            rows: Some(0),
            elapsed_ms: 1,
            code: "OK",
            view: None,
            bytes: None,
            note: None,
            // ⭐ REQ-145 · 不是半个答案（那个事实由 audit_pending 写 pending=true）。
            pending: false,
        };
        {
            let sink = AuditSink::open(&path).expect("打开审计文件");
            sink.record(&rec);
        }
        {
            let sink = AuditSink::open(&path).expect("再次打开（追加）");
            sink.record(&rec);
        }
        let text = std::fs::read_to_string(&path).expect("读回审计文件");
        assert_eq!(
            text.lines().count(),
            2,
            "第二次打开必须追加而不是截断：{text:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

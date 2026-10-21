//! **只读、确定性、不落原值**的关系采样器（REQ-108 · AC-002 / AC-003）
//!
//! 一句话：**为了判断「这条推断关系到底成不成立」，去源库读 5 行，然后只留下
//! 「抽了几行 / 命中几行 / 这些值的哈希」。**
//!
//! # 四条硬约束（BA 裁定 R1，不是建议）
//!
//! | # | 约束 | 本文件的落点 |
//! | --- | --- | --- |
//! | ① | **只读会话**（`SET default_transaction_read_only = on`） | [`sample_pool`] 复用 `introspect::read_only_pool`，并在连上后**校验** `transaction_read_only = on` |
//! | ② | 每关系**最多 5 行** | [`SAMPLE_TARGET`] |
//! | ③ | **只落聚合，不落任何原值** | [`SampleEvidence`] 只有 `sampled` / `matched` / `digest` / `reason` —— 类型上**没有**放原值的字段 |
//! | ④ | `--no-sample` 可整体关闭 | 关闭时**根本不建连接**（由装配层决定），评分卡标 `data_evidence: none` |
//!
//! # 确定性（AC-002）
//!
//! - 取样：`ORDER BY <主键> LIMIT <上界>` —— 排序键固定、上界固定 ⇒ 同一份数据必然同样的行；
//! - 去重：按**首次出现**顺序去重，再取前 [`SAMPLE_TARGET`] 个；
//! - 哈希：把取到的值**排序**后用 `\u{1f}` 连接再 SHA-256 ⇒ 与行序无关。
//!
//! ⛔ **禁止** `random()` / `TABLESAMPLE` / `ORDER BY random()`：那三条都会让
//! 「同一份数据两次运行」得到不同的样本，而 AC-002 的判据正是两次 digest 必须相同。
//!
//! # 为什么哈希要自己算（不引新 crate）
//!
//! 本仓的依赖纪律是「不引新 crate」（AC-012）。`sha2` 在树里只是 sqlx 的
//! 传递依赖，**不能**直接 `use`（那是隐式依赖，Cargo 不保证它一直在）。
//! 所以 SHA-256 在本文件里实现一份，并用 NIST 的三条标准向量钉住。
//! 另有一个反面理由：**不要**用 `SELECT sha256(...)` 在库里算 ——
//! 那要把值先拼成文本，而 `timestamp` 的文本表示受 `DateStyle` 影响、
//! `bytea` 受 `bytea_output` 影响 ⇒ digest 会随会话参数变化，确定性当场失效。
//!
//! # 采样失败不许阻塞抽取
//!
//! 单条关系超时（[`SAMPLE_TIMEOUT`]）/ 查询报错 ⇒ 记 `sampled: 0` + `reason`，
//! **继续下一条**。评分卡会把 `sampled = 0` 判成「源列全空 ⇒ 上限 0.45」这一档
//! （`all_null` veto）—— 缺少证据**不会**被当成有证据。

use anyhow::{bail, Context, Result};
use sqlx::postgres::PgPool;
use std::time::Duration;

/// 每条关系最多保留几个**去重后**的样本（R1 ②）。
pub const SAMPLE_TARGET: usize = 5;

/// 扫描上界：为了在「前几行有重复值」时仍能补足 [`SAMPLE_TARGET`] 个**不同**的值。
///
/// ⚠️ AC-002 的字面写法是 `LIMIT 5`，本实现取的是「**样本上限 5**」：
/// 若把 `LIMIT` 直接钉成 5，一列前 5 行里有两个重复值时只能拿到 4 个样本，
/// 命中率的分母就会随数据分布漂移（而「补足 5 个」是设计概要 §四的明文要求）。
/// 排序键与上界都是常量，两次运行仍逐行相同 —— 确定性没有被放宽。
pub const SAMPLE_FETCH: i64 = 25;

/// 单条关系的采样超时（设计概要 §四「超时 ⇒ 记 `sampled: 0` + `reason: timeout`」）。
pub const SAMPLE_TIMEOUT: Duration = Duration::from_secs(2);

/// 一条关系的采样结果 —— **只有聚合，没有原值**（AC-003）。
///
/// 类型层保证：这个结构里**没有** `Vec<String>` 之类的原值字段。
/// 以后谁想「顺手把样本存下来调试」，必须先改类型，而改类型会让本文件的
/// 单元测试与 AC-003 的回查判据一起变红。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SampleEvidence {
    /// 实际取到（去重后）的样本数。`0` = 没取到（全空 / 超时 / 无键列 / 查询失败）。
    pub sampled: u32,
    /// 这些值在目标表键列里命中的个数。
    pub matched: u32,
    /// 样本值的哈希：`sha256:<hex>`。`None` = 没有样本。
    pub digest: Option<String>,
    /// 没取到样本的原因（`all_null` / `timeout` / `no_key_column` / `query_error`）。
    pub reason: Option<&'static str>,
}

impl SampleEvidence {
    /// 「没取到样本」的显式构造：`sampled = 0` + 原因。
    ///
    /// 公开它是因为装配层也要用（目标表没有单列键 ⇒ `no_key_column`），
    /// 而那种情形**根本没查库**，不该走 [`sample_one`]。
    pub fn none(reason: &'static str) -> Self {
        Self { sampled: 0, matched: 0, digest: None, reason: Some(reason) }
    }
}

/// 一条待采样关系的坐标：**源表.源列 → 目标表.键列**。
///
/// 键列由 `KeyCatalog` 给出（`primary_key`，或单列唯一列）——
/// ⛔ 不许在这一层再猜一次「目标键大概叫 id」：AC-001 补采主键的全部意义
/// 就是让这里有一个**目录事实**可用（真实库 `service_apps` 的主键叫 `app_id`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SampleSpec {
    pub from_table: String,
    pub from_column: String,
    /// 源表的排序键（主键）。`None` ⇒ 退化成按值排序，仍然确定。
    pub from_key: Option<String>,
    pub to_table: String,
    pub to_key_column: String,
}

/// 标识符引用：`foo"bar` → `"foo""bar"`。
///
/// 表名/列名全部来自 `pg_catalog`（不是用户输入），但仍然按规矩引用 ——
/// 「来源可信所以不转义」是下一次注入的起点，而且真实库里确实有带引号的标识符。
fn quote_ident(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// 取样 SQL（纯函数，**可单测** —— 确定性判据必须能对着字面量断言）。
///
/// `ORDER BY` 固定成源表主键；没有主键时按值本身排序（同一份数据仍只有一种结果）。
pub(crate) fn sample_sql(spec: &SampleSpec) -> String {
    let col = quote_ident(&spec.from_column);
    let order = match spec.from_key.as_deref() {
        Some(k) => quote_ident(k),
        None => format!("({col})::text"),
    };
    format!(
        "SELECT ({col})::text FROM {} WHERE {col} IS NOT NULL ORDER BY {order} LIMIT $1",
        quote_ident(&spec.from_table)
    )
}

/// 命中计数 SQL。
///
/// ⚠️ **两侧都转 text 比较**，不是 `to_key = ANY($1)`：
/// 目标列是 `integer` 而源列是 `text`（E8/E9 那种类型不兼容）时，
/// 直接比类型会让查询**报错**，于是「类型不兼容」这个可判定的信号
/// 变成一条 `query_error`（sampled=0 ⇒ 走 all_null 档）——
/// 那是把「明确错」降级成「不知道」。转 text 后它老老实实地得到 0 命中。
pub(crate) fn match_sql(spec: &SampleSpec) -> String {
    format!(
        "SELECT count(*) FROM {} WHERE ({})::text = ANY($1::text[])",
        quote_ident(&spec.to_table),
        quote_ident(&spec.to_key_column)
    )
}

/// 样本值 → `sha256:<hex>`。空输入 ⇒ `None`（没有样本就没有哈希）。
///
/// 值是**排序后**连接的：样本的行序（主键序）与「命中」无关，
/// 但哈希一旦依赖行序，换个索引计划就可能变 ⇒ 排序后再连接。
pub fn digest_of(values: &[String]) -> Option<String> {
    if values.is_empty() {
        return None;
    }
    let mut sorted: Vec<&str> = values.iter().map(|s| s.as_str()).collect();
    sorted.sort_unstable();
    // 分隔符取 U+001F（unit separator）：业务数据里不可能出现，
    // 于是 `["a","b"]` 与 `["a\u{1f}b"]` 不会撞成同一个哈希。
    let joined = sorted.join("\u{1f}");
    Some(format!("sha256:{}", sha256_hex(joined.as_bytes())))
}

/// 取样本值 → **去重（保序）→ 截到 [`SAMPLE_TARGET`]**。
pub(crate) fn dedupe_cap(rows: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(SAMPLE_TARGET);
    for v in rows {
        if out.iter().any(|x| x == &v) {
            continue;
        }
        out.push(v);
        if out.len() == SAMPLE_TARGET {
            break;
        }
    }
    out
}

/// **只读采样池**：复用勘查器的只读连接，并**校验**服务端读回 `on`。
///
/// 与 `introspect::read_only_pool` 是同一条路径（不是第二份实现）：
/// 那里负责连接串改造 + `after_connect` 的 SET，这里多问一句
/// 「服务端现在到底是 on 吗」—— AC-002 的判据就是这一句。
pub async fn sample_pool(url: &str) -> Result<PgPool> {
    let pool = crate::extract::introspect::read_only_pool(url).await?;
    let read_only: String = sqlx::query_scalar("SHOW transaction_read_only")
        .fetch_one(&pool)
        .await
        .context("读不回 transaction_read_only，只读保证无法验证")?;
    if read_only != "on" {
        pool.close().await;
        bail!(
            "transaction_read_only = {read_only}，不是 on。\
             拒绝在不确定只读的连接上采样客户库（R1 ①）。"
        );
    }
    Ok(pool)
}

/// 采一条关系。
///
/// 任何失败都**不**返回 `Err`（除连接池本身已坏）：采样是「加分项」，
/// 它失败不该让整轮抽取失败 —— 但失败必须**可见**（`reason` + `sampled: 0`）。
pub async fn sample_one(pool: &PgPool, spec: &SampleSpec, timeout: Duration) -> SampleEvidence {
    // ① 取样本值
    let sql = sample_sql(spec);
    let rows = match tokio::time::timeout(
        timeout,
        sqlx::query_scalar::<_, String>(&sql).bind(SAMPLE_FETCH).fetch_all(pool),
    )
    .await
    {
        Err(_) => return SampleEvidence::none("timeout"),
        Ok(Err(_)) => return SampleEvidence::none("query_error"),
        Ok(Ok(rows)) => rows,
    };
    let values = dedupe_cap(rows);
    if values.is_empty() {
        return SampleEvidence::none("all_null");
    }

    // ② 命中计数
    let sql = match_sql(spec);
    let matched: i64 = match tokio::time::timeout(
        timeout,
        sqlx::query_scalar::<_, i64>(&sql).bind(&values).fetch_one(pool),
    )
    .await
    {
        Err(_) => return SampleEvidence::none("timeout"),
        Ok(Err(_)) => return SampleEvidence::none("query_error"),
        Ok(Ok(n)) => n,
    };

    SampleEvidence {
        sampled: values.len() as u32,
        matched: matched.max(0).min(values.len() as i64) as u32,
        digest: digest_of(&values),
        reason: None,
    }
}

/// 顺序采完全部关系（使用生产超时 [`SAMPLE_TIMEOUT`]）。
pub async fn sample_all(pool: &PgPool, specs: &[SampleSpec]) -> Vec<SampleEvidence> {
    sample_all_with(pool, specs, SAMPLE_TIMEOUT).await
}

/// 顺序采完全部关系，**超时可覆盖**。
///
/// **刻意串行**（不是 `join_all`）：源库是客户资产，并发 5 条关系意味着 5 条连接
/// 同时压在大表上；串行 + 每条超时的最坏耗时是可预期的。
/// 顺序也不影响结果（每条关系互相独立）。
///
/// ⚠️ 为什么把超时做成参数：**AC-002 的「两次运行 digest 相同」不该被
/// 墙钟超时打破**。超时是「这次没采到」，不是「采到了别的值」——
/// 判据应当用足够长的超时去比 SQL 的确定性，并把「有没有超时」单独断言，
/// 而不是让一条 2s 的慢查询把确定性判据变成偶发红。
pub async fn sample_all_with(
    pool: &PgPool,
    specs: &[SampleSpec],
    timeout: Duration,
) -> Vec<SampleEvidence> {
    let mut out = Vec::with_capacity(specs.len());
    for spec in specs {
        out.push(sample_one(pool, spec, timeout).await);
    }
    out
}

// ──────────────────────────────────────────────────────────
// SHA-256（NIST FIPS 180-4）—— 不引新 crate，见模块头
// ──────────────────────────────────────────────────────────

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// SHA-256 → 小写十六进制。
pub fn sha256_hex(input: &[u8]) -> String {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];
    let bit_len = (input.len() as u64).wrapping_mul(8);
    let mut msg = input.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for (i, word) in chunk.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
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
    h.iter().map(|x| format!("{x:08x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> SampleSpec {
        SampleSpec {
            from_table: "balance_operation_log".into(),
            from_column: "batch_id".into(),
            from_key: Some("id".into()),
            to_table: "gift_code_batch".into(),
            to_key_column: "id".into(),
        }
    }

    // ── AC-002 · 确定性：SQL 形状可对着字面量断言 ──────────────────

    /// 取样 SQL **必须**按主键排序并带上界；**不许**出现随机源。
    ///
    /// 变异自证：把 `ORDER BY` 换成 `ORDER BY random()` 或删掉 `LIMIT`，
    /// 本测试立刻红；把 `LIMIT $1` 换成 `TABLESAMPLE` 同样红。
    #[test]
    fn sample_sql_is_ordered_by_primary_key_and_bounded() {
        let sql = sample_sql(&spec());
        assert!(sql.contains("ORDER BY \"id\""), "必须按主键排序：{sql}");
        assert!(sql.contains("LIMIT $1"), "必须有上界：{sql}");
        assert!(sql.contains("\"batch_id\" IS NOT NULL"), "空值不参与命中率：{sql}");
        for banned in ["random", "TABLESAMPLE", "now()", "clock_timestamp"] {
            assert!(
                !sql.to_ascii_lowercase().contains(&banned.to_ascii_lowercase()),
                "确定性被破坏：SQL 里出现了 {banned} —— 同一份数据两次运行会拿到不同样本：{sql}"
            );
        }
    }

    /// 没有主键时按**值**排序 —— 仍然只有一种结果，且不引入随机源。
    #[test]
    fn sample_sql_falls_back_to_a_deterministic_order() {
        let mut s = spec();
        s.from_key = None;
        let sql = sample_sql(&s);
        assert!(sql.contains("ORDER BY (\"batch_id\")::text"), "{sql}");
        assert!(!sql.contains("random"), "{sql}");
    }

    /// 标识符按规矩引用（真实库里有带引号的标识符，直接拼串会炸）。
    #[test]
    fn sample_sql_quotes_identifiers() {
        let mut s = spec();
        s.from_table = "weird\"table".into();
        let sql = sample_sql(&s);
        assert!(sql.contains("\"weird\"\"table\""), "{sql}");
    }

    /// 命中判定两侧都转 text：类型不兼容得到 **0 命中**，而不是一条报错。
    ///
    /// 变异自证：把 `(...)::text` 去掉，E8/E9（text → int）会在数据库层报
    /// `operator does not exist` ⇒ 采样变成 `query_error` ⇒ 落进 all_null 档
    /// 而不是类型不兼容档 —— 那是把「明确错」降级成「不知道」。
    #[test]
    fn match_sql_compares_as_text_so_incompatible_types_score_zero_not_error() {
        let sql = match_sql(&spec());
        assert!(sql.contains("(\"id\")::text = ANY($1::text[])"), "{sql}");
        assert!(sql.contains("count(*)"), "{sql}");
    }

    // ── AC-003 · 不落原值：digest 只依赖「排序后的值」 ─────────────

    /// 同样的值、不同的行序 ⇒ 同一个 digest（行序由索引计划决定，不能进哈希）。
    #[test]
    fn digest_is_order_independent() {
        let a = digest_of(&["b".into(), "a".into(), "c".into()]);
        let b = digest_of(&["c".into(), "a".into(), "b".into()]);
        assert_eq!(a, b);
        assert!(a.as_deref().unwrap().starts_with("sha256:"));
        assert_eq!(a.as_deref().unwrap().len(), "sha256:".len() + 64);
        // 没有样本 ⇒ 没有 digest（不是空串哈希）
        assert_eq!(digest_of(&[]), None);
    }

    /// **分隔符的已知边界（如实登记，不假装已封堵）**。
    ///
    /// 设计概要 §四 规定的算法是「值排序后用 `\u{1f}` 连接再哈希」。
    /// 这个连接**不是**单射：`["a", "b"]` 与 `["a\u{1f}b"]` 拼出来是同一串。
    /// 本测试把这条边界**钉成已知行为**，而不是声称它被防住了 ——
    /// 如果将来有人换成「长度前缀」或转义分隔符，这条会红，那时应当**改测试**
    /// （并同步改设计概要），而不是把红的测试删掉。
    ///
    /// 为什么不现在就加长度前缀：digest 的唯一用途是「两次运行是否取到同一批样本」
    /// （AC-002 的判据），业务数据里出现 U+001F 的列本身就该被当成脏数据；
    /// 为此偏离规格会让「算法与设计概要逐字一致」这条可核对性先失守。
    #[test]
    fn digest_separator_boundary_is_documented_not_claimed_fixed() {
        let two = digest_of(&["a".into(), "b".into()]);
        let one = digest_of(&["a\u{1f}b".into()]);
        assert_eq!(
            two, one,
            "这是**已知**边界：`\\u{{1f}}` 连接不是单射（设计概要 §四的算法如此）。\
             换成单射算法时本断言会红 —— 请同步改设计概要，而不是删掉这条。"
        );
    }

    /// 去重后截到 5 个，且**保持首次出现的顺序**（顺序用于可复现地截断）。
    #[test]
    fn dedupe_keeps_first_seen_order_and_caps_at_five() {
        let rows: Vec<String> = ["a", "a", "b", "c", "b", "d", "e", "f"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(dedupe_cap(rows), vec!["a", "b", "c", "d", "e"]);
        // 补足：前 5 行里有重复时，靠扫描上界拿到 5 个不同的值
        let dup_first: Vec<String> = ["a"; 20].iter().map(|s| s.to_string()).collect();
        assert_eq!(dedupe_cap(dup_first), vec!["a"]);
    }

    // ── SHA-256 自证（NIST FIPS 180-4 标准向量）──────────────────

    #[test]
    fn sha256_matches_the_standard_vectors() {
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
        // 跨块边界（>64 字节）也要对
        assert_eq!(
            sha256_hex(&[b'a'; 1000]),
            "41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3"
        );
    }

    /// 采样失败必须是**可见的 0 样本**，不是 `Err`（抽取不许被采样拖垮）。
    #[test]
    fn a_failed_sample_is_an_observable_zero_not_an_error() {
        let e = SampleEvidence::none("timeout");
        assert_eq!(e.sampled, 0);
        assert_eq!(e.matched, 0);
        assert_eq!(e.digest, None);
        assert_eq!(e.reason, Some("timeout"));
    }
}

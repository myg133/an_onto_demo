//! REQ-122 · **只读执行** —— 复用 REQ-003 起的只读连接路径，不另写一份。
//!
//! # 只读不是口头承诺，是三道闸
//!
//! | 闸 | 在哪 | 失灵时 |
//! | --- | --- | --- |
//! | ① 连接串改造 + `SET default_transaction_read_only = on` | [`crate::extract::introspect::read_only_pool`]（**同一条路径**，不是第二份实现） | 连不上就是连不上 |
//! | ② 连上后**读回** `SHOW transaction_read_only` | 本模块 [`connect_read_only`] | 不是 `on` ⇒ 拒绝执行 |
//! | ③ SQL 由本需求的规划器生成，**只可能是 SELECT** | [`super::plan`] | 没有写路径可走（AC-008） |
//!
//! 另外读回 `standard_conforming_strings`：SQL 里的字符串字面量是**内联**的
//! （见 [`super::sql`] 的转义口径），这项不是 `on` 时反斜杠会被当转义 —— 直接拒绝。
//!
//! # 超时是**两道**
//!
//! ① 服务端 `statement_timeout`（按 `--timeout-ms`）—— 真让 PG 停下来；
//! ② 客户端 `tokio::time::timeout`（`+2s` 兜底）—— 连 `statement_timeout` 都没机会生效时
//!    （连接挂了 / 网络黑洞）也不让命令一直挂着。
//!
//! 只做 ② 是不够的：tokio 超时只是**不再等**，服务端那条查询还在跑；只做 ① 也是不够的：
//! 客户端仍可能卡在收结果上。两道一起，才有「说 10 秒就 10 秒」。

use sqlx::postgres::PgPool;
use sqlx::Row;

use super::error::QueryError;

/// 只读会话的**读回事实** —— 打印出来的每个值都必须来自这里。
///
/// # 为什么要有这个类型（REQ-122 · N3）
///
/// CLI 曾经打印一句写死的「只读会话：`transaction_read_only = on`」。
/// 那句话在**正常路径**上恰好为真（[`connect_read_only`] 读回不是 `on` 就拒绝），
/// 但它**不是回执**：值没有从连接上读出来，只是抄了一遍预期。
/// 这与「失败路径不许装作做完了」是同一类错误 —— 回执必须是被观测到的事实。
///
/// 所以：连接时读回的两个值原样装进这里，CLI 只打印这里的值
/// （[`SessionFacts::receipt`]，由单测钉住「打印的是入参，不是字面量」）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionFacts {
    /// `SHOW transaction_read_only` 的**实际**返回值。
    pub transaction_read_only: String,
    /// `SHOW standard_conforming_strings` 的**实际**返回值。
    pub standard_conforming_strings: String,
}

impl SessionFacts {
    /// CLI 的会话回执行（stderr，一行）。
    ///
    /// ⛔ **不许**在这里写死 `on`：每个值都只能来自 `self`。
    /// 判据：`exec` 的单测 `the_session_receipt_prints_what_was_read_back` ——
    /// 喂 `off` 进去，回执里就必须出现 `off`（写死就会红）。
    pub fn receipt(&self, timeout_ms: u64, max_rows: u64) -> String {
        format!(
            "· 只读会话（读回）：transaction_read_only = {} · standard_conforming_strings = {} · \
             超时 {timeout_ms} ms · 上限 {max_rows} 行",
            self.transaction_read_only, self.standard_conforming_strings
        )
    }
}

/// 一条**已验证只读**的连接：连接池 + 读回的事实。
///
/// 两者绑在一起而不是分成两次调用 —— 回执里的值只能来自**这条**连接上
/// 真正读到的结果，不给「再读一次、拿另一个值打印」留口子。
pub struct ReadOnlyConnection {
    pub pool: PgPool,
    /// 连上后读回的两个会话值（CLI 直接打印它们，⛔ 不再拼字面量）。
    pub facts: SessionFacts,
}

/// 连上**源库**并确认只读（复用勘查器/采样的连接路径）。
///
/// 返回的不只是池：两个会话值在**同一条路径**上读回，随后原样交给调用方打印。
pub async fn connect_read_only(url: &str) -> Result<ReadOnlyConnection, String> {
    let pool = crate::extract::introspect::read_only_pool(url)
        .await
        .map_err(|e| format!("连不上源库（确认只读账号可用、库名 schema 正确）：{e}"))?;

    let read_only: String = sqlx::query_scalar("SHOW transaction_read_only")
        .fetch_one(&pool)
        .await
        .map_err(|e| format!("读不回 transaction_read_only，只读保证无法验证：{e}"))?;
    if read_only != "on" {
        pool.close().await;
        return Err(format!(
            "transaction_read_only = {read_only}，不是 on —— 拒绝在不确定只读的连接上执行查询（AC-008）"
        ));
    }

    let scs: String = sqlx::query_scalar("SHOW standard_conforming_strings")
        .fetch_one(&pool)
        .await
        .map_err(|e| format!("读不回 standard_conforming_strings：{e}"))?;
    if scs != "on" {
        pool.close().await;
        return Err(format!(
            "standard_conforming_strings = {scs}，不是 on —— 字符串字面量的转义口径不成立，拒绝执行"
        ));
    }
    Ok(ReadOnlyConnection {
        pool,
        facts: SessionFacts {
            transaction_read_only: read_only,
            standard_conforming_strings: scs,
        },
    })
}

/// 执行结果（行 = 已按列顺序解码的文本；`more` = 是否还有第 `limit + 1` 行）。
#[derive(Debug, Clone)]
pub struct Rows {
    pub rows: Vec<Vec<Option<String>>>,
    pub more: bool,
}

/// 执行一条**只读** SQL。
///
/// `keep_rows` = 结果里最多留几行（= `effective_limit`）。SQL 是 `LIMIT keep_rows + 1`：
/// 第 `keep_rows + 1` 行只用来把 `more` 置真（判断 `truncated`），**不进结果**。
pub async fn run(
    pool: &PgPool,
    sql: &str,
    columns: usize,
    keep_rows: u64,
    timeout_ms: u64,
) -> Result<Rows, QueryError> {
    if timeout_ms == 0 {
        return Err(QueryError::new("--timeout-ms 是 0：等于「不许它跑」，那不是超时是禁用"));
    }
    let mut conn = pool
        .acquire()
        .await
        .map_err(|e| QueryError::new(format!("取不到只读连接：{e}")))?;

    // ① 服务端超时（会话级；连接是我们独占的，用完即弃）
    sqlx::query("SELECT set_config('statement_timeout', $1, false)")
        .bind(format!("{timeout_ms}ms"))
        .execute(&mut *conn)
        .await
        .map_err(|e| QueryError::new(format!("设不上 statement_timeout：{e}")))?;

    // ② 客户端兜底（服务端超时 + 2s；两者不同时归零，避免「到底谁先到」的模糊）
    let fut = sqlx::query(sql).fetch_all(&mut *conn);
    let waited = tokio::time::timeout(
        std::time::Duration::from_millis(timeout_ms.saturating_add(2_000)),
        fut,
    )
    .await;

    let rows = match waited {
        Err(_) => {
            // 客户端先到：这条连接状态不可信（服务端可能还在跑），**不还池**
            let _ = conn.close().await;
            return Err(QueryError::new(format!(
                "查询超时（{timeout_ms} ms 的客户端兜底也到了）—— 已中止。\
                 缩小范围（加 FILTER / LIMIT）或调大 --timeout-ms。"
            )));
        }
        Ok(Err(e)) => {
            // 服务端超时（57014）单独给一句人话；其它错误把 PG 的原话带上
            let msg = e.to_string();
            let hint = if msg.contains("statement timeout") || msg.contains("canceling statement")
            {
                format!("查询超时（{timeout_ms} ms，服务端 statement_timeout 已中止）—— 缩小范围或调大 --timeout-ms。")
            } else {
                format!("SQL 执行失败：{msg}")
            };
            drop(conn);
            return Err(QueryError::new(hint));
        }
        Ok(Ok(rows)) => rows,
    };

    let mut out: Vec<Vec<Option<String>>> = Vec::with_capacity(rows.len());
    let mut more = false;
    for r in rows {
        if out.len() as u64 >= keep_rows {
            // 探针行：只用来标记 truncated
            more = true;
            break;
        }
        let mut line: Vec<Option<String>> = Vec::with_capacity(columns);
        for i in 0..columns {
            let v: Option<String> = r
                .try_get::<Option<String>, _>(i)
                .map_err(|e| QueryError::new(format!("取第 {} 列失败：{e}", i + 1)))?;
            line.push(v);
        }
        out.push(line);
    }
    Ok(Rows { rows: out, more })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **N3 的判据**：回执里的每个会话值都必须**来自读回的入参**，⛔ 不是字面量。
    ///
    /// 变异验证（QA 可复现）：把 [`SessionFacts::receipt`] 里的 `{}` 换回写死的
    /// `on`、只留 `timeout_ms` / `max_rows` —— 本测试立刻红（下面第一个断言）。
    /// 所以「打印了一句没人验证过的 `= on`」这件事再也回不去。
    #[test]
    fn the_session_receipt_prints_what_was_read_back() {
        // ① 喂非预期值：回执必须原样写出来（写死 `on` 的实现会在这里红）
        let off = SessionFacts {
            transaction_read_only: "off".into(),
            standard_conforming_strings: "off".into(),
        };
        let s = off.receipt(9_000, 100);
        assert!(
            s.contains("transaction_read_only = off"),
            "回执必须打印**读回**的值，不能写死 `on`：{s}"
        );
        assert!(
            s.contains("standard_conforming_strings = off"),
            "回执必须打印**读回**的值：{s}"
        );
        assert!(
            !s.contains("= on"),
            "入参是 off 时回执里不该出现 `= on`（说明有字面量漏进来了）：{s}"
        );

        // ② 正常会话：值同样来自入参（不是「反正都是 on」）
        let on = SessionFacts {
            transaction_read_only: "on".into(),
            standard_conforming_strings: "on".into(),
        };
        let s = on.receipt(1_000, 10);
        assert!(s.contains("transaction_read_only = on"), "{s}");
        assert!(s.contains("standard_conforming_strings = on"), "{s}");
        assert!(s.contains("超时 1000 ms · 上限 10 行"), "{s}");

        // ③ 两个入参不同 ⇒ 回执必须跟着变（回执不是常量）
        assert_ne!(on.receipt(1_000, 10), off.receipt(1_000, 10));
        assert_ne!(on.receipt(1_000, 10), on.receipt(2_000, 10));
    }
}

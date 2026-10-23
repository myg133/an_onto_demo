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

/// 连上**源库**并确认只读（复用勘查器/采样的连接路径）。
pub async fn connect_read_only(url: &str) -> Result<PgPool, String> {
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
    Ok(pool)
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

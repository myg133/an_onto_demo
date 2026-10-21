//! **`ai_cache` 表**（REQ-109 · 指纹缓存）。
//!
//! ```sql
//! CREATE TABLE IF NOT EXISTS ai_cache (
//!     fingerprint   TEXT PRIMARY KEY,
//!     output_json   JSONB NOT NULL,        -- {semantic_fit, rationale, cites, choice}
//!     model         TEXT NOT NULL,
//!     prompt_sha    TEXT NOT NULL,
//!     tokens_used   INTEGER,
//!     created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
//! );
//! ```
//!
//! # 为什么表里**同时**存 `output_json` 与 `model/prompt_sha/tokens_used`
//!
//! 那三栏**不是**判断的内容，是**当初那次调用**的账：命中缓存时要如实报
//! 「这条判断是哪个模型判的、花了多少 token」（溯源），而这几个值无法从
//! `output_json` 反推（判断本身里没有它们）。把它们塞进 `output_json`
//! 会让「判断」与「调用元数据」混成一份，缓存回放时还得剥——
//! 分栏之后反序列化目标只有一个形状。
//!
//! # 为什么走「预加载 → 内存编排 → 回写」而不是「编排中逐条查库」
//!
//! 编排（`extract::ai_semantic::AiSemanticJudge`）是**同步**的，而 sqlx 是 async。
//! 在同步函数里 `block_on` 会在已有 runtime 上下文里 panic（ADR-002 · N3，
//! `agent::acp::mod` 记着这个坑）。所以 I/O 集中在编排**前后**两处：
//! [`PgAiCache::preload`] 与 [`PgAiCache::persist`]，中间那张
//! [`MemoryAiCache`] 是唯一的判断来源 —— 「命中绝不调用」因此在类型上成立
//! （编排器只看得见内存缓存，看不见网络）。
//!
//! ⚠️ **回写只写 `fresh`**：预加载进来但没被改写的条目不会被重写，
//! 免得每次运行都把 `created_at` 刷新一遍（那会让「这条缓存多老」不可读）。

use sqlx::postgres::PgPool;
use sqlx::Row;

use crate::extract::ai_semantic::{CachedEntry, CachedOutput, MemoryAiCache};

/// 建表 DDL（幂等）—— 由 `db::pg::migrate` 编进同一条 batch。
///
/// 与 `agent_registry::SCHEMA_AGENT_REGISTRY` 同一做法：DDL 只写一次，
/// 物理上仍在那条取了咨询锁的 batch 里（REQ-025 / REQ-039 的并发纪律）。
pub const SCHEMA_AI_CACHE: &str = r#"
-- REQ-109 · AI 语义判断的指纹缓存（成本闸的另一半：命中绝不调用）
CREATE TABLE IF NOT EXISTS ai_cache (
  fingerprint  TEXT PRIMARY KEY,
  output_json  JSONB NOT NULL,
  model        TEXT NOT NULL,
  prompt_sha   TEXT NOT NULL,
  tokens_used  INTEGER,
  created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
"#;

/// PG 版缓存（**只在编排前后**碰库，见模块头）。
#[derive(Clone)]
pub struct PgAiCache {
    pool: PgPool,
}

impl PgAiCache {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// 按指纹批量取。**一次往返**（`= ANY($1)`），不是逐条查。
    pub async fn preload(&self, fingerprints: &[String]) -> Result<MemoryAiCache, sqlx::Error> {
        if fingerprints.is_empty() {
            return Ok(MemoryAiCache::new());
        }
        let rows = sqlx::query(
            "SELECT fingerprint, output_json, model, prompt_sha, tokens_used
             FROM ai_cache WHERE fingerprint = ANY($1)",
        )
        .bind(fingerprints.to_vec())
        .fetch_all(&self.pool)
        .await?;

        let mut entries: Vec<(String, CachedEntry)> = Vec::with_capacity(rows.len());
        for row in rows {
            let fp: String = row.try_get("fingerprint")?;
            let raw: serde_json::Value = row.try_get("output_json")?;
            let output: CachedOutput = match serde_json::from_value(raw.clone()) {
                Ok(o) => o,
                // ⚠️ 兼容**同一 REQ 迭代期内**的旧行：本表刚引入时先存的是
                // 裸的判定对象（`{semantic_fit, rationale, cites, choice}`），
                // 后来才改成 `{judgement | rejected}`。
                // 只在「认得出是一份判定」时兼容；别的形状一律报错（fail-closed）。
                Err(_) if raw.get("semantic_fit").is_some() => CachedOutput {
                    judgement: Some(serde_json::from_value(raw).map_err(|e| {
                        sqlx::Error::Protocol(format!(
                            "ai_cache.output_json 是旧格式但读不懂（fingerprint={fp}）：{e}"
                        ))
                    })?),
                    rejected: None,
                },
                Err(e) => {
                    // 缓存里的 JSON 读不懂 = 这份缓存坏了。**报错**而不是「当没命中」：
                    // 后者会静默重算并覆盖，把「坏了」变成一次花钱的正常运行。
                    return Err(sqlx::Error::Protocol(format!(
                        "ai_cache.output_json 形状不合法（fingerprint={fp}）：{e}"
                    )));
                }
            };
            let tokens: Option<i32> = row.try_get("tokens_used")?;
            entries.push((
                fp,
                CachedEntry {
                    output,
                    model: row.try_get("model")?,
                    prompt_sha: row.try_get("prompt_sha")?,
                    // NULL ⇒ 0：这一条是「当初没报用量」的历史缓存。
                    // 不把它算成「免费」，只如实回放 0（报告里 tokens 总量会偏低，
                    // 这是唯一诚实的表示 —— 编一个数字更糟）。
                    tokens: tokens.unwrap_or(0).max(0) as u64,
                },
            ));
        }
        Ok(MemoryAiCache::with_entries(entries))
    }

    /// 把本轮**新写入**的条目 upsert 进表（`--refresh` 覆盖同指纹的旧值）。
    ///
    /// 返回写入条数（报告里打印，让「这一轮真的落了缓存」可见）。
    pub async fn persist(&self, cache: &MemoryAiCache) -> Result<usize, sqlx::Error> {
        if cache.fresh.is_empty() {
            return Ok(0);
        }
        let mut tx = self.pool.begin().await?;
        for (fp, entry) in &cache.fresh {
            let output = serde_json::to_value(&entry.output).map_err(|e| {
                sqlx::Error::Protocol(format!("ai_cache 回答的序列化失败：{e}"))
            })?;
            sqlx::query(
                "INSERT INTO ai_cache (fingerprint, output_json, model, prompt_sha, tokens_used)
                 VALUES ($1,$2,$3,$4,$5)
                 ON CONFLICT (fingerprint) DO UPDATE
                   SET output_json = EXCLUDED.output_json,
                       model       = EXCLUDED.model,
                       prompt_sha  = EXCLUDED.prompt_sha,
                       tokens_used = EXCLUDED.tokens_used,
                       created_at  = now()",
            )
            .bind(fp)
            .bind(output)
            .bind(&entry.model)
            .bind(&entry.prompt_sha)
            .bind(entry.tokens as i64)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(cache.fresh.len())
    }

    /// 表里有多少条（测试与报告用；**不删任何东西**）。
    pub async fn count(&self) -> Result<i64, sqlx::Error> {
        let row = sqlx::query("SELECT count(*) AS n FROM ai_cache")
            .fetch_one(&self.pool)
            .await?;
        row.try_get("n")
    }

    /// **只删某几个指纹**（测试用：验「删掉缓存后必须重新调用」，AC-003）。
    ///
    /// 为什么不是 `TRUNCATE`：这张表在实际部署里是**跨运行共享**的资产，
    /// 一个测试不该把别人跑出来的缓存全清了。
    pub async fn delete_fingerprints(&self, fingerprints: &[String]) -> Result<u64, sqlx::Error> {
        if fingerprints.is_empty() {
            return Ok(0);
        }
        let r = sqlx::query("DELETE FROM ai_cache WHERE fingerprint = ANY($1)")
            .bind(fingerprints.to_vec())
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected())
    }

    /// **按模型删** —— 只给测试与运维用（正常路径从不删缓存，只 upsert）。
    ///
    /// 测试用它隔离自己的缓存行：`model` 是测试给定的假模型名，
    /// 于是互不干扰，也不会碰到开发机上真实跑出来的行。
    pub async fn delete_by_model(&self, model: &str) -> Result<u64, sqlx::Error> {
        let r = sqlx::query("DELETE FROM ai_cache WHERE model = $1")
            .bind(model)
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected())
    }
}

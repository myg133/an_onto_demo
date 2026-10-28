//! ⭐ REQ-175 · **建项目发的 key 必须能鉴权** —— 静态 token 表未命中时**按摘要查库**。
//!
//! # 为什么要有这个模块
//!
//! `POST /api/projects` 会发一把 `ontok_q_<64 hex>`，并且把它的 `sha256` 写进
//! `project_keys.token_sha256`（[`crate::console::provision::create_project`]）。
//! 但在 REQ-175 之前，**运行期从不读这张表**：身份门只认 `--tokens` 那份静态 JSON，
//! 于是「建项目 → 发 key → 数字员工用」这条链在**发 key 那一步就断了** ——
//! 生产实测该 key 调 `GET /api/catalog` 得到 401。
//!
//! 本模块补的就是这一跳：
//!
//! ```text
//! 静态表没命中（TokenTable::find == None）
//!    ↓ sha256(token)              ← 与 provision 写库**同一个函数**（audit::sha256_hex）
//! SELECT … FROM project_keys WHERE token_sha256 = $1 AND revoked_at IS NULL
//!    ↓ 命中 ⇒ project_id → 项目名
//! TokenEntry { kind: Query, project: Some(<项目名>), … }
//! ```
//!
//! # 三条边界（每一条都是判据）
//!
//! 1. **未命中 / 已吊销 ⇒ `None`**（调用方回 **401**，⛔ 不是 403：403 会泄露
//!    「这把 key 存在，只是被吊销了」）；
//! 2. **静态表优先**：调用方先查 [`TokenTable::find`](super::tokens::TokenTable::find)，
//!    只有它返回 `None` 才进本模块 —— 静态表里的 token 行为一个字节都不变；
//! 3. **摘要只用于查库 / 审计**：本模块**不保留 token 原文**，返回的 [`TokenEntry`]
//!    的 `token` 字段是**空串**（见 `TokenEntry::from_db_key`）。
//!
//! # 缓存（60s · 键 = 摘要）
//!
//! 每请求都查库是不可接受的：`project_keys` 在**鉴权热路径**上，而一把 key 会被
//! 数字员工每个请求都带一次。所以命中的结果按 **`sha256(token)`** 缓存
//! [`KEY_CACHE_TTL`]（60s）：
//!
//! - 键是**摘要**，缓存里没有原文（与审计同一个口径）；
//! - 只缓存**命中**（`revoked_at IS NULL` 的那一行）：未命中不缓存 —— 否则
//!   「刚建好项目、刚发的 key」会被一次更早的探测挡在门外；
//! - **吊销的可见性 ≤ TTL**：`revoked_at` 置值后，最迟一次 TTL 过期就会变 401。
//!   这是**刻意的权衡**（每请求查库 vs 多等最多 60s），AC-004 就是按「≤ TTL 内
//!   变 401」写的；判据用短 TTL 的构造器（[`DbKeyAuth::with_ttl`]）把这件事钉死。
//!
//! # 已知边界（⛔ 不写成「已封堵」）
//!
//! - `project_keys` **没有租户列** ⇒ 由 DB key 还原的 [`TokenEntry`] `tenant = None`。
//!   本体服务模式（`--ontology-only`）没有租户概念，catalog / describe / TTL / MCP
//!   catalog 都正常；**普通模式**（连了业务库）下 SPARQL 会得到「租户为空」的
//!   参数错误 —— 那是这张表缺少租户归属的既有限制，不在 REQ-175 的范围里。
//! - 吊销是**软删**（`revoked_at`），本模块只认 `IS NULL`；物理删除同样等效
//!   （查不到 ⇒ `None` ⇒ 401），但会丢掉「这把曾经存在」的审计线索。
//! - 缓存是**进程内**的：多副本部署时，各副本各自最多晚 TTL 才看到吊销。
//! - **未命中不缓存**（见上）：伪造 / 过期 key 的每个请求都会换来一次
//!   `token_sha256 = $1` 的等值查询（走唯一索引，约 1 ms）。要防「拿废 key 打满库」
//!   应在网关层限流，⛔ 不在这一层加负缓存 —— 那会让「刚发的 key」在最长 TTL 内被
//!   一次更早的探测挡住（AC-001 的现场形态就是「建完项目立刻用」）。

use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{Duration, Instant};

use axum::http::StatusCode;
use sqlx::postgres::PgPool;

use crate::db::bundle_store::PgBundleStore;

use super::audit::sha256_hex;
use super::error::ApiError;
use super::tokens::TokenEntry;

/// ⭐ REQ-175 · DB key 绑的项目 ≠ 本进程 `--project` —— **403**。
///
/// 为什么单独一个码（而不是复用 `key.not_admin`）：`key.not_admin` 说的是
/// 「这张 key 不是写面的钥匙」，而这里的 key **是**一把合法的查询 key，只是绑在
/// **另一个**项目上。两者对运维的下一步完全不同（前者去要 admin，后者换 key / 换部署），
/// 混成一个码会把排查带错方向。
pub const KEY_PROJECT_MISMATCH: &str = "key.project_mismatch";

/// DB key 命中结果的缓存存活时间（60s —— demand §二-6 的建议值）。
pub const KEY_CACHE_TTL: Duration = Duration::from_secs(60);

/// 一条缓存命中：绑定的项目名 + 写进来的时刻（TTL 用单调时钟，⛔ 不用系统时间）。
#[derive(Debug, Clone)]
struct Hit {
    project: String,
    at: Instant,
}

/// **DB 发的 key 的鉴权器**（一个进程一份；两种部署模式共用同一个类型）。
///
/// 它只做一件事：`token 原文 → （摘要 → 查库）→ 绑定的项目名`。⛔ 不放行、
/// ⛔ 不做项目切换、⛔ 不碰静态表 —— 「这张 key 能不能读」由调用方的身份门决定。
#[derive(Debug)]
pub struct DbKeyAuth {
    /// 元数据库（`project_keys` 在它里面）。
    pool: PgPool,
    /// 命中结果的最长存活时间（判据可以用 [`Self::with_ttl`] 缩到毫秒级）。
    ttl: Duration,
    /// 键 = `sha256(token)` 的小写十六进制（**不是**原文）。
    cache: RwLock<HashMap<String, Hit>>,
}

impl DbKeyAuth {
    /// 生产构造器：TTL = [`KEY_CACHE_TTL`]。
    pub fn new(pool: PgPool) -> Self {
        Self::with_ttl(pool, KEY_CACHE_TTL)
    }

    /// 显式 TTL 的构造器（判据用：把「吊销 ≤ TTL 生效」压到毫秒级）。
    pub fn with_ttl(pool: PgPool, ttl: Duration) -> Self {
        Self {
            pool,
            ttl,
            cache: RwLock::new(HashMap::new()),
        }
    }

    /// 当前 TTL（判据 / 启动提示可读）。
    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    /// ⭐ 解析一把**静态表里没有**的 token。
    ///
    /// | 情形 | 返回 |
    /// | --- | --- |
    /// | 空 / 纯空白 | `Ok(None)`（连摘要都不算，⛔ 不打库） |
    /// | 库里有一行 `revoked_at IS NULL` 且摘要相等 | `Ok(Some(entry))`（`kind = Query`，绑项目） |
    /// | 没有这一行 / 已吊销 | `Ok(None)`（调用方回 **401**） |
    /// | 元数据库读失败 | `Err(500)`（⛔ 不吞成 401：那会把「库坏了」说成「你的 key 错了」） |
    ///
    /// ⚠️ 比较走的是**唯一索引 `token_sha256 = $1`** 的等值查询 —— 摘要本身就是
    /// 定长的 `sha256` 十六进制，比较在数据库侧完成，本模块不持有原文。
    pub async fn resolve(&self, presented: &str) -> Result<Option<TokenEntry>, ApiError> {
        let token = presented.trim();
        if token.is_empty() {
            return Ok(None);
        }
        // ⭐ 与 `provision::create_project` 写库时**同一个函数**、同一份参数
        // （`key.as_bytes()`）—— ⛔ 不要在这里另写一份摘要实现。
        let digest = sha256_hex(token.as_bytes());
        if let Some(project) = self.cached_project(&digest) {
            return Ok(Some(TokenEntry::from_db_key(project, digest)));
        }
        let project = PgBundleStore::new(self.pool.clone())
            .project_for_active_key(&digest)
            .await
            .map_err(|e| {
                // ⛔ 不把 digest / token / SQL 写进 message：错误正文会直接到调用方。
                ApiError::with_code(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "INTERNAL",
                    format!("核对 DB 发的 key 时读元数据库失败：{e}"),
                    "这是服务端与元数据库之间的问题（连接 / 表结构 / 权限），与你的凭据无关。\
                     重试一次；仍失败请把本页的 code 与时间交给运维。",
                )
            })?;
        let Some(project) = project else {
            // 未命中 / 已吊销：**不缓存**（见模块头「只缓存命中」）。
            // 顺带把这条摘要的旧命中删掉 —— 它刚刚被库证明已失效（吊销后的第一次
            // 重查就走这里 ⇒ `cached_len()` 会回到 0，判据据此证明「真的重查了库」）。
            self.forget(&digest);
            return Ok(None);
        };
        self.remember(digest.clone(), project.clone());
        Ok(Some(TokenEntry::from_db_key(project, digest)))
    }

    /// 命中缓存（顺带判 TTL）。过期 ⇒ `None`（下一次 `resolve` 会重新查库）。
    fn cached_project(&self, digest: &str) -> Option<String> {
        let c = read(&self.cache);
        let hit = c.get(digest)?;
        (hit.at.elapsed() < self.ttl).then(|| hit.project.clone())
    }

    /// 记一次命中。键是摘要；缓存大小由「`project_keys` 里有多少把有效 key」上界决定
    /// （一项目一把），⛔ 不需要再做淘汰。
    fn remember(&self, digest: String, project: String) {
        write(&self.cache).insert(
            digest,
            Hit {
                project,
                at: Instant::now(),
            },
        );
    }

    /// 缓存里有几条（判据用：证明「同一把 key 的第二个请求没有再查库」）。
    pub fn cached_len(&self) -> usize {
        read(&self.cache).len()
    }

    /// 删掉一条缓存（库刚刚证明它已失效：吊销 / 行被删）。
    fn forget(&self, digest: &str) {
        write(&self.cache).remove(digest);
    }

    /// 清空缓存（运维 / 判据的**强制失效**入口：不等 TTL 也能让吊销立刻生效）。
    ///
    /// ⚠️ 生产路径**没有**调用它：进程内清空只影响自己，多副本时别的副本仍各等各的 TTL
    /// —— 那个不一致会让人误以为「吊销已全局生效」。真要做到这一点需要跨进程机制
    /// （LISTEN/NOTIFY 一类），那不在 REQ-175 的范围里。
    pub fn invalidate_all(&self) {
        write(&self.cache).clear();
    }
}

/// 中毒的锁不该让鉴权停摆：缓存放手 = 下一次重新查库（最多多一次 SELECT）。
fn read<T>(m: &RwLock<T>) -> std::sync::RwLockReadGuard<'_, T> {
    match m.read() {
        Ok(g) => g,
        Err(e) => e.into_inner(),
    }
}

fn write<T>(m: &RwLock<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    match m.write() {
        Ok(g) => g,
        Err(e) => e.into_inner(),
    }
}

// ══════════════════════════════════════════════════════════════
// 判据（纯函数 + 缓存行为；**不连库**）
// ══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    /// 一个**永远连不上**的懒连接池：证明「没必要的路径一次库都不打」。
    fn lazy_pool() -> PgPool {
        sqlx::postgres::PgPoolOptions::new()
            .acquire_timeout(Duration::from_millis(200))
            .connect_lazy("postgresql://nobody@127.0.0.1:1/none")
            .expect("lazy pool")
    }

    /// 空 / 纯空白的凭据 ⇒ `Ok(None)`，且**不查库**（池指向 127.0.0.1:1）。
    #[tokio::test]
    async fn a_blank_token_is_none_without_touching_the_database() {
        let auth = DbKeyAuth::new(lazy_pool());
        assert!(auth.resolve("").await.expect("空串不该报错").is_none());
        assert!(auth.resolve("   ").await.expect("空白不该报错").is_none());
        assert_eq!(auth.cached_len(), 0);
    }

    /// 缓存**只**因 TTL 过期而失效（不是每请求都重算）。
    ///
    /// 变异：把 `hit.at.elapsed() < self.ttl` 改成恒 `true` ⇒ 第二条断言红
    /// （吊销在这条路上就永远看不见了）。
    #[tokio::test]
    async fn a_cached_hit_expires_after_the_ttl() {
        let auth = DbKeyAuth::with_ttl(lazy_pool(), Duration::from_millis(50));
        auth.remember("digest-only".into(), "cloud-resource".into());
        assert_eq!(
            auth.cached_project("digest-only").as_deref(),
            Some("cloud-resource"),
            "TTL 内必须命中（证明没有每请求打库）"
        );
        assert_eq!(auth.cached_len(), 1);
        // 过期：把时刻往回拨（不 sleep —— 判据不依赖墙钟粒度）。
        {
            let mut c = write(&auth.cache);
            let hit = c.get_mut("digest-only").expect("刚写进去");
            hit.at = Instant::now() - Duration::from_millis(200);
        }
        assert_eq!(auth.cached_project("digest-only"), None, "TTL 过后必须重新查库");
        auth.invalidate_all();
        assert_eq!(auth.cached_len(), 0, "强制失效入口要真的清空");
    }

    /// 默认 TTL 就是 demand 里写的 60s（⛔ 别悄悄改小/改大）。
    #[tokio::test]
    async fn the_default_ttl_is_sixty_seconds() {
        assert_eq!(KEY_CACHE_TTL, Duration::from_secs(60));
        assert_eq!(DbKeyAuth::new(lazy_pool()).ttl(), Duration::from_secs(60));
    }

    /// 错误码是**常量**（端点语义，不是运行时拼接）。
    #[test]
    fn the_project_mismatch_code_is_registered_as_a_constant() {
        assert_eq!(KEY_PROJECT_MISMATCH, "key.project_mismatch");
    }
}

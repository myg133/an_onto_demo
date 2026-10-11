//! 数据库连接池。
//!
//! **连接串只从环境变量读**（见 `config`），代码里不出现口令。

use sqlx::postgres::{PgPool, PgPoolOptions};
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("连不上数据库：{0}")]
    Connect(#[from] sqlx::Error),
    #[error("缺少环境变量 {0}（口令不得写入代码或文档）")]
    MissingEnv(&'static str),
}

/// 从指定环境变量建连接池。
pub async fn connect_from_env(key: &'static str) -> Result<PgPool, DbError> {
    let url = std::env::var(key).map_err(|_| DbError::MissingEnv(key))?;
    connect(&url).await
}

/// 从连接串建连接池。
///
/// `max_connections(5)` 与 acquire 10s —— 测试库是外部共用库，
/// 别把连接数开大。
pub async fn connect(url: &str) -> Result<PgPool, DbError> {
    Ok(PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(10))
        .connect(url)
        .await?)
}

/// **测试专用**连接池（REQ-030）。
///
/// # 为什么单独一个函数，而不改 [`connect`] 的默认值
///
/// [`connect`] 是**生产路径**（CLI 走它）。`max_connections(5)` 是生产侧的取值，
/// 要改它得由生产需求驱动、并有对应的生产侧理由；本任务只掌握了
/// **测试侧**的证据（测试库连接占用曲线），拿测试证据去改生产默认值是越权。
/// 另有一条现实理由：测试要的 `max_connections(1)` 是**测试编排**的性质
/// （一个 `#[tokio::test]` 内部几乎不并发取连接），生产要不要 1 条是另一回事，
/// 两者耦合后任何一边调整都会误伤另一边。
///
/// 所以：生产默认值原样保留，测试走本函数。
///
/// # 为什么测试要 `max_connections(1)`
///
/// 实测（见 `tests/common/mod.rs` 的注释）：测试库是**共享集群**，
/// `max_connections = 100` 是**集群级**、跨库共享的，与另外 5 个业务库
/// （`postgres` / `superset` / `<业务库名>` …）
/// 抢同一批槽位，且那 5 个库常年占着 30 条。
///
/// 而测试侧是「**每个测试各建一个池**」（刻意不用全局共享池，理由见
/// `tests/common/mod.rs`）。峰值连接数因此约为
/// `单个测试二进制内的并行测试数 × 每池连接数` —— 每池从 5 降到 1，
/// 这一项就从 `P × 5` 降到 `P × 1`。
///
/// 「几乎不并发」是可查证的事实，不是乐观假设：全仓 `tests/` 下
/// **没有任何一处** `.acquire()` 或 `.begin()` 持着连接跨越另一个取连接的 await，
/// 真正需要并发的两处测试（`test_project.rs` 的 seq 并发、`test_extraction_agent.rs`
/// 的同键并发登记）已显式改走 [`connect_for_tests`] 的宽池入口，不受本默认值影响。
///
/// # `idle_timeout` / `max_lifetime` 为什么必须有
///
/// `PgPool` 被 drop 时，sqlx 靠 `PoolConnection::drop` 向**当前 runtime** spawn
/// 关闭任务（`sqlx-core/src/pool/connection.rs`）。`#[tokio::test]` 每个测试各起
/// 一个 runtime，测试一结束 runtime 就销毁 —— 那个 spawn 出去的关闭任务若没跑完，
/// 连接就只能在**进程退出**时才被 socket EOF 回收。短 `idle_timeout` 让空闲连接
/// 在**秒级**、由池自己的 reaper 主动关闭，不必等服务端。
pub async fn connect_for_tests(url: &str, max_connections: u32) -> Result<PgPool, DbError> {
    Ok(PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(Duration::from_secs(10))
        // 空转超过 10s 的连接由 reaper 主动关掉，不留到进程退出。
        .idle_timeout(Some(Duration::from_secs(10)))
        // 兜底：即使一直被交替使用，单条连接也不许活过 5 分钟。
        .max_lifetime(Some(Duration::from_secs(300)))
        .connect(url)
        .await?)
}

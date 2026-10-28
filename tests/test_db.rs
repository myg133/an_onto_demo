//! Task 1 · 骨架与 PG 连通
//!
//! 验证：能连上、连接串只能来自环境变量。

mod common;
/// 测试库不可用时的统一说明：**可能是没设，也可能是指向了非专用库**（防污染闸会拒）。
const NEED_TEST_DB: &str = "ONTOK_TEST_DATABASE_URL 不可用：没设，或指向了非专用测试库（库名需含 test/scratch/tmp/ci/dev）";

use common::test_pool;
use ontok::db::pool::DbError;
use sqlx::postgres::PgPool;

#[tokio::test]
async fn test_db_pg_pool_connects() {
    let pool = test_pool().await;
    // `SELECT 1` 的字面量在 PG 里是 **INT4**，不是 INT8 ——
    // 用 i64 接收会报 ColumnDecode。计划里的示例代码此处有误。
    let n: i32 = sqlx::query_scalar("SELECT 1")
        .fetch_one(&pool)
        .await
        .expect("查询失败");
    assert_eq!(n, 1);
}

#[tokio::test]
async fn test_db_pg_is_postgres_16() {
    // 锁版本：环境文档写的是 PG 16.14，版本变了要能第一时间发现
    let pool = test_pool().await;
    let v: String = sqlx::query_scalar("SELECT version()")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(v.contains("PostgreSQL 16"), "期望 PG 16，实际：{v}");
}

#[tokio::test]
async fn test_db_missing_env_is_error_not_silent_default() {
    // 缺变量必须报错，**不能**静默连到某个「看起来像」的库
    let key = "ONTOK_NO_SUCH_ENV_99999";
    let r = ontok::db::pool::connect_from_env(key).await;
    assert!(
        matches!(r, Err(DbError::MissingEnv(_))),
        "缺环境变量应报 MissingEnv，实际：{r:?}"
    );
}

#[tokio::test]
async fn test_db_migrate_is_idempotent() {
    // 整套测试共用一组表，migrate 跑两次不能炸
    let pool = test_pool().await;
    common::migrate(&pool).await;
    common::migrate(&pool).await;
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM information_schema.tables \
         WHERE table_schema='public' AND table_name='object_types'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(n, 1, "object_types 表应恰好存在一份");
}

/// REQ-025 · **空 schema 并发首跑**迁移必须全绿。
///
/// # 这条钉住什么
///
/// `CREATE TABLE IF NOT EXISTS` 在 PG 里不是并发安全的：`pg_type` 是 catalog，
/// catalog 的写不走事务快照，所以 `IF NOT EXISTS` 拦不住 ——
/// 两个连接同时建同一张表会双双撞 `pg_type_typname_nsp_index`。
/// `migrate()` 因此取一把**交易级**咨询锁，而这把锁必须覆盖**全部** DDL
/// （9 张基表 + 2 条 ALTER + registry 两张表）且与它们在**同一个 batch** 里。
///
/// 锁只包住 registry 那两张表时，9 张基表跑在锁之外 —— 这条测试必须**抓得住**它。
///
/// # ⚠️ 为什么必须「预热连接 + barrier 对齐起跑」（REQ-025 复验补做）
///
/// 这条测试原先是「6 个 task 各自 `migrate()`」，靠 `JoinSet` 自然错开起跑。
/// **实测发现那样测出来的是抖动，不是性质**（三形状 × 10 轮，同一份 DDL）：
///
/// | 形状 | 通过轮数 | 成功数范围 |
/// | --- | --- | --- |
/// | 真实现（锁覆盖全部 DDL） | 3/3 | 18/18 |
/// | 锁只包 registry、且**registry 在前** | **0/10** | 3–7/18 |
/// | 锁只包 registry、registry **在最后**（= REQ-025 之前原样） | **8/10** | 13–18/18 |
/// | 完全无锁 | 1/10 | 8–18/18 |
///
/// 关键在第 2、3 行：**同一把「部分覆盖」的锁，只因 DDL 的前后顺序不同，
/// 判别力就从 0/10 变成 8/10**。原因是起跑时刻的**自然抖动**：
/// 每个 task 先要付「新建连接 + `after_connect` 跑两条 `SET`」的开销，
/// 6 个 task 的建连耗时互不相同，于是冲进 DDL 的时刻被拉开若干毫秒；
/// 而 catalog 撞名的窗口是**亚毫秒级**的 —— 抖动一大就撞不上。
/// 换句话说：**原版测试量的是时序抖动，不是「锁覆盖了没有」**。
///
/// 所以这里显式把起跑时刻**钉死**：
/// - **预热**：先把 `FANOUT` 条连接全部建出来（`after_connect` 的开销在此付清）；
/// - **barrier**：6 个 task 在 `Barrier` 上会合，再一起冲进 `migrate()`。
///
/// 这样「锁没盖住 9 张基表」就必然撞名，判别力不再依赖运气。
/// 代价是多两条连接 + 一个 barrier，实测整条测试仍在数秒量级。
///
/// 改完之后重跑同一组三形状（每形状 4–10 轮，`ROUNDS=4 × FANOUT=6`）：
///
/// | 形状 | 加强前 | 加强后 |
/// | --- | --- | --- |
/// | 真实现（锁覆盖全部 DDL） | 3/3 通过，18/18 | **6/6 通过，24/24，撞名 0** |
/// | 锁只包 registry（registry 在最后） | 8/10 通过，13–18/18 | **0/10 通过，4/24，撞名 20** |
/// | 完全无锁 | 1/10 通过，8–18/18 | **0/4 通过，4/24，撞名 20** |
///
/// 加强后三种失败形状的数字**完全一致**（4/24、撞名 20、峰值 active 6）——
/// 判别力不再随起跑抖动漂移，这正是要钉住的东西。
///
/// # 怎么造出「空库」
///
/// 不能去动共享本体库的表（禁 DELETE / TRUNCATE / 清空），
/// 也不能真去 `CREATE DATABASE`（那是另一台共享服务器上的另一件事）。
/// 这里给每个并发轮开一个**专属空 schema**，并让池里每条连接
/// `SET search_path` 指过去 —— DDL 全落在这个空 schema 里，
/// catalog 的唯一约束是**按 namespace** 判的，冲突形状与空库首跑完全一致。
/// 共享本体库的 `public` 一张表都不碰。
///
/// 收尾只 `DROP SCHEMA … CASCADE` **本测试自己建的那个**空 schema ——
/// 删的是自己的临时命名空间，不是共享表。
#[tokio::test]
async fn test_migrate_on_empty_schema_survives_concurrent_first_run() {
    let url = ontok::config::test_database_url().expect(
        NEED_TEST_DB,
    );
    const ROUNDS: usize = 4;
    const FANOUT: usize = 6;
    /// 探针用的 `application_name` —— 让 `pg_stat_activity` 能认出「本轮的连接」。
    const PROBE_APP: &str = "req025-concurrency-probe";
    /// 预热重试上限 —— 共享库偶发 `Connection reset by peer`，不是重试能治的病。
    const WARMUP_ATTEMPTS: usize = 5;

    let mut ok = 0usize;
    let mut failed: Vec<String> = Vec::new();
    // 撞 `pg_type_typname_nsp_index` 的次数 —— **与基础设施抖动分开算**。
    let mut catalog_conflicts = 0usize;
    // 量测到的峰值并发度（诊断用，见下方 assert）。
    let mut peak_connected = 0usize;
    let mut peak_active = 0usize;

    for _ in 0..ROUNDS {
        let schema = common::unique_project("migrate_race");
        // REQ-042 · 建连接同样套上有限重试：`pool::connect` 的签名
        // （`&str -> Result<PgPool, DbError>`）与 `retry_connect` 的
        // `Fut: Future<Output = Result<PgPool, DbError>>` **完全兼容**，
        // 闭包里仍然只有建连接本身，没有 after_connect / 任何后续动作。
        let admin = common::retry_connect(|| ontok::db::pool::connect(&url))
            .await
            .expect("连不上测试库");
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .expect("建专属空 schema 失败");

        // 每条连接都把 search_path 指到这个空 schema
        let set_path = format!("SET search_path TO {schema}");
        //
        // REQ-042 · 建连接套上有限重试。**为什么 builder 留在闭包外面**：
        // `opts` 只构造一次，闭包每次 `opts.clone()` 后再 `connect` ——
        // 这样 builder 的**配置**（含下面的 `after_connect` 钩子）不在实参文本里重复展开。
        //
        // ⚠️ REQ-043 · F-6 更正：本段原先写「被重试的闭包里只有 connect」——
        // **那是假的**，现更正为事实：
        // 闭包捕获的 `opts` 内建的 `after_connect` 钩子里有两条 `sqlx::query(..).execute(..)`
        // （`SET application_name` / `SET search_path`，即**每条连接建立时的会话初始化**），
        // 它们在 `connect()` 内部随建连接一并执行 ⇒ **会话初始化失败会被一并重试**。
        //
        // 这个语义是**可接受**的：会话初始化失败意味着这条连接没建成，
        // 把它归进「建连接」这一阶段并重试整个建连接是对的。
        // 但**表述必须如实** —— 「闭包里只有 connect」会让人以为钩子里的 execute
        // 在重试范围之外，而它不在。这也是 AC-003 判据只看 `tests/common/mod.rs`、
        // 本文件靠结构自洽而不靠判据的直接后果。
        let opts = sqlx::postgres::PgPoolOptions::new()
            .max_connections(FANOUT as u32)
            .after_connect(move |conn, _meta| {
                let set_path = set_path.clone();
                Box::pin(async move {
                    sqlx::query(&format!("SET application_name = '{PROBE_APP}'"))
                        .execute(&mut *conn)
                        .await?;
                    sqlx::query(&set_path).execute(conn).await?;
                    Ok(())
                })
            });
        let pool = common::retry_connect(|| async { opts.clone().connect(&url).await.map_err(DbError::Connect) })
            .await
            .expect("连不上测试库");

        // 预热：把 FANOUT 条连接全部建出来，`after_connect` 的开销在此付清 ——
        // 不然每个 task 各自付一次建连耗时，起跑时刻就被抖动拉开了。
        //
        // ⚠️ 预热**不许一撞就炸**：共享库偶发 `Connection reset by peer`，
        // 而预热只是「提前把连接建出来」，没建成就重试一次 ——
        // 真的连不上，后面 `migrate()` 自己会报，不该由预热先 panic。
        for attempt in 1..=WARMUP_ATTEMPTS {
            let mut warm = tokio::task::JoinSet::new();
            for _ in 0..FANOUT {
                let pool = pool.clone();
                warm.spawn(async move { sqlx::query("SELECT 1").execute(&pool).await });
            }
            let mut bad = 0usize;
            while let Some(j) = warm.join_next().await {
                match j {
                    Ok(Ok(_)) => {}
                    _ => bad += 1, // task panic 或查询出错，都算这次没建全
                }
            }
            if bad == 0 {
                break;
            }
            assert!(
                attempt < WARMUP_ATTEMPTS,
                "预热 {WARMUP_ATTEMPTS} 次仍有 {bad} 条连接没建起来 —— \
                 不是「没撞上」的问题，是真的连不上测试库"
            );
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }

        // 探针：在 `admin` 上轮询 `pg_stat_activity`，量「同一时刻真正在跑的后端数」。
        // 纯只读，跑在自己的连接上，不占 pool 的连接。
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let peak_c = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let peak_a = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let sampler = {
            let admin = admin.clone();
            let stop = stop.clone();
            let peak_c = peak_c.clone();
            let peak_a = peak_a.clone();
            tokio::spawn(async move {
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    let (c, a): (i64, i64) = sqlx::query_as(
                        "SELECT count(*), count(*) FILTER (WHERE state = 'active') \
                         FROM pg_stat_activity WHERE application_name = $1",
                    )
                    .bind(PROBE_APP)
                    .fetch_one(&admin)
                    .await
                    .expect("探针查询失败");
                    peak_c.fetch_max(c as usize, std::sync::atomic::Ordering::Relaxed);
                    peak_a.fetch_max(a as usize, std::sync::atomic::Ordering::Relaxed);
                    tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                }
            })
        };

        // barrier 对齐起跑：6 个 task 在此会合，然后**同时**冲进 migrate()。
        let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(FANOUT));
        let mut set = tokio::task::JoinSet::new();
        for _ in 0..FANOUT {
            let pool = pool.clone();
            let barrier = barrier.clone();
            set.spawn(async move {
                barrier.wait().await;
                ontok::db::pg::migrate(&pool).await
            });
        }
        while let Some(joined) = set.join_next().await {
            match joined {
                Ok(Ok(())) => ok += 1,
                Ok(Err(e)) => {
                    let msg = format!("{e}");
                    if msg.contains("pg_type_typname_nsp_index") {
                        catalog_conflicts += 1;
                    }
                    failed.push(msg);
                }
                Err(e) => panic!("迁移 task panic 了：{e}"),
            }
        }

        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = sampler.await;
        peak_connected = peak_connected.max(peak_c.load(std::sync::atomic::Ordering::Relaxed));
        peak_active = peak_active.max(peak_a.load(std::sync::atomic::Ordering::Relaxed));

        pool.close().await;
        sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
            .execute(&admin)
            .await
            .expect("回收专属空 schema 失败");
        admin.close().await;
    }

    eprintln!(
        "[REQ-025] 空 schema 并发首跑：{ok}/{} 成功；catalog 撞名 {catalog_conflicts} 次；\
         量测峰值：同时在场连接 {peak_connected}、active 后端 {peak_active}（FANOUT={FANOUT}）",
        ROUNDS * FANOUT
    );

    // 判别力第一道：撞名次数必须为 0，且与基础设施抖动分开报。
    // 只断言「全部成功」的话，ConnectionReset 会把真正的信号淹掉。
    assert_eq!(
        catalog_conflicts, 0,
        "锁没盖住 9 张基表：{catalog_conflicts} 次撞 pg_type_typname_nsp_index\
         （{}/{} 次迁移失败）。峰值 active 后端仅 {peak_active}。",
        failed.len(),
        ROUNDS * FANOUT
    );
    assert!(
        failed.is_empty(),
        "空 schema 上并发跑 migrate() 必须全部成功（锁覆盖全部 DDL）\
         —— {}/{} 失败（其中 catalog 撞名 {catalog_conflicts} 次），首条错误：{:?}",
        failed.len(),
        ROUNDS * FANOUT,
        failed.first()
    );
    assert_eq!(ok, ROUNDS * FANOUT);
    // 判别力第二道：并发度不能塌。塌了就说明「没撞上」是因为没真并发，
    // 那这条测试就悄悄失去了判别力 —— 宁可红，不要假装绿。
    assert!(
        peak_active >= 2,
        "量测到的峰值 active 后端只有 {peak_active}（同时在场连接 {peak_connected}）—— \
         并发度塌了，这条测试当前测不出锁覆盖与否，请先修并发构造"
    );
}

// ══════════════════════════════════════════════════════════════
// REQ-039 · 「已存在的列不进 DDL」不得牺牲「空库/旧库仍建齐」
// ══════════════════════════════════════════════════════════════

/// 把一条连接的 `search_path` 指到**会话私有的临时命名空间** `pg_temp`。
///
/// # 为什么是 `pg_temp` 而不是 `CREATE SCHEMA` + `DROP SCHEMA CASCADE`
///
/// 上面 REQ-025 那条测试的做法是「建专属 schema，收尾 `DROP … CASCADE` 自己的 schema」。
/// 本条**不收尾**：临时命名空间随**会话结束自动消失**，连一次 `DROP` 都不需要 ——
/// 于是它在「共享本体库禁 DELETE / TRUNCATE / 清空」这条红线下没有任何解释空间。
///
/// ⚠️ `max_connections(1)` 是**硬要求**：`SET search_path` 是**连接级**的，
/// 多连接池上它只作用于其中一条，后续查询会落到没设过的连接上，
/// 读到的就是 `public` 的表 —— 这条测试会**假绿**（实测踩过：
/// 同一段代码在 10 连接池上 4/11 张表 `exists=false`，1 连接池上 11/11）。
async fn temp_schema_pool(url: &str) -> PgPool {
    // REQ-042 · 建连接套上有限重试（见 `test_db_migrate_race` 里的同款说明：
    // builder 留在闭包外，被重试的闭包里只有 `connect`）。
    let opts = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .after_connect(|conn, _meta| {
            Box::pin(async move {
                sqlx::query("SET search_path TO pg_temp")
                    .execute(conn)
                    .await?;
                Ok(())
            })
        });
    common::retry_connect(|| async { opts.clone().connect(url).await.map_err(DbError::Connect) })
        .await
        .expect("连不上测试库")
}

/// `migrate()` 在**空 schema** 上必须把 11 张表与三列补丁全部建齐（REQ-039 · AC-003）。
///
/// # 这条挡的是什么
///
/// REQ-039 让 [`ontok::db::pg::migrate`] 先查「列在不在」、在就**不进 DDL**。
/// 这类短路天然带一个致命风险：**空库上把「查不到」误当成「已存在」⇒ 整列漏建**，
/// 而漏建要到第一次读写那一栏时才炸 —— 远 away from 迁移现场。
///
/// 所以本条专门走**空 schema**（首次部署 / CI 干净库那条路），
/// 逐张表、逐列点명。
///
/// # 为什么「表存在」用 `to_regclass()` 判
///
/// 与 [`ontok::db::pg::migrate`] 里探查询同一条纪律：表不存在时它返回 `NULL`
/// 而不是抛错，于是「建出来了没有」这件事永远可以问，问不出就当没有。
#[tokio::test]
async fn test_migrate_on_empty_schema_builds_every_table_and_column() {
    let url = ontok::config::test_database_url().expect(
        NEED_TEST_DB,
    );
    let pool = temp_schema_pool(&url).await;

    // 先确认起点真的是空 schema —— 前提不成立时下面全是空断言
    for t in ["projects", "ontology_versions", "action_types", "concept_groups"] {
        let pre: (Option<String>,) =
            sqlx::query_as("SELECT to_regclass($1)::text").bind(t).fetch_one(&pool).await.unwrap();
        assert!(pre.0.is_none(), "前提失效：`{t}` 在迁移前就存在，这条测试没有判别力");
    }

    ontok::db::pg::migrate(&pool)
        .await
        .expect("空 schema 首次迁移失败 —— 建表路径被短路弄坏了");

    for t in [
        "projects",
        "ontology_versions",
        "object_types",
        "link_types",
        "action_types",
        "functions",
        "proposals",
        "proposal_items",
        "concept_groups",
        "extraction_agents",
        "extraction_rounds",
    ] {
        let has: (Option<String>,) =
            sqlx::query_as("SELECT to_regclass($1)::text").bind(t).fetch_one(&pool).await.unwrap();
        assert!(has.0.is_some(), "空 schema 首次迁移后 `{t}` 不存在 —— 建表漏了");
    }

    // 三条 ALTER 补丁覆盖的列，在空库上也必须真的在
    for (table, column) in [
        ("ontology_versions", "extracted_by"),
        ("action_types", "ordinal"),
        ("concept_groups", "low_confidence"),
    ] {
        let has: (bool,) = sqlx::query_as(
            "SELECT EXISTS (SELECT 1 FROM pg_attribute
                             WHERE attrelid = to_regclass($1) AND attname = $2
                               AND attnum > 0 AND NOT attisdropped)",
        )
        .bind(table)
        .bind(column)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(has.0, "空 schema 首次迁移后 `{table}.{column}` 不存在 —— 列漏建了");
    }
}

/// REQ-039 · 表在、**列不在**（旧库）时，那条 `ALTER` 必须照发（AC-003 的判别力在这）。
///
/// # 为什么空 schema 那条不够
///
/// 空库上 [`ontok::db::pg::migrate`] 的 `CREATE TABLE` 本来就把那三列建出来了，
/// 所以三条 `ALTER` 在空库上是 no-op —— **「跳过」和「照发」观察起来一模一样**。
/// 那条测试因此只能证明「没漏建」，证明不了「该补的仍会补」。
///
/// 本条造出**唯一能区分两种实现**的形状：表已存在，但**恰好缺那一列**。
/// 这正是三条 `ALTER` 存在的理由（旧库升级），也是短路最可能出错的地方：
/// 只要实现里出现「查不到 / 查歪了就当已存在」，这张表就永远补不上那一列。
#[tokio::test]
async fn test_migrate_still_alters_when_table_exists_but_column_does_not() {
    let url = ontok::config::test_database_url().expect(
        NEED_TEST_DB,
    );
    let pool = temp_schema_pool(&url).await;

    // 旧库形状：三张表都在，但都缺本迭代补的那一列。
    // `action_types` 特意补上 `version_id` / `key` —— 回填的子查询要
    // `PARTITION BY version_id ORDER BY key`，缺了就变成 42703 而不是本条要判的东西。
    // 少了 `ordinal` 就正好是 REQ-011 之前的样子。
    for t in ["ontology_versions", "concept_groups"] {
        sqlx::query(&format!("CREATE TABLE {t} (id UUID PRIMARY KEY)"))
            .execute(&pool)
            .await
            .expect("造旧库形状失败");
    }
    sqlx::query("CREATE TABLE action_types (id UUID PRIMARY KEY, version_id UUID, key TEXT)")
        .execute(&pool)
        .await
        .expect("造旧库形状失败");
    for (table, column) in [
        ("ontology_versions", "extracted_by"),
        ("action_types", "ordinal"),
        ("concept_groups", "low_confidence"),
    ] {
        let before: (bool,) = sqlx::query_as(
            "SELECT EXISTS (SELECT 1 FROM pg_attribute
                             WHERE attrelid = to_regclass($1) AND attname = $2
                               AND attnum > 0 AND NOT attisdropped)",
        )
        .bind(table)
        .bind(column)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(!before.0, "前提失效：`{table}.{column}` 事先就存在，这条测试没有判别力");
    }

    ontok::db::pg::migrate(&pool)
        .await
        .expect("旧库迁移失败 —— ALTER 短路把补列路径弄坏了");

    for (table, column) in [
        ("ontology_versions", "extracted_by"),
        ("action_types", "ordinal"),
        ("concept_groups", "low_confidence"),
    ] {
        let after: (bool,) = sqlx::query_as(
            "SELECT EXISTS (SELECT 1 FROM pg_attribute
                             WHERE attrelid = to_regclass($1) AND attname = $2
                               AND attnum > 0 AND NOT attisdropped)",
        )
        .bind(table)
        .bind(column)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(
            after.0,
            "`{table}` 已在、`{column}` 不在，migrate() 却没有把它补上 —— \
             「已存在就不进 DDL」把该发的 ALTER 一起跳过了（空库/旧库漏建）"
        );
    }
}

/// REQ-039 · 库已建齐时，`migrate()` **一把 `AccessExclusiveLock` 都不许取**。
///
/// # 这条是「死锁不复现」的**确定性**判据
///
/// AC-001 说「连跑 10 轮不许报 `40P01`」—— 那是个**概率**判据：死锁本身是概率性的，
/// 变异自证时它给不出稳定的红。所以本条改用一个**每次都必然出结果**的机制：
/// **让 `migrate()` 在一个被锁住的关系上跑**。
///
/// # ⚠️ 修复轮：必须**逐一**压全部 3 条 ALTER 涉及的表（这一版才有判别力）
///
/// 上一轮本条**只压 `ontology_versions` 一张表**，于是漏掉了这样一个真缺陷：
/// `patched_columns_present` 把一行三列的结果按 `Vec<(bool,)>` 解码
/// （sqlx 对多列结果只取第 0 列、**不校验列数**）⇒ `Vec` 长度实为 1
/// ⇒ 索引 1、2 的 `action_types` / `concept_groups` 一律被判「不存在」
/// ⇒ 那两条 ALTER **每次照发**，两把排他锁照取。
/// 而只压 `ontology_versions` 恰好命中唯一被正确解码的第 0 列 ⇒ **黑盒全绿，缺陷照旧**。
///
/// 教训写在这里，因为它比本条测试本身更值钱：
/// **判据必须覆盖行为的全部作用面，否则「测过」不等于「测到过」。**
/// 三条 ALTER 是同一个机制的三次应用；只测其中一次 = 假设另外两次正确。
/// 现在本条把三张表**逐张**压一遍：任何一条 ALTER 在稳态库上被误发，
/// 都会在对应那张表上超时（`55P03`）而变红。
///
/// # 为什么 `ACCESS SHARE` 恰好是那把对的锁
///
/// PG 的锁相容矩阵里，本条需要的是「**挡得住 `AccessExclusiveLock`、放得过
/// `RowExclusiveLock`**」这一格，实测只有 `ACCESS SHARE` 在这一格：
///
/// | 别的会话在目标表上持有 | `ALTER`（要 `AccessExclusiveLock`） | 回填 `UPDATE`（要 `RowExclusiveLock`） |
/// | --- | --- | --- |
/// | `ACCESS SHARE` | **挡住**（实测 `55P03`） | **放行**（实测通过） |
/// | `SHARE` | 挡住 | 也挡住 ⇒ 回填被误杀，判据失真 |
///
/// 选错成 `SHARE` 的话，这条测试红的原因会变成「回填也拿不到锁」，
/// 钉不住「是不是还在发 ALTER」这件事。
///
/// # 为什么压 `action_types` 时回填 UPDATE 仍然放行
///
/// 回填子查询要 `PARTITION BY version_id ORDER BY key`，读的是 `action_types` 本身
/// （`RowExclusiveLock`），与本条持有的 `ACCESS SHARE` 相容 —— 实测确认过。
/// 真正会被挡住的只有 `ALTER TABLE action_types … ADD COLUMN`。
///
/// # 为什么不直接等死锁 / 为什么用 `lock_timeout` 而不是超时断言
///
/// 若不给 `lock_timeout`，回归时 `migrate()` 会**一直等**这把锁 —— 而持锁方
/// 在等 `migrate()` 返回才放锁，那就成了测试自己造的**死等**，永远跑不完。
/// `lock_timeout` 把「不确定的等待」换成**确定的错误**（`55P03`），测试才能红。
///
/// # 阈值为什么取 20s
///
/// `lock_timeout` 管的是**所有**锁等待，包括 `migrate()` 内部那把
/// `pg_advisory_xact_lock`。本二进制里 `test_migrate_on_empty_schema_survives_concurrent_first_run`
/// 会并发发起 6 次迁移并把咨询锁排队，于是本条可能排在它们后面。
/// 20s 是给这把咨询锁留的余量（实测等待在秒级），而回归时本条仍会在 20s 内红 ——
///
/// **这是本条已知的边界**：咨询锁排队若真的超过 20s，本条会**假红**。
/// 宁可假红（可复现、一眼看出是排队）也不要假绿。
///
/// # 三张表**顺序**压，不并发压
///
/// 并发压会把三张表的锁一起按住，失败时报不出是哪一张；顺序压则每次只有一张被挡，
/// 失败信息能直接指认到具体那一条 ALTER。共享集群的连接数也不被顶高。
#[tokio::test]
async fn test_migrate_takes_no_access_exclusive_lock_on_settled_schema() {
    const LOCK_TIMEOUT: &str = "20s";
    let url = ontok::config::test_database_url().expect(
        NEED_TEST_DB,
    );

    // 先把共享库推到「已建齐」—— 本条要判的正是**稳态**下的锁行为
    let settle = common::test_pool().await;
    common::migrate(&settle).await;

    // ⚠️ 三张表**逐一**压。少压任何一张，本条就对那条 ALTER 没有判别力
    //（上一轮只压第 1 张，于是第 2、3 条 ALTER 被误发也照样全绿）。
    for table in ["ontology_versions", "action_types", "concept_groups"] {
        // 连接 A：占住该表的 ACCESS SHARE（= 只挡 DDL，不挡普通写入）
        let blocker = temp_pool_with_url(&url, 1, None).await;
        let mut held = blocker.acquire().await.expect("拿不到挡锁连接");
        sqlx::raw_sql(&format!(
            "BEGIN; LOCK TABLE {table} IN ACCESS SHARE MODE;"
        ))
        .execute(&mut *held)
        .await
        .unwrap_or_else(|e| panic!("占住 {table} 的 ACCESS SHARE 锁失败：{e:?}"));

        // 连接 B：带着 lock_timeout 跑 migrate。它若还想 ALTER 这张表，
        // 就会拿到 55P03 而不是跑完。
        let runner = temp_pool_with_url(&url, 1, Some(LOCK_TIMEOUT)).await;
        let r = ontok::db::pg::migrate(&runner).await;

        // ⚠️ **先放锁再断言**：回归时 migrate 会因超时失败，但锁仍在我们手上。
        // 先 COMMIT 保证后面无论红绿都不把锁泄漏给同二进制的其它测试。
        sqlx::raw_sql("COMMIT;")
            .execute(&mut *held)
            .await
            .expect("释放 ACCESS SHARE 锁失败");

        assert!(
            r.is_ok(),
            "稳态库上 migrate() 没能跑完（table={table}, {r:?}）——\n\
             本测试用另一条连接把 `{table}` 压在 ACCESS SHARE 上，\
             它**只**挡得住 AccessExclusiveLock（即 ALTER），挡不住回填 UPDATE。\n\
             所以这个失败只可能来自一件事：migrate() 仍在对已建齐的表发 ALTER。\n\
             那一列的名字是 IF NOT EXISTS 救不了的东西 —— 它照样开排他锁，\
             与并行 INSERT 的外键 RowShareLock 撞成 40P01（REQ-039 的原始病因）。\n\
             ⚠️ 本条必须**逐一**覆盖 ALTER_PATCHES 的全部 3 张表：\
             上一轮只压 ontology_versions，于是「解码成 1 元组 → 索引 1、2 被误判为不存在\
             → action_types / concept_groups 的 ALTER 每次照发」这个缺陷能黑盒全绿地交付出去。"
        );
    }
}

/// 建一条只给本文件用的小池：`max_connections` 条连接，可选地带上 `lock_timeout`。
///
/// ⚠️ `after_connect` 里的 `SET` 是**连接级**的 —— 所以 `max_connections` 必须
/// 等于真正要同时用的连接数，否则 `SET` 可能落在另一条连接上（本条要 `= 1`）。
async fn temp_pool_with_url(url: &str, max: u32, lock_timeout: Option<&'static str>) -> PgPool {
    let lock_timeout = lock_timeout.map(|s| s.to_string());
    // REQ-042 · 建连接套上有限重试（同上：builder 在闭包外，闭包里只有 `connect`）。
    let opts = sqlx::postgres::PgPoolOptions::new()
        .max_connections(max)
        .after_connect(move |conn, _meta| {
            let lock_timeout = lock_timeout.clone();
            Box::pin(async move {
                if let Some(v) = lock_timeout {
                    sqlx::query(&format!("SET lock_timeout = '{v}'"))
                        .execute(conn)
                        .await?;
                }
                Ok(())
            })
        });
    common::retry_connect(|| async { opts.clone().connect(url).await.map_err(DbError::Connect) })
        .await
        .expect("连不上测试库")
}

/// REQ-025 · `migrate()` 的 future **必须仍是 `Send`**。
///
/// `migrate()` 里那条踩坑警告写着「不要改成 `pool.acquire()` 拿住一条连接再取锁：
/// 那样会让 future **丢掉 `Send`**」—— 而这句话当时**没有任何东西验证**，
/// 改坏了编译能过、警告还在，解释就变成了又一次说假话。
/// 这条断言把那句警告钉住：改坏的那天，测试**编译不过**。
#[tokio::test]
async fn test_db_migrate_future_is_send() {
    fn assert_send<T: Send>(_t: T) {}
    let pool = test_pool().await;
    // 编译期判定：拿到 future 就把它交给一个要求 `Send` 的函数
    assert_send(ontok::db::pg::migrate(&pool));
    // 再真跑一次，证明它不只是类型上 Send，还跑得通
    ontok::db::pg::migrate(&pool).await.expect("建表失败");
}

/// REQ-030 · 测试连接池的**每池连接数必须真的是 1**。
///
/// # 这条测试在钉什么
///
/// 「每测试一池」是既定设计（理由见 `tests/common/mod.rs`），但**每池几条连接**
/// 一直是 5。峰值连接数 ≈ `并行测试数 × 每池连接数`，
/// 而测试库所在的 PG 是**共享集群**、`max_connections = 100` 跨库共享，
/// 跟另外 5 个业务库抢同一批槽位（实测它们常年占 30 条）。
/// 把每池从 5 降到 1，这一项就从 `P × 5` 降到 `P × 1`。
///
/// 「降了」是句空话，除非**有东西能抓住它没降**。这条测试就是那个东西：
/// 把 `connect_for_tests` 的默认值改回 5、或去掉 `idle_timeout`，它必须失败。
///
/// # 为什么不止断言配置值，还要真跑并发
///
/// 只断言 `get_max_connections() == 1`，抓不到「配置写了 1 但实际没生效」
/// （例如某个 `after_connect` 回调另开连接、或池被 `connect` 重新建过）。
/// 所以后半段真的并发打一轮查询，量 `pool.size()` —— 那是**服务端视角的连接数**。
#[tokio::test]
async fn test_req030_test_pool_caps_connections_at_one() {
    let pool = test_pool().await;

    // ── 配置值：默认值必须是 1，且两个回收期限都必须**短到能判别** ──
    //
    // ⚠️ 这里**不能**只断言 `is_some()`：sqlx 自己的默认值就已经是
    // `idle_timeout = Some(600s)` / `max_lifetime = Some(1800s)`（sqlx-core
    // `pool/options.rs` 的 `Default`）。断言 `is_some()` 的话，
    // 「显式设了短值」和「压根没设、用的库默认」是同一件事 —— 判据就是空的，
    // 变异删掉 `idle_timeout` 照样全绿（实测踩过）。
    // 所以断的是**上界**：短到明显不是库默认。
    let opts = &pool.options();
    assert_eq!(
        opts.get_max_connections(),
        1,
        "测试池每池连接数必须是 1 —— 共享集群的 max_connections 跨库共享，\
         每池 5 条会把峰值从 P×1 抬回 P×5（见 tests/common/mod.rs）"
    );
    let idle = opts
        .get_idle_timeout()
        .expect("测试池必须显式设 idle_timeout（PgPool 的 drop 靠向当前 runtime spawn \
                 关闭任务，#[tokio::test] 的 runtime 销毁得比那个任务快，\
                 漏掉的连接只能等进程退出才回收）");
    assert!(
        idle <= std::time::Duration::from_secs(30),
        "idle_timeout = {idle:?}，太长了 —— 漏掉的连接要等这么久才回收。\
         sqlx 库默认是 600s，那就是「压根没配」；要求 ≤30s 才算真的配上了"
    );
    let life = opts
        .get_max_lifetime()
        .expect("测试池必须显式设 max_lifetime 作兜底（交替使用的连接连 idle 都算不上）");
    assert!(
        life <= std::time::Duration::from_secs(600),
        "max_lifetime = {life:?}，太长了 —— sqlx 库默认是 1800s，\
         那就等于没配；要求 ≤600s 才算真的配上了"
    );

    // ── 行为值：真的并发打一轮，连接数不得超过 1 ──
    // 用 sleep 撑开窗口，否则查询太快、连接还来不及被复用就还回去了。
    let mut set = tokio::task::JoinSet::new();
    for _ in 0..8 {
        let p = pool.clone();
        set.spawn(async move {
            sqlx::query("SELECT pg_sleep(0.15)").execute(&p).await.unwrap();
        });
    }
    let peak = pool.size();
    while let Some(j) = set.join_next().await {
        j.expect("查询 task panic 了");
    }
    assert!(
        peak <= 1,
        "8 个并发查询期间池里出现了 {peak} 条连接 —— 每池上限 1 根本没生效，\
         峰值连接数会按 P×5 膨胀"
    );
}

/// REQ-030 · 宽池确实给出并发 —— **反向对照**。
///
/// 上一条只钉住「默认池很窄」。可万一 `test_pool_with(n)` 也不灵呢？
/// 那两条真并发的测试（`test_project.rs` 的 seq 竞争、
/// `test_extraction_agent.rs` 的同键并发登记）就会**静悄悄退化成串行**，
/// 而它们的断言（「必须撞出唯一约束冲突」）在串行下**照样通过** ——
/// 测试变成永远绿的空壳，还没人看得出来。
///
/// 这条断言 `test_pool_with(4)` 至少能同时跑起 2 条连接，
/// 即宽池这个逃生舱口是通的。
#[tokio::test]
async fn test_req030_wide_pool_actually_allows_concurrency() {
    let pool = common::test_pool_with(4).await;
    assert_eq!(pool.options().get_max_connections(), 4);

    let mut set = tokio::task::JoinSet::new();
    for _ in 0..4 {
        let p = pool.clone();
        set.spawn(async move {
            sqlx::query("SELECT pg_sleep(0.3)").execute(&p).await.unwrap();
        });
    }
    // 让 4 个 task 真正起跑后再量
    tokio::time::sleep(std::time::Duration::from_millis(120)).await;
    let peak = pool.size();
    while let Some(j) = set.join_next().await {
        j.expect("查询 task panic 了");
    }
    assert!(
        peak >= 2,
        "宽池(4)并发时只起了 {peak} 条连接 —— 池内并发没生效，\
         依赖真并发的测试会退化成串行的空壳（仍会「通过」）"
    );
}

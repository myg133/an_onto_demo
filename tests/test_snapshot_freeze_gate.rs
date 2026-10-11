//! REQ-038 · `set_extracted_by` 的冻结闸门（**运行期**判据）。
//!
//! # 为什么这两条不放在 `test_snapshot_write_paths.rs` 里
//!
//! 不是为了分文件好看，是因为**放在那儿会把测试二进制变成不稳定的**：
//!
//! `test_snapshot_write_paths.rs` 里的
//! `test_backfill_is_deterministic_preserves_and_idempotent` 会**额外跑 3 次**
//! `common::migrate()`。`migrate()` 执行
//! `ALTER TABLE ... ADD COLUMN IF NOT EXISTS` —— 即使列已存在，ALTER TABLE 仍要取
//! **AccessExclusiveLock**。而同一二进制里其它测试正在 INSERT 带外键的行，
//! 外键检查要取 **RowShareLock**。两者并发 ⇒ 经典的锁环，实测报
//! `40P01 deadlock detected`（一个进程等 AccessExclusiveLock 被对方挡，
//! 对方同时等 RowShareLock）。
//!
//! 实测（同一台机、同一共享集群、同一命令）：
//!   · 基线 `9cc7613`：该二进制连跑 6 轮，**红了 2 轮**，同样是上面那个死锁；
//!   · 把本文件这两条 DB 判据塞进去后：连跑 6 轮**红了 3 轮**。
//!
//! ⚠️ 基线**本来就不稳** —— 那个死锁是既有的，不是本 REQ 引入的。
//! 拆二进制的收益是「不再加重它」，不是「把它修好」；
//! 底层 hazard 仍未修，见交付报告「发现但未修」。
//! （另：死锁率随共享集群当时的负载浮动，不声称稳定值。）
//!
//! `cargo test` 默认**逐个测试二进制串行执行**、二进制**内部并行**，
//! 所以把会 DDL + 插外键行的 DB 判据放进独立二进制，它就不会与
//! `test_backfill_..._idempotent` 的 DDL 循环并发。
//!
//! ⚠️ 底层那个「同二进制内 DDL 与 FK 写入并发」 hazards 是**既有**的，
//! 本 REQ 没有动它（那是 `test_backfill_..._idempotent` 的问题）。
//! 这里只是**不往那口锅里再加水**。见交付报告「发现但未修」。
//!
//! 结构期判据（源码顺序 / 同事务）不碰数据库，仍留在
//! `test_snapshot_write_paths.rs`，与 REQ-033 的结构判据放在一起。

mod common;
use common::test_pool;
use ontok::db::pg::StoreError;
use ontok::db::project::PgProjectStore;
use ontok::db::snapshot_store::PgStore;
use ontok::domain::snapshot::ExtractedBy;
use uuid::Uuid;

#[tokio::test]
async fn test_set_extracted_by_rejects_frozen_version() {
    let pool = test_pool().await;
    let projects = PgProjectStore::new(pool.clone());
    let store = PgStore::new(pool.clone());
    projects.migrate().await.unwrap();
    let project = common::fresh_project(&projects, "req038gate").await;

    let eb = ExtractedBy {
        agent: "vendor-extractor".into(),
        version: "1.0.0".into(),
        round: 1,
        failed_groups: vec![],
        source: None,
    };

    // ① 未冻结 ⇒ 正常写入（闸门必须真的放行，否则下面全绿也没有意义）
    let v = projects.new_version(project.id).await.unwrap();
    store
        .set_extracted_by(v.id, &Some(eb.clone()))
        .await
        .unwrap();
    let got = store.get_extracted_by(v.id).await.unwrap();
    assert_eq!(
        got,
        Some(Some(eb.clone())),
        "未冻结版本必须能写入 extracted_by（前提失效：闸门被写成了永远拒绝）"
    );

    projects.freeze(v.id).await.unwrap();

    // ② 冻结后写 NULL（= 人工修订）必须被拒
    let r = store.set_extracted_by(v.id, &None).await;
    assert!(
        matches!(r, Err(StoreError::VersionFrozen { .. })),
        "冻结版本上的 set_extracted_by 必须报 VersionFrozen，实际：{r:?}"
    );

    // ③ 溯源没被抹成 NULL —— 这是本 REQ 真正的破口所在
    assert!(
        store.get_extracted_by(v.id).await.unwrap().flatten().is_some(),
        "被拒的那次写入仍把 extracted_by 抹成了 NULL —— \
         NULL 在 ADR-001 里是「人工修订」，等于给冻结版本伪造了一个人工修订标记"
    );

    // ④ 冻结后写**非空**值同样必须被拒（不能只挡 NULL）
    let other = ExtractedBy {
        agent: "attacker".into(),
        version: "9.9.9".into(),
        round: 9,
        failed_groups: vec![],
        source: None,
    };
    let r2 = store.set_extracted_by(v.id, &Some(other)).await;
    assert!(
        matches!(r2, Err(StoreError::VersionFrozen { .. })),
        "冻结版本上写非空 extracted_by 也必须被拒，实际：{r2:?}"
    );
    assert_eq!(
        store.get_extracted_by(v.id).await.unwrap(),
        Some(Some(eb.clone())),
        "第二次被拒的写入改动了 extracted_by"
    );
}

/// 版本不存在 ⇒ `Db(RowNotFound)`，与 `save_snapshot` 同形。
///
/// 这条同时是「闸门真的在查库」的判别力证据：
/// 裸 UPDATE 打到不存在的 `id` 时**匹配 0 行且不报错**，`execute` 照样返回 `Ok`，
/// 冻结闸被整段摘掉时这个测试会红。若哪天有人为了省一次查询把闸门删了，
/// 这里就是拦住他的那一条。
#[tokio::test]
async fn test_set_extracted_by_unknown_version_is_row_not_found() {
    let pool = test_pool().await;
    let store = PgStore::new(pool.clone());
    store.migrate().await.unwrap();

    let missing = Uuid::new_v4();
    let r = store.set_extracted_by(missing, &None).await;
    assert!(
        matches!(r, Err(StoreError::Db(sqlx::Error::RowNotFound))),
        "对不存在的版本写 extracted_by 必须报 RowNotFound（与 save_snapshot 同形），实际：{r:?}"
    );
}
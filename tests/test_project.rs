//! Task 2 · Project + OntologyVersion
//!
//! 重点：版本隔离、`frozen_at` 三态、seq 递增。

mod common;
use common::{test_pool, unique_project};
use ontok::db::project::{NewProject, PgProjectStore, ProjectError};

async fn store() -> PgProjectStore {
    let s = PgProjectStore::new(test_pool().await);
    s.migrate().await.unwrap();
    s
}

#[tokio::test]
async fn test_project_create_and_isolated() {
    let s = store().await;
    let a = s.create(NewProject { name: unique_project("acme") }).await.unwrap();
    let b = s.create(NewProject { name: unique_project("globex") }).await.unwrap();
    assert_ne!(a.id, b.id, "不同项目必须是不同 id");
}

#[tokio::test]
async fn test_project_duplicate_name_rejected() {
    let s = store().await;
    let name = unique_project("dup");
    s.create(NewProject { name: name.clone() }).await.unwrap();
    let r = s.create(NewProject { name: name.clone() }).await;
    assert!(
        matches!(r, Err(ProjectError::NameTaken(_))),
        "同名项目应被拒，实际：{r:?}"
    );
}

#[tokio::test]
async fn test_project_get_or_create_is_idempotent() {
    // ⚠️ **本文件唯一故意保留 `get_or_create` 的测试，且是全仓仅此一处。**
    //
    // 理由：这里 `get_or_create` 不是「拿项目」的便利手段，而是**被测对象本身** ——
    // 本测试断言的正是它「同名两次调用返回同一个项目」这条幂等语义。
    // 换成 `create` 会把这个测试改成断言别的东西（`create` 的第二次调用返回
    // `NameTaken`，那是 `test_project_duplicate_name_rejected` 的活）。
    //
    // 与 REQ-026 的关系：REQ-026 要求**测试侧拿项目一律 fail-closed（撞名即失败）**，
    // 前提是「复用」永远不是测试想要的语义。而本测试要的恰恰**就是**复用语义本身，
    // 所以它不受那条约束，也不构成「撞名会静默读到脏数据」的隐患：
    // 名字由 `unique_project` 现算，两次调用在**同一次运行内**指向同一行。
    let s = store().await;
    let name = unique_project("goc");
    let a = s.get_or_create(&name).await.unwrap();
    let b = s.get_or_create(&name).await.unwrap();
    assert_eq!(a.id, b.id, "get_or_create 两次应拿到同一个项目");
}

#[tokio::test]
async fn test_version_first_seq_is_one() {
    let s = store().await;
    let p = common::fresh_project(&s, "seq").await;
    let v = s.new_version(p.id).await.unwrap();
    assert_eq!(v.seq, 1, "首版 seq 从 1 开始");
}

#[tokio::test]
async fn test_version_frozen_at_is_none_before_freeze() {
    // **三态第一个坑**：`frozen_at` 必须是 NULL 而不是 now()
    let s = store().await;
    let p = common::fresh_project(&s, "frozen").await;
    let v = s.new_version(p.id).await.unwrap();
    assert!(
        v.frozen_at.is_none(),
        "未冻结时 frozen_at 应为 None，实际：{:?}",
        v.frozen_at
    );
    assert!(!s.is_frozen(v.id).await.unwrap());
}

#[tokio::test]
async fn test_version_freeze_sets_frozen_at_and_is_idempotent() {
    let s = store().await;
    let p = common::fresh_project(&s, "frz").await;
    let v = s.new_version(p.id).await.unwrap();
    let f1 = s.freeze(v.id).await.unwrap();
    assert!(f1.frozen_at.is_some());
    let f2 = s.freeze(v.id).await.unwrap();
    assert_eq!(f1.frozen_at, f2.frozen_at, "重复冻结应幂等，不改时间戳");
}

#[tokio::test]
async fn test_version_seq_increments_and_is_isolated() {
    let s = store().await;
    let a = common::fresh_project(&s, "inc_a").await;
    let b = common::fresh_project(&s, "inc_b").await;

    s.new_version(a.id).await.unwrap();
    s.new_version(a.id).await.unwrap();
    let va = s.list_versions(a.id).await.unwrap();
    assert_eq!(va.iter().map(|v| v.seq).collect::<Vec<_>>(), vec![1, 2]);

    // 隔离：B 项目看不到 A 的版本，且 B 的首版仍是 1
    let vb = s.list_versions(b.id).await.unwrap();
    assert!(vb.is_empty(), "B 还没有版本");
    let b1 = s.new_version(b.id).await.unwrap();
    assert_eq!(b1.seq, 1, "每个项目的 seq 各自从 1 开始");
    assert_eq!(s.list_versions(b.id).await.unwrap().len(), 1);
}

#[tokio::test]
async fn test_version_by_seq_not_found_is_error() {
    let s = store().await;
    let p = common::fresh_project(&s, "nf").await;
    let r = s.version_by_seq(p.id, &p.name, 99).await;
    assert!(
        matches!(r, Err(ProjectError::VersionNotFound { .. })),
        "不存在的 seq 应报 VersionNotFound，实际：{r:?}"
    );
}

#[tokio::test]
async fn test_version_latest_picks_highest_seq() {
    let s = store().await;
    let p = common::fresh_project(&s, "latest").await;
    s.new_version(p.id).await.unwrap();
    s.new_version(p.id).await.unwrap();
    s.new_version(p.id).await.unwrap();
    let l = s.latest_version(p.id).await.unwrap().unwrap();
    assert_eq!(l.seq, 3);
}

#[tokio::test]
async fn test_version_seq_under_concurrency_is_unique() {
    // **seq 必须在事务里加锁取** —— 只用 MAX() 不加锁时，
    // 两个并发调用会算出同一个 seq，撞 UNIQUE(project_id, seq)。
    //
    // 用 Barrier 让所有任务**在同一时刻**起跑：否则任务可能串行执行，
    // 根本撞不上，测试就成了「永远通过」的假命题
    // （第一次写这个测试时就是那样 —— 变异回退后它仍然 ok）。
    //
    // ⚠️ Barrier 只对齐**起跑时刻**，不保证**同时**在飞：8 个 task 共用一个池，
    // 池若只有 1 条连接，它们照样一个接一个串行执行，seq 永不冲突，测试又变回
    // 「永远通过」的假命题。所以本测试**必须**用 N 条连接的宽池（REQ-030）。
    const N: usize = 8;
    let s = PgProjectStore::new(common::test_pool_with(N as u32).await);
    s.migrate().await.unwrap();
    let p = common::fresh_project(&s, "conc").await;

    let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(N));
    let mut set = tokio::task::JoinSet::new();
    for _ in 0..N {
        let store = s.clone();
        let pid = p.id;
        let b = barrier.clone();
        set.spawn(async move {
            b.wait().await; // 齐步走
            store.new_version(pid).await.map(|v| v.seq)
        });
    }

    let mut seqs = Vec::new();
    let mut errors = Vec::new();
    while let Some(r) = set.join_next().await {
        match r.unwrap() {
            Ok(seq) => seqs.push(seq),
            Err(e) => errors.push(e.to_string()),
        }
    }
    seqs.sort();
    assert!(
        errors.is_empty(),
        "并发建版本不应报错（撞唯一约束说明 seq 计算没加锁）：{errors:?}"
    );
    assert_eq!(
        seqs,
        (1..=N as i32).collect::<Vec<_>>(),
        "并发建版本必须得到 1..={N} 互不相同的 seq，实际：{seqs:?}"
    );
}

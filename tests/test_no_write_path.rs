//! AC-004 · 切片 1 无任何可执行写回路径
//!
//! **这是本 MVP 的安全底线**（规格 §9.1）：只读闭环。
//!
//! # 「无任何路径」无法穷尽证明
//!
//! 单个测试只能证伪它想到的路径。这里做的是**已知入口清单 + 逐条断言不存在**，
//! 判定口径是「清单内 0 个可执行入口」= 通过 —— **不声称数学意义上的完备**
//! （QA 的 U-01 明确标了这一点）。

mod common;
use common::test_pool;
use ontok::db::project::PgProjectStore;
use ontok::db::snapshot_store::PgStore;
use ontok::domain::snapshot::Snapshot;

/// 写回指的是「把本体里的东西写回客户的业务系统」。
/// 规格 §5.2 的 `WriteBinding` 描述了写回**长什么样**，但切片 1 不得有执行器。
#[test]
fn test_no_write_path_in_slice1() {
    // ① 领域层不得有任何写回执行入口
    //    写回需要「目标系统 + 表 + 字段 + 取值」，全是 I/O；
    //    只要 domain 层零 I/O（AC-002），就不可能在这里执行写回。
    let src = include_str!("../src/domain/mod.rs");
    assert!(src.contains("pub mod"), "domain/mod.rs 声明的模块应可读");

    // ② 逐个领域模块里不得出现写回动词
    for (name, code) in [
        ("snapshot", include_str!("../src/domain/snapshot.rs")),
        ("apply", include_str!("../src/domain/apply.rs")),
        ("review", include_str!("../src/domain/review.rs")),
        ("view", include_str!("../src/domain/view.rs")),
        ("diff", include_str!("../src/domain/diff.rs")),
        ("hierarchy", include_str!("../src/domain/hierarchy.rs")),
    ] {
        for verb in ["fn execute", "fn write_back", "fn dispatch", "fn run_action", "fn apply_effect"] {
            assert!(
                !code.contains(verb),
                "切片 1 不得有写回执行入口：{name}.rs 里出现了 `{verb}`"
            );
        }
    }
}

/// WriteBinding 的结构**必须在**（它是切片 2 的地基），
/// 但它只是数据，没有执行器。
#[test]
fn test_write_binding_is_structure_only() {
    let snap = Snapshot::new(uuid::Uuid::nil(), 1);
    // 结构在：编译期就该证明（下面这行构造不出 WriteBinding 才算失败）
    let _wb = ontok::domain::snapshot::WriteBinding {
        system: "erp".into(),
        table: "t_ticket".into(),
        field: "status".into(),
        key_expr: serde_json::json!({"col": "ticket_id"}),
        value_map: Default::default(),
        supported_values: vec!["dispatched".into()],
        conflict_field: Some("row_version".into()),
        evidence: vec![],
    };
    // 快照里没有可执行的 effect
    assert!(snap.action_types.is_empty());
    assert!(snap.functions.is_empty());
}

/// 存储层只应连接**这一个** PG，不得持有指向客户业务系统的第二个连接。
///
/// 写回需要「另一个系统」的连接。切片 1 的连接目标唯一 ⇒ 结构上不可能写回。
#[tokio::test]
async fn test_store_connects_to_exactly_one_database() {
    let pool = test_pool().await;
    let db: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&pool)
        .await
        .unwrap();

    let store = PgStore::new(pool.clone());
    let projects = PgProjectStore::new(pool.clone());
    let proposals = ontok::db::proposal::PgProposalStore::new(pool.clone());
    let applier = ontok::db::apply::PgApplier::new(pool.clone());

    // 四个存储入口，连接目标必须全都是同一个库
    for (name, opt) in [
        ("PgStore", store.pool().connect_options().get_database().map(|s| s.to_string())),
        ("PgProjectStore", projects.pool().connect_options().get_database().map(|s| s.to_string())),
        ("PgProposalStore", proposals.pool().connect_options().get_database().map(|s| s.to_string())),
        ("PgApplier", applier.pool().connect_options().get_database().map(|s| s.to_string())),
    ] {
        assert_eq!(opt.as_deref(), Some(db.as_str()), "{name} 的连接目标应与测试库一致");
    }
}

/// 连接串**只能来自环境变量** —— 代码里没有硬编码的连接目标，
/// 也就没有「偷偷连到客户系统」的可能。
#[test]
fn test_no_hardcoded_connection_string() {
    for (name, code) in [
        ("pool", include_str!("../src/db/pool.rs")),
        ("config", include_str!("../src/config.rs")),
        ("pg", include_str!("../src/db/pg.rs")),
        ("project", include_str!("../src/db/project.rs")),
        ("apply", include_str!("../src/db/apply.rs")),
    ] {
        assert!(
            !code.contains("postgresql://") && !code.contains("postgres://"),
            "{name}.rs 里不得出现硬编码连接串（口令不得入库）"
        );
    }
}

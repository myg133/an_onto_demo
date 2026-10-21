//! REQ-115 · **N-1：导入闸的接线必须被真进程钉住**（QA 复审阻断）。
//!
//! # 为什么必须是真进程
//!
//! `scan_group_names` 的单测只能证明「这个函数算得出命中」，
//! 证明不了 `concept import` **真的调了它**。QA 实测过：
//! 把 `src/cli.rs` 里的调用点删掉（函数留着），`lib` 与 `test_req115_groups` **全绿**
//! —— 这和 REQ-114 那个「探针数文本不看行为」是同一类病：**函数对了，但没人证明它被调用。**
//!
//! 所以本文件 spawn 真 `ontok concept import`：
//!
//! 1. `scope_hint` 含 `key` 的声明 ⇒ **exit 1**、stderr 点名「组 + 字段 + 词条」、
//!    **库里提案数不变**（被拒的声明一条提案都不许留）；
//! 2. 干净声明 ⇒ **exit 0**、提案数 **+1**。
//!
//! # 变异验证（本测试有牙）
//!
//! 把 `src/cli.rs` 里 `let bad_names = …` + `if !bad_names.is_empty() { … }` 整段注释掉，
//! ① 会变成 exit 0 并建出提案 ⇒ 本测试**必须红**（已在交付里实测贴出）。

mod common;

use ontok::db::project::PgProjectStore;
use ontok::domain::concept::{
    content_digest, ConceptDeclaration, ConceptGroup, DeclarationRevision,
};

const DECLARED_BY: &str = "req115-import-gate-test";
const RULE: &str = "测试用规则：只验闸的接线";

/// 一份最小的合法声明（一个组、一个成员）。`scope_hint` 由调用方给。
fn declaration(scope_hint: Option<&str>) -> String {
    let groups = vec![ConceptGroup {
        key: "g1".to_string(),
        display_name: "干净组名".to_string(),
        members: vec!["t1".to_string()],
        scope_hint: scope_hint.map(str::to_string),
        low_confidence: Some(true),
        extracted_by: None,
        evidence: vec![],
    }];
    let digest = content_digest(&groups, RULE, DECLARED_BY);
    let decl = ConceptDeclaration {
        version: "1.0.0".to_string(),
        declared_by: DECLARED_BY.to_string(),
        rule: RULE.to_string(),
        confirmed_by: None,
        source: None,
        reviewed_by: None,
        history: vec![DeclarationRevision {
            version: "1.0.0".to_string(),
            content_digest: digest,
            note: "测试".to_string(),
        }],
        groups,
    };
    serde_json::to_string_pretty(&decl).expect("声明必须可序列化")
}

/// 建一个**全新**项目 + v1（未冻结）—— `concept import` 需要一个可提案的 base。
async fn fresh_project_with_v1(prefix: &str) -> String {
    let pool = common::test_pool().await;
    let ps = PgProjectStore::new(pool.clone());
    ps.migrate().await.expect("建表失败");
    let name = common::unique_project(prefix);
    let p = common::new_project(&ps, &name).await;
    ps.new_version(p.id).await.expect("建版本失败");
    name
}

/// 该项目的提案数（直接读库，不信 CLI 自述）。
async fn proposal_count(project: &str) -> i64 {
    let pool = common::test_pool().await;
    sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM proposals pr \
         JOIN projects p ON p.id = pr.project_id \
         WHERE p.name = $1",
    )
    .bind(project)
    .fetch_one(&pool)
    .await
    .expect("查提案数失败")
}

fn write_decl(json: &str, tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir();
    let file = dir.join(format!(
        "req115-import-gate-{}-{tag}.json",
        std::process::id()
    ));
    std::fs::write(&file, json).expect("写声明文件失败");
    file
}

fn run_import(file: &std::path::Path, project: &str) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_ontok"))
        .args(["concept", "import", "--file"])
        .arg(file)
        .args(["--project", project, "--base-version", "1"])
        .env(
            "ONTOK_DATABASE_URL",
            ontok::config::test_database_url().expect("缺 ONTOK_TEST_DATABASE_URL"),
        )
        .output()
        .expect("跑真 `ontok concept import` 失败")
}

// ── ① 脏 scope_hint ⇒ 真进程 exit 1 + 不入库 ────────────────

#[tokio::test]
async fn dirty_scope_hint_is_rejected_by_the_real_import_process() {
    let project = fresh_project_with_v1("req115gate_dirty").await;
    let file = write_decl(
        &declaration(Some("客户手上有什么凭证、以及未持 Key 的试用请求")),
        "dirty",
    );

    let out = run_import(&file, &project);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert_eq!(
        out.status.code(),
        Some(1),
        "脏 scope_hint 必须 exit 1（否则闸根本没接线）\nstdout={stdout}\nstderr={stderr}"
    );
    assert!(
        stderr.contains("组 `g1` 的 scope_hint 含建模术语 `key`"),
        "stderr 必须点名「组 + 字段 + 词条」，实际：\n{stderr}"
    );
    assert_eq!(
        proposal_count(&project).await,
        0,
        "被拒的声明**一条提案都不许留下**（校验必须在碰库之前）"
    );

    let _ = std::fs::remove_file(&file);
}

// ── ② 干净声明 ⇒ 真进程 exit 0 + 提案 +1 ────────────────────

#[tokio::test]
async fn clean_declaration_is_imported_by_the_real_import_process() {
    let project = fresh_project_with_v1("req115gate_clean").await;
    let file = write_decl(
        &declaration(Some("客户手上有什么凭证、以及未持凭证的试用请求")),
        "clean",
    );

    let out = run_import(&file, &project);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert_eq!(
        out.status.code(),
        Some(0),
        "干净声明必须 exit 0\nstdout={stdout}\nstderr={stderr}"
    );
    assert!(
        stdout.contains("共 1 个概念组"),
        "成功回执应报导入的组数，实际：\n{stdout}"
    );
    assert_eq!(
        proposal_count(&project).await,
        1,
        "成功导入必须真的落一条提案（否则「通过」是假绿）"
    );

    let _ = std::fs::remove_file(&file);
}

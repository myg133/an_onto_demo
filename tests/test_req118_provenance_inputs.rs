//! REQ-118 · A-1（P4）· **提案 → 库**的溯源：`inputs.names` / `inputs.concepts`。
//!
//! ADR-005 P4 原文：「产物里能查到这条用的是**哪份提案**、**谁签的**」。
//! 只把 sha 写进输入侧文件（`TableNames._proposal`）不算 —— 提案一入库，
//! 审核记录就得能回答「这 96 个名字来自哪个文件」。所以本文件真跑 CLI：
//!
//! ```text
//! ontok-propose --names f ──▶ proposal.json（inputs.names）
//!        │ ontok propose import
//!        ▼
//!   proposals.inputs（PG）        ← 本文件从**库里**读回来比对 sha256
//! ```
//!
//! | 用例 | 判据 |
//! | --- | --- |
//! | names | `inputs.names = {file, sha256, agent, version, reviewed_by}`；sha256 == 文件实际 sha256；`reviewed_by` 键存在且为 null |
//! | names + exclude | **两个块都要活下来**（`inputs.exclusions` 的校验分支不许把 `names` 吞掉） |
//! | concepts | `inputs.concepts = {file, sha256, version, declared_by, confirmed_by, reviewed_by}`；sha256 == 声明文件实际 sha256 |
//!
//! 需要 `ONTOK_TEST_DATABASE_URL`（与其余 DB 用例同一前提）。

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use ontok::db::project::PgProjectStore;
use ontok::db::proposal::PgProposalStore;
use ontok::domain::concept::{
    content_digest, ConceptDeclaration, ConceptGroup, DeclarationRevision,
};
use serde_json::{json, Value};
use uuid::Uuid;

const BIN: &str = env!("CARGO_BIN_EXE_ontok");
const BIN_PROPOSE: &str = env!("CARGO_BIN_EXE_ontok-propose");

fn crate_root() -> PathBuf {
    common::crate_root()
}

fn fixture_schema() -> PathBuf {
    crate_root().join("tests/fixtures/req118/schema.json")
}

fn scratch(tag: &str) -> PathBuf {
    let p = crate_root()
        .join("out")
        .join("req118-provenance")
        .join(format!("{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).expect("建临时目录");
    p
}

fn sha256_file(path: &Path) -> String {
    ontok::extract::sample::sha256_hex(&std::fs::read(path).expect("读文件"))
}

fn write_names(dir: &Path, with_proposal_ref: bool) -> PathBuf {
    let mut doc = json!({
        "agent": "ai:fake-model-req118",
        "version": "0.1.0+ai.fake-model-req118",
        "generated_at": "2026-10-22T00:00:00+00:00",
        "source": "public schema, 4 tables (introspected 2026-10-22)",
        "names": {
            "customers": "客户档案",
            "orders": "销售订单",
            "alembic_version": "数据库版本台账",
            "_volc_dts_pg_ddl_abc": "同步临时表"
        }
    });
    if with_proposal_ref {
        doc.as_object_mut().unwrap().insert(
            "_proposal".to_string(),
            json!({
                "file": "out/req118-real/cache-fresh/names.proposal.json",
                "sha256": "5d991434fafcc1300e47c687266efd8abee85ba960a46de76a2f29f67c844955",
                "decided_by": "ai:fake-model-req118",
                // 未签：字段存在且为 null（P2）
                "reviewed_by": null
            }),
        );
    }
    let p = dir.join(if with_proposal_ref {
        "names-with-proposal.json"
    } else {
        "names-plain.json"
    });
    std::fs::write(&p, serde_json::to_string_pretty(&doc).unwrap()).unwrap();
    p
}

fn write_exclusions(dir: &Path) -> PathBuf {
    let doc = json!({
        "version": "0.1.1",
        "rule": "只排除不是客户业务世界里的事物",
        "declared_by": "ai:fake-model-req118",
        "reviewed_by": null,
        "exclude": [{
            "table": "_volc_dts_pg_ddl_abc",
            "category": "cdc_residue",
            "reason": "同步工具生成的临时表，不属于客户业务",
            "decided_by": "ai:fake-model-req118",
            "confidence": 0.97
        }]
    });
    let p = dir.join("exclusions.json");
    std::fs::write(&p, serde_json::to_string_pretty(&doc).unwrap()).unwrap();
    p
}

fn run_propose(project: &str, names: &Path, exclusions: Option<&Path>, dir: &Path) -> Value {
    let out = dir.join("proposal.json");
    let stats = dir.join("stats.json");
    let mut args: Vec<String> = vec![
        "--schema".into(),
        fixture_schema().to_string_lossy().into_owned(),
        "--names".into(),
        names.to_string_lossy().into_owned(),
        "--out".into(),
        out.to_string_lossy().into_owned(),
        "--stats".into(),
        stats.to_string_lossy().into_owned(),
        "--project".into(),
        project.to_string(),
        "--no-sample".into(),
    ];
    if let Some(ex) = exclusions {
        args.push("--exclude".into());
        args.push(ex.to_string_lossy().into_owned());
    }
    let o = Command::new(BIN_PROPOSE)
        .args(&args)
        .current_dir(crate_root())
        .output()
        .expect("跑不动 ontok-propose");
    assert!(
        o.status.success(),
        "ontok-propose 失败：\n{}",
        String::from_utf8_lossy(&o.stderr)
    );
    serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).expect("proposal 是 JSON")
}

fn run_import(url: &str, proposal: &Path) -> String {
    let o = Command::new(BIN)
        .args(["propose", "import", "--file", proposal.to_str().unwrap()])
        .env("ONTOK_DATABASE_URL", url)
        .env("NO_COLOR", "1")
        .current_dir(crate_root())
        .output()
        .expect("跑不动 ontok propose import");
    assert!(
        o.status.success(),
        "propose import 失败：\nSTDOUT:\n{}\nSTDERR:\n{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).into_owned()
}

async fn newest_proposal_inputs(
    pool: &sqlx::PgPool,
    store: &PgProposalStore,
    project_id: Uuid,
) -> Value {
    let rows: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM proposals WHERE project_id = $1 ORDER BY created_at DESC, id DESC LIMIT 1",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await
    .expect("查最新提案");
    assert_eq!(rows.len(), 1, "至少导入过一条提案");
    store
        .inputs(rows[0].0)
        .await
        .expect("读 inputs")
        .expect("inputs 已落库")
}

#[tokio::test]
async fn names_provenance_is_persisted_with_matching_sha256() {
    let url = ontok::config::test_database_url().expect("缺 ONTOK_TEST_DATABASE_URL");
    let pool = common::test_pool().await;
    common::migrate(&pool).await;
    let projects = PgProjectStore::new(pool.clone());
    let proposals = PgProposalStore::new(pool.clone());
    let p = common::fresh_project(&projects, "req118_a1_names").await;
    projects.new_version(p.id).await.expect("建版本");

    let dir = scratch("names");
    let names = write_names(&dir, true);
    let proposal = run_propose(&p.name, &names, None, &dir);
    // 产物里就有（P1：可 diff）
    let block = &proposal["inputs"]["names"];
    assert_eq!(block["sha256"], sha256_file(&names));
    assert_eq!(block["agent"], "ai:fake-model-req118");
    assert_eq!(block["version"], "0.1.0+ai.fake-model-req118");
    assert!(block.get("reviewed_by").is_some(), "字段必须存在（允许 null）");
    assert_eq!(block["reviewed_by"], Value::Null, "P2：未签");
    assert_eq!(block["proposal"]["sha256"], "5d991434fafcc1300e47c687266efd8abee85ba960a46de76a2f29f67c844955");

    let proposal_path = dir.join("proposal.json");
    run_import(&url, &proposal_path);

    // ★ A-1 的判据：**从库里**读回来，sha256 与文件实际值一致
    let inputs = newest_proposal_inputs(&pool, &proposals, p.id).await;
    let stored = &inputs["names"];
    assert_eq!(
        stored["sha256"].as_str().unwrap(),
        sha256_file(&names),
        "A-1：库里 inputs.names.sha256 必须等于 --names 文件的实际 sha256"
    );
    assert_eq!(stored["file"], names.to_string_lossy().as_ref());
    assert_eq!(stored["agent"], "ai:fake-model-req118");
    assert!(stored.get("reviewed_by").is_some(), "A-1：reviewed_by 字段必须存在");
    assert_eq!(stored["reviewed_by"], Value::Null);
}

#[tokio::test]
async fn names_and_exclusions_both_survive_import() {
    let url = ontok::config::test_database_url().expect("缺 ONTOK_TEST_DATABASE_URL");
    let pool = common::test_pool().await;
    common::migrate(&pool).await;
    let projects = PgProjectStore::new(pool.clone());
    let proposals = PgProposalStore::new(pool.clone());
    let p = common::fresh_project(&projects, "req118_a1_both").await;
    projects.new_version(p.id).await.expect("建版本");

    let dir = scratch("both");
    let names = write_names(&dir, true);
    let ex = write_exclusions(&dir);
    let proposal = run_propose(&p.name, &names, Some(&ex), &dir);
    assert!(proposal["inputs"]["names"].is_object(), "names 块要在");
    assert!(proposal["inputs"]["exclusions"].is_object(), "exclusions 块要在");

    run_import(&url, &dir.join("proposal.json"));
    let inputs = newest_proposal_inputs(&pool, &proposals, p.id).await;
    // ⭐ 回归点：上一版 `Record::from_inputs` 命中时会只写 `{exclusions: rec}`，
    //    把 `names` 整块吞掉。这条用例就是那个缺口的钉子。
    assert_eq!(
        inputs["names"]["sha256"].as_str().unwrap(),
        sha256_file(&names),
        "A-1：有 --exclude 时 inputs.names 不许被吞掉"
    );
    assert_eq!(
        inputs["exclusions"]["sha256"].as_str().unwrap(),
        sha256_file(&ex),
        "REQ-113：inputs.exclusions.sha256 仍须等于排除文件实际 sha256"
    );
}

#[tokio::test]
async fn concept_declaration_provenance_is_persisted_with_matching_sha256() {
    let url = ontok::config::test_database_url().expect("缺 ONTOK_TEST_DATABASE_URL");
    let pool = common::test_pool().await;
    common::migrate(&pool).await;
    let projects = PgProjectStore::new(pool.clone());
    let proposals = PgProposalStore::new(pool.clone());
    let p = common::fresh_project(&projects, "req118_a1_concepts").await;
    projects.new_version(p.id).await.expect("建版本");

    // 一份最小但**契约自洽**的 AI 声明（digest 用生产函数算，不手抄）
    const DECLARED_BY: &str = "ai:fake-model-req118";
    const RULE: &str = "AI 语义分组（测试夹具）";
    let groups = vec![ConceptGroup {
        key: "concept_01".into(),
        display_name: "客户与订单".into(),
        members: vec!["customers".into(), "orders".into()],
        scope_hint: None,
        low_confidence: None,
        extracted_by: None,
        evidence: vec![],
    }];
    let digest = content_digest(&groups, RULE, DECLARED_BY);
    let decl = ConceptDeclaration {
        version: "1.0.0".into(),
        declared_by: DECLARED_BY.into(),
        rule: RULE.into(),
        confirmed_by: None,
        source: Some("ai".into()),
        reviewed_by: None,
        history: vec![DeclarationRevision {
            version: "1.0.0".into(),
            content_digest: digest,
            note: "测试夹具".into(),
        }],
        groups,
    };
    decl.verify().expect("夹具声明必须自洽");
    let dir = scratch("concepts");
    let file = dir.join("concepts.proposal.json");
    std::fs::write(&file, serde_json::to_string_pretty(&decl).unwrap()).unwrap();

    let o = Command::new(BIN)
        .args([
            "concept",
            "import",
            "--file",
            file.to_str().unwrap(),
            "--project",
            p.name.as_str(),
            "--base-version",
            "1",
        ])
        .env("ONTOK_DATABASE_URL", &url)
        .env("NO_COLOR", "1")
        .current_dir(crate_root())
        .output()
        .expect("跑不动 ontok concept import");
    assert!(
        o.status.success(),
        "concept import 失败：\n{}",
        String::from_utf8_lossy(&o.stderr)
    );

    let inputs = newest_proposal_inputs(&pool, &proposals, p.id).await;
    let block = &inputs["concepts"];
    assert_eq!(
        block["sha256"].as_str().unwrap(),
        sha256_file(&file),
        "A-1：库里 inputs.concepts.sha256 必须等于声明文件的实际 sha256"
    );
    assert_eq!(block["file"], file.to_string_lossy().as_ref());
    assert_eq!(block["version"], "1.0.0");
    assert_eq!(block["declared_by"], DECLARED_BY);
    assert!(block.get("confirmed_by").is_some(), "字段必须存在（允许 null）");
    assert!(block.get("reviewed_by").is_some(), "字段必须存在（允许 null）");
    assert_eq!(block["confirmed_by"], Value::Null);
    assert_eq!(block["reviewed_by"], Value::Null);
}


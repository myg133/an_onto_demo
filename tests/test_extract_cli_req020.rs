//! REQ-020 · 第 2 轮抽取的「基版本冻结」**提前**失败（不要白跑一趟）
//!
//! | 判据 | 为什么要有 |
//! | --- | --- |
//! | 最新版本已冻结 ⇒ 第 2 轮**开口就拒**，不留提案文件 | 冻结检查原先是**事后**的（只在 import / apply），于是抽取照常跑完（实测 6.9s 量级）才在 import 硬失败 —— 白跑 + 晚失败 |
//! | 错误信息指名**哪一版**被冻结、**为什么**不能在其上提案、**怎么办** | 「换个说法重试」类报错等于没说；冻结是显式动作，解冻是人工决策，不能让用户猜 |
//! | **绝不**退回空清单 | 退回空清单会让 key 双向核对的三种违规**静默不可达** —— REQ-015 的老病根 |
//!
//! # 与 `ontok propose import` 的分工
//!
//! import 那道闸（`src/cli.rs`）**必须留着**：本文件管的是 `ontok-propose`
//! 这一步别把算力花在注定被拒的一轮上。两处是**同一个事实的两道闸**，
//! 不是重复 —— 一道挡早、一道挡晚。
//!
//! # 跑 CLI
//!
//! 走真实二进制（`CARGO_BIN_EXE_ontok-propose`），临时目录一律落在
//! `.review-workspace/req020/` 下 —— 共享本体库只 INSERT，不删不清。

mod common;

use ontok::db::project::PgProjectStore;
use ontok::db::PgStore;
use ontok::domain::snapshot::{EntityType, Snapshot};

const NAMES_JSON: &str = r#"{
  "agent": "table-name-translator",
  "version": "2026.10.07",
  "generated_at": "2026-10-07T10:00:00Z",
  "source": "req020-test",
  "names": {
    "t_dev": "设备",
    "t_customer": "客户"
  }
}"#;

fn schema_json() -> String {
    let t = |name: &str, comment: &str, cols: &[(&str, &str)]| {
        serde_json::json!({
            "name": name,
            "comment": comment,
            "column_count": cols.len(),
            "commented_count": cols.len(),
            "columns": cols.iter().map(|(c, cm)| serde_json::json!({
                "name": c, "data_type": "text", "nullable": true,
                "comment": cm, "is_primary_key": false
            })).collect::<Vec<_>>(),
        })
    };
    serde_json::to_string_pretty(&serde_json::json!({
        "server_version": "PostgreSQL 17.2",
        "transaction_read_only": "on",
        "schema": "public",
        "table_count": 2,
        "column_count": 4,
        "commented_column_count": 4,
        "foreign_key_count": 0,
        "introspected_at": "2026-10-07T10:00:00Z",
        "tables": [
            t("t_dev", "设备表", &[("id", "主键"), ("sn", "序列号"), ("customer_id", "客户ID"), ("note", "备注")]),
            t("t_customer", "客户表", &[("id", "主键"), ("code", "客户编码")]),
        ],
    }))
    .unwrap()
}

fn workspace(tag: &str) -> std::path::PathBuf {
    let dir = std::path::Path::new("..")
        .join(".review-workspace")
        .join("req020")
        .join(format!("{}-{}", tag, std::process::id()));
    std::fs::create_dir_all(&dir).expect("建临时目录失败");
    dir
}

/// 跑一次 CLI。
fn run_cli(dir: &std::path::Path, project: &str, extra: &[&str]) -> (bool, String, String) {
    let sp = dir.join("schema.json");
    let np = dir.join("names.json");
    std::fs::write(&sp, schema_json()).unwrap();
    std::fs::write(&np, NAMES_JSON).unwrap();
    let out = dir.join("proposal.json");
    let stats = dir.join("stats.json");
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_ontok-propose"));
    cmd.args([
        "--schema",
        sp.to_str().unwrap(),
        "--names",
        np.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--stats",
        stats.to_str().unwrap(),
        "--project",
        project,
    ])
    .args(extra)
    .env(
        "ONTOK_DATABASE_URL",
        ontok::config::test_database_url().expect("缺 ONTOK_TEST_DATABASE_URL"),
    );
    let o = cmd.output().expect("跑 ontok-propose 失败");
    (
        o.status.success(),
        String::from_utf8_lossy(&o.stdout).to_string(),
        String::from_utf8_lossy(&o.stderr).to_string(),
    )
}

/// 造一个**两版**的 project 并**冻结最新那版** —— QA 实测复现缺口时的形状。
///
/// 只 INSERT / UPDATE，共享本体库不做 DELETE / TRUNCATE。
async fn seed_two_versions_with_latest_frozen(project: &str) {
    let pool = common::test_pool().await;
    common::migrate(&pool).await;
    let projects = PgProjectStore::new(pool.clone());
    let p = common::new_project(&projects, project).await;

    for display_name in ["V1的名字", "V2的名字"] {
        let v = projects.new_version(p.id).await.expect("建版本");
        let snap = Snapshot {
            version_id: v.id,
            seq: v.seq,
            extracted_by: None,
            entity_types: vec![EntityType {
                key: "t_dev".into(),
                display_name: display_name.into(),
                description: None,
                aliases: vec![],
                parent: None,
                disjoint_with: vec![],
                properties: vec![],
                evidence: vec![],
            }],
            link_types: vec![],
            action_types: vec![],
            functions: vec![],
            concept_groups: vec![],
        };
        PgStore::new(pool.clone())
            .save_snapshot(&snap)
            .await
            .expect("写版本快照");
    }
    // 冻结**最新**那一版 —— 缺口就在这里：`latest_version` 不过滤 frozen_at，
    // 所以第 2 轮取到的基版本恰好是这一版，而它在冻结名单上。
    let latest = projects.latest_version(p.id).await.unwrap().expect("有版本");
    projects.freeze(latest.id).await.expect("冻结最新版本");
}

// ──────────────────────────────────────────────────────────
// 判据一：基版本已冻结 ⇒ 第 2 轮当场失败，且**不留提案文件**
// ──────────────────────────────────────────────────────────

/// ★ 修复前：第 2 轮**照常跑完**（提案照常产出、base_version=2），
/// 直到 `ontok-propose import` 才硬失败；修复后：在载入基版本时就停。
///
/// 断言「不留产物」是因为——跑了一半留下的 `proposal.json` 会让人
/// 以为这轮成功了，隔一天才发现它 import 不进去。
#[tokio::test]
async fn test_round_two_fails_early_when_latest_version_is_frozen() {
    let project = common::unique_project("req020frozen");
    seed_two_versions_with_latest_frozen(&project).await;
    let dir = workspace("frozen");

    let (ok, _so, se) = run_cli(&dir, &project, &["--round", "2"]);
    assert!(!ok, "基版本已冻结时第 2 轮必须失败，不许跑完再让人自己去 import 时才发现");
    assert!(
        !dir.join("proposal.json").exists(),
        "被拒绝时不许留下提案文件（那会让人以为跑成功了）"
    );
    assert!(
        !dir.join("stats.json").exists(),
        "被拒绝时不许留下统计文件（同上：看起来像跑过了）"
    );
    assert!(
        se.contains("冻结"),
        "错误信息要点破「冻结」，实际 stderr：{se}"
    );
    assert!(
        se.contains("v2") && se.contains(&project),
        "错误信息要指名**哪一版**（v2）与**哪个 project**，否则用户得自己去翻版本列表，实际 stderr：{se}"
    );
}

/// 错误信息必须说人话：为什么不能在其上提案 + 怎么办。
///
/// 「为什么」不给，用户会当成偶发错误重试；「怎么办」不给，用户只能去
/// 猜或者干脆放弃这一轮。这里把三件要说的都钉住。
#[tokio::test]
async fn test_frozen_base_error_explains_why_and_how_to_proceed() {
    let project = common::unique_project("req020frozenmsg");
    seed_two_versions_with_latest_frozen(&project).await;
    let dir = workspace("frozen-msg");

    let (ok, _so, se) = run_cli(&dir, &project, &["--round", "2"]);
    assert!(!ok, "应当失败，实际成功跑完了");

    // ① 为什么：冻结版不接受变更 / 是判据
    assert!(
        se.contains("不能") && (se.contains("判据") || se.contains("不接受变更")),
        "要说清**为什么**不能在冻结版上提案，实际 stderr：{se}"
    );
    // ② 怎么办：给出可执行的下一步（开新版本 / 先确认是否误冻）
    assert!(
        se.contains("version new"),
        "必须给出可执行的下一步（开一版新的当基线），实际 stderr：{se}"
    );
    assert!(
        se.contains("version list"),
        "要给出查冻结时刻的办法，好判断是不是误冻，实际 stderr：{se}"
    );
    // ③ 不许悄悄改语义：不得替用户改去查上一版（那会让「取最新」悄悄变形）
    assert!(
        !se.contains("v1") || se.contains("之前"),
        "本步刻意不改「取最新」的语义、不悄悄回落上一版；信息里出现 v1 只允许是「你想要的基线也许是 v1 之前那一版」这类说明，实际 stderr：{se}"
    );
}

/// **绝不退回空清单**：冻结**不许**被当成「没有基版本」而放行空清单跑过。
///
/// 退回空清单正是 REQ-015 的老病根 —— key 双向核对的三种违规
/// （`ExistingKeyDeclaredAsNew` / `ExistingDisplayNameChanged` /
/// 经 constraints 的 `ForbiddenTerm`）会重新变成**静默不可达**。
/// 阴性对照见下面：同一个 project，第 1 轮根本不比对基版本，所以不该报冻结。
#[tokio::test]
async fn test_frozen_base_does_not_silently_proceed_with_empty_types() {
    let project = common::unique_project("req020frozenempty");
    seed_two_versions_with_latest_frozen(&project).await;
    let dir = workspace("frozen-empty");

    let (ok, so, se) = run_cli(&dir, &project, &["--round", "2"]);
    assert!(!ok, "第 2 轮必须失败 —— 退回空清单就等于让三种违规静默不可达");
    assert!(
        !se.contains("已有类型 0") && !so.contains("已有类型 0"),
        "不许出现「已有类型 0 个」这种看起来跑过的迹象（那是退回空清单的形状），\
         stdout：{so}\nstderr：{se}"
    );
}

// ──────────────────────────────────────────────────────────
// 判据二：**别把没冻结的情形一起改了**
// ──────────────────────────────────────────────────────────

/// 阴性对照：最新版本**没**冻结 ⇒ 第 2 轮照常跑通。
///
/// 这条守住「提前校验不是无条件早退」—— 否则修冻结缺口就把
/// 正常轮次一起毙掉了。
#[tokio::test]
async fn test_round_two_still_runs_when_latest_version_is_not_frozen() {
    let project = common::unique_project("req020unfrozen");
    let dir = workspace("unfrozen");

    // 造一版（不冻结）
    {
        let pool = common::test_pool().await;
        common::migrate(&pool).await;
        let projects = PgProjectStore::new(pool.clone());
        let p = common::new_project(&projects, &project).await;
        let v = projects.new_version(p.id).await.expect("建版本");
        let snap = Snapshot {
            version_id: v.id,
            seq: v.seq,
            extracted_by: None,
            entity_types: vec![EntityType {
                key: "t_dev".into(),
                display_name: "V1的名字".into(),
                description: None,
                aliases: vec![],
                parent: None,
                disjoint_with: vec![],
                properties: vec![],
                evidence: vec![],
            }],
            link_types: vec![],
            action_types: vec![],
            functions: vec![],
            concept_groups: vec![],
        };
        PgStore::new(pool).save_snapshot(&snap).await.expect("写快照");
    }

    let (ok, so, se) = run_cli(&dir, &project, &["--round", "2"]);
    assert!(ok, "基版本未冻结时第 2 轮应当照常跑通，stderr：{se}");
    assert!(
        so.contains("已有类型") && so.contains("1 个"),
        "未冻结时清单照常从基版本载入，实际 stdout：{so}"
    );
    assert!(
        !se.contains("冻结"),
        "未冻结就不该报冻结（否则这条判据区分不了冻结/未冻结），实际 stderr：{se}"
    );
}

/// 第 1 轮不取基版本 ⇒ 冻结与否都**不**该拦它。
///
/// 第 1 轮压根不比对基版本（`load_existing_types` 在 `round <= 1` 直接返回），
/// 所以这条守住「提前校验没有漏进第 1 轮」。
#[tokio::test]
async fn test_round_one_is_unaffected_by_a_frozen_version() {
    let project = common::unique_project("req020r1frozen");
    seed_two_versions_with_latest_frozen(&project).await;
    let dir = workspace("r1-frozen");

    let (ok, _so, se) = run_cli(&dir, &project, &["--round", "1"]);
    assert!(ok, "第 1 轮不比对基版本，不该被冻结拦住，stderr：{se}");
    assert!(!se.contains("冻结"), "第 1 轮不该提冻结，实际 stderr：{se}");
}
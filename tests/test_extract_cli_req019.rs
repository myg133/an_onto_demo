//! REQ-019 · 生产入口（`ontok-propose`）的四项判据
//!
//! | 判据 | 为什么要有 |
//! | --- | --- |
//! | 第 2 轮从**基版本**载入「已有类型清单」 | REQ-014 的 key 双向核对在生产上一次都没跑过：清单恒空 ⇒ `ExistingKeyDeclaredAsNew` / `ExistingDisplayNameChanged` /（经 constraints 的）`ForbiddenTerm` 三种违规不可达 |
//! | `--attempts` 真的可配 | REQ-015 把 attempts 钉死 1，规格 §7.1 设计点⑤「重试 1–2 次」在生产上从未兑现 |
//! | stdout 措辞不再误导 | 「阶段1/2 成功 91 · 失败 0」读起来像「阶段 2 核过了」，而规则实现下那个 0 是**自证**，结构上不可能非 0 |
//! | **`base_version` = 实际比对的那一版** | 提案声明 v1、清单却从 v2 载入 ⇒ 阶段 1 拿 v2 验、apply 的 diff 挂在 v1 上，§8.4 的版本锁定归因**无声地**指错版本 |
//!
//! # 关于本文件跑 CLI
//!
//! 走真实二进制（`CARGO_BIN_EXE_ontok-propose`），临时目录一律落在
//! `.review-workspace/req019/` 下 —— 共享本体库只 INSERT，不删不清。

mod common;

/// 测试库不可用时的统一说明：**可能是没设，也可能是指向了非专用库**（防污染闸会拒）。
const NEED_TEST_DB: &str = "ONTOK_TEST_DATABASE_URL 不可用：没设，或指向了非专用测试库（库名需含 test/scratch/tmp/ci/dev）";

use ontok::db::project::PgProjectStore;
use ontok::db::PgStore;
use ontok::domain::snapshot::{EntityType, Snapshot};

const NAMES_JSON: &str = r#"{
  "agent": "table-name-translator",
  "version": "2026.10.07",
  "generated_at": "2026-10-07T10:00:00Z",
  "source": "req019-test",
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
        .join("req019")
        .join(format!("{}-{}", tag, std::process::id()));
    std::fs::create_dir_all(&dir).expect("建临时目录失败");
    dir
}

fn write_inputs(dir: &std::path::Path) -> (String, String) {
    let sp = dir.join("schema.json");
    let np = dir.join("names.json");
    std::fs::write(&sp, schema_json()).unwrap();
    std::fs::write(&np, NAMES_JSON).unwrap();
    (sp.to_str().unwrap().to_string(), np.to_str().unwrap().to_string())
}

/// 跑一次 CLI。`extra` 里可带 `--round` / `--attempts` / `--project`。
fn run_cli(dir: &std::path::Path, project: &str, extra: &[&str]) -> (bool, String, String) {
    let (sp, np) = write_inputs(dir);
    let out = dir.join("proposal.json");
    let stats = dir.join("stats.json");
    let mut cmd = propose_bin();
    cmd.args([
        "--schema",
        &sp,
        "--names",
        &np,
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
        ontok::config::test_database_url().expect(NEED_TEST_DB),
    );
    let o = cmd.output().expect("跑 ontok-propose 失败");
    (
        o.status.success(),
        String::from_utf8_lossy(&o.stdout).to_string(),
        String::from_utf8_lossy(&o.stderr).to_string(),
    )
}

// ──────────────────────────────────────────────────────────
// 判据一：第 2 轮载入基版本的已有类型
// ──────────────────────────────────────────────────────────

/// 造一个「上一轮已落库」的基版本：`t_dev` 的显示名是**设备台账**，
/// 而这一轮规则抽出来的是**设备** —— 改名，按设计点④合法但必须看得见。
async fn seed_base_version(project: &str) {
    let pool = common::test_pool().await;
    common::migrate(&pool).await;
    let projects = PgProjectStore::new(pool.clone());
    let p = common::new_project(&projects, project).await;
    let v = projects.new_version(p.id).await.expect("建基版本");
    let snap = Snapshot {
        version_id: v.id,
        seq: v.seq,
        extracted_by: None,
        entity_types: vec![EntityType {
            key: "t_dev".into(),
            display_name: "设备台账".into(),
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
    PgStore::new(pool)
        .save_snapshot(&snap)
        .await
        .expect("写基版本快照");
}

/// ★ 第 2 轮：key 被改显示名会被检出；第 1 轮（同一份库）不会。
///
/// 这是 REQ-014 的 key 双向核对在生产上的**第一次**真跑：
/// REQ-015 之前 `existing_types` 恒空，所以这两种违规在生产上不可达。
#[tokio::test]
async fn test_round_two_loads_existing_types_and_detects_display_name_change() {
    let project = common::unique_project("req019r2");
    seed_base_version(&project).await;
    let dir = workspace("round2");

    // 第 2 轮：载入基版本 ⇒ 检出改名
    let (ok, so, se) = run_cli(&dir, &project, &["--round", "2"]);
    assert!(ok, "第 2 轮应当跑通，stderr：{se}");
    assert!(
        so.contains("已有类型") && so.contains("1 个"),
        "第 2 轮必须把载入的已有类型**报出来**（不静默），实际 stdout：{so}"
    );
    assert!(
        se.contains("ExistingDisplayNameChanged") && se.contains("t_dev"),
        "第 2 轮 key 被改显示名必须被检出，实际 stderr：{se}"
    );
    assert!(
        se.contains("设备台账"),
        "违规里必须回显基版本的已知显示名，让人能判断是「有意改名」还是「抽取漂了」，实际：{se}"
    );
}

/// 阴性对照：**第 1 轮**不载入清单 ⇒ 同一个 key 不会被报成改名。
///
/// 两轮跑的是同一份 schema、同一个 project，差别只有 `--round` ——
/// 这正是「清单有没有被载入」的可判别差异。
#[tokio::test]
async fn test_round_one_does_not_load_existing_types() {
    let project = common::unique_project("req019r1");
    seed_base_version(&project).await;
    let dir = workspace("round1");

    let (ok, _so, se) = run_cli(&dir, &project, &["--round", "1"]);
    assert!(ok, "第 1 轮应当跑通，stderr：{se}");
    assert!(
        !se.contains("ExistingDisplayNameChanged"),
        "第 1 轮没有基版本可比，不该报改名（否则这条判据区分不了两轮），实际 stderr：{se}"
    );
}

/// 第 2 轮却**没有**基版本 ⇒ 明确报错，不许退回空清单假装跑过了。
///
/// 退回空清单就是让三种违规**静默不可达**的老路（隐患 3 的病根）。
#[tokio::test]
async fn test_round_two_without_base_version_fails_closed() {
    let project = common::unique_project("req019empty");
    let dir = workspace("round2-empty");

    let (ok, _so, se) = run_cli(&dir, &project, &["--round", "2"]);
    assert!(!ok, "第 2 轮没有基版本必须失败，不许静默用空清单跑过");
    assert!(
        se.contains(project.as_str()) && (se.contains("基版本") || se.contains("已有类型清单")),
        "错误信息要说清缺的是哪个 project 的基版本、为什么需要它，实际 stderr：{se}"
    );
    assert!(
        !dir.join("proposal.json").exists(),
        "被拒绝时不许留下提案文件（那会让人以为跑成功了）"
    );
}

// ──────────────────────────────────────────────────────────
// 判据二：--attempts 真的可配
// ──────────────────────────────────────────────────────────

/// `--attempts` 可传，且真的走通（规则实现首次即成功，行为不变）。
#[test]
fn test_attempts_flag_is_accepted_and_default_is_used_when_absent() {
    let project = common::unique_project("req019att");
    let dir = workspace("attempts");

    let (ok, _so, se) = run_cli(&dir, &project, &["--attempts", "5"]);
    assert!(ok, "--attempts 5 应当跑通，stderr：{se}");

    let (ok, _so, se) = run_cli(&dir, &project, &[]);
    assert!(ok, "不给 --attempts 走默认值，也应跑通，stderr：{se}");
}

/// `--attempts 0` 直接报错 —— 「重试 0 次」等于「根本没抽过却报成功」，
/// 设计点⑤明令禁止（`drive_group` 里也会夹到 ≥1，但 CLI 不该收这个值）。
#[test]
fn test_attempts_zero_is_rejected() {
    let project = common::unique_project("req019att0");
    let dir = workspace("attempts0");
    let (ok, _so, se) = run_cli(&dir, &project, &["--attempts", "0"]);
    assert!(!ok, "--attempts 0 必须被拒");
    assert!(se.contains("--attempts"), "错误信息要指名参数，实际 stderr：{se}");
}

// ──────────────────────────────────────────────────────────
// 判据三：stdout 措辞不再误导
// ──────────────────────────────────────────────────────────

/// 阶段 1/2 那一行**不能**读成「阶段 2 核过了、没问题」。
#[test]
fn test_cli_wording_does_not_claim_stage_two_passed() {
    let project = common::unique_project("req019wording");
    let dir = workspace("wording");
    let (ok, so, se) = run_cli(&dir, &project, &[]);
    assert!(ok, "CLI 应当跑通，stderr：{se}");

    assert!(
        so.contains("规则实现跑通"),
        "分组那一行要点明这是「规则实现跑通」，不是「核对通过」，实际 stdout：{so}"
    );
    assert!(
        so.contains("自证"),
        "必须点破违规 0 条是自证（声明与 items 同源），实际 stdout：{so}"
    );
    assert!(
        !so.contains("（成功 "),
        "旧的「（成功 N · 失败 M）」措辞会让人读成阶段 2 通过了，必须去掉，实际 stdout：{so}"
    );
}

// ──────────────────────────────────────────────────────────
// 判据四：`base_version` 反映**实际比对**的那一版（版本归因）
// ──────────────────────────────────────────────────────────

/// 造一个**两版**的 project：v1 与 v2 的 `t_dev` 显示名故意不同，
/// 于是「这一轮到底比对了哪一版」从违规回显的「已知显示名」上一眼可辨。
///
/// 返回 `(v1 的 id, v2 的 id)`。v2 是**最新**版 ⇒ 载入清单应当取它。
async fn seed_two_versions(project: &str) -> (uuid::Uuid, uuid::Uuid) {
    let pool = common::test_pool().await;
    common::migrate(&pool).await;
    let projects = PgProjectStore::new(pool.clone());
    let p = common::new_project(&projects, project).await;

    let mut ids = Vec::new();
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
        ids.push(v.id);
    }
    (ids[0], ids[1])
}

/// ★ 提案声明的基版本、阶段 1 实际比对的版本、apply 会取的 diff 基线
/// —— **三者必须是同一版**。
///
/// 修之前的形状：`existing_types` 从 v2 载入，而提案里 `base_version` 写死 1。
/// 于是同一次运行里出现「基版本 v2」（载入那行）与「基版本 v1」（提案那行）
/// 两个互相打架的数字，而 apply 照着 1 去取 diff 快照 —— 归因指错版本，
/// 而且**没有任何一处报错**。这条判据就是把那处无声错挂变成会红的断言。
#[tokio::test]
async fn test_base_version_matches_the_version_actually_compared_against() {
    let project = common::unique_project("req019bv");
    let (_v1, v2) = seed_two_versions(&project).await;
    let dir = workspace("basever");

    let (ok, so, se) = run_cli(&dir, &project, &["--round", "2"]);
    assert!(ok, "第 2 轮应当跑通，stderr：{se}");

    // ── ① 阶段 1 实际比对的是 v2（从违规回显的「已知显示名」反推）──
    assert!(
        se.contains("ExistingDisplayNameChanged") && se.contains("V2的名字"),
        "清单应当从**最新**的 v2 载入，违规里回显的已知显示名必须是 v2 的，实际 stderr：{se}"
    );
    assert!(
        !se.contains("V1的名字"),
        "若回显的是 v1 的名字，说明比对对象与「取最新」语义不符，实际 stderr：{se}"
    );

    // ── ② 提案声明的 base_version 等于那一版 ──
    let proposal: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("proposal.json")).unwrap()).unwrap();
    let declared = proposal["base_version"].as_u64().expect("提案缺 base_version");
    assert_eq!(
        declared, 2,
        "提案必须声明**实际比对的那一版**（v2），而不是硬编码的 1 —— \
         声明错版会让 apply 的 diff 基线与阶段 1 的核对对象分叉，产物：{proposal}"
    );

    // ── ③ apply 的 diff 基线由这个号解析出来 ⇒ 必须落在同一版上 ──
    // `src/cli.rs` 的 import 就是 `version_by_seq(doc["base_version"]) -> base_version_id`，
    // `apply` 再拿 `base_version_id` 取快照。这里把那条链走一遍，
    // 断言它解析到的正是阶段 1 载入清单时用的那个 version id。
    let pool = common::test_pool().await;
    let projects = PgProjectStore::new(pool);
    let p = projects.get_by_name(&project).await.unwrap().expect("project 应存在");
    let resolved = projects
        .version_by_seq(p.id, &project, declared as i32)
        .await
        .expect("声明的基版本号必须解析得到，否则 import 会直接失败");
    assert_eq!(
        resolved.id, v2,
        "声明的 base_version 解析出来的版本，必须与载入清单用的是**同一个** version id"
    );

    // ── ④ stdout 不得同屏出现两个互相打架的基版本号 ──
    assert!(
        !so.contains("基版本 v1"),
        "同屏出现「基版本 v2」与「基版本 v1」正是这次修的那个矛盾，实际 stdout：{so}"
    );
    assert!(
        so.contains("基版本 v2"),
        "两处基版本都应当打印同一个 v2，实际 stdout：{so}"
    );
}

/// 阴性对照：第 1 轮（无基版本）提案里仍然是 `base_version: 1`。
///
/// 这条守住「别把第 1 轮一起改了」——第 1 轮没有基版本可比，
/// 声明 1 才是如实的，也正是 REQ-002 契约的既有取值。
#[tokio::test]
async fn test_round_one_still_declares_base_version_one() {
    let project = common::unique_project("req019bv1");
    seed_two_versions(&project).await; // 库里有 v1/v2，但第 1 轮不比对
    let dir = workspace("basever-r1");

    let (ok, _so, se) = run_cli(&dir, &project, &["--round", "1"]);
    assert!(ok, "第 1 轮应当跑通，stderr：{se}");

    let proposal: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("proposal.json")).unwrap()).unwrap();
    assert_eq!(
        proposal["base_version"].as_u64(),
        Some(1),
        "第 1 轮没有基版本可比，base_version 必须仍为 1（本 REQ 不改第 1 轮行为），产物：{proposal}"
    );
}

/// REQ-152：构建面命令已并入 **`ontok-builder`**（子命令 `propose`）。
///
/// 参数、产物与退出码一个字节都没变 —— 只是二进制名字变了、多一层子命令。
/// 返回 `Command` 而不是 `&str`: 调用点大多是 `Command::new(..).arg(..)` 的链，
/// 换成字符串就得在每个调用点各插一行 `.arg("propose")`（漏一个就红得莫名其妙）。
fn propose_bin() -> std::process::Command {
    let mut c = std::process::Command::new(env!("CARGO_BIN_EXE_ontok-builder"));
    c.arg("propose");
    c
}

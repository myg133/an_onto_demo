//! REQ-022 · ADR-003 决策 (b) · `extracted_by` 的**来源通道**留痕
//!
//! # 这个文件钉住什么（以及为什么它必须走**真二进制**）
//!
//! | 判据 | 为什么要有 |
//! | --- | --- |
//! | `ontok propose import` 写进去的溯源值带 `source = declared` | ADR-003 实测：这条通道从 JSON 原样取值、零校验，编造的 `agent/version/failed_groups` 能一路进库。系统**无法**证明声明是真的（ADR-003 D2），那就至少**留痕** |
//> | 提案侧的 `source`（**勘查留痕字符串**）**不许**被当成来源通道 | 两个 `source` 字段同名、不同义（见 `extract::propose::ProposalProvenance` 的文档）。整块反序列化会在类型碰巧兼容时**静默错位** |
//> | `failed_groups` 的**对象**形状能被导进来 | 否则「工具自己产的提案导不进去、只有手写的能导」—— 一个没人明说的反向激励 |
//> | **`concept import` 那条通道**同样标 `declared` | 复审的变异自证发现：把 `cli.rs` 里 concept 通道的 `Declared` 换成 `ToolProduced`（冒充自产），本文件**全绿** —— 两条通道里有一条零覆盖。`snapshot.rs` 声称「两条都标 declared」，那就**两条都钉住** |
//!
//! 必须走真二进制（`CARGO_BIN_EXE_ontok`）：这条留痕发生在 `src/cli.rs`
//! 的 import 通道里，用 store 直接构造 `ExtractedBy` 的测试**碰不到**它。

mod common;

use common::{test_pool, unique_project};
use ontok::db::project::PgProjectStore;
use sqlx::Row;

/// 仓库里那份**自带正确指纹**的概念声明 —— `concept import` 是 fail-closed 的
/// （指纹对不上就拒），所以测试不能随手编一份。
const DECL_JSON: &str = include_str!("../src/domain/concept_groups.transition.json");

/// 建一个带 v1 版本的 project —— `propose import` 要挂在某版上
/// （ADR-001 的版本锁定：提案必有一个基版本）。
///
/// ⚠️ 只 INSERT：共享本体库上不 DELETE / 不 TRUNCATE（会打断别人的运行），
/// 隔离靠唯一 project 名（`unique_project`）。
async fn project_with_a_base_version() -> String {
    let pool = test_pool().await;
    let ps = PgProjectStore::new(pool.clone());
    ps.migrate().await.unwrap();
    let name = unique_project("provsrc");
    let p = common::new_project(&ps, &name).await;
    ps.new_version(p.id).await.unwrap();
    name
}

/// 库里那一份 `extracted_by` 的原始 JSON（**不信程序的自述**，直接取列）。
///
/// 纪律同 QA-B 的取证方式：判定读的是**库里的原始值**。
async fn stored_extracted_by(project: &str) -> Option<serde_json::Value> {
    let pool = test_pool().await;
    sqlx::query(
        "SELECT pr.extracted_by FROM proposals pr \
         JOIN projects p ON p.id = pr.project_id \
         WHERE p.name = $1 ORDER BY pr.created_at DESC LIMIT 1",
    )
    .bind(project)
    .fetch_one(&pool)
    .await
    .expect("该 project 下应有提案行")
    .try_get::<Option<serde_json::Value>, _>("extracted_by")
    .expect("extracted_by 列可读成 JSON")
}

/// ★ 提案侧 `source` 是**勘查留痕字符串**，快照侧 `source` 是**来源通道** ——
/// 两者同名不同义。整块反序列化会把前者当成后者（一次静默的语义错位）。
#[tokio::test]
async fn test_import_marks_extracted_by_as_declared_not_as_introspect_provenance() {
    let project = project_with_a_base_version().await;
    let dir = std::path::Path::new("..")
        .join(".review-workspace")
        .join("req022")
        .join("import")
        .join(format!("{}-{}", std::process::id(), line!()));
    std::fs::create_dir_all(&dir).expect("建临时目录失败");
    let file = dir.join("proposal.json");

    // 这份 JSON 的 `extracted_by` 是**本工具产出的形状**：
    //   - `source` = 勘查留痕（字符串，`extract_with_context` 写的那句）
    //   - `failed_groups` = 结构化对象数组（`GroupFailure` 的序列化）
    std::fs::write(
        &file,
        serde_json::to_string_pretty(&serde_json::json!({
            "project": project,
            "base_version": 1,
            "round": 1,
            "extracted_by": {
                "agent": "mcode-agent",
                "version": "2026.10.07",
                "round": 1,
                "failed_groups": [
                    {"group_id": "table:t_dev", "table": "t_dev", "attempts": 2, "reason": "超时"}
                ],
                "source": "introspect 2026-10-05T04:25:53Z schema=public read_only=on"
            },
            "items": [
                {
                    "kind": "add_entity_type",
                    "target": "设备",
                    "payload": {"key": "t_dev", "display_name": "设备", "properties": []}
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_ontok-builder"))
        .args(["import", "--file"])
        .arg(&file)
        .env(
            "ONTOK_DATABASE_URL",
            ontok::config::test_database_url().expect("缺 ONTOK_TEST_DATABASE_URL"),
        )
        .output()
        .expect("跑 ontok propose import 失败");
    assert!(
        out.status.success(),
        "import 失败：{}",
        String::from_utf8_lossy(&out.stderr)
    );

    // 列非 NULL（导入的提案带着一份声明，不是人工修订）
    let stored = stored_extracted_by(&project)
        .await
        .expect("extracted_by 列非空");

    // ① 来源通道 = 外部声明（ADR-003 (b)）
    assert_eq!(
        stored["source"],
        serde_json::json!("declared"),
        "从外部 JSON 读进来的溯源值必须标成 declared（系统未验证其真伪）"
    );

    // ② **勘查留痕不许被当成来源通道** —— 这是本测试的核心防错位断言
    assert!(
        !stored["source"].as_str().unwrap_or_default().starts_with("introspect"),
        "提案侧的 `source` 是**勘查留痕**（introspect …），不是来源通道 —— \
         两者同名不同义，写进去就是一次静默的语义错位"
    );

    // ③ 结构化 failed_groups 被显式取 group_id（不是照抄对象）
    assert_eq!(
        stored["failed_groups"],
        serde_json::json!(["table:t_dev"]),
        "failed_groups 的对象形状取 group_id —— ADR-003 点名的危险字段必须**可核对**"
    );

    // ④ 原有字段逐个保真（这次改动不许动取值口径）
    assert_eq!(stored["agent"], serde_json::json!("mcode-agent"));
    assert_eq!(stored["version"], serde_json::json!("2026.10.07"));
    assert_eq!(stored["round"], serde_json::json!(1));
}

/// ★ **`concept import` 这条通道也必须标 `declared`**（REQ-022 复审补的洞）。
///
/// 为什么补：`src/domain/snapshot.rs` 的来源通道表声称「`cli.rs` 的**两条**
/// import 通道都写 `Declared`」，而变异自证发现**这条通道当时零覆盖** ——
/// 把 `cli.rs` 里 concept 通道的 `Declared` 改成 `ToolProduced`（冒充自产），
/// 整个测试文件**全绿**。一份**没有测试守护的文档断言**正是本项目要消灭的东西，
/// 所以两条通道现在都被钉住。
#[tokio::test]
async fn test_concept_import_also_marks_extracted_by_as_declared() {
    let project = project_with_a_base_version().await;
    let dir = std::path::Path::new("..")
        .join(".review-workspace")
        .join("req022")
        .join("import-concept")
        .join(format!("{}-{}", std::process::id(), line!()));
    std::fs::create_dir_all(&dir).expect("建临时目录失败");
    // 仓库里那份声明**自带正确指纹**（`test_concept_transition` 钉住了
    // `verify() == Ok(())`），而 concept 通道是 fail-closed 的 ——
    // 指纹对不上就拒，所以这里不能随手编一份。
    let file = dir.join("declaration.json");
    std::fs::write(&file, DECL_JSON).unwrap();

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_ontok-builder"))
        .args(["concept", "import", "--file"])
        .arg(&file)
        .args(["--project", &project])
        .env(
            "ONTOK_DATABASE_URL",
            ontok::config::test_database_url().expect("缺 ONTOK_TEST_DATABASE_URL"),
        )
        .output()
        .expect("跑 ontok concept import 失败");
    assert!(
        out.status.success(),
        "concept import 失败：{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stored = stored_extracted_by(&project)
        .await
        .expect("extracted_by 列非空");
    assert_eq!(
        stored["source"],
        serde_json::json!("declared"),
        "概念声明同样是从**外部文件**读进来的（系统未验证其真伪）⇒ 必须标 declared，\
         绝不许标成 tool_produced（那是拿「在工具里跑」冒充「自己产的」）。实际：{stored}"
    );
}

/// 没有 `extracted_by` 的提案 = 人工修订（ADR-001），**不许**被补一个来源通道。
#[tokio::test]
async fn test_proposal_without_extracted_by_stays_null() {
    let project = project_with_a_base_version().await;
    let dir = std::path::Path::new("..")
        .join(".review-workspace")
        .join("req022")
        .join("import-none")
        .join(format!("{}-{}", std::process::id(), line!()));
    std::fs::create_dir_all(&dir).expect("建临时目录失败");
    let file = dir.join("proposal.json");
    std::fs::write(
        &file,
        serde_json::to_string_pretty(&serde_json::json!({
            "project": project,
            "base_version": 1,
            "round": 1,
            "items": [
                {
                    "kind": "add_entity_type",
                    "target": "设备",
                    "payload": {"key": "t_dev", "display_name": "设备", "properties": []}
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_ontok-builder"))
        .args(["import", "--file"])
        .arg(&file)
        .env(
            "ONTOK_DATABASE_URL",
            ontok::config::test_database_url().expect("缺 ONTOK_TEST_DATABASE_URL"),
        )
        .output()
        .expect("跑 ontok propose import 失败");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

    let stored = stored_extracted_by(&project).await;
    assert!(
        stored.is_none(),
        "没有溯源声明 = 人工修订（ADR-001：记 null，不许推断、不许补来源通道），实际：{stored:?}"
    );
}

/// 旧数据（`source` 字段不存在）⇒ `None`，**不猜**成 `tool_produced`。
///
/// 猜成「自产」等于凭空给一份来路不明的声明盖上「这是我们自己跑的」章 ——
/// 而这正是 ADR-003 要消灭的那类假。
#[test]
fn test_missing_source_field_stays_none_instead_of_guessing_tool_produced() {
    let eb: ontok::domain::snapshot::ExtractedBy = serde_json::from_value(serde_json::json!({
        "agent": "old-agent",
        "version": "0.9",
        "round": 1,
        "failed_groups": []
    }))
    .expect("旧形状（无 source）必须仍能反序列化 —— 不许把历史数据判死");
    assert_eq!(eb.source, None, "缺字段 = 不知道来源，不许往好的方向猜");
    assert!(!eb.is_declared(), "None 不等于 declared —— 那会把「不知道」说成「外部声明」");
    assert!(eb.fully_succeeded(), "failed_groups 为空 = 全部成功（ADR-001 的口径不许被这次改动动到）");
}

/// `tool_produced` 今天是**为将来预留**的值（全仓无写入方，REQ-022 复审实测）。
///
/// 保留它是为了**反序列化仍然 fail-closed**：将来真有自产路径时，
/// 「`source` 不在这两个值之内」必须在反序列化时就失败，而不是被读成别的。
/// 所以这个预留值的**读语义**必须钉住 —— 钉不住的话，它就只是一个占位符。
#[test]
fn test_reserved_tool_produced_is_not_declared_but_is_still_distinguishable_from_none() {
    use ontok::domain::snapshot::ProvenanceSource;

    let eb = ontok::domain::snapshot::ExtractedBy {
        source: Some(ProvenanceSource::ToolProduced),
        ..Default::default()
    };
    assert!(
        !eb.is_declared(),
        "自产**不是**外部声明 —— 判别力全在这一个 bool 上"
    );

    // 预留值必须**能被解码**，且与 `None`（不知道）**可区分** ——
    // 若它解析不了，将来那条自产路径就只能去写 `Declared`，那才是把自产说成外部声明。
    let round: ontok::domain::snapshot::ExtractedBy = serde_json::from_value(serde_json::json!({
        "agent": "a", "version": "1", "round": 1, "failed_groups": [],
        "source": "tool_produced"
    }))
    .expect("预留值必须能被反序列化读到（否则它挡不住将来的路径）");
    assert_eq!(round.source, Some(ProvenanceSource::ToolProduced));
    assert!(!round.is_declared());
    assert_ne!(round.source, None, "自产与「不知道」必须可区分");

    // 枚举的 fail-closed：不是这两个值之一 ⇒ **反序列化就失败**
    let bad = serde_json::from_value::<ontok::domain::snapshot::ExtractedBy>(serde_json::json!({
        "agent": "a", "version": "1", "round": 1, "failed_groups": [],
        "source": "trusted"          // 自由字符串 —— 正是本类型要挡的那种值
    }));
    assert!(
        bad.is_err(),
        "自由字符串必须被反序列化拒绝（枚举的全部价值就是「不是这两个之一」当场失败）"
    );
}
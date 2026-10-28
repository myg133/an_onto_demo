//! REQ-113 · 排除表（**产品能力**，不是脚本预处理）—— AC-001 ~ AC-010 的判据。
//!
//! # 这一层测的是什么（与 `src/extract/exclusions.rs` 的单元测试分工）
//!
//! 单元测试钉的是**判据**（拒绝空理由 / 类别枚举 / sha256 / 剔除 / 闭合断言）。
//! 这里钉的是**产品路径**：
//!
//! | # | 判据 | 怎么打 |
//! | --- | --- | --- |
//! | AC-001 | 被排除的表**不产生任何实体/属性/关系** | 真跑 `ontok-propose --exclude`，在 `proposal.json` 里回查表名 |
//! | AC-002 | `seen - excluded == extracted` | 读 `stats.json` 的三个数，并断言闭合 |
//! | AC-003 | 每条都有理由 + 决策人；缺了 ⇒ **拒绝文件** | 三个坏文件各跑一次，断言 exit≠0 且**没产出** |
//! | AC-004 | 溯源 `inputs.exclusions = {file, version, sha256}` | 读提案元信息；换一个字节 ⇒ sha 变 |
//! | AC-005 | 陈旧排除：默认警告 / `--strict-exclusions` ⇒ exit≠0 | 两种模式各跑一次 |
//! | AC-006 | 审核界面只读区块，条数 = `tables_excluded` | 真路由 `GET /api/review`（`Router::oneshot`） |
//! | AC-007 | 排除优先于「无中文名」 | 造一张**两者都占**的表，断言它只进 excluded |
//! | AC-008 | 确定性：两次运行逐字节相同 | 跑两次，比 `proposal.json` / `stats.json` 的**字节** |
//!
//! ⚠️ 本文件**不含任何真实客户表名**（REQ-084 的保留项纪律）：
//! 夹具是合成的（`keep_a` / `noise_x` …），真实库实测用仓库根那份 `exclusions.json`。

mod common;

/// 测试库不可用时的统一说明：**可能是没设，也可能是指向了非专用库**（防污染闸会拒）。
const NEED_TEST_DB: &str = "ONTOK_TEST_DATABASE_URL 不可用：没设，或指向了非专用测试库（库名需含 test/scratch/tmp/ci/dev）";

use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use common::test_pool;
use ontok::db::project::PgProjectStore;
use ontok::db::proposal::{NewItem, NewProposal, PgProposalStore};
use ontok::db::snapshot_store::PgStore;
use ontok::domain::review::ItemKind;
use ontok::domain::snapshot::Snapshot;
use ontok::review_ui::http::{build_router, prepare};
use ontok::review_ui::ReviewArgs;
use serde_json::{json, Value};
use tower::ServiceExt;

const TOKEN: &str = "correct-horse-battery";

// ══════════════════════════════════════════════════════════════
// 夹具：一份**合成**的小 schema + 排除文件
// ══════════════════════════════════════════════════════════════

/// 一个独立的工作目录（每次调用唯一），测试结束不清理（与本仓既有做法一致：
/// 临时产物是排障证据，清理它反而让「跑过没跑过」无据可查）。
fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ontok-req113-{}-{}-{}",
        tag,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).expect("建临时目录");
    dir
}

fn col(name: &str, comment: Option<&str>) -> Value {
    json!({
        "name": name,
        "data_type": "text",
        "nullable": true,
        "comment": comment,
        "is_primary_key": name == "id",
    })
}

fn table(name: &str, comment: Option<&str>, cols: &[&str]) -> Value {
    let columns: Vec<Value> = cols.iter().map(|c| col(c, None)).collect();
    let commented = columns.iter().filter(|c| c["comment"].is_string()).count();
    json!({
        "name": name,
        "comment": comment,
        "columns": columns,
        "column_count": cols.len(),
        "commented_count": commented,
    })
}

/// 5 张表的 schema：
/// - `keep_a` / `keep_b`：有表注释 ⇒ 拿得到中文名（保留）；
/// - `noise_x` / `noise_y`：**无**注释、也不在翻译表里 ⇒ 本来会进 `tables_without_cn_name`；
/// - `stays_nameless`：**无**中文名但**不在**排除文件里 ⇒ 必须留在 `tables_without_cn_name`。
fn write_schema(dir: &Path) -> PathBuf {
    let tables = vec![
        table("keep_a", Some("客户"), &["id", "customer_id"]),
        table("keep_b", Some("订单"), &["id"]),
        table("noise_x", None, &["id", "payload"]),
        table("noise_y", None, &["id", "payload"]),
        table("stays_nameless", None, &["id"]),
    ];
    let column_count: usize = tables.iter().map(|t| t["column_count"].as_u64().unwrap() as usize).sum();
    let commented: usize = tables.iter().map(|t| t["commented_count"].as_u64().unwrap() as usize).sum();
    let report = json!({
        "server_version": "PostgreSQL 16.14 (test fixture)",
        "transaction_read_only": "on",
        "schema": "public",
        "table_count": tables.len(),
        "column_count": column_count,
        "commented_column_count": commented,
        "foreign_key_count": 0,
        "tables": tables,
        "introspected_at": "2026-10-21T00:00:00Z",
    });
    let p = dir.join("schema.json");
    std::fs::write(&p, serde_json::to_string_pretty(&report).unwrap()).expect("写 schema");
    p
}

/// 一份**合规**的排除文件：排掉两张噪声表。
///
/// 刻意带 `keep_but_flagged`（一张**照常进本体**的表）——
/// 「排除」与「待裁决」必须分两栏记，合并就会把结论与问题混成一个列表。
fn write_exclusions(dir: &Path, name: &str, stale: Option<&str>, decided_by: &str) -> PathBuf {
    let mut exclude = vec![
        json!({
            "table": "noise_x",
            "category": "cdc_residue",
            "reason": "数据同步工具自动生成的临时表，不对应任何客户业务",
            "decided_by": decided_by,
            "confidence": 0.97,
            "evidence": "表名带哈希后缀；无表注释、无列注释",
        }),
        json!({
            "table": "noise_y",
            "category": "internal_pipeline",
            "reason": "内部消息投递簿记，客户不可见",
            "decided_by": decided_by,
        }),
    ];
    if let Some(t) = stale {
        exclude.push(json!({
            "table": t,
            "category": "backup_snapshot",
            "reason": "某次改动前的整表备份副本，业务只认原表",
            "decided_by": decided_by,
        }));
    }
    let doc = json!({
        "version": "0.1.1",
        "declared_by": "ai-table-judge / test",
        "reviewed_by": "qa-req113（复核）",
        "rule": "只排除「不是客户业务世界里的事物」的表",
        "exclude": exclude,
        "keep_but_flagged": [
            { "table": "keep_b", "why": "列语义不足以判断", "concern": "可能只是后台配置" }
        ],
        "keep_normal": ["keep_a", "stays_nameless"],
    });
    let p = dir.join(name);
    std::fs::write(&p, serde_json::to_string_pretty(&doc).unwrap()).expect("写排除文件");
    p
}

/// 翻译表：只给 `keep_a` / `keep_b` 之外的表留空，确保 `noise_*` 拿不到中文名。
///
/// 路径走 [`common::crate_root`]（**运行时**定位）而不是 `env!("CARGO_MANIFEST_DIR")`：
/// 后者是裸编译期锚点，`test_anchor_guard` 会（正确地）拦下它 ——
/// 同一份测试换了工作目录就不该读到另一份文件。
fn names_path() -> PathBuf {
    // 用仓库里那份真实翻译表：它的条目与合成表名不重叠 ⇒ 合成表全部走「注释 → 空」。
    common::crate_root().join("src/extract/table_names.json")
}

struct Run {
    status: Option<i32>,
    stdout: String,
    stderr: String,
    proposal: Option<Value>,
    stats: Option<Value>,
    proposal_bytes: Vec<u8>,
    stats_bytes: Vec<u8>,
    /// 提案旁边**除提案与 stats 之外**还多了什么（用来证明「没有侧车」）。
    siblings: Vec<String>,
}

/// 跑一次 `ontok-propose`。`extra` 追加到固定参数之后。
fn run_propose(dir: &Path, schema: &Path, exclusions: Option<&Path>, extra: &[&str], tag: &str) -> Run {
    let out = dir.join(format!("proposal-{tag}.json"));
    let stats = dir.join(format!("stats-{tag}.json"));
    let mut cmd = propose_bin();
    cmd.args([
        "--schema",
        schema.to_str().unwrap(),
        "--names",
        names_path().to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--stats",
        stats.to_str().unwrap(),
        "--project",
        "req113",
        // 合成 schema 没有键信息、也不连源库：显式关采样，免得打上「未采样」的噪声
        "--no-sample",
    ]);
    if let Some(ex) = exclusions {
        cmd.arg("--exclude").arg(ex);
    }
    cmd.args(extra);
    // 关掉一切可能从环境里漏进来的颜色/日志开关，保证 stderr 可判读
    cmd.env("NO_COLOR", "1");
    let o = cmd.output().expect("跑不动 ontok-propose");

    let read = |p: &Path| std::fs::read(p).ok();
    // 记下 `--out` 目录里**新出现**的文件：用来断言「没有侧车、只有 proposal + stats」
    // （pending / rejected 是 REQ-108 的既有产文件，不算新东西）。
    let siblings = std::fs::read_dir(dir)
        .map(|it| {
            it.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.starts_with(&format!("proposal-{tag}")) || n.starts_with(&format!("stats-{tag}")))
                .collect()
        })
        .unwrap_or_default();
    Run {
        status: o.status.code(),
        stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
        proposal: read(&out).and_then(|b| serde_json::from_slice(&b).ok()),
        stats: read(&stats).and_then(|b| serde_json::from_slice(&b).ok()),
        proposal_bytes: read(&out).unwrap_or_default(),
        stats_bytes: read(&stats).unwrap_or_default(),
        siblings,
    }
}

fn entity_keys(proposal: &Value) -> Vec<String> {
    proposal["items"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter(|i| i["kind"] == "add_entity_type")
                .filter_map(|i| i["payload"]["key"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

// ══════════════════════════════════════════════════════════════
// AC-001 / AC-002 / AC-007 · 剔除发生在抽取之前
// ══════════════════════════════════════════════════════════════

#[test]
fn test_excluded_tables_never_reach_the_proposal_and_counts_close() {
    let dir = scratch("basic");
    let schema = write_schema(&dir);
    let ex = write_exclusions(&dir, "exclusions.json", None, "ai-table-judge");
    let r = run_propose(&dir, &schema, Some(&ex), &[], "basic");

    assert_eq!(r.status, Some(0), "正常输入必须成功：\nSTDOUT:\n{}\nSTDERR:\n{}", r.stdout, r.stderr);

    let proposal = r.proposal.expect("有提案");
    let stats = r.stats.expect("有统计");

    // ── AC-001（精确化）· 被排除的表只允许出现在 `inputs.exclusions` 段内 ──
    //
    // ① `items` 段里 grep = 0（「不建实体」的真正判据）；
    // ② 把 `inputs.exclusions` 整段删掉后，剩余文本里 grep = 0。
    let text = String::from_utf8_lossy(&r.proposal_bytes).into_owned();
    let items_text = serde_json::to_string(&proposal["items"]).unwrap();
    for gone in ["noise_x", "noise_y"] {
        assert!(
            !items_text.contains(gone),
            "AC-001 ①：被排除的表 {gone:?} 不得出现在 items 里（实体/属性/关系/证据都不许）"
        );
    }
    // ② 用「序列化后的提案」做替换：把整段 inputs 拿掉，剩下的就是「除审计段之外的所有字节」
    let mut stripped = proposal.clone();
    stripped["inputs"] = Value::Null;
    let stripped_text = serde_json::to_string(&stripped).unwrap();
    for gone in ["noise_x", "noise_y"] {
        assert!(
            !stripped_text.contains(gone),
            "AC-001 ②：删掉 `inputs.exclusions` 后，被排除表名 {gone:?} 仍出现 ⇒ 审计段之外还有痕迹"
        );
    }
    assert!(
        text.contains("noise_x"),
        "反向确认这条判据不是恒真：审计段里**应当**有这个表名（它是元信息，不是实体）"
    );
    // 反向：审计段必须真的有它（否则上面 ② 的「删掉后为 0」是空话）
    assert_eq!(proposal["inputs"]["exclusions"]["excluded"][0]["table"], "noise_x");
    // 「没有侧车」：`--out` 目录里只有 proposal + stats（外加 REQ-108 的 pending/rejected）
    for s in &r.siblings {
        assert!(
            !s.contains("exclusions"),
            "⛔ BA 裁定：明细进提案、取消侧车 —— `--out` 旁边不许再出现 {s:?}"
        );
    }

    // 三计数闭合（AC-002），且口径是「勘查 5 − 排除 2 = 抽取 3」
    assert_eq!(stats["tables_seen"], 5, "tables_seen 是**排除之前**的勘查表数：{stats}");
    assert_eq!(stats["tables_excluded"], 2, "{stats}");
    assert_eq!(stats["tables_extracted"], 3, "{stats}");
    assert_eq!(
        stats["tables_seen"].as_u64().unwrap() - stats["tables_excluded"].as_u64().unwrap(),
        stats["tables_extracted"].as_u64().unwrap(),
        "AC-002：seen - excluded == extracted 必须成立"
    );
    // 逐条记录：表名 / 类别 / 理由 / 决策人（AC-003 的「可读」在数据面）
    let excluded = stats["excluded_tables"].as_array().expect("有逐条记录");
    assert_eq!(excluded.len(), 2);
    assert_eq!(excluded[0]["table"], "noise_x");
    assert_eq!(excluded[0]["category"], "cdc_residue");
    assert!(!excluded[0]["reason"].as_str().unwrap().is_empty());
    assert_eq!(excluded[0]["decided_by"], "ai-table-judge");
    assert_eq!(excluded[0]["confidence"], 0.97, "AI 判的置信度要带进统计");

    // 实体数 == tables_extracted（AC-001 的第二半）
    let keys = entity_keys(&proposal);
    assert_eq!(keys.len(), 3, "AC-001：实体数必须等于 tables_extracted，实际 {keys:?}");
    assert!(!keys.iter().any(|k| k == "noise_x" || k == "noise_y"));

    // AC-007 · 与「无中文名」**分开记**，且排除优先。
    //
    // `noise_x` / `noise_y` 本来也没有中文名（无注释、不在翻译表里）。
    // 它们被排除 ⇒ **只**进 excluded，绝不许再进 `tables_without_cn_name`。
    let nameless: Vec<String> = stats["tables_without_cn_name"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect();
    assert!(
        nameless.iter().any(|t| t == "stays_nameless"),
        "没被排除、又确实没中文名的表仍要记进 without_cn_name：{nameless:?}"
    );
    for gone in ["noise_x", "noise_y"] {
        assert!(
            !nameless.iter().any(|t| t == gone),
            "AC-007：{gone:?} 既被排除又没中文名 ⇒ **只进 excluded**，不进 without_cn_name（实际：{nameless:?}）"
        );
    }

    // 「待裁决」单独一栏：它**没有被排除**，照常进本体（keep_b 在实体里）
    assert_eq!(stats["flagged_tables"].as_array().unwrap().len(), 1);
    assert_eq!(stats["flagged_tables"][0]["table"], "keep_b");
    assert!(keys.iter().any(|k| k == "keep_b"), "待裁决的表必须照常建实体");
}

// ══════════════════════════════════════════════════════════════
// AC-004 · 溯源
// ══════════════════════════════════════════════════════════════

#[test]
fn test_provenance_is_the_full_audit_block_in_one_file() {
    let dir = scratch("prov");
    let schema = write_schema(&dir);
    let ex = write_exclusions(&dir, "exclusions.json", None, "ai-table-judge");
    let expected_sha = ontok::extract::sample::sha256_hex(&std::fs::read(&ex).unwrap());
    let r = run_propose(&dir, &schema, Some(&ex), &[], "prov");
    assert_eq!(r.status, Some(0), "{}", r.stderr);

    let proposal = r.proposal.expect("有提案");
    let stats = r.stats.clone().expect("有统计");
    let block = &proposal["inputs"]["exclusions"];
    assert!(block.is_object(), "AC-004：提案元信息必须有 inputs.exclusions，实际：{proposal}");

    // 元信息（AC-004 的三个必需键都在）
    assert_eq!(block["file"], ex.to_string_lossy().as_ref(), "file 记的是调用方给的路径");
    assert_eq!(block["version"], "0.1.1");
    assert_eq!(block["sha256"], expected_sha, "sha256 必须是**文件原始字节**的哈希");
    assert_eq!(block["declared_by"], "ai-table-judge / test");
    assert_eq!(block["reviewed_by"], "qa-req113（复核）", "「谁判的」与「谁签的」分开记");

    // 审计段自带三计数（与 stats.json 同源）
    assert_eq!(block["counts"]["seen"], 5);
    assert_eq!(block["counts"]["excluded"], 2);
    assert_eq!(block["counts"]["extracted"], 3);
    assert_eq!(block["counts"]["flagged"], 1);
    assert_eq!(
        block["counts"].as_object().unwrap()["seen"].as_u64().unwrap()
            - block["counts"]["excluded"].as_u64().unwrap(),
        block["counts"]["extracted"].as_u64().unwrap(),
        "审计段里那三个数也必须闭合"
    );
    assert_eq!(block["counts"]["excluded"], stats["tables_excluded"]);
    assert_eq!(block["counts"]["extracted"], stats["tables_extracted"]);

    // 逐条明细**就在提案里**（BA 裁定：单一文件，无侧车）
    let excluded = block["excluded"].as_array().expect("审计段带逐条明细");
    assert_eq!(excluded.len(), 2);
    assert_eq!(excluded[0]["table"], "noise_x");
    assert_eq!(excluded[0]["category"], "cdc_residue");
    assert_eq!(excluded[0]["decided_by"], "ai-table-judge");
    assert_eq!(excluded[0]["confidence"], 0.97);
    assert!(excluded[0]["evidence"].as_str().unwrap().contains("哈希"));
    // 「保留但存疑」是另一栏，且**不在** excluded 里
    let flagged = block["flagged"].as_array().expect("审计段带待裁决清单");
    assert_eq!(flagged.len(), 1);
    assert_eq!(flagged[0]["table"], "keep_b");
    assert_eq!(block["stale"].as_array().unwrap().len(), 0);

    // 只有两个产文件：提案 + stats（没有侧车）
    assert_eq!(r.siblings.len(), 2, "只许 proposal 与 stats 两个文件，实际：{:?}", r.siblings);
}

// ══════════════════════════════════════════════════════════════
// AC-004 + 「单一文件」· `propose import` 只读一份提案就能落库
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_single_file_import_persists_the_audit_block() {
    use ontok::db::project::PgProjectStore;
    use ontok::db::proposal::PgProposalStore;

    let dir = scratch("import");
    let schema = write_schema(&dir);
    let ex = write_exclusions(&dir, "exclusions.json", None, "ai-table-judge");
    let r = run_propose(&dir, &schema, Some(&ex), &[], "import");
    assert_eq!(r.status, Some(0), "{}", r.stderr);
    let proposal_path = dir.join("proposal-import.json");
    assert!(proposal_path.exists());

    // 提案里的 `project` 是 `ontok-propose --project req113`（本文件统一用它）：
    // 这里确保它存在且有 v1，再由 CLI 走**只读一个 --file** 的导入路径。
    let url = ontok::config::test_database_url().expect(NEED_TEST_DB);
    let pool = common::test_pool().await;
    common::migrate(&pool).await;
    let projects = PgProjectStore::new(pool.clone());
    let p = projects.get_or_create("req113").await.unwrap();
    if projects.list_versions(p.id).await.unwrap().is_empty() {
        projects.new_version(p.id).await.unwrap();
    }

    // ⭐ 只给一个 `--file`：没有侧车、没有第二个参数
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_ontok-builder"))
        .args(["import", "--file", proposal_path.to_str().unwrap()])
        .env("ONTOK_DATABASE_URL", &url)
        .env("NO_COLOR", "1")
        .output()
        .expect("跑不动 ontok");
    assert!(
        o.status.success(),
        "单文件导入必须成功：\nSTDOUT:\n{}\nSTDERR:\n{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    let stdout = String::from_utf8_lossy(&o.stdout);
    assert!(stdout.contains("排除记录"), "回执要说清落了几条排除：{stdout}");

    // 库里那一份就是完整审计段（审核界面读它）。
    // 项目 `req113` 是多次运行共用的（本文件里所有 CLI 用例都写它），
    // 所以取**最新**那条提案 —— 旧的没有 inputs，不该被读到。
    let proposals = PgProposalStore::new(pool.clone());
    let rows: Vec<(uuid::Uuid,)> = sqlx::query_as(
        "SELECT id FROM proposals WHERE project_id = $1 ORDER BY created_at DESC, id DESC LIMIT 1",
    )
    .bind(p.id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(rows.len(), 1, "至少导入过一条提案");
    let inputs = proposals.inputs(rows[0].0).await.unwrap().expect("inputs 落库了");
    let block = &inputs["exclusions"];
    assert_eq!(block["counts"]["excluded"], 2);
    assert_eq!(block["excluded"].as_array().unwrap().len(), 2);
    assert_eq!(block["excluded"][0]["reason"], "数据同步工具自动生成的临时表，不对应任何客户业务");
    assert_eq!(block["flagged"].as_array().unwrap().len(), 1);
    assert_eq!(block["reviewed_by"], "qa-req113（复核）");
    // 落库的这份能被审核界面那条路径读回来（同一套校验）
    let rec = ontok::extract::exclusions::Record::from_inputs(&inputs)
        .unwrap()
        .expect("库里有记录");
    assert_eq!(rec.count(), 2);
    assert!(rec.counts.close());
}

#[test]
fn test_one_byte_change_changes_the_recorded_sha256() {
    let dir = scratch("sha");
    let schema = write_schema(&dir);
    let a = write_exclusions(&dir, "a.json", None, "ai-table-judge");
    // 改**一个字符**（理由里的一个字），其余一字不动
    let text = std::fs::read_to_string(&a).unwrap().replace("临时表", "临时 表");
    let b = dir.join("b.json");
    std::fs::write(&b, text).unwrap();

    let ra = run_propose(&dir, &schema, Some(&a), &[], "sha-a");
    let rb = run_propose(&dir, &schema, Some(&b), &[], "sha-b");
    assert_eq!((ra.status, rb.status), (Some(0), Some(0)));

    let sa = ra.proposal.unwrap()["inputs"]["exclusions"]["sha256"].as_str().unwrap().to_string();
    let sb = rb.proposal.unwrap()["inputs"]["exclusions"]["sha256"].as_str().unwrap().to_string();
    assert_ne!(sa, sb, "AC-004：改一个字节 ⇒ sha256 必须变");
    assert_eq!(sa.len(), 64);
}

// ══════════════════════════════════════════════════════════════
// AC-003 · 校验失败 ⇒ 拒绝文件（exit≠0，且**不产出**）
// ══════════════════════════════════════════════════════════════

fn bad_file(dir: &Path, name: &str, entry: Value) -> PathBuf {
    let doc = json!({
        "version": "0.1.1",
        "declared_by": "ai-table-judge / test",
        "rule": "测试用",
        "exclude": [entry],
    });
    let p = dir.join(name);
    std::fs::write(&p, serde_json::to_string_pretty(&doc).unwrap()).unwrap();
    p
}

#[test]
fn test_empty_reason_is_rejected_and_nothing_is_written() {
    let dir = scratch("bad-reason");
    let schema = write_schema(&dir);
    let ex = bad_file(
        &dir,
        "bad.json",
        json!({"table":"noise_x","category":"cdc_residue","reason":"   ","decided_by":"BA"}),
    );
    let r = run_propose(&dir, &schema, Some(&ex), &[], "bad-reason");
    assert_ne!(r.status, Some(0), "AC-003：理由为空 ⇒ exit≠0");
    assert!(r.stderr.contains("reason"), "报错要点名字段：{}", r.stderr);
    assert!(r.proposal.is_none(), "拒绝文件时**不产出提案**");
    assert!(r.stats.is_none(), "拒绝文件时**不产出统计**");
}

#[test]
fn test_unknown_category_is_rejected() {
    let dir = scratch("bad-cat");
    let schema = write_schema(&dir);
    let ex = bad_file(
        &dir,
        "bad.json",
        json!({"table":"noise_x","category":"技术原因","reason":"这是一条足够长的理由","decided_by":"BA"}),
    );
    let r = run_propose(&dir, &schema, Some(&ex), &[], "bad-cat");
    assert_ne!(r.status, Some(0), "AC-003：类别不在枚举内 ⇒ exit≠0");
    assert!(r.stderr.contains("cdc_residue"), "报错要列全合法类别：{}", r.stderr);
    assert!(r.proposal.is_none());
}

#[test]
fn test_missing_decided_by_is_rejected() {
    let dir = scratch("bad-owner");
    let schema = write_schema(&dir);
    let ex = bad_file(
        &dir,
        "bad.json",
        json!({"table":"noise_x","category":"cdc_residue","reason":"这是一条足够长的理由"}),
    );
    let r = run_propose(&dir, &schema, Some(&ex), &[], "bad-owner");
    assert_ne!(r.status, Some(0), "AC-003：缺决策人 ⇒ exit≠0");
    assert!(r.stderr.contains("decided_by"), "{}", r.stderr);
    assert!(r.proposal.is_none());
}

// ══════════════════════════════════════════════════════════════
// AC-005 · 陈旧排除两种模式
// ══════════════════════════════════════════════════════════════

#[test]
fn test_stale_exclusion_warns_by_default_but_fails_under_strict() {
    let dir = scratch("stale");
    let schema = write_schema(&dir);
    let ex = write_exclusions(&dir, "exclusions.json", Some("gone_table_xyz"), "ai-table-judge");

    // ① 默认：警告 + 计数，**exit 0**（R4：库结构会变，硬失败会把老文件逼出命令行）
    let soft = run_propose(&dir, &schema, Some(&ex), &[], "stale-soft");
    assert_eq!(soft.status, Some(0), "默认模式不许失败：{}", soft.stderr);
    assert!(
        soft.stderr.contains("gone_table_xyz"),
        "AC-005：陈旧排除必须**点名**警告，实际 stderr：{}",
        soft.stderr
    );
    let stats = soft.stats.expect("有统计");
    assert_eq!(stats["stale_exclusions"], json!(["gone_table_xyz"]), "陈旧清单进 stats");
    assert_eq!(stats["tables_excluded"], 2, "陈旧的**不算**排除（它本来就不在 schema 里）");
    assert_eq!(stats["tables_seen"].as_u64().unwrap() - stats["tables_excluded"].as_u64().unwrap(), 3);

    // ② --strict-exclusions：当场 exit≠0，且点名哪几条陈旧、不产出
    let strict = run_propose(&dir, &schema, Some(&ex), &["--strict-exclusions"], "stale-strict");
    assert_ne!(strict.status, Some(0), "AC-005：--strict-exclusions ⇒ exit≠0");
    assert!(strict.stderr.contains("gone_table_xyz"), "要点名：{}", strict.stderr);
    assert!(strict.stderr.contains("陈旧"), "{}", strict.stderr);
    assert!(strict.proposal.is_none(), "严格模式失败时**不产出**提案");
    assert!(strict.stats.is_none());
}

#[test]
fn test_strict_without_exclude_is_rejected_as_meaningless() {
    let dir = scratch("strict-alone");
    let schema = write_schema(&dir);
    let r = run_propose(&dir, &schema, None, &["--strict-exclusions"], "strict-alone");
    assert_ne!(r.status, Some(0), "没有 --exclude 时 --strict-exclusions 没有意义，应当拒绝");
    assert!(r.stderr.contains("--strict-exclusions"), "{}", r.stderr);
}

// ══════════════════════════════════════════════════════════════
// AC-008 · 确定性（两次运行逐字节相同）
// ══════════════════════════════════════════════════════════════

#[test]
fn test_two_runs_are_byte_identical() {
    let dir = scratch("determinism");
    let schema = write_schema(&dir);
    let ex = write_exclusions(&dir, "exclusions.json", None, "ai-table-judge");

    let a = run_propose(&dir, &schema, Some(&ex), &[], "det-a");
    let b = run_propose(&dir, &schema, Some(&ex), &[], "det-b");
    assert_eq!((a.status, b.status), (Some(0), Some(0)));

    assert_eq!(
        a.proposal_bytes, b.proposal_bytes,
        "AC-008：同一份 schema + 同一份 exclusions ⇒ proposal.json 必须逐字节相同"
    );
    assert_eq!(
        a.stats_bytes, b.stats_bytes,
        "AC-008：stats.json 也必须逐字节相同"
    );
    assert!(!a.proposal_bytes.is_empty());
}

// ══════════════════════════════════════════════════════════════
// AC-006 · 审核界面：只读区块，条数 = tables_excluded
// ══════════════════════════════════════════════════════════════

/// 一份「与真实产出一致」的 inputs 记录（键名与 `exclusions::Record` 对齐）。
///
/// `seen = excluded + 2`（固定 2 张真的抽出实体）：审计段自带的三计数必须闭合，
/// 否则 `Record::from_inputs` 会（正确地）拒绝它 —— 这也是本测试想钉住的。
fn record_with(excluded: usize, flagged: usize) -> Value {
    let tables: Vec<Value> = (0..excluded)
        .map(|i| {
            json!({
                "table": format!("noise_{i}"),
                "category": "cdc_residue",
                "reason": "数据同步工具自动生成的临时表，不对应任何客户业务",
                "decided_by": "ai-table-judge",
                "confidence": 0.9,
            })
        })
        .collect();
    let flagged_list: Vec<Value> = (0..flagged)
        .map(|i| {
            json!({
                "table": format!("kept_{i}"),
                "why": "列语义不足以判断",
                "concern": "若只是后台账号则与 admin_users 重复",
            })
        })
        .collect();
    json!({
        "exclusions": {
            "file": "exclusions.json",
            "version": "0.1.1",
            "sha256": "a".repeat(64),
            "declared_by": "ai-table-judge / test",
            "reviewed_by": "qa-req113（复核）",
            "rule": "只排除不是客户业务世界里的事物",
            "counts": {
                "seen": excluded + 2,
                "excluded": excluded,
                "extracted": 2,
                "stale": 1,
                "flagged": flagged,
            },
            "excluded": tables,
            "stale": ["gone_table_xyz"],
            "flagged": flagged_list,
        }
    })
}

async fn review_fixture(prefix: &str, inputs: Option<Value>) -> (axum::Router, String) {
    let pool = test_pool().await;
    common::migrate(&pool).await;
    let projects = PgProjectStore::new(pool.clone());
    let store = PgStore::new(pool.clone());
    let proposals = PgProposalStore::new(pool.clone());

    let p = common::fresh_project(&projects, prefix).await;
    let v1 = projects.new_version(p.id).await.unwrap();
    store.save_snapshot(&Snapshot::new(v1.id, 1)).await.unwrap();
    let prop = proposals
        .create(
            p.id,
            NewProposal { base_version_id: v1.id, round: 1, extracted_by: None },
            &[NewItem {
                kind: ItemKind::AddEntityType,
                target_key: "客户".to_string(),
                payload: json!({"key": "customer", "display_name": "客户", "properties": []}),
            }],
        )
        .await
        .unwrap();
    if let Some(inputs) = inputs {
        proposals.set_inputs(prop.id, &inputs).await.unwrap();
    }
    let args = ReviewArgs {
        project: p.name.clone(),
        proposal: prop.id.to_string(),
        bind: "127.0.0.1:0".parse().unwrap(),
        token: TOKEN.to_string(),
    };
    let state = prepare(&pool, &args).await.expect("审核界面启动闸门");
    (build_router(state), p.name)
}

async fn get_review(router: axum::Router) -> (StatusCode, Value) {
    let resp = router
        .oneshot(
            Request::builder()
                .uri("/api/review")
                .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 22).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

#[tokio::test]
async fn test_review_api_exposes_exclusions_readonly_and_separates_flagged() {
    let (router, _) = review_fixture("excl", Some(record_with(3, 2))).await;
    let (status, body) = get_review(router).await;
    assert_eq!(status, StatusCode::OK, "正文：{body}");

    let x = &body["exclusions"];
    assert_eq!(x["present"], true, "AC-006：被排除的表必须在审核界面可见：{body}");
    assert_eq!(x["readonly"], true, "⛔ 只读：改排除要改文件重跑");
    assert_eq!(x["count"], 3, "AC-006：区块条数 = tables_excluded");
    assert_eq!(x["tables"].as_array().unwrap().len(), 3);
    // 表名 + 类别 + 理由 + 决策人（AC-006 的四栏）+ 置信度（AI 判的那一份才有）
    let first = &x["tables"][0];
    assert_eq!(first["table"], "noise_0");
    assert_eq!(first["category"], "cdc_residue");
    assert!(first["reason"].as_str().unwrap().contains("临时表"));
    assert_eq!(first["decided_by"], "ai-table-judge");
    assert_eq!(first["confidence"], 0.9);
    assert_eq!(x["sha256"].as_str().unwrap().len(), 64);
    assert_eq!(x["reviewed_by"], "qa-req113（复核）");
    assert_eq!(x["stale"], json!(["gone_table_xyz"]));

    // ⛔ 「排除」与「待裁决」**两栏分开**：待裁决的不在 tables 里
    assert_eq!(x["flagged_count"], 2);
    let tables: Vec<&str> = x["tables"].as_array().unwrap().iter().map(|t| t["table"].as_str().unwrap()).collect();
    for f in x["flagged"].as_array().unwrap() {
        assert!(
            !tables.contains(&f["table"].as_str().unwrap()),
            "待裁决的表不许出现在「被排除」清单里：{f}"
        );
    }
    // 页面上「被排除 N 张」与「保留但存疑 M 张」是两块，不是一块
    assert_eq!(x["count"].as_u64().unwrap() + x["flagged_count"].as_u64().unwrap(), 5);

    // 页面壳本身仍然零数据（/ 免口令），且区块是 HTML 里一个只读 section
    let html = ontok::review_ui::page::REVIEW_HTML;
    assert!(html.contains(r#"id="excluded""#), "页面必须有「被排除」区块");
    assert!(html.contains(r#"id="flagged""#), "页面必须有「保留但存疑」区块");
    assert!(
        !html.contains("x-edit") && !html.contains("editable"),
        "⛔ 排除区块不许有编辑入口"
    );
}

#[tokio::test]
async fn test_review_api_says_why_there_is_no_exclusion_record() {
    // 人工修订 / REQ-113 之前的提案：没有 inputs ⇒ 不是「排除了 0 张」，是**不知道**
    let (router, _) = review_fixture("exclnone", None).await;
    let (status, body) = get_review(router).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["exclusions"]["present"], false);
    assert!(
        body["exclusions"]["reason"].as_str().unwrap().contains("没有输入记录"),
        "三种「没有记录」要分开说：{}",
        body["exclusions"]["reason"]
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

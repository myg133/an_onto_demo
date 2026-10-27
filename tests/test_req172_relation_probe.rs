//! ⭐ REQ-172 · **表间关系探索**的真进程 + 真库判据（假 ACP、真 SQL）。
//!
//! | # | 判据 | 测试 |
//! | --- | --- | --- |
//! | 1 | 值域 ⊆ 键域 ⇒ 候选带 `domain`；⊄ ⇒ 不带 | [`probe_verifies_domain_and_never_persists_sample_values`] |
//! | 2 | 只读：连接串强制 `default_transaction_read_only=on` ⇒ 写入必被拒 | 同上 |
//! | 3 | A 轮：成立出关系、不成立不出、无依据必丢 | [`judge_two_rounds_validate_and_drop`] |
//! | 4 | B 轮：孤立表能提出来，`source: ai` / `band: low` / 每表 ≤3 | 同上 + `build run` 链 |
//! | 5 | 校验层：谓词不在词表 / 列不存在 / 目标非键 各自丢弃并计数 | 同上 |
//! | 6 | `build run --dsn` ⇒ 实体覆盖全部表、关系落进草稿 | [`build_run_dsn_covers_every_table_and_lands_relations`] |
//!
//! ⚠️ 每条判据都用**真库**（`ONTOK_TEST_DATABASE_URL`）+ **真进程**（`ontok-builder`）；
//! 数据库一轮一建、跑完删（⚠️ 需要 CREATE DATABASE 权限；没有就**响亮跳过**）。
//! 模型是 `tests/fixtures/req172/fake-relate-acp.js`（真 ACP 子进程、假模型）。
//!
//! ⚠️ 本文件**不**用真 UAT 库：判据要能红，而真库的数据会变。真库实测记在 changelog。

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_ontok-builder")
}

fn crate_root() -> PathBuf {
    common::source_root::crate_root()
}

fn fixture(rel: &str) -> PathBuf {
    crate_root().join("tests/fixtures/req172").join(rel)
}

fn fake_acp() -> PathBuf {
    fixture("fake-relate-acp.js")
}

fn relate_skill() -> PathBuf {
    fixture("relate-skill.md")
}

fn doc_skill() -> PathBuf {
    crate_root().join("tests/fixtures/req147/skill-fixture.md")
}

fn node_ok() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn db_url() -> Option<String> {
    for k in ["ONTOK_TEST_DATABASE_URL", "ONTOK_DATABASE_URL"] {
        if let Ok(v) = std::env::var(k) {
            if !v.trim().is_empty() {
                return Some(v);
            }
        }
    }
    None
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "ontok-req172-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&d).expect("建临时目录");
    d
}

fn swap_db(url: &str, db: &str) -> String {
    let (base, query) = match url.split_once('?') {
        Some((b, q)) => (b, Some(q)),
        None => (url, None),
    };
    let head = base.rsplit_once('/').map(|(h, _)| h).unwrap_or(base);
    match query {
        Some(q) => format!("{head}/{db}?{q}"),
        None => format!("{head}/{db}"),
    }
}

fn read_json(path: &Path) -> Value {
    let text =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("读不到 {}：{e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{} 不是合法 JSON：{e}", path.display()))
}

/// 一轮独占一个库（避免上一轮残留的表影响「候选/关系」计数）。
struct TestDb {
    admin_url: String,
    url: String,
    name: String,
}

async fn create_db(tag: &str) -> Option<TestDb> {
    let base = db_url()?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let name = format!("req172_{tag}_{}_{stamp}", std::process::id());
    let admin = sqlx::PgPool::connect(&base).await.ok()?;
    let created = sqlx::query(&format!("CREATE DATABASE \"{name}\""))
        .execute(&admin)
        .await;
    admin.close().await;
    if created.is_err() {
        common::skip::missing(&format!("可建库的测试账号（CREATE DATABASE 被拒）"), tag);
        return None;
    }
    Some(TestDb {
        url: swap_db(&base, &name),
        admin_url: base,
        name,
    })
}

impl TestDb {
    async fn drop(self) {
        if let Ok(admin) = sqlx::PgPool::connect(&self.admin_url).await {
            let _ = sqlx::query(&format!(
                "DROP DATABASE IF EXISTS \"{}\" WITH (FORCE)",
                self.name
            ))
            .execute(&admin)
            .await;
            admin.close().await;
        }
    }
}

/// 建一套**能同时考出「值域对上 / 对不上 / 同名列 / 无候选」**的源表。
async fn build_tables(db: &TestDb, tag: &str) -> Vec<String> {
    let p = format!("r172_{tag}");
    let t = |s: &str| format!("{p}_{s}");
    let customers = t("customers");
    let balances = t("balances");
    let orders = t("orders");
    let region_dim = t("region_dim");
    let region_map = t("region_map");
    let payments = t("payments");
    let logs = t("logs");
    let tags = t("tags");
    let coupons = t("coupons");
    let pool = sqlx::PgPool::connect(&db.url).await.expect("连本轮库");
    let stmts = vec![
        format!("CREATE TABLE \"{customers}\" (id text PRIMARY KEY, name text)"),
        format!("COMMENT ON TABLE \"{customers}\" IS '客户'"),
        format!("CREATE TABLE \"{balances}\" (id text PRIMARY KEY, customer_id text)"),
        format!("COMMENT ON TABLE \"{balances}\" IS '客户余额'"),
        format!("COMMENT ON COLUMN \"{balances}\".customer_id IS '客户编号'"),
        format!("CREATE TABLE \"{orders}\" (id text PRIMARY KEY, customer_id text)"),
        format!("COMMENT ON TABLE \"{orders}\" IS '订单'"),
        format!("COMMENT ON COLUMN \"{orders}\".customer_id IS '客户编号'"),
        format!("CREATE TABLE \"{region_dim}\" (region_code text PRIMARY KEY, note text)"),
        format!("COMMENT ON TABLE \"{region_dim}\" IS '区域维度'"),
        format!("CREATE TABLE \"{region_map}\" (id text PRIMARY KEY, region_code text)"),
        format!("COMMENT ON TABLE \"{region_map}\" IS '区域映射'"),
        format!("CREATE TABLE \"{payments}\" (id text PRIMARY KEY, order_id text)"),
        format!("COMMENT ON TABLE \"{payments}\" IS '支付'"),
        format!("COMMENT ON COLUMN \"{payments}\".order_id IS '订单编号'"),
        format!("CREATE TABLE \"{logs}\" (id text PRIMARY KEY, customer_id text)"),
        format!("COMMENT ON TABLE \"{logs}\" IS '日志'"),
        format!("COMMENT ON COLUMN \"{logs}\".customer_id IS '用户ID'"),
        format!("CREATE TABLE \"{tags}\" (id text PRIMARY KEY, tag_id text)"),
        format!("COMMENT ON TABLE \"{tags}\" IS '标签'"),
        format!("CREATE TABLE \"{coupons}\" (id text PRIMARY KEY, batch_id text)"),
        format!("COMMENT ON TABLE \"{coupons}\" IS '优惠券'"),
        // 数据：customers.id 里放一个**只该存在于内存**的值，用来证明「抽样不落盘」
        format!(
            "INSERT INTO \"{customers}\" VALUES ('cust-secret-9f2a','客户一'),('c2','客户二'),('c3','客户三')"
        ),
        format!(
            "INSERT INTO \"{balances}\" VALUES ('b1','cust-secret-9f2a'),('b2','c2'),('b3','c3'),('b4','c2')"
        ),
        // 值域**对不上**：ghost-* 不在 customers.id
        format!("INSERT INTO \"{orders}\" VALUES ('o1','ghost-9'),('o2','ghost-8')"),
        format!("INSERT INTO \"{region_dim}\" VALUES ('cn','中国'),('us','美国')"),
        format!("INSERT INTO \"{region_map}\" VALUES ('m1','cn'),('m2','us')"),
        format!("INSERT INTO \"{payments}\" VALUES ('p1','o1')"),
        format!("INSERT INTO \"{logs}\" VALUES ('l1','cust-secret-9f2a')"),
        format!("INSERT INTO \"{tags}\" VALUES ('t1','unk')"),
        format!("INSERT INTO \"{coupons}\" VALUES ('c1','batch-1')"),
    ];
    for sql in stmts {
        sqlx::query(&sql)
            .execute(&pool)
            .await
            .unwrap_or_else(|e| panic!("建源表失败：{sql} —— {e}"));
    }
    pool.close().await;
    vec![
        customers, balances, orders, region_dim, region_map, payments, logs, tags, coupons,
    ]
}

async fn write_schema(db: &TestDb, path: &Path) {
    let (report, keys) = ontok::extract::introspect::introspect_with_keys(&db.url)
        .await
        .expect("勘查本轮库");
    let mut product = serde_json::to_value(&report).expect("SchemaReport 序列化");
    keys.inject_into_product(&mut product);
    std::fs::write(
        path,
        format!(
            "{}\n",
            serde_json::to_string_pretty(&product).expect("pretty")
        ),
    )
    .expect("写 schema.json");
}

/// 跑一条 `ontok-builder` 子命令。
fn run_bin(args: &[String], envs: &[(&str, &str)]) -> std::process::Output {
    let mut cmd = Command::new(bin());
    cmd.args(args);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    cmd.output().expect("跑不了 ontok-builder")
}

fn must_ok(out: &std::process::Output, what: &str) -> String {
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(
        out.status.success(),
        "{what} 应退出 0，实际 {:?}\nstderr:\n{stderr}",
        out.status.code()
    );
    // 这两个命令的【人话摘要】走 stdout（JSON 产物落文件），与 access validate 同一口径。
    String::from_utf8_lossy(&out.stdout).to_string()
}

// ══════════════════════════════════════════════════════════════
// 判据 1 + 2 · 值域验证 + 只读
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn probe_verifies_domain_and_never_persists_sample_values() {
    const NAME: &str = "probe_verifies_domain_and_never_persists_sample_values";
    if db_url().is_none() {
        common::skip::missing("ONTOK_TEST_DATABASE_URL", NAME);
        return;
    }
    let Some(db) = create_db("probe").await else {
        return;
    };
    let _ = build_tables(&db, "probe").await;
    let root = temp_dir("probe");
    let schema = root.join("schema.json");
    write_schema(&db, &schema).await;

    // ── 判据 2：只读是**连接串强制**的，写入必被拒 ──
    let ro = ontok::extract::sample::sample_pool(&db.url)
        .await
        .expect("只读池");
    let denied = sqlx::query("INSERT INTO \"r172_probe_customers\" VALUES ('x','x')")
        .execute(&ro)
        .await
        .expect_err("写入必须被拒（default_transaction_read_only=on）");
    let msg = format!("{denied}");
    assert!(
        msg.to_lowercase().contains("read-only") || msg.contains("25006"),
        "写入被拒的理由必须是只读事务，实际：{msg}"
    );
    ro.close().await;

    // ── 判据 1：probe 真进程 ──
    let out = root.join("relation-candidates.json");
    let probe_run = run_bin(
        &[
            "relation".into(),
            "probe".into(),
            "--schema".into(),
            schema.display().to_string(),
            "--dsn".into(),
            db.url.clone(),
            "--out".into(),
            out.display().to_string(),
            "--sample-rows".into(),
            "50".into(),
        ],
        &[],
    );
    let stdout = must_ok(&probe_run, "relation probe");
    assert!(stdout.contains("值域验证"), "屏幕要报值域验证：\n{stdout}");

    let text = std::fs::read_to_string(&out).expect("读候选文件");
    // 抽样不落盘：值域样本（那个唯一值）不许出现在产物里。
    assert!(
        !text.contains("cust-secret-9f2a"),
        "⛔ 样本值落进了产物 —— 值域验证只能落计数"
    );
    let doc: Value = serde_json::from_str(&text).expect("候选 JSON");
    assert_eq!(doc["kind"], "relation-candidates");
    assert_eq!(doc["source"]["read_only"], "on", "只读凭据要写进产物");
    assert!(
        !doc["source"]["dsn"].as_str().unwrap_or("").contains('@'),
        "产物里的 DSN 必须是脱敏后的 host/db：{}",
        doc["source"]["dsn"]
    );

    let find = |child: &str, col: &str, parent: &str, key: &str| -> Option<Value> {
        doc["candidates"]
            .as_array()?
            .iter()
            .find(|c| {
                c["child_table"].as_str().unwrap_or("").ends_with(child)
                    && c["child_column"] == col
                    && c["parent_table"].as_str().unwrap_or("").ends_with(parent)
                    && c["parent_column"] == key
            })
            .cloned()
    };

    // 值域 ⊆ 键域 ⇒ evidence 有 domain、score 升、band high
    let good = find("_balances", "customer_id", "_customers", "id")
        .expect("balances.customer_id → customers.id 应是候选");
    let ev = good["evidence"].as_array().unwrap();
    assert!(
        ev.iter().any(|e| e["source"] == "domain"),
        "值域对上的候选必须带 domain 证据：{good}"
    );
    assert_eq!(good["domain"]["subset"], true);
    assert!(good["score"].as_f64().unwrap() >= 0.9);
    assert_eq!(good["band"], "high");

    // 值域 ⊄ 键域 ⇒ 没有 domain 证据（但仍留命名候选，交给人/AI 判）
    let bad = find("_orders", "customer_id", "_customers", "id")
        .expect("orders.customer_id → customers.id 应是候选（命名）");
    let ev = bad["evidence"].as_array().unwrap();
    assert!(
        !ev.iter().any(|e| e["source"] == "domain"),
        "值域对不上不许出现 domain 证据：{bad}"
    );
    assert_eq!(bad["domain"]["tested"], true);
    assert_eq!(bad["domain"]["subset"], false);
    assert!(bad["score"].as_f64().unwrap() < 0.9);

    // 同名列：region_map.region_code → region_dim.region_code（父表是主键）
    let same =
        find("_region_map", "region_code", "_region_dim", "region_code").expect("同名列候选应存在");
    assert!(same["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["source"] == "samelenum"));

    // 无候选的列必须进未解析（⛔ 不静默）
    let unresolved = doc["unresolved"].as_array().unwrap();
    assert!(
        unresolved.iter().any(|u| {
            u["table"].as_str().unwrap_or("").ends_with("_coupons")
                && u["column"] == "batch_id"
                && u["code"] == "no_candidate"
        }),
        "batch_id 没有候选表，必须进 no_candidate：{}",
        doc["unresolved"]
    );

    db.drop().await;
}

// ══════════════════════════════════════════════════════════════
// 判据 3 + 4 + 5 · AI 两轮 + 校验层
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn judge_two_rounds_validate_and_drop() {
    const NAME: &str = "judge_two_rounds_validate_and_drop";
    if db_url().is_none() {
        common::skip::missing("ONTOK_TEST_DATABASE_URL", NAME);
        return;
    }
    if !node_ok() || !fake_acp().is_file() {
        common::skip::missing("node / 假 ACP 夹具", NAME);
        return;
    }
    let Some(db) = create_db("judge").await else {
        return;
    };
    let _ = build_tables(&db, "judge").await;
    let root = temp_dir("judge");
    let schema = root.join("schema.json");
    write_schema(&db, &schema).await;
    let cands = root.join("relation-candidates.json");
    must_ok(
        &run_bin(
            &[
                "relation".into(),
                "probe".into(),
                "--schema".into(),
                schema.display().to_string(),
                "--dsn".into(),
                db.url.clone(),
                "--out".into(),
                cands.display().to_string(),
            ],
            &[],
        ),
        "relation probe",
    );

    let log = root.join("calls.log");
    let rel_out = root.join("relations.json");
    let agent_cmd = format!("node {}", fake_acp().display());
    let envs = [
        ("FAKE_ACP_LOG", log.to_str().unwrap()),
        ("FAKE_ACP_MODE", "valid"),
    ];
    let stdout = must_ok(
        &run_bin(
            &[
                "relation".into(),
                "judge".into(),
                "--candidates".into(),
                cands.display().to_string(),
                "--dsn".into(),
                db.url.clone(),
                "--agent".into(),
                "acp".into(),
                "--agent-command".into(),
                agent_cmd.clone(),
                "--relate-skill".into(),
                relate_skill().display().to_string(),
                "--out".into(),
                rel_out.display().to_string(),
                "--agent-timeout-secs".into(),
                "60".into(),
            ],
            &envs,
        ),
        "relation judge",
    );
    assert!(
        stdout.contains("关系判定完成"),
        "屏幕要报判定完成：\n{stdout}"
    );
    let text = std::fs::read_to_string(&rel_out).expect("读判定文件");
    let doc: Value = serde_json::from_str(&text).expect("判定 JSON");
    assert_eq!(doc["kind"], "relation-judged");
    let a_batches = doc["round_a"]["batches"].as_u64().unwrap();
    let b_batches = doc["round_b"]["batches"].as_u64().unwrap();
    let calls = std::fs::read_to_string(&log)
        .map(|s| s.lines().count())
        .unwrap_or(0);
    assert_eq!(
        calls as u64,
        a_batches + b_batches + 1,
        "调用数 = A 批 + B 批 + 预热 1 次（实际 {calls}）"
    );

    // 判据 3：成立出关系；不成立不出；无依据被丢
    let rels = doc["relations"].as_array().unwrap();
    assert!(
        rels.iter().any(|r| r["child_table"]
            .as_str()
            .unwrap_or("")
            .ends_with("_balances")
            && r["parent_table"]
                .as_str()
                .unwrap_or("")
                .ends_with("_customers")),
        "A 轮「成立」的候选必须出关系：{}",
        doc["relations"]
    );
    assert!(
        !rels
            .iter()
            .any(|r| r["child_table"].as_str().unwrap_or("").ends_with("_logs")),
        "A 轮「不成立」的候选不许出关系：{}",
        doc["relations"]
    );
    let drops = doc["drops"].as_object().unwrap();
    for code in [
        "no_basis",
        "unknown_predicate",
        "basis_column_unknown",
        "nothing_held",
    ] {
        assert!(
            drops.get(code).and_then(|v| v.as_u64()).unwrap_or(0) >= 1,
            "判据 5/3：{code} 必须至少丢 1 条，实际 {}",
            doc["drops"]
        );
    }

    // 判据 4：B 轮补漏（source: ai / band: low / 每表 ≤3）
    let b_rels: Vec<&Value> = rels.iter().filter(|r| r["round"] == "b").collect();
    assert!(!b_rels.is_empty(), "B 轮必须提出关系：{}", doc["relations"]);
    for r in &b_rels {
        assert_eq!(r["source"], "ai");
        assert_eq!(r["band"], "low");
    }
    let mut per_table: std::collections::BTreeMap<&str, usize> = Default::default();
    for r in &b_rels {
        *per_table
            .entry(r["child_table"].as_str().unwrap_or(""))
            .or_insert(0) += 1;
    }
    assert!(
        per_table.values().all(|n| *n <= 3),
        "每表最多 3 条：{per_table:?}"
    );
    assert!(
        drops
            .get("too_many_per_table")
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
            >= 1,
        "超量的 B 轮关系必须被截断并计数：{}",
        doc["drops"]
    );
    assert!(
        drops
            .get("unknown_table")
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
            >= 1,
        "B 轮点了不存在的表必须被丢：{}",
        doc["drops"]
    );
    assert!(
        drops
            .get("unknown_column")
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
            >= 1,
        "B 轮点了不存在的列必须被丢：{}",
        doc["drops"]
    );

    // 缓存：同一输入第二次 0 调用、产物逐字节相同
    let before = std::fs::read(&rel_out).unwrap();
    let calls_before = std::fs::read_to_string(&log).unwrap().lines().count();
    must_ok(
        &run_bin(
            &[
                "relation".into(),
                "judge".into(),
                "--candidates".into(),
                cands.display().to_string(),
                "--dsn".into(),
                db.url.clone(),
                "--agent".into(),
                "acp".into(),
                "--agent-command".into(),
                agent_cmd.clone(),
                "--relate-skill".into(),
                relate_skill().display().to_string(),
                "--out".into(),
                rel_out.display().to_string(),
                "--agent-timeout-secs".into(),
                "60".into(),
            ],
            &envs,
        ),
        "relation judge（缓存重跑）",
    );
    let calls_after = std::fs::read_to_string(&log).unwrap().lines().count();
    assert_eq!(
        calls_after,
        calls_before,
        "全命中缓存的重跑不许再发 prompt（实际多了 {} 次）",
        calls_after - calls_before
    );
    // 第二次：0 token、全命中；关系本身与第一次相同（stats 里 cache_hits/tokens 必然变）。
    let after: Value = serde_json::from_str(&std::fs::read_to_string(&rel_out).unwrap()).unwrap();
    assert_eq!(
        after["round_a"]["tokens"], 0,
        "缓存重跑不许再花 token：{after}"
    );
    assert_eq!(
        after["round_b"]["tokens"], 0,
        "缓存重跑不许再花 token：{after}"
    );
    assert_eq!(
        after["round_a"]["cache_hits"].as_u64().unwrap_or(0),
        a_batches,
        "A 轮应全部命中缓存"
    );
    assert_eq!(
        after["round_b"]["cache_hits"].as_u64().unwrap_or(0),
        b_batches,
        "B 轮应全部命中缓存"
    );
    let before_doc: Value = serde_json::from_slice(&before).unwrap();
    assert_eq!(
        after["relations"], before_doc["relations"],
        "关系必须与第一次一致"
    );

    // 失败要响：全部无依据 ⇒ 非 0 且**不产产物**
    let bad_out = root.join("relations-nobasis.json");
    let failed = run_bin(
        &[
            "relation".into(),
            "judge".into(),
            "--candidates".into(),
            cands.display().to_string(),
            "--dsn".into(),
            db.url.clone(),
            "--agent".into(),
            "acp".into(),
            "--agent-command".into(),
            agent_cmd.clone(),
            "--relate-skill".into(),
            relate_skill().display().to_string(),
            "--out".into(),
            bad_out.display().to_string(),
            "--agent-timeout-secs".into(),
            "60".into(),
        ],
        &[("FAKE_ACP_MODE", "nobasis")],
    );
    assert!(
        !failed.status.success(),
        "全部无依据必须非 0（⛔ 不产空产物）"
    );
    assert!(!bad_out.exists(), "失败时不许留下 relations.json");

    db.drop().await;
}

// ══════════════════════════════════════════════════════════════
// 判据 6 · `build run --dsn` 一条链
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn build_run_dsn_covers_every_table_and_lands_relations() {
    const NAME: &str = "build_run_dsn_covers_every_table_and_lands_relations";
    if db_url().is_none() {
        common::skip::missing("ONTOK_TEST_DATABASE_URL", NAME);
        return;
    }
    if !node_ok() || !fake_acp().is_file() {
        common::skip::missing("node / 假 ACP 夹具", NAME);
        return;
    }
    let Some(db) = create_db("build").await else {
        return;
    };
    let tables = build_tables(&db, "build").await;
    let root = temp_dir("build");
    let docs = root.join("docs");
    std::fs::create_dir_all(&docs).expect("docs");
    std::fs::write(docs.join("a.md"), "# 客户\n\n客户余额属于客户。\n").expect("写文档");

    let o = ontok::review_open::build_run::Options {
        project: Some(format!("req172_{}", std::process::id())),
        input: docs,
        agent: "acp".into(),
        agent_command: format!("node {}", fake_acp().display()),
        agent_skill: doc_skill(),
        agent_timeout_secs: Some(60),
        strict_deterministic: false,
        work_dir: root.join("wd"),
        schema: None,
        dsn: Some(db.url.clone()),
        proposal: None,
        allow_doc_only: false,
        base_version: None,
        propose: None, // 自动：给了 --dsn 就开
        relate_skill: Some(relate_skill()),
        bind: "127.0.0.1:0".into(),
        no_browser: true,
    };
    let art = ontok::review_open::build_run::run_draft(&o)
        .await
        .expect("build run 的 ①②③ 应走通");

    for p in [
        &art.candidates,
        &art.relations,
        &art.proposed,
        &art.proposal_final,
    ] {
        assert!(p.is_file(), "缺少中间产物 {}（重跑诊断用）", p.display());
    }
    let draft = read_json(&art.draft);
    let items = draft["items"].as_array().expect("草稿要有 items");

    // 实体：schema 里的表全都覆盖（propose 打底的作用）
    let entity_keys: Vec<&str> = items
        .iter()
        .filter(|i| i["kind"] == "add_entity_type")
        .filter_map(|i| i["payload"]["key"].as_str())
        .collect();
    for t in &tables {
        assert!(
            entity_keys.iter().any(|k| k == t),
            "表 {t} 没有进草稿（实体覆盖不全：{}）",
            entity_keys.len()
        );
    }

    // 关系：与 `doc merge` 同一形状（band low / reviewed_by null / evidence.source）
    let links: Vec<&Value> = items
        .iter()
        .filter(|i| i["kind"] == "add_link_type")
        .collect();
    assert!(
        !links.is_empty(),
        "草稿里一条关系都没有：{}",
        draft["items"]
    );
    let ours: Vec<&&Value> = links
        .iter()
        .filter(|l| {
            l.pointer("/payload/evidence/0/source")
                .and_then(|v| v.as_str())
                .is_some()
        })
        .collect();
    assert!(
        !ours.is_empty(),
        "关系条目必须带 payload.evidence[0].source"
    );
    for l in &ours {
        assert_eq!(l["band"], "low");
        assert!(l["reviewed_by"].is_null());
        let src = l["payload"]["evidence"][0]["source"].as_str().unwrap();
        assert!(
            ["naming", "domain", "ai"].contains(&src),
            "source 只能是 naming|domain|ai，实际 {src}"
        );
        assert!(
            l["payload"]["cardinality"].as_str().is_some(),
            "关系必须带 cardinality"
        );
        assert!(
            ["true", "false"].contains(&l["payload"]["join_verified"].as_str().unwrap_or("")),
            "关系必须带 join_verified 三态（R2RML 靠它决定生不生成 joinCondition）：{l}"
        );
    }
    // balances.customer_id → customers.id 应带 domain（值域对上）
    assert!(
        ours.iter().any(|l| {
            l.pointer("/payload/evidence/0/source")
                .and_then(|v| v.as_str())
                == Some("domain")
                && l["payload"]["from_type"]
                    .as_str()
                    .unwrap_or("")
                    .ends_with("_balances")
        }),
        "值域对上的关系必须以 source=domain 落进草稿：{}",
        draft["items"]
    );
    // 草稿是评审台的输入，⛔ 不能自带签名
    assert!(
        items.iter().all(|i| i["reviewed_by"].is_null()),
        "草稿里不许有已签条目（人还没签）"
    );

    db.drop().await;
}

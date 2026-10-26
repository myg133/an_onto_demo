//! REQ-120 rev.4 · **G-1（阻断项）的进程级判据**：签字文件里的租户键 `inherit` 必须被拒。
//!
//! # 为什么这条判据必须是**真进程**
//!
//! G-1 的原话是「**这条路径不存在**」，而不是「我们的生成器不产它」。
//! 单元测试证明的是 `load_signed` 这个函数会拒；**真进程**证明的是
//! 「拿一份手改过的签字文件跑 `ontok-propose`，产物里没有那条边、退出码非 0」——
//! 那才是 QA 能复现的攻击面（签字文件是文本，谁都能改）。
//!
//! 三条判据：
//! 1. 租户键列的 `inherit` ⇒ **exit ≠ 0**，stderr 点名「哪条边 / 哪个列 / 为什么」；
//! 2. 同一次运行**不写产物**（`--out` 一个字节都不落）；
//! 3. **非租户列**的 `inherit` 照常通过（别误伤 `api_call_logs__licenses`）。

use std::path::{Path, PathBuf};
use std::process::Command;

/// 一份最小 schema：`api_call_logs` 两个外键列 + 两张目标表（都带主键）。
///
/// 为什么自己造而不用现成 fixture：G-1 要的是「租户键 vs 非租户键」的**对照**，
/// 两个列必须长在同一张表里，才能证明拒绝的是**列级**判断而不是整表判断。
fn schema_json() -> String {
    serde_json::json!({
        "server_version": "PostgreSQL 17.2",
        "transaction_read_only": "on",
        "schema": "public",
        "table_count": 3,
        "column_count": 6,
        "commented_column_count": 0,
        "foreign_key_count": 0,
        "introspected_at": "2026-10-23T00:00:00Z",
        "tables": [
            {
                "name": "api_call_logs",
                "comment": "API 调用日志",
                "column_count": 3,
                "commented_count": 0,
                "primary_key": "id",
                "unique_columns": ["id"],
                "columns": [
                    {"name": "id", "data_type": "text", "nullable": false, "comment": null, "is_primary_key": true},
                    {"name": "license_id", "data_type": "text", "nullable": true, "comment": null, "is_primary_key": false},
                    {"name": "customer_id", "data_type": "text", "nullable": true, "comment": null, "is_primary_key": false}
                ]
            },
            {
                "name": "licenses",
                "comment": "授权",
                "column_count": 1,
                "commented_count": 0,
                "primary_key": "id",
                "unique_columns": ["id"],
                "columns": [
                    {"name": "id", "data_type": "text", "nullable": false, "comment": null, "is_primary_key": true}
                ]
            },
            {
                "name": "customers",
                "comment": "客户",
                "column_count": 1,
                "commented_count": 0,
                "primary_key": "id",
                "unique_columns": ["id"],
                "columns": [
                    {"name": "id", "data_type": "text", "nullable": false, "comment": null, "is_primary_key": true}
                ]
            }
        ]
    })
    .to_string()
}

fn names_json() -> String {
    serde_json::json!({
        "agent": "test",
        "version": "1",
        "generated_at": "2026-10-23T00:00:00Z",
        "source": "test",
        "names": {
            "api_call_logs": "调用日志",
            "licenses": "授权",
            "customers": "客户"
        }
    })
    .to_string()
}

fn tenancy_json() -> String {
    serde_json::json!({
        "columns": ["customer_id", "main_customer_id", "sub_customer_id"],
        "reviewed_by": "test"
    })
    .to_string()
}

/// 一份签字产物：`column` 列的 inherit 提案（`to` 目标），已签。
fn signed_inherit(subject: &str, column: &str, to: &str) -> String {
    serde_json::json!({
        "version": 1,
        "kind": "fk-proposals",
        "method": "fk-value-overlap-v1",
        "source": {
            "schema_file": "schema.json",
            "schema_sha256": "00",
            "schema": "public",
            "read_only": "on",
            "tables": 3,
            "columns": 6
        },
        "proposals": [{
            "subject": subject,
            "claim": "同名同义列继承",
            "evidence": ["inherited from other.license_id (verified 5/5)"],
            "facts": {
                "to_table": to,
                "to_key_column": "id",
                "target_kind": "primary_key",
                "matched": 5,
                "sampled": 5,
                "window_rows": 5,
                "window_distinct": 5,
                "ratio_milli": 1000,
                "samples": [],
                "sample_unique": false,
                "unique_column": false,
                "match_kind": "plural",
                "stem": column.trim_end_matches("_id"),
                "inherit": {
                    "from_table": "other_table",
                    "from_column": column,
                    "matched": 5,
                    "sampled": 5,
                    "ratio_milli": 1000,
                    "peers": 0
                }
            },
            "confidence": 0.75,
            "band": "inherit",
            "cardinality": "N:1",
            "reason": "同名同义列继承：other_table 实测 5/5；本列无数据",
            "decided_by": "rule:fk-value-overlap",
            "reviewed_by": "attacker"
        }],
        "rejected": [],
        "unresolved": [],
        "stats": {}
    })
    .to_string()
}

fn write(dir: &Path, name: &str, body: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, body).expect("写夹具失败");
    p
}

fn tmp(tag: &str) -> PathBuf {
    let dir = Path::new("..")
        .join(".review-workspace")
        .join("req120")
        .join(format!("{}-{}-{}", tag, std::process::id(), line!()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("建临时目录失败");
    dir
}

/// 跑一次真进程：返回 (是否成功, stderr, 产物是否存在)。
fn run(dir: &Path, signed: &Path, out: &Path, with_tenancy: bool) -> (bool, String, bool) {
    let schema = write(dir, "schema.json", &schema_json());
    let names = write(dir, "names.json", &names_json());
    let stats = dir.join("stats.json");
    let mut cmd = propose_bin();
    cmd.args([
        "--schema",
        schema.to_str().unwrap(),
        "--names",
        names.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--stats",
        stats.to_str().unwrap(),
        "--project",
        "g1",
        "--fk-sign",
        signed.to_str().unwrap(),
    ]);
    if with_tenancy {
        let t = write(dir, "tenancy.json", &tenancy_json());
        cmd.args(["--tenancy", t.to_str().unwrap()]);
    }
    let o = cmd.output().expect("跑 ontok-propose 失败");
    (
        o.status.success(),
        String::from_utf8_lossy(&o.stderr).to_string(),
        out.exists(),
    )
}

/// **判据 1 + 2**：租户键列的 inherit ⇒ 拒绝整份 + 不写产物。
#[test]
fn tenant_key_inherit_is_rejected_by_the_real_cli() {
    let dir = tmp("tenant");
    let signed = write(
        &dir,
        "signed.json",
        &signed_inherit("api_call_logs.customer_id", "customer_id", "customers"),
    );
    let out = dir.join("proposal.json");
    let (ok, stderr, produced) = run(&dir, &signed, &out, true);

    assert!(!ok, "租户键的 inherit 必须让进程失败；stderr={stderr}");
    assert!(!produced, "被拒时**不许**留下产物：{}", out.display());
    // 点名：哪条边、哪个列、为什么
    assert!(stderr.contains("api_call_logs.customer_id"), "要点名边：{stderr}");
    assert!(stderr.contains("租户键"), "要说清是租户键：{stderr}");
    assert!(stderr.contains("拒绝签字产物"), "{stderr}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// **判据 3**：非租户列的 inherit 照常通过，并真的进了提案。
#[test]
fn license_id_inherit_still_passes_the_real_cli() {
    let dir = tmp("license");
    let signed = write(
        &dir,
        "signed.json",
        &signed_inherit("api_call_logs.license_id", "license_id", "licenses"),
    );
    let out = dir.join("proposal.json");
    let (ok, stderr, produced) = run(&dir, &signed, &out, true);
    assert!(ok, "非租户列的 inherit 必须能过；stderr={stderr}");
    assert!(produced, "应当写出提案");

    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    let link = doc["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| {
            i["kind"] == "add_link_type"
                && i["payload"]["from_type"] == "api_call_logs"
                && i["payload"]["to_type"] == "licenses"
        })
        .expect("inherit 边必须进提案");
    assert_eq!(link["payload"]["join_verified"], serde_json::json!("inherit"));
    assert!(
        link["payload"]["join_evidence"]
            .as_str()
            .unwrap()
            .starts_with("inherited from "),
        "继承证据要写明来源：{link}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// 不给 `--tenancy` 时，inherit 签字产物**同样**被拒（无法复核 ⇒ fail-closed）。
#[test]
fn inherit_without_tenancy_declaration_is_rejected_too() {
    let dir = tmp("notenancy");
    let signed = write(
        &dir,
        "signed.json",
        &signed_inherit("api_call_logs.license_id", "license_id", "licenses"),
    );
    let out = dir.join("proposal.json");
    let (ok, stderr, produced) = run(&dir, &signed, &out, false);
    assert!(!ok, "没有租户声明时 inherit 不许放行；stderr={stderr}");
    assert!(!produced);
    assert!(stderr.contains("租户键声明"), "{stderr}");
    let _ = std::fs::remove_dir_all(&dir);
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

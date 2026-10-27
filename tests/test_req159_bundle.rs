//! REQ-159 · **构建包（`bundle pack` / `bundle verify`）**的专属守卫。
//!
//! # 这一单要治的病
//!
//! 构建端把「本体 + AM + 源库描述」打成一个包交给服务端入库。中间有两处
//! **静默出错**的坑，都不是「报错」而是「看起来成功了」：
//!
//! | 坑 | 后果 | 本文件怎么钉 |
//! | --- | --- | --- |
//! | 拿 `ontology show` 的**视图**当快照 | 服务端入库后关系**证据全丢**，一个字节都不报错 | `snapshot_in_bundle_is_the_raw_struct_not_a_view` + `EVIDENCE_MISSING` 用例 |
//! | 手工改 manifest（sha256 / bytes 不同步） | 服务端整包拒，而且拒在哪一格要看日志 | 每个 `bundle.sha256.mismatch` / `files.mismatch` 用例 |
//!
//! # 本文件钉住的判据
//!
//! | # | 判据 | 用例 |
//! | --- | --- | --- |
//! | AC-001 | 包内快照 = `Snapshot` 的 `serde_json::to_vec` **原文** | `snapshot_in_bundle_is_the_raw_struct_not_a_view` |
//! | AC-002 | 同输入 + 同 `--built-at` ⇒ 两次打包**逐字节相同** | `pack_is_bytewise_deterministic` |
//! | AC-003 | store-only zip（method 0 / 无 data descriptor / 固定时间戳） | `zip_is_store_only_with_fixed_timestamps` + `zip_roundtrip_*` |
//! | AC-004 | 8 个拒绝码**每一个**都有对应用例（见文末对照表） | `reject_*` |
//! | AC-005 | `assemble` / `verify` 是**纯函数**（不出现 sqlx / 连接池 / 网络） | `assemble_and_verify_are_pure_functions` |
//! | AC-006 | 连库只在 `cli.rs` 边缘，且 `verify` 在**连库之前**分派 | `cli_wires_bundle_after_the_pre_connect_dispatch` |
//! | AC-007 | ⛔ 不新增 crate（zip / flate2 不许出现在 `Cargo.toml`） | `no_zip_crate_was_added` |
//!
//! ## 8 个拒绝码 → 用例
//!
//! | 码 | 用例 |
//! | --- | --- |
//! | `bundle.manifest.invalid` | `reject_manifest_invalid_missing_required_field` / `reject_manifest_invalid_unknown_field` / `reject_manifest_invalid_requires_dsn_without_dialect` / `reject_manifest_invalid_db_path_not_in_bundle` |
//! | `bundle.files.mismatch` | `reject_files_mismatch_when_a_file_is_undeclared` / `reject_files_mismatch_when_a_declared_file_is_missing` / `reject_files_mismatch_when_manifest_lists_itself` |
//! | `bundle.sha256.mismatch` | `reject_sha256_mismatch_when_declared_sha_is_wrong` / `reject_sha256_mismatch_when_declared_bytes_is_wrong` / `reject_sha256_mismatch_when_container_bytes_are_corrupted` |
//! | `bundle.project.mismatch` | ⛔ **服务端专属**（构建端不知道收件项目）—— 见 `server_only_codes_are_documented_as_such` |
//! | `bundle.version.unsupported` | `reject_version_unsupported` |
//! | `bundle.path.illegal` | `reject_path_illegal_in_files` / `reject_path_illegal_for_a_handmade_entry_name` |
//! | `db.dsn.missing` | ⛔ **服务端专属**（构建端永远不该有 DSN）—— 见 `server_only_codes_are_documented_as_such` |
//! | `bundle.too_large` | `reject_too_large` |
//!
//! 另有两条构建端补充码：`bundle.snapshot.evidence_missing`
//! （`reject_evidence_missing` / `evidence_null_counts_as_missing`）与
//! `bundle.credential.suspect`（`credential_suspect_is_a_warning_and_does_not_reject`，**警告级**）。
//! 第三条补充码是**警告级**的 `bundle.manifest.not_first`
//! （`manifest_not_first_is_only_a_warning`）。
//!
//! # QA 退回的 5 条各由谁钉住
//!
//! | # | 问题 | 用例 |
//! | --- | --- | --- |
//! | F-01 | `sha256` 报错路径被双前缀 | 库内单测 `bundle::verify::tests::sha256_must_be_64_lowercase_hex`（`cargo test --lib bundle::`） |
//! | F-02 | `evidence: null` 被当「有证据」 | `evidence_null_counts_as_missing` |
//! | F-03 | manifest 不是第一个 entry 应只警告 | `manifest_not_first_is_only_a_warning` |
//! | F-04 | 派生 entry 名含 `..` ⇒ 打出「自己必拒」的包 | `pack_refuses_a_dotdot_entry_name_instead_of_a_self_rejected_bundle` + `pack_rejects_a_dotdot_declaration_name_before_db_and_writes_nothing`（真进程） |
//! | F-05 | zip 名超长静默截断 | 库内单测 `bundle::zip::tests::name_too_long_is_rejected_instead_of_truncated` |
//!
//! ⚠️ 除 F-04 的那条**真进程**用例外，其余全部对着
//! `ontok::bundle::{assemble, verify}` 这两个纯函数断言，一条都不连库。
//!
//! ⚠️ 本文件不加 `//! @guard-manifest: REQ-081`（与 REQ-107/157/158 的同类文件一致）。

// ⭐ 判据读源码要走 `tests/common/source_root.rs`（**运行时**定位 crate 根）。
// ⛔ 不用 `env!("CARGO_MANIFEST_DIR")`：它是编译期常量，二进制搬到别处仍会去读
// 「编它时那棵树」；REQ-046 的事故形态见 `common/source_root.rs` 的文件头。
mod common;

use ontok::bundle::{assemble, code, verify, verify_with_limit, zip, PackInput};
use ontok::domain::snapshot::{EntityType, Evidence, LinkType, Property, Snapshot};
use ontok::query::audit::sha256_hex;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::process::Command;

// ══════════════════════════════════════════════════════════════
// 夹具
// ══════════════════════════════════════════════════════════════

/// `--built-at` 钉死（确定性用例的前提）。
const BUILT_AT: &str = "2026-10-27T02:30:00Z";
const PROJECT: &str = "cloud-resource";
const SEQ: i32 = 5;

fn prop(name: &str, column: &str) -> Property {
    Property {
        name: name.to_string(),
        column: column.to_string(),
        nullable: false,
        note: None,
        sample: Some("cvm-xxxx".to_string()),
    }
}

fn ev(table: &str, column: &str) -> Evidence {
    Evidence {
        table: table.to_string(),
        column: column.to_string(),
        claim: format!("{table}.{column} 的样本落在这里"),
    }
}

/// 一份**最小但齐全**的快照：2 个类 / 1 条关系 / 2 个属性 / 带证据。
///
/// ⚠️ 刻意让 `link_types[0].evidence` 非空：包里的快照必须逐字节等于
/// 这个结构体的 `to_vec`，而「证据还在」正是这一单要保的东西。
fn snapshot() -> Snapshot {
    let mut s = Snapshot::new(uuid::Uuid::nil(), SEQ);
    s.entity_types = vec![
        EntityType {
            key: "cloud_resource".to_string(),
            display_name: "云资源".to_string(),
            description: Some("云上可计费的资源".to_string()),
            aliases: vec!["资源".to_string()],
            parent: None,
            disjoint_with: vec![],
            properties: vec![prop("资源 ID", "resource_id"), prop("地域", "region")],
            evidence: vec![ev("t_resource", "id")],
        },
        EntityType {
            key: "cvm_instance".to_string(),
            display_name: "云主机".to_string(),
            description: None,
            aliases: vec![],
            parent: Some("cloud_resource".to_string()),
            disjoint_with: vec!["bucket".to_string()],
            properties: vec![],
            evidence: vec![],
        },
    ];
    s.link_types = vec![LinkType {
        key: "cvm_belongs_to_region".to_string(),
        from_type: "cvm_instance".to_string(),
        to_type: "cloud_resource".to_string(),
        cardinality: "N:1".to_string(),
        semantic: "belongs_to".to_string(),
        evidence: vec![ev("t_cvm", "resource_id")],
        confidence: None,
        join_verified: None,
        join_evidence: Some("命中 120/120".to_string()),
    }];
    s
}

/// AM 文件的最小形状（`components.am` 的 entities / sources 从这里数）。
fn am_bytes() -> Vec<u8> {
    serde_json::to_vec_pretty(&json!({
        "contract_version": 1,
        "mapping_version": 1,
        "project": PROJECT,
        "sources": [{"id": "tcapi", "kind": "http", "base_url": "https://cvm.tencentcloudapi.com"}],
        "entities": [
            {"id": "cvm", "class": "cvm_instance", "get": {"ref": "DescribeInstances"}},
            {"id": "res", "class": "cloud_resource", "get": {"ref": "DescribeResources"}}
        ],
        "relations": []
    }))
    .unwrap()
}

fn db_bytes() -> Vec<u8> {
    serde_json::to_vec_pretty(&json!({
        "schema": "public",
        "tables": [{"name": "t_cvm", "columns": [{"name": "resource_id", "type": "text"}]}]
    }))
    .unwrap()
}

/// 一份**四件齐全**的输入（本体 + AM + 源库 + 声明）。
fn input() -> PackInput {
    let mut i = PackInput::new(PROJECT, SEQ, BUILT_AT, snapshot());
    i.builder_host = Some("lion-mbp".to_string());
    i.am = vec![("access-mapping.json".to_string(), am_bytes())];
    i.db = Some(ontok::bundle::pack::DbInput {
        bytes: db_bytes(),
        requires_dsn: true,
        dialect: None, // 由 pack 补 postgresql（契约的缺省）
    });
    i.declarations = vec![
        ("policy.json".to_string(), br#"{"version":1}"#.to_vec()),
        ("tenancy.json".to_string(), br#"{"version":1}"#.to_vec()),
    ];
    i
}

/// 打一份**能过自检**的包。
fn packed() -> Vec<u8> {
    assemble(&input()).expect("夹具应当打得出来")
}

fn entries_of(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    zip::read(bytes)
        .expect("读不出 zip")
        .into_iter()
        .map(|e| (e.path, e.data))
        .collect()
}

fn manifest_of(bytes: &[u8]) -> Value {
    let map: BTreeMap<String, Vec<u8>> = entries_of(bytes).into_iter().collect();
    serde_json::from_slice(map.get("manifest.json").expect("没有 manifest.json")).expect("manifest 不是 JSON")
}

/// 重打一份包（**故意绕过 `assemble`**：很多拒绝码只能靠构造坏包才测得出来）。
fn rebuild(bytes: &[u8], mutate: impl FnOnce(&mut Vec<(String, Vec<u8>)>)) -> Vec<u8> {
    let mut entries = entries_of(bytes);
    mutate(&mut entries);
    zip::write(&entries).expect("重打失败")
}

/// 改 manifest（`manifest.json` 不在 `files[]` 里 ⇒ 改它不会牵动别处的 sha256）。
fn edit_manifest(bytes: &[u8], mut f: impl FnMut(&mut Value)) -> Vec<u8> {
    rebuild(bytes, |entries| {
        for (path, data) in entries.iter_mut() {
            if path == "manifest.json" {
                let mut v: Value = serde_json::from_slice(data).expect("manifest 不是 JSON");
                f(&mut v);
                *data = serde_json::to_vec_pretty(&v).expect("manifest 序列化失败");
            }
        }
    })
}

/// 替换包内某个文件的字节，并**同步修好** `files[]` 里的 sha256 / bytes。
///
/// 这样才测得到「只坏一件事」的用例：否则每个用例都会同时报 files.mismatch。
fn replace_file(bytes: &[u8], path: &str, new_bytes: Vec<u8>) -> Vec<u8> {
    let new_sha = sha256_hex(&new_bytes);
    let new_len = new_bytes.len();
    let mut out = rebuild(bytes, |entries| {
        for (p, d) in entries.iter_mut() {
            if p == path {
                *d = new_bytes.clone();
            }
        }
    });
    out = edit_manifest(&out, |m| {
        for f in m["files"].as_array_mut().expect("files 不是数组") {
            if f["path"] == Value::from(path) {
                f["sha256"] = Value::from(new_sha.clone());
                f["bytes"] = Value::from(new_len);
            }
        }
    });
    out
}

/// 手写一份 zip（**不经过 `zip::write`**，于是能造出非法 entry 名 / 外来结构）。
fn raw_store_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut cd: Vec<u8> = Vec::new();
    for (name, data) in entries {
        let n = name.as_bytes();
        let crc = zip::crc32(data);
        let off = out.len() as u32;
        let size = data.len() as u32;

        out.extend_from_slice(&zip::LOCAL_SIG.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&zip::FLAG_UTF8.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&zip::DOS_TIME_MIDNIGHT.to_le_bytes());
        out.extend_from_slice(&zip::DOS_DATE_1980_01_01.to_le_bytes());
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&(n.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(n);
        out.extend_from_slice(data);

        cd.extend_from_slice(&zip::CENTRAL_SIG.to_le_bytes());
        cd.extend_from_slice(&0x0014u16.to_le_bytes());
        cd.extend_from_slice(&20u16.to_le_bytes());
        cd.extend_from_slice(&zip::FLAG_UTF8.to_le_bytes());
        cd.extend_from_slice(&0u16.to_le_bytes());
        cd.extend_from_slice(&zip::DOS_TIME_MIDNIGHT.to_le_bytes());
        cd.extend_from_slice(&zip::DOS_DATE_1980_01_01.to_le_bytes());
        cd.extend_from_slice(&crc.to_le_bytes());
        cd.extend_from_slice(&size.to_le_bytes());
        cd.extend_from_slice(&size.to_le_bytes());
        cd.extend_from_slice(&(n.len() as u16).to_le_bytes());
        cd.extend_from_slice(&0u16.to_le_bytes());
        cd.extend_from_slice(&0u16.to_le_bytes());
        cd.extend_from_slice(&0u16.to_le_bytes());
        cd.extend_from_slice(&0u16.to_le_bytes());
        cd.extend_from_slice(&0u32.to_le_bytes());
        cd.extend_from_slice(&off.to_le_bytes());
        cd.extend_from_slice(n);
    }
    let cd_off = out.len() as u32;
    let cd_size = cd.len() as u32;
    out.extend_from_slice(&cd);
    out.extend_from_slice(&zip::EOCD_SIG.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_off.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

/// 报告里出现的码（去重、保持顺序）。
fn codes(report: &ontok::bundle::Report) -> Vec<&'static str> {
    let mut v: Vec<&'static str> = report.problems.iter().map(|p| p.code).collect();
    v.dedup();
    v
}

// ══════════════════════════════════════════════════════════════
// AC-003 · 容器（store-only zip）
// ══════════════════════════════════════════════════════════════

#[test]
fn crc32_matches_the_standard_check_value() {
    // zlib / Python zipfile 用的就是这一份 CRC32 表。
    assert_eq!(zip::crc32(b"123456789"), 0xCBF4_3926);
    assert_eq!(zip::crc32(b""), 0);
    assert_ne!(zip::crc32(b"a"), zip::crc32(b"A"));
}

#[test]
fn zip_roundtrip_is_bytewise_lossless() {
    let entries = vec![
        ("manifest.json".to_string(), br#"{"a":1}"#.to_vec()),
        ("ontology/snapshot.json".to_string(), vec![]),
        ("db/schema.json".to_string(), "中文内容也在包里原样躺着".as_bytes().to_vec()),
    ];
    let bytes = zip::write(&entries).expect("写失败");
    let back = zip::read(&bytes).expect("读失败");
    assert_eq!(back.len(), 3);
    for (i, (path, data)) in entries.iter().enumerate() {
        assert_eq!(&back[i].path, path);
        assert_eq!(&back[i].data, data, "`{path}` 的字节不相等");
        assert!(back[i].crc_ok, "`{path}` 的 CRC32 对不上");
    }
}

#[test]
fn zip_is_store_only_with_fixed_timestamps() {
    let bytes = zip::write(&[("manifest.json".to_string(), b"{}".to_vec())]).unwrap();
    // local file header: sig(4) version(2) flags(2) method(2) time(2) date(2) …
    assert_eq!(
        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        zip::LOCAL_SIG
    );
    assert_eq!(u16::from_le_bytes([bytes[6], bytes[7]]), zip::FLAG_UTF8);
    assert_eq!(
        u16::from_le_bytes([bytes[6], bytes[7]]) & 0x0008,
        0,
        "⛔ 不许置 data descriptor（bit 3）"
    );
    assert_eq!(u16::from_le_bytes([bytes[8], bytes[9]]), 0, "method 必须是 0（store）");
    assert_eq!(
        u16::from_le_bytes([bytes[10], bytes[11]]),
        zip::DOS_TIME_MIDNIGHT,
        "时间戳必须固定"
    );
    assert_eq!(
        u16::from_le_bytes([bytes[12], bytes[13]]),
        zip::DOS_DATE_1980_01_01,
        "日期必须固定（1980-01-01）"
    );
    // 两次写出的字节相同 ⇒ 没有渗进时间 / 文件名顺序之类的环境因素。
    assert_eq!(
        bytes,
        zip::write(&[("manifest.json".to_string(), b"{}".to_vec())]).unwrap()
    );
}

// ══════════════════════════════════════════════════════════════
// AC-001 / AC-002 · 打包的语义与确定性
// ══════════════════════════════════════════════════════════════

#[test]
fn snapshot_in_bundle_is_the_raw_struct_not_a_view() {
    let bytes = packed();
    let map: BTreeMap<String, Vec<u8>> = entries_of(&bytes).into_iter().collect();
    let in_bundle = map.get("ontology/snapshot.json").expect("包里没有快照");

    // ① 逐字节等于 `Snapshot` 的 `serde_json::to_vec` —— 中间不许有任何 view / filter。
    assert_eq!(
        in_bundle,
        &serde_json::to_vec(&snapshot()).unwrap(),
        "包里的快照不是 Snapshot 原文（被投影过了？）"
    );

    // ② 反空转：这些字段**只有**快照原文才有，`LinkTypeView`（5 个字段）一个都不会有。
    let v: Value = serde_json::from_slice(in_bundle).unwrap();
    let link = &v["link_types"][0];
    assert!(link.get("evidence").is_some(), "关系证据不见了 —— 视图当快照用了");
    assert!(link.get("join_evidence").is_some(), "连接验证的证据不见了");
    assert!(
        link.get("cardinality").is_some() && link.get("semantic").is_some(),
        "视图的那 5 个字段也应当在（否则这不是快照）"
    );
    let e = &v["entity_types"][0];
    assert_eq!(e["properties"][0]["column"], Value::from("resource_id"), "列名不见了");
    assert!(e.get("evidence").is_some(), "类的证据不见了");
    for k in ["version_id", "seq", "extracted_by", "action_types", "functions", "concept_groups"] {
        assert!(v.get(k).is_some(), "快照缺 `{k}` 这一格");
    }
}

#[test]
fn pack_is_bytewise_deterministic() {
    let a = packed();
    let b = packed();
    assert_eq!(a, b, "同一输入 + 同一 --built-at ⇒ 两次打包必须逐字节相同");

    // 输入顺序不影响产物（`assemble` 按 path 升序排）。
    let mut shuffled = input();
    shuffled.declarations.reverse();
    assert_eq!(assemble(&shuffled).unwrap(), a, "声明文件给的顺序不该影响产物");

    // 反空转：`--built-at` 变了，产物必须变（否则「钉死时间」这件事没被证明）。
    let mut other = input();
    other.built_at = "2026-10-28T02:30:00Z".to_string();
    assert_ne!(assemble(&other).unwrap(), a, "换了 built_at 却打出一样的包");
}

#[test]
fn pack_puts_manifest_first_and_lists_every_other_file_in_ascending_order() {
    let bytes = packed();
    let names: Vec<String> = entries_of(&bytes).into_iter().map(|(p, _)| p).collect();
    assert_eq!(names[0], "manifest.json", "manifest 必须是第一个 entry");
    let rest = &names[1..];
    let mut sorted = rest.to_vec();
    sorted.sort();
    assert_eq!(rest, sorted.as_slice(), "其余 entry 必须按 path 升序");

    let m = manifest_of(&bytes);
    let declared: Vec<String> = m["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(declared, rest, "files[] 必须与包内其余文件**同序同集合**");
    assert!(
        !declared.contains(&"manifest.json".to_string()),
        "files[] 不许列 manifest.json 自己（它无法包含自己的 sha256）"
    );

    // bytes / sha256 逐条对上真实字节（这正是服务端会做的事）。
    let map: BTreeMap<String, Vec<u8>> = entries_of(&bytes).into_iter().collect();
    for f in m["files"].as_array().unwrap() {
        let p = f["path"].as_str().unwrap();
        assert_eq!(f["bytes"].as_u64().unwrap() as usize, map[p].len(), "`{p}` 的 bytes 不符");
        assert_eq!(f["sha256"].as_str().unwrap(), sha256_hex(&map[p]), "`{p}` 的 sha256 不符");
    }
}

#[test]
fn verify_accepts_a_freshly_packed_bundle() {
    let report = verify(&packed());
    assert!(report.ok, "刚打出来的包过不了自检：{:?}", report.problems);
    assert!(report.problems.is_empty());
    assert!(report.warnings.is_empty(), "干净的包不该有警告：{:?}", report.warnings);

    let s = &report.summary;
    assert_eq!(s.project.as_deref(), Some(PROJECT));
    assert_eq!(s.ontology_seq, Some(SEQ as i64));
    assert_eq!(s.bundle_version.as_deref(), Some("1.0"));
    assert_eq!(s.built_at.as_deref(), Some(BUILT_AT));
    assert_eq!(s.requires_dsn, Some(true));
    assert_eq!(s.entity_types, Some(2));
    assert_eq!(s.link_types, Some(1));
    // 5 个 entry：manifest + ontology/snapshot + am + db + 2 声明 = 6
    assert_eq!(s.files_in_manifest, 5);
    assert_eq!(s.entries, 6);
}

#[test]
fn pack_defaults_dialect_to_postgresql_when_requires_dsn() {
    let m = manifest_of(&packed());
    assert_eq!(m["components"]["db"]["requires_dsn"], Value::from(true));
    assert_eq!(m["components"]["db"]["dialect"], Value::from("postgresql"));
    // am 的两个计数是从 AM 文件里数出来的，不是猜的。
    assert_eq!(m["components"]["am"]["entities"], Value::from(2));
    assert_eq!(m["components"]["am"]["sources"], Value::from(1));
    // 357 = 2 个属性（1 个类有 2 个）—— `properties` 是各实体属性数之和。
    assert_eq!(m["components"]["ontology"]["properties"], Value::from(2));
}

#[test]
fn pack_refuses_a_project_name_the_schema_would_reject() {
    let mut bad = input();
    bad.project = "Cloud_Resource".to_string();
    let e = assemble(&bad).expect_err("坏项目名应当打不出来");
    assert!(format!("{e}").contains("pattern"), "错误信息要点名 pattern：{e}");

    let mut bad_seq = input();
    bad_seq.ontology_seq = 0;
    assert!(assemble(&bad_seq).is_err(), "`ontology_seq < 1` 应当打不出来");

    let mut bad_time = input();
    bad_time.built_at = "昨天".to_string();
    assert!(assemble(&bad_time).is_err(), "非 RFC3339 的 built_at 应当打不出来");
}

// ══════════════════════════════════════════════════════════════
// 拒绝码 1 · bundle.manifest.invalid
// ══════════════════════════════════════════════════════════════

#[test]
fn reject_manifest_invalid_missing_required_field() {
    let bytes = edit_manifest(&packed(), |m| {
        m.as_object_mut().unwrap().remove("built_at");
    });
    let report = verify(&bytes);
    assert!(!report.ok);
    assert!(codes(&report).contains(&code::MANIFEST_INVALID));
    let p = report
        .problems
        .iter()
        .find(|p| p.code == code::MANIFEST_INVALID)
        .unwrap();
    assert_eq!(p.path, "manifest.json.built_at", "必须点名到字段路径");
    assert!(!p.hint.is_empty(), "问题必须带「怎么改」");
}

#[test]
fn reject_manifest_invalid_unknown_field() {
    let bytes = edit_manifest(&packed(), |m| {
        m["signature"] = Value::from("nope"); // schema 是 additionalProperties:false
    });
    let report = verify(&bytes);
    let p = report
        .problems
        .iter()
        .find(|p| p.code == code::MANIFEST_INVALID)
        .unwrap();
    assert_eq!(p.path, "manifest.json.signature");
    assert!(p.message.contains("additionalProperties"));
}

#[test]
fn reject_manifest_invalid_requires_dsn_without_dialect() {
    let bytes = edit_manifest(&packed(), |m| {
        m["components"]["db"].as_object_mut().unwrap().remove("dialect");
    });
    let report = verify(&bytes);
    let p = report
        .problems
        .iter()
        .find(|p| p.code == code::MANIFEST_INVALID)
        .unwrap();
    assert_eq!(p.path, "manifest.json.components.db.dialect");
}

#[test]
fn reject_manifest_invalid_db_path_not_in_bundle() {
    let bytes = edit_manifest(&packed(), |m| {
        m["components"]["db"]["path"] = Value::from("db/schema-v2.json");
    });
    let report = verify(&bytes);
    let p = report
        .problems
        .iter()
        .find(|p| p.code == code::MANIFEST_INVALID && p.path.contains("components.db.path"))
        .expect("应当点名 components.db.path");
    assert!(p.message.contains("db/schema-v2.json"));
}

// ══════════════════════════════════════════════════════════════
// 拒绝码 2 · bundle.files.mismatch
// ══════════════════════════════════════════════════════════════

#[test]
fn reject_files_mismatch_when_a_file_is_undeclared() {
    // 包里多一个文件（没写进 files[]）⇒ 多一个也拒。
    let bytes = rebuild(&packed(), |entries| {
        entries.push(("README.txt".to_string(), b"human notes".to_vec()));
    });
    let report = verify(&bytes);
    assert!(!report.ok);
    let p = report
        .problems
        .iter()
        .find(|p| p.code == code::FILES_MISMATCH)
        .expect("应当报 files.mismatch");
    assert!(p.message.contains("README.txt"), "必须点名多出来的文件：{}", p.message);
}

#[test]
fn reject_files_mismatch_when_a_declared_file_is_missing() {
    // 包内少一个文件（files[] 还声称有）⇒ 少一个也拒。
    let bytes = rebuild(&packed(), |entries| {
        entries.retain(|(p, _)| p != "db/schema.json");
    });
    let report = verify(&bytes);
    let p = report
        .problems
        .iter()
        .find(|p| p.code == code::FILES_MISMATCH)
        .expect("应当报 files.mismatch");
    assert!(p.message.contains("db/schema.json"), "必须点名少掉的文件：{}", p.message);
}

#[test]
fn reject_files_mismatch_when_manifest_lists_itself() {
    let bytes = edit_manifest(&packed(), |m| {
        let files = m["files"].as_array_mut().unwrap();
        files.push(json!({
            "path": "manifest.json",
            "sha256": "0".repeat(64),
            "bytes": 1
        }));
    });
    let report = verify(&bytes);
    let p = report
        .problems
        .iter()
        .find(|p| p.code == code::FILES_MISMATCH)
        .unwrap();
    assert!(
        p.message.contains("无法包含自己的 sha256"),
        "要解释为什么 manifest.json 不许列进来：{}",
        p.message
    );
}

// ══════════════════════════════════════════════════════════════
// 拒绝码 3 · bundle.sha256.mismatch
// ══════════════════════════════════════════════════════════════

#[test]
fn reject_sha256_mismatch_when_declared_sha_is_wrong() {
    let bytes = edit_manifest(&packed(), |m| {
        for f in m["files"].as_array_mut().unwrap() {
            if f["path"] == Value::from("ontology/snapshot.json") {
                f["sha256"] = Value::from("f".repeat(64));
            }
        }
    });
    let report = verify(&bytes);
    let p = report
        .problems
        .iter()
        .find(|p| p.code == code::SHA256_MISMATCH)
        .expect("应当报 sha256.mismatch");
    assert!(p.path.contains("snapshot.json"), "要点名文件：{}", p.path);
}

#[test]
fn reject_sha256_mismatch_when_declared_bytes_is_wrong() {
    let bytes = edit_manifest(&packed(), |m| {
        for f in m["files"].as_array_mut().unwrap() {
            if f["path"] == Value::from("db/schema.json") {
                let n = f["bytes"].as_u64().unwrap();
                f["bytes"] = Value::from(n + 1);
            }
        }
    });
    let report = verify(&bytes);
    let p = report
        .problems
        .iter()
        .find(|p| p.code == code::SHA256_MISMATCH)
        .expect("应当报 sha256.mismatch");
    assert_eq!(p.path, "db/schema.json");
    assert!(p.message.contains("bytes"), "要说是 bytes 不符：{}", p.message);
}

#[test]
fn reject_sha256_mismatch_when_container_bytes_are_corrupted() {
    // 改包里的**数据字节**（不动 manifest）⇒ CRC32 先炸，sha256 也随之不符。
    let mut bytes = packed();
    let at = bytes
        .windows(9)
        .position(|w| w == &b"resource_"[..])
        .expect("夹具里应当有 resource_ 这个片段");
    bytes[at] ^= 0xFF;
    let report = verify(&bytes);
    assert!(!report.ok, "被改过的包必须拒");
    let p = report
        .problems
        .iter()
        .find(|p| p.code == code::SHA256_MISMATCH)
        .expect("应当报 sha256.mismatch");
    assert!(
        p.message.contains("CRC32") || p.message.contains("sha256"),
        "要指明是完整性不符：{}",
        p.message
    );
}

// ══════════════════════════════════════════════════════════════
// 拒绝码 5 · bundle.version.unsupported
// ══════════════════════════════════════════════════════════════

#[test]
fn reject_version_unsupported() {
    let bytes = edit_manifest(&packed(), |m| {
        m["bundle_version"] = Value::from("2.0");
    });
    let report = verify(&bytes);
    let p = report
        .problems
        .iter()
        .find(|p| p.code == code::VERSION_UNSUPPORTED)
        .expect("应当报 version.unsupported");
    assert_eq!(p.path, "manifest.json.bundle_version");
    assert!(p.message.contains("1.0"), "要列出支持的版本：{}", p.message);
}

// ══════════════════════════════════════════════════════════════
// 拒绝码 6 · bundle.path.illegal
// ══════════════════════════════════════════════════════════════

#[test]
fn reject_path_illegal_in_files() {
    let bytes = edit_manifest(&packed(), |m| {
        let files = m["files"].as_array_mut().unwrap();
        files[0]["path"] = Value::from("../etc/passwd");
    });
    let report = verify(&bytes);
    let p = report
        .problems
        .iter()
        .find(|p| p.code == code::PATH_ILLEGAL)
        .expect("应当报 path.illegal");
    assert!(p.path.contains("files[0].path"), "要点名那一格：{}", p.path);
}

#[test]
fn reject_path_illegal_for_a_handmade_entry_name() {
    // 手写的 zip（`zip::write` 会先拒非法名，所以这里绕开它）。
    // 包内真实存在一个 `../` 开头的 entry ⇒ 解包器可能写到包外。
    let bytes = raw_store_zip(&[
        ("manifest.json", br#"{"bundle_version":"1.0"}"#),
        ("../evil.json", br#"{}"#),
    ]);
    let report = verify(&bytes);
    let p = report
        .problems
        .iter()
        .find(|p| p.code == code::PATH_ILLEGAL)
        .expect("应当报 path.illegal");
    assert_eq!(p.path, "../evil.json");
}

// ══════════════════════════════════════════════════════════════
// 拒绝码 8 · bundle.too_large
// ══════════════════════════════════════════════════════════════

#[test]
fn reject_too_large() {
    let bytes = packed();
    let limit = (bytes.len() as u64) - 1;
    let report = verify_with_limit(&bytes, limit);
    let p = report
        .problems
        .iter()
        .find(|p| p.code == code::TOO_LARGE)
        .expect("应当报 too_large");
    assert!(p.message.contains(&bytes.len().to_string()), "要报实际大小");
    assert!(p.message.contains(&limit.to_string()), "要报上限");
    // 上限够大时同一份包必须过（反空转：别让 too_large 恒真）。
    assert!(verify_with_limit(&bytes, bytes.len() as u64).ok);
}

// ══════════════════════════════════════════════════════════════
// 构建端补充码 9 / 10
// ══════════════════════════════════════════════════════════════

#[test]
fn reject_evidence_missing() {
    // 把快照里的 `evidence` 键整体删掉 —— 这就是「视图当快照用」之后的形状。
    let mut snap: Value = serde_json::from_slice(&serde_json::to_vec(&snapshot()).unwrap()).unwrap();
    snap["link_types"][0].as_object_mut().unwrap().remove("evidence");
    let without_evidence = serde_json::to_vec(&snap).unwrap();

    let bytes = replace_file(&packed(), "ontology/snapshot.json", without_evidence);
    let report = verify(&bytes);
    assert!(!report.ok);
    let p = report
        .problems
        .iter()
        .find(|p| p.code == code::EVIDENCE_MISSING)
        .expect("应当报 evidence_missing");
    assert_eq!(p.path, "ontology/snapshot.json");
    assert!(p.message.contains("cvm_belongs_to_region"), "要点名是哪条关系：{}", p.message);
    assert!(p.hint.contains("get_snapshot"), "改法要指向正确取法：{}", p.hint);
}

#[test]
fn credential_suspect_is_a_warning_and_does_not_reject() {
    let leaky = br#"{"tables":[{"name":"t","dsn_note":"postgres://user:pw@host:5432/db"}]}"#.to_vec();
    let bytes = replace_file(&packed(), "db/schema.json", leaky);
    let report = verify(&bytes);
    assert!(report.ok, "凭据只告警不拒（v1）：{:?}", report.problems);
    assert!(report.problems.is_empty());
    let w = report
        .warnings
        .iter()
        .find(|p| p.code == code::CREDENTIAL_SUSPECT)
        .expect("应当有凭据警告");
    assert_eq!(w.path, "db/schema.json");
    assert!(w.message.contains("postgres://"), "要点名扫到的片段：{}", w.message);
}

// ══════════════════════════════════════════════════════════════
// 服务端专属的两个码（构建端**不该**假装能判）
// ══════════════════════════════════════════════════════════════

#[test]
fn server_only_codes_are_documented_as_such() {
    // `bundle.project.mismatch` 与 `db.dsn.missing` 在服务端判：
    // 构建端不知道收件项目、也永远不该有 DSN。这里钉的是「码存在且被登记」，
    // 而不是「构建端会报它」（假装能判只会给出假的通过）。
    assert_eq!(code::PROJECT_MISMATCH, "bundle.project.mismatch");
    assert_eq!(code::DSN_MISSING, "db.dsn.missing");
    let all = code::ALL;
    assert!(all.contains(&code::PROJECT_MISMATCH));
    assert!(all.contains(&code::DSN_MISSING));

    // 反空转：一份干净的包**不会**报这两个码（构建端没有它们的判据）。
    let report = verify(&packed());
    assert!(!codes(&report).contains(&code::PROJECT_MISMATCH));
    assert!(!codes(&report).contains(&code::DSN_MISSING));
}

// ══════════════════════════════════════════════════════════════
// AC-005 / AC-006 / AC-007 · 结构判据（源码级）
// ══════════════════════════════════════════════════════════════

fn repo_file(rel: &str) -> String {
    // ⭐ 锚点是**运行时**定位到的 crate 根（`tests/common/source_root.rs` 的
    // `crate_root`），不是编译期常量 —— 见本文件 `mod common;` 那一处的注释。
    let p = common::source_root::crate_root().join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("读不到 {}：{e}", p.display()))
}

/// 去掉注释行 / 行尾注释 —— 判据看的是**代码**，不是文档（文档里提 `PgStore`
/// 恰恰是「正确取法」的说明，不该被判红）。
fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| {
            if l.trim_start().starts_with("//") {
                return String::new();
            }
            match l.find(" //") {
                Some(i) => l[..i].to_string(),
                None => l.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn assemble_and_verify_are_pure_functions() {
    for (file, raw) in [
        ("src/bundle/pack.rs", repo_file("src/bundle/pack.rs")),
        ("src/bundle/verify.rs", repo_file("src/bundle/verify.rs")),
    ] {
        let src = code_only(&raw);
        for forbidden in [
            "sqlx::",
            "PgStore",
            "PgProjectStore",
            "db::pool",
            "crate::db::",
            "ureq::",
            "std::fs::",
            "std::process::Command",
            "tokio::",
        ] {
            assert!(
                !src.contains(forbidden),
                "⛔ `{file}` 的**代码**里出现了 `{forbidden}` —— 这两个函数必须是纯函数（不连库 / 不读盘 / 不发网络）"
            );
        }
    }
}

#[test]
fn cli_wires_bundle_after_the_pre_connect_dispatch() {
    let src = repo_file("src/cli.rs");
    assert!(
        src.contains("Cmd::Bundle { cmd } => return bundle_cmd(cmd).await,"),
        "`run()` 里必须有 bundle 的分派"
    );
    assert!(src.contains("pub enum BundleCmd"), "必须有 BundleCmd");
    assert!(
        src.contains("EXIT_BUNDLE_REJECTED: i32 = 2"),
        "必须新增退出码常量 2（派单 §2）"
    );
    assert_eq!(ontok::cli::EXIT_BUNDLE_REJECTED, 2);
    assert!(
        src.contains("std::process::exit(EXIT_BUNDLE_REJECTED)"),
        "`bundle verify` 拒了必须以 2 退出（不是 1、也不是 0）"
    );

    // ⚠️ 位置：`bundle` 的分派必须在**建连接池 / 迁移之前** ——
    // 否则 `bundle verify`（纯本地）会被「连不上库」挡住，报错指向假原因。
    let dispatch = src.find("Cmd::Bundle { cmd } => return bundle_cmd(cmd).await,").unwrap();
    let connect = src
        .find("crate::db::pg::migrate(&pool).await?;")
        .expect("找不到 run() 里的迁移调用");
    assert!(
        dispatch < connect,
        "`bundle` 必须在连库之前分派（dispatch @{dispatch}，migrate @{connect}）"
    );

    // 连库只有一处：pack 的支路（照抄 relation_verbs_check）。
    assert!(src.contains("async fn snapshot_for_pack("), "取快照必须收在一个函数里");
    assert!(
        src.contains("PgStore::new(pool)") && src.contains("get_snapshot(v.id)"),
        "取快照必须用 `PgStore::get_snapshot` 拿结构体（快照原文），⛔ 不许绕道 view"
    );
}

#[test]
fn no_zip_crate_was_added() {
    let toml = repo_file("Cargo.toml");
    for forbidden in ["zip =", "zip_rs", "flate2 =", "async_zip"] {
        assert!(
            !toml.contains(forbidden),
            "⛔ `Cargo.toml` 里出现了 `{forbidden}` —— 本单不许新增 crate（ZIP 是手写的）"
        );
    }
    // 反空转：确认我们读到的确实是那份依赖段。
    assert!(toml.contains("[dependencies]"));
    assert!(toml.contains("serde_json = \"1\""));
}

// ══════════════════════════════════════════════════════════════
// QA 退回项（F-02 / F-03 / F-04）—— 每条都钉「判据在**打包前**就成立」
// ══════════════════════════════════════════════════════════════

/// **F-04**（纯函数层）：`v1..2.json` 是合法文件名，但会派生出自检必拒的
/// 包内路径 ⇒ 必须在 `assemble` 阶段就按**参数错误**拒，⛔ 不许打出包再自检失败。
#[test]
fn pack_refuses_a_dotdot_entry_name_instead_of_a_self_rejected_bundle() {
    let mut bad = input();
    bad.declarations = vec![("v1..2.json".to_string(), br#"{"version":1}"#.to_vec())];
    let e = assemble(&bad).expect_err("含 `..` 的派生 entry 名必须在打包阶段就拒");
    assert!(e.is_bad_entry_path(), "必须归到「参数不合规」这一档：{e}");
    let msg = e.to_string();
    assert!(msg.contains("--declarations"), "要点名 flag：{msg}");
    assert!(msg.contains("v1..2.json"), "要点名文件名：{msg}");
    assert!(
        msg.contains("declarations/v1..2.json"),
        "要点名它会变成哪个包内路径：{msg}"
    );
    assert!(msg.contains(".."), "要点明原因：{msg}");

    // `--am` 的第二个文件同样要拒（第一个走规范槽位，永远合法）。
    let mut bad_am = input();
    bad_am.am = vec![
        ("access-mapping.json".to_string(), am_bytes()),
        ("v1..2.json".to_string(), am_bytes()),
    ];
    let e = assemble(&bad_am).expect_err("`--am` 的派生名同样必须拒");
    assert!(e.to_string().contains("--am"), "{e}");

    // 反空转：把双点换成单连字符就应当打得出来（否则这条判据是恒真的）。
    let mut ok = input();
    ok.declarations = vec![("v1-2.json".to_string(), br#"{"version":1}"#.to_vec())];
    assert!(assemble(&ok).is_ok(), "`v1-2.json` 不该被拒");
}

/// **F-04**（真进程层）：报参数错误 + **不产出文件**，而且是在**连库之前**。
///
/// 为什么这条必须是真进程：`bundle_pack` 是 `async` 的 CLI handler，
/// 单元测试够不到「有没有把文件写到盘上」「退出码是几」这两件事。
#[test]
fn pack_rejects_a_dotdot_declaration_name_before_db_and_writes_nothing() {
    let dir = std::env::temp_dir().join(format!("req159-f04-{}", std::process::id()));
    let decl = dir.join("declarations");
    std::fs::create_dir_all(&decl).expect("建临时目录失败");
    std::fs::write(decl.join("v1..2.json"), b"{\"version\":1}").expect("写不出声明文件");
    let out = dir.join("never-written.ontokbundle");
    let _ = std::fs::remove_file(&out);

    let output = Command::new(env!("CARGO_BIN_EXE_ontok-builder"))
        .args(["bundle", "pack", "--project", "cloud-resource", "--version"])
        .arg(SEQ.to_string())
        .arg("--built-at")
        .arg(BUILT_AT)
        .arg("--out")
        .arg(&out)
        .arg("--declarations")
        .arg(&decl)
        // 这条错误与数据库无关：⛔ 不许先撞「连不上库」。
        .env_remove(ontok::config::DATABASE_URL_ENV)
        .env_remove(ontok::config::TEST_DATABASE_URL_ENV)
        .output()
        .expect("跑不起 ontok-builder");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(2),
        "参数不合法 ⇒ 退出码 2（与「包会被拒」同码）；stderr：\n{stderr}"
    );
    assert!(
        !out.exists(),
        "⛔ 这条错误必须在**产出文件之前**报出来（`{}` 被创建了）",
        out.display()
    );
    assert!(stderr.contains("--declarations"), "要点名 flag：\n{stderr}");
    assert!(stderr.contains("v1..2.json"), "要点名文件名：\n{stderr}");
    assert!(
        !stderr.contains("连不上库") && !stderr.contains("ONTOK_DATABASE_URL"),
        "这一档不该牵扯数据库：\n{stderr}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// **F-03**：`manifest.json` 不是第一个 entry ⇒ **只警告**（契约没把顺序写成硬规则）。
#[test]
fn manifest_not_first_is_only_a_warning() {
    let bytes = rebuild(&packed(), |entries| {
        let pos = entries
            .iter()
            .position(|(p, _)| p == "manifest.json")
            .expect("包里没有 manifest.json");
        let m = entries.remove(pos);
        entries.push(m);
    });
    let report = verify(&bytes);
    assert!(
        report.ok,
        "顺序不是硬规则，⛔ 不许因此拒包：{:?}",
        report.problems
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.code == code::MANIFEST_NOT_FIRST),
        "应当给一条「不是第一个」的警告：{:?}",
        report.warnings
    );
    // 反空转：本工具自己的产物恒把 manifest 放第一 ⇒ 不该有这条警告。
    assert!(verify(&packed())
        .warnings
        .iter()
        .all(|w| w.code != code::MANIFEST_NOT_FIRST));
}

/// **F-02**：`"evidence": null` 不是证据 —— 判据是「是数组」，不是「键存在」。
#[test]
fn evidence_null_counts_as_missing() {
    let mut snap: Value = serde_json::from_slice(&serde_json::to_vec(&snapshot()).unwrap()).unwrap();
    snap["link_types"][0]["evidence"] = Value::Null;
    let bytes = replace_file(
        &packed(),
        "ontology/snapshot.json",
        serde_json::to_vec(&snap).unwrap(),
    );
    let report = verify(&bytes);
    assert!(
        report
            .problems
            .iter()
            .any(|p| p.code == code::EVIDENCE_MISSING),
        "`evidence: null` 必须算缺失：{:?}",
        report.problems
    );

    // 空数组是**合法**的（契约：「哪怕空数组」）—— 反空转，别把空数组也拒了。
    let mut empty: Value =
        serde_json::from_slice(&serde_json::to_vec(&snapshot()).unwrap()).unwrap();
    empty["link_types"][0]["evidence"] = json!([]);
    let bytes = replace_file(
        &packed(),
        "ontology/snapshot.json",
        serde_json::to_vec(&empty).unwrap(),
    );
    assert!(
        verify(&bytes).ok,
        "`evidence: []` 必须放行（契约原文「存在（哪怕空数组）」）"
    );
}

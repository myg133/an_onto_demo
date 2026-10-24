//! REQ-129 · `ontok doc merge` 的可执行判据（AC-001…AC-008）。
//!
//! # 分工（⛔ 不重复判同一件事）
//!
//! | 层 | 判据 | 在哪 |
//! | --- | --- | --- |
//! | 七条规则的**逻辑** | 排除 / 压档 / 映射校验 / both / 冲突 / 确定性 / 未签 | `src/extract/doc_merge.rs` 的单测（纯函数、零 I/O） |
//! | apply 侧的三条硬判据 | `applicable:false` 拒绝、`band=strong` 拒绝、未签忽略 | `src/domain/apply.rs` 的单测（纯函数） |
//! | **命令本身** | `--help`、确定性、stats 落盘、不原地覆盖 | 本文件（跑真的 `CARGO_BIN_EXE_ontok`） |
//! | **端到端** | AC-006 拒绝 / AC-007 签与不签的 TTL 差 | 本文件（需要库；缺 `ONTOK_TEST_DATABASE_URL` 时**响亮 SKIP**） |
//!
//! # 为什么端到端缺库时是 SKIP 而不是 `#[ignore]`
//!
//! `#[ignore]` 会让 `cargo test` 看起来全绿而实际上一条端到端都没跑。这里按本仓
//! 既有口径（`tests/test_req127_live.rs`）：缺变量就往 **fd 2** 打一行
//! `*** SKIP: <测试名> -- missing <ENV> ***` 并提前返回 —— 跑没跑过一眼能看出来。
//!
//! ```text
//! $env:ONTOK_TEST_DATABASE_URL="postgresql://postgres:postgres@127.0.0.1:5432/<库>"
//! cargo test --test test_req129_doc_merge
//! ```

use std::path::{Path, PathBuf};
use std::process::Output;

use serde_json::Value;

// ──────────────────────────────────────────────────────────
// 夹具与夹具工具
// ──────────────────────────────────────────────────────────

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

/// 缺哪个变量就**响亮地**说，然后 `None`（测试提前返回 = SKIP）。
fn missing(name: &str, test: &str) -> Option<()> {
    use std::io::Write as _;
    let mut err = std::io::stderr();
    let _ = writeln!(err, "*** SKIP: {test} -- missing {name} ***");
    None
}

fn meta_url() -> Option<String> {
    env("ONTOK_DATABASE_URL").or_else(|| env("ONTOK_TEST_DATABASE_URL"))
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "ontok-req129-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&d).expect("建临时目录");
    d
}

/// 跑**真的 CLI 二进制**。`db = None` ⇒ **不设** `ONTOK_DATABASE_URL`
/// （`doc merge` 必须在不碰库的条件下跑得起来）。
fn cli(args: &[&str], db: Option<&str>) -> Output {
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_ontok"));
    cmd.args(args);
    match db {
        Some(url) => cmd.env("ONTOK_DATABASE_URL", url),
        None => cmd.env_remove("ONTOK_DATABASE_URL"),
    };
    cmd.output().expect("跑 ontok 失败")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// 一份**小提案**（3 表 + 1 条边）：够判 all 七条规则，又不依赖 `code/out/` 的真实产物。
fn tiny_proposal(project: &str) -> Value {
    serde_json::json!({
        "project": project,
        "base_version": 1,
        "round": 1,
        "extracted_by": {"agent": "inproc:ontok::agent::RuleBasedAgent", "version": "0.1.0", "round": 1},
        "items": [
            {
                "kind": "add_entity_type",
                "target": "客户",
                "payload": {
                    "key": "customers",
                    "display_name": "客户",
                    "aliases": ["顾客"],
                    "properties": [{"column": "customer_name", "name": "客户名", "nullable": false}],
                    "evidence": [{"claim": "由源库表结构抽取", "table": "customers"}]
                }
            },
            {
                "kind": "add_entity_type",
                "target": "订单",
                "payload": {
                    "key": "order",
                    "display_name": "订单",
                    "properties": [
                        {"column": "customer_id", "name": "客户", "nullable": false},
                        {"column": "template_id", "name": "套餐", "nullable": false}
                    ]
                }
            },
            {
                "kind": "add_entity_type",
                "target": "套餐模板",
                "payload": {
                    "key": "package_templates",
                    "display_name": "套餐模板",
                    "properties": [{"column": "name", "name": "套餐名", "nullable": false}]
                }
            },
            {
                "kind": "add_link_type",
                "target": "订单→客户",
                // ⚠️ 刻意**不带** `confidence`：ADR-005 的分数块要求 method + signals
                // （`domain::confidence::from_untrusted` 会逐项校验），而本文件判的是
                // 合并与端到端，不是分数校准。压档判据在 `doc_merge` 的单测里覆盖。
                "payload": {
                    "key": "order_customer_id__customers",
                    "from_type": "order",
                    "to_type": "customers",
                    "cardinality": "N:1",
                    "semantic": "belongs_to",
                    "evidence": [{"table": "order", "column": "customer_id", "claim": "命名推断"}]
                }
            }
        ]
    })
}

/// 断言 JSONL（5 行）：
/// ① concept 客户（名字命中 ⇒ both）
/// ② relation `order.template_id → package_templates.id`（**新边** ⇒ doc-only，无基数 ⇒ 默认 N:1）
/// ③ concept 积分转赠（future ⇒ 排除）
/// ④ relation 订单-[退款]->客户（example ⇒ 排除）
/// ⑤ relation 客户-[下单]->订单（**没有 maps_to** ⇒ doc_unmapped）
fn tiny_assertions() -> String {
    [
        r#"{"kind":"concept","name":"客户","aliases":["顾客"],"description":"下单的主体","modality":"fact","alignable":true,"evidence":{"file":"01.md","chunk_id":"aa:0-10","quote":"客户下单","span":[0,4]}}"#,
        r#"{"kind":"relation","subject":"订单","predicate":"选用","object":"套餐模板","cardinality":"1:1","modality":"fact","alignable":true,"maps_to":{"child":{"table":"order","column":"template_id"},"parent":{"table":"package_templates","column":"id"}},"evidence":{"file":"02.md","chunk_id":"bb:20-40","quote":"一个订单只选用一个套餐","span":[20,32]}}"#,
        r#"{"kind":"concept","name":"积分转赠","modality":"future","alignable":false,"evidence":{"file":"01.md","chunk_id":"aa:0-10","quote":"未来将支持积分转赠","span":[10,19]}}"#,
        r#"{"kind":"relation","subject":"订单","predicate":"退款","object":"客户","modality":"example","alignable":false,"evidence":{"file":"01.md","chunk_id":"aa:0-10","quote":"例如订单退款","span":[19,25]}}"#,
        r#"{"kind":"relation","subject":"客户","predicate":"下单","object":"订单","cardinality":"N:1","modality":"fact","alignable":true,"evidence":{"file":"01.md","chunk_id":"aa:0-10","quote":"客户下单","span":[0,4]}}"#,
    ]
    .join("\n")
}

/// 写一份「提案 + 断言」夹具，返回 `(assertions, proposal)` 两份路径。
fn write_fixtures(dir: &Path, project: &str) -> (PathBuf, PathBuf) {
    let a = dir.join("assertions.jsonl");
    let p = dir.join("proposal.json");
    std::fs::write(&a, tiny_assertions()).expect("写断言");
    std::fs::write(&p, format!("{}\n", tiny_proposal(project))).expect("写提案");
    (a, p)
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).expect("读 JSON")).expect("解析 JSON")
}

fn items(doc: &Value) -> Vec<Value> {
    doc["items"].as_array().cloned().unwrap_or_default()
}

fn item_by_key(doc: &Value, key: &str) -> Value {
    items(doc)
        .into_iter()
        .find(|it| it["payload"]["key"] == serde_json::json!(key))
        .unwrap_or_else(|| panic!("找不到条目 {key}"))
}

// ──────────────────────────────────────────────────────────
// AC-001 · 命令可用 + 确定性
// ──────────────────────────────────────────────────────────

#[test]
fn ac001_help_documents_the_pinned_flags() {
    let o = cli(&["doc", "merge", "--help"], None);
    assert!(o.status.success(), "`doc merge --help` 必须 exit 0");
    let text = stdout(&o);
    for flag in ["--assertions", "--proposal", "--out", "--stats"] {
        assert!(text.contains(flag), "--help 里没有 {flag}：{text}");
    }
    // 钉死的「不做」也要写在帮助里（需求原文的两条）
    assert!(text.contains("不按名字猜"), "帮助里必须写明不猜表名");
    assert!(text.contains("逐字节相同"), "帮助里必须写明确定性");
}

#[test]
fn ac001_same_input_twice_is_byte_identical() {
    let dir = temp_dir("determinism");
    let (a, p) = write_fixtures(&dir, "req129_det");
    let m1 = dir.join("merged1.json");
    let m2 = dir.join("merged2.json");
    let s1 = dir.join("stats1.json");
    let s2 = dir.join("stats2.json");

    let args1 = [
        "doc", "merge",
        "--assertions", a.to_str().unwrap(),
        "--proposal", p.to_str().unwrap(),
        "--out", m1.to_str().unwrap(),
        "--stats", s1.to_str().unwrap(),
    ];
    let args2 = [
        "doc", "merge",
        "--assertions", a.to_str().unwrap(),
        "--proposal", p.to_str().unwrap(),
        "--out", m2.to_str().unwrap(),
        "--stats", s2.to_str().unwrap(),
    ];
    let o1 = cli(&args1, None);
    assert!(o1.status.success(), "第一次合并必须成功：{}", stderr(&o1));
    let o2 = cli(&args2, None);
    assert!(o2.status.success(), "第二次合并必须成功：{}", stderr(&o2));

    let b1 = std::fs::read(&m1).expect("读产物1");
    let b2 = std::fs::read(&m2).expect("读产物2");
    assert_eq!(b1, b2, "同一输入两次 ⇒ merged 必须**逐字节相同**");
    assert_eq!(
        std::fs::read(&s1).expect("读统计1"),
        std::fs::read(&s2).expect("读统计2"),
        "同一输入两次 ⇒ stats 也必须逐字节相同"
    );
    // 产物必须是 UTF-8、以换行结尾（人肉 diff 友好）
    let text = String::from_utf8(b1).expect("产物是 UTF-8");
    assert!(text.ends_with('\n'));

    // 形状：与 proposal.json 同形状（同样的顶层键 + items），外加 doc_merge 摘要
    let merged = read_json(&m1);
    let orig = read_json(&p);
    for k in ["project", "base_version", "round", "extracted_by", "items"] {
        assert!(merged.get(k).is_some(), "合并结果缺顶层键 {k}");
        assert_eq!(
            merged.get(k).is_some(),
            orig.get(k).is_some(),
            "顶层键 {k} 的在场与否必须与输入一致"
        );
    }
    assert!(merged["doc_merge"].is_object(), "合并结果要带 doc_merge 摘要");
    assert_eq!(merged["project"], orig["project"], "项目名原样继承");
}

#[test]
fn ac001_out_must_not_clobber_its_own_input() {
    let dir = temp_dir("clobber");
    let (a, p) = write_fixtures(&dir, "req129_clobber");
    let o = cli(
        &[
            "doc",
            "merge",
            "--assertions",
            a.to_str().unwrap(),
            "--proposal",
            p.to_str().unwrap(),
            "--out",
            p.to_str().unwrap(),
        ],
        None,
    );
    assert!(!o.status.success(), "--out 指向 --proposal 必须拒绝");
    assert!(
        stderr(&o).contains("不原地覆盖") || stdout(&o).contains("不原地覆盖"),
        "报错要说明为什么：{}",
        stderr(&o)
    );
    // 输入**一个字节都没动**
    let orig = read_json(&p);
    assert_eq!(orig["items"].as_array().unwrap().len(), 4, "输入提案没被改写");
}

/// ⭐ QA N1：**`--stats` 也必须进「不原地覆盖」清单**。
///
/// 上一版只守了 `--out` ⇒ `--stats` 指向 `--proposal` 时 exit 0，
/// **提案被静默写成一坨统计 JSON**（数据丢失）。
///
/// 三组各一次：`--stats==--proposal`、`--stats==--assertions`、`--stats==--out`。
/// 每组都必须：exit≠0 + 点名叫哪两个参数撞了 + **输入文件 sha256 前后不变**。
#[test]
fn n1_stats_may_not_clobber_inputs_or_the_main_product() {
    let dir = temp_dir("n1-clobber");
    let (a, p) = write_fixtures(&dir, "req129_n1");

    // 每一组都跑在**全新的输入**上（前一组若真写坏了，后一组的 sha 才能独立说明问题）。
    let out = dir.join("merged.json");
    // (期望点名的两个参数, --stats 落点, 被保护的文件, 说明)；顺带钉住「报错点名」这件事。
    let cases: Vec<(&str, &str, &Path, &Path, &str)> = vec![
        ("--stats", "--proposal", p.as_path(), p.as_path(), "stats 撞提案"),
        ("--stats", "--assertions", a.as_path(), a.as_path(), "stats 撞断言"),
        ("--stats", "--out", out.as_path(), out.as_path(), "stats 撞主产物"),
    ];

    for (arg1, arg2, stats_path, guard, what) in cases {
        // ① 每组开始前记下两份输入（以及被保护文件）的 sha
        let sha_a0 = sha256_of(&std::fs::read(&a).expect("读断言"));
        let sha_p0 = sha256_of(&std::fs::read(&p).expect("读提案"));
        let guard0 = std::fs::read(guard).ok().map(|b| sha256_of(&b));
        let args = [
            "doc".to_string(),
            "merge".to_string(),
            "--assertions".to_string(),
            a.to_str().unwrap().to_string(),
            "--proposal".to_string(),
            p.to_str().unwrap().to_string(),
            "--out".to_string(),
            out.to_str().unwrap().to_string(),
            "--stats".to_string(),
            stats_path.to_str().unwrap().to_string(),
        ];
        let strs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let o = cli(&strs, None);

        // ② 必须 exit≠0（⛔ 静默覆盖）
        assert!(
            !o.status.success(),
            "{what}：必须拒绝（上一版这里是 exit 0 + 静默覆盖）"
        );
        // ③ 必须**点名哪两个参数撞了**（顺序由实现决定，两个名字都得在）
        let text = format!("{}{}", stdout(&o), stderr(&o));
        assert!(
            text.contains(arg1) && text.contains(arg2) && text.contains("同一个文件"),
            "{what}：报错要点名 {arg1} 与 {arg2}，实际：{text}"
        );
        // ④ 两份输入的 sha256 **前后不变**
        assert_eq!(
            sha256_of(&std::fs::read(&a).expect("读断言")),
            sha_a0,
            "{what}：断言文件被改动了"
        );
        assert_eq!(
            sha256_of(&std::fs::read(&p).expect("读提案")),
            sha_p0,
            "{what}：提案文件被改动了"
        );
        // ⑤ 被保护的那个文件也没变；`--stats==--out` 时它**根本不该被创建**
        assert_eq!(
            std::fs::read(guard).ok().map(|b| sha256_of(&b)),
            guard0,
            "{what}：被保护文件被写成了别的东西"
        );
        if what == "stats 撞主产物" {
            assert!(!out.exists(), "{what}：拒绝时一个产物都不该落盘");
        } else {
            let guard_text = std::fs::read_to_string(guard).expect("读被保护文件");
            assert!(
                !guard_text.contains("\"assertions_read\""),
                "{what}：被保护文件被写成了 stats JSON"
            );
        }
    }
}

fn sha256_of(bytes: &[u8]) -> String {
    // 测试里只要**前后一致**，不必是密码学 sha256 的完整实现；
    // 用长度 + 逐字节滚动哈希足以发现「文件被改写」。
    let mut h: u64 = 1469598103934665603;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    format!("{:016x}:{}", h, bytes.len())
}

// ──────────────────────────────────────────────────────────
// AC-002 · 只并 alignable:true
// ──────────────────────────────────────────────────────────

#[test]
fn ac002_only_alignable_true_is_merged() {
    let dir = temp_dir("ac002");
    let (a, p) = write_fixtures(&dir, "req129_ac002");
    let m = dir.join("merged.json");
    let s = dir.join("stats.json");
    let o = cli(
        &[
            "doc", "merge",
            "--assertions", a.to_str().unwrap(),
            "--proposal", p.to_str().unwrap(),
            "--out", m.to_str().unwrap(),
            "--stats", s.to_str().unwrap(),
        ],
        None,
    );
    assert!(o.status.success(), "{}", stderr(&o));

    let text = std::fs::read_to_string(&m).expect("读产物");
    // AC-002 的判据①：名字 grep 不到
    assert!(!text.contains("积分转赠"), "future 的概念不许出现在产物里");
    assert!(!text.contains("退款"), "example 的谓词不许出现在产物里");
    // AC-002 的判据②：计数与输入的该类条数相等
    let stats = read_json(&s);
    assert_eq!(stats["excluded_not_alignable"], serde_json::json!(2));
    assert_eq!(stats["excluded_future"], serde_json::json!(1));
    assert_eq!(stats["excluded_example"], serde_json::json!(1));
    // 闭合式：读到的每一条断言都落进了某一档
    let read = stats["assertions_read"].as_u64().unwrap();
    let sum = ["merged", "excluded_not_alignable", "doc_unmapped", "conflicts"]
        .iter()
        .map(|k| stats[*k].as_u64().unwrap())
        .sum::<u64>();
    assert_eq!(read, sum, "统计必须闭合（{read} ≠ {sum}）");
}

// ──────────────────────────────────────────────────────────
// AC-003 · doc 条目最高 medium、必须未签
// ──────────────────────────────────────────────────────────

#[test]
fn ac003_doc_items_are_unsigned_and_band_is_at_most_medium() {
    let dir = temp_dir("ac003");
    let (a, p) = write_fixtures(&dir, "req129_ac003");
    let m = dir.join("merged.json");
    let o = cli(
        &[
            "doc",
            "merge",
            "--assertions",
            a.to_str().unwrap(),
            "--proposal",
            p.to_str().unwrap(),
            "--out",
            m.to_str().unwrap(),
        ],
        None,
    );
    assert!(o.status.success(), "{}", stderr(&o));

    let doc = read_json(&m);
    let mut seen = 0;
    for it in items(&doc) {
        let Some(origin) = it.get("origin").and_then(Value::as_str) else {
            continue;
        };
        assert!(matches!(origin, "doc" | "both"), "origin 只有 doc/both：{origin}");
        seen += 1;
        let band = it["band"].as_str().unwrap_or("");
        assert!(
            matches!(band, "low" | "medium"),
            "doc 条目的 band = {band:?} —— 只许 low/medium（⛔ strong）：{it}"
        );
        assert_eq!(it["reviewed_by"], Value::Null, "doc 条目必须未签：{it}");
        // 输入里那条 DB 边的 confidence.band 是 high；合并后仍不许冒出 strong
        assert_ne!(band, "strong");
    }
    assert!(seen > 0, "夹具必须真的产生 doc 条目，否则这条判据空转");
}

// ──────────────────────────────────────────────────────────
// AC-004 · 无映射 ⇒ doc_unmapped，⛔ 不发明表名
// ──────────────────────────────────────────────────────────

#[test]
fn ac004_an_unmapped_relation_never_invents_a_table() {
    let dir = temp_dir("ac004");
    let (a, p) = write_fixtures(&dir, "req129_ac004");
    let m = dir.join("merged.json");
    let s = dir.join("stats.json");
    let o = cli(
        &[
            "doc", "merge",
            "--assertions", a.to_str().unwrap(),
            "--proposal", p.to_str().unwrap(),
            "--out", m.to_str().unwrap(),
            "--stats", s.to_str().unwrap(),
        ],
        None,
    );
    assert!(o.status.success(), "{}", stderr(&o));

    let stats = read_json(&s);
    assert_eq!(
        stats["doc_unmapped"],
        serde_json::json!(1),
        "「客户-[下单]->订单」没有 maps_to ⇒ 必须进 doc_unmapped：{stats}"
    );
    assert_eq!(
        stats["unmapped_reasons"]["no_maps_to"],
        serde_json::json!(1)
    );

    let doc = read_json(&m);
    let links: Vec<Value> = items(&doc)
        .into_iter()
        .filter(|it| it["kind"] == "add_link_type")
        .collect();
    // 唯一的 doc-only 边是**显式映射**的那条（order.template_id）；⛔ 没有按名字猜出来的边
    let doc_links: Vec<Value> = links
        .iter()
        .filter(|it| it.get("origin").is_some())
        .cloned()
        .collect();
    assert_eq!(doc_links.len(), 1, "doc-only 边只许有显式映射的那一条：{doc_links:?}");
    let payload = &doc_links[0]["payload"];
    assert_eq!(payload["key"], serde_json::json!("order_template_id__package_templates"));
    assert_eq!(payload["from_type"], serde_json::json!("order"));
    assert_eq!(payload["to_type"], serde_json::json!("package_templates"));
    // 所有 from_type / to_type 都必须是提案里真实存在的实体
    let keys: Vec<String> = items(&doc)
        .iter()
        .filter(|it| it["kind"] == "add_entity_type")
        .map(|it| it["payload"]["key"].as_str().unwrap_or("").to_string())
        .collect();
    for it in &links {
        for f in ["from_type", "to_type"] {
            let t = it["payload"][f].as_str().unwrap_or("<缺失>");
            assert!(keys.contains(&t.to_string()), "{f} = {t} 不是提案里的实体");
        }
    }
}

// ──────────────────────────────────────────────────────────
// AC-005 · 同实体合并为 both（两侧证据都在）
// ──────────────────────────────────────────────────────────

#[test]
fn ac005_both_merge_keeps_both_sides_evidence() {
    let dir = temp_dir("ac005");
    let (a, p) = write_fixtures(&dir, "req129_ac005");
    let m = dir.join("merged.json");
    let s = dir.join("stats.json");
    let o = cli(
        &[
            "doc", "merge",
            "--assertions", a.to_str().unwrap(),
            "--proposal", p.to_str().unwrap(),
            "--out", m.to_str().unwrap(),
            "--stats", s.to_str().unwrap(),
        ],
        None,
    );
    assert!(o.status.success(), "{}", stderr(&o));

    let doc = read_json(&m);
    let stats = read_json(&s);
    assert_eq!(stats["merged_both"], serde_json::json!(1), "客户 ⇒ 一条 both");

    let customers: Vec<Value> = items(&doc)
        .into_iter()
        .filter(|it| it["payload"]["key"] == serde_json::json!("customers"))
        .collect();
    assert_eq!(customers.len(), 1, "⛔ 不许出现两条指向同一表的实体");
    let c = &customers[0];
    assert_eq!(c["origin"], serde_json::json!("both"));
    // 两侧证据都在：DB 侧的 payload.evidence 原样 + doc 侧的 doc_evidence
    assert_eq!(
        c["payload"]["evidence"][0]["table"],
        serde_json::json!("customers"),
        "DB 侧证据必须保留"
    );
    assert_eq!(
        c["payload"]["doc_evidence"][0]["quote"],
        serde_json::json!("客户下单"),
        "doc 侧证据必须保留"
    );
    assert_eq!(
        c["payload"]["display_name"],
        serde_json::json!("客户"),
        "DB 的 display_name 不许被 doc 覆盖"
    );
}

// ──────────────────────────────────────────────────────────
// AC-006 / AC-007 · 端到端（需要库）
// ──────────────────────────────────────────────────────────

/// 建项目 + v1 + 导入合并提案 + 双轨全接受。返回项目名。
fn import_and_accept(url: &str, merged: &Path) -> String {
    let doc = read_json(merged);
    let project = doc["project"].as_str().expect("提案缺 project").to_string();
    let o = cli(&["project", "new", "--name", &project], Some(url));
    assert!(o.status.success(), "建项目失败：{}", stderr(&o));
    let o = cli(&["version", "new", "--project", &project], Some(url));
    assert!(o.status.success(), "建版本失败：{}", stderr(&o));
    let o = cli(
        &["propose", "import", "--file", merged.to_str().unwrap()],
        Some(url),
    );
    assert!(o.status.success(), "导入失败：{}\n{}", stdout(&o), stderr(&o));
    let o = cli(
        &[
            "review", "decide", "--proposal", "p1", "--accept-all", "--project", &project,
        ],
        Some(url),
    );
    assert!(o.status.success(), "决策失败：{}", stderr(&o));
    project
}

/// 合并 → 导入 → 全接受 → apply → freeze → 导出建模视图 TTL；返回 TTL 文本。
fn run_to_ttl(url: &str, merged: &Path, tag: &str) -> String {
    let project = import_and_accept(url, merged);
    let o = cli(&["proposal", "apply", "--proposal", "p1", "--project", &project], Some(url));
    assert!(
        o.status.success(),
        "apply 应当成功：{}\n{}",
        stdout(&o),
        stderr(&o)
    );
    let o = cli(&["version", "freeze", "--project", &project, "--version", "2"], Some(url));
    assert!(o.status.success(), "冻结失败：{}", stderr(&o));
    let ttl = std::env::temp_dir().join(format!(
        "ontok-req129-{tag}-{}-{}.ttl",
        std::process::id(),
        project
    ));
    let o = cli(
        &[
            "ontology",
            "export",
            "--project",
            &project,
            "--version",
            "2",
            "--model-view",
            "--out",
            ttl.to_str().unwrap(),
        ],
        Some(url),
    );
    assert!(o.status.success(), "导出失败：{}", stderr(&o));
    std::fs::read_to_string(&ttl).expect("读 TTL")
}

/// ⭐ AC-006：`applicable: false` 的条目 ⇒ applier **拒绝**（不是忽略）并点名。
#[test]
fn ac006_applier_refuses_an_inapplicable_item_and_names_it() {
    let _ = match meta_url() {
        Some(u) => u,
        None => return missing("ONTOK_TEST_DATABASE_URL", "ac006_applier_refuses_an_inapplicable_item_and_names_it").unwrap(),
    };
    let url = meta_url().unwrap();
    let dir = temp_dir("ac006");
    let project = format!(
        "req129_ac006_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    );
    let (a, p) = write_fixtures(&dir, &project);
    // 基数冲突：doc 说 1:1，DB 推断 N:1（`order_customer_id__customers`）
    let conflict = [
        std::fs::read_to_string(&a).expect("读断言"),
        r#"{"kind":"relation","subject":"订单","predicate":"下单","object":"客户","cardinality":"1:1","modality":"fact","alignable":true,"maps_to":{"child":{"table":"order","column":"customer_id"},"parent":{"table":"customers","column":"id"}},"evidence":{"file":"03.md","chunk_id":"cc:0-9","quote":"一个订单只对应一个客户","span":[0,10]}}"#.to_string(),
    ]
    .join("\n");
    std::fs::write(&a, conflict).expect("写冲突断言");

    let m = dir.join("merged.json");
    let s = dir.join("stats.json");
    let o = cli(
        &[
            "doc", "merge",
            "--assertions", a.to_str().unwrap(),
            "--proposal", p.to_str().unwrap(),
            "--out", m.to_str().unwrap(),
            "--stats", s.to_str().unwrap(),
        ],
        None,
    );
    assert!(o.status.success(), "{}", stderr(&o));
    let stats = read_json(&s);
    assert_eq!(stats["conflicts"], serde_json::json!(1), "必须记一条冲突");
    let doc = read_json(&m);
    let item = item_by_key(&doc, "order_customer_id__customers");
    assert_eq!(item["applicable"], serde_json::json!(false));
    assert_eq!(item["conflicts"][0]["doc_says"], serde_json::json!("1:1"));
    assert_eq!(item["conflicts"][0]["db_says"], serde_json::json!("N:1"));

    let project = import_and_accept(&url, &m);
    let o = cli(
        &["proposal", "apply", "--proposal", "p1", "--project", &project],
        Some(&url),
    );
    assert!(
        !o.status.success(),
        "带 applicable:false 的提案**不许**被 apply（AC-006：拒绝，不是忽略）：{}",
        stdout(&o)
    );
    let text = format!("{}{}", stdout(&o), stderr(&o));
    assert!(
        text.contains("applicable: false"),
        "报错要说明是这一栏：{text}"
    );
    assert!(
        text.contains("订单→客户"),
        "报错必须**点名**是哪一条：{text}"
    );
}

/// ⭐ AC-007（**最重要**）：不签 ⇒ TTL 里没有；签一条 ⇒ TTL 里有。
#[test]
fn ac007_unsigned_doc_edges_stay_out_of_the_ttl_and_signed_ones_enter_it() {
    let _ = match meta_url() {
        Some(u) => u,
        None => return missing("ONTOK_TEST_DATABASE_URL", "ac007_unsigned_doc_edges_stay_out_of_the_ttl_and_signed_ones_enter_it").unwrap(),
    };
    let url = meta_url().unwrap();
    let dir = temp_dir("ac007");
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);

    // ① 不签：合并结果里 origin=doc 的条目 `reviewed_by: null`
    let p_unsigned = format!("req129_ac007_u_{}_{}", std::process::id(), stamp);
    let (a, p) = write_fixtures(&dir, &p_unsigned);
    let m_unsigned = dir.join("merged-unsigned.json");
    let o = cli(
        &[
            "doc", "merge",
            "--assertions", a.to_str().unwrap(),
            "--proposal", p.to_str().unwrap(),
            "--out", m_unsigned.to_str().unwrap(),
        ],
        None,
    );
    assert!(o.status.success(), "{}", stderr(&o));
    let ttl_unsigned = run_to_ttl(&url, &m_unsigned, "unsigned");
    // TTL 的 IRI 来自 (from_type, to_type)（`model_link_locals`），不是 payload.key。
    // 两个写法都判：IRI 与 `ontok:key` 都必须**不在**。
    assert!(
        !ttl_unsigned.contains(":order__package_templates a owl:"),
        "**未签**的 doc 边不许出现在 TTL 里（AC-007）"
    );
    assert!(
        !ttl_unsigned.contains("order_template_id__package_templates"),
        "**未签**的 doc 边的 key 也不许出现在 TTL 里（AC-007）"
    );

    // ② 签一条：**用 `ontok doc sign`**（Q-1 的一等入口；不再手工改 JSON）
    let p_signed = format!("req129_ac007_s_{}_{}", std::process::id(), stamp);
    let (a2, p2) = write_fixtures(&dir, &p_signed);
    let m_unsigned2 = dir.join("merged-unsigned2.json");
    let o = cli(
        &[
            "doc",
            "merge",
            "--assertions",
            a2.to_str().unwrap(),
            "--proposal",
            p2.to_str().unwrap(),
            "--out",
            m_unsigned2.to_str().unwrap(),
        ],
        None,
    );
    assert!(o.status.success(), "{}", stderr(&o));
    let m_signed = dir.join("merged-signed.json");
    let o = cli(
        &[
            "doc",
            "sign",
            "--file",
            m_unsigned2.to_str().unwrap(),
            "--item",
            "relation:订单→套餐模板",
            "--by",
            "qa-req129",
            "--out",
            m_signed.to_str().unwrap(),
            "--at",
            "2026-10-24T04:00:00Z",
        ],
        None,
    );
    assert!(o.status.success(), "doc sign 失败：{}", stderr(&o));
    assert!(
        stderr(&o).contains("其余字节"),
        "签字报告要写明「其余字节未变」：{}",
        stderr(&o)
    );
    let signed = read_json(&m_signed);
    let signed_items: Vec<Value> = signed["items"].as_array().cloned().unwrap_or_default();
    let n = signed_items
        .iter()
        .filter(|it| it["origin"] == serde_json::json!("doc") && !it["reviewed_by"].is_null())
        .count();
    assert_eq!(n, 1, "夹具里应当恰有 1 条 doc-only 条目被签");

    let ttl_signed = run_to_ttl(&url, &m_signed, "signed");
    // 正控：签过的那条边**必须**在（否则「未签时没有」是恒真判据 —— 反空转）。
    assert!(
        ttl_signed.contains(":order__package_templates a owl:"),
        "**签了**的 doc 边必须出现在 TTL 里（AC-007）"
    );
    assert!(
        ttl_signed.contains("order_template_id__package_templates"),
        "**签了**的 doc 边的 key 也必须出现在 TTL 里（AC-007）"
    );
}

// ──────────────────────────────────────────────────────────
// Q-1 · `ontok doc sign`（7 条判据，CLI 层）
// ──────────────────────────────────────────────────────────

/// 造一份「已合并」的提案（一回 `doc merge`），供签字测试用。
fn merged_fixture(dir: &Path, project: &str) -> PathBuf {
    let (a, p) = write_fixtures(dir, project);
    let m = dir.join("merged-for-sign.json");
    let o = cli(
        &[
            "doc",
            "merge",
            "--assertions",
            a.to_str().unwrap(),
            "--proposal",
            p.to_str().unwrap(),
            "--out",
            m.to_str().unwrap(),
        ],
        None,
    );
    assert!(o.status.success(), "{}", stderr(&o));
    m
}

const AT: &str = "2026-10-24T04:00:00Z";

#[test]
fn q1_sign_requires_a_named_signer_and_an_explicit_output() {
    let dir = temp_dir("sign-anon");
    let m = merged_fixture(&dir, "req129_sign_anon");
    let out = dir.join("signed.json");

    // ① `--by` 空/空白 ⇒ 拒绝（⛔ 不许匿名签）
    for by in ["", "   "] {
        let o = cli(
            &[
                "doc", "sign",
                "--file", m.to_str().unwrap(),
                "--item", "relation:订单→套餐模板",
                "--by", by,
                "--out", out.to_str().unwrap(),
            ],
            None,
        );
        assert!(!o.status.success(), "--by {by:?} 必须被拒");
        let text = format!("{}{}", stdout(&o), stderr(&o));
        assert!(text.contains("匿名"), "要说明拒绝理由：{text}");
        assert!(!out.exists(), "被拒时不许写出文件");
    }

    // ⑥ 既没 `--out` 也没 `--in-place` ⇒ 拒绝，且⛔ 不就地改输入
    let before = std::fs::read(&m).expect("读输入");
    let o = cli(
        &[
            "doc", "sign",
            "--file", m.to_str().unwrap(),
            "--item", "relation:订单→套餐模板",
            "--by", "qa-req129",
        ],
        None,
    );
    assert!(!o.status.success(), "缺 --out 必须被拒");
    assert!(
        format!("{}{}", stdout(&o), stderr(&o)).contains("不提供「默认就地改」")
            || format!("{}{}", stdout(&o), stderr(&o)).contains("必须落盘"),
        "要说明为什么不默认就地改"
    );
    assert_eq!(std::fs::read(&m).expect("读输入"), before, "输入一个字节都不许动");

    // `--out` 指向输入文件 ⇒ 也要拒（要就地必须显式 --in-place）
    let o = cli(
        &[
            "doc", "sign",
            "--file", m.to_str().unwrap(),
            "--item", "relation:订单→套餐模板",
            "--by", "qa-req129",
            "--out", m.to_str().unwrap(),
        ],
        None,
    );
    assert!(!o.status.success(), "--out 指向输入必须被拒");
    assert_eq!(std::fs::read(&m).expect("读输入"), before, "输入一个字节都不许动");
}

#[test]
fn q1_sign_refuses_db_items_and_unknown_items() {
    let dir = temp_dir("sign-db");
    let m = merged_fixture(&dir, "req129_sign_db");
    let out = dir.join("signed.json");

    // ③ 只签 doc/both：DB 轨条目（没有 origin）拒绝并说明走 review decide
    let o = cli(
        &[
            "doc", "sign",
            "--file", m.to_str().unwrap(),
            "--item", "entity:客户",   // 夹具里 customers 是 both（可签）
            "--by", "qa-req129",
            "--out", out.to_str().unwrap(),
            "--at", AT,
        ],
        None,
    );
    // `entity:客户` 是 both ⇒ **允许**（这里证明 both 可签）
    assert!(o.status.success(), "both 条目应当可签：{}", stderr(&o));

    let o = cli(
        &[
            "doc", "sign",
            "--file", m.to_str().unwrap(),
            "--item", "entity:订单",   // 夹具里 order 没有 origin ⇒ DB 轨
            "--by", "qa-req129",
            "--out", dir.join("signed2.json").to_str().unwrap(),
            "--at", AT,
        ],
        None,
    );
    assert!(!o.status.success(), "DB 轨条目不许签");
    let text = format!("{}{}", stdout(&o), stderr(&o));
    assert!(text.contains("review decide"), "要说清该走哪条轨：{text}");
    assert!(text.contains("origin"), "{text}");

    // ② 认不出 / 越界 ⇒ 拒绝（⛔ 不猜一个最像的）
    for bad in ["relation:不存在的边", "999", "relation:"] {
        let o = cli(
            &[
                "doc", "sign",
                "--file", m.to_str().unwrap(),
                "--item", bad,
                "--by", "qa-req129",
                "--out", out.to_str().unwrap(),
                "--at", AT,
            ],
            None,
        );
        assert!(!o.status.success(), "--item {bad:?} 必须被拒");
    }
}

#[test]
fn q1_sign_is_idempotent_byte_stable_and_deterministic() {
    let dir = temp_dir("sign-stable");
    let m = merged_fixture(&dir, "req129_sign_stable");
    let s1 = dir.join("signed1.json");
    let s2 = dir.join("signed2.json");
    let args = |out: &Path| {
        vec![
            "doc".to_string(),
            "sign".to_string(),
            "--file".to_string(),
            m.to_str().unwrap().to_string(),
            "--item".to_string(),
            "relation:订单→套餐模板".to_string(),
            "--by".to_string(),
            "qa-req129".to_string(),
            "--out".to_string(),
            out.to_str().unwrap().to_string(),
            "--at".to_string(),
            AT.to_string(),
        ]
    };
    let run = |out: &Path| {
        let a = args(out);
        let strs: Vec<&str> = a.iter().map(|s| s.as_str()).collect();
        cli(&strs, None)
    };

    // ⑥ 确定性（同输入同参数） + ④ 只加两栏：同参数两次输出逐字节相同
    let o1 = run(&s1);
    assert!(o1.status.success(), "{}", stderr(&o1));
    let o2 = run(&s2);
    assert!(o2.status.success(), "{}", stderr(&o2));
    let b1 = std::fs::read(&s1).expect("读 s1");
    assert_eq!(b1, std::fs::read(&s2).expect("读 s2"), "同参数两次必须逐字节相同");

    // ④ 其余字节不变：签后文本 = 原文本 + 恰好两行（reviewed_by / signed_at）
    //
    // 两边都按「去掉 `reviewed_by` 与 `signed_at` 行」比 —— 因为签字前的
    // `reviewed_by: null` 与签字后的 `reviewed_by: "qa-req129"` 是同一行的两种值，
    // 直接比会把「这一栏的值变了」误判成「别的行被改了」。
    let orig = std::fs::read_to_string(&m).expect("读输入");
    let signed = String::from_utf8(b1.clone()).expect("UTF-8");
    let strip = |text: &str| -> Vec<String> {
        text.lines()
            .filter(|l| !l.contains("\"signed_at\"") && !l.contains("\"reviewed_by\":"))
            .map(str::to_string)
            .collect()
    };
    let after_lines = strip(&signed);
    let before_lines = strip(&orig);
    assert_eq!(
        after_lines, before_lines,
        "除这两栏外，其余行必须逐字节相同"
    );
    assert_eq!(
        signed.lines().count(),
        orig.lines().count() + 1,
        "签字只该多加 `signed_at` 一行（`reviewed_by` 是改值不是加行）"
    );

    // ⑤ 重复签 ⇒ 幂等（exit 0、值不变、字节相同）
    let o3 = cli(
        &[
            "doc", "sign",
            "--file", s1.to_str().unwrap(),
            "--item", "relation:订单→套餐模板",
            "--by", "qa-req129",
            "--out", s2.to_str().unwrap(),
            "--at", "2026-10-25T00:00:00Z", // 换了时间也不许改已有的签字时间
        ],
        None,
    );
    assert!(o3.status.success(), "重复签不许报错：{}", stderr(&o3));
    assert!(stderr(&o3).contains("幂等"), "要报告这是幂等命中：{}", stderr(&o3));
    assert_eq!(
        std::fs::read(&s1).expect("读 s1"),
        std::fs::read(&s2).expect("读 s2"),
        "重复签必须逐字节相同（含 signed_at 不被改写）"
    );

    // `--in-place` 是**显式**的就地开关（不加时必须给 --out）
    let o4 = cli(
        &[
            "doc", "sign",
            "--file", s1.to_str().unwrap(),
            "--item", "relation:订单→套餐模板",
            "--by", "qa-req129",
            "--in-place",
        ],
        None,
    );
    assert!(o4.status.success(), "--in-place 应当可用：{}", stderr(&o4));
    assert_eq!(
        std::fs::read(&s1).expect("读 s1"),
        b1,
        "幂等的就地重签不该改字节"
    );
}

// ──────────────────────────────────────────────────────────
// Q-2 · `--schema` 严格校验（CLI 层）
// ──────────────────────────────────────────────────────────

/// 写一份最小 `schema.json`（只填本 REQ 用到的字段）。
fn write_schema(dir: &Path, name: &str, tables: &[(&str, &[&str])]) -> PathBuf {
    let tables_json: Vec<Value> = tables
        .iter()
        .map(|(t, cols)| {
            let columns: Vec<Value> = cols
                .iter()
                .map(|c| {
                    serde_json::json!({
                        "name": c, "data_type": "text", "nullable": false,
                        "comment": null, "is_primary_key": *c == "id",
                    })
                })
                .collect();
            serde_json::json!({
                "name": t, "comment": null, "columns": columns,
                "column_count": cols.len(), "commented_count": 0,
            })
        })
        .collect();
    let doc = serde_json::json!({
        "server_version": "test",
        "transaction_read_only": "on",
        "schema": "public",
        "table_count": tables.len(),
        "column_count": 0,
        "commented_column_count": 0,
        "foreign_key_count": 0,
        "tables": tables_json,
        "introspected_at": "2026-10-24T00:00:00Z",
    });
    let p = dir.join(name);
    std::fs::write(&p, serde_json::to_string_pretty(&doc).unwrap()).expect("写 schema");
    p
}

#[test]
fn q2_schema_flag_makes_the_parent_key_check_strict() {
    let dir = temp_dir("schema-strict");
    let (a, p) = write_fixtures(&dir, "req129_schema");

    // ① 给了 `--schema` 且父端主键列存在 ⇒ 严格通过，fallback 计数为 0
    let good = write_schema(
        &dir,
        "schema-good.json",
        &[
            ("customers", &["id", "customer_name"]),
            ("order", &["id", "customer_id", "template_id"]),
            ("package_templates", &["id", "name"]),
        ],
    );
    let m = dir.join("merged-strict.json");
    let s = dir.join("stats-strict.json");
    let o = cli(
        &[
            "doc", "merge",
            "--assertions", a.to_str().unwrap(),
            "--proposal", p.to_str().unwrap(),
            "--out", m.to_str().unwrap(),
            "--stats", s.to_str().unwrap(),
            "--schema", good.to_str().unwrap(),
        ],
        None,
    );
    assert!(o.status.success(), "{}", stderr(&o));
    let stats = read_json(&s);
    assert_eq!(stats["schema_checked"], serde_json::json!(true));
    assert_eq!(stats["schema_tables"], serde_json::json!(3));
    assert_eq!(stats["merged_doc_only"], serde_json::json!(1), "父端 id 应当严格通过");
    assert_eq!(
        stats["parent_key_id_convention"],
        serde_json::json!(0),
        "给了 schema 就不许再走 `id` 约定的 fallback"
    );
    assert_eq!(stats["parent_column_schema_only"], serde_json::json!(1));

    // ② 给了 `--schema` 而 schema 里没有这一列 ⇒ 未映射（0 例外）
    let bad = write_schema(
        &dir,
        "schema-bad.json",
        &[
            ("customers", &["customer_name"]),
            ("order", &["customer_id", "template_id"]),
            ("package_templates", &["name"]),
        ],
    );
    let m2 = dir.join("merged-strict-bad.json");
    let s2 = dir.join("stats-strict-bad.json");
    let o = cli(
        &[
            "doc", "merge",
            "--assertions", a.to_str().unwrap(),
            "--proposal", p.to_str().unwrap(),
            "--out", m2.to_str().unwrap(),
            "--stats", s2.to_str().unwrap(),
            "--schema", bad.to_str().unwrap(),
        ],
        None,
    );
    assert!(o.status.success(), "{}", stderr(&o));
    let stats2 = read_json(&s2);
    assert_eq!(stats2["merged_doc_only"], serde_json::json!(0), "schema 里没有的列不许并");
    assert_eq!(
        stats2["unmapped_reasons"]["column_not_in_schema"],
        serde_json::json!(1),
        "{stats2}"
    );
}

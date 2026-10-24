//! REQ-129 · `ontok doc merge` 的可执行判据（AC-001…AC-008）。
//!
//! REQ-130 收尾了两条：
//! - **N-3**：AC-002 的判据从「整文件 grep」改成「**按条目身份匹配**」，
//!   并补了反向判据（[`ac002_reverse_injected_excluded_entry_turns_the_criterion_red`]）。
//! - **N-2**：`doc sign` 多写一栏 `signed_at_source`（`arg` / `system`），
//!   老产物缺这一栏时按缺省处理（见 AC-003 / AC-004 两节）。
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
//! 统一口径（REQ-131 · `tests/common/skip.rs`）：缺变量就往 **fd 2** 打一行
//! `*** SKIP: <测试名> -- 缺 <夹具> -- 这条判据没有执行，不是通过 ***` 并**正常返回**
//! —— 跑没跑过一眼能看出来（`Select-String '\*\*\* SKIP:'` 能数条数）。
//!
//! ⚠️ **CI 用 `ONTOK_REQUIRE_FIXTURES=1`**：同样的缺失 ⇒ 用例**失败**
//! （「CI 要求夹具必须存在」），跳过就混不过去。
//!
//! ```text
//! $env:ONTOK_TEST_DATABASE_URL="postgresql://postgres:postgres@127.0.0.1:5432/<库>"
//! cargo test --test test_req129_doc_merge
//!
//! # CI / 想强制「夹具必须存在」时
//! $env:ONTOK_REQUIRE_FIXTURES="1"; cargo test --test test_req129_doc_merge
//! ```

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Output;

use serde_json::{json, Value};

mod common;

// ──────────────────────────────────────────────────────────
// 夹具与夹具工具
// ──────────────────────────────────────────────────────────

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
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
//
// ⭐ REQ-130 · N-3：这条判据原来写成「在 merged.json 里 grep 不到这些名字」，
// 那是**假阳性**：被排除的词会出现在**别的已并入条目**的 `evidence.quote` /
// `payload.doc_evidence[].quote` 里 —— 那是**原文片段**，不是条目。
// 真实 fixture 里「积分转赠」就出现在已并入条目 `客户` 的引文里。
//
// 所以判据改成**按条目身份**匹配（[`excluded_entries_in`]），并且分工如下：
//
// | 判据 | 抓什么 | 在哪 |
// | --- | --- | --- |
// | 按条目身份（本文件） | 被排除的概念/关系**变成了一个条目** | [`excluded_entries_in`] |
// | `excluded_not_alignable` 计数 | 被排除的断言**被当成主张并进了已有条目**（不新增条目，只加 doc_evidence） | `stats.json` |
//
// 两条缺一不可：只有身份匹配会漏掉「并进已有条目」；只有计数会漏掉
// 「计数没算错、但条目还是被塞进去了」。⛔ 整文件 grep 两条都不是。
// ──────────────────────────────────────────────────────────

/// N-3 · **被排除断言的「身份」**（不是「词」）。
///
/// ⭐ REQ-130 QA：判据**不许假红**，所以不能按词、更不能按 `contains` 判。
/// 反例是合法的：`套餐退款→客户`（`semantic = 退款`）是一条**合法边** ——
/// 端点 `套餐退款` 是「退款」的**超串**，谓词又恰好等于被排除谓词。
/// 按词判会把它判成「被排除的断言并进来了」，这与在 `evidence.quote` 里 grep 到
/// 「退款」是**同一类错**：假阳性只是从引文搬到了端点。
///
/// 所以判据比的是**身份**，不是词：
///
/// | 断言 | 身份 | 判法 |
/// | --- | --- | --- |
/// | concept | 名字 | 整值**相等** |
/// | relation | `(subject, predicate, object)` 三元组 | 三项**整值相等**（方向可反） |
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum ExcludedIdentity {
    Concept(String),
    Relation {
        subject: String,
        predicate: String,
        object: String,
    },
}

/// N-3 · **按条目身份**判「被排除的断言有没有变成条目」（⛔ 不是整文件 grep）。
///
/// 只读条目的**身份**字段，⛔ 不读任何 `evidence` / `doc_evidence` / `claim`：
/// 引文是原文片段，里面提到被排除的词是正常的。条目身份是：
///
/// - `add_entity_type`（概念）：`target` / `payload.display_name` / `payload.key` —— 与
///   被排除**概念名**比**相等**；
/// - `add_link_type`（关系）：
///   - 两端（`target` 的 `A` / `B`、`payload.from_type` / `to_type`）与**概念名**比**相等**；
///   - `payload.semantic`（谓词）与被排除关系的 `predicate` 比**相等**，且两端与被排除
///     关系的 `subject` / `object` **整值相等**（三元组身份，方向可反）。
///
/// 返回**违规描述**（空 = 绿）。红了要能点名第几条、哪个字段 —— 否则人只能看到一坨红。
fn excluded_entries_in(doc: &Value, excluded: &[ExcludedIdentity]) -> Vec<String> {
    let mut leaks: Vec<String> = Vec::new();
    for (i, it) in items(doc).iter().enumerate() {
        let no = i + 1;
        match it["kind"].as_str().unwrap_or("") {
            "add_entity_type" => {
                let names = [
                    ("target", it["target"].as_str().unwrap_or("")),
                    (
                        "payload.display_name",
                        it["payload"]["display_name"].as_str().unwrap_or(""),
                    ),
                    ("payload.key", it["payload"]["key"].as_str().unwrap_or("")),
                ];
                for ex in excluded {
                    let ExcludedIdentity::Concept(name) = ex else {
                        continue;
                    };
                    for (field, v) in names {
                        // 概念是「名字」：整值相等，`客户` 不会命中 `客户经理`。
                        if !v.is_empty() && v == name {
                            leaks.push(format!(
                                "第 {no} 条：concept 的 {field} = {v:?} —— 被排除的概念「{name}」成了一个条目"
                            ));
                        }
                    }
                }
            }
            "add_link_type" => {
                // 两端：target 写成 `A→B`（也接受 `->`）
                let normalized = it["target"].as_str().unwrap_or("").replace("->", "→");
                let halves: Vec<&str> = normalized
                    .split('→')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .collect();
                let semantic = it["payload"]["semantic"].as_str().unwrap_or("");
                // 概念身份用**相等**判：展示名两端 + 表 key（key 是表名，命中不了中文概念名）
                let mut endpoints: Vec<(&str, &str)> =
                    halves.iter().map(|h| ("target 的一端", *h)).collect();
                for f in ["from_type", "to_type"] {
                    if let Some(v) = it["payload"][f].as_str() {
                        endpoints.push((f, v));
                    }
                }
                for ex in excluded {
                    match ex {
                        ExcludedIdentity::Concept(name) => {
                            for (field, v) in &endpoints {
                                if v == name {
                                    leaks.push(format!(
                                        "第 {no} 条：relation 的 {field} = {v:?} —— 被排除的概念「{name}」出现在关系的一端"
                                    ));
                                }
                            }
                        }
                        ExcludedIdentity::Relation {
                            subject,
                            predicate,
                            object,
                        } => {
                            // 谓词**整值**相等（`退款` ≠ `退款成功`），不是 contains。
                            if semantic != predicate || halves.len() != 2 {
                                continue;
                            }
                            let (left, right) = (halves[0], halves[1]);
                            // 推导方向（child→parent）与断言方向（subject→object）可以相反。
                            if (left == subject && right == object)
                                || (left == object && right == subject)
                            {
                                leaks.push(format!(
                                    "第 {no} 条：relation 身份 = ({left:?}, {semantic:?}, {right:?}) \
                                     与被排除断言 ({subject:?}, {predicate:?}, {object:?}) 相同 \
                                     —— 被排除的关系成了一个条目"
                                ));
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    leaks
}

/// 从断言 JSONL 里推出「被排除的**身份**」（只收 `alignable:false` 的断言）。
///
/// 概念收 `name`；关系收完整三元组 —— ⛔ 不收单个词：拿 `predicate` 当词用，
/// 合法的 `套餐退款→客户`（`semantic=退款`）就会被误判（假红）。
fn excluded_identities(jsonl: &str) -> Vec<ExcludedIdentity> {
    let mut set: BTreeSet<ExcludedIdentity> = BTreeSet::new();
    for (ln, raw) in jsonl.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let v: Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("断言 JSONL 第 {} 行不是合法 JSON：{e}", ln + 1));
        if v["alignable"].as_bool() != Some(false) {
            continue;
        }
        match v["kind"].as_str().unwrap_or("") {
            "concept" => {
                if let Some(n) = v["name"].as_str().map(str::trim).filter(|s| !s.is_empty()) {
                    set.insert(ExcludedIdentity::Concept(n.to_string()));
                }
            }
            "relation" => {
                let s = v["subject"].as_str().unwrap_or("").trim();
                let p = v["predicate"].as_str().unwrap_or("").trim();
                let o = v["object"].as_str().unwrap_or("").trim();
                // 三元组缺一项就不是可判的身份（这种夹具本身该修，⛔ 不猜）。
                if !s.is_empty() && !p.is_empty() && !o.is_empty() {
                    set.insert(ExcludedIdentity::Relation {
                        subject: s.to_string(),
                        predicate: p.to_string(),
                        object: o.to_string(),
                    });
                }
            }
            _ => {}
        }
    }
    set.into_iter().collect()
}

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

    // AC-002 的判据①（REQ-130 改法）：**按条目身份**判，⛔ 不是整文件 grep
    let doc = read_json(&m);
    let excluded = excluded_identities(&std::fs::read_to_string(&a).expect("读断言"));
    assert_eq!(
        excluded,
        vec![
            ExcludedIdentity::Concept("积分转赠".to_string()),
            ExcludedIdentity::Relation {
                subject: "订单".to_string(),
                predicate: "退款".to_string(),
                object: "客户".to_string(),
            },
        ],
        "夹具里被排除的身份：1 个概念 + 1 条关系三元组"
    );
    let leaks = excluded_entries_in(&doc, &excluded);
    assert!(
        leaks.is_empty(),
        "future 的概念 / example 的关系都不许成为条目：{leaks:#?}"
    );
    // AC-002 的判据②：计数与输入的该类条数相等
    let stats = read_json(&s);
    assert_eq!(stats["excluded_not_alignable"], json!(2));
    assert_eq!(stats["excluded_future"], json!(1));
    assert_eq!(stats["excluded_example"], json!(1));
    // 闭合式：读到的每一条断言都落进了某一档
    let read = stats["assertions_read"].as_u64().unwrap();
    let sum = ["merged", "excluded_not_alignable", "doc_unmapped", "conflicts"]
        .iter()
        .map(|k| stats[*k].as_u64().unwrap())
        .sum::<u64>();
    assert_eq!(read, sum, "统计必须闭合（{read} ≠ {sum}）");
}

/// ⭐ REQ-130 · AC-002 的**反向判据**（「判据有牙」的唯一证明）。
///
/// 两半，缺一不可：
///
/// ① **假阳性底料**：往**已经并入**的条目（`客户`）的 `payload.doc_evidence[].quote`
///    里塞一句提到「积分转赠」的原文 ⇒ 按条目判据必须**绿**。
///    真实 fixture 里就是这样（`客户` 的引文：「平台计划在下一财年支持积分转赠」）。
///    把判据改回整文件 grep ⇒ 这一半立刻红 —— 这就是 N-3 说的假阳性。
/// ② **塞条目**：手工往 `items[]` 塞一条 `alignable:false` 的 concept 条目 / 一条
///    端点**等于**被排除概念的边 / 一条**身份三元组与被排除关系相同**的边 ⇒
///    判据必须**红**，且点名。把判据改松（只看某个顶层键 / 只数个数）⇒ 这一半红。
///
/// 与之**成对**的是 [`ac002_a_legal_superstring_edge_is_not_a_leak`]：只做红会过严
/// （`套餐退款→客户` 是合法边），只做绿会放水。
///
/// 只有①没有② = 判据被改松了没人知道；只有②没有① = 改回 grep 也能绿。
#[test]
fn ac002_reverse_injected_excluded_entry_turns_the_criterion_red() {
    let dir = temp_dir("ac002-reverse");
    let (a, p) = write_fixtures(&dir, "req129_ac002_rev");
    let m = dir.join("merged.json");
    let o = cli(
        &[
            "doc", "merge",
            "--assertions", a.to_str().unwrap(),
            "--proposal", p.to_str().unwrap(),
            "--out", m.to_str().unwrap(),
        ],
        None,
    );
    assert!(o.status.success(), "{}", stderr(&o));

    let excluded = excluded_identities(&std::fs::read_to_string(&a).expect("读断言"));
    assert_eq!(
        excluded.len(),
        2,
        "夹具里应当有 1 个被排除概念 + 1 条被排除关系：{excluded:?}"
    );
    let base = items(&read_json(&m));
    assert!(!base.is_empty(), "夹具必须真的产出条目，否则这条判据空转");

    // ── ① 假阳性底料：引文里出现被排除词 ⇒ 不许判成「并入」 ──
    let mut bait = base.clone();
    let victim = bait
        .iter_mut()
        .find(|it| it["payload"]["key"] == json!("customers"))
        .expect("夹具里必须有 customers 条目（both）");
    victim["payload"]["doc_evidence"]
        .as_array_mut()
        .expect("both 条目必须有 doc_evidence")
        .push(json!({
            "kind": "concept",
            "name": "客户",
            "quote": "## 未来规划\n\n平台计划在下一财年支持积分转赠",
            "file": "03-points.md",
            "chunk_id": "aa:367-429",
            "span": [367, 429]
        }));
    let mut bait_doc = read_json(&m);
    bait_doc["items"] = Value::Array(bait);
    let leaks = excluded_entries_in(&bait_doc, &excluded);
    assert!(
        leaks.is_empty(),
        "引文里提到「积分转赠」≠ 条目被并入（这正是 N-3 的假阳性）：{leaks:#?}"
    );
    // ①b 同一份文档，**整文件 grep 会红** —— 证明这一半真的能抓住「判据改回 grep」
    assert!(
        serde_json::to_string(&bait_doc).unwrap().contains("积分转赠"),
        "底料必须能被整文件搜索命中，否则它证明不了 grep 会假红"
    );

    // ── ② 塞一条 `alignable:false` 的**概念条目** ⇒ 必须红 ──
    let mut injected = base.clone();
    injected.push(json!({
        "kind": "add_entity_type",
        "target": "积分转赠",
        "alignable": false,
        "payload": {"key": "points_transfer", "display_name": "积分转赠"}
    }));
    let mut red = read_json(&m);
    red["items"] = Value::Array(injected);
    let leaks = excluded_entries_in(&red, &excluded);
    let expect_seq = format!("第 {} 条", base.len() + 1);
    assert!(
        !leaks.is_empty(),
        "塞一条被排除的概念条目，判据必须红（⛔ 只改判据不补反向 = 没验证）"
    );
    assert!(
        leaks.iter().all(|l| l.contains("积分转赠") && l.contains(&expect_seq)),
        "报错必须点名是哪一条、哪个字段，实际：{leaks:#?}"
    );

    // ── ②b 关系的**端点等于**被排除概念 ⇒ 必须红 ──
    let mut link_leak = base.clone();
    link_leak.push(json!({
        "kind": "add_link_type",
        "target": "客户→积分转赠",
        "payload": {
            "key": "customers_points_transfer",
            "from_type": "customers",
            "to_type": "points_transfer",
            "semantic": "转赠"
        }
    }));
    let mut red = read_json(&m);
    red["items"] = Value::Array(link_leak);
    let leaks = excluded_entries_in(&red, &excluded);
    assert!(
        !leaks.is_empty(),
        "端点等于被排除概念的 relation 条目必须被抓到（subject/object 一侧）"
    );
    assert!(leaks.iter().any(|l| l.contains("积分转赠")), "{leaks:#?}");

    // ── ②c 与被排除关系**身份三元组相同**的条目 ⇒ 必须红 ──
    // 夹具里被排除的关系是「订单 -[退款]-> 客户」：两端 + 谓词整值相等才算命中。
    let mut pred_leak = base.clone();
    pred_leak.push(json!({
        "kind": "add_link_type",
        "target": "订单→客户",
        "payload": {
            "key": "order_refund_customers",
            "from_type": "order",
            "to_type": "customers",
            "semantic": "退款"
        }
    }));
    let mut red = read_json(&m);
    red["items"] = Value::Array(pred_leak);
    let leaks = excluded_entries_in(&red, &excluded);
    eprintln!("[红] 身份与被排除关系相同的条目，判据结论：{leaks:#?}（必须非空）");
    assert!(
        leaks
            .iter()
            .any(|l| l.contains("退款") && l.contains("身份") && l.contains("订单")),
        "与被排除关系身份相同的条目必须被抓到（含谓词）：{leaks:#?}"
    );
}

/// ⭐ REQ-130 QA · **绿红成对**的另一半：合法的「超串」端点不许假红。
///
/// `套餐退款→客户`（`semantic = 退款`）是一条**合法边**：
/// - 端点 `套餐退款` **包含**「退款」（超串）；
/// - 谓词**整值等于**被排除关系的谓词「退款」。
///
/// 用 `contains` 判（上一版实现）⇒ 假红；按**身份三元组**判 ⇒ 绿。
/// 只做反向（红）会过严，只做这条（绿）会放水 —— 两条必须成对。
#[test]
fn ac002_a_legal_superstring_edge_is_not_a_leak() {
    let dir = temp_dir("ac002-superstring");
    let (a, p) = write_fixtures(&dir, "req129_ac002_super");
    let m = dir.join("merged.json");
    let o = cli(
        &[
            "doc", "merge",
            "--assertions", a.to_str().unwrap(),
            "--proposal", p.to_str().unwrap(),
            "--out", m.to_str().unwrap(),
        ],
        None,
    );
    assert!(o.status.success(), "{}", stderr(&o));

    let excluded = excluded_identities(&std::fs::read_to_string(&a).expect("读断言"));
    // 底料自证：这条合法边确实同时踩中「超串」与「谓词整值相等」两个坑，
    // 所以它能抓住 contains 判据（换成相等判据它就该绿）。
    assert!(
        "套餐退款".contains("退款"),
        "夹具必须是真超串，否则这条绿判据证明不了什么"
    );
    assert!(
        excluded.iter().any(|e| matches!(
            e,
            ExcludedIdentity::Relation { predicate, .. } if predicate == "退款"
        )),
        "被排除关系的谓词必须是「退款」，这条绿判据才有意义：{excluded:?}"
    );

    let mut all = items(&read_json(&m));
    all.push(json!({
        "kind": "add_link_type",
        "target": "套餐退款→客户",
        "payload": {
            "key": "refund_package_customers",
            "from_type": "refund_package",
            "to_type": "customers",
            "semantic": "退款"
        }
    }));
    let mut doc = read_json(&m);
    doc["items"] = Value::Array(all);
    let leaks = excluded_entries_in(&doc, &excluded);
    eprintln!("[绿] 合法边「套餐退款→客户」(semantic=退款) 的判据结论：{leaks:#?}（应为空）");
    assert!(
        leaks.is_empty(),
        "「套餐退款→客户」(semantic=退款) 是**合法边**：端点超串 + 谓词同值都不该判成\
         「被排除关系并入了」。用 contains 判就会在这里假红：{leaks:#?}"
    );
}

/// 真实 fixture 的产物路径（**找不到就响亮 SKIP**，⛔ 不静默当绿）。
///
/// 两种布局都认：worktree 内（`<crate>/out/...`）与 workspace 布局
/// （`<workspace>/code/out/...`，本 worktree 就是这种）。
fn real_fixture() -> Option<(PathBuf, PathBuf)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut bases = vec![root.to_path_buf()];
    if let Some(parent) = root.parent() {
        bases.push(parent.join("code"));
    }
    let first = |rel: &str| bases.iter().map(|b| b.join(rel)).find(|p| p.is_file());
    let assertions = first("out/req116/cache/assertions.jsonl")?;
    let proposal = first("out/proposal.json")?;
    Some((assertions, proposal))
}

/// ⭐ REQ-130 · AC-001/AC-002 在**真实 fixture** 上的判据：
/// `excluded_not_alignable == 28` **且** 产物里没有任何被排除的条目。
///
/// 这条同时是 N-3 的自证：同一份产物里「积分转赠」**确实出现**（在已并入条目
/// `客户` 的引文里）——所以旧的整文件 grep 判据在这里必红，而按条目的判据必须绿。
#[test]
fn ac002_real_fixture_excludes_28_and_no_excluded_entry() {
    const NAME: &str = "ac002_real_fixture_excludes_28_and_no_excluded_entry";
    let Some((a, p)) = real_fixture() else {
        return common::skip::missing("out/req116/cache/assertions.jsonl（+ out/proposal.json）", NAME);
    };
    let dir = temp_dir("ac002-real");
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
    assert!(o.status.success(), "真实 fixture 合并失败：{}", stderr(&o));

    // ① 计数判据（保留：这条是对的）
    let stats = read_json(&s);
    assert_eq!(stats["assertions_read"], json!(56), "真实 fixture 是 56 行");
    assert_eq!(
        stats["excluded_not_alignable"],
        json!(28),
        "56 行里 28 条 alignable:false ⇒ 必须全记在排除档：{stats}"
    );

    // ② 按条目判据
    let excluded = excluded_identities(&std::fs::read_to_string(&a).expect("读断言"));
    assert_eq!(excluded.len(), 2, "真实 fixture 有两类被排除概念：{excluded:?}");
    let doc = read_json(&m);
    let leaks = excluded_entries_in(&doc, &excluded);
    assert!(
        leaks.is_empty(),
        "真实 fixture 上不许有被排除的条目：{leaks:#?}"
    );

    // ③ 假阳性证据：整文件里**确实**能搜到被排除的词（在引文里）
    //    ⇒ 证明「改回 grep 就假红」不是纸上谈兵。
    let raw = std::fs::read_to_string(&m).expect("读产物");
    assert!(
        raw.contains("积分转赠"),
        "真实产物里应当保留这个假阳性证据（引文提到被排除词）；\
         若哪天夹具变了，这条判据要重新说明它为什么还有意义"
    );
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
        None => return common::skip::missing("ONTOK_TEST_DATABASE_URL", "ac006_applier_refuses_an_inapplicable_item_and_names_it"),
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
        None => return common::skip::missing("ONTOK_TEST_DATABASE_URL", "ac007_unsigned_doc_edges_stay_out_of_the_ttl_and_signed_ones_enter_it"),
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

    // ④ 其余字节不变：签后文本 = 原文本 + 恰好两行（signed_at / signed_at_source）
    //
    // 两边都按「去掉 `reviewed_by` 与签字两栏」比 —— 因为签字前的
    // `reviewed_by: null` 与签字后的 `reviewed_by: "qa-req129"` 是同一行的两种值，
    // 直接比会把「这一栏的值变了」误判成「别的行被改了」。
    let orig = std::fs::read_to_string(&m).expect("读输入");
    let signed = String::from_utf8(b1.clone()).expect("UTF-8");
    let strip = |text: &str| -> Vec<String> {
        text.lines()
            .filter(|l| {
                !l.contains("\"signed_at\"")
                    && !l.contains("\"signed_at_source\"")
                    && !l.contains("\"reviewed_by\":")
            })
            .map(str::to_string)
            .collect()
    };
    let after_lines = strip(&signed);
    let before_lines = strip(&orig);
    assert_eq!(
        after_lines, before_lines,
        "除这三栏外，其余行必须逐字节相同"
    );
    assert_eq!(
        signed.lines().count(),
        orig.lines().count() + 2,
        "签字只该多加 `signed_at` 与 `signed_at_source` 两行（`reviewed_by` 是改值不是加行）"
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
// REQ-130 · AC-003 / AC-004 · `signed_at_source` + 老产物兼容
//
// 审计要能回答「这个时间是人给的还是机器取的」：`--at` 给的是**声明**（可复现），
// 省略时取的是**记录**（只属于那一次运行），可信度不同 ⇒ 必须分栏记。
// 这一栏**不改变 Q-3 的裁定**（`--at` 仍可选），只标注来源。
// ──────────────────────────────────────────────────────────

/// 夹具里**被签过**的那一条（必须恰有一条 —— 否则判据空转或走眼）。
fn the_signed_item(doc: &Value) -> Value {
    let signed: Vec<Value> = items(doc)
        .into_iter()
        .filter(|it| !it["reviewed_by"].is_null())
        .collect();
    assert_eq!(signed.len(), 1, "夹具里应当恰有 1 条被签的条目：{signed:#?}");
    signed[0].clone()
}

/// 把渲染好的 JSON 里的 `signed_at_source` 全去掉（模拟 REQ-129 的老产物）。
fn without_signed_at_source(doc: &Value) -> Value {
    let mut v = doc.clone();
    for it in v["items"].as_array_mut().expect("items") {
        if let Some(o) = it.as_object_mut() {
            o.remove("signed_at_source");
        }
    }
    v
}

/// ⭐ AC-003（前半）：给了 `--at` ⇒ `signed_at_source: "arg"`。
#[test]
fn ac003_signed_at_source_is_arg_when_at_is_given() {
    let dir = temp_dir("src-arg");
    let m = merged_fixture(&dir, "req130_src_arg");
    let out = dir.join("signed-arg.json");
    let o = cli(
        &[
            "doc", "sign",
            "--file", m.to_str().unwrap(),
            "--item", "relation:订单→套餐模板",
            "--by", "qa-req130",
            "--out", out.to_str().unwrap(),
            "--at", AT,
        ],
        None,
    );
    assert!(o.status.success(), "{}", stderr(&o));

    let it = the_signed_item(&read_json(&out));
    assert_eq!(
        it["signed_at_source"], json!("arg"),
        "给了 --at ⇒ 来源是 arg（人给的声明）：{it}"
    );
    assert_eq!(it["signed_at"], json!(AT), "时间与来源必须同进同退");
    assert!(
        stderr(&o).contains("时间来源") && stderr(&o).contains("arg"),
        "报告里要写出这一栏（可数、可见）：{}",
        stderr(&o)
    );
}

/// ⭐ AC-003（后半）：省略 `--at` ⇒ `signed_at_source: "system"`，且时间**确实**是
/// 本次运行的系统时钟（否则「恒写 system + 恒写死时间」也能骗过这条判据）。
#[test]
fn ac003_signed_at_source_is_system_when_at_is_omitted() {
    let dir = temp_dir("src-system");
    let m = merged_fixture(&dir, "req130_src_system");
    let out = dir.join("signed-system.json");
    let before = chrono::Utc::now();
    let o = cli(
        &[
            "doc", "sign",
            "--file", m.to_str().unwrap(),
            "--item", "relation:订单→套餐模板",
            "--by", "qa-req130",
            "--out", out.to_str().unwrap(),
        ],
        None,
    );
    let after = chrono::Utc::now();
    assert!(o.status.success(), "{}", stderr(&o));

    let it = the_signed_item(&read_json(&out));
    assert_eq!(
        it["signed_at_source"], json!("system"),
        "省略 --at ⇒ 来源是 system（本次运行取的记录）：{it}"
    );
    let written = chrono::DateTime::parse_from_rfc3339(it["signed_at"].as_str().unwrap())
        .expect("signed_at 必须是 RFC3339");
    assert!(
        written.with_timezone(&chrono::Utc) >= before - chrono::Duration::seconds(2)
            && written.with_timezone(&chrono::Utc) <= after + chrono::Duration::seconds(2),
        "system 来源的时间必须落在这次运行的时钟窗口里：{} ∉ [{before}, {after}]",
        it["signed_at"]
    );
    assert!(
        stderr(&o).contains("时间来源") && stderr(&o).contains("system"),
        "报告里要写出这一栏：{}",
        stderr(&o)
    );
}

/// ⛔ AC-003 反空转：`arg` 与 `system` 必须真的**分得开** ——
/// 同一条目、同一份输入，只差「有没有给 `--at`」，两栏值必须不同。
#[test]
fn ac003_arg_and_system_are_distinguishable() {
    let dir = temp_dir("src-both");
    let m = merged_fixture(&dir, "req130_src_both");
    let with_at = dir.join("with-at.json");
    let without_at = dir.join("without-at.json");
    for (out, at) in [(&with_at, Some(AT)), (&without_at, None)] {
        let mut args = vec![
            "doc".to_string(), "sign".to_string(),
            "--file".to_string(), m.to_str().unwrap().to_string(),
            "--item".to_string(), "relation:订单→套餐模板".to_string(),
            "--by".to_string(), "qa-req130".to_string(),
            "--out".to_string(), out.to_str().unwrap().to_string(),
        ];
        if let Some(t) = at {
            args.push("--at".to_string());
            args.push(t.to_string());
        }
        let strs: Vec<&str> = args.iter().map(String::as_str).collect();
        let o = cli(&strs, None);
        assert!(o.status.success(), "{}", stderr(&o));
    }
    let a = the_signed_item(&read_json(&with_at));
    let b = the_signed_item(&read_json(&without_at));
    assert_eq!(a["signed_at_source"], json!("arg"));
    assert_eq!(b["signed_at_source"], json!("system"));
    assert_ne!(
        a["signed_at_source"], b["signed_at_source"],
        "两种情形必须分得开（否则这一栏等于没写）"
    );
}

/// ⭐ AC-004：读回**缺少** `signed_at_source` 的老产物 ⇒ 不报错（按缺省处理）。
///
/// 两个方向都判：
/// 1. 对老产物**重签同一条**（幂等）⇒ exit 0、字节不变（⛔ 不补写，也不 panic）；
/// 2. 在同一份老产物上签**另一条** ⇒ 新签名带 `signed_at_source`，老那条仍缺这一栏。
///
/// ⛔ 「不补写」是刻意的：老产物里 `arg` 与 `system` 已经分不出来，
/// 补一个 `system` 就是**伪造审计结论**。缺字段比假字段诚实。
#[test]
fn ac004_legacy_artifact_without_source_is_read_back_without_error() {
    let dir = temp_dir("legacy");
    let m = merged_fixture(&dir, "req130_legacy");
    let signed = dir.join("signed.json");
    let o = cli(
        &[
            "doc", "sign",
            "--file", m.to_str().unwrap(),
            "--item", "relation:订单→套餐模板",
            "--by", "qa-req130",
            "--out", signed.to_str().unwrap(),
            "--at", AT,
        ],
        None,
    );
    assert!(o.status.success(), "{}", stderr(&o));

    // 造一份「REQ-129 老产物」：签字在，但没有 signed_at_source
    let legacy = dir.join("legacy-signed.json");
    let legacy_doc = without_signed_at_source(&read_json(&signed));
    std::fs::write(
        &legacy,
        serde_json::to_string_pretty(&legacy_doc).expect("序列化") + "\n",
    )
    .expect("写老产物");
    let legacy_text = std::fs::read_to_string(&legacy).expect("读老产物");
    assert!(
        !legacy_text.contains("signed_at_source"),
        "夹具必须是老产物：没有这一栏"
    );

    // ① 重签同一条 ⇒ 幂等，不报错、字节不变
    let before_bytes = std::fs::read(&legacy).expect("读老产物");
    let o = cli(
        &[
            "doc", "sign",
            "--file", legacy.to_str().unwrap(),
            "--item", "relation:订单→套餐模板",
            "--by", "qa-req130",
            "--in-place",
            "--at", AT,
        ],
        None,
    );
    assert!(
        o.status.success(),
        "老产物（缺 signed_at_source）必须能读回、不许报错：{}",
        stderr(&o)
    );
    assert_eq!(
        std::fs::read(&legacy).expect("读老产物"),
        before_bytes,
        "幂等的重签不许改字节（更不许偷偷补上这一栏）"
    );
    let text = format!("{}{}", stdout(&o), stderr(&o));
    assert!(
        text.contains("老产物") && text.contains("缺省"),
        "报告要说清这是「老产物缺字段、按缺省处理」：{text}"
    );
    let doc = read_json(&legacy);
    let it = the_signed_item(&doc);
    assert!(
        it.get("signed_at_source").is_none(),
        "⛔ 不补写：老产物里的来源仍然是未知"
    );

    // ② 同一份老产物上签**另一条** ⇒ 新签名带来源，老那条不变
    let out = dir.join("legacy-plus.json");
    let o = cli(
        &[
            "doc", "sign",
            "--file", legacy.to_str().unwrap(),
            "--item", "entity:客户", // 夹具里 customers 是 both ⇒ 可签
            "--by", "qa-req130",
            "--out", out.to_str().unwrap(),
            "--at", AT,
        ],
        None,
    );
    assert!(
        o.status.success(),
        "老产物里另一条可签条目也必须能签：{}",
        stderr(&o)
    );
    let doc = read_json(&out);
    let fresh: Vec<Value> = items(&doc)
        .into_iter()
        .filter(|it| it["payload"]["key"] == json!("customers"))
        .collect();
    assert_eq!(fresh.len(), 1);
    assert_eq!(
        fresh[0]["signed_at_source"], json!("arg"),
        "新签名必须带来源：{fresh:?}"
    );
    let old: Vec<Value> = items(&doc)
        .into_iter()
        .filter(|it| it["payload"]["key"] == json!("order_template_id__package_templates"))
        .collect();
    assert_eq!(old.len(), 1);
    assert!(
        old[0]["signed_at_source"].is_null(),
        "老签名（本来就缺这一栏）不许被顺手补写：{old:?}"
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

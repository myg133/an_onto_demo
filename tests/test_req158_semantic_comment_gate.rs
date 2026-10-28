//! REQ-158 · **fail-open 修复的专属守卫**：客户视图 TTL 的英文语义不许经闸放行。
//!
//! # 洞是什么（实测，不是推测）
//!
//! 「漏翻即拒绝发布」的第四档只认 **JSON 形状**（`meaning_values` 只找键
//! `"meaning"`），而客户视图 TTL 把同一条数据序列化成 **注释形状**：
//! `rdfs:comment "语义：<token>（<基数>）"@zh`。于是：
//!
//! ```text
//! scan_modeling_terms(r#"rdfs:comment "语义：applies_to（多对多）"@zh ;"#)  ⇒ []   ← 漏（fail-open）
//! scan_modeling_terms(r#"{"meaning":"applies_to"}"#)                        ⇒ 命中 ← 抓到
//! ```
//!
//! 同一份数据两种形状、守卫只认一种 ⇒ 英文标识符**直接进客户视图**（v3 里
//! 13 个动词 / 14 行）。洞的**性质**是 fail-open：发错数据给客户，且**不会自己暴露**。
//!
//! # 本文件钉住的四条判据（必须能红能绿）
//!
//! | # | 输入 | 期望 | 旧代码 |
//! | --- | --- | --- | --- |
//! | ① | TTL `语义：applies_to（多对多）` | **命中**且点名 `applies_to` | ❌ 红（放行） |
//! | ② | TTL `语义：归属于（多对一）` | **不命中**（不许误报中文） | ✅ 绿 |
//! | ③ | JSON `{"meaning":"applies_to"}` | **命中**（原防线不许被破坏） | ✅ 绿 |
//! | ④ | JSON `{"meaning":"归属于"}` | **不命中** | ✅ 绿 |
//!
//! ② 同时是「**不许把整行/整份文档当值**」的判据：那条串里有一堆 ASCII 语法
//! （`rdfs:comment` / `@zh`），只要取数器把整行当值，中文语义就会被带成命中。
//!
//! # 端到端
//!
//! - [`publishing_a_customer_ttl_with_an_english_semantic_is_refused_before_anything_is_written`]
//!   走**真进程**：建项目 → 写快照 → `version freeze` → `ontok-builder ontology export --out`，
//!   断言「拒绝 + 点名那个值 + 原文件一个字节都没变」。
//! - [`the_real_gate_refuses_the_demands_own_probe_value_and_names_it`]
//!   对 demand §二 点名的那个值（`applies_to`）走**真发布闸** `render(C6, CustomerTurtle)`
//!   —— 那是 `gate_text(spec, doc.body())` 的调用点，CLI / HTTP 逐字同一条路径。
//!
//! ⚠️ 本文件不加 `//! @guard-manifest: REQ-081`（与 REQ-107/156 的同类文件一致）。

mod common;

/// 测试库不可用时的统一说明：**可能是没设，也可能是指向了非专用库**（防污染闸会拒）。
const NEED_TEST_DB: &str = "ONTOK_TEST_DATABASE_URL 不可用：没设，或指向了非专用测试库（库名需含 test/scratch/tmp/ci/dev）";

use ontok::domain::pipeline::{render, OutletId, Product};
use ontok::domain::ttl::{serialize_customer, TtlOptions, DEFAULT_BASE};
use ontok::domain::view::{scan_modeling_terms, CustomerView, EntityView, PropertyView, RelationView};

// ══════════════════════════════════════════════════════════════
// 夹具 —— 四条判据的四个输入串（与 demand §二 / §三 逐字一致）
// ══════════════════════════════════════════════════════════════

/// ① TTL 形状 · 英文语义（demand §二 的探针原串）。
const TTL_ENGLISH: &str = r#"rdfs:comment "语义：applies_to（多对多）"@zh ;"#;

/// ② TTL 形状 · 已翻成中文的语义（对照组）。
const TTL_CHINESE: &str = r#"rdfs:comment "语义：归属于（多对一）"@zh ;"#;

/// ③ JSON 形状 · 英文语义（第四档原本就抓得住的那条，⛔ 不许被这次改造放过）。
const JSON_ENGLISH: &str = r#"{"meaning":"applies_to"}"#;

/// ④ JSON 形状 · 已翻成中文的语义。
const JSON_CHINESE: &str = r#"{"meaning":"归属于"}"#;

/// 一段**真实形状**的客户视图关系块（用来证明取数器不吃整行/整文档，
/// 也证明它在多行块里同样只取 `语义：` 后面那一小段）。
fn relation_block(meaning: &str, how_many: &str) -> String {
    format!(
        ":{local} a owl:ObjectProperty ;\n\
         \x20   rdfs:label \"{a} → {b}\"@zh ;\n\
         \x20   rdfs:comment \"语义：{meaning}（{how_many}）\"@zh ;\n\
         \x20   skos:altLabel \"{meaning}\"@zh ;\n\
         \x20   rdfs:range :target .\n",
        local = "rel_1",
        a = "订单",
        b = "客户",
    )
}

fn opts() -> TtlOptions<'static> {
    TtlOptions {
        project: "样例项目",
        version: 2,
        base: DEFAULT_BASE,
        action_type_count: 0,
        tenancy: None,
        schema: None,
        api_classes: None,
    }
}

/// 一份最小客户视图，关系语义是**原样**传进来的（⛔ 不过 `semantic_cn`）——
/// 用来模拟「上游没有翻译」的视图（REQ-156 之前的产物、或手工改过的数据）。
fn view_with_meaning(meaning: &str) -> CustomerView {
    CustomerView {
        objects: vec![
            EntityView {
                name: "订单".into(),
                note: None,
                belongs_to: None,
                aliases: vec![],
                properties: vec![PropertyView {
                    name: "下单时间".into(),
                    note: None,
                    required: false,
                    sample: None,
                }],
            },
            EntityView {
                name: "客户".into(),
                note: None,
                belongs_to: None,
                aliases: vec![],
                properties: vec![],
            },
        ],
        relations: vec![RelationView {
            from: "订单".into(),
            to: "客户".into(),
            how_many: "多对一".into(),
            meaning: meaning.into(),
        }],
    }
}

// ══════════════════════════════════════════════════════════════
// 判据 ① · TTL 形状的英文语义**必须**命中（旧代码红）
// ══════════════════════════════════════════════════════════════

#[test]
fn criterion_1_ttl_comment_shape_with_english_semantic_is_a_hit() {
    let hits = scan_modeling_terms(TTL_ENGLISH);
    eprintln!("判据①{TTL_ENGLISH} ⇒ {hits:?}");
    assert!(
        !hits.is_empty(),
        "TTL 形状的英文语义**没有被抓住** —— 这正是 fail-open（发错数据给客户，\
         且不会自己暴露）。\n输入：{TTL_ENGLISH}"
    );
    assert!(
        hits.iter().any(|h| h.contains("applies_to")),
        "命中清单必须**点名那个值**（`applies_to`），否则运维不知道该改哪条语义：{hits:?}"
    );
    assert!(
        hits.iter().any(|h| h.contains("未翻译")),
        "命中理由必须说清是「未翻译」，不是别的档位碰巧命中：{hits:?}"
    );
}

/// 多行**真实形状**的块里也要抓 —— 不是只认「一整行只有这一句」的玩具输入。
#[test]
fn criterion_1b_english_semantic_inside_a_real_ttl_block_is_a_hit() {
    let block = relation_block("applies_to", "多对多");
    let hits = scan_modeling_terms(&block);
    eprintln!("判据①b 多行块 ⇒ {hits:?}");
    assert!(
        hits.iter().any(|h| h.contains("applies_to")),
        "真实形状的 TTL 块里，英文语义必须被点名：{hits:?}\n{block}"
    );
}

// ══════════════════════════════════════════════════════════════
// 判据 ② · 中文语义**不许**误报（同时是「不许吃掉整行」的判据）
// ══════════════════════════════════════════════════════════════

#[test]
fn criterion_2_ttl_comment_shape_with_chinese_semantic_is_not_a_hit() {
    let hits = scan_modeling_terms(TTL_CHINESE);
    eprintln!("判据②{TTL_CHINESE} ⇒ {hits:?}");
    assert!(
        hits.is_empty(),
        "已翻成中文的语义被误杀了 —— 中文视图会**全部**拒绝发布：{hits:?}\n\
         输入：{TTL_CHINESE}"
    );
    // 反空转：这条串里**本来就有** ASCII 字母（`rdfs` / `comment` / `zh`）。
    // 只要取数器把整行（或整份文档）当「值」，上面那句 `hits.is_empty()` 必红 ——
    // 所以「不误杀中文」与「不吃整行」是**同一条断言**，不是两条。
    assert!(
        TTL_CHINESE.chars().any(|c| c.is_ascii_alphabetic()),
        "反空转：对照组夹具本身必须含 ASCII 字母，否则它证明不了「没吃整行」"
    );
}

/// 真实形状的中文块（含 `skos:altLabel` / `rdfs:range` 等 ASCII 语法）⇒ 零命中。
#[test]
fn criterion_2b_chinese_semantic_inside_a_real_ttl_block_is_not_a_hit() {
    let block = relation_block("归属于", "多对一");
    let hits = scan_modeling_terms(&block);
    eprintln!("判据②b 多行块 ⇒ {hits:?}");
    assert!(hits.is_empty(), "真实形状的中文块被误杀：{hits:?}\n{block}");
}

// ══════════════════════════════════════════════════════════════
// 判据 ③ / ④ · JSON 那条防线**原样**有效（⛔ 不许因为改造而放过）
// ══════════════════════════════════════════════════════════════

#[test]
fn criterion_3_json_shape_with_english_semantic_is_still_a_hit() {
    let hits = scan_modeling_terms(JSON_ENGLISH);
    eprintln!("判据③{JSON_ENGLISH} ⇒ {hits:?}");
    assert!(
        hits.iter().any(|h| h.contains("applies_to")),
        "JSON 形状的英文语义被放过了 —— 第四档（防线本体）被改坏了：{hits:?}"
    );
}

#[test]
fn criterion_4_json_shape_with_chinese_semantic_is_not_a_hit() {
    let hits = scan_modeling_terms(JSON_CHINESE);
    eprintln!("判据④{JSON_CHINESE} ⇒ {hits:?}");
    assert!(hits.is_empty(), "JSON 形状的中文语义被误杀：{hits:?}");
}

/// 跨形状反空转：**同一段文本里两种形状都没有**英文语义时，零命中；
/// 只要有一个形状漏了，本判据就红。
#[test]
fn both_shapes_agree_on_the_same_document() {
    let clean = format!("{TTL_CHINESE}\n{JSON_CHINESE}");
    assert!(
        scan_modeling_terms(&clean).is_empty(),
        "两种形状都是中文时不许有命中：{:?}",
        scan_modeling_terms(&clean)
    );
    for bad in [TTL_ENGLISH, JSON_ENGLISH] {
        assert!(
            scan_modeling_terms(bad).iter().any(|h| h.contains("applies_to")),
            "形状 `{bad}` 上的英文语义没被抓到 —— 两种形状必须**同时**覆盖"
        );
    }
}

// ══════════════════════════════════════════════════════════════
// 端到端 ③' · demand 点名的那个值（`applies_to`）走**真发布闸**
// ══════════════════════════════════════════════════════════════

/// 真管线（`render` ⇒ `gate_text(spec, doc.body())`）必须**拒绝**并点名 `applies_to`。
///
/// 为什么用 `render` 而不是真进程：`semantic_cn` 已经把 15 种真实动词都翻成中文了，
/// 所以真进程**再也造不出** `语义：applies_to（…）`。但闸的职责与上游翻没翻无关 ——
/// 「TTL 里出现英文语义 ⇒ 拒绝发布」必须**独立于**上游行为成立（上游回退/手工数据/
/// 新抽取器都可能在闸前把英文写进 TTL）。所以这条判据直接喂真闸一份带英文语义的视图。
#[test]
fn the_real_gate_refuses_the_demands_own_probe_value_and_names_it() {
    let view = view_with_meaning("applies_to");
    // 反空转：夹具真的把英文写进了 TTL 的注释形状（否则下面测的不是这条形状）。
    let doc = serialize_customer(&view, &opts());
    assert!(
        doc.body().contains("语义：applies_to（多对一）"),
        "夹具没把 `applies_to` 写进 TTL 的 `语义：` 注释，本判据测不到东西：\n{}",
        doc.body()
    );

    let err = render(OutletId::C6, Product::CustomerTurtle { view: &view, opts: opts() })
        .expect_err("含英文语义的客户视图 TTL 必须被拒绝发布（fail-closed）");
    let hits = err.hits();
    eprintln!("端到端③'：outlet={} leak={} hits={hits:?}", err.outlet(), err.is_leak());
    assert!(err.is_leak(), "拒绝的理由必须是「泄露了建模术语」，不是渲染失败");
    assert_eq!(err.outlet(), "C6", "客户视图 TTL 的出口应当是 C6");
    assert!(
        hits.iter().any(|h| h.contains("applies_to")),
        "错误信息里必须**点名那个值**（`applies_to`）：{hits:?}"
    );

    // 对照组：同一份夹具把语义换回中文 ⇒ 放行。
    let ok = render(
        OutletId::C6,
        Product::CustomerTurtle { view: &view_with_meaning("归属于"), opts: opts() },
    )
    .expect("中文语义必须放行");
    assert!(ok.as_str().contains("语义：归属于（多对一）"));
}

// ══════════════════════════════════════════════════════════════
// 端到端 ③ · 真进程：拒绝 + 点名 + **原文件一个字节都没变**
// ══════════════════════════════════════════════════════════════

/// 建「项目 + 第 1 版 + 快照 + 冻结」，返回 `(project, version_seq)`。
///
/// `semantic` 走 `LinkType.semantic`（那一刻**还没**过 `semantic_cn`）——
/// 闸是发布前的最后一道，它必须对这个字段的任何取值都成立。
async fn frozen_version_with_semantic(prefix: &str, semantic: &str) -> (String, i32) {
    use ontok::db::project::PgProjectStore;
    use ontok::db::snapshot_store::PgStore;
    use ontok::domain::snapshot::{EntityType, ExtractedBy, LinkType, Property, Snapshot};

    let pool = common::test_pool().await;
    let ps = PgProjectStore::new(pool.clone());
    let store = PgStore::new(pool.clone());
    store.migrate().await.expect("建表失败");
    let p = common::fresh_project(&ps, prefix).await;
    let v = ps.new_version(p.id).await.expect("建版本失败");

    let mut snap = Snapshot::new(v.id, v.seq);
    snap.extracted_by = Some(ExtractedBy {
        agent: "inproc:ontok::agent::RuleBasedAgent".into(),
        version: "0.1.0".into(),
        round: 1,
        failed_groups: vec![],
        source: None,
    });
    snap.entity_types.push(EntityType {
        key: "alpha".into(),
        display_name: "订单".into(),
        description: None,
        aliases: vec![],
        parent: None,
        disjoint_with: vec![],
        properties: vec![Property {
            name: "下单时间".into(),
            column: "created_at".into(),
            nullable: false,
            note: None,
            sample: None,
        }],
        evidence: vec![],
    });
    snap.entity_types.push(EntityType {
        key: "beta".into(),
        display_name: "客户".into(),
        description: None,
        aliases: vec![],
        parent: None,
        disjoint_with: vec![],
        properties: vec![],
        evidence: vec![],
    });
    snap.link_types.push(LinkType {
        key: "alpha_beta".into(),
        from_type: "alpha".into(),
        to_type: "beta".into(),
        cardinality: "N:1".into(),
        semantic: semantic.into(),
        evidence: vec![],
        confidence: None,
        join_verified: None,
        join_evidence: None,
    });
    store.save_snapshot(&snap).await.expect("写快照失败");
    ps.freeze(v.id).await.expect("冻结失败");
    (p.name, v.seq)
}

/// `ontok-builder ontology export --out <file>` —— **真进程**，与用户敲的那条命令同一入口。
fn run_export(project: &str, version: i32, out: &std::path::Path) -> std::process::Output {
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_ontok-builder"));
    cmd.args(["ontology", "export", "--project", project, "--version", &version.to_string()]);
    cmd.args(["--out", out.to_str().expect("临时路径必须是 UTF-8")]);
    cmd.env(
        "ONTOK_DATABASE_URL",
        ontok::config::test_database_url().expect(NEED_TEST_DB),
    );
    cmd.output().expect("跑 ontok ontology export 失败")
}

/// ⭐ 端到端：含英文语义的客户视图 ⇒ **发布被拒** + 点名那个值 + **原文件一个字节都没变**。
///
/// 「文件没变」证明的是**校验在写之前**：`prepare_ttl_export` 之后先 `publish_customer`，
/// 命中即 `std::process::exit(EXIT_LEAKED_MODELING_TERMS)`，`--out` 的
/// `std::fs::write` 根本走不到。所以预置一份哨兵文件，拒绝之后它必须**逐字节**还是原样。
#[tokio::test]
async fn publishing_a_customer_ttl_with_an_english_semantic_is_refused_before_anything_is_written() {
    const EXIT_LEAKED: i32 = ontok::cli::EXIT_LEAKED_MODELING_TERMS;
    let dir = std::env::temp_dir().join(format!("req158-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("建临时目录失败");
    let file = dir.join("customer.ttl");
    // 哨兵：拒绝之后必须逐字节原样（连 mtime 都不该被这次运行改写内容）。
    let sentinel: &[u8] = "# 哨兵：这是发布前就存在的客户视图 TTL，被拒绝的发布不许动它\n".as_bytes();
    std::fs::write(&file, sentinel).expect("写哨兵失败");

    // ① 未翻译的语义（真库 v3 里 13 个动词的同类：抽取器随时可能吐出一个新拼法）
    let (project, version) = frozen_version_with_semantic("req158bad", "aggregates").await;
    let out = run_export(&project, version, &file);
    let written = std::fs::read(&file).expect("读不到 --out 指向的文件");
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    eprintln!(
        "REQ-158 现场（英文语义）：project={project} version={version} · exit={:?} · \
         stdout={} 字节 · --out={:?}\nstderr: {}",
        out.status.code(),
        out.stdout.len(),
        String::from_utf8_lossy(&written),
        stderr.trim()
    );

    assert_eq!(
        out.status.code(),
        Some(EXIT_LEAKED),
        "含英文语义的客户视图必须以「拒绝出图」的退出码结束，实际 {:?}；stderr={stderr}",
        out.status.code()
    );
    assert!(out.stdout.is_empty(), "拒绝时 stdout 不许有任何字节");
    assert!(
        stderr.contains("aggregates"),
        "错误信息必须**点名那个值**（`aggregates`），否则运维无从下手：{stderr}"
    );
    assert!(stderr.contains("C6"), "stderr 应当说明是哪条出口：{stderr}");
    assert_eq!(
        written, sentinel,
        "闸必须在**写之前**：被拒绝的发布不许改动原文件一个字节，实际写进了：\n{}",
        String::from_utf8_lossy(&written)
    );

    // ② 对照组：把语义换成已翻好的中文 ⇒ 放行，且真写出了那一句注释形状。
    let (ok_project, ok_version) = frozen_version_with_semantic("req158ok", "belongs_to").await;
    let ok = run_export(&ok_project, ok_version, &file);
    let ok_written = std::fs::read_to_string(&file).expect("读不到 --out 产物");
    eprintln!(
        "REQ-158 现场（中文语义）：project={ok_project} version={ok_version} · exit={:?}\n\
         产物里的语义行：{:?}",
        ok.status.code(),
        ok_written
            .lines()
            .find(|l| l.contains("语义："))
            .unwrap_or("<没有语义行>")
    );
    assert_eq!(
        ok.status.code(),
        Some(0),
        "中文语义必须导出成功；stderr={}",
        String::from_utf8_lossy(&ok.stderr)
    );
    assert!(
        ok_written.contains("语义：归属于（多对一）"),
        "对照组必须真的写出中文的 `语义：` 注释（否则上面的拒绝证明不了就是这个形状）：\n{ok_written}"
    );
    assert!(
        !ok_written.contains("belongs_to"),
        "客户视图 TTL 里不许出现英文语义 key：\n{ok_written}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

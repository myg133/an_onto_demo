//! REQ-110 · 内置 TTL 导出的**测设层判据**（AC-001 / AC-002 / AC-003 / AC-008 / AC-009）。
//!
//! # 分工（别把判据写重）
//!
//! | 判据 | 住在哪 |
//! | --- | --- |
//! | AC-004 / AC-005 / AC-006 / AC-007（HTTP 行为） | `src/server/http.rs` 的 `#[cfg(test)]`（要拿私有 `AppState`） |
//! | AC-001 / AC-002 / AC-003 / AC-008（序列化与闸） | 本文件 |
//! | AC-009（出口登记） | `tests/test_req107_*.rs`（登记表就在那里） |
//! | AC-010（判据改写） | `src/server/http.rs` 的 `unauthenticated_probes_learn_nothing_but_the_shell_exists` |
//!
//! # AC-001 的「rdflib 解析」这一半
//!
//! 本轮**不引依赖**（Cargo.toml 不许加 RDF 库），所以测试进程里没有解析器。
//! 本文件钉的是**可机械核对的部分**：三类三元组的条数 == 视图里的条数、
//! 真库（`anspire_ai` v2）上的 83 / 866 / 52、两次导出逐字节相同。
//! 「rdflib 解析 0 错误 + 与 `ai-ontology.ttl` 同构」由交付报告里的
//! **外部验证脚本**给出（`python -c "rdflib..."`）—— 这是环境事实，不是本文件能替代的。

mod common;

use ontok::domain::pipeline::{render, OutletId, Product, PipelineError};
use ontok::domain::ttl::{serialize_customer, serialize_model, TtlOptions, DEFAULT_BASE};
use ontok::domain::view::{CustomerView, EntityView, PropertyView, RelationView};

// ══════════════════════════════════════════════════════════════
// 夹具
// ══════════════════════════════════════════════════════════════

fn clean_view() -> CustomerView {
    CustomerView {
        objects: vec![
            EntityView {
                name: "订单".into(),
                note: Some("一张订单".into()),
                belongs_to: None,
                aliases: vec![],
                properties: vec![
                    PropertyView {
                        name: "下单时间".into(),
                        note: None,
                        required: true,
                        sample: Some("2026-01-01".into()),
                    },
                    PropertyView {
                        name: "金额".into(),
                        note: Some("含税".into()),
                        required: false,
                        sample: None,
                    },
                ],
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
            meaning: "归属于".into(),
        }],
    }
}

fn opts() -> TtlOptions<'static> {
    TtlOptions { project: "样例项目", version: 2, base: DEFAULT_BASE, action_type_count: 0 }
}

/// 计一个块类型的条数（块头固定是 `a owl:Xxx ;`）。
fn count_blocks(doc: &str, kind: &str) -> usize {
    doc.matches(&format!("a {kind}")).count()
}

// ══════════════════════════════════════════════════════════════
// AC-001 · 三类三元组条数 == 视图里的条数
// ══════════════════════════════════════════════════════════════

#[test]
fn ac001_class_property_and_link_counts_match_the_view() {
    let view = clean_view();
    let doc = serialize_customer(&view, &opts());
    let text = doc.doc();

    assert_eq!(
        count_blocks(text, "owl:Class"),
        view.objects.len(),
        "owl:Class 条数必须等于对象数：\n{text}"
    );
    let props: usize = view.objects.iter().map(|o| o.properties.len()).sum();
    assert_eq!(
        count_blocks(text, "owl:DatatypeProperty"),
        props,
        "DatatypeProperty 条数必须等于属性总数：\n{text}"
    );
    assert_eq!(
        text.matches("owl:ObjectProperty").count(),
        view.relations.len(),
        "ObjectProperty 条数必须等于关系数：\n{text}"
    );
    // 关系两端都要落成 domain / range（否则「对象属性」只是个空壳）。
    assert_eq!(text.matches("rdfs:domain").count(), props + view.relations.len(), "{text}");
    assert_eq!(text.matches("rdfs:range").count(), props + view.relations.len(), "{text}");
}

// ══════════════════════════════════════════════════════════════
// AC-002 · 客户视图导出**必须过守门器**（命中即拒绝）
// ══════════════════════════════════════════════════════════════

#[test]
fn ac002_a_poisoned_note_is_refused_by_the_customer_export() {
    let mut view = clean_view();
    // 「主键」是守门词；它出现在**说明**里 ⇒ 会变成 rdfs:comment 字面量。
    // 这正说明为什么要「序列化之后再扫」：结构字段层面它只是 note。
    view.objects[0].note = Some("本表的主键服务中心，按业务口径解读".into());

    let err = render(OutletId::C6, Product::CustomerTurtle { view: &view, opts: opts() })
        .expect_err("客户视图带建模术语时必须拒绝导出（AC-002）");
    assert!(err.is_leak(), "失败必须是「发布闸命中」，不是渲染错误：{err:?}");
    assert!(
        err.hits().iter().any(|h| h == "主键"),
        "命中清单里要报出词条（只进 stderr / 调用方）：{:?}",
        err.hits()
    );
    assert!(
        !err.message().contains("主键"),
        "错误文案本身不许回显词条：{}",
        err.message()
    );

    // 清掉之后必须成功 —— 否则上一条证明的只是「恒拒绝」。
    view.objects[0].note = Some("一张订单".into());
    let ok = render(OutletId::C6, Product::CustomerTurtle { view: &view, opts: opts() })
        .expect("清掉词条后必须能导出");
    assert!(ok.as_str().contains("a owl:Class"), "{}", ok.as_str());
}

#[test]
fn ac002_the_gate_scans_the_serialized_bytes_not_the_struct_fields() {
    // 词条藏在**属性名**里：结构上一字段是 `name`，序列化后是 rdfs:label + IRI。
    let mut view = clean_view();
    view.objects[0].properties[0].name = "主键".into();
    let err = render(OutletId::C6, Product::CustomerTurtle { view: &view, opts: opts() })
        .expect_err("属性名里的建模术语同样要被拦住");
    assert!(err.is_leak(), "{err:?}");
}

// ══════════════════════════════════════════════════════════════
// AC-003 · 确定性（逐字节）
// ══════════════════════════════════════════════════════════════

#[test]
fn ac003_two_exports_are_byte_identical_and_order_independent() {
    let a = render(OutletId::C6, Product::CustomerTurtle { view: &clean_view(), opts: opts() })
        .expect("干净视图");
    let b = render(OutletId::C6, Product::CustomerTurtle { view: &clean_view(), opts: opts() })
        .expect("干净视图");
    assert_eq!(a.as_str(), b.as_str(), "同一条路径两次导出必须逐字节相同（AC-003）");

    // 反序输入：对象 / 属性 / 关系都倒过来，输出必须**一字不差**。
    // （只比「两次调用」不够：它挡不住 HashMap 迭代序，这一条才挡得住。）
    let mut rev = clean_view();
    rev.objects.reverse();
    for o in &mut rev.objects {
        o.properties.reverse();
    }
    rev.relations.reverse();
    let c = render(OutletId::C6, Product::CustomerTurtle { view: &rev, opts: opts() })
        .expect("干净视图");
    assert_eq!(a.as_str(), c.as_str(), "输出依赖了输入顺序 ⇒ 不是确定性序列化");
}

#[test]
fn ac003_the_generation_time_is_not_written_into_the_body() {
    let doc = serialize_customer(&clean_view(), &opts());
    // 时间不许进正文：任何 ISO-8601 日期时间都会让两次导出差一个字节。
    assert!(
        !doc.doc().contains("dcterms:created"),
        "禁止把生成时间写进文件（会破坏逐字节一致）：\n{}",
        doc.doc()
    );
    assert!(
        !doc.body().contains("T00:00:00"),
        "正文里出现了时间戳形态：\n{}",
        doc.body()
    );
}

// ══════════════════════════════════════════════════════════════
// AC-008 · 有损项在文件头显式声明
// ══════════════════════════════════════════════════════════════

#[test]
fn ac008_the_header_declares_all_three_losses_and_the_action_count() {
    let view = clean_view();
    let doc = serialize_customer(&view, &opts());
    // 文件头 = 前缀 + `#` 声明块；到**本体节点**那一行为止（`\n<https://` 才是它，
    // 前缀行里的 `<https://` 不算）。
    let cut = doc.doc().find("\n<https://ontok.local/").expect("找不到本体节点");
    let header = &doc.doc()[..cut];
    for kw in ["ActionType", "semantic", "cardinality"] {
        assert!(header.contains(kw), "文件头缺少 `{kw}`（AC-008）：\n{header}");
    }
    assert!(header.contains("未导出（0 个）"), "文件头必须写出动作层**数量**：\n{header}");
    // 数量跟着数据走（CLI 传快照里的真实条数）。
    let with_actions = TtlOptions { action_type_count: 7, ..opts() };
    let doc2 = serialize_customer(&view, &with_actions);
    assert!(doc2.doc().contains("未导出（7 个）"), "动作层条数必须如实声明");
}

/// ⛔ **文件头不能进闸的扫描范围**：它按 AC-008 必须写 `cardinality`，
/// 而 `cardinality` 是守门词 —— 「扫整份文件」与「文件头必须写它」不可能同时成立。
///
/// 所以判据钉两件事：① 正文里没有守门词；② 头里出现与否**不影响**闸的结论。
#[test]
fn ac008_the_constant_header_is_outside_the_gated_bytes() {
    let doc = serialize_customer(&clean_view(), &opts());
    assert!(doc.doc().contains("cardinality"), "头里必须写（AC-008）");
    assert!(!doc.body().contains("cardinality"), "正文里不许出现（否则闸会恒命中）");
    let hits = ontok::domain::view::scan_modeling_terms(doc.body());
    assert!(hits.is_empty(), "干净视图的正文不该命中任何守门词：{hits:?}");
}

// ══════════════════════════════════════════════════════════════
// 建模视图：显式豁免（不是「没调闸」）
// ══════════════════════════════════════════════════════════════

#[test]
fn modeling_export_is_exempt_but_customer_export_is_not() {
    use ontok::domain::model_view::{ModelEntityView, ModelPropertyView, ModelTrackView};
    let model = ModelTrackView {
        seq: 2,
        extracted_by: None,
        entity_types: vec![ModelEntityView {
            key: "order".into(),
            display_name: "订单".into(),
            description: Some("本表的主键服务中心".into()), // ← 同样的词条
            aliases: vec![],
            parent: None,
            disjoint_with: vec![],
            properties: vec![ModelPropertyView {
                name: "订单号".into(),
                column: "order_no".into(),
                nullable: false,
                note: None,
                sample: None,
            }],
            evidence: vec![],
        }],
        link_types: vec![],
        action_types: vec![],
        functions: vec![],
    };
    let mv = render(OutletId::D9, Product::ModelTurtle { view: &model, opts: opts() })
        .expect("建模轨必须放行（建模术语在这一轨是要被裁决的内容）");
    assert!(mv.as_str().contains("ontok:key \"order\""), "{}", mv.as_str());

    // 反向对照：同一段文字放进客户视图 ⇒ 必须被拦。
    let mut cv = clean_view();
    cv.objects[0].note = Some("本表的主键服务中心".into());
    let err = render(OutletId::C6, Product::CustomerTurtle { view: &cv, opts: opts() })
        .expect_err("客户轨必须被拦");
    assert!(err.is_leak(), "{err:?}");
    // 误标（把建模产物从客户轨发）= 形态不相容，不是闸命中 —— 换 ID 关不掉闸。
    let mismatch = render(OutletId::C6, Product::ModelTurtle { view: &model, opts: opts() })
        .expect_err("建模产物不许从客户轨出口发布");
    assert!(matches!(mismatch, PipelineError::Render { .. }), "{mismatch:?}");
}

// ══════════════════════════════════════════════════════════════
// 纪律 H · 只有一条序列化路径
// ══════════════════════════════════════════════════════════════

/// ⛔ 出口文件里**不许直调**序列化器 —— 必须经 `domain::pipeline`。
///
/// 这是否定式判据（比「两边都调了同一个函数」强）：只要有人在
/// `cli.rs` / `http.rs` 里写一行 `serialize_customer(`，这条就红。
#[test]
fn discipline_h_only_the_pipeline_calls_the_serializer() {
    let refs = ["serialize_customer(", "serialize_model("];
    for file in ["src/cli.rs", "src/server/http.rs"] {
        let code = common::strip_comments(&common::read_source(file));
        let code = code.split("#[cfg(test)]").next().unwrap_or("").to_string();
        for r in refs {
            assert!(
                !code.contains(r),
                "{file} 里出现了 `{r}` —— TTL 序列化只许经 `domain::pipeline` 调用（纪律 H）"
            );
        }
    }
    // 正向对照（纪律 E）：管线里**确实**调了它，否则上面全是空转。
    let pipeline = common::strip_comments(&common::read_source("src/domain/pipeline.rs"));
    assert!(pipeline.contains("serialize_customer("), "管线没调序列化器 ⇒ 上面两条是空转");
    assert!(pipeline.contains("serialize_model("), "管线没调建模序列化器");
}

/// ⛔ 本轮**不引任何新依赖**（Turtle 手写序列化）。
#[test]
fn no_new_dependency_for_turtle() {
    let cargo = common::read_source("Cargo.toml");
    for bad in ["oxrdf", "sophia", "rio_turtle", "rio-turtle", "rdf=", "ttl="] {
        assert!(
            !cargo.to_lowercase().contains(&bad.to_lowercase()),
            "Cargo.toml 里出现了 `{bad}` —— 本轮不许引入 RDF 依赖（Turtle 手写）"
        );
    }
}

/// `domain/` 零 I/O 也覆盖新模块：`ttl.rs` 不碰文件 / 标准输出 / 库。
#[test]
fn ttl_module_is_pure() {
    let src = common::strip_comments(&common::read_source("src/domain/ttl.rs"));
    for bad in ["std::fs", "std::io", "println!", "eprintln!", "sqlx", "PgStore", "db::"] {
        assert!(!src.contains(bad), "ttl.rs 不是纯函数：出现了 `{bad}`");
    }
}

// ══════════════════════════════════════════════════════════════
// 真库：anspire_ai v2（环境事实；缺了就**明确**说，不静默跳过）
// ══════════════════════════════════════════════════════════════

/// 与参照物 `code/out/ai-ontology.ttl` 对齐的三个数。
///
/// ⚠️ 这是**环境事实**（BA 提供的库里有 `anspire_ai` v2），不是本轮编出来的常量：
/// 若库被重建 / 换库，这三个数字变了，本条会红 —— 那是**提醒**，不是假失败。
/// 外部 rdflib 验证见交付报告（同构 + 5954 → 5953 的 `dcterms:created` 差异）。
const REF_CLASSES: usize = 83;
const REF_DATATYPE_PROPS: usize = 866;
const REF_OBJECT_PROPS: usize = 52;

#[tokio::test]
async fn live_anspire_ai_v2_exports_the_reference_counts_and_is_deterministic() {
    use ontok::db::project::PgProjectStore;
    use ontok::db::snapshot_store::PgStore;

    let pool = common::test_pool().await;
    let ps = PgProjectStore::new(pool.clone());
    let store = PgStore::new(pool.clone());

    let Some(p) = ps.get_by_name("anspire_ai").await.unwrap() else {
        eprintln!(
            "· 库里没有 `anspire_ai` ⇒ 跳过真库那半（结构判据仍在上面的测试里跑）。\
             这不是绿色通过，是**环境缺失**，交付报告里已如实登记。"
        );
        return;
    };
    let Ok(v) = ps.version_by_seq(p.id, &p.name, 2).await else {
        eprintln!("· `anspire_ai` 没有第 2 版 ⇒ 跳过真库那半。");
        return;
    };
    let snap = store
        .get_snapshot(v.id)
        .await
        .unwrap()
        .expect("第 2 版应当有快照");

    // ── 建模视图：83 / 866 / 52，且与视图里的条数**逐项相等** ──
    let mv = ontok::domain::model_view::project_model_track(&snap);
    let total_props: usize = mv.entity_types.iter().map(|e| e.properties.len()).sum();
    assert_eq!(mv.entity_types.len(), REF_CLASSES, "与参照物的类数不符");
    assert_eq!(total_props, REF_DATATYPE_PROPS, "与参照物的属性数不符");
    assert_eq!(mv.link_types.len(), REF_OBJECT_PROPS, "与参照物的关系数不符");

    let ttl_opts = TtlOptions {
        project: "anspire_ai",
        version: 2,
        base: DEFAULT_BASE,
        action_type_count: snap.action_types.len(),
    };
    let one = render(OutletId::D9, Product::ModelTurtle { view: &mv, opts: ttl_opts })
        .expect("建模轨不过闸")
        .into_text();
    let two = render(OutletId::D9, Product::ModelTurtle { view: &mv, opts: ttl_opts })
        .expect("建模轨不过闸")
        .into_text();
    assert_eq!(one, two, "真库导出两次必须逐字节相同（AC-003）");
    assert_eq!(
        count_blocks(&one, "owl:Class"),
        mv.entity_types.len(),
        "owl:Class 条数 != 类数"
    );
    assert_eq!(
        count_blocks(&one, "owl:DatatypeProperty"),
        total_props,
        "DatatypeProperty 条数 != 属性总数"
    );
    assert_eq!(
        one.matches("owl:ObjectProperty").count(),
        mv.link_types.len(),
        "ObjectProperty 条数 != 关系数"
    );

    // ── 客户视图：必须过闸（真实数据上也不许有守门词） ──
    let cv = ontok::domain::view::project(&ontok::cli::snapshot_to_view(&snap));
    let customer = render(OutletId::C6, Product::CustomerTurtle { view: &cv, opts: ttl_opts })
        .expect("真实客户视图必须过守门器")
        .into_text();
    assert_eq!(
        count_blocks(&customer, "owl:Class"),
        cv.objects.len(),
        "客户视图的类数不对"
    );
    assert!(!customer.contains("dcterms:created"), "不许写生成时间");
}

// ══════════════════════════════════════════════════════════════
// AC-002 / AC-003 · **真进程**：CLI 的退出码与逐字节确定性
// ══════════════════════════════════════════════════════════════

/// 建一个「项目 + 第 1 版 + 快照 + 冻结」，返回 `(project_name, version_seq)`。
async fn live_frozen_version(prefix: &str, poisoned: bool) -> (String, i32) {
    use ontok::db::project::PgProjectStore;
    use ontok::db::snapshot_store::PgStore;
    use ontok::domain::snapshot::{EntityType, ExtractedBy, LinkType, Property, Snapshot};

    let pool = common::test_pool().await;
    let ps = PgProjectStore::new(pool.clone());
    let store = PgStore::new(pool.clone());
    store.migrate().await.expect("建表失败");
    let p = common::fresh_project(&ps, prefix).await;
    let v = ps.new_version(p.id).await.expect("建版本失败");

    let note = if poisoned {
        // 「主键」是守门词；写成**不含 ASCII 标识符**的形态，保证它到达闸门
        // （带 `order_no` 这类会被 REQ-104 的前处理整段隐去 —— 那测的就不是闸了）。
        Some("本表的主键服务中心，按业务口径解读".to_string())
    } else {
        Some("一张订单".to_string())
    };

    let mut snap = Snapshot::new(v.id, v.seq);
    snap.extracted_by = Some(ExtractedBy {
        agent: "inproc:ontok::agent::RuleBasedAgent".into(),
        version: "0.1.0".into(),
        round: 1,
        failed_groups: vec![],
        source: None,
    });
    snap.entity_types.push(EntityType {
        key: "order".into(),
        display_name: "订单".into(),
        description: note,
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
        key: "customer".into(),
        display_name: "客户".into(),
        description: None,
        aliases: vec![],
        parent: None,
        disjoint_with: vec![],
        properties: vec![],
        evidence: vec![],
    });
    snap.link_types.push(LinkType {
        key: "order_customer".into(),
        from_type: "order".into(),
        to_type: "customer".into(),
        cardinality: "N:1".into(),
        semantic: "belongs_to".into(),
        evidence: vec![],
        confidence: None,
    });
    store.save_snapshot(&snap).await.expect("写快照失败");
    ps.freeze(v.id).await.expect("冻结失败");
    (p.name, v.seq)
}

fn run_export(project: &str, version: i32, extra: &[&str]) -> std::process::Output {
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_ontok"));
    cmd.args(["ontology", "export", "--project", project, "--version", &version.to_string()]);
    cmd.args(extra);
    cmd.env(
        "ONTOK_DATABASE_URL",
        ontok::config::test_database_url().expect("缺 ONTOK_TEST_DATABASE_URL"),
    );
    cmd.output().expect("跑 ontok ontology export 失败")
}

/// AC-002（CLI 侧）：客户视图里带词条 ⇒ **exit≠0**，stdout **一个字节都没有**，
/// 词条只进 stderr。清掉之后同一版能导出。
#[tokio::test]
async fn live_cli_export_refuses_a_poisoned_note_with_a_nonzero_exit() {
    let (bad_project, bad_v) = live_frozen_version("req110bad", true).await;
    let out = run_export(&bad_project, bad_v, &[]);
    assert_eq!(
        out.status.code(),
        Some(ontok::cli::EXIT_LEAKED_MODELING_TERMS),
        "命中守门器必须以「拒绝出图」的退出码结束，实际 {:?}；stderr={}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stdout.is_empty(), "拒绝时 stdout 不许有任何字节");
    let stderr = String::from_utf8_lossy(&out.stderr);
    // 交付证据（`--nocapture` 可见）：AC-002 的**现场输出**。
    eprintln!(
        "AC-002 现场：project={bad_project} version={bad_v} · exit={:?} · stdout={} 字节\n\
         stderr: {}",
        out.status.code(),
        out.stdout.len(),
        stderr.trim()
    );
    assert!(stderr.contains("主键"), "stderr 必须报出命中的词条：{stderr}");
    assert!(stderr.contains("C6"), "stderr 应当说明是哪条出口：{stderr}");

    // 对照组（纪律 E）：同一套流程、干净说明 ⇒ exit 0 且 stdout 有内容。
    let (ok_project, ok_v) = live_frozen_version("req110ok", false).await;
    let ok = run_export(&ok_project, ok_v, &[]);
    assert_eq!(
        ok.status.code(),
        Some(0),
        "干净视图必须导出成功；stderr={}",
        String::from_utf8_lossy(&ok.stderr)
    );
    let text = String::from_utf8(ok.stdout).expect("TTL 必须是 UTF-8");
    assert!(text.starts_with("@prefix : <https://ontok.local/"), "{text}");
    assert!(text.contains("a owl:Class"), "{text}");
}

/// AC-003（CLI 侧）：**两次进程级导出**逐字节相同；`--out` 与 stdout 同一批字节。
#[tokio::test]
async fn live_cli_export_is_byte_identical_across_two_processes() {
    let (project, version) = live_frozen_version("req110det", false).await;

    let a = run_export(&project, version, &[]);
    let b = run_export(&project, version, &[]);
    assert_eq!(a.status.code(), Some(0));
    assert_eq!(b.status.code(), Some(0));
    assert_eq!(a.stdout, b.stdout, "两次导出必须逐字节相同（AC-003）");
    assert!(!a.stdout.is_empty());

    // `--out` 写的字节 == stdout 的字节（R5 的落点，也是 C6/C7 共用一份产物的证据）。
    let dir = std::env::temp_dir().join(format!("req110-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("建临时目录失败");
    let file = dir.join("out.ttl");
    let c = run_export(&project, version, &["--out", file.to_str().unwrap()]);
    assert_eq!(c.status.code(), Some(0), "stderr={}", String::from_utf8_lossy(&c.stderr));
    assert!(c.stdout.is_empty(), "--out 时 stdout 必须留空（脚本友好）");
    let written = std::fs::read(&file).expect("读不到 --out 产物");
    assert_eq!(written, a.stdout, "--out 与 stdout 必须是同一批字节");
    let _ = std::fs::remove_dir_all(&dir);
}


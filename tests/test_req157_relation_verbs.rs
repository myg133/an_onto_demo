//! REQ-157 · **关系动词字典 + 校验命令 + skill/schema 对齐**的专属守卫（P1 + P2）。
//!
//! # 这一单要治的病
//!
//! v3 的 34 条关系用了 15 种动词：同义并存（`attached_to` / `attaches_to`、
//! `binds` / `binds_to`）、泛词（`uses` / `contains` / `associated_with`），
//! 加上**抽取契约的两份文件互相打架**（skill 说「用具体动宾短语」，
//! `parse.rs` 的 schema 示例却把三个词写死成枚举）⇒ agent 自由发挥。
//! 根因不是 UI，是**没有词汇表可校验**。
//!
//! # 本文件钉住的判据
//!
//! | # | 判据 | 怎么钉 |
//! | --- | --- | --- |
//! | AC-001 | 坏字典（重复别名 / 别名撞主键 / 缺字段 / 重复主键）⇒ **拒绝加载并点名** | 真进程 `--verbs` |
//! | AC-002 | 造 `uses` ⇒ `forbidden` + **exit 非 0** | 真进程 + 临时快照 |
//! | AC-003 | 造 `attaches_to` ⇒ 归一 `attached_to` + **exit 0** + 报告里有 `alias_normalized` | 真进程 |
//! | AC-004 | 造 `xyz_unknown` ⇒ `unknown` + **恰好 3 个候选** + exit 非 0 | 真进程 |
//! | AC-005 | 字典接入后 REQ-156 的 15 个动词语义**一个不变** | 逐词对表 |
//! | AC-006 | `parse.rs` 里再也搜不到那个枚举；同一段能看出「要查字典」 | 源码 |
//! | AC-010 | 只把英文语义放进 `skos:altLabel` ⇒ **真闸拒绝并点名** | `render(C6, …)` |
//! | AC-011 | `/api/catalog` 的 `semantic` 仍是**英文机器键** | `build_items` + 源码未改 |
//!
//! ⚠️ 本文件不加 `//! @guard-manifest: REQ-081`（与 REQ-107/156/158 的同类文件一致）。

mod common;

use ontok::domain::pipeline::{render, OutletId, Product};
use ontok::domain::relation_verbs::{load_str, Classified, IssueKind, Report};
use ontok::domain::snapshot::{EntityType, LinkType, Property, Snapshot};
use ontok::domain::ttl::{serialize_customer, TtlOptions, DEFAULT_BASE};
use ontok::domain::view::{
    scan_modeling_terms, CustomerView, EntityView, PropertyView, RelationView,
};
use std::path::{Path, PathBuf};
use std::process::Output;

// ══════════════════════════════════════════════════════════════
// 夹具
// ══════════════════════════════════════════════════════════════

/// 每次运行一个独立临时目录（`--snapshot` / `--verbs` 都是本地文件）。
struct Sandbox {
    dir: PathBuf,
}

impl Sandbox {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("req157-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("建临时目录失败");
        Self { dir }
    }

    fn write(&self, name: &str, body: &str) -> PathBuf {
        let p = self.dir.join(name);
        std::fs::write(&p, body).expect("写临时文件失败");
        p
    }

    /// 一份**单条关系**的快照形状 JSON（`link_types[]`）。
    fn snapshot(&self, name: &str, semantic: &str) -> PathBuf {
        let body = format!(
            r#"{{"entity_types":[],
                 "link_types":[{{"key":"alpha_beta","from_type":"alpha","to_type":"beta",
                                  "cardinality":"N:1","semantic":"{semantic}"}}]}}"#
        );
        self.write(name, &body)
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// 真进程：`ontok-builder relation verbs check …`（与用户敲的是同一个入口）。
fn run_check(args: &[&std::ffi::OsStr]) -> Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_ontok-builder"))
        .arg("relation")
        .arg("verbs")
        .arg("check")
        .args(args)
        .output()
        .expect("跑 ontok-builder relation verbs check 失败")
}

fn p(path: &Path) -> &std::ffi::OsStr {
    path.as_os_str()
}

/// stdout 必须是**可解析的 JSON**（报告双份：JSON 走 stdout、人话走 stderr）。
fn json_of(out: &Output) -> serde_json::Value {
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "stdout 不是合法 JSON（{e}）。stdout={:?}\nstderr={}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    })
}

fn stderr_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

// ══════════════════════════════════════════════════════════════
// AC-001 · 坏字典 ⇒ 拒绝加载并点名
// ══════════════════════════════════════════════════════════════

#[test]
fn ac001_bad_dictionaries_are_refused_and_the_offender_is_named() {
    let sb = Sandbox::new("ac001");
    let good_snapshot = sb.snapshot("ok.json", "belongs_to");

    // ① 同一别名出现在两个主键下
    let dup_alias = sb.write(
        "dup-alias.json",
        r#"{"version":"1.0.0",
            "verbs":{"belongs_to":{"label":"归属于","aliases":["binds"]},
                     "binds_to":{"label":"绑定到","aliases":["binds"]}},
            "forbidden":[]}"#,
    );
    // ② 别名与某个主键同名
    let alias_clash = sb.write(
        "alias-clash.json",
        r#"{"version":"1.0.0",
            "verbs":{"binds_to":{"label":"绑定到","aliases":["depends_on"]},
                     "depends_on":{"label":"依赖","aliases":[]}},
            "forbidden":[]}"#,
    );
    // ③ 缺 `version`
    let no_version = sb.write("no-version.json", r#"{"verbs":{"has":{"label":"拥有","aliases":[]}}}"#);
    // ④ 缺 `verbs`
    let no_verbs = sb.write("no-verbs.json", r#"{"version":"1.0.0","forbidden":[]}"#);
    // ⑤ 主键重复（serde 会静默覆盖 ⇒ 必须自己扫原文才发现）
    let dup_key = sb.write(
        "dup-key.json",
        r#"{"version":"1.0.0",
            "verbs":{"has":{"label":"拥有","aliases":[]},
                     "has":{"label":"有了","aliases":[]}},
            "forbidden":[]}"#,
    );

    for (label, path, want) in [
        ("重复别名", dup_alias, "binds"),
        ("别名撞主键", alias_clash, "depends_on"),
        ("缺 version", no_version, "version"),
        ("缺 verbs", no_verbs, "verbs"),
        ("主键重复", dup_key, "`has`"),
    ] {
        let out = run_check(&["--snapshot".as_ref(), p(&good_snapshot), "--verbs".as_ref(), p(&path)]);
        let err = stderr_of(&out);
        eprintln!("AC-001 [{label}] exit={:?}\nstderr: {}", out.status.code(), err.trim());
        assert!(
            !out.status.success(),
            "坏字典（{label}）必须被拒绝加载，实际 exit={:?}；stderr={err}",
            out.status.code()
        );
        assert!(
            err.contains(want),
            "坏字典（{label}）的报错必须**点名** `{want}`，实际：{err}"
        );
        assert!(
            err.contains("拒绝加载"),
            "报错要说清是「拒绝加载」而不是别的失败（{label}）：{err}"
        );
        assert!(
            out.stdout.is_empty(),
            "字典都没加载成功，不该输出任何报告（{label}）：{:?}",
            String::from_utf8_lossy(&out.stdout)
        );
    }

    // 反空转：**好字典照样能加载**（否则上面五条只是「什么都不能加载」）。
    let out = run_check(&["--snapshot".as_ref(), p(&good_snapshot)]);
    assert!(
        out.status.success(),
        "默认（嵌入）字典应当能加载并放行 belongs_to；stderr={}",
        stderr_of(&out)
    );
}

/// 库级：`load_str` 的错误也要点名（CLI 只是转发它的报错）。
#[test]
fn ac001_dictionary_loader_names_the_offending_token() {
    let e = load_str(
        r#"{"version":"1.0.0",
            "verbs":{"attached_to":{"label":"挂载于","aliases":["attaches_to"]},
                     "binds_to":{"label":"绑定到","aliases":["attaches_to"]}},
            "forbidden":[]}"#,
    )
    .expect_err("同一别名出现在两个主键下必须拒绝");
    let msg = e.to_string();
    assert!(msg.contains("attaches_to"), "要点名别名：{msg}");
    assert!(msg.contains("attached_to") && msg.contains("binds_to"), "要点名两个主键：{msg}");
}

// ══════════════════════════════════════════════════════════════
// AC-002 / AC-003 / AC-004 · 三类结论 + 退出码
// ══════════════════════════════════════════════════════════════

/// AC-002 · 造 `uses` ⇒ `forbidden` 命中且 **exit 非 0**。
#[test]
fn ac002_uses_is_forbidden_and_exits_non_zero() {
    let sb = Sandbox::new("ac002");
    let snap = sb.snapshot("uses.json", "uses");
    let out = run_check(&["--snapshot".as_ref(), p(&snap)]);
    let json = json_of(&out);
    eprintln!("AC-002 exit={:?} json={json}", out.status.code());

    assert_ne!(out.status.code(), Some(0), "泛词 `uses` 必须让命令非 0 退出");
    assert_eq!(json["verdict"], "fail");
    assert_eq!(json["counts"]["forbidden"], 1, "要报出一条 forbidden：{json}");
    assert_eq!(json["issues"][0]["kind"], "forbidden");
    assert_eq!(json["issues"][0]["semantic"], "uses");
    // 人话那份也要说清是「泛词」而不是「未知词」。
    let err = stderr_of(&out);
    assert!(err.contains("forbidden"), "人话报告要点名类别：{err}");
    assert!(err.contains("uses"), "人话报告要点名那个词：{err}");
}

/// AC-003 · 造 `attaches_to` ⇒ 归一 `attached_to`、**exit 0**、报告里有 `alias_normalized`。
#[test]
fn ac003_attaches_to_is_normalized_and_exits_zero() {
    let sb = Sandbox::new("ac003");
    let snap = sb.snapshot("attaches.json", "attaches_to");
    let out = run_check(&["--snapshot".as_ref(), p(&snap)]);
    let json = json_of(&out);
    eprintln!("AC-003 exit={:?} json={json}", out.status.code());

    assert_eq!(out.status.code(), Some(0), "别名只是提示，不许阻塞：{}", stderr_of(&out));
    assert_eq!(json["verdict"], "pass");
    assert_eq!(json["counts"]["alias_normalized"], 1, "报告里必须有 alias_normalized：{json}");
    assert_eq!(json["issues"][0]["kind"], "alias_normalized");
    assert_eq!(json["issues"][0]["semantic"], "attaches_to");
    assert_eq!(json["issues"][0]["canonical"], "attached_to", "必须归一到主键");
    assert_eq!(json["issues"][0]["label"], "挂载于");
}

/// AC-004 · 造 `xyz_unknown` ⇒ `unknown` + **3 个候选** + exit 非 0。
#[test]
fn ac004_unknown_gets_three_candidates_and_exits_non_zero() {
    let sb = Sandbox::new("ac004");
    let snap = sb.snapshot("unknown.json", "xyz_unknown");
    let out = run_check(&["--snapshot".as_ref(), p(&snap)]);
    let json = json_of(&out);
    eprintln!("AC-004 exit={:?} json={json}", out.status.code());

    assert_ne!(out.status.code(), Some(0), "字典外的词必须让命令非 0 退出");
    assert_eq!(json["counts"]["unknown"], 1);
    assert_eq!(json["issues"][0]["kind"], "unknown");
    let cands = json["issues"][0]["candidates"].as_array().expect("候选必须是数组");
    assert_eq!(cands.len(), 3, "AC-004 要求**恰好 3 个**候选：{cands:?}");
    for c in cands {
        assert!(
            c.as_str().is_some_and(|s| !s.is_empty()),
            "候选必须是非空字符串：{c}"
        );
    }
    let err = stderr_of(&out);
    assert!(err.contains("候选"), "人话报告里要给出候选：{err}");
}

/// `--snapshot` 认三种形状：快照 `link_types[]` / 提案 `links[]` / **客户视图**
/// `relations[]`（后者的值已经过 `semantic_cn`，是中文标签）。
///
/// 「认客户视图」不是锦上添花：发布前想拿它自检一份**将要发出去的**视图时，
/// 若中文标签被报成 `unknown`，这条命令就没人敢用（34 条假 unknown）。
#[test]
fn snapshot_accepts_all_three_shapes_and_labels_are_not_unknown() {
    let sb = Sandbox::new("shapes");

    // ① 提案形状（原始英文 key）
    let proposal = sb.write(
        "proposal.json",
        r#"{"version":"1.0.0","items":[],
            "links":[{"key":"alpha_beta","from_type":"alpha","to_type":"beta","semantic":"binds"}]}"#,
    );
    let out = run_check(&["--snapshot".as_ref(), p(&proposal)]);
    let json = json_of(&out);
    assert_eq!(out.status.code(), Some(0), "别名不该阻塞：{}", stderr_of(&out));
    assert_eq!(json["counts"]["alias_normalized"], 1);

    // ② 客户视图形状（`meaning` 是**中文标签**）
    let view = sb.write(
        "view.json",
        r#"{"objects":[],"relations":[
            {"from":"订单","to":"客户","how_many":"多对一","meaning":"归属于"},
            {"from":"磁盘","to":"实例","how_many":"多对多","meaning":"挂载于"},
            {"from":"监听器","to":"后端","how_many":"多对多","meaning":"绑定到"}]}"#,
    );
    let out = run_check(&["--snapshot".as_ref(), p(&view)]);
    let json = json_of(&out);
    eprintln!("客户视图形状：exit={:?} json={json}", out.status.code());
    assert_eq!(
        out.status.code(),
        Some(0),
        "一份**全中文**的客户视图必须放行（把标签报成 unknown 等于没人敢用这条命令）：{}",
        stderr_of(&out)
    );
    assert_eq!(json["counts"]["unknown"], 0);
    assert_eq!(json["clean"], 3);
    assert_eq!(json["source"]["kind"], "snapshot");
}

/// 认不出的 JSON 形状要**报错并点名三种形状**，⛔ 不许「猜一个像语义的字段」。
#[test]
fn snapshot_with_an_unknown_shape_is_an_error_not_a_silent_pass() {
    let sb = Sandbox::new("shape-bad");
    let bad = sb.write("bad-shape.json", r#"{"things":[{"whatever":"uses"}]}"#);
    let out = run_check(&["--snapshot".as_ref(), p(&bad)]);
    let err = stderr_of(&out);
    eprintln!("坏形状：exit={:?} stderr={err}", out.status.code());
    assert!(!out.status.success(), "认不出形状必须失败，不许静默「0 条关系 = 通过」");
    assert!(out.stdout.is_empty(), "失败时不该产出报告 JSON");
    for want in ["link_types", "links", "relations"] {
        assert!(err.contains(want), "报错要点名它认的三种形状（缺 `{want}`）：{err}");
    }
}

// ══════════════════════════════════════════════════════════════
// AC-005 · 字典接入后，REQ-156 的 15 个动词标签一个不变
// ══════════════════════════════════════════════════════════════

#[test]
fn ac005_semantic_labels_are_unchanged_after_wiring_the_dictionary() {
    // REQ-156 的原始映射表（19 个 key / 15 种动词 + 两对同义 + 三个泛词）。
    let table: [(&str, &str); 19] = [
        ("has", "拥有"),
        ("belongs_to", "归属于"),
        ("depends_on", "依赖"),
        ("references", "关联到"),
        ("records", "记录了"),
        ("derived_from", "算自"),
        ("binds_to", "绑定到"),
        ("binds", "绑定到"),
        ("attached_to", "挂载于"),
        ("attaches_to", "挂载于"),
        ("notifies_via", "通知到"),
        ("applies_to", "适用于"),
        ("targets", "指向"),
        ("bills", "计费到"),
        ("based_on", "基于"),
        ("deployed_to", "部署于"),
        ("uses", "使用"),
        ("contains", "包含"),
        ("associated_with", "关联"),
    ];
    for (raw, want) in table {
        let label = ontok::domain::view::semantic_label(raw)
            .unwrap_or_else(|| panic!("{raw} 应当有中文标签"));
        assert_eq!(label, want, "{raw} 的中文标签变了（REQ-156 的客户视图会跟着变意思）");
        assert!(
            !label.chars().any(|c| c.is_ascii_alphabetic()),
            "{raw} 的标签含 ASCII：{label}"
        );
    }

    // 同义归一：别名与主键**同一个**标签（字典与 match 两条路径必须一致）。
    assert_eq!(
        ontok::domain::view::semantic_label("attaches_to"),
        ontok::domain::view::semantic_label("attached_to")
    );
    assert_eq!(
        ontok::domain::view::semantic_label("binds"),
        ontok::domain::view::semantic_label("binds_to")
    );

    // 兜底没变：认不出的词仍原样返回（第四档守卫靠它判「漏翻」）。
    assert_eq!(ontok::domain::view::semantic_label("aggregates"), None);

    // 字典新增的动词现在也能翻（`forwards_to` 不在 REQ-156 的 15 个里）。
    assert_eq!(
        ontok::domain::view::semantic_label("forwards_to").as_deref(),
        Some("转发到")
    );
    assert_eq!(
        ontok::domain::view::semantic_label("forwarded_to").as_deref(),
        Some("转发到"),
        "别名也要能翻（字典 aliases）"
    );
}

// ══════════════════════════════════════════════════════════════
// AC-006 · schema 示例不再枚举，且指向字典
// ══════════════════════════════════════════════════════════════

#[test]
fn ac006_parse_rs_no_longer_enumerates_verbs_and_points_to_the_dictionary() {
    let src = common::read_source("src/agent/acp/parse.rs");
    // ① 那个枚举**一个字都不许留**（它就是「两份契约打架」的那一头）。
    assert!(
        !src.contains("has 或 belongs_to 或 references"),
        "`parse.rs` 里还留着旧的动词枚举 —— agent 会照着它自由发挥"
    );
    // ② 同一段必须能看出「要去查字典」，并且带上 forbidden 这条硬约束。
    assert!(
        src.contains("relation-verbs.json"),
        "schema 示例里没指向 `relation-verbs.json`，agent 不知道该查哪本字典"
    );
    assert!(
        src.contains("forbidden"),
        "规则区没提 `forbidden` —— 泛词照旧会被写出来"
    );
    assert!(
        src.contains("aliases"),
        "规则区没提 `aliases` —— 同义写法不会被归一"
    );
    // ③ 示例值本身**不是**任何一个真实动词（避免又变成一份隐式枚举）。
    assert!(
        src.contains("<从 relation-verbs.json 的 key 里选一个，别自己造>"),
        "`semantic` 的示例值应当是「去查字典」的占位，而不是某个具体词"
    );
}

// ══════════════════════════════════════════════════════════════
// AC-010 · 只把英文放进 `skos:altLabel` ⇒ 真闸拒绝并点名
// ══════════════════════════════════════════════════════════════

fn opts() -> TtlOptions<'static> {
    TtlOptions {
        project: "样例项目",
        version: 3,
        base: DEFAULT_BASE,
        action_type_count: 0,
        tenancy: None,
        schema: None,
        api_classes: None,
    }
}

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

/// **只**把英文语义放进 `skos:altLabel`、`rdfs:comment` 用中文 ⇒ 真闸必须拒绝并点名。
///
/// 这份 TTL 是**手搓的**，但走的是真闸：`render(C6, Product::Text(..))` 与
/// `CustomerTurtle` 调的是**同一个** `gate_text(spec, doc.body())` —— 闸的职责与
/// 「上游到底怎么渲染的」无关（上游换渲染器 / 手工数据 / 新抽取器都可能在闸前
/// 把英文写进 altLabel）。
///
/// 旧代码在这一条上**放行**（今天靠「同一份文档里 `语义：` 先命中」遮住 ——
/// 那是巧合，不是防线）。
#[test]
fn ac010_english_only_in_altlabel_is_refused_by_the_real_gate() {
    let doc = "\
:rel_1 a owl:ObjectProperty ;
    rdfs:label \"订单 → 客户\"@zh ;
    rdfs:comment \"语义：归属于（多对一）\"@zh ;
    skos:altLabel \"attaches_to\"@zh ;
    rdfs:domain :订单 ;
    rdfs:range :客户 .
";
    // 反空转：夹具的 comment 确实是**中文**（否则测的就不是 altLabel 这一档）。
    assert!(doc.contains("语义：归属于（多对一）"));
    assert!(!doc.contains("语义：attaches_to"));

    let hits = scan_modeling_terms(doc);
    eprintln!("AC-010 单元：hits={hits:?}");
    assert!(
        hits.iter().any(|h| h.contains("attaches_to")),
        "只写在 altLabel 里的英文语义没被抓住 —— 这正是 REQ-158 登记的遗留 fail-open：{hits:?}"
    );
    assert!(
        hits.iter().any(|h| h.contains("未翻译")),
        "命中理由必须说清是「未翻译」：{hits:?}"
    );

    // 真闸（C6 = 客户视图 TTL 的出口；`Product::Text` 与 `CustomerTurtle` 共用 `gate_text`）。
    let err = render(OutletId::C6, Product::Text(doc.to_string()))
        .expect_err("只把英文放进 altLabel 的客户视图必须被拒绝发布（fail-closed）");
    eprintln!("AC-010 真闸：outlet={} leak={} hits={:?}", err.outlet(), err.is_leak(), err.hits());
    assert!(err.is_leak(), "拒绝理由必须是「泄露了建模术语」");
    assert_eq!(err.outlet(), "C6");
    assert!(
        err.hits().iter().any(|h| h.contains("attaches_to")),
        "错误信息必须点名那个词：{:?}",
        err.hits()
    );

    // 对照组：同一个位置换成中文 ⇒ 放行。
    let ok_doc = doc.replace("skos:altLabel \"attaches_to\"@zh", "skos:altLabel \"挂载于\"@zh");
    render(OutletId::C6, Product::Text(ok_doc)).expect("中文 altLabel 必须放行");
}

/// ⛔ **不许误杀真实 v3**：实体别名（`NAT` / `Certificates` …）也写进 `skos:altLabel`，
/// 它们不是「漏翻的语义」。
///
/// 实测（2026-10-27 · 真实 v3 `cloud-resource`）：不加形状门时导出 **exit=2**
/// 并把 `NAT` 报成「未翻译的关系语义」。那条判据会把**每一份真实客户视图**
/// 都拒掉（`/api/ontology.ttl` 一起挂）。
#[test]
fn ac010_real_entity_aliases_in_altlabel_are_not_mistaken_for_semantics() {
    // ① 单元：实体别名形态（大写 / 驼峰 / 中文+大写）**不进**这一档。
    for ok in ["NAT", "Certificates", "SSL 证书", "COS 存储桶", "NatGateway", "CDB 实例"] {
        let doc = format!(
            ":alpha a owl:Class ;\n    skos:altLabel \"{ok}\"@zh .\n"
        );
        let hits = scan_modeling_terms(&doc);
        assert!(
            hits.is_empty(),
            "实体别名「{ok}」被误判成未翻译的语义 ⇒ 真实客户视图会全部拒发：{hits:?}"
        );
    }
    // 反空转：同一个位置换成**机器动词形态**就必须抓（否则上面那条恒绿）。
    for bad in ["uses", "belongs_to", "aggregates", "attaches_to"] {
        let doc = format!(":rel_1 a owl:ObjectProperty ;\n    skos:altLabel \"{bad}\"@zh .\n");
        assert!(
            scan_modeling_terms(&doc).iter().any(|h| h.contains(bad)),
            "机器动词形态的 altLabel「{bad}」必须被抓"
        );
    }

    // ② 端到端：把真实 v3 那种别名（`NAT`）灌进客户视图，真序列化 + 真闸必须**放行**。
    let mut view = view_with_meaning("归属于");
    view.objects[1].aliases = vec!["NAT".into(), "Certificates".into()];
    let doc = serialize_customer(&view, &opts());
    assert!(
        doc.body().contains("skos:altLabel \"NAT\"@zh"),
        "夹具没把 `NAT` 写进 altLabel，本判据测不到东西：\n{}",
        doc.body()
    );
    assert!(doc.body().contains("语义：归属于（多对一）"));
    render(OutletId::C6, Product::CustomerTurtle { view: &view, opts: opts() })
        .expect("实体别名里有英文，不许把整份客户视图拒掉");
}

// ══════════════════════════════════════════════════════════════
// AC-011 · `/api/catalog` 的 `semantic` 仍是**英文机器键**
// ══════════════════════════════════════════════════════════════

#[test]
fn ac011_catalog_semantic_stays_the_english_machine_key() {
    // ① 运行时：本体模式下 `semantic` 原样是英文 key，中文只在 `semantic_label`。
    let mut snap = Snapshot::new(uuid::Uuid::new_v4(), 3);
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
        semantic: "belongs_to".into(),
        evidence: vec![],
        confidence: None,
        join_verified: None,
        join_evidence: None,
    });

    let model = ontok::domain::model_view::project_model_track(&snap);
    let items = ontok::query::catalog::build_items(&model, None, None);
    let rel = items
        .iter()
        .find_map(|i| match i {
            ontok::query::catalog::CatalogItem::Relation(r) => Some(r),
            _ => None,
        })
        .expect("本体模式下关系必须被列出");
    eprintln!("AC-011 catalog: semantic={:?} label={:?}", rel.semantic, rel.semantic_label);
    assert_eq!(rel.semantic, "belongs_to", "⛔ `semantic` 必须仍是英文机器键（REQ-156 的产品决定）");
    assert_eq!(
        rel.semantic_label.as_deref(),
        Some("归属于"),
        "中文只放在 `semantic_label` 里"
    );

    // ② 源码：`src/query/catalog.rs` 与 `develop` **零差异** —— 这一单一个字都没改它。
    let root = common::crate_root();
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["diff", "--numstat", "develop", "--", "src/query/catalog.rs"])
        .output()
        .expect("跑不起 git（本判据依赖 git，与 test_req107_red_lines 同一口径）");
    let numstat = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        numstat.trim().is_empty(),
        "⛔ `src/query/catalog.rs` 被改了 —— `semantic` 不许因为「统一中文」而改成中文：\n{numstat}"
    );
}

// ══════════════════════════════════════════════════════════════
// 报告形状 · 双份输出 + 计数口径
// ══════════════════════════════════════════════════════════════

/// JSON 与「人话」**同时**产出：stdout 是纯 JSON（可 `| jq`），stderr 是给人看的。
#[test]
fn the_report_is_double_sided_and_the_counts_are_consistent() {
    let sb = Sandbox::new("double");
    // 五条关系：2 条干净、1 条 forbidden、1 条别名、1 条 unknown。
    let snap = sb.write(
        "mixed.json",
        r#"{"link_types":[
            {"key":"a","from_type":"alpha","to_type":"beta","semantic":"belongs_to"},
            {"key":"b","from_type":"alpha","to_type":"beta","semantic":"uses"},
            {"key":"c","from_type":"alpha","to_type":"beta","semantic":"binds"},
            {"key":"d","from_type":"alpha","to_type":"beta","semantic":"xyz_unknown"},
            {"key":"e","from_type":"alpha","to_type":"beta","semantic":"references"}]}"#,
    );
    let out = run_check(&["--snapshot".as_ref(), p(&snap)]);
    let json = json_of(&out);
    let err = stderr_of(&out);
    eprintln!("双份：exit={:?}\nstdout(JSON)=\n{json}\nstderr(人话)=\n{err}", out.status.code());
    assert_eq!(json["relations"], 5);
    assert_eq!(json["clean"], 2);
    assert_eq!(json["counts"]["forbidden"], 1);
    assert_eq!(json["counts"]["alias_normalized"], 1);
    assert_eq!(json["counts"]["unknown"], 1);
    assert_eq!(json["counts"]["deprecated"], 0);
    assert!(err.contains("关系动词校验"), "stderr 必须是那份人话报告：{err}");
    assert!(err.contains("结论："), "人话报告要有结论行：{err}");
    assert_ne!(out.status.code(), Some(0));
}

/// 库级：`contains` 是 **deprecated**（不是 forbidden），别名归一不阻塞。
///
/// 这条钉的是「别把 `contains` 放进 `forbidden`」—— 真实 v3 里就有一条
/// `contains`，放进去等于把**已冻结的历史数据**判成新错误。
#[test]
fn deprecated_is_a_warning_and_never_blocks() {
    let dict = ontok::domain::relation_verbs::embedded().expect("嵌入字典必须能加载");
    match dict.classify("contains") {
        Classified::Primary { deprecated, .. } => {
            assert!(deprecated.is_some(), "`contains` 必须标 deprecated");
        }
        other => panic!("`contains` 应当是主键（deprecated），实际 {other:?}"),
    }
    assert!(
        !dict.forbidden.iter().any(|f| f == "contains"),
        "⛔ `contains` 不许进 forbidden —— 真实 v3 有一条，放进去会让 v3 直接判死"
    );

    let r = Report::build(
        dict,
        vec![
            ("contains".to_string(), occ(0)),
            ("binds".to_string(), occ(1)),
            ("belongs_to".to_string(), occ(2)),
        ],
    );
    assert_eq!(r.count(IssueKind::Deprecated), 1);
    assert_eq!(r.count(IssueKind::AliasNormalized), 1);
    assert!(!r.blocking(), "只有 deprecated + alias_normalized ⇒ 不许阻塞");
    assert_eq!(r.clean(), 1);
}

fn occ(i: usize) -> ontok::domain::relation_verbs::Occurrence {
    ontok::domain::relation_verbs::Occurrence {
        location: format!("link_types[{i}]"),
        from: Some("a".into()),
        to: Some("b".into()),
    }
}

/// 字典文件**随仓库发布**且**编进二进制**：`include_str!` 的路径由
/// `src/domain/relation_verbs.rs` 决定 ⇒ 仓库根那份必须存在、且是同一份内容。
///
/// ⚠️ 路径走 [`common::crate_root`]（运行时锚点），**不用** `env!("CARGO_MANIFEST_DIR")`
/// —— 后者被 `tests/test_anchor_guard.rs` 的 G-1 全域禁止（除锚点文件外）。
#[test]
fn the_dictionary_file_ships_with_the_repo_and_is_the_one_embedded() {
    let path = common::crate_root().join("relation-verbs.json");
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("仓库根的 relation-verbs.json 不见了（{e}）：{}", path.display()));
    let dict = load_str(&raw).expect("仓库根那份字典必须能加载");
    assert_eq!(dict.version, "1.0.0");
    assert_eq!(dict.verbs.len(), 16, "§三 的字典有 16 个主键");
    assert_eq!(dict.forbidden.len(), 6);
    // 嵌入那份与磁盘这份**必须是同一版**（否则 builder 试的与服务面跑的不是一本字典）。
    let embedded = ontok::domain::relation_verbs::embedded().expect("嵌入字典必须能加载");
    assert_eq!(&dict, embedded, "磁盘与嵌入的字典内容不一致");
}

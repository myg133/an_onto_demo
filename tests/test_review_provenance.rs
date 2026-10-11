//! REQ-060 · **提案级 `extracted_by` 在 FDE 通道的呈现**（AC-001 ~ AC-004）。
//!
//! 测试名带 `test_review_provenance_` 前缀。
//!
//! # 本文件是「纯逻辑」那一半（零 I/O）
//!
//! 真库那一半在 `test_review_show_e2e.rs`（`ProposalRow.extracted_by` 走 JSONB
//! 往返）。本文件直接喂 `serde_json::Value` —— 因为**可疑形态**（空串 /
//! 纯空白 / 缺 `agent` / JSON null）在真库里很难自然造出来，
//! 而那正是 AC-003 要抓的东西。
//!
//! # 与 `test_review_view.rs` 的分工
//!
//! - `test_review_view.rs` = 两轨分界 / 守门（REQ-056 / REQ-057）；
//! - 本文件 = 归属的**三态呈现**与**提案级**这一性质（REQ-060）。

use ontok::domain::review::{Decision, ItemKind, ItemView};
use ontok::domain::review_view::{
    audit_fde_channel, project_review_track_with_summary_and_provenance, render_review_text,
    verify_provenance_scope, verify_track_separation, ProposalProvenanceView, ProvenanceState,
    ReviewTrackView, PROVENANCE_LINE_PREFIX, PROVENANCE_SCOPE_NOTE,
};
use ontok::domain::view::scan_modeling_terms;
use serde_json::{json, Value};

// ── fixture ──────────────────────────────────────────────

/// 三条条目 —— 数量 > 1 是关键：
/// **「逐条目重复打印」这个误导只有在 N>1 时才现形**。
fn fixture_items() -> Vec<ItemView> {
    vec![
        ItemView {
            id: "1:add_entity_type:客户".into(),
            kind: ItemKind::AddEntityType,
            target: "客户".into(),
            from_type: None,
            to_type: None,
            biz_status: Decision::Accepted,
            model_status: Decision::Accepted,
            payload: json!({
                "key": "customer",
                "display_name": "客户",
                "properties": [{ "name": "客户编号", "column": "customer_pk" }],
                "evidence": [{ "table": "t_customer", "column": "customer_no", "claim": "主键列名一致" }]
            }),
        },
        ItemView {
            id: "2:add_entity_type:客户订阅".into(),
            kind: ItemKind::AddEntityType,
            target: "客户订阅".into(),
            from_type: None,
            to_type: None,
            biz_status: Decision::Accepted,
            model_status: Decision::Accepted,
            payload: json!({
                "key": "subscription",
                "display_name": "客户订阅",
                "properties": [{ "name": "客户编号", "column": "customer_id" }]
            }),
        },
        ItemView {
            id: "3:add_link_type:客户 客户订阅".into(),
            kind: ItemKind::AddLinkType,
            target: "客户 客户订阅".into(),
            from_type: Some("customer".into()),
            to_type: Some("subscription".into()),
            biz_status: Decision::Accepted,
            model_status: Decision::Pending,
            payload: json!({
                "key": "customer_has_subscription",
                "from_type": "customer",
                "to_type": "subscription",
                "cardinality": "1:N"
            }),
        },
    ]
}

fn baseline() -> std::collections::BTreeSet<String> {
    ["customer".to_string(), "subscription".to_string()].into_iter().collect()
}

/// 提案级归属 → （投影, 渲染文本）。
fn view_and_text(extracted_by: Option<&Value>) -> (ReviewTrackView, String) {
    let items = fixture_items();
    let (view, _s) =
        project_review_track_with_summary_and_provenance(&items, &baseline(), extracted_by);
    let text = render_review_text(&view);
    (view, text)
}

/// **人为构造的夹具** —— 刻意**不是**真实产物的副本。
///
/// # 为什么不用真实取值
///
/// 夹具该用什么值**由它测什么决定**，不由「今天产物里恰好是什么」决定：
/// 本函数喂的是 `Declared` 这一态的**渲染契约** —— 只要 `agent` 是非空串，
/// 渲染器就必须打出 `agent=<值>`。这个机制对**任意**非空串都成立，
/// 所以「拿它证明真实产物的形状」是**名不副实**的说法。
/// （真实形状由真库那一半证：`test_review_show_e2e.rs` 走 JSONB 往返。）
///
/// 取值刻意与真实产物**不同** —— 夹具长得一眼就不像实测原文，
/// 才不会有人把它当证据引用。
///
/// # 真实产物里那份归属长什么样（**对照**，不是本夹具的取值）
///
/// - `extracted_by.agent` = `inproc:ontok::agent::RuleBasedAgent`
/// - `extracted_by.version` = `0.1.0`
///
/// 那两个值由**代码自己**给出（[`ontok::agent::factory::INPROC_EXECUTOR_ID`] /
/// `_VERSION`），不是谁登记上去的。
///
/// ⚠️ 它们会随实现漂移 ⇒ 这段注释就会变成**假事实**（REQ-061 F-1 就是这么烂掉的）。
/// 所以 [`test_review_provenance_fixture_comment_names_the_real_agent`]
/// 直接读本文件**源码**来核对：注释里出现的 `inproc:` 取值必须**逐字**等于那个常量。
fn declared_eb() -> Value {
    json!({ "agent": "mcode-agent", "version": "2026.10.07", "round": 1, "failed_groups": [] })
}

// ── AC-001 · 夹具注释不许腐烂（REQ-061 F-1）─────────────

/// 本文件源码本身（`include_str!`）—— 用来读**注释里的字**。
const THIS_FILE: &str = include_str!("test_review_provenance.rs");

/// 从源码里取出 `fn declared_eb` 紧上面那一整段文档注释（`///` 行）。
///
/// 找不到锚点就 `panic` —— 一个「取不到东西就算通过」的判据**恒为真**，
/// 那正是本会话反复栽的坑。
fn doc_comment_above_declared_eb(src: &str) -> String {
    let anchor = src
        .lines()
        .position(|l| l.starts_with("fn declared_eb("))
        .expect("本文件里必须有 `fn declared_eb(` —— 改名了就连同本函数一起改");
    let mut doc: Vec<&str> = Vec::new();
    for l in src.lines().take(anchor).collect::<Vec<_>>().into_iter().rev() {
        match l.trim().strip_prefix("///") {
            Some(rest) => doc.push(rest),
            None => break,
        }
    }
    doc.reverse();
    doc.join("\n")
}

/// 【AC-001 · 注释腐烂可检测】**注释里那份真实取值必须与代码给出的常量一致**。
///
/// 这条是 F-1 的**真正价值**：注释里的取值一旦与
/// [`ontok::agent::factory::INPROC_EXECUTOR_ID`] 对不上，**测试会响**，
/// 而不是靠人记得去改注释。
#[test]
fn test_review_provenance_fixture_comment_names_the_real_agent() {
    let doc = doc_comment_above_declared_eb(THIS_FILE);
    let real_agent = ontok::agent::factory::INPROC_EXECUTOR_ID;
    let real_version = ontok::agent::factory::INPROC_EXECUTOR_VERSION;

    assert!(!doc.is_empty(), "没读到 `declared_eb` 的文档注释 ⇒ 本判据取不到东西（恒为真）");

    // 注释里出现的**每一个** `inproc:` 取值，都必须逐字等于那个常量。
    // 只查「含不含」不够 —— 注释里同时留着新旧两个值时那也会「含」。
    let mut quoted: Vec<&str> = Vec::new();
    for tok in doc.split_whitespace() {
        let tok = tok.trim_matches(|c: char| {
            !c.is_ascii_alphanumeric() && c != ':' && c != '_' && c != '.'
        });
        if tok.strip_prefix("inproc:").is_some_and(|suffix| !suffix.is_empty()) {
            // ⚠️ 只认「`inproc:` + 至少一个字符」—— 注释里把 `inproc:` 当**前缀**
            // 举例（``凡是 `inproc:` 开头的取值``）不算引用了一个取值。
            quoted.push(tok);
        }
    }
    assert!(
        !quoted.is_empty(),
        "注释里没有写出真实 agent 取值 ⇒ 对照信息丢了（腐不腐烂都看不出来）"
    );
    for q in quoted {
        assert_eq!(
            q, real_agent,
            "注释里那份 agent 取值已与 INPROC_EXECUTOR_ID 对不上（{q:?} vs {real_agent:?}）⇒ 注释腐烂"
        );
    }

    assert!(
        doc.contains(real_version),
        "注释里没有写出真实 version 取值（应为 {real_version}）⇒ 对照信息丢了或已腐烂"
    );

    // 反向也钉住：夹具**不许**被改成照抄真实值 ——
    // 夹具该用什么值由它测什么决定；照抄真实值会让「人为构造」这句话
    // 变成另一句没人核对的假事实（那正是 F-1 的病根）。
    assert_ne!(
        declared_eb().get("agent").and_then(|v| v.as_str()),
        Some(real_agent),
        "夹具取值被改成了真实产物取值 ⇒ 夹具开始跟着产物漂移"
    );
}

// ── AC-001 · 显示提案级归属 ─────────────────────────────

/// 【AC-001】**有 `extracted_by` ⇒ 输出必须含它**。
#[test]
fn test_review_provenance_declared_is_visible() {
    let (view, text) = view_and_text(Some(&declared_eb()));
    assert_eq!(
        view.provenance.state,
        ProvenanceState::Declared {
            agent: "mcode-agent".into(),
            version: Some("2026.10.07".into()),
            round: Some(1),
            failed_groups: vec![],
        }
    );
    assert!(text.contains("agent=mcode-agent"), "输出里找不到归属\n实际输出：\n{text}");
    assert!(text.contains("version=2026.10.07"), "输出里找不到版本\n实际输出：\n{text}");
}

/// 【AC-001】**`extracted_by` 为 `None`（SQL NULL = 人工修订）⇒ 明确表述**。
///
/// AC 明写：不能打空、不能省略、不能打 `None` 字面量。
#[test]
fn test_review_provenance_none_never_renders_empty_or_none_literal() {
    let (view, text) = view_and_text(None);
    assert_eq!(view.provenance.state, ProvenanceState::HumanRevised);

    assert!(text.contains("人工修订"), "None 归属没有明确表述\n实际输出：\n{text}");
    // AC 点名的三种失败形态，一个都不许有。
    assert!(!text.contains("agent=None"), "打了 `None` 字面量\n实际输出：\n{text}");
    assert!(!text.contains("extracted_by=None"), "打了 `None` 字面量\n实际输出：\n{text}");
    // 「打空」= 归属行只剩前缀 + 括号，什么实质内容都没有。
    let line = text
        .lines()
        .find(|l| l.contains(PROVENANCE_LINE_PREFIX))
        .expect("归属行必须存在（AC-001：不许省略）");
    let body = line.trim_start().trim_start_matches(PROVENANCE_LINE_PREFIX);
    assert!(
        body.len() > 20,
        "归属行几乎打空了（body={body:?}）—— 那与「没有这一行」对 FDE 是同一件事"
    );
}

/// 【AC-001 · 区分度】**「没提供」不许显示成「人工修订」**。
///
/// 两者对 FDE 的含义**相反**：人工修订 = 有人审过；没提供 = 我不知道。
/// 混起来会给出**假安心** —— 比不给更危险。
#[test]
fn test_review_provenance_not_provided_is_distinct_from_human_revised() {
    // 两参入口 = 调用方压根没接归属。
    let items = fixture_items();
    let (view, _s) =
        ontok::domain::review_view::project_review_track_with_summary(&items, &baseline());
    let text = render_review_text(&view);

    assert_eq!(view.provenance.state, ProvenanceState::NotProvided);
    assert!(text.contains("未提供"), "未提供状态没有明确表述\n实际输出：\n{text}");
    assert!(
        !text.contains("人工修订，ADR-001"),
        "「没提供」被说成了「人工修订」\n实际输出：\n{text}"
    );
}

/// 【AC-001 · REQ-061 F-3】**终端输出里不许出现 Markdown 标记**。
///
/// 终端不渲染 Markdown ⇒ `**不是**` 会带着星号原样打给 FDE 看。
/// ⚠️ 只管**终端输出**：源码注释里的 `**…**` 是合适的（那边确实渲染 Markdown），
/// 所以这条判据查的是**渲染出来的文本**，不是源码。
#[test]
fn test_review_provenance_rendered_text_has_no_markdown_markers() {
    let cases: Vec<(&str, Option<Value>)> = vec![
        ("有归属", Some(declared_eb())),
        ("无归属（SQL NULL）", None),
        ("可疑（agent 空串）", Some(json!({ "agent": "" }))),
        ("可疑（缺 agent）", Some(json!({ "version": "1.0" }))),
    ];
    for (name, eb) in &cases {
        let (_view, text) = view_and_text(eb.as_ref());
        let bad: Vec<&str> = text.lines().filter(|l| l.contains("**")).collect();
        assert!(bad.is_empty(), "{name} 的终端输出里出现了 Markdown 星号：{bad:?}\n实际输出：\n{text}");
    }
}

// ── AC-002 · 措辞说清是提案级 ───────────────────────────

/// 【AC-002】**措辞必须让 FDE 知道这是整份提案的归属**。
#[test]
fn test_review_provenance_wording_says_proposal_level() {
    let (_view, text) = view_and_text(Some(&declared_eb()));
    assert!(
        text.contains(PROVENANCE_SCOPE_NOTE),
        "措辞没说清这是提案级\n实际输出：\n{text}"
    );
    assert!(
        text.contains("提案级抽取归属"),
        "归属行没有标明是提案级\n实际输出：\n{text}"
    );
}

/// 【AC-002 · 判别力自证】**变异「把提案级归属逐条目重复打印」⇒ 必须被抓住**。
///
/// 这是 AC 点名的**误导**变体：会让 FDE 以为每个条目由不同 agent 抽。
/// 变体是**改渲染器**（真的重复打），不是在造输入。
#[test]
fn test_review_provenance_per_item_repetition_is_caught() {
    let (view, _good) = view_and_text(Some(&declared_eb()));

    // 变异：把归属行逐条目重复 —— 3 个条目 ⇒ 3 行。
    let mutant = format!(
        "整体通过：否\n{PROVENANCE_LINE_PREFIX}agent=mcode-agent（{PROVENANCE_SCOPE_NOTE}）\n  [1] entity · 状态=Pending\n{PROVENANCE_LINE_PREFIX}agent=mcode-agent\n  [2] entity · 状态=Pending\n{PROVENANCE_LINE_PREFIX}agent=mcode-agent\n  [3] link · 状态=Pending\n"
    );

    let violations = verify_provenance_scope(&view, &mutant);
    assert!(
        !violations.is_empty(),
        "「逐条目重复打印」这个误导没有被抓住（判据无判别力）\n变异输出：\n{mutant}"
    );
    assert!(
        violations.iter().any(|v| v.contains("应恰好 1 行")),
        "违规理由没指出「重复打印」：{violations:?}\n变异输出：\n{mutant}"
    );

    // 对照：未变异的渲染必须**通过**同一条判据（否则判据恒为真）。
    let (_v2, good) = view_and_text(Some(&declared_eb()));
    assert!(
        verify_provenance_scope(&view, &good).is_empty(),
        "未变异的渲染被判违规（判据恒为真 ⇒ 无判别力）：{:?}",
        verify_provenance_scope(&view, &good)
    );
}

/// 【AC-002 · 判别力自证】**变异 M2b「归属只拼进第 1 个条目头行」⇒ 必须被抓住**。
///
/// 这是 QA 实测的**存活变异**：规则①/②/③ 都躲得过它 ——
///  - ① 全文里带归属前缀的行仍然**恰好 1 行**（只是那一行变了形状）；
///  - ② 条目头行 `  [1] … · 状态=…` **不含**两轨前缀，
///    而且它排在第一条两轨行**之前**（行号更小）⇒ 不违反「排在条目之后」；
///  - ③ 那一行里仍然含 `agent=<值>` ⇒ 判定词照样出现。
///
/// 所以必须另有一条规则说「归属**独占一行**」（判据③ · 规则④）。
#[test]
fn test_review_provenance_glued_into_item_header_is_caught() {
    let (view, good) = view_and_text(Some(&declared_eb()));

    // 对照：未变异的渲染必须通过同一条判据（否则判据恒为真，无判别力）。
    let ok = verify_provenance_scope(&view, &good);
    assert!(ok.is_empty(), "未变异的渲染被判违规（判据恒为真 ⇒ 无判别力）：{ok:?}\n实际输出：\n{good}");

    let header = good
        .lines()
        .find(|l| l.trim_start().starts_with("[1]"))
        .expect("渲染里必须有第 1 个条目头行")
        .to_string();
    let prov_line = good
        .lines()
        .find(|l| l.contains(PROVENANCE_LINE_PREFIX))
        .expect("未变异输出里必须有归属行")
        .to_string();

    // 变异 M2b：归属**不独占一行**，只拼进第 1 个条目头行（` · ` 分隔）。
    let mutant: String = good
        .lines()
        .filter(|l| !l.contains(PROVENANCE_LINE_PREFIX))
        .map(|l| {
            if *l == header {
                format!("{header} · {}", prov_line.trim_start())
            } else {
                (*l).to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";

    // 论证前提真的成立：① 没有被触发（否则抓到的可能是①，规则④就白测了）。
    assert_eq!(
        mutant.lines().filter(|l| l.contains(PROVENANCE_LINE_PREFIX)).count(),
        1,
        "这个变异不该动归属行数 —— 动了我就抓不到 M2b 本身\n变异输出：\n{mutant}"
    );

    let violations = verify_provenance_scope(&view, &mutant);
    assert!(
        !violations.is_empty(),
        "「归属只拼进第 1 个条目头行」没有被抓住（判据无判别力）\n变异输出：\n{mutant}"
    );
    assert!(
        violations.iter().any(|v| v.contains("条目头行")),
        "违规理由没指出「被拼进条目头行」：{violations:?}\n变异输出：\n{mutant}"
    );
}

/// 【AC-002】**归属行不得混进两轨**，也不得排在条目之后。
#[test]
fn test_review_provenance_line_never_enters_the_two_tracks() {
    let (view, text) = view_and_text(Some(&declared_eb()));
    assert!(
        verify_track_separation(&view, &text).is_empty(),
        "加了归属之后两轨分界被判违规：{:?}\n实际输出：\n{text}",
        verify_track_separation(&view, &text)
    );
    // 业务轨行里一个归属词都不许有。
    for l in text.lines().filter(|l| l.contains("业务轨：")) {
        assert!(
            !l.contains("agent="),
            "业务轨行混进了归属：{l}\n那会让客户视图的建模术语守门失效"
        );
    }
}

// ── AC-003 · 形态可疑 ⇒ 显式标注 ───────────────────────

/// 【AC-003】**可疑形态各一条 ⇒ 必须被标注**。
///
/// 这一条同时是**「不许写『有就打、没有就打空』」**的执行：
/// 每一种可疑形态都有一个**独立**的判据输入，
/// 而它们的输出**必须彼此不同**（否则「可疑」也是一句空话）。
#[test]
fn test_review_provenance_suspicious_shapes_are_labelled() {
    let cases: Vec<(&str, Value)> = vec![
        ("空串 agent", json!({ "agent": "" })),
        ("纯空白 agent", json!({ "agent": "   \t " })),
        ("缺 agent", json!({ "version": "1.0", "round": 1 })),
        ("agent 不是字符串", json!({ "agent": 42 })),
        ("不是对象", json!("mcode-agent")),
        ("JSON null", json!(null)),
    ];

    let mut rendered: Vec<String> = Vec::new();
    for (name, eb) in &cases {
        let (view, text) = view_and_text(Some(eb));
        assert!(
            matches!(view.provenance.state, ProvenanceState::Suspected { .. }),
            "形态可疑却没有被标注（{name}）：{:?}",
            view.provenance.state
        );
        assert!(
            text.contains("形态可疑"),
            "形态可疑却没有在输出里说明（{name}）\n实际输出：\n{text}"
        );
        // 关键：可疑时**不许照打** agent 值 —— 空串照样打出去 = 没标注。
        assert!(
            !text.contains("agent=（未声明）"),
            "可疑形态被当成合法归属渲染了（{name}）\n实际输出：\n{text}"
        );
        // 判据：这一行**正确标注了**可疑 ⇒ 判据应当**放行**。
        //
        // ⚠️ 方向很容易搞反：`verify_provenance_scope` 报的是
        //「呈现没做到位」（缺行 / 重复 / 没表态），
        // 而「这一行被标成了可疑」恰恰是**做到了位**。
        // 所以此处必须为空 —— 反过来，
        // 「把可疑形态照打成合法归属」才会让判据报违规（见下面的变异）。
        assert!(
            verify_provenance_scope(&view, &text).is_empty(),
            "正确标注了可疑，判据却报违规（判据方向反了）：{:?}\n实际输出：\n{text}",
            verify_provenance_scope(&view, &text)
        );
        rendered.push(text);
    }

    // 变异：**照打**可疑形态（把 `agent: ""` 当成合法归属渲染）⇒ 必须被抓住。
    {
        let (view, _good) = view_and_text(Some(&json!({ "agent": "" })));
        // 把「形态可疑」那一段替换成一条看起来正常的合法归属行。
        let good_line = view_and_text(Some(&declared_eb())).1;
        let prov_line = good_line
            .lines()
            .find(|l| l.contains(PROVENANCE_LINE_PREFIX))
            .expect("未变异输出里必须有归属行")
            .to_string();
        let (v2, t2) = view_and_text(Some(&json!({ "agent": "" })));
        let mutant = t2.replace(
            t2.lines()
                .find(|l| l.contains(PROVENANCE_LINE_PREFIX))
                .unwrap(),
            &prov_line,
        );
        // 对照：判据在**未变异**的那份上必须放行（否则恒为真，无判别力）。
        assert!(
            verify_provenance_scope(&view, &_good).is_empty(),
            "正确标注了可疑却报违规（判据恒为真）"
        );
        let violations = verify_provenance_scope(&v2, &mutant);
        assert!(
            !violations.is_empty(),
            "「照打可疑形态」没有被抓住（判据无判别力）\n变异输出：\n{mutant}"
        );
    }

    // 六种可疑各自的原因**互不相同** ⇒ 「可疑」不是一句套话。
    let reasons: std::collections::BTreeSet<String> = cases
        .iter()
        .map(|(_, eb)| match ProposalProvenanceView::from_extracted_by(Some(eb)).state {
            ProvenanceState::Suspected { reason, .. } => reason,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        reasons.len(),
        cases.len(),
        "可疑原因有重复 ⇒ 判据分不出形态：{reasons:?}"
    );
}

/// 【AC-003 · 区分度】**「可疑」不许与「人工修订」混为一谈**。
#[test]
fn test_review_provenance_suspicious_is_not_human_revised() {
    let (view, text) = view_and_text(Some(&json!({ "agent": "" })));
    assert!(matches!(view.provenance.state, ProvenanceState::Suspected { .. }));
    // 「写了但写得不对」与「压根没写」对 FDE 是相反的结论。
    assert!(
        !text.contains("人工修订，ADR-001"),
        "可疑形态被说成了「人工修订」（给出假安心）\n实际输出：\n{text}"
    );
}

/// 【AC-003】**失败分组非空必须显式说出来** —— 「抽出来了」≠「全抽出来了」。
#[test]
fn test_review_provenance_failed_groups_are_surfaced() {
    let eb = json!({
        "agent": "mcode-agent",
        "version": "1.0",
        "round": 2,
        "failed_groups": ["t_orders", "t_payments"]
    });
    let (_view, text) = view_and_text(Some(&eb));
    assert!(text.contains("2 个分组抽取失败"), "失败分组没说出来\n实际输出：\n{text}");
    assert!(text.contains("t_orders") && text.contains("t_payments"), "实际输出：\n{text}");
}

// ── AC-004 · 不破坏既有守门与通道 ───────────────────────

/// 【AC-004 · 核心】**`premise_holds` 不得因为「显示归属」而变真**。
///
/// 这是 AC 点名的坑：把归属塞进**建模轨**、而 `sourced_fields` 判的是
/// 「有 payload 来源且非空的建模字段」⇒ `premise_holds` 因**渲染产物**变真
/// ⇒ REQ-057 的解耦被破坏。
///
/// 做法：造一份**建模栏全空**的条目集（有 key 就没有 —— 所以连 key 都不给），
/// 分别在「无归属」与「有归属」两种输入下问 `premise_holds`，
/// **两个取值必须逐 bit 相同**。
#[test]
fn test_review_provenance_premise_holds_is_bit_identical_with_and_without_provenance() {
    // 一条**连 key 都没有**的条目：`sourced_fields()` 必然全空 ⇒
    // 无归属时 `premise_holds == false`。
    let bare = vec![ItemView {
        id: "1:add_entity_type:裸条目".into(),
        kind: ItemKind::AddEntityType,
        target: "裸条目".into(),
        from_type: None,
        to_type: None,
        biz_status: Decision::Pending,
        model_status: Decision::Pending,
        payload: json!({ "display_name": "裸条目" }),
    }];

    let without = ontok::domain::review_view::project_review_track_with_summary_and_provenance(
        &bare,
        &baseline(),
        None,
    );
    let with = ontok::domain::review_view::project_review_track_with_summary_and_provenance(
        &bare,
        &baseline(),
        Some(&declared_eb()),
    );

    let a_without = audit_fde_channel(&without.0);
    let a_with = audit_fde_channel(&with.0);

    assert!(
        !a_without.premise_holds,
        "前提基线不成立（建模栏全空时 premise_holds 应当为 false）：{:?}",
        a_without.hits
    );
    // ⭐ AC-004 的独立核实：两个取值逐 bit 相同。
    assert_eq!(
        a_without.premise_holds, a_with.premise_holds,
        "premise_holds 因为「多打了归属」而变化 ⇒ REQ-057 的解耦被破坏\n无归属：{:?}\n有归属：{:?}",
        a_without, a_with
    );
    // hits 也必须一模一样（不只是那个 bool）。
    assert_eq!(
        a_without.hits, a_with.hits,
        "hits 因为「多打了归属」而变化 ⇒ 判据与渲染产物耦合了"
    );
    // 另一侧：有建模数据的输入下，两者都必须为 true（不许被「归属」压成 false）。
    let (full_a, _ta) = view_and_text(None);
    let (full_b, _tb) = view_and_text(Some(&declared_eb()));
    assert_eq!(
        audit_fde_channel(&full_a).premise_holds,
        audit_fde_channel(&full_b).premise_holds,
        "有建模数据时，归属的有无改变了 premise_holds"
    );
    assert!(audit_fde_channel(&full_b).premise_holds);
}

/// 【AC-004】**`sourced_fields` 压根没有「归属」这一栏** —— 结构性的，不是约定。
#[test]
fn test_review_provenance_sourced_fields_has_no_provenance_entry() {
    let (view, _t) = view_and_text(Some(&declared_eb()));
    for it in &view.items {
        let fields = it.model.sourced_fields();
        for f in &fields {
            assert!(
                !f.contains("agent") && !f.contains("proven") && !f.contains("by"),
                "sourced_fields 里出现了归属相关的栏 {f:?} ⇒ AC-004 的解耦被破坏"
            );
        }
        // 渲染标记同理：归属不许混进建模轨的 marker 列表。
        for m in it.model.sourced_markers() {
            assert!(
                !m.contains("agent="),
                "sourced_markers 里出现了归属标记 {m:?} ⇒ 归属混进了建模轨"
            );
        }
    }
}

/// 【AC-004】**客户视图的建模术语守门一条不许动**。
///
/// 归属那一行走 FDE 通道，**不过**守门（那里建模术语本就是要被裁决的内容）。
/// 但业务轨那一侧必须仍然干净 —— 否则「两轨分离」当场失效。
#[test]
fn test_review_provenance_biz_track_still_passes_modeling_term_guard() {
    let (_view, text) = view_and_text(Some(&declared_eb()));
    let biz: Vec<&str> = text.lines().filter(|l| l.contains("业务轨：")).collect();
    assert!(!biz.is_empty(), "fixture 没有业务轨行（判据恒为真？）");
    let blob = biz.join("\n");
    let leaked = scan_modeling_terms(&blob);
    assert!(leaked.is_empty(), "业务轨侧泄露建模术语：{leaked:?}\n实际内容：\n{blob}");
}

/// 【AC-004】**两轨分界判据零退化**：加了归属之后仍逐行可分。
#[test]
fn test_review_provenance_track_separation_survives() {
    for (name, eb) in [
        ("无归属", None),
        ("有归属", Some(declared_eb())),
        ("可疑归属", Some(json!({ "agent": "" }))),
    ] {
        let (view, text) = view_and_text(eb.as_ref());
        let v = verify_track_separation(&view, &text);
        assert!(v.is_empty(), "{name} 之下两轨分界退化：{v:#?}\n实际输出：\n{text}");
        assert!(
            verify_provenance_scope(&view, &text).is_empty(),
            "{name} 之下归属判据报违规：{:?}",
            verify_provenance_scope(&view, &text)
        );
    }
}

// ── 交付物：贴实测原文 ──────────────────────────────────

/// 【展示用】**打印四种状态下的真实输出**（交付物，非判据）。
#[test]
fn test_review_provenance_print_all_four_states() {
    let show = |name: &str, text: &str| println!("\n===== REQ-060 · {name} =====\n{text}===== 结束 =====\n");

    let (_v, t) = view_and_text(Some(&declared_eb()));
    // ⚠️ REQ-061 F-1：这一份的 agent 取值是**人为构造的夹具**（见 `declared_eb`），
    // 不是实测原文 —— 标签不许再写成「真实产物形态」，那正是 F-1 的那句假事实。
    show("Declared（夹具形态：agent 取值人为构造）", &t);

    let (_v, t) = view_and_text(None);
    show("HumanRevised（SQL NULL = 人工修订）", &t);

    // NotProvided 走两参入口（调用方压根没提供）。
    let items = fixture_items();
    let (view, _s) =
        ontok::domain::review_view::project_review_track_with_summary(&items, &baseline());
    show("NotProvided（这条路径没接归属）", &render_review_text(&view));

    let (_v, t) = view_and_text(Some(&json!({ "agent": "" })));
    show("Suspected（agent 空串）", &t);

    let (_v, t) = view_and_text(Some(&json!({ "version": "1.0", "round": 1 })));
    show("Suspected（缺 agent）", &t);
}

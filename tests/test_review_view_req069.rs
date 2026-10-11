//! REQ-069 · **payload 不得能注入「行结构」**（换行不转义 ⇒ 一个字节能造出一整行）。
//!
//! 测试名带 `test_req069_` 前缀。
//!
//! # 这条修的是**渲染层的一个性质**，不是某条判据的缺口
//!
//! `verify_track_separation` / `verify_provenance_scope` / `is_item_header_line`
//! 全是**纯句法**的，且它们读的是**渲染后的行**。于是 payload 里一个换行
//! 就能凭空造出一整行，那一行每个字节都来自 payload，却被判据当成
//! 渲染器自己打的结构行 —— 实测能造出**理由全错的误报**。
//!
//! # 三条测试各管什么
//!
//! | 测试 | 性质 | 杀谁 |
//! | --- | --- | --- |
//! | [`test_req069_payload_cannot_add_a_line_anywhere`] | **通用**：把换行塞进**每一个** payload 栏，渲染行数必须与干净基线**逐条相等** | **M1**（不处理换行） |
//! | [`test_req069_forged_line_looking_like_a_real_structural_line_is_neutralised`] | **M4（灵魂）**：payload 造出**带完整前缀与等号**的伪结构行 | **M4**（弱处理，如「只缩进」） |
//! | [`test_req069_carriage_return_is_neutralised_too`] | LF 与 CR **分别**都被挡 | **M2 / M3**（只挡一个） |
//!
//! # 为什么行数是**通用**判据（而不是逐栏各写一条）
//!
//! AC-002 要求**修在渲染层**、一次免疫所有判据 ⇒ 测试也必须是**一条**通用断言：
//! 「换行进任何一栏，都不许让渲染结果的**行数**变多」。
//! 逐栏各写一条 = 把「N 个判据各加一层检查」这个错误搬进测试。
//! 语料由 [`vecine_forged_line_payloads`] 生成 ⇒ 加一栏新 payload 栏也自动被覆盖。

use ontok::domain::review::{Decision, ItemKind, ItemView};
use ontok::domain::review_view::{
    project_review_track_with_summary_and_provenance, render_review_text, verify_provenance_scope,
    verify_track_separation, MODEL_LINE_PREFIX, PROVENANCE_LINE_PREFIX,
};
use serde_json::{json, Value};
use std::collections::BTreeSet;

/// LF 在渲染时的转义写法 —— 与被测实现**独立**写一遍
/// （测试不 import 被测实现里的私有常量；见 REQ-065 测试的同一纪律）。
const MARK_ESCAPE: char = '\u{2401}';
/// ␊ —— LF 的 Control Picture 转义。
const LINE_FEED_ESCAPE: char = '\u{240A}';
/// ␍ —— CR 的 Control Picture 转义。
const CARRIAGE_RETURN_ESCAPE: char = '\u{240D}';

// ── fixture ──────────────────────────────────────────────

fn item(target: &str, payload: Value) -> ItemView {
    ItemView {
        id: "1:add_entity_type:客户".into(),
        kind: ItemKind::AddEntityType,
        target: target.into(),
        from_type: None,
        to_type: None,
        biz_status: Decision::Accepted,
        model_status: Decision::Accepted,
        payload,
    }
}

/// 关系类条目，**两端取自 payload**（`it.from_type` / `it.to_type` 留空）。
///
/// ⚠️ 投影里 `it.from_type` **优先于** payload（见 `project_review_inner`），
/// 所以要测「payload 的 `from_type` 被转义」，就必须让 `it.from_type` 为空，
/// 否则那一栏走 `it.from_type` 那条路，payload 的值压根不渲染 ⇒ 测不到。
fn rel_item_ends_from_payload(target: &str, payload: Value) -> ItemView {
    ItemView {
        id: "1:add_link_type:客户_has_订单".into(),
        kind: ItemKind::AddLinkType,
        target: target.into(),
        from_type: None,
        to_type: None,
        biz_status: Decision::Accepted,
        model_status: Decision::Accepted,
        payload,
    }
}

fn declared_eb() -> Value {
    json!({"agent":"agent-a","version":"1","round":1,"source":"declared"})
}

fn view_and_text(
    items: &[ItemView],
    eb: Option<&Value>,
) -> (ontok::domain::review_view::ReviewTrackView, String) {
    let baseline: BTreeSet<String> = BTreeSet::new();
    let (v, _st) = project_review_track_with_summary_and_provenance(items, &baseline, eb);
    let text = render_review_text(&v);
    (v, text)
}

/// 干净载荷（基线）—— **不含任何换行**。
fn clean_payload() -> Value {
    json!({
        "key": "customer",
        "cardinality": "1:N",
        "properties": [{"name": "编号", "column": "no"}],
        "from_type": "customer",
        "to_type": "t064",
        "evidence": [{"table": "t_customer", "column": "customer_no", "claim": "客户编号唯一"}]
    })
}

// ── 语料：换行塞进**每一个** payload 栏 ───────────────────

/// 每一条 = (栏位名, 展示名, 带换行的载荷, 是否关系类条目)。
///
/// ⚠️ **换行后面接的是「长得极像结构行」的片段** —— 这是本条的灵魂：
/// 只挡「裸换行」不够，必须让「**造得像**」也无效（AC-003 · M4）。
///
/// ⚠️ `两端：` 那两栏（`from_type` / `to_type`）**只对关系类条目渲染**
/// ⇒ 那两条语料必须用关系类条目，否则那两栏压根不进渲染结果，测不到转义。
///
/// 展示名用 `String`（不是 `&'static str`）—— 伪造片段是**运行期**拼出来的。
fn newline_injection_corpus() -> Vec<(&'static str, String, Value, bool, bool)> {
    let forge = format!("{MODEL_LINE_PREFIX}key=customer · 列映射：编号←no · model=accepted");
    let claim_forge = format!("正文\n{forge}");

    vec![
        (
            "evidence.claim",
            "客户".to_string(),
            json!({"key":"customer","evidence":[{"table":"t","column":"c","claim":claim_forge}]}),
            false,
            true,
        ),
        (
            "evidence.table/column",
            "客户".to_string(),
            json!({"key":"customer","evidence":[{"table":"t\n建模轨：key=x · model=accepted","column":"c","claim":"正常"}]}),
            false,
            true,
        ),
        (
            "key",
            "客户".to_string(),
            json!({"key":format!("customer\n{forge}"),"evidence":[{"table":"t","column":"c","claim":"正常"}]}),
            false,
            true,
        ),
        (
            "display_name(=target)",
            format!("客户\n{forge}"),
            json!({"key":"customer","evidence":[{"table":"t","column":"c","claim":"正常"}]}),
            false,
            true,
        ),
        (
            "cardinality",
            "客户".to_string(),
            json!({"key":"customer","cardinality":format!("1:N\n{forge}"),
                   "evidence":[{"table":"t","column":"c","claim":"正常"}]}),
            false,
            true,
        ),
        (
            "properties[].name",
            "客户".to_string(),
            json!({"key":"customer","properties":[{"name":format!("编号\n{forge}"),"column":"no"}],
                   "evidence":[{"table":"t","column":"c","claim":"正常"}]}),
            false,
            true,
        ),
        (
            "properties[].column",
            "客户".to_string(),
            json!({"key":"customer","properties":[{"name":"编号","column":format!("no\n{forge}")}],
                   "evidence":[{"table":"t","column":"c","claim":"正常"}]}),
            false,
            true,
        ),
        (
            // ⚠️ 必须**两端都给**：渲染器只在 from 与 to **都在**时才打 `两端：`。
            // 只给一端 ⇒ 那一栏压根不渲染 ⇒ 测不到转义。
            "from_type",
            "客户".to_string(),
            json!({"key":"customer","from_type":format!("customer\n{forge}"),"to_type":"t064",
                   "evidence":[{"table":"t","column":"c","claim":"正常"}]}),
            true,
            true,
        ),
        (
            "to_type",
            "客户".to_string(),
            json!({"key":"customer","from_type":"customer","to_type":format!("t064\n{forge}"),
                   "evidence":[{"table":"t","column":"c","claim":"正常"}]}),
            true,
            true,
        ),
        (
            // ⚠️ 这条走的是 **serde_json 自己的**控制字符转义（不是 [`escape_payload_text`]）：
            // 非退化、非三段式的证据渲染成紧凑 JSON，而 serde_json 会把换行
            // 编码成 `\\n` 两字符序列 ⇒ **本来就造不出行**。
            // 保留它是为了守住「那条既有防护仍在」（REQ-065 模块文档记着这一格）。
            "evidence 非三段式(紧凑JSON兜底·serde_json 自带转义)",
            "客户".to_string(),
            json!({"key":"customer","evidence":[format!("普通字符串\n{forge}")]}),
            false,
            false, // ← 不走 escape_payload_text ⇒ 不要求出现 ␊
        ),
        (
            // ⚠️ 换行必须落在**三段式认得的栏**上（`table` / `column` / `claim`）。
            // 放在 `extra` 这种**渲染器根本不读的键**里 ⇒ 那段文本压根不进渲染结果，
            // 「看不到转义标记」是**既有行为**，不是被吞了（实测确认过）。
            // 这里刻意把换行放在 `claim` 上，即真会被读的那一栏。
            "evidence 对象的多余键(不进渲染·既有行为)",
            "客户".to_string(),
            json!({"key":"customer","evidence":[{"table":"t","column":"c","claim":"正常","extra":format!("\n{forge}")}]}),
            false,
            false, // ← 不进渲染 ⇒ 不要求出现 ␊
        ),
    ]
}

// ── AC-001 · 通用：换行不得改变行结构 ──────────────────────

/// 【AC-001 / AC-002】**payload 里的换行不得在渲染结果中增加任何一行**。
///
/// # 为什么这是**一条**测试而不是 N 条
///
/// AC-002 要求修在渲染层、一次免疫所有判据 ⇒ 判据必须**一条**：
/// 「换行进任何一栏，行数都不许变」。逐栏各写一条 = 把「N 份检查」搬进测试。
///
/// # 判别力（为什么它不是恒真）
///
/// 它比的是**同一个渲染器**在「带换行载荷」与「干净载荷」下的行数。
/// ⇒ 去掉换行处理（M1）时，带换行那一侧必然多出伪结构行 ⇒ **行数不等 ⇒ 红**。
/// 另附**前提断言**：载荷里**真的**有换行（否则本测试在恒真地上跑）。
#[test]
fn test_req069_payload_cannot_add_a_line_anywhere() {
    let baseline = view_and_text(&[item("客户", clean_payload())], Some(&declared_eb())).1;
    let base_lines = baseline.lines().count();

    // 前提（对照组本身）：干净载荷下三条判据都通过、基线行数已知。
    assert!(
        base_lines > 0,
        "干净载荷竟渲染出 0 行 ⇒ 基线不可用，后面的比较没有意义"
    );

    for (field, target, payload, relational, via_escape) in newline_injection_corpus() {
        // 关系类语料必须用「两端取自 payload」的关系类条目 ——
        // 否则 `两端：` 不渲染，或走 `it.from_type` 那条路 ⇒ payload 值测不到。
        let mk = if relational { rel_item_ends_from_payload } else { item };
        let base_payload = if relational {
            json!({"key":"customer","from_type":"customer","to_type":"t064",
                   "evidence":[{"table":"t","column":"c","claim":"正常"}]})
        } else {
            clean_payload()
        };
        let (_v, text) =
            view_and_text(&[mk(&target, payload.clone())], Some(&declared_eb()));
        let base_lines =
            view_and_text(&[mk("客户", base_payload)], Some(&declared_eb())).1.lines().count();

        // 前提：这批载荷里**真的**含换行 —— 否则该行语料是恒真的。
        // ⚠️ 注意 `payload.to_string()` 是 **JSON 编码**，换行在里面已是 `\\n`
        // 两字符序列 ⇒ **不能**拿它判「原串有没有换行」（恒假 ⇒ 永远红）。
        // 真正的换行只可能出现在**展示名**（`target`）里。
        let payload_has_lf = target.contains('\n') || payload.to_string().contains("\\n");
        assert!(
            payload_has_lf,
            "语料「{field}」里没有换行 ⇒ 该条语料证明不了任何事（恒真 = 无判别力）"
        );

        assert_eq!(
            text.lines().count(),
            base_lines,
            "payload 的「{field}」里的换行在渲染结果里多切出了 {} 行（基线 {base_lines} 行）\
             ⇒ payload 造出了行结构\n--- 渲染结果 ---\n{text}",
            text.lines().count() as i64 - base_lines as i64
        );

        // 换行必须**被转义**而不是**被删掉**：FDE 仍要看得见正文，
        // AC-001 只要求「造不成结构行」，不要求「内容消失」。
        //
        // ⚠️ 这条断言**依赖语料覆盖到该栏**：每条语料都刻意让换行落在
        // **确实会被渲染**的栏上（`两端：` 那两条用关系类条目）。
        // ⇒ 「看不到转义标记」就只可能是「被吞了」，不会是「没渲染」。
        // ⚠️ 只对**走 [`escape_payload_text`] 的那些栏**断言转义标记：
        // 紧凑 JSON 那条走的是 serde_json 自带的控制字符转义（`\n` 两字符），
        // 它的防护是**另一套机制**，不能拿 ␊ 去断言它。
        if via_escape {
            assert!(
                text.contains(LINE_FEED_ESCAPE),
                "「{field}」的换行既没造出新行、也没留下转义标记 ⇒ 它被整段吞掉了\
                 （AC-001 只保「不造行结构」，不该顺手把证据藏起来）\n--- 渲染结果 ---\n{text}"
            );
        }
        let _ = payload_has_lf;
    }
}

/// 【AC-001】**载荷前缀仍被转义**（REQ-058 的不变量没有被本条改坏）。
///
/// 换行转义是**加**在既有转义上的，不是**替换**它 —— 两条都得在。
#[test]
fn test_req069_degenerate_mark_escape_still_works() {
    let payload = json!({
        "key": "customer",
        "evidence": [{"table":"t","column":"c","claim":format!("a\nb{MARK_ESCAPE}c")}]
    });
    let (v, text) = view_and_text(&[item("客户", payload)], Some(&declared_eb()));
    // ␀ 定界符转义仍在（REQ-058）：载荷里的 ␁ 原样透出。
    assert!(
        text.contains(MARK_ESCAPE),
        "载荷里的 ␁ 不见了 ⇒ REQ-058 的定界符转义被本条改坏了\n{text}"
    );
    assert!(
        text.contains(LINE_FEED_ESCAPE),
        "换行转义不在 ⇒ 本条没生效\n{text}"
    );
    // 且退化判据仍认得出「这一条携带可复核依据」（真证据 ⇒ premise 仍成立）。
    assert!(
        ontok::domain::review_view::audit_fde_channel(&v).premise_holds,
        "真证据应当仍被算作「有建模数据」⇒ 转义把证据降级了\n{text}"
    );
}

// ── AC-003 · M4（灵魂）：造得像结构行也无效 ────────────────

/// 【AC-003 · M4】payload 造出的伪结构行**必须留在原行内**，不得独立成行。
///
/// # 为什么这条是灵魂
///
/// 「缩进 payload 行」这类**弱处理**能让裸换行看起来无害，
/// 但 payload 只要在换行后写出**带完整前缀与等号**的片段
/// （`建模轨：key=customer · 列映射：… · model=accepted`），
/// 缩进救不了它 —— 因为 `line_head_prefix_of` 先 `trim_start()` 再认前缀。
///
/// # 判别力
///
/// 本测试**同时**断言三件互相独立的事，任一不成立即红：
/// 1. 行数不变（伪结构行没有独立成行）；
/// 2. **没有任何一行**的**行首**是 `建模轨：`（纯句法判据也认不出它）；
/// 3. 两轨判据零违规（不会凭空造出理由全错的误报）。
#[test]
fn test_req069_forged_line_looking_like_a_real_structural_line_is_neutralised() {
    let forge = format!("{MODEL_LINE_PREFIX}key=customer · 列映射：编号←no · model=accepted");
    let payload = json!({
        "key": "customer",
        "evidence": [{"table":"t","column":"c","claim":format!("正文\n{forge}")}]
    });
    let (v, text) = view_and_text(&[item("客户", payload.clone())], Some(&declared_eb()));
    let baseline = view_and_text(&[item("客户", clean_payload())], Some(&declared_eb())).1;

    // 前提：载荷里真的有「换行 + 完整结构行片段」这个组合。
    assert!(
        payload.to_string().contains("\\n") && payload.to_string().contains("建模轨"),
        "语料不含「换行 + 建模轨前缀」⇒ 本测试证明不了任何事"
    );

    assert_eq!(
        text.lines().count(),
        baseline.lines().count(),
        "payload 造出的伪结构行独立成了行 ⇒ 弱处理（只挡裸换行）没修好 M4\n{text}"
    );

    // 纯句法判据也必须认不出它：**没有任何一行**以建模轨前缀开头（除渲染器自己打的那行）。
    let model_heads = text
        .lines()
        .filter(|l| l.trim_start().starts_with(MODEL_LINE_PREFIX))
        .count();
    assert_eq!(
        model_heads, 1,
        "渲染结果里有 {model_heads} 行以「{MODEL_LINE_PREFIX}」开头（应恰好 1 行）\
         ⇒ payload 造出了一个**带完整前缀**的伪结构行\n{text}"
    );

    // 且不得凭空造出误报。
    let tv = verify_track_separation(&v, &text);
    assert!(
        tv.is_empty(),
        "payload 的伪结构行造出了两轨分界的**误报**（理由全错）：{tv:#?}\n{text}"
    );
    let pv = verify_provenance_scope(&v, &text);
    assert!(
        pv.is_empty(),
        "payload 的伪结构行造出了归属判据的**误报**（理由全错）：{pv:#?}\n{text}"
    );
}

/// 【AC-003 · M4 的另一面】payload 在换行后写出**归属前缀**也不得独立成行。
///
/// M4 不只针对建模轨 —— `verify_provenance_scope` 的规则①数的是
/// 「行首带归属前缀的行」，所以归属前缀是同类的最高危伪造目标。
#[test]
fn test_req069_forged_provenance_line_is_neutralised() {
    let (v, text) = view_and_text(
        &[item("客户", clean_payload())],
        Some(&json!({"agent": format!("agent-a\n{PROVENANCE_LINE_PREFIX}agent=伪造")})),
    );
    let prov_heads = text
        .lines()
        .filter(|l| l.trim_start().starts_with(PROVENANCE_LINE_PREFIX))
        .count();
    assert_eq!(
        prov_heads, 1,
        "渲染结果里有 {prov_heads} 行以归属前缀开头（应恰好 1 行）\
         ⇒ payload 在归属名里用换行造出了第二条归属行\n{text}"
    );
    let pv = verify_provenance_scope(&v, &text);
    assert!(
        pv.is_empty(),
        "伪造的归属行造出了归属判据的**误报**：{pv:#?}\n{text}"
    );
}

// ── AC-003 · M2 / M3：LF 与 CR 分别都要挡 ─────────────────

/// 【AC-003 · M2/M3】**LF 与 CR 各自**都必须被转义 —— 只挡一个等于没挡。
///
/// # 为什么要分开测
///
/// 只处理 `\n` 时，`\r` 会原样进入渲染结果：**在终端里 `\r` 是回车**
/// ⇒ 视觉上同样能把一行「顶掉」，造出与换行等价的错位。
/// 只处理 `\r` 时，`\n` 更是直接切出新行。
#[test]
fn test_req069_carriage_return_is_neutralised_too() {
    let baseline = view_and_text(&[item("客户", clean_payload())], Some(&declared_eb())).1;

    for (ch, esc, name) in [('\n', LINE_FEED_ESCAPE, "LF"), ('\r', CARRIAGE_RETURN_ESCAPE, "CR")] {
        let payload = json!({
            "key":"customer",
            "evidence":[{"table":"t","column":"c",
                         "claim":format!("正文{ch}{MODEL_LINE_PREFIX}key=x · model=accepted")}]
        });
        let (_v, text) = view_and_text(&[item("客户", payload)], Some(&declared_eb()));

        assert_eq!(
            text.lines().count(),
            baseline.lines().count(),
            "{name} 未被挡住 ⇒ 渲染结果多出了伪结构行\n{text}"
        );
        assert!(
            text.contains(esc),
            "{name} 的转义标记 {esc:?} 不在渲染结果里 ⇒ 只处理了另一个字符\n{text}"
        );
        // 真换行一个都不许**多**出来（那才是「造行结构」的直接证据）。
        // ⚠️ 基线里渲染器**自己**就打了 N 个行尾换行（N = 行数）——
        // 所以不能断言「只剩 1 个」，只能断言「不多于基线」。
        let base_newlines = baseline.chars().filter(|c| *c == ch).count();
        let got_newlines = text.chars().filter(|c| *c == ch).count();
        assert!(
            got_newlines <= base_newlines,
            "渲染结果里有 {got_newlines} 个未转义的 {name}（干净基线 {base_newlines} 个）\
             ⇒ {name} 逃过了转义、混进了渲染结果\n{text:?}"
        );
    }
}

// ── AC-002 · 只有一套转义出口 ────────────────────────────

/// 本文件源码本身 —— 用来**数**转义实现有几套。
const SRC: &str = include_str!("../src/domain/review_view.rs");

/// 【AC-002】**转义出口只有一处** —— REQ-058 那个函数已被**吸收**，不是并排留下。
///
/// AC 原文：「两套实现必然漂移，而『两套实现不一致』正是 REQ-058 要消灭的那类 bug」。
/// 本条把「只有一套」钉成可执行断言 —— 否则漂移不会被任何人发现。
#[test]
fn test_req069_one_escape_exit_exists() {
    // ① REQ-058 的 `escape_evidence_mark` 已被吸收进 `escape_payload_text`。
    assert!(
        !SRC.contains("fn escape_evidence_mark("),
        "`escape_evidence_mark` 又冒出来了 ⇒ 存在两套转义实现（AC-002 禁止）"
    );
    assert_eq!(
        SRC.matches("fn escape_payload_text(").count(),
        1,
        "转义函数的定义应当只有 1 处"
    );
    // ② 两个转义符号**同区**（Control Pictures U+2400 起）⇒ 是同一套机制的扩展。
    for (name, code) in [
        ("DEGENERATE_EVIDENCE_ESCAPE", "\u{2401}"),
        ("LINE_FEED_ESCAPE", "\u{240A}"),
        ("CARRIAGE_RETURN_ESCAPE", "\u{240D}"),
    ] {
        assert!(
            SRC.contains(code),
            "转义符号 {name} 不在源码里 ⇒ 换行转义可能另造了一套机制"
        );
    }
    // ③ 守卫自检：把「唯一出口」这条守卫施加一次变异，它**必须能红**。
    let guard = |s: &str| !s.contains("fn escape_evidence_mark(") && s.contains("fn escape_payload_text(");
    assert!(guard(SRC), "守卫对真实源码不通过 ⇒ 守卫写错了");
    let mutant = SRC.replace("fn escape_payload_text(", "fn escape_evidence_mark(");
    assert!(
        !guard(&mutant),
        "守卫杀不掉「恢复成两套实现」的变异 ⇒ 它恒为真（永远绿 = 无防护）"
    );
}

// ── AC-002 · 转义**发生过**（REQ-071：判据从「写法」换成「值」）──

/// 毒载荷的哨兵 —— 两侧各一个，中间夹一个**裸 LF**。
///
/// 两侧相同是为了让「这一位还在不在」可判定（载荷被整段吞掉 ⇒ 哨兵消失）；
/// 中间那个**裸 LF** 才是被测的那一刀。
const POISON: &str = "<<POS>>";

/// `Value` 树里带**裸** LF / CR 的字符串叶子。
///
/// # ⚠️ 为什么不能拿 `v.to_string()` 判
///
/// `Display for Value` 是 **JSON 编码** —— 换行在里面已是 `\\n` **两字符序列**
/// ⇒ `s.contains('\n')` **恒假** ⇒ 判据恒绿 = 无防护。
/// 这正是本项目反复踩的那一族（恒假 ⇒ 永远绿 ⇒ 看起来有防护其实没有）。
/// 必须**逐个字符串叶子**看**解码后**的内容。
fn raw_line_break_leaves(v: &Value) -> Vec<String> {
    match v {
        Value::String(s) => {
            if s.contains('\n') || s.contains('\r') {
                vec![s.clone()]
            } else {
                vec![]
            }
        }
        Value::Array(a) => a.iter().flat_map(raw_line_break_leaves).collect(),
        Value::Object(o) => o.values().flat_map(raw_line_break_leaves).collect(),
        _ => vec![],
    }
}

/// 把一份 JSON 里**每一处字符串**（任意深度）换成 `<<POS>>` + 裸 LF + `<<POS>>`。
fn poison_json(v: &Value) -> Value {
    match v {
        Value::String(_) => Value::String(format!("{POISON}\n{POISON}")),
        Value::Array(a) => Value::Array(a.iter().map(poison_json).collect()),
        Value::Object(o) => o
            .iter()
            .map(|(k, v)| (k.clone(), poison_json(v)))
            .collect(),
        other => other.clone(),
    }
}

/// 把一个 [`ItemView`] 的**每一个能承载任意文本的字段**毒化
/// （含 `payload` 里任意深度）。
///
/// # 为什么走 `Serialize`/`Deserialize` 往返，而不是逐字段点名
///
/// 逐字段点名 = **又一份封闭枚举**，而「新增一个字段」恰恰是本条要防的那件事。
/// 走往返则**不认字段名**：将来给 `ItemView` 加一个新的 `String` 字段，这一条
/// **自动**覆盖它。
///
/// # 怎么跳过「装不了任意文本」的字段
///
/// `kind` / `biz_status` / `model_status` 是**枚举**，序列化成一个判别式字符串。
/// 把它们也毒化 ⇒ 反序列化直接失败（`unknown variant`）⇒ 毒化根本落不下去。
/// 这里用**试毒**：毒一个字段后先试着还原，**还原不回来就跳过它**。
/// 跳过是**正确**的 —— 一个类型上装不进任意文本的字段，本来就不是换行注入面。
/// （判别式字符串要能被接受，必须正好等于某个变体名，不可能带裸换行。）
///
/// 往返本身必须**恒等** —— 否则毒化会静默丢掉某些字段、覆盖面比看上去小。
/// 那条断言钉的就是这一格。
fn poisoned_item(item: &ItemView) -> ItemView {
    let json = serde_json::to_value(item).expect("ItemView 必须可序列化");
    let round: ItemView = serde_json::from_value(json.clone()).expect("ItemView 必须能被 JSON 还原");
    assert_eq!(
        &round, item,
        "ItemView 的 Serialize/Deserialize 往返不是恒等 ⇒ 毒化会静默丢字段，本条覆盖面比看上去小"
    );

    let mut out = json.clone();
    if let Value::Object(map) = &mut out {
        for (key, val) in map.iter_mut() {
            match val {
                // 自由 JSON（`payload`）⇒ 任意深度全部毒化。
                Value::Array(_) | Value::Object(_) => *val = poison_json(val),
                // 顶层字符串 ⇒ 试毒：还原得回来就留下毒，还原不来就跳过。
                Value::String(_) => {
                    let poison = Value::String(format!("{POISON}\n{POISON}"));
                    let mut probe = json.clone();
                    if let Value::Object(pm) = &mut probe {
                        pm.insert(key.clone(), poison.clone());
                    }
                    if serde_json::from_value::<ItemView>(probe).is_ok() {
                        *val = poison;
                    }
                }
                _ => {}
            }
        }
    }
    serde_json::from_value(out).expect("毒化后仍必须能还原成 ItemView")
}

/// 【AC-002】**投影交给渲染层的每一个字符串都不许带裸换行** —— 判据是**值**，不是**写法**。
///
/// # REQ-071：这一条原先判的是「写法像不像」，现已换成「转义有没有发生」
///
/// **原先**：判据是一份**封闭的写法枚举** —— 断言源码里出现
/// `…map(escape_payload_text)`、且**不**出现 `…map(String::from)`。
/// QA 的 EXP-B 把 `blocked[].target` 改成 `.clone()`：那**不在词表的任何一侧**
/// ⇒ 守卫照绿，而注入确实被重新打开（探针 `STRUCTURAL` false → true）。
/// ⇒ 它判的是「源码里出现的写法像不像我登记的那几种」，**不是**「转义有没有发生」。
///
/// **现在**：判据是**投影值本身**。喂一批「每一处载荷串都带裸换行」的输入，
/// 把整份 [`ontok::domain::review_view::ReviewTrackView`] 序列化成 [`Value`] 后
/// **递归遍历每一个字符串叶子**，断言**没有一个叶子含裸 LF / CR**。
///
/// # 为什么这**不是**「又一份枚举」
///
/// | | 旧判据 | 新判据 |
/// | --- | --- | --- |
/// | 枚举的是什么 | **Rust 写法**（无限种；REQ-066 已证穷尽不了） | **数据结构**（`ItemView` / `ReviewTrackView` 的字段，有限，且改动会**编译不过**） |
/// | 有人新增一个未转义的字段位 | ⛔ 词表里没有 ⇒ **绿**（EXP-B 实测 7 个二进制 100 条全绿） | 🔴 叶子遍历**自动**覆盖 ⇒ **红** |
/// | 判的是 | 源码**文本** | 投影**值** |
/// | 把转义换成**等价**写法 | 🔴 误报（词表里没有 ⇒ 「写法变了」） | 🟢 绿（**转义确实发生了**，本条就该绿） |
///
/// 关键差别：**枚举「写法」追不掉尾巴**（词表永远能补出新的）；
/// **枚举「字段」是有限且有代价的** —— 新增一个字段要改 `ItemView` / `ModelFacing`
/// 的定义，那是**一处显眼的、能被 review 看见的改动**，
/// 不是「顺手把 `.map(escape_payload_text)` 换成 `.clone()`」。
///
/// # ⚠️ 为什么**不**把判据做成「与被测实现共用同一份转义函数」
///
/// （BA AC-001 提示的另一个方向，此处**主动否定**，理由记在这里。）
/// 若测试拿 `escape_payload_text` 自己算出「期望值」再比对，
/// 那么一旦**转义函数本身**被摘掉或改坏，两边会**一起错、一起对**
/// ⇒ 守卫恰好在防护消失的那一刻变绿。**这是循环论证。**
/// 本文件因此继续**独立**写一遍转义标记（`LINE_FEED_ESCAPE` 等），
/// 与 REQ-069 其余测试的同一纪律一致。
///
/// # ⚠️ 边界（照实写，不是免责）
///
/// - 它证的是「**没有裸换行留在投影里**」，**不是**「载荷被转义了」。
///   「把换行删掉」也能让它绿 ⇒ **「被转义而不是被吞掉」由**语料那条的 `␊` 断言负责
///   （见 [`test_req069_payload_cannot_add_a_line_anywhere`]）。两条分工明确。
/// - 它只管**投影这一层**。渲染层自己 `format!` 拼出来的换行不归它管。
/// - 它**不做逐位定位**：它给的是「有叶子脏了」，不告诉你是哪一位。
///   逐位的换行断言在 `tests/test_review_view_req071.rs`（REQ-071 · AC-002）。
/// - 它**毒不到将来新增的输入面**：`ItemView` 与 `extracted_by` 之外的输入
///   （例如将来某个新参数直接带字符串进来）不会被毒化 ⇒ 那一格仍是纪律。
#[test]
fn test_req069_every_payload_string_read_goes_through_the_escape() {
    // 一份能触发**尽可能多**投影出口的输入：
    // 关系类条目（触发 `两端：` / `blocked` / 悬空），
    // payload 覆盖 key / cardinality / properties / from_type / to_type / evidence，
    // `extracted_by` 覆盖 agent / version / failed_groups。
    let base = ItemView {
        id: "1:add_link_type:客户_has_订单".into(),
        kind: ItemKind::AddLinkType,
        target: "客户_has_订单".into(),
        from_type: Some("customer".into()),
        to_type: Some("t064".into()),
        biz_status: Decision::Accepted,
        model_status: Decision::Accepted,
        payload: json!({
            "key": "customer",
            "cardinality": "1:N",
            "properties": [{"name": "编号", "column": "no"}],
            "from_type": "customer",
            "to_type": "t064",
            "evidence": [{"table": "t_customer", "column": "customer_no", "claim": "客户编号唯一"}]
        }),
    };
    let base_eb = json!({
        "agent": "agent-a",
        "version": "1",
        "round": 1,
        "failed_groups": ["g1"],
        "source": "declared"
    });

    let poisoned = poisoned_item(&base);
    let poisoned_eb = poison_json(&base_eb);
    let poisoned_json = serde_json::to_value(&poisoned).expect("毒化后的条目必须可序列化");

    // ── 前提①：毒载荷里**真的**有裸换行，且覆盖面不是一两处 ──────────
    // 这一批毒化至少命中 16 处字符串（id / target / from_type / to_type = 4，
    // payload 9，extracted_by 3），门槛取 10 留余量。
    let dirty_in = raw_line_break_leaves(&poisoned_json).len()
        + raw_line_break_leaves(&poisoned_eb).len();
    assert!(
        dirty_in >= 10,
        "毒载荷只命中 {dirty_in} 处字段位 ⇒ 覆盖面不足，本条会因「毒化没打到东西」而恒绿"
    );

    // ── 前提②：干净对照本身也是干净的（否则下面的「绿」没有意义）──────
    let (clean_view, _) =
        project_review_track_with_summary_and_provenance(&[base], &BTreeSet::new(), Some(&base_eb));
    let clean_json = serde_json::to_value(&clean_view).expect("投影必须可序列化");
    assert!(
        raw_line_break_leaves(&clean_json).is_empty(),
        "干净输入的投影里就有裸换行 ⇒ 判据在基线上本来就是红的，后面的比较没有意义"
    );

    // ── 判据：毒载荷投影的每一个字符串叶子都不许带裸 LF / CR ──────────
    let (view, _text) = project_review_track_with_summary_and_provenance(
        &[poisoned],
        &BTreeSet::new(),
        Some(&poisoned_eb),
    );
    let proj = serde_json::to_value(&view).expect("投影必须可序列化");
    let dirty_out = raw_line_break_leaves(&proj);
    assert!(
        dirty_out.is_empty(),
        "投影里有 {} 个字符串叶子带着裸换行：{dirty_out:#?}\
         \n⇒ 这一位绕过了 escape_payload_text，payload 能注入行结构",
        dirty_out.len()
    );

    // ── 前提③：判据**必须能红**、且**不能恒红** ───────────────────────
    let canary_dirty = Value::String(format!("{POISON}\n{POISON}"));
    assert!(
        !raw_line_break_leaves(&canary_dirty).is_empty(),
        "判据对「一个带裸换行的字符串」都判不出来 ⇒ 它恒为真（永远绿 = 无防护）"
    );
    let canary_clean = Value::String(POISON.to_string());
    assert!(
        raw_line_break_leaves(&canary_clean).is_empty(),
        "判据把「没有裸换行的字符串」也判成脏 ⇒ 它恒为假（永远红 = 误报）"
    );
}

/// 【AC-002】转义**发生在既有归一化之后** —— 否则改的就是既有判据行为。
///
/// 反例：`key = "\n"`。转义在前 ⇒ `"␊"` 非空 ⇒ `sourced_fields` 凭空多一栏
/// ⇒ `premise_holds` 行为被本条改掉（那是 AC-004 明令禁止的）。
#[test]
fn test_req069_blank_key_stays_absent() {
    // 纯空白（含换行）的 key 仍然判「没有 key」—— 与 REQ-057（F-D）一致。
    for k in ["\n", "\r", "   ", "\n\r\n", " "] {
        let payload = json!({"key":k,"evidence":[{"table":"t","column":"c","claim":"正常"}]});
        let (v, text) = view_and_text(&[item("客户", payload)], Some(&declared_eb()));
        assert!(
            text.contains("（无 key）"),
            "key={k:?} 应当落成「（无 key）」（缺失可见），实际渲染：\n{text}"
        );
        // 且该条目**不**因为这个空 key 而被算成「有 key」。
        assert!(
            !v.items[0].model.sourced_fields().contains(&"key"),
            "空 key 被算成有建模数据 ⇒ 转义跑在了空值判定之前"
        );
    }
}
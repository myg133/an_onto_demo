//! REQ-071 · **已接上转义的字段位必须有换行断言**（REQ-069 QA 实测出的 7 个覆盖缺口）。
//!
//! 测试名带 `test_req071_` 前缀。被测对象 `src/domain/review_view.rs` **一行未改** —— 本条纯测试侧。
//!
//! # 这条修的是哪一格
//!
//! REQ-069 把 **18 个字段位**全部接上了 `escape_payload_text`，但它自己的语料
//! （`newline_injection_corpus`，在 `tests/test_review_view_req069.rs`）**只覆盖 10 位**。
//! QA 用一套**独立写的 22 位探针**逐位注入复核（`.review-workspace/req069qa/QA.md` §2），实测：
//!
//! | | 字段位 | 换行注入测试 |
//! | --- | --- | --- |
//! | ✅ 有（10 位） | `payload.key` / `properties[].name` / `properties[].column` / `evidence[].claim` / `evidence[].table` / `evidence[].column` / `payload.cardinality` / `payload.from_type` / `payload.to_type` / `it.target` | REQ-069 语料 |
//! | ✅ 有（1 位） | `provenance.agent` | `test_req069_forged_provenance_line_is_neutralised` |
//! | 🔴 **缺（7 位）** | **L / M / N / O / P / S / T**（下表） | **无** |
//!
//! ⇒ 那 7 位一旦被改回未转义，**测试一声不吭** —— 这就是 AC-002 要堵的格。
//!
//! # 位号沿用 REQ-069 QA §2 的那张表
//!
//! **别自己另起一套命名。** 行号会随合并漂移，**字段位不会**；
//! 位号是那份复核的索引，跨报告对账全靠它。
//!
//! | 位 | 投影里的路径 | 转义发生在源码的哪一句（**按内容定位**） | 载荷怎么送进去 |
//! | --- | --- | --- | --- |
//! | **L** | `items[].model.from_type` | `from_type: it.from_type.as_deref().map(escape_payload_text)` | `ItemView::from_type` |
//! | **M** | `items[].model.to_type` | `to_type: it.to_type.as_deref().map(escape_payload_text)` | `ItemView::to_type` |
//! | **N** | `items[].reason` | `reason: …as_deref().map(escape_payload_text)` | 直接给一份 `ProposalStatus`（见该格注释） |
//! | **O** | `blocked[].target` | `target: escape_payload_text(&b.target)` | 关系类条目 + 两端不可用 |
//! | **P** | `blocked[].reason` | `reason: b.reason.as_deref().map(escape_payload_text)` | 关系类条目 + `from_type` 带换行 |
//! | **S** | `provenance.state::Declared.version` | `.map(str::trim).filter(…).map(escape_payload_text)` | `extracted_by.version` |
//! | **T** | `provenance.state::Declared.failed_groups[]` | `v.as_str().map(escape_payload_text)`（`render_group_ref`） | `extracted_by.failed_groups[0]` |
//!
//! # 判别力：为什么每一条都不是恒真
//!
//! 每一位断言四件事，**任一不成立即红**，且**缺一不可**：
//!
//! 1. **输入侧** —— 喂进去的那个串**真的**含裸 LF。
//!    ⚠️ **逐个字符串叶子看**；**不能**用 `payload.to_string()`
//!    （那是 JSON 编码，换行已是 `\\n` 两字符序列 ⇒ 判「有没有换行」**恒假**
//!    ⇒ 前提断言变成「永远红」）。这一族本项目踩过多次。
//! 2. **到达侧** —— 投影的**那一位**确实被填上了，且**干净对照的同一位不含 `␊`**。
//!    没有这一格，「看不到换行」只说明「没渲染」；而对照排除了「`␊` 本来就在那儿」。
//! 3. **转义轴** —— 该位上**出现 `␊`** ⇒ 换行被**转义**了（不是被整段吞掉）。
//! 4. **裸换行轴** —— 该位上**不含**裸 LF / CR。
//!
//! 3 与 4 必须**同时**在：只判 3 会漏「换成别的控制符」，只判 4 会漏「把换行删掉」。
//!
//! # ⚠️ 边界（照实写，不是免责）
//!
//! - **N 这一位在当前 `summarize` 下永远拿不到载荷文本**（`ItemVerdict::reason`
//!   只可能是「业务轨显式拒绝」/「建模轨显式拒绝」两个固定串）。所以这一格只能用
//!   **直接构造 `ProposalStatus`** 来见证。它是**防御性**的：`ProposalStatus` 是 `pub`，
//!   `project_review_track(items, status)` 接受任意一份 ⇒ 是**真实的 API 面**，
//!   转义位在，只是当前没有活的喂入路径。照实登记，不假装它是活的。
//! - 判的是**投影**（逐位取字段），不是「整段渲染文本里出现过 `␊`」。
//!   后者会被**邻位代答** —— 任何一位转义成功都能让整段含 `␊` ⇒ 假阴性。
//! - **Q `dangling_relations`** 走 `{:?}`（Rust Debug 自带转义），修前就不可注入，
//!   不在本表内。源码仍对它调 `escape_payload_text`，那是**冗余防护**；
//!   本条不替它立断言 —— 无洞可堵时立断言只会制造恒真。

use ontok::domain::review::{Decision, EntryState, ItemKind, ItemView, ItemVerdict, ProposalStatus};
use ontok::domain::review_view::{
    project_review_track, project_review_track_with_summary_and_provenance, render_review_text,
    ProvenanceState, ReviewTrackView, MODEL_LINE_PREFIX, PROVENANCE_LINE_PREFIX,
};
use serde_json::{json, Value};
use std::collections::BTreeSet;

/// ␊ —— LF 的 Control Picture 转义。**与被测实现独立写一遍**
/// （测试不 import 被测实现里的私有常量；同 REQ-069 测试的纪律）。
const LINE_FEED_ESCAPE: char = '\u{240A}';

// ── fixture ──────────────────────────────────────────────

/// 换行后面接「长得极像结构行」的片段 —— 只挡「裸换行」不够（REQ-069 · AC-003 · M4）。
fn forge(prefix: &str) -> String {
    format!("\n{prefix}key=伪造 · model=accepted")
}

fn has_raw_line_break(s: &str) -> bool {
    s.contains('\n') || s.contains('\r')
}

/// 关系类条目。`from` / `to` 给值 ⇒ 走 `it.from_type` / `it.to_type`（**L / M**），
/// 留空 ⇒ 回落 `payload_str`（REQ-069 语料已覆盖的那条路）。
fn rel_item(target: &str, from: Option<&str>, to: Option<&str>) -> ItemView {
    ItemView {
        id: "1:add_link_type:客户_has_订单".into(),
        kind: ItemKind::AddLinkType,
        target: target.into(),
        from_type: from.map(str::to_string),
        to_type: to.map(str::to_string),
        biz_status: Decision::Accepted,
        model_status: Decision::Accepted,
        payload: json!({"evidence":[{"table":"t","column":"c","claim":"正常"}]}),
    }
}

fn entity_item(target: &str) -> ItemView {
    ItemView {
        id: "1:add_entity_type:客户".into(),
        kind: ItemKind::AddEntityType,
        target: target.into(),
        from_type: None,
        to_type: None,
        biz_status: Decision::Accepted,
        model_status: Decision::Accepted,
        payload: json!({"key":"customer","evidence":[{"table":"t","column":"c","claim":"正常"}]}),
    }
}

fn declared_eb() -> Value {
    json!({"agent":"agent-a","version":"1","round":1,"source":"declared"})
}

/// 一次投影的输入。`status` 为 `None` ⇒ 走真实的 `summarize` 路径；
/// `Some` ⇒ 直接给一份 `ProposalStatus`（只有 **N** 需要，见模块注释）。
struct Inputs {
    items: Vec<ItemView>,
    status: Option<ProposalStatus>,
    eb: Option<Value>,
}

fn project(inp: &Inputs) -> (ReviewTrackView, String) {
    match &inp.status {
        None => {
            let (v, _st) = project_review_track_with_summary_and_provenance(
                &inp.items,
                &BTreeSet::new(),
                inp.eb.as_ref(),
            );
            let text = render_review_text(&v);
            (v, text)
        }
        Some(st) => {
            let v = project_review_track(&inp.items, st);
            let text = render_review_text(&v);
            (v, text)
        }
    }
}

/// 从投影里把**某一位**取出来 —— 取的是**那一位本身**，不是整段文本。
type Probe = fn(&ReviewTrackView) -> Option<String>;

// ── 七位的见证（位号 = REQ-069 QA §2）────────────────────

struct Case {
    /// 位号 + 投影路径
    slot: &'static str,
    /// **喂进去的那个串的原文** —— 前提①断言它真的含裸 LF。
    /// 单独存一份是为了让前提断言针对**输入**，而不是针对已经过了转义的**输出**
    /// （对着输出判「输入有没有换行」是循环论证）。
    dirty_raw: String,
    dirty: Inputs,
    clean: Inputs,
    probe: Probe,
}

fn cases() -> Vec<Case> {
    let model_forge = forge(MODEL_LINE_PREFIX);
    let prov_forge = forge(PROVENANCE_LINE_PREFIX);

    // 各位的脏值（先建一次，避免「断言用的串」与「喂进去的串」分叉）
    let l_raw = format!("customer{model_forge}");
    let m_raw = format!("t064{model_forge}");
    let n_raw = format!("建模轨显式拒绝{model_forge}");
    let o_raw = format!("客户_has_订单{model_forge}");
    let s_raw = format!("1{prov_forge}");
    let t_raw = format!("g1{prov_forge}");
    let clean_rel = || Inputs {
        items: vec![rel_item("客户_has_订单", Some("customer"), Some("t064"))],
        status: None,
        eb: Some(declared_eb()),
    };
    let clean_entity = || Inputs {
        items: vec![entity_item("客户")],
        status: None,
        eb: Some(declared_eb()),
    };

    vec![
        // ── L · items[].model.from_type ────────────────────────────────
        Case {
            slot: "L `items[].model.from_type`（`it.from_type`）",
            dirty_raw: l_raw.clone(),
            dirty: Inputs {
                items: vec![rel_item(
                    "客户_has_订单",
                    Some(&l_raw),
                    Some("t064"),
                )],
                status: None,
                eb: Some(declared_eb()),
            },
            clean: clean_rel(),
            probe: |v| v.items.first().and_then(|i| i.model.from_type.clone()),
        },
        // ── M · items[].model.to_type ──────────────────────────────────
        Case {
            slot: "M `items[].model.to_type`（`it.to_type`）",
            dirty_raw: m_raw.clone(),
            dirty: Inputs {
                items: vec![rel_item(
                    "客户_has_订单",
                    Some("customer"),
                    Some(&m_raw),
                )],
                status: None,
                eb: Some(declared_eb()),
            },
            clean: clean_rel(),
            probe: |v| v.items.first().and_then(|i| i.model.to_type.clone()),
        },
        // ── N · items[].reason（verdict.reason）─────────────────────────
        // ⚠️ `summarize` 的 `ItemVerdict::reason` 只可能是两个固定串 ⇒ 这一格只能
        // 直接构造 `ProposalStatus` 来见证（见模块注释的「边界」一节）。
        Case {
            slot: "N `items[].reason`（`verdict.reason`，防御性 · 直接构造 ProposalStatus）",
            dirty_raw: n_raw.clone(),
            dirty: Inputs {
                items: vec![entity_item("客户")],
                status: Some(ProposalStatus {
                    passed: false,
                    verdicts: vec![ItemVerdict {
                        id: "1:add_entity_type:客户".into(),
                        target: "客户".into(),
                        state: EntryState::Vetoed,
                        reason: Some(n_raw.clone()),
                    }],
                    applicable: vec![],
                    blocked: vec![],
                    dangling_relations: vec![],
                }),
                eb: Some(declared_eb()),
            },
            clean: Inputs {
                items: vec![entity_item("客户")],
                status: Some(ProposalStatus {
                    passed: false,
                    verdicts: vec![ItemVerdict {
                        id: "1:add_entity_type:客户".into(),
                        target: "客户".into(),
                        state: EntryState::Vetoed,
                        reason: Some("建模轨显式拒绝".into()),
                    }],
                    applicable: vec![],
                    blocked: vec![],
                    dangling_relations: vec![],
                }),
                eb: Some(declared_eb()),
            },
            probe: |v| v.items.first().and_then(|i| i.reason.clone()),
        },
        // ── O · blocked[].target ───────────────────────────────────────
        // 两端不在可用集合里 ⇒ `summarize` 必定把它推进 `blocked`（前提②断言）。
        Case {
            slot: "O `blocked[].target`",
            dirty_raw: o_raw.clone(),
            dirty: Inputs {
                items: vec![rel_item(&o_raw, Some("customer"), Some("t064"))],
                status: None,
                eb: Some(declared_eb()),
            },
            clean: clean_rel(),
            probe: |v| v.blocked.first().map(|b| b.target.clone()),
        },
        // ── P · blocked[].reason ───────────────────────────────────────
        // `reason` = `format!("…但 {} 尚未可用", missing.join("、"))`，
        // 而 `missing` 直接来自 `it.from_type` / `it.to_type` ⇒ 换行真的会进这一格。
        Case {
            slot: "P `blocked[].reason`",
            dirty_raw: l_raw.clone(),
            dirty: Inputs {
                items: vec![rel_item(
                    "客户_has_订单",
                    Some(&l_raw),
                    Some("t064"),
                )],
                status: None,
                eb: Some(declared_eb()),
            },
            clean: clean_rel(),
            probe: |v| v.blocked.first().and_then(|b| b.reason.clone()),
        },
        // ── S · provenance.version ─────────────────────────────────────
        Case {
            slot: "S `provenance.state::Declared.version`",
            dirty_raw: s_raw.clone(),
            dirty: Inputs {
                items: vec![entity_item("客户")],
                status: None,
                eb: Some(json!({"agent":"agent-a","version":s_raw.clone(),
                                "round":1,"source":"declared"})),
            },
            clean: clean_entity(),
            probe: |v| match &v.provenance.state {
                ProvenanceState::Declared { version, .. } => version.clone(),
                _ => None,
            },
        },
        // ── T · provenance.failed_groups[] ─────────────────────────────
        Case {
            slot: "T `provenance.state::Declared.failed_groups[]`",
            dirty_raw: t_raw.clone(),
            dirty: Inputs {
                items: vec![entity_item("客户")],
                status: None,
                eb: Some(json!({"agent":"agent-a","version":"1","round":1,
                                "failed_groups":[t_raw.clone()],
                                "source":"declared"})),
            },
            clean: Inputs {
                items: vec![entity_item("客户")],
                status: None,
                // ⚠️ 干净对照**必须也有 failed_groups**，否则 probe 在对照侧返回 `None`
                // （「这一位是空的」）—— 那不是载荷没走到，是对照本身没喂。
                eb: Some(json!({"agent":"agent-a","version":"1","round":1,
                                "failed_groups":["g1"],
                                "source":"declared"})),
            },
            probe: |v| match &v.provenance.state {
                ProvenanceState::Declared { failed_groups, .. } => {
                    failed_groups.first().cloned()
                }
                _ => None,
            },
        },
    ]
}

/// 【AC-002 / AC-003 · M2】**七位逐一**：喂进带裸换行的载荷，那一位必须被转义。
///
/// # M2 是本条的**灵魂**
///
/// 把其中**任意一位**改回未转义（删掉 `escape_payload_text`、或换成 `.clone()` /
/// `.map(String::from)` / `.to_string()` 等**词表外**的写法），**必须**变红。
///
/// ⚠️ 逐位取字段（[`Probe`]）而不是「整段渲染文本里出现过 `␊`」：
/// 后者会被**邻位代答** —— 任何一位转义成功都能让整段含 `␊` ⇒ 假阴性。
/// 这正是 QA 在 REQ-069 守则上实测出的 EXP-B 那一族（词表外写法可绕过）。
#[test]
fn test_req071_all_seven_escaped_slots_have_a_newline_assertion() {
    let all = cases();
    assert_eq!(all.len(), 7, "本条覆盖 QA 点名的 7 位；数目变了要回来改表");

    for c in all {
        // ── 前提① · 输入侧：喂进去的串**真的**含裸 LF ──────────────────
        assert!(
            has_raw_line_break(&c.dirty_raw),
            "{}：喂进去的载荷里没有裸换行 ⇒ 本条对这一位是**空断言**（恒真 = 无判别力）",
            c.slot
        );

        let (view, text) = project(&c.dirty);
        let (clean_view, clean_text) = project(&c.clean);

        // ── 前提② · 到达侧：脏/干净**同一位都被填上**，且干净那侧**不含 ␊** ──
        let got = (c.probe)(&view).unwrap_or_else(|| {
            panic!(
                "{}：投影里这一位是空的 ⇒ 载荷压根没走到它，本条对这一位是**空断言**",
                c.slot
            )
        });
        let clean_got = (c.probe)(&clean_view).unwrap_or_else(|| {
            panic!(
                "{}：干净对照里这一位是空的 ⇒ 投影路径变了，本条的对照不成立",
                c.slot
            )
        });
        assert!(
            !clean_got.contains(LINE_FEED_ESCAPE),
            "{}：干净对照里这一位本来就含 ␊ ⇒ 它不是被本条的载荷转义出来的（前提失效）",
            c.slot
        );
        assert_ne!(
            got, clean_got,
            "{}：脏输入在这一位上与干净对照**完全相同** ⇒ 载荷没流到这一位（前提失效）",
            c.slot
        );

        // ── 判据③ · 转义轴：该位上出现 ␊ ⇒ 换行被转义，而不是被整段吞掉 ──
        assert!(
            got.contains(LINE_FEED_ESCAPE),
            "{}：这一位上没有 ␊ ⇒ 它的换行没经过 escape_payload_text\n这一位 = {got:?}",
            c.slot
        );

        // ── 判据④ · 裸换行轴：该位上不许有裸 LF / CR ─────────────────
        assert!(
            !has_raw_line_break(&got),
            "{}：这一位带着**裸**换行 ⇒ payload 注入重新打开（值 = {got:?}）",
            c.slot
        );

        // ── 对照：脏输入不得比干净输入多切出任何一行 ─────────────────
        assert_eq!(
            text.lines().count(),
            clean_text.lines().count(),
            "{}：换行在渲染结果里多切出了行 ⇒ payload 造出了行结构\n--- 脏 ---\n{text}\n--- 干净 ---\n{clean_text}",
            c.slot
        );
    }
}

/// 【AC-002】七位在**渲染文本**里也不得造出**伪结构行**。
///
/// # 为什么投影那条之外还要这一条
///
/// 投影那一条问的是「这一位有没有转义」。这一条问的是**句法判据真正读的那一面**：
/// 渲染结果里**以结构前缀开头**的行数，必须与干净对照**逐条相等** ——
/// 即使某一格投影干净、渲染层却把它 `format!` 出去，这一格也会响。
#[test]
fn test_req071_seven_slots_do_not_forge_structural_lines() {
    for c in cases() {
        let (_v, text) = project(&c.dirty);
        let (_cv, clean_text) = project(&c.clean);

        for (label, prefix) in [
            ("建模轨", MODEL_LINE_PREFIX),
            ("归属", PROVENANCE_LINE_PREFIX),
        ] {
            let heads = text.lines().filter(|l| l.trim_start().starts_with(prefix)).count();
            let clean_heads = clean_text
                .lines()
                .filter(|l| l.trim_start().starts_with(prefix))
                .count();
            assert_eq!(
                heads, clean_heads,
                "{}：渲染结果里以「{label}」开头的行数比干净对照多了 {} 行\
                 ⇒ payload 在这一位上用换行造出了伪结构行\n--- 脏 ---\n{text}",
                c.slot,
                heads as i64 - clean_heads as i64
            );
        }
    }
}

/// 【AC-002】守卫自检：判据**必须能红**，且**不能恒红**。
///
/// 对着已知答案跑一遍，防止判据本身写坏后「永远绿」（本项目反复出现的那一族）。
#[test]
fn test_req071_the_criterion_can_go_both_ways() {
    assert!(
        has_raw_line_break("a\nb"),
        "判据杀不掉「带裸换行」⇒ 它恒为假（永远红）"
    );
    assert!(
        !has_raw_line_break(&format!("a{LINE_FEED_ESCAPE}b")),
        "判据把「已转义」也判成裸换行 ⇒ 恒为假（永远红）"
    );
    assert!(
        !has_raw_line_break("ab"),
        "判据把「干净串」也判成脏 ⇒ 恒为假（永远红）"
    );
}

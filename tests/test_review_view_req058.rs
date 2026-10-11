//! REQ-058 · **退化 evidence 仍过判据（F-1）** + **分界前缀锐边（F-2）**。
//!
//! 测试名带 `test_req058_` 前缀。
//!
//! # 两条病根
//!
//! | | 病 | 修 |
//! | --- | --- | --- |
//! | **F-1** | 退化 evidence（`""` / `"   "` / `{}` / `null` / `[]` / `0` / `false` / `{claim:""}`）经渲染得到**非空串** ⇒ `sourced_fields` 判「有依据」 | 判据问 [`evidence_line_carries_data`]：退化类别从**原始 `Value`** 判出，渲染成**保留标记** |
//! | **F-2** | `contains` 认轨 + `trim_start_matches` 剥前缀 ⇒ 展示名 / claim 含「业务轨：」「建模轨：」就误报 | **行首**认轨，**只剥一次** |
//!
//! # 每条断言都对着一个变异
//!
//! AC-001 要求七种退化形态**逐个**堵住、且**渲染各不相同** —— 所以下面
//! 既有「逐个过判据」，也有「逐个渲染互不相同」两条独立断言，
//! 前者杀 M2（只堵一��），后者杀 M3（统一占位串）。

use ontok::domain::review::{Decision, ItemKind, ItemView};
use ontok::domain::review_view::{
    audit_fde_channel, project_review_track_with_summary, render_review_text,
    verify_track_separation, ReviewTrackView, BIZ_LINE_PREFIX, DEGENERATE_EVIDENCE_MARK,
    MODEL_LINE_PREFIX,
};
use serde_json::{json, Value};
use std::collections::BTreeSet;

// ── fixture ──────────────────────────────────────────────

/// 只有 `evidence` 一栏的条目 —— 这样 `premise_holds` 只可能由证据决定。
fn evidence_only_item(evidence: Value) -> ItemView {
    ItemView {
        id: "1:add_entity_type:客户".into(),
        kind: ItemKind::AddEntityType,
        target: "客户".into(),
        from_type: None,
        to_type: None,
        biz_status: Decision::Accepted,
        model_status: Decision::Accepted,
        payload: json!({ "evidence": [evidence] }),
    }
}

/// 一个**建模栏齐备**的条目 —— 用来看退化证据会不会压掉别的那几栏。
fn full_item(target: &str, payload: Value) -> ItemView {
    ItemView {
        id: format!("1:add_entity_type:{target}"),
        kind: ItemKind::AddEntityType,
        target: target.into(),
        from_type: None,
        to_type: None,
        biz_status: Decision::Accepted,
        model_status: Decision::Accepted,
        payload,
    }
}

fn view(items: Vec<ItemView>) -> ReviewTrackView {
    project_review_track_with_summary(&items, &BTreeSet::new()).0
}

/// 单条证据渲染后的那一行（`payload_evidence` 只出一条 ⇒ 下标 0）。
fn rendered_evidence(v: &ReviewTrackView) -> String {
    v.items[0].model.evidence[0].clone()
}

/// AC-001 点名的**七种**退化形态（f 行含 `0` 与 `false` 两个取值，共 8 个值）。
///
/// ⚠️ 这份清单是**实测**得来的，不是推断：
/// `serde_json` 的 `Display` 对 `Value::String` **带引号**，
/// 所以 `""` 渲染成的是**两个字符** `"` `"`，`"   "` 渲染成 `"   "` ——
/// `trim()` 之后都**非空** ⇒ 它们同样冒充「有依据」。
fn degenerate() -> Vec<(&'static str, Value)> {
    vec![
        ("a 空串", json!("")),
        ("b 纯空白", json!("   ")),
        ("c 空对象", json!({})),
        ("d null", json!(null)),
        ("e 空数组", json!([])),
        ("f 数字 0", json!(0)),
        ("f 布尔 false", json!(false)),
        ("g 有键无值", json!({ "claim": "" })),
    ]
}

/// 一条**真的**证据（对照组）。
fn real_evidence() -> Value {
    json!({ "table": "t_customer", "column": "customer_no", "claim": "主键列名与注释一致" })
}

// ── F-1 · AC-001 逐个堵住 ────────────────────────────────

/// 【B-1 · QA 阻断】**任何 evidence 元素都不得渲染成空串**。
///
/// # 死因（QA 实测）
///
/// 我第一版重构 `render_evidence_entry` 时，把基线结尾那条兜底分支
/// （`if !s.is_empty() { return s; }` ⇒ 否则退回紧凑 JSON）弄丢了。
/// 于是**认不出三段式、但确实携带信息**的形状（`{"other":"x"}` 等）
/// 渲染成**空串** —— 输出里只剩光秃秃的「证据：」，FDE 既看不出有内容、
/// 也看不出缺了什么。**那比渲染成占位串更糟：占位至少看得见。**
///
/// 这同时踩了 AC-001 自己写的两条（不静默丢掉 / 不许冒充「有依据」），
/// 并且是相对基线的**功能回归**。
///
/// ⚠️ 本条**不**判这些形状为退化 —— 它们确实携带信息，
/// `premise_holds` 对它们**保持 `true` 是对的**。
/// 要修的是「静默丢」，不是「把它判成退化」。
///
/// # 杀的是哪个变异
///
/// **M6「删掉渲染兜底分支」**。
#[test]
fn test_req058_recognisable_shapes_never_render_empty() {
    // 覆盖面刻意放宽：凡是「认不出三段式、但不是退化」的形状都在内。
    let carried: Vec<(&str, Value)> = vec![
        ("键不是三段式", json!({ "other": "x" })),
        ("claim 不是字符串", json!({ "claim": 5 })),
        ("只有 table 之外的键", json!({ "table": "t_customer", "semantic": "has" })),
        ("嵌套对象", json!({ "nested": { "a": 1 } })),
        ("数组套对象", json!([{ "other": "x" }])),
        ("普通字符串", json!("这���是一条人写的说明")),
        ("带空白的 claim + table", json!({ "table": "t", "claim": "  " })),
        ("table 为空、column 有值", json!({ "table": "", "column": "customer_no" })),
    ];
    for (label, ev) in carried {
        let v = view(vec![evidence_only_item(ev.clone())]);
        let line = rendered_evidence(&v);
        assert!(
            !line.trim().is_empty(),
            "[{label}] 渲染成了空串 ⇒ 这条 evidence 被**静默丢掉**了\n\
             基线会退回紧凑 JSON `{}`，重构把它弄丢了（QA B-1）",
            ev
        );
        // 且它确实**不是**退化项 —— 否则「非空」是靠占位串蒙混过关的。
        assert!(
            !line.starts_with(DEGENERATE_EVIDENCE_MARK),
            "[{label}] 被判成了退化（渲染成 {line:?}）—— 本条只要求「不静默丢」，\
             这类形状确实携带信息，premise_holds 应当保持 true"
        );
        assert!(
            audit_fde_channel(&v).premise_holds,
            "[{label}] 这类形状携带信息，premise_holds 应当保持 true（渲染成 {line:?}）"
        );
        // 且 payload 里实际有的内容**看得见** —— B-1 的教训同样适用于这里。
        //
        // ⚠️ 只对「**一个三段式键都没有**」的形状要求：那才是走**兜底分支**的形状。
        // 带 `table` / `column` / `claim` 的形状由三段式呈现决定，
        // 额外键（如 `semantic`）**基线也不渲染** —— 那不是回归，不在本条范围。
        if let Some(o) = ev.as_object() {
            if !["table", "column", "claim"].iter().any(|k| o.contains_key(*k)) {
                for (key, value) in o {
                    assert!(
                        line.contains(&value.to_string()),
                        "[{label}] 键 {key:?} 的取值 {value} 没出现在渲染结果 {line:?} 里 ⇒ \
                         兜底分支把它藏起来了（B-1）"
                    );
                }
            }
        }
    }
}

/// 【B-1 同类】**退化标签不许把 payload 里已有的内容藏起来**。
///
/// B-1 是「元素整个渲染成空串」；这一条是**同一个毛病的另一头**：
/// 元素被标成退化时，标签必须**带着原始内容**，否则 `[1,2]` 会渲染成
/// `全退化数组` —— FDE 知道它退化了，却**看不到退化的东西是什么**。
///
/// ⚠️ 与上一条相反：这些形状**确实是退化**（裸标量不是引用也不是一句话），
/// `premise_holds = false` 是对的。要守的是「不静默丢」。
#[test]
fn test_req058_degenerate_label_still_shows_the_original_content() {
    for (label, ev) in [
        ("非空数组", json!([1, 2])),
        ("全退化数组", json!([{ "claim": "" }])),
        ("裸标量", json!(0)),
        ("裸标量 false", json!(false)),
    ] {
        let v = view(vec![evidence_only_item(ev.clone())]);
        let line = rendered_evidence(&v);
        assert!(
            !audit_fde_channel(&v).premise_holds,
            "[{label}] 裸标量/全退化数组确实不是引用，premise_holds 应当为 false（{line:?}）"
        );
        assert!(
            line.contains(&ev.to_string()),
            "[{label}] 退化标签 {line:?} 把 payload 的实际内容藏起来了 ⇒ \
             退化成什么看不出来（B-1 的同类问题）"
        );
    }
}

/// 【AC-001】**七种退化形态逐个都不得让 `premise_holds = true`**。
///
/// # 杀的是哪个变异
///
/// **M1「退回判渲染串」**与**M2「只堵 `{}` / `null`，放行 `0` / `false` / `[]`」** ——
/// 八个取值逐个断言，所以任何「堵了一类、放了几类」的实现都必红。
#[test]
fn test_req058_each_degenerate_form_is_blocked_individually() {
    // 刻意**不**在第一个失败处 panic —— 一次跑完把七种形态各自的判决都打出来，
    // 变异报告里才能逐条说明「是哪几种被放行了」。
    let mut leaks: Vec<String> = Vec::new();
    let mut broken_premises: Vec<String> = Vec::new();
    for (label, ev) in degenerate() {
        let v = view(vec![evidence_only_item(ev.clone())]);

        // 前提：它**确实**被渲染出来了（不是「压根没进 evidence 列表」）。
        if v.items[0].model.evidence.len() != 1 {
            leaks.push(format!(
                "[{label}] 退化项被静默丢掉了 —— 「丢掉 = FDE 少一条复核依据，却看不出来」"
            ));
            continue;
        }
        // 前提：渲染结果**非空** —— 正是这一点让旧判据 `!e.trim().is_empty()` 漏网。
        let line = rendered_evidence(&v);
        if line.trim().is_empty() {
            broken_premises.push(format!(
                "[{label}] 前提不成立：渲染结果居然是空的，那旧判据本来就抓得到"
            ));
            continue;
        }

        let audit = audit_fde_channel(&v);
        if audit.premise_holds {
            leaks.push(format!("[{label}] 渲染成 {line:?}（非空）却仍判 premise_holds=true"));
        }
        if audit.hits.iter().any(|h| h.ends_with(".evidence")) {
            leaks.push(format!("[{label}] 退化 evidence 进了 hits：{:?}", audit.hits));
        }
    }
    assert!(
        leaks.is_empty(),
        "退化 evidence 冒充「有依据」：\n{}",
        leaks.join("\n")
    );
    assert!(
        broken_premises.is_empty(),
        "测试自身的前提不成立：\n{}",
        broken_premises.join("\n")
    );
}

/// 【AC-001】**七种退化形态的渲染结果必须各不相同**。
///
/// # 杀的是哪个变异
///
/// **M3「把退化值统一渲染成一个占位串」** —— 那样 FDE 就看不出
/// 「这一条到底退化成什么」，等于把问题重新藏起来。
#[test]
fn test_req058_each_degenerate_form_renders_distinctly() {
    let mut seen: Vec<(String, String)> = Vec::new();
    for (label, ev) in degenerate() {
        let v = view(vec![evidence_only_item(ev.clone())]);
        let line = rendered_evidence(&v);
        if let Some((other, _)) = seen.iter().find(|(_, l)| *l == line) {
            panic!(
                "[{label}] 与 [{other}] 渲染成了同一个串 {line:?} ⇒ \
                 退化形态被合并成一类，AC-001 不许合并"
            );
        }
        // 每个都必须是**带保留标记的退化措辞**，而不是伪装成内容的普通串。
        assert!(
            line.starts_with(DEGENERATE_EVIDENCE_MARK) && line.ends_with(DEGENERATE_EVIDENCE_MARK),
            "[{label}] 渲染结果 {line:?} 不带保留定界符 ⇒ FDE 看不见它退化"
        );
        seen.push((label.to_string(), line));
    }
    assert_eq!(seen.len(), degenerate().len(), "退化取值清单被改动过");
}

/// 【AC-001 · 保留呈现】**退化项照样出现在输出里**（不静默丢掉）。
///
/// 「保留呈现 + 不让它冒充有依据」是两件事，缺一不可：
/// 只修判据会把证据藏起来；只保留呈现就是原来那个洞。
#[test]
fn test_req058_degenerate_evidence_is_still_shown_to_fde() {
    for (label, ev) in degenerate() {
        let v = view(vec![evidence_only_item(ev.clone())]);
        let text = render_review_text(&v);
        assert!(
            text.contains("证据："),
            "[{label}] 输出里连「证据：」都没有 ⇒ 退化项被静默丢掉了\n实际输出：\n{text}"
        );
        assert!(
            text.contains(&rendered_evidence(&v)),
            "[{label}] 渲染串没打进建模轨行\n实际输出：\n{text}"
        );
    }
}

/// 【AC-001 · 正控】**真证据仍然让 `premise_holds = true`**。
///
/// 没有这条，下面那些「全红」可能只是因为判据坏了。
#[test]
fn test_req058_real_evidence_still_holds_premise() {
    let v = view(vec![evidence_only_item(real_evidence())]);
    let audit = audit_fde_channel(&v);
    assert!(audit.premise_holds, "真证据反而被判成没有依据：{:?}", audit.hits);
    assert_eq!(audit.hits, vec!["item[1].evidence".to_string()]);
    assert_eq!(rendered_evidence(&v), "t_customer.customer_no：主键列名与注释一致");
}

/// 【AC-001】**退化项与真证据混在一起时，只由真证据定前提**。
#[test]
fn test_req058_degenerate_mixed_with_real_evidence_still_holds() {
    let v = view(vec![ItemView {
        id: "1:add_entity_type:客户".into(),
        kind: ItemKind::AddEntityType,
        target: "客户".into(),
        from_type: None,
        to_type: None,
        biz_status: Decision::Accepted,
        model_status: Decision::Accepted,
        payload: json!({ "evidence": [json!({}), real_evidence()] }),
    }]);
    let audit = audit_fde_channel(&v);
    assert!(audit.premise_holds, "真证据在列 ⇒ 前提应当成立：{:?}", audit.hits);
    assert_eq!(v.items[0].model.evidence.len(), 2, "两条都该呈现");
    assert!(audit.hits.iter().any(|h| h.ends_with(".evidence")));
}

/// 【AC-002 · 判据不许有洞】**payload 文本伪造不出保留定界符**。
///
/// 若渲染时不转义 `␀`，一条 claim 恰好是「␀空对象␀」就会被误判成退化项
/// —— 判据就从「挡空证据」变成「挡真证据」。所以渲染时**必须**转义。
#[test]
fn test_req058_payload_text_cannot_forge_the_degenerate_mark() {
    let forged = format!("{DEGENERATE_EVIDENCE_MARK}空对象{DEGENERATE_EVIDENCE_MARK}");
    let ev = json!({ "table": "t_customer", "column": "customer_no", "claim": forged });
    let v = view(vec![evidence_only_item(ev)]);

    let line = rendered_evidence(&v);
    assert!(
        !line.starts_with(DEGENERATE_EVIDENCE_MARK) && !line.ends_with(DEGENERATE_EVIDENCE_MARK),
        "伪造串没被挡住：{line:?} ⇒ 转义没生效，判据有洞"
    );
    assert!(
        line.contains(&DEGENERATE_EVIDENCE_MARK.to_string().replace(DEGENERATE_EVIDENCE_MARK, "\u{2401}")),
        "payload 里的定界符应当被转义成 ␁ 而不是原样打出去：{line:?}"
    );
    assert!(
        audit_fde_channel(&v).premise_holds,
        "一条带定位的真证据被误判成退化：{line:?}"
    );
}

/// 【AC-001 / AC-002】**退化证据不得压掉同一条目上别的那几栏**。
///
/// `hits` 要照常列出 `key` / `columns` —— 只是**不列** `evidence`。
#[test]
fn test_req058_degenerate_evidence_does_not_hide_other_sourced_fields() {
    let v = view(vec![full_item(
        "客户",
        json!({
            "key": "customer",
            "properties": [{ "name": "客户编号", "column": "customer_pk" }],
            "evidence": [json!({})]
        }),
    )]);
    let audit = audit_fde_channel(&v);
    assert!(audit.premise_holds, "key 与列映射都来自 payload，前提应当成立");
    assert_eq!(
        audit.hits,
        vec!["item[1].key".to_string(), "item[1].columns".to_string()],
        "hits 里不该有 evidence，但 key / columns 一个都不能少"
    );
    // 而输出里那条退化的证据**仍然看得见**。
    let text = render_review_text(&v);
    assert!(text.contains("证据："), "退化证据没被呈现：\n{text}");
}

// ── F-2 · AC-004 前缀只剥一次 ──────────────────────────

/// AC-004：claim 含 `BIZ_LINE_PREFIX` 时，`verify_track_separation` **零违规**。
///
/// # 杀的是哪个变异
///
/// **M5「让 claim 里的『业务轨：』也参与前缀剥离」** —— 恢复 `contains` 认轨
/// 就会让建模轨行被当成业务轨行 ⇒ 行数对不上 + 逐字比对错位（实测 2 条误报）。
#[test]
fn test_req058_claim_containing_track_prefix_is_not_a_false_positive() {
    let v = view(vec![full_item(
        "客户",
        json!({
            "key": "customer",
            "properties": [{ "name": "客户编号", "column": "customer_pk" }],
            "evidence": [{
                "table": "t_customer", "column": "customer_no",
                "claim": format!("业务轨：{BIZ_LINE_PREFIX}这是 claim 里的正文，不是分轨标记")
            }]
        }),
    )]);
    let text = render_review_text(&v);
    let violations = verify_track_separation(&v, &text);
    assert!(
        violations.is_empty(),
        "claim 里带「业务轨：」被判成分轨违规（误报）：{violations:#?}\n实际输出：\n{text}"
    );
}

/// 【AC-004】**展示名含两轨前缀 ⇒ 零违规**。
///
/// 两个前缀各测一遍 —— 原登记只报了展示名含「业务轨：」，
/// QA 实测**建模轨：** 那一种面更宽。
#[test]
fn test_req058_display_name_containing_track_prefix_is_not_a_false_positive() {
    for target in [format!("{BIZ_LINE_PREFIX}客户"), format!("{MODEL_LINE_PREFIX}客户")] {
        let v = view(vec![full_item(
            &target,
            json!({
                "key": "customer",
                "properties": [{ "name": "客户编号", "column": "customer_pk" }],
                "evidence": [real_evidence()]
            }),
        )]);
        let text = render_review_text(&v);
        let violations = verify_track_separation(&v, &text);
        assert!(
            violations.is_empty(),
            "展示名 {target:?} 含两轨前缀被判违规（误报）：{violations:#?}\n实际输出：\n{text}"
        );
    }
}

/// 【AC-004 · 不可区分性锁】**为什么规则①救不回来 —— 把这件事钉成可执行断言**。
///
/// # 这条锁的是「判定输入」本身，不是某次判决
///
/// REQ-058 删掉了「任一行不得同时带两个前缀」那条规则①。
/// QA 独立复核给出的删除理由比 Dev 当初的更硬：
///
/// `verify_track_separation` 只拿到 `(view, rendered)`。
/// 而下面这两种形状，从**渲染文本上不可区分**：
///
/// ```text
/// (a) 合法：claim 正文里写了「业务轨：客户 · biz=accepted」
/// 业务轨：客户 · biz=accepted
/// 建模轨：key=customer · 证据：业务轨：客户 · biz=accepted · model=accepted
///
/// (b) 真 bug：建模轨行额外内联了业务轨正文
/// 业务轨：客户 · biz=accepted
/// 建模轨：业务轨：客户 · biz=accepted · key=customer · model=accepted
/// ```
///
/// 两者的建模轨行**都同时含两个前缀**，而「建模轨行里为什么会出现
/// `业务轨：…`」在 `(view, rendered)` 这一层**推不出来** ——
/// 证据 claim 是**任意的 payload 文本**。所以规则①要么恒真误报，
/// 要么恒假漏报，**不可能既保检测力又不误伤**。
///
/// ⚠️ **本测试不假装规则①的检测力还在。** 形状 (b) 现在**抓不到**
/// （报告 §5.3 与 §6 已如实写明这项损失）。
///
/// # 那这条测试守什么
///
/// 守的是**「不可区分」这个前提本身**：
/// 一旦有人改了渲染 —— 比如给 claim 里的两轨前缀做转义、
/// 或把证据拆到别的行 —— (a) 与 (b) 就**重新变得可区分**了，
/// 此时规则①**可以恢复**，而这条测试会先响。
#[test]
fn test_req058_legitimate_claim_makes_tracks_textually_indistinguishable() {
    // claim 正文**逐字**抄了业务轨那一行。
    let claim = format!("{BIZ_LINE_PREFIX}客户 · biz=accepted");
    let v = view(vec![full_item(
        "客户",
        json!({
            "key": "customer",
            "evidence": [{ "table": "t_customer", "column": "customer_no", "claim": claim }]
        }),
    )]);
    let text = render_review_text(&v);

    let biz_line = text
        .lines()
        .find(|l| l.trim_start().starts_with(BIZ_LINE_PREFIX))
        .expect("fixture 里没有业务轨行")
        .trim()
        .to_string();
    let model_line = text
        .lines()
        .find(|l| l.trim_start().starts_with(MODEL_LINE_PREFIX))
        .expect("fixture 里没有建模轨行");

    // 建模轨行**逐字**含着业务轨行那一整行 —— 形状 (a) 与 (b) 在文本上同形。
    assert!(
        model_line.contains(&biz_line),
        "前提不成立：建模轨行没有逐字含着业务轨行（{biz_line:?}），\
         本条证明不了不可区分性\n实际输出：\n{text}"
    );
    // 这正是规则①会开火的那两个条件 —— 在这里是**合法 payload** 触发的。
    assert!(
        model_line.contains(BIZ_LINE_PREFIX) && model_line.contains(MODEL_LINE_PREFIX),
        "前提不成立：建模轨行没有同时含两个前缀\n实际输出：\n{text}"
    );
    // 而判据对此**必须零违规** —— 它是 claim 正文，不是分轨标记。
    let violations = verify_track_separation(&v, &text);
    assert!(
        violations.is_empty(),
        "合法的 claim 正文被判成分轨违规（误报）：{violations:#?}\n实际输出：\n{text}"
    );
}

/// 【AC-004 · 剥一次】**`strip_prefix` 换成 `trim_start_matches` 必红**。
///
/// 展示名本身就是「业务轨：客户」⇒ 行首连着两个 `业务轨：`。
/// 剥一次得到 `业务轨：客户 · biz=…`（正确）；
/// 剥两次得到 `客户 · biz=…` ⇒ 与期望对不上 ⇒ 误报。
#[test]
fn test_req058_prefix_is_stripped_exactly_once() {
    let target = format!("{BIZ_LINE_PREFIX}客户");
    let v = view(vec![full_item(&target, json!({ "key": "customer" }))]);
    let text = render_review_text(&v);

    // 前提：行首确实连着两个前缀 —— 不然这条测试证明不了什么。
    let biz_line = text
        .lines()
        .find(|l| l.trim_start().starts_with(BIZ_LINE_PREFIX))
        .expect("fixture 里没有业务轨行");
    assert!(
        biz_line.trim_start().starts_with(&format!("{BIZ_LINE_PREFIX}{BIZ_LINE_PREFIX}")),
        "前提不成立：业务轨行没有重复前缀，实际 {biz_line:?}"
    );

    let violations = verify_track_separation(&v, &text);
    assert!(
        violations.is_empty(),
        "前缀被重复剥离 ⇒ 误报：{violations:#?}\n实际输出：\n{text}"
    );
}

/// 【AC-004 · 回归】**真的两轨混成一行时，仍然必须被抓**。
///
/// REQ-058 把「任一行不得同时带两个前缀」那条删掉了（它对任意 payload 文本
/// 只可能误报）。本条守住**替换它的那条规则**没有因此漏抓 ——
/// 混行会让建模轨行消失 ⇒ 行数对不上。
#[test]
fn test_req058_merged_track_lines_are_still_caught() {
    let v = view(vec![
        full_item("客户", json!({ "key": "customer" })),
        full_item("订单", json!({ "key": "order" })),
    ]);
    let text = render_review_text(&v);
    assert!(verify_track_separation(&v, &text).is_empty(), "前提不成立：正控不干净");

    // 变异：把第一条的业务轨行整段并进建模轨行。
    let biz_body = text
        .lines()
        .find(|l| l.trim_start().starts_with(BIZ_LINE_PREFIX))
        .expect("fixture 里没有业务轨行")
        .trim_start()
        .to_string();
    let merged = text
        .replacen(biz_body.as_str(), "", 1)
        .replace(MODEL_LINE_PREFIX, &format!("{biz_body} {MODEL_LINE_PREFIX}"));
    assert_ne!(merged, text, "变异没生效");
    assert!(
        !merged.lines().any(|l| l.trim_start().starts_with(MODEL_LINE_PREFIX) && l.trim_start().starts_with(BIZ_LINE_PREFIX)),
        "前提不成立：变异造不出「行首同时带两前缀」的行"
    );

    let violations = verify_track_separation(&v, &merged);
    assert!(
        !violations.is_empty(),
        "两轨混成一行没被抓到（BYPASS_A 变体回归）\n变异后输出：\n{merged}"
    );
    assert!(
        violations.iter().any(|v| v.contains("建模轨行数")),
        "应当由行数规则抓住（混进来那轨的行没了），实际：{violations:#?}"
    );
}

/// 【AC-004 · 回归】**业务轨行被塞进建模栏仍然被抓**（行首识别没有放松规则②）。
#[test]
fn test_req058_polluted_biz_line_is_still_caught() {
    let v = view(vec![full_item("客户", json!({ "key": "customer" }))]);
    let text = render_review_text(&v);
    let polluted = text.replacen(" · biz=accepted", " · biz=accepted · key=customer", 1);
    assert_ne!(polluted, text, "变异没生效");

    let violations = verify_track_separation(&v, &polluted);
    assert!(
        violations.iter().any(|v| v.contains("业务轨行多了/少了东西")),
        "BYPASS_A 没抓到：{violations:#?}\n变异后输出：\n{polluted}"
    );
}

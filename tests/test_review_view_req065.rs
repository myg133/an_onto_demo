//! REQ-065 · **任何退化标签都必须携带该元素的原始载荷**（不变量复核）。
//!
//! 测试名带 `test_req065_` 前缀。
//!
//! # 本条立的那条不变量
//!
//! > **任何退化标签的渲染结果，都必须携带该元素的原始载荷信息。**
//!
//! 落到可执行的问句上是：**看得见的那个串里，能看见这个元素的原始载荷**。
//!
//! # ⚠️ 它**不是**「渲染非空」
//!
//! `␀有键无值␀` 非空 —— 它照样藏了键名。
//! REQ-058 的 B-1 锁（`test_req058_recognisable_shapes_never_render_empty`）测的是
//! **非空**（挡「静默丢」）；本文件测的是它的**上一层**（挡「藏内容」）。
//! 两条并存、缺一不可，本文件**不取代**前者。
//!
//! # 三条测试各管什么
//!
//! | 测试 | 性质 | 杀谁 |
//! | --- | --- | --- |
//! | [`test_req065_every_degenerate_label_carries_the_original_payload`] | **通用**：语料按**形状网格**生成，断言只问 payload，**不认任何类别名** | **M2（灵魂）**：新加一个藏内容的退化类别 |
//! | [`test_req065_degenerate_corpus_covers_every_category_by_exact_rendering`] | 逐类别**精确**渲染表（穷举 8 类） | 类别措辞漂移 / 被合并；**故意不作为 M1·M2 的杀手**（见其文档） |
//! | [`test_req065_key_without_value_shows_which_keys_exist`] | AC-002 专项：`table`/`column`/`claim` 键名可见 | **M1**：`KeyWithoutValue` 退回不含键名 |
//!
//! # 为什么「类别」不是判据的依据
//!
//! 「每个退化标签都必须携带载荷」是**按不变量逐个复核**得到的纪律，
//! **不是**「BA 点名的 + 想到的」逐个补 —— 所以这里的断言**只从 payload 出发**
//! （「这个串里有没有 `payload` 的紧凑 JSON」），**没有一处提到类别名**。
//! 换成**任何一个**退化类别藏载荷，它都会红。
//!
//! # 结构性保障（生产侧，测试只是把它钉住）
//!
//! `review_view::EvidenceDegeneracy::label` 把标签拼成
//! `类别名 + 载荷指纹(payload)`，**指纹在 `match` 之外追加**
//! ⇒ 新增退化类别**结构上带得上**载荷。

use ontok::domain::review::{Decision, ItemKind, ItemView};
use ontok::domain::review_view::{
    audit_fde_channel, project_review_track_with_summary, DEGENERATE_EVIDENCE_MARK,
};
use serde_json::{json, Value};
use std::collections::BTreeSet;

/// 保留标记的转义写法 —— 与 `review_view` 内部同一套（此处独立写一遍，
/// 测试不 import 被测实现里的私有常量）。
const MARK_ESCAPE: char = '\u{2401}';

// ── fixture ──────────────────────────────────────────────

/// 只有 `evidence` 一栏的条目 —— 这样渲染结果只可能由证据决定。
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

/// 单个 evidence 元素渲染后的那**一行**。
///
/// ⚠️ 它只取 `model.evidence` 里的一行，而不是 `render_review_text` 的整段输出 ——
/// 整段里混着 `key=` / `证据：` 等渲染器自己打的字，会让「载荷在不在」
/// 与「渲染器打了什么」**分不开**，判据因此读的不是它该读的东西。
fn rendered_line(ev: &Value) -> String {
    let v = project_review_track_with_summary(
        &[evidence_only_item(ev.clone())],
        &BTreeSet::new(),
    )
    .0;
    assert_eq!(
        v.items[0].model.evidence.len(),
        1,
        "证据元素被静默丢掉了 —— 「丢掉 = FDE 少一条复核依据，却看不出来」"
    );
    v.items[0].model.evidence[0].clone()
}

// ── 判据的两个小工具 ─────────────────────────────────────

/// 这个 payload **是不是**「保留标记包起来的退化措辞」。
///
/// 判据**只看渲染结果的结构**（定界符），**不认措辞** ——
/// 所以它对「新增一个退化类别」同样成立。
fn is_degenerate_line(line: &str) -> bool {
    line.chars().count() > 1
        && line.starts_with(DEGENERATE_EVIDENCE_MARK)
        && line.ends_with(DEGENERATE_EVIDENCE_MARK)
}

/// 这个 payload **有没有可带的内容**。
///
/// JSON 的四个**规范空形**（`""` / `null` / `{}` / `[]`）各只有一个取值，
/// 类别名已经把它说尽 ⇒ **无可藏**，不要求载荷指纹。
/// 其余一律**有可带的内容** —— 哪怕内容只是空白。
///
/// ⚠️ 这份清单与 `review_view::evidence_payload_fingerprint` 同一口径，
/// 这里**独立**写一遍而不是 import：测试引用被测实现的私有函数，
/// 就等于让判据跟着实现一起漂移。
fn has_nothing_to_hide(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::String(s) => s.is_empty(),
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
        _ => false,
    }
}

/// 这个 payload 在渲染结果里**应该**看得见的那个字符串。
fn expected_fingerprint(v: &Value) -> String {
    v.to_string().replace(DEGENERATE_EVIDENCE_MARK, &MARK_ESCAPE.to_string())
}

// ── 语料：按**形状网格**生成，不是按类别列举 ───────────────

/// 本文件全部退化形状的语料。
///
/// # 为什么要**生成**而不是列清单
///
/// 「退化类别」是**实现里的东西**；判据要认的是**载荷**。
/// 按类别列清单 = 把测试钉死在「已知的这八类」——
/// 那正是 REQ-058 QA 说的「只认得已知的几个，下一个人再加一个照样藏内容」。
/// 所以这里按**形状**铺开：
/// 空形 × 值形态 × 键名集合 × 数组形态 × **近似但不是退化的**形状。
fn corpus() -> Vec<(String, Value)> {
    let mut c: Vec<(String, Value)> = Vec::new();

    // ── ① JSON 规范空形：各只有一个取值，**无可藏** ──
    c.push(("规范空形/空串".into(), json!("")));
    c.push(("规范空形/null".into(), json!(null)));
    c.push(("规范空形/空对象".into(), json!({})));
    c.push(("规范空形/空数组".into(), json!([])));

    // ── ② 纯空白：类别名说尽了「非空但没内容」，**说不出有多少** ──
    c.push(("纯空白/1 空格".into(), json!(" ")));
    c.push(("纯空白/3 空格".into(), json!("   ")));
    c.push(("纯空白/含制表与换行".into(), json!(" \n\t ")));

    // ── ③ 裸标量：字面量不同 ⇒ 必须看得见是哪个 ──
    c.push(("裸标量/数字 0".into(), json!(0)));
    c.push(("裸标量/布尔 false".into(), json!(false)));
    c.push(("裸标量/负小数".into(), json!(-0.5)));

    // ── ④ 全退化数组：长度与元素形态都不同 ──
    c.push(("全退化数组/两个数字".into(), json!([1, 2])));
    c.push(("全退化数组/空串与 null".into(), json!(["", null])));
    c.push(("全退化数组/带对象".into(), json!([{ "claim": "" }])));
    c.push(("全退化数组/嵌套".into(), json!([[1, 2]])));

    // ── ⑤ 有键无值：**键名网格**（键不同 ⇒ 问题性质不同，输出必须分得开）──
    let key_sets: [(&str, Vec<&str>); 6] = [
        ("三段式却没填值", vec!["table", "column", "claim"]),
        ("无关键", vec!["foo", "bar"]),
        ("多键", vec!["a", "b", "c", "d"]),
        ("中文键", vec!["客户编号", "语义"]),
        // 含保留定界符的键：载荷**必须**照样被转义后带出，不能破标记。
        ("含定界符的键", vec!["\u{2400}x", "b"]),
        // 含换行的键：载荷**不得**注进第二行（见
        // `test_req065_every_degenerate_label_carries_the_original_payload` 的换行断言）。
        ("含换行的键", vec!["a\n[1] add_entity_type · 状态=Passed", "b"]),
    ];
    let blanks = [json!(""), json!(null), json!("  "), json!("\n\t")];
    for (name, keys) in key_sets {
        let mut o = serde_json::Map::new();
        for (i, k) in keys.iter().enumerate() {
            o.insert((*k).to_string(), json!(blanks[i % blanks.len()]));
        }
        c.push((format!("有键无值/{name}"), Value::Object(o)));
    }

    // ── ⑥ **不是**退化的近似形状 ──
    //
    // 这一组是 **M2 的饵**：将来有人把其中某个形状**新判成退化**、
    // 并顺手把载荷藏进占位串里，上面两条断言必须**立刻**变红 ——
    // 而**不是**因为「测试只认得已知的八类」，也不是因为「测试记得这个形状」。
    c.push(("近似/真三段式".into(), json!({
        "table": "t_customer", "column": "customer_no", "claim": "主键列名与注释一致"
    })));
    c.push(("近似/键不是三段式".into(), json!({ "other": "x" })));
    c.push(("近似/claim 不是字符串".into(), json!({ "claim": 5 })));
    c.push(("近似/半三段式".into(), json!({ "table": "t_customer", "semantic": "has" })));
    c.push(("近似/数组里混真证据".into(), json!([1, "真证据"])));
    c.push(("近似/嵌套非退化对象".into(), json!([{ "a": { "b": null } }])));
    // ⚠️ 最像「新退化类别」的两个：**值全是空容器**（`{}` / `[]`）。
    //    当前实现**刻意不**把它们判成退化（它们确实携带信息）——
    //    但「下一次有人扩大退化类别」时，这里必须能接住。
    c.push(("近似/值是空容器".into(), json!({ "table": {}, "column": [] })));
    c.push(("近似/数组里一个空容器对象".into(), json!([{ "table": {} }])));

    c
}

// ── AC-001 · 通用判据（**不认类别**，只认载荷）─────────────

/// 【AC-001 · 灵魂】**任何退化标签都必须携带该元素的原始载荷**。
///
/// # 判据（逐条独立）
///
/// ① 每个元素都渲染出一条**非空**串 —— B-1 的锁不许回归；
/// ② 判成退化 ⇒ 带保留定界符（否则 FDE 看不见它退化）；
/// ③ **判成退化且有可带的内容 ⇒ 渲染串里必须能看见该元素的紧凑 JSON**；
/// ④ 退化标签里**不得**出现原始换行 —— 载荷不得注入行结构；
/// ⑤ **互异性**：两个不同的载荷**不得**渲染成逐字相同的串；
/// ⑥ 未判成退化的元素**不得**带退化标记（判据不许自相矛盾）。
///
/// # ③ 为什么不是 `assert!(!line.is_empty())`
///
/// `␀有键无值␀` 非空 —— 它照样藏了键名。
/// ③ 问的是「**看得见的串里有没有原始载荷**」，那才是本条的不变量。
///
/// # 杀的是哪个变异
///
/// - **M1**（`KeyWithoutValue` 退回不含键名）→ ③ 红（且 ⑤ 也红）；
/// - **M2**（**新加**一个藏内容的退化类别）→ ③ 红 —— 这是本条的**核心价值**：
///   杀它的判据**一处类别名都没出现**，所以它认的不是「已知的几类」。
#[test]
fn test_req065_every_degenerate_label_carries_the_original_payload() {
    let mut empty_lines: Vec<String> = Vec::new();
    let mut unmarked: Vec<String> = Vec::new();
    let mut marked_without_payload: Vec<String> = Vec::new();
    let mut line_injection: Vec<String> = Vec::new();
    let mut false_marks: Vec<String> = Vec::new();
    let mut rendered: Vec<(String, String)> = Vec::new();

    for (name, ev) in corpus() {
        let line = rendered_line(&ev);
        rendered.push((name.clone(), line.clone()));

        // ① 非空（REQ-058 B-1 的锁，本条不许让它回归）
        if line.trim().is_empty() {
            empty_lines.push(format!("[{name}] 渲染成了空串"));
            continue;
        }

        // ④ 载荷不得注进行结构：标签里出现原始换行 ⇒ 第二行是 payload 造出来的。
        if line.contains('\n') {
            line_injection.push(format!("[{name}] 渲染串里出现了原始换行：{line:?}"));
        }

        let degenerate = is_degenerate_line(&line);
        // ⑥ 非退化元素不许带退化标记（判据与渲染不许各说各话）
        if !degenerate && has_nothing_to_hide(&ev) {
            false_marks.push(format!(
                "[{name}] 载荷 {ev} 是规范空形，却没被标成退化 ⇒ 判据把它算成了「有依据」"
            ));
        }

        // ② + ③ 本条的正题
        if degenerate {
            // 两个定界符之间必须真的有话 —— 空壳标签等于「看不见它退化」。
            let inner = line
                .strip_prefix(DEGENERATE_EVIDENCE_MARK)
                .and_then(|s| s.strip_suffix(DEGENERATE_EVIDENCE_MARK))
                .unwrap_or_default();
            if inner.trim().is_empty() {
                unmarked.push(format!("[{name}] 退化标签里没有任何措辞：{line:?}"));
                continue;
            }
            if !has_nothing_to_hide(&ev) {
                let want = expected_fingerprint(&ev);
                if !line.contains(&want) {
                    marked_without_payload.push(format!(
                        "[{name}] 退化标签 {line:?} 里**看不见**该元素的载荷 {want:?}\n\
                         ⇒ 退化标签把原始载荷藏起来了（REQ-065 不变量）"
                    ));
                }
            }
        }
    }

    assert!(empty_lines.is_empty(), "渲染成了空串（B-1 回归）：\n{}", empty_lines.join("\n"));
    assert!(
        unmarked.is_empty(),
        "退化标签里没有任何措辞（FDE 看不见它退化成什么）：\n{}",
        unmarked.join("\n")
    );
    assert!(
        marked_without_payload.is_empty(),
        "退化标签藏起了原始载荷：\n{}",
        marked_without_payload.join("\n")
    );
    assert!(
        line_injection.is_empty(),
        "载荷注入了行结构（渲染串里出现原始换行）：\n{}",
        line_injection.join("\n")
    );
    assert!(
        false_marks.is_empty(),
        "规范空形没被标成退化（判据把它算成了「有依据」）：\n{}",
        false_marks.join("\n")
    );

    // ⑤ 互异性：不同载荷 ⇒ 不同渲染串。
    //
    // ⚠️ 这一条**独立于**上面的逐项断言：它是「藏了载荷」的另一种表现形态 ——
    // 标签仍非空、仍带着类别名，但把两个不同的载荷压成同一个串
    // （`{"table":"","column":"","claim":""}` 与 `{"foo":"","bar":null}`
    // 就是 REQ-058 QA 实测到的那一对）。
    let mut collisions: Vec<String> = Vec::new();
    for i in 0..rendered.len() {
        for j in (i + 1)..rendered.len() {
            if rendered[i].1 == rendered[j].1 {
                collisions.push(format!("[{}] 与 [{}] 渲染成同一个串 {:?}", rendered[i].0, rendered[j].0, rendered[i].1));
            }
        }
    }
    assert!(
        collisions.is_empty(),
        "不同载荷渲染成逐字相同的串 ⇒ 载荷被藏起来了：\n{}",
        collisions.join("\n")
    );

    // 前提自检：语料必须**真的**同时踩到退化与非退化两条路，
    // 否则上面那些断言可能在一条空跑的路上全绿（恒真判据的老坑）。
    let degenerate_count = corpus().iter().filter(|(_, e)| is_degenerate_line(&rendered_line(e))).count();
    assert!(
        degenerate_count >= 8,
        "语料只踩到 {degenerate_count} 个退化形状 ⇒ 覆盖面不足，判据可能在空跑"
    );
}

/// 【AC-001】退化语料**穷举 8 个类别**并钉住**逐个**的精确渲染。
///
/// # 它与上一条的分工（写明是为了不被误当成同一个东西）
///
/// 上一条是**通用**判据（只认载荷、不认类别），是 M2 的杀手；
/// 本条是**逐类别**的记录：把当前实现的 8 个退化类别**穷举**出来，
/// 钉住各自的措辞与载荷。
///
/// ⚠️ 本条**不是**覆盖面保证：将来新增第 9 个类别而语料没跟着铺，
/// 本条**照样绿**。真正的保证是上一条（+ 生产侧 `label()` 的结构）——
/// **若把覆盖面寄托在这张表上，就又回到「只认得已知的几个」。**
#[test]
fn test_req065_degenerate_corpus_covers_every_category_by_exact_rendering() {
    let rows: [(&str, Value); 8] = [
        ("空串", json!("")),
        ("纯空白", json!("   ")),
        ("空对象", json!({})),
        ("null", json!(null)),
        ("空数组", json!([])),
        ("全退化数组", json!([1, 2])),
        ("裸值", json!(0)),
        ("有键无值", json!({ "table": "", "column": "", "claim": "" })),
    ];

    let mut categories: Vec<&str> = Vec::new();
    for (category, ev) in &rows {
        let want = if has_nothing_to_hide(ev) {
            format!("{DEGENERATE_EVIDENCE_MARK}{category}{DEGENERATE_EVIDENCE_MARK}")
        } else {
            format!(
                "{DEGENERATE_EVIDENCE_MARK}{category} {}{DEGENERATE_EVIDENCE_MARK}",
                expected_fingerprint(ev)
            )
        };
        let got = rendered_line(ev);
        assert_eq!(
            got, want,
            "[{category}] 渲染与预期不符（穷举表要求逐类别可见、且带载荷）"
        );
        if categories.contains(&category) {
            panic!("类别 [{category}] 在表里出现了两次");
        }
        categories.push(category);
    }
    assert_eq!(
        categories.len(),
        rows.len(),
        "退化类别表被改动过 —— 新增/删除类别必须在这里留痕"
    );
}

/// 【AC-001 · 变异的靶标】**载荷指纹真的进了输出**。
///
/// # 为什么这条单独存在
///
/// 「语料里有载荷不同的两个退化形状，而它们的渲染串不同」——
/// **反过来**就证明了「渲染串的差异来自指纹」。
/// 没有它，上面那条测试在「指纹被整体删掉」时会在更早的断言上红，
/// **看不出是这一条锁在管**；而它在 M1（只藏对象载荷）下**会红**，
/// 正好把「指纹确实进了输出」这件事钉成可执行的。
#[test]
fn test_req065_payload_fingerprint_reaches_the_rendered_line() {
    let pairs: [(Value, Value); 4] = [
        (json!({ "table": "", "column": "", "claim": "" }), json!({ "foo": "", "bar": null })),
        (json!({ "a": "", "b": "" }), json!({ "a": "", "b": "", "c": "" })),
        (json!({ "客户编号": "" }), json!({ "客户名称": "" })),
        (json!({ "a": "" }), json!({ "\u{2400}a": "" })),
    ];
    let mut failures: Vec<String> = Vec::new();
    for (a, b) in pairs {
        let (ra, rb) = (rendered_line(&a), rendered_line(&b));
        if ra == rb {
            failures.push(format!("载荷不同却渲染成同一个串：{ra:?}（{a} vs {b}）"));
        }
    }
    assert!(
        failures.is_empty(),
        "载荷指纹没有进渲染结果（标签只带了类别名）：\n{}",
        failures.join("\n")
    );
}

/// 【AC-002】`KeyWithoutValue` 必须**看得见键名** —— 且不牺牲判据判决。
///
/// # 三件事
///
/// ① `table` / `column` / `claim` 三个已知键名**字面可见**；
/// ② QA 在 REQ-058 复验时实测的那一对载荷**不得**渲染成逐字相同的串；
/// ③ `premise_holds` **仍为 `false`** —— 要修的是**呈现**，不是判据
///    （这类确实是退化，判成「有依据」才是错的）。
#[test]
fn test_req065_key_without_value_shows_which_keys_exist() {
    let three_seg = json!({ "table": "", "column": "", "claim": "" });
    let unrelated = json!({ "foo": "", "bar": null });
    let line = rendered_line(&three_seg);

    for key in ["table", "column", "claim"] {
        assert!(
            line.contains(key),
            "键名 [{key}] 在渲染结果里看不见：{line:?}\n\
             ⇒ 提案方看不出自己漏填了哪几栏（REQ-065 AC-002）"
        );
    }

    let other_line = rendered_line(&unrelated);
    assert_ne!(
        line, other_line,
        "「三段式没填值」与「发了无关键」渲染成同一个串 ⇒ 两种不同性质的问题分不开\n\
         实际渲染：{line:?}"
    );

    // ③ 判据判决不变：这类**确实是**退化，不许因为带了载荷就变成「有依据」。
    let v = project_review_track_with_summary(
        &[evidence_only_item(three_seg.clone())],
        &BTreeSet::new(),
    )
    .0;
    let audit = audit_fde_channel(&v);
    assert!(
        !audit.premise_holds,
        "带键名之后 `premise_holds` 变真了 ⇒ 载荷被当成了可复核依据（{audit:?}）"
    );
}
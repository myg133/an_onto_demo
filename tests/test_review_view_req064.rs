//! REQ-064 · **`verify_provenance_scope` 规则①的归属前缀仍用 `contains`（同机制残留）**。
//!
//! 测试名带 `test_req064_` 前缀。
//!
//! # 病根
//!
//! REQ-058 修掉了 F-2（`contains` 认轨 + `trim_start_matches` 剥前缀）在
//! `verify_track_separation` 里的**两处**锐边，但**同一文件里还有第三处**：
//! `verify_provenance_scope` 的规则①仍用 `contains(PROVENANCE_LINE_PREFIX)`
//! 认归属行 ⇒ claim / 展示名里只要含「提案级抽取归属：」这几个字，
//! 那一行就被**数成一条归属行** ⇒ 规则①报「行数为 2」的**误报**。
//!
//! # 修法
//!
//! 改用 REQ-058 已落地的**行首认轨**手段（`line_head_prefix_of`）——
//! **不新写第二套前缀识别**，两套实现必然漂移。
//! （REQ-073 起，本文件**直接调用**那一个实现，判据与守卫共用同一份。）
//!
//! # 每条断言都对着一个变异
//!
//! | 测试 | 杀的变异 |
//! | --- | --- |
//! | `..._claim_containing_...` / `..._display_name_containing_...` | **M1**（改回 `contains`）、**M2**（`starts_with \|\| contains` 混合写法） |
//! | `..._still_recognised_in_all_four_states` / `..._indented_...` | **M3**（收窄过头，真归属行也认不出） |
//! | `..._new_predicate_is_a_pure_tightening` | 「纯收紧」的包含论证（AC-003） |
//! | `..._glued_into_item_header_...` / `..._duplicated_paste_...` | 规则④ 改写后是否**还**抓得住 REQ-061 的 M2b |
//! | `..._single_prefix_recogniser` | **AC-001**：漂移出第二套前缀识别 |
//! | `..._mutually_non_prefixing` | 认前缀 if 链的顺序不变量（否则调整顺序会静默改变判决） |

use ontok::domain::review::{Decision, ItemKind, ItemView};
use ontok::domain::review_view::{
    line_head_prefix_of, project_review_track_with_summary,
    project_review_track_with_summary_and_provenance, render_review_text, verify_provenance_scope,
    verify_track_separation, BIZ_LINE_PREFIX, MODEL_LINE_PREFIX, PROVENANCE_LINE_PREFIX,
    ReviewTrackView,
};
use serde_json::{json, Value};
use std::collections::BTreeSet;

// ── fixture ──────────────────────────────────────────────

fn baseline() -> BTreeSet<String> {
    ["customer".to_string(), "subscription".to_string()].into_iter().collect()
}

/// 一个**建模栏齐备**的条目 —— 展示名与证据 claim 都是 payload 原样文本。
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

/// 三条目 —— N > 1 是 REQ-058 立的规矩：**「逐条目重复打印」那个误导
/// 只有 N > 1 时才现形**，单条目夹具会让规则①恒真。
fn three_items(target: &str, payload: Value) -> Vec<ItemView> {
    (0..3)
        .map(|i| {
            let mut it = full_item(target, payload.clone());
            it.id = format!("{i}:add_entity_type:{target}");
            it
        })
        .collect()
}

/// 有 `extracted_by` ⇒ 带归属；投影与渲染**一次**产出（不自造 rendered）。
fn view_and_text(
    items: &[ItemView],
    extracted_by: Option<&Value>,
) -> (ReviewTrackView, String) {
    let (view, _s) =
        project_review_track_with_summary_and_provenance(items, &baseline(), extracted_by);
    let text = render_review_text(&view);
    (view, text)
}

fn declared_eb() -> Value {
    json!({ "agent": "mcode-agent", "version": "2026.10.07", "round": 1, "failed_groups": [] })
}

/// 「这一行的行首（去掉缩进）是归属前缀」—— **直接调用生产实现**。
///
/// # ⚠️ REQ-073：这里**曾经**是判据的**复刻**
///
/// 原写法在本文件里另写了一份 `line.trim_start().starts_with(PROVENANCE_LINE_PREFIX)`，
/// 与生产 [`line_head_prefix_of`] 的归属分支**各写各的** ——
/// 判据改了而复刻没改 ⇒ 这些前提断言会**对着过时的认知**判断。
///
/// 现在直接 `use` 生产那一份 ⇒ **不可能再分叉**。
/// （`tests/*.rs` 是集成测试、只看得到 `pub`，所以生产侧是 `#[doc(hidden)] pub`；
/// 代价与另一条被否掉的方案见 `tests/test_review_view_req068.rs` 的同名说明。）
fn is_provenance_line_head(line: &str) -> bool {
    line_head_prefix_of(line) == Some(PROVENANCE_LINE_PREFIX)
}

// ── AC-002 · claim / 展示名含归属前缀 ⇒ 零违规 ──────────────

/// 【AC-002 · 杀 M1 / M2】**证据 claim 里含「提案级抽取归属：」⇒ 零违规**。
///
/// # 杀的是哪个变异
///
/// - **M1**：规则①改回 `contains`；
/// - **M2**：规则①写成 `starts_with(..) || contains(..)` 的**混合写法**
///   （只剥一次但保留 `contains` 语义 —— BA 在 REQ-058 里栽过的同一个坑）。
///
/// 两个变异都会把 claim 那一行**数成第二条归属行** ⇒ 规则①报「行数为 2」。
#[test]
fn test_req064_claim_containing_provenance_prefix_is_not_a_false_positive() {
    let items = three_items(
        "客户",
        json!({
            "key": "customer",
            "properties": [{ "name": "客户编号", "column": "customer_pk" }],
            "evidence": [{
                "table": "t_customer",
                "column": "customer_no",
                "claim": format!("{PROVENANCE_LINE_PREFIX}agent=伪造的归属")
            }]
        }),
    );
    let (v, text) = view_and_text(&items, Some(&declared_eb()));

    // 前提：claim 正文**真的**落在了渲染文本里，且**真的**不在行首 ——
    // 否则这条测试什么都没测（恒为真的判据 = 无判别力）。
    let mid_line_mentions =
        text.lines().filter(|l| l.contains(PROVENANCE_LINE_PREFIX) && !is_provenance_line_head(l)).count();
    assert!(
        mid_line_mentions >= 1,
        "前提不成立：渲染里没有「含前缀但不在行首」的行，本条证明不了误报\n实际输出：\n{text}"
    );

    let violations = verify_provenance_scope(&v, &text);
    assert!(
        violations.is_empty(),
        "claim 里带「{PROVENANCE_LINE_PREFIX}」被判成归属行（误报）：{violations:#?}\n实际输出：\n{text}"
    );
    // 两轨判据也不该受牵连 —— 两条判据共用同一个认前缀机制，但**各判各的**。
    let tv = verify_track_separation(&v, &text);
    assert!(tv.is_empty(), "两轨分界被判违规：{tv:#?}\n实际输出：\n{text}");
}

/// 【AC-002 · 杀 M1 / M2】**展示名含「提案级抽取归属：」⇒ 零违规**。
///
/// AC 点名的是「claim **或别的字段**」，所以两个 payload 字段各测一遍。
#[test]
fn test_req064_display_name_containing_provenance_prefix_is_not_a_false_positive() {
    let target = format!("{PROVENANCE_LINE_PREFIX}客户");
    let items = three_items(
        &target,
        json!({
            "key": "customer",
            "properties": [{ "name": "客户编号", "column": "customer_pk" }],
            "evidence": [{ "table": "t_customer", "column": "customer_no", "claim": "客户编号唯一" }]
        }),
    );
    let (v, text) = view_and_text(&items, Some(&declared_eb()));

    let mid_line_mentions =
        text.lines().filter(|l| l.contains(PROVENANCE_LINE_PREFIX) && !is_provenance_line_head(l)).count();
    assert_eq!(
        mid_line_mentions, 3,
        "前提不成立：3 个条目的展示名都应产出一处非行首的前缀出现，实际 {mid_line_mentions}\n实际输出：\n{text}"
    );

    let violations = verify_provenance_scope(&v, &text);
    assert!(
        violations.is_empty(),
        "展示名里带「{PROVENANCE_LINE_PREFIX}」被判成归属行（误报）：{violations:#?}\n实际输出：\n{text}"
    );
    let tv = verify_track_separation(&v, &text);
    assert!(tv.is_empty(), "两轨分界被判违规：{tv:#?}\n实际输出：\n{text}");
}

// ── AC-003 · 反向变异：真归属行仍然被认出 ──────────────────

/// 【AC-003 · 杀 M3】**四种归属状态的真归属行都必须被认出 ⇒ 零违规**。
///
/// # 杀的是哪个变异
///
/// **M3**「收窄认前缀条件时收窄过头，忘了真实归属行的位置就在行首」——
/// 例如把条件改成「必须以列 0 开头」或「整行必须逐字等于某个模板」。
/// 那样真归属行会**认不出来** ⇒ 规则①报「行数为 0」⇒ 这里变红。
#[test]
fn test_req064_real_provenance_line_is_still_recognised_in_all_four_states() {
    let items = three_items(
        "客户",
        json!({ "key": "customer", "evidence": [{ "table": "t_customer", "column": "customer_no", "claim": "客户编号唯一" }] }),
    );

    let cases: Vec<(&str, ReviewTrackView, String)> = vec![
        {
            let (v, t) = view_and_text(&items, Some(&declared_eb()));
            ("已申报", v, t)
        },
        {
            let (v, t) = view_and_text(&items, None);
            ("人工修订", v, t)
        },
        {
            // 两参入口 = 调用方压根没接归属 ⇒ 未提供。
            let (v, _s) = project_review_track_with_summary(&items, &baseline());
            let t = render_review_text(&v);
            ("未提供", v, t)
        },
        {
            let (v, t) = view_and_text(&items, Some(&json!({ "agent": "" })));
            ("形态可疑", v, t)
        },
    ];

    for (name, v, text) in cases {
        // 前提：真归属行就在行首（渲染器打的就是行首，不是缩进后）。
        let heads = text.lines().filter(|l| is_provenance_line_head(l)).count();
        assert_eq!(
            heads, 1,
            "前提不成立：{name} 之下渲染没有恰好一条行首归属行，实际 {heads}\n实际输出：\n{text}"
        );
        let violations = verify_provenance_scope(&v, &text);
        assert!(
            violations.is_empty(),
            "{name} 之下真归属行被认不出来了（判据收窄过头）：{violations:#?}\n实际输出：\n{text}"
        );
    }
}

/// 【AC-003 · 杀 M3 的另一种收窄】**带缩进的归属行仍然被认出**。
///
/// 行首认轨认的是「**去掉缩进之后**的行首」—— 这是 REQ-058 定的语义，
/// 三处共用同一套。把它收窄成「必须逐字从列 0 开始」是另一种收窄过头。
#[test]
fn test_req064_indented_provenance_line_is_still_recognised() {
    let items = three_items(
        "客户",
        json!({ "key": "customer", "evidence": [{ "table": "t_customer", "column": "customer_no", "claim": "客户编号唯一" }] }),
    );
    let (v, text) = view_and_text(&items, Some(&declared_eb()));

    // 变异：把归属行整体缩进两格（渲染格式承诺「缩进 + 前缀打在行首」）。
    let indented = text
        .lines()
        .map(|l| {
            if is_provenance_line_head(l) { format!("  {l}") } else { l.to_string() }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";

    let violations = verify_provenance_scope(&v, &indented);
    assert!(
        violations.is_empty(),
        "带缩进的归属行被认不出来了（收窄过头 · 要求列 0）：{violations:#?}\n变异输出：\n{indented}"
    );
}

// ── AC-003 · 纯收紧的包含论证 + 样本计数 ─────────────────

/// 【AC-003】**新判据命中 ⇒ 旧判据必命中（纯收紧）**，并给出样本计数。
///
/// # 论证
///
/// 逐行看：对任意行 `l` 与前缀 `P`，
///
/// ```text
/// 新判据：l.trim_start().starts_with(P)
/// 旧判据：l.contains(P)
/// ```
///
/// `trim_start()` 只删**行首**空白，不改其余字符的位置 ⇒ 若 `l` 以 `P` 开头，
/// 则 `P` 必然作为子串出现在 `l` 中 ⇒ `新 ⇒ 旧`。
/// 所以**新判据判为归属行的行集合 ⊆ 旧判据的**，
/// 即**没有任何一行是「新判据命中、旧判据不命中」**（新过旧挂 = 0）。
///
/// 反方向**不成立、也不应该成立**：claim 里含前缀的那一行
/// 旧判据命中、新判据不命中（**旧过新挂**）—— 那正是本条要消灭的误报。
#[test]
fn test_req064_new_predicate_is_a_pure_tightening() {
    let claim_payload = json!({
        "key": "customer",
        "evidence": [{
            "table": "t_customer", "column": "customer_no",
            "claim": format!("{PROVENANCE_LINE_PREFIX}agent=claim 正文里的引用")
        }]
    });
    let name_payload = json!({
        "key": "customer",
        "evidence": [{ "table": "t_customer", "column": "customer_no", "claim": "客户编号唯一" }]
    });

    // 语料：真实渲染 + 两个**判别力**样本 + 三个 REQ-061 变异。
    let mut corpus: Vec<(String, String)> = Vec::new();
    for (name, items, eb) in [
        ("基线(3条目)", three_items("客户", name_payload.clone()), Some(declared_eb())),
        ("claim 含前缀", three_items("客户", claim_payload.clone()), Some(declared_eb())),
        (
            "展示名含前缀",
            three_items(&format!("{PROVENANCE_LINE_PREFIX}客户"), name_payload.clone()),
            Some(declared_eb()),
        ),
        ("人工修订", three_items("客户", name_payload.clone()), None),
        ("形态可疑", three_items("客户", name_payload.clone()), Some(json!({ "agent": "" }))),
    ] {
        corpus.push((name.to_string(), view_and_text(&items, eb.as_ref()).1));
    }
    let good = view_and_text(&three_items("客户", name_payload.clone()), Some(&declared_eb())).1;
    let prov_line = good
        .lines()
        .find(|l| is_provenance_line_head(l))
        .expect("未变异输出里必须有行首归属行")
        .to_string();
    let header = good
        .lines()
        .find(|l| l.trim_start().starts_with("[1]"))
        .expect("渲染里必须有第 1 个条目头行")
        .to_string();
    // M2b：归属被拼进条目头行（独占行消失）。
    corpus.push((
        "变异M2b(拼进条目头行)".to_string(),
        good.lines()
            .filter(|l| !l.contains(PROVENANCE_LINE_PREFIX))
            .map(|l| if *l == header { format!("{l} · {}", prov_line.trim_start()) } else { (*l).to_string() })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n",
    ));
    // 逐条目重复打印（REQ-060 的误导变体）。
    corpus.push((
        "变异(逐条目重复打印)".to_string(),
        good.lines()
            .flat_map(|l| {
                if is_provenance_line_head(l) {
                    vec![l.to_string(), PROVENANCE_LINE_PREFIX.to_string()]
                } else {
                    vec![l.to_string()]
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n",
    ));
    // 重复粘贴到条目头行、而真归属行仍在（REQ-064 收紧后**仍必须**被抓）。
    corpus.push((
        "变异(重复粘贴到条目头行)".to_string(),
        good.lines()
            .map(|l| if *l == header { format!("{l} · {}", prov_line.trim_start()) } else { (*l).to_string() })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n",
    ));

    let mut total = 0usize;
    let mut old_only = 0usize; // 旧过新挂 —— 本条**故意**制造的差异
    let mut new_only: Vec<(String, String)> = Vec::new(); // 新过旧挂 —— 必须是 0（纯收紧）
    for (name, text) in &corpus {
        for l in text.lines() {
            total += 1;
            let new_hit = is_provenance_line_head(l);
            let old_hit = l.contains(PROVENANCE_LINE_PREFIX);
            if new_hit && !old_hit {
                new_only.push((name.clone(), l.to_string()));
            }
            if old_hit && !new_hit {
                old_only += 1;
            }
        }
    }

    assert!(
        new_only.is_empty(),
        "纯收紧被破坏：新判据命中而旧判据不命中 {n} 处 ⇒ 新判据集合不再是旧判据集合的子集：{new_only:#?}",
        n = new_only.len()
    );
    assert!(
        old_only > 0,
        "旧过新挂 = 0 ⇒ 语料根本没覆盖到差异，这包含论证是**恒真**的（无判别力）：\
         总行数 {total}"
    );
    println!("AC-003 样本计数：总行 {total} · 新过旧挂 {} · 旧过新挂 {old_only}", new_only.len());
}

// ── 规则④ 改写后的检测力（回归） ─────────────────────────

/// 【回归】**REQ-061 的 M2b「归属只拼进第 1 个条目头行」仍然被抓，且说得出位置**。
///
/// 规则④ 在 REQ-064 里**改了形态**（见源码注释），所以这条守着改写后它还抓得住。
#[test]
fn test_req064_glued_into_item_header_is_still_caught_and_named() {
    let items = three_items(
        "客户",
        json!({ "key": "customer", "evidence": [{ "table": "t_customer", "column": "customer_no", "claim": "客户编号唯一" }] }),
    );
    let (v, good) = view_and_text(&items, Some(&declared_eb()));
    assert!(
        verify_provenance_scope(&v, &good).is_empty(),
        "对照组没过 ⇒ 判据恒为真"
    );
    let prov_line = good
        .lines()
        .find(|l| is_provenance_line_head(l))
        .expect("未变异输出里必须有行首归属行")
        .to_string();
    let header = good
        .lines()
        .find(|l| l.trim_start().starts_with("[1]"))
        .expect("渲染里必须有第 1 个条目头行")
        .to_string();
    let mutant = good
        .lines()
        .filter(|l| !l.contains(PROVENANCE_LINE_PREFIX))
        .map(|l| if *l == header { format!("{l} · {}", prov_line.trim_start()) } else { (*l).to_string() })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";

    let violations = verify_provenance_scope(&v, &mutant);
    assert!(
        !violations.is_empty(),
        "「归属只拼进第 1 个条目头行」没有被抓住（规则④ 改写后失去检测力）\n变异输出：\n{mutant}"
    );
    assert!(
        violations.iter().any(|x| x.contains("条目头行")),
        "违规理由没指出「被拼进条目头行」：{violations:?}\n变异输出：\n{mutant}"
    );
}

/// 【回归】**归属行还在、却又被重复粘到条目头行 ⇒ 仍然被抓**。
///
/// # 为什么这条不能省
///
/// 规则①改成行首认之后，「多出一处非行首的归属前缀」在 payload 行上
/// 与「claim 正文里提到它」**逐字不可区分**（同 REQ-058 报告 §5.3 的
/// 形状 (a)/(b)）—— 那里**故意**不报。
///
/// # QA 的反例：payload 里的换行**曾经**能把「归属前缀」切到**行首**
///
/// `evidence_object_body` 曾把 claim **逐字** `push_str`，全文件只转义退化标记的
/// 字符、**不转义换行** ⇒ claim 里的 `\n` 会把建模轨那一行**切成多行**，
/// 切出来的片段可以**逐字长得像条目头行**：
///
/// ```text
/// 建模轨：key=customer · 证据：…：第一段
/// [1] 提案级抽取归属：agent=这是 claim 的正文   ← 这一行每个字节都来自 payload
/// 第二段 · model=accepted
/// ```
///
/// 中间那行 `is_item_header_line`（**纯句法**）判它是条目头行。
/// ⇒ 它**不是**渲染器粘上去的，却长得一模一样。
///
/// ⚠️ **REQ-069 已从根上修掉这个形态**：所有取自 payload 的文本在**进投影那一刻**
/// 一律走 `escape_payload_text`（LF/CR ⇒ `␊`/`␍`）⇒ payload **造不出新行**。
/// 下面那条前提断言已随之从「`faked == 3`」改成「`faked == 0`」——
/// **改的是前提描述的那个事实，不是判据的松紧**。
///
/// ⚠️ 这条是 REQ-064 第二轮 QA 判 FAIL 的**唯一阻断项**：
/// 我第一轮加的规则④ 分支 (b)「条目头行上出现归属前缀 ⇒ 违规」
/// 就是靠这个纯句法判据开的火 ⇒ **3 个条目 ⇒ 3 条理由全错的误报**，
/// 而实际上什么都没发生。分支 (b) 已删。
///
/// 现在规则④ 只剩分支 (a)（`prov_lines` 为空时才看 `contains`），
/// 而这一形态下 `prov_lines` 仍**恰好 1**（真归属行）⇒ 零违规。
#[test]
fn test_req064_newline_in_claim_is_not_a_pasted_provenance_line() {
    let claim = format!("第一段\n[1] {PROVENANCE_LINE_PREFIX}agent=这是 claim 的正文\n第二段");
    let items = three_items(
        "客户",
        json!({
            "key": "customer",
            "evidence": [{ "table": "t_customer", "column": "customer_no", "claim": claim }]
        }),
    );
    let (v, text) = view_and_text(&items, Some(&declared_eb()));

    // 前提：payload 的换行**不再**切出「行首带归属前缀」的片段行。
    //
    // ⚠️⚠️ **REQ-069 改了这条前提的取值方向**（3 → 0）—— 这不是「放宽断言」，
    // 是**前提描述的那个事实本身被修掉了**。
    //
    // 原来这里断言 `faked == 3`：那是**刻意**确认「payload 真的能造出伪结构行」，
    // 好让下面 `violations.is_empty()` 那句**不是恒真**（当时真正要防的是
    // 「规则④ 分支 (b) 对这个形态开火 ⇒ 3 条理由全错的误报」）。
    // REQ-069 在**渲染层**掐掉了换行 ⇒ 那个形态**不再存在** ⇒ 前提必须改成 0。
    //
    // # 为什么改前提**没有**让这条测试失去判别力
    //
    // 判别力**全部转移到了前提这一句上**：一旦有人把渲染层的换行转义去掉
    // （M1），`faked` 立刻回到 3 ⇒ 这条测试**照样变红**。
    // ⚠️ 所以「前提恒为 0」是个**危险的恒真** —— 它只在「转义确实生效」时为真，
    // 而那正是被测代码的性质。`test_req069_payload_cannot_add_a_line_anywhere`
    // 另有一条**独立的**通用守卫盯同一件事（两条互不依赖）。
    let faked = text
        .lines()
        .filter(|l| l.trim_start().starts_with(&format!("[1] {PROVENANCE_LINE_PREFIX}")))
        .count();
    assert_eq!(
        faked, 0,
        "前提不成立：payload 的换行应当**造不出**「[1] 归属前缀…」的片段行（REQ-069），实际 {faked}\
         ⇒ 渲染层又开始逐字拼接 payload 了\n实际输出：\n{text}"
    );
    // 且真归属行仍然**恰好一条**（所以这条测的不是规则①）。
    assert_eq!(
        text.lines().filter(|l| is_provenance_line_head(l)).count(),
        1,
        "前提不成立：真归属行应当还在（否则测到的是规则①，不是规则④）\n实际输出：\n{text}"
    );

    let violations = verify_provenance_scope(&v, &text);
    assert!(
        violations.is_empty(),
        "claim 里的换行被判成「归属被重复拼进条目头行」（误报，理由全错）：{violations:#?}\n实际输出：\n{text}"
    );
}

// ── AC-001 · 只有一套前缀识别 ────────────────────────────

/// 本文件源码本身 —— 用来**数**认前缀的实现有几套。
const SRC: &str = include_str!("../src/domain/review_view.rs");

/// 【AC-001】**前缀识别只有一处**，归属判据不许自己认前缀。
///
/// AC-原文：「两套实现必然漂移，而『两套前缀识别不一致』正是 REQ-058 要消灭的那类 bug」。
/// 这条把「只有一套」钉成可执行断言 —— 否则漂移不会被任何人发现。
#[test]
fn test_req064_only_one_prefix_recogniser_exists() {
    // ① 认前缀的函数**只有一个定义**。
    let defs = SRC.matches("fn line_head_prefix_of(").count();
    assert_eq!(defs, 1, "认前缀的函数应当只有一个定义，实际 {defs} 处");

    // ② REQ-058 那个两轨专用的 `track_prefix_of` 已经**不存在**了 ——
    //    它已被 `line_head_prefix_of` 取代，不是被并排留下。
    assert!(
        !SRC.contains("fn track_prefix_of("),
        "`track_prefix_of` 又冒出来了 ⇒ 存在两套认前缀的实现"
    );

    // ③ `prov_lines` 的构造**必须**走共用 recogniser，且**不得**用 `contains`。
    //
    // ⚠️ 这里找的是**源码里的常量名**（`contains(PROVENANCE_LINE_PREFIX)`），
    // 不是展开后的字面量 —— 源码引用的是常量名，拿字面量去搜**恒为假**，
    // 那就成了一条「永远绿」的判据（恒为真 ⇒ 无判别力）。
    let start = SRC.find("let prov_lines").expect("源码里应当有 `let prov_lines`");
    let stmt = SRC[start..].lines().take(6).collect::<Vec<_>>().join("\n");

    // 把守卫判据本身抽成闭包 —— 下面要对**它**做变异自检。
    let guard_passes = |s: &str| {
        s.contains("line_head_prefix_of(l) == Some(PROVENANCE_LINE_PREFIX)")
            && !s.contains("contains(PROVENANCE_LINE_PREFIX)")
    };

    assert!(guard_passes(&stmt), "`prov_lines` 没有走共用的行首认轨 helper，或仍用 `contains` 认归属前缀：\n{stmt}");

    // ⚠️ 自检：**守卫必须真的能红** ——
    // 「取不到东西就算通过」的判据恒为真，正是本会话反复栽的坑。
    //
    // ⚠️⚠️ 这里**必须从 `SRC` 的真实语句出发**做变异。
    // 第一轮写成「另写一个字面量、断言它含某子串」——
    // 那个断言的真值**完全不依赖 `SRC`**，约等于 `assert!("ab".contains("ab"))`，
    // **它没有证明它声称要证明的事**（QA 第二轮判 FAIL 的阻断项之一）。
    // 现在改成：拿真实语句，分别施加 M1 / M2 两种变异，守卫必须判红。
    //
    // 若 `SRC` 的形状变了导致 `.replace` 打不中，`guard_passes` 仍为真 ⇒
    // `assert!(!…)` 变红 ⇒ 逼人回来更新守卫（这正是想要的行为）。
    let m1 = stmt.replace(
        "line_head_prefix_of(l) == Some(PROVENANCE_LINE_PREFIX)",
        "l.contains(PROVENANCE_LINE_PREFIX)",
    );
    assert!(!guard_passes(&m1), "守卫杀不掉 M1（改回 contains）⇒ 它恒为真（永远绿）");

    let m2 = format!("{stmt} || l.contains(PROVENANCE_LINE_PREFIX)");
    assert!(!guard_passes(&m2), "守卫杀不掉 M2（starts_with || contains 混合写法）⇒ 它恒为真（永远绿）");
}

/// 【不变量】**三个前缀两两不互为前缀** ⇒ 认前缀的 if 链**顺序无关**。
///
/// 源码注释里写了「三个前缀两两不互为前缀，所以 if 链的先后顺序不影响结果」。
/// 注释会腐烂（REQ-061 F-1 的教训）⇒ 这条把它钉成可执行断言：
/// 一旦有人改了任一前缀的字面量而破坏了这条不变量，这里就红。
#[test]
fn test_req064_the_three_prefixes_are_mutually_non_prefixing() {
    let ps = [BIZ_LINE_PREFIX, MODEL_LINE_PREFIX, PROVENANCE_LINE_PREFIX];
    for (i, a) in ps.iter().enumerate() {
        for (j, b) in ps.iter().enumerate() {
            if i == j {
                continue;
            }
            assert!(
                !b.starts_with(a),
                "{b:?} 以 {a:?} 开头 ⇒ 认前缀的 if 链**顺序**会影响判决 \
                 （源码注释断言的「顺序无关」已成假事实）"
            );
        }
    }
}

//! REQ-056 · **`review show` 建模投影**测试（AC-001 / AC-002 / AC-003）。
//!
//! 测试名带 `test_review_view_` 前缀。
//!
//! # 本文件要证明的是「同一条判据、同一份数据、相反的判决」
//!
//! 与 REQ-055 的 `test_model_view.rs` 同一套论证：
//!
//! - FDE 通道输出 ⇒ [`scan_modeling_terms`] **命中**（建模术语是要被裁决的内容）；
//! - 客户视图输出 ⇒ [`scan_modeling_terms`] **空**（建模术语不得泄露给客户）。
//!
//! 两条由**同一个 fixture 提案**驱动。两轨的分界则由
//! [`BIZ_LINE_PREFIX`] / [`MODEL_LINE_PREFIX`] 两行前缀**逐行**核对 ——
//! 「两轨各占一行」是可执行的判据，不是文档里的约定。

use ontok::domain::review::{Decision, ItemKind, ItemView};
use ontok::domain::review_view::{
    audit_fde_channel, project_review_track_with_summary, render_review_text,
    verify_track_separation, ReviewTrackView, BIZ_LINE_PREFIX, KEY_MISSING_MARK, MODEL_LINE_PREFIX,
};
use ontok::domain::view::{
    project, scan_modeling_terms, EntityTypeView, LinkTypeView, PropertyTypeView, SnapshotView,
};

/// 建模栏的标记词 —— **业务轨那一行一个都不许有**。
const MODEL_MARKERS: &[&str] = &[
    "key=",
    "证据",
    "基数",
    "两端",
    "←",
    "evidence",
    "cardinality",
    "column",
];

// ── fixture ──────────────────────────────────────────────

/// 一份**刻意富含建模术语**的提案条目集。
///
/// 每一处建模术语都要能在下游被指名道姓地问出来：
/// - 类型 `key` = `customer` / `subscription`（系统标识）
/// - 列名 `customer_pk` 含 `pk`；`customer_id` 含外键语义
/// - `cardinality` = `1:N`（基数记号）
/// - `evidence` 非空，且带 `主键` / `外键` 字样
/// - 展示名（业务轨那一侧）**全是干净的业务词** —— 这正是两轨能分开的原料
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
            payload: serde_json::json!({
                "key": "customer",
                "display_name": "客户",
                "properties": [
                    { "name": "客户编号", "column": "customer_pk",  "nullable": false },
                    { "name": "客户状态", "column": "status_code",  "nullable": true  }
                ],
                "evidence": [
                    { "table": "t_customer", "column": "customer_no",
                      "claim": "主键列名与注释一致" }
                ]
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
            payload: serde_json::json!({
                "key": "subscription",
                "display_name": "客户订阅",
                "properties": [
                    { "name": "客户编号", "column": "customer_id", "nullable": false }
                ],
                "evidence": [
                    { "table": "t_subscription", "column": "customer_no",
                      "claim": "外键列名与类型表命名一致" }
                ]
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
            payload: serde_json::json!({
                "key": "customer_has_subscription",
                "from_type": "customer",
                "to_type": "subscription",
                "cardinality": "1:N",
                "semantic": "has",
                "evidence": [
                    { "table": "t_subscription", "column": "customer_id",
                      "claim": "外键指向 t_customer" }
                ]
            }),
        },
    ]
}

/// 基线类型：关系两端靠它才判得为「可用」。
fn baseline() -> std::collections::BTreeSet<String> {
    ["customer".to_string(), "subscription".to_string()].into_iter().collect()
}

fn fde_view() -> ReviewTrackView {
    let items = fixture_items();
    let (view, _status) = project_review_track_with_summary(&items, &baseline());
    view
}

fn fde_text() -> String {
    render_review_text(&fde_view())
}

/// 任意条目集 → （投影, 渲染文本）—— BYPASS 探针与判据都收**投影**，
/// 所以这里一次把两样都给出来。
fn view_and_text(items: &[ItemView]) -> (ReviewTrackView, String) {
    let (view, _s) = project_review_track_with_summary(items, &baseline());
    let text = render_review_text(&view);
    (view, text)
}

/// 一个条目：`target`（展示名）+ `payload` 两样都可任意构造。
fn item(id: &str, target: &str, payload: serde_json::Value) -> ItemView {
    ItemView {
        id: id.into(),
        kind: ItemKind::AddEntityType,
        target: target.into(),
        from_type: None,
        to_type: None,
        biz_status: Decision::Accepted,
        model_status: Decision::Accepted,
        payload,
    }
}

/// 同一份 fixture 数据 → 客户视图的投影输入。
///
/// ⚠️ 这是 `cli.rs::snapshot_to_view` 的**测试内同形副本**，理由与
/// `test_model_view.rs` 同：让**同一批 key / 列名 / 基数 / 证据**
/// 同时喂给两个通道，变量只有「用哪个投影」。
fn to_customer_input() -> SnapshotView {
    SnapshotView {
        entity_types: vec![
            EntityTypeView {
                key: "customer".into(),
                display_name: "客户".into(),
                description: Some("签约的企业主体".into()),
                aliases: vec!["用户".into()],
                parent: None,
                disjoint_with: vec!["supplier".into()],
                properties: vec![
                    PropertyTypeView {
                        name: "客户编号".into(),
                        column: "customer_pk".into(),
                        nullable: false,
                        note: Some("营业执照号".into()),
                        sample: Some("«已脱敏»".into()),
                    },
                    PropertyTypeView {
                        name: "客户状态".into(),
                        column: "status_code".into(),
                        nullable: true,
                        note: None,
                        sample: None,
                    },
                ],
                evidence: vec![serde_json::json!({
                    "table": "t_customer", "column": "customer_no",
                    "claim": "主键列名与注释一致"
                })],
            },
            EntityTypeView {
                key: "subscription".into(),
                display_name: "客户订阅".into(),
                description: None,
                aliases: vec![],
                parent: Some("customer".into()),
                disjoint_with: vec![],
                properties: vec![PropertyTypeView {
                    name: "客户编号".into(),
                    column: "customer_id".into(),
                    nullable: false,
                    note: None,
                    sample: None,
                }],
                evidence: vec![],
            },
        ],
        link_types: vec![LinkTypeView {
            key: "customer_has_subscription".into(),
            from_type: "customer".into(),
            to_type: "subscription".into(),
            cardinality: "1:N".into(),
            semantic: "has".into(),
        }],
    }
}

// ── AC-001 · 判别力自证 ──────────────────────────────────

/// 【AC-001】**含建模术语的提案 ⇒ `review show` 输出含建模术语**。
#[test]
fn test_review_view_show_carries_modeling_terms() {
    let text = fde_text();

    // 逐个指名道姓地在内容里核对（不是「守门器叫了」就算过 ——
    // 那只说明有命中，不说明命中的是哪几个词）。
    for (term, where_) in [
        ("key=customer", "实体层系统标识"),
        ("key=subscription", "第二个实体层系统标识"),
        ("customer_pk", "列映射里的主键列名"),
        ("customer_id", "列映射里的外键列名"),
        ("证据：", "证据栏"),
        ("t_customer.customer_no", "证据的表.列定位"),
        ("基数：1:N", "关系层基数"),
        ("两端：customer→subscription", "关系两端"),
        ("key=customer_has_subscription", "关系层系统标识"),
    ] {
        assert!(text.contains(term), "`review show` 输出里找不到 {term}（{where_}）\n实际输出：\n{text}");
    }
}

/// 【AC-001 的另一半】**同一份提案在业务轨的呈现上不得被污染**。
///
/// 建模字段进来了，但**只进建模轨那一行**；业务轨那一行保持干净。
#[test]
fn test_review_view_biz_facing_not_polluted_by_modeling_fields() {
    let text = fde_text();
    let biz_lines: Vec<&str> =
        text.lines().filter(|l| l.contains(BIZ_LINE_PREFIX)).collect();

    assert!(
        !biz_lines.is_empty(),
        "fixture 里一条业务轨行都没有 —— 判据无从下手\n实际输出：\n{text}"
    );
    for line in &biz_lines {
        for marker in MODEL_MARKERS {
            assert!(
                !line.contains(marker),
                "业务轨那一行混进了建模栏 {marker}（AC-001 反面 / AC-003）\n该行：{line}"
            );
        }
    }

    // ⚠️ **再走一遍守门器** —— 上面是逐 marker 比对，这一条是守门器自己的判决。
    // 两者都要成立：守门器判「业务轨这一侧是干净的」。
    let biz_blob = biz_lines.join("\n");
    let leaked = scan_modeling_terms(&biz_blob);
    assert!(leaked.is_empty(), "业务轨呈现被判为泄露建模术语：{leaked:?}\n实际内容：\n{biz_blob}");
}

// ── AC-002 · 建模术语不因此进入客户视图 ────────────────────

/// 【AC-002 · 核心】**同一份数据、同一条判据、相反的判决**。
///
/// 两条单独验都不构成「分离」的证据 —— 合并成一条才构成：
/// 变量只有「用哪个投影」，其余（数据、判据、序列化）全部相同。
#[test]
fn test_review_view_same_data_opposite_verdicts_on_two_channels() {
    let fde = fde_text();
    let customer_json =
        serde_json::to_string(&project(&to_customer_input())).unwrap();

    let fde_hits = scan_modeling_terms(&fde);
    let customer_hits = scan_modeling_terms(&customer_json);

    assert!(
        !fde_hits.is_empty(),
        "FDE 通道这一侧必须是脏的 —— 那正是它存在的理由\n实际输出：\n{fde}"
    );
    assert!(
        customer_hits.is_empty(),
        "客户视图这一侧必须是干净的：{customer_hits:?}\n实际输出：\n{customer_json}"
    );

    // ⚠️ **判别力必须来自「数据」，不只是「字段名」**。
    //
    // 只断言 `!fde_hits.is_empty()` 不够：`key` / `evidence` 这些**字段名本身**
    // 就是建模术语，于是「把内容全清空、只留字段名」的退化实现也能蒙混过关。
    // 所以下面逐个指名道姓地在 FDE 那一侧的**内容**里核对。
    for (term, where_) in [
        ("customer_pk", "列映射的主键列名"),
        ("customer_id", "列映射的外键列名"),
        ("1:N", "基数记号"),
        ("customer_has_subscription", "关系层系统标识"),
    ] {
        assert!(
            fde.contains(term),
            "FDE 通道输出里找不到 {term}（{where_}）—— 数据没真打出来"
        );
    }
}

/// 【AC-001 · 判据】**「FDE 通道不过守门」是带条件的豁免，不是约定**。
///
/// 本条是判据的**判别力**所在：证明豁免取决于一个**可检查的性质**
/// ——「**有建模字段确实来自 payload 且非空**」—— 而不是取决于谁的注释写得好。
///
/// # ⚠️ REQ-057 改写了本条
///
/// 早先这里传的是**渲染文本**、判据是「扫没扫到建模术语」。
/// 那个判据**对任何非空提案恒为真**（渲染器无条件打 `key=`，`key` 是建模术语）
/// ⇒ 测的其实是「提案是否非空」，测不到「字段有没有打出来」。
///
/// 现在判据收 [`ReviewTrackView`]，问的是**字段溯源**。
#[test]
fn test_review_view_fde_exemption_is_conditional_on_a_checkable_property() {
    // 前提成立：建模栏确实来自 payload 且非空 ⇒ 豁免成立。
    let (fde_view, fde) = view_and_text(&fixture_items());
    let fde_audit = audit_fde_channel(&fde_view);

    assert!(
        fde_audit.premise_holds,
        "FDE 通道豁免前提不成立：payload 里有建模栏，判据却说没有\n实际输出：\n{fde}"
    );
    assert!(
        !fde_audit.hits.is_empty(),
        "premise_holds 与 hits 自相矛盾（判据自检失效）"
    );

    // 前提不成立：同样走这条判据，但建模栏**一个都没打出来** ⇒ 豁免被拒。
    //
    // ⚠️ 这一条才是「判据」与「注释」的分界：
    // 写成注释的话，两边都会叫「FDE 通道」，这里就抓不住任何东西。
    let bare = vec![item("bare", "客户", serde_json::json!({ "display_name": "客户" }))];
    let (bare_view, bare_text) = view_and_text(&bare);
    let bare_audit = audit_fde_channel(&bare_view);

    assert!(
        !bare_audit.premise_holds,
        "建模栏一个都没打出来，判据却判前提成立 —— 恒真的老毛病回来了\n实际输出：\n{bare_text}"
    );
    let bare_hits = bare_audit.hits.clone();
    assert!(
        bare_hits.is_empty(),
        "premise_holds 与 hits 自相矛盾（判据自检失效）：{bare_hits:?}"
    );

    // 客户视图那一侧：**不是** `ReviewTrackView`，走的是词表守门器。
    //
    // ⚠️ 刻意保留这一段：它证明两个通道的**文本**仍然可分，
    // 也钉住 REQ-057 的分工 —— 客户侧判「文本里有没有泄露词」，
    // FDE 侧判「字段有没有从 payload 回来」，**不再共用同一条路径**。
    let customer_json = serde_json::to_string(&project(&to_customer_input())).unwrap();
    assert!(
        scan_modeling_terms(&customer_json).is_empty(),
        "客户视图输出被判为 FDE 通道 —— 词表守门被削弱了\n实际输出：\n{customer_json}"
    );
}

/// 【AC-002 · 守门那一侧仍然拦得住】**守门器对客户视图那一侧仍然拦得住**。
///
/// AC-002 要的判别力自证：构造「建模字段同时出现在 FDE 通道与客户视图输出」
/// 的场景，看**守门还拦不拦得住客户视图那一侧**。
///
/// 这里把建模术语**硬塞进客户视图**（模拟模板/文案引入术语 —— REQ-055 记过
/// 「JSON 干净不代表页面干净」），断言守门器**仍然命中**：
/// 本需求**没有**削弱客户视图那道闸。
#[test]
fn test_review_view_gate_still_blocks_customer_view_when_terms_injected() {
    // 一个只有业务措辞的客户视图 —— 守门通过。
    let clean = serde_json::to_string(&project(&to_customer_input())).unwrap();
    assert!(
        scan_modeling_terms(&clean).is_empty(),
        "前提不成立：干净输入本该通过守门\n实际输出：\n{clean}"
    );

    // ⚠️ **同一份数据**，把建模术语注入客户视图的字段值里。
    // 注入的是建模轨**本来就在展示**的那几个词（key / 证据 / 基数 / 列名），
    // 证明「这些词进了客户视图」这件事一旦发生，守门照样拦。
    let mut polluted = to_customer_input();
    polluted.entity_types[0].display_name = "客户主键".into(); // 含「主键」
    polluted.entity_types[0].properties[0].column = "customer_pk".into();
    polluted.link_types[0].cardinality = "1:N".into();
    let polluted_json = serde_json::to_string(&project(&polluted)).unwrap();

    let leaked = scan_modeling_terms(&polluted_json);
    assert!(
        !leaked.is_empty(),
        "建模术语进了客户视图，守门却没叫 —— 守门被削弱了\n实际输出：\n{polluted_json}"
    );

    // 同一批词，在 FDE 通道那一侧是**正常内容**，不构成问题。
    let fde = fde_text();
    assert!(
        scan_modeling_terms(&fde).len() > leaked.len().min(usize::MAX),
        "FDE 通道自检异常"
    );
}

// ── AC-003 · 两轨分界不糊 ─────────────────────────────────

/// 【AC-003】**业务轨与建模轨各占一行，逐行可核对**。
///
/// 判别力方向：变异「把两轨字段混在一起打」——
/// - 混成一行且**不带前缀** ⇒ `biz_lines` / `model_lines` 少一条 ⇒ 被行数判据抓住；
/// - 混成一行且**带业务轨前缀** ⇒ 该行含 `key=` ⇒ 被 marker 判据抓住。
/// 两条路都走不通。
#[test]
fn test_review_view_two_tracks_never_share_a_line() {
    let text = fde_text();
    let items = fixture_items();

    let biz_lines: Vec<&str> =
        text.lines().filter(|l| l.contains(BIZ_LINE_PREFIX)).collect();
    let model_lines: Vec<&str> =
        text.lines().filter(|l| l.contains(MODEL_LINE_PREFIX)).collect();

    // ① 行数对得上：一个条目一行，两轨不共行。
    assert_eq!(
        biz_lines.len(),
        items.len(),
        "业务轨行数与条目数不符（两轨可能被混成了一行）\n实际输出：\n{text}"
    );
    assert_eq!(
        model_lines.len(),
        items.len(),
        "建模轨行数与条目数不符（两轨可能被混成了一行）\n实际输出：\n{text}"
    );

    // ② 业务轨那一行不含任何建模栏。
    for line in &biz_lines {
        for marker in MODEL_MARKERS {
            assert!(
                !line.contains(marker),
                "业务轨那一行混进了建模栏 {marker}\n该行：{line}"
            );
        }
    }

    // ③ 建模轨那一行**必须**带建模栏（否则「建模轨那一行」是空的，
    // 分界就只剩标签没有内容）。
    for line in &model_lines {
        assert!(
            line.contains("key="),
            "建模轨那一行没有建模栏 —— 两轨只是贴了个标签\n该行：{line}"
        );
    }
}

/// 【AC-003】**关系类的两端类型必须是 key 而不是展示名**。
///
/// 这是两轨分界的一个**具体的坑**：展示名（业务轨）与 key（建模轨）
/// 在 `ItemView` 里分处 `target` 与 payload，一旦取错，
/// FDE 看到的就是「两端都是中文名」，判断不出是不是同一个类型。
#[test]
fn test_review_view_link_ends_are_keys_not_display_names() {
    let items = fixture_items();
    let (view, _s) =
        ontok::domain::review_view::project_review_track_with_summary(&items, &baseline());
    let link = view
        .items
        .iter()
        .find(|i| i.kind == ItemKind::AddLinkType)
        .expect("fixture 里没有关系条目");

    assert_eq!(link.model.from_type.as_deref(), Some("customer"));
    assert_eq!(link.model.to_type.as_deref(), Some("subscription"));
    // 业务轨那一侧是展示名。
    assert!(link.biz.display_target.contains("客户"));
    // 建模轨那一侧**不含**展示名。
    //
    // ⚠️ REQ-057：断言收紧成**逐字相等**。原断言是
    // `!type_key.contains("客户")` —— 一个回退出来的展示名
    // 只要不含「客户」两个字就能蒙混过关（比如 BYPASS_E 的「主键服务中心」）。
    // 既然 `type_key` 现在**只**来自 payload，那就直接断言它等于 payload 里的 key。
    assert_eq!(
        link.model.type_key.as_deref(),
        Some("customer_has_subscription"),
        "建模轨的 type_key 必须逐字等于 payload.key，不能是展示名：{:?}",
        link.model.type_key
    );
}

// ══════════════════════════════════════════════════════════
// REQ-057 · F-A：判据必须落在「字段有没有打出来」上
// ══════════════════════════════════════════════════════════

/// 【AC-001-① · BYPASS_E】**建模字段一个都没打出来 ⇒ `premise_holds == false`**。
///
/// 这是 QA 的探针 **BYPASS_E**，也是本需求的**阻断项**：
///
/// - 建模栏全空：`type_key` 取不到、无 `columns`/`evidence`/`cardinality`/两端；
/// - 展示名刻意写成「**主键服务中心**」—— 含「主键」，`scan_modeling_terms` 必命中。
///
/// # ⚠️ 这条断言的判别力全在后半段
///
/// 老判据（扫文本）在这个输入下会给 `true` —— 那正是 QA 判 FAIL 的现象。
/// 所以这里**同时**断言两件事：
/// 1. 文本里**确实**扫得到建模术语（词还在 —— 证明不是「词表变了导致扫不到」）；
/// 2. `premise_holds` **仍然是 `false`**（判据不再跟着词走）。
///
/// 只断言 (2) 的话，将来有人把渲染里的建模术语删干净，(2) 依然通过 ⇒ 无判别力。
#[test]
fn test_review_view_bypass_e_premise_false_even_when_terms_present_in_text() {
    let bypass = vec![item(
        "bypass_e",
        "主键服务中心",
        serde_json::json!({ "display_name": "主键服务中心" }),
    )];
    let (view, text) = view_and_text(&bypass);

    // ① 前提：文本里**确实**有建模术语。老判据在这里会给 true。
    let hits = scan_modeling_terms(&text);
    assert!(
        !hits.is_empty(),
        "前提不成立：这条探针本来就该被词表扫到建模术语，现在扫不到了 —— \
         那判据变 false 的原因就变成了「词没了」，不再是「判据不看词」\n实际输出：\n{text}\n\
         hits={hits:?}"
    );
    assert!(
        hits.iter().any(|h| h.contains("主键")),
        "预期至少命中「主键」：{hits:?}\n实际输出：\n{text}"
    );

    // ② 判据：新判据**不看词**，看字段溯源 ⇒ false。
    let audit = audit_fde_channel(&view);
    assert!(
        !audit.premise_holds,
        "BYPASS_E 建模栏全空，premise_holds 却成立 —— F-A 没修好\n\
         老判据会被文本里的「主键」骗成 true，这正是 QA 判 FAIL 的现象\n\
         实际输出：\n{text}"
    );
    let hits = audit.hits.clone();
    assert!(
        hits.is_empty(),
        "建模栏全空，hits 却非空：{hits:?}\n实际输出：\n{text}"
    );
}

/// 【AC-001-② · 别修过头】**正常条目（有 key/证据/列映射）⇒ 仍然 `true`**。
#[test]
fn test_review_view_premise_still_holds_for_normal_items() {
    let (view, text) = view_and_text(&fixture_items());
    let audit = audit_fde_channel(&view);

    assert!(
        audit.premise_holds,
        "正常条目被误判成「建模字段没打出来」—— 判据修过头了\n实际输出：\n{text}"
    );
    // 逐条指名道姓：三条 fixture 各带了哪几栏，不能只有一个笼统的 true。
    let expected: Vec<String> = vec![
        "item[1].key".into(),
        "item[1].columns".into(),
        "item[1].evidence".into(),
        "item[2].key".into(),
        "item[2].columns".into(),
        "item[2].evidence".into(),
        "item[3].key".into(),
        "item[3].evidence".into(),
        "item[3].cardinality".into(),
        "item[3].from_to".into(),
    ];
    for e in &expected {
        assert!(
            audit.hits.contains(e),
            "hits 里缺 {e}（实际：{:?}）\n实际输出：\n{text}",
            audit.hits
        );
    }
}

/// 【AC-001-③】**「只有 `key=` 空串、其余建模字段全无」⇒ 必须 `false`**。
///
/// 这条专治「把空串当成有」——
/// `payload.key = ""` 在老实现里会渲染出 `key=`（一个空值），
/// 而 `key` 这个**词**照样让守门器命中 ⇒ 前提照样成立。
#[test]
fn test_review_view_empty_key_alone_does_not_hold_premise() {
    let bypass = vec![item(
        "empty_key",
        "客户",
        serde_json::json!({ "key": "", "display_name": "客户" }),
    )];
    let (view, text) = view_and_text(&bypass);
    let audit = audit_fde_channel(&view);

    assert!(
        !audit.premise_holds,
        "payload.key 是空串，premise_holds 却成立\n实际输出：\n{text}"
    );
    // 纯空白同理。
    let blank = vec![item(
        "blank_key",
        "客户",
        serde_json::json!({ "key": "   ", "display_name": "客户" }),
    )];
    let (blank_view, blank_text) = view_and_text(&blank);
    assert!(
        !audit_fde_channel(&blank_view).premise_holds,
        "payload.key 是纯空白，premise_holds 却成立\n实际输出：\n{blank_text}"
    );
}

/// 【AC-001 · 反向隔离】**`hits` 是字段溯源，不是词表命中**。
///
/// 断言形态本身就是判别力：每一项都必须形如 `item[<数字>].<字段名>`。
/// 一旦有人把 `hits` 改回「守门器命中的词」，这里立刻挂 ——
/// 因为「主键」「cardinality」这类词不带 `item[N].` 前缀。
#[test]
fn test_review_view_hits_are_field_provenance_not_wordlist() {
    let (view, _) = view_and_text(&fixture_items());
    let audit = audit_fde_channel(&view);

    assert!(!audit.hits.is_empty(), "前提不成立：hits 空了");
    for h in &audit.hits {
        let Some(rest) = h.strip_prefix("item[") else {
            panic!("hits 里混进了非字段溯源的项 {h:?} —— 判据又退回看词了：{:?}", audit.hits);
        };
        let Some((seq, field)) = rest.split_once("].") else {
            panic!("hits 里混进了非字段溯源的项 {h:?}：{:?}", audit.hits);
        };
        assert!(seq.parse::<usize>().is_ok(), "item 序号不是数字：{h:?}");
        assert!(
            ["key", "columns", "evidence", "cardinality", "from_to"].contains(&field),
            "未知字段名 {field:?} —— 这不像字段溯源，更像从别处扫来的词：{h:?}"
        );
    }
}

// ══════════════════════════════════════════════════════════
// REQ-057 · F-D：type_key 回退不得冒充 key
// ══════════════════════════════════════════════════════════

/// 【AC-002 · 核心】**payload 缺 `key` ⇒ 展示名**不得**被当 key 显示**。
///
/// 逐字断言（不是「有输出」那种）：
/// 渲染文本里**不许出现** `key=主键服务中心`，必须出现 `key={KEY_MISSING_MARK}`。
#[test]
fn test_review_view_missing_key_never_renders_display_name_as_key() {
    let payload_less = vec![item(
        "no_key",
        "主键服务中心",
        serde_json::json!({ "display_name": "主键服务中心" }),
    )];
    let (view, text) = view_and_text(&payload_less);

    // ① 逐字：展示名**不得**紧跟在 `key=` 后面。
    assert!(
        !text.contains("key=主键服务中心"),
        "展示名被当成了 key（F-D 未修）\n实际输出：\n{text}"
    );
    // ② 逐字：缺失必须是**显式标记**，让 FDE 看得见。
    assert!(
        text.contains(&format!("key={KEY_MISSING_MARK}")),
        "payload 缺 key 时没有打出显式缺失标记 {KEY_MISSING_MARK:?}\n实际输出：\n{text}"
    );
    // ③ 逐字（结构层）：投影里 `type_key` 是 `None`，不是那个展示名。
    assert_eq!(
        view.items[0].model.type_key, None,
        "type_key 回退成了展示名：{:?}",
        view.items[0].model.type_key
    );
    // ④ 展示名仍然**只**出现在业务轨那一行（它没被删掉，只是不再冒充 key）。
    let biz: Vec<&str> = text.lines().filter(|l| l.contains(BIZ_LINE_PREFIX)).collect();
    assert_eq!(biz.len(), 1, "业务轨行数不对\n实际输出：\n{text}");
    assert!(
        biz[0].contains("主键服务中心"),
        "展示名应当仍在业务轨呈现\n实际输出：\n{text}"
    );
}

/// 【AC-002 · 补漏】**空白 `key` 同样不回退**，与缺 `key` 一视同仁。
#[test]
fn test_review_view_blank_key_does_not_fall_back_to_display_name() {
    let blank = vec![item(
        "blank",
        "客户中心",
        serde_json::json!({ "key": "  ", "display_name": "客户中心" }),
    )];
    let (_view, text) = view_and_text(&blank);
    assert!(
        !text.contains("key=客户中心"),
        "空白 key 回退成了展示名\n实际输出：\n{text}"
    );
    assert!(
        text.contains(&format!("key={KEY_MISSING_MARK}")),
        "空白 key 应打出缺失标记\n实际输出：\n{text}"
    );
}

// ══════════════════════════════════════════════════════════
// REQ-057 · AC-003：BYPASS_A / B / C 三条的判据
// ══════════════════════════════════════════════════════════

/// 【AC-003 · 正控】**真实渲染下 `verify_track_separation` 必须零违规**。
///
/// 没有这条，下面的 BYPASS 断言可能是「恒真」—— 判据乱报也算通过。
#[test]
fn test_review_view_separation_check_is_clean_on_real_rendering() {
    let (view, text) = view_and_text(&fixture_items());
    let violations = verify_track_separation(&view, &text);
    assert!(
        violations.is_empty(),
        "真实渲染被判为串轨/重复/漏打：{violations:#?}\n实际输出：\n{text}"
    );
}

/// 【AC-003 · BYPASS_A】**建模字段塞进业务轨行 ⇒ 必须被抓**。
///
/// 判据是 [`verify_track_separation`] 的规则②：
/// 业务轨行去掉前缀后**必须恰好**是 `[display_target, "biz=<status>"]`。
///
/// # 为什么这条判据**不受词表影响**
///
/// 它比的是 **token 数量与取值**，不是「有没有某个词」。
/// 往 [`scan_modeling_terms`] 的词表里加词、加多少条，都不会让这条判据变松。
#[test]
fn test_review_view_bypass_a_modeling_fields_on_biz_line_is_caught() {
    let (view, text) = view_and_text(&fixture_items());
    assert!(verify_track_separation(&view, &text).is_empty(), "前提不成立：正控不干净");

    // 变异：把建模栏追加到业务轨那一行后面（QA 的 BYPASS_A）。
    let polluted = text.replacen(" · biz=accepted", " · biz=accepted · key=customer", 1);
    assert_ne!(polluted, text, "变异没生效：业务轨行里没找到 `· biz=accepted`");

    let violations = verify_track_separation(&view, &polluted);
    assert!(
        !violations.is_empty(),
        "BYPASS_A：建模栏混进业务轨行，判据没抓到\n变异后输出：\n{polluted}"
    );
    assert!(
        violations.iter().any(|v| v.contains("业务轨")),
        "BYPASS_A 违规理由应当点名业务轨，实际：{violations:#?}"
    );
}

/// 【AC-003 · BYPASS_A 变体】**两轨混成一行（带业务轨前缀）⇒ 必须被抓**。
#[test]
fn test_review_view_bypass_a_two_tracks_merged_into_one_line_is_caught() {
    let (view, text) = view_and_text(&fixture_items());

    // 变异：把业务轨那一行整段并进建模轨那一行。
    let biz_body = text
        .lines()
        .find(|l| l.contains(BIZ_LINE_PREFIX))
        .expect("fixture 里没有业务轨行")
        .trim_start()
        .to_string();
    let merged = text.replacen(biz_body.as_str(), "", 1).replace(
        &format!("{MODEL_LINE_PREFIX}"),
        &format!("{biz_body} {MODEL_LINE_PREFIX}"),
    );
    assert_ne!(merged, text, "变异没生效");

    let violations = verify_track_separation(&view, &merged);
    assert!(
        !violations.is_empty(),
        "BYPASS_A 变体：两轨混成一行，判据没抓到\n变异后输出：\n{merged}"
    );
}

/// 【AC-003 · BYPASS_C】**两轨内容重复 ⇒ 必须被抓**。
///
/// QA 指出老判据对这条**完全不抓** —— 因为 `premise_holds` 只问「有没有词」，
/// 两轨重复时词只会更多，判据只会更满意。
///
/// 新判据有两处独立抓它：
/// 规则③（建模轨行不得与业务轨行逐字相同 / 不得含业务轨前缀）
/// + 规则④（payload 来源的栏必须真的打在建模轨行上）。
#[test]
fn test_review_view_bypass_c_duplicated_tracks_are_caught() {
    let (view, text) = view_and_text(&fixture_items());

    let model_line = text
        .lines()
        .find(|l| l.contains(MODEL_LINE_PREFIX))
        .expect("fixture 里没有建模轨行")
        .to_string();
    let biz_line =
        text.lines().find(|l| l.contains(BIZ_LINE_PREFIX)).expect("fixture 里没有业务轨行");

    // 变异 C-1：建模轨那一行被业务轨的内容顶替。
    let c1: String = text
        .lines()
        .map(|l| if l.contains(MODEL_LINE_PREFIX) { biz_line.to_string() } else { l.to_string() })
        .collect::<Vec<_>>()
        .join("\n");
    assert_ne!(c1, text, "变异 C-1 没生效");
    let v1 = verify_track_separation(&view, &c1);
    assert!(
        !v1.is_empty(),
        "BYPASS_C-1：建模轨行被业务轨内容顶替，判据没抓到\n变异后输出：\n{c1}"
    );

    // 变异 C-2：建模轨行照抄业务轨的措辞，但**去掉**业务轨前缀，
    // 于是规则①（不共行）放过它 —— 必须由规则③/④抓住。
    let c2 = text.replace(
        model_line.as_str(),
        biz_line.trim_start().replace(BIZ_LINE_PREFIX, "").as_str(),
    );
    assert_ne!(c2, text, "变异 C-2 没生效");
    assert!(
        !c2.lines().any(|l| l.contains(BIZ_LINE_PREFIX) && l.contains(MODEL_LINE_PREFIX)),
        "前提不成立：变异 C-2 本该绕过「前缀共行」那条规则"
    );
    let v2 = verify_track_separation(&view, &c2);
    assert!(
        !v2.is_empty(),
        "BYPASS_C-2：去掉前缀后的复制品，判据没抓到\n变异后输出：\n{c2}"
    );
}

/// 【AC-003 · 规则④】**渲染漏打某一栏 ⇒ 必须被抓**。
///
/// 这一条守住「投影里有、输出上没有」—— 也就是 FDE **看不见**建模数据。
#[test]
fn test_review_view_dropped_modeling_field_is_caught() {
    let (view, text) = view_and_text(&fixture_items());

    // 变异：把第一条的列映射整段抠掉。
    let mutated = text.replacen(" · 列映射：客户编号←customer_pk、客户状态←status_code", "", 1);
    assert_ne!(mutated, text, "变异没生效：没找到列映射那一段");

    let violations = verify_track_separation(&view, &mutated);
    assert!(
        violations.iter().any(|v| v.contains("列映射")),
        "漏打列映射没被抓到，实际违规：{violations:#?}\n变异后输出：\n{mutated}"
    );
}

/// 【AC-003 · BYPASS_B】**省略 `key` 只留 `evidence` ⇒ 展示名不得冒充 key**。
///
/// # 本条的**判据是什么、不判据是什么**（说清楚，别默默放过）
///
/// - **判**：`key` 缺失时渲染 `key=（无 key）`，**绝不**显示展示名
///   —— 这条是 F-D 的直接推论，逐字断言。
/// - **不判**：`premise_holds` 此时**仍为 `true`**。
///   这是**正确**语义 —— `evidence` 确实是来自 payload 且非空的建模栏，
///   前提判据只回答「有没有建模数据」，**不负责**判断条目完不完整
///   （那是规则④ / FDE 本人的活）。
/// - **缺口可见**：`hits` 里会出现 `item[N].evidence` 而**没有** `item[N].key`，
///   条目缺哪一栏一眼看得出来。下面逐字断言这一点。
#[test]
fn test_review_view_bypass_b_evidence_only_never_shows_display_name_as_key() {
    let evidence_only = vec![item(
        "bypass_b",
        "客户中心",
        serde_json::json!({
            "display_name": "客户中心",
            "evidence": [{ "table": "t_customer", "column": "customer_no", "claim": "可复核" }]
        }),
    )];
    let (view, text) = view_and_text(&evidence_only);

    // ① 逐字：展示名没被当 key。
    assert!(
        !text.contains("key=客户中心"),
        "展示名被当成了 key\n实际输出：\n{text}"
    );
    assert!(
        text.contains(&format!("key={KEY_MISSING_MARK}")),
        "缺 key 应打出显式缺失标记\n实际输出：\n{text}"
    );

    // ② 前提判据：evidence 是真的 ⇒ 前提成立（**不是** false，别误判成 false）。
    let audit = audit_fde_channel(&view);
    assert!(
        audit.premise_holds,
        "payload 里确有 evidence，前提不该被判成不成立：{:?}",
        audit.hits
    );
    assert!(
        audit.hits.contains(&"item[1].evidence".to_string()),
        "hits 应列出 evidence：{:?}",
        audit.hits
    );
    // ③ 缺口可见：key 不在列表里。
    assert!(
        !audit.hits.contains(&"item[1].key".to_string()),
        "payload 没有 key，hits 里却出现了 item[1].key：{:?}",
        audit.hits
    );
}
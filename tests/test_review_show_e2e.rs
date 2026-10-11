//! REQ-056 · **`review show` 端到端**（走真实存储层）。
//!
//! 与 `test_review_view.rs` 的分工：
//!
//! - `test_review_view.rs` = **纯逻辑**（零 I/O）—— 证明「两轨分得开、守门拦得住」；
//! - 本文件 = **端到端** —— 走真实的 `PgProposalStore` → `to_item_views` →
//!   `summarize` → 建模投影，证明**真实落库的提案**经 `review show` 那一整条
//!   路径之后，建模字段确实在输出里。
//!
//! # 为什么必须走一遍真库
//!
//! 纯逻辑测试喂的是手工构造的 `ItemView`。而 `review show` 真正读到的
//! 是**从 `payload` 列反序列化出来的** `ItemView` —— 两者之间隔着
//! `PgProposalStore::to_item_views`。**投影层没丢的字段，存储层未必带得回来**，
//! 而那正是「FDE 闭眼点通过」最可能藏身的地方。
//!
//! 本文件复刻 `cli.rs::review_cmd` 的 `ReviewCmd::Show` 分支**完全相同**的调用序列，
//! 只把 `println!` 换成 `render_review_text` 的返回值（`cli.rs` 就是
//! `print!("{text}")`，所以两者是同一段字节）。

mod common;
use common::test_pool;
use ontok::db::project::PgProjectStore;
use ontok::db::proposal::{NewItem, NewProposal, PgProposalStore};
use ontok::db::snapshot_store::PgStore;
use ontok::domain::review::{summarize, Decision, ItemKind, Track};
use ontok::domain::review_view::{
    audit_fde_channel, project_review_track, project_review_track_with_provenance, render_review_text,
    verify_provenance_scope, verify_track_separation,
    ReviewTrackView,
};
use ontok::domain::snapshot::Snapshot;
use ontok::domain::view::scan_modeling_terms;
use serde_json::json;

/// 带**建模术语**的实体条目。
///
/// ⚠️ `target_key` 这里放的是**展示名**，与 `agent::assemble::entity_item` 一致
/// （那里 `target = t.display_name`，再由 `db::proposal` 原样存进 `target_key` 列）。
/// `tests/test_apply.rs` 的 fixture 放的是 key —— 那是**另一种**形状，
/// 照抄会让本文件打印出来的不是生产里真实的那一段。
fn ent(key: &str, display: &str, column: &str, claim: &str) -> NewItem {
    NewItem {
        kind: ItemKind::AddEntityType,
        target_key: display.into(),
        payload: json!({
            "key": key,
            "display_name": display,
            "properties": [{"name": display, "column": column, "nullable": false}],
            "evidence": [{"table": format!("t_{key}"), "column": "customer_no", "claim": claim}]
        }),
    }
}

/// 带**建模术语**的关系条目。
///
/// ⚠️ 同上：`target_key` 放的是「人看的标签」（`CandidateLink.target` 的定义）。
fn link(from: &str, to: &str, from_display: &str, to_display: &str) -> NewItem {
    NewItem {
        kind: ItemKind::AddLinkType,
        target_key: format!("{from_display}→{to_display}"),
        payload: json!({
            "key": format!("{from}_has_{to}"),
            "from_type": from, "to_type": to,
            "cardinality": "1:N", "semantic": "has",
            "evidence": [{"table": format!("t_{to}"), "column": "customer_id",
                          "claim": "外键指向 t_customer"}]
        }),
    }
}

/// 复刻 `cli.rs::review_cmd` 的 `ReviewCmd::Show` 分支。
///
/// ⚠️ 调用序列与 `cli.rs` **逐行对应**（见该分支）：
/// `get` → `items` → `to_item_views` → `get_snapshot(base)` → 基线集合
/// → `summarize` → 建模投影 → 渲染。
async fn review_show_text(items_fixture: &[NewItem]) -> String {
    review_show_view(items_fixture, None).await.1
}

/// REQ-057：判据收的是**投影**不是文本，所以端到端这一层要把 view 一并带出来。
///
/// REQ-060：多收一个 `extracted_by` —— 走真实 JSONB 往返。
async fn review_show_view(
    items_fixture: &[NewItem],
    extracted_by: Option<&ontok::domain::snapshot::ExtractedBy>,
) -> (ReviewTrackView, String) {
    let pool = test_pool().await;
    let projects = PgProjectStore::new(pool.clone());
    let proposals = PgProposalStore::new(pool.clone());
    let store = PgStore::new(pool.clone());
    projects.migrate().await.unwrap();
    let p = common::fresh_project(&projects, "review_show").await;
    let v1 = projects.new_version(p.id).await.unwrap().id;
    store
        .save_snapshot(&Snapshot::new(v1, 1))
        .await
        .unwrap();

    let prop = proposals
        .create(
            p.id,
            NewProposal { base_version_id: v1, round: 1, extracted_by: extracted_by.cloned() },
            items_fixture,
        )
        .await
        .unwrap();

    // ── 以下逐行对应 cli.rs 的 Show 分支 ──
    let row = proposals.get(prop.id).await.unwrap().unwrap();
    let items = proposals.items(prop.id).await.unwrap();
    let views = PgProposalStore::to_item_views(&items);
    let base = store
        .get_snapshot(row.base_version_id)
        .await
        .unwrap()
        .expect("base 版本快照不存在");
    let baseline: std::collections::BTreeSet<String> =
        base.entity_types.iter().map(|e| e.key.clone()).collect();
    let st = summarize(&views, &baseline);
    // REQ-060：走 `ProposalRow.extracted_by`（JSONB 往返后的真实取值）。
    let rv = project_review_track_with_provenance(&views, &st, row.extracted_by.as_ref());
    let text = render_review_text(&rv);
    (rv, text)
}

/// fixture：三条条目，两端类型都在基线里 ⇒ 关系类能判为通过。
fn fixture_items() -> Vec<NewItem> {
    vec![
        ent("customer", "客户", "customer_pk", "主键列名与注释一致"),
        ent("subscription", "客户订阅", "customer_id", "外键列名与类型表命名一致"),
        link("customer", "subscription", "客户", "客户订阅"),
    ]
}

/// 【AC-001 · 端到端】**真实落库的提案 ⇒ `review show` 输出含建模字段**。
#[tokio::test]
async fn test_review_show_e2e_carries_modeling_fields() {
    let text = review_show_text(&fixture_items()).await;

    for (term, where_) in [
        ("key=customer", "实体层系统标识"),
        ("key=subscription", "第二个实体层系统标识"),
        ("customer_pk", "列映射的主键列名"),
        ("customer_id", "列映射的外键列名"),
        ("证据：", "证据栏"),
        ("t_customer.customer_no", "证据的表.列定位"),
        ("基数：1:N", "关系层基数"),
        ("两端：customer→subscription", "关系两端"),
        ("key=customer_has_subscription", "关系层系统标识"),
    ] {
        assert!(
            text.contains(term),
            "端到端 `review show` 输出里找不到 {term}（{where_}）\n实际输出：\n{text}"
        );
    }

    // 判据对这份投影有反应 —— payload 里的建模栏确实回来了。
    // ⚠️ REQ-057：判据收的是**投影**，问的是「字段有没有从 payload 回来」。
    let (view, _) = review_show_view(&fixture_items(), None).await;
    let audit = audit_fde_channel(&view);
    assert!(
        audit.premise_holds,
        "FDE 通道豁免前提不成立：hits={:?}",
        audit.hits
    );
    assert!(
        audit.hits.iter().any(|h| h.ends_with(".evidence")),
        "端到端路径下 evidence 应当来自 payload（hits 全空 = 存储层把字段丢了）：{:?}",
        audit.hits
    );
}

/// 【AC-003 · 端到端】**两轨渲染零违规** —— 判据② 走真库也是干净的。
#[tokio::test]
async fn test_review_show_e2e_track_separation_check_is_clean() {
    let (view, text) = review_show_view(&fixture_items(), None).await;
    let violations = verify_track_separation(&view, &text);
    assert!(violations.is_empty(), "端到端路径下两轨分界被判违规：{violations:#?}\n实际输出：\n{text}");
}

/// 【AC-002/F-D · 端到端】**payload 缺 `key` ⇒ 展示名不得冒充 key**。
///
/// 走真库这一层，是为了确认 `PgProposalStore::to_item_views` 不会在
/// 反序列化过程中**补回**一个 key（那会让纯逻辑那层的修复在生产路径上失效）。
#[tokio::test]
async fn test_review_show_e2e_missing_key_never_becomes_display_name() {
    let bare = vec![NewItem {
        kind: ItemKind::AddEntityType,
        target_key: "主键服务中心".into(),
        payload: json!({ "display_name": "主键服务中心" }),
    }];
    let (view, text) = review_show_view(&bare, None).await;

    assert!(
        !text.contains("key=主键服务中心"),
        "端到端路径下展示名被当成了 key\n实际输出：\n{text}"
    );
    assert_eq!(
        view.items[0].model.type_key, None,
        "端到端路径下 type_key 回退成了展示名：{:?}",
        view.items[0].model.type_key
    );
    // 且判据明确报出「没有建模数据」。
    assert!(
        !audit_fde_channel(&view).premise_holds,
        "建模栏一个都没打出来，前提却成立\n实际输出：\n{text}"
    );
}

/// 【AC-003 · 端到端】**两轨各占一行**，业务轨那一行不含任何建模栏。
#[tokio::test]
async fn test_review_show_e2e_two_tracks_stay_on_separate_lines() {
    let text = review_show_text(&fixture_items()).await;
    let biz: Vec<&str> = text.lines().filter(|l| l.contains("业务轨：")).collect();
    let model: Vec<&str> = text.lines().filter(|l| l.contains("建模轨：")).collect();

    assert_eq!(biz.len(), 3, "业务轨行数与条目数不符\n实际输出：\n{text}");
    assert_eq!(model.len(), 3, "建模轨行数与条目数不符\n实际输出：\n{text}");

    for line in &biz {
        for marker in ["key=", "证据", "基数", "两端", "←"] {
            assert!(
                !line.contains(marker),
                "端到端路径下业务轨那行混进了建模栏 {marker}\n该行：{line}"
            );
        }
    }
    for line in &model {
        assert!(line.contains("key="), "建模轨那行没有建模栏\n该行：{line}");
    }

    // 业务轨那一侧过守门器 —— 端到端也不许被污染。
    let blob = biz.join("\n");
    let leaked = scan_modeling_terms(&blob);
    assert!(leaked.is_empty(), "业务轨呈现泄露建模术语：{leaked:?}\n实际内容：\n{blob}");
}

/// 【展示用】**打印 `review show` 的真实输出**。
///
/// 本条不是判据，是**交付物**：`cargo test --test test_review_show_e2e -- --nocapture`
/// 会把 FDE 此刻真正看到的那段文本原样打出来。
#[tokio::test]
async fn test_review_show_e2e_print_real_output() {
    let text = review_show_text(&fixture_items()).await;
    println!("\n===== `review show` 真实输出（REQ-056 之后）=====\n{text}===== 结束 =====\n");

    // 打印之前已通过两轨裁决：**建模轨 pending ⇒ 整体未通过**。
    // 这是「裁决发生在 apply 之前」的直接体现：此刻 FDE 才刚看得见
    // `model=pending`，而 apply 的门禁要求两轨都 accepted。
    assert!(text.contains("整体通过：否"), "实际输出：\n{text}");
}

/// 【AC-001 · 反例】**空 payload 的条目不会伪造建模字段**。
///
/// 端到端读回来的是 `payload` 列的真实内容。若提案条目压根没带
/// `cardinality`，投影**不许**凭空造一个 —— FDE 看到「有基数」而库里没有，
/// 比看不到更危险。
#[tokio::test]
async fn test_review_show_e2e_no_fabricated_fields_when_payload_lacks_them() {
    let bare = vec![NewItem {
        kind: ItemKind::AddEntityType,
        target_key: "customer".into(),
        payload: json!({ "key": "customer", "display_name": "客户" }),
    }];
    let text = review_show_text(&bare).await;

    assert!(!text.contains("证据："), "无 evidence 却被渲染出证据栏\n实际输出：\n{text}");
    assert!(!text.contains("列映射："), "无 properties 却被渲染出列映射\n实际输出：\n{text}");
    assert!(!text.contains("基数："), "无 cardinality 却被渲染出基数\n实际输出：\n{text}");
    // 但 key 仍应带出来 —— payload 给了。
    assert!(text.contains("key=customer"), "实际输出：\n{text}");
}

/// 【状态流转】**两轨裁决的终态**反映在建模轨那一行上。
#[tokio::test]
async fn test_review_show_e2e_reflects_both_track_decisions() {
    let pool = test_pool().await;
    let projects = PgProjectStore::new(pool.clone());
    let proposals = PgProposalStore::new(pool.clone());
    let store = PgStore::new(pool.clone());
    projects.migrate().await.unwrap();
    let p = common::fresh_project(&projects, "review_show_decide").await;
    let v1 = projects.new_version(p.id).await.unwrap().id;
    store.save_snapshot(&Snapshot::new(v1, 1)).await.unwrap();

    let prop = proposals
        .create(
            p.id,
            NewProposal { base_version_id: v1, round: 1, extracted_by: None },
            &[ent("customer", "客户", "customer_pk", "主键列名与注释一致")],
        )
        .await
        .unwrap();

    async fn render(
        store: &PgStore,
        proposals: &PgProposalStore,
        pid: uuid::Uuid,
    ) -> String {
        let row = proposals.get(pid).await.unwrap().unwrap();
        let items = proposals.items(pid).await.unwrap();
        let views = PgProposalStore::to_item_views(&items);
        let base = store.get_snapshot(row.base_version_id).await.unwrap().unwrap();
        let baseline: std::collections::BTreeSet<String> =
            base.entity_types.iter().map(|e| e.key.clone()).collect();
        let st = summarize(&views, &baseline);
        render_review_text(&project_review_track(&views, &st))
    }

    // 未裁决：两轨都 pending。
    let t0 = render(&store, &proposals, prop.id).await;
    assert!(t0.contains("biz=pending") && t0.contains("model=pending"), "实际输出：\n{t0}");
    assert!(t0.contains("整体通过：否"), "实际输出：\n{t0}");

    // 两轨都接受 ⇒ 整体通过 —— **裁决之后**建模信息仍在输出里。
    for it in proposals.items(prop.id).await.unwrap() {
        proposals.set_decision(it.id, Track::Biz, Decision::Accepted).await.unwrap();
        proposals.set_decision(it.id, Track::Model, Decision::Accepted).await.unwrap();
    }
    let t1 = render(&store, &proposals, prop.id).await;
    assert!(t1.contains("biz=accepted") && t1.contains("model=accepted"), "实际输出：\n{t1}");
    assert!(t1.contains("整体通过：是"), "实际输出：\n{t1}");
    // ⚠️ 关键：判据状态变了，**建模字段不许跟着消失** ——
    // 那正是 REQ-055 投影做不到的事（它读的是快照，裁决时刻只有 base 版）。
    assert!(t1.contains("key=customer") && t1.contains("customer_pk"), "裁决后建模字段丢失\n实际输出：\n{t1}");
}
// ══════════════════════════════════════════════════════════
// REQ-060 · 提案级 `extracted_by`（走真库的 JSONB 往返）
// ══════════════════════════════════════════════════════════

/// 造一份**带归属**的 `ExtractedBy`（REQ-059 之后产物里的真实形状）。
fn declared_provenance() -> ontok::domain::snapshot::ExtractedBy {
    use ontok::domain::snapshot::{ExtractedBy, ProvenanceSource};
    ExtractedBy {
        agent: "mcode-agent".into(),
        version: "2026.10.07".into(),
        round: 1,
        failed_groups: vec![],
        source: Some(ProvenanceSource::ToolProduced),
    }
}

/// 【AC-001 · 端到端】**真库往返后 `extracted_by` 仍在输出里**。
///
/// 走真库这一层，是为了确认 `ProposalRow.extracted_by` 的 JSONB 往返
/// **不会**把归属弄丢 —— 丢了的话，纯逻辑那层的修复在生产路径上等于没做。
#[tokio::test]
async fn test_review_show_e2e_provenance_survives_jsonb_roundtrip() {
    let eb = declared_provenance();
    let (view, text) = review_show_view(&fixture_items(), Some(&eb)).await;

    assert!(
        text.contains("agent=mcode-agent"),
        "端到端路径下提案级归属丢失（JSONB 往返把它弄没了）\n实际输出：\n{text}"
    );
    assert!(
        text.contains("version=2026.10.07"),
        "端到端路径下版本丢失\n实际输出：\n{text}"
    );
    assert!(
        text.contains("提案级抽取归属"),
        "端到端路径下没有标明这是提案级\n实际输出：\n{text}"
    );
    // 归属**恰好一次** —— 不是逐条目。
    let n = text.lines().filter(|l| l.contains("提案级抽取归属：")).count();
    assert_eq!(n, 1, "端到端路径下归属行数 = {n}（应恰好 1 行）\n实际输出：\n{text}");
    assert!(
        verify_provenance_scope(&view, &text).is_empty(),
        "端到端路径下归属判据报违规：{:?}\n实际输出：\n{text}",
        verify_provenance_scope(&view, &text)
    );
}

/// 【AC-001 · 端到端】**SQL NULL ⇒ 人工修订**，不是「未提供」。
#[tokio::test]
async fn test_review_show_e2e_null_provenance_reads_as_human_revised() {
    let (view, text) = review_show_view(&fixture_items(), None).await;

    assert!(
        text.contains("人工修订"),
        "端到端路径下 SQL NULL 没有显示为「人工修订」\n实际输出：\n{text}"
    );
    // ⚠️ 与「未提供」必须分开：前者是「有人审过」，后者是「我不知道」。
    assert!(
        !text.contains("未提供"),
        "端到端路径下把「人工修订」说成了「未提供」（丢掉了 ADR-001 的信息）\n实际输出：\n{text}"
    );
    assert_eq!(
        view.provenance.state,
        ontok::domain::review_view::ProvenanceState::HumanRevised
    );
}

/// 【AC-004 · 端到端】**`premise_holds` 不因为显示归属而变真**。
///
/// 判据：一份**连 key 都没有**的提案，`sourced_fields()` 必然全空。
/// 有无归属两种输入下 `premise_holds` 与 `hits` 必须**逐 bit 相同**。
#[tokio::test]
async fn test_review_show_e2e_premise_holds_unaffected_by_provenance() {
    let bare = vec![NewItem {
        kind: ItemKind::AddEntityType,
        target_key: "裸条目".into(),
        payload: json!({ "display_name": "裸条目" }),
    }];

    let (v_none, t_none) = review_show_view(&bare, None).await;
    let eb = declared_provenance();
    let (v_some, t_some) = review_show_view(&bare, Some(&eb)).await;

    // 无归属时前提必须不成立 —— 这是对照组。
    assert!(
        !audit_fde_channel(&v_none).premise_holds,
        "基线不对：建模栏全空时 premise_holds 应当为 false，hits={:?}",
        audit_fde_channel(&v_none).hits
    );
    // ⭐ 有归属时必须**仍然**不成立，且 hits 一模一样。
    let a_none = audit_fde_channel(&v_none);
    let a_some = audit_fde_channel(&v_some);
    assert_eq!(
        a_none.premise_holds, a_some.premise_holds,
        "premise_holds 因为显示归属而变化 ⇒ REQ-057 的解耦被破坏\n无归属：{a_none:?}\n有归属：{a_some:?}"
    );
    assert_eq!(
        a_none.hits, a_some.hits,
        "hits 因为显示归属而变化 ⇒ 判据与渲染产物耦合了"
    );
    // 输出里两处都确实打出了归属（证明输入真的不同，不是没生效）。
    assert!(t_none.contains("提案级抽取归属"), "t_none 没打归属\n{t_none}");
    assert!(t_some.contains("agent=mcode-agent"), "t_some 没打归属\n{t_some}");
}

/// 【AC-004 · 端到端】**两轨分界 + 归属判据在真库路径上都是干净的**。
#[tokio::test]
async fn test_review_show_e2e_judgements_clean_with_provenance() {
    let eb = declared_provenance();
    let (view, text) = review_show_view(&fixture_items(), Some(&eb)).await;
    assert!(
        verify_track_separation(&view, &text).is_empty(),
        "端到端路径下两轨分界退化：{:?}\n实际输出：\n{text}",
        verify_track_separation(&view, &text)
    );
    assert!(
        verify_provenance_scope(&view, &text).is_empty(),
        "端到端路径下归属判据报违规：{:?}",
        verify_provenance_scope(&view, &text)
    );
    // 业务轨那侧仍然过建模术语守门。
    let biz: Vec<&str> = text.lines().filter(|l| l.contains("业务轨：")).collect();
    let blob = biz.join("\n");
    assert!(
        scan_modeling_terms(&blob).is_empty(),
        "业务轨侧泄露建模术语：{:?}\n实际内容：\n{blob}",
        scan_modeling_terms(&blob)
    );
}

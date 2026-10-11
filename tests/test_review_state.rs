//! Task 6 · 双轨审核状态机测试
//!
//! 验收 AC-007。测试名带 `test_review_` 前缀，便于过滤。

use ontok::domain::review::{
    apply_decision, entry_state, summarize, veto_count, Decision, EntryState, ItemKind, ItemView,
    ProposalStatus, Track,
};
use std::collections::BTreeSet;

fn item(kind: ItemKind, target: &str, biz: Decision, model: Decision) -> ItemView {
    let (id, from, to) = match kind {
        ItemKind::AddLinkType => {
            let (f, t) = target.split_once("→").unwrap_or(("A", "B"));
            (
                format!("add_link:{target}"),
                Some(f.to_string()),
                Some(t.to_string()),
            )
        }
        _ => (format!("{}:{target}", kind.as_str()), None, None),
    };
    ItemView {
        id,
        kind,
        target: target.to_string(),
        from_type: from,
        to_type: to,
        biz_status: biz,
        model_status: model,
        // 审核状态机**不读** payload 的业务字段（只看 kind / target / 两端 / 双轨状态），
        // 但 `type_key()` 会读 payload.key —— 真实数据里实体条目的
        // target 是**展示名**、payload.key 是**系统 key**，两者不同。
        // 这里刻意不填，让「payload 没给 key 时退回 target」这条路径
        // 成为默认值；需要区分命名空间的测试自己显式设置 payload。
        payload: serde_json::Value::Null,
    }
}

/// 造一个「target 是展示名、payload.key 是系统 key」的实体条目。
/// **这才是真实数据的样子** —— 早期测试都用 target 当 key，
/// 于是命名空间混了也测不出来。
fn entity_item_with_key(target: &str, key: &str, biz: Decision, model: Decision) -> ItemView {
    let mut it = item(ItemKind::AddEntityType, target, biz, model);
    it.payload = serde_json::json!({ "key": key, "display_name": target });
    it
}

fn empty_baseline() -> BTreeSet<String> {
    BTreeSet::new()
}

fn baseline(keys: &[&str]) -> BTreeSet<String> {
    keys.iter().map(|s| s.to_string()).collect()
}

// ── 单条目状态 ────────────────────────────────────────────

#[test]
fn test_review_both_tracks_accepted_is_passed() {
    let it = item(ItemKind::AddEntityType, "客户", Decision::Accepted, Decision::Accepted);
    assert_eq!(entry_state(&it), EntryState::Passed);
}

#[test]
fn test_review_biz_reject_vetoes_regardless_of_model() {
    let it = item(ItemKind::AddEntityType, "客户", Decision::Rejected, Decision::Accepted);
    assert_eq!(entry_state(&it), EntryState::Vetoed, "业务轨拒绝必须否决");
}

#[test]
fn test_review_model_reject_vetoes_regardless_of_biz() {
    let it = item(ItemKind::AddEntityType, "客户", Decision::Accepted, Decision::Rejected);
    assert_eq!(entry_state(&it), EntryState::Vetoed, "建模轨拒绝必须否决");
}

#[test]
fn test_review_biz_pending_is_not_passed() {
    let it = item(ItemKind::AddEntityType, "客户", Decision::Pending, Decision::Accepted);
    assert_eq!(entry_state(&it), EntryState::Pending, "业务轨未决不算通过");
}

#[test]
fn test_review_model_pending_is_not_passed() {
    // 规格 §8.2：代签是「谁来写这个决定」，**不是「可以不作决定」**。
    // 建模轨必须「完整执行」，所以 Pending 不算通过。
    let it = item(ItemKind::AddEntityType, "客户", Decision::Accepted, Decision::Pending);
    assert_eq!(entry_state(&it), EntryState::Pending, "建模轨未决不得视为通过");
}

#[test]
fn test_review_both_pending_is_pending() {
    let it = item(ItemKind::AddEntityType, "客户", Decision::Pending, Decision::Pending);
    assert_eq!(entry_state(&it), EntryState::Pending);
}

// ── 关系类依赖两端 ────────────────────────────────────────

#[test]
fn test_review_link_acceptance_requires_both_ends_accepted() {
    let items = vec![
        item(ItemKind::AddEntityType, "客户", Decision::Accepted, Decision::Accepted),
        item(ItemKind::AddEntityType, "订单", Decision::Pending, Decision::Accepted),
        item(ItemKind::AddLinkType, "客户→订单", Decision::Accepted, Decision::Accepted),
    ];
    let st = summarize(&items, &empty_baseline());

    // 客户 双轨通过 → 可用；订单 业务轨未决 → 不可用
    // 所以这条关系必须被拦
    assert_eq!(st.applicable, vec!["add_entity_type:客户"]);
    assert_eq!(st.dangling_relations.len(), 1, "关系应被识别为悬空");
    let b = st.blocked.first().expect("应有一条 blocked");
    assert!(b.target == "客户→订单");
    assert!(
        b.reason.as_deref().unwrap_or("").contains("订单"),
        "原因应点名缺失的一端，实际：{:?}",
        b.reason
    );
}

#[test]
fn test_review_link_ok_when_both_ends_in_baseline() {
    let items = vec![item(
        ItemKind::AddLinkType,
        "客户→订单",
        Decision::Accepted,
        Decision::Accepted,
    )];
    let st = summarize(&items, &baseline(&["客户", "订单"]));
    assert_eq!(st.applicable.len(), 1);
    assert!(st.blocked.is_empty());
}

#[test]
fn test_review_link_ok_when_both_ends_added_in_same_proposal() {
    let items = vec![
        item(ItemKind::AddEntityType, "客户", Decision::Accepted, Decision::Accepted),
        item(ItemKind::AddEntityType, "订单", Decision::Accepted, Decision::Accepted),
        item(ItemKind::AddLinkType, "客户→订单", Decision::Accepted, Decision::Accepted),
    ];
    let st = summarize(&items, &empty_baseline());
    // 本提案内新增的两个类型都通过了 → 关系可用
    assert_eq!(st.applicable.len(), 3);
    assert!(st.dangling_relations.is_empty());
}

#[test]
fn test_review_link_missing_both_ends() {
    let items = vec![item(
        ItemKind::AddLinkType,
        "不存在A→不存在B",
        Decision::Accepted,
        Decision::Accepted,
    )];
    let st = summarize(&items, &empty_baseline());
    assert!(st.applicable.is_empty());
    assert_eq!(st.dangling_relations.len(), 1);
}

// ── 整体状态 ──────────────────────────────────────────────

#[test]
fn test_review_two_tracks_judge_independently() {
    // 业务轨通过、建模轨拒绝 → 整体不通过
    let items = vec![item(
        ItemKind::AddEntityType,
        "客户",
        Decision::Accepted,
        Decision::Rejected,
    )];
    let st = summarize(&items, &empty_baseline());
    assert!(!st.passed, "建模轨拒绝不能被业务轨通过覆盖");
    assert!(st.applicable.is_empty());
    assert_eq!(veto_count(&items), 1);
}

#[test]
fn test_review_pending_item_blocks_overall_pass() {
    let items = vec![
        item(ItemKind::AddEntityType, "客户", Decision::Accepted, Decision::Accepted),
        item(ItemKind::AddEntityType, "订单", Decision::Pending, Decision::Pending),
    ];
    let st = summarize(&items, &empty_baseline());
    assert!(!st.passed, "有条目未决时整体不应通过");
    // 但已通过的那条仍可 apply
    assert_eq!(st.applicable, vec!["add_entity_type:客户"]);
}

#[test]
fn test_review_empty_proposal_is_not_passed() {
    let st = summarize(&[], &empty_baseline());
    assert!(!st.passed, "空提案不应通过");
    assert!(st.applicable.is_empty());
}

#[test]
fn test_review_verdicts_cover_every_item() {
    let items = vec![
        item(ItemKind::AddEntityType, "客户", Decision::Accepted, Decision::Accepted),
        item(ItemKind::AddEntityType, "订单", Decision::Accepted, Decision::Rejected),
    ];
    let st: ProposalStatus = summarize(&items, &empty_baseline());
    assert_eq!(st.verdicts.len(), items.len(), "每个条目都要有判定");
    assert_eq!(st.verdicts[1].state, EntryState::Vetoed);
    assert!(st.verdicts[1].reason.is_some());
}

// ── B1 · `passed` 的承重条件：blocked 非空 → passed 必须是 false ──────────
//
// 背景：apply 的**唯一**审核闸门是 `st.passed`（`src/domain/apply.rs:55`）。
// 独立审核查出 `passed` 表达式里那一行 `&& blocked.is_empty()`
// （`src/domain/review.rs:281`）**零测试覆盖**：
// 原有的 `test_review_link_acceptance_requires_both_ends_accepted`
// 只断言 blocked 列表里有什么，**从不断言 `passed` 的值**。

#[test]
fn test_review_blocked_item_forces_passed_false() {
    // 刻意构造得**只有** blocked/dangling 守卫能拦下它：
    //   · applicable 非空（否则被 `!applicable.is_empty()` 拦）
    //   · 每一条 entry_state 都是 Passed（否则被 `all(Passed)` 拦）
    //   · items 非空
    // 「订单」既不在基线、本提案里也没有任何条目创建它 → 关系两端不齐。
    let items = vec![
        item(ItemKind::AddEntityType, "客户", Decision::Accepted, Decision::Accepted),
        item(ItemKind::AddLinkType, "客户→订单", Decision::Accepted, Decision::Accepted),
    ];
    let st = summarize(&items, &empty_baseline());

    // 前提先钉死，否则「passed=false」可能只是被别的条件拦下的
    assert_eq!(st.applicable, vec!["add_entity_type:客户"], "前提：确实有可应用条目");
    assert!(
        items.iter().all(|it| entry_state(it) == EntryState::Passed),
        "前提：每一条都通过双轨"
    );
    assert_eq!(st.blocked.len(), 1, "前提：确实产生了一条 blocked");

    assert!(
        !st.passed,
        "blocked 非空时 passed 必须为 false —— passed 的契约是「可进入 apply」（AC-007）"
    );
}

#[test]
fn test_review_blocked_and_dangling_cover_the_same_items() {
    // **为什么要有这条测试**：它把一个当前成立的结构事实钉住 ——
    // `blocked` 与 `dangling_relations` 恒等（`src/domain/review.rs:252-253`
    // 在同一个 else 分支里一起 push，push 的还是同一个 id）。
    //
    // 后果必须说清楚：**正因为恒等，单独删掉 `&& blocked.is_empty()`
    // 在运行时是语义空操作** —— 没有任何**运行时**断言能证伪它
    // （实测：只删这一行，上面那些断言 `passed` 的测试仍然全绿）。
    // 让它变红的是下一条的源码守卫。
    // 本条的真正价值在「将来」：一旦有人给 blocked 加**第二个生产者**
    // （比如层级不合法的条目也进 blocked），这条测试立刻变红，
    // 提醒他 `passed` 的守卫仍然是承重的、不能顺手删。
    let items = vec![
        item(ItemKind::AddEntityType, "客户", Decision::Accepted, Decision::Accepted),
        item(ItemKind::AddLinkType, "客户→订单", Decision::Accepted, Decision::Accepted),
        item(ItemKind::AddLinkType, "资产→不存在的类型", Decision::Accepted, Decision::Accepted),
    ];
    let st = summarize(&items, &empty_baseline());

    let mut blocked_ids: Vec<_> = st.blocked.iter().map(|b| b.id.clone()).collect();
    let mut dangling_ids = st.dangling_relations.clone();
    blocked_ids.sort();
    dangling_ids.sort();
    assert_eq!(
        blocked_ids, dangling_ids,
        "blocked 与 dangling_relations 必须覆盖同一批条目（当前实现里它们在同一个分支 push）"
    );
    assert!(!blocked_ids.is_empty(), "前提：本例确实产生了 blocked");
}

#[test]
fn test_review_passed_expression_keeps_both_guard_clauses() {
    // **为什么这条是「读源码」的断言**：上一条测试说明了 ——
    // `blocked` 与 `dangling_relations` 恒等，所以**运行时无法**证伪
    // 「把 `&& blocked.is_empty()` 从 `passed` 里删掉」这个变异：
    // 无论什么输入，删掉它前后 `passed` 的值都一模一样。
    // 审核要求「删掉这行必须让 `test_review_state` 变红」，
    // 而能做到的唯一办法就是把守卫本身钉在源码上。
    //
    // 本仓库已有同类约定：`test_no_write_path.rs` 用 `include_str!`
    // 断言「不存在写回入口」这类结构性不变量。这里沿用，
    // 并把判据限定在 `passed` 表达式本身，不牵连别处。
    let src = include_str!("../src/domain/review.rs");
    let start = src.find("let passed =").expect("review.rs 里应有 `let passed =`");
    let end_rel = src[start..]
        .find("ProposalStatus {")
        .expect("`passed` 表达式应以 `ProposalStatus {` 收尾");
    let expr = &src[start..start + end_rel];

    for guard in ["dangling_relations.is_empty()", "blocked.is_empty()"] {
        assert!(
            expr.contains(guard),
            "`passed` 表达式里必须有 `{guard}` —— 它是阻止「悬空/被拦的提案\n\
             被 apply 写进不可变快照」的一道闸门，删掉不得有任何测试变红。\n\
             实际表达式：\n{expr}"
        );
    }
    assert!(
        expr.contains("items.iter().all("),
        "`passed` 表达式里必须有「所有条目都通过」这一条，实际：\n{expr}"
    );
}

// ── 判定写入 ──────────────────────────────────────────────

#[test]
fn test_review_apply_decision_is_immutable() {
    let it = item(ItemKind::AddEntityType, "客户", Decision::Pending, Decision::Pending);
    let next = apply_decision(&it, Track::Biz, Decision::Accepted);
    assert_eq!(next.biz_status, Decision::Accepted);
    assert_eq!(next.model_status, Decision::Pending, "另一轨不受影响");
    assert_eq!(it.biz_status, Decision::Pending, "原条目不可变");
}

#[test]
fn test_review_apply_decision_model_track() {
    let it = item(ItemKind::AddEntityType, "客户", Decision::Accepted, Decision::Pending);
    let next = apply_decision(&it, Track::Model, Decision::Rejected);
    assert_eq!(next.model_status, Decision::Rejected);
    assert_eq!(entry_state(&next), EntryState::Vetoed);
}

// ── B-1 · 删除目标不得成为野指针 ──────────────────────────

#[test]
fn test_review_removed_type_is_not_available_to_new_link() {
    // **B-1 反例**：基线里有「订单」，本提案把它删了，
    // 同时新建一条指向它的关系 → 早期版本 available 只做加法不扣减，
    // 这条关系被判为可用，apply 之后留下指向不存在实体的野指针。
    let items = vec![
        item(ItemKind::RemoveEntityType, "订单", Decision::Accepted, Decision::Accepted),
        item(ItemKind::AddLinkType, "客户→订单", Decision::Accepted, Decision::Accepted),
    ];
    let st = summarize(&items, &baseline(&["客户", "订单"]));

    assert!(
        !st.applicable.contains(&"add_link:客户→订单".to_string()),
        "指向本提案内被删类型的关系不得判为可用：{:?}",
        st.applicable
    );
    assert_eq!(st.dangling_relations.len(), 1, "应识别为悬空");
    assert!(!st.passed, "存在悬空关系时整体不得通过");
}

#[test]
fn test_review_removed_type_still_blocks_link_to_it() {
    // 顺序无关：无论删除条目排在关系前面还是后面，都得被拦
    let items = vec![
        item(ItemKind::AddLinkType, "客户→订单", Decision::Accepted, Decision::Accepted),
        item(ItemKind::RemoveEntityType, "订单", Decision::Accepted, Decision::Accepted),
    ];
    let st = summarize(&items, &baseline(&["客户", "订单"]));
    assert_eq!(st.dangling_relations.len(), 1, "删除与关系同提案时仍须拦");
    assert!(!st.passed);
}

#[test]
fn test_review_link_to_surviving_type_is_fine() {
    // 反例的对偶：只删「订单」，关系指向「客户」时不该被误拦
    let items = vec![
        item(ItemKind::RemoveEntityType, "订单", Decision::Accepted, Decision::Accepted),
        item(ItemKind::AddLinkType, "客户→设备", Decision::Accepted, Decision::Accepted),
    ];
    let st = summarize(&items, &baseline(&["客户", "订单", "设备"]));
    assert!(
        st.applicable.contains(&"add_link:客户→设备".to_string()),
        "指向存活类型的关系应放行：{:?}",
        st.applicable
    );
    assert!(st.dangling_relations.is_empty());
}

#[test]
fn test_review_pending_removal_does_not_block_link() {
    // 删除条目未决 → 类型还在，关系不该被拦
    let items = vec![
        item(ItemKind::RemoveEntityType, "订单", Decision::Pending, Decision::Pending),
        item(ItemKind::AddLinkType, "客户→订单", Decision::Accepted, Decision::Accepted),
    ];
    let st = summarize(&items, &baseline(&["客户", "订单"]));
    assert!(st.dangling_relations.is_empty(), "未决的删除不产生野指针");
}

// ── B-2 · 悬空关系时不得自报通过 ──────────────────────────
#[test]
fn test_review_dangling_relation_forces_not_passed() {
    // **B-2 反例**：1 条可用 + 1 条悬空 → 早期版本只看「有可应用条目」
    // 就自报 passed=true，调用方以它为闸门会在悬空状态下执行 apply。
    let items = vec![
        item(ItemKind::AddEntityType, "客户", Decision::Accepted, Decision::Accepted),
        item(ItemKind::AddLinkType, "客户→订单", Decision::Accepted, Decision::Accepted),
    ];
    let st = summarize(&items, &empty_baseline());

    assert_eq!(st.applicable.len(), 1, "确实有一条可用");
    assert_eq!(st.dangling_relations.len(), 1, "确实有一条悬空");
    assert!(
        !st.passed,
        "有悬空关系时 passed 必须为 false（passed 的契约是「可进入 apply」）"
    );
}

#[test]
fn test_review_all_passed_with_no_dangling_is_passed() {
    // 对偶：全部通过且无悬空 → passed=true（别把闸门关死）
    let items = vec![
        item(ItemKind::AddEntityType, "客户", Decision::Accepted, Decision::Accepted),
        item(ItemKind::AddEntityType, "订单", Decision::Accepted, Decision::Accepted),
        item(ItemKind::AddLinkType, "客户→订单", Decision::Accepted, Decision::Accepted),
    ];
    let st = summarize(&items, &empty_baseline());
    assert!(st.passed, "无悬空且全通过时必须放行：{:?}", st);
}

// ── 命名空间：target 是展示名，key 是系统标识 ─────────────

#[test]
fn test_review_available_uses_type_key_not_display_target() {
    // **端到端跑出来才发现的 bug**：条目 target 是展示名「客户」，
    // 而关系两端从 payload 取的是系统 key `customer`。
    // 早期版本把 target 塞进 available 集合 → 集合里是「客户」、
    // 查的时候用 `customer` → 关系全被判成悬空，apply 永远做不出关系。
    //
    // **之前的单测测不出来**：它们的 target 与 payload.key 恰好是同一个词，
    // 两个命名空间混了也看不出。真正的输入（fixture）里它们是「客户」vs `customer`。
    // 这条测试**故意让两者不同**。
    let ent_item = entity_item_with_key("客户", "customer", Decision::Accepted, Decision::Accepted);
    let sub_item = entity_item_with_key("订阅", "subscription", Decision::Accepted, Decision::Accepted);

    // 关系类的两端来自 payload，是**系统 key**（真实数据就是这样）
    let mut link_item = item(ItemKind::AddLinkType, "客户→订阅", Decision::Accepted, Decision::Accepted);
    link_item.from_type = Some("customer".into());
    link_item.to_type = Some("subscription".into());
    link_item.target = "客户—订阅".into();
    link_item.payload = serde_json::json!({
        "key": "customer_has_subscription",
        "from_type": "customer", "to_type": "subscription"
    });

    let st = summarize(&[ent_item, sub_item, link_item], &empty_baseline());

    assert_eq!(
        st.applicable.len(),
        3,
        "两端都被接受时，关系不该被判成悬空：{:?}",
        st
    );
    assert!(
        st.dangling_relations.is_empty(),
        "available 必须用 key 命名空间，实际悬空：{:?}",
        st.dangling_relations
    );
    assert!(st.passed, "全部通过且无悬空时 passed 应为真：{:?}", st);
}

#[test]
fn test_review_type_key_falls_back_to_target_when_payload_has_no_key() {
    // payload 没给 key 时退回 target —— 不能返回空串
    let it = item(ItemKind::AddEntityType, "客户", Decision::Pending, Decision::Pending);
    assert_eq!(it.type_key(), "客户");

    let mut with_key = it.clone();
    with_key.payload = serde_json::json!({ "key": "customer" });
    assert_eq!(with_key.type_key(), "customer", "payload 有 key 时优先用它");
}

#[test]
fn test_review_remove_uses_type_key_namespace() {
    // 删除也走 key 命名空间：target 是「订单」展示名，payload.key 是 `t064`
    let mut del = entity_item_with_key("订单", "t064", Decision::Accepted, Decision::Accepted);
    del.kind = ItemKind::RemoveEntityType;
    del.id = "remove_entity_type:订单".into();

    let mut link_item = item(ItemKind::AddLinkType, "客户→订单", Decision::Accepted, Decision::Accepted);
    link_item.from_type = Some("customer".into());
    link_item.to_type = Some("t064".into());
    link_item.target = "客户—订单".into();
    link_item.payload = serde_json::json!({ "from_type": "customer", "to_type": "t064" });

    // 基线里是 key `t064`（英文）
    let st = summarize(&[del, link_item], &baseline(&["customer", "t064"]));
    assert!(
        !st.dangling_relations.is_empty(),
        "删掉 t064 后，指向它的关系必须被拦：{:?}",
        st
    );
    assert!(!st.passed);
}

//! Task 11 · 层级纯函数测试
//!
//! 验收 AC-009。测试名带 `test_hierarchy` 前缀，
//! 供验收命令 `cargo test test_hierarchy` 匹配。

use ontok::domain::hierarchy::{
    ancestors_of, check_reparent, descendants_of, validate_hierarchy, would_create_cycle, Parents,
};
use std::collections::BTreeMap;

fn p(pairs: &[(&str, Option<&str>)]) -> Parents {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.map(|s| s.to_string())))
        .collect()
}

// ── 祖先 ────────────────────────────────────────────────

#[test]
fn test_hierarchy_ancestors_nearest_first() {
    let t = p(&[("资产", None), ("设备", Some("资产")), ("机器人设备", Some("设备"))]);
    assert_eq!(ancestors_of("机器人设备", &t), vec!["设备", "资产"]);
}

#[test]
fn test_hierarchy_ancestors_exclude_self() {
    let t = p(&[("资产", None), ("设备", Some("资产"))]);
    // 祖先不含自身
    assert_eq!(ancestors_of("设备", &t), vec!["资产"]);
    assert!(ancestors_of("资产", &t).is_empty());
}

#[test]
fn test_hierarchy_ancestors_stop_on_cycle() {
    // 防御：环存在时不得死循环
    let t = p(&[("A", Some("B")), ("B", Some("A"))]);
    let r = ancestors_of("A", &t);
    assert_eq!(r, vec!["B"]);
}

#[test]
fn test_hierarchy_ancestors_stop_on_dangling() {
    let t = p(&[("设备", Some("不存在"))]);
    assert!(ancestors_of("设备", &t).is_empty());
}

// ── 后代 ────────────────────────────────────────────────

#[test]
fn test_hierarchy_descendants_walk_down() {
    let t = p(&[
        ("资产", None),
        ("设备", Some("资产")),
        ("机器人设备", Some("设备")),
        ("手工设备", Some("设备")),
    ]);
    let mut d = descendants_of("设备", &t);
    d.sort();
    // 注意：Rust 的 String 排序按 UTF-8 码位，不是拼音 ——
    // 手(0x624B) < 机(0x673A)，所以排序后「手工」在「机器人」前面。
    assert_eq!(d, vec!["手工设备", "机器人设备"]);
}

#[test]
fn test_hierarchy_descendants_exclude_self() {
    // **这才是能咬住实现的形状**：取一个**确实有后代**的节点，
    // 断言结果里没有自己。
    //
    // 早期版本用的是 `设备`（它恰好没有子节点），
    // 于是「排除自身」这个断言**无论实现对错都通过** —— 假命题测试。
    // 「有后代」+「不含自己」才有判别力。
    let t = p(&[("资产", None), ("设备", Some("资产")), ("机器人设备", Some("设备"))]);
    let mut d = descendants_of("资产", &t);
    d.sort();
    // UTF-8 码位排序：机(0x673A) < 设(0x8BBE)，所以「机器人设备」在前
    assert_eq!(d, vec!["机器人设备", "设备"]);
    assert!(
        !d.contains(&"资产".to_string()),
        "有后代的节点，其后代列表里仍不该出现自己：{d:?}"
    );

    // 叶子节点确实没有后代（这条是补充，不是主断言）
    let t2 = p(&[("资产", None), ("设备", Some("资产"))]);
    assert!(descendants_of("设备", &t2).is_empty());
}

#[test]
fn test_hierarchy_descendants_exclude_self_under_cycle() {
    // **G-2 反例**：环存在时也不得把自己算成后代。
    // 早期实现只用 seen 挡环、没预置自身 → 自环返回 ["A"]、互环返回 ["B","A"]。
    let self_cycle: Parents = BTreeMap::from([("A".to_string(), Some("A".to_string()))]);
    let d = descendants_of("A", &self_cycle);
    assert!(
        !d.contains(&"A".to_string()),
        "自环时不得把自己算成后代：{d:?}"
    );

    let mutual: Parents = BTreeMap::from([
        ("A".to_string(), Some("B".to_string())),
        ("B".to_string(), Some("A".to_string())),
    ]);
    let d = descendants_of("A", &mutual);
    assert!(
        !d.contains(&"A".to_string()),
        "互环时不得把自己算成后代：{d:?}"
    );
}

#[test]
fn test_hierarchy_descendants_deep_chain() {
    let t = p(&[
        ("A", None),
        ("B", Some("A")),
        ("C", Some("B")),
        ("D", Some("C")),
    ]);
    let mut d = descendants_of("A", &t);
    d.sort();
    assert_eq!(d, vec!["B", "C", "D"]);
}

// ── 环检测 ──────────────────────────────────────────────

#[test]
fn test_hierarchy_detects_cycle() {
    let t = p(&[("A", Some("B")), ("B", Some("C")), ("C", Some("A"))]);
    let problems = validate_hierarchy(&t);
    assert!(problems.iter().any(|x| x.starts_with("cycle")), "环未被检出：{problems:?}");
}

#[test]
fn test_hierarchy_detects_self_cycle() {
    let t = p(&[("A", Some("A"))]);
    assert!(validate_hierarchy(&t).iter().any(|x| x.starts_with("cycle")));
}

#[test]
fn test_hierarchy_accepts_valid_tree() {
    let t = p(&[("资产", None), ("设备", Some("资产")), ("机器人设备", Some("设备"))]);
    assert!(validate_hierarchy(&t).is_empty(), "合法树不应有问题：{:?}", validate_hierarchy(&t));
}

#[test]
fn test_hierarchy_detects_dangling_parent() {
    let t = p(&[("设备", Some("不存在的类型"))]);
    let problems = validate_hierarchy(&t);
    assert!(
        problems.iter().any(|x| x.starts_with("dangling_parent")),
        "悬空父未被检出：{problems:?}"
    );
}

// ── 写入前校验 ──────────────────────────────────────────

#[test]
fn test_hierarchy_would_create_cycle_on_self() {
    let t = p(&[("A", None)]);
    assert!(would_create_cycle(&t, "A", Some("A")), "自环必须被拦");
}

#[test]
fn test_hierarchy_would_create_cycle_on_descendant() {
    // A 是 B 的祖先；把 A 挂到 B 下 → 成环
    let t: Parents = BTreeMap::from([
        ("资产".to_string(), None),
        ("设备".to_string(), Some("资产".to_string())),
    ]);
    assert!(would_create_cycle(&t, "资产", Some("设备")), "挂到后代下必须被拦");
}

#[test]
fn test_hierarchy_would_not_cycle_on_normal_reparent() {
    let t: Parents = BTreeMap::from([
        ("资产".to_string(), None),
        ("设备".to_string(), Some("资产".to_string())),
        ("其他".to_string(), None),
    ]);
    assert!(!would_create_cycle(&t, "设备", Some("其他")), "正常改挂父不该报环");
    assert!(!would_create_cycle(&t, "设备", None), "摘掉父不该报环");
}

#[test]
fn test_hierarchy_check_reparent_messages() {
    let t: Parents = BTreeMap::from([
        ("资产".to_string(), None),
        ("设备".to_string(), Some("资产".to_string())),
    ]);
    // 正常
    assert!(check_reparent(&t, "设备", Some("资产")).is_ok());
    // 成环
    let e = check_reparent(&t, "资产", Some("设备")).unwrap_err();
    assert!(e.starts_with("cycle"), "环的报错信息不对：{e}");
    // 悬空
    let e = check_reparent(&t, "设备", Some("不存在")).unwrap_err();
    assert!(e.starts_with("dangling_parent"), "悬空的报错信息不对：{e}");
}

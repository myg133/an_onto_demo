//! 类层级的纯函数：祖先 / 后代 / 环检测 / 悬空父引用。
//!
//! **本模块零 I/O**（验收 AC-002）。输入是 `{key: 父key 或 None}`，
//! 输出是普通列表。所有函数对**已存在环**做防御，绝不死循环。
//!
//! 行为参照：REQ-001 spike 的 `spike/synthesize.py` 同名逻辑（已实测）。

use std::collections::{BTreeMap, BTreeSet};

/// key -> 父类型 key（`None` 表示根）
pub type Parents = BTreeMap<String, Option<String>>;

/// 从 `key` 向上走到根，**不含自身**，离自己最近的祖先在前。
///
/// 环存在时停止（防御），不会无限循环。
pub fn ancestors_of(key: &str, parents: &Parents) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    seen.insert(key);
    let mut cur = parents.get(key).and_then(|p| p.as_deref());

    // 最多走 parents.len() 步 —— 环一定会在步数内被 seen 拦住
    for _ in 0..parents.len() {
        match cur {
            None => break,
            Some(p) if !parents.contains_key(p) => break, // 悬空父：停下，问题由 validate 报
            Some(p) if !seen.insert(p) => break,          // 环：停下
            Some(p) => {
                out.push(p.to_string());
                cur = parents.get(p).and_then(|x| x.as_deref());
            }
        }
    }
    out
}

/// `key` 的所有后代（子孙），**不含自身**。
///
/// 环存在时依然不含自身：开栈前先把 `key` 预置进 `seen`。
/// 早期版本只用 `seen` 挡环、没预置自身 → 自环时返回 `["A"]`、
/// 互环时返回 `["B", "A"]`，**把自己算成自己的后代**。
/// 文档承诺的「不含自身」在环下不成立，而当时的测试用的是无环树
/// （`设备` 恰好没有子节点），无论实现对错都通过 —— 典型的假命题测试。
pub fn descendants_of(key: &str, parents: &Parents) -> Vec<String> {
    let mut children: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (k, p) in parents {
        if let Some(p) = p.as_deref() {
            children.entry(p).or_default().push(k.as_str());
        }
    }

    let mut out = Vec::new();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    seen.insert(key); // 自身永远不算后代
    let mut stack: Vec<&str> = children.get(key).cloned().unwrap_or_default();

    while let Some(cur) = stack.pop() {
        if !seen.insert(cur) {
            continue; // 环 / 已访问
        }
        out.push(cur.to_string());
        if let Some(kids) = children.get(cur) {
            stack.extend(kids.iter().copied());
        }
    }
    out
}

/// 从 `name` 向上走，若路径上出现重复节点则返回该环的完整路径。
///
/// **与 `ancestors_of` 的区别**：`ancestors_of` 遇环会「停下并返回已走过的部分」，
/// 它是**查询**语义（我要祖先）；本函数是**检测**语义（我要找出环在哪）。
/// 早期版本误用 `ancestors_of` 来检环 —— 它正好在能报告环的那一刻 break 掉了，
/// 导致环永远检不出来（环检测完全失效，遍历会不终止）。
fn find_cycle_from(name: &str, parents: &Parents) -> Option<Vec<String>> {
    let mut path = vec![name.to_string()];
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    seen.insert(name);

    let mut cur = name;
    while let Some(p) = parents.get(cur).and_then(|x| x.as_deref()) {
        if seen.contains(p) {
            path.push(p.to_string());
            return Some(path);
        }
        seen.insert(p);
        path.push(p.to_string());
        cur = p;
    }
    None
}

/// 校验层级，返回**问题清单**（空 = 通过）。
///
/// 两类问题：
/// 1. `dangling_parent` —— 父类型不存在
/// 2. `cycle` —— 沿父链走回自己
pub fn validate_hierarchy(parents: &Parents) -> Vec<String> {
    let mut problems = Vec::new();

    for (name, parent) in parents {
        if let Some(p) = parent {
            if !parents.contains_key(p) {
                problems.push(format!("dangling_parent: {name} 的父类型 {p} 不存在"));
            }
        }
    }

    for name in parents.keys() {
        if let Some(path) = find_cycle_from(name, parents) {
            problems.push(format!("cycle: {}", path.join(" -> ")));
        }
    }

    problems
}

/// 把 `candidate` 的父改成 `new_parent` 会不会成环。
///
/// 写入前校验 —— 规格 §4.6 要求环必须被拦住，
/// 否则遍历查询不终止。
pub fn would_create_cycle(parents: &Parents, candidate: &str, new_parent: Option<&str>) -> bool {
    let Some(np) = new_parent else { return false };
    if np == candidate {
        return true; // 自环
    }
    // 模拟改挂后，从新父出发能否走回 candidate
    let mut simulated = parents.clone();
    simulated.insert(candidate.to_string(), Some(np.to_string()));
    find_cycle_from(np, &simulated).is_some()
}

/// 校验用便捷函数：`candidate` 设成 `new_parent` 后是否合法。
pub fn check_reparent(parents: &Parents, candidate: &str, new_parent: Option<&str>) -> Result<(), String> {
    if would_create_cycle(parents, candidate, new_parent) {
        let np = new_parent.unwrap_or("(无)");
        return Err(format!("cycle: 把 {candidate} 挂到 {np} 下会形成环"));
    }
    if let Some(np) = new_parent {
        if !parents.contains_key(np) {
            return Err(format!("dangling_parent: 父类型 {np} 不存在"));
        }
    }
    Ok(())
}

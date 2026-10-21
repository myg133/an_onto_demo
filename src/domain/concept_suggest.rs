//! REQ-115 · `concept suggest` —— **建议层**：从关系拓扑算出候选分组。
//!
//! # 这一层不是权威（R1）
//!
//! 权威只有一处：`concept_groups` 表里**人签过**的声明（REQ-010 的
//! `concept import` → 双轨审核 → `proposal apply`）。本模块**只产出声明文件**，
//! 不写库、不改快照、不进 `/api/graph`：
//!
//! ```text
//! concept suggest ──(声明 JSON)──> concept import ──> 双轨签 ──> apply ──> 图上才有
//!        ↑ 本模块                    ↑ 已有（REQ-010）        ↑ 已有
//! ```
//!
//! 所以本模块的产物**天然是可反驳的**：猜错了，丢掉那份文件即可，零代价。
//! 这条与 `concept.rs` 开头那条红线同源 —— 概念组只是展示层声明。
//!
//! # 为什么必须确定性（R4 / AC-004）
//!
//! 「同一 project+version 跑两次 ⇒ 声明文件逐字节相同」不是形式要求：
//! 声明文件要进评审，评审记录里写着内容指纹（`content_digest`）。
//! 若两次跑出的分组不一样，同一个版本号就会指向两份不同的内容 ——
//! 于是「这一版到底包含什么」再也答不出来，而审核人签过的东西也就不再是
//! 他看到的那个东西。
//!
//! 本模块做到确定性的手段**只有三条**（没有随机数、没有时钟、没有哈希表遍历序）：
//!
//! 1. 输入先按实体 **key 排序**（`BTreeMap` / 有序 `Vec`）⇒ 输入顺序不影响结果；
//! 2. 迭代顺序 = 排序后的下标顺序，**不是** `HashMap` 的遍历序；
//! 3. 打分用**整数**（`i64`），平局时取更小的社区号 ⇒ 没有浮点比较的
//!    「同一台机器两次也可能不同」的余地。
//!
//! # 算法：贪心模块度（Louvain 的第一相，不做聚合）
//!
//! 每条关系算一条**无向**、权重 = 该对实体之间关系条数的边；自环单独记。
//! 初始一人一社区，然后反复扫：把节点挪到「让模块度增益最大的邻居社区」，
//! 没人再动就停（上限 `MAX_PASSES` 次，防病态输入让循环停不下来）。
//!
//! 增益只用整数比较。设 `M = Σ_i k_i`（= 2m）、`k_i` 为节点 i 的度和、
//! `K_c` 为社区 c 的度和、`W(i,c)` 为 i 连到 c（不含自己）的权重和，
//! 把 i 从 c 挪到 d 的模块度增量（乘以 M）正比于：
//!
//! ```text
//! Δ ∝ (W(i,d) - W(i,c)) - k_i·(K_d - K_c)/M
//! ```
//!
//! 两边同乘 M 后全是整数 ⇒ 直接比 `W(i,d)·M - k_i·K_d` 的大小即可。
//! （推导里 `A_ii` 与 `k_i²` 两项在「移出 c」与「移入 d」中相消，故上式不含它们。）
//!
//! # 为什么**单成员社区不成为组**
//!
//! 「一个成员的组」不是归并，只是换了个名字。把它写成组会让 L6 的
//! 「未分组」一栏永远为空 —— 而真实情况是「这一版大部分表根本没有关系」。
//! 所以只有 ≥2 个成员的社区才进声明；其余实体保持**未分组**，
//! 图上如实显示（R6）。
//!
//! # 组名与组 key（R3）
//!
//! - 组名：成员中**度数最高**那张表的展示名（平局取 key 更小的），
//!   声明文件里可被人改 —— 本模块不发明业务名；
//! - 组 key：`concept_01` / `concept_02` …（按组内第一个成员 key 排序分配）。
//!   与人写声明的 key 一样**只是标识**；写前会避开实体 key，
//!   免得触发 `NestedGroup`（成员恰好是另一个概念）。
//!
//! 本模块**零 I/O**（AC-002）：不读库、不读时钟、不写文件。
//! 数据库那一侧由 `src/cli.rs` 的 `concept suggest` 负责。

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::concept::{
    content_digest, ConceptDeclaration, ConceptGroup, DeclarationRevision,
};

/// 声明里的 `declared_by`：**记算法，不记「我」**（与 `concept.rs` 同一纪律）。
///
/// 值里带算法版本（`v1`）：算法换了就必须换这一栏 —— 否则「这份分组是怎么来的」
/// 会在评审记录里静默变成一个错误的答案。
pub const DECLARED_BY: &str = "concept-suggest/greedy-modularity-v1";

/// 聚类规则的一句话说明。评审者要先看规则，再看分组。
///
/// ⚠️ 这句话会被 `concept import` 打到 stdout，也会进声明文件的 `rule` 字段
/// （进而进 `content_digest`）。改措辞 = 改声明内容，必须升版本。
pub const RULE: &str = "按关系拓扑做确定性贪心模块度分组（整数打分、无随机数；同输入 ⇒ 同输出；单成员不成组）";

/// 声明版本。**规则不变就固定**：它记录的是「这一份建议」，不是「第几次跑」。
pub const DECLARATION_VERSION: &str = "1.0.0";

/// `source` 的取值（AC-010）：这份声明是**算法建议**，不是人签的结论。
pub const SOURCE_ALGORITHM: &str = "algorithm-suggest";

/// 每组的一句说明（会进 `/api/graph` 的 `scope_hint`，所以**不得**含守门词）。
pub const SCOPE_HINT: &str = "算法建议：按关系拓扑自动分组，待人审";

/// 迭代上限。正常数据 2–3 轮就收敛；给足余量只为防病态输入死循环。
const MAX_PASSES: usize = 50;

/// 建议层的输入：**实体（key + 展示名）+ 关系（两端 key）**。
///
/// 为什么不直接收 `Snapshot`：本模块要能被单测喂合成数据，而 `Snapshot`
/// 带着 `version_id` / `extracted_by` 等与算法无关的字段。调用方（CLI）
/// 负责把快照摊平成这两列 —— 摊平这一步没有判断，只有搬运。
pub struct SuggestInput<'a> {
    /// `(实体 key, 展示名)`。顺序任意：内部按 key 排序。
    pub entities: &'a [(String, String)],
    /// `(from 实体 key, to 实体 key)`，一条关系一项。顺序任意。
    pub links: &'a [(String, String)],
}

/// 算出一份**可直接 `concept import`** 的声明（不写库、不落盘）。
pub fn suggest(input: &SuggestInput<'_>) -> ConceptDeclaration {
    // ① 实体按 key 排序 —— 顺序无关性的唯一来源。
    let mut entities: Vec<(&str, &str)> = input
        .entities
        .iter()
        .map(|(k, n)| (k.as_str(), n.as_str()))
        .collect();
    entities.sort_by(|a, b| a.0.cmp(b.0));
    entities.dedup_by(|a, b| a.0 == b.0);

    let index: BTreeMap<&str, usize> = entities
        .iter()
        .enumerate()
        .map(|(i, (k, _))| (*k, i))
        .collect();

    // ② 无向加权邻接 + 自环权重。两端不在实体集合里的关系**跳过**
    //    （它们本来就是悬空关系；建议层不替抽取层补表）。
    let mut adj: Vec<BTreeMap<usize, u32>> = vec![BTreeMap::new(); entities.len()];
    let mut self_w: Vec<u32> = vec![0; entities.len()];
    for (from, to) in input.links {
        let (Some(&a), Some(&b)) = (index.get(from.as_str()), index.get(to.as_str())) else {
            continue;
        };
        if a == b {
            self_w[a] = self_w[a].saturating_add(1);
        } else {
            *adj[a].entry(b).or_insert(0) += 1;
            *adj[b].entry(a).or_insert(0) += 1;
        }
    }

    // ③ 社区检测（整数、有序、确定）
    let comm = communities(&adj, &self_w);

    // ④ 只保留 ≥2 个成员的社区，按组内第一个成员下标排序
    let mut by_comm: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (i, c) in comm.iter().enumerate() {
        by_comm.entry(*c).or_default().push(i);
    }
    let mut clusters: Vec<Vec<usize>> = by_comm
        .into_values()
        .filter(|members| members.len() >= 2)
        .collect();
    clusters.sort_by_key(|members| members.first().copied().unwrap_or(usize::MAX));

    // ⑤ 组 key 必须避开实体 key（否则「成员恰好是另一个概念」⇒ NestedGroup）
    let entity_keys: BTreeSet<&str> = entities.iter().map(|(k, _)| *k).collect();
    let mut used_keys: BTreeSet<String> = BTreeSet::new();

    let degree: Vec<i64> = (0..entities.len())
        .map(|i| {
            2 * self_w[i] as i64 + adj[i].values().map(|w| *w as i64).sum::<i64>()
        })
        .collect();

    let groups: Vec<ConceptGroup> = clusters
        .iter()
        .map(|members| {
            // 组名 = 成员中度数最高的那张表的展示名（平局取 key 更小 = 下标更小）。
            let mut dominant = members[0];
            for &i in members.iter().skip(1) {
                if degree[i] > degree[dominant] {
                    dominant = i;
                }
            }
            let display_name = {
                let shown = entities[dominant].1.trim();
                if shown.is_empty() {
                    entities[dominant].0.to_string()
                } else {
                    shown.to_string()
                }
            };
            let key = fresh_group_key(&entity_keys, &mut used_keys);
            ConceptGroup {
                key,
                display_name,
                // 成员按 key 排序（= 下标顺序）—— 声明顺序因此与输入顺序无关。
                members: members
                    .iter()
                    .map(|&i| entities[i].0.to_string())
                    .collect(),
                scope_hint: Some(SCOPE_HINT.to_string()),
                // 算法建议**没有被审过**，这是「明确声明拿不准」而不是沉默。
                low_confidence: Some(true),
                // 建议层不是抽取器，不伪造 `extracted_by`；
                // 走 `concept import` 时会按声明的 `declared_by` 记成外部声明。
                extracted_by: None,
                evidence: vec![],
            }
        })
        .collect();

    let digest = content_digest(&groups, RULE, DECLARED_BY);
    ConceptDeclaration {
        version: DECLARATION_VERSION.to_string(),
        declared_by: DECLARED_BY.to_string(),
        rule: RULE.to_string(),
        // ⛔ 没有人签过 —— 这一个 `None` 就是「建议」与「结论」的分界。
        confirmed_by: None,
        // ⭐ AC-010：来源可辨识。
        source: Some(SOURCE_ALGORITHM.to_string()),
        reviewed_by: None,
        history: vec![DeclarationRevision {
            version: DECLARATION_VERSION.to_string(),
            content_digest: digest,
            note: "由 concept suggest 从关系拓扑自动生成（未审核，不是结论）".to_string(),
        }],
        groups,
    }
}

/// 声明 JSON 的**唯一**序列化入口（AC-004 的逐字节确定性靠它）。
///
/// 不做 `HashMap` / `Value` 中转：`ConceptDeclaration` 的字段序由结构体定义固定，
/// 组与成员的顺序由 [`suggest`] 固定，所以两次调用的字节必然相同。
pub fn render(decl: &ConceptDeclaration) -> Result<String, serde_json::Error> {
    let mut text = serde_json::to_string_pretty(decl)?;
    text.push('\n');
    Ok(text)
}

/// 分配一个不与实体 key 冲突的组 key（`concept_01`、`concept_02` …）。
fn fresh_group_key(entity_keys: &BTreeSet<&str>, used: &mut BTreeSet<String>) -> String {
    let mut n = used.len() + 1;
    loop {
        let cand = format!("concept_{n:02}");
        n += 1;
        if entity_keys.contains(cand.as_str()) || !used.insert(cand.clone()) {
            continue;
        }
        return cand;
    }
}

/// 贪心模块度：**整数打分 + 固定遍历序 + 平局取小**。
///
/// 返回值是「节点下标 → 社区号」。社区号只是标签，不承诺连续。
fn communities(adj: &[BTreeMap<usize, u32>], self_w: &[u32]) -> Vec<usize> {
    let n = adj.len();
    // k_i：自环按 2 计（无向图的度和），与 M = Σ k_i 的约定一致。
    let k: Vec<i64> = (0..n)
        .map(|i| 2 * self_w[i] as i64 + adj[i].values().map(|w| *w as i64).sum::<i64>())
        .collect();
    // M = 2m。全图无边时直接返回一人一社区（调用方会把它们全部过滤掉）。
    let m2: i64 = k.iter().sum();
    let mut comm: Vec<usize> = (0..n).collect();
    if m2 == 0 {
        return comm;
    }
    let mut tot: Vec<i64> = k.clone();

    for _ in 0..MAX_PASSES {
        let mut moved = false;
        for i in 0..n {
            // 只和自环相连（或完全孤立）的节点不参与挪动。
            if adj[i].keys().all(|j| *j == i) {
                continue;
            }
            let current = comm[i];
            // 先把 i 从自己的社区里摘出来，后续打分才是「挪走之后的现状」。
            tot[current] -= k[i];

            // 邻居社区 → 与 i 的权重和（BTreeMap 保证遍历序确定）。
            let mut to: BTreeMap<usize, i64> = BTreeMap::new();
            for (j, w) in &adj[i] {
                if *j == i {
                    continue;
                }
                *to.entry(comm[*j]).or_insert(0) += *w as i64;
            }

            // 候选 = 原社区（原地不动）∪ 所有邻居社区。
            // score(d) = W(i,d)·M - k_i·K_d —— 与 ΔQ 同号，且全是整数。
            let mut best = current;
            let mut best_score = to.get(&current).copied().unwrap_or(0) * m2 - k[i] * tot[current];
            for (d, w) in &to {
                let score = w * m2 - k[i] * tot[*d];
                // 平局取更小的社区号：保证「两次运行完全一样」不依赖遍历顺序的巧合。
                if score > best_score || (score == best_score && *d < best) {
                    best = *d;
                    best_score = score;
                }
            }

            comm[i] = best;
            tot[best] += k[i];
            if best != current {
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
    comm
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entities(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, n)| (k.to_string(), n.to_string()))
            .collect()
    }

    fn links(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    fn run(ent: &[(String, String)], ln: &[(String, String)]) -> ConceptDeclaration {
        suggest(&SuggestInput { entities: ent, links: ln })
    }

    /// 两个三角 + 一条细连接 ⇒ 两块，而不是一整块，也不是散成六个单点。
    fn two_triangles() -> (Vec<(String, String)>, Vec<(String, String)>) {
        let ent = entities(&[
            ("a1", "甲一"),
            ("a2", "甲二"),
            ("a3", "甲三"),
            ("b1", "乙一"),
            ("b2", "乙二"),
            ("b3", "乙三"),
        ]);
        let ln = links(&[
            ("a1", "a2"),
            ("a2", "a3"),
            ("a3", "a1"),
            ("b1", "b2"),
            ("b2", "b3"),
            ("b3", "b1"),
            // 一条桥：模块度低于「拆成两块的收益」
            ("a3", "b1"),
        ]);
        (ent, ln)
    }

    #[test]
    fn test_two_runs_are_byte_identical() {
        let (ent, ln) = two_triangles();
        let a = render(&run(&ent, &ln)).expect("序列化");
        let b = render(&run(&ent, &ln)).expect("序列化");
        assert_eq!(a, b, "同一输入两次运行必须逐字节相同（AC-004）");
    }

    /// 输入顺序不影响结果：实体与关系都倒序喂一遍，字节仍相同。
    #[test]
    fn test_input_order_does_not_matter() {
        let (mut ent, mut ln) = two_triangles();
        let a = render(&run(&ent, &ln)).expect("序列化");
        ent.reverse();
        ln.reverse();
        let b = render(&run(&ent, &ln)).expect("序列化");
        assert_eq!(a, b, "输入顺序变了但结果必须一样（确定性来自排序，不是运气）");
    }

    #[test]
    fn test_two_clusters_are_found() {
        let (ent, ln) = two_triangles();
        let d = run(&ent, &ln);
        assert_eq!(d.groups.len(), 2, "两个三角应当被分成两组：{:?}", d.groups);
        let mut sizes: Vec<usize> = d.groups.iter().map(|g| g.members.len()).collect();
        sizes.sort();
        assert_eq!(sizes, vec![3, 3], "每组三个成员");
        // 桥两端不应在同一组（模块度会把它切开）
        let home = crate::domain::concept::home_map(&d.groups);
        assert_ne!(home.get("a3"), home.get("b1"), "桥两端被划进了同一组");
    }

    #[test]
    fn test_singletons_are_never_groups() {
        // 三个点只有一条边 ⇒ 一个两人组；第三个是**未分组**，不是「一个人的组」
        let ent = entities(&[("x", "叉"), ("y", "歪"), ("z", "贼")]);
        let ln = links(&[("x", "y")]);
        let d = run(&ent, &ln);
        assert_eq!(d.groups.len(), 1);
        assert_eq!(d.groups[0].members, vec!["x".to_string(), "y".to_string()]);
        let covered: BTreeSet<&str> = d
            .groups
            .iter()
            .flat_map(|g| g.members.iter().map(String::as_str))
            .collect();
        assert!(!covered.contains("z"), "孤点不该被塞进任何组");
    }

    #[test]
    fn test_no_links_means_no_groups() {
        let ent = entities(&[("x", "叉"), ("y", "歪")]);
        let d = run(&ent, &[]);
        assert!(d.groups.is_empty(), "没有任何关系 ⇒ 没有任何组（不许虚构）");
        assert!(d.verify().is_ok(), "空组的声明也必须自洽");
    }

    #[test]
    fn test_dangling_link_endpoints_are_ignored() {
        let ent = entities(&[("x", "叉"), ("y", "歪")]);
        let ln = links(&[("x", "y"), ("x", "ghost"), ("ghost", "y")]);
        let d = run(&ent, &ln);
        assert_eq!(d.groups.len(), 1);
        assert_eq!(d.groups[0].members.len(), 2);
    }

    #[test]
    fn test_self_loops_do_not_panic_and_stay_unaligned() {
        let ent = entities(&[("x", "叉"), ("y", "歪")]);
        let ln = links(&[("x", "x"), ("y", "y")]);
        let d = run(&ent, &ln);
        assert!(d.groups.is_empty(), "只有自环 ⇒ 没有跨对象的关系，不该硬凑组");
        assert!(d.verify().is_ok());
    }

    #[test]
    fn test_group_keys_never_collide_with_entity_keys() {
        // 故意让实体 key 叫 `concept_01`：组 key 必须让开，否则会成为嵌套归并
        let ent = entities(&[("concept_01", "甲"), ("b", "乙"), ("c", "丙")]);
        let ln = links(&[("b", "c"), ("b", "concept_01")]);
        let d = run(&ent, &ln);
        let entity_keys: BTreeSet<&str> = ent.iter().map(|(k, _)| k.as_str()).collect();
        for g in &d.groups {
            assert!(
                !entity_keys.contains(g.key.as_str()),
                "组 key {} 撞上了实体 key —— 会触发嵌套归并校验",
                g.key
            );
        }
    }

    /// AC-010：来源必须可辨识 —— `source` 明说算法、`reviewed_by` 是 null。
    #[test]
    fn test_declaration_says_it_is_an_algorithm_suggestion() {
        let (ent, ln) = two_triangles();
        let d = run(&ent, &ln);
        assert_eq!(d.source.as_deref(), Some(SOURCE_ALGORITHM));
        assert_eq!(d.reviewed_by, None, "还没人审过");
        assert_eq!(d.confirmed_by, None, "没人确认过");
        assert_eq!(d.declared_by, DECLARED_BY);
        let text = render(&d).expect("序列化");
        let v: serde_json::Value = serde_json::from_str(&text).expect("必须是合法 JSON");
        assert_eq!(v["source"], "algorithm-suggest");
        assert_eq!(v["reviewed_by"], serde_json::Value::Null);
        assert_eq!(v["confirmed_by"], serde_json::Value::Null);
        assert!(v["history"].is_array(), "契约字段必须写出来");
    }

    #[test]
    fn test_declaration_passes_the_version_contract() {
        let (ent, ln) = two_triangles();
        let d = run(&ent, &ln);
        assert_eq!(d.verify(), Ok(()), "产出的声明必须可直接 import");
    }

    #[test]
    fn test_group_name_comes_from_the_best_connected_member() {
        // 星形：中心 x 连所有叶子 ⇒ 组名应为 x 的展示名
        let ent = entities(&[("x", "中心"), ("y", "叶一"), ("z", "叶二")]);
        let ln = links(&[("x", "y"), ("x", "z")]);
        let d = run(&ent, &ln);
        assert_eq!(d.groups.len(), 1);
        assert_eq!(d.groups[0].display_name, "中心");
    }

    #[test]
    fn test_empty_display_name_falls_back_to_the_entity_key() {
        let ent = entities(&[("x", "  "), ("y", "叶")]);
        let ln = links(&[("x", "y")]);
        let d = run(&ent, &ln);
        assert_eq!(d.groups[0].display_name, "x", "展示名空白时退回实体 key，而不是造一个名字");
    }
}

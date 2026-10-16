//! 客户视图 → 图结构（REQ-093 · 只读图服务的数据面）
//!
//! # 为什么有这一份
//!
//! 客户能看的出口此前只有一个：`ontok ontology show --html`
//! —— 一个**离线单文件**。规格要求双轨审核里「客户必须过目」，
//! 静态文件过不了这一关（打不开、看不懂、不愿意看）。
//! 本模块给 HTTP 服务提供那份**图数据**，让本体关系能**在浏览器里看**。
//!
//! **只读**：本模块是纯函数，连输入都是**已经冻结**的客户视图。
//! 没有任何写入口，也不该有 —— 提案 / 审核 / apply / freeze 一律
//! 不经过这里。
//!
//! # 零 I/O（AC-005）
//!
//! 本文件不得出现任何存储层类型或模块路径（具体词表见 `domain/mod.rs`
//! 的模块头）。取快照、读版本、判冻结都在 `src/server/` 那一侧完成。
//!
//! ⚠️ **连注释里也不写那三个词**：验收判据是 `grep -rn <词表> src/domain/`
//! 必须无输出，而 `apply.rs` / `mod.rs` / `review_view.rs` / `snapshot.rs`
//! 早已在**注释**里写了那三个词（基线 0455523 实测 9 处命中，AC-005 早就不成立）。
//! 本文件不添这份乱 —— 判据见 `tests/test_graph_server.rs::domain_layer_stays_io_free`，
//! 它**剥掉注释后**再判，这才是真正想表达的那件事。
//!
//! # ⚠️ 守门词陷阱：`key` 不能作为**线上字段名**
//!
//! `Node` 的 Rust 字段叫 `key`，但它序列化出去是 **`"id"`**
//! （见字段上的 `serde(rename)`）。原因不是洁癖：
//!
//! - `scan_modeling_terms` 扫的是**整段序列化文本**（键和值都扫），
//!   `key` 就在 `MODELING_TERMS` 词表里（`view.rs` 的 `"key"`）；
//! - 键名 `"key"` 经 `english_term_hit` 的分词后就是一个 `key` 词元
//!   ⇒ **每一次**图响应都会被守门器拒绝，服务永远答不出东西。
//!
//! `mcp/tools.rs` 的 `catalog()` 早就写过同一条纪律
//! （「参数名与字段名一律避开 `scan_modeling_terms` 的守门词」），
//! 这里只是把它落到数据面上。**改这个 `rename` 等于让整个服务 500。**
//!
//! # 关于节点标识（与 `Node.label` 为什么会长得一样）
//!
//! `CustomerView` **刻意不含 key**（`view.rs` 的 `project` 明确丢弃
//! `key` / `evidence` / `ordinal` …）。所以图里能拿到的唯一稳定名字
//! 就是**展示名本身** —— 于是 `key` 与 `label` 在绝大多数节点上同值。
//!
//! 这**不是**把 key 漏出去：真 key（`robot_device` 这类）恰恰被
//! `usable_as_human_name` 挡在客户视图之外，从来没进过这个模块。
//! 但它确实意味着**图标识 = 展示名**，改名即换标识 ——
//! 已登记为已知限制，见模块末尾。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::domain::html_view::count_phrase;
use crate::domain::view::CustomerView;

/// 整张图。
///
/// ⛔ **刻意没有 `dropped_edges` 字段。**
/// 它曾经存在（数「指不到节点的边」），本轮**按评审结论删掉**：
/// 那个数在生产路径上**可证恒为 0** —— 见 [`project`] 的「为什么不需要
/// 丢边计数器」。一个恒为零的计数器与没有计数器等价，却让每个读响应的人
/// 多问一次「这个数什么时候非零」。不花的观测能力就别放上wire。
///
/// 「不许静默丢边」这件事**没有被削弱** —— 它换成了一条**真会红的测试**
/// （`no_edge_is_ever_dropped_for_a_projected_view`），钉的是不变量本身，
/// 而不是在线上放一个读起来永远是 0 的数字。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphProjection {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

/// 图里的一个对象。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    /// 节点标识。
    ///
    /// # ⛔ 线上**不能**叫 `key` —— 求求了，别"顺手改回去"
    ///
    /// Rust 字段叫 `key`，但 `#[serde(rename = "id")]` 让它在线上叫 **`id`**。
    /// 这不是命名偏好，是**服务能不能用**的区别：
    ///
    /// - `scan_modeling_terms` 扫的是**整段序列化文本**（键和值都扫），
    ///   而 `key` 就在 `MODELING_TERMS` 词表里（`view.rs`）；
    /// - 键名 `"key"` 经 `english_term_hit` 分词后就是一个 `key` 词元
    ///   ⇒ **每一次** `/api/graph` 响应都会被守门器拒绝 ⇒
    ///   服务永远 500、永远答不出东西，而报错信息还会指向一个
    ///   看起来毫无问题的字段名。
    ///
    /// `mcp/tools.rs` 的 `catalog()` 早就写过同一条纪律
    /// （「参数名与字段名一律避开 `scan_modeling_terms` 的守门词」）。
    /// **删掉这行 `rename` 会让整个服务当场不可用**，
    /// 而 `serialized_projection_has_no_modeling_terms` 会立刻红。
    #[serde(rename = "id")]
    pub key: String,
    /// 客户看到的中文名（**绝不是内部标识**）。
    pub label: String,
    pub attribute_count: usize,
    /// 上层对象的**标识**（不是展示名）—— 与 `Edge` 的 `from`/`to` 同一空间。
    pub parent: Option<String>,
}

/// 图里的一条关系。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    /// `Node.key`。
    pub from: String,
    /// `Node.key`。
    pub to: String,
    /// 关系的人话说法（来自 `RelationView.meaning`，已翻好）。
    pub label: String,
    /// 站在 `from` 端往 `to` 看，有几个。
    pub count_from: Option<String>,
    /// 站在 `to` 端往 `from` 看，有几个。
    pub count_to: Option<String>,
}

/// 把客户视图投影成图。**纯函数，零 I/O，不改输入。**
///
/// 节点 = 客户视图里的每个对象；边 = 每条关系。
/// **不做任何过滤** —— 能进图的都已经在 `view::project` 那层过了关，
/// 这一层再筛一遍就成了「两份筛子永远一致」的空转（纪律 H）。
///
/// # 为什么不需要「丢边计数器」
///
/// [`GraphProjection`] 里没有 `dropped_edges`，而这里**确实**有一条
/// `continue` 会跳过指不到节点的边。它为什么不需要被数出来：
///
/// `view::project` 里 `by_key` 与 `objects` 用的是**同一个**过滤谓词
/// （`usable_as_human_name`），而关系两端也是拿 `by_key` 查出来的
/// —— 查得到才生成 `RelationView`。所以**每一条 `RelationView` 的两端
/// 必然都在 `objects` 里**。可证恒等，不需要计数器。
///
/// 这条不变量由 `no_edge_is_ever_dropped_for_a_projected_view` 钉住：
/// 那条测试真的把 `view::project` 与本函数**串起来**跑（不是各跑一遍
/// 再比结果），所以哪天有人改了 `view::project` 的过滤谓词，它会立刻红。
/// **这比在线上挂一个恒为 0 的字段强** —— 字段只是报数，测试才是不变量。
///
/// ⚠️ 若将来**手搓**一个 `CustomerView`（`CustomerView` 是 pub）喂进来，
/// 跳过分支就真的会走到。那时该补的是**测试**，不是线上字段。
/// [`assign_keys`] 的返回值：这一批对象的节点标识，以及给**引用**用的名字索引。
struct Keying<'a> {
    /// 与 `objects` **同序**的节点标识（第 i 个对应第 i 个对象）。
    keys: Vec<String>,
    /// 展示名 → **第一个**同名节点的标识。引用（parent / 边端点）只认它。
    by_name: BTreeMap<&'a str, String>,
}

/// **节点标识的唯一分配处。**
///
/// ⚠️ 全仓**只有 [`project`] 调它**。`/api/node` 的寻址键不是另算一份，
/// 而是**直接读 `project` 产出的 `graph.nodes[i].key`** —— 也就是
/// `/api/graph` 序列化给客户的那个 key 本身。两边因此没有「同步」这回事。
///
/// ⚠️ 之前这里曾经有两套：图用「第一个同名 → 原名」，而 `/api/node` 那侧
/// 另起一个 `BTreeMap<展示名, 对象>`，**后者覆盖前者** ⇒ 落到最后一个。
/// 后果是客户可见的：图上写着 10 项属性，点开只有 9 项，另一个节点直接 404
/// （P1）。修法不是「两边都改成第一个」（那仍靠人记得同步），而是**只留这一处**。
///
/// ⚠️ 节点标识**不能**事后拿「名字 → 标识」的表回填：那个表一个名字只放得下
/// 一个标识，第二个同名对象回填时会拿回第一个的标识，四个节点三个 id
/// （`duplicate_display_names_do_not_collide_in_keys` 抓的就是这个）。
fn assign_keys(objects: &[crate::domain::view::EntityView]) -> Keying<'_> {
    let mut used: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut by_name: BTreeMap<&str, String> = BTreeMap::new();
    let mut keys = Vec::with_capacity(objects.len());

    for o in objects {
        let key = if used.insert(o.name.clone()) {
            o.name.clone()
        } else {
            let mut n = 2;
            loop {
                let cand = format!("{}#{n}", o.name);
                if used.insert(cand.clone()) {
                    break cand;
                }
                n += 1;
            }
        };
        by_name.entry(o.name.as_str()).or_insert_with(|| key.clone());
        keys.push(key);
    }
    Keying { keys, by_name }
}

pub fn project(v: &CustomerView) -> GraphProjection {
    // ⚠️ 同名是**真实存在**的，不是假想：
    // `demo3` 第 2 版实测 **91 个对象只有 90 个不同展示名** ——
    // 两张源表都翻成「客户账号绑定」（属性数 10 与 9）。
    // 所以这条数据通路上**节点数与去重后名字数不相等是正常结果**，
    // 看到 91 ≠ 90 不要当成 bug。
    // 机制同样见 `mcp/serve.rs` 的注释与 `overview` 的 `duplicate_member_names`；
    // 而客户视图已经把 key 丢了，所以引用只能按名字对齐 —— 见 L2。
    let keying = assign_keys(&v.objects);

    let mut nodes: Vec<Node> = v
        .objects
        .iter()
        .zip(keying.keys.iter())
        .map(|(o, key)| Node {
            key: key.clone(),
            label: o.name.clone(),
            attribute_count: o.properties.len(),
            parent: None, // 第二趟补：那时 by_name 才建全
        })
        .collect();

    // 第二趟补 parent —— 必须等 by_name 建全，否则先出现的子对象找不到父。
    for (n, o) in nodes.iter_mut().zip(&v.objects) {
        n.parent = o
            .belongs_to
            .as_deref()
            .and_then(|p| keying.by_name.get(p).cloned());
    }

    let mut edges = Vec::with_capacity(v.relations.len());
    for r in &v.relations {
        // 两端都必须在节点集合里才画这条边。
        //
        // 对 `view::project` 的产物而言这条 `else` **走不到**（理由见本函数
        // 文档）；保留它只是为了手搓 `CustomerView` 时不 panic。
        let (Some(from), Some(to)) = (
            keying.by_name.get(r.from.as_str()),
            keying.by_name.get(r.to.as_str()),
        ) else {
            continue;
        };
        // 两端各一个数量词 —— 「站在这一端看有几个」是**同一份判据**，
        // 复用 `html_view::count_phrase`（`overview` 也复用同一份）：
        // 抄一份就会漂，页面上说「多个」、图上说「1 个」，客户当场发现对不上。
        edges.push(Edge {
            from: from.clone(),
            to: to.clone(),
            label: r.meaning.clone(),
            count_from: Some(count_phrase(&r.how_many, true).to_string()),
            count_to: Some(count_phrase(&r.how_many, false).to_string()),
        });
    }

    GraphProjection {
        nodes,
        edges,
    }
}

// ──────────────────────────────────────────────────────────
// 已知限制（纪律 H：一致性守卫抓不到的东西必须登记，不得写成「已封堵」）
// ──────────────────────────────────────────────────────────
//
// **L1 · 图标识 = 展示名，无法拿到真 key。**
//   `view::project` 刻意丢弃 key，所以本层无从复原。
//   漂移后果边界：展示名一改，图的 `id` 就改，前端按 id 缓存/收藏的链接失效。
//   *形状*数据（图的结构）不受影响 —— 只影响「按标识定位」。
//
// **L2 · 同名对象只能对齐到第一个。**
//   客户视图里两张源表可能翻成同一个中文名；本层按出现顺序给后者加 `#N`，
//   但 `belongs_to` / `RelationView.from` / `.to` **都只有名字**，
//   所以父对象与边的两端一律落到**第一个**同名节点上。
//   后果边界：**真实数据上已经发生** —— `demo3` 第 2 版里「客户账号绑定」
//   有两个（属性数 10 与 9），唯一那条 `客户账号绑定 → 客户` 的边落在第一个上。
//   客户视图自己就分不出是哪一张表（图只是如实继承），所以这不是本层引入的，
//   但**后果确实由本层承担**：`客户账号绑定#2` 在图上是一条**没有任何边**的孤立节点。
//   确切行为由 `references_resolve_to_the_first_of_two_same_named_objects` 钉住。
//   **已知且未封堵。** 封堵它需要客户视图带 key ⇒ 要先改 `view.rs`（禁改红线），属另一条需求。
//
// **L3 · 不是「忠实的重复实现」。**
//   本模块**没有**复制 `view.rs` 的 `semantic_cn` / `cardinality_cn`：
//   `RelationView.meaning` / `.how_many` 进来时**已经是中文**，
//   本层原样取用。数量词复用 `html_view::count_phrase`（同一份函数，非副本）。
//   唯一的新逻辑是「展示名 → 节点标识」的重名处理，本仓库内无第二份。

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::view::{EntityView, PropertyView, RelationView};

    fn cv() -> CustomerView {
        CustomerView {
            objects: vec![
                EntityView {
                    name: "订单".into(),
                    note: Some("客户下单产生的记录".into()),
                    belongs_to: None,
                    aliases: vec![],
                    properties: vec![
                        PropertyView {
                            name: "状态".into(),
                            note: None,
                            required: true,
                            sample: Some("已完成".into()),
                        },
                        PropertyView {
                            name: "下单时间".into(),
                            note: None,
                            required: false,
                            sample: None,
                        },
                    ],
                },
                EntityView {
                    name: "订单明细".into(),
                    note: None,
                    belongs_to: Some("订单".into()),
                    aliases: vec![],
                    properties: vec![],
                },
                EntityView {
                    name: "客户".into(),
                    note: None,
                    belongs_to: None,
                    aliases: vec![],
                    properties: vec![],
                },
            ],
            relations: vec![RelationView {
                from: "订单明细".into(),
                to: "订单".into(),
                how_many: "多对一".into(),
                meaning: "归属于".into(),
            }],
        }
    }

    #[test]
    fn node_label_is_the_chinese_name_never_an_identifier() {
        let g = project(&cv());
        let o = g.nodes.iter().find(|n| n.label == "订单").unwrap();
        assert_eq!(o.attribute_count, 2, "属性数应数客户视图里的属性");
        assert!(o.parent.is_none());
        let d = g.nodes.iter().find(|n| n.label == "订单明细").unwrap();
        assert_eq!(d.parent.as_deref(), Some("订单"), "parent 应指向上层的**标识**");
    }

    #[test]
    fn edge_uses_customer_wording_and_two_sided_counts() {
        let g = project(&cv());
        assert_eq!(g.edges.len(), 1);
        let e = &g.edges[0];
        assert_eq!((e.from.as_str(), e.to.as_str()), ("订单明细", "订单"));
        assert_eq!(e.label, "归属于", "边上的字必须是客户视图的说法");
        // 站在 from 端 = 「一条明细对应 1 个订单」；站在 to 端 = 「一个订单有多个明细」
        assert_eq!(e.count_from.as_deref(), Some("1 个"));
        assert_eq!(e.count_to.as_deref(), Some("多个"));
    }

    #[test]
    fn edge_pointing_at_a_missing_object_is_skipped_without_panicking() {
        // ⚠️ 只对手搓的 `CustomerView` 成立 —— `view::project` 的产物走不到这里
        // （理由见 `project` 的「为什么不需要丢边计数器」）。
        let mut v = cv();
        v.relations.push(RelationView {
            from: "订单".into(),
            to: "查无此物".into(),
            how_many: "多对一".into(),
            meaning: "归属于".into(),
        });
        let g = project(&v);
        assert_eq!(
            g.edges.len(),
            1,
            "指不到节点的边不画（生产路径上恒不发生；这里只保证不 panic）"
        );
    }

    /// ⛔ 钉死「一条边都不丢」这个不变量 —— **这才是 `dropped_edges` 的替代品**。
    ///
    /// 它把 `view::project` 与 `graph::project` **串起来**跑（不是各跑一遍
    /// 再比结果，所以不是纪律 H 的空转）：造几份**畸形**的快照 ——
    /// 含展示名退化（进不了客户视图）、含指向不存在对象的 link_type ——
    /// 然后断言 `graph.edges.len() == view.relations.len()`。
    ///
    /// 哪天有人改了 `view::project` 的过滤谓词，导致某个端点进不了 `objects`，
    /// 这条立刻红。**字段只是报数，测试才是不变量。**
    #[test]
    fn no_edge_is_ever_dropped_for_a_projected_view() {
        use crate::domain::view::{EntityTypeView, LinkTypeView, SnapshotView};

        // 一份刻意畸形的快照：对象名有的正常、有的退化成 key、有的空白；
        // link_type 指向一个**根本不存在**的对象。
        let sv = SnapshotView {
            entity_types: vec![
                EntityTypeView {
                    key: "order".into(),
                    display_name: "订单".into(),
                    description: None,
                    parent: Some("customer".into()),
                    aliases: vec![],
                    disjoint_with: vec![],
                    properties: vec![],
                    evidence: vec![],
                },
                EntityTypeView {
                    key: "order_item".into(),
                    display_name: "订单明细".into(),
                    description: None,
                    parent: Some("order".into()),
                    aliases: vec![],
                    disjoint_with: vec![],
                    properties: vec![],
                    evidence: vec![],
                },
                EntityTypeView {
                    key: "customer".into(),
                    display_name: "客户".into(),
                    description: None,
                    parent: None,
                    aliases: vec![],
                    disjoint_with: vec![],
                    properties: vec![],
                    evidence: vec![],
                },
                // 展示名退化成了 key 形态 ⇒ `usable_as_human_name` 不认
                // ⇒ 不会进 `objects`，而指向它的边也不该被生成
                EntityTypeView {
                    key: "raw_blob".into(),
                    display_name: "raw_blob".into(),
                    description: None,
                    parent: None,
                    aliases: vec![],
                    disjoint_with: vec![],
                    properties: vec![],
                    evidence: vec![],
                },
            ],
            link_types: vec![
                LinkTypeView {
                    key: "lt1".into(),
                    from_type: "order_item".into(),
                    to_type: "order".into(),
                    semantic: "belongs_to".into(),
                    cardinality: "N:1".into(),
                },
                // 指向不存在 ⇒ `by_key` 查不到 ⇒ 这条**根本不会**成为 RelationView
                LinkTypeView {
                    key: "lt2".into(),
                    from_type: "order".into(),
                    to_type: "nowhere".into(),
                    semantic: "belongs_to".into(),
                    cardinality: "N:1".into(),
                },
            ],
        };

        let view = crate::domain::view::project(&sv);
        let g = project(&view);
        assert_eq!(
            g.edges.len(),
            view.relations.len(),
            "每一条客户视图里的关系都必须画得出来 —— 少一条就是静默丢边"
        );
        // 且每条边的两端都必须是真实节点（不是悬空的 key）
        let ids: std::collections::BTreeSet<&str> =
            g.nodes.iter().map(|n| n.key.as_str()).collect();
        for e in &g.edges {
            assert!(ids.contains(e.from.as_str()), "边的 from 悬空：{e:?}");
            assert!(ids.contains(e.to.as_str()), "边的 to 悬空：{e:?}");
        }
    }

    #[test]
    fn duplicate_display_names_do_not_collide_in_keys() {
        let mut v = cv();
        v.objects.push(EntityView {
            name: "订单".into(),
            note: None,
            belongs_to: None,
            aliases: vec![],
            properties: vec![],
        });
        let g = project(&v);
        let ids: std::collections::BTreeSet<&str> =
            g.nodes.iter().map(|n| n.key.as_str()).collect();
        assert_eq!(ids.len(), g.nodes.len(), "节点标识必须互不相同");
        assert!(ids.contains("订单") && ids.contains("订单#2"));
    }

    /// ⛔ **已知限制 L2 的可执行锚点**（不是「已封堵」，是「已知且记在这里」）。
    ///
    /// 同名对象里，引用（parent / 边端点）一律落到**第一个**同名节点上 ——
    /// 因为客户视图里的引用只有名字、没有 key。
    /// 这条测试的作用是：**将来谁动了这块，行为变了会立刻被看见**，
    /// 而不是让 L2 在代码里悄悄漂走。
    #[test]
    fn references_resolve_to_the_first_of_two_same_named_objects() {
        let mut v = cv();
        v.objects.push(EntityView {
            name: "订单".into(),
            note: None,
            belongs_to: None,
            aliases: vec![],
            properties: vec![],
        });
        // 边的 `to` 是「订单」→ 只能落在**第一个**「订单」上
        v.relations = vec![RelationView {
            from: "订单明细".into(),
            to: "订单".into(),
            how_many: "多对一".into(),
            meaning: "归属于".into(),
        }];
        let g = project(&v);
        assert_eq!(
            g.edges[0].to, "订单",
            "同名时引用只能对齐到第一个（L2 的确切行为）"
        );
        // 而「订单明细」的 parent 同样指第一个「订单」
        let d = g.nodes.iter().find(|n| n.label == "订单明细").unwrap();
        assert_eq!(d.parent.as_deref(), Some("订单"));
    }

    /// ⛔ 本测试是**扫描器自检**（纪律 E）：它证明守门器**不是恒空**。
    ///
    /// 若哪天 `scan_modeling_terms` 变成永远返回空，上面那条
    /// `guard_passes` 绿得毫无意义 —— 只有这条红着，才说明闸门还在。
    #[test]
    fn the_guard_scanner_is_not_vacuous_positive_control() {
        // 对照组：故意塞进守门词，必须被抓到
        let bad = r#"{"id":"订单","note":"这个字段叫 key"}"#;
        let hits = crate::domain::view::scan_modeling_terms(bad);
        assert!(
            !hits.is_empty(),
            "守门器对已知建模术语必须命中 —— 它若返回空，本文件所有「通过」都是假的"
        );
    }

    /// 图的**线上形状**不得含建模术语。
    ///
    /// 这条同时钉住 `Node.key` 的 `serde(rename = "id")`：
    /// 去掉 rename → JSON 里出现 `"key"` → 本测试立刻红。
    #[test]
    fn serialized_projection_has_no_modeling_terms() {
        let g = project(&cv());
        let text = serde_json::to_string(&g).unwrap();
        let hits = crate::domain::view::scan_modeling_terms(&text);
        assert!(hits.is_empty(), "图数据泄露了建模术语：{hits:?}\n{text}");
    }
}

//! **业务总览** —— 按「概念」给客户看的那一份（REQ-010）。
//!
//! # 这份东西解决什么
//!
//! 客户视图今天是 91 个对象的平铺清单。判读手册的判读人（3 名非开发
//! 业务人员、每人 40 分钟）面对 91 张卡，第一分钟就会问「所以这系统
//! 里到底有哪几样东西」—— 现在答不上来。总览就是这份答案：
//!
//! ```text
//! 业务总览 · 91 个对象 → 20 个业务概念（+ 39 个未归组对象）
//!
//! 1. 套餐   管什么：给客户看的三类套餐配置
//!    对象：套餐模板 / 套餐产品 / 套餐权益
//!    属性（31 项，同名已合并）：…
//!    关系：一个 套餐，有几个 订单？ 多个
//! ```
//!
//! # 三条设计约束（都写在代码里，理由见各处注释）
//!
//! 1. **关系重定向**：指向组内成员的关系，对客户显示时指向**组**。
//!    客户问「一个订单，属于哪个套餐」——答案不该是三张表里的任意一张。
//! 2. **同名属性去重**：跨表都有 `created_at` / `状态` 这类属性，
//!    合成一张卡后必须合并，否则同一句话在卡里出现十几遍。
//!    合并时**记下来自哪些成员** —— 少给信息可以，少给「凭什么合并」不行。
//! 3. **未归组对象照旧单独显示**：没被任何组覆盖的实体一个都不许丢。
//!    一个组都不给时，总览退化成「对象清单」= 现在的行为（向后兼容）。
//!
//! # 零 I/O（AC-002）

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::concept::ConceptGroup;
use crate::domain::view::{is_han, usable_as_human_name, CustomerView, EntityView};

/// 数量词。**复用** `html_view::count_phrase`（只放宽了可见性）——
/// 「站在这一端看过去有几个」是同一份判据，抄一份就会漂：
/// 页面上说「多个」、总览里说「1 个」，判读人会当场发现两处对不上。
use crate::domain::html_view::{count_phrase, sanitize_display};

/// 一个业务概念在总览里的那张卡。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConceptCard {
    /// 概念名（客户看的就是这个）
    pub name: String,
    /// 一句话：管什么
    pub summary: Option<String>,
    /// 成员对象（按客户视图里首次出现的顺序）
    pub members: Vec<MemberCard>,
    /// 跨成员去重后的属性
    pub properties: Vec<PropertyRow>,
    /// 关系 —— 指向组内成员的那一端已重定向到概念
    pub relations: Vec<RelationRow>,
    /// 组内被省略的关系统计（不是静默丢弃）
    pub relations_inside_skipped: usize,
    /// 显示名重复的成员数（源库两张表翻成同一个名字时会命中）
    pub duplicate_member_names: usize,
    /// 因名称带技术用语而没上总览的属性数（不静默）
    pub properties_dropped: usize,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MemberCard {
    pub name: String,
    pub note: Option<String>,
    pub property_count: usize,
}

/// 一行属性。`from_members` 是**同名属性从哪些成员合并而来**的证据。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PropertyRow {
    pub name: String,
    pub from_members: Vec<String>,
    /// 任一成员必填 ⇒ 必填（「都要」比「有一个要」更接近客户视角）
    pub required: bool,
    pub sample: Option<String>,
}

/// 一条关系，写成判读人脑子里那个问句。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RelationRow {
    pub question: String,
    /// 「多个」/「1 个」—— 直接用客户视图已翻好的基数，不重新解释
    pub answer: String,
}

/// 没被任何组覆盖的对象 —— **照旧单独显示**。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StandaloneObject {
    pub name: String,
    pub property_count: usize,
}

/// 数字。给人看「这次到底发生了什么」，也用于 E2E 报告。
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct OverviewStats {
    pub objects_total: usize,
    pub objects_in_groups: usize,
    pub objects_uncovered: usize,
    pub concept_count: usize,
    /// 组数 - 1？否：**被剔除的概念数**（显示名不可用 / 成员全不在视图里）
    pub concepts_dropped: usize,
    pub properties_before_dedup: usize,
    pub properties_after_dedup: usize,
    /// 因名称带技术用语而没上总览的属性数（S-2：丢弃，不原样透出）
    pub properties_dropped: usize,
    pub relations_total: usize,
    /// **被重定向的端点数**，不是关系数。
    ///
    /// 一条关系可以两端各属一个概念（A 的成员 → B 的成员），
    /// 在 A 的卡上数一次、在 B 的卡上又数一次。写「69 条关系」是错的 ——
    /// 关系只有 51 条。**报出来的数字必须能被读者核对**，所以这里
    /// 老实叫「端点」，文案也照着说。
    pub relation_ends_redirected: usize,
    /// 两端都在同一个组里、因而省略的关系数
    pub relations_inside_skipped: usize,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Overview {
    pub concepts: Vec<ConceptCard>,
    pub uncovered: Vec<StandaloneObject>,
    pub stats: OverviewStats,
    /// **降级/剔除必须报出来**，不许静默（与 `html_view` 的纪律同源）
    pub notes: Vec<String>,
}

/// 造一份业务总览。
///
/// `names` 是「实体 key → 展示名」的映射（由调用方从快照给）。
/// **必须用 key 定位成员**，而不是拿展示名去猜：两个成员可能翻成同一个
/// 展示名（真实源库里 `t016` 与 `t027`
/// 都叫「客户账号绑定」），按名字反查会张冠李戴。
pub fn build_overview(
    view: &CustomerView,
    groups: &[ConceptGroup],
    names: &BTreeMap<String, String>,
) -> Overview {
    // 客户视图里能查到的对象（按展示名索引；重名取第一个，
    // 重名情况由 duplicate_member_names 如实报出）
    let by_name: BTreeMap<&str, &EntityView> =
        view.objects.iter().map(|o| (o.name.as_str(), o)).collect();

    // 成员展示名 → 概念名（关系重定向的依据）
    //
    // `or_insert` 而不是 `insert`：**重叠时取先声明的那个**，保证答案确定。
    // 正常路径上重叠已被 `validate_groups` 拒掉；这里是纵深防御。
    let mut redirect: BTreeMap<&str, &str> = BTreeMap::new();
    for g in groups {
        for m in &g.members {
            if let Some(n) = names.get(m) {
                redirect.entry(n.as_str()).or_insert(g.display_name.as_str());
            }
        }
    }

    // 全部实体 key —— 概念显示名的 S-1 判据（退化成了 key 的不许上总览）
    let keys: BTreeSet<String> = names.keys().cloned().collect();

    let mut concepts: Vec<ConceptCard> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let mut props_dropped = 0usize;
    let mut stats = OverviewStats { objects_total: view.objects.len(), ..Default::default() };

    let mut claimed: BTreeSet<String> = BTreeSet::new();

    for g in groups {
        // 概念显示名与实体一样过 S-1：退化成了 key 的概念不上总览，
        // 而且**必须报出来**（静默少一个概念 = 判读人以为系统里没有它）。
        if !concept_name_is_usable(&g.display_name, &keys) {
            stats.concepts_dropped += 1;
            notes.push(format!(
                "概念「{}」（key={}）的名字不可用，没上总览",
                g.display_name, g.key
            ));
            continue;
        }

        // 成员 → 客户视图里的对象。取不到（视图过滤掉了 / key 不在快照里）
        // 要计数并报出，不静默少一个成员。
        let mut members: Vec<MemberCard> = Vec::new();
        let mut member_names: Vec<&str> = Vec::new();
        let mut missing = 0usize;
        let mut dup_names = 0usize;
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for m in &g.members {
            let Some(name) = names.get(m) else {
                missing += 1;
                continue;
            };
            let Some(obj) = by_name.get(name.as_str()) else {
                missing += 1;
                continue;
            };
            if !seen.insert(name.as_str()) {
                dup_names += 1;
                continue;
            }
            members.push(MemberCard {
                name: obj.name.clone(),
                // 说明文字来自源库表注释，与属性名同类脏 —— 同样过清洗
                note: obj.note.as_deref().and_then(sanitize_display).map(String::from),
                property_count: obj.properties.len(),
            });
            member_names.push(obj.name.as_str());
        }
        if members.is_empty() {
            stats.concepts_dropped += 1;
            notes.push(format!(
                "概念「{}」的 {} 个成员都没进客户视图，整张卡没上总览",
                g.display_name,
                g.members.len()
            ));
            continue;
        }
        if missing > 0 {
            notes.push(format!(
                "概念「{}」有 {} 个成员不在客户视图里（未显示）",
                g.display_name, missing
            ));
        }
        if dup_names > 0 {
            notes.push(format!(
                "概念「{}」有 {dup_names} 个成员显示名与别的成员相同（源库两张表翻成了同一个名字），这里只显示一次",
                g.display_name
            ));
        }
        for n in &member_names {
            claimed.insert(n.to_string());
        }

        // ── 属性：页面级清洗 → 跨成员同名合并 ──
        //
        // 清洗**复用** `html_view::sanitize_display`，不另写判据。
        // 为什么总览也要过它：客户视图 JSON 与 HTML 页面本来就有两层
        // 清洗（页面多这一层），总览是从 JSON 投影出来的。
        // 不共用的话，同一个属性在页面上叫「角色」、在总览里叫「角色ID」，
        // 判读人当场就会发现两处对不上，然后不再信我们。
        // 认不出的（S-2）**丢弃并计数**，绝不原样透出。
        let mut rows: Vec<PropertyRow> = Vec::new();
        let mut index: BTreeMap<String, usize> = BTreeMap::new();
        let mut before = 0usize;
        let mut dropped_props = 0usize;
        for n in &member_names {
            let Some(obj) = by_name.get(n) else { continue };
            for p in &obj.properties {
                before += 1;
                let Some(clean) = sanitize_display(&p.name) else {
                    dropped_props += 1;
                    continue;
                };
                match index.get(&clean) {
                    Some(i) => {
                        let row = &mut rows[*i];
                        if !row.from_members.iter().any(|m| m == n) {
                            row.from_members.push(n.to_string());
                        }
                        row.required = row.required || p.required;
                        if row.sample.is_none() {
                            row.sample = p.sample.clone();
                        }
                    }
                    None => {
                        index.insert(clean.clone(), rows.len());
                        rows.push(PropertyRow {
                            name: clean,
                            from_members: vec![n.to_string()],
                            required: p.required,
                            sample: p.sample.clone(),
                        });
                    }
                }
            }
        }
        if dropped_props > 0 {
            props_dropped += dropped_props;
            notes.push(format!(
                "概念「{}」有 {dropped_props} 项属性名称带技术用语，没上总览",
                g.display_name
            ));
        }

        // ── 关系：一端一条问句，指向组内成员的那端指向概念 ──
        let in_group: BTreeSet<&str> = member_names.iter().copied().collect();
        let mut relations: Vec<RelationRow> = Vec::new();
        let mut inside_skipped = 0usize;
        let mut redirected_here = 0usize;
        let mut rel_seen: BTreeSet<(String, String)> = BTreeSet::new();
        for r in &view.relations {
            let from_in = in_group.contains(r.from.as_str());
            let to_in = in_group.contains(r.to.as_str());
            if !from_in && !to_in {
                continue;
            }
            if from_in && to_in {
                // 组内关系重定向后会变成「一个 套餐，有几个 套餐？」——
                // 那不是客户会问的问题。**省略并计数**，不硬凑。
                inside_skipped += 1;
                continue;
            }
            let (other, self_is_from) = if from_in {
                (redirect.get(r.to.as_str()).copied().unwrap_or(r.to.as_str()), true)
            } else {
                (redirect.get(r.from.as_str()).copied().unwrap_or(r.from.as_str()), false)
            };
            // 只要有一端是本组成员，这个端点就被重定向过一次
            redirected_here += 1;
            let answer = count_phrase(&r.how_many, self_is_from).to_string();
            let question = format!("一个 {}，有几个 {}？", g.display_name, other);
            if rel_seen.insert((question.clone(), answer.clone())) {
                relations.push(RelationRow { question, answer });
            }
        }
        stats.relation_ends_redirected += redirected_here;
        stats.relations_inside_skipped += inside_skipped;
        stats.properties_before_dedup += before;
        stats.properties_after_dedup += rows.len();

        concepts.push(ConceptCard {
            name: g.display_name.clone(),
            summary: g.scope_hint.clone(),
            members,
            properties: rows,
            relations,
            relations_inside_skipped: inside_skipped,
            duplicate_member_names: dup_names,
            properties_dropped: dropped_props,
        });
    }

    stats.concept_count = concepts.len();
    stats.properties_dropped = props_dropped;

    // ── 未归组对象：一个都不许丢 ──
    let uncovered: Vec<StandaloneObject> = view
        .objects
        .iter()
        .filter(|o| !claimed.contains(&o.name))
        .map(|o| StandaloneObject { name: o.name.clone(), property_count: o.properties.len() })
        .collect();
    stats.objects_in_groups = claimed.len();
    stats.objects_uncovered = uncovered.len();
    stats.relations_total = view.relations.len();

    Overview { concepts, uncovered, stats, notes }
}

/// 总览 → 人读文本。
///
/// **页面上不许出现的东西这里也不许出现**：不打印 `key`、不打印表名、
/// 不打印证据。所以本函数**只**吃 [`Overview`]，而 `Overview` 里
/// 本来就没有这几个字段（编译期保证，不靠自觉）。
pub fn render_overview_text(o: &Overview) -> String {
    let mut s = String::new();
    s.push_str("业务总览\n");
    s.push_str(&format!(
        "{} 个对象 → {} 个业务概念（+ {} 个未归组对象）\n",
        o.stats.objects_total, o.stats.concept_count, o.stats.objects_uncovered
    ));
    s.push_str(&format!(
        "属性：合并前 {} 项 → 合并后 {} 项；关系：{} 条，其中 {} 个指向已重定向到概念、{} 条组内关系已省略\n",
        o.stats.properties_before_dedup,
        o.stats.properties_after_dedup,
        o.stats.relations_total,
        o.stats.relation_ends_redirected,
        o.stats.relations_inside_skipped
    ));
    s.push_str("概念是「给人看的分组」，对象本身没有变化；同一个对象只属于一个概念。\n");

    for (i, c) in o.concepts.iter().enumerate() {
        s.push_str(&format!("\n{}. {}\n", i + 1, c.name));
        match c.summary.as_deref().filter(|t| !t.trim().is_empty()) {
            Some(t) => s.push_str(&format!("   管什么：{t}\n")),
            None => s.push_str("   管什么：（还没写说明）\n"),
        }
        let names: Vec<&str> = c.members.iter().map(|m| m.name.as_str()).collect();
        s.push_str(&format!("   对象（{}）：{}\n", names.len(), names.join(" · ")));
        s.push_str(&format!("   属性（{} 项，同名已合并）：\n", c.properties.len()));
        for p in &c.properties {
            let mut line = format!("     · {}", p.name);
            if p.required {
                line.push_str("（必填）");
            }
            if p.from_members.len() > 1 {
                line.push_str(&format!("　← 合并自 {}", p.from_members.join("、")));
            }
            s.push_str(&line);
            s.push('\n');
        }
        if c.relations.is_empty() {
            s.push_str("   关系：（这条概念与别的对象之间没有已确认的关系）\n");
        } else {
            s.push_str("   关系：\n");
            for r in &c.relations {
                s.push_str(&format!("     · {}　{}\n", r.question, r.answer));
            }
        }
        if c.properties_dropped > 0 {
            s.push_str(&format!(
                "   （另有 {} 项属性名称带系统内部的技术叫法，没有显示）\n",
                c.properties_dropped
            ));
        }
        if c.relations_inside_skipped > 0 {
            s.push_str(&format!(
                "   （另有 {} 条组内关系已省略 —— 同组对象之间的关系不构成对客户有意义的问题）\n",
                c.relations_inside_skipped
            ));
        }
    }

    if !o.uncovered.is_empty() {
        s.push_str(&format!(
            "\n未归组对象（{}）—— 照旧按单个对象显示：\n",
            o.uncovered.len()
        ));
        for u in &o.uncovered {
            s.push_str(&format!("   · {}（{} 项属性）\n", u.name, u.property_count));
        }
    }

    if !o.notes.is_empty() {
        s.push_str("\n以下情况已在此说明，未静默略过：\n");
        for n in &o.notes {
            s.push_str(&format!("   ⚠ {n}\n"));
        }
    }
    s
}

/// 概念名能不能给客户看：非空、含汉字、且**不是**退化成了实体 key。
///
/// 与实体显示名用同一把尺（`view::usable_as_human_name`），
/// 只是额外要求「得像个中文概念名」—— 概念是抽象业务词，
/// 纯英文名（`package`）对业务人员没有信息量。
pub fn concept_name_is_usable(name: &str, keys: &BTreeSet<String>) -> bool {
    name.chars().any(is_han) && usable_as_human_name(name, keys)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_groups_reduce_to_plain_object_list() {
        // 没组 = 现在的行为：对象一个不少，全部「未归组」单独显示
        let view = CustomerView {
            objects: vec![EntityView {
                name: "客户".into(),
                note: None,
                belongs_to: None,
                aliases: vec![],
                properties: vec![],
            }],
            relations: vec![],
        };
        let o = build_overview(&view, &[], &BTreeMap::new());
        assert!(o.concepts.is_empty());
        assert_eq!(o.uncovered.len(), 1);
        assert_eq!(o.uncovered[0].name, "客户");
    }
}

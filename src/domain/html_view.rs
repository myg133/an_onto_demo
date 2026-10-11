//! 客户视图 → 单文件 HTML（REQ-006 · 任务 A）
//!
//! # 这份代码存在的原因
//!
//! `ontok ontology show` 原本只输出 JSON，而**判读人是客户方的业务人员**——
//! 他们不会读 JSON。判读手册（`2026-10-03-ac013-judging-playbook.md`）把
//! 「客户视图页面」列为判读的第一件材料，**页面上不许出现任何建模术语**。
//!
//! # 三条硬约束（对应判读材料的硬要求）
//!
//! 1. **零外部依赖**：不引 CDN、不引外部字体、不用构建工具。
//!    判读人可能**离线**双击打开。CSS/JS 全部内联，字体只用系统字体栈。
//! 2. **零建模术语**：页面上不出现 `key` / `实体` / `基数` / `外键` / `主键` /
//!    `证据` / `快照` / `版本号` / `N:1` / `id` / `entity` / `property` / `link`，
//!    也不出现表名、列名、实体 key。
//! 3. **关系按人的问法显示**：「一个客户有几条 API 调用日志」，
//!    而不是「API 调用日志 多对一 客户」。
//!
//! # 基数方向那个坑
//!
//! 视图里 `API 调用日志 多对一 客户` 读作「一条日志属于一个客户」，
//! 但人自然想的是「**一个客户有几条 API 调用日志**」——
//! **同一件事，从不同端看，问法不同、数量也不同**。
//! 所以每条关系要在**两端各生成一个问句**，而不是只生成一个记号。
//! 见 [`relation_as_question`]。
//!
//! **本模块不改动 `how_many` 的语义方向**（那属下一轮），
//! 只负责把已有的中文方向翻译成人话问法。

use std::collections::BTreeMap;

use crate::domain::identifier_shape;
use crate::domain::view::{CustomerView, EntityView, PropertyView, RelationView};

/// 判读手册 §3.1 的**分层抽样 13 个对象**（按概念族分层，非随机抽 20%）。
///
/// 族内分配：客户族 4 / 套餐族 3 / 子账号族 2 / 账务族 2 / 稀疏 2。
///
/// # ★ 名单只放**业务对象**（REQ-009 · 阻断项 ①）
///
/// 判读人是 3 名**非开发者**业务人员，每人 40 分钟 —— **注意力是唯一稀缺资源**。
/// ★ 标记的职责是「这 13 个是本次要判读的主体对象」，所以技术设施
/// （alembic 迁移表、鉴权缓存事件流）**不占 ★ 名额**：
/// 业务人员没有能力判断它们、也不需要判断，把注意力花在这里挤掉的是
/// 「客户有没有套餐」「余额对不对」这些真正要答的问题。
///
/// 技术反例**没有消失** —— 它们照常列在页面上（§3.1 Q6c 仍指着它们问），
/// 只是不高亮。**反例的本意（验准入边界）与「该给业务人员看的东西」是两件事**，
/// 以前把两者塞进同一个 ★ 才导致 3/13 的判读注意力被浪费。
///
/// 页面按这份清单高亮（★），让判读人不用在 90 个对象里自己找。
/// 可用 `--sampling <file>` 覆盖（每行一个对象名）。
pub const DEFAULT_SAMPLING: &[&str] = &[
    // 客户族（4）—— 现状最碎，是概念合并问题的主战场
    "客户",
    "客户订阅",
    "客户折扣返利",
    "客户意向",
    // 套餐族（3）—— 归并的典型正例
    "套餐模板",
    "套餐产品",
    "套餐权益",
    // 子账号族（2）—— 第二个正例
    "子账号权限",
    "子账号额度",
    // 账务族（2）—— **REQ-009 新增**：原先 13 个名额里一个账务对象都没有，
    // 而「客户余额账户 / 扣费流水」是客户口述里出现频率最高的两件事，
    // 属性数也最多（26 / 24）。Q2「属性对不对」在它们身上信息量最大。
    "客户余额账户",
    "扣费流水",
    // 稀疏对象（2）—— 属性数最少的 2 个**业务**对象，验边界：
    // 属性少到只剩 3~4 个时客户还认不认得。
    // ⚠️ 原先这一族里的 `数据库迁移版本`（1 个属性「版本数量」，alembic 迁移表）
    //    已移出 ★ —— 它验的是「技术设施」，不是「业务边界」。
    "试用记录",
    "内部员工",
];

/// HTML 转义。数据来自源库注释，**必须**转义。
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// 关系在**这一端**的数量词。
///
/// 输入是视图里已翻好的中文方向（`how_many`），输出是站在
/// `self_is_from` 这一端看过去「有几个」。
///
/// | `how_many` | from 端 | to 端 |
/// | --- | --- | --- |
/// | 多对一 | 1 个 | 多个 |
/// | 一对多 | 多个 | 1 个 |
/// | 多对多 | 多个 | 多个 |
/// | 唯一对应 | 1 个 | 1 个 |
/// | 其它 | 多个 | 多个 |
///
/// 「其它」这一行**刻意不静默猜**：`how_many` 的兜底已由
/// [`crate::domain::view::scan_modeling_terms`] 第三/四档守门当场抓住、
/// 整份视图拒绝发布，所以走到这里的都是已认识的形态。
///
/// 【REQ-010 只放宽可见性，逻辑一个字没动】业务总览的关系问句要复用它 ——
/// 「站在这一端看过去有几个」是同一份判据。两处各写一份的话，
/// 页面说「多个」而总览说「1 个」，判读人会当场发现两处对不上。
pub fn count_phrase(how_many: &str, self_is_from: bool) -> &'static str {
    let many = "多个";
    let one = "1 个";
    match (how_many, self_is_from) {
        ("多对一", true) => one,   // 一条日志指向一个客户
        ("多对一", false) => many, // 一个客户有几条日志 ← 人自然想问的
        ("一对多", true) => many,
        ("一对多", false) => one,
        ("多对多", _) => many,
        ("唯一对应", _) => one,
        _ => many,
    }
}

/// 站在某一个对象的立场，把一条关系写成**人话问句**。
///
/// 关键：**问句的方向跟着「当前对象是关系的哪一端」翻转**。
/// `API 调用日志 多对一 客户`：
/// - 当前对象是「API 调用日志」→ 「一个 API 调用日志，对应几个 客户？」→ 1 个
/// - 当前对象是「客户」→ 「一个 客户，有几个 API 调用日志？」→ 多个
///
/// 第二个问句才是判读人脑子里真正在问的那个。
fn relation_as_question(
    self_name: &str,
    other_name: &str,
    how_many: &str,
    self_is_from: bool,
) -> (String, &'static str) {
    let q = format!("一个 {}，有几个 {}？", esc(self_name), esc(other_name));
    (q, count_phrase(how_many, self_is_from))
}

/// 首字分组。
///
/// **按首字分，不按拼音** —— 拼音分组需要一张拼音表，
/// 那要么引外部依赖（违反「零外部依赖、离线可开」）、
/// 要么手抄几千字（违反本项目「词表是枚举」那条纪律）。
/// 首字分组的价值一样：91 个平铺翻不动，分组后能折叠。
fn group_of(name: &str) -> String {
    match name.chars().next() {
        Some(c) if crate::domain::view::is_han(c) => c.to_string(),
        _ => "其他".to_string(),
    }
}

/// **客户视图页面明令禁止的词**（判读手册 §材料清单）。
///
/// 与 [`crate::domain::view::MODELING_TERMS`] 的分工：那���档守的是
/// 「**序列化后的 JSON** 里不许有」，这一档守的是「**给人看的页面上不许有**」——
/// 后者更宽，多了 `快照` / `版本号` / `实体` 这类只在页面上被禁的词。
///
/// **只用于剔除对象名**。翻译表里出现过 `子账号绑定权限快照` 这种名字
/// （源表 `sub_account_bind_permission`），它对业务人员既费解又带建模味，
/// **不上页面**，但也**不静默** —— 调用方会拿到被剔除的清单并打印出来。
pub const PAGE_FORBIDDEN_CJK: &[&str] = &[
    "实体", "基数", "外键", "主键", "证据", "快照", "版本号",
];

/// 渲染结果。`skipped` 是**因含禁用词而没上页面**的对象名 —— 必须报给调用方。
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedPage {
    pub html: String,
    /// 因对象名带建模用语而没上页面的
    pub skipped: Vec<String>,
    /// 因属性名/说明带技术用语而没上页面的
    pub skipped_props: Vec<String>,
}

/// 名字里带了页面禁用词吗？
fn name_is_forbidden(name: &str) -> bool {
    PAGE_FORBIDDEN_CJK.iter().any(|t| name.contains(t))
}

/// 这段文本里有没有**源库原始标识符**的形态？
///
/// ⚠️ REQ-029 起，本文件**不再持有判据副本** —— 判据只有一份，在
/// [`crate::domain::identifier_shape`]，页面层与 MCP 层**共用同一个函数**
/// （本地副本曾与 MCP 侧漂移，`对应abc.def` 在页面侧**漏抓**，客户看得见的
/// 名字里会留着 `对应abcdef`）。这里只保留**调用**。
///
/// 判据是**形态**不是词表：
/// - `customer_id` / `llm_model.model_id` / `api_key` —— 下划线或点路径，
///   这是**列名 / 表名**的形态，业务语言里不会出现；
/// - 独立的 `key` 词元 —— 建模术语本体（[`identifier_shape::is_modeling_literal`]）。
///
/// 命中就整条丢弃：这些串**没有任何办法翻成人话**，
/// 硬改只会造出「关联模型（对应 llm 模型 模型编号）」这种半吊子。
fn has_raw_identifier(s: &str) -> bool {
    identifier_shape::has_raw_identifier(s)
}

/// 从显示名里摘掉 `ID` / `id` 这个技术记号。
///
/// 「客户ID」→「客户」，「创建者(管理员ID)」→「创建者(管理员)」。
///
/// **为什么是摘掉而不是整条丢弃**：`ID` 是业务人员都懂的通用后缀，
/// 真正碍眼的是它是英文技术记号。整条丢掉会让判读页凭空少几十个属性
/// （Q2 属性识别率的分母跟着掉），摘掉之后属性还在、只是不再带技术味。
/// 摘完什么都不剩的才丢。
fn strip_id_token(s: &str) -> String {
    // 先把 ASCII 词元切成独立片段（前面补一个分隔符，便于回退整段）
    let mut out = String::with_capacity(s.len());
    let mut tok = String::new();
    let flush = |out: &mut String, tok: &mut String| {
        if tok.is_empty() {
            return;
        }
        if !tok.eq_ignore_ascii_case("id") {
            out.push_str(tok);
        }
        tok.clear();
    };
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            tok.push(c);
        } else {
            flush(&mut out, &mut tok);
            out.push(c);
        }
    }
    flush(&mut out, &mut tok);

    // 空白**压成单个**而不是全删 —— 「AI 搜索」不能变成「AI搜索」
    let mut cleaned = String::with_capacity(out.len());
    let mut prev_space = false;
    for c in out.chars() {
        if c.is_whitespace() {
            if !prev_space {
                cleaned.push(' ');
                prev_space = true;
            }
        } else {
            cleaned.push(c);
            prev_space = false;
        }
    }
    // 只摘两端的**分隔符**；括号是有配对的，不能一刀切掉
    // （否则「主账号ID（冻结操作者）」会变成「主账号（冻结操作者」）
    let cleaned = cleaned
        .trim_matches(|c: char| " \t:：,，、-—_".contains(c))
        .to_string();
    // 摘 ID 之后可能留下空括号
    let cleaned = cleaned.replace("（）", "").replace("()", "");
    cleaned.trim().to_string()
}

/// **属性显示名的页面级清洗**。
///
/// 抽取侧的注释质量门槛（REQ-005）只判「有没有汉字」，
/// 于是 `客户ID` / `用量快照` / `核销人customer_id` 这类**含汉字的技术串**
/// 会原样进客户视图。页面是判读人唯一的材料，**必须在这一层再挡一次**。
///
/// | 情形 | 处理 | 例 |
/// | --- | --- | --- |
/// | 含中文建模术语 | 丢弃 | `用量快照` → 丢 |
/// | 含源库原始标识符 | 丢弃 | `核销人customer_id` → 丢 |
/// | 只带 `ID` 记号 | 摘掉记号保留 | `客户ID` → `客户` |
/// | 其余 | 原样 | `扣款币种` → `扣款币种` |
///
/// 返回 `None` = 这个属性不上页面（调用方必须报出来，不许静默）。
pub fn sanitize_display(name: &str) -> Option<String> {
    if name_is_forbidden(name) || has_raw_identifier(name) {
        return None;
    }
    let stripped = strip_id_token(name);
    if stripped.is_empty() {
        return None;
    }
    Some(stripped)
}

/// 属性名和说明**清洗之后**是否渲染成同一句话。
///
/// # 为什么只判「完全相同」，不判「包含」
///
/// REQ-007 实测：真实源库的 598 条带说明属性里，**598 条说明 == 属性名**
/// （不是复审估的 96%，是 100%）。判读人看到的是每个名字连着出现两遍，
/// 第一反应是「这工具坏了」—— 而 Q2 问的正是「它有哪几个属性？」。
///
/// **只收敛「完全相同」**：
/// - 相同 ⇒ 纯冗余，去掉零信息损失；
/// - **包含 ≠ 冗余**。`属性名=状态`、`说明=状态（成功/失败）` 时，
///   说明里多出来的括号内容**是信息**，去掉它等于删内容。
///   把「包含」也收敛掉，会让判读人**悄悄少读一句话**，
///   而静默丢信息正是这个项目栽过六次的那类坑。
///
/// **比较的是「清洗后」的两串**，不是原始两串：名字和说明各自都要过
/// [`sanitize_display`]（摘 `ID` 记号、压空白），`客户ID` / `客户 ID`
/// 清洗后都是「客户」，同样属于该收敛的重复。
fn note_is_redundant(name: &str, note: &str) -> bool {
    name.trim() == note.trim()
}

/// 把一份客户视图渲染成自包含 HTML。
///
/// `sampling` 是要高亮的对象名（判读手册 §3.1 的 13 个）。
/// 清单里有、但视图里没有的对象**不报错**（视图可能裁掉了），
/// 但也不高亮 —— 页面上不会凭空多出对象。
pub fn render_html(view: &CustomerView, sampling: &[String]) -> RenderedPage {
    let star: Vec<&str> = sampling.iter().map(|s| s.as_str()).collect();
    let is_starred = |n: &str| star.contains(&n);

    // ── 关系按对象归拢：一端一条问句 ──
    let mut outgoing: BTreeMap<&str, Vec<(&str, &str)>> = BTreeMap::new(); // from → (to, how_many)
    let mut incoming: BTreeMap<&str, Vec<(&str, &str)>> = BTreeMap::new(); // to   → (from, how_many)
    for RelationView { from, to, how_many, .. } in &view.relations {
        outgoing.entry(from.as_str()).or_default().push((to.as_str(), how_many.as_str()));
        incoming.entry(to.as_str()).or_default().push((from.as_str(), how_many.as_str()));
    }

    let render_object = |o: &EntityView, skipped_props: &mut Vec<String>| -> String {
        let mut h = String::new();
        // 本对象剔除计数 = 渲染完之后 `skipped_props` 的增量（REQ-007）。
        let before = skipped_props.len();
        let star_mark = if is_starred(&o.name) { "<span class=\"star\">★ 本次重点查看</span>" } else { "" };
        h.push_str(&format!("<summary><span class=\"名\">{}</span>{star_mark}</summary>", esc(&o.name)));
        h.push_str("<div class=\"体\">");

        // 对象说明同样来自源库表注释，会带进 `API-Key限额配置表` 这种技术串。
        // 说明文字是补充性的，脏了就整段不显示（并记进被剔除清单），不硬改。
        if let Some(note) = o
            .note
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .and_then(sanitize_display)
        {
            h.push_str(&format!("<p class=\"说明\">{}</p>", esc(&note)));
        }

        // 上层对象：说成「它是……的一部分」，不用「父级」这种词
        if let Some(parent) = o.belongs_to.as_deref().filter(|s| !s.trim().is_empty()) {
            h.push_str(&format!(
                "<p class=\"说明\">它是「{}」的一部分。</p>",
                esc(parent)
            ));
        }

        // ── 属性（显示前逐条过页面级清洗）──
        // `cleaned_note` 存的是**已经过清洗的说明**，渲染时直接复用 ——
        // 清洗必须是纯函数，两次调用结果一致，但重复调用等于把判据写两遍。
        let mut shown: Vec<(&PropertyView, String, Option<String>)> = Vec::new();
        for p in &o.properties {
            match sanitize_display(&p.name) {
                // 属性名干净，说明文字再单独过一道
                Some(n) => {
                    let raw_note = p.note.as_deref().filter(|s| !s.trim().is_empty());
                    let note = raw_note.and_then(sanitize_display);
                    if raw_note.is_some() && note.is_none() {
                        skipped_props.push(format!("{}/{}（说明文字带技术用语）", o.name, p.name));
                    }
                    shown.push((p, n, note));
                }
                None => skipped_props.push(format!("{}/{}", o.name, p.name)),
            }
        }
        let skipped_here = skipped_props.len() - before;
        h.push_str(&format!("<h4>它记录了什么（{} 项）</h4>", shown.len()));
        // ── 剔除告知（REQ-007）：本页少显示了多少，**当场说清楚** ──
        //
        // 静默剔除是本项目最贵的一种 fail-open：判读人问「这对象属性怎么这么少」，
        // 我们会归因为「客户认不出」或「抽取能力不足」，
        // 而真相是「我们没给它看」—— Q2（属性识别率 ≥70%）会被系统性污染。
        // 只报**数量**、**不报被剔掉的原名**（那等于把技术串再泄一次）。
        if skipped_here > 0 {
            h.push_str(&format!(
                "<p class=\"未显示\">另有 {} 项因为名称带系统内部的技术叫法，没有显示。</p>",
                skipped_here
            ));
        }
        if shown.is_empty() {
            h.push_str("<p class=\"空\">这一项目前还没有整理出可显示的内容。</p>");
        } else {
            h.push_str("<ul class=\"属性\">");
            for (p, name, note) in &shown {
                h.push_str("<li>");
                h.push_str(&format!("<span class=\"属性名\">{}</span>", esc(name)));
                // 说明与名字重复时**只显示一个**（REQ-007）。`as_deref()` 后再判一次，
                // 因为 `shown` 里存的 `note` 可能就是 `None`。
                if let Some(note) = note.as_deref().filter(|n| !note_is_redundant(name, n)) {
                    h.push_str(&format!("<span class=\"属性说明\">{}</span>", esc(note)));
                }
                if let Some(s) = p.sample.as_deref().filter(|s| !s.trim().is_empty()) {
                    h.push_str(&format!("<span class=\"样例\">举例：{}</span>", esc(s)));
                }
                h.push_str("</li>");
            }
            h.push_str("</ul>");
        }

        // ── 关系：每条关系在两端各生成一个问句 ──
        let mut qs: Vec<(String, &'static str)> = Vec::new();
        if let Some(list) = outgoing.get(o.name.as_str()) {
            for (to, how_many) in list {
                qs.push(relation_as_question(&o.name, to, how_many, true));
            }
        }
        if let Some(list) = incoming.get(o.name.as_str()) {
            for (from, how_many) in list {
                qs.push(relation_as_question(&o.name, from, how_many, false));
            }
        }
        h.push_str(&format!("<h4>它和别的东西是什么关系（{} 条）</h4>", qs.len()));
        if qs.is_empty() {
            h.push_str("<p class=\"空\">目前还没找到它和别的东西的关联。</p>");
        } else {
            h.push_str("<ul class=\"关系\">");
            for (q, ans) in qs {
                h.push_str(&format!("<li><span class=\"问\">{}</span><span class=\"答\">{}</span></li>", q, ans));
            }
            h.push_str("</ul>");
        }

        h.push_str("</div></details>");
        h
    };

    // ── 全部对象按首字分组 ──
    // 含禁用词的对象**不上页面**，但要被记下来报出去（fail-closed 且不静默）。
    let mut skipped: Vec<String> = Vec::new();
    let mut skipped_props: Vec<String> = Vec::new();
    let mut groups: BTreeMap<String, Vec<&EntityView>> = BTreeMap::new();
    for o in &view.objects {
        if name_is_forbidden(&o.name) {
            skipped.push(o.name.clone());
            continue;
        }
        groups.entry(group_of(&o.name)).or_default().push(o);
    }

    // ── 先渲染全部对象，再拼页头 ──
    //
    // 顺序是刻意的：页头要报「剔除了多少」，而剔除数**只有渲染完才知道**。
    // 先渲染再拼页头，就不必把计数从渲染层往外倒腾。
    let mut objects_html = String::new();
    for (g, items) in &groups {
        // 命中 ★ 的组默认展开，其余折叠 —— 判读人第一眼看到的是重点
        let opened = items.iter().any(|o| is_starred(&o.name));
        objects_html.push_str(&format!(
            "<details class=\"分组\"{}><summary class=\"组名\">{} <span class=\"计数\">{} 个</span></summary>",
            if opened { " open" } else { "" },
            esc(g),
            items.len()
        ));
        for o in items {
            let cls = if is_starred(&o.name) { "对象 重点" } else { "对象" };
            objects_html.push_str(&format!("<details class=\"{cls}\">"));
            objects_html.push_str(&render_object(o, &mut skipped_props));
        }
        objects_html.push_str("</details>");
    }

    let mut body = String::new();
    body.push_str("<p class=\"提示\">点一个名字就能展开。共 ");
    body.push_str(&groups.values().map(|v| v.len()).sum::<usize>().to_string());
    body.push_str(" 个对象，按首字分组；标了 ★ 的是本次判读建议重点看的 ");
    body.push_str(
        &groups
            .values()
            .flatten()
            .filter(|o| is_starred(&o.name))
            .count()
            .to_string(),
    );
    body.push_str(" 个。</p>");

    // ── 剔除告知（REQ-007）────────────────────────────────────
    //
    // **判读人手里只有这一份材料**。剔除只写进 stderr 等于：
    // 客户说「这对象属性怎么这么少」→ 我们归因为「客户认不出」或
    // 「抽取能力不足」→ 真相是「我们没给它看」→ **Q2 被系统性污染**。
    //
    // **措辞的三条约束**（都是这页的真实失败模式）：
    // 1. **只报数量，绝不报被剔掉的原名** —— 原名正是含技术用语的那些串，
    //    印上去等于又漏一次（泄漏路径从「显示」搬到「说明」不算修好）。
    // 2. **不能说「剔除」「过滤」「非法」** —— 那是我们的内部判断，
    //    业务人员听了只会更困惑（"谁剔的？凭什么？"）。说「系统内部的技术叫法」
    //    是描述**名字的来源**，不是指责数据。
    // 3. **不带任何建模术语** —— 页头正文会被 `scan_modeling_terms` 守门词
    //    全量扫（`cli.rs` fail-closed），也过不了 `BA/verify-page.js` 的
    //    「建模 / 字段 / 属性表」黑名单。
    let obj_n = skipped.len();
    let prop_n = skipped_props.len();
    if obj_n + prop_n > 0 {
        body.push_str("<div class=\"未显示汇总\"><p class=\"未显示标题\">这份清单没有列全，说明一下</p><ul class=\"未显示\">");
        if obj_n > 0 {
            body.push_str(&format!(
                "<li>有 {obj_n} 个对象的名字带系统内部的技术叫法，没有列进来。</li>"
            ));
        }
        if prop_n > 0 {
            body.push_str(&format!(
                "<li>另有 {prop_n} 项因为名称带系统内部的技术叫法，没有显示（每个对象下面会单独标出它自己少了几项）。</li>"
            ));
        }
        body.push_str("</ul><p class=\"未显示脚\">这些项本身不是不重要，是当前叫法还带着系统的内部说法，直接搬过来会让清单难读。想看它们请告诉我们。</p></div>");
    }

    body.push_str("<p class=\"查找\"><input class=\"查找框\" type=\"text\" placeholder=\"输入对象名，页面上会只留匹配的那些\"></p>");
    body.push_str(&objects_html);

    let script = r#"
const box = document.querySelector('.查找框');
const cards = Array.from(document.querySelectorAll('.对象'));
const groups = Array.from(document.querySelectorAll('.分组'));
function 过滤() {
  const q = box.value.trim();
  cards.forEach(function (c) {
    c.style.display = (q === '' || c.textContent.indexOf(q) >= 0) ? '' : 'none';
  });
  groups.forEach(function (g) {
    const any = Array.from(g.querySelectorAll('.对象')).some(function (c) {
      return c.style.display !== 'none';
    });
    g.style.display = any ? '' : 'none';
  });
}
box.addEventListener('input', 过滤);
"#;

    let html = format!(        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>业务对象总览</title>
<style>
*{{box-sizing:border-box}}
body{{margin:0;padding:28px 20px 60px;background:#f6f7f9;color:#1b1f24;
line-height:1.7;font-family:"PingFang SC","Hiragino Sans GB","Microsoft YaHei",sans-serif}}
.wrap{{max-width:960px;margin:0 auto}}
h1{{font-size:22px;margin:0 0 6px}}
.副题,.提示,.空,.说明{{color:#5b6570;font-size:14px}}
.副题{{margin:0 0 18px}}
.查找{{margin:14px 0 20px}}
.查找框{{width:100%;padding:10px 12px;font-size:15px;border:1px solid #d3d8de;
border-radius:8px;background:#fff}}
.分组{{margin:0 0 10px;border:1px solid #e2e6ea;border-radius:10px;background:#fff}}
.组名{{cursor:pointer;padding:11px 14px;font-weight:600;font-size:16px}}
.计数{{color:#78828c;font-weight:400;font-size:13px;margin-left:6px}}
.对象{{margin:0 10px 8px;border:1px solid #e6eaee;border-radius:8px;background:#fcfdfe}}
.对象.重点{{border-color:#c8912f;background:#fffdf6}}
.对象>summary{{cursor:pointer;padding:10px 12px;font-size:15px}}
.名{{font-weight:600}}
.star{{margin-left:10px;color:#a9761a;font-size:13px}}
.体{{padding:2px 14px 14px}}
h4{{margin:16px 0 6px;font-size:14px;color:#39424c}}
.属性,.关系{{margin:0;padding:0;list-style:none}}
.属性 li{{padding:6px 0;border-top:1px solid #eef1f4}}
.属性 li:first-child{{border-top:none}}
.属性名{{font-weight:600;margin-right:10px}}
.属性说明{{color:#5b6570;font-size:13px}}
.样例{{color:#8a929b;font-size:12px;margin-left:8px}}
.未显示{{color:#8a6410;background:#fdf6e7;border:1px solid #e8d9b4;border-radius:6px;
padding:7px 10px;font-size:13px;margin:6px 0 0}}
.未显示汇总{{margin:14px 0 4px;padding:11px 14px;border:1px solid #e8d9b4;
border-left:4px solid #c8912f;border-radius:8px;background:#fffdf6}}
.未显示标题{{margin:0 0 6px;font-weight:600;font-size:14px;color:#6b5410}}
.未显示汇总 ul{{margin:0;padding-left:20px}}
.未显示汇总 li{{margin:3px 0;font-size:14px}}
.未显示脚{{margin:8px 0 0;font-size:13px;color:#5b6570}}
.关系 li{{padding:6px 0;display:flex;gap:10px;align-items:baseline;flex-wrap:wrap}}
.问{{flex:0 1 auto}}
.答{{color:#1b5e20;background:#eaf5ea;border-radius:6px;padding:1px 8px;font-size:13px}}
</style>
</head>
<body><div class="wrap">
<h1>业务对象总览</h1>
<p class="副题">这份清单把贵方系统里识别出来的业务对象列了出来。点开任意一个，可以看到它记录了什么、以及它和别的东西是什么关系。</p>
{body}
</div>
<script>
{script}
</script>
</body>
</html>
"#,
        body = body
    );

    skipped_props.sort();
    RenderedPage { html, skipped, skipped_props }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::view::{EntityView, PropertyView};

    fn sample_view() -> CustomerView {
        CustomerView {
            objects: vec![
                EntityView {
                    name: "客户".into(),
                    note: Some("贵方的客户主体".into()),
                    belongs_to: None,
                    aliases: vec![],
                    properties: vec![PropertyView {
                        name: "名称".into(),
                        note: Some("客户的简称".into()),
                        required: true,
                        sample: None,
                    }],
                },
                EntityView {
                    name: "API 调用日志".into(),
                    note: None,
                    belongs_to: None,
                    aliases: vec![],
                    properties: vec![],
                },
            ],
            relations: vec![RelationView {
                from: "API 调用日志".into(),
                to: "客户".into(),
                how_many: "多对一".into(),
                meaning: "归属于".into(),
            }],
        }
    }

    /// **验收主判据**：页面上不许出现任何建模术语。
    ///
    /// 变异：把 `render_html` 里的中文标签换回 `key`/`实体`/`N:1` 之类 ⇒ 立刻变红。
    #[test]
    fn test_html_has_no_modeling_terms() {
        let html = render_html(&sample_view(), &[]).html;
        for term in [
            "key", "实体", "基数", "外键", "主键", "证据", "快照", "版本号", "N:1", "entity",
            "property", "link", "cardinality", "ordinal", "evidence",
        ] {
            assert!(
                !html.contains(term),
                "页面上出现了建模术语「{term}」"
            );
        }
    }

    /// 关系必须按「A 有几个 B」的问法出现，且**两端方向相反**。
    ///
    /// 这条钉住任务说明点名的坑：`API 调用日志 多对一 客户`
    /// 在日志那一端是「1 个」，在客户那一端必须是「多个」。
    #[test]
    fn test_relation_uses_natural_question_per_end() {
        let v = sample_view();
        let html = render_html(&v, &[]).html;
        // 客户这一端 —— 判读人真正想问的那句
        assert!(
            html.contains("一个 客户，有几个 API 调用日志？"),
            "客户那一端应当显示「一个 客户，有几个 API 调用日志？」"
        );
        // 日志那一端 —— 方向相反，数量不同
        assert!(
            html.contains("一个 API 调用日志，有几个 客户？"),
            "日志那一端应当显示反向问句"
        );
        // 两端问句后面跟着的数量词必须不同
        assert!(html.contains("多个"));
    }

    /// 页面里绝不许出现「多对一」这种基数词 —— 它是 `how_many` 的原文，
    /// 是建模记号的另一种写法。问句已经把它讲完了。
    #[test]
    fn test_page_never_shows_cardinality_wording() {
        let html = render_html(&sample_view(), &[]).html;
        for w in ["多对一", "一对多", "多对多", "唯一对应"] {
            assert!(!html.contains(w), "页面出现了基数说法「{w}」");
        }
    }

    /// **REQ-009 · 阻断项 ① 的钉子**：★ 名单里**不许有技术设施**。
    ///
    /// 判读人是 3 名非开发者业务人员，★ 的职责是「这 13 个是要判读的主体对象」。
    /// `数据库迁移版本`（alembic 迁移表，唯一属性叫「版本数量」）、
    /// `鉴权缓存变更事件` / `鉴权缓存触发状态`（内部鉴权缓存的事件溯源设施）
    /// 占掉 3/13 的注意力，而业务人员**没有能力判断、也不需要判断**它们。
    ///
    /// 变异：把任一技术设施写回 [`DEFAULT_SAMPLING`] ⇒ 本测试立刻变红。
    #[test]
    fn test_sampling_list_has_no_technical_infrastructure() {
        for tech in [
            "数据库迁移版本",
            "鉴权缓存变更事件",
            "鉴权缓存触发状态",
            "鉴权缓存消息事件",
        ] {
            assert!(
                !DEFAULT_SAMPLING.contains(&tech),
                "技术设施「{tech}」不许占 ★ 名额 —— 它不是业务人员要判读的对象"
            );
        }
    }

    /// ★ 名单仍然是 13 个（手册 §3.1 的抽样规模是刻意定的，不是随手取的）。
    #[test]
    fn test_sampling_list_keeps_thirteen_objects() {
        assert_eq!(
            DEFAULT_SAMPLING.len(),
            13,
            "抽样规模变了要同步改判读手册 §3.1"
        );
    }

    /// 翻译表里**不许有**会被 `PAGE_FORBIDDEN_CJK` 整对象滤掉的名字（REQ-009 · ③）。
    ///
    /// 病根：`sub_account_bind_permission` 曾译成「子账号绑定权限快照」，
    /// 「快照」是**版本快照**的建模用语，于是这张**真业务表**
    /// （主账号↔子账号绑定关系）被整对象剔除，页面告知又不点名 ——
    /// 客户以为「子账号绑定」这件事根本不存在。
    ///
    /// 变异：把翻译表里的名字改回带「快照」二字 ⇒ 本测试立刻变红。
    #[test]
    fn test_no_translated_table_name_is_filtered_by_page_guard() {
        let json = include_str!("../extract/table_names.json");
        let tn: serde_json::Value = serde_json::from_str(json).expect("翻译表 JSON 合法");
        let names = tn["names"].as_object().expect("翻译表有 names 字段");
        for (table, cn) in names {
            let cn = cn.as_str().unwrap_or("");
            for term in PAGE_FORBIDDEN_CJK {
                assert!(
                    !cn.contains(term),
                    "表 {table} 的中文名「{cn}」含页面禁用词「{term}」，\
                     整个对象会被剔除 —— 请换一个不带建模用语的业务叫法"
                );
            }
        }
    }

    /// **翻译表是知识资产**：改翻译就必须改 `version`（`names.rs` 的维护约定）。
    ///
    /// 这条不是形式主义 —— diff 要能说清「是哪一次翻译变的」，
    /// 否则 `extracted_by` 契约（REQ-003 AC-011）就失效了。
    #[test]
    fn test_table_names_version_is_not_the_initial_one() {
        let json = include_str!("../extract/table_names.json");
        let tn: serde_json::Value = serde_json::from_str(json).expect("翻译表 JSON 合法");
        let version = tn["version"].as_str().expect("翻译表有 version");
        assert_ne!(
            version, "1.0.0",
            "翻译表改过却没有升 version —— diff 说不清是哪次翻译变的"
        );
    }

    /// 抽样清单里的对象必须被高亮，且清单外的不能被误标。
    #[test]
    fn test_sampling_list_highlights_exactly_its_own_objects() {
        let html = render_html(&sample_view(), &["客户".to_string()]).html;
        assert!(html.contains("本次重点查看"));
        // 「客户」是清单成员，「API 调用日志」不是 —— ★ 数应当只有 1 个
        assert_eq!(html.matches("本次重点查看").count(), 1, "★ 只应标在清单内的对象上");
    }

    /// 清单里的对象若视图里没有，**不许**凭空在页面上多出来。
    #[test]
    fn test_sampling_object_absent_from_view_is_not_invented() {
        let html = render_html(&sample_view(), &["并不存在的对象".to_string()]).html;
        assert!(!html.contains("并不存在的对象"));
    }

    /// 零外部依赖：不能有 `http`/`cdn`/外链字体/外部脚本。
    #[test]
    fn test_html_is_self_contained_offline() {
        let html = render_html(&sample_view(), &[]).html;
        for bad in ["http://", "https://", "//cdn", "src=\"http", "@import"] {
            assert!(!html.contains(bad), "页面引用了外部资源：{bad}");
        }
    }

    /// 数据里的尖括号必须转义 —— 否则源库注释能注入标签。
    #[test]
    fn test_html_escapes_untrusted_text() {
        let mut v = sample_view();
        v.objects[0].note = Some("<script>alert(1)</script>".into());
        let html = render_html(&v, &[]).html;
        assert!(!html.contains("<script>alert"));
        assert!(html.contains("&lt;script&gt;"));
    }

    /// 判别力自证：把「按人的问法」退化成「直接打印 how_many」必须变红。
    #[test]
    fn test_count_phrase_flips_with_end() {
        assert_eq!(count_phrase("多对一", true), "1 个");
        assert_eq!(count_phrase("多对一", false), "多个");
        assert_eq!(count_phrase("一对多", true), "多个");
        assert_eq!(count_phrase("一对多", false), "1 个");
        assert_eq!(count_phrase("唯一对应", true), "1 个");
        assert_eq!(count_phrase("多对多", true), "多个");
    }

    /// 名字里带建模术语的对象（真实库有 `子账号绑定权限快照`）
    /// **不许上页面**，但也**不许静默消失** —— 必须在 `skipped` 里报出来。
    ///
    /// 变异：把 `name_is_forbidden` 改成恒 false ⇒ 本测试立刻变红。
    #[test]
    fn test_object_with_modeling_term_is_skipped_but_reported() {
        let mut v = sample_view();
        v.objects.push(EntityView {
            name: "子账号绑定权限快照".into(),
            note: None,
            belongs_to: None,
            aliases: vec![],
            properties: vec![],
        });
        let out = render_html(&v, &[]);
        assert!(
            !out.html.contains("子账号绑定权限快照"),
            "带「快照」的对象名不许出现在页面上"
        );
        assert_eq!(
            out.skipped,
            vec!["子账号绑定权限快照".to_string()],
            "被剔除的对象必须报出来，不许静默"
        );
    }

    /// 剔除不该误伤正常业务名 —— 判据只认**建模术语**，
    /// 不是「名字里带某个字」。
    #[test]
    fn test_skip_does_not_swallow_normal_business_names() {
        let mut v = sample_view();
        v.objects.push(EntityView {
            name: "客户版本记录".into(),
            note: None,
            belongs_to: None,
            aliases: vec![],
            properties: vec![],
        });
        let out = render_html(&v, &[]);
        assert!(out.html.contains("客户版本记录"), "「版本」不是禁用词，不该被剔除");
        assert!(out.skipped.is_empty());
    }

    // ── 页面级清洗（真实源库实测带进来的技术串）─────────────────
    //
    // 抽取侧的注释门槛只判「有没有汉字」，于是下面这些**含汉字的技术串**
    // 原样进了客户视图。判读人是业务人员，这一层必须再挡一次。

    #[test]
    fn test_sanitize_strips_id_suffix() {
        for (raw, want) in [
            ("客户ID", "客户"),
            ("创建者ID", "创建者"),
            ("用户id", "用户"),
            ("权益id", "权益"),
            ("用户 ID", "用户"),
            ("主账号ID（冻结操作者）", "主账号（冻结操作者）"),
        ] {
            assert_eq!(
                sanitize_display(raw).as_deref(),
                Some(want),
                "{raw} 应摘掉 ID 记号而不是整条丢掉"
            );
        }
    }

    #[test]
    fn test_sanitize_drops_modeling_and_raw_identifiers() {
        // 含中文建模术语 / 含源库原始标识符 —— 没有任何办法翻成人话，整条丢
        for bad in [
            "用量快照",
            "价格快照",
            "核销人customer_id",
            "关联模型ID(对应llm_model.model_id)",
            "供应商配置(base_url, api_key等)",
            "ID",
        ] {
            assert_eq!(
                sanitize_display(bad),
                None,
                "{bad} 带技术用语，必须整条丢弃而不是原样上页面"
            );
        }
    }

    /// 反向：正常中文业务名**一个都不许**被误伤 ——
    /// 清洗过宽比不清洗更糟（会悄悄吃掉客户看得懂的属性）。
    #[test]
    fn test_sanitize_keeps_normal_chinese_names() {
        for ok in ["扣款币种", "名称", "状态", "过期时间", "重试数量", "AI 搜索", "操作人"] {
            assert_eq!(
                sanitize_display(ok).as_deref(),
                Some(ok),
                "{ok} 是正常业务名，不该被清洗掉"
            );
        }
    }

    /// 端到端：带 `ID` 的属性不许以原样出现在页面上。
    #[test]
    fn test_page_never_shows_id_suffix_in_property_names() {
        let mut v = sample_view();
        v.objects[0].properties.push(PropertyView {
            name: "客户ID".into(),
            note: None,
            required: false,
            sample: None,
        });
        let out = render_html(&v, &[]);
        assert!(!out.html.contains("客户ID"), "页面上不许出现「客户ID」这种带技术记号的名字");
        assert!(out.html.contains(">客户<"), "摘掉 ID 之后属性仍应当保留");
    }

    // ── REQ-007 · ①属性名重复 ────────────────────────────────
    //
    // 真实源库实测：598 条带说明属性里 **598 条说明 == 属性名**。
    // 判读人第一反应是「这工具坏了」，Q2 问的正是「它有哪几个属性？」。

    fn view_with_prop(name: &str, note: Option<&str>) -> CustomerView {
        let mut v = sample_view();
        v.objects.truncate(1);
        v.relations.clear();
        v.objects[0].properties = vec![PropertyView {
            name: name.into(),
            note: note.map(String::from),
            required: false,
            sample: None,
        }];
        v
    }

    /// 页面上渲染出来的「属性说明」span 个数。
    ///
    /// **为什么数 span 而不是数字符串**：属性名和说明是**同一个词**时，
    /// 数「页面上出现几次这个词」会被对象名、说明文字、样例一起带偏
    /// （`客户` 这个对象的属性也叫 `客户`）。数结构最稳。
    fn note_span_count(html: &str) -> usize {
        html.matches("<span class=\"属性说明\">").count()
    }

    /// **主判据**：说明与名字相同时，**只显示一个**。
    ///
    /// 变异：把 `note_is_redundant` 改成恒 false ⇒ 立刻变红。
    #[test]
    fn test_page_does_not_repeat_name_when_note_equals_name() {
        let out = render_html(&view_with_prop("用户", Some("用户")), &[]);
        assert_eq!(out.html.matches(">用户<").count(), 1, "名字本身应当出现一次");
        assert_eq!(
            note_span_count(&out.html),
            0,
            "说明与名字相同时不该再渲染第二个「属性说明」span"
        );
    }

    /// 比较的是**清洗后**的两串 —— `客户ID` / `客户 ID` 清洗后都是「客户」，
    /// 属于同一类重复。只比原始串会漏掉这一档。
    ///
    /// 判据数的是 `属性说明` span 的个数，**不是**页面上出现几次「客户」——
    /// 对象名本身也叫「客户」，用字符串计数会被它带偏（这坑踩过一次）。
    #[test]
    fn test_page_dedups_after_sanitizing_both_sides() {
        let out = render_html(&view_with_prop("客户ID", Some("客户 ID")), &[]);
        assert_eq!(
            note_span_count(&out.html),
            0,
            "名字与说明清洗后相同就只显示一个（ID 记号不该让它看起来是两句话）"
        );
        assert!(out.html.contains(">客户<"), "属性本身必须仍然留着");
    }

    /// **反向护栏**：说明**包含**名字时**不许**收敛掉 ——
    /// 多出来的那半句是信息，删掉它就是静默丢内容。
    #[test]
    fn test_page_keeps_note_that_merely_contains_name() {
        let out = render_html(&view_with_prop("状态", Some("状态（成功/失败）")), &[]);
        assert!(out.html.contains("状态（成功/失败）"), "说明里多出来的信息不许被当成重复删掉");
        assert_eq!(note_span_count(&out.html), 1, "包含不等于重复，说明必须照常显示");
    }

    /// 真的不一样时，两个都要显示。
    #[test]
    fn test_page_shows_both_when_note_is_genuinely_different() {
        let out = render_html(&view_with_prop("过期时间", Some("到期后额度失效")), &[]);
        assert!(out.html.contains("过期时间"));
        assert!(out.html.contains("到期后额度失效"));
        assert_eq!(note_span_count(&out.html), 1);
    }

    /// 判据本身的判别力：等于 → 冗余；包含 / 内容不同 → 不是。
    #[test]
    fn test_note_is_redundant_criterion_has_teeth() {
        assert!(note_is_redundant("用户", "用户"));
        assert!(note_is_redundant("用户", " 用户 "), "两端空白不算内容差异");
        assert!(!note_is_redundant("状态", "状态（成功/失败）"));
        assert!(!note_is_redundant("过期时间", "到期后额度失效"));
        assert!(!note_is_redundant("名称", "名称说明"), "多出来的字就是信息，不算重复");
    }

    // ── REQ-007 · ③剔除必须告知 ──────────────────────────────

    /// **主判据**：属性被剔除时，页面上**必须出现**少显示了多少。
    ///
    /// 静默剔除会让 Q2（属性识别率）结论反向：客户说「属性怎么这么少」，
    /// 我们会归因为「客户认不出」或「抽取能力不足」，而真相是「我们没给它看」。
    ///
    /// 变异：删掉那段 `未显示汇总` 拼接 ⇒ 立刻变红。
    #[test]
    fn test_page_tells_judge_how_many_props_were_withheld() {
        let mut v = view_with_prop("名称", None);
        // 两个带技术用语的名字：含建模术语 / 含源库原始标识符
        v.objects[0].properties.push(PropertyView {
            name: "用量快照".into(),
            note: None,
            required: false,
            sample: None,
        });
        v.objects[0].properties.push(PropertyView {
            name: "核销人customer_id".into(),
            note: None,
            required: false,
            sample: None,
        });
        let out = render_html(&v, &[]);
        assert!(out.html.contains("另有 2 项"), "页面上必须写明本对象少显示了几项：{}", out.html);
        // 汇总里也要出现
        assert!(out.html.contains("另有 2 项因为名称带系统内部的技术叫法"));
    }

    /// **绝不许把被剔掉的技术名搬到说明里** —— 那是把泄漏换了个位置。
    #[test]
    fn test_withheld_notice_never_prints_the_technical_names() {
        let mut v = view_with_prop("名称", None);
        for bad in ["用量快照", "核销人customer_id", "关联模型ID(对应llm_model.model_id)"] {
            v.objects[0].properties.push(PropertyView {
                name: bad.into(),
                note: None,
                required: false,
                sample: None,
            });
        }
        let out = render_html(&v, &[]);
        for bad in ["用量快照", "customer_id", "llm_model", "model_id", "快照"] {
            assert!(
                !out.html.contains(bad),
                "被剔除的技术名「{bad}」被印到页面上了 —— 换个位置漏出去不算修好"
            );
        }
        // 但数量必须仍然如实报出
        assert!(out.html.contains("另有 3 项"), "报数量不能被「不许报名字」误伤");
    }

    /// 剔除提示本身**不许带建模术语** —— 页头正文会被 `scan_modeling_terms`
    /// 全量扫（`cli.rs` fail-closed），也过不了 `verify-page.js` 的黑名单。
    #[test]
    fn test_withheld_notice_wording_is_terminology_free() {
        let mut v = sample_view();
        v.objects.push(EntityView {
            name: "子账号绑定权限快照".into(),
            note: None,
            belongs_to: None,
            aliases: vec![],
            properties: vec![],
        });
        v.objects[0].properties.push(PropertyView {
            name: "用量快照".into(),
            note: None,
            required: false,
            sample: None,
        });
        let out = render_html(&v, &[]);
        // 只截取我们自己加的提示段来查（对象名「子账号绑定权限快照」是测试数据）
        let notice = out
            .html
            .split("<div class=\"未显示汇总\">")
            .nth(1)
            .expect("页面上必须有剔除提示段")
            .split("</div>")
            .next()
            .unwrap();
        for bad in ["建模", "字段", "属性表", "外键", "主键", "基数", "证据", "实体", "剔除", "过滤", "非法"] {
            assert!(!notice.contains(bad), "剔除提示里出现了不该给业务人员看的词「{bad}」");
        }
        assert!(
            crate::domain::view::scan_modeling_terms(notice).is_empty(),
            "剔除提示自身触发了建模术语守门：{:?}",
            crate::domain::view::scan_modeling_terms(notice)
        );
    }

    /// 一个都没剔时**不许**出现提示段 —— 无中生有会让判读人多问一句「什么没显示？」。
    ///
    /// 判据查的是 `未显示汇总` 这个**容器**而不是「未显示」二字：
    /// `.未显示` 同时是 CSS 类名，在 `<style>` 里必然出现。
    #[test]
    fn test_no_notice_when_nothing_was_withheld() {
        let out = render_html(&sample_view(), &[]);
        assert!(
            !out.html.contains("<div class=\"未显示汇总\">"),
            "没有剔除时不该出现剔除提示段"
        );
    }
}

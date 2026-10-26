//! REQ-156 · `semantic_cn` 的**专属不变式**（红线例外的第二道锁）。
//!
//! # 为什么有这份文件
//!
//! `tests/test_req107_red_lines.rs` 把 `src/domain/view.rs` 列为整文件禁改的红线。
//! REQ-156 必须在这份文件里**追加** 9 种真实动词的中文映射（②），于是 ① 的判据
//! 被点名窄化成「只允许追加」。但「只允许追加」这句话本身不能靠人看 diff ——
//! 本文件把「追加了什么、没改什么」变成四条机器判据：
//!
//! 1. **原有 6 条映射逐字未变** —— 防「顺手改语义」；
//! 2. **兜底仍是 `_ => s.to_string()`** —— 这是 `untranslated_semantic_hit` 的前提；
//! 3. **每个分支的输出都不含 ASCII 字母** —— AC-002 的机器形态；
//! 4. **`untranslated_semantic_hit` 的判据未被改写** —— 防线本身。
//!
//! ⚠️ 用 `include_str!` 读**源码**做结构断言（不是逐字全文比对，那太脆）。
//! 变异自证：把 `belongs_to` 改成「所属」⇒ 判据 1 红；把 `_ =>` 改成
//! `_ => "未知"` ⇒ 判据 2 红。

/// 被保护的红线文件源码（`include_str!` 保证读到的就是编译进测试的同一份）。
const VIEW_SRC: &str = include_str!("../src/domain/view.rs");

/// 取一个函数的**函数体**（从函数签名行到与之配对的第一个 `\n}`）。
///
/// 本文件只用来取 `semantic_cn` / `untranslated_semantic_hit` —— 两者的函数体里
/// 没有嵌套的 `}` 顶格行（match 臂都以逗号结尾，闭包在行内），所以「第一个顶格
/// `}`」就是函数体终点。锚点缺失时 panic（⛔ 返回空串会让判据恒绿）。
fn body_of<'a>(src: &'a str, signature: &str) -> &'a str {
    let start = src
        .find(signature)
        .unwrap_or_else(|| panic!("源码里找不到 `{signature}` —— 函数被改名/挪走了？"));
    let rest = &src[start..];
    let end = rest
        .find("\n}")
        .unwrap_or_else(|| panic!("`{signature}` 找不到函数体终点"));
    &rest[..end]
}

/// `=>` 右侧的**第一个字符串字面量**（match arm 的输出）。
fn rhs_literal(arm: &str) -> Option<&str> {
    let (_, rhs) = arm.split_once("=>")?;
    let open = rhs.find('"')?;
    let rest = &rhs[open + 1..];
    let close = rest.find('"')?;
    Some(&rest[..close])
}

/// 取 `semantic_cn` 里**每一个 match arm**（以 `=>` 结尾、非兜底）。
fn semantic_cn_arms() -> Vec<&'static str> {
    body_of(VIEW_SRC, "fn semantic_cn(s: &str) -> String {")
        .lines()
        .map(str::trim)
        .filter(|l| l.contains("=>") && !l.contains("_ =>"))
        .collect()
}

#[test]
fn semantic_cn_keeps_the_six_original_mappings_verbatim() {
    let body = body_of(VIEW_SRC, "fn semantic_cn(s: &str) -> String {");
    // 逐字：连 `=>` 与 `.to_string()` 的写法都不许变（改了一行就是改语义的信号）。
    for (key, label) in [
        ("has", "拥有"),
        ("belongs_to", "归属于"),
        ("depends_on", "依赖"),
        ("references", "关联到"),
        ("records", "记录了"),
        ("derived_from", "算自"),
    ] {
        let want = format!("\"{key}\" => \"{label}\".to_string(),");
        assert!(
            body.contains(&want),
            "REQ-156 之前的原有映射被改动了：找不到 `{want}`\n\
             ⛔ 只允许**追加**新动词；改既有语义会让所有客户的 `meaning` 悄悄变意思。"
        );
    }
}

#[test]
fn semantic_cn_keeps_the_raw_fallback_for_the_guard() {
    let body = body_of(VIEW_SRC, "fn semantic_cn(s: &str) -> String {");
    assert!(
        body.contains("_ => s.to_string(),"),
        "`semantic_cn` 的兜底不再是 `_ => s.to_string()` —— \
         `untranslated_semantic_hit` 正是靠「兜底原样返回英文」来判漏翻。\n\
         ⛔ 改成任何编出来的中文（如 `_ => \"未知\"`）都等于把防线关掉。"
    );
    // 兜底只能有一个（多处 `_ =>` 说明 match 被改成了别的形状）。
    assert_eq!(
        body.matches("_ =>").count(),
        1,
        "`semantic_cn` 里出现了多个 `_ =>`：\n{body}"
    );
}

#[test]
fn every_semantic_cn_arm_output_is_chinese_without_ascii_letters() {
    // AC-002 的判据与 `untranslated_semantic_hit` **同一条**：出现 ASCII 字母就不是中文标签。
    let arms = semantic_cn_arms();
    assert!(
        arms.len() >= 17,
        "match arm 只剩 {} 个 —— 映射被删了？",
        arms.len()
    );
    for arm in &arms {
        let out = rhs_literal(arm)
            .unwrap_or_else(|| panic!("match arm 的 `=>` 右边没有字符串字面量：{arm}"));
        assert!(
            !out.chars().any(|c| c.is_ascii_alphabetic()),
            "语义分支输出了 ASCII 字母（等于把英文 key 当标签）：`{arm}`"
        );
        assert!(!out.trim().is_empty(), "语义分支输出空标签：`{arm}`");
    }
}

#[test]
fn real_verb_table_is_covered_and_untranslated_semantic_hit_is_unchanged() {
    // 15 种真实动词（含两对同义）→ 19 个 key，全部要有分支。
    for (key, label) in [
        ("belongs_to", "归属于"),
        ("references", "关联到"),
        ("binds_to", "绑定到"),
        ("binds", "绑定到"),
        ("attached_to", "挂载于"),
        ("attaches_to", "挂载于"),
        ("notifies_via", "通知到"),
        ("applies_to", "适用于"),
        ("targets", "指向"),
        ("uses", "使用"),
        ("contains", "包含"),
        ("associated_with", "关联"),
        ("bills", "计费到"),
        ("based_on", "基于"),
        ("deployed_to", "部署于"),
        ("has", "拥有"),
        ("depends_on", "依赖"),
        ("records", "记录了"),
        ("derived_from", "算自"),
    ] {
        let needle_key = format!("\"{key}\"");
        let needle_label = format!("\"{label}\"");
        assert!(
            semantic_cn_arms()
                .iter()
                .any(|arm| arm.contains(&needle_key) && arm.contains(&needle_label)),
            "缺少 `{key}` → `{label}` 的分支（REQ-156 的 15 种真实动词映射）"
        );
    }

    // 第四档防线本身：判据仍是「meaning 里出现任何 ASCII 字母就抓」。
    let guard = body_of(
        VIEW_SRC,
        "fn untranslated_semantic_hit(json: &str) -> Option<String> {",
    );
    assert!(
        guard.contains("meaning_values(json)"),
        "`untranslated_semantic_hit` 不再扫 `meaning_values` —— 防线被改写了：\n{guard}"
    );
    assert!(
        guard.contains("is_ascii_alphabetic()"),
        "`untranslated_semantic_hit` 不再按 ASCII 字母判定 —— 防线被改写了：\n{guard}"
    );

    // `semantic_label` 也必须继续用同一条形态判据（认不出 ⇒ None，⛔ 不编中文）。
    let label_fn = body_of(
        VIEW_SRC,
        "pub fn semantic_label(s: &str) -> Option<String> {",
    );
    assert!(
        label_fn.contains("is_ascii_alphabetic()"),
        "`semantic_label` 不再拒绝含 ASCII 的「标签」—— 未知动词会被伪装成中文：\n{label_fn}"
    );
}

/// AC-002 的**端到端形态**：把 15 个真实动词的中文标签灌进客户视图的 `meaning`，
/// 让真正的第四档守门器跑一遍 —— 必须零命中。
#[test]
fn translated_semantic_values_pass_the_real_guard() {
    let labels = [
        "归属于",
        "关联到",
        "绑定到",
        "挂载于",
        "通知到",
        "适用于",
        "指向",
        "使用",
        "包含",
        "关联",
        "计费到",
        "基于",
        "部署于",
        "拥有",
        "依赖",
        "记录了",
        "算自",
    ];
    let relations: Vec<String> = labels
        .iter()
        .map(|l| format!(r#"{{"from":"A","to":"B","how_many":"多对一","meaning":"{l}"}}"#))
        .collect();
    let view = format!(r#"{{"objects":[],"relations":[{}]}}"#, relations.join(","));
    let hits = ontok::domain::view::scan_modeling_terms(&view);
    assert!(
        hits.is_empty(),
        "已翻成中文的语义被守门器误杀了：{hits:?}\n{view}"
    );

    // 反向：把任一标签换回英文 key，守门器必须抓到（判据不是恒绿）。
    let leaked = r#"{"objects":[],"relations":[{"from":"A","to":"B","how_many":"多对一","meaning":"belongs_to"}]}"#;
    let hits = ontok::domain::view::scan_modeling_terms(leaked);
    assert!(
        hits.iter().any(|h| h.contains("未翻译的关系语义")),
        "第四档守门器没抓住英文 `meaning` —— AC-002 的判据是空的：{hits:?}"
    );
}

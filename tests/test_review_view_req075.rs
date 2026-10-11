//! REQ-075 · **守卫的守卫** —— 让「守卫被静默删除 / 被悄悄摘掉」不再**无痕**。
//!
//! @guard-manifest: REQ-081
//!
//! 测试名带 `test_req075_` 前缀。
//!
//! # 本条为什么单开
//!
//! REQ-070 的 QA 实测（决定性实验 **M1′**，触发形状全语料外）：
//!
//! | | lib（行为层） | req070 |
//! | --- | --- | --- |
//! | 原样 | 🔴 117/1 | 🔴 5/1 |
//! | **M1′** | 🟢 **118/0** | 🔴 5/1（**只剩结构守卫**） |
//!
//! ⇒ **真正与语料无关的杀手只剩结构守卫一条**。而它自陈：
//! 是**形状探针**、**抓不到别名绕过**、**删掉就没人查**。
//! REQ-068 的守卫同样自陈抓不到别名绕过 —— **两条需求的护栏
//! 都建立在「守卫会被维护」这个未被检查的假设上。**
//!
//! # 本文件只做两件事
//!
//! | AC | 做什么 |
//! | --- | --- |
//! | **AC-001** | 断言「两条守卫文件**还在**，而且**里面那些守卫测试还在**」 |
//! | **AC-004** | 断言守卫文档**仍如实标注**「抓不到别名绕过」 |
//!
//! AC-002 / AC-003 的实现在 `tests/test_review_view_req070.rs`
//! （删掉字面量判别力 + 直接调用生产的 `label()`），不在本文件。
//!
//! # ⚠️⚠️ **本文件不是封堵，只是「计数变化可见」**（必读，别读成别的）
//!
//! | 事实 | 后果 |
//! | --- | --- |
//! | **本文件自己也能被删** | 删掉它 ⇒ 下面两条 `include_str!` 的**编译错误一起消失** ⇒ 守卫的删除重新变成**无痕** |
//! | **本文件自己也能被改空** | 把断言改成恒真（`assert!(true)`）⇒ 与删掉等价 |
//!
//! ⇒ 它只把「静默删除守卫」从**无痕**变成「**编译报错 / 计数变化**」。
//! **这不是封堵，也不声称是。** 任何一层「守卫的守卫」都会遇到同一个递归问题：
//! 再往上一层，它自己也可以被删。**递归到此为止 —— 这里就是最后一层。**
//!
//! # 为什么用 `include_str!` 而不是运行时读路径
//!
//! `include_str!` 给出比「存在性」**更强**的一格：文件被删 ⇒
//! **本二进制编译失败**（E0463），而不是「运行到那条断言才发现」。
//! ⚠️ 代价要说清：编译失败会**同时**让本二进制里的**全部**测试没法跑 ——
//! 这是**故意**的（让删除在最早的时刻、最响的位置暴露），
//! 但它确实比「一条断言变红」影响面更大。
//!
//! # 为什么断言「测试**名字**」而不是「测试**条数**」
//!
//! 只数条数的话，改个名字就绕过去了（条数不变）。
//! 名字是**逐条**钉的 ⇒ 少一条、改一条、改名一条，三种都会红。
//! ⚠️ 反过来，**故意新增**一条守卫测试时，这里也要跟着加一行 ——
//! 这是**有意的成本**：新增守卫必须在这里**留痕**，而不是悄悄加。

use ontok::domain::review_view::DEGENERATE_EVIDENCE_MARK;

// ══════════════════════════════════════════════════════════
// 清点表（唯一的「期望值」来源）
// ══════════════════════════════════════════════════════════

/// REQ-068 的守卫文件 —— REQ-075 的 `include_str!` 是它**存在**的证明。
const REQ068: &str = include_str!("test_review_view_req068.rs");

/// REQ-070 的守卫文件 —— 同上。
const REQ070: &str = include_str!("test_review_view_req070.rs");

/// **下一层**（REQ-081）的守卫文件 —— REQ-081 是**本文件自己**的对称那一层。
///
/// # 为什么要加这一行（AC-001 的递归问题）
///
/// 上面的两条 `include_str!` 只能抓「REQ-068 / REQ-070 被删」。
/// **删掉本文件**时，这两条的编译错误会**跟着一起消失** ⇒ 守卫的删除重新变成无痕
/// （模块注释里已登记的递归终点）。
///
/// ⇒ 加这一行之后，删掉本文件 ⇒ **REQ-081 的二进制编译失败**；
/// 反过来 REQ-081 那边也 `include_str!` 着本文件 ⇒ 删掉它 ⇒ 本二进制编译失败。
/// ⇒ **两层互钉**。⚠️ 但**两层一起删**仍然 exit 0 —— **递归到此为止，本仓不声称封堵。**
const REQ081: &str = include_str!("test_guard_manifest_req081.rs");

/// 每条守卫文件里**必须还在**的测试名。
///
/// ⚠️ 这是**故意写死的清单**：新增守卫测试必须在这里登记（见模块注释末段）。
/// 它抓的是「删一条 / 改一条 / 改名一条」三种静默退化。
const INVENTORY: [(&str, &[&str]); 2] = [
    (
        "test_review_view_req068.rs",
        &[
            "test_req068_agent_name_carrying_track_prefix_is_not_a_false_positive",
            "test_req068_glued_track_line_after_provenance_is_still_flagged",
            "test_req068_non_item_line_mentioning_track_prefix_before_provenance_is_not_a_false_positive",
            "test_req068_pure_tightening_sample_counts",
            "test_req068_no_second_contains_based_prefix_recogniser",
            "test_req068_head_only_recognition_behaviourally",
            "test_req068_all_four_provenance_states_still_recognised",
            "test_req068_indented_provenance_line_is_still_recognised",
            // REQ-074 新增：`:1046`（找「第一条条目行」那一格）的**语义兜底**。
            // 它之前只有「一条文本守卫 + 一句手写构造样本」两处 ⇒ **同时**改掉
            // 那两处会静默退化（实测 PRE-M3 = 全绿）。载荷行由**真实渲染器**产出，
            // 所以守卫模式与那句构造样本都动不到它。
            "test_req074_first_item_detection_ignores_every_payload_derived_non_head_line",
            // REQ-074 新增：**反向变异** —— `:1046` 那一格过去**从未**被断言过会开火
            // （整格删掉 / 恒返回 `None` 全绿，实测 PRE-M6 = 68/0）。
            "test_req074_provenance_after_first_item_line_is_still_flagged",
        ],
    ),
    (
        "test_review_view_req070.rs",
        &[
            "test_req070_degenerate_label_carries_payload_regardless_of_corpus",
            "test_req070_sealing_never_breaks_the_degenerate_mark_structure",
            "test_req070_label_has_no_second_return_path",
            // REQ-075 新增：判别力脱离源码字面量的那条**行为扫描**。
            "test_req070_every_degeneracy_label_carries_the_payload_of_its_value",
            "test_req070_no_return_path_bypasses_the_degeneracy_dispatch",
            "test_req070_the_seal_is_still_wired_into_the_only_render_call_site",
            // REQ-077 新增：封条**逐条**生效（一次喂多条 evidence）。
            // 那一格在 REQ-076 交付件里**完全不可观测**（QA 的 M2prime2 实测 192/0 全绿）。
            "test_req070_the_seal_applies_to_every_evidence_entry_not_only_the_first",
            "test_req070_module_doc_never_states_payload_carrying_as_an_absolute",
        ],
    ),
];

/// 取守卫文件的文本（按文件名）。
fn guard_src(file: &str) -> &'static str {
    match file {
        "test_review_view_req068.rs" => REQ068,
        "test_review_view_req070.rs" => REQ070,
        other => panic!("清点表里出现了没有登记的文件：{other}"),
    }
}

// ══════════════════════════════════════════════════════════
// AC-001 · 守卫的**存在性**必须可检测
// ══════════════════════════════════════════════════════════

/// 【AC-001】两条守卫文件**还在**，且里面**每一条守卫测试还在**。
///
/// # 它抓的是
///
/// - 删掉**整条**守卫测试（QA 的 **M1**）⇒ 本条报红；
/// - 改掉守卫测试的**名字**（⇒ 那条断言报红）；
/// - 删掉**整个守卫文件** ⇒ 上面两条 `include_str!` **编译失败**（E0463）。
///
/// # ⚠️ 前提自检（不许省）：本条自己**不是恒真**
///
/// 下面先断言「清单里的名字确实能在**真实文本**里逐条点到」，再断言「它们都在」；
/// 若 `INVENTORY` 与文件脱节（改名忘了登记），会**先**在这里报红，
/// 而不是让下面那条在空跑的路上假装通过。
#[test]
fn test_req075_guard_files_still_carry_every_guard_test_in_the_inventory() {
    let mut missing: Vec<String> = Vec::new();
    let mut present = 0usize;

    for (file, names) in INVENTORY {
        let src = guard_src(file);
        // 前提自检：文件**有内容**。空文件会让下面整条循环零次跑过。
        assert!(
            src.len() > 500,
            "{file} 只剩 {} 字节 ⇒ 它被清空了，「守卫还在」这件事必须重新确认",
            src.len()
        );
        for name in names {
            let needle = format!("fn {name}()");
            if src.contains(&needle) {
                present += 1;
            } else {
                missing.push(format!("{file}: `fn {name}()`"));
            }
        }
    }

    let expected: usize = INVENTORY.iter().map(|(_, n)| n.len()).sum();
    assert!(
        present > 0,
        "一条守卫测试都没点到 ⇒ 下面的断言在一条空跑的路上"
    );
    assert_eq!(
        present, expected,
        "守卫测试**少了或改了名**（点到 {present} / 清点 {expected} 条）——\n\
         少了这些，REQ-068 / REQ-070 的护栏就只剩一个**没人检查的假设**：\n  - {}",
        missing.join("\n  - ")
    );
}

/// 【AC-001 · 配套】`#[test]` 的**条数**必须与清点表**一致**（防「加了但没登记」）。
///
/// # 为什么要这一条
///
/// 上一条只查「清单里的**还在**」，查不出「**多了**一条没登记的守卫」。
/// 而 REQ-075 的整个前提是「护栏不该在没人知情的情况下变动」——
/// **加**一条不被记下的守卫，同样是护栏在无痕变动（只是方向相反）。
///
/// ⚠️ 它的失败模式是**假阳性**（有人加了守卫忘了登记）⇒ 提示写明是「请回来登记」，
/// 不是「守卫坏了」。
#[test]
fn test_req075_guard_test_counts_match_the_inventory() {
    let mut rows: Vec<String> = Vec::new();
    let mut ok = true;
    for (file, names) in INVENTORY {
        let src = guard_src(file);
        // ⚠️ 数的是 `#[test]` 属性本身，不是注释里提到的那三个字。
        //   （守卫文件的注释占比很高 —— REQ-065 的 QA 实测过 56% ——
        //     数错了就是恒假。）
        let attrs = src.lines().filter(|l| l.trim() == "#[test]").count();
        if attrs != names.len() {
            ok = false;
            rows.push(format!(
                "{file}: 文件里有 {attrs} 个 `#[test]`，清点表登记了 {} 条 —— \
                 请回来登记（这通常意味着**刚新增了一条守卫测试**）",
                names.len()
            ));
        }
    }
    assert!(ok, "守卫测试条数与清点表对不上：\n  - {}", rows.join("\n  - "));
}

/// 【AC-004 · 复发守卫】守卫文档**必须仍然如实标注**「抓不到别名绕过」。
///
/// # 为什么盯注释
///
/// QA 在 REQ-065 判过一句很重的话：「**报告会被跳过，注释不会。**」
/// AC-004 明令：「M2 / M3 允许绿，但**必须如实标注** ——
/// **不许写『本条已封堵别名绕过』」。那么「标注还在不在」就该有东西盯着。
///
/// ⚠️ 它盯的是**具体几串**标记，不是「不许有强断言」（后者没法执行）。
/// 标记被删 ⇒ 本条红，提示是「把边界写回去」。
#[test]
fn test_req075_guard_docs_still_mark_the_alias_boundary_as_unclosed() {
    // 每条守卫文件**必须**在位的那几串（照 REQ-070 的既有做法：钉具体措辞）。
    const REQUIRED: [(&str, [&str; 2]); 2] = [
        ("test_review_view_req068.rs", ["不声称封堵别名绕过", "仍然绿"]),
        ("test_review_view_req070.rs", ["不声称", "允许绿"]),
    ];
    // **禁掉**的过度声明：这两串一旦出现，就是 QA 两次判过的过度声明。
    const BANNED: [&str; 3] = ["本条已封堵别名绕过", "已彻底封堵别名绕过", "别名绕过全部封堵"];

    let mut missing: Vec<String> = Vec::new();
    let mut overclaimed: Vec<String> = Vec::new();
    for (file, marks) in REQUIRED {
        let src = guard_src(file);
        for m in marks {
            if !src.contains(m) {
                missing.push(format!("{file}: 缺 {m:?}"));
            }
        }
        for b in BANNED {
            if src.contains(b) {
                overclaimed.push(format!("{file}: 出现了 {b:?}"));
            }
        }
    }

    assert!(
        missing.is_empty(),
        "守卫文档里的**如实标注**不见了 —— AC-004 要求 M2 / M3「允许绿但必须如实标注」：\n  - {}\n\
         ⚠️ 没有标注 ≠ 问题变小，只等于**没人知道缺口还在**。",
        missing.join("\n  - ")
    );
    assert!(
        overclaimed.is_empty(),
        "守卫文档里出现了**过度声明**（QA 在 REQ-068 / REQ-070 上两次判过这类说法）：\n  - {}",
        overclaimed.join("\n  - ")
    );
}

/// 【AC-001 · 反向自查】本文件**自己**得处在它所声称的位置上（防止搬错目录 / 读成别的文件）。
///
/// # 为什么需要这条
///
/// 本文件的两条 `include_str!` 是**相对本文件**解析的。如果哪天有人把本文件
/// 复制一份到别处、或把 `include_str!` 指向别的东西，「元测试」就会检查一个
/// **并非真正在跑的那份**守卫 —— 而它照样全绿。
///
/// ⇒ 把「本文件确实在读那两条守卫的真实内容」钉成**可执行断言**：
/// 读到的文本里必须**逐字**含有那条守卫自己的模块文档首行，
/// 且本文件里那两个标记串**必须真的来自读到的文本**。
#[test]
fn test_req075_this_meta_test_really_reads_the_guard_files() {
    assert!(
        REQ070.contains("//! REQ-070 · **退化标签的载荷保护不得依赖「触发形状恰好在语料里」**。"),
        "读到的 REQ-070 文本首行不对 ⇒ [`include_str!`] 指到了别的东西，本文件在检查一份**没在跑**的守卫"
    );
    assert!(
        REQ068.contains("//! REQ-068 · **两轨前缀识别里剩下的两处 `contains`**"),
        "读到的 REQ-068 文本首行不对 ⇒ [`include_str!`] 指到了别的东西，本文件在检查一份**没在跑**的守卫"
    );
    // 前两段里那个「已登记的已知边界」标记必须**确实来自读到的文本**，
    // 而不是本文件自己写了一份 ⇒ 否则就是「自己给自己发证」。
    assert!(
        REQ068.contains("不声称封堵别名绕过"),
        "读到的 REQ-068 文本里没有那条边界标记 ⇒ 上一条的检查对象不对"
    );
    // ⚠️ **刻意不查**「REQ-070 里没有那条被 M9 证伪的字面量断言」。
    // 那会是一条**新的文本探针**，而它第一步就被自己的注释绊倒：
    // 「解释删掉了什么」的那段注释里必然**引用**了那句被删的代码
    // （本文件第一版就是这么自己把自己判红的）。
    // ⇒ AC-002 的删除**由变异实验证明**（报告 §M9：影子变量 + 逐字保留字面量 ⇒ req070 红），
    // 证据强度远高于「文本里搜不到某串字」。
    // 常量 import 证明本文件确实链接到了生产 crate（不是纯文本游戏）。
    assert_eq!(
        DEGENERATE_EVIDENCE_MARK,
        '\u{2400}',
        "生产 crate 的定界符常量变了 ⇒ REQ-070 的退化标签判定要重新确认"
    );
}

/// 【REQ-081 · AC-001】**下一层**守卫（`test_guard_manifest_req081.rs`）必须**还接着**。
///
/// # 它补的是本文件自己登记过的那一格
///
/// 模块注释写明：「本文件自己也能被删 ⇒ 下面两条 `include_str!` 的编译错误一起消失
/// ⇒ 守卫的删除重新变成**无痕**」。
///
/// ⇒ 加了 [`REQ081`] 这一条之后：**删掉本文件 ⇒ 本二进制编译失败**（不是「一条红」）。
/// REQ-081 那边对称地 `include_str!` 着本文件 ⇒ 两层**互钉**。
///
/// # ⚠️ 边界（⛔ 不许读成「递归解决了」）
///
/// **两层一起删** ⇒ 两处 `include_str!` 一起消失 ⇒ exit 0，**没有一条测试会红**。
/// 那一格**原理上封不住**，本仓不声称封堵；它只由「退出码非 0 的约定」兜。
///
/// # 它抓的是「锚点指错了」，不是「锚点在不在」
///
/// 锚点**存在**由 `include_str!` 在**编译期**保证；这条保证的是
/// 「读到的那份确实是**正在跑的那份**」——照本文件
/// `test_req075_this_meta_test_really_reads_the_guard_files` 的既有做法。
#[test]
fn test_req075_the_next_guard_layer_is_still_attached_to_this_one() {
    // 前提自检：锚点**不是空的**。空文件会让下面的 `contains` 在空跑的路上假装通过。
    assert!(
        REQ081.len() > 500,
        "下一层守卫文件只剩 {} 字节 ⇒ 它被清空了，「下一层还接着」必须重新确认",
        REQ081.len()
    );
    assert!(
        REQ081.contains("//! REQ-081 · **守卫的守卫 · 第二层**"),
        "读到的 REQ-081 文本首行不对 ⇒ [`include_str!`] 指到了别的东西，\
         本层在钉一份**没在跑**的守卫"
    );
    // 互钉的那一格必须真的在位：下一层必须也指着本文件。
    assert!(
        REQ081.contains("include_str!(\"test_review_view_req075.rs\")"),
        "🔴 REQ-081 里没有指回本文件的 `include_str!` ⇒ **删掉本文件会重新变成无痕**"
    );
}
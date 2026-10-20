//! REQ-104 · 客户视图**属性级剔除**的判据。
//!
//! ## 这份测试守的是什么
//!
//! REQ-104 给三个出口（JSON / `/api/node` / MCP）接了一道属性级剔除，
//! 目的是让 `demo3` 第 2 版那 3 个点不开的节点打得开。
//!
//! **危险只有一种**：这道剔除如果**顺手把真·建模术语也吃掉**，
//! 那就不是「解阻」，是**把发布闸门拆了** —— 而且拆得静悄悄：
//! 节点能打开了，守门器却再也看不见那些词。
//!
//! 所以本文件的第一条测试是**否定式**的：
//! 构造真的含建模术语的视图，过完剔除之后**必须仍然到达守门器、仍然被拒**。
//!
//! ## 判据跑在哪一层
//!
//! - 行为判据（本文件主体）**编译进测试二进制** —— 变异源码后重新编译，
//!   二进制 sha256 会变，取二进制 sha256 作证据。
//! - `all_three_outlets_call_the_one_shared_function` 是**源码扫描型守卫**
//!   （运行时读工作区文件），它的变异证据要取**被扫文件的工作区 sha256**，
//!   不是二进制 sha256 —— 理由见 `tests/test_graph_server.rs` 模块头。
//!
//! ⚠️ **REQ-107 改写了它**（AC §七 A3：改写、不许删）：收口后三个出口**不许**
//! 自己调判据，判据只在 `crate::domain::pipeline` 里执行一次。⇒ 判据从
//! 「三个文件都 contains 判据名」改成**否定式**「三个文件的判据直调点数各为 0，
//! 且都必须有管线调用点」。**放宽了吗？没有** —— 旧判据在 M3 形态下全绿
//! （本模块 `property_filter.rs` 的 ③ 段登记过），新判据在 M3 形态下必红。
//!
//! ## ⛔ 本文件的合成夹具**不得**出现真实客户标识符
//!
//! 本文件第一版把 `demo3` 第 2 版的**真实列注释原文**当夹具写了进去
//! （含真实表 key 与真实列名）⇒ 脱敏扫描判为**新暴露**（推送前被拦下）。
//!
//! **约定**（本文件内新增夹具一律遵守）：
//!
//! - 客户专有的标识符 —— 表名、列名、配置项名 —— 一律用 **`wg_` 前缀**的合成 token；
//! - 对象名用 `样例…` 开头；
//! - `pk` / `key` / `Key` / `CDC` / `Kafka` 等**通用技术词汇**可以保留：
//!   它们不是客户资产，而且 `key` **就是**被测的建模术语字面量，换掉测试就废了。
//!
//! ### 为什么这里**没有**一条自动判据（BA 已拍板待定，先写清理由）
//!
//! 能真正判「有没有撞上真实值」的那份词表在
//! `src/extract/table_names.json` —— 它是 **skip-worktree + gitignore 的本地资产，
//! 不入库、CI 上不存在**。据此写测试只有两种结局：
//!
//! - fail-closed（缺文件即失败）⇒ **CI 上永远红**，且与 `drop_rate.rs` 一样
//!   要求每个执行者手工准备资产；
//! - 缺文件就跳过 ⇒ **恒过的空门禁**，正是本项目栽过六次的那类假绿。
//!
//! ⇒ 本轮**不加**这类判据，改为**约定 + 注释**。
//! 真要机器化，正确落点是既有的 REQ-084 登记层
//! （`tests/test_retained_names_req084.rs` 的 `sources()` + `retained_names.req084.tsv`），
//! 但那要改一个带计数下界与保留项裁定的**受守文件**，且需为本文件登记保留项
//! —— 超出「顺手加一道轻量检查」的范围，留给 BA 决定是否单独立项。

mod common;

use ontok::domain::property_filter::{filter_view, DroppedProp};
use ontok::domain::view::{CustomerView, EntityView, PropertyView};
use ontok::mcp::tools::guard;

/// 造一个单对象、单属性的客户视图。
fn view_with(object: &str, prop_name: &str, note: Option<&str>) -> CustomerView {
    CustomerView {
        objects: vec![EntityView {
            name: object.to_string(),
            note: None,
            belongs_to: None,
            aliases: vec![],
            properties: vec![PropertyView {
                name: prop_name.to_string(),
                note: note.map(str::to_string),
                required: true,
                sample: None,
            }],
        }],
        relations: vec![],
    }
}

/// 过一遍守门器 —— 「剔除之后还会不会命中」。
fn guarded(view: &CustomerView) -> Result<String, String> {
    guard(&serde_json::to_value(view).expect("序列化"))
}

/// ── AC-002 安全性：真·建模术语**必须**仍然到达守门器并被拒 ─────────────
///
/// 这些是 AC-002 的对照组（`主键服务中心` 那一例见
/// `tests/test_review_view.rs`）。它们**不是**源库标识符形态，
/// 所以剔除判据对它们**一律不命中** —— 这正是本轮不能削弱的那条性质。
///
/// ⛔ 变异自证：把 `filter_view` 改成「一律剔除」或「按建模词剔除」，
/// 本测试**必须转红**（`still_reaches` 与 `is_rejected` 同时失效）。
#[test]
fn modeling_terms_are_never_swallowed_by_the_filter() {
    // (说明文字, 为什么它是「真·建模术语」而不是「技术串」)
    let cases = [
        ("主键标识", "中文建模术语本体"),
        ("外键指向客户", "中文建模术语本体"),
        ("主键服务中心", "AC-002 对照用例：展示名里的建模用语"),
    ];
    for (text, why) in cases {
        let filtered = filter_view(&view_with("客户", text, None));
        assert_eq!(
            filtered.dropped_len(),
            0,
            "{text:?}（{why}）被剔除判据吃掉了 —— 这道剔除只该管源库标识符"
        );
        assert!(
            guarded(&filtered.view).is_err(),
            "{text:?}（{why}）过完剔除后守门器居然放行了 —— 发布闸门被拆了"
        );
    }
}

/// 未翻译的**基数记号**与**英文建模词**同样必须被拒。
///
/// 它们是 `scan_modeling_terms` 的第三、四档（按**形态**判，不靠词表），
/// 与 `identifier_shape` 的形态判据是两套东西 —— 本测试钉住两者不串味。
#[test]
fn untranslated_notation_and_english_terms_still_get_rejected() {
    for text in ["1:N", "N:1", "1..*", "cardinality", "CARDINALITY", "ordinal"] {
        let filtered = filter_view(&view_with("客户", text, None));
        assert_eq!(
            filtered.dropped_len(),
            0,
            "{text:?} 被剔除判据吃掉了 —— 它是建模术语，不是源库标识符"
        );
        assert!(
            guarded(&filtered.view).is_err(),
            "{text:?} 过完剔除后守门器居然放行了"
        );
    }
}

// ── 正向：四条「长得像真实数据」的合成串必须被剔掉（这条就是本轮要修的东西）──

/// 四条**形状**取自真实源库的合成串 —— 每条命中剔除判据的**机制**与真实数据一致。
///
/// # ⚠️ 这些是**合成数据**，不是客户数据
///
/// 上一版这里写的是**逐字复制的真实列注释**（含真实表名 `wg_` 化的那份）。
/// 脱敏扫描判定为**新暴露**（真实列注释原文此前从未出现在已推送内容里），
/// 故整组换成**形状相同、取值全虚构**的串：
///
/// - 客户专有的标识符（表名、列名、配置项名）一律换成 `wg_` 前缀的合成 token；
/// - 保留 `pk` / `key` / `CDC` / `Kafka` —— 这些是**通用技术词汇**，
///   不是客户资产，而且 `key` **就是**被测的那个建模术语字面量，换掉它测试就废了。
///
/// `wg_` 前缀保证「一眼看得出是合成夹具」，同时**不会**像 `t001…t091`
/// 那种脱敏占位表那样与真实值混淆（本项目栽过那个坑）。
const SYNTH_DIRTY: [(&str, &str); 4] = [
    (
        "样例事件编号：CDC/wg_op/pk/wg_lsn、wg_sync/wg_run/wg_kind、wg_pkg_life/...。",
        "含 pk 与多个 snake_case 技术串",
    ),
    (
        "Kafka key；单 key 事件使用 wg_evt_key，wg_defaults 使用 wg_cfg_rate。",
        "Kafka 消息键 + 两个 snake_case 标识符",
    ),
    (
        "单 key 事件依赖的 wg_defaults 版本，用于处理 Kafka 跨 key 乱序。",
        "独立 key 词元 —— 建模术语字面量那一档",
    ),
    (
        "样例关联的 Key 凭据",
        "中文前缀让业务前缀豁免失效（REQ-105 单独立项，本轮不修）",
    ),
];

/// 那四条：剔除后属性消失，且**整个节点**守门通过。
///
/// 这条是 REQ-104 的回归钉子 —— 它红 = 那 3 个节点又点不开了。
#[test]
fn the_four_synthetic_dirty_attributes_are_removed_and_the_node_opens() {
    for (text, why) in SYNTH_DIRTY {
        let filtered = filter_view(&view_with("样例用量台账", text, None));
        assert_eq!(filtered.dropped_len(), 1, "{text:?}（{why}）应当被剔掉");
        assert!(
            filtered.view.objects[0].properties.is_empty(),
            "{text:?} 剔了却还留在属性列表里"
        );
        assert!(
            guarded(&filtered.view).is_ok(),
            "{text:?}（{why}）剔完之后整份节点仍被守门拒发"
        );
    }
}

/// 名字脏 ⇒ 整条属性走；名字干净、说明脏 ⇒ **只隐去说明**，属性还在。
///
/// 两种动作必须分开报 —— 判读人分不清「少了一条属性」和「少了一句说明」。
#[test]
fn a_dirty_note_only_silences_the_note() {
    let filtered = filter_view(&view_with(
        "客户",
        "所属授权",
        Some("样例关联的 Key 凭据"),
    ));
    let props = &filtered.view.objects[0].properties;
    assert_eq!(props.len(), 1, "属性本身该留着");
    assert_eq!(props[0].note, None, "说明该被隐去");
    assert_eq!(
        filtered.dropped,
        vec![DroppedProp {
            object: "客户".into(),
            property: "所属授权".into(),
            whole_property: false,
        }]
    );
    assert!(guarded(&filtered.view).is_ok());
}

/// 剔除是**幂等**的。
///
/// 三个出口都可能拿到上游已处理过的视图；「跑一次少一条」是最难查的一类偏差。
#[test]
fn filtering_twice_changes_nothing() {
    let mut v = view_with("客户", "下单时间", None);
    v.objects[0].properties.push(PropertyView {
        name: "样例关联的 Key 凭据".into(),
        note: Some("样例关联的 Key 凭据".into()),
        required: false,
        sample: None,
    });
    v.objects[0]
        .properties
        .push(PropertyView { name: "customer_pk".into(), note: None, required: false, sample: None });
    v.objects.push(EntityView {
        name: "订单".into(),
        note: Some("来源 t052.model_id 的订单".into()),
        belongs_to: None,
        aliases: vec![],
        properties: vec![PropertyView { name: "金额".into(), note: None, required: true, sample: None }],
    });

    let once = filter_view(&v);
    let twice = filter_view(&once.view);
    assert_eq!(once.view, twice.view, "剔除不幂等：跑两次与跑一次结果不同");
    // 幂等的第二面：**第二遍不该再报任何剔除** —— 若它又报一遍，
    // 说明第一遍没剔干净，两套动作在叠加。
    assert!(
        twice.dropped.is_empty(),
        "第二遍又报剔除，说明第一遍没剔干净：{:?}",
        twice.dropped
    );
}

/// 剔除**不改变**投影本身 —— 这是「门禁覆盖面缩小」那条已知限制的前提。
///
/// `tests/drop_rate` 门禁调的就是 `view::project()`。若哪天有人把剔除搬进
/// `project()`，门禁分子会 +20、活体口径撞破 `MAX_DROP_RATE = 0.10`。
/// 本测试**钉住**剔除目前在投影之后，把那条取舍固定下来。
#[test]
fn the_projection_itself_is_left_alone() {
    use ontok::domain::view::{
        project, EntityTypeView, LinkTypeView, PropertyTypeView, SnapshotView,
    };
    let snap = SnapshotView {
        entity_types: vec![EntityTypeView {
            key: "wg_usage_ledger".into(),
            display_name: "样例用量台账".into(),
            description: None,
            aliases: vec![],
            parent: None,
            disjoint_with: vec![],
            properties: vec![PropertyTypeView {
                name: "样例关联的 Key 凭据".into(),
                column: "wg_license_ref".into(),
                nullable: false,
                note: Some("样例关联的 Key 凭据".into()),
                sample: None,
            }],
            evidence: vec![],
        }],
        link_types: vec![LinkTypeView {
            key: String::new(),
            from_type: String::new(),
            to_type: String::new(),
            cardinality: String::new(),
            semantic: String::new(),
        }],
    };
    let projected = project(&snap);
    assert_eq!(
        projected.objects[0].properties.len(),
        1,
        "投影里不该被剔除 —— 剔除在出口层，不在投影内"
    );
    assert!(
        guarded(&projected).is_err(),
        "投影输出（含那条脏属性）应当照旧被守门拒发"
    );
    assert_eq!(
        filter_view(&projected).view.objects[0].properties.len(),
        0,
        "出口层剔除应当把它拿掉"
    );
}

/// 业务人话**一个都不许少**。
///
/// 剔除判据是形态判据，误伤面必须钉住：这些都是页面上天天在显示的属性。
///
/// ⚠️ **刻意不放的两类**，理由都是「与页面出口保持一致」，不是「放过不管」：
///
/// 1. `消息类型：transaction/rule_alert` —— 含 snake_case（`rule_alert`），
///    **剔掉是对的**：它已在页面出口既有的 25 条剔除清单里实测出现过。
/// 2. `License Key` / `api_key` —— `api_key` 是 snake_case；
///    `License Key` 的 `Key` 命中 `identifier_shape::is_modeling_literal`
///    （大小写不敏感等于 `key`）。**实测真实 `demo3` v2 客户视图里这类属性 0 条**
///    （`license key` 注释落在 `activity_keys.license_key` / `licenses.license_key`，
///    抽取后没进客户视图），所以这是**继承页面出口既有行为**的理论误伤面，
///    不是本轮新增的损失。
///
/// 这两条若被"优化"成放行，就会与页面出口**行为不一致** —— 那才是真问题。
#[test]
fn ordinary_business_attributes_survive_untouched() {
    for text in [
        "下单时间",
        "所属授权",
        "客户下单产生的记录",
        "金额",
        "用量",
        "所属客户",
        "一对多关系",
        "客户ID",
    ] {
        let filtered = filter_view(&view_with("客户", text, None));
        assert_eq!(
            filtered.dropped_len(),
            0,
            "{text:?} 被误伤了 —— 它是业务人话"
        );
        assert_eq!(filtered.view.objects[0].properties.len(), 1);
    }
}

// ── 三处出口共用一个函数（源码扫描型守卫，变异证据取被扫文件 sha256）────

/// ⭐ REQ-107 · **否定式**：三个出口都**不许自己调判据**，一律经由管线。
///
/// # 为什么从正向改成否定式（AC §七 A3：改写，⛔ 不许删）
///
/// 收口前这条断言的是「三个文件里 `contains("property_filter::filter_view")`」。
/// 本文件（`src/domain/property_filter.rs:129-142`）自己登记了它的盲区：
/// **保留共享调用、同时另加一份真在生效的本地判据 ⇒ QA 变异 M3 实测 9 条全绿**。
/// 正向判据钉不住这个形态 —— 它只问「调了没有」，不问「还有没有第二份」。
///
/// 收口后判据换成**否定式**：
/// 1. 出口文件里对判据的**直调点数必须为 0**（剔除/清洗/闸全部在
///    `domain::pipeline` 里执行）；
/// 2. 每个出口都必须有**管线调用点**（否则「0 命中」可以靠「什么都不做」达成）。
///
/// 两条缺一不可：只做 1，出口可以把整段删掉；只做 2，出口可以「调管线 + 另写一份」。
///
/// 名字**沿用**（A3：改写、不许删）：它守的意图没变 —— 剔除只许有**一份**实现，
/// 三个出口都走它；变的是判据方向。
#[test]
fn all_three_outlets_call_the_one_shared_function() {
    // (文件, 人读的名字, 必须出现的管线调用点)
    let outlets: [(&str, &str, &[&str]); 3] = [
        ("src/cli.rs", "ontology show（JSON 出口）", &["OutletSpec::C3"]),
        ("src/server/http.rs", "只读图 /api/node", &["prepare_view", "OutletSpec::H3"]),
        ("src/mcp/serve.rs", "MCP 通道", &["prepare_view", "OutletSpec::MCP_TOOLS"]),
    ];

    // 判据直调点的**唯一**允许位置就是管线模块本身；出口文件里必须是 0。
    //
    // ⚠️ 针脚是 `tools::guard(` / `mcp::tools::guard(` 而**不是**裸 `guard(`：
    // 后者会把 `install_broken_pipe_guard()` 这类无关调用算成判据调用
    // （实测 `src/cli.rs:288` / `:738` 就是它）⇒ 判据恒红 = 恒假。
    // 这是 TC-001 用例原文的一个**已知缺陷**（它写的是裸 `guard(`），
    // 交付物「异议」里已登记。
    const CRITERION_CALLS: &[&str] = &[
        "filter_view(",
        "has_raw_identifier(",
        "sanitize_display(",
        "scan_modeling_terms(",
        "tools::guard(",
    ];

    for (rel, which, needles) in outlets {
        // ⚠️ 必须剥注释：本文件/出口文件的注释里就写着这些名字（裸 grep 会假红）。
        // ⚠️ 只扫**测试模块之前**的生产代码：`http.rs` 的 `#[cfg(test)]` 里有
        //    「错误文案自己过闸」那条测试，它**调用的就是判据** —— 那是测试代码。
        let raw = common::strip_comments(&common::read_source(rel));
        let src = raw.split("#[cfg(test)]").next().unwrap_or("").to_string();

        for bad in CRITERION_CALLS {
            let hits: Vec<usize> = src
                .match_indices(bad)
                .map(|(off, _)| src[..off].matches('\n').count() + 1)
                .collect();
            assert!(
                hits.is_empty(),
                "{which}（{rel}）里还有 {} 处判据直调 `{bad}`（行 {:?}）—— \
                 剔除/清洗/闸只许经由 `crate::domain::pipeline`，出口层一份都不许留",
                hits.len(),
                hits
            );
        }

        for needle in needles {
            assert!(
                src.contains(needle),
                "{which}（{rel}）里找不到管线调用点 `{needle}` —— \
                 「判据直调点为 0」也可以靠「出口什么都不做」达成，所以这一条必须有"
            );
        }
    }

    // 反空转：管线模块里**必须**有判据调用，否则上面那圈「0 命中」是因为全仓都没有它。
    let pipeline = common::strip_comments(&common::read_source("src/domain/pipeline.rs"));
    assert!(
        pipeline.contains("filter_view(") && pipeline.contains("scan_modeling_terms("),
        "管线模块里居然没有判据调用 —— 上面的「出口 0 命中」是空转"
    );
}

/// 剥注释这一步本身不能是空转（纪律 E：扫出「零」先怀疑扫描器）。
#[test]
fn strip_comments_is_not_vacuous() {
    let raw = "// filter_view(&cv)\nlet x = 1;\n/* scan_modeling_terms(x) */\nlet y = 2;\n";
    let out = common::strip_comments(raw);
    assert!(!out.contains("filter_view"), "行注释没被剥掉：{out}");
    assert!(!out.contains("scan_modeling_terms"), "块注释没被剥掉：{out}");
    assert!(out.contains("let x = 1;") && out.contains("let y = 2;"), "真代码被误剥：{out}");
    // 字符串字面量**不该**被剥（它是代码，判据要能看见它 ⇒ 宁可假红）。
    let kept = common::strip_comments("let s = \"filter_view(\";");
    assert!(kept.contains("filter_view("), "字符串字面量被误剥：{kept}");
    // 字符常量里的 `'` 不许把后续代码吞掉。
    let ch = common::strip_comments("let q = '\\'';\nlet z = 3;\n");
    assert!(ch.contains("let z = 3;"), "字符常量吞掉了后续代码：{ch}");
}

/// ⛔ 守门器**一行没改**：仍是 fail-closed，仍是整份序列化后全文扫。
///
/// 本轮只许在闸门**之前**减属性，不许动判据本身。
#[test]
fn the_guard_is_still_fail_closed_and_unmodified() {
    let src = common::read_source("src/mcp/tools.rs");
    assert!(
        src.contains("pub fn guard(value: &Value) -> Result<String, String> {"),
        "guard 的签名变了 —— 本轮不许改守门器"
    );
    for needle in [
        "let text = serde_json::to_string_pretty(value)",
        "crate::domain::view::scan_modeling_terms(&text)",
        "已拒绝外送",
    ] {
        assert!(
            src.contains(needle),
            "guard 的判据或文案被改动了（缺 {needle:?}）—— 本轮不许改守门器"
        );
    }
    // 行为上也钉一遍：命中即 Err，且**不带具体词条**
    let err = guard(&serde_json::json!({ "t": "主键" })).expect_err("主键必须被拒");
    assert!(!err.contains("主键"), "错误文案把守门词回显出去了：{err}");
    assert!(guard(&serde_json::json!({ "t": "下单时间" })).is_ok());
}

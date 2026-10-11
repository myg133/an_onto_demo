//! REQ-005 · 客户视图属性名质量（AC-013 判读的前置条件）
//!
//! # 病根
//!
//! 抽取器早期是**无条件采用列注释**当属性名（`name = comment.unwrap_or_default()`），
//! 而 `view::project()` 只在 `name` 为**空串**时才去问 `humanize_column()`。
//! 于是**一条英文注释把词表彻底挡在门外** —— 词表里有 `license → 授权`、
//! `COLUMN_LABELS` 里有 `license_id → 所属授权`，全都没机会出手。
//!
//! 真实源库 91 表 / 964 属性里**恰好 8 条**注释一个汉字都没有，
//! 全部由这一条路径漏出去。
//!
//! # 本文件钉住什么
//!
//! 1. 类 A（5 处）**再也不能**以非中文形态出现在客户视图；
//! 2. 类 B（2 处合法枚举值域）**必须原样保留在 `note`** —— 翻了反而对不上
//!    库里的实际值；但**不许再占着「属性名」这个位置**（REQ-007：那是说明，
//!    不是名字，属性名要走词表拿「类型」/「状态」）；
//! 3. 中英混排的注释**一个都不能被误杀**（这是「145 个含英文但不全是英文」那批）；
//! 4. 英文**不许从 `note` 字段绕回来** —— 同一段英文换个字段名照样是泄露。
//!    ⚠️ 第 4 条只对**类 A**成立；类 B 的值域**就是要**留在 `note` 里（见第 2 条）。
//!
//! **零 I/O**：用的是内存里的合成 schema，不连任何数据库。

use ontok::domain::view::{
    humanize_column, project, scan_modeling_terms, EntityTypeView, LinkTypeView, PropertyTypeView,
    SnapshotView,
};
use ontok::extract::introspect::{ColumnMeta, SchemaReport, TableMeta};
use ontok::extract::names::TableNames;
use ontok::extract::propose::{classify_comment, extract, CommentQuality, ExtractionStats};

/// 真实源库里**那 8 条**（表、对象名、列、注释、类型）。
///
/// `is_primary_key` 一律 false —— 每张表只挂这一个业务列，
/// 于是客户视图里每个对象恰好一个属性，按对象名取名字不会认错。
const REAL: &[(&str, &str, &str, &str)] = &[
    // ── 类 A：纯英文，必须降级 ──
    ("activity_keys", "活动关键词", "license_key", "license key"),
    ("licenses", "授权凭证", "license_key", "license key"),
    ("api_call_logs", "API 调用日志", "license_id", "License ID"),
    ("instance_manager", "实例管理", "session_id", "SessionId"),
    ("customer_provider_permissions", "客户服务商权限", "customer_quota_policies",
     "user quota policies in JSON array format"),
    // ── 类 B：合法枚举值域，有意不翻 ──
    ("app_billing_transactions", "应用扣费交易", "status", "processing/charged/failed"),
    ("llm_model", "大模型", "type", "0: llm, 1: emb"),
    // ── 类 C：类 A 的同型，翻译与否是判断题 ──
    ("llm_provider_group_items", "服务商分组明细", "provider_id", "Provider ID"),
    // ── 中英混排：**绝不可被误杀**（范围外的「145 个」）──
    ("order", "订单", "status", "状态 rech:成功 pending:支付中 free:取消"),
    ("deductions", "扣费流水", "license_id", "license 标识"),
    ("llm_routing_rules", "模型路由规则", "provider_id", "直接指定Provider(优先级最高)"),
];

/// 无注释列 —— 走的是「本来就没注释」的路径，与降级路径要分开验证。
const NO_COMMENT: (&str, &str, &str) = ("licenses_archive", "历史授权凭证", "license_key");

fn build_schema() -> SchemaReport {
    let mut tables: Vec<TableMeta> = Vec::new();
    for (t, _obj, col, comment) in REAL {
        tables.push(one_col_table(t, col, Some(comment)));
    }
    tables.push(one_col_table(NO_COMMENT.0, NO_COMMENT.2, None));
    SchemaReport {
        server_version: "test".into(),
        transaction_read_only: "on".into(),
        schema: "public".into(),
        table_count: tables.len(),
        column_count: 0,
        commented_column_count: 0,
        foreign_key_count: 0,
        tables,
        introspected_at: "test".into(),
    }
}

/// 一张表 = 一个 `id` 主键 + 一个业务列。`id` 列会被抽取器跳过
/// （`is_primary_key && name == "id"`），所以每个对象恰好剩一个属性。
fn one_col_table(table: &str, col: &str, comment: Option<&str>) -> TableMeta {
    TableMeta {
        name: table.into(),
        comment: None,
        columns: vec![
            ColumnMeta {
                name: "id".into(),
                data_type: "bigint".into(),
                nullable: false,
                comment: Some("主键标识".into()),
                is_primary_key: true,
            },
            ColumnMeta {
                name: col.into(),
                data_type: "character varying".into(),
                nullable: false,
                comment: comment.map(String::from),
                is_primary_key: false,
            },
        ],
        column_count: 2,
        commented_count: usize::from(comment.is_some()),
    }
}

fn build_names() -> TableNames {
    let mut names = std::collections::HashMap::new();
    for (t, obj, _, _) in REAL {
        names.insert((*t).to_string(), (*obj).to_string());
    }
    names.insert(NO_COMMENT.0.to_string(), NO_COMMENT.1.to_string());
    TableNames {
        agent: "test".into(),
        version: "test".into(),
        generated_at: "test".into(),
        source: "test".into(),
        names,
    }
}

fn to_snapshot(items: &[serde_json::Value]) -> SnapshotView {
    let mut entity_types = Vec::new();
    let mut link_types = Vec::new();
    for it in items {
        let p = &it["payload"];
        match it["kind"].as_str().unwrap_or_default() {
            "add_entity_type" => entity_types.push(EntityTypeView {
                key: p["key"].as_str().unwrap_or_default().into(),
                display_name: p["display_name"].as_str().unwrap_or_default().into(),
                description: p.get("description").and_then(|d| d.as_str()).map(String::from),
                aliases: vec![],
                parent: None,
                disjoint_with: vec![],
                properties: p["properties"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .map(|pr| PropertyTypeView {
                        name: pr["name"].as_str().unwrap_or_default().into(),
                        column: pr["column"].as_str().unwrap_or_default().into(),
                        nullable: pr["nullable"].as_bool().unwrap_or(false),
                        note: pr.get("note").and_then(|n| n.as_str()).map(String::from),
                        sample: None,
                    })
                    .collect(),
                evidence: vec![],
            }),
            "add_link_type" => link_types.push(LinkTypeView {
                key: p["key"].as_str().unwrap_or_default().into(),
                from_type: p["from_type"].as_str().unwrap_or_default().into(),
                to_type: p["to_type"].as_str().unwrap_or_default().into(),
                cardinality: p["cardinality"].as_str().unwrap_or_default().into(),
                semantic: p["semantic"].as_str().unwrap_or_default().into(),
            }),
            _ => {}
        }
    }
    SnapshotView { entity_types, link_types }
}

fn run() -> (ontok::domain::view::CustomerView, ExtractionStats) {
    let out = extract(&build_schema(), &build_names(), "props_fix", "test", "test").unwrap();
    let snap = to_snapshot(&out.proposal.items);
    (project(&snap), out.stats)
}

/// REQ-008 新增用例用：拿一组自造的行跑完整抽取→投影链路。
/// 每行一张表（`id` 主键 + 一个业务列），因此「取第一个属性」不会认错对象。
fn run_with(rows: &[(&str, &str, &str, Option<&str>)]) -> (ontok::domain::view::CustomerView, ExtractionStats) {
    let mut tables: Vec<TableMeta> = Vec::new();
    let mut names = std::collections::HashMap::new();
    for (t, obj, col, comment) in rows {
        names.insert((*t).to_string(), (*obj).to_string());
        tables.push(one_col_table(t, col, *comment));
    }
    let schema = SchemaReport {
        server_version: "test".into(),
        transaction_read_only: "on".into(),
        schema: "public".into(),
        table_count: tables.len(),
        column_count: 0,
        commented_column_count: 0,
        foreign_key_count: 0,
        tables,
        introspected_at: "test".into(),
    };
    let tn = TableNames {
        agent: "test".into(),
        version: "test".into(),
        generated_at: "test".into(),
        source: "test".into(),
        names,
    };
    let out = extract(&schema, &tn, "props_fix", "test", "test").unwrap();
    let snap = to_snapshot(&out.proposal.items);
    (project(&snap), out.stats)
}

/// 客户视图里某对象的属性名（每张表只挂一个业务列，取不到 = 属性被丢弃）。
fn prop_name(view: &ontok::domain::view::CustomerView, obj: &str) -> Option<String> {
    view.objects
        .iter()
        .find(|o| o.name == obj)
        .and_then(|o| o.properties.first())
        .map(|p| p.name.clone())
}

fn prop_note(view: &ontok::domain::view::CustomerView, obj: &str) -> Option<String> {
    view.objects
        .iter()
        .find(|o| o.name == obj)
        .and_then(|o| o.properties.first())
        .and_then(|p| p.note.clone())
}

/// **类 A · 主测试**：5 处非中文属性名必须全部不再是英文。
///
/// 变异：把质量门槛调成「永远判 HumanReadable」（即回到修复前的无条件采用），
/// 或删掉 `LowQuality` 分支 —— 本测试立刻变红。
#[test]
fn test_class_a_english_names_reach_customer_as_chinese() {
    let (view, stats) = run();

    // 词表能翻出来的：真的翻成了中文
    assert_eq!(prop_name(&view, "活动关键词").as_deref(), Some("授权标识"), "license_key 应走词表");
    assert_eq!(prop_name(&view, "授权凭证").as_deref(), Some("授权标识"), "license_key 应走词表");
    assert_eq!(prop_name(&view, "API 调用日志").as_deref(), Some("所属授权"), "license_id 命中 COLUMN_LABELS");
    // 无注释列本来就走词表，行为不变（回归护栏）
    assert_eq!(prop_name(&view, "历史授权凭证").as_deref(), Some("授权标识"));

    // `session_id` —— 词表原本**没有** `session` 词根，回退返回 None，
    // 属性被 S-2 丢弃、客户视图少一项。
    // BA 复核判定「这是词表真实缺词、不是形态问题」→ 已补 `session → 会话` 词条。
    // `session_id → 会话` 是语义映射，没有形态规则能推出来，所以只能补词条 ——
    // 这跟「形态能解决却用词表打补丁」是两回事。
    assert_eq!(
        prop_name(&view, "实例管理").as_deref(),
        Some("会话"),
        "补了 session 词根后 session_id 应翻成中文，不再被丢弃"
    );
    // `customer_quota_policies` 是**复合词**（customer + quota + policies），
    // 补 `policies` 单个词根救不了 —— `humanize_column` 只在剥技术后缀后查词根、
    // 以及整词查，两条路都覆盖不到复合词。
    //
    // **REQ-006 行为变更（唯一一处改动既有断言，理由如下）**：
    // REQ-005 当初的注释写明「根治要等『复合词拆分』那一轮」—— 本轮就是那一轮，
    // 且 `customer_quota_policies` 正是审核点名的复合词缺口样例。
    // REQ-006 补上 `resolve_stem`（逐词元解析，全词元都认识才拼）之后，
    // 它第一次能被翻成中文。**S-2 的关注点没有被放宽**：
    // 变化是「丢弃」→「翻成中文」，**不是**「丢弃」→「原样透出英文」；
    // 下面那段「一个都不许残留英文」的断言原封不动继续钉着。
    assert_eq!(
        prop_name(&view, "客户服务商权限").as_deref(),
        Some("客户额度策略"),
        "customer_quota_policies 走复合词拆分后应翻成中文，而不是原样透出或丢弃"
    );

    // **本轮新增的反向护栏**：复合词拆分**没有**把 S-2 变成 fail-open。
    // 只要有一个词元不认识，整个复合词仍然必须丢弃 ——
    // `blob_ref_xyz` / `zzz_unmapped_token` / `aisearch_times_internal_v2`
    // 三个词元里各有一个不认识，全都必须返回 None。
    // 若有人把 `resolve_stem` 改成「认识的词元翻译、不认识的丢掉再拼剩下的」，
    // 本断言立刻变红。
    for opaque in [
        "blob_ref_xyz",
        "zzz_unmapped_token",
        "aisearch_times_internal_v2",
        "customer_xyz_policies", // 只有 policies 认识，customer_xyz 不认识
    ] {
        assert_eq!(
            ontok::domain::view::humanize_column(opaque),
            None,
            "复合词 {opaque} 里有词元不认识，必须整体丢弃而不是拼出半吊子人话"
        );
    }

    // 整份视图里，一个都不许残留这 5 段英文
    let json = serde_json::to_string(&view).unwrap();
    for leak in [
        "license key",
        "License ID",
        "SessionId",
        "user quota policies in JSON array format",
    ] {
        assert!(!json.contains(leak), "客户视图里仍残留英文「{leak}」");
    }

    // 降级次数必须被显式记进统计（fail-open 的教训：不许静默降级）
    assert_eq!(stats.props_comment_rejected, 6, "6 条纯英文注释应被降级");
}

/// **类 B · 钉子（REQ-007 改写）**：合法枚举值域
/// **必须原样保留在 `note` 里，且不许再占着「属性名」这个位置**。
///
/// # REQ-005 结论未变、REQ-007 只改位置
///
/// REQ-005 说「值域不翻」—— 翻了会破坏信息：客户看到「处理中」，
/// 回库查到的是 `processing`。**这条一个字都没动**，值域仍在英文。
///
/// REQ-007 修的是**另一件事**：值域是「这一列有哪些取值」，
/// 属于说明；属性名是「这一列叫什么」。
/// 让 `0: llm, 1: emb` 当属性名，页面上就是一条**由英文常量拼成的假名字**，
/// 判读人读到的是「这个对象有个属性叫 `0: llm, 1: emb`」。
/// 置空 name 之后 `project()` 去问 `humanize_column(column)`，
/// 词表里 `type → 类型` / `status → 状态` 本来就收着。
///
/// **变异自证**：
/// - 把 `EnumDomain` 分支改回 `(txt.clone(), comment.clone())` ⇒ 第 2、4 条断言变红；
/// - 把 `EnumDomain` 的 `note` 也置空（同 `LowQuality`）⇒ 第 3、5 条断言变红。
#[test]
fn test_class_b_enum_domains_keep_values_but_not_as_name() {
    let (view, stats) = run();

    // ① 属性名回到人话：走词表，不再是值域
    assert_eq!(
        prop_name(&view, "应用扣费交易").as_deref(),
        Some("状态"),
        "status 列应走词表拿到「状态」，属性名不该是值域"
    );
    assert_eq!(
        prop_name(&view, "大模型").as_deref(),
        Some("类型"),
        "type 列应走词表拿到「类型」，属性名不该是值域"
    );

    // ② 值域**原样留在说明里**（英文是对的：那就是库里真存的值）
    assert_eq!(
        prop_note(&view, "应用扣费交易").as_deref(),
        Some("processing/charged/failed"),
        "枚举值域必须原样保留在 note（翻译反而对不上库里的实际值）"
    );
    assert_eq!(
        prop_note(&view, "大模型").as_deref(),
        Some("0: llm, 1: emb"),
        "枚举值域必须原样保留在 note"
    );

    // ③ **反向护栏**：`EnumDomain` 与 `LowQuality` 对 note 的处理**故意相反**。
    //    若有人「顺手统一」把 note 也置空，上面两条 note 断言立刻变红。
    //    这里再钉一层：值域不得计入降级数（降级 = 连词表都不给走）。
    assert_eq!(
        stats.props_comment_rejected, 6,
        "枚举值域不得计入降级数（它要走词表拿名字，不是被丢掉）"
    );

    // ④ 值域本身**没有**被翻成中文 —— REQ-005 的原始结论
    let json = serde_json::to_string(&view).unwrap();
    for leaked in ["处理中", "已扣费", "嵌入"] {
        assert!(!json.contains(leaked), "枚举值域被翻了：出现了中文值 {leaked}");
    }

    // 判据本身也直接钉一遍
    for c in ["processing/charged/failed", "0: llm, 1: emb"] {
        assert_eq!(classify_comment(c), CommentQuality::EnumDomain);
    }
}

/// **误杀防线**：中英混排的注释一个都不能动。
///
/// 变异：把门槛写成「汉字占比 ≥ 0.5」—— 本测试立刻变红。
///
/// ⚠️ **REQ-008 行为变更（唯一一处改动既有断言）**：
/// `扣费流水.license_id` 的注释 `license 标识` **不再照用**，改显示为「所属授权」。
/// 原因：这一列是**外键**（指向 `licenses`），而 `license` 就是 `licenses` 的英文名 ——
/// 照用的话页面上会出现一个**纯英文属性名** `license 标识`，
/// 而关系明明已经能给出人话。外键的属性名由**关系**决定（REQ-008 任务 1）。
/// 这条新行为由 `tests/test_fk_naming.rs::test_english_restatement_of_the_target_is_not_kept_as_a_name`
/// 正面钉住。
///
/// **本测试的防线没有因此放宽**：另外两行（非外键的中英混排注释）
/// 仍然逐字保留，中英混排**不许被误杀**这条主张一条没松。
#[test]
fn test_mixed_chinese_comments_are_preserved_verbatim() {
    let (view, _) = run();
    for (obj, want) in [
        ("订单", "状态 rech:成功 pending:支付中 free:取消"),
        // ⚠️ 外键 + 注释只是目标的英文名 ⇒ 走关系（见上面的说明）
        ("扣费流水", "所属授权"),
        ("模型路由规则", "直接指定Provider(优先级最高)"),
    ] {
        assert_eq!(prop_name(&view, obj).as_deref(), Some(want), "中英混排注释「{obj}」被误改了");
    }
}

/// **类 C · 决定**：`Provider ID` 翻成「服务商」。
///
/// 判据不是「Provider 是不是内部黑话」——全平台的词表里
/// `provider → 服务商`、对象名也是「大模型服务商 / 服务商分组」，
/// 所以 `Provider` 就是**服务商**的英文写法，客户看得懂。
/// 它漏出去的原因与其他 7 条**完全同一个**：注释被无条件采用。
/// 因此按同一条规则处理，不搞特例。
#[test]
fn test_class_c_provider_id_is_translated() {
    let (view, _) = run();
    assert_eq!(
        prop_name(&view, "服务商分组明细").as_deref(),
        Some("服务商"),
        "Provider ID 必须翻成中文，不许原样透给客户"
    );
}

/// **`note` 也不许绕回来**：低质量注释换个字段名照样是泄露。
#[test]
fn test_low_quality_comment_does_not_leak_through_note() {
    let (view, _) = run();
    for obj in ["活动关键词", "授权凭证", "API 调用日志", "历史授权凭证"] {
        assert_eq!(
            prop_note(&view, obj),
            None,
            "「{obj}」的 note 仍是那段低质量英文注释 —— 换个字段名也是泄露"
        );
    }
}

/// **REQ-008 · 任务 3a**：屈折形态是**规则**，不是新增词条。
///
/// 真实源库 81 条丢弃里，`prices` / `models` / `available_cycles` /
/// `weekdays` / `tokenhub_notices` 全是**同一个意思的屈折写法** ——
/// 词表里早有 `price` / `model` / `available` / `cycle` / `weekday` / `notice`。
/// 补词条等于把同一个意思再抄一遍，下个客户库换个写法又漏。
///
/// **变异自证**：把 `inflect_variants` 改成返回空 vec ⇒ 本测试立刻变红。
#[test]
fn test_inflection_is_morphology_not_wordlist() {
    // 认识 → 认识（屈折还原后查得到）
    for (col, want) in [
        ("prices", Some("价格")),              // prices → price
        ("cycles", Some("周期")),              // cycles → cycle
        ("models", Some("模型")),              // models → model
        ("weekdays", Some("工作日")),          // weekdays → weekday
        ("tokenhub_notices", Some("令牌中心公告")), // notices → notice
        ("available_cycles", Some("可用周期")), // 先还原末词元，再命中 `_cycle` 形态后缀
        ("original_prices", None),             // `original` 不认识 ⇒ 仍然丢弃
    ] {
        assert_eq!(
            humanize_column(col).as_deref(),
            want,
            "「{col}」应当走屈折规则（而不是靠补词条）"
        );
    }

    // **不碰的三类**：去掉后缀会造出另一个词。
    // 已在词表里的走**原形**命中（优先级高于屈折），不在词表里的仍然 `None` ——
    // 也就是说 `-ss` / `-us` / `-is` / 过短的词，屈折规则一律不许改动答案。
    for (col, want) in [
        ("status", Some("状态")),   // -us：不许被剥成 `statu`
        ("address", Some("地址")),  // -ss：不许被剥成 `addres`
        ("analysis", None),         // -is：复数是 `analyses`，剥 `s` 是错的
        ("gas", None),              // 过短
        ("is", None),               // 过短
        ("bus", None),              // 过短
        ("news", None),             // 剥成 `new` 也不是词，但不许因此给出别的答案
    ] {
        assert_eq!(
            humanize_column(col).as_deref(),
            want,
            "「{col}」被屈折规则改坏了（原形命中优先，-ss/-us/-is 与过短的词不许剥后缀）"
        );
    }

    // **fail-closed 不变**：屈折救不出的仍然整体丢弃（S-2）
    for opaque in [
        "blobs_ref_xyz",     // `ref` 不认识
        "zzz_unmapped_tokens", // `zzz` 不认识
        "aisearch_times_internal_v2", // `aisearch` 不认识
    ] {
        assert_eq!(humanize_column(opaque), None, "「{opaque}」必须继续整体丢弃");
    }
}

/// **REQ-008 · 任务 3b**：`is_enum_domain` 的 **fail-open** 洞。
///
/// `is_value_atom` 只看字符集合（ASCII 字母数字 + `_ - . *`），
/// 于是 `name,email` 这种**字段名列表**也被判成合法值域 ⇒「有意不翻」
/// ⇒ 英文原样进客户视图，守门器 exit 0 —— 典型的 fail-open。
///
/// 修法：一份**全是我们认识的列名用词、且一个都没有「编码:」前缀**的原子列表，
/// 是字段名列表而不是值域。
///
/// **变异自证**：删掉 `is_enum_domain` 里的 `!is_field_name_list(&parts)` ⇒
/// 第 1、2 条断言变红。
#[test]
fn test_field_name_list_is_not_an_enum_domain() {
    // ① `name,email` **必须**不再被当成值域（该走 LowQuality：注释不进页面）
    for c in ["name,email", "name,phone,email", "email,name"] {
        assert_eq!(
            classify_comment(c),
            CommentQuality::LowQuality,
            "「{c}」是**字段名列表**，不是值域 —— 判成值域就等于有意让英文漏出去"
        );
    }

    // ② 真正的值域一条都不许被误伤（**全有或全无**的代价就在这里）
    for c in [
        "processing/charged/failed",
        "0: llm, 1: emb",       // `llm` 在词表里，但**有编码前缀** ⇒ 仍是值域
        "0: llm, 1: emb, 2: rerank",
        "Y/N",
        "M/D/H",
        "utf-8/mb4",
        "active/inactive",
        "ok,failed,timeout",
        "a;b;c",
    ] {
        assert_eq!(
            classify_comment(c),
            CommentQuality::EnumDomain,
            "「{c}」是合法值域，必须继续豁免（翻了反而对不上库里的实际值）"
        );
    }

    // ③ 端到端：`name,email` 不许以任何形态出现在客户视图里
    let (view, _) = run_with(&[("export_fields", "导出字段", "export_fields", Some("name,email"))]);
    let json = serde_json::to_string(&view).unwrap();
    assert!(
        !json.contains("name,email") && !json.contains("email"),
        "字段名列表漏进客户视图了：{json}"
    );
}

/// **刻画测试**：四档守门词（`scan_modeling_terms`）**抓不到**这类英文。
///
/// 为什么留着这个断言：它说明 REQ-005 的修复**必须**落在抽取器，
/// 加守门词是没用的 —— `license_key` 的 `key` 词元因为 `license`
/// 在 `BUSINESS_ALLOWED_PREFIXES` 里被白名单放行；`SessionId` 整体
/// 也不等于任何守门词。这就是典型的 **fail-open**：错数据发出去、
/// 退出码还是 0、机器一声不吭。
///
/// ⚠️ 若此断言**变红**，说明守门器新增了这一层的拦截能力 ——
/// 那时请重新评估：抽取层是否还需要自降级，以及本文件要如何调整。
#[test]
fn test_runtime_guard_is_blind_to_english_property_names() {
    let snap = SnapshotView {
        entity_types: vec![EntityTypeView {
            key: "activity_keys".into(),
            display_name: "活动关键词".into(),
            description: None,
            aliases: vec![],
            parent: None,
            disjoint_with: vec![],
            properties: vec![
                PropertyTypeView {
                    name: "license key".into(),
                    column: "license_key".into(),
                    nullable: false,
                    note: Some("license key".into()),
                    sample: None,
                },
                PropertyTypeView {
                    name: "SessionId".into(),
                    column: "session_id".into(),
                    nullable: false,
                    note: Some("SessionId".into()),
                    sample: None,
                },
            ],
            evidence: vec![],
        }],
        link_types: vec![],
    };
    let json = serde_json::to_string(&project(&snap)).unwrap();
    assert!(json.contains("license key") && json.contains("SessionId"));
    assert!(
        scan_modeling_terms(&json).is_empty(),
        "守门器若已能抓到这类英文，说明分层变了：本测试与抽取层的降级策略都需重新评估"
    );
}

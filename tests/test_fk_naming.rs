//! REQ-008 · 任务 1 · 外键属性名**从关系推导**，不从列注释拿
//!
//! # 病根（这一轮最重要的一个）
//!
//! 真实源库同一个 `customer_id`，37 张表的注释各写各的：
//!
//! | 注释 | 张数 | 改前页面上显示成 |
//! | --- | --- | --- |
//! | `客户标识` | 10 | 客户标识 |
//! | `客户ID` | 6 | **客户**（页面层把 `ID` 记号摘掉） |
//! | `用户id` / `用户ID` / `用户 ID` | 5 | 用户 |
//! | 无注释 | 12 | 所属客户（`COLUMN_LABELS` 生效） |
//! | 带限定的 4 条 | 4 | 见下（**必须保持不变**） |
//!
//! 判读人打开「客户」这一屏，会看到**同一个东西 4 种叫法**，
//! 其中一种还跟**对象名**重名（「客户」既是对象、又是它的属性）——
//! 直接污染 Q1（这是什么）与 Q2（它有哪几个属性）。
//!
//! # 修法
//!
//! 外键是**关系**，不是自由文本。显示名由「谁指向谁」决定
//! （`customer_id → customers` → 「所属客户」），
//! **不是**由这一张表的注释碰巧怎么写决定。
//!
//! # 判据（本文件钉住的三条不变量）
//!
//! 1. **同一个 `customer_id` 只有一种显示名** —— 注释说什么都不算；
//! 2. **带角色限定的外键保持原样** —— `main_customer_id`（主账号）、
//!    `sub_customer_id`（子账号）、`source_customer_id`（划拨来源）
//!    语义确实不同，**绝不**统一；连注释本身带限定的
//!    （`子账号ID` / `账号ID（余额所属账号）` / `被代扣Open用户ID`）也保留；
//! 3. **S-2 红线不许被绕过** —— 低质量英文注释（`License ID`）
//!    即使这一列是外键，也**绝不许**当属性名。
//!
//! **零 I/O**（除最后一个数据驱动测试只读本地脱敏 `schema.json`）。

use ontok::domain::view::{
    humanize_column, project, EntityTypeView, LinkTypeView, PropertyTypeView, SnapshotView,
};
use ontok::extract::introspect::{ColumnMeta, SchemaReport, TableMeta};
use ontok::extract::names::TableNames;
use ontok::extract::propose::extract;

/// (表, 中文对象名, 列, 注释)
type Row = (&'static str, &'static str, &'static str, Option<&'static str>);

/// 合成 schema：一张目标表 `customers` + 若干带外键的表。
fn schema_of(rows: &[Row], targets: &[(&str, &str)]) -> (SchemaReport, TableNames) {
    let mut tables: Vec<TableMeta> = Vec::new();
    let mut names = std::collections::HashMap::new();
    for (t, cn) in targets {
        names.insert((*t).to_string(), (*cn).to_string());
        tables.push(table(t, "id", None));
    }
    for (t, cn, col, comment) in rows {
        names.insert((*t).to_string(), (*cn).to_string());
        tables.push(table(t, col, *comment));
    }
    (
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
        },
        TableNames {
            agent: "test".into(),
            version: "test".into(),
            generated_at: "test".into(),
            source: "test".into(),
            names,
        },
    )
}

fn table(name: &str, col: &str, comment: Option<&str>) -> TableMeta {
    TableMeta {
        name: name.into(),
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

fn run(rows: &[Row], targets: &[(&str, &str)]) -> ontok::domain::view::CustomerView {
    let (schema, names) = schema_of(rows, targets);
    let out = extract(&schema, &names, "fk_fix", "test", "test").unwrap();
    let mut entity_types = Vec::new();
    let mut link_types = Vec::new();
    for it in &out.proposal.items {
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
    project(&SnapshotView { entity_types, link_types })
}

/// 对象 `obj` 的属性（名, 说明）。
fn props(view: &ontok::domain::view::CustomerView, obj: &str) -> Vec<(String, Option<String>)> {
    view.objects
        .iter()
        .find(|o| o.name == obj)
        .map(|o| {
            o.properties.iter().map(|p| (p.name.clone(), p.note.clone())).collect()
        })
        .unwrap_or_default()
}

fn only_prop(view: &ontok::domain::view::CustomerView, obj: &str) -> (String, Option<String>) {
    let p = props(view, obj);
    assert_eq!(p.len(), 1, "「{obj}」应当恰好一个属性，实际 {p:?}");
    p.into_iter().next().unwrap()
}

/// **不变量 1**：同一个 `customer_id`，不管注释写什么，显示名都只有一种。
#[test]
fn test_customer_id_name_comes_from_the_relation_not_the_comment() {
    let rows: Vec<Row> = vec![
        ("t_kehu_biaoshi", "甲表", "customer_id", Some("客户标识")),
        ("t_kehu_id", "乙表", "customer_id", Some("客户ID")),
        ("t_yonghu_id", "丙表", "customer_id", Some("用户id")),
        ("t_yonghu_id2", "丁表", "customer_id", Some("用户 ID")),
        ("t_no_comment", "戊表", "customer_id", None),
        // 关系层的人话名也要跟着改（不是只改一个 `customer_id` 的特例）
        ("t_admin", "己表", "admin_user_id", Some("管理员ID")),
        // 词表里没有的列 ⇒ 走「所属 + 目标对象名」
        ("t_share", "庚表", "share_usage_id", Some("分享用量ID")),
        // 判据的**保守边界**：带了「的」这类结构词的注释不算「只是复述」，
        // 关系表达不了的信息一律保留（真实库的 `share_usage_id` 就是这一类）
        ("t_share2", "辛表", "share_usage_id", Some("关联的分享使用记录")),
    ];
    let view = run(&rows, &[("customers", "客户"), ("admin_users", "后台管理员"), ("share_usage", "分享用量")]);

    for obj in ["甲表", "乙表", "丙表", "丁表", "戊表"] {
        let (name, note) = only_prop(&view, obj);
        assert_eq!(name, "所属客户", "「{obj}」的属性名必须由关系推导，实际 {name:?}");
        assert_eq!(note, None, "「{obj}」那段复述性注释不该留在说明里（换字段名的同一串废话）");
    }
    // 词表/关系两条路给的是同一个答案 ⇒ 不存在「4 种叫法」
    assert_eq!(only_prop(&view, "己表").0, "所属后台管理员");
    assert_eq!(only_prop(&view, "庚表").0, "所属分享用量");
    // 保守边界：带结构词的注释保留原样（宁可留，不静默改写语义）
    assert_eq!(only_prop(&view, "辛表").0, "关联的分享使用记录");

    // **属性名绝不能等于对象名**（「客户」既是对象又是它的属性 → 污染 Q1/Q2）
    for obj in ["甲表", "乙表", "丙表", "丁表", "戊表"] {
        let (name, _) = only_prop(&view, obj);
        assert_ne!(name, "客户", "属性名与对象名同名会污染判读人的 Q1/Q2");
    }
}

/// **不变量 2**：带角色限定的外键**保持原样**。
///
/// 逐列核对真实 `schema.json` 的结论（不是假设）：
/// `main_customer_id` / `sub_customer_id` / `source_customer_id` /
/// `from_customer_id` / `to_customer_id` / `target_customer_id`
/// 在库里**都推不出关系**（没有 `main_customers` 这类表），
/// 所以它们的注释天然保留 —— 这一条由本测试钉死。
#[test]
fn test_role_qualified_foreign_keys_keep_their_comment() {
    let rows: Vec<Row> = vec![
        ("t_main", "主表", "main_customer_id", Some("主账号ID")),
        ("t_sub", "子表", "sub_customer_id", Some("子账号ID")),
        ("t_source", "来源表", "source_customer_id", Some("划拨来源主账号ID")),
        ("t_from", "转出表", "from_customer_id", Some("转出账号（主账号）")),
        ("t_to", "转入表", "to_customer_id", Some("转入账号（子账号）")),
        ("t_target", "模拟表", "target_customer_id", Some("被模拟用户ID")),
        // 无注释的 main_customer_id：走词表 `main_customer_id → 主客户`
        ("t_main_nc", "主表无注释", "main_customer_id", None),
        // 关系推得出、但注释**带限定**的 4 种真实形态：同样必须保留
        ("t_qualified1", "限定一", "customer_id", Some("子账号ID")),
        ("t_qualified2", "限定二", "customer_id", Some("账号ID（余额所属账号）")),
        ("t_qualified3", "限定三", "customer_id", Some("账号ID（统计维度账号）")),
        ("t_qualified4", "限定四", "customer_id", Some("被代扣Open用户ID")),
    ];
    let view = run(&rows, &[("customers", "客户")]);
    for (obj, want) in [
        ("主表", "主账号ID"),
        ("子表", "子账号ID"),
        ("来源表", "划拨来源主账号ID"),
        ("转出表", "转出账号（主账号）"),
        ("转入表", "转入账号（子账号）"),
        ("模拟表", "被模拟用户ID"),
        // 限定型：关系表达不了，注释原样当属性名
        ("限定一", "子账号ID"),
        ("限定二", "账号ID（余额所属账号）"),
        ("限定三", "账号ID（统计维度账号）"),
        ("限定四", "被代扣Open用户ID"),
    ] {
        let (name, _) = only_prop(&view, obj);
        assert_eq!(name, want, "「{obj}」的语义限定被误统一了");
    }
    // 无注释的 main_customer_id 走词表，不是「所属客户」
    assert_eq!(only_prop(&view, "主表无注释").0, "主客户");
}

/// **注释里用的是目标自己的英文名**，也是复述 —— 否则页面上会留着一个
/// **纯英文属性名**。
///
/// 实测（改前的页面）：`扣费流水.license_id` 的注释是 `license 标识`，
/// 含汉字 ⇒ 走「人话照用」⇒ 属性名就是 `license 标识`，
/// 判读人直接看到一个英文属性名。而关系明明已经能给出「所属授权」。
///
/// **变异自证**：删掉 `fk_comment_is_restatement` 里的第 3b 段 ⇒ 本测试变红。
#[test]
fn test_english_restatement_of_the_target_is_not_kept_as_a_name() {
    let rows: Vec<Row> = vec![
        ("t_license", "扣费流水", "license_id", Some("license 标识")),
        ("t_license2", "用量明细", "license_id", Some("授权标识")),
        ("t_order", "客户订阅", "order_id", Some("order编号")),
    ];
    let view = run(&rows, &[("licenses", "授权凭证"), ("order", "订单")]);
    for (obj, want) in [("扣费流水", "所属授权"), ("用量明细", "所属授权"), ("客户订阅", "所属订单")] {
        let (name, note) = only_prop(&view, obj);
        assert_eq!(name, want, "「{obj}」的属性名必须是关系推出来的，英文原样是泄露");
        assert_eq!(note, None, "「{obj}」的复述性注释不该留在说明里");
    }
    // 整份视图里不许再出现这个英文属性名
    let json = serde_json::to_string(&view).unwrap();
    assert!(!json.contains("license 标识"), "英文属性名漏出去了：{json}");
}

/// **不变量 3（S-2 红线）**：低质量英文注释不许借「关系推导」当上属性名。///
/// 这条是**本轮真写错过一次**的地方：第一版把外键分支放在质量门槛**之前**，
/// `api_call_logs.license_id` 的 `License ID` 直接当上了属性名，
/// 被 `tests/test_prop_name_quality.rs` 的既有断言抓住。
#[test]
fn test_low_quality_comment_never_becomes_a_name_even_for_a_foreign_key() {
    let rows: Vec<Row> = vec![
        ("t_license", "授权表", "license_id", Some("License ID")),
        ("t_session", "会话表", "session_id", Some("SessionId")),
    ];
    let view = run(&rows, &[("licenses", "授权凭证")]);
    let json = serde_json::to_string(&view).unwrap();
    for leak in ["License ID", "SessionId", "license_id", "session_id"] {
        assert!(!json.contains(leak), "英文「{leak}」漏进客户视图了：{json}");
    }
    // `license_id` 有词表条目，兜底后仍然是人话；`session_id` 词表有 `会话` 词根
    assert_eq!(only_prop(&view, "授权表").0, "所属授权");
    assert_eq!(only_prop(&view, "会话表").0, "会话");
}

/// 回归护栏：视图层的人话化兜底（`name` 为空时）仍按词表走。
#[test]
fn test_view_level_fallback_still_uses_the_word_list() {
    assert_eq!(humanize_column("customer_id").as_deref(), Some("所属客户"));
    assert_eq!(humanize_column("main_customer_id").as_deref(), Some("主客户"));
    assert_eq!(humanize_column("blob_ref_xyz_id"), None, "S-2：认不出必须 None");
}

/// **数据驱动**：真实源库 91 表 / 964 属性上，外键显示名的**全量**核对。
///
/// 判据一：`customer_id` 的显示名**只允许**是 `所属客户`，
/// 或那 4 条逐列核对过的限定型注释 —— 不允许出现 `客户标识` / `客户`。
/// 判据二：`主客户` / `子账号ID` / `划拨来源主账号ID` 一条都不许少。
///
/// **fail-closed**：读不到脱敏 `schema.json` 就判失败（与 `drop_rate.rs` 同一纪律）。
#[test]
fn test_real_schema_customer_id_has_exactly_one_display_name() {
    let path = std::env::var("ONTOK_SCHEMA_JSON").unwrap_or_else(|_| {
        "/home/myg133/Projects/Anchnet/Ontology/.review-workspace/fix-props/schema.json"
            .to_string()
    });
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "读不到脱敏 schema.json：{path}（{e}）—— 这个测试是外键命名的**回归门禁**，\
             读不到输入时它必须失败，不能安静跳过（drop_rate.rs 同一纪律）。\
             补救：设置 ONTOK_SCHEMA_JSON 指向它。"
        )
    });
    let report: SchemaReport = serde_json::from_str(&raw).expect("schema JSON 解析失败");
    let names_raw = std::fs::read_to_string("src/extract/table_names.json").expect("读不到翻译表");
    let names = TableNames::load(&names_raw).expect("翻译表加载失败");
    let out = extract(&report, &names, "fk_probe", "fk_probe", "fk_probe").expect("抽取失败");

    // 显示名 = 抽取名；空名则回落到 humanize_column（project() 的兜底）
    let mut plain = 0usize;
    let mut qualified: Vec<String> = Vec::new();
    let mut role_kept: Vec<String> = Vec::new();
    for it in &out.proposal.items {
        if it["kind"].as_str() != Some("add_entity_type") {
            continue;
        }
        let p = &it["payload"];
        let table = p["key"].as_str().unwrap_or_default();
        for pr in p["properties"].as_array().cloned().unwrap_or_default() {
            let col = pr["column"].as_str().unwrap_or_default();
            let name = pr["name"].as_str().unwrap_or_default().to_string();
            let shown = if name.trim().is_empty() {
                humanize_column(col).unwrap_or_default()
            } else {
                name
            };
            match col {
                "customer_id" => {
                    if shown == "所属客户" {
                        plain += 1;
                    } else {
                        qualified.push(format!("{table}: {shown}"));
                    }
                }
                // 角色限定的外键：关系推不出 ⇒ 注释必须原样留下
                "main_customer_id" | "sub_customer_id" | "source_customer_id" => {
                    role_kept.push(format!("{col}={shown}"));
                }
                _ => {}
            }
            // 判据一：任何一个属性都不许显示成「客户标识」
            assert_ne!(shown, "客户标识", "{table}/{col} 仍显示成「客户标识」");
        }
    }

    assert_eq!(plain, 33, "37 条 customer_id 里有 33 条应当统一成「所属客户」");
    assert_eq!(
        qualified,
        vec![
            // 这 4 条的注释**逐列核对过**：语义与「所属客户」确实不同
            "app_billing_transactions: 被代扣Open用户ID".to_string(),
            "balance_operation_log: 账号ID（余额所属账号）".to_string(),
            "dim_customer_binding: 账号ID（统计维度账号）".to_string(),
            "frozen_assets: 子账号ID".to_string(),
        ],
        "带限定的 customer_id 必须原样保留，且**只有**这 4 条"
    );
    for want in ["main_customer_id=主客户", "sub_customer_id=子账号ID", "source_customer_id=划拨来源主账号ID"] {
        assert!(
            role_kept.iter().any(|s| s == want),
            "「{want}」不见了 —— 角色限定的外键被误统一了（现有：{role_kept:?}）"
        );
    }
}

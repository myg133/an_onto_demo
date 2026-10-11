//! 丢弃率**判定**（REQ-007 · 修掉「零 assert 的假绿测试」）
//!
//! 跑真实源库的 `schema.json`（脱敏元数据，无业务数据行），报出：
//! **抽出来的属性里有多少在投影成客户视图时被丢弃**。
//!
//! # 为什么要有这个探针
//!
//! 审核报告说「17.8% 静默丢弃」，但那是外部复算的数字。
//! 这个文件把同一件事变成**仓库内可复跑的判定** ——
//! 改形态处理器后重跑，数字必须下降，**而且超线要变红**。
//!
//! # REQ-007：它原来是一个**永远不会红**的测试
//!
//! 原版只有 `println!`、**零个 `assert`** —— 丢弃率从 8.4% 涨回 20%
//! 它照样 `test result: ok`。schema 文件读不到时更是直接 `return`，
//! CI 上等于**完全不设防**。这正是本项目栽过六次的那类「假绿」。
//!
//! 现在有两道硬判据（见 [`MAX_DROP_RATE`] 与下面的 fail-closed 断言）。
//!
//! **零 I/O**：只读本地 `schema.json`，不连任何数据库。
//! schema 路径可用 `ONTOK_SCHEMA_JSON` 覆盖。

use ontok::domain::view::{project, EntityTypeView, LinkTypeView, PropertyTypeView, SnapshotView};
use ontok::extract::introspect::SchemaReport;
use ontok::extract::names::TableNames;
use ontok::extract::propose::extract;

/// 脱敏的真实源库元数据所在目录（项目内，被 gitignore 忽略）。
const DEFAULT_SCHEMA: &str = "../.review-workspace/fix-props/schema.json";
const DEFAULT_NAMES: &str = "src/extract/table_names.json";

/// ── 判据一：丢弃率上限 ──────────────────────────────────────
///
/// # 为什么取 10%
///
/// | 数字 | 含义 | 出处 |
/// | --- | --- | --- |
/// | **17.8%** | 形态处理器**不存在**时的历史丢弃率 | REQ-006 立项依据 |
/// | **8.4%** | REQ-006 形态处理器落地后的当前值（81/964） | 本文件实测 |
/// | **10%** | 本判据 | 取两者之间 |
///
/// 取 10% 的理由：
/// - **必须高于 8.4%**，否则日常波动就红，等于没有判别力；
/// - **必须远低于 17.8%**，这样「有人把形态处理器改坏了」会立刻越线。
///   17.8% 与 10% 之间有 7.8 个百分点、约 75 条属性的余量 ——
///   足够容纳 schema 刷新带来的正常浮动，又足够窄到能抓住真的回归。
/// - 剩下的 8.4% 里约 12 条是**已知未修的屈折形态**（`cycle`✓ / `cycles`✗），
///   下一轮修完这条线还会**继续下降**。判据只卡上限、不卡下限，
///   下一轮改进不会被这个测试挡住。
///
/// **它是回归哨兵，不是精度仪器** —— 目标是「明显变坏就红」，
/// 不是「小数点后一位都要对得上」。
const MAX_DROP_RATE: f64 = 0.10;

// ── 判据二：schema 本身的体量下限 ────────────────────────────
//
// **没有这条，上面那条判据可以被「空输入」骗过去**：
// `total_props == 0` 时 `rate` 被算成 0.0，一个恒过。
// 这是本项目最熟的一种 fail-open —— 判据存在，但没有判别力。
//
// 下限刻意压在实测值下方（91 表 / 964 属性），留出 schema 刷新的余量；
// 但仍是**明确的正数下界**：schema 被清空、被换成占位文件、
// 或抽取器整体失效时，本测试必须变红而不是安静地报 0%。
const MIN_TABLES: usize = 80;
const MIN_PROPS: usize = 900;

fn schema_path() -> String {
    std::env::var("ONTOK_SCHEMA_JSON").unwrap_or_else(|_| DEFAULT_SCHEMA.to_string())
}

/// 逐属性对齐：把「抽取出的属性」和「投影后活下来的属性」按
/// （对象名, 源列名）配对，算出被丢弃的那批。
#[test]
fn real_schema_drop_rate_report() {
    let path = schema_path();

    // ── fail-closed：读不到 schema 就**判失败**，不是「跳过」──
    //
    // REQ-007 之前这里是 `return`：文件不在就安静通过。
    // 一个「测不出问题」的门禁比没有门禁更糟 —— 它让人以为这一项有人守着。
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "读不到脱敏 schema.json：{path}（{e}）\n\
             这个测试是丢弃率的**回归门禁**，读不到输入时它必须失败 —— \
             静默跳过等于没有门禁。\n\
             补救：设置 ONTOK_SCHEMA_JSON 指向脱敏的 schema.json，\
             或放到 {DEFAULT_SCHEMA}（文件被 gitignore，需自行准备）。"
        )
    });

    let report: SchemaReport = serde_json::from_str(&raw).expect("schema JSON 解析失败");
    let names_raw = std::fs::read_to_string(DEFAULT_NAMES).expect("读不到 table_names.json");
    let names = TableNames::load(&names_raw).expect("翻译表加载失败");

    let outcome = extract(&report, &names, "dropprobe", "dropprobe", "dropprobe")
        .expect("抽取失败");

    // 提案 items → SnapshotView（与 cli.rs 走同一条投影路径）
    let mut entity_types = Vec::new();
    let mut link_types = Vec::new();
    for it in &outcome.proposal.items {
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

    let total_props: usize = entity_types.iter().map(|e| e.properties.len()).sum();
    let cv = project(&SnapshotView { entity_types: entity_types.clone(), link_types });
    let kept: usize = cv.objects.iter().map(|o| o.properties.len()).sum();
    let dropped = total_props.saturating_sub(kept);
    let rate = if total_props == 0 { 0.0 } else { dropped as f64 / total_props as f64 };

    println!(
        "── 真实源库丢弃率 ──\n  表 {} · 抽出属性 {} · 视图存活 {} · 丢弃 {} · 丢弃率 {:.1}%",
        report.tables.len(),
        total_props,
        kept,
        dropped,
        rate * 100.0
    );

    // 逐属性列出被丢弃的（对象 / 源列）—— 任务 B 验收 2 的 diff 输入。
    let kept_names: std::collections::HashSet<(String, String)> = cv
        .objects
        .iter()
        .flat_map(|o| o.properties.iter().map(|p| (o.name.clone(), p.name.clone())))
        .collect();
    let mut dropped_list: Vec<(String, String)> = Vec::new();
    for e in &entity_types {
        for p in &e.properties {
            let shown = if p.name.trim().is_empty() {
                ontok::domain::view::humanize_column(&p.column)
            } else {
                Some(p.name.clone())
            };
            if let Some(n) = shown {
                if !kept_names.contains(&(e.display_name.clone(), n)) {
                    dropped_list.push((e.display_name.clone(), p.column.clone()));
                }
            } else {
                dropped_list.push((e.display_name.clone(), p.column.clone()));
            }
        }
    }
    println!("  被丢弃的属性（前 200 条，格式 对象/源列）:");
    for (o, c) in dropped_list.iter().take(200) {
        println!("    {o}/{c}");
    }
    println!("  被丢弃总数: {}", dropped_list.len());

    // ── 判据二：输入体量（先查输入，再查结果）──────────────────
    //
    // 顺序刻意如此：**先证明「测的是真东西」，再看比例**。
    // 否则一个空 schema 会得到 rate = 0.0 并「通过」判据一。
    assert!(
        report.tables.len() >= MIN_TABLES,
        "schema 只读到 {} 张表（判据要求 ≥ {MIN_TABLES}）—— 输入不对，\
         后面的丢弃率是拿残缺输入算出来的，没有意义",
        report.tables.len()
    );
    assert!(
        total_props >= MIN_PROPS,
        "只抽出 {total_props} 个属性（判据要求 ≥ {MIN_PROPS}）—— \
         抽取器可能整体失效了，丢弃率会假性变好"
    );

    // ── 判据一：丢弃率上限 ────────────────────────────────────
    assert!(
        rate <= MAX_DROP_RATE,
        "丢弃率 {:.1}% 超过上限 {:.1}%（{dropped}/{total_props} 被丢弃）\n\
         改动形态处理器或词表时丢了这么多属性 = 客户视图在变少，\
         Q2（属性识别率 ≥70%）会被直接拖低。",
        rate * 100.0,
        MAX_DROP_RATE * 100.0
    );
}

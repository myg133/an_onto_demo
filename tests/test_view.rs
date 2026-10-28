//! Task 7 · 客户视图投影测试
//!
//! 验收 AC-005。测试名带 `test_view_` 前缀。

use ontok::domain::view::{
    project, scan_modeling_terms, CustomerView, EntityTypeView, LinkTypeView, PropertyTypeView,
    SnapshotView,
};

fn demo_snapshot() -> SnapshotView {
    SnapshotView {
        entity_types: vec![
            EntityTypeView {
                key: "customer".into(),
                display_name: "客户".into(),
                description: Some("签约的企业主体".into()),
                aliases: vec!["用户".into(), "客户账号".into()],
                parent: None,
                disjoint_with: vec!["supplier".into()],
                properties: vec![
                    PropertyTypeView {
                        name: "客户编号".into(),
                        column: "customer_no".into(),
                        nullable: false,
                        note: Some("营业执照号".into()),
                        sample: Some("«已脱敏»".into()),
                    },
                    PropertyTypeView {
                        name: "客户名称".into(),
                        column: "customer_name".into(),
                        nullable: true,
                        note: None,
                        sample: Some("某某科技".into()),
                    },
                ],
                evidence: vec![serde_json::json!({"table": "t025", "column": "customer_no"})],
            },
            EntityTypeView {
                key: "subscription".into(),
                display_name: "客户订阅".into(),
                description: Some("客户购买的调用额度套餐".into()),
                aliases: vec!["订阅".into()],
                parent: Some("customer".into()),
                disjoint_with: vec![],
                properties: vec![PropertyTypeView {
                    name: "客户编号".into(),
                    column: "customer_id".into(),
                    nullable: false,
                    note: None,
                    sample: Some("«已脱敏»".into()),
                }],
                evidence: vec![],
            },
        ],
        link_types: vec![LinkTypeView {
            key: "customer_has_subscription".into(),
            from_type: "customer".into(),
            to_type: "subscription".into(),
            cardinality: "1:N".into(),
            semantic: "has".into(),
        }],
    }
}

// ── 红线：零建模术语 ──────────────────────────────────────

#[test]
fn test_view_biz_view_never_exposes_modeling_terms() {
    let v = project(&demo_snapshot());
    let s = serde_json::to_string(&v).unwrap();
    let leaked = scan_modeling_terms(&s);
    assert!(leaked.is_empty(), "客户视图泄露了建模术语：{leaked:?}\n实际输出：{s}");
}

#[test]
fn test_view_omits_reserved_fields() {
    let v: CustomerView = project(&demo_snapshot());
    let raw = serde_json::to_value(&v).unwrap();
    let obj = raw.as_object().unwrap();
    for k in [
        "action_types", "functions", "extracted_by", "proposed_by",
        "approved_by", "evidence", "ordinal", "disjoint_with", "key",
    ] {
        assert!(!obj.contains_key(k), "客户视图不该出现顶层字段 {k}");
    }
    // 对象层也不该有 key / evidence / disjoint_with
    let e0 = obj["objects"][0].as_object().unwrap();
    for k in ["key", "evidence", "disjoint_with"] {
        assert!(!e0.contains_key(k), "对象层不该有 {k}");
    }
}

// ── 人话化 ────────────────────────────────────────────────

#[test]
fn test_view_parent_is_shown_as_name_not_key() {
    let v = project(&demo_snapshot());
    let sub = v.objects.iter().find(|o| o.name == "客户订阅").unwrap();
    assert_eq!(sub.belongs_to.as_deref(), Some("客户"), "上层应显示名字而非 key");
    assert_ne!(sub.belongs_to.as_deref(), Some("customer"), "不能暴露 key");
}

#[test]
fn test_view_relations_use_human_words() {
    let v = project(&demo_snapshot());
    assert_eq!(v.relations.len(), 1);
    let r = &v.relations[0];
    assert_eq!(r.from, "客户");
    assert_eq!(r.to, "客户订阅");
    assert_eq!(r.how_many, "一对多");
    assert_eq!(r.meaning, "拥有");
}

#[test]
fn test_view_keeps_aliases() {
    let v = project(&demo_snapshot());
    let c = v.objects.iter().find(|o| o.name == "客户").unwrap();
    assert_eq!(c.aliases, vec!["用户", "客户账号"], "别名是客户认得的词，必须留");
}

#[test]
fn test_view_property_sample_survives() {
    let v = project(&demo_snapshot());
    let c = v.objects.iter().find(|o| o.name == "客户").unwrap();
    let p = &c.properties[1];
    assert_eq!(p.sample.as_deref(), Some("某某科技"));
    assert!(!p.required);
    assert!(c.properties[0].required, "非空属性应标为必填");
}

// ── 边界情况 ──────────────────────────────────────────────

#[test]
fn test_view_relation_to_unknown_type_is_dropped() {
    let mut s = demo_snapshot();
    s.link_types.push(LinkTypeView {
        key: "bad".into(),
        from_type: "customer".into(),
        to_type: "不存在的类型".into(),
        cardinality: "1:1".into(),
        semantic: "has".into(),
    });
    let v = project(&s);
    assert_eq!(v.relations.len(), 1, "指向不存在对象的关系应被丢弃");
}

#[test]
fn test_view_dangling_parent_becomes_none() {
    let mut s = demo_snapshot();
    s.entity_types[1].parent = Some("不存在".into());
    let v = project(&s);
    let sub = v.objects.iter().find(|o| o.name == "客户订阅").unwrap();
    assert_eq!(sub.belongs_to, None, "父对象不存在时应为 None，不暴露原始 key");
}

#[test]
fn test_view_empty_snapshot_is_empty_not_error() {
    let v = project(&SnapshotView { entity_types: vec![], link_types: vec![] });
    assert!(v.objects.is_empty());
    assert!(v.relations.is_empty());
    let s = serde_json::to_string(&v).unwrap();
    assert!(scan_modeling_terms(&s).is_empty(), "空视图同样不许泄露术语");
}

#[test]
fn test_view_blacklist_scanner_catches_a_planted_term() {
    // 守门器本身要有效：故意塞一个 "cardinality" 进去，必须被抓到
    let bad = r#"{"objects":[],"relations":[],"cardinality":"1:N"}"#;
    let hit = scan_modeling_terms(bad);
    assert!(hit.contains(&"cardinality".to_string()), "黑名单扫描器失效了：{hit:?}");
}

#[test]
fn test_view_falls_back_to_humanized_column_when_name_missing() {
    // 抽取侧漏给属性名时，用列名人话化兜底（customer_id → 所属客户）
    let mut s = demo_snapshot();
    s.entity_types[0].properties[0].name = "".into();
    s.entity_types[0].properties[0].column = "customer_id".into();
    let v = project(&s);
    let c = v.objects.iter().find(|o| o.name == "客户").unwrap();
    assert_eq!(c.properties[0].name, "所属客户", "空名应回退到列名人话化");
}

#[test]
fn test_view_drops_property_when_column_unhumanizable() {
    // **S-2 反例**：列名表外且没给 name 时，原样透出 `blob_ref_xyz_id`
    // 就是把技术串当业务名给人看。宁可丢掉这个属性。
    let mut s = demo_snapshot();
    s.entity_types[0].properties[0].name = "".into();
    s.entity_types[0].properties[0].column = "blob_ref_xyz_id".into();
    let v = project(&s);
    let c = v.objects.iter().find(|o| o.name == "客户").unwrap();
    assert!(
        !c.properties.iter().any(|p| p.name.contains("blob")),
        "表外列不得原样出现在客户视图：{:?}",
        c.properties
    );
}

#[test]
fn test_view_humanizes_generic_column_via_business_word() {
    // 表外但可识别的列（order_no）应被翻成人话，而不是丢掉或原样透出
    let mut s = demo_snapshot();
    s.entity_types[0].properties[0].name = "".into();
    s.entity_types[0].properties[0].column = "order_no".into();
    let v = project(&s);
    let c = v.objects.iter().find(|o| o.name == "客户").unwrap();
    assert_eq!(c.properties[0].name, "订单编号", "可识别列应翻成人话保留");
}

#[test]
fn test_view_never_publishes_key_as_display_name() {
    // **S-1 反例**：抽取侧漏翻人话，display_name 退化成 key 本身
    // → 该对象整个不发布，关系/父对象也查不到它 → 不会泄露标识符
    let mut s = demo_snapshot();
    s.entity_types[0].display_name = "customer".into(); // 退化成 key
    let v = project(&s);
    let json = serde_json::to_string(&v).unwrap();
    assert!(
        !json.contains("\"customer\""),
        "退化对象不得以 key 形式发布：{json}"
    );
    // 指向它的关系也随之丢弃（两端解析不到可用人名）
    assert!(v.relations.is_empty(), "指向退化对象的关系应被丢弃");
}

#[test]
fn test_view_blacklist_scanner_is_case_insensitive() {
    // **S-3 反例**：守门词大小写混写也要抓到，否则守门器形同虚设
    let bad = r#"{"objects":[],"relations":[],"CARDINALITY":"1:N"}"#;
    let hit = scan_modeling_terms(bad);
    assert!(
        hit.contains(&"cardinality".to_string()),
        "黑名单扫描应大小写不敏感：{hit:?}"
    );
}

// ── REQ-003 · 守门词分层匹配（AC-105）────────────────────
//
// 动机：真实源库里有 `license_key`/`public_key`/`config_key`/`keywords` 等 11 个
// 含 `key` 的对象。原实现是子串匹配 → 「License Key」（授权密钥，正常业务概念）
// 会被判成建模术语泄露，运行时守门直接 exit 2 关闭整个客户视图。
// 改成：中文词包含匹配 + 英文词按词元/整体匹配。

#[test]
fn test_view_guard_allows_real_business_key_concepts() {
    // 这些是源库里真实存在的业务概念，绝不能被误杀
    for ok in [
        "授权密钥",           // license_key 的中文译名
        "License Key",
        "API Key",
        "public_key",
        "config_key",
        "cache_key",
        "aggregate_key",
        "idempotency_key",
        "keywords",
        "关键词",
        "访问密钥",
    ] {
        let hit = scan_modeling_terms(ok);
        assert!(hit.is_empty(), "正常业务概念「{ok}」被误判为建模术语：{hit:?}");
    }
}

#[test]
fn test_view_guard_still_catches_leaked_identifiers() {
    // **S-3 回归点不能丢**：大小写绕过必须仍然被抓
    for bad in ["Key", "KEY", "CARDINALITY", "cardinality", "primary_key", "PK", "pk"] {
        let hit = scan_modeling_terms(bad);
        assert!(!hit.is_empty(), "建模术语「{bad}」必须被抓到");
    }
}

#[test]
fn test_view_guard_still_catches_cjk_terms_anywhere_in_text() {
    // 中文不分词 → 出现在句中也该抓
    for bad in ["该字段是主键", "基数一对多", "外键约束", "证据来源", "客户主键ID"] {
        let hit = scan_modeling_terms(bad);
        assert!(!hit.is_empty(), "中文建模术语「{bad}」必须被抓到");
    }
}

#[test]
fn test_view_guard_catches_key_in_serialized_view() {
    // 真实泄露形态：序列化后的 JSON 里键名是 "key"
    let v = project(&demo_snapshot());
    let mut raw = serde_json::to_value(&v).unwrap();
    raw["objects"][0]["key"] = serde_json::json!("customer");
    let s = serde_json::to_string(&raw).unwrap();
    let hit = scan_modeling_terms(&s);
    assert!(!hit.is_empty(), "视图里出现 key 字段必须被抓：{s}");
}

// ── REQ-003 · 基数记号零透出（B1 · AC-005/AC-106）────────
//
// 独立审核实测：真实 E2E 产出的客户视图 51 条关系 `how_many` **全是** `N:1`，
// 而 `cardinality_cn` 只认 `1:1`/`1:N`/`N:M` —— `N:1` 落进 `_ =>` 分支
// 把内部记号原样透给客户；而守门词里又没有记号形态，**抓不到**（结构性失明）。
//
// 所以这里刻意**不写成「测 N:1 这一个值」** —— 那样将来再多一种记号照样漏。
// 判据是**不变量**，对任意记号都成立：
//   「要么翻成人话，要么被守门器当场抓住」= 绝不会静默出图。

/// 已知记号 → 约定译文。新增记号时**先改这里再改 `cardinality_cn`**。
const KNOWN_CARDINALITIES: &[(&str, &str)] = &[
    ("1:1", "唯一对应"),
    ("1:N", "一对多"),
    ("N:M", "多对多"),
    ("N:1", "多对一"),
    // 2026-10-28 客户实测：抽取端产出的是下面这些**等价写法**，
    // 原来只认 N:M ⇒ 落到 _ => 原样透出 ⇒ 发布闸按「未翻译的基数记号」拒掉。
    ("N:N", "多对多"),
    ("M:N", "多对多"),
    ("M:M", "多对多"),
    ("n:n", "多对多"),
    ("n:1", "多对一"),
    ("1:n", "一对多"),
];

/// 造一个只含一条关系、基数给定的快照。
fn snapshot_with_cardinality(card: &str) -> SnapshotView {
    let mut s = demo_snapshot();
    s.link_types[0].cardinality = card.into();
    s
}

#[test]
fn test_view_translates_every_known_cardinality_notation() {
    // 判据 ①：**已知**记号必须翻成人话，不能靠「守门器抓到」蒙混过关。
    // 变异：删掉 `cardinality_cn` 的 `N:1` 分支 ⇒ 本测试立刻变红。
    for (raw, want) in KNOWN_CARDINALITIES {
        let v = project(&snapshot_with_cardinality(raw));
        assert_eq!(v.relations.len(), 1, "基数 {raw} 不该让关系被丢掉");
        let got = &v.relations[0].how_many;
        assert_eq!(got, want, "基数记号 {raw} 应当翻成人话，实际 {got}");
        assert!(
            !got.contains(':') && !got.contains(".."),
            "翻译结果不该还留着记号形态：{got}"
        );
    }
}

#[test]
fn test_view_no_cardinality_notation_passes_silently() {
    // 判据 ②：**任意**记号（含 `cardinality_cn` 还不认识的形态）
    // 都不可能静默进入客户视图 —— 漏翻必须让整份视图被守门器拦下。
    // 变异：把第三档守门词（记号形态匹配）削弱/删掉 ⇒ 本测试立刻变红。
    //
    // 形态都是「单字符 : 单字符」或「单字符 .. 单字符」—— 与抽取侧
    // （`propose.rs` 只产出 `1:N` / `N:1`）和第三档守门词的覆盖面一致。
    let notations = [
        "1:1", "1:N", "N:M", "N:1", // 已知
        // 未知：M:N / n:1 已翻成人话（见 KNOWN 表），挪出去了
        "1:M", "0:1", "1:0", "1..*", "0..1", "N..M", // 未知
    ];
    for raw in notations {
        let v = project(&snapshot_with_cardinality(raw));
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(v.relations.len(), 1, "基数 {raw} 不该让关系被丢掉");
        let translated = KNOWN_CARDINALITIES
            .iter()
            .any(|(k, w)| *k == raw && v.relations[0].how_many == *w);
        let caught = scan_modeling_terms(&json);
        assert!(
            translated || !caught.is_empty(),
            "基数记号「{raw}」既没被翻成人话、也没被守门器抓住 —— \
             它会原样透给客户。视图：{json}"
        );
        if translated {
            assert!(
                caught.is_empty(),
                "已翻成中文的记号不该再被报出来：{caught:?}\n{json}"
            );
            assert!(
                !json.contains(raw),
                "记号「{raw}」已翻成中文，JSON 里不该还留着原文：{json}"
            );
        }
    }
}

// ── REQ-003 · 关系语义零透出（X2 · 与 B1 同构 · AC-005/AC-106）──
//
// `semantic_cn` 的 `_ =>` 兜底与 B1 修之前的 `cardinality_cn` **一模一样**：
// 未识别的语义原样透给客户。复审实测 `belongs_to` **不被任何守门词覆盖**，
// 把真实产物 51 条 `meaning` 全改回 `belongs_to` 仍 exit 0。
// 今天真实产物的 `meaning` 全是「归属于」—— 零泄漏是**运气**，不是防线。
//
// 判据与 B1 同构，是**不变量**而不是「测 belongs_to 这一个值」：
//   「要么翻成中文人话，要么被守门器当场抓住」= 绝不会静默出图。

/// 已知语义 → 约定译文。新增语义时**先改这里再改 `semantic_cn`**。
const KNOWN_SEMANTICS: &[(&str, &str)] = &[
    ("has", "拥有"),
    ("belongs_to", "归属于"),
    ("depends_on", "依赖"),
    ("references", "关联到"),
    ("records", "记录了"),
    ("derived_from", "算自"),
];

/// 造一个只含一条关系、语义给定的快照。
fn snapshot_with_semantic(sem: &str) -> SnapshotView {
    let mut s = demo_snapshot();
    s.link_types[0].semantic = sem.into();
    s
}

#[test]
fn test_view_translates_every_known_semantic() {
    // 判据 ①：**已知**语义必须翻成中文人话，不能靠「守门器抓到」蒙混过关。
    // 变异：删掉 `semantic_cn` 的某个分支 ⇒ 本测试立刻变红。
    for (raw, want) in KNOWN_SEMANTICS {
        let v = project(&snapshot_with_semantic(raw));
        assert_eq!(v.relations.len(), 1, "语义 {raw} 不该让关系被丢掉");
        let got = &v.relations[0].meaning;
        assert_eq!(got, want, "语义 {raw} 应当翻成中文人话，实际 {got}");
    }
}

#[test]
fn test_view_no_untranslated_semantic_passes_silently() {
    // 判据 ②：**任意**语义（含 `semantic_cn` 还不认识的）都不可能静默
    // 进入客户视图。变异：把第四档守门（语义形态判定）削弱/删掉 ⇒ 立刻变红。
    //
    // 后半组是词表里**没有**的拼法 —— 这一档判的是**形态**（一个汉字都没有
    // 却含 ASCII 词元），不是枚举；写成枚举的话这 6 个会全部漏过去。
    let semantics = [
        "has", "belongs_to", "depends_on", "references", "records", "derived_from", // 已知
        "aggregates", "is_a_of", "consists_of", "memberOf", "PART_OF", "linked_to", // 未知
    ];
    for raw in semantics {
        let v = project(&snapshot_with_semantic(raw));
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(v.relations.len(), 1, "语义 {raw} 不该让关系被丢掉");
        let translated = KNOWN_SEMANTICS
            .iter()
            .any(|(k, w)| *k == raw && v.relations[0].meaning == *w);
        let caught = scan_modeling_terms(&json);
        assert!(
            translated || !caught.is_empty(),
            "关系语义「{raw}」既没被翻成中文人话、也没被守门器抓住 —— \
             它会原样透给客户。视图：{json}"
        );
        if translated {
            assert!(
                caught.is_empty(),
                "已翻成中文的语义不该再被报出来：{caught:?}\n{json}"
            );
            // 注意：**不能**照抄 B1 那条 `!json.contains(raw)` ——
            // 客户视图里有一个键就叫 `belongs_to`（`EntityView::belongs_to`），
            // 翻对了全文里也仍然有这个键名，那条断言必然假红。
            // 真正该查的是**取值**：语义字段里不许还留着原文。
            let shown = &v.relations[0].meaning;
            assert!(
                !shown.contains(raw),
                "语义「{raw}」已翻成中文，取值里不该还留着原文：{shown}"
            );
        }
    }
}

#[test]
fn test_view_guard_does_not_mistake_the_belongs_to_key_for_a_semantic_leak() {
    // 反向：客户视图里**有一个键就叫 `belongs_to`**（`EntityView::belongs_to`）——
    // 它是字段名，不是给客户看的关系语义。第四档只看 `meaning` 字段的取值；
    // 写成全文本扫的话，**每一份**视图都会误杀（= 永远出不了图）。
    let v = project(&demo_snapshot());
    let json = serde_json::to_string(&v).unwrap();
    assert!(
        json.contains("belongs_to"),
        "本用例的前提失效：序列化后的视图里已经没有 belongs_to 这个键了"
    );
    let hit = scan_modeling_terms(&json);
    assert!(
        hit.is_empty(),
        "belongs_to 作为**键名**不该被判成语义泄漏（会关掉整份客户视图）：{hit:?}\n{json}"
    );
}

// ── REQ-003 收尾复审 V1 · 第四档语义守门对「中英混排」fail-open ──
//
// 旧判据是「一个汉字都没有却含 ASCII 词元」——只要有一个汉字就放行。
// 「拥有 belongs_to」里的「拥有」把它放了，`belongs_to` 原样透给客户。
// **这个方向的失败是 fail-open（发错数据给客户），前三轮挖到的都是 fail-closed（拒发）**。

#[test]
fn test_view_guard_catches_semantic_with_chinese_prefix() {
    for leak in [
        "拥有 belongs_to",
        "归属于 N:1",
        "一对多 has",
        "主键 key",
        "contains cardinality",
    ] {
        let v = format!(
            r#"{{"from":"A","to":"B","how_many":"多对一","meaning":"{leak}"}}"#
        );
        let hit = scan_modeling_terms(&v);
        assert!(!hit.is_empty(), "语义值「{leak}」混着英文却没被抓到：{v}");
    }
}

#[test]
fn test_view_guard_still_allows_pure_chinese_semantics() {
    // 收紧判据后**不能**反过来误杀正常中文语义 ——
    // 这是复审指出的「过严侧无守」：删掉实现这类测试会绿（改坏了），说明它有鉴别力。
    for ok in ["归属于", "拥有", "多对多", "一对多", "唯一对应", "组合"] {
        let v = format!(
            r#"{{"from":"A","to":"B","how_many":"多对一","meaning":"{ok}"}}"#
        );
        let hit = scan_modeling_terms(&v);
        assert!(hit.is_empty(), "纯中文语义「{ok}」被误杀：{hit:?}");
    }
}

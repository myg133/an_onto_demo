//! 快照 diff —— **纯逻辑，零 I/O**。
//!
//! 回答一个问题：**这一版比上一版，本体变了什么？**
//!
//! # MVP 只做四类
//!
//! - `Added` / `Removed` —— 按 `key` 比对
//! - `Modified` —— 同 key 的标量字段变了
//! - `Hierarchy` —— `parent` 变了。**先当普通 Modified 处理**
//!   （影响面分析属暂缓项，影响面要问「谁依赖它」，那是查询不是 diff）
//!
//! # 必须跳过的
//!
//! **`evidence`** —— 证据每次抽取都会变（表名列名可能重跑一次就不同），
//! 它不是本体演进。把它算进 diff 会让每次 diff 都几百条噪声，
//! 评审的人会开始无视 diff —— 那比漏报更危险。

use crate::domain::snapshot::{EntityType, LinkType, Snapshot};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffKind {
    Added,
    Removed,
    Modified,
    /// `parent` 变化。语义上独立成类，但 MVP 按 Modified 输出
    Hierarchy,
}

impl DiffKind {
    pub fn as_str(self) -> &'static str {
        match self {
            DiffKind::Added => "added",
            DiffKind::Removed => "removed",
            DiffKind::Modified => "modified",
            DiffKind::Hierarchy => "hierarchy",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DiffItem {
    pub kind: DiffKind,
    /// 实体类型 key 或关系 key
    pub target: String,
    /// 变化了哪个字段（Added/Removed 时为 None）
    pub field: Option<String>,
    pub before: Option<Value>,
    pub after: Option<Value>,
}

impl DiffItem {
    /// 人话一行 —— 给 CLI 与客户看
    pub fn summary(&self) -> String {
        let k = self.kind.as_str();
        match (&self.field, &self.before, &self.after) {
            (Some(f), b, a) => format!(
                "{} {} · {}：{} → {}",
                k,
                self.target,
                f,
                render(b.as_ref()),
                render(a.as_ref())
            ),
            _ => format!("{} {}", k, self.target),
        }
    }
}

fn render(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => "（无）".to_string(),
        Some(Value::String(s)) if s.is_empty() => "（空串）".to_string(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(a)) if a.is_empty() => "[]".to_string(),
        Some(other) => other.to_string(),
    }
}

/// 比较两个快照。
pub fn diff_snapshots(before: &Snapshot, after: &Snapshot) -> Vec<DiffItem> {
    let mut out = Vec::new();
    diff_entities(before, after, &mut out);
    diff_links(before, after, &mut out);
    out
}

fn diff_entities(before: &Snapshot, after: &Snapshot, out: &mut Vec<DiffItem>) {
    let bm: std::collections::BTreeMap<&str, &EntityType> =
        before.entity_types.iter().map(|e| (e.key.as_str(), e)).collect();
    let am: std::collections::BTreeMap<&str, &EntityType> =
        after.entity_types.iter().map(|e| (e.key.as_str(), e)).collect();

    // 新增
    for (k, a) in &am {
        if !bm.contains_key(k) {
            out.push(DiffItem {
                kind: DiffKind::Added,
                target: (*k).to_string(),
                field: None,
                before: None,
                after: Some(entity_json(a)),
            });
        }
    }
    // 删除
    for (k, b) in &bm {
        if !am.contains_key(k) {
            out.push(DiffItem {
                kind: DiffKind::Removed,
                target: (*k).to_string(),
                field: None,
                before: Some(entity_json(b)),
                after: None,
            });
        }
    }
    // 修改
    for (k, b) in &bm {
        let Some(a) = am.get(k) else { continue };
        // 标量字段
        for (field, bv, av) in [
            ("display_name", sv(&b.display_name), sv(&a.display_name)),
            ("description", opt_str(&b.description), opt_str(&a.description)),
            ("aliases", arr(&b.aliases), arr(&a.aliases)),
            ("disjoint_with", arr(&b.disjoint_with), arr(&a.disjoint_with)),
        ] {
            if bv != av {
                out.push(DiffItem {
                    kind: DiffKind::Modified,
                    target: (*k).to_string(),
                    field: Some(field.to_string()),
                    before: Some(bv),
                    after: Some(av),
                });
            }
        }
        // 层级单独成类
        if b.parent != a.parent {
            out.push(DiffItem {
                kind: DiffKind::Hierarchy,
                target: (*k).to_string(),
                field: Some("parent".into()),
                before: Some(opt_str(&b.parent)),
                after: Some(opt_str(&a.parent)),
            });
        }
        // 属性：按 column 比对
        diff_properties(*k, b, a, out);
        // **evidence 刻意不比** —— 见文件头说明
    }
}

fn diff_properties(key: &str, b: &EntityType, a: &EntityType, out: &mut Vec<DiffItem>) {
    use std::collections::BTreeMap;
    let bp: BTreeMap<&str, &crate::domain::snapshot::Property> =
        b.properties.iter().map(|p| (p.column.as_str(), p)).collect();
    let ap: BTreeMap<&str, &crate::domain::snapshot::Property> =
        a.properties.iter().map(|p| (p.column.as_str(), p)).collect();

    for (col, np) in &ap {
        if !bp.contains_key(col) {
            out.push(DiffItem {
                kind: DiffKind::Added,
                target: format!("{key}.{col}"),
                field: Some("property".into()),
                before: None,
                after: Some(prop_json(np)),
            });
        }
    }
    for (col, op) in &bp {
        if !ap.contains_key(col) {
            out.push(DiffItem {
                kind: DiffKind::Removed,
                target: format!("{key}.{col}"),
                field: Some("property".into()),
                before: Some(prop_json(op)),
                after: None,
            });
        }
    }
    for (col, op) in &bp {
        let Some(np) = ap.get(col) else { continue };
        for (f, ov, nv) in [
            ("name", sv(&op.name), sv(&np.name)),
            ("nullable", Value::from(op.nullable), Value::from(np.nullable)),
            ("note", opt_str(&op.note), opt_str(&np.note)),
        ] {
            if ov != nv {
                out.push(DiffItem {
                    kind: DiffKind::Modified,
                    target: format!("{key}.{col}"),
                    field: Some(f.to_string()),
                    before: Some(ov),
                    after: Some(nv),
                });
            }
        }
    }
}

fn diff_links(before: &Snapshot, after: &Snapshot, out: &mut Vec<DiffItem>) {
    let bm: std::collections::BTreeMap<&str, &LinkType> =
        before.link_types.iter().map(|l| (l.key.as_str(), l)).collect();
    let am: std::collections::BTreeMap<&str, &LinkType> =
        after.link_types.iter().map(|l| (l.key.as_str(), l)).collect();

    for (k, a) in &am {
        if !bm.contains_key(k) {
            out.push(DiffItem {
                kind: DiffKind::Added,
                target: (*k).to_string(),
                field: Some("link_type".into()),
                before: None,
                after: Some(serde_json::to_value(a).unwrap_or(Value::Null)),
            });
        }
    }
    for (k, b) in &bm {
        if !am.contains_key(k) {
            out.push(DiffItem {
                kind: DiffKind::Removed,
                target: (*k).to_string(),
                field: Some("link_type".into()),
                before: Some(serde_json::to_value(b).unwrap_or(Value::Null)),
                after: None,
            });
        }
    }
    for (k, b) in &bm {
        let Some(a) = am.get(k) else { continue };
        for (f, bv, av) in [
            ("from_type", sv(&b.from_type), sv(&a.from_type)),
            ("to_type", sv(&b.to_type), sv(&a.to_type)),
            ("cardinality", sv(&b.cardinality), sv(&a.cardinality)),
            ("semantic", sv(&b.semantic), sv(&a.semantic)),
        ] {
            if bv != av {
                out.push(DiffItem {
                    kind: DiffKind::Modified,
                    target: (*k).to_string(),
                    field: Some(f.to_string()),
                    before: Some(bv),
                    after: Some(av),
                });
            }
        }
    }
}

/// `&String` / `&str` → JSON 字符串值。
/// `Value::from(&String)` **不存在**（只有 `From<&str>` 和 `From<String>`），
/// 所以统一走这个 helper，免得每处都写 `.as_str()`。
fn sv(s: &str) -> Value {
    Value::from(s)
}

fn opt_str(s: &Option<String>) -> Value {
    match s {
        None => Value::Null,
        Some(v) => Value::from(v.as_str()),
    }
}

fn arr(v: &[String]) -> Value {
    Value::from(v)
}

/// 实体类型的可比投影 —— **不含 evidence**。
fn entity_json(e: &EntityType) -> Value {
    serde_json::json!({
        "key": e.key,
        "display_name": e.display_name,
        "description": e.description,
        "aliases": e.aliases,
        "parent": e.parent,
        "disjoint_with": e.disjoint_with,
        "properties": e.properties.iter().map(prop_json).collect::<Vec<_>>(),
    })
}

fn prop_json(p: &crate::domain::snapshot::Property) -> Value {
    serde_json::json!({
        "name": p.name, "column": p.column, "nullable": p.nullable, "note": p.note
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::snapshot::Evidence;

    fn ent(key: &str, name: &str) -> EntityType {
        EntityType {
            key: key.into(),
            display_name: name.into(),
            description: None,
            aliases: vec![],
            parent: None,
            disjoint_with: vec![],
            properties: vec![],
            evidence: vec![Evidence { table: "t".into(), column: "c".into(), claim: "x".into() }],
        }
    }

    #[test]
    fn test_diff_ignores_evidence_entirely() {
        // **本文件最容易写错的地方**：evidence 变了但本体没变 → 必须零 diff
        let mut a = Snapshot::new(uuid::Uuid::nil(), 1);
        a.entity_types = vec![ent("customer", "客户")];
        let mut b = a.clone();
        b.entity_types[0].evidence = vec![
            Evidence { table: "t2".into(), column: "c2".into(), claim: "别的说法".into() },
        ];
        let d = diff_snapshots(&a, &b);
        assert!(d.is_empty(), "evidence 变化不得产生 diff：{d:?}");
    }

    #[test]
    fn test_diff_detects_added_and_removed() {
        let mut a = Snapshot::new(uuid::Uuid::nil(), 1);
        a.entity_types = vec![ent("customer", "客户")];
        let mut b = a.clone();
        b.entity_types.push(ent("t064", "订单"));
        let d = diff_snapshots(&a, &b);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].kind, DiffKind::Added);
        assert_eq!(d[0].target, "t064");

        let d2 = diff_snapshots(&b, &a);
        assert_eq!(d2[0].kind, DiffKind::Removed);
        assert_eq!(d2[0].target, "t064");
    }
}

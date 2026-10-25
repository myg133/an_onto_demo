//! 契约里的「取值路径」—— 一个**极小**的 JSON 路径子集（⛔ 不是 JSONPath 实现）。
//!
//! # 只支持契约真正用到的两种形状
//!
//! | 形状 | 例子 | 用在哪 |
//! | --- | --- | --- |
//! | 点分单值 | `Placement.Zone` / `$.Response.TotalCount` | `fields.<k>.path`、`paging.total_path`、`identity.key_path` |
//! | 点分 + 末尾 `[*]` | `$.Response.InstanceSet[*]` | `entities[].iterator` |
//!
//! # ⛔ 明确不做（写进「已知限制」，不是待办）
//!
//! - 不支持 `[0]` 下标 / 通配 `*` / 递归 `..` / 过滤器 `[?(@.x)]` —— 契约 v1 不需要，
//!   而「实现一个半吊子 JSONPath」比「只认两种形状」更容易在评审里骗过人。
//! - 不支持键名里带 `.` 的对象（腾讯云 / 阿里云的响应都没有这种键；真遇到就是契约要升级）。
//! - 路径全部是**相对**的：`$.` 前缀可有可无（作者手写时常带、程序拼时常不带）。
//!
//! # 为什么「路径不存在」与「值是 null」必须分开
//!
//! `validate` 的第四条检查（`fields.x.path` 在 fixture 里真的存在）判的是**键在不在**，
//! 不是**值空不空**：`"InstanceName": ""` 是合法数据（实例没起名），
//! 而 `InstanceName` 整个键不存在说明**映射写歪了**。两者混为一谈会让这条检查变成噪声。

use serde_json::Value;

/// 把路径拆成键序列（去掉 `$` / `$.` 前缀，丢掉空段）。
pub fn segments(path: &str) -> Vec<&str> {
    let p = path.trim();
    let p = p.strip_prefix('$').unwrap_or(p);
    let p = p.strip_prefix('.').unwrap_or(p);
    if p.is_empty() {
        return Vec::new();
    }
    p.split('.').filter(|s| !s.is_empty()).collect()
}

/// 这条路径是不是**迭代器**（末尾 `[*]`）。
pub fn is_iterator(path: &str) -> bool {
    let p = path.trim();
    let stripped = p.strip_suffix("[*]").unwrap_or(p);
    stripped != p && !segments(stripped).is_empty()
}

/// 迭代器路径去掉末尾的 `[*]`；不是迭代器就原样返回。
pub fn strip_iter(path: &str) -> &str {
    let p = path.trim();
    p.strip_suffix("[*]").unwrap_or(p)
}

/// 点分单值取值。`*` / `[*]` 出现在中间 ⇒ 取不到（`None`），⛔ 不猜。
pub fn select<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    let segs = segments(strip_iter(path));
    if segs.is_empty() {
        return None;
    }
    let mut cur = root;
    for seg in segs {
        if seg == "*" || seg.ends_with("[*]") {
            return None;
        }
        cur = cur.get(seg)?;
    }
    Some(cur)
}

/// 迭代器取值：`iterator` 的最后一段是数组属性，返回它的**全部元素**。
///
/// ⛔ 不是数组 ⇒ 空（调用方负责报 `ITERATOR_EMPTY`）。
pub fn iterate<'a>(root: &'a Value, path: &str) -> Vec<&'a Value> {
    match select(root, path) {
        Some(Value::Array(items)) => items.iter().collect(),
        _ => Vec::new(),
    }
}

/// 键存在吗（值为 `null` 也算存在）。
pub fn exists(root: &Value, path: &str) -> bool {
    select(root, path).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn doc() -> Value {
        json!({
            "Response": {
                "TotalCount": 250,
                "InstanceSet": [
                    { "InstanceId": "ins-1", "Placement": { "Zone": "z3" }, "InstanceName": "" },
                    { "InstanceId": "ins-2", "Placement": { "Zone": "z2" }, "InstanceName": null }
                ]
            }
        })
    }

    #[test]
    fn segments_accepts_both_prefixes() {
        assert_eq!(segments("$.Response.TotalCount"), vec!["Response", "TotalCount"]);
        assert_eq!(segments("Placement.Zone"), vec!["Placement", "Zone"]);
        assert_eq!(segments("$"), Vec::<&str>::new());
    }

    #[test]
    fn iterator_detection() {
        assert!(is_iterator("$.Response.InstanceSet[*]"));
        assert!(!is_iterator("$.Response.InstanceSet"));
        assert_eq!(strip_iter("$.Response.InstanceSet[*]"), "$.Response.InstanceSet");
        assert_eq!(strip_iter("a.b"), "a.b");
    }

    #[test]
    fn select_and_iterate() {
        let d = doc();
        assert_eq!(select(&d, "$.Response.TotalCount"), Some(&json!(250)));
        assert_eq!(select(&d, "Response.TotalCount"), Some(&json!(250)));
        assert!(select(&d, "$.Response.Nope").is_none());
        let rows = iterate(&d, "$.Response.InstanceSet[*]");
        assert_eq!(rows.len(), 2);
        // 不是数组 ⇒ 空（⛔ 不 panic、不猜）
        assert!(iterate(&d, "$.Response.TotalCount[*]").is_empty());
    }

    #[test]
    fn emptiness_vs_absence() {
        let d = doc();
        let rows = iterate(&d, "$.Response.InstanceSet[*]");
        // `""` 与 `null` 都算**存在**（键在），这正是 validate 第四条要的口径。
        assert!(exists(rows[0], "InstanceName"), "空串也算存在");
        assert!(exists(rows[1], "InstanceName"), "null 也算存在");
        // `$` / `$.` 前缀可有可无（作者手写时带、程序拼时常不带）—— 两种都取到。
        assert!(exists(rows[0], "$.InstanceName"), "`$.` 前缀是允许的写法");
        assert!(!exists(rows[0], "InstanceId.X"), "多一层路径取不到");
    }
}

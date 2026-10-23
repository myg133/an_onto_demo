//! REQ-122 · SQL 文本原语 —— 标识符引用、字面量渲染、类型判断。
//!
//! # 为什么字面量是**内联**的（而不是绑定参数）
//!
//! | 做法 | 代价 |
//! | --- | --- |
//! | `$1` 绑定 | `numeric = $1`（sqlx 把 `&str` 声明成 `text`）在 PG 里**类型不匹配**；要给每处 cast 还得先知道列类型，等于把表达式又算一遍 |
//! | 内联（本模块） | 只有一处要小心：转义。换来 `sql_echo` **逐字等于真正执行的 SQL** —— AC-005/R5 要的正是「SQL 里有那个条件」 |
//!
//! 转义口径（唯一的一处）：
//! - 标识符（表名 / 列名 / 别名）：`"` → `""`，整体用 `"` 包起来；
//! - 字符串字面量：`'` → `''`，整体用 `'` 包起来；含 NUL ⇒ **报错**（PG 的文本协议不接受）。
//!
//! ⚠️ 前提是服务端 `standard_conforming_strings = on`（PG ≥ 9.1 的默认值）。
//! 这条不是「假定」：连接建立时由 [`super::exec`] **读回来校验**，不是 `on` 就拒绝执行 ——
//! 否则反斜杠会被当成转义，`\` 就能拼出第二个引号。

use super::error::QueryError;
use super::query::{Literal, LiteralKind};

/// 标识符：`foo"bar` → `"foo""bar"`。
pub(crate) fn ident(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// 表别名（`t0` / `t1` …）—— 由本模块生成，仍按标识符引用（同一条规矩，不留例外）。
pub(crate) fn alias(i: usize) -> String {
    format!("t{i}")
}

/// 字符串字面量：`O'Brien` → `'O''Brien'`。
pub(crate) fn text_literal(s: &str) -> Result<String, QueryError> {
    if s.contains('\0') {
        return Err(QueryError::new(
            "字面量里含有 NUL 字符 —— PG 的文本协议不接受，⛔ 不能悄悄截断",
        ));
    }
    Ok(format!("'{}'", s.replace('\'', "''")))
}

/// 数值字面量（只有**确认**是数字时才原样写进 SQL）。
///
/// 不接受 `NaN` / `inf` / 十六进制 —— 它们要么不是字面量语法，要么会被 PG 解释成别的东西。
pub(crate) fn numeric_literal(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    let body = t.strip_prefix('-').or_else(|| t.strip_prefix('+')).unwrap_or(t);
    let mut digits = 0usize;
    let mut dots = 0usize;
    let mut exp = false;
    let chars: Vec<char> = body.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_digit() {
            digits += 1;
            i += 1;
        } else if c == '.' && dots == 0 && !exp {
            dots += 1;
            i += 1;
        } else if (c == 'e' || c == 'E') && !exp && digits > 0 {
            exp = true;
            i += 1;
            if i < chars.len() && (chars[i] == '+' || chars[i] == '-') {
                i += 1;
            }
        } else {
            return None;
        }
    }
    if digits == 0 {
        return None;
    }
    Some(t.to_string())
}

/// SQL 数值类型？—— 决定数字字面量能不能原样写（`varchar = 1` 在 PG 里是错的）。
pub(crate) fn is_numeric_type(data_type: &str) -> bool {
    let t = data_type.trim().to_ascii_lowercase();
    let t = t.split('(').next().unwrap_or(&t).trim().to_string();
    matches!(
        t.as_str(),
        "smallint"
            | "integer"
            | "bigint"
            | "int"
            | "int2"
            | "int4"
            | "int8"
            | "serial"
            | "smallserial"
            | "bigserial"
            | "numeric"
            | "decimal"
            | "real"
            | "double precision"
            | "float"
            | "float4"
            | "float8"
            | "money"
    )
}

/// SQL 布尔类型？
fn is_bool_type(data_type: &str) -> bool {
    matches!(data_type.trim().to_ascii_lowercase().as_str(), "boolean" | "bool")
}

/// 把字面量渲染成 SQL —— **按目标列的类型**决定是否加引号。
///
/// `target` = 目标列的 `data_type`（`None` ⇒ 当作文本，加引号）。
pub(crate) fn literal_sql(lit: &Literal, target: Option<&str>) -> Result<String, QueryError> {
    match target.map(is_numeric_type) {
        Some(true) => {
            // 数值列：只接受数字字面量。字符串 "100" 也**不**自动转 ——
            // 那会让「写错了」看起来像「写对了」（数字与字符串在 SPARQL 里不是一回事）。
            match lit.numeric_value() {
                Some(_) => numeric_literal(&lit.text).ok_or_else(|| {
                    QueryError::new(format!("字面量 {:?} 不是合法的数值", lit.text))
                }),
                None => Err(QueryError::new(format!(
                    "字面量 {:?} 与数值列（{}）不匹配 —— ⛔ 不做隐式转换（要比较数字就写数字）",
                    lit.text,
                    target.unwrap_or("numeric")
                ))),
            }
        }
        _ => {
            if lit.kind == LiteralKind::Boolean && target.map(is_bool_type) == Some(true) {
                return Ok(if lit.text == "true" { "TRUE".into() } else { "FALSE".into() });
            }
            text_literal(&lit.text)
        }
    }
}

/// 租户值渲染 —— 与字面量**同一条**规矩（AC：注入可见；R3：不能确定就拒绝）。
///
/// 数值列而租户值不是数字 ⇒ **报错**，⛔ 不「尽量转一下」：
/// 转失败的那一次就是跨租户泄漏。
pub(crate) fn tenant_sql(value: &str, target: Option<&str>, table: &str, column: &str) -> Result<String, QueryError> {
    let v = value.trim();
    if v.is_empty() {
        return Err(QueryError::new(
            "租户为空 —— ⛔ 不能拿空值当租户（那会把所有租户的数据都放出来）",
        ));
    }
    match target.map(is_numeric_type) {
        Some(true) => numeric_literal(v).map(|n| n).ok_or_else(|| {
            QueryError::new(format!(
                "租户值 {v:?} 与列 {table}.{column} 的类型（{}）不匹配 —— ⛔ 不做隐式转换",
                target.unwrap_or("numeric")
            ))
        }),
        _ => text_literal(v),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lit(s: &str, kind: LiteralKind) -> Literal {
        Literal { text: s.into(), kind }
    }

    #[test]
    fn identifiers_are_quoted_and_escaped() {
        assert_eq!(ident("balances"), "\"balances\"");
        assert_eq!(ident("we\"ird"), "\"we\"\"ird\"");
    }

    #[test]
    fn text_literals_escape_single_quotes() {
        assert_eq!(text_literal("O'Brien").unwrap(), "'O''Brien'");
        assert_eq!(text_literal("RMB").unwrap(), "'RMB'");
    }

    #[test]
    fn the_classic_injection_payload_stays_inside_one_literal() {
        let payload = "x' OR '1'='1";
        let sql = text_literal(payload).unwrap();
        assert_eq!(sql, "'x'' OR ''1''=''1'");
        // 整串仍是**一个**字面量：首尾各一个引号，内部引号全部成对
        assert!(sql.starts_with('\'') && sql.ends_with('\''));
        assert_eq!(sql[1..sql.len() - 1].matches('\'').count() % 2, 0);
    }

    #[test]
    fn nul_is_rejected_not_truncated() {
        assert!(text_literal("a\0b").is_err());
    }

    #[test]
    fn numeric_literals_are_recognized_strictly() {
        for ok in ["0", "1", "-1", "1.5", "-0.25", "1e9", "1.5E-3", "+7"] {
            assert!(numeric_literal(ok).is_some(), "{ok}");
        }
        for bad in ["", "-", "1..2", "NaN", "inf", "0x10", "1,5", "1 2"] {
            assert!(numeric_literal(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn numeric_columns_accept_only_numeric_literals() {
        assert_eq!(
            literal_sql(&lit("100", LiteralKind::Integer), Some("numeric")).unwrap(),
            "100"
        );
        // 字符串写进数值列 ⇒ 报错，而不是悄悄加引号让 PG 去转
        let e = literal_sql(&lit("100", LiteralKind::String), Some("integer")).unwrap_err();
        assert!(e.message().contains("不做隐式转换"), "{e}");
    }

    #[test]
    fn text_columns_quote_everything_even_numbers() {
        assert_eq!(
            literal_sql(&lit("100", LiteralKind::Integer), Some("character varying")).unwrap(),
            "'100'"
        );
    }

    #[test]
    fn booleans_are_rendered_as_keywords_only_for_boolean_columns() {
        assert_eq!(
            literal_sql(&lit("true", LiteralKind::Boolean), Some("boolean")).unwrap(),
            "TRUE"
        );
        assert_eq!(
            literal_sql(&lit("true", LiteralKind::Boolean), Some("text")).unwrap(),
            "'true'"
        );
    }

    #[test]
    fn a_non_numeric_tenant_value_for_a_numeric_column_is_rejected() {
        let e = tenant_sql("C0001", Some("bigint"), "balances", "owner_no").unwrap_err();
        assert!(e.message().contains("balances.owner_no"), "{e}");
        assert!(e.message().contains("不做隐式转换"), "{e}");
        assert_eq!(tenant_sql("42", Some("bigint"), "t", "c").unwrap(), "42");
        assert_eq!(tenant_sql("C0001", Some("text"), "t", "c").unwrap(), "'C0001'");
    }

    #[test]
    fn an_empty_tenant_is_rejected() {
        assert!(tenant_sql("  ", Some("text"), "t", "c").is_err());
    }

    #[test]
    fn sql_types_are_classified_by_their_real_postgres_names() {
        for n in ["bigint", "integer", "numeric(10,2)", "NUMERIC", "double precision", "real"] {
            assert!(is_numeric_type(n), "{n}");
        }
        for t in ["character varying", "text", "timestamp without time zone", "uuid", "jsonb"] {
            assert!(!is_numeric_type(t), "{t}");
        }
    }
}

//! REQ-124 · **三份声明的读 / 校验 / 保存**（R4：保存即校验，失败不落盘）。
//!
//! # 校验**不**在这里重新实现一遍
//!
//! 三份声明各有一个**既有**的解析器，它们已经把「形状 + 自洽性」判掉了：
//!
//! | 声明 | 既有解析器 | 它已经管的 |
//! | --- | --- | --- |
//! | `exclusions.json` | [`crate::extract::exclusions::Exclusions::parse`] | `category` 枚举 / `reason` 长度 / `decided_by` 非空 / 重复表 |
//! | `query-policy.json` | [`crate::sparql::policy::QueryPolicy::parse`] | `main_only` 必须带 `reason` / `publicDimensions` 必须带 `reason` / 同一张表不能既过滤又公共 |
//! | `tenancy.json` | [`crate::domain::ttl::TenancyDecl::parse`] | `deny_unknown_fields` / trim + 去重 |
//!
//! 本模块只补**解析器拿不到的那一半**：**对照真实 schema 的「表 / 列必须存在」**。
//! 解析器手里没有 schema（它是纯函数，AC-002 的「domain 零 I/O」），所以这条判据
//! 只能落在控制面 —— 而它是 R4 里唯一需要外部事实的那一条。
//!
//! # 保存的顺序就是判据
//!
//! `validate` → `save`。⛔ **没有**「先写再校验」或「写一半再回滚」：
//! 一次失败的 PUT 结束后，原文件必须**一个字节都不变**（测试用前后 sha256 自证）。
//! `save` 还会写后读回、再算一次 sha256：写失败（磁盘满 / 权限）不会被报成「已保存」。

use std::path::{Path, PathBuf};

use crate::domain::ttl::{SchemaTypes, TenancyDecl};
use crate::extract::exclusions::Exclusions;
use crate::query::audit::sha256_hex;
use crate::sparql::policy::QueryPolicy;

use super::OpError;

/// 三份声明的路径（`console serve` 的三个参数）。
#[derive(Debug, Clone)]
pub struct DeclPaths {
    pub exclusions: PathBuf,
    pub policy: PathBuf,
    pub tenancy: PathBuf,
}

/// `/api/config/{kind}` 的 `kind`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclKind {
    Exclusions,
    Policy,
    Tenancy,
}

impl DeclKind {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim() {
            "exclusions" => Some(Self::Exclusions),
            "policy" => Some(Self::Policy),
            "tenancy" => Some(Self::Tenancy),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Exclusions => "exclusions",
            Self::Policy => "policy",
            Self::Tenancy => "tenancy",
        }
    }

    /// 这份声明在磁盘上的位置（**唯一**的取值点：GET / PUT / 审计都用它）。
    pub fn path(self, paths: &DeclPaths) -> &Path {
        match self {
            Self::Exclusions => &paths.exclusions,
            Self::Policy => &paths.policy,
            Self::Tenancy => &paths.tenancy,
        }
    }

    /// 读原文（GET 与「保存前 sha256」共用）。
    pub fn read(self, paths: &DeclPaths) -> Result<String, OpError> {
        let path = self.path(paths);
        std::fs::read_to_string(path).map_err(|e| {
            OpError::not_found(
                "DECLARATION_NOT_FOUND",
                format!("读不到{}声明 {}：{e}", self.as_str(), path.display()),
                "确认 `console serve` 的 `--exclusions` / `--policy` / `--tenancy` 指对了文件；\
                 文件缺失时不会自动创建（声明必须由人给）。",
            )
        })
    }
}

/// 对照 schema 校验一份**已经通过形状解析**的声明。
///
/// 返回的 `Ok(())` 意思是「可以落盘」；`Err` 一定是 400（[`OpError::bad_request`]），
/// 消息里逐条点名**哪张表 / 哪一列**。
pub fn validate(
    kind: DeclKind,
    path: &Path,
    text: &str,
    schema: &SchemaTypes,
) -> Result<(), OpError> {
    match kind {
        DeclKind::Exclusions => validate_exclusions(path, text, schema),
        DeclKind::Policy => validate_policy(path, text, schema),
        DeclKind::Tenancy => validate_tenancy(path, text, schema),
    }
}

fn invalid(kind: DeclKind, path: &Path, problems: &[String]) -> OpError {
    let names = problems.join("\n  - ");
    OpError::bad_request(
        "DECLARATION_INVALID",
        format!(
            "{} 声明 {} 不合法（{} 处）：\n  - {}",
            kind.as_str(),
            path.display(),
            problems.len(),
            names
        ),
        "修正后重发 PUT。⛔ 这次失败**没有改动原文件**（校验在写之前）。\
         表 / 列只能来自 `--schema`（真实 schema），⛔ 不猜、不自动补。",
    )
}

fn validate_exclusions(path: &Path, text: &str, schema: &SchemaTypes) -> Result<(), OpError> {
    // ① 既有解析器：category 枚举 / reason / decided_by / 重复表 / 空数组。
    let ex = Exclusions::parse(text.as_bytes(), &path.to_string_lossy()).map_err(|e| {
        OpError::bad_request(
            "DECLARATION_INVALID",
            format!("exclusions 声明 {} 不合法：{e:#}", path.display()),
            "`exclude[]` 每条要有 table / category / reason（≥ 6 字）/ decided_by；\
             category 只能是既有七类枚举之一。",
        )
    })?;
    // ② 本模块补的那一半：被排除的表必须真的在 schema 里（否则排除的是个不存在的名字，
    //    而它看起来「生效了」）。
    let mut problems: Vec<String> = Vec::new();
    for e in ex.entries() {
        if !schema.has_table(&e.table) {
            problems.push(format!(
                "exclude[].table = {:?} 不在 schema 里（排除一个不存在的表等于没有做出决定）",
                e.table
            ));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(invalid(DeclKind::Exclusions, path, &problems))
    }
}

fn validate_policy(path: &Path, text: &str, schema: &SchemaTypes) -> Result<(), OpError> {
    // ① 既有解析器：version / tenantSubject / coverage=main_only 必须带 reason /
    //    publicDimensions 必须带 reason / 同一张表不能既过滤又公共。
    let p = QueryPolicy::parse(text).map_err(|e| {
        OpError::bad_request(
            "DECLARATION_INVALID",
            format!("policy 声明 {} 不合法：{}", path.display(), e.message()),
            "修正后重发。`coverage: \"main_only\"` 必须写 `reason`；\
             公共维度必须写 `reason`（不写理由的「公共」等于没人负责）。",
        )
    })?;

    // ② 对照 schema：主体表 / 主体列 / 逐表过滤列 / 公共维度表都必须存在。
    let mut problems: Vec<String> = Vec::new();
    if !schema.has_table(&p.tenant_subject) {
        problems.push(format!(
            "tenantSubject = {:?} 不在 schema 里",
            p.tenant_subject
        ));
    } else if !schema.has_column(&p.tenant_subject, &p.tenant_id_column) {
        problems.push(format!(
            "tenantIdColumn = {:?} 在表 {:?} 里不存在（可用的列：{}）",
            p.tenant_id_column,
            p.tenant_subject,
            schema
                .column_names(&p.tenant_subject)
                .unwrap_or_default()
                .join("、")
        ));
    }
    for (table, rule) in &p.per_table {
        if !schema.has_table(table) {
            problems.push(format!("perTable.{table} 不在 schema 里"));
            continue;
        }
        if !schema.has_column(table, &rule.tenant_column) {
            problems.push(format!(
                "perTable.{table}.tenantColumn = {:?} 在该表里不存在（可用的列：{}）",
                rule.tenant_column,
                schema
                    .column_names(table)
                    .unwrap_or_default()
                    .join("、")
            ));
        }
    }
    for d in &p.public_dimensions {
        if !schema.has_table(&d.table) {
            problems.push(format!("publicDimensions 里的 {:?} 不在 schema 里", d.table));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(invalid(DeclKind::Policy, path, &problems))
    }
}

fn validate_tenancy(path: &Path, text: &str, schema: &SchemaTypes) -> Result<(), OpError> {
    // ① 既有解析器：deny_unknown_fields / trim / 去重 / 排序。
    let d = TenancyDecl::parse(text).map_err(|e| {
        OpError::bad_request(
            "DECLARATION_INVALID",
            format!("tenancy 声明 {} 不合法：{e}", path.display()),
            "形状：{\"columns\":[\"…\"],\"tables\":[\"…\"]?,\"reviewed_by\":\"…\"?}；\
             ⛔ 未知字段会被拒绝（把 `tables` 写成 `table` 不会被静默忽略）。",
        )
    })?;

    // ② 对照 schema：给了 `tables` ⇒ 表要存在、列要在那些表里；
    //    没给 ⇒ 每个列至少要能在**某一张**表里找到（否则它是一个不存在的名字）。
    let mut problems: Vec<String> = Vec::new();
    let all_tables: Vec<String> = schema.table_names().map(str::to_string).collect();
    match &d.tables {
        Some(tables) if tables.is_empty() => {
            problems.push("tables = [] 是空数组：要么省略（= 全部实体），要么写明表名".into());
        }
        Some(tables) => {
            for t in tables {
                if !schema.has_table(t) {
                    problems.push(format!("tables 里的 {:?} 不在 schema 里", t));
                    continue;
                }
                for c in &d.columns {
                    if !schema.has_column(t, c) {
                        problems.push(format!(
                            "columns 里的 {:?} 在表 {:?} 里不存在（可用的列：{}）",
                            c,
                            t,
                            schema.column_names(t).unwrap_or_default().join("、")
                        ));
                    }
                }
            }
        }
        None => {
            for c in &d.columns {
                let hit = all_tables.iter().any(|t| schema.has_column(t, c));
                if !hit {
                    problems.push(format!(
                        "columns 里的 {:?} 不在 schema 的任何一张表里",
                        c
                    ));
                }
            }
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(invalid(DeclKind::Tenancy, path, &problems))
    }
}

/// 保存（**只在 validate 通过后调用**）。返回 `(字节数, sha256)`。
///
/// 写后**读回再算一次**：写失败（权限 / 磁盘满 / 被截断）不会被报成「已保存」。
/// 读回不一致 ⇒ 500（磁盘层面的问题，不是用户参数问题）。
pub fn save(path: &Path, text: &str) -> Result<(usize, String), OpError> {
    let bytes = text.as_bytes();
    let sha = sha256_hex(bytes);
    std::fs::write(path, bytes).map_err(|e| {
        OpError::internal(format!("写不出 {}：{e}（校验已通过，但落盘失败）", path.display()))
    })?;
    let back = std::fs::read(path).map_err(|e| {
        OpError::internal(format!("写后读不回 {}：{e}（磁盘问题）", path.display()))
    })?;
    if sha256_hex(&back) != sha {
        return Err(OpError::internal(format!(
            "{} 写后读回的字节与写入不一致（期望 sha256 {sha}）—— \
             这次保存不可信，请检查磁盘 / 同目录的其他写入者",
            path.display()
        )));
    }
    Ok((bytes.len(), sha))
}

// ══════════════════════════════════════════════════════════════
// 判据（纯函数 + 临时文件；不连库、不起 socket）
// ══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    const SCHEMA: &str = r#"{"tables":[
      {"name":"customers","primary_key":"id","composite_primary_key":[],"unique_columns":["id"],
       "columns":[{"name":"id","data_type":"text","is_primary_key":true},
                  {"name":"customer_name","data_type":"text"}]},
      {"name":"orders","primary_key":"order_id","composite_primary_key":[],"unique_columns":["order_id"],
       "columns":[{"name":"order_id","data_type":"text","is_primary_key":true},
                  {"name":"customer_id","data_type":"text"}]}
    ]}"#;

    fn schema() -> SchemaTypes {
        SchemaTypes::parse(SCHEMA).expect("夹具 schema")
    }

    fn policy(table: &str, column: &str) -> String {
        format!(
            r#"{{"version":"1.0.0","tenantSubject":"customers","tenantIdColumn":"id",
                 "perTable":{{"{table}":{{"tenantColumn":"{column}","reason":"业务口径"}}}},
                 "publicDimensions":[]}}"#
        )
    }

    fn tmp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "ontok-req124-{}-{}-{name}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ))
    }

    #[test]
    fn a_policy_that_names_a_missing_table_or_column_is_rejected_by_name() {
        let s = schema();
        let p = tmp("policy.json");
        let e = validate(DeclKind::Policy, &p, &policy("ghost", "customer_id"), &s).unwrap_err();
        assert_eq!(e.status, axum::http::StatusCode::BAD_REQUEST);
        assert!(e.message.contains("ghost"), "要点名表：{}", e.message);

        let e = validate(DeclKind::Policy, &p, &policy("orders", "ghost_col"), &s).unwrap_err();
        assert!(e.message.contains("ghost_col"), "要点名列：{}", e.message);
    }

    #[test]
    fn main_only_without_a_reason_is_rejected_by_the_existing_parser() {
        let s = schema();
        let raw = format!(
            r#"{{"version":"1.0.0","tenantSubject":"customers","tenantIdColumn":"id",
                 "perTable":{{"orders":{{"tenantColumn":"customer_id","coverage":"main_only"}}}},
                 "publicDimensions":[]}}"#
        );
        let e = validate(DeclKind::Policy, &tmp("p.json"), &raw, &s).unwrap_err();
        assert!(e.message.contains("reason"), "{}", e.message);
    }

    #[test]
    fn a_public_dimension_without_a_reason_is_rejected() {
        let s = schema();
        let raw = r#"{"version":"1.0.0","tenantSubject":"customers","tenantIdColumn":"id",
                     "publicDimensions":[{"table":"orders","reason":"  "}]}"#;
        let e = validate(DeclKind::Policy, &tmp("p.json"), raw, &s).unwrap_err();
        assert!(e.message.contains("reason"), "{}", e.message);
    }

    #[test]
    fn an_unknown_exclusion_category_or_a_missing_table_is_rejected() {
        let s = schema();
        let p = tmp("exclusions.json");
        let bad_cat = r#"{"version":"0.1.0","rule":"r","declared_by":"ba",
            "exclude":[{"table":"orders","category":"whatever","reason":"够长的理由","decided_by":"ba"}]}"#;
        let e = validate(DeclKind::Exclusions, &p, bad_cat, &s).unwrap_err();
        assert!(e.message.contains("category"), "{}", e.message);

        let ghost = r#"{"version":"0.1.0","rule":"r","declared_by":"ba",
            "exclude":[{"table":"ghost","category":"cdc_residue","reason":"够长的理由","decided_by":"ba"}]}"#;
        let e = validate(DeclKind::Exclusions, &p, ghost, &s).unwrap_err();
        assert!(e.message.contains("ghost"), "{}", e.message);

        let no_reason = r#"{"version":"0.1.0","rule":"r","declared_by":"ba",
            "exclude":[{"table":"orders","category":"cdc_residue","reason":"  ","decided_by":"ba"}]}"#;
        assert!(validate(DeclKind::Exclusions, &p, no_reason, &s).is_err());
    }

    #[test]
    fn tenancy_columns_must_exist_in_the_schema() {
        let s = schema();
        let p = tmp("tenancy.json");
        let ok = r#"{"columns":["customer_id"],"reviewed_by":"安全负责人"}"#;
        assert!(validate(DeclKind::Tenancy, &p, ok, &s).is_ok());

        let bad = r#"{"columns":["ghost_col"]}"#;
        let e = validate(DeclKind::Tenancy, &p, bad, &s).unwrap_err();
        assert!(e.message.contains("ghost_col"), "{}", e.message);

        let bad_table = r#"{"columns":["customer_id"],"tables":["ghost"]}"#;
        let e = validate(DeclKind::Tenancy, &p, bad_table, &s).unwrap_err();
        assert!(e.message.contains("ghost"), "{}", e.message);
    }

    #[test]
    fn save_writes_the_bytes_and_reports_the_same_sha() {
        let p = tmp("save.json");
        let text = "{\"columns\":[]}\n";
        let (n, sha) = save(&p, text).expect("保存");
        assert_eq!(n, text.len());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), text);
        assert_eq!(sha256_hex(text.as_bytes()), sha);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn a_parse_error_never_touches_the_file() {
        let p = tmp("untouched.json");
        let original = "{\"columns\":[\"customer_id\"]}\n";
        std::fs::write(&p, original).unwrap();
        let before = sha256_hex(&std::fs::read(&p).unwrap());
        let err = validate(DeclKind::Tenancy, &p, "not json", &schema()).unwrap_err();
        assert_eq!(err.status, axum::http::StatusCode::BAD_REQUEST);
        let after = sha256_hex(&std::fs::read(&p).unwrap());
        assert_eq!(before, after, "校验失败不许改原文件");
        let _ = std::fs::remove_file(&p);
    }

    /// 反空转：确认本文件的 JSON 夹具真的被解析过（而不是字符串比较）。
    #[test]
    fn the_fixture_schema_is_real() {
        let s = schema();
        assert_eq!(s.data_type("orders", "customer_id"), Some("text"));
        assert!(!s.has_column("orders", "ghost_col"));
    }
}

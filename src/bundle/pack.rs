//! `bundle pack` 的**纯函数**核心：把已读好的字节装进 store-only zip。
//!
//! ⛔ 本文件**不连库、不读文件、不调模型、不发网络**：入参全是调用方已经取好的
//! 字节与结构体。理由见 [`crate::bundle`] 模块头 —— 包的自检与组装必须能在
//! 没有数据库的机器上跑（`tests/test_req159_bundle.rs` 直接对着它断言，
//! 一条用例都不连库）。
//!
//! 连库那一步在 [`crate::cli`]：照抄 `relation_verbs_check`
//! （`require_env` → `connect` → `get_by_name` → `version_by_seq` → `get_snapshot`），
//! 拿到 [`crate::domain::snapshot::Snapshot`] 之后**原样**交给本模块。
//!
//! # ⚠️ 快照原文的唯一入口
//!
//! [`PackInput::snapshot`] 的类型是 [`Snapshot`]（结构体），**不是** `Value`。
//! 这是刻意的：一旦允许传 `Value`，调用方就能把 `ontology show` 的**视图**
//! 塞进来（`LinkTypeView` 只有 5 个字段，`evidence` 会被静默丢掉）。
//! 类型上堵死这条路，比在注释里提醒一百遍都管用 ——
//! 序列化只发生在 [`assemble`] 里的那一行 `serde_json::to_vec`。

use crate::bundle::zip;
use crate::bundle::{
    AM_PATH, BUILDER_NAME, BUNDLE_VERSION, DB_PATH, DECLARATIONS_DIR, MANIFEST_PATH, SNAPSHOT_PATH,
};
use crate::domain::snapshot::Snapshot;
use crate::query::audit::sha256_hex;
use serde_json::{json, Value};
use std::fmt;

/// 打包失败（⇒ `cli.rs` 一律按**退出码 1**：环境 / 参数 / IO 错误）。
///
/// 「包不合规」不走这里，走 [`crate::bundle::verify`] 的报告 + 退出码 2 ——
/// 两者的区别是「我打不出来」与「打出来了但会被服务端拒」。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackError {
    /// 参数不合法（项目名 / 时间 / 方言 / 路径 / 重复文件名）。
    Invalid(String),
    /// 用户给的文件名派生出的**包内 entry 名不合规** —— 这是**参数错误**，
    /// 而且打出来的包**必被自检拒**（QA F-04）。
    ///
    /// ⚠️ 单独一个变体（而不是塞进 [`PackError::Invalid`]）是为了让
    /// `cli.rs` 能按 `exit 2` 收尾并打出「哪个 flag + 哪个文件名 + 为什么」，
    /// 而不是笼统地报「打包失败」或（更糟）打完再报「这是打包端的 bug」。
    IllegalEntryPath {
        /// 来源 flag（`--am` / `--declarations`）。
        flag: String,
        /// 用户给的源文件名。
        name: String,
        /// 会被打成什么包内路径。
        entry: String,
        /// 不合规的原因（来自 [`crate::bundle::path_problem`]）。
        why: String,
    },
    /// 容器写不出来（路径非法 / 超 4 GiB / 名字超长 / 重名）。
    Zip(zip::ZipError),
}

impl fmt::Display for PackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PackError::Invalid(m) => write!(f, "{m}"),
            PackError::IllegalEntryPath { flag, name, entry, why } => write!(
                f,
                "`{flag}` 的文件 `{name}` 会被打成包内路径 `{entry}`，但这个路径不合规：{why}"
            ),
            PackError::Zip(e) => write!(f, "{e}"),
        }
    }
}

impl PackError {
    /// 是不是「参数合不合法」这一档（`cli.rs` 据此走 `exit 2`，而不是 `exit 1`）。
    pub fn is_bad_entry_path(&self) -> bool {
        matches!(self, PackError::IllegalEntryPath { .. })
    }
}

impl std::error::Error for PackError {}

impl From<zip::ZipError> for PackError {
    fn from(e: zip::ZipError) -> Self {
        PackError::Zip(e)
    }
}

/// `db/schema.json` 这一件（可选）。⚠️ **不含 DSN** —— 这里是描述，不是连接串。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbInput {
    /// `db/schema.json` 的原始字节（原样进包，不做二次序列化）。
    pub bytes: Vec<u8>,
    /// `true` ⇒ 服务端必须先拿到可用 DSN 才允许 `import`。
    pub requires_dsn: bool,
    /// 方言；`requires_dsn=true` 时**必填**（缺省补 `postgresql`）。
    pub dialect: Option<String>,
}

impl DbInput {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self { bytes, requires_dsn: false, dialect: None }
    }
}

/// 打包输入 —— **全部已读好**（本模块不做任何 I/O）。
#[derive(Debug, Clone)]
pub struct PackInput {
    /// 项目名（schema pattern [`PROJECT_NAME_PATTERN`] = `^[a-z0-9][a-z0-9_-]{1,62}$`）。
    pub project: String,
    /// 构建端这份本体的序号（`--version`）。
    pub ontology_seq: i32,
    /// RFC3339 时间串 —— **由调用方钉死**才能保证确定性（缺省值在 `cli.rs` 算）。
    pub built_at: String,
    /// `builder.version`。
    pub builder_version: String,
    /// `builder.host`（可空：容器里两个环境变量都可能没有）。
    pub builder_host: Option<String>,
    /// ⚠️ **快照原文**（结构体，不是视图、不是 `Value`）。
    pub snapshot: Snapshot,
    /// `--am` 的原样字节：`(原文件名, 字节)`；**第一个**落 [`AM_PATH`]。
    pub am: Vec<(String, Vec<u8>)>,
    /// `--db-schema`。
    pub db: Option<DbInput>,
    /// `--declarations <dir>` 下的文件：`(文件名, 字节)`（**不含子目录**）。
    pub declarations: Vec<(String, Vec<u8>)>,
}

impl PackInput {
    /// 一份最小输入（只有本体；`builder` 用本 crate 的版本号）。
    pub fn new(project: impl Into<String>, ontology_seq: i32, built_at: impl Into<String>, snapshot: Snapshot) -> Self {
        Self {
            project: project.into(),
            ontology_seq,
            built_at: built_at.into(),
            builder_version: env!("CARGO_PKG_VERSION").to_string(),
            builder_host: None,
            snapshot,
            am: Vec::new(),
            db: None,
            declarations: Vec::new(),
        }
    }
}

/// 项目名是否符合 `manifest.schema.json` 的 pattern（[`PROJECT_NAME_PATTERN`]）。
///
/// 手写而不是用 regex crate（⛔ 不新增依赖），规则只有三条：
/// 长度 2..=63、首字符是小写字母或数字、其余是小写字母 / 数字 / `-` / `_`。
///
/// # ⚠️ 下划线是**后加的**（REQ-159 · 用户 + join 拍板 A）
///
/// 真实项目名里有 `anspire_real` 这种写法。只放宽 `_` 这一格，
/// 其余照旧禁：大写（防与 URL 路径大小写规则打架）、空格、`/`、`.`、`..`、非 ASCII。
/// ⛔ 别再往这张白名单里加字符 —— 项目名会进 manifest、URL 路径与审计日志。
pub fn is_valid_project_name(p: &str) -> bool {
    let b = p.as_bytes();
    if b.len() < 2 || b.len() > 63 {
        return false;
    }
    if !(b[0].is_ascii_lowercase() || b[0].is_ascii_digit()) {
        return false;
    }
    b.iter()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'-' | b'_'))
}

/// 支持的数据源方言（与 `manifest.schema.json` 的 `dbComponent.dialect.enum` 一致）。
///
/// ⚠️ **唯一一份**：打包端（拒绝非法的 `--db-dialect`）与自检端（校验 manifest）
/// 读的是同一个常量 —— 两处各写一份迟早会漂移成「打得出来但过不了自检」。
pub const DIALECTS: &[&str] = &["postgresql", "mysql", "sqlserver", "oracle"];

/// 项目名的 pattern **原文** —— 报错文案里印的就是这一串。
///
/// ⚠️ 为什么要一个常量（REQ-159 · join 定位）：原来 pattern 在
/// 「实现」和「两条错误文案」里各写一遍，改一处忘两处就成了
/// **消息写旧 pattern、代码用新 pattern** —— 比不改更坏（用户照着旧 pattern
/// 改名字，改完还是被拒）。现在文案直接引用它，想漂都漂不了。
/// `cargo test` 里有用例钉着「代码接受/拒绝的集合」与这串 pattern 一致。
pub const PROJECT_NAME_PATTERN: &str = "^[a-z0-9][a-z0-9_-]{1,62}$";

// ───────────────── 源文件名 → 包内路径（**唯一**一份映射） ─────────────────

/// 第 `index` 个 `--am` 落成什么包内路径。
///
/// 第一个走契约的**规范槽位** [`AM_PATH`]（`components.am.path` 指向它）；
/// 其余落 `am/<原文件名>`。⚠️ 打包端与 CLI 的前置校验**共用**这个函数 ——
/// 各写一份迟早会出现「校验看着没事、打出来却叫另一个名字」。
pub fn am_entry_path(index: usize, file_name: &str) -> String {
    if index == 0 {
        AM_PATH.to_string()
    } else {
        format!("am/{file_name}")
    }
}

/// `--declarations <dir>` 里的某个文件落成什么包内路径。
pub fn declaration_entry_path(file_name: &str) -> String {
    format!("{DECLARATIONS_DIR}/{file_name}")
}

/// 校验「一个源文件名 + 它派生出的 entry 名」是否合规。
///
/// 判据是 [`crate::bundle::path_problem`] —— **与 `verify` 同一条**，
/// 于是「打得出来」与「过得了自检」在路径这件事上必然一致（QA F-04）。
/// ⚠️ `cli.rs` 在**连库之前**就调它：别等打完再靠自检失败把参数问题
/// 指成打包端的 bug。
pub fn check_entry_path(flag: &str, name: &str, entry: &str) -> Result<(), PackError> {
    match crate::bundle::path_problem(entry) {
        None => Ok(()),
        Some(why) => Err(PackError::IllegalEntryPath {
            flag: flag.to_string(),
            name: name.to_string(),
            entry: entry.to_string(),
            why,
        }),
    }
}

/// 打一个包，返回**完整 zip 字节**。
///
/// 顺序（两条都是契约，不是口味）：
/// 1. `manifest.json` **第一个**；
/// 2. 其余 entry 按 path **升序**；`files[]` 与它逐条对应（同样升序、同样不含 manifest 自己）。
pub fn assemble(input: &PackInput) -> Result<Vec<u8>, PackError> {
    // ① 参数先拒 —— 打出来一个「自己都要拒」的包是最坏的交付物。
    if !is_valid_project_name(&input.project) {
        return Err(PackError::Invalid(format!(
            "项目名 `{}` 不符合 schema 的 pattern `{}`\
             （小写字母/数字开头，允许小写字母/数字/连字符/下划线，总长 2..=63）",
            input.project, PROJECT_NAME_PATTERN
        )));
    }
    if input.ontology_seq < 1 {
        return Err(PackError::Invalid(format!(
            "`--version` 必须 ≥ 1（schema 的 `ontology_seq.minimum`），实际 {}",
            input.ontology_seq
        )));
    }
    if chrono::DateTime::parse_from_rfc3339(&input.built_at).is_err() {
        return Err(PackError::Invalid(format!(
            "`--built-at` 必须是 RFC3339（例 `2026-10-27T02:30:00Z`），实际 `{}`",
            input.built_at
        )));
    }

    // ② 快照原文 —— ⚠️ 这一行是「证据不丢」的唯一保证：直接序列化结构体。
    let snapshot_bytes = serde_json::to_vec(&input.snapshot)
        .map_err(|e| PackError::Invalid(format!("快照序列化失败：{e}")))?;

    // ③ 收集 entry（此时先不管顺序）。
    let mut payload: Vec<(String, Vec<u8>)> = Vec::with_capacity(
        1 + input.am.len() + usize::from(input.db.is_some()) + input.declarations.len(),
    );
    payload.push((SNAPSHOT_PATH.to_string(), snapshot_bytes));
    for (i, (name, bytes)) in input.am.iter().enumerate() {
        let path = am_entry_path(i, name);
        // 用户给的文件名 → entry 名，**在这里**就按与 verify 同一条规则拒。
        check_entry_path("--am", name, &path)?;
        payload.push((path, bytes.clone()));
    }
    if let Some(db) = &input.db {
        payload.push((DB_PATH.to_string(), db.bytes.clone()));
    }
    for (name, bytes) in &input.declarations {
        let path = declaration_entry_path(name);
        check_entry_path("--declarations", name, &path)?;
        payload.push((path, bytes.clone()));
    }

    // ④ 按 path 升序（契约要求；也让 "同一输入同一输出" 与调用顺序无关）。
    payload.sort_by(|a, b| a.0.cmp(&b.0));

    // ⑤ 路径与重名校验（`--declarations` 里两个同名文件、或某个名字撞上规范槽位）。
    //    ⚠️ 路径判据用 `crate::bundle::path_problem`（与 verify 同一份），
    //    ⛔ 不用 `zip::illegal_path_reason`：那一份只认「`..` **段**」，
    //    会放行 `v1..2.json`，而吐出来的包自己会拒它（QA F-04）。
    for (path, _) in &payload {
        if let Some(why) = crate::bundle::path_problem(path) {
            return Err(PackError::Invalid(format!(
                "包内路径 `{path}` 非法：{why}（schema 只允许 [A-Za-z0-9._/-]，且不许含 `..`）"
            )));
        }
    }
    for w in payload.windows(2) {
        if w[0].0 == w[1].0 {
            return Err(PackError::Invalid(format!(
                "包内路径 `{}` 出现两次（同名文件？）—— 一个路径只能有一份内容",
                w[0].0
            )));
        }
    }

    // ⑥ `files[]`：**除 `manifest.json` 自身以外**的每一个文件，按 path 升序。
    //    快照那一份的 sha256 与 `components.ontology.sha256` **同源**（都按字节算，
    //    不缓存、不换算）—— 两处口径一旦分家，服务端会先信一个再拒另一个。
    let snapshot_sha = payload
        .iter()
        .find(|(p, _)| p == SNAPSHOT_PATH)
        .map(|(_, b)| sha256_hex(b))
        .ok_or_else(|| PackError::Invalid("内部错误：包里没有 ontology/snapshot.json".to_string()))?;
    let files: Vec<Value> = payload
        .iter()
        .map(|(path, bytes)| {
            json!({
                "path": path,
                "sha256": sha256_hex(bytes),
                "bytes": bytes.len(),
            })
        })
        .collect();

    // ⑦ `components`：ontology 恒有；am / db 有才写（缺的**不写空对象**：
    //    `components` 是 additionalProperties:false，多一个键就整包拒）。
    let properties: usize = input
        .snapshot
        .entity_types
        .iter()
        .map(|e| e.properties.len())
        .sum();
    let mut components = serde_json::Map::new();
    components.insert(
        "ontology".to_string(),
        json!({
            "path": SNAPSHOT_PATH,
            "sha256": snapshot_sha,
            "entity_types": input.snapshot.entity_types.len(),
            "link_types": input.snapshot.link_types.len(),
            "properties": properties,
        }),
    );
    if let Some((_, bytes)) = input.am.first() {        let (entities, sources) = count_am(bytes)?;
        components.insert(
            "am".to_string(),
            json!({
                "path": AM_PATH,
                "sha256": sha256_hex(bytes),
                "entities": entities,
                "sources": sources,
            }),
        );
    }
    if let Some(db) = &input.db {
        let requires_dsn = db.requires_dsn;
        let dialect = match (&db.dialect, requires_dsn) {
            (Some(d), _) => {
                if !DIALECTS.contains(&d.as_str()) {
                    return Err(PackError::Invalid(format!(
                        "`--db-dialect {d}` 不在 schema 的 enum 里（{}）",
                        DIALECTS.join(" / ")
                    )));
                }
                Some(d.clone())
            }
            // 契约：`requires_dsn=true` ⇒ `dialect` 必填，缺省 postgresql。
            (None, true) => Some("postgresql".to_string()),
            (None, false) => None,
        };
        let mut dbc = serde_json::Map::new();
        dbc.insert("path".to_string(), json!(DB_PATH));
        dbc.insert("sha256".to_string(), json!(sha256_hex(&db.bytes)));
        dbc.insert("requires_dsn".to_string(), json!(requires_dsn));
        if let Some(d) = dialect {
            dbc.insert("dialect".to_string(), json!(d));
        }
        components.insert("db".to_string(), Value::Object(dbc));
    }

    // ⑧ manifest。键序由 `serde_json::Map`（BTreeMap）决定 ⇒ 与插入顺序无关，确定性。
    let mut manifest = serde_json::Map::new();
    manifest.insert("bundle_version".to_string(), json!(BUNDLE_VERSION));
    manifest.insert("project".to_string(), json!(input.project));
    manifest.insert("ontology_seq".to_string(), json!(input.ontology_seq));
    manifest.insert("built_at".to_string(), json!(input.built_at));
    let mut builder = serde_json::Map::new();
    builder.insert("name".to_string(), json!(BUILDER_NAME));
    builder.insert("version".to_string(), json!(input.builder_version));
    if let Some(host) = &input.builder_host {
        builder.insert("host".to_string(), json!(host));
    }
    manifest.insert("builder".to_string(), Value::Object(builder));
    manifest.insert("components".to_string(), Value::Object(components));
    manifest.insert("files".to_string(), Value::Array(files));
    let manifest_bytes = serde_json::to_vec_pretty(&Value::Object(manifest))
        .map_err(|e| PackError::Invalid(format!("manifest 序列化失败：{e}")))?;

    // ⑨ 落盘顺序：manifest 第一，其余保持升序。
    let mut entries: Vec<(String, Vec<u8>)> =
        Vec::with_capacity(payload.len() + 1);
    entries.push((MANIFEST_PATH.to_string(), manifest_bytes));
    entries.extend(payload);
    Ok(zip::write(&entries)?)
}

/// `components.am` 的两个计数（来自 AM 文件本身，**不猜**）。
///
/// 解析失败就是失败：计数说不清时宁可让 `pack` 退出 1，
/// 也不要往 manifest 里写一个「看起来合理」的 0。
fn count_am(bytes: &[u8]) -> Result<(usize, usize), PackError> {
    let v: Value = serde_json::from_slice(bytes)
        .map_err(|e| PackError::Invalid(format!("`--am` 的文件不是合法 JSON：{e}")))?;
    let n = |k: &str| -> Result<usize, PackError> {
        match v.get(k) {
            Some(Value::Array(a)) => Ok(a.len()),
            None => Ok(0),
            Some(_) => Err(PackError::Invalid(format!(
                "`--am` 的 `{k}` 必须是数组（schema / access 契约的口径）"
            ))),
        }
    };
    Ok((n("entities")?, n("sources")?))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 反空转：项目名 pattern 的每条规则都各有一条正例与一条反例。
    ///
    /// ⚠️ REQ-159 · 用户 + join 拍板 A：`_` 从「拒绝」改成「允许」
    /// （真实项目名 `anspire_real`）。本用例改成「先拒后放」的那一格也在册，
    /// 免得下次有人凭印象把它改回去。
    #[test]
    fn project_name_pattern_matches_the_schema() {
        for ok in [
            "ab",
            "cloud-resource",
            "a1",
            "x-9",
            "anspire_real", // ← 拍板 A 的动机：带下划线的真实项目名
            "a_b",
            "demo_3-x",
        ] {
            assert!(is_valid_project_name(ok), "`{ok}` 应当合法");
        }
        for bad in [
            "",             // 太短
            "a",            // pattern 要求首字符 + 至少 1 个
            "1",            // 同上
            "-abc",         // 首字符不许是 `-`
            "_abc",         // 首字符不许是 `_`
            "A_bad",        // 大写
            "a b",          // 空格
            "a/b",          // 斜杠
            "a.b",          // 点
            "a..b",         // 双点
            "中文",         // 非 ASCII
            "anspire real", // 空格（用户口径的反例）
            "anspire/real", // 斜杠（用户口径的反例）
        ] {
            assert!(!is_valid_project_name(bad), "`{bad}` 应当被拒");
        }
        // 63 = 合法上限；64 = 越界（下划线也走同一条长度规则）
        assert!(is_valid_project_name(&"a".repeat(63)));
        assert!(!is_valid_project_name(&"a".repeat(64)));
        assert!(is_valid_project_name(&format!("a{}", "_".repeat(62))));
        // pattern 原文必须与实现同步（文案里印的就是它）。
        assert_eq!(PROJECT_NAME_PATTERN, "^[a-z0-9][a-z0-9_-]{1,62}$");
    }
}

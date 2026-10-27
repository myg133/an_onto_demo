//! `bundle verify` 的**纯函数**核心：把包按契约逐条验一遍，出 [`Report`]。
//!
//! ⛔ 不连库、不读配置文件、不发网络：入参就是包的**字节**。
//! 这条纪律是承重的 —— 包的自检必须在没有数据库的机器上跑（CI、FDE 的笔记本、
//! 服务器上的 `docker run` 里），否则「包不合规」会被「连不上库」盖住。
//!
//! # 检查顺序（为什么不是契约里那个 1..9 的顺序）
//!
//! | 步 | 检查 | 码 |
//! | --- | --- | --- |
//! | 0 | 体积 ≤ 100 MB | [`code::TOO_LARGE`] |
//! | 1 | 容器能解析（EOCD → central directory → 逐 entry） | [`code::MANIFEST_INVALID`] |
//! | 2 | 每个 entry 的 CRC32 与内容相符 | [`code::SHA256_MISMATCH`] |
//! | 3 | `manifest.json` 存在、是合法 JSON、符合 schema（必填 / 类型 / 未知字段） | [`code::MANIFEST_INVALID`] |
//! | 4 | `bundle_version` 只认 `"1.0"` | [`code::VERSION_UNSUPPORTED`] |
//! | 5 | 所有路径合法（entry 名 + `files[].path` + `components.*.path`） | [`code::PATH_ILLEGAL`] |
//! | 6 | `files[]` 与包内实际文件**集合相等**（不含 `manifest.json` 自己） | [`code::FILES_MISMATCH`] |
//! | 7 | 逐文件 sha256 与 `bytes` | [`code::SHA256_MISMATCH`] |
//! | 8 | `requires_dsn` / `dialect` / `components.*.path` 都在包内 | [`code::MANIFEST_INVALID`] |
//! | 9 | `link_types[*]` 必须带 `evidence` 键 | [`code::EVIDENCE_MISSING`] |
//! | 10 | 全文扫凭据（**警告级**，v1 只告警不拒） | [`code::CREDENTIAL_SUSPECT`] |
//!
//! 顺序上的两处刻意调整，理由都是「让报告指向真原因」：
//!
//! - **体积最先**：一个 200 MB 的包不必再逐 entry 解一遍。
//! - **版本先于文件集合**：版本不认识时，`files[]` 那套语义本身可能就是另一套，
//!   先报一堆「文件不符」只会把人引到错的方向。
//!
//! 契约里那 8 个码中的 [`code::PROJECT_MISMATCH`] 与 [`code::DSN_MISSING`]
//! **本命令判不出来**（构建端不知道收件项目、也永远不该有 DSN），
//! 它们在服务端（REQ-159 P1a）判 —— 见 [`crate::bundle`] 模块头的表。
//!
//! # 关于 `manifest.json` 该不该列进 `files[]`
//!
//! **不列**。manifest 无法包含自己的 sha256（自指），列进去只能是错的。
//! ⇒ 本命令把 `files[]` 里的 `manifest.json` 条目当作**多余**（[`code::FILES_MISMATCH`]）。
//! 契约的 `manifest.example.json` 里列了这一条，但那是**示例文件**
//! （它的 sha256 全是 `REPLACE_WITH_REAL_SHA256_64_HEX` 占位）——
//! 派单 §3.2 的正文写得明确：「`files[]` = 除 `manifest.json` 自身外，包内每一个文件」。
//!
//! # 8 个拒绝码之外的两条
//!
//! [`code::EVIDENCE_MISSING`] 与 [`code::CREDENTIAL_SUSPECT`] 是**构建端**的补充：
//! 前者钉「别把视图当快照」（服务端看不出区别，只会静默丢证据），
//! 后者对应 demand §三-5「v1 只告警不拒」。

use crate::bundle::zip;
use crate::bundle::{
    code, Problem, BUNDLE_VERSION, MANIFEST_PATH, MAX_BUNDLE_BYTES, SNAPSHOT_PATH,
    SUPPORTED_BUNDLE_VERSIONS,
};
use crate::query::audit::sha256_hex;
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::BTreeSet;

/// 包级问题（不属于某个包内文件）的 `path` 占位符。
const PKG: &str = "(package)";

/// 凭据扫描的 5 个模式（demand §十一-3 第 5 条 + 派单 §4 第 8 条）。
const CREDENTIAL_PATTERNS: &[&str] = &[
    "postgres://",
    "mysql://",
    "-----BEGIN",
    "\"password\"",
    "\"dsn\"",
];

// ───────────────────────── 报告 ─────────────────────────

/// 包的概况（JSON 的 `summary` 那一格）。**全部从包里读出来**，读不到就是 `None`。
#[derive(Debug, Clone, Default, Serialize)]
pub struct Summary {
    /// 容器里一共几个 entry（含 `manifest.json`）。
    pub entries: usize,
    /// `files[]` 声明了几个文件。
    pub files_in_manifest: usize,
    /// 容器字节数。
    pub bytes: u64,
    pub bundle_version: Option<String>,
    pub project: Option<String>,
    pub ontology_seq: Option<i64>,
    pub built_at: Option<String>,
    /// `components.db.requires_dsn`（没有 db 组件 ⇒ `None`）。
    pub requires_dsn: Option<bool>,
    /// `components.ontology.entity_types`（manifest 自报的计数，**不是**重新数出来的）。
    pub entity_types: Option<u64>,
    /// `components.ontology.link_types`。
    pub link_types: Option<u64>,
}

/// 自检结论。
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    /// `problems` 为空才是 `true`（警告不影响它 —— v1 凭据只告警）。
    pub ok: bool,
    /// **阻断级**问题（任一非空 ⇒ `bundle verify` 退出码 2）。
    pub problems: Vec<Problem>,
    /// 非阻断级（今天只有凭据可疑）。
    pub warnings: Vec<Problem>,
    pub summary: Summary,
}

impl Report {
    fn finish(problems: Vec<Problem>, warnings: Vec<Problem>, summary: Summary) -> Self {
        Self { ok: problems.is_empty(), problems, warnings, summary }
    }

    /// `bundle verify` 打到 stdout 的 JSON（稳定形状）。
    pub fn to_json(&self) -> Value {
        serde_json::json!({
            "ok": self.ok,
            "problems": self.problems,
            "warnings": self.warnings,
            "summary": self.summary,
        })
    }

    /// `bundle verify` 打到 stderr 的人话。
    ///
    /// 与 JSON 那份**同时**产出（口径同 `access validate` / `relation verbs check`）：
    /// 人看这一份，脚本 `| jq` 那一份。
    pub fn human(&self) -> String {
        let mut s = String::new();
        let summ = &self.summary;
        s.push_str(&format!(
            "bundle verify · {} 第 {} 版 · 包格式 {} · {} 个文件 / {} 字节\n",
            summ.project.as_deref().unwrap_or("（项目名读不出）"),
            summ.ontology_seq
                .map(|n| n.to_string())
                .unwrap_or_else(|| "?".to_string()),
            summ.bundle_version.as_deref().unwrap_or("?"),
            summ.files_in_manifest,
            summ.bytes
        ));
        if let Some(t) = &summ.built_at {
            s.push_str(&format!("  打包时间 {t}（构建端的标识，服务端入库会重分配版本号）\n"));
        }
        if let Some(r) = summ.requires_dsn {
            s.push_str(&format!(
                "  源库      {}\n",
                if r {
                    "requires_dsn=true ⇒ 服务端会先要 DSN（包里不含 DSN）"
                } else {
                    "requires_dsn=false ⇒ 有 schema 描述但不连库"
                }
            ));
        }
        if self.ok {
            s.push_str("✓ 通过：manifest 符合 schema，files[] 覆盖包内每个文件，sha256 全部一致，路径安全\n");
        } else {
            s.push_str(&format!("✗ 拒绝：{} 个问题（退出码 2）\n", self.problems.len()));
            for p in &self.problems {
                s.push_str(&format!("  [{}] {}\n", p.code, p.path));
                s.push_str(&format!("      结论：{}\n", p.message));
                s.push_str(&format!("      改法：{}\n", p.hint));
            }
        }
        if !self.warnings.is_empty() {
            s.push_str(&format!(
                "⚠ {} 条警告（v1 只告警不拒，见契约 §三-5）\n",
                self.warnings.len()
            ));
            for w in &self.warnings {
                s.push_str(&format!("  [{}] {}\n", w.code, w.path));
                s.push_str(&format!("      结论：{}\n", w.message));
                s.push_str(&format!("      改法：{}\n", w.hint));
            }
        }
        s
    }
}

// ───────────────────────── 入口 ─────────────────────────

/// 自检一个包（体积上限用契约默认的 100 MB）。
pub fn verify(bytes: &[u8]) -> Report {
    verify_with_limit(bytes, MAX_BUNDLE_BYTES)
}

/// 自检一个包，**体积上限可注入**（[`verify`] 就是 `limit = ` [`MAX_BUNDLE_BYTES`] 的那次调用）。
///
/// 注入限额不是为了生产用法，是为了能真测 [`code::TOO_LARGE`]：
/// 造一个 100 MB 的包只为了断言一行 exit code，是把测试写成暴力。
pub fn verify_with_limit(bytes: &[u8], max_bytes: u64) -> Report {
    let mut problems: Vec<Problem> = Vec::new();
    let mut warnings: Vec<Problem> = Vec::new();
    let mut summary = Summary { bytes: bytes.len() as u64, ..Summary::default() };

    // 0 · 体积（最便宜的失败先报；一个 200 MB 的包不值得再逐 entry 解一遍）。
    if bytes.len() as u64 > max_bytes {
        problems.push(Problem::new(
            code::TOO_LARGE,
            PKG,
            format!("包有 {} 字节，超过上限 {max_bytes} 字节", bytes.len()),
            "契约 §十一-4 第 8 条的上限是 100 MB。先看是不是把数据行 / 样本库塞进来了\
             （包只放表/列/关系映射，⛔ 不含数据）；确实是本体太大就找服务端调上限",
        ));
        return Report::finish(problems, warnings, summary);
    }

    // 1 · 容器。
    let entries = match zip::read(bytes) {
        Ok(e) => e,
        Err(e) => {
            problems.push(Problem::new(
                code::MANIFEST_INVALID,
                PKG,
                format!("包不是能读的 zip：{e}"),
                "`.ontokbundle` 内部是 **store-only zip**（method 0、无 data descriptor）。\
                 重新 `ontok-builder bundle pack` 打一份；手改过的包（或 zip 重压缩过的）这里就会炸",
            ));
            return Report::finish(problems, warnings, summary);
        }
    };
    summary.entries = entries.len();

    // 2 · 容器内的 CRC32（「解开了但内容坏了」与「解不开」不是同一件事）。
    for e in &entries {
        if !e.crc_ok {
            problems.push(Problem::new(
                code::SHA256_MISMATCH,
                e.path.clone(),
                format!(
                    "容器内 CRC32 与数据不符（entry 里存的是 {:08x}，按数据重算对不上）—— \
                     这个文件在运输途中被改过或截断过",
                    e.crc32
                ),
                "重新打包。⛔ 不要手工修 zip：CRC 修好了 sha256 也还是不符，两处一起骗不过去",
            ));
        }
    }

    // 3 · manifest 存在 + 是 JSON + 符合 schema。
    let Some(manifest_entry) = entries.iter().find(|e| e.path == MANIFEST_PATH) else {
        problems.push(Problem::new(
            code::MANIFEST_INVALID,
            MANIFEST_PATH,
            format!("包里没有 `{MANIFEST_PATH}`（实际 entry：{}）", entry_list(&entries)),
            "manifest 是包的目录，缺了服务端无法判断包里有什么。重新 `bundle pack` 即可",
        ));
        return Report::finish(problems, warnings, summary);
    };
    let manifest: Value = match serde_json::from_slice(&manifest_entry.data) {
        Ok(v) => v,
        Err(e) => {
            problems.push(Problem::new(
                code::MANIFEST_INVALID,
                MANIFEST_PATH,
                format!("`{MANIFEST_PATH}` 不是合法 JSON（{e}）"),
                "重新打包。manifest 由工具生成，手改一定会漏掉 sha256 / bytes 这两格",
            ));
            return Report::finish(problems, warnings, summary);
        }
    };
    let mut structural: Vec<Problem> = Vec::new();
    validate_manifest(&manifest, &mut structural);
    fill_summary(&manifest, &mut summary);
    problems.extend(structural);

    // 4 · 版本（不认识就在这里停：后面那套字段语义可能根本不是这一版的）。
    match manifest.get("bundle_version").and_then(Value::as_str) {
        Some(v) if SUPPORTED_BUNDLE_VERSIONS.contains(&v) => {}
        Some(v) => problems.push(Problem::new(
            code::VERSION_UNSUPPORTED,
            "manifest.json.bundle_version",
            format!(
                "包格式版本 `{v}` 不认识（认识的：{}）",
                SUPPORTED_BUNDLE_VERSIONS.join(" / ")
            ),
            format!("本工具只会写 `{BUNDLE_VERSION}`。把构建端升/降级到与服务端同一版，或让服务端支持 `{v}`"),
        )),
        // 缺字段 / 类型不符已由 validate_manifest 报过，这里不重复。
        None => {}
    }

    // 5 · 路径（entry 名 + files[].path + components.*.path 一起看：
    //     判据是「包内不许出现会被解包器当路径穿越的名字」，写在哪一格都一样）。
    for e in &entries {
        if let Some(why) = zip::illegal_path_reason(&e.path) {
            problems.push(Problem::new(
                code::PATH_ILLEGAL,
                e.path.clone(),
                format!("包内 entry 名非法：{why}"),
                "路径只许 [A-Za-z0-9._-/]、相对、不含 `..`。重新打包即可（pack 会先拒这种名字）",
            ));
        }
    }
    let declared_files = manifest.get("files").and_then(Value::as_array);
    if let Some(arr) = declared_files {
        for (i, f) in arr.iter().enumerate() {
            if let Some(p) = f.get("path").and_then(Value::as_str) {
                if let Some(why) = path_problem(p) {
                    problems.push(Problem::new(
                        code::PATH_ILLEGAL,
                        format!("manifest.json.files[{i}].path"),
                        format!("声明的路径 `{p}` 非法：{why}"),
                        "路径只许 [A-Za-z0-9._-/]、相对、不含 `..`（防穿越）",
                    ));
                }
            }
        }
    }
    for (field, value) in component_paths(&manifest) {
        if let Some(why) = path_problem(&value) {
            problems.push(Problem::new(
                code::PATH_ILLEGAL,
                field.clone(),
                format!("`{value}` 非法：{why}"),
                "components 里的 path 也必须是包内的相对路径（解包器只认这一种）",
            ));
        }
    }

    // 6 · `files[]` 与实际文件集合相等。
    let actual: BTreeSet<&str> = entries
        .iter()
        .map(|e| e.path.as_str())
        .filter(|p| *p != MANIFEST_PATH)
        .collect();
    let mut declared: Vec<(&str, &Value)> = Vec::new();
    if let Some(arr) = declared_files {
        for f in arr {
            if let Some(p) = f.get("path").and_then(Value::as_str) {
                declared.push((p, f));
            }
        }
    }
    let declared_set: BTreeSet<&str> = declared.iter().map(|(p, _)| *p).collect();
    let missing: Vec<&str> = actual.difference(&declared_set).copied().collect();
    let extra: Vec<&str> = declared_set.difference(&actual).copied().collect();
    if !missing.is_empty() || !extra.is_empty() {
        let mut msg = String::new();
        if !missing.is_empty() {
            msg.push_str(&format!("包里多出 {} 个未声明的文件：{}", missing.len(), missing.join(", ")));
        }
        if !extra.is_empty() {
            if !msg.is_empty() {
                msg.push_str("；");
            }
            let listed_manifest = extra.contains(&MANIFEST_PATH);
            msg.push_str(&format!(
                "`files[]` 多声明了 {} 个包里没有的文件：{}{}",
                extra.len(),
                extra.join(", "),
                if listed_manifest {
                    "（`manifest.json` 不许列进 `files[]`：它无法包含自己的 sha256）"
                } else {
                    ""
                }
            ));
        }
        problems.push(Problem::new(
            code::FILES_MISMATCH,
            "manifest.json.files",
            msg,
            "`files[]` 必须与包内文件**集合相等**（除 `manifest.json` 自身）。\
             重新 `bundle pack` 是唯一不会漏的做法",
        ));
    }

    // 7 · 逐文件 sha256 + bytes。
    for (path, f) in &declared {
        let Some(entry) = entries.iter().find(|e| e.path == *path) else {
            continue; // 缺文件已在 6 里点过名，这里不重复报
        };
        let real_sha = sha256_hex(&entry.data);
        let real_len = entry.data.len();
        if let Some(declared_sha) = f.get("sha256").and_then(Value::as_str) {
            if declared_sha != real_sha {
                problems.push(Problem::new(
                    code::SHA256_MISMATCH,
                    path.to_string(),
                    format!("sha256 不符：manifest 说 {declared_sha}，包里的字节算出 {real_sha}"),
                    "这个文件的字节被改过（或 manifest 是手改的）。重新打包，别手改 manifest",
                ));
            }
        }
        if let Some(declared_len) = f.get("bytes").and_then(Value::as_i64) {
            if declared_len != real_len as i64 {
                problems.push(Problem::new(
                    code::SHA256_MISMATCH,
                    path.to_string(),
                    format!("bytes 不符：manifest 说 {declared_len}，实际 {real_len} 字节"),
                    "`bytes` 与 `sha256` 是同一格事实的两种写法，改一个不改另一个一定会被拒",
                ));
            }
        }
    }

    // 8 · `requires_dsn` / `dialect` / 组件 path 的落点。
    if let Some(db) = manifest.get("components").and_then(|c| c.get("db")) {
        let requires_dsn = db.get("requires_dsn").and_then(Value::as_bool);
        if requires_dsn == Some(true) && db.get("dialect").and_then(Value::as_str).is_none() {
            problems.push(Problem::new(
                code::MANIFEST_INVALID,
                "manifest.json.components.db.dialect",
                "`requires_dsn=true` 但没写 `dialect`（schema 的 allOf 要求此时 dialect 必填）",
                "要么给 `--db-dialect postgresql`（缺省就是它），要么把 `--requires-dsn` 去掉",
            ));
        }
        // ⚠️ `components.db.path` 的「在不在包里」由下面那个统一循环判 ——
        // 这里再判一次就会对同一个缺陷报两条一样的问题。
    }
    for (field, value) in component_paths(&manifest) {
        if !actual.contains(value.as_str()) && value != MANIFEST_PATH {
            problems.push(Problem::new(
                code::MANIFEST_INVALID,
                field.clone(),
                format!("`{value}` 不在包里（实际 entry：{}）", entry_list(&entries)),
                "components 里的每个 path 都要在包里真实存在，并且被 `files[]` 覆盖",
            ));
        }
    }

    // 9 · 快照里的关系证据。
    if let Some(snap_entry) = entries.iter().find(|e| e.path == SNAPSHOT_PATH) {
        match serde_json::from_slice::<Value>(&snap_entry.data) {
            Ok(snap) => {
                if let Some(links) = snap.get("link_types").and_then(Value::as_array) {
                    let mut offenders: Vec<usize> = Vec::new();
                    for (i, l) in links.iter().enumerate() {
                        match l.as_object() {
                            Some(o) if o.contains_key("evidence") => {}
                            _ => offenders.push(i),
                        }
                    }
                    if !offenders.is_empty() {
                        let shown: Vec<String> = offenders
                            .iter()
                            .take(3)
                            .map(|i| format!("link_types[{i}] key={}", key_of(&links[*i])))
                            .collect();
                        problems.push(Problem::new(
                            code::EVIDENCE_MISSING,
                            SNAPSHOT_PATH,
                            format!(
                                "{} / {} 条关系没有 `evidence` 键（前几条：{}）",
                                offenders.len(),
                                links.len(),
                                shown.join("、")
                            ),
                            "这份快照像是**视图**（`LinkTypeView` 只有 5 个字段，没有 evidence/table/column）。\
                             包里的快照必须是 `PgStore::get_snapshot` 读出的 `Snapshot` **原文**，\
                             中间不许经过任何 view / filter / projection",
                        ));
                    }
                }
            }
            Err(e) => problems.push(Problem::new(
                code::MANIFEST_INVALID,
                SNAPSHOT_PATH,
                format!("`{SNAPSHOT_PATH}` 不是合法 JSON（{e}）"),
                "重新打包（这一份由 `serde_json::to_vec(&Snapshot)` 直出）",
            )),
        }
    }

    // 10 · 凭据（**警告级**：v1 只告警不拒，见 demand §三-5）。
    for e in &entries {
        let text = String::from_utf8_lossy(&e.data);
        let hits: Vec<&str> = CREDENTIAL_PATTERNS
            .iter()
            .copied()
            .filter(|p| text.contains(p))
            .collect();
        if !hits.is_empty() {
            warnings.push(Problem::new(
                code::CREDENTIAL_SUSPECT,
                e.path.clone(),
                format!("扫到可疑片段：{}", hits.join(" / ")),
                "包里⛔ 不许有凭据（密码 / 私钥 / DSN）。v1 只告警不拒，但请确认这一份不是把环境机密打进来了",
            ));
        }
    }

    Report::finish(problems, warnings, summary)
}

/// 路径判据的**唯一**入口（`zip::illegal_path_reason` + schema pattern 里的 `(?!.*\.\.)`）。
///
/// 为什么把 `..` 的单查并进来：`manifest.schema.json` 的 pattern 是
/// `^(?!/)(?!.*\.\.)[A-Za-z0-9._/-]+$` —— 它连 `a..b.json` 这种**没有段**的
/// 双点在中间也拒。我们按 schema 的字面来（打包端也用同一条 ⇒ 打得出来的一定过得了自检）。
fn path_problem(p: &str) -> Option<String> {
    if let Some(why) = zip::illegal_path_reason(p) {
        return Some(why.to_string());
    }
    if p.contains("..") {
        return Some("含 `..`（schema 的 path pattern 不许，防穿越）".to_string());
    }
    None
}

fn entry_list(entries: &[zip::Entry]) -> String {
    if entries.is_empty() {
        "（空包）".to_string()
    } else {
        entries.iter().map(|e| e.path.as_str()).collect::<Vec<_>>().join(", ")
    }
}

fn key_of(v: &Value) -> &str {
    v.get("key").and_then(Value::as_str).unwrap_or("?")
}

/// `components.*.path` 的 `(字段路径, 取值)`。
fn component_paths(manifest: &Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Some(c) = manifest.get("components").and_then(Value::as_object) {
        for (name, comp) in c {
            if let Some(p) = comp.get("path").and_then(Value::as_str) {
                out.push((format!("manifest.json.components.{name}.path"), p.to_string()));
            }
        }
    }
    out
}

fn fill_summary(manifest: &Value, s: &mut Summary) {
    let str_at = |p: &[&str]| -> Option<String> {
        let mut cur = manifest;
        for k in p {
            cur = cur.get(*k)?;
        }
        cur.as_str().map(str::to_string)
    };
    let int_at = |p: &[&str]| -> Option<i64> {
        let mut cur = manifest;
        for k in p {
            cur = cur.get(*k)?;
        }
        cur.as_i64()
    };
    s.bundle_version = str_at(&["bundle_version"]);
    s.project = str_at(&["project"]);
    s.ontology_seq = int_at(&["ontology_seq"]);
    s.built_at = str_at(&["built_at"]);
    s.requires_dsn = manifest
        .get("components")
        .and_then(|c| c.get("db"))
        .and_then(|d| d.get("requires_dsn"))
        .and_then(Value::as_bool);
    s.entity_types = int_at(&["components", "ontology", "entity_types"]).map(|n| n as u64);
    s.link_types = int_at(&["components", "ontology", "link_types"]).map(|n| n as u64);
    s.files_in_manifest = manifest
        .get("files")
        .and_then(Value::as_array)
        .map(|a| a.len())
        .unwrap_or(0);
}

// ───────────────────────── manifest 结构校验 ─────────────────────────

/// 手写 JSON Schema 校验（子集：`required` / `type` / `pattern` / `enum` /
/// `additionalProperties:false` / `minimum` / `minItems` / `if-then`）。
///
/// ⛔ 不引 `jsonschema` crate（Cargo.toml 的第一条纪律）。
/// 好处是**每条问题都带字段路径**（`components.db.dialect`），
/// 而库给的是「line 12 column 5 不符合 schema」—— 那种话改不动代码。
fn validate_manifest(m: &Value, out: &mut Vec<Problem>) {
    let Some(obj) = m.as_object() else {
        out.push(Problem::new(
            code::MANIFEST_INVALID,
            MANIFEST_PATH,
            "顶层必须是 JSON 对象",
            "重新打包（manifest 由工具生成）",
        ));
        return;
    };
    const ROOT_KEYS: &[&str] = &[
        "bundle_version",
        "project",
        "ontology_seq",
        "built_at",
        "builder",
        "components",
        "files",
    ];
    reject_unknown_keys(obj, ROOT_KEYS, "", out);
    for k in ROOT_KEYS {
        if !obj.contains_key(*k) {
            out.push(missing(&join("", k)));
        }
    }

    // bundle_version：类型（取值由第 4 步判）
    want_str(obj, "bundle_version", "", out);

    // project：pattern
    if let Some(p) = want_str(obj, "project", "", out) {
        if !crate::bundle::pack::is_valid_project_name(p) {
            out.push(Problem::new(
                code::MANIFEST_INVALID,
                "manifest.json.project",
                format!("`{p}` 不符合 pattern `^[a-z0-9][a-z0-9-]{{1,62}}$`"),
                "项目名只许小写字母 / 数字 / 连字符，首字符是字母或数字，总长 2..=63",
            ));
        }
    }

    // ontology_seq：integer ≥ 1
    if let Some(seq) = want_i64(obj, "ontology_seq", "", out) {
        if seq < 1 {
            out.push(Problem::new(
                code::MANIFEST_INVALID,
                "manifest.json.ontology_seq",
                format!("`ontology_seq` 必须 ≥ 1（schema 的 minimum），实际 {seq}"),
                "传 `--version <n>` 且 n ≥ 1",
            ));
        }
    }

    // built_at：format date-time（RFC3339）
    if let Some(t) = want_str(obj, "built_at", "", out) {
        if chrono::DateTime::parse_from_rfc3339(t).is_err() {
            out.push(Problem::new(
                code::MANIFEST_INVALID,
                "manifest.json.built_at",
                format!("`{t}` 不是 RFC3339 时间（schema 的 format: date-time）"),
                "用 `--built-at 2026-10-27T02:30:00Z` 这种带 Z / 偏移的写法",
            ));
        }
    }

    // builder
    if let Some(b) = want_obj(obj, "builder", "", out) {
        reject_unknown_keys(b, &["name", "version", "host"], "builder", out);
        for k in ["name", "version"] {
            if !b.contains_key(k) {
                out.push(missing(&join("builder", k)));
            }
            want_str(b, k, "builder", out);
        }
        if b.contains_key("host") {
            want_str(b, "host", "builder", out);
        }
    }

    // components
    if let Some(c) = want_obj(obj, "components", "", out) {
        reject_unknown_keys(c, &["ontology", "am", "db"], "components", out);
        if !c.contains_key("ontology") {
            out.push(missing("components.ontology"));
        }
        if let Some(o) = want_obj(c, "ontology", "components", out) {
            component_common(o, "components.ontology", out);
        }
        if let Some(a) = want_obj(c, "am", "components", out) {
            component_common(a, "components.am", out);
        }
        if let Some(d) = want_obj(c, "db", "components", out) {
            reject_unknown_keys(
                d,
                &["path", "sha256", "requires_dsn", "dialect"],
                "components.db",
                out,
            );
            for k in ["path", "sha256", "requires_dsn"] {
                if !d.contains_key(k) {
                    out.push(missing(&join("components.db", k)));
                }
            }
            if let Some(sha) = want_str(d, "sha256", "components.db", out) {
                check_sha256(sha, "components.db.sha256", out);
            }
            want_str(d, "path", "components.db", out);
            want_bool(d, "requires_dsn", "components.db", out);
            if let Some(dialect) = want_str(d, "dialect", "components.db", out) {
                if !DIALECTS.contains(&dialect) {
                    out.push(Problem::new(
                        code::MANIFEST_INVALID,
                        "manifest.json.components.db.dialect",
                        format!("`{dialect}` 不在 enum 里（{}）", DIALECTS.join(" / ")),
                        "方言只能是 postgresql / mysql / sqlserver / oracle",
                    ));
                }
            }
        }
    }

    // files：array(minItems 1) of {path, sha256, bytes}
    match obj.get("files") {
        None => {} // 缺字段已在上面报过
        Some(Value::Array(arr)) => {
            if arr.is_empty() {
                out.push(Problem::new(
                    code::MANIFEST_INVALID,
                    "manifest.json.files",
                    "`files` 是空数组（schema 的 minItems: 1）",
                    "包里至少要有 `ontology/snapshot.json` 这一条",
                ));
            }
            for (i, item) in arr.iter().enumerate() {
                let Some(f) = item.as_object() else {
                    out.push(type_error(&format!("files[{i}]"), "object"));
                    continue;
                };
                let base = format!("files[{i}]");
                reject_unknown_keys(f, &["path", "sha256", "bytes"], &base, out);
                for k in ["path", "sha256", "bytes"] {
                    if !f.contains_key(k) {
                        out.push(missing(&format!("{base}.{k}")));
                    }
                }
                if let Some(sha) = want_str(f, "sha256", &base, out) {
                    check_sha256(sha, &format!("manifest.json.{base}.sha256"), out);
                }
                // ⚠️ `path` 的合法性**不在这里判**：它归 `bundle.path.illegal`（第 5 步），
                // 由 [`path_problem`] 一个入口说话 —— 同一个缺陷不该出两个码。
                if let Some(n) = want_i64(f, "bytes", &base, out) {
                    if n < 0 {
                        out.push(Problem::new(
                            code::MANIFEST_INVALID,
                            format!("manifest.json.{base}.bytes"),
                            format!("`bytes` 不能为负（实际 {n}）"),
                            "`bytes` 是文件真实长度，重新打包即可",
                        ));
                    }
                }
            }
        }
        Some(other) => out.push(type_error("files", &type_name(other))),
    }
}

/// 契约里 `dialect.enum` 的四个取值 —— 与打包端**同一个常量**（不各写一份）。
const DIALECTS: &[&str] = crate::bundle::pack::DIALECTS;

/// `$defs.component` 的公共部分（`path` + `sha256`；`additionalProperties: true`）。
fn component_common(o: &Map<String, Value>, base: &str, out: &mut Vec<Problem>) {
    for k in ["path", "sha256"] {
        if !o.contains_key(k) {
            out.push(missing(&join(base, k)));
        }
    }
    want_str(o, "path", base, out);
    if let Some(sha) = want_str(o, "sha256", base, out) {
        check_sha256(sha, &format!("manifest.json.{base}.sha256"), out);
    }
}

fn check_sha256(sha: &str, at: &str, out: &mut Vec<Problem>) {
    let ok = sha.len() == 64
        && sha
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if !ok {
        out.push(Problem::new(
            code::MANIFEST_INVALID,
            format!("manifest.json.{at}"),
            format!("`{sha}` 不是 64 位小写十六进制（schema 的 `^[0-9a-f]{{64}}$`）"),
            "sha256 由工具算，⛔ 不要手写；重新打包即可",
        ));
    }
}

fn reject_unknown_keys(o: &Map<String, Value>, allowed: &[&str], base: &str, out: &mut Vec<Problem>) {
    for k in o.keys() {
        if !allowed.contains(&k.as_str()) {
            let at = join(base, k);
            out.push(Problem::new(
                code::MANIFEST_INVALID,
                format!("manifest.json.{at}"),
                format!("`{at}` 不是 schema 里的字段（`additionalProperties: false` 会拒整包）"),
                if base.is_empty() {
                    format!("顶层只许：{}", allowed.join(" / "))
                } else {
                    format!("`{base}` 下只许：{}", allowed.join(" / "))
                },
            ));
        }
    }
}

fn join(base: &str, k: &str) -> String {
    if base.is_empty() {
        k.to_string()
    } else {
        format!("{base}.{k}")
    }
}

fn missing(at: &str) -> Problem {
    Problem::new(
        code::MANIFEST_INVALID,
        format!("manifest.json.{at}"),
        format!("缺必填字段 `{at}`（schema 的 required）—— ⛔ 缺字段不猜默认值"),
        "重新打包。手写 manifest 会缺这一格，而服务端不会替你补",
    )
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn type_error(at: &str, want: &str) -> Problem {
    Problem::new(
        code::MANIFEST_INVALID,
        format!("manifest.json.{at}"),
        format!("类型不符：schema 要 {want}"),
        "重新打包（manifest 由工具生成）",
    )
}

fn want_str<'a>(
    o: &'a Map<String, Value>,
    k: &str,
    base: &str,
    out: &mut Vec<Problem>,
) -> Option<&'a str> {
    match o.get(k) {
        None => None,
        Some(Value::String(s)) => Some(s),
        Some(other) => {
            out.push(type_error(&join(base, k), &format!("string（实际 {}）", type_name(other))));
            None
        }
    }
}

fn want_i64(o: &Map<String, Value>, k: &str, base: &str, out: &mut Vec<Problem>) -> Option<i64> {
    match o.get(k) {
        None => None,
        Some(Value::Number(n)) => match n.as_i64() {
            Some(v) => Some(v),
            None => {
                out.push(type_error(&join(base, k), "integer"));
                None
            }
        },
        Some(other) => {
            out.push(type_error(&join(base, k), &format!("integer（实际 {}）", type_name(other))));
            None
        }
    }
}

fn want_bool(o: &Map<String, Value>, k: &str, base: &str, out: &mut Vec<Problem>) -> Option<bool> {
    match o.get(k) {
        None => None,
        Some(Value::Bool(b)) => Some(*b),
        Some(other) => {
            out.push(type_error(&join(base, k), &format!("boolean（实际 {}）", type_name(other))));
            None
        }
    }
}

fn want_obj<'a>(
    o: &'a Map<String, Value>,
    k: &str,
    base: &str,
    out: &mut Vec<Problem>,
) -> Option<&'a Map<String, Value>> {
    match o.get(k) {
        None => None,
        Some(Value::Object(m)) => Some(m),
        Some(other) => {
            out.push(type_error(&join(base, k), &format!("object（实际 {}）", type_name(other))));
            None
        }
    }
}

// ───────────────────────── 单测（纯函数，不连库） ─────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> Value {
        serde_json::json!({
            "bundle_version": "1.0",
            "project": "cloud-resource",
            "ontology_seq": 5,
            "built_at": "2026-10-27T02:30:00Z",
            "builder": {"name": "ontok-builder", "version": "0.1.0"},
            "components": {"ontology": {"path": "ontology/snapshot.json", "sha256": "a".repeat(64)}},
            "files": [{"path": "ontology/snapshot.json", "sha256": "a".repeat(64), "bytes": 2}],
        })
    }

    #[test]
    fn a_minimal_valid_manifest_has_no_structural_problems() {
        let mut out = Vec::new();
        validate_manifest(&manifest(), &mut out);
        assert!(out.is_empty(), "不该有问题：{:?}", out);
    }

    #[test]
    fn unknown_root_key_is_reported_with_its_path() {
        let mut m = manifest();
        m["extra"] = Value::from(1);
        let mut out = Vec::new();
        validate_manifest(&m, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].path, "manifest.json.extra");
        assert!(out[0].message.contains("additionalProperties"));
    }

    #[test]
    fn missing_required_field_is_named_not_defaulted() {
        let mut m = manifest();
        m.as_object_mut().unwrap().remove("built_at");
        let mut out = Vec::new();
        validate_manifest(&m, &mut out);
        assert!(out.iter().any(|p| p.path == "manifest.json.built_at"));
    }

    #[test]
    fn wrong_type_is_named_with_the_field_path() {
        let mut m = manifest();
        m["ontology_seq"] = Value::from("5");
        let mut out = Vec::new();
        validate_manifest(&m, &mut out);
        assert!(out
            .iter()
            .any(|p| p.path == "manifest.json.ontology_seq" && p.message.contains("integer")));
    }

    #[test]
    fn sha256_must_be_64_lowercase_hex() {
        let mut m = manifest();
        m["files"][0]["sha256"] = Value::from("REPLACE_WITH_REAL_SHA256_64_HEX");
        let mut out = Vec::new();
        validate_manifest(&m, &mut out);
        assert!(out
            .iter()
            .any(|p| p.path == "manifest.json.files[0].sha256"));
    }
}

//! ⭐ REQ-159 · **构建包（`.ontokbundle`）的打包与自检**。
//!
//! 契约：`BA/demands/REQ-159/demand.md` §三（包格式）/ §十一（构建端清单）/
//! §十一-4（会被拒的 8 种），schema：`BA/demands/REQ-159/manifest.schema.json`。
//!
//! # 两条命令，一个模块
//!
//! | 命令 | 入口 | 连库？ | 发网络？ | 调模型？ |
//! | --- | --- | --- | --- | --- |
//! | `bundle pack` | [`pack::assemble`]（纯函数）+ `cli.rs` 边缘取快照 | **是**（只读元数据库） | 否 | 否 |
//! | `bundle verify` | [`verify::verify`]（纯函数） | **否** | 否 | 否 |
//!
//! `pack::assemble` 与 `verify::verify` 的**入参是已经读好的字节 / 结构**，
//! 一个 `sqlx` 都不碰 —— 理由很实在：包的自检必须在没有库的机器上跑
//! （也必须在 CI 里跑），而「自检失败」与「连不上库」是两件毫不相干的事。
//! 连库只发生在 [`crate::cli`] 的边缘一处（照抄 `relation_verbs_check`）。
//!
//! # 为什么 `ontology/snapshot.json` 必须是 `Snapshot` 的**原文**
//!
//! ⚠️ 这是本需求**最容易踩的坑**，写在这里而不是只写在派单里：
//! `ontology show` / `--model-view` 打的是**视图**，而 `LinkTypeView` 只有 5 个字段
//! （`key` / `from_type` / `to_type` / `cardinality` / `semantic`）——
//! **没有 `evidence`、没有 `table` / `column`、没有 `joinVerified`**。
//! 拿视图当包内容，服务端入库后「这条关系凭什么成立」的证据会**静默消失**，
//! 而且没有任何一个字节会报错。
//!
//! ⇒ 本模块只接受 [`crate::domain::snapshot::Snapshot`] 结构体，
//! 由 [`pack::assemble`] 内部做 `serde_json::to_vec`，**中间不许经过任何
//! view / filter / projection**。`verify` 的第 9 条检查
//! （[`code::EVIDENCE_MISSING`]）就是从**另一头**钉这件事：
//! 包里的 `link_types[*]` 必须带 `evidence` 键（哪怕空数组）。
//!
//! # 为什么手写 store-only zip（而不是加 `zip` / `flate2`）
//!
//! `Cargo.toml` 顶部写着「⛔ 不新增 crate」。包里的内容全是 JSON，
//! 压缩率对人没有价值，但**确定性**有价值：store（不压缩）意味着
//! 「磁盘上的字节 = 内存里的字节」，于是「同一输入 + 同一 `--built-at`
//! ⇒ 两次打包逐字节相同」可以直接断言。理由与格式细节见 [`zip`] 模块头。
//!
//! # 拒绝码（8 个）
//!
//! 码是**契约词汇**，不是本地口味：服务端（REQ-159 P1a）用同一批码。
//! 谁判得出来，见 `detected_by` 一列 —— 有的码**只能**服务端判，
//! 构建端不该假装自己能判（那样只会给出假的「通过」）。
//!
//! | # | 码 | 判据 | 谁判 |
//! | --- | --- | --- | --- |
//! | 1 | [`code::MANIFEST_INVALID`] | manifest 缺字段 / 类型不符 / 未知字段 / `db.path` 不在包内 | 构建端 + 服务端 |
//! | 2 | [`code::FILES_MISMATCH`] | `files[]` 与实际文件**集合不等**（多、少都拒） | 构建端 + 服务端 |
//! | 3 | [`code::SHA256_MISMATCH`] | 某文件 sha256 / `bytes` 不符，或容器 CRC32 不符 | 构建端 + 服务端 |
//! | 4 | [`code::PROJECT_MISMATCH`] | `project` 与**接收项目**不同名 | ⛔ **只有服务端**（构建端不知道收件人） |
//! | 5 | [`code::VERSION_UNSUPPORTED`] | `bundle_version` 不认识（只认 `"1.0"`） | 构建端 + 服务端 |
//! | 6 | [`code::PATH_ILLEGAL`] | 绝对路径 / `..` / 反斜杠 / 字符集越界 | 构建端 + 服务端 |
//! | 7 | [`code::DSN_MISSING`] | `requires_dsn=true` 但服务端没配到可用 DSN | ⛔ **只有服务端**（构建端永远不该有 DSN） |
//! | 8 | [`code::TOO_LARGE`] | 包体积超上限（默认 100 MB） | 构建端（提前避开）+ 服务端 |
//!
//! # 容器码（第 9 个，服务端同名）
//!
//! [`code::UNSUPPORTED_COMPRESSION`] —— entry 的**压缩方法不是 0（store）**。
//! 这是「容器不被支持」，⛔ 不是 [`code::MANIFEST_INVALID`]：manifest 一个字节都还没读，
//! 报 `manifest.invalid` 只会让人去改 manifest（改一百遍也没用）。
//! 服务端（join）对同一场景用**同名码**，两边口径一致。
//!
//! 另有**三条构建端补充**的码（服务端也有它们的对应物）：
//! [`code::EVIDENCE_MISSING`]（快照里的关系证据被视图吞了，**拒绝**）、
//! [`code::CREDENTIAL_SUSPECT`]（**警告级**：v1 只告警不拒，见 demand §三-5）与
//! [`code::MANIFEST_NOT_FIRST`]（**警告级**：manifest 不是第一个 entry，
//! 只提示不改判 —— 契约没把它写成硬拒绝条件）。
//!
//! # 确定性
//!
//! 同一份输入 + 同一个 `--built-at` ⇒ **两次 `pack` 输出逐字节相同**：
//!
//! - zip 里不留时间戳（固定 DOS date `0x0021` / time `0x0000`）、不写 extra / comment、
//!   不设外部属性（见 [`zip`]）；
//! - `files[]` 与包内顺序都按 path **升序**（`manifest.json` 永远是第一个 entry）；
//! - manifest 的键序由 `serde_json` 的 `BTreeMap` 决定（不依赖插入顺序）。
//!
//! `tests/test_req159_bundle.rs` 的 `pack_is_bytewise_deterministic` 钉这一条。

pub mod pack;
pub mod verify;
pub mod zip;

pub use pack::{assemble, PackError, PackInput};
pub use verify::{verify, verify_with_limit, Report};
pub use zip::Entry;

use serde::Serialize;

// ───────────────────────── 契约常量（唯一事实源） ─────────────────────────

/// `bundle_version` —— 本工具**唯一**会写的包格式版本。
pub const BUNDLE_VERSION: &str = "1.0";

/// 服务端认识的包格式版本（其余一律 [`code::VERSION_UNSUPPORTED`]）。
pub const SUPPORTED_BUNDLE_VERSIONS: &[&str] = &[BUNDLE_VERSION];

/// 清单在包内的路径（**永远是第一个 entry**）。
pub const MANIFEST_PATH: &str = "manifest.json";
/// 快照在包内的路径（`Snapshot` 原文，⛔ 不是视图）。
pub const SNAPSHOT_PATH: &str = "ontology/snapshot.json";
/// 访问映射在包内的**规范路径**（`--am` 只有一个时就用它）。
pub const AM_PATH: &str = "am/access-mapping.json";
/// 源库描述在包内的路径。
pub const DB_PATH: &str = "db/schema.json";
/// 声明（policy / tenancy / exclusions / recipe）在包内的目录。
pub const DECLARATIONS_DIR: &str = "declarations";
/// `builder.name` 的取值。
pub const BUILDER_NAME: &str = "ontok-builder";
/// 包体积上限（默认 100 MB）—— 与 demand §十一-4 第 8 条一致。
pub const MAX_BUNDLE_BYTES: u64 = 100 * 1024 * 1024;

// ───────────────────────── 拒绝码 ─────────────────────────

/// 8 个拒绝码 + 2 个构建端补充码（**只在这里写字符串**）。
///
/// ⛔ 别在别处再拼一遍字面量：服务端（REQ-159 P1a）与前端都要按码分支，
/// 拼错一个字母就是一个没人认得的错误码。
pub mod code {
    /// 1 · manifest 缺字段 / 类型不符 / 未知字段。
    pub const MANIFEST_INVALID: &str = "bundle.manifest.invalid";
    /// 2 · `files[]` 与实际文件集合不等。
    pub const FILES_MISMATCH: &str = "bundle.files.mismatch";
    /// 3 · sha256 / `bytes` 不符，或容器 CRC32 不符。
    pub const SHA256_MISMATCH: &str = "bundle.sha256.mismatch";
    /// 4 · `project` 与接收项目不同名 —— ⛔ 只有服务端判得出来。
    pub const PROJECT_MISMATCH: &str = "bundle.project.mismatch";
    /// 5 · `bundle_version` 不认识。
    pub const VERSION_UNSUPPORTED: &str = "bundle.version.unsupported";
    /// 6 · 路径非法（绝对 / `..` / 反斜杠 / 字符集）。
    pub const PATH_ILLEGAL: &str = "bundle.path.illegal";
    /// 7 · `requires_dsn=true` 但没配到可用 DSN —— ⛔ 只有服务端判得出来。
    pub const DSN_MISSING: &str = "db.dsn.missing";
    /// 8 · 包体积超上限。
    pub const TOO_LARGE: &str = "bundle.too_large";

    /// 9 · **容器的压缩方法不是 store（method 0）**。
    ///
    /// store-only 是**契约**（demand §三：包内是原样字节，`bundle pack` 恒写 method 0），
    /// 不是「本工具的偏好」。第三方重压成 deflate 的包 ⇒ 拒，但码要说清是
    /// **容器不被支持**，而不是 [`MANIFEST_INVALID`]（那是「目录本身不合规」，
    /// 一个字节都没读进去，混用会让下游去改 manifest —— 改一百遍也没用）。
    ///
    /// ⚠️ 服务端（join）在同一场景用**同名码**，两边口径一致；
    /// 别在服务端改叫 `bundle.zip.method` 之类的名字。
    pub const UNSUPPORTED_COMPRESSION: &str = "bundle.unsupported.compression";

    /// 10（构建端补充）· 快照的关系证据缺失 —— 视图当快照用的那个坑。
    pub const EVIDENCE_MISSING: &str = "bundle.snapshot.evidence_missing";
    /// 11（构建端补充）· 包内疑似凭据 —— **警告级**，v1 只告警不拒。
    pub const CREDENTIAL_SUSPECT: &str = "bundle.credential.suspect";
    /// 12（构建端补充）· `manifest.json` 不是第一个 entry —— **警告级**。
    ///
    /// 契约只把「manifest 第一个」写成**打包端**的产出规范，**没**把它列为
    /// 服务端的硬拒绝条件 ⇒ 这里只提示。⛔ 别把它升成拒绝：那会因为一条
    /// 排序偏好去拒掉别的打包器产出的、语义完全正确的包。
    pub const MANIFEST_NOT_FIRST: &str = "bundle.manifest.not_first";

    /// 全部码（`Report` 的 `summary.codes` 与测试对账用）。
    pub const ALL: &[&str] = &[
        MANIFEST_INVALID,
        FILES_MISMATCH,
        SHA256_MISMATCH,
        PROJECT_MISMATCH,
        VERSION_UNSUPPORTED,
        PATH_ILLEGAL,
        DSN_MISSING,
        TOO_LARGE,
        UNSUPPORTED_COMPRESSION,
        EVIDENCE_MISSING,
        CREDENTIAL_SUSPECT,
        MANIFEST_NOT_FIRST,
    ];
}

/// 包内路径判据的**唯一**入口：`zip::illegal_path_reason` + schema pattern 里的 `(?!.*\.\.)`。
///
/// ⚠️ 为什么必须是**一个**函数（QA F-04 的根因）：
/// `manifest.schema.json` 的 `files[].path.pattern` 是
/// `^(?!/)(?!.*\.\.)[A-Za-z0-9._/-]+$` —— 它连 `v1..2.json` 这种**没有 `..` 段**
/// 的连续双点也拒；而「`..` 段」判据（[`zip::illegal_path_reason`]）放行它。
/// 若打包端只按「段」判、自检端按 schema 字面判，就会出现
/// **打得出来、自己必拒**的包（`v1..2.json` 在 Windows / Linux 上都是合法文件名）。
/// ⇒ 两端调用同一个函数；命中一律在**打包阶段**按参数错误拒（`exit 2`），
/// ⛔ 不许等打完再靠自检失败把它指成「打包端的 bug」。
pub fn path_problem(p: &str) -> Option<String> {
    if let Some(why) = zip::illegal_path_reason(p) {
        return Some(why.to_string());
    }
    if p.contains("..") {
        return Some("含 `..`（schema 的 path pattern `(?!.*\\.\\.)` 不许，防穿越）".to_string());
    }
    None
}

// ───────────────────────── 报告里的一个问题 ─────────────────────────

/// 一条**可执行**的判定：码 + 位置 + 发生了什么 + 怎么改。
///
/// 「怎么改」（`hint`）不是客套话：`bundle verify` 的定位就是让 FDE 在
/// 上传之前自己改完，而不是等服务端回一个码再去猜。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Problem {
    /// 拒绝码（[`code`] 里的常量之一）。
    pub code: &'static str,
    /// 出问题的包内路径 / 字段路径（`manifest.json`、`components.db.dialect` …）。
    pub path: String,
    /// 具体结论（点名到字段 / 文件 / 数字）。
    pub message: String,
    /// 怎么改。
    pub hint: String,
}

impl Problem {
    pub fn new(code: &'static str, path: impl Into<String>, message: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            code,
            path: path.into(),
            message: message.into(),
            hint: hint.into(),
        }
    }

    /// 人话一行（stderr 那一份）。
    pub fn one_line(&self) -> String {
        format!("{} · {} · {}（{}）", self.code, self.path, self.message, self.hint)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 码表不许有重复 —— 有重复就说明「哪个码管哪件事」已经说不清了。
    #[test]
    fn all_codes_are_distinct_and_namespaced() {
        let mut seen = std::collections::BTreeSet::new();
        for c in code::ALL {
            assert!(seen.insert(*c), "拒绝码重复：{c}");
            assert!(
                c.starts_with("bundle.") || c.starts_with("db."),
                "拒绝码 `{c}` 不在契约的命名空间里"
            );
        }
        // 8 个契约码必须都在表里（少一个就是下游要对着不存在的东西分支）。
        for must in [
            code::MANIFEST_INVALID,
            code::FILES_MISMATCH,
            code::SHA256_MISMATCH,
            code::PROJECT_MISMATCH,
            code::VERSION_UNSUPPORTED,
            code::PATH_ILLEGAL,
            code::DSN_MISSING,
            code::TOO_LARGE,
        ] {
            assert!(seen.contains(must), "契约的 8 个码少了 `{must}`");
        }
    }

    #[test]
    fn supported_versions_are_exactly_one_point_zero() {
        assert_eq!(SUPPORTED_BUNDLE_VERSIONS, &["1.0"]);
        assert_eq!(BUNDLE_VERSION, "1.0");
    }
}

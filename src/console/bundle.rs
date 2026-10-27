//! ⭐ REQ-159 · **构建包（bundle）** —— 格式校验 + manifest。
//!
//! 契约：`BA/demands/REQ-159/demand.md` §三（包格式）/ §十一-4（8 个错误码）/
//! `BA/demands/REQ-159/manifest.schema.json`（manifest 的 JSON Schema）。
//!
//! # 这一层只做一件事，且**按契约给的那个顺序**
//!
//! ```text
//! 体积（100 MB） → zip 合法性 → 路径安全 → manifest 按 schema → files[] 逐个 sha256
//!   → project 与接收项目同名 → 组件计数自洽
//! ```
//!
//! ⛔ 这一层**不写库、不建版本、不碰快照五行**。它的产物是一个
//! [`VerifiedBundle`]（整包字节 + 解析好的 manifest + 已反序列化的快照），
//! 落库与解析入库在 [`crate::console::provision`]。
//!
//! # 8 个错误码（`demand.md` §十一-4）就在这个文件里收口
//!
//! | 码 | HTTP | 什么时候 |
//! | --- | --- | --- |
//! | `bundle.too_large` | 413 | 文件超过 100 MB |
//! | `bundle.path.illegal` | 400 | 路径含 `..` / 绝对路径 / 符号链接 / 反斜杠 |
//! | `bundle.manifest.invalid` | 400 | manifest 缺字段 / 类型不符 / 计数不自洽 |
//! | `bundle.version.unsupported` | 400 | `bundle_version` 不是 `"1.0"` |
//! | `bundle.files.mismatch` | 400 | `files[]` 与包内实际文件集合不等 / `bytes` 不符 / **列了 `manifest.json` 自己** |
//! | `bundle.sha256.mismatch` | 400 | 任一文件（或整包）的 sha256 不符 |
//! | `bundle.project.mismatch` | 409 | 包里的 `project` ≠ 接收项目（防串包） |
//! | `db.dsn.missing` | 422 | `requires_dsn=true` 但 DSN 没配 / 没测通（**在 import 那一层判**） |
//!
//! 另外两个码属于**请求形状**，不在这 8 条之内（也不是「包内容不合规」）：
//!
//! - `bundle.upload.invalid`（415）：上传体被封装过（`multipart/form-data`）——
//!   ⭐ 契约更正（2026-10-27）：本端点收的是**包的原始字节**
//!   （`application/octet-stream`），不是 multipart 字段；
//! - `bundle.parse.failed`（422，`demand.md` §五-6 明写）：包里的 `snapshot.json`
//!   反序列化不成 [`Snapshot`] —— **import 时才可能发生**（上传时那次是校验）。
//!
//! # zip：为什么手写一个只读解析器
//!
//! R1「不引新依赖」的落点：`zip` crate 不在依赖树里，`flate2` 也**不是**直接依赖
//! （本单不许新增）。包格式只要求「读几个 entry、取原始字节」，所以这里实现的是
//! **只读子集**：不支持 zip64、不支持加密、不支持多分卷、不支持 deflate ——
//! 遇到就**明确拒绝**（不猜、不半读），而不是「尽力而为」。
//!
//! 契约把容器钉成 **store-only zip**（method 0、无 data descriptor、`manifest.json`
//! 是第一条 entry），构建端也只产出 method 0。于是本读取器**只认 method 0**：
//! 别的压缩方法一律 `bundle.unsupported.compression`（见读包器里的 `match method`）。
//!
//! ⛔ 不校验 entry 的 CRC32：契约的完整性判据是**每个文件的 sha256**
//! （比 CRC32 强得多），再校一遍 CRC 只是多一处「两个判据打架时听谁的」。
//!
//! # 已知边界（诚实登记）
//!
//! - `components.am` 的计数键名是 `entities` / `sources`（⛔ **不是** `tenants` ——
//!   契约示例里那个 `"tenants": 3` 已在 2026-10-27 作废：构建端产出的 AM
//!   （`access::Mapping`）里没有 tenant 概念）。这两个计数与包内 AM JSON 的
//!   `entities` / `sources` 数组长度对账；`components.ontology` 的三计数
//!   （entity_types / link_types / properties）与快照**直接数出来**的对账。
//! - 解压后的总量有 256 MB 上限（zip 炸弹闸），包内 entry 数上限 4096。

use std::collections::BTreeMap;

use axum::http::StatusCode;
use serde_json::{Map, Value};

use crate::query::error::ApiError;

// ══════════════════════════════════════════════════════════════
// 常量
// ══════════════════════════════════════════════════════════════

/// 包体积上限（`demand.md` §十一-4 第 8 条：默认 100 MB）。
pub const MAX_BUNDLE_BYTES: usize = 100 * 1024 * 1024;

/// 解压后总字节上限 —— zip 炸弹闸。包体积上限是 100 MB，解压后给 2.5× 余量。
pub const MAX_UNCOMPRESSED_BYTES: usize = 256 * 1024 * 1024;

/// 包内 entry 数上限。真实包是「本体 + AM + db 描述 + 几份声明」≈ 十个。
pub const MAX_ENTRIES: usize = 4096;

/// manifest 在包内的**固定**路径（清单是包的目录，不能由别处指过来）。
pub const MANIFEST_PATH: &str = "manifest.json";

/// 服务端认识的包格式版本。⛔ 不认识的**拒绝**，不尝试兼容（schema 的 `const`）。
pub const SUPPORTED_BUNDLE_VERSIONS: &[&str] = &["1.0"];

// ══════════════════════════════════════════════════════════════
// 错误
// ══════════════════════════════════════════════════════════════

/// 一次包校验失败：**稳定错误码** + 人话 + 下一步。
///
/// 与 [`crate::console::OpError`] 同一条设计（`code` 是 `&'static str`：
/// 错误码是端点语义的常量，不是运行时字符串），但多带一个 `status`：
/// 这 8 条各自的 HTTP 语义不同（413 / 409 / 422 / 400）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
    pub hint: String,
}

impl BundleError {
    fn new(
        status: StatusCode,
        code: &'static str,
        message: impl Into<String>,
        hint: impl Into<String>,
    ) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            hint: hint.into(),
        }
    }

    pub fn into_api(self) -> ApiError {
        ApiError::with_code(self.status, self.code, self.message, self.hint)
    }
}

impl std::fmt::Display for BundleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}（{}）", self.message, self.code)
    }
}

impl std::error::Error for BundleError {}

fn too_large(actual: usize) -> BundleError {
    BundleError::new(
        StatusCode::PAYLOAD_TOO_LARGE,
        "bundle.too_large",
        format!(
            "包的大小是 {} 字节（{:.1} MB），上限是 {} 字节（100 MB）。",
            actual,
            actual as f64 / (1024.0 * 1024.0),
            MAX_BUNDLE_BYTES
        ),
        "包是「本体 + AM + 源库描述」，量级应是百 KB 级。超限通常意味着把数据行 / \
         二进制附件打进去了 —— 检查 `files[]` 里最大的那几项；⛔ 数据不进包。",
    )
}

fn manifest_invalid(what: impl Into<String>) -> BundleError {
    BundleError::new(
        StatusCode::BAD_REQUEST,
        "bundle.manifest.invalid",
        what,
        "manifest 的形状以 `BA/demands/REQ-159/manifest.schema.json` 为准：\
         缺字段 / 类型不符 / 多出未声明的字段一律**拒绝**（服务端不猜默认值）。",
    )
}

fn version_unsupported(got: &str) -> BundleError {
    BundleError::new(
        StatusCode::BAD_REQUEST,
        "bundle.version.unsupported",
        format!(
            "包的 bundle_version = {got:?}，本服务端只认 {:?}。",
            SUPPORTED_BUNDLE_VERSIONS
        ),
        "包格式版本不匹配：用与服务端同代的 `ontok-builder bundle pack` 重新打包。\
         ⛔ 服务端不尝试兼容不认识的版本（schema 里 `bundle_version` 是 const）。",
    )
}

fn path_illegal(path: &str, why: &str) -> BundleError {
    BundleError::new(
        StatusCode::BAD_REQUEST,
        "bundle.path.illegal",
        format!("包内路径 {path:?} 不合法：{why}。"),
        "包内路径必须是**相对路径**（正斜杠分隔），且不含 `..` / 绝对路径 / \
         盘符 / 符号链接 —— 这是路径穿越防护（demand §三-6）。",
    )
}

fn files_mismatch(detail: impl Into<String>) -> BundleError {
    BundleError::new(
        StatusCode::BAD_REQUEST,
        "bundle.files.mismatch",
        detail,
        "`files[]` 必须**覆盖包内每一个文件**（manifest.json 自身除外），集合相等、\
         `bytes` 相符。用 `ontok-builder bundle verify` 自检后重打一次包。",
    )
}

fn sha_mismatch(file: &str, declared: &str, actual: &str) -> BundleError {
    BundleError::new(
        StatusCode::BAD_REQUEST,
        "bundle.sha256.mismatch",
        format!("文件 {file:?} 的 sha256 不符：manifest 声明 {declared}，实际 {actual}。"),
        "包在传输 / 存放过程中被改过，或者打出来的清单不是这份内容的。\
         ⛔ 整包拒（一个文件不符就拒，不做部分导入）。",
    )
}

fn project_mismatch(bundle: &str, target: &str) -> BundleError {
    BundleError::new(
        StatusCode::CONFLICT,
        "bundle.project.mismatch",
        format!("包里的 project = {bundle:?}，而接收项目是 {target:?}。"),
        "一个包只能装一个项目（防串包）。换一个项目上传，或者让构建端用\
         `--project {target}` 重新打包。",
    )
}

fn parse_failed(detail: impl Into<String>) -> BundleError {
    BundleError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        "bundle.parse.failed",
        detail,
        "`ontology/snapshot.json` 必须是 `domain::snapshot::Snapshot` 的 **serde_json 原文**\
         （⛔ 不是 `ontology show` 的视图 JSON：视图没有 evidence / table / column）。",
    )
}

/// 上传体被**封装**过（`multipart/form-data`）时的拒绝。
///
/// ⭐ 契约更正（2026-10-27）：本端点收的是包的**原始字节**
/// （`application/octet-stream`），不是 multipart 里的一个字段。三种写法里选了最直白的：
///
/// - multipart ⇒ 要 axum 的 `multipart` feature ⇒ 拉 `multer`（不在依赖树里，违反 R1）；
/// - 手写 multipart 解析器 ⇒ 能跑，但多一层「多一个 CRLF 就 sha256 不符」的失败面；
/// - **原始字节** ⇒ 前端发 `File` 的 ArrayBuffer、curl 用 `--data-binary`，两处都最短。
pub fn reject_encapsulated(content_type: &str) -> BundleError {
    BundleError::new(
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "bundle.upload.invalid",
        format!(
            "Content-Type = {content_type:?}：本端点收的是包的**原始字节**，不是 multipart 字段。"
        ),
        "改法：`curl -X POST --data-binary @x.ontokbundle \
         -H \"Content-Type: application/octet-stream\" -H \"Authorization: Bearer <admin>\" \
         …/api/projects/<p>/bundles`；前端直接把 `File` 的 ArrayBuffer 发出去即可。",
    )
}

// ══════════════════════════════════════════════════════════════
// 状态机（唯一权威）
// ══════════════════════════════════════════════════════════════

/// 一个包的状态（`demand.md` §四 状态机）。
///
/// ⚠️ 库里 `project_bundles.state` 存的是 [`State::as_str`] 的字面值，
/// **不加 CHECK 约束**：状态机的唯一权威是这个枚举，加一份 CHECK 就是两份定义。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// 上传并校验通过（还没配 DSN / 还没解析）。
    Uploaded,
    /// 包要 DSN，但还没配 / 没测通。
    DbPending,
    /// 解析入库成功（**未冻结**）。
    Imported,
    /// 已人工激活（冻结 + 生效指针）。
    Activated,
    /// 解析失败（事务已回滚）。
    Failed,
}

impl State {
    pub const fn as_str(self) -> &'static str {
        match self {
            State::Uploaded => "uploaded",
            State::DbPending => "db_pending",
            State::Imported => "imported",
            State::Activated => "activated",
            State::Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "uploaded" => Some(State::Uploaded),
            "db_pending" => Some(State::DbPending),
            "imported" => Some(State::Imported),
            "activated" => Some(State::Activated),
            "failed" => Some(State::Failed),
            _ => None,
        }
    }
}

// ══════════════════════════════════════════════════════════════
// 最小 zip 读取器（只读子集）
// ══════════════════════════════════════════════════════════════

/// 包内一个 entry（已解压）。
#[derive(Debug, Clone)]
pub struct ZipEntry {
    /// 包内相对路径（正斜杠）。目录 entry 以 `/` 结尾。
    pub name: String,
    pub data: Vec<u8>,
    pub is_dir: bool,
    /// unix 权限位（`external_attr >> 16`）；非 unix 打包工具会给 0。
    pub unix_mode: Option<u32>,
}

const EOCD_SIG: u32 = 0x0605_4b50;
const CD_SIG: u32 = 0x0201_4b50;
const LFH_SIG: u32 = 0x0403_4b50;

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    let s = b.get(at..at + 2)?;
    Some(u16::from_le_bytes([s[0], s[1]]))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at + 4)?;
    Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

/// 读整个 zip（**路径安全在解压之前**逐条判）。
pub fn read_zip(bytes: &[u8]) -> Result<Vec<ZipEntry>, BundleError> {
    let not_zip = |why: &str| {
        manifest_invalid(format!(
            "包不是可读的 zip（{why}）—— 扩展名应是 `.ontokbundle`，内容是一个普通 zip。"
        ))
    };

    // ── EOCD：从尾部往前找（注释最长 65535 字节）──────────────
    let tail_start = bytes.len().saturating_sub(22 + 65_535);
    let mut eocd = None;
    let mut at = bytes.len().saturating_sub(22);
    loop {
        if at < tail_start {
            break;
        }
        if u32_at(bytes, at) == Some(EOCD_SIG) {
            eocd = Some(at);
            break;
        }
        if at == 0 {
            break;
        }
        at -= 1;
    }
    let eocd = eocd.ok_or_else(|| not_zip("找不到中央目录结尾（EOCD）"))?;

    let entries_total = u16_at(bytes, eocd + 10).ok_or_else(|| not_zip("EOCD 截断"))? as usize;
    let cd_size = u32_at(bytes, eocd + 12).ok_or_else(|| not_zip("EOCD 截断"))? as usize;
    let cd_offset = u32_at(bytes, eocd + 16).ok_or_else(|| not_zip("EOCD 截断"))? as usize;
    let disk_no = u16_at(bytes, eocd + 4).unwrap_or(0);
    let cd_disk = u16_at(bytes, eocd + 6).unwrap_or(0);

    // zip64 / 多分卷：明确拒绝，不半读。
    if entries_total == 0xFFFF || cd_size == 0xFFFF_FFFF || cd_offset == 0xFFFF_FFFF {
        return Err(not_zip(
            "zip64 不受支持（条目 >65534 或偏移 >4 GB）；普通包用不到 zip64",
        ));
    }
    if disk_no != 0 || cd_disk != 0 {
        return Err(not_zip("多分卷 zip 不受支持"));
    }
    if entries_total > MAX_ENTRIES {
        return Err(not_zip(&format!(
            "包内 entry 数 {} 超过上限 {MAX_ENTRIES}",
            entries_total
        )));
    }
    if cd_offset.checked_add(cd_size).map(|e| e > bytes.len()) != Some(false) {
        return Err(not_zip("中央目录越界"));
    }

    // ── 中央目录：逐条 ───────────────────────────────────────
    let mut out: Vec<ZipEntry> = Vec::with_capacity(entries_total);
    let mut p = cd_offset;
    let mut total_uncompressed = 0usize;
    for i in 0..entries_total {
        if u32_at(bytes, p) != Some(CD_SIG) {
            return Err(not_zip(&format!("第 {} 条中央目录记录签名不符", i + 1)));
        }
        let flags = u16_at(bytes, p + 8).ok_or_else(|| not_zip("中央目录截断"))?;
        let method = u16_at(bytes, p + 10).ok_or_else(|| not_zip("中央目录截断"))?;
        let comp_size = u32_at(bytes, p + 20).ok_or_else(|| not_zip("中央目录截断"))? as usize;
        let uncomp_size = u32_at(bytes, p + 24).ok_or_else(|| not_zip("中央目录截断"))? as usize;
        let name_len = u16_at(bytes, p + 28).ok_or_else(|| not_zip("中央目录截断"))? as usize;
        let extra_len = u16_at(bytes, p + 30).ok_or_else(|| not_zip("中央目录截断"))? as usize;
        let comment_len = u16_at(bytes, p + 32).ok_or_else(|| not_zip("中央目录截断"))? as usize;
        let ext_attr = u32_at(bytes, p + 38).ok_or_else(|| not_zip("中央目录截断"))?;
        let local_offset = u32_at(bytes, p + 42).ok_or_else(|| not_zip("中央目录截断"))? as usize;

        let name_bytes = bytes
            .get(p + 46..p + 46 + name_len)
            .ok_or_else(|| not_zip("中央目录里的文件名越界"))?;
        let name = String::from_utf8(name_bytes.to_vec())
            .map_err(|_| not_zip("包内文件名不是 UTF-8（本服务端只认 UTF-8 路径）"))?;

        // ① 路径安全 —— 在读**任何字节**之前判。
        check_path(&name)?;

        if ext_attr >> 16 == 0xA000 || (ext_attr >> 16) & 0xF000 == 0xA000 {
            return Err(path_illegal(&name, "是符号链接"));
        }
        if flags & 0x0001 != 0 {
            return Err(not_zip("包是加密 zip（v1 不支持加密包）"));
        }

        let is_dir = name.ends_with('/');
        let unix_mode = Some(ext_attr >> 16).filter(|m| *m != 0);

        let data = if is_dir {
            Vec::new()
        } else {
            // ② 本地头 → 数据区
            if u32_at(bytes, local_offset) != Some(LFH_SIG) {
                return Err(not_zip(&format!("{name:?} 的本地头签名不符")));
            }
            let l_name_len =
                u16_at(bytes, local_offset + 26).ok_or_else(|| not_zip("本地头截断"))? as usize;
            let l_extra_len =
                u16_at(bytes, local_offset + 28).ok_or_else(|| not_zip("本地头截断"))? as usize;
            let start = local_offset + 30 + l_name_len + l_extra_len;
            let raw = bytes
                .get(start..start + comp_size)
                .ok_or_else(|| not_zip(&format!("{name:?} 的数据区越界")))?;

            total_uncompressed = total_uncompressed.saturating_add(uncomp_size);
            if total_uncompressed > MAX_UNCOMPRESSED_BYTES {
                return Err(too_large(total_uncompressed).with_hint(
                    "解压后的总量超过 256 MB —— 这是 zip 炸弹闸（包体积上限 100 MB，\
                     解压后给 2.5× 余量）；真包是百 KB 级。",
                ));
            }

            match method {
                0 => {
                    if raw.len() != uncomp_size {
                        return Err(not_zip(&format!(
                            "{name:?} 是 stored，但声明长度 {uncomp_size} 与数据长度 {} 不符",
                            raw.len()
                        )));
                    }
                    raw.to_vec()
                }
                // ⛔ 契约只允许 store-only（method = 0），构建端也只产出 method = 0。
                // 于是这里**明确拒绝**而不是「尽力解压」：deflate 支持曾经靠一个
                // 直接依赖（`flate2`）实现，而本单 R1 不许新增 crate；契约收口成
                // store-only 之后，那条分支连理由都没了（见 `Cargo.toml` 的登记）。
                other => {
                    return Err(BundleError::new(
                        StatusCode::BAD_REQUEST,
                        "bundle.unsupported.compression",
                        format!(
                            "{name:?} 的压缩方法是 {other}，而契约只允许 store-only\
                             （method = 0，即不压缩），收到 method = {other}。"
                        ),
                        "包必须是 store-only zip（method = 0）：用 `ontok-builder bundle pack` \
                         打出来的包就是这样，⛔ 不要用会做 deflate 的通用打包器（`zip -9` 一类）\
                         重打一次 —— 那样即使内容一样，契约也拒。服务端**不解压**不认识的方法。",
                    ));
                }
            }
        };

        out.push(ZipEntry {
            name,
            data,
            is_dir,
            unix_mode,
        });
        p += 46 + name_len + extra_len + comment_len;
    }
    Ok(out)
}

impl BundleError {
    fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = hint.into();
        self
    }
}

/// 包内路径安全（`demand.md` §三-6）。
///
/// 判据是**逐段**的，不是「字符串里含 `..`」那种松判：
/// - 必须是相对路径（不许以 `/` 或 `\` 开头、不许有盘符 `C:`）；
/// - 只许正斜杠（反斜杠在 Windows 上是分隔符，放进来就是双重解释）；
/// - 不许空段 / `.` / `..`；
/// - 不许 NUL / 控制字符；
/// - 单段长度与总长度有上限（免得造出病态路径）。
pub fn check_path(name: &str) -> Result<(), BundleError> {
    if name.is_empty() {
        return Err(path_illegal(name, "空路径"));
    }
    if name.len() > 1024 {
        return Err(path_illegal(name, "路径过长"));
    }
    if name.starts_with('/') || name.starts_with('\\') {
        return Err(path_illegal(name, "绝对路径"));
    }
    if name.contains('\\') {
        return Err(path_illegal(name, "含反斜杠（包内路径只认正斜杠）"));
    }
    if name.contains(':') {
        return Err(path_illegal(name, "含冒号（盘符 / ADS）"));
    }
    if name.chars().any(|c| c == '\0' || (c.is_control() && c != '\t')) {
        return Err(path_illegal(name, "含 NUL / 控制字符"));
    }
    for seg in name.split('/') {
        if seg == ".." {
            return Err(path_illegal(name, "含 `..` 段（路径穿越）"));
        }
        if seg == "." {
            return Err(path_illegal(name, "含 `.` 段"));
        }
        // 尾部的空段只允许出现在目录 entry（`dir/`）。
        if seg.is_empty() && !name.ends_with('/') {
            return Err(path_illegal(name, "含空路径段"));
        }
    }
    Ok(())
}

// ══════════════════════════════════════════════════════════════
// manifest
// ══════════════════════════════════════════════════════════════

/// manifest 里的一个组件（`components.ontology` / `.am` / `.db`）。
#[derive(Debug, Clone)]
pub struct Component {
    pub path: String,
    pub sha256: String,
    /// 只有 `db` 组件有；其余恒 `false`。
    pub requires_dsn: bool,
    pub dialect: Option<String>,
    /// 声明的计数（`entity_types` / `link_types` / `properties` / `tenants` …）原样保留。
    pub counts: BTreeMap<String, i64>,
    /// 组件原始 JSON（回显用）。
    pub raw: Value,
}

/// `files[]` 的一行。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ManifestFile {
    pub path: String,
    pub sha256: String,
    pub bytes: i64,
}

/// 解析 + 校验通过的 manifest。
#[derive(Debug, Clone)]
pub struct Manifest {
    pub bundle_version: String,
    pub project: String,
    pub ontology_seq: i64,
    pub built_at: String,
    pub builder_name: String,
    pub builder_version: String,
    pub builder_host: Option<String>,
    pub ontology: Component,
    pub am: Option<Component>,
    pub db: Option<Component>,
    pub files: Vec<ManifestFile>,
    /// 原文（落库 `project_bundles.manifest`，也是 `GET` 回显的来源）。
    pub raw: Value,
}

impl Manifest {
    /// 这个包要不要 DSN（`components.db.requires_dsn`，demand §七-6 的唯一判据）。
    pub fn requires_dsn(&self) -> bool {
        self.db.as_ref().map(|d| d.requires_dsn).unwrap_or(false)
    }

    pub fn dialect(&self) -> Option<&str> {
        self.db.as_ref().and_then(|d| d.dialect.as_deref())
    }
}

fn as_obj<'a>(v: &'a Value, path: &str) -> Result<&'a Map<String, Value>, BundleError> {
    v.as_object()
        .ok_or_else(|| manifest_invalid(format!("`{path}` 必须是对象，收到 {}", kind_of(v))))
}

fn kind_of(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "布尔",
        Value::Number(_) => "数字",
        Value::String(_) => "字符串",
        Value::Array(_) => "数组",
        Value::Object(_) => "对象",
    }
}

fn need_str<'a>(obj: &'a Map<String, Value>, key: &str, path: &str) -> Result<&'a str, BundleError> {
    match obj.get(key) {
        None => Err(manifest_invalid(format!("`{path}.{key}` 缺失"))),
        Some(Value::String(s)) if !s.trim().is_empty() => Ok(s.as_str()),
        Some(other) => Err(manifest_invalid(format!(
            "`{path}.{key}` 必须是非空字符串，收到 {}",
            kind_of(other)
        ))),
    }
}

fn opt_str(obj: &Map<String, Value>, key: &str, path: &str) -> Result<Option<String>, BundleError> {
    match obj.get(key) {
        None => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(other) => Err(manifest_invalid(format!(
            "`{path}.{key}` 必须是字符串，收到 {}",
            kind_of(other)
        ))),
    }
}

fn need_int(obj: &Map<String, Value>, key: &str, path: &str) -> Result<i64, BundleError> {
    match obj.get(key) {
        None => Err(manifest_invalid(format!("`{path}.{key}` 缺失"))),
        Some(Value::Number(n)) => n.as_i64().ok_or_else(|| {
            manifest_invalid(format!("`{path}.{key}` 必须是整数，收到 {n}"))
        }),
        Some(other) => Err(manifest_invalid(format!(
            "`{path}.{key}` 必须是整数，收到 {}",
            kind_of(other)
        ))),
    }
}

fn need_bool(obj: &Map<String, Value>, key: &str, path: &str) -> Result<bool, BundleError> {
    match obj.get(key) {
        None => Err(manifest_invalid(format!("`{path}.{key}` 缺失"))),
        Some(Value::Bool(b)) => Ok(*b),
        Some(other) => Err(manifest_invalid(format!(
            "`{path}.{key}` 必须是布尔，收到 {}",
            kind_of(other)
        ))),
    }
}

/// `additionalProperties: false` 的落点：多出来的字段 ⇒ 点名拒绝。
fn only_keys(obj: &Map<String, Value>, allowed: &[&str], path: &str) -> Result<(), BundleError> {
    let mut extra: Vec<&str> = obj
        .keys()
        .map(String::as_str)
        .filter(|k| !allowed.contains(k))
        .collect();
    if extra.is_empty() {
        return Ok(());
    }
    extra.sort_unstable();
    Err(manifest_invalid(format!(
        "`{path}` 里有未声明的字段：{extra:?}（manifest 不许带 schema 之外的字段）"
    )))
}

/// 64 位小写十六进制（schema 里 `^[0-9a-f]{64}$`）。
fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// 项目名（schema 里 `^[a-z0-9][a-z0-9_-]{1,62}$`）。
///
/// ⭐ 契约更正（2026-10-27）：**允许下划线**。库里既有项目名大量带 `_`
/// （`req129_ac006_22140_1793033086208` 一类），而这条 pattern 同时是
/// `POST /api/projects` 与 `manifest.project` 的**同一条**判据 ⇒ 禁掉 `_`
/// 等于让历史项目既建不了行、也打不了包（真库上已实测复现）。
/// 仍然禁大写 / 空格 / 斜杠 / 点。权威出处：
/// `BA/demands/REQ-159/manifest.schema.json` 的 `project.pattern`（已同步为带 `_` 的版本）。
pub fn valid_project_name(s: &str) -> bool {
    let b = s.as_bytes();
    if !(2..=63).contains(&b.len()) {
        return false;
    }
    let first = b[0];
    if !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
        return false;
    }
    b.iter()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-' || *c == b'_')
}

fn valid_iso8601_date_time(s: &str) -> bool {
    chrono::DateTime::parse_from_rfc3339(s).is_ok()
}

fn parse_component(v: &Value, path: &str, is_db: bool) -> Result<Component, BundleError> {
    let obj = as_obj(v, path)?;
    // `component`（ontology / am）在 schema 里是 `additionalProperties: true`，
    // `dbComponent` 是 `false` —— 判据照抄 schema，不自己加严。
    if is_db {
        only_keys(
            obj,
            &["path", "sha256", "requires_dsn", "dialect"],
            path,
        )?;
    }
    let p = need_str(obj, "path", path)?;
    check_path(p)?;
    let sha = need_str(obj, "sha256", path)?;
    if !is_sha256_hex(sha) {
        return Err(manifest_invalid(format!(
            "`{path}.sha256` 必须是 64 位小写十六进制，收到 {sha:?}"
        )));
    }
    let requires_dsn = if is_db {
        need_bool(obj, "requires_dsn", path)?
    } else {
        false
    };
    let dialect = opt_str(obj, "dialect", path)?;
    if is_db {
        if let Some(d) = dialect.as_deref() {
            let allowed = ["postgresql", "mysql", "sqlserver", "oracle"];
            if !allowed.contains(&d) {
                return Err(manifest_invalid(format!(
                    "`{path}.dialect` = {d:?} 不在 {allowed:?} 之内"
                )));
            }
        }
        // schema 的 allOf/if-then：`requires_dsn=true` ⇒ `dialect` 必填。
        if requires_dsn && dialect.is_none() {
            return Err(manifest_invalid(format!(
                "`{path}.requires_dsn = true` 却没有 `{path}.dialect`（必填）"
            )));
        }
    }
    let mut counts = BTreeMap::new();
    for (k, val) in obj {
        if matches!(k.as_str(), "path" | "sha256" | "requires_dsn" | "dialect") {
            continue;
        }
        if let Some(n) = val.as_i64() {
            counts.insert(k.clone(), n);
        }
    }
    Ok(Component {
        path: p.to_string(),
        sha256: sha.to_string(),
        requires_dsn,
        dialect,
        counts,
        raw: v.clone(),
    })
}

/// manifest 的**逐条** schema 校验（`manifest.schema.json`）。
///
/// 为什么手写而不是引一个 JSON Schema 校验器：R1 不引新依赖，而这份 schema
/// 只有 6 个顶层字段 + 3 个组件 —— 手写的收益是**错误信息带字段路径**
/// （demand §十一-4 第 1 条明写要「报具体字段路径」）。
pub fn parse_manifest(v: &Value) -> Result<Manifest, BundleError> {
    let obj = as_obj(v, "manifest")?;
    only_keys(
        obj,
        &[
            "bundle_version",
            "project",
            "ontology_seq",
            "built_at",
            "builder",
            "components",
            "files",
        ],
        "manifest",
    )?;

    // bundle_version 先判：不认识就是版本问题，不该报成「缺字段」。
    let bv = need_str(obj, "bundle_version", "manifest")?;
    if !SUPPORTED_BUNDLE_VERSIONS.contains(&bv) {
        return Err(version_unsupported(bv));
    }

    let project = need_str(obj, "project", "manifest")?;
    if !valid_project_name(project) {
        return Err(manifest_invalid(format!(
            "`manifest.project` = {project:?} 不符合 `^[a-z0-9][a-z0-9_-]{{1,62}}$`"
        )));
    }
    let ontology_seq = need_int(obj, "ontology_seq", "manifest")?;
    if ontology_seq < 1 {
        return Err(manifest_invalid(format!(
            "`manifest.ontology_seq` 必须 ≥ 1，收到 {ontology_seq}"
        )));
    }
    let built_at = need_str(obj, "built_at", "manifest")?;
    if !valid_iso8601_date_time(built_at) {
        return Err(manifest_invalid(format!(
            "`manifest.built_at` = {built_at:?} 不是 RFC3339 时间（要带 Z 或偏移）"
        )));
    }

    let builder_obj = as_obj(
        obj.get("builder")
            .ok_or_else(|| manifest_invalid("`manifest.builder` 缺失"))?,
        "manifest.builder",
    )?;
    only_keys(builder_obj, &["name", "version", "host"], "manifest.builder")?;
    let builder_name = need_str(builder_obj, "name", "manifest.builder")?.to_string();
    let builder_version = need_str(builder_obj, "version", "manifest.builder")?.to_string();
    let builder_host = opt_str(builder_obj, "host", "manifest.builder")?;

    let comps = as_obj(
        obj.get("components")
            .ok_or_else(|| manifest_invalid("`manifest.components` 缺失"))?,
        "manifest.components",
    )?;
    only_keys(comps, &["ontology", "am", "db"], "manifest.components")?;
    let ontology_v = comps
        .get("ontology")
        .ok_or_else(|| manifest_invalid("`manifest.components.ontology` 缺失（必填）"))?;
    let ontology = parse_component(ontology_v, "manifest.components.ontology", false)?;
    let am = comps
        .get("am")
        .map(|v| parse_component(v, "manifest.components.am", false))
        .transpose()?;
    let db = comps
        .get("db")
        .map(|v| parse_component(v, "manifest.components.db", true))
        .transpose()?;

    let files_v = obj
        .get("files")
        .ok_or_else(|| manifest_invalid("`manifest.files` 缺失"))?;
    let arr = files_v
        .as_array()
        .ok_or_else(|| manifest_invalid("`manifest.files` 必须是数组"))?;
    if arr.is_empty() {
        return Err(manifest_invalid("`manifest.files` 不能是空数组（至少一个文件）"));
    }
    let mut files = Vec::with_capacity(arr.len());
    let mut seen: BTreeMap<&str, ()> = BTreeMap::new();
    for (i, f) in arr.iter().enumerate() {
        let at = format!("manifest.files[{i}]");
        let o = as_obj(f, &at)?;
        only_keys(o, &["path", "sha256", "bytes"], &at)?;
        let path = need_str(o, "path", &at)?;
        check_path(path)?;
        if seen.insert(path, ()).is_some() {
            return Err(manifest_invalid(format!(
                "`{at}.path` = {path:?} 在 `files[]` 里出现了两次"
            )));
        }
        let sha = need_str(o, "sha256", &at)?;
        if !is_sha256_hex(sha) {
            return Err(manifest_invalid(format!(
                "`{at}.sha256` 必须是 64 位小写十六进制，收到 {sha:?}"
            )));
        }
        let bytes = need_int(o, "bytes", &at)?;
        if bytes < 0 {
            return Err(manifest_invalid(format!(
                "`{at}.bytes` 必须 ≥ 0，收到 {bytes}"
            )));
        }
        files.push(ManifestFile {
            path: path.to_string(),
            sha256: sha.to_string(),
            bytes,
        });
    }

    Ok(Manifest {
        bundle_version: bv.to_string(),
        project: project.to_string(),
        ontology_seq,
        built_at: built_at.to_string(),
        builder_name,
        builder_version,
        builder_host,
        ontology,
        am,
        db,
        files,
        raw: v.clone(),
    })
}

// ══════════════════════════════════════════════════════════════
// 整包校验
// ══════════════════════════════════════════════════════════════

/// 校验通过的一个包：**原始字节**（要落库）+ 解析结果。
#[derive(Debug)]
pub struct VerifiedBundle {
    /// zip 原始字节（整包）。
    pub bytes: Vec<u8>,
    /// 整包 sha256（`project_bundles.bundle_sha256`）。
    pub bundle_sha256: String,
    pub manifest: Manifest,
    /// 已反序列化 + 已对账的快照（import 时会**重新**解析一次，见 `provision`）。
    pub snapshot: crate::domain::snapshot::Snapshot,
    /// 解析好的包内文件（路径 → 字节），供 import 复用（避免二次解压）。
    pub files: BTreeMap<String, Vec<u8>>,
}

impl VerifiedBundle {
    pub fn requires_dsn(&self) -> bool {
        self.manifest.requires_dsn()
    }

    /// 快照的字节（按 manifest 声明的路径取）。
    pub fn snapshot_bytes(&self) -> Option<&[u8]> {
        self.files
            .get(&self.manifest.ontology.path)
            .map(Vec::as_slice)
    }
}

/// 整包校验（顺序见模块头）。
///
/// `expect_project` 是**接收项目**的名字（端点路径 `:p` 的那一段）。
pub fn verify_and_read(bundle: Vec<u8>, expect_project: &str) -> Result<VerifiedBundle, BundleError> {
    // ① 体积
    if bundle.len() > MAX_BUNDLE_BYTES {
        return Err(too_large(bundle.len()));
    }
    let bundle_sha256 = crate::query::audit::sha256_hex(&bundle);

    // ② zip 合法性 + ③ 路径安全（read_zip 内部逐条判）
    let entries = read_zip(&bundle)?;

    // ④ manifest 按 schema
    let manifest_entry = entries
        .iter()
        .find(|e| e.name == MANIFEST_PATH)
        .ok_or_else(|| manifest_invalid("包根下没有 `manifest.json`"))?;
    let manifest_value: Value = serde_json::from_slice(&manifest_entry.data).map_err(|e| {
        manifest_invalid(format!("`{MANIFEST_PATH}` 不是合法 JSON：{e}"))
    })?;
    let manifest = parse_manifest(&manifest_value)?;

    // ⑤ files[] 覆盖 + 逐个 sha256
    let actual: BTreeMap<&str, &ZipEntry> = entries
        .iter()
        .filter(|e| !e.is_dir && e.name != MANIFEST_PATH)
        .map(|e| (e.name.as_str(), e))
        .collect();
    check_file_set(&manifest, &actual)?;
    check_file_digests(&manifest, &actual)?;

    // ⑥ project 与接收项目同名（防串包）
    if manifest.project != expect_project {
        return Err(project_mismatch(&manifest.project, expect_project));
    }

    // ⑦ 组件计数自洽（先反序列化快照 —— 它同时是 import 的输入）
    let snap_bytes = actual
        .get(manifest.ontology.path.as_str())
        .map(|e| e.data.as_slice())
        .ok_or_else(|| {
            files_mismatch(format!(
                "`components.ontology.path` = {:?} 不在包内",
                manifest.ontology.path
            ))
        })?;
    let snapshot: crate::domain::snapshot::Snapshot =
        serde_json::from_slice(snap_bytes).map_err(|e| {
            parse_failed(format!(
                "包内 {:?} 反序列化不成 Snapshot：{e}",
                manifest.ontology.path
            ))
        })?;
    check_counts(&manifest, &snapshot, &actual)?;

    let files = actual
        .iter()
        .map(|(k, v)| (k.to_string(), v.data.clone()))
        .collect();

    Ok(VerifiedBundle {
        bytes: bundle,
        bundle_sha256,
        manifest,
        snapshot,
        files,
    })
}

/// `files[]` 与包内实际文件**集合相等** + `bytes` 相符。
///
/// ⭐ 契约更正（2026-10-27）：`files[]` 覆盖包里**除 `manifest.json` 之外**的每个文件。
/// **`files[]` 里出现 `manifest.json` ⇒ `bundle.files.mismatch`**（明确判错，
/// ⛔ 不静默忽略）：清单不可能写自己的 sha256 —— 那是一个**物理上不满足的自指**
/// （写完才算得出，算完又要改）。构建端的实现是「除自身外每个文件」，
/// 两边必须同一条判据，否则对同一个包会给出**相反**结论。
fn check_file_set(
    manifest: &Manifest,
    actual: &BTreeMap<&str, &ZipEntry>,
) -> Result<(), BundleError> {
    // ① 自指：清单列了自己
    if let Some(f) = manifest.files.iter().find(|f| f.path == MANIFEST_PATH) {
        return Err(files_mismatch(format!(
            "`files[]` 里列了 `{MANIFEST_PATH}` 自己（声明的 sha256 = {}）—— \
             清单不可能写自己的摘要（自指：写完才算得出，算完又要改）。",
            f.sha256
        )));
    }

    let declared: BTreeMap<&str, &ManifestFile> =
        manifest.files.iter().map(|f| (f.path.as_str(), f)).collect();

    let mut missing: Vec<&str> = declared
        .keys()
        .filter(|p| !actual.contains_key(**p))
        .copied()
        .collect();
    let mut extra: Vec<&str> = actual
        .keys()
        .filter(|p| !declared.contains_key(**p))
        .copied()
        .collect();
    missing.sort_unstable();
    extra.sort_unstable();

    if !missing.is_empty() || !extra.is_empty() {
        return Err(files_mismatch(format!(
            "`files[]` 与包内实际文件不一致：清单里有但包内没有 {missing:?}；\
             包内有但清单没登记 {extra:?}"
        )));
    }
    for (path, f) in declared {
        let e = actual[path];
        if e.data.len() as i64 != f.bytes {
            return Err(files_mismatch(format!(
                "文件 {path:?} 的字节数不符：清单声明 {}，实际 {}",
                f.bytes,
                e.data.len()
            )));
        }
    }
    Ok(())
}

/// 逐个 sha256（+ 组件声明的 sha256 必须与 `files[]` 里那一行一致）。
fn check_file_digests(
    manifest: &Manifest,
    actual: &BTreeMap<&str, &ZipEntry>,
) -> Result<(), BundleError> {
    let declared: BTreeMap<&str, &ManifestFile> =
        manifest.files.iter().map(|f| (f.path.as_str(), f)).collect();

    for (path, e) in actual {
        let f = declared[path];
        let got = crate::query::audit::sha256_hex(&e.data);
        if got != f.sha256 {
            return Err(sha_mismatch(path, &f.sha256, &got));
        }
    }

    // 组件的 `sha256` 是第二处声明 —— 两处不一致同样是「这包说不清自己是什么」。
    for (name, c) in [
        ("ontology", Some(&manifest.ontology)),
        ("am", manifest.am.as_ref()),
        ("db", manifest.db.as_ref()),
    ] {
        let Some(c) = c else { continue };
        let Some(f) = declared.get(c.path.as_str()) else {
            return Err(files_mismatch(format!(
                "`components.{name}.path` = {:?} 没有出现在 `files[]` 里",
                c.path
            )));
        };
        if f.sha256 != c.sha256 {
            return Err(sha_mismatch(&c.path, &c.sha256, &f.sha256));
        }
    }
    Ok(())
}

/// 组件计数自洽：`components.*` 声明的计数 vs **包里实际数出来**的。
///
/// | 组件 | 键 | 怎么数 |
/// | --- | --- | --- |
/// | `ontology` | `entity_types` / `link_types` / `properties` | 快照结构体上直接数 |
/// | `am` | `entities` / `sources` | AM JSON 里那两个**数组**的长度（`access::Mapping` 的字段名就是这两个） |
///
/// ⚠️ **不是 `tenants`**：契约示例里那个 `"tenants": 3` 已作废（2026-10-27）——
/// 构建端产出的 AM 里没有 tenant 概念。旧包若还写着 `tenants`，它既不会被对账
/// 也不会被拒（`component` 在 schema 里是 `additionalProperties: true`）。
///
/// 声明里没写的计数不判（schema 里它们可选）。
fn check_counts(
    manifest: &Manifest,
    snapshot: &crate::domain::snapshot::Snapshot,
    actual: &BTreeMap<&str, &ZipEntry>,
) -> Result<(), BundleError> {
    let properties: usize = snapshot
        .entity_types
        .iter()
        .map(|e| e.properties.len())
        .sum();
    let actual_counts = [
        ("entity_types", snapshot.entity_types.len()),
        ("link_types", snapshot.link_types.len()),
        ("properties", properties),
    ];
    for (k, got) in actual_counts {
        if let Some(declared) = manifest.ontology.counts.get(k) {
            if *declared != got as i64 {
                return Err(manifest_invalid(format!(
                    "`components.ontology.{k}` 声明 {declared}，而快照里数出来是 {got} —— \
                     组件计数不自洽（包自己说的话与包里的内容不符）"
                )));
            }
        }
    }

    // `am`：`entities` / `sources` 是 AM JSON 里两个数组的长度。
    if let Some(am) = &manifest.am {
        let wanted: Vec<(&str, i64)> = ["entities", "sources"]
            .iter()
            .filter_map(|k| am.counts.get(*k).map(|v| (*k, *v)))
            .collect();
        if !wanted.is_empty() {
            let raw = actual
                .get(am.path.as_str())
                .map(|e| e.data.as_slice())
                .ok_or_else(|| {
                    files_mismatch(format!(
                        "`components.am.path` = {:?} 不在包内",
                        am.path
                    ))
                })?;
            let v: Value = serde_json::from_slice(raw).map_err(|e| {
                manifest_invalid(format!(
                    "`components.am.path` = {:?} 不是合法 JSON（声明的计数无从对账）：{e}",
                    am.path
                ))
            })?;
            for (k, declared) in wanted {
                let got = v
                    .get(k)
                    .and_then(Value::as_array)
                    .map(|a| a.len())
                    .ok_or_else(|| {
                        manifest_invalid(format!(
                            "`components.am.{k}` 声明了 {declared}，但 AM JSON 里没有 `{k}` 数组 —— \
                             声明的计数无从对账"
                        ))
                    })?;
                if declared != got as i64 {
                    return Err(manifest_invalid(format!(
                        "`components.am.{k}` 声明 {declared}，而 AM 里数出来是 {got}"
                    )));
                }
            }
        }
    }
    Ok(())
}

// ══════════════════════════════════════════════════════════════
// 判据（纯函数：不连库、不绑 socket）
// ══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── 路径安全 ────────────────────────────────────────────

    #[test]
    fn path_traversal_and_absolute_paths_are_rejected() {
        for bad in [
            "../evil.json",
            "a/../../b.json",
            "/etc/passwd",
            "\\windows\\x",
            "C:/x.json",
            "c:x.json",
            "a//b.json",
            "./a.json",
            "",
            "a/\u{0}b.json",
        ] {
            let got = check_path(bad);
            assert!(got.is_err(), "{bad:?} 应当被拒（bundle.path.illegal）");
            let e = got.unwrap_err();
            assert_eq!(e.code, "bundle.path.illegal", "{bad:?}");
        }
        for good in ["manifest.json", "ontology/snapshot.json", "am/access-mapping.json", "d/"] {
            check_path(good).unwrap_or_else(|e| panic!("{good:?} 应当放行：{e}"));
        }
    }

    // ── manifest ────────────────────────────────────────────

    fn good_manifest() -> Value {
        serde_json::json!({
            "bundle_version": "1.0",
            "project": "cloud-resource",
            "ontology_seq": 5,
            "built_at": "2026-10-27T10:00:00Z",
            "builder": {"name": "ontok-builder", "version": "0.2.9", "host": "lion-mbp"},
            "components": {
                "ontology": {
                    "path": "ontology/snapshot.json",
                    "sha256": "a".repeat(64),
                    "entity_types": 0, "link_types": 0, "properties": 0
                }
            },
            "files": [
                {"path": "ontology/snapshot.json", "sha256": "a".repeat(64), "bytes": 2}
            ]
        })
    }

    #[test]
    fn the_good_manifest_parses_and_carries_the_counts() {
        let m = parse_manifest(&good_manifest()).expect("合法 manifest");
        assert_eq!(m.project, "cloud-resource");
        assert_eq!(m.ontology_seq, 5);
        assert!(!m.requires_dsn());
        assert_eq!(m.ontology.counts.get("entity_types"), Some(&0));
    }

    #[test]
    fn a_missing_field_names_the_path() {
        let mut v = good_manifest();
        v.as_object_mut().unwrap().remove("built_at");
        let e = parse_manifest(&v).unwrap_err();
        assert_eq!(e.code, "bundle.manifest.invalid");
        assert!(e.message.contains("manifest.built_at"), "必须点名字段路径：{}", e.message);
    }

    #[test]
    fn an_unknown_top_level_field_is_rejected() {
        let mut v = good_manifest();
        v["extra"] = serde_json::json!(1);
        let e = parse_manifest(&v).unwrap_err();
        assert_eq!(e.code, "bundle.manifest.invalid");
        assert!(e.message.contains("extra"), "{}", e.message);
    }

    #[test]
    fn an_unknown_bundle_version_is_its_own_code() {
        let mut v = good_manifest();
        v["bundle_version"] = serde_json::json!("2.0");
        let e = parse_manifest(&v).unwrap_err();
        assert_eq!(e.code, "bundle.version.unsupported");
        assert!(e.message.contains("2.0"), "{}", e.message);
    }

    #[test]
    fn requires_dsn_must_carry_a_dialect() {
        let mut v = good_manifest();
        v["components"]["db"] = serde_json::json!({
            "path": "db/schema.json", "sha256": "b".repeat(64), "requires_dsn": true
        });
        let e = parse_manifest(&v).unwrap_err();
        assert!(e.message.contains("dialect"), "{}", e.message);

        v["components"]["db"]["dialect"] = serde_json::json!("postgresql");
        let m = parse_manifest(&v).expect("补上 dialect 后合法");
        assert!(m.requires_dsn());
        assert_eq!(m.dialect(), Some("postgresql"));
    }

    #[test]
    fn a_bad_project_name_or_sha_is_rejected() {
        let mut v = good_manifest();
        v["project"] = serde_json::json!("Cloud_Resource");
        assert_eq!(parse_manifest(&v).unwrap_err().code, "bundle.manifest.invalid");

        let mut v = good_manifest();
        v["files"][0]["sha256"] = serde_json::json!("ABC");
        let e = parse_manifest(&v).unwrap_err();
        assert!(e.message.contains("manifest.files[0].sha256"), "{}", e.message);
    }

    #[test]
    fn valid_project_name_matches_the_schema_pattern() {
        assert!(valid_project_name("cloud-resource"));
        assert!(valid_project_name("a1"));
        assert!(!valid_project_name("a"));
        assert!(!valid_project_name("-a"));
        assert!(!valid_project_name("A-b"));
        // ⭐ 2026-10-27 决策 A：项目名允许下划线（库里既有 `anspire_real` / `req129_ac006_…`）
        assert!(valid_project_name("a_b"));
        assert!(valid_project_name("req129_ac006_22140"));
        assert!(!valid_project_name("a b"));   // 空格仍拒
        assert!(!valid_project_name("a/b"));   // 斜杠仍拒（路径穿越）
        assert!(!valid_project_name(&"a".repeat(64)));
    }

    // ── zip（store-only）──────────────────────────────────────

    /// 造一个 **store-only** zip（method 0）。CRC 固定写 0 —— 本读取器**不校验 CRC**
    /// （完整性判据是每个文件的 sha256，见模块头）。
    ///
    /// ⛔ 刻意**不**提供 deflate 选项：契约只允许 store-only，读取器对 method ≠ 0
    /// 的拒绝由 [`zip_rejects_a_non_stored_entry_as_unsupported_compression`] 钉住，
    /// 那份夹具手搓一个 method 字节即可（不需要任何压缩实现）。
    fn make_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        fn crc32(data: &[u8]) -> u32 {
            let mut crc = 0xFFFF_FFFFu32;
            for b in data {
                crc ^= *b as u32;
                for _ in 0..8 {
                    let mask = (crc & 1).wrapping_neg();
                    crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
                }
            }
            !crc
        }
        let mut out: Vec<u8> = Vec::new();
        let mut central: Vec<u8> = Vec::new();
        for (name, data) in entries {
            let payload: Vec<u8> = data.to_vec();
            let method = 0u16;
            let offset = out.len() as u32;
            out.extend_from_slice(&LFH_SIG.to_le_bytes());
            out.extend_from_slice(&20u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&method.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&crc32(data).to_le_bytes());
            out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(&payload);

            central.extend_from_slice(&CD_SIG.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&method.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&crc32(data).to_le_bytes());
            central.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            central.extend_from_slice(&(data.len() as u32).to_le_bytes());
            central.extend_from_slice(&(name.len() as u16).to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u32.to_le_bytes());
            central.extend_from_slice(&offset.to_le_bytes());
            central.extend_from_slice(name.as_bytes());
        }
        let cd_offset = out.len() as u32;
        let cd_size = central.len() as u32;
        out.extend_from_slice(&central);
        out.extend_from_slice(&EOCD_SIG.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&cd_size.to_le_bytes());
        out.extend_from_slice(&cd_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    /// 把 store-only zip 里**所有** entry 的压缩方法字节改成 `method`
    /// （本地头 + 中央目录两处都要改，否则是「两处声明打架」，会先被别的判据抓到）。
    ///
    /// 只在本地头 / 中央目录的固定偏移上改，**不碰数据区** —— 于是「method 变了、
    /// 字节长度没变」正是契约之外的形态，读取器该在**解之前**就拒。
    fn with_method(zip: &[u8], method: u16) -> Vec<u8> {
        let mut out = zip.to_vec();
        // 本地头：sig(4) ver(2) flags(2) ⇒ method 在 +8。
        // 中央目录：sig(4) ver_made(2) ver_need(2) flags(2) ⇒ method 在 +10。
        for sig in [LFH_SIG, CD_SIG] {
            let mut at = 0usize;
            while let Some(rel) = out[at..]
                .windows(4)
                .position(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]) == sig)
            {
                let p = at + rel;
                let m = if sig == LFH_SIG { p + 8 } else { p + 10 };
                out[m..m + 2].copy_from_slice(&method.to_le_bytes());
                at = p + 4;
            }
        }
        out
    }

    #[test]
    fn zip_reads_stored_entries() {
        let body = b"{\"hello\":\"world\"}".to_vec();
        let zip = make_zip(&[("a.json", &body), ("b/c.json", &body)]);
        let got = read_zip(&zip).expect("应当读得出");
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].name, "a.json");
        assert_eq!(got[0].data, body);
        assert_eq!(got[1].name, "b/c.json");
        assert_eq!(got[1].data, body);
    }

    /// ⛔ 契约只允许 store-only（method 0）：method 8（deflate）/ 其它一律**明确拒绝**。
    ///
    /// 反空转：夹具真的是「method ≠ 0、字节没变」的容器（`with_method` 改的是本地头
    /// 与中央目录的同一个字段），所以本断言不是「随便造个坏包也能拒」。
    #[test]
    fn zip_rejects_a_non_stored_entry_as_unsupported_compression() {
        let body = b"{\"hello\":\"world\"}".to_vec();
        for method in [8u16, 1, 99] {
            let zip = with_method(&make_zip(&[("a.json", &body)]), method);
            let e = read_zip(&zip).unwrap_err();
            assert_eq!(
                e.code, "bundle.unsupported.compression",
                "method {method} 应当按「契约只允许 store-only」拒，实得 {e:?}"
            );
            assert!(
                e.message.contains(&format!("method = {method}")),
                "报错要点名收到的 method：{}",
                e.message
            );
            assert!(
                e.message.contains("store-only"),
                "报错要说清契约只允许 store-only：{}",
                e.message
            );
        }
        // 反空转：method 0 的那一份必须读得出（否则上面那圈「全拒」是废话）。
        assert!(read_zip(&make_zip(&[("a.json", &body)])).is_ok());
    }

    #[test]
    fn zip_rejects_a_traversal_entry_before_reading_bytes() {
        let zip = make_zip(&[("../evil.json", b"x")]);
        let e = read_zip(&zip).unwrap_err();
        assert_eq!(e.code, "bundle.path.illegal");
    }

    #[test]
    fn non_zip_bytes_are_manifest_invalid() {
        let e = read_zip(b"not a zip at all, definitely not").unwrap_err();
        assert_eq!(e.code, "bundle.manifest.invalid");
        assert!(e.message.contains("zip"), "{}", e.message);
    }

    // ── 整包 ────────────────────────────────────────────────

    /// 一个**真**包：manifest 与 snapshot 的 sha/bytes 都对得上。
    fn bundle_for(project: &str, snapshot: &crate::domain::snapshot::Snapshot) -> Vec<u8> {
        let snap_bytes = serde_json::to_vec(snapshot).unwrap();
        let props: usize = snapshot.entity_types.iter().map(|e| e.properties.len()).sum();
        let mut m = good_manifest();
        m["project"] = serde_json::json!(project);
        m["components"]["ontology"]["sha256"] =
            serde_json::json!(crate::query::audit::sha256_hex(&snap_bytes));
        m["components"]["ontology"]["entity_types"] = serde_json::json!(snapshot.entity_types.len());
        m["components"]["ontology"]["link_types"] = serde_json::json!(snapshot.link_types.len());
        m["components"]["ontology"]["properties"] = serde_json::json!(props);
        m["files"] = serde_json::json!([{
            "path": "ontology/snapshot.json",
            "sha256": crate::query::audit::sha256_hex(&snap_bytes),
            "bytes": snap_bytes.len(),
        }]);
        let m_bytes = serde_json::to_vec(&m).unwrap();
        make_zip(&[
            ("manifest.json", &m_bytes),
            ("ontology/snapshot.json", &snap_bytes),
        ])
    }

    #[test]
    fn a_well_formed_bundle_verifies() {
        let mut snap = crate::domain::snapshot::Snapshot::new(uuid::Uuid::nil(), 5);
        snap.entity_types.push(crate::domain::snapshot::EntityType {
            key: "vm".into(),
            display_name: "虚机".into(),
            description: None,
            aliases: vec![],
            parent: None,
            disjoint_with: vec![],
            properties: vec![],
            evidence: vec![],
        });
        let zip = bundle_for("cloud-resource", &snap);
        let v = verify_and_read(zip, "cloud-resource").expect("应当校验通过");
        assert_eq!(v.manifest.project, "cloud-resource");
        assert_eq!(v.snapshot.entity_types.len(), 1);
        assert!(!v.requires_dsn());
        assert_eq!(v.bundle_sha256.len(), 64);
    }

    #[test]
    fn a_tampered_file_is_sha256_mismatch() {
        let snap = crate::domain::snapshot::Snapshot::new(uuid::Uuid::nil(), 5);
        let zip = bundle_for("cloud-resource", &snap);
        let entries = read_zip(&zip).unwrap();
        // **同长度**改一个字节（`bytes` 不变、sha256 变）—— 这才是 AC-002 的形态：
        // 「篡改一个文件的 sha256」而不是「包的长度变了」。
        let mut rebuilt: Vec<(String, Vec<u8>)> = Vec::new();
        for e in &entries {
            let mut d = e.data.clone();
            if e.name == "ontology/snapshot.json" {
                let last = d.len() - 1;
                d[last] = if d[last] == b'}' { b']' } else { b'}' };
            }
            rebuilt.push((e.name.clone(), d));
        }
        let refs: Vec<(&str, &[u8])> = rebuilt
            .iter()
            .map(|(n, d)| (n.as_str(), d.as_slice()))
            .collect();
        let tampered = make_zip(&refs);
        let e = verify_and_read(tampered, "cloud-resource").unwrap_err();
        assert_eq!(e.code, "bundle.sha256.mismatch");
        assert!(e.message.contains("ontology/snapshot.json"), "{}", e.message);
    }

    /// 校验顺序的第二面：**长度**变了 ⇒ `bundle.files.mismatch`（`bytes` 那一栏先判），
    /// 不是 sha256 那条。两条错误码各自对应一种「包与清单对不上」的形态。
    #[test]
    fn a_file_whose_length_changed_is_files_mismatch() {
        let snap = crate::domain::snapshot::Snapshot::new(uuid::Uuid::nil(), 5);
        let zip = bundle_for("cloud-resource", &snap);
        let entries = read_zip(&zip).unwrap();
        let mut rebuilt: Vec<(String, Vec<u8>)> = Vec::new();
        for e in &entries {
            let mut d = e.data.clone();
            if e.name == "ontology/snapshot.json" {
                d.push(b' ');
            }
            rebuilt.push((e.name.clone(), d));
        }
        let refs: Vec<(&str, &[u8])> = rebuilt
            .iter()
            .map(|(n, d)| (n.as_str(), d.as_slice()))
            .collect();
        let bigger = make_zip(&refs);
        let e = verify_and_read(bigger, "cloud-resource").unwrap_err();
        assert_eq!(e.code, "bundle.files.mismatch");
    }

    #[test]
    fn a_wrong_project_is_project_mismatch() {
        let snap = crate::domain::snapshot::Snapshot::new(uuid::Uuid::nil(), 5);
        let zip = bundle_for("other-project", &snap);
        let e = verify_and_read(zip, "cloud-resource").unwrap_err();
        assert_eq!(e.code, "bundle.project.mismatch");
        assert!(e.message.contains("other-project") && e.message.contains("cloud-resource"));
    }

    /// ⭐ 契约更正（2026-10-27）：`files[]` 列了 `manifest.json` 自己 ⇒
    /// **`bundle.files.mismatch`**（明确判错，不是静默忽略）。
    #[test]
    fn a_files_entry_pointing_at_the_manifest_itself_is_rejected() {
        let snap = crate::domain::snapshot::Snapshot::new(uuid::Uuid::nil(), 5);
        let snap_bytes = serde_json::to_vec(&snap).unwrap();
        let mut m = good_manifest();
        m["components"]["ontology"]["sha256"] =
            serde_json::json!(crate::query::audit::sha256_hex(&snap_bytes));
        m["files"] = serde_json::json!([
            {"path": "ontology/snapshot.json",
             "sha256": crate::query::audit::sha256_hex(&snap_bytes),
             "bytes": snap_bytes.len()},
            // 自指：清单列了自己
            {"path": "manifest.json", "sha256": "c".repeat(64), "bytes": 100},
        ]);
        let m_bytes = serde_json::to_vec(&m).unwrap();
        let zip = make_zip(&[
            ("manifest.json", &m_bytes),
            ("ontology/snapshot.json", &snap_bytes),
        ]);
        let e = verify_and_read(zip, "cloud-resource").unwrap_err();
        assert_eq!(e.code, "bundle.files.mismatch");
        assert!(e.message.contains("manifest.json"), "{}", e.message);
        assert!(e.message.contains("自指"), "{}", e.message);
    }

    #[test]
    fn a_count_that_does_not_match_the_snapshot_is_rejected() {
        let snap = crate::domain::snapshot::Snapshot::new(uuid::Uuid::nil(), 5);
        let snap_bytes = serde_json::to_vec(&snap).unwrap();
        let mut m = good_manifest();
        m["components"]["ontology"]["sha256"] =
            serde_json::json!(crate::query::audit::sha256_hex(&snap_bytes));
        m["components"]["ontology"]["entity_types"] = serde_json::json!(30); // 谎报
        m["files"] = serde_json::json!([{
            "path": "ontology/snapshot.json",
            "sha256": crate::query::audit::sha256_hex(&snap_bytes),
            "bytes": snap_bytes.len(),
        }]);
        let m_bytes = serde_json::to_vec(&m).unwrap();
        let zip = make_zip(&[
            ("manifest.json", &m_bytes),
            ("ontology/snapshot.json", &snap_bytes),
        ]);
        let e = verify_and_read(zip, "cloud-resource").unwrap_err();
        assert_eq!(e.code, "bundle.manifest.invalid");
        assert!(e.message.contains("entity_types"), "{}", e.message);
    }

    // ── multipart ───────────────────────────────────────────
}

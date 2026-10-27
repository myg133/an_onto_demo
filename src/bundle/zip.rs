//! **store-only ZIP** —— 手写写 + 手写读 + 手写 CRC32。
//!
//! # 为什么手写（而不是 `zip` / `flate2`）
//!
//! `Cargo.toml` 顶部写着「⛔ 不新增 crate」，而 `.ontokbundle` 只是**搬运容器**：
//! 内容全是已经序列化好的 JSON，压缩率对人没有价值，但**确定性**有价值。
//! store（method 0）意味着「磁盘上的字节 = 内存里的字节」，
//! 于是「同一输入 ⇒ 同一输出」可以逐字节断言 —— 换成 deflate 就得先钉死
//! 压缩器版本与压缩级别，那是把确定性押在别人的实现上。
//!
//! # 格式（只实现能用到的这一格）
//!
//! ```text
//! [local file header + data] × N   ← 无 data descriptor（bit 3 = 0）
//! [central directory entry]  × N
//! [EOCD]
//! ```
//!
//! | 字段 | 取值 | 为什么 |
//! | --- | --- | --- |
//! | method | `0`（store） | 任何 `unzip` / `zipfile` / 各语言 zip 库都能读 |
//! | bit 3（data descriptor） | **0** | 有它就得在 data 之后再写签名块，多一处可漂移的字节 |
//! | bit 11（UTF-8） | **1** | 文件名按 UTF-8 解释，而不是 CP437 |
//! | 时间戳 | time `0x0000` + date `0x0021` | `0x0021` = DOS date 1980-01-01；配 00:00:00 即「1980-01-01 00:00」，**固定** ⇒ 两次打包字节相同 |
//! | 外部属性 | `0` | 不设 unix mode ⇒ 不受平台 / umask 影响 |
//! | extra / comment | 长度恒 `0` | 少两处「谁写进来的」 |
//! | version made by | `0x0014` | host = 0（MS-DOS FAT），上字节即版本 2.0 —— 与「外部属性为 0」自洽 |
//!
//! ⛔ **不实现**：deflate、zip64、加密、data descriptor、跨盘、符号链接。
//! 读到这些形状一律**报错**（fail-closed），不猜。

use std::fmt;

// ───────────────────────── 常量 ─────────────────────────

/// local file header 签名（`PK\x03\x04`）。
pub const LOCAL_SIG: u32 = 0x0403_4b50;
/// central directory header 签名（`PK\x01\x02`）。
pub const CENTRAL_SIG: u32 = 0x0201_4b50;
/// end of central directory 签名（`PK\x05\x06`）。
pub const EOCD_SIG: u32 = 0x0605_4b50;

/// general purpose bit 11：文件名是 UTF-8。
pub const FLAG_UTF8: u16 = 0x0800;
/// general purpose bit 3：data descriptor（**本实现恒不置位**）。
pub const FLAG_DATA_DESCRIPTOR: u16 = 0x0008;

/// 压缩方法：store（不压缩）。
pub const METHOD_STORE: u16 = 0;

/// DOS **date**：`0x0021` = 1980-01-01（zip 纪元）。见模块头的表格。
pub const DOS_DATE_1980_01_01: u16 = 0x0021;
/// DOS **time**：`0x0000` = 00:00:00。与 [`DOS_DATE_1980_01_01`] 合起来才是「1980-01-01 00:00」。
pub const DOS_TIME_MIDNIGHT: u16 = 0x0000;

/// "version needed to extract" = 2.0（store + 无 zip64 的最低要求）。
const VERSION_NEEDED: u16 = 20;
/// "version made by" = MS-DOS host（高字节 0）+ 2.0。
const VERSION_MADE_BY: u16 = 0x0014;

/// local header 固定部分长度。
const LOCAL_HEADER_LEN: usize = 30;
/// central directory entry 固定部分长度。
const CENTRAL_HEADER_LEN: usize = 46;
/// EOCD 固定部分长度。
const EOCD_LEN: usize = 22;

// ───────────────────────── 错误 ─────────────────────────

/// zip 层面的失败。**每一种都点名**，因为下游要把它翻成 `bundle.*` 拒绝码。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZipError {
    /// 路径非法（绝对路径 / `..` / 反斜杠 / 字符集越界 / 空段）。
    PathIllegal { path: String, why: &'static str },
    /// 同一份 zip 里两个 entry 同名。
    DuplicatePath { path: String },
    /// 单个 entry 或总大小越过 u32（不做 zip64）。
    TooLarge { path: String, bytes: usize },
    /// entry 数越过 u16（不做 zip64）。
    TooManyEntries { count: usize },
    /// 文件名超过 u16 能表达的长度（zip 的 name length 字段只有 2 字节）。
    ///
    /// ⚠️ **必须报错，不许截断**（QA F-05）：`name.len() as u16` 会把 65537 字节的
    /// 名字写成 1 —— 读回来是另一个文件名，而 CRC 仍对得上，于是得到一个
    /// 「结构合法但内容错位」的容器，比直接拒难查得多。
    NameTooLong { path: String, bytes: usize },
    /// 容器结构坏（截断 / 签名不对 / 字段组合不受支持）。
    BadFormat(String),
}

impl fmt::Display for ZipError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ZipError::PathIllegal { path, why } => write!(f, "路径非法 `{path}`：{why}"),
            ZipError::DuplicatePath { path } => write!(f, "包内有两个同名 entry：`{path}`"),
            ZipError::TooLarge { path, bytes } => {
                write!(f, "`{path}` 有 {bytes} 字节，单文件超过 4 GiB（本实现不做 zip64）")
            }
            ZipError::TooManyEntries { count } => {
                write!(f, "有 {count} 个 entry，超过 65535（本实现不做 zip64）")
            }
            ZipError::NameTooLong { path, bytes } => write!(
                f,
                "文件名有 {bytes} 字节，超过 65535（zip 的 name length 字段是 u16；\
                 本实现不做 zip64 扩展）：`{}…`",
                path.chars().take(60).collect::<String>()
            ),
            ZipError::BadFormat(m) => write!(f, "容器结构不合规：{m}"),
        }
    }
}

impl std::error::Error for ZipError {}

// ───────────────────────── CRC32 ─────────────────────────

/// CRC32（IEEE 802.3，反射多项式 `0xEDB88320`）—— **zip 用的就是这一个**。
///
/// 查表法，表在编译期算好（`const fn`），运行期零初始化。
pub fn crc32(data: &[u8]) -> u32 {
    let mut c: u32 = 0xFFFF_FFFF;
    for &b in data {
        c = CRC32_TABLE[((c ^ u32::from(b)) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

/// `crc32` 的查表（256 项）。
static CRC32_TABLE: [u32; 256] = build_crc32_table();

const fn build_crc32_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}

// ───────────────────────── 路径规则（写与读共用） ─────────────────────────

/// 一个包内路径**为什么**非法；合法 ⇒ `None`。
///
/// 规则来自 `manifest.schema.json` 的 `files[].path.pattern`
/// （`^(?!/)(?!.*\.\.)[A-Za-z0-9._/-]+$`）与 REQ-159 §三-6（防穿越）：
///
/// | 规则 | 为什么 |
/// | --- | --- |
/// | 非空 | 空路径不是一个文件 |
/// | 不以 `/` 开头、无盘符式 `C:` | 绝对路径 ⇒ 解包可能落到任意位置 |
/// | 无 `\` | 反斜杠在 zip 里不是分隔符，会被某些读取器当转义 |
/// | 任何一段不是 `.` / `..` | `..` 穿越；`.` 段是无意义的别名 |
/// | 无空段（`//` / 首尾 `/`） | 同上，且不同工具对空段处理不一致 |
/// | 字符集 `[A-Za-z0-9._/-]` | 与 schema pattern 同款 —— 打包端就要拒，而不是等自检拒 |
pub fn illegal_path_reason(p: &str) -> Option<&'static str> {
    if p.is_empty() {
        return Some("空路径");
    }
    if p.contains('\\') {
        return Some("含反斜杠（zip 的路径分隔只认正斜杠）");
    }
    if p.starts_with('/') {
        return Some("是绝对路径（不许以 / 开头）");
    }
    if p.len() >= 2 && p.as_bytes()[1] == b':' && p.as_bytes()[0].is_ascii_alphabetic() {
        return Some("带盘符（绝对路径的另一种写法）");
    }
    for seg in p.split('/') {
        if seg.is_empty() {
            return Some("含空段（// 或首尾有 /）");
        }
        if seg == "." || seg == ".." {
            return Some("含 `.` / `..` 段（防穿越）");
        }
    }
    if p
        .chars()
        .any(|c| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/')))
    {
        return Some("含 schema 允许字符集之外的字符（只许 A-Za-z0-9._-/）");
    }
    None
}

// ───────────────────────── 写 ─────────────────────────

fn push_u16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn push_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// 写一份 store-only zip。
///
/// `entries` 的**顺序即包内顺序**（调用方负责把 `manifest.json` 放第一、
/// 其余按 path 升序 —— 那两条是契约，不是本函数的口味）。
/// 本函数只保证「进来的顺序，原样落盘」。
pub fn write(entries: &[(String, Vec<u8>)]) -> Result<Vec<u8>, ZipError> {
    if entries.len() > u16::MAX as usize {
        return Err(ZipError::TooManyEntries { count: entries.len() });
    }

    // ① 先整体校验（路径 / 重名 / 大小 / 名字长度），**一个字节都不写**才落盘：
    //    半份 zip 比没有 zip 更坏 —— 它看起来是合法的。
    let mut seen = std::collections::BTreeSet::new();
    for (path, data) in entries {
        if let Some(why) = illegal_path_reason(path) {
            return Err(ZipError::PathIllegal { path: path.clone(), why });
        }
        // ⚠️ 名字长度必须是 u16（QA F-05）：⛔ 不许 `as u16` 静默截断。
        if path.len() > u16::MAX as usize {
            return Err(ZipError::NameTooLong { path: path.clone(), bytes: path.len() });
        }
        if !seen.insert(path.as_str()) {
            return Err(ZipError::DuplicatePath { path: path.clone() });
        }
        if data.len() > u32::MAX as usize {
            return Err(ZipError::TooLarge { path: path.clone(), bytes: data.len() });
        }
    }

    let mut out: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();

    // ② local header + data，逐个追加（偏移在写的同时记下来）。
    for (path, data) in entries {
        let name = path.as_bytes();
        let crc = crc32(data);
        let size = data.len() as u32;
        let offset = out.len() as u32;

        push_u32(&mut out, LOCAL_SIG);
        push_u16(&mut out, VERSION_NEEDED);
        push_u16(&mut out, FLAG_UTF8); // bit 3 恒 0：无 data descriptor
        push_u16(&mut out, METHOD_STORE);
        push_u16(&mut out, DOS_TIME_MIDNIGHT);
        push_u16(&mut out, DOS_DATE_1980_01_01);
        push_u32(&mut out, crc);
        push_u32(&mut out, size); // compressed size == uncompressed（store）
        push_u32(&mut out, size);
        push_u16(&mut out, name.len() as u16);
        push_u16(&mut out, 0); // extra len
        out.extend_from_slice(name);
        out.extend_from_slice(data);

        // ③ central directory entry（与 local header 一一对应）。
        push_u32(&mut central, CENTRAL_SIG);
        push_u16(&mut central, VERSION_MADE_BY);
        push_u16(&mut central, VERSION_NEEDED);
        push_u16(&mut central, FLAG_UTF8);
        push_u16(&mut central, METHOD_STORE);
        push_u16(&mut central, DOS_TIME_MIDNIGHT);
        push_u16(&mut central, DOS_DATE_1980_01_01);
        push_u32(&mut central, crc);
        push_u32(&mut central, size);
        push_u32(&mut central, size);
        push_u16(&mut central, name.len() as u16);
        push_u16(&mut central, 0); // extra len
        push_u16(&mut central, 0); // comment len
        push_u16(&mut central, 0); // disk number start
        push_u16(&mut central, 0); // internal attrs
        push_u32(&mut central, 0); // external attrs（⛔ 不设 unix mode）
        push_u32(&mut central, offset);
        central.extend_from_slice(name);
    }

    // ④ central directory + EOCD。
    let cd_offset = out.len() as u32;
    let cd_size = central.len() as u32;
    out.extend_from_slice(&central);

    push_u32(&mut out, EOCD_SIG);
    push_u16(&mut out, 0); // 本盘号
    push_u16(&mut out, 0); // central directory 起始盘
    push_u16(&mut out, entries.len() as u16); // 本盘 entry 数
    push_u16(&mut out, entries.len() as u16); // entry 总数
    push_u32(&mut out, cd_size);
    push_u32(&mut out, cd_offset);
    push_u16(&mut out, 0); // comment len

    Ok(out)
}

// ───────────────────────── 读 ─────────────────────────

/// 读出来的一个 entry。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// 包内路径（正斜杠，UTF-8）。
    pub path: String,
    /// 原始字节。
    pub data: Vec<u8>,
    /// **落盘时写的** CRC32（central directory 那一格）。
    pub crc32: u32,
    /// 按 `data` 重算的 CRC32 与 [`Entry::crc32`] 是否相等。
    ///
    /// 刻意**不在这里报错**：CRC 不符属于「字节完整性」，
    /// 由 [`crate::bundle::verify`] 翻成 `bundle.sha256.mismatch`（8 个拒绝码里的那一档），
    /// 而不是让整包在「解不开」和「解开了但内容坏了」之间失去区分。
    pub crc_ok: bool,
}

fn read_u16(b: &[u8], at: usize) -> Result<u16, ZipError> {
    let s = b
        .get(at..at + 2)
        .ok_or_else(|| ZipError::BadFormat(format!("读 u16 越界（偏移 {at}）")))?;
    Ok(u16::from_le_bytes([s[0], s[1]]))
}

fn read_u32(b: &[u8], at: usize) -> Result<u32, ZipError> {
    let s = b
        .get(at..at + 4)
        .ok_or_else(|| ZipError::BadFormat(format!("读 u32 越界（偏移 {at}）")))?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn read_name(b: &[u8], at: usize, len: usize) -> Result<String, ZipError> {
    let s = b
        .get(at..at + len)
        .ok_or_else(|| ZipError::BadFormat(format!("读文件名越界（偏移 {at}，长 {len}）")))?;
    String::from_utf8(s.to_vec())
        .map_err(|_| ZipError::BadFormat(format!("偏移 {at} 的文件名不是合法 UTF-8")))
}

/// 解析一份 store-only zip。**每个 entry 的 CRC 都会重算**（结果在 [`Entry::crc_ok`]）。
pub fn read(bytes: &[u8]) -> Result<Vec<Entry>, ZipError> {
    // ① EOCD 在文件尾（可能带注释）。从后往前找第一个签名，最多回溯 64 KiB + 22。
    if bytes.len() < EOCD_LEN {
        return Err(ZipError::BadFormat(format!(
            "只有 {} 字节，连 EOCD 都放不下（至少 {} 字节）",
            bytes.len(),
            EOCD_LEN
        )));
    }
    let lowest = bytes.len().saturating_sub(EOCD_LEN + u16::MAX as usize);
    let mut eocd_at: Option<usize> = None;
    let mut at = bytes.len() - EOCD_LEN;
    loop {
        if read_u32(bytes, at)? == EOCD_SIG {
            eocd_at = Some(at);
            break;
        }
        if at == lowest {
            break;
        }
        at -= 1;
    }
    let eocd_at = eocd_at.ok_or_else(|| {
        ZipError::BadFormat("找不到 EOCD 签名（不是 zip，或文件被截断）".into())
    })?;

    let count = read_u16(bytes, eocd_at + 10)? as usize;
    let cd_size = read_u32(bytes, eocd_at + 12)? as usize;
    let cd_offset = read_u32(bytes, eocd_at + 16)? as usize;
    let comment_len = read_u16(bytes, eocd_at + 20)? as usize;
    if eocd_at + EOCD_LEN + comment_len > bytes.len() {
        return Err(ZipError::BadFormat("EOCD 声明了比文件更长的注释".into()));
    }
    if cd_offset.checked_add(cd_size).map(|e| e > bytes.len()).unwrap_or(true) {
        return Err(ZipError::BadFormat(format!(
            "central directory 越界（offset {cd_offset} + size {cd_size} > {} 字节）",
            bytes.len()
        )));
    }

    // ② 逐个 central directory entry。
    let mut entries: Vec<Entry> = Vec::with_capacity(count);
    let mut pos = cd_offset;
    for i in 0..count {
        if read_u32(bytes, pos)? != CENTRAL_SIG {
            return Err(ZipError::BadFormat(format!(
                "第 {i} 个 central directory entry 签名不对（偏移 {pos}）"
            )));
        }
        let made_by = read_u16(bytes, pos + 4)?;
        let flags = read_u16(bytes, pos + 8)?;
        let method = read_u16(bytes, pos + 10)?;
        let crc = read_u32(bytes, pos + 16)?;
        let csize = read_u32(bytes, pos + 20)? as usize;
        let usize_ = read_u32(bytes, pos + 24)? as usize;
        let name_len = read_u16(bytes, pos + 28)? as usize;
        let extra_len = read_u16(bytes, pos + 30)? as usize;
        let comment = read_u16(bytes, pos + 32)? as usize;
        let local_offset = read_u32(bytes, pos + 42)? as usize;

        if flags & FLAG_DATA_DESCRIPTOR != 0 {
            return Err(ZipError::BadFormat(format!(
                "第 {i} 个 entry 置了 bit 3（data descriptor）—— 本实现不写也不认"
            )));
        }
        if method != METHOD_STORE {
            return Err(ZipError::BadFormat(format!(
                "第 {i} 个 entry 的压缩方法是 {method}，只认 0（store）"
            )));
        }
        if csize != usize_ {
            return Err(ZipError::BadFormat(format!(
                "第 {i} 个 entry 的压缩/原长不一致（{csize} vs {usize_}）—— 不是 store"
            )));
        }
        // 高位字节是 host（0 = MS-DOS / FAT）。只做**记录性**校验：
        // 非 0 host 的包本来就不该由本工具产生，但读的时候不因此拒绝整包。
        let _ = made_by;

        let path = read_name(bytes, pos + CENTRAL_HEADER_LEN, name_len)?;
        // ⚠️ 这里**不判**路径合法性：读只是「把字节摊开」，判据归 [`crate::bundle::verify`]
        // （它会翻成 `bundle.path.illegal`，而不是让整个包在「解不开」这一档里丢掉细节）。
        // ⛔ 任何把 entry 落到磁盘的调用方**必须自己**再查一遍
        // [`illegal_path_reason`] —— 本函数不承担解包防护。
        pos += CENTRAL_HEADER_LEN + name_len + extra_len + comment;

        // ③ local header → data。
        if read_u32(bytes, local_offset)? != LOCAL_SIG {
            return Err(ZipError::BadFormat(format!(
                "`{path}` 的 local header 签名不对（偏移 {local_offset}）"
            )));
        }
        let l_name_len = read_u16(bytes, local_offset + 26)? as usize;
        let l_extra_len = read_u16(bytes, local_offset + 28)? as usize;
        let local_name = read_name(bytes, local_offset + LOCAL_HEADER_LEN, l_name_len)?;
        if local_name != path {
            return Err(ZipError::BadFormat(format!(
                "central directory 与 local header 的文件名不一致：`{path}` vs `{local_name}`"
            )));
        }
        let data_at = local_offset + LOCAL_HEADER_LEN + l_name_len + l_extra_len;
        let data = bytes
            .get(data_at..data_at + csize)
            .ok_or_else(|| {
                ZipError::BadFormat(format!(
                    "`{path}` 的数据越界（{data_at} + {csize} > {}）",
                    bytes.len()
                ))
            })?
            .to_vec();

        if entries.iter().any(|e| e.path == path) {
            return Err(ZipError::DuplicatePath { path });
        }
        entries.push(Entry {
            crc32: crc,
            crc_ok: crc32(&data) == crc,
            path,
            data,
        });
    }

    if pos != cd_offset + cd_size {
        return Err(ZipError::BadFormat(format!(
            "central directory 实际长度与我们走过的长度不符（{} vs {cd_size}）",
            pos - cd_offset
        )));
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CRC32 的标准自检值：`crc32("123456789") == 0xCBF43926`（与 zlib / Python 同值）。
    #[test]
    fn crc32_matches_the_standard_check_value() {
        assert_eq!(crc32(b""), 0x0000_0000);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b"The quick brown fox jumps over the lazy dog"), 0x414F_A339);
        // 单比特翻转必然改 CRC（反空转：别让「恒返回 0」也过）。
        assert_ne!(crc32(b"a"), crc32(b"b"));
    }

    #[test]
    fn write_then_read_is_bytewise_identical() {
        let entries = vec![
            ("manifest.json".to_string(), br#"{"a":1}"#.to_vec()),
            ("ontology/snapshot.json".to_string(), vec![]),
            ("db/schema.json".to_string(), "中文内容".as_bytes().to_vec()),
        ];
        let zip = write(&entries).expect("写失败");
        let back = read(&zip).expect("读失败");
        assert_eq!(back.len(), 3);
        for (i, (path, data)) in entries.iter().enumerate() {
            assert_eq!(&back[i].path, path);
            assert_eq!(&back[i].data, data);
            assert!(back[i].crc_ok, "`{path}` 的 CRC 没对上");
        }
    }

    #[test]
    fn write_is_deterministic_and_uses_store_without_data_descriptor() {
        let entries = vec![("manifest.json".to_string(), b"{}".to_vec())];
        let a = write(&entries).unwrap();
        let b = write(&entries).unwrap();
        assert_eq!(a, b, "同一输入两次写出的字节必须相同");

        // flags = UTF-8（0x0800），bit 3 = 0
        let flags = u16::from_le_bytes([a[6], a[7]]);
        assert_eq!(flags, FLAG_UTF8);
        assert_eq!(flags & FLAG_DATA_DESCRIPTOR, 0);
        // method = 0（store）
        assert_eq!(u16::from_le_bytes([a[8], a[9]]), METHOD_STORE);
        // 时间戳固定：time 0x0000 + date 0x0021
        assert_eq!(u16::from_le_bytes([a[10], a[11]]), DOS_TIME_MIDNIGHT);
        assert_eq!(u16::from_le_bytes([a[12], a[13]]), DOS_DATE_1980_01_01);
        // 外部属性 = 0（central directory 里，固定部分第 38..42 字节）
        let central_at = a.len() - EOCD_LEN - CENTRAL_HEADER_LEN - "manifest.json".len();
        assert_eq!(read_u32(&a, central_at + 38).unwrap(), 0);
    }

    #[test]
    fn illegal_paths_are_rejected_on_write() {
        for bad in [
            "",
            "/abs.json",
            "../up.json",
            "a/../b.json",
            "a\\b.json",
            "C:/x.json",
            "a//b.json",
            "a/./b.json",
            "中文.json",
        ] {
            let e = write(&[(bad.to_string(), vec![])]);
            assert!(
                matches!(e, Err(ZipError::PathIllegal { .. })),
                "`{bad}` 应当被拒，实际 {e:?}"
            );
        }
        // 合法对照（反空转）
        assert!(write(&[("a/b-c_d.1.json".to_string(), vec![])]).is_ok());
    }

    #[test]
    fn reader_rejects_truncated_and_garbage_input() {
        assert!(matches!(read(b"PK"), Err(ZipError::BadFormat(_))));
        let zip = write(&[("a.json".to_string(), b"123".to_vec())]).unwrap();
        let cut = &zip[..zip.len() - 1];
        assert!(matches!(read(cut), Err(ZipError::BadFormat(_))));
    }

    #[test]
    fn reader_detects_crc_corruption_without_losing_the_entry() {
        let mut zip = write(&[("a.json".to_string(), b"1234567890".to_vec())]).unwrap();
        // 数据在 local header(30) + name(6) 之后；翻一个字节，CRC 必然不符。
        let at = LOCAL_HEADER_LEN + "a.json".len();
        zip[at] ^= 0xFF;
        let back = read(&zip).expect("结构没坏，应当读得出来");
        assert_eq!(back.len(), 1);
        assert!(!back[0].crc_ok, "改了数据却没发现 CRC 不符");
    }

    /// QA F-05：名字超长必须**报错**，⛔ 不许 `as u16` 静默截断成另一个名字。
    #[test]
    fn name_too_long_is_rejected_instead_of_truncated() {
        let too_long = "a".repeat(u16::MAX as usize + 1);
        let e = write(&[(too_long.clone(), b"x".to_vec())]);
        match e {
            Err(ZipError::NameTooLong { bytes, .. }) => assert_eq!(bytes, too_long.len()),
            other => panic!("超长名字应当报 NameTooLong，实际 {other:?}"),
        }
        // 反空转：正好 65535 字节的名字**可以**写（边界不是随手挑的数字）。
        let edge = "a".repeat(u16::MAX as usize);
        let zip = write(&[(edge.clone(), b"x".to_vec())]).expect("65535 字节的名字应当能写");
        let back = read(&zip).expect("读不回来");
        assert_eq!(back[0].path, edge, "读回来的名字必须与被写的一字不差");
    }
}

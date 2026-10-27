//! ⭐ REQ-159 · **密文列** —— 源库 DSN 的解析 / 加密 / 连通性探测 + query key 的加密。
//!
//! # 三件事、三条硬约束
//!
//! | 事 | 约束 | 判据 |
//! | --- | --- | --- |
//! | 存 | 元数据库**加密列**（AES-256-GCM，密钥来自环境变量） | AC-009：响应 / 日志 / 审计里搜不到明文 |
//! | 回显 | ⛔ DSN 响应里**没有** `dsn`；只有 `host` / `database` / `user` | AC-009 |
//! | 测 | `POST …/database/test` **失败也返 200**，错误**分类**（auth / timeout / dns / refused） | AC-005 |
//!
//! # 一把密钥保护**两列**（REQ-159 契约变更 · 2026-10-27 10:25）
//!
//! | 列 | 明文 | 为什么可逆 |
//! | --- | --- | --- |
//! | `project_databases.dsn_ciphertext` | 源库连接串 | 探测 / import 时要解出来用 |
//! | `project_keys.token_ciphertext` | 发给数字员工的 query key | admin 要能**再次查看**（AC-011） |
//!
//! `project_keys` **同时**留 `token_sha256`：鉴权按摘要查（快、不碰密钥、不把明文
//! 拉进热路径），查看端点才解密。⛔ 只有 ciphertext 的方案会让每次鉴权都解密。
//!
//! # 加密：为什么是 AES-256-GCM 且密钥只从环境来
//!
//! `demand.md` §七-5 定的是「元数据库加密列（AES-GCM，密钥来自环境）」。
//! 用 `ring` 的 AEAD（**它已经在这个二进制里**，TLS 在用 —— 见 `Cargo.toml` 的登记），
//! 不自己写 GCM：认证标签校验与常量时间比较属于「写错了看不出来」的部分。
//!
//! ## 密文形状（两列同形）
//!
//! ```text
//! v1:<hex( nonce(12) || ciphertext || tag(16) )>
//! ```
//!
//! - `v1:` 前缀是**版本位**：将来换算法 / 换密钥长度时，老密文仍然读得出来
//!   （⛔ 而不是「解密失败 ⇒ 当成空 DSN」那种静默降级）；
//! - nonce **每条随机**（`SystemRandom`）。固定 nonce 在 GCM 下是灾难性的
//!   （两条密文一异或就出明文），而 `upsert` 会反复写同一条记录，所以随机 nonce
//!   是必须的，不是「够用就行」。
//!
//! ## 缺密钥 ⇒ **拒绝启动**，不是回落到默认值
//!
//! `ONTOK_SECRET_KEY`（兼容名 `ONTOK_DSN_KEY`）必须是 **64 个十六进制字符**（32 字节）。
//! 缺失 / 形状不对 ⇒ [`SecretCipher::from_env`] 报人话错误，`console serve` 在
//! **绑 socket 之前**退出。⛔ 没有「本地调试可以先跳过」的旁路 —— 那正是三周后
//! 没人记得它还开着的开关（与 `ONTOK_VIEW_TOKEN` 同一条纪律，见 `src/config.rs`）。
//!
//! # 已知边界（诚实登记）
//!
//! - 密钥轮换**不做**（demand §十五-5「保留 vs 放弃」明写放弃）：换密钥会让已有
//!   密文读不出来，而 v1 的处理方式是「重新 `PUT` 一次 DSN」/「`rotate` 一次 key」。
//! - 探测是**主动连库**：`database/test` 会真的建立一条到源库的连接（这正是它存在的
//!   意义 —— 让 UI 能区分「测过了但不通」），连接数与超时都收紧到 1 条 / 8 秒。

use std::time::{Duration, Instant};

use sqlx::postgres::PgPoolOptions;

// ══════════════════════════════════════════════════════════════
// 常量
// ══════════════════════════════════════════════════════════════

/// 密钥的环境变量名（**只从环境读，代码里不出现任何密钥值**）。
///
/// ⚠️ 名字里有 SECRET 而不是 DSN：v1 **只有一把密钥、一套 AEAD**，
/// 它同时保护两列密文 —— `project_databases.dsn_ciphertext`（源库口令）与
/// `project_keys.token_ciphertext`（可再次查看的 query key）。
/// ⛔ **不要为 key 再引第二套密钥体系**：两把密钥意味着两处轮换、两处失配、
/// 以及「换了一把之后哪条密文读不出来」这种没人能一眼回答的问题。
pub const SECRET_KEY_ENV: &str = "ONTOK_SECRET_KEY";

/// 兼容名（REQ-159 的第一版文档写的是这个名字）。读到它等价于上面那个。
///
/// ⚠️ 两个名字**同时设置且值不同 ⇒ 拒绝启动**：那种状态下「哪把才是对的」
/// 没有正确答案，猜一个会把另一条密文变成永久读不出来。
pub const LEGACY_SECRET_KEY_ENV: &str = "ONTOK_DSN_KEY";

/// 密钥长度（字节）。AES-256。
pub const SECRET_KEY_BYTES: usize = 32;

/// 密文前缀（版本位）。
pub const CIPHER_PREFIX: &str = "v1:";

/// 探测的**总**墙钟上界。超过就判 `db.connect.timeout`（不再等 sqlx 自己的超时）。
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(8);

/// 一次探测只开 1 条连接（这是探针，不是连接池）。
const PROBE_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(5);

// ══════════════════════════════════════════════════════════════
// 错误
// ══════════════════════════════════════════════════════════════

#[derive(Debug, thiserror::Error)]
pub enum DsnError {
    /// 启动期：缺 / 坏密钥。**这是拒绝启动的理由**，不是运行期错误。
    #[error(
        "环境变量 {SECRET_KEY_ENV} 不合法：{0}\n  \
         它必须是 {n} 个十六进制字符（{n} 位 = 32 字节 = AES-256 的密钥）。\n  \
         生成一个：`openssl rand -hex 32`（或任何能给出 32 字节随机数的工具）。\n  \
         ⛔ 不提供默认值 —— 默认密钥等于没有加密。",
        n = SECRET_KEY_BYTES * 2
    )]
    BadKey(String),

    #[error("DSN 不合法：{0}")]
    BadDsn(String),

    #[error(
        "密文读不出来（{0}）—— 它可能是用另一把 {SECRET_KEY_ENV} 加的密\
         （v1 只有一把密钥，换过密钥的库要重新写一次 DSN / key）"
    )]
    Cipher(String),
}

// ══════════════════════════════════════════════════════════════
// 加密列
// ══════════════════════════════════════════════════════════════

/// 元数据库里**密文列**的对称加密器（AES-256-GCM）。
///
/// # 它保护两列，不是一个
///
/// | 列 | 明文是什么 | 为什么需要可逆 |
/// | --- | --- | --- |
/// | `project_databases.dsn_ciphertext` | 源库连接串 | 测连通 / import 时要解出来用 |
/// | `project_keys.token_ciphertext` | 发给数字员工的 query key | admin 要能**再次查看**（AC-011） |
///
/// 两列用**同一把密钥、同一套算法**：v1 的纪律是「只有一套密钥体系」。
///
/// `Debug` 是手写的：⛔ 密钥本身**不许**出现在任何 `{:?}` 里
/// （日志、panic 消息、审计都可能顺手打一个结构体）。
#[derive(Clone)]
pub struct SecretCipher {
    key: [u8; SECRET_KEY_BYTES],
}

impl std::fmt::Debug for SecretCipher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretCipher(<redacted>)")
    }
}

impl SecretCipher {
    /// 从 32 字节密钥构造。
    pub fn from_key(key: [u8; SECRET_KEY_BYTES]) -> Self {
        Self { key }
    }

    /// 从 64 位十六进制字符串构造。
    pub fn from_hex(hex: &str) -> Result<Self, DsnError> {
        let t = hex.trim();
        if t.len() != SECRET_KEY_BYTES * 2 {
            return Err(DsnError::BadKey(format!(
                "长度是 {} 字符，要求 {}",
                t.len(),
                SECRET_KEY_BYTES * 2
            )));
        }
        let mut key = [0u8; SECRET_KEY_BYTES];
        for (i, byte) in key.iter_mut().enumerate() {
            let hi = hex_val(t.as_bytes()[i * 2]).ok_or_else(|| {
                DsnError::BadKey(format!("第 {} 个字符不是十六进制", i * 2 + 1))
            })?;
            let lo = hex_val(t.as_bytes()[i * 2 + 1]).ok_or_else(|| {
                DsnError::BadKey(format!("第 {} 个字符不是十六进制", i * 2 + 2))
            })?;
            *byte = (hi << 4) | lo;
        }
        Ok(Self { key })
    }

    /// **只从环境变量读**。缺失 / 空 / 形状不对 ⇒ 错误（调用方拒绝启动）。
    ///
    /// 认两个名字（[`SECRET_KEY_ENV`] 与兼容名 [`LEGACY_SECRET_KEY_ENV`]）：
    /// 两个都没设 ⇒ 缺密钥；都设了且**不一样** ⇒ 拒绝（不猜哪把对，猜错会让
    /// 另一条密文永久读不出来）。
    pub fn from_env() -> Result<Self, DsnError> {
        let read = |name: &str| {
            std::env::var(name)
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        match (read(SECRET_KEY_ENV), read(LEGACY_SECRET_KEY_ENV)) {
            (Some(a), Some(b)) if a != b => Err(DsnError::BadKey(format!(
                "{SECRET_KEY_ENV} 与 {LEGACY_SECRET_KEY_ENV} 都设了，而且**值不一样** —— \
                 拒绝启动：这时「哪把才是对的」没有正确答案，猜一个会让另一条密文永久读不出来。\
                 请只保留一个（推荐 {SECRET_KEY_ENV}）。"
            ))),
            (Some(v), _) | (None, Some(v)) => Self::from_hex(&v),
            (None, None) => Err(DsnError::BadKey(format!(
                "既没有设置 {SECRET_KEY_ENV}，也没有设置兼容名 {LEGACY_SECRET_KEY_ENV}"
            ))),
        }
    }

    /// [`Self::from_env`] 的**纯函数**内核（判据用它，避免测试去改进程环境）。
    pub fn from_lookup<F>(lookup: F) -> Result<Self, DsnError>
    where
        F: Fn(&str) -> Option<String>,
    {
        let read = |name: &str| {
            lookup(name)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        match (read(SECRET_KEY_ENV), read(LEGACY_SECRET_KEY_ENV)) {
            (Some(a), Some(b)) if a != b => Err(DsnError::BadKey(format!(
                "{SECRET_KEY_ENV} 与 {LEGACY_SECRET_KEY_ENV} 都设了，而且**值不一样** —— \
                 拒绝启动：这时「哪把才是对的」没有正确答案，猜一个会让另一条密文永久读不出来。\
                 请只保留一个（推荐 {SECRET_KEY_ENV}）。"
            ))),
            (Some(v), _) | (None, Some(v)) => Self::from_hex(&v),
            (None, None) => Err(DsnError::BadKey(format!(
                "既没有设置 {SECRET_KEY_ENV}，也没有设置兼容名 {LEGACY_SECRET_KEY_ENV}"
            ))),
        }
    }

    /// 测试 / 单测用：一把固定的密钥。
    ///
    /// ⛔ **生产路径不许调它**：它是一把写死的密钥（`[0x42; 32]`），
    /// 用它加密等于没有加密。生产路径只有 [`Self::from_env`]（缺密钥即拒绝启动）。
    /// 设成 `#[doc(hidden)]` 是为了让它不出现在公开文档的「可用 API」里 ——
    /// 它存在只是为了「测试能注入一把确定的密钥」，这一点本身是判据（AC-009/011）。
    #[doc(hidden)]
    pub fn for_tests() -> Self {
        Self::from_key([0x42u8; SECRET_KEY_BYTES])
    }

    fn aead(&self) -> Result<ring::aead::LessSafeKey, DsnError> {
        let unbound = ring::aead::UnboundKey::new(&ring::aead::AES_256_GCM, &self.key)
            .map_err(|_| DsnError::Cipher("AES-256-GCM 的密钥长度不被接受".to_string()))?;
        Ok(ring::aead::LessSafeKey::new(unbound))
    }

    /// 明文 → `v1:<hex>`。每条都换一个随机 nonce。
    pub fn encrypt(&self, plaintext: &str) -> Result<String, DsnError> {
        use ring::rand::SecureRandom;
        let mut nonce_bytes = [0u8; ring::aead::NONCE_LEN];
        ring::rand::SystemRandom::new()
            .fill(&mut nonce_bytes)
            .map_err(|_| DsnError::Cipher("拿不到随机数（SystemRandom 失败）".to_string()))?;
        let key = self.aead()?;
        let mut buf = plaintext.as_bytes().to_vec();
        key.seal_in_place_append_tag(
            ring::aead::Nonce::assume_unique_for_key(nonce_bytes),
            ring::aead::Aad::empty(),
            &mut buf,
        )
        .map_err(|_| DsnError::Cipher("加密失败".to_string()))?;
        let mut out = Vec::with_capacity(nonce_bytes.len() + buf.len());
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&buf);
        Ok(format!("{CIPHER_PREFIX}{}", hex_lower(&out)))
    }

    /// `v1:<hex>` → 明文。**失败就是失败**（⛔ 不回落到空串 / 默认 DSN）。
    pub fn decrypt(&self, stored: &str) -> Result<String, DsnError> {
        let hex = stored
            .trim()
            .strip_prefix(CIPHER_PREFIX)
            .ok_or_else(|| {
                DsnError::Cipher(format!(
                    "没有 {CIPHER_PREFIX} 前缀（这条密文不是本版本写的）"
                ))
            })?;
        let raw = hex_decode(hex)
            .map_err(|e| DsnError::Cipher(format!("十六进制解不开：{e}")))?;
        if raw.len() <= ring::aead::NONCE_LEN {
            return Err(DsnError::Cipher("密文太短（连 nonce 都不够）".to_string()));
        }
        let (nonce, ct) = raw.split_at(ring::aead::NONCE_LEN);
        let mut nonce_bytes = [0u8; ring::aead::NONCE_LEN];
        nonce_bytes.copy_from_slice(nonce);
        let key = self.aead()?;
        let mut buf = ct.to_vec();
        let plain = key
            .open_in_place(
                ring::aead::Nonce::assume_unique_for_key(nonce_bytes),
                ring::aead::Aad::empty(),
                &mut buf,
            )
            .map_err(|_| DsnError::Cipher("认证标签校验失败（密钥不对或密文被改）".to_string()))?;
        String::from_utf8(plain.to_vec())
            .map_err(|_| DsnError::Cipher("解出来的不是 UTF-8".to_string()))
    }

    /// 把一个可能含 DSN 的字符串里的敏感片段抹掉。
    ///
    /// 用在**所有**会把外部错误文本写进响应 / 日志的地方：sqlx 的错误绝大多数
    /// 只带地址，但「绝大多数」不够 —— 一旦某条错误带上了整条连接串，
    /// AC-009 就当场破了，而这种破法是**随机**出现的。
    pub fn scrub(&self, text: &str, dsn: &str) -> String {
        let mut out = text.to_string();
        if !dsn.is_empty() {
            out = out.replace(dsn, "<dsn>");
        }
        if let Ok(parts) = parse_dsn(dsn) {
            if let Some(pw) = parts.password.as_deref() {
                if pw.len() >= 3 {
                    out = out.replace(pw, "<redacted>");
                }
            }
        }
        out
    }
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn hex_decode(s: &str) -> Result<Vec<u8>, String> {
    if s.len() % 2 != 0 {
        return Err(format!("长度 {} 是奇数", s.len()));
    }
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len() / 2);
    for i in (0..b.len()).step_by(2) {
        let hi = hex_val(b[i]).ok_or_else(|| format!("第 {} 个字符不是十六进制", i + 1))?;
        let lo = hex_val(b[i + 1]).ok_or_else(|| format!("第 {} 个字符不是十六进制", i + 2))?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

// ══════════════════════════════════════════════════════════════
// DSN 解析
// ══════════════════════════════════════════════════════════════

/// 拆开的 DSN —— **只包含可以回显的字段** + 需要保密的 `password`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DsnParts {
    pub scheme: String,
    pub user: String,
    pub password: Option<String>,
    pub host: String,
    pub port: Option<u16>,
    pub database: String,
}

impl DsnParts {
    /// 回显用：⛔ 不带 password。
    pub fn display_host(&self) -> String {
        match self.port {
            Some(p) => format!("{}:{p}", self.host),
            None => self.host.clone(),
        }
    }
}

/// 解析 `postgresql://user:pass@host:5432/db[?params]`。
///
/// 只认 `postgres` / `postgresql` 两种 scheme：v1 的连通性探测走 sqlx 的 PG 驱动，
/// 认了 mysql 的 DSN 却测不了，等于给用户一个假承诺。
pub fn parse_dsn(dsn: &str) -> Result<DsnParts, DsnError> {
    let t = dsn.trim();
    if t.is_empty() {
        return Err(DsnError::BadDsn("是空字符串".to_string()));
    }
    let (scheme, rest) = t
        .split_once("://")
        .ok_or_else(|| DsnError::BadDsn("缺 `scheme://`（形如 postgresql://user:pass@host:5432/db）".to_string()))?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "postgresql" && scheme != "postgres" {
        return Err(DsnError::BadDsn(format!(
            "scheme 是 {scheme:?}，本服务端只认 postgresql / postgres"
        )));
    }

    // authority 到第一个 `/` 或 `?` 为止
    let (authority, tail) = match rest.find(['/', '?']) {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    if authority.is_empty() {
        return Err(DsnError::BadDsn("缺主机".to_string()));
    }

    let (userinfo, hostport) = match authority.rsplit_once('@') {
        Some((u, h)) => (Some(u), h),
        None => (None, authority),
    };
    let (user, password) = match userinfo {
        None | Some("") => (String::new(), None),
        Some(u) => match u.split_once(':') {
            Some((n, p)) => (pct_decode(n), Some(pct_decode(p))),
            None => (pct_decode(u), None),
        },
    };
    if user.is_empty() {
        return Err(DsnError::BadDsn(
            "缺用户名（形如 postgresql://user:pass@host:5432/db）".to_string(),
        ));
    }

    let (host, port) = split_host_port(hostport)?;

    let path = tail.strip_prefix('/').unwrap_or("");
    let raw_db = path.split(['?', '#']).next().unwrap_or("");
    if raw_db.is_empty() {
        return Err(DsnError::BadDsn(
            "缺库名（形如 postgresql://user:pass@host:5432/db）".to_string(),
        ));
    }
    let database = pct_decode(raw_db);

    Ok(DsnParts {
        scheme,
        user,
        password,
        host,
        port,
        database,
    })
}

fn split_host_port(hostport: &str) -> Result<(String, Option<u16>), DsnError> {
    if hostport.is_empty() {
        return Err(DsnError::BadDsn("缺主机".to_string()));
    }
    // IPv6：`[::1]:5432`
    if let Some(rest) = hostport.strip_prefix('[') {
        let (h, after) = rest
            .split_once(']')
            .ok_or_else(|| DsnError::BadDsn("IPv6 主机少了 `]`".to_string()))?;
        let port = match after.strip_prefix(':') {
            Some(p) if !p.is_empty() => Some(parse_port(p)?),
            Some(_) => None,
            None => None,
        };
        return Ok((h.to_string(), port));
    }
    match hostport.rsplit_once(':') {
        Some((h, p)) if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) => {
            Ok((h.to_string(), Some(parse_port(p)?)))
        }
        _ => Ok((hostport.to_string(), None)),
    }
}

fn parse_port(p: &str) -> Result<u16, DsnError> {
    p.parse::<u16>()
        .map_err(|_| DsnError::BadDsn(format!("端口 {p:?} 不是 1–65535 的整数")))
}

/// 只解 `%XX`（用户 / 口令 / 库名里可能出现 `@` `:` `/` 的转义形态）。
fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex_val(b[i + 1]), hex_val(b[i + 2])) {
                out.push((h << 4) | l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

// ══════════════════════════════════════════════════════════════
// 连通性探测
// ══════════════════════════════════════════════════════════════

/// 一次探测的失败（**分类**是 AC-005 的核心）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DsnFailure {
    /// `db.connect.auth` / `.timeout` / `.dns` / `.refused` / `.tls` / `.unknown`。
    pub code: &'static str,
    pub message: String,
}

/// 一次探测的结果（**成功或失败都返 200**，由这个结构区分）。
#[derive(Debug, Clone)]
pub struct DsnTest {
    pub ok: bool,
    pub latency_ms: u128,
    pub server_version: Option<String>,
    pub error: Option<DsnFailure>,
}

impl DsnTest {
    fn fail(started: Instant, f: DsnFailure) -> Self {
        Self {
            ok: false,
            latency_ms: started.elapsed().as_millis(),
            server_version: None,
            error: Some(f),
        }
    }
}

/// 真连一次：建 1 条连接 → `SELECT version()`。
///
/// # 为什么是 `SELECT version()`
///
/// 它同时回答三件事：连接真的建立了（不是「端口开着」）、认证过了、
/// 会话能执行 SQL。`SELECT 1` 少了最后一条的信息量（`version()` 的返回值
/// 还会回填到响应的 `server_version`）。
pub async fn probe(dsn: &str, cipher: &SecretCipher) -> DsnTest {
    let started = Instant::now();
    let parts = match parse_dsn(dsn) {
        Ok(p) => p,
        Err(e) => {
            return DsnTest::fail(
                started,
                DsnFailure {
                    code: "db.dsn.invalid",
                    message: cipher.scrub(&e.to_string(), dsn),
                },
            )
        }
    };

    // ⓪ **先解析名字**（再决定要不要连）。
    //
    // 为什么把 DNS 判定提到连接**之前**：
    // - 从 sqlx 的错误文本里猜「这是不是 DNS 问题」不可靠 —— 实测 Windows 上对
    //   `.invalid` 域名，池的 `acquire_timeout` 会先触发，报出来的是
    //   `pool timed out while waiting for an open connection`，一个 `dns` 都没有；
    // - 解析失败的**根本原因**在名字层，判在这里既准确又能立刻给出可行动的话；
    // - 解析成功的库不受影响（多一次本地解析，微秒级）。
    if let Err(why) = resolve_host(&parts.host, parts.port).await {
        return DsnTest::fail(
            started,
            DsnFailure {
                code: "db.connect.dns",
                message: cipher.scrub(
                    &format!(
                        "域名解析不出来：{}（{why}）—— 检查主机名 / DNS / 内网解析",
                        parts.host
                    ),
                    dsn,
                ),
            },
        );
    }

    let fut = async {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .acquire_timeout(PROBE_ACQUIRE_TIMEOUT)
            .connect(dsn)
            .await?;
        let row: (String,) = sqlx::query_as("SELECT version()").fetch_one(&pool).await?;
        pool.close().await;
        Ok::<(String,), sqlx::Error>(row)
    };

    match tokio::time::timeout(PROBE_TIMEOUT, fut).await {
        Ok(Ok((version,))) => DsnTest {
            ok: true,
            latency_ms: started.elapsed().as_millis(),
            server_version: Some(short_version(&version)),
            error: None,
        },
        Ok(Err(e)) => {
            // ① 认证：SQLSTATE 28xxx 最可靠，先判它。
            if is_auth_error(&e) {
                return DsnTest::fail(
                    started,
                    DsnFailure {
                        code: "db.connect.auth",
                        message: cipher.scrub(
                            &format!(
                                "认证失败：用 {} 连 {} 时用户名或口令不对（{e}）",
                                parts.user,
                                parts.display_host()
                            ),
                            dsn,
                        ),
                    },
                );
            }
            // ② 其余按错误链分类（超时 / 拒绝 / TLS / 未知）。
            //    DNS 已在 ⓪ 步判过，这里不必再猜。
            DsnTest::fail(started, classify(&e, cipher, dsn, &parts))
        }
        Err(_) => DsnTest::fail(
            started,
            DsnFailure {
                code: "db.connect.timeout",
                message: format!(
                    "连 {} 超过 {} 秒没有响应（网络 / 防火墙 / 白名单 / 库没起来）。",
                    parts.display_host(),
                    PROBE_TIMEOUT.as_secs()
                ),
            },
        ),
    }
}

/// 这个名字解析得出来吗？`Ok(())` = 能；`Err(为什么)` = 不能。
///
/// ⚠️ **解析器不答话也算「解析不出来」**（`db.connect.dns`，消息里写明是超时）：
/// 对运维来说「名字没解析出来」与「解析器 5 秒没给答案」是同一层的问题，
/// 处理动作也一样（查主机名 / DNS / 内网解析）。⛔ 不把它报成「连不上库」——
/// 那会把人引到错误的排查方向（防火墙 / 库没起）。
async fn resolve_host(host: &str, port: Option<u16>) -> Result<(), String> {
    let target = if host.contains(':') {
        format!("[{}]:{}", host, port.unwrap_or(5432))
    } else {
        format!("{}:{}", host, port.unwrap_or(5432))
    };
    let join = tokio::task::spawn_blocking(move || {
        use std::net::ToSocketAddrs;
        match target.to_socket_addrs() {
            Ok(mut it) => match it.next() {
                Some(_) => Ok(()),
                None => Err("解析结果为空".to_string()),
            },
            Err(e) => Err(e.to_string()),
        }
    });
    const RESOLVE_TIMEOUT: Duration = Duration::from_secs(5);
    match tokio::time::timeout(RESOLVE_TIMEOUT, join).await {
        Ok(Ok(r)) => r,
        // 线程被取消 / panic：不猜，交给连接去报（那时分类会是 unknown/timeout）
        Ok(Err(_)) => Ok(()),
        Err(_) => Err(format!(
            "解析器 {} 秒内没有给出答案",
            RESOLVE_TIMEOUT.as_secs()
        )),
    }
}

/// 认证类错误（PG 的 `28xxx` 一族 + 文本兜底）。
fn is_auth_error(e: &sqlx::Error) -> bool {
    if let sqlx::Error::Database(d) = e {
        if matches!(d.code().as_deref(), Some("28P01") | Some("28000")) {
            return true;
        }
    }
    let mut chain = String::new();
    let mut cur: Option<&(dyn std::error::Error + 'static)> = Some(e);
    let mut guard = 0;
    while let Some(err) = cur {
        chain.push_str(&err.to_string());
        chain.push(' ');
        cur = err.source();
        guard += 1;
        if guard > 8 {
            break;
        }
    }
    let low = chain.to_ascii_lowercase();
    low.contains("password authentication failed")
        || low.contains("28p01")
        || low.contains("invalid_password")
        || low.contains("authentication failed")
}

/// `PostgreSQL 15.4 on x86_64-pc-linux-gnu, compiled by …` → `PostgreSQL 15.4`
/// （第一行 + 第一个 ` on ` / 逗号之前：完整串带编译机与编译器，对 UI 没有信息量）。
fn short_version(raw: &str) -> String {
    let first = raw.lines().next().unwrap_or(raw).trim();
    let head = first.split(',').next().unwrap_or(first).trim();
    let head = head.split(" on ").next().unwrap_or(head).trim();
    head.to_string()
}

/// 错误分类（AC-005：auth / timeout / dns 必须能分开）。
fn classify(e: &sqlx::Error, cipher: &SecretCipher, dsn: &str, parts: &DsnParts) -> DsnFailure {
    // 整条错误链的小写文本 —— sqlx 的根因经常藏在 source 里。
    let mut chain = String::new();
    let mut cur: Option<&(dyn std::error::Error + 'static)> = Some(e);
    let mut guard = 0;
    while let Some(err) = cur {
        chain.push_str(&err.to_string());
        chain.push(' ');
        cur = err.source();
        guard += 1;
        if guard > 8 {
            break;
        }
    }
    let low = chain.to_ascii_lowercase();
    let msg = |tail: &str| cipher.scrub(&format!("{tail}（{chain}）"), dsn);

    // ① 认证：PG 的 SQLSTATE 优先（28P01 = invalid_password，28000 = invalid_authorization）。
    if let sqlx::Error::Database(d) = e {
        if matches!(d.code().as_deref(), Some("28P01") | Some("28000")) {
            return DsnFailure {
                code: "db.connect.auth",
                message: msg(&format!(
                    "认证失败：用 {} 连 {} 时用户名或口令不对",
                    parts.user,
                    parts.display_host()
                )),
            };
        }
    }
    if low.contains("password authentication failed")
        || low.contains("28p01")
        || low.contains("invalid_password")
        || low.contains("authentication failed")
    {
        return DsnFailure {
            code: "db.connect.auth",
            message: msg(&format!(
                "认证失败：用 {} 连 {} 时用户名或口令不对",
                parts.user,
                parts.display_host()
            )),
        };
    }

    // ② 超时
    if low.contains("timed out") || low.contains("timeout") || low.contains("pooltimedout") {
        return DsnFailure {
            code: "db.connect.timeout",
            message: msg(&format!(
                "连接 {} 超时（网络不通 / 防火墙 / 库没起来）",
                parts.display_host()
            )),
        };
    }

    // ③ DNS（Windows 11001 / Linux "Name or service not known"）
    if low.contains("dns")
        || low.contains("lookup address")
        || low.contains("name or service not known")
        || low.contains("no such host")
        || low.contains("11001")
        || low.contains("nodename nor servname")
        || low.contains("failed to lookup")
    {
        return DsnFailure {
            code: "db.connect.dns",
            message: msg(&format!(
                "域名解析不出来：{}（检查主机名 / DNS / 内网解析）",
                parts.host
            )),
        };
    }

    // ④ 拒绝（端口没开 / 服务没起）
    if low.contains("connection refused")
        || low.contains("actively refused")
        || low.contains("10061")
        || low.contains("econnrefused")
    {
        return DsnFailure {
            code: "db.connect.refused",
            message: msg(&format!(
                "{} 拒绝了连接（端口没开 / 库没起来 / 只监听本机）",
                parts.display_host()
            )),
        };
    }

    // ⑤ TLS / 证书
    if low.contains("certificate") || low.contains("tls") || low.contains("ssl") {
        return DsnFailure {
            code: "db.connect.tls",
            message: msg(&format!(
                "与 {} 的 TLS 握手失败（证书 / 版本不匹配）",
                parts.display_host()
            )),
        };
    }

    // ⑥ 库不存在（3D000）/ 其它 —— 分类到 unknown，但原文保留（已 scrub）。
    DsnFailure {
        code: "db.connect.unknown",
        message: msg("连不上源库"),
    }
}

// ══════════════════════════════════════════════════════════════
// 判据
// ══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── 解析 ────────────────────────────────────────────────

    #[test]
    fn parse_a_normal_dsn_and_never_expose_the_password_in_host() {
        let p = parse_dsn("postgresql://alice:s3cret@db.internal:5432/orders?sslmode=require")
            .expect("合法");
        assert_eq!(p.scheme, "postgresql");
        assert_eq!(p.user, "alice");
        assert_eq!(p.password.as_deref(), Some("s3cret"));
        assert_eq!(p.host, "db.internal");
        assert_eq!(p.port, Some(5432));
        assert_eq!(p.database, "orders");
        assert_eq!(p.display_host(), "db.internal:5432");
        assert!(!format!("{p:?}").contains("db.internal:5432/alice"));
    }

    #[test]
    fn parse_handles_ipv6_and_percent_escapes() {
        let p = parse_dsn("postgres://u%40corp:p%3Ass@[::1]:6543/db").expect("合法");
        assert_eq!(p.user, "u@corp");
        assert_eq!(p.password.as_deref(), Some("p:ss"));
        assert_eq!(p.host, "::1");
        assert_eq!(p.port, Some(6543));

        let p = parse_dsn("postgresql://u@host/db").expect("无端口也算合法");
        assert_eq!(p.port, None);
        assert_eq!(p.password, None);
    }

    #[test]
    fn parse_rejects_the_wrong_scheme_and_missing_pieces() {
        for bad in [
            "mysql://u:p@h:3306/db",
            "postgresql://u:p@h:5432",
            "postgresql:///db",
            "",
            "not-a-dsn",
            "postgresql://h:5432/db",           // 缺用户名
            "postgresql://u:p@h:notaport/db",   // 端口不是数字 ⇒ 当成主机名的一部分
        ] {
            let got = parse_dsn(bad);
            if bad.contains("notaport") {
                // 端口不是数字时按「主机名里带冒号」处理，不是错误 —— 这里只是确认不 panic。
                continue;
            }
            assert!(got.is_err(), "{bad:?} 应当被拒：{got:?}");
        }
    }

    // ── 加密列 ──────────────────────────────────────────────

    #[test]
    fn encrypt_decrypt_roundtrip_and_ciphertext_does_not_contain_plaintext() {
        let c = SecretCipher::from_hex(&"a1".repeat(32)).expect("合法密钥");
        let dsn = "postgresql://alice:s3cret@db.internal:5432/orders";
        let ct = c.encrypt(dsn).expect("加密");
        assert!(ct.starts_with(CIPHER_PREFIX), "{ct}");
        assert!(!ct.contains("s3cret"), "密文里出现了明文口令：{ct}");
        assert!(!ct.contains("db.internal"), "密文里出现了主机：{ct}");
        let back = c.decrypt(&ct).expect("解密");
        assert_eq!(back, dsn);
    }

    #[test]
    fn the_same_plaintext_encrypts_to_different_ciphertexts() {
        let c = SecretCipher::for_tests();
        let a = c.encrypt("postgresql://u:p@h/db").unwrap();
        let b = c.encrypt("postgresql://u:p@h/db").unwrap();
        assert_ne!(a, b, "nonce 必须每条随机（GCM 下固定 nonce 会毁掉机密性）");
        assert_eq!(c.decrypt(&a).unwrap(), c.decrypt(&b).unwrap());
    }

    #[test]
    fn a_wrong_key_or_tampered_ciphertext_fails_loudly() {
        let a = SecretCipher::from_hex(&"11".repeat(32)).unwrap();
        let b = SecretCipher::from_hex(&"22".repeat(32)).unwrap();
        let ct = a.encrypt("postgresql://u:p@h/db").unwrap();
        let e = b.decrypt(&ct).unwrap_err();
        assert!(matches!(e, DsnError::Cipher(_)), "{e}");

        // 改一位十六进制 ⇒ 认证标签必须失败（⛔ 不静默返回半截明文）
        let mut t = ct.clone();
        let last = t.pop().unwrap();
        t.push(if last == 'a' { 'b' } else { 'a' });
        assert!(a.decrypt(&t).is_err());

        // 没有版本前缀 ⇒ 明确报错
        assert!(a.decrypt("deadbeef").is_err());
    }

    #[test]
    fn a_bad_key_is_rejected_with_a_human_message() {
        for bad in ["", "abc", &"zz".repeat(32)] {
            let e = SecretCipher::from_hex(bad).unwrap_err();
            let msg = e.to_string();
            assert!(msg.contains(SECRET_KEY_ENV), "错误里要点名环境变量：{msg}");
        }
        // 缺环境变量也必须是一个**可读**的错误（不是 panic、不是默认密钥）。
        // 用纯函数内核判，⛔ 不去 `set_var`/`remove_var`（那会污染同进程里的并行测试）。
        let e = SecretCipher::from_lookup(|_| None).unwrap_err();
        assert!(e.to_string().contains("不提供默认值"), "{e}");
        // 两个名字都设了且不一样 ⇒ 拒绝（不猜哪把对）
        let e = SecretCipher::from_lookup(|n| {
            Some(match n {
                SECRET_KEY_ENV => "11".repeat(32),
                _ => "22".repeat(32),
            })
        })
        .unwrap_err();
        assert!(e.to_string().contains("值不一样"), "{e}");
        // 只设兼容名 ⇒ 照收（REQ-159 第一版文档用的是那个名字）
        let c = SecretCipher::from_lookup(|n| {
            (n == LEGACY_SECRET_KEY_ENV).then(|| "33".repeat(32))
        })
        .expect("兼容名应当被接受");
        assert_eq!(c.decrypt(&c.encrypt("x").unwrap()).unwrap(), "x");
    }

    #[test]
    fn debug_never_prints_the_key() {
        let c = SecretCipher::from_hex(&"9f".repeat(32)).unwrap();
        let s = format!("{c:?}");
        assert!(!s.contains("9f9f"), "{s}");
        assert!(s.contains("redacted"), "{s}");
    }

    #[test]
    fn scrub_removes_the_dsn_and_the_password_from_foreign_error_text() {
        let c = SecretCipher::for_tests();
        let dsn = "postgresql://alice:s3cret@db.internal:5432/orders";
        // ① 整条 DSN 出现在外部错误文本里 ⇒ 整段换成 `<dsn>`（连 host 一起走了，
        //    这是**更强**的处置：AC-009 要求的是「搜不到 DSN 明文」）。
        let text = format!("error connecting to {dsn}: password s3cret rejected");
        let out = c.scrub(&text, dsn);
        assert!(!out.contains(dsn), "{out}");
        assert!(!out.contains("s3cret"), "{out}");
        // ② 只有口令片段（没有整条 DSN）时也必须抹掉，host 可以留 ——
        //    demand §五-4 允许日志写 host/db，⛔ 但绝不许带口令。
        let out = c.scrub("auth failed for user alice with s3cret", dsn);
        assert!(!out.contains("s3cret"), "{out}");
        assert!(out.contains("alice"), "用户名不是秘密：{out}");
    }

    #[test]
    fn short_version_keeps_only_the_product_line() {
        assert_eq!(
            short_version("PostgreSQL 15.4 on x86_64-pc-linux-gnu, compiled by gcc 12"),
            "PostgreSQL 15.4"
        );
        assert_eq!(short_version("PostgreSQL 16.2"), "PostgreSQL 16.2");
    }

    // ── 分类（不需要真库：喂合成的 sqlx 错误）──────────────

    fn parts() -> DsnParts {
        parse_dsn("postgresql://alice:s3cret@db.internal:5432/orders").unwrap()
    }

    fn io_err(kind: std::io::ErrorKind, msg: &str) -> sqlx::Error {
        sqlx::Error::Io(std::io::Error::new(kind, msg))
    }

    #[test]
    fn dns_errors_classify_as_dns() {
        let c = SecretCipher::for_tests();
        let e = io_err(
            std::io::ErrorKind::Other,
            "dns error: failed to lookup address information: Name or service not known",
        );
        let f = classify(&e, &c, "postgresql://alice:s3cret@nohost:5432/orders", &parts());
        assert_eq!(f.code, "db.connect.dns", "{}", f.message);
    }

    #[test]
    fn timeout_errors_classify_as_timeout() {
        let c = SecretCipher::for_tests();
        let e = io_err(std::io::ErrorKind::TimedOut, "operation timed out");
        let f = classify(&e, &c, "postgresql://a:b@h:5432/d", &parts());
        assert_eq!(f.code, "db.connect.timeout", "{}", f.message);
    }

    #[test]
    fn refused_errors_classify_as_refused() {
        let c = SecretCipher::for_tests();
        let e = io_err(
            std::io::ErrorKind::ConnectionRefused,
            "No connection could be made because the target machine actively refused it. (os error 10061)",
        );
        let f = classify(&e, &c, "postgresql://a:b@h:5432/d", &parts());
        assert_eq!(f.code, "db.connect.refused", "{}", f.message);
    }

    #[test]
    fn auth_errors_classify_as_auth() {
        let c = SecretCipher::for_tests();
        let e = io_err(
            std::io::ErrorKind::Other,
            "password authentication failed for user \"alice\"",
        );
        let f = classify(&e, &c, "postgresql://a:b@h:5432/d", &parts());
        assert_eq!(f.code, "db.connect.auth", "{}", f.message);
        assert!(!f.message.contains("s3cret"), "分类消息里不许出现口令：{}", f.message);
    }
}

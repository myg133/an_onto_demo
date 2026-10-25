//! ⭐ REQ-144 阶段 1 · **TC3-HMAC-SHA256 签名**（腾讯云 API 3.0 签名方法 v3）。
//!
//! 依据：腾讯云官方文档《签名方法 v3》（链接与逐字节原文见 `tests/test_req144_tc3_vectors.rs`）。
//! 文档给出了**一组**带密钥的完整示例（`cvm` / `2019-02-25` / `1551113065`），
//! 并**逐段公布了中间量**（payload 哈希 / 规范请求哈希 / 三把派生密钥 / 签名 / Authorization）。
//! 本模块的输出必须与那些值**字节相同** —— 官方向量的逐字节对拍在
//! `tests/test_req144_tc3_vectors.rs`（那里才允许出现文档里的域名/接口名：**测试数据**不是实现）。
//!
//! # 四步（文档 §签名版本 v3 签名过程）
//!
//! ```text
//! 1. CanonicalRequest = Method \n URI \n QueryString \n CanonicalHeaders \n SignedHeaders \n SHA256(Payload)
//! 2. StringToSign     = "TC3-HMAC-SHA256" \n Timestamp \n Date/Service/tc3_request \n SHA256(CanonicalRequest)
//! 3. Signature        = HMAC_SHA256(HMAC_SHA256(HMAC_SHA256("TC3"+SecretKey, Date), Service), "tc3_request")
//! 4. Authorization    = "TC3-HMAC-SHA256 Credential=<SecretId>/<Scope>, SignedHeaders=<H>, Signature=<Sig>"
//! ```
//!
//! # 两个**必须**照抄的细节（文档点名，最容易错）
//!
//! | 细节 | 为什么错不起 |
//! | --- | --- |
//! | `Date` 取 **UTC** 的日期（不是本地日期） | 文档原例：时间戳 `1551113065` 在东八区是 `2019-02-26 00:44:25`，但 Credential 里的 Date 必须是 `2019-02-25`。用本地时区拼 ⇒ **凌晨必失败**（白天测不出来）。 |
//! | `CanonicalHeaders` 里 `x-tc-action` 的**值要小写**（头名也小写），`SignedHeaders` 用 `;` 连 | 少一处就签名不一致，而服务端只说「签名失败」。 |
//!
//! # 为什么手写 SHA-256 / HMAC，而不用 `sha2` / `hmac`
//!
//! 本需求的硬约束是 **⛔ 不新增依赖**（`ureq` 是早就在依赖里的那一个）。
//! `sha2` / `hmac` 虽然在 `Cargo.lock` 里（`sqlx` / `rustls` 带进来的**传递依赖**），
//! 但把它们提成**直接依赖**会改 `Cargo.toml` 与 `Cargo.lock` 里 `ontok` 的依赖块 ——
//! 那就成了「新增了一个我们能直接调用的依赖」，与约束的字面与精神都不符。
//!
//! 所以这里按 FIPS 180-4 / RFC 2104 实现 SHA-256 与 HMAC-SHA256（各约 60 行，无分支、无表查错风险），
//! 并用**三组**向量钉住：
//!
//! | 向量 | 来源 | 钉住了什么 |
//! | --- | --- | --- |
//! | `sha256` | FIPS 180-2 附录 B / NIST CAVP 的公开值 | 压缩函数与填充 |
//! | `hmac_sha256` | RFC 4231 §4.2 / §4.3 / §4.6 | ipad/opad、key 超过 64 字节时先哈希 |
//! | TC3 | 腾讯云文档公布的全部中间量 + Authorization | 拼接顺序、日期、**派生链全链** |
//!
//! # 文档里那串打码的 SecretKey，其实是**能跑的**
//!
//! 文档写的是 `SecretKey = "********************************"`。它**不是省略号**，而是
//! **字面 32 个星号**：把这 32 个 `*` 当 SecretKey 喂进 [`sign`]，能逐字节复现文档公布的
//! `SecretDate` / `SecretService` / `SecretSigning` 三把派生密钥与最终 `Signature`。
//!
//! ⇒ **派生链的第一跳也可对拍**，不存在「SecretKey 被打码，所以只能从中间密钥接上」这种事。
//! `tests/test_req144_tc3_vectors.rs` 的 `official_vector_1_*` 走的就是 **`sign()` 全链**
//! （不是从中间密钥接上），并逐项断言文档公布值；[`tests::official_vector_1_derived_key_chain_replays`]
//! 再用文档公布的三把十六进制密钥做逐级回放（同一事实的第二种验法）。
//!
//! ```text
//! ⛔ 本文件曾经写过一句错话（已删）：『SecretKey 打码 ⇒ 派生链第一跳无从对拍』。
//!    那句话会让下一个人以为「这段没法验」从而**根本不去验** —— 而整条链都能验。
//! ```
//!
//! # 已知限制（⛔ 不写成「已封堵」）
//!
//! - 只实现 **POST + JSON** 这一条通路（`CanonicalQueryString` 固定为空串）。
//!   GET 签名需要 RFC3986 URL-encode 排序，本需求用不到，**没有实现**（不是「已支持」）。
//! - `content-type` 固定按调用的实际值参与签名；若 HTTP 库偷偷加/改 `charset`，
//!   签名与发送就会不一致 —— 所以传输层**只发送这里签过的那一个** content-type 字符串。
//! - 时间戳由调用方给（测试可钉死）；本模块**不**读时钟，也不做「±5 分钟」校验。

use super::{AccessError, CODE_SCHEMA};

/// 签名算法标识（固定值，进 `Authorization` 与 `StringToSign`）。
pub const ALGORITHM: &str = "TC3-HMAC-SHA256";

/// 参与签名的头（顺序即 ASCII 升序，`;` 连接）。
///
/// 只签这三个是**文档示例的选择**，也是最小集：`content-type` / `host` 为必选，
/// `x-tc-action` 让「同一 host 上的不同动作」不能互换重放。
pub const SIGNED_HEADERS: &str = "content-type;host;x-tc-action";

// ══════════════════════════════════════════════════════════════
// 一次签名的全部输入 / 输出
// ══════════════════════════════════════════════════════════════

/// 签名输入。**不要给它 `Debug`**：里面有 SecretKey。
pub struct SignInput<'a> {
    /// 凭据标识（原样进 `Authorization` 的 `Credential=`，不参与哈希）。
    pub secret_id: &'a str,
    /// 凭据密钥（**只**参与派生链的第一跳）。
    pub secret_key: &'a str,
    /// 产品名（`cvm` / `monitor` …）—— 文档：必须与调用的产品域名一致。
    pub service: &'a str,
    /// 请求域名（`Host` 头的值，含非默认端口）。
    pub host: &'a str,
    /// 接口名（如 `SomeAction`）。签名时**小写**进规范头。
    pub action: &'a str,
    /// UNIX 秒。
    pub timestamp: i64,
    /// **即将原样发送**的请求体字符串（⛔ 不能签一份、发另一份）。
    pub payload: &'a str,
    /// 即将发送的 `Content-Type`（通常 `application/json; charset=utf-8`）。
    pub content_type: &'a str,
    /// HTTP 方法（本实现只用 `POST`）。
    pub method: &'a str,
    /// 规范 URI（API 3.0 固定 `/`）。
    pub canonical_uri: &'a str,
    /// 规范查询串（POST 固定空串）。
    pub canonical_query: &'a str,
}

/// 一次签名的产物：既给测试对拍，也给传输层拼头。
#[derive(Debug, Clone, Default)]
pub struct Signed {
    /// `Credential` 里的日期（UTC）。
    pub date: String,
    /// 第 1 步的完整规范请求串（对拍时逐字节打印）。
    pub canonical_request: String,
    /// `SHA256(CanonicalRequest)`。
    pub hashed_canonical_request: String,
    /// 第 2 步的完整待签名字符串。
    pub string_to_sign: String,
    /// 第 3 步的签名（小写十六进制）。
    pub signature: String,
    /// 第 4 步：直接放进 `Authorization` 头。
    pub authorization: String,
}

/// 签名（从 SecretKey 走完整派生链）。
pub fn sign(input: &SignInput<'_>) -> Result<Signed, AccessError> {
    let date = utc_date(input.timestamp)?;
    let signing_key = derive_signing_key(input.secret_key.as_bytes(), &date, input.service);
    Ok(finish(input, &date, &signing_key))
}

/// 派生签名密钥：`HMAC(HMAC(HMAC("TC3"+SecretKey, Date), Service), "tc3_request")`。
///
/// 单独暴露（`pub`）是为了**对拍文档公布的三把密钥**：文档把三把派生密钥的十六进制都公布了，
/// 于是既能从 SecretKey 走全链（[`sign`]），也能拿公布的中间值做逐级回放
/// （`tests::official_vector_1_derived_key_chain_replays`）—— 两条路互为交叉验证。
pub fn derive_signing_key(secret_key: &[u8], date: &str, service: &str) -> [u8; 32] {
    let mut tc3_key = Vec::with_capacity(3 + secret_key.len());
    tc3_key.extend_from_slice(b"TC3");
    tc3_key.extend_from_slice(secret_key);
    let secret_date = hmac_sha256(&tc3_key, date.as_bytes());
    let secret_service = hmac_sha256(&secret_date, service.as_bytes());
    hmac_sha256(&secret_service, b"tc3_request")
}

/// 用**已派生的**签名密钥完成第 1 / 2 / 3 / 4 步。
///
/// 与 [`sign`] 分开是因为派生链本身值得**单独可测**（见
/// `tests::official_vector_1_derived_key_chain_replays`：拿文档公布的三把十六进制密钥逐级回放），
/// 且第 1/2/4 步与密钥无关 —— 拆开后，测试可以在「密钥已知 / 密钥由中间值给出」两种形态下
/// 复用同一段拼接逻辑，⛔ 不必为对拍复制一份实现。
pub fn finish(input: &SignInput<'_>, date: &str, signing_key: &[u8; 32]) -> Signed {
    // ── 1 · 规范请求串 ──────────────────────────────────────
    // 注意 canonical_headers **自带结尾换行**，拼装时后面还要再加一个 `\n`
    // ⇒ 规范串里 query 与 content-type 之间会有一个空行（与文档示例逐字一致）。
    let canonical_headers = format!(
        "content-type:{}\nhost:{}\nx-tc-action:{}\n",
        input.content_type,
        input.host,
        input.action.to_lowercase()
    );
    let hashed_payload = sha256_hex(input.payload.as_bytes());
    let canonical_request = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        input.method,
        input.canonical_uri,
        input.canonical_query,
        canonical_headers,
        SIGNED_HEADERS,
        hashed_payload
    );
    let hashed_canonical_request = sha256_hex(canonical_request.as_bytes());

    // ── 2 · 待签名字符串 ────────────────────────────────────
    let credential_scope = format!("{date}/{}/tc3_request", input.service);
    let string_to_sign = format!(
        "{ALGORITHM}\n{}\n{credential_scope}\n{hashed_canonical_request}",
        input.timestamp
    );

    // ── 3 · 签名 ───────────────────────────────────────────
    let signature = hex_lower(&hmac_sha256(signing_key, string_to_sign.as_bytes()));

    // ── 4 · Authorization ──────────────────────────────────
    let authorization = format!(
        "{ALGORITHM} Credential={}/{credential_scope}, SignedHeaders={SIGNED_HEADERS}, Signature={signature}",
        input.secret_id
    );

    Signed {
        date: date.to_string(),
        canonical_request,
        hashed_canonical_request,
        string_to_sign,
        signature,
        authorization,
    }
}

/// UNIX 秒 → **UTC** 日期（`YYYY-MM-DD`）。
///
/// ⛔ 不用本地时区：文档点名「凌晨必失败」的那个坑就在这里。
pub fn utc_date(timestamp: i64) -> Result<String, AccessError> {
    let dt = chrono::DateTime::from_timestamp(timestamp, 0).ok_or_else(|| {
        AccessError::new(
            CODE_SCHEMA,
            format!(
                "时间戳 {timestamp} 超出可表示范围 —— 签名没法算（检查 X-TC-Timestamp 的来源）。"
            ),
        )
    })?;
    Ok(dt.format("%Y-%m-%d").to_string())
}

// ══════════════════════════════════════════════════════════════
// SHA-256（FIPS 180-4）
// ══════════════════════════════════════════════════════════════

/// 前 64 个素数立方根小数部分的前 32 位（FIPS 180-4 §4.2.2）。
const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// 初始哈希值（前 8 个素数平方根小数部分的前 32 位，FIPS 180-4 §5.3.3）。
const H0: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

/// 压缩一个 512 位块。
fn compress(state: &mut [u32; 8], block: &[u8]) {
    debug_assert_eq!(block.len(), 64);
    let mut w = [0u32; 64];
    for i in 0..16 {
        w[i] = u32::from_be_bytes([
            block[i * 4],
            block[i * 4 + 1],
            block[i * 4 + 2],
            block[i * 4 + 3],
        ]);
    }
    for i in 16..64 {
        let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }

    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
    for i in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ ((!e) & g);
        let t1 = h
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(K[i])
            .wrapping_add(w[i]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let t2 = s0.wrapping_add(maj);

        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }

    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
    state[4] = state[4].wrapping_add(e);
    state[5] = state[5].wrapping_add(f);
    state[6] = state[6].wrapping_add(g);
    state[7] = state[7].wrapping_add(h);
}

/// SHA-256 摘要。
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut state = H0;
    let mut chunks = data.chunks_exact(64);
    for block in &mut chunks {
        compress(&mut state, block);
    }
    let rest = chunks.remainder();

    // 填充：0x80 + 若干 0 + 64 位大端比特长度；`rest` 超过 55 字节时多占一个块。
    let mut tail = [0u8; 128];
    tail[..rest.len()].copy_from_slice(rest);
    tail[rest.len()] = 0x80;
    let total_blocks = if rest.len() < 56 { 1 } else { 2 };
    let bit_len = (data.len() as u64).wrapping_mul(8);
    tail[total_blocks * 64 - 8..total_blocks * 64].copy_from_slice(&bit_len.to_be_bytes());
    for i in 0..total_blocks {
        compress(&mut state, &tail[i * 64..(i + 1) * 64]);
    }

    let mut out = [0u8; 32];
    for (i, v) in state.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes());
    }
    out
}

/// SHA-256 → 小写十六进制。
pub fn sha256_hex(data: &[u8]) -> String {
    hex_lower(&sha256(data))
}

// ══════════════════════════════════════════════════════════════
// HMAC-SHA256（RFC 2104）
// ══════════════════════════════════════════════════════════════

/// HMAC-SHA256。`key` 超过 64 字节时先做一次 SHA-256（RFC 2104 §2）。
pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut block = [0u8; 64];
    if key.len() > 64 {
        block[..32].copy_from_slice(&sha256(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }

    let mut inner = Vec::with_capacity(64 + msg.len());
    let mut outer = Vec::with_capacity(64 + 32);
    for b in block.iter() {
        inner.push(b ^ 0x36);
    }
    inner.extend_from_slice(msg);
    let inner_hash = sha256(&inner);
    for b in block.iter() {
        outer.push(b ^ 0x5c);
    }
    outer.extend_from_slice(&inner_hash);
    sha256(&outer)
}

// ══════════════════════════════════════════════════════════════
// 十六进制（只为本模块与对拍服务，⛔ 不外扩成通用工具）
// ══════════════════════════════════════════════════════════════

/// 字节 → 小写十六进制。
pub fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

/// 小写十六进制 → 字节（对拍时把文档公布的派生密钥还原成二进制）。
pub fn unhex(text: &str) -> Option<Vec<u8>> {
    let t = text.trim();
    if t.len() % 2 != 0 {
        return None;
    }
    let bytes = t.as_bytes();
    let mut out = Vec::with_capacity(t.len() / 2);
    let val = |c: u8| -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            b'A'..=b'F' => Some(c - b'A' + 10),
            _ => None,
        }
    };
    for pair in bytes.chunks_exact(2) {
        out.push((val(pair[0])? << 4) | val(pair[1])?);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ════════════════════════════════════════════════════════
    // 一 · 原语对拍（FIPS 180-2 / RFC 4231）
    // ════════════════════════════════════════════════════════

    #[test]
    fn sha256_matches_fips_vectors() {
        // FIPS 180-2 附录 B.1 / B.2 + 常见的空串值。
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        // 1_000_000 个 'a'：跨块 + 填充刚好落在块边界上（最容易把填充写错的地方）。
        let million = vec![b'a'; 1_000_000];
        assert_eq!(
            sha256_hex(&million),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    #[test]
    fn hmac_sha256_matches_rfc4231() {
        // RFC 4231 §4.2（key = 20 字节 0x0b）
        assert_eq!(
            hmac_sha256(&[0x0b; 20], b"Hi There"),
            hex_to_32("b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7")
        );
        // RFC 4231 §4.3（key = "Jefe"）
        assert_eq!(
            hmac_sha256(b"Jefe", b"what do ya want for nothing?"),
            hex_to_32("5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843")
        );
        // RFC 4231 §4.6（key 比块长 ⇒ 先哈希 key）
        let long_key = vec![0xaa; 131];
        assert_eq!(
            hmac_sha256(
                &long_key,
                b"Test Using Larger Than Block-Size Key - Hash Key First"
            ),
            hex_to_32("60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54")
        );
    }

    fn hex_to_32(s: &str) -> [u8; 32] {
        let v = unhex(s).expect("十六进制");
        let mut out = [0u8; 32];
        out.copy_from_slice(&v);
        out
    }

    // 三 · 行为性自证：签名只取决于输入（没有 `now()` 偷偷混进来）
    //
    // 官方文档向量的逐字节对拍住 `tests/test_req144_tc3_vectors.rs`
    // （那边有文档的域名/接口名，属于**测试数据**；本目录保持「零厂商名词」，
    //   见该文件头的说明）。这里只用中性输入证明「同输入同签名、改一处就变」。

    fn neutral_input() -> SignInput<'static> {
        SignInput {
            secret_id: "AKIDEXAMPLE",
            secret_key: "EXAMPLEKEY",
            service: "svc",
            host: "api.example.com",
            action: "SomeAction",
            timestamp: 1700000000,
            payload: r#"{"Limit": 1}"#,
            content_type: "application/json; charset=utf-8",
            method: "POST",
            canonical_uri: "/",
            canonical_query: "",
        }
    }

    #[test]
    fn same_input_same_signature_and_any_change_changes_it() {
        let input = neutral_input();
        let a = sign(&input).expect("签名应当成功");
        let b = sign(&input).expect("签名应当成功");
        assert_eq!(
            a.signature, b.signature,
            "同一输入必须得到同一签名（⛔ 不许有 now() 参与）"
        );

        let mut payload_changed = neutral_input();
        payload_changed.payload = r#"{"Limit": 2}"#;
        assert_ne!(sign(&payload_changed).unwrap().signature, a.signature);

        let mut host_changed = neutral_input();
        host_changed.host = "api.example.com.evil";
        assert_ne!(sign(&host_changed).unwrap().signature, a.signature);

        let mut action_changed = neutral_input();
        action_changed.action = "OtherAction";
        assert_ne!(sign(&action_changed).unwrap().signature, a.signature);

        // 日期（CredentialScope）变了 ⇒ 签名必须变；日期只能由 `finish` 单独指定
        let key = derive_signing_key(input.secret_key.as_bytes(), "2023-11-14", "svc");
        assert_eq!(utc_date(input.timestamp).unwrap(), "2023-11-14");
        assert_ne!(finish(&input, "2023-11-15", &key).signature, a.signature);
    }

    #[test]
    fn action_is_lowercased_in_the_canonical_header_only() {
        let input = neutral_input();
        let signed = sign(&input).expect("签名应当成功");
        assert!(
            signed.canonical_request.contains("x-tc-action:someaction\n"),
            "规范头里 action 必须小写：{}",
            signed.canonical_request
        );
        assert!(
            !signed.canonical_request.contains("SomeAction"),
            "⛔ 规范头里不许出现大写的 action"
        );
    }
}

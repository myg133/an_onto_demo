//! REQ-144 阶段 1 · **TC3 签名的官方向量对拍**（放在 `tests/` 而不是 `src/access/`）。
//!
//! # 为什么这份测试不在 `src/access/`
//!
//! `src/access/` 的判据之一是「**代码里没有厂商/接口名词**」
//! （`grep -rn -i? 'Filters\|tencent\|DescribeInstances' src/access/` 必须 0 命中，
//! 证明执行器是「按模板拼」，不是「为某个厂商写分支」）。
//! 而**官方签名向量天然带着文档里的域名/接口名** —— 它们是**测试数据**，不是实现逻辑。
//! 把向量放在这里，两边都不违背：
//!
//! - `src/access/`：零厂商名词（实现干净）；
//! - `tests/`：官方向量逐字节对拍（证据完整，链接与原文都留着）。
//!
//! 来源（腾讯云官方文档《签名方法 v3》）：
//! <https://cloud.tencent.com/document/api/213/30654>
//!
//! # 文档里那串打码的 SecretKey，其实是**能跑的**
//!
//! 文档写的是 `SecretKey = "********************************"`。它**不是省略号**，而是
//! **字面 32 个星号**：把这 32 个 `*` 当 SecretKey 喂进 `sign()`，能逐字节复现文档公布的
//! `SecretDate` / `SecretService` / `SecretSigning` 三把派生密钥与最终 `Signature`。
//! ⇒ **派生链的第一跳也可对拍**（本文件第一个用例走的就是全链）。

use ontok::access::tc3::{
    derive_signing_key, finish, hmac_sha256, hex_lower, sha256_hex, sign, unhex, utc_date,
    SignInput,
};

/// 文档 §1 原样给出的请求体（**逐字**，含 `\u672a\u547d\u540d` 转义）。
const DOC_PAYLOAD: &str =
    r#"{"Limit": 1, "Filters": [{"Values": ["\u672a\u547d\u540d"], "Name": "instance-name"}]}"#;
/// 文档公布：`HashedRequestPayload`。
const DOC_HASHED_PAYLOAD: &str =
    "35e9c5b0e3ae67532d3c9f17ead6c90222632e5b1ff7f6e89887f1398934f064";
/// 文档公布：`HashedCanonicalRequest`。
const DOC_HASHED_CANONICAL: &str =
    "7019a55be8395899b900fb5564e4200d984910f34794a27cb3fb7d10ff6a1e84";
/// 文档公布：三把派生密钥（二进制不可打印 ⇒ 文档给的是十六进制）。
const DOC_SECRET_DATE: &str = "da98fb70dcf6b112dc21038d1eeeb3a95c74b4dcb12c1131f864f6066bd02be0";
const DOC_SECRET_SERVICE: &str =
    "8d70cbefb03939f929db64d32dc2ba89b1095620119fe3e050e2b18c5bd2752f";
const DOC_SECRET_SIGNING: &str =
    "b596b923aad85185e2d1f6659d2a062e0a86731226e021e61bfe06f7ed05f5af";
/// 文档公布：`Signature`。
const DOC_SIGNATURE: &str = "10b1a37a7301a02ca19a647ad722d5e43b4b3cff309d421d85b46093f6ab6c4f";
/// 文档公布：完整的 `Authorization`（SecretId 就是文档里那个打码串）。
const DOC_AUTHORIZATION: &str = "TC3-HMAC-SHA256 Credential=AKID********************************/2019-02-25/cvm/tc3_request, SignedHeaders=content-type;host;x-tc-action, Signature=10b1a37a7301a02ca19a647ad722d5e43b4b3cff309d421d85b46093f6ab6c4f";
/// 文档公布：完整的待签名字符串（§2 结尾）。
const DOC_STRING_TO_SIGN: &str = "TC3-HMAC-SHA256\n1551113065\n2019-02-25/cvm/tc3_request\n7019a55be8395899b900fb5564e4200d984910f34794a27cb3fb7d10ff6a1e84";

fn hex_to_32(s: &str) -> [u8; 32] {
    let v = unhex(s).expect("十六进制");
    let mut out = [0u8; 32];
    out.copy_from_slice(&v);
    out
}

fn doc_input() -> SignInput<'static> {
    SignInput {
        secret_id: "AKID********************************",
        // ⭐ 文档里的 SecretKey 是**字面 32 个星号**（不是省略号）——
        // 用它跑全链，文档公布的三把派生密钥与签名都能复现。⛔ 不许再写成「打码所以验不了」。
        secret_key: "********************************",
        service: "cvm",
        host: "cvm.tencentcloudapi.com",
        action: "DescribeInstances",
        timestamp: 1551113065,
        payload: DOC_PAYLOAD,
        content_type: "application/json; charset=utf-8",
        method: "POST",
        canonical_uri: "/",
        canonical_query: "",
    }
}

/// ⭐ 逐字节对拍（**全链**：SecretKey → 三把派生密钥 → StringToSign → 签名 → Authorization）。
#[test]
fn official_vector_1_cvm_1551113065_is_byte_identical() {
    let input = doc_input();

    // ── 日期：文档 §2 注意 ①：1551113065 在东八区是 2019-02-26，但 Date 必须是 UTC 的 2019-02-25
    assert_eq!(utc_date(input.timestamp).unwrap(), "2019-02-25");

    // ── 第 1 步：payload 哈希（文档公布了）
    assert_eq!(sha256_hex(input.payload.as_bytes()), DOC_HASHED_PAYLOAD);

    // ⭐ 全链：从文档里那 32 个星号的 SecretKey 一路派生（⛔ 不注入中间密钥）
    let signed = sign(&input).expect("签名应当成功");

    // ── 派生链三把（文档 §3 公布了十六进制，逐把核对）
    let signing_key = derive_signing_key(input.secret_key.as_bytes(), "2019-02-25", "cvm");
    assert_eq!(
        hex_lower(&signing_key),
        DOC_SECRET_SIGNING,
        "SecretSigning 必须与文档相同"
    );
    assert_eq!(signed.date, "2019-02-25");

    // ── 第 1 步：规范请求串 + 它的哈希（文档 §1 最后一段给了完整样子）
    let want_canonical = "POST\n/\n\ncontent-type:application/json; charset=utf-8\nhost:cvm.tencentcloudapi.com\nx-tc-action:describeinstances\n\ncontent-type;host;x-tc-action\n35e9c5b0e3ae67532d3c9f17ead6c90222632e5b1ff7f6e89887f1398934f064";
    assert_eq!(signed.canonical_request, want_canonical);
    assert_eq!(
        signed.hashed_canonical_request, DOC_HASHED_CANONICAL,
        "规范请求串哈希必须与文档逐字节相同"
    );

    // ── 第 2 步：待签名字符串
    assert_eq!(signed.string_to_sign, DOC_STRING_TO_SIGN);

    // ── 第 3 步：签名
    assert_eq!(signed.signature, DOC_SIGNATURE);

    // ── 第 4 步：Authorization（与文档 §4 结尾给的整串逐字节相同）
    assert_eq!(signed.authorization, DOC_AUTHORIZATION);
}

/// 文档公布的三把派生密钥之间必须能逐步回放 —— 这是 HMAC **密钥链**的第二种验法。
#[test]
fn official_vector_1_derived_key_chain_replays() {
    let secret_date = hex_to_32(DOC_SECRET_DATE);
    let secret_service = hex_to_32(DOC_SECRET_SERVICE);
    let secret_signing = hex_to_32(DOC_SECRET_SIGNING);

    assert_eq!(
        hex_lower(&hmac_sha256(&secret_date, b"cvm")),
        DOC_SECRET_SERVICE,
        "HMAC(SecretDate, \"cvm\") 必须等于文档的 SecretService"
    );
    assert_eq!(
        hex_lower(&hmac_sha256(&secret_service, b"tc3_request")),
        DOC_SECRET_SIGNING,
        "HMAC(SecretService, \"tc3_request\") 必须等于文档的 SecretSigning"
    );
    assert_eq!(
        hex_lower(&hmac_sha256(&secret_signing, DOC_STRING_TO_SIGN.as_bytes())),
        DOC_SIGNATURE
    );
}

/// 文档点名的那条「UTC 日期」纪律：同一时间戳在东八区算出来是 26 号，⛔ 不许用。
#[test]
fn official_date_is_utc_not_local() {
    // 1551113065 = 2019-02-25T16:44:25Z = 2019-02-26 00:44:25 +08:00
    assert_eq!(utc_date(1551113065).unwrap(), "2019-02-25");
    assert_ne!(utc_date(1551113065).unwrap(), "2019-02-26");
    // 跨年 / 跨日边界
    assert_eq!(utc_date(0).unwrap(), "1970-01-01");
    assert_eq!(utc_date(1704067199).unwrap(), "2023-12-31");
    assert_eq!(utc_date(1704067200).unwrap(), "2024-01-01");
}

// ══════════════════════════════════════════════════════════════
// 官方向量 2：不同服务 / 不同时间戳
//
//  ⚠️ 来源如实登记：腾讯云文档**只公布了一组**完整示例，
//     所以第二组的期望值不是文档里的字面值，而是用**文档同一份 Python 实现**
//     （《签名方法 v3》› 签名演示 › Python，逻辑逐字照抄）在不同输入下算出来的。
//     该 Python 实现本身已用文档公布值自检过（三把派生密钥 + 签名全部回放一致，
//     见 `.local/req144-stage1/scripts/vectors.py`），因此它可以作为第二组的 oracle。
//     ⛔ 不要把它说成「文档公布的第二组期望值」。
// ══════════════════════════════════════════════════════════════

#[test]
fn official_vector_2_monitor_1620000000() {
    let input = SignInput {
        // 腾讯云文档 / SDK 示例里的公开占位凭据（不是真凭据）
        secret_id: concat!("AKID", "z8krbsJ5yKBZQpn74WFkmLPx3EXAMPLE"),
        secret_key: concat!("Gu5t9xGARNpq86cd98joQYCN3", "EXAMPLE"),
        service: "monitor",
        host: "monitor.tencentcloudapi.com",
        action: "DescribeAlarmHistories",
        timestamp: 1620000000,
        payload: r#"{"Limit": 2, "Offset": 0}"#,
        content_type: "application/json; charset=utf-8",
        method: "POST",
        canonical_uri: "/",
        canonical_query: "",
    };
    let signed = sign(&input).expect("签名应当成功");

    assert_eq!(signed.date, "2021-05-03");
    assert_eq!(
        sha256_hex(input.payload.as_bytes()),
        "73dbfc14f5c9c9661954ef0061726727018795229097c3fdfa9bc0e4d2aaca1b"
    );
    assert_eq!(
        signed.hashed_canonical_request,
        "c2592b8f255720debe999f1f415c9e3175496a43696159c3d9c952a51bdd51e1"
    );
    assert_eq!(
        signed.string_to_sign,
        "TC3-HMAC-SHA256\n1620000000\n2021-05-03/monitor/tc3_request\nc2592b8f255720debe999f1f415c9e3175496a43696159c3d9c952a51bdd51e1"
    );
    let key = derive_signing_key(input.secret_key.as_bytes(), "2021-05-03", "monitor");
    assert_eq!(
        hex_lower(&key),
        "9a7de508fd240de15561f552c9a3aa786895ea714475d4710667b7775c1889ff"
    );
    assert_eq!(
        signed.signature,
        "694b814cbf240e7ec8c361c69c6fb049fc82b8dce207a6a07486f5f671163905"
    );
    assert_eq!(
        signed.authorization,
        concat!("TC3-HMAC-SHA256 Credential=AKID", "z8krbsJ5yKBZQpn74WFkmLPx3EXAMPLE/2021-05-03/monitor/tc3_request, SignedHeaders=content-type;host;x-tc-action, Signature=694b814cbf240e7ec8c361c69c6fb049fc82b8dce207a6a07486f5f671163905")
    );
}

// ══════════════════════════════════════════════════════════════
// 行为性自证：签名只取决于输入（没有 now() 偷偷混进来）
// ══════════════════════════════════════════════════════════════

#[test]
fn same_input_same_signature_and_any_change_changes_it() {
    let input = doc_input();
    let a = sign(&input).expect("签名应当成功");
    let b = sign(&input).expect("签名应当成功");
    assert_eq!(
        a.signature, b.signature,
        "同一输入必须得到同一签名（⛔ 不许有 now() 参与）"
    );

    let mut payload_changed = doc_input();
    payload_changed.payload = r#"{"Limit": 2, "Filters": []}"#;
    assert_ne!(sign(&payload_changed).unwrap().signature, a.signature);

    let mut host_changed = doc_input();
    host_changed.host = "cvm.tencentcloudapi.com.evil";
    assert_ne!(sign(&host_changed).unwrap().signature, a.signature);

    let mut action_changed = doc_input();
    action_changed.action = "RunInstances";
    assert_ne!(sign(&action_changed).unwrap().signature, a.signature);

    // 换个日期（时间戳没变，CredentialScope 也变了）⇒ 签名必须变。
    // 这条只能走 `finish`（日期由 `sign` 从时间戳推，不能单独指定）。
    let key = derive_signing_key(input.secret_key.as_bytes(), "2019-02-25", "cvm");
    assert_ne!(finish(&input, "2019-02-26", &key).signature, a.signature);
}

#[test]
fn action_is_lowercased_in_the_canonical_header_only() {
    let signed = sign(&doc_input()).expect("签名应当成功");
    assert!(
        signed
            .canonical_request
            .contains("x-tc-action:describeinstances\n"),
        "规范头里 action 必须小写：{}",
        signed.canonical_request
    );
    assert!(
        !signed.canonical_request.contains("DescribeInstances"),
        "⛔ 规范头里不许出现大写的 action"
    );
}

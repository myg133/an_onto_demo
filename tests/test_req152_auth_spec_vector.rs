//! REQ-152 · `docs/AUTH-SPEC.md` 的**自测向量** —— 文档里写的期望值必须被实现复现。
//!
//! 这条测试做两件事（缺一不可）：
//!
//! | # | 断言 | 为什么 |
//! | --- | --- | --- |
//! | ① | 用 `access::tc3::sign()` 复算 §7 的每个中间值与签名 | 算法**跑得通**（AC-110 机判） |
//! | ② | `docs/AUTH-SPEC.md` **原文里**确实写着这些期望值 | 文档与实现**不许各走一边**：改了算法没改文档（或反过来）直接红 |
//!
//! ⚠️ 预期值来自官方向量（《签名方法 v3》§1 的请求 + `content-type;host;x-tc-action` 三头签名集），
//! 与 `tests/test_req144_tc3_vectors.rs` 是同一组常量 —— 那份盯「全链对拍」，
//! 这份盯「文档里贴出来的数字能被复算」。

use ontok::access::tc3::{hex_lower, sign, SignInput};

mod common;

const DOC_PAYLOAD: &str =
    r#"{"Limit": 1, "Filters": [{"Values": ["\u672a\u547d\u540d"], "Name": "instance-name"}]}"#;
const HASHED_PAYLOAD: &str =
    "35e9c5b0e3ae67532d3c9f17ead6c90222632e5b1ff7f6e89887f1398934f064";
const SECRET_DATE: &str = "da98fb70dcf6b112dc21038d1eeeb3a95c74b4dcb12c1131f864f6066bd02be0";
const SECRET_SERVICE: &str = "8d70cbefb03939f929db64d32dc2ba89b1095620119fe3e050e2b18c5bd2752f";
const SECRET_SIGNING: &str = "b596b923aad85185e2d1f6659d2a062e0a86731226e021e61bfe06f7ed05f5af";
const HASHED_CANONICAL: &str =
    "7019a55be8395899b900fb5564e4200d984910f34794a27cb3fb7d10ff6a1e84";
const SIGNATURE: &str = "10b1a37a7301a02ca19a647ad722d5e43b4b3cff309d421d85b46093f6ab6c4f";
const SIGNED_HEADERS: &str = "content-type;host;x-tc-action";

const CANONICAL_REQUEST: &str = "POST\n/\n\ncontent-type:application/json; charset=utf-8\nhost:cvm.tencentcloudapi.com\nx-tc-action:describeinstances\n\ncontent-type;host;x-tc-action\n35e9c5b0e3ae67532d3c9f17ead6c90222632e5b1ff7f6e89887f1398934f064";
const STRING_TO_SIGN: &str = "TC3-HMAC-SHA256\n1551113065\n2019-02-25/cvm/tc3_request\n7019a55be8395899b900fb5564e4200d984910f34794a27cb3fb7d10ff6a1e84";

fn input() -> SignInput<'static> {
    SignInput {
        secret_id: "AKID********************************",
        // 文档里那 32 个星号是**字面**的 SecretKey（不是打码）。
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

/// ① 实现复算：§7 的每一个数字都对得上。
#[test]
fn the_documented_vector_recomputes_byte_for_byte() {
    let signed = sign(&input()).expect("签名");
    assert_eq!(
        ontok::access::tc3::sha256_hex(DOC_PAYLOAD.as_bytes()),
        HASHED_PAYLOAD
    );
    assert_eq!(signed.date, "2019-02-25", "scope 的 date 必须是 UTC 日期");
    assert_eq!(signed.canonical_request, CANONICAL_REQUEST);
    assert_eq!(signed.hashed_canonical_request, HASHED_CANONICAL);
    assert_eq!(signed.string_to_sign, STRING_TO_SIGN);
    assert_eq!(signed.signature, SIGNATURE);
    assert!(
        signed.authorization.ends_with(&format!(
            "SignedHeaders={SIGNED_HEADERS}, Signature={SIGNATURE}"
        )),
        "{}",
        signed.authorization
    );

    // 派生链三把（文档公布了十六进制 ⇒ 每跳都能单独对拍，⛔ 只看长度等于没测）。
    let mut tc3_key = b"TC3".to_vec();
    tc3_key.extend_from_slice(input().secret_key.as_bytes());
    assert_eq!(
        hex_lower(&ontok::access::tc3::hmac_sha256(&tc3_key, b"2019-02-25")),
        SECRET_DATE,
        "SecretDate = HMAC(\"TC3\"+SecretKey, date)"
    );
    let secret_date = ontok::access::tc3::unhex(SECRET_DATE).expect("十六进制");
    assert_eq!(
        hex_lower(&ontok::access::tc3::hmac_sha256(&secret_date, b"cvm")),
        SECRET_SERVICE,
        "SecretService = HMAC(SecretDate, service)"
    );
    let secret_service = ontok::access::tc3::unhex(SECRET_SERVICE).expect("十六进制");
    assert_eq!(
        hex_lower(&ontok::access::tc3::hmac_sha256(
            &secret_service,
            b"tc3_request"
        )),
        SECRET_SIGNING,
        "SecretSigning = HMAC(SecretService, \"tc3_request\")"
    );
    let key = ontok::access::tc3::derive_signing_key(
        input().secret_key.as_bytes(),
        "2019-02-25",
        "cvm",
    );
    assert_eq!(hex_lower(&key), SECRET_SIGNING, "派生链的终点要一致");
}

/// ② 文档里确实写着这些期望值（文档 ↔ 实现同一个向量，⛔ 不许各写一份）。
#[test]
fn the_spec_document_carries_the_same_vector() {
    // ⚠️ 读文档走 `common::source_root`（REQ-046/047）：⛔ 不用裸 `env!("CARGO_MANIFEST_DIR")`
    // —— 那是编译期常量，二进制被搬到别处就指向不存在的目录（`test_anchor_guard` 会红）。
    let doc = common::source_root::read_source("docs/AUTH-SPEC.md");

    for (label, needle) in [
        ("算法名", "TC3-HMAC-SHA256"),
        ("签名", SIGNATURE),
        ("payload 哈希", HASHED_PAYLOAD),
        ("规范串哈希", HASHED_CANONICAL),
        ("SecretSigning", SECRET_SIGNING),
        ("SignedHeaders", SIGNED_HEADERS),
        ("时间戳", "1551113065"),
        ("UTC 日期", "2019-02-25"),
        ("scope", "2019-02-25/cvm/tc3_request"),
        ("规范请求串", CANONICAL_REQUEST),
        ("待签名字符串", STRING_TO_SIGN),
    ] {
        assert!(
            doc.contains(needle),
            "文档里缺 {label}（期望原文含 {needle:?}）—— 改了实现没改文档？"
        );
    }
    // 三个「必须写清」的规范化规则：小写、排序、结尾换行。
    for rule in ["小写", "升序", "行尾 `\\n`"] {
        assert!(doc.contains(rule), "文档缺规范化规则：{rule}");
    }
}

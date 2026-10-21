//! REQ-107 · AC-005：13 条客户类出口的**零可见变化**（TC-016）。
//!
//! # 口径（重要）
//!
//! | 项 | 值 |
//! | --- | --- |
//! | 比什么 | **13 份字节**：C1(=C2) / C3 / C4 / C5 / H1 / H2 / H3×2 / M1–M5 |
//! | 比到哪一层 | **逐字节**（`assert_eq!` + 差异偏移与前后 40 字节） |
//! | golden 的来源 | **基线 worktree**（`b714f48`）跑本文件的生成器产出；`PROVENANCE.txt` 第一行钉死 |
//! | **排除** | **Z1**（AC-006 要求它的 400 正文**变**；AC §七 A2 裁定它是**唯一**允许的变化） |
//! | 不属本口径 | **stderr 报告形状**（卡点③ 的裁定产物）—— 它不进 stdout/响应体，逐条登记在交付物 §5 |
//!
//! # ⚠️ 这条判据的**已知天花板**（⛔ 不许写成「已封死」）
//!
//! 本文件在**基线**上也要能编译（生成器要在 `b714f48` 的 worktree 里跑），
//! 所以它只能用**基线就有的**函数（`render_html` / `filter_view` / `build_overview` /
//! `index_html` / `build_router` / `handle_line`）**复刻**各出口，不能引用
//! `domain::pipeline`（收口后才有）。
//!
//! ⇒ 「复刻」与「生产」可能分叉（纪律 H 的忠实重复实现）。
//! 补强有两条，**都在**：
//! - `tests/test_req107_pipeline_bytes.rs`：**管线产出**必须逐字节等于同一份 golden
//!   （把生产钉在 golden 上，复刻漂了也不影响这一格）；
//! - TC-017 的真进程字节比对（需要数据库，本机不可复核，如实登记）。
//!
//! ⚠️ 本文件**不加** `//! @guard-manifest: REQ-081`（REQ-107 A7-1）。

// ⚠️ 本文件**不依赖** `common::strip_comments` / `common::req107` 这两个本分支才有的工具
// （要在**基线 worktree**（`b714f48`）里也能编译），但允许 `mod common;` ——
// 基线就有 `crate_root()`，而 `env!("CARGO_MANIFEST_DIR")` 是 `tests/test_anchor_guard.rs`
// G-1 **明令禁止**的裸调用（必须走锚点）。
mod common;

#[path = "common/req107.rs"]
mod fixture_mod;

use fixture_mod::{concept_groups, fixture, names, overview, sampling, GOLDEN_FILES, MCP_CALLS};
use ontok::domain::view::CustomerView;

const TOKEN: &str = "correct-horse-battery";
const PROJECT: &str = "样例项目";

fn golden_dir() -> std::path::PathBuf {
    common::crate_root()
        .join("tests")
        .join("fixtures")
        .join("req107")
        .join("golden")
}

/// 复刻一条客户类出口的**字节**（只用基线就有的生产函数）。
async fn actual_bytes() -> Vec<(&'static str, Vec<u8>)> {
    use axum::body::Body;
    use axum::http::{header, Request};
    use tower::ServiceExt;

    let cv: CustomerView = fixture();
    let mut out: Vec<(&'static str, Vec<u8>)> = Vec::new();

    // ── C1 / C2（同一份 `page.html`；C2 写文件、C1 写 stdout，字节相同）──
    let page = ontok::domain::html_view::render_html(&cv, &sampling());
    out.push(("C1", page.html.clone().into_bytes()));

    // ── C3（JSON 出口：先剔除、再序列化、再加尾换行 = `emit_domain_line`）──
    let filtered = ontok::domain::property_filter::filter_view(&cv);
    let mut json = serde_json::to_string_pretty(&filtered.view).unwrap();
    json.push('\n');
    out.push(("C3", json.into_bytes()));

    // ── C4 / C5（业务总览：文本不加尾换行、JSON 加）──
    let ov = overview();
    out.push((
        "C4",
        ontok::domain::overview::render_overview_text(&ov).into_bytes(),
    ));
    let mut ov_json = serde_json::to_string_pretty(&ov).unwrap();
    ov_json.push('\n');
    out.push(("C5", ov_json.into_bytes()));

    // ── H1 / H2 / H3（走**生产接线** `state_from_view` + 真实路由）──
    //
    // ⚠️ REQ-110：`state_from_view` 多了一个 `action_type_count` 参数
    // （只影响 TTL 文件头的有损声明）。⇒ 本文件的「在基线上也要能编译」
    // 这条**历史属性到此为止**：生成器是在 b714f48 的一次性步骤，
    // golden 已经产出并锚定；判据本身（逐字节比 golden）不受影响。
    let st = ontok::server::http::state_from_view(&cv, PROJECT, 2, TOKEN, 0);
    let app = ontok::server::http::build_router(st.clone());
    let get = |uri: String| {
        let app = app.clone();
        async move {
            let r = app
                .oneshot(
                    Request::builder()
                        .uri(uri)
                        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            let status = r.status();
            let body = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
            (status, body.to_vec())
        }
    };

    let (s, b) = get("/".to_string()).await;
    assert!(s.is_success(), "H1 首页必须 200（干净夹具）");
    out.push(("H1", b));

    let (s, b) = get(format!("/api/graph?project={PROJECT}&version=2")).await;
    assert!(s.is_success(), "H2 /api/graph 必须 200（干净夹具）");
    out.push(("H2", b));

    // 节点 key 取自**过滤后**的图（与生产同一口径）。
    let keys: Vec<String> = ontok::domain::graph::project(&filtered.view)
        .nodes
        .iter()
        .map(|n| n.key.clone())
        .collect();
    assert!(keys.len() >= 2, "夹具至少要两个对象才能出两份 H3 golden");
    for (id, key) in [("H3a", &keys[0]), ("H3b", &keys[1])] {
        let (s, b) = get(format!(
            "/api/node?project={PROJECT}&version=2&id={}",
            urlencode(key)
        ))
        .await;
        assert!(s.is_success(), "H3 /api/node 必须 200（干净夹具）：id={key}");
        out.push((id, b));
    }

    // ── M1–M5（MCP：`handle_line` 是生产的同步入口）──
    let ctx = mcp_ctx(&filtered.view, &ov);
    for (id, tool, args) in MCP_CALLS {
        let line = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"{tool}","arguments":{args}}}}}"#
        );
        let resp = ontok::mcp::serve::handle_line(&ctx, &line).expect("请求必须有响应");
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["result"]["isError"], false, "{id}（{tool}）在干净夹具上必须成功：{resp}");
        out.push((id, resp.into_bytes()));
    }

    out
}

/// 与生产同口径装配 MCP 上下文（视图已过前处理；总览由 `build_overview` 现算）。
fn mcp_ctx(view: &CustomerView, ov: &ontok::domain::overview::Overview) -> ontok::mcp::tools::OntologyContext {
    let mut confirmation = std::collections::BTreeMap::new();
    for g in concept_groups() {
        confirmation.insert(
            g.display_name.clone(),
            ontok::mcp::tools::confirmation_of(&g),
        );
    }
    ontok::mcp::tools::OntologyContext {
        project: PROJECT.to_string(),
        version: 2,
        frozen_at: "2026-01-01T00:00:00Z".to_string(),
        view: view.clone(),
        overview: ov.clone(),
        names: names(),
        confirmation,
    }
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

// ══════════════════════════════════════════════════════════════
// 生成器（只在 `REQ107_OUT` 设置时写文件）
// ══════════════════════════════════════════════════════════════

/// ⚠️ **刻意不用 `#[ignore]`**：AC-009 / TC-027 明令「不许新增 `#[ignore]`」，
/// 而附录 A.5 建议加一个。两者冲突时按 AC-009 走 —— 改成「环境变量没设置就什么都不做」。
///
/// ⛔ 这**不是**门禁：它不判定任何东西。判定在下面那条测试里。
/// 产出时**不碰** `PROVENANCE.txt` / `SHA256SUMS`（那两个是「golden 来源」的锚，
/// 由生成器改写就等于允许「用改完的代码生成 golden」）。
#[tokio::test]
#[allow(non_snake_case)]
async fn req107_write_golden_only_when_REQ107_OUT_is_set() {
    let Ok(out_dir) = std::env::var("REQ107_OUT") else {
        eprintln!(
            "· REQ107_OUT 未设置 ⇒ 不产出 golden（这是**生成器**，不是判据；\
             判定见 test_req107_zero_visible_change_across_13_customer_outlets）"
        );
        return;
    };
    let dir = std::path::PathBuf::from(out_dir);
    std::fs::create_dir_all(&dir).expect("建不出 REQ107_OUT 目录");
    for (id, bytes) in actual_bytes().await {
        let name = GOLDEN_FILES
            .iter()
            .find(|(gid, _)| *gid == id)
            .map(|(_, n)| *n)
            .unwrap_or_else(|| panic!("{id} 没有登记 golden 文件名"));
        std::fs::write(dir.join(name), &bytes).expect("写 golden 失败");
        eprintln!("· 写出 {name}（{} 字节）", bytes.len());
    }
    // 自证：写出来的东西必须能被明确数出来（13 份）。
    assert_eq!(GOLDEN_FILES.len(), 13, "golden 必须是 13 份（C2 复用 C1）");
}

// ══════════════════════════════════════════════════════════════
// 常态判据
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_req107_zero_visible_change_across_13_customer_outlets() {
    let dir = golden_dir();

    // ① golden 的**来源**必须被钉死在基线 commit 上（防「用改完的代码生成 golden」）。
    let prov = std::fs::read_to_string(dir.join("PROVENANCE.txt"))
        .unwrap_or_else(|e| panic!("读不到 {}: {e}", dir.join("PROVENANCE.txt").display()));
    assert_eq!(
        prov.lines().next().map(str::trim),
        Some("b714f48"),
        "`PROVENANCE.txt` 的第一行必须是基线 commit `b714f48` —— 否则 golden 的来历不可信"
    );

    // ② `SHA256SUMS` 与文件内容互校（防 golden 被手改一个小字符让它「对上」）。
    let sums = std::fs::read_to_string(dir.join("SHA256SUMS")).expect("读不到 SHA256SUMS");
    let mut checked = 0usize;
    for line in sums.lines().filter(|l| !l.trim().is_empty()) {
        let mut it = line.split_whitespace();
        let want = it.next().expect("SHA256SUMS 行格式：<hex>  <file>");
        let name = it.next().expect("SHA256SUMS 行格式：<hex>  <file>");
        let bytes = std::fs::read(dir.join(name))
            .unwrap_or_else(|e| panic!("SHA256SUMS 里的 {name} 读不到：{e}"));
        let got = sha256_hex(&bytes);
        assert_eq!(got, want, "{name} 的内容与 SHA256SUMS 不符 —— golden 被改过");
        checked += 1;
    }
    assert_eq!(checked, 13, "SHA256SUMS 必须覆盖 13 份 golden，实际 {checked} 份");

    // ③ 逐份比对：当前代码的字节 == golden。
    let got = actual_bytes().await;
    assert_eq!(got.len(), 13, "13 条客户类出口，实际产出 {} 份", got.len());
    for (id, bytes) in &got {
        let name = GOLDEN_FILES
            .iter()
            .find(|(gid, _)| gid == id)
            .map(|(_, n)| *n)
            .unwrap();
        let want = std::fs::read(dir.join(name)).expect("读不到 golden");
        assert_bytes_equal(id, name, &want, bytes);

        // ④ 反空转：产物不许是空的（双方都空 ⇒ 恒真）。
        assert!(!bytes.is_empty(), "{id} 的产物是空的");
        if *id == "C1" {
            assert!(
                bytes.len() > 200,
                "C1 的页面只有 {} 字节 —— 太短了，多半是渲染坏了",
                bytes.len()
            );
        }
    }

    // ⑤ C1 与 C2 必须共用**同一份**字节（`--out` 写文件与 stdout 写的是同一批）。
    //    复刻侧只有一个来源，所以这里钉的是「两份 golden 不许分家」这个事实：
    let c1 = std::fs::read(dir.join("C1_html.txt")).unwrap();
    let c2_source = std::fs::read_to_string(dir.join("PROVENANCE.txt")).unwrap();
    assert!(
        c2_source.contains("C2 复用 C1_html.txt"),
        "`PROVENANCE.txt` 必须写明 C2 与 C1 共用同一份字节（否则「C2 也零变化」这句话没有落点）"
    );
    assert!(!c1.is_empty());
}

/// 逐字节相等；不等时打印**第一处差异的偏移 + 前后各 40 字节**（⛔ 不只打印长度）。
fn assert_bytes_equal(id: &str, name: &str, want: &[u8], got: &[u8]) {
    if want == got {
        return;
    }
    let at = want
        .iter()
        .zip(got.iter())
        .position(|(a, b)| a != b)
        .unwrap_or_else(|| want.len().min(got.len()));
    let lo = at.saturating_sub(40);
    let ctx = |s: &[u8]| {
        String::from_utf8_lossy(&s[lo..(at + 40).min(s.len())]).to_string()
    };
    panic!(
        "出口 {id}（{name}）的字节变了 —— 要么改回去，要么在 AC-005 变化清单里登记。\n\
         第一处差异：偏移 {at}（want 长 {} / got 长 {}）\n\
         golden: {:?}\n\
         now   : {:?}",
        want.len(),
        got.len(),
        ctx(want),
        ctx(got)
    );
}

// ══════════════════════════════════════════════════════════════
// SHA-256（不引依赖：本仓没有 sha2，而 AC 不许新增依赖）
// ══════════════════════════════════════════════════════════════

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

/// 标准 SHA-256（hex 小写）。自检见下面的 `sha256_is_not_vacuous`。
fn sha256_hex(data: &[u8]) -> String {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64) * 8;
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
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
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

/// `SHA256SUMS` 互校这一步本身不能是空转（纪律 E：扫出「零」先怀疑扫描器）。
#[test]
fn sha256_is_not_vacuous() {
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_ne!(sha256_hex(b"abc"), sha256_hex(b"abd"));
}

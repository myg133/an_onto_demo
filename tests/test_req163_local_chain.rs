//! ⭐ REQ-163 · **构建端「纯本地链」**的真进程判据。
//!
//! | # | 判据 | 它说什么 |
//! | --- | --- | --- |
//! | R1 | [`build_run_reaches_a_review_desk_without_any_db_env`] | 清空 `ONTOK_DATABASE_URL` / `ONTOK_TEST_DATABASE_URL` / `ONTOK_SECRET_KEY` 之后，`build run` 仍然：退出码 0 → `<work-dir>/out/proposal.json` 存在 → 服务起在 `127.0.0.1:<port>` → `GET /api/state` 的 `draft.items > 0` |
//! | R2 | [`the_cli_pack_is_byte_identical_to_the_page_export`] | 同一个定版 + 同一个 `built_at` ⇒ `bundle pack --dir` 与**页面导出**的包 **sha256 相同**；`bundle verify` 退出 0 |
//! | R2 | [`pack_dir_and_project_are_mutually_exclusive_and_named_in_the_error`] | `--dir` + `--project` ⇒ 退出 2 且**点名**两个参数（一个文件都不产出） |
//! | R2 | [`pack_needs_a_source_and_says_so`] | 两个都没给 ⇒ 退出 2，并把两种用法写全 |
//! | F3/F4 | [`a_non_loopback_bind_is_refused_with_exit_2_and_no_work_dir`] | 非回环 `--bind` ⇒ **退出 2**（不是 ④ 段的 1）、`--work-dir` 里**一个文件都没建**（⛔ 不白烧 token），且报错**不许**建议 `--allow-public`（本命令没有那个开关） |
//! | — | [`the_build_run_help_describes_the_local_chain`] | `build run --help` 里不再有口令 / 审计 / 非回环那些旧开关（无库、无口令） |
//!
//! # 为什么是**真进程**
//!
//! R1 的两条「不依赖」是**进程边界**上的事实：`run_draft` 的单元判据只能说「函数没连库」，
//! 说不了「`ontok-builder` 在三个环境变量都被删掉时仍然起得来」。所以这里起真二进制。
//! R2 的「逐字节相同」也必须真：两边各跑一次真实的打包路径，比 sha256。
//!
//! ⚠️ 本文件**不碰元数据库**：一条 SQL 都没有。夹具是 `tests/fixtures/req147/` 的假 ACP
//! agent（Node）—— 它只讲 stdio JSON-RPC，不起真 `pi`。

mod common;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::Value;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_ontok-builder")
}

/// crate 根：走运行时锚点（⛔ 不用编译期常量 —— REQ-046 的事故形态）。
fn crate_root() -> PathBuf {
    common::source_root::crate_root()
}

fn fake_acp() -> PathBuf {
    crate_root().join("tests/fixtures/req147/fake-acp.js")
}

fn skill_fixture() -> PathBuf {
    crate_root().join("tests/fixtures/req147/skill-fixture.md")
}

fn node_ok() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "ontok-req163-e2e-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&d).expect("建临时目录");
    d
}

/// 三个「旧台」环境变量：⭐ R1 的判据就是**它们都不在**。
const DB_ENVS: [&str; 3] = [
    "ONTOK_DATABASE_URL",
    "ONTOK_TEST_DATABASE_URL",
    "ONTOK_SECRET_KEY",
];

/// 从子进程环境里**删掉**（不是设成空串）那三个变量，并打印父进程里的状态。
fn clean_env(cmd: &mut Command) -> &mut Command {
    for k in DB_ENVS {
        cmd.env_remove(k);
    }
    cmd
}

fn env_state(name: &str) -> String {
    match std::env::var(name) {
        Ok(_) => "<set（父进程有值；子进程用 env_remove 删掉）>".to_string(),
        Err(_) => "<unset>".to_string(),
    }
}

// ══════════════════════════════════════════════════════════════
// 最小 HTTP 客户端（⛔ 不引依赖：判据只看字节）
// ══════════════════════════════════════════════════════════════

struct Reply {
    status: u16,
    body: Vec<u8>,
}

impl Reply {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).to_string()
    }
}

fn raw_http(addr: std::net::SocketAddr, method: &str, path: &str, body: Option<&[u8]>) -> Reply {
    let mut s = TcpStream::connect(addr).expect("连不上刚绑好的端口");
    s.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
    let mut head = format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n");
    if let Some(b) = body {
        head.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            b.len()
        ));
    }
    head.push_str("\r\n");
    s.write_all(head.as_bytes()).expect("写请求头");
    if let Some(b) = body {
        s.write_all(b).expect("写请求体");
    }
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).expect("读响应");
    let split = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("响应里没有头/体分隔");
    let head_text = String::from_utf8_lossy(&buf[..split]).to_string();
    let status: u16 = head_text
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .expect("状态行读不出");
    Reply {
        status,
        body: buf[split + 4..].to_vec(),
    }
}

// ══════════════════════════════════════════════════════════════
// R1 · 一条命令到评审台（三个 env 全删）
// ══════════════════════════════════════════════════════════════

/// ⭐ **R1 的硬判据**：`build run` 全程不连库、不要口令。
///
/// 它顺带把 R2 的「与页面导出逐字节相同」也走了：页面导出（`POST /api/bundle`）拿到的
/// `built_at` 从包里读出来，再用同一个值跑 `bundle pack --dir`，比 sha256。
#[test]
fn build_run_reaches_a_review_desk_without_any_db_env() {
    if !node_ok() || !fake_acp().is_file() {
        eprintln!("SKIP（R1 真进程）：本机没有 node 或假 ACP 夹具 —— 这不是通过，这条判据**未验**。");
        return;
    }

    println!(
        "R1 · 父进程环境：ONTOK_DATABASE_URL={} · ONTOK_TEST_DATABASE_URL={} · ONTOK_SECRET_KEY={}",
        env_state(DB_ENVS[0]),
        env_state(DB_ENVS[1]),
        env_state(DB_ENVS[2]),
    );

    let root = temp_dir("chain");
    let docs = root.join("docs");
    std::fs::create_dir_all(&docs).expect("建语料目录");
    std::fs::write(
        docs.join("a.md"),
        "# 采集约束\n\n只读采集云证书与公网域名 TLS 握手结果。\n\n仅允许执行 Describe、List 类命令。\n",
    )
    .expect("写语料");
    // 工作目录的**名字就是项目名**（REQ-163 的口径）；这里故意给一个带空格 / 大写的名字，
    // 顺手证明 sanitize 与 `console serve --dir` 是同一份。
    let work = root.join("Demo Workspace");
    let out = root.join("cli.ontokbundle");

    let mut cmd = Command::new(bin());
    cmd.args([
        "build",
        "run",
        "--input",
        &docs.display().to_string(),
        "--work-dir",
        &work.display().to_string(),
        "--agent",
        "acp",
        "--agent-command",
        &format!("node {}", fake_acp().display()),
        "--agent-skill",
        &skill_fixture().display().to_string(),
        "--agent-timeout-secs",
        "60",
        "--bind",
        "127.0.0.1:0",
        "--no-browser",
    ]);
    clean_env(&mut cmd);
    cmd.env("FAKE_ACP_MODE", "valid");
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("起不了 ontok-builder build run");

    // ── stdout：收起来以供断言（另起线程，避免管道写满卡住子进程）────
    let stdout = child.stdout.take().expect("stdout");
    let (out_tx, out_rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        let mut buf = String::new();
        let mut r = stdout;
        let _ = r.read_to_string(&mut buf);
        let _ = out_tx.send(buf);
    });

    // ── stderr：逐行读，找横幅里的真实端口 ────────────────────────
    let stderr = child.stderr.take().expect("stderr");
    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            if tx.send(line.unwrap_or_default()).is_err() {
                break;
            }
        }
    });
    let mut seen: Vec<String> = Vec::new();
    let addr = loop {
        match rx.recv_timeout(Duration::from_secs(180)) {
            Ok(line) => {
                let found = line
                    .split("http://")
                    .nth(1)
                    .map(|rest| rest.split('/').next().unwrap_or(""))
                    .and_then(|authority| authority.parse::<std::net::SocketAddr>().ok());
                seen.push(line);
                if let Some(addr) = found {
                    println!("R1 · 子进程已就绪（三个库/密钥变量都不在）：http://{addr}/");
                    break addr;
                }
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                panic!(
                    "等 `build run` 的服务启动超时（{e}）—— 这条判据要证「没有库也能到评审台」，\
                     起不来就是 R1 不成立。已收到的 stderr：\n{}",
                    seen.join("\n")
                );
            }
        }
    };

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // ③ 的产物：草稿（stdout 回执 + 文件）
        let draft = work.join("out/proposal.json");
        assert!(draft.is_file(), "⛔ 没有写出草稿：{}", draft.display());
        assert!(
            work.join("merged.json").is_file(),
            "② 的产物（merged.json）应当还在工作目录里（可单跑 doc merge 复核）"
        );

        // ④ 的服务：/api/state 的 draft.items > 0（**没有口令**，直接匿名 GET）
        let r = raw_http(addr, "GET", "/api/state", None);
        assert_eq!(r.status, 200, "state: {}", r.text());
        let state = r.json();
        assert_eq!(state["draft"]["exists"], true, "{state}");
        let items = state["draft"]["items"].as_u64().unwrap_or(0);
        assert!(items > 0, "⛔ /api/state 的 draft.items = {items}（要 > 0）：{state}");
        assert!(
            state["dir"].as_str().unwrap_or("").contains("Demo Workspace"),
            "服务的工作目录必须是 --work-dir 那个：{state}"
        );

        // 签完 + 定版（这一步**是人**在页面上做的事；这里用同一批 HTTP 端点代跑）
        let r = raw_http(addr, "GET", "/api/draft", None);
        assert_eq!(r.status, 200);
        let keys: Vec<String> = r.json()["items"]
            .as_array()
            .expect("items")
            .iter()
            .map(|i| i["key"].as_str().expect("key").to_string())
            .collect();
        assert_eq!(keys.len() as u64, items, "草稿条数与 state 要一致");
        for key in &keys {
            let body = serde_json::json!({"key": key, "biz": "accepted", "model": "accepted"})
                .to_string();
            let r = raw_http(addr, "POST", "/api/decide", Some(body.as_bytes()));
            assert_eq!(r.status, 200, "decide {key}: {}", r.text());
        }
        let r = raw_http(addr, "POST", "/api/finalize", Some(b"{}"));
        assert_eq!(r.status, 200, "finalize: {}", r.text());
        assert_eq!(r.json()["seq"], 1);

        // 页面导出：`POST /api/bundle`（**页面那条路**，built_at 由 `now_rfc3339` 取）
        let r = raw_http(addr, "POST", "/api/bundle", Some(b"{}"));
        assert_eq!(r.status, 200, "bundle: {}", r.text());
        let page = r.json();
        let name = page["file"].as_str().expect("file").to_string();
        let page_path = work.join(".ontok/bundles").join(&name);
        let page_bytes = std::fs::read(&page_path).expect("页面导出的包要落盘");

        // 从页面那个包的 manifest 里读 `built_at`（R2-3 的「钉死它才算得出来」）
        let entries = ontok::bundle::zip::read(&page_bytes).expect("读包");
        let manifest: Value = serde_json::from_slice(
            &entries
                .iter()
                .find(|e| e.path == ontok::bundle::MANIFEST_PATH)
                .expect("包里要有 manifest.json")
                .data,
        )
        .expect("manifest 是合法 JSON");
        let built_at = manifest["built_at"].as_str().expect("built_at").to_string();
        println!(
            "R2 · 页面导出的包：{name}（{} 字节 · sha256 {} · built_at {built_at}）",
            page_bytes.len(),
            ontok::query::audit::sha256_hex(&page_bytes)
        );

        // ── R2 · CLI 从**同一个工作目录**出包（同一 built_at）──────────
        let mut cmd = Command::new(bin());
        cmd.args([
            "bundle",
            "pack",
            "--dir",
            &work.display().to_string(),
            "--version",
            "v1",
            "--out",
            &out.display().to_string(),
            "--built-at",
            &built_at,
        ]);
        clean_env(&mut cmd);
        let pack = cmd.output().expect("跑不了 bundle pack --dir");
        let pack_err = String::from_utf8_lossy(&pack.stderr).to_string();
        let pack_out = String::from_utf8_lossy(&pack.stdout).to_string();
        assert_eq!(
            pack.status.code(),
            Some(0),
            "`bundle pack --dir` 必须 0 退出：\n{pack_err}\n{pack_out}"
        );
        assert!(
            !pack_err.contains("ONTOK_DATABASE_URL"),
            "⛔ `--dir` 这条路不该牵扯数据库：\n{pack_err}"
        );
        let cli_bytes = std::fs::read(&out).expect("CLI 出的包要落盘");
        let page_sha = ontok::query::audit::sha256_hex(&page_bytes);
        let cli_sha = ontok::query::audit::sha256_hex(&cli_bytes);
        println!("R2 · bundle pack --dir 的包：{} 字节 · sha256 {cli_sha}", cli_bytes.len());
        assert_eq!(
            cli_bytes, page_bytes,
            "⛔ CLI 的包与页面导出的包**不逐字节相同**（同定版 + 同 built_at）——\
             这说明两条打包路径已经分家"
        );
        assert_eq!(cli_sha, page_sha);
        assert_eq!(manifest["ontology_seq"], 1);
        // 项目名 = 目录名 sanitize（`Demo Workspace` → `demo_workspace`）
        assert_eq!(
            manifest["project"].as_str(),
            Some(ontok::console::local::project_name_of_workdir(&work).as_str()),
            "包 manifest 的项目名必须来自工作目录名"
        );

        // ── 自检：包过得了自己的闸 ───────────────────────────────
        let mut cmd = Command::new(bin());
        cmd.args(["bundle", "verify", "--file", &out.display().to_string()]);
        clean_env(&mut cmd);
        let verify = cmd.output().expect("跑不了 bundle verify");
        let verify_out = String::from_utf8_lossy(&verify.stdout).to_string();
        let verify_err = String::from_utf8_lossy(&verify.stderr).to_string();
        println!("R2 · bundle verify 退出码 = {:?}", verify.status.code());
        assert!(
            verify.status.success(),
            "bundle verify 必须 0：\n{verify_err}\n{verify_out}"
        );
        assert!(verify_out.contains("\"ok\": true"), "{verify_out}");

        // ── 一致性：`--dir` 与 `--project` 互斥（点名）──────────────
        let clash = temp_dir("clash");
        let mut cmd = Command::new(bin());
        cmd.args([
            "bundle",
            "pack",
            "--dir",
            &work.display().to_string(),
            "--project",
            "demo",
            "--out",
            &clash.join("never.ontokbundle").display().to_string(),
        ]);
        clean_env(&mut cmd);
        let bad = cmd.output().expect("跑不了 bundle pack（互斥）");
        let bad_err = String::from_utf8_lossy(&bad.stderr).to_string();
        assert_eq!(
            bad.status.code(),
            Some(2),
            "两组参数互斥 ⇒ 退出 2（参数不合法）：\n{bad_err}"
        );
        for needle in ["--dir", "--project", "互斥"] {
            assert!(bad_err.contains(needle), "要点名 {needle:?}：\n{bad_err}");
        }
        assert!(
            !clash.join("never.ontokbundle").exists(),
            "⛔ 互斥这条错误必须在**产出文件之前**报出来"
        );
        std::fs::remove_dir_all(&clash).ok();
    }));

    let _ = child.kill();
    let _ = child.wait();
    let build_stdout = out_rx.recv_timeout(Duration::from_secs(30)).unwrap_or_default();
    println!("R1 · build run stdout：\n{build_stdout}");

    // 失败也要先清理，再把 panic 抛回去（否则临时目录会留在盘上）。
    let cleanup = || {
        let _ = std::fs::remove_dir_all(&root);
    };
    if let Err(panic) = result {
        cleanup();
        std::panic::resume_unwind(panic);
    }
    // ③ 的回执（**没有库**也能读到这一行）
    assert!(
        build_stdout.contains("草稿已写出"),
        "build run 的 stdout 应当有草稿回执：\n{build_stdout}"
    );
    assert!(
        build_stdout.contains("不自动 apply / freeze"),
        "R3 声明要在屏幕上：\n{build_stdout}"
    );
    cleanup();
}

// ══════════════════════════════════════════════════════════════
// R2 · 参数闸（不需要 node）
// ══════════════════════════════════════════════════════════════

#[test]
fn pack_dir_and_project_are_mutually_exclusive_and_named_in_the_error() {
    let dir = temp_dir("excl");
    let out = dir.join("never.ontokbundle");
    let mut cmd = Command::new(bin());
    cmd.args([
        "bundle",
        "pack",
        "--dir",
        &dir.display().to_string(),
        "--project",
        "demo",
        "--out",
        &out.display().to_string(),
    ]);
    clean_env(&mut cmd);
    let r = cmd.output().expect("跑不了 bundle pack");
    let err = String::from_utf8_lossy(&r.stderr).to_string();
    assert_eq!(r.status.code(), Some(2), "参数不合法 ⇒ 2：\n{err}");
    assert!(err.contains("--dir") && err.contains("--project"), "{err}");
    assert!(!out.exists(), "⛔ 拒了就不许产出文件：{}", out.display());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn pack_needs_a_source_and_says_so() {
    let dir = temp_dir("nosource");
    let out = dir.join("never.ontokbundle");
    let mut cmd = Command::new(bin());
    cmd.args([
        "bundle",
        "pack",
        "--out",
        &out.display().to_string(),
    ]);
    clean_env(&mut cmd);
    let r = cmd.output().expect("跑不了 bundle pack");
    let err = String::from_utf8_lossy(&r.stderr).to_string();
    assert_eq!(r.status.code(), Some(2), "{err}");
    // 两种用法都要写全（人照着抄就能走）。
    assert!(err.contains("--dir"), "{err}");
    assert!(err.contains("--project"), "{err}");
    assert!(!out.exists(), "⛔ 拒了就不许产出文件");
    std::fs::remove_dir_all(&dir).ok();
}

/// `--dir` 模式**不许**带读库那套的组件（带上就与页面那个包不是同一个东西了）。
#[test]
fn pack_dir_refuses_the_db_side_components() {
    let dir = temp_dir("dirwitham");
    let am = dir.join("access-mapping.json");
    std::fs::write(&am, br#"{"entities":[],"sources":[]}"#).expect("写 am");
    let out = dir.join("never.ontokbundle");
    let mut cmd = Command::new(bin());
    cmd.args([
        "bundle",
        "pack",
        "--dir",
        &dir.display().to_string(),
        "--am",
        &am.display().to_string(),
        "--out",
        &out.display().to_string(),
    ]);
    clean_env(&mut cmd);
    let r = cmd.output().expect("跑不了 bundle pack");
    let err = String::from_utf8_lossy(&r.stderr).to_string();
    assert_eq!(r.status.code(), Some(2), "{err}");
    assert!(err.contains("--am"), "要点名那个开关：{err}");
    assert!(!out.exists(), "⛔ 拒了就不许产出文件");
    std::fs::remove_dir_all(&dir).ok();
}

/// `--dir` 指向一个没有定版的工作目录 ⇒ 明确说「还没有定版」（不是「打不出包」）。
#[test]
fn pack_dir_without_a_finalized_version_says_so() {
    let dir = temp_dir("noversion");
    std::fs::create_dir_all(dir.join("out")).expect("建工作目录");
    std::fs::write(dir.join("out/proposal.json"), br#"{"items":[]}"#).expect("写草稿");
    let out = dir.join("never.ontokbundle");
    let mut cmd = Command::new(bin());
    cmd.args([
        "bundle",
        "pack",
        "--dir",
        &dir.display().to_string(),
        "--out",
        &out.display().to_string(),
    ]);
    clean_env(&mut cmd);
    let r = cmd.output().expect("跑不了 bundle pack");
    let err = String::from_utf8_lossy(&r.stderr).to_string();
    assert!(!r.status.success(), "没有定版 ⇒ 必须失败：\n{err}");
    assert!(err.contains("定版"), "要说明「导出的是定版，不是草稿」：{err}");
    assert!(!out.exists(), "⛔ 失败不许产出文件");
    std::fs::remove_dir_all(&dir).ok();
}

// ══════════════════════════════════════════════════════════════
// QA 退回项 F3 / F4 · `--bind` 的闸在**入参闸**里，而且不许建议一个不存在的开关
// ══════════════════════════════════════════════════════════════

/// ⭐ **F3**：非回环 `--bind` ⇒ 退出码 **2**，且 `--work-dir` 里**一个文件都没建**。
///
/// 为什么必须是真进程：这条要证的是「**没走** ①②③」—— 也就是「没有白烧 ACP token、
/// 没有往盘上落断言/合并/草稿」。所以既要看退出码，也要看工作目录是否根本不存在。
///
/// ⚠️ 判据里还钉了两件容易退化的东西：
/// 1. 退出码是 **2**（入参错），⛔ 不是 ④ 段那条环境错的 1；
/// 2. 文案⛔ 不许教人「加 `--allow-public`」（**F4**：REQ-163 已把那个开关从本命令删掉）。
#[test]
fn a_non_loopback_bind_is_refused_with_exit_2_and_no_work_dir() {
    let root = temp_dir("bind-gate");
    let docs = root.join("docs");
    std::fs::create_dir_all(&docs).expect("建语料目录");
    std::fs::write(docs.join("a.md"), "# 客户\n\n订单属于客户。\n").expect("写语料");
    let skill = root.join("SKILL.md");
    std::fs::write(&skill, "# 抽取契约（测试用）\n").expect("写 skill");
    // 工作目录故意**不存在**：闸门若在 ①②③ 之后，它会被建出来。
    let work = root.join("never-built");

    let mut cmd = Command::new(bin());
    cmd.args([
        "build",
        "run",
        "--input",
        &docs.display().to_string(),
        "--work-dir",
        &work.display().to_string(),
        "--agent",
        "acp",
        "--agent-command",
        "ontok-agent-that-does-not-exist-req163",
        "--agent-skill",
        &skill.display().to_string(),
        "--bind",
        "0.0.0.0:8899",
        "--no-browser",
    ]);
    clean_env(&mut cmd);
    let r = cmd.output().expect("跑不了 build run（非回环）");
    let out = String::from_utf8_lossy(&r.stdout).to_string();
    let err = String::from_utf8_lossy(&r.stderr).to_string();

    assert_eq!(
        r.status.code(),
        Some(2),
        "非回环 `--bind` 是**入参错**（2），不是 ④ 段的环境错（1）：\n{err}\n{out}"
    );
    for needle in ["--bind", "只绑回环"] {
        assert!(err.contains(needle), "要点名 {needle:?}：\n{err}");
    }
    // ⛔ F4：不许建议一个本命令没有的开关。
    assert!(
        !err.contains("请加 `--allow-public`") && !err.contains("加 --allow-public"),
        "⛔ 报错在建议一个已删掉的开关（F4）：\n{err}"
    );
    // ⛔ F3 的核心：一个文件都没建（连 work-dir 本身）。
    assert!(
        !work.exists(),
        "⛔ 非回环地址却在 `--work-dir` 里建了东西（说明闸在 ①②③ 之后）：{}",
        work.display()
    );
    // 也没走到任何一段（否则屏幕上会有段标题）。
    assert!(
        !err.contains("第 1/4 段"),
        "⛔ 地址错了却已经开始抽取：\n{err}"
    );

    // ── 反空转：同一个工作目录换成回环地址 ⇒ **过闸**（走到 ① 段才失败）──
    let mut cmd = Command::new(bin());
    cmd.args([
        "build",
        "run",
        "--input",
        &docs.display().to_string(),
        "--work-dir",
        &work.display().to_string(),
        "--agent",
        "acp",
        "--agent-command",
        "ontok-agent-that-does-not-exist-req163",
        "--agent-skill",
        &skill.display().to_string(),
        "--bind",
        "127.0.0.1:0",
        "--no-browser",
    ]);
    clean_env(&mut cmd);
    let ok = cmd.output().expect("跑不了 build run（回环）");
    let ok_err = String::from_utf8_lossy(&ok.stderr).to_string();
    assert_eq!(
        ok.status.code(),
        Some(1),
        "回环地址要过闸、走到 ① 段（那里才因为 agent 不存在而失败）：\n{ok_err}"
    );
    assert!(
        ok_err.contains("第 1/4 段"),
        "回环地址必须走到 ① 段：\n{ok_err}"
    );
    assert!(work.is_dir(), "过了闸才建工作目录（这正是 F3 的对比面）");

    std::fs::remove_dir_all(&root).ok();
}

// ══════════════════════════════════════════════════════════════
// 可发现性：`build run --help` 说的是新口径
// ══════════════════════════════════════════════════════════════

#[test]
fn the_build_run_help_describes_the_local_chain() {
    let r = Command::new(bin())
        .args(["build", "run", "--help"])
        .output()
        .expect("起不了 ontok-builder");
    assert!(r.status.success(), "--help 必须 0 退出");
    let text = String::from_utf8_lossy(&r.stdout).to_string();
    for needle in [
        "--input", "--work-dir", "--agent", "--bind", "--no-browser", "纯本地", "评审台",
    ] {
        assert!(text.contains(needle), "--help 里缺 {needle:?}：\n{text}");
    }
    // ⛔ 旧台的那几个开关（口令 / 审计 / 非回环 / 必填项目）都不该再是**选项**。
    //    ⚠️ `--allow-public` 允许出现在**说明文字**里（「末尾起的是本机评审台，没有 --allow-public」），
    //    所以只钉「它不在选项表里」（clap 的选项行有 6 个空格缩进）。
    for gone in ["--token-ttl", "--audit-out", "--base "] {
        assert!(
            !text.contains(gone),
            "--help 里不该再有 {gone:?}（REQ-163 去口令 / 去库）：\n{text}"
        );
    }
    assert!(
        !text.contains("      --allow-public"),
        "--help 的**选项表**里不该再有 --allow-public（本机评审台只绑回环）：\n{text}"
    );
    // `--project` 还在，但**不再是必填**（`--help` 里显示为可选；这里只钉它存在）。
    assert!(text.contains("--project"), "--help 里要能看到 --project：\n{text}");
}

/// 工作目录里已经有 v1 时，`build run` 的草稿抬头必须是 v2（缺省 = 最新定版 + 1）。
///
/// ⚠️ 这条不跑 agent：只读目录（`plan_base_seq`）—— 判据在
/// `src/review_open/build_run.rs` 的单元测试里；这里钉的是**路径约定**那一半：
/// `--dir` 在 `<work>/.ontok/versions/vN/` 下找快照（页面与 CLI 同一个目录）。
#[test]
fn the_dir_mode_reads_versions_from_the_same_place_the_page_writes_them() {
    let dir = temp_dir("layout");
    std::fs::create_dir_all(dir.join("out")).expect("建工作目录");
    std::fs::write(dir.join("out/proposal.json"), br#"{"items":[]}"#).expect("写草稿");
    // 页面写在这里（`local::VERSIONS_REL`），CLI 必须从这里读。
    let versions = dir.join(ontok::console::local::VERSIONS_REL);
    assert_eq!(
        versions,
        dir.join(".ontok/versions"),
        "定版目录的约定变了：页面与 CLI 会读不到彼此的文件"
    );
    std::fs::create_dir_all(versions.join("v7")).expect("造一个定版目录");
    // 目录在、快照坏 ⇒ 报错要点名那个文件（而不是笼统「打不出包」）。
    let out = dir.join("bad.ontokbundle");
    let mut cmd = Command::new(bin());
    cmd.args([
        "bundle",
        "pack",
        "--dir",
        &dir.display().to_string(),
        "--out",
        &out.display().to_string(),
    ]);
    clean_env(&mut cmd);
    let r = cmd.output().expect("跑不了 bundle pack");
    let err = String::from_utf8_lossy(&r.stderr).to_string();
    assert!(!r.status.success(), "缺 snapshot.json ⇒ 必须失败：\n{err}");
    assert!(
        err.contains("snapshot.json") || err.contains("定版"),
        "要点名缺的是什么：{err}"
    );
    assert!(!out.exists());
    std::fs::remove_dir_all(&dir).ok();
}

/// 未使用的路径辅助（保留给将来加判据时用；⛔ 不引入 dead_code 噪音）。
#[allow(dead_code)]
fn _path_hint(p: &Path) -> String {
    p.display().to_string()
}

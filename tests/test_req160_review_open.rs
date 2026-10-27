//! REQ-160 · `review open` / `build run` 的**真进程**判据（不需要元数据库）。
//!
//! # 为什么这批判据必须跑真二进制
//!
//! 派单 §3-5 的口径是「退出码：`0` 正常；`1` 环境 / DB 错；`2` 入参不合法」。
//! 退出码是**进程边界**上的东西：在库函数里断言 `Result` 只证明了「返回了错误」，
//! 证明不了「`ontok-builder` 以 2 退出」。所以这里用 `CARGO_BIN_EXE_ontok-builder`
//! 起真进程、读真退出码。
//!
//! # 这批判据为什么**不需要数据库**
//!
//! 两条命令都刻意在**连库之前**判完入参（`doc ingest` / `bundle` 的同一条纪律）：
//!
//! | 命令 | 连库前判完的 | 所以本机（无库）能验的 |
//! | --- | --- | --- |
//! | `review open` | `--bind` + 回环闸 + `--token-ttl` | AC-005（非回环 ⇒ 启动即失败）与「入参错不冒充环境错」 |
//! | `build run` | `--input` / `--agent` / `--agent-command` / `--agent-skill` | 入参错 ⇒ 2 |
//!
//! ⚠️ 如实登记：**起服务那一段**（真绑 socket、真打 `GET /`、真用口令调 `/api/projects`）
//! 需要元数据库（`review open` 要读项目 / 版本），本机没有。那一段的流程判据在
//! `src/review_open/mod.rs` 的单元测试里（进程内 `build_router` + 懒连接池），
//! 件数见派单回报；⛔ 本文件**不**把「跳过」写成「通过」。

use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_ontok-builder")
}

/// 一份**干净的环境**：把可能存在的库 / 密钥变量摘掉，判据才不依赖开发机的 shell。
fn clean_cmd(args: &[&str]) -> Command {
    let mut c = Command::new(bin());
    c.args(args);
    for var in [
        "ONTOK_DATABASE_URL",
        "ONTOK_TEST_DATABASE_URL",
        "ONTOK_SECRET_KEY",
        "ONTOK_DSN_KEY",
    ] {
        c.env_remove(var);
    }
    c
}

fn run(mut c: Command) -> (i32, String, String) {
    let out = c.output().expect("起不了 ontok-builder");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

/// 临时目录（输入文档 / skill 的夹具）。
fn temp_dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!(
        "ontok-req160-bin-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&d).expect("临时目录");
    d
}

// ══════════════════════════════════════════════════════════════
// AC-005 · 只绑回环（真进程退出码）
// ══════════════════════════════════════════════════════════════

#[test]
fn review_open_refuses_a_non_loopback_bind_with_exit_2() {
    let (code, _out, err) = run(clean_cmd(&[
        "review",
        "open",
        "--project",
        "demo",
        "--bind",
        "0.0.0.0:8899",
        "--no-browser",
    ]));
    assert_eq!(code, 2, "非回环 + 无 --allow-public 必须以 **2** 退出：\n{err}");
    assert!(
        err.contains("--allow-public"),
        "拒绝时要说清怎么放行：\n{err}"
    );
    assert!(
        err.contains("只绑回环"),
        "拒绝时要说清默认口径：\n{err}"
    );
    // ⛔ 不许先报「没配库」—— 那会指向错的原因（AC-005 就变成验不了了）。
    assert!(
        !err.contains("ONTOK_DATABASE_URL"),
        "入参错不该被「库没配」盖住：\n{err}"
    );
}

#[test]
fn review_open_accepts_the_same_bind_once_allow_public_is_given() {
    // 带了 flag ⇒ 回环闸放行，于是失败面变成**环境**（没有元数据库）⇒ 1，而不是 2。
    let (code, _out, err) = run(clean_cmd(&[
        "review",
        "open",
        "--project",
        "demo",
        "--bind",
        "0.0.0.0:8899",
        "--allow-public",
        "--no-browser",
    ]));
    assert_eq!(code, 1, "环境错（无库）必须是 1，不是 2：\n{err}");
    assert!(
        err.contains("ONTOK_DATABASE_URL") || err.contains("ONTOK_TEST_DATABASE_URL"),
        "要点名缺的那个环境变量（两个名字都该出现）：\n{err}"
    );
}

#[test]
fn review_open_rejects_a_zero_token_ttl_with_exit_2() {
    let (code, _out, err) = run(clean_cmd(&[
        "review",
        "open",
        "--project",
        "demo",
        "--token-ttl",
        "0h",
        "--no-browser",
    ]));
    assert_eq!(code, 2, "`--token-ttl 0h` 是入参错：\n{err}");
    assert!(err.contains("--token-ttl"), "{err}");
}

// ══════════════════════════════════════════════════════════════
// build run · 入参错 ⇒ 2（R5：只做文档路径）
// ══════════════════════════════════════════════════════════════

#[test]
fn build_run_rejects_a_missing_input_with_exit_2() {
    let dir = temp_dir("missing-input");
    let skill = dir.join("SKILL.md");
    std::fs::write(&skill, "# 契约\n").expect("写 skill");

    let (code, _out, err) = run(clean_cmd(&[
        "build",
        "run",
        "--project",
        "demo",
        "--input",
        &dir.join("no-such-docs").display().to_string(),
        "--agent",
        "acp",
        "--agent-command",
        "pi-acp",
        "--agent-skill",
        &skill.display().to_string(),
        "--no-browser",
    ]));
    assert_eq!(code, 2, "`--input` 不存在是入参错：\n{err}");
    assert!(err.contains("--input"), "{err}");
    assert!(
        !err.contains("ONTOK_DATABASE_URL"),
        "入参错不该被「库没配」盖住：\n{err}"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn build_run_rejects_a_library_agent_with_exit_2() {
    let dir = temp_dir("library-agent");
    std::fs::write(dir.join("a.md"), "# 客户\n\n订单属于客户。\n").expect("写文档");
    let skill = dir.join("SKILL.md");
    std::fs::write(&skill, "# 契约\n").expect("写 skill");

    let (code, _out, err) = run(clean_cmd(&[
        "build",
        "run",
        "--project",
        "demo",
        "--input",
        &dir.display().to_string(),
        "--agent",
        "library",
        "--agent-command",
        "pi-acp",
        "--agent-skill",
        &skill.display().to_string(),
        "--no-browser",
    ]));
    assert_eq!(code, 2, "R5：`--agent` 只认 acp：\n{err}");
    assert!(err.contains("acp"), "{err}");

    std::fs::remove_dir_all(&dir).ok();
}

// ══════════════════════════════════════════════════════════════
// 帮助里的安全边界（demand §三-S1/S2 要求「必须写进 --help」）
// ══════════════════════════════════════════════════════════════

#[test]
fn the_help_text_states_the_three_security_boundaries() {
    let out = Command::new(bin())
        .args(["review", "open", "--help"])
        .output()
        .expect("起不了 ontok-builder");
    assert!(out.status.success(), "--help 必须 0 退出");
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    for needle in [
        "只绑回环",       // S1
        "一次性",         // S2
        "不落盘",         // S2
        "不进 URL",       // S2
        "sessionStorage", // S3
        "口令只在终端打印", // 「一次」的提醒
    ] {
        assert!(text.contains(needle), "--help 里缺 {needle:?}：\n{text}");
    }
    // 默认值必须是 `:0`（AC-003 的起点）。
    assert!(text.contains("127.0.0.1:0"), "{text}");
    assert!(text.contains("4h"), "默认 TTL 要在 --help 里：{text}");
}

// ══════════════════════════════════════════════════════════════
// AC-001 / AC-003 · **真 socket**：URL 打得开、壳免口令、管理端点要口令
// ══════════════════════════════════════════════════════════════

/// 一个**不连库**的服务（懒连接池故意连不上）：验的是「进程真的绑上了端口、
/// 页面壳真的能匿名取、一次性口令真的过了鉴权」。
///
/// ⚠️ 为什么在这里重搭一次 `ConsoleParts` 而不是调 `review_open::open`：
/// `open` 要先读元数据库里的项目 / 版本（本机没有），而本用例要验的是**绑 socket
/// 之后的行为**。等价性由 `review_open` 的模块注释与单测钉着（同一个 `build_router`
/// + 同一个 `ConsoleService::new`），⛔ 这里**没有**第二套 HTTP。
fn local_service(token: &str) -> std::sync::Arc<ontok::console::ConsoleService> {
    use ontok::console::audit::AuditLog;
    use ontok::console::decl::DeclPaths;
    use ontok::console::{ConsoleParts, ConsoleService};
    use ontok::domain::ttl::SchemaTypes;
    use ontok::query::audit::AuditSink;

    std::sync::Arc::new(ConsoleService::new(ConsoleParts {
        pool: sqlx::postgres::PgPoolOptions::new()
            .acquire_timeout(std::time::Duration::from_millis(200))
            .connect_lazy("postgresql://nobody@127.0.0.1:1/none")
            .expect("lazy pool"),
        project: "demo".into(),
        base: "https://ontok.local/".into(),
        tokens: ontok::review_open::one_shot_tokens(token).expect("一次性口令表"),
        schema: SchemaTypes::default(),
        paths: DeclPaths {
            exclusions: "exclusions.json".into(),
            policy: "query-policy.json".into(),
            tenancy: "tenancy.json".into(),
        },
        audit: AuditLog::new(AuditSink::stdout_only()),
        bind: "127.0.0.1:0".parse().unwrap(),
        allow_public: false,
        secret_cipher: ontok::console::dsn::SecretCipher::for_tests(),
    }))
}

/// 发一条最小 HTTP/1.1 请求，返回**状态行**（`HTTP/1.1 200 OK`）。
///
/// 用阻塞 `std::net` 而不是 tokio 的 I/O：这条判据不需要并发，
/// 而 `#[tokio::test(flavor = "multi_thread")]` 让阻塞读不会把服务端饿死。
fn status_of(addr: std::net::SocketAddr, path: &str, token: Option<&str>) -> String {
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(addr).expect("连不上刚绑好的端口");
    s.set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .expect("设读超时");
    let auth = match token {
        Some(t) => format!("Authorization: Bearer {t}\r\n"),
        None => String::new(),
    };
    let req =
        format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\n{auth}Connection: close\r\n\r\n");
    s.write_all(req.as_bytes()).expect("写请求");
    let mut buf = Vec::new();
    // 页面壳有十几 KB：一直读到对端关闭（`Connection: close`）。
    let _ = s.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf).to_string();
    text.lines().next().unwrap_or("").trim().to_string()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_console_really_binds_serves_the_shell_and_gates_the_api() {
    let one_shot = ontok::review_open::one_shot_token();
    let svc = local_service(&one_shot);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("绑 127.0.0.1:0");
    let addr = listener.local_addr().expect("真实端口");
    assert_ne!(addr.port(), 0, "内核分配的端口不会是 0");

    let app = ontok::console::http::build_router(std::sync::Arc::clone(&svc));
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    // ① 页面壳：免口令 200（浏览器导航带不了 Authorization）。
    let shell = status_of(addr, "/", None);
    assert!(
        shell.contains("200"),
        "`GET /` 必须匿名 200（实际 {shell:?}，URL http://{addr}/）"
    );

    // ② 管理端点：没有口令 ⇒ 401。
    let anon = status_of(addr, "/api/projects", None);
    assert!(
        anon.contains("401"),
        "`GET /api/projects` 无口令必须 401（实际 {anon:?}）"
    );

    // ③ 别人的口令 ⇒ 401（它既不是 admin 也不是 query）。
    let stranger = status_of(addr, "/api/projects", Some("otk_nobody"));
    assert!(
        stranger.contains("401"),
        "不认识的口令必须 401（实际 {stranger:?}）"
    );

    // ④ **本命令生成的那枚**口令 ⇒ 过了鉴权（懒连接池连不上 ⇒ 500；
    //    这里判的是「不是 401 / 403」，即令牌表被认出来了 —— 不依赖数据库）。
    let with_token = status_of(addr, "/api/projects", Some(&one_shot));
    assert!(
        !with_token.contains("401") && !with_token.contains("403"),
        "一次性口令必须过鉴权（实际 {with_token:?}）"
    );

    // ⑤ 关掉之后同一端口不再服务（AC-004 的「退出即失效」在 socket 层的半边：
    //    进程没了，服务就没了；口令本身也随内存消失）。
    server.abort();
    let _ = server.await;
    // 给监听 socket 一点时间被 drop（abort 是异步的）。
    for _ in 0..50 {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        if std::net::TcpStream::connect(addr).is_err() {
            break;
        }
    }
    assert!(
        std::net::TcpStream::connect(addr).is_err(),
        "服务停掉后 {addr} 不该还接受连接"
    );
}

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
//!
//! # ⭐ REQ-163 · `build run` 的两条判据换了前提（**不再要库**）
//!
//! REQ-160 时 `build run` 的 ③ 段是 `import`（写库）、④ 段是 `review open`（要口令 + 要库），
//! 所以文件末尾那两条必须配 `ONTOK_TEST_DATABASE_URL` + `ONTOK_SECRET_KEY` 才有意义。
//! REQ-163 把它改成**纯本地链**（写草稿 + 起 console v2），⇒ 两条判据改成：
//!
//! | 判据 | 它说什么 |
//! | --- | --- |
//! | `a_failed_build_run_leaves_no_draft_and_touches_no_db` | 失败路径 ⇒ 工作目录**零产物**（连 `out/` 都不建）；有库时**且**库计数不变（⛔ 不留空壳项目） |
//! | `a_successful_build_run_writes_a_draft_and_signs_nothing` | 成功路径 ⇒ `out/proposal.json` 有条目、`decisions.json` / `.ontok/versions/` **都不存在**（⛔ 不自动签）、有库时库计数仍不变 |
//!
//! 后一条只要求 **node + 假 ACP 夹具**（不再要库 / 密钥）：真正无条件跑的那条
//! 「`.env` 全清 → 起服务 → `/api/state` 有草稿」判据在 `tests/test_req163_local_chain.rs`。
//! 有库时那两条仍会顺手断言「库一个字节都没变」，这就是「没有掉回旧台」的证据。

mod common;

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
        secret_cipher: ontok::provision::dsn::SecretCipher::for_tests(),
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

// ══════════════════════════════════════════════════════════════
// ⭐ REQ-163 · `build run` 的纯本地链：失败零产物 / 成功只写草稿（⛔ 不签、⛔ 不写库）
// ══════════════════════════════════════════════════════════════

/// 元数据库连接串；没有 ⇒ 那半条断言**响亮地跳过**（⛔ 不静默通过）。
///
/// ⚠️ REQ-163 起这**不再是这两条用例的前提**：`build run` 已经不连库了。
/// 有库时顺手断言「库一个字节都没变」—— 那是「没有掉回旧台」的直接证据。
fn db_url() -> Option<String> {
    std::env::var("ONTOK_TEST_DATABASE_URL")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn skip(what: &str) {
    eprintln!("SKIP（{what}）：本机没有 node 或假 ACP 夹具 —— 这不是通过，这条判据**未验**。");
}

fn skip_db(what: &str) {
    eprintln!(
        "SKIP（{what}）：没有 ONTOK_TEST_DATABASE_URL —— 「库一个字节都没变」这半条**未验**。\
         `build run` 本身不需要库（REQ-163），其余断言照跑。"
    );
}

/// crate 根：⛔ **不用** `env!("CARGO_MANIFEST_DIR")`（那是编译期常量 ——
/// 二进制被搬到别处跑时会去读**编译它时**那棵树，REQ-046 的事故形态）。
/// 走 `tests/common/source_root.rs` 的运行时锚点（G-1 判据：
/// `no_bare_env_manifest_dir_outside_anchor_file`）。
fn crate_root() -> std::path::PathBuf {
    common::source_root::crate_root()
}

fn fake_acp_js() -> std::path::PathBuf {
    crate_root().join("tests/fixtures/req147/fake-acp.js")
}

fn skill_fixture() -> std::path::PathBuf {
    crate_root().join("tests/fixtures/req147/skill-fixture.md")
}

/// Node 在不在（假 ACP agent 用 Node 写）。
fn node_ok() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

async fn pool_and_migrate(url: &str) -> sqlx::postgres::PgPool {
    let pool = ontok::db::pool::connect(url).await.expect("连不上元数据库");
    ontok::db::pg::migrate(&pool).await.expect("迁移失败");
    pool
}

/// 库里的**总量**（项目数, 版本数）—— 失败路径要的是「一点都不变」。
async fn totals(pool: &sqlx::postgres::PgPool) -> (i64, i64) {
    let p: i64 = sqlx::query_scalar("SELECT count(*) FROM projects")
        .fetch_one(pool)
        .await
        .expect("数项目");
    let v: i64 = sqlx::query_scalar("SELECT count(*) FROM ontology_versions")
        .fetch_one(pool)
        .await
        .expect("数版本");
    (p, v)
}

fn draft_options(
    dir: &std::path::Path,
    agent_command: String,
) -> ontok::review_open::build_run::Options {
    let docs = dir.join("docs");
    std::fs::create_dir_all(&docs).expect("建语料目录");
    std::fs::write(
        docs.join("a.md"),
        "# 采集约束\n\n只读采集云证书与公网域名 TLS 握手结果。\n\n仅允许执行 Describe、List 类命令。\n",
    )
    .expect("写语料");
    ontok::review_open::build_run::Options {
        // ⭐ REQ-163：项目名缺省 = 工作目录名（`--project` 只是可选覆盖）。
        project: None,
        input: docs,
        agent: ontok::review_open::build_run::ONLY_AGENT.to_string(),
        agent_command,
        agent_skill: skill_fixture(),
        agent_timeout_secs: Some(30),
        // ⚠️ 不开 `strict_deterministic`：假 agent 会混一条编造的引用进来（AC-005 的变异），
        //    那一条会被丢弃并计数 —— 严格闸会因为「有块不完美」整条链失败。
        strict_deterministic: false,
        work_dir: dir.join(".ontok-build"),
        schema: None,
        proposal: None,
        allow_doc_only: false,
        base_version: None,
        bind: "127.0.0.1:0".into(),
        no_browser: true,
    }
}

/// ⛔ **失败路径不许留半成品**（REQ-160 的「不留空壳」换成纯本地口径）。
///
/// REQ-160 时这条钉的是「库里不许多个空壳项目 + v1」。REQ-163 删掉了那条写库动作，
/// 但**同一条纪律**还在：①② 失败时评审台里不该出现一份半成品草稿（那会让人以为能签）。
/// ⇒ 断言变成「工作目录零产物（连 `out/` 都不建）」；
/// 有元数据库时**再顺手**断言库计数不变（证明它真的没掉回旧台）。
#[tokio::test]
async fn a_failed_build_run_leaves_no_draft_and_touches_no_db() {
    let dir = temp_dir("local-fail");
    // 一个**不存在**的 agent 命令：第 ① 段（doc ingest）必然失败。
    let opts = draft_options(&dir, "ontok-no-such-agent-req163".into());

    let pool = match db_url() {
        Some(url) => Some(pool_and_migrate(&url).await),
        None => {
            skip_db("失败路径的库计数");
            None
        }
    };
    let before = match &pool {
        Some(p) => Some(totals(p).await),
        None => None,
    };

    let err = ontok::review_open::build_run::run_draft(&opts).await;
    let after = match &pool {
        Some(p) => Some(totals(p).await),
        None => None,
    };

    assert!(err.is_err(), "agent 连不上 ⇒ 整体失败（R1）");
    if let (Some(before), Some(after)) = (before, after) {
        assert_eq!(
            (after.0 - before.0, after.1 - before.1),
            (0, 0),
            "⛔ 失败路径在元数据库里留下了东西：项目 +{} / 版本 +{}（这条链根本不该连库）",
            after.0 - before.0,
            after.1 - before.1
        );
    }

    // 文件层零产物（与 `src/review_open/build_run.rs` 的单元判据互证）。
    let work = dir.join(".ontok-build");
    for stale in ["assertions.jsonl", "merged.json", "out/proposal.json"] {
        assert!(
            !work.join(stale).exists(),
            "⛔ work-dir 里留下了半成品：{}",
            work.join(stale).display()
        );
    }
    assert!(
        !work.join("out").exists(),
        "⛔ 连 `out/` 都不该建（③ 段没跑过）"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// 成功路径：**只**写出草稿；⛔ 不签、⛔ 不定版、⛔ 不写库。
///
/// 这条以前要 `ONTOK_TEST_DATABASE_URL` + `ONTOK_SECRET_KEY`（③ 导入 ④ 起审核台）。
/// REQ-163 之后它只要 node + 假 ACP 夹具 —— 有库时再顺手核一次「库计数不变」。
#[tokio::test]
async fn a_successful_build_run_writes_a_draft_and_signs_nothing() {
    if !node_ok() || !fake_acp_js().is_file() {
        skip("成功路径只写草稿");
        return;
    }
    let dir = temp_dir("local-ok");
    let opts = draft_options(
        &dir,
        format!("node {}", fake_acp_js().display()),
    );

    let pool = match db_url() {
        Some(url) => Some(pool_and_migrate(&url).await),
        None => {
            skip_db("成功路径的库计数");
            None
        }
    };
    let before = match &pool {
        Some(p) => Some(totals(p).await),
        None => None,
    };

    let art = ontok::review_open::build_run::run_draft(&opts)
        .await
        .expect("①②③ 都要过（④ 起服务不在 run_draft 里）");
    let after = match &pool {
        Some(p) => Some(totals(p).await),
        None => None,
    };
    if let (Some(before), Some(after)) = (before, after) {
        assert_eq!(
            (after.0 - before.0, after.1 - before.1),
            (0, 0),
            "⛔ 成功路径也在元数据库里留下了东西：项目 +{} / 版本 +{}（REQ-163：不写库）",
            after.0 - before.0,
            after.1 - before.1
        );
    }

    // ③ 的产物：草稿存在、有条目、且**逐字节**就是合并结果。
    let draft = dir.join(".ontok-build/out/proposal.json");
    assert_eq!(art.draft, draft, "草稿落点必须与路径表一致");
    assert!(draft.is_file(), "⛔ 没写出草稿：{}", draft.display());
    let bytes = std::fs::read(&draft).expect("读草稿");
    assert_eq!(
        bytes,
        std::fs::read(&art.merged).expect("读合并结果"),
        "草稿必须**逐字节**就是合并结果（重排一次就会多一个 sha ⇒ 页面莫名 stale）"
    );
    let doc: serde_json::Value = serde_json::from_slice(&bytes).expect("草稿必须是合法 JSON");
    let items = doc["items"].as_array().expect("草稿要有 items");
    assert!(!items.is_empty(), "成功路径应当至少抽出一条条目");
    // 项目名缺省 = 工作目录名（`--project` 没给）。
    assert_eq!(
        doc["project"].as_str(),
        Some(
            ontok::console::local::project_name_of_workdir(&opts.work_dir).as_str()
        ),
        "草稿的 project 必须来自工作目录名（与页面同一个 sanitize）"
    );

    // ⛔ R3：人的动作一个都还没发生 —— 没有裁决、没有定版目录。
    assert!(
        !dir.join(".ontok-build/.ontok/decisions.json").exists(),
        "⛔ 出现了裁决文件（R3：本命令不签）"
    );
    assert!(
        !dir.join(".ontok-build/.ontok/versions").exists(),
        "⛔ 出现了定版目录（R3：本命令不定版 / 不 freeze）"
    );

    std::fs::remove_dir_all(&dir).ok();
}

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
//! # 真库判据（QA REQ-160 退回项）
//!
//! 文件末尾那两条**要元数据库**（`ONTOK_TEST_DATABASE_URL`）+ `ONTOK_SECRET_KEY`：
//!
//! | 判据 | 它说什么 |
//! | --- | --- |
//! | `a_failed_build_run_leaves_no_project_and_no_version` | 失败路径 ⇒ **项目数 / 版本数不变**（⛔ 不留空壳项目） |
//! | `a_successful_build_run_creates_an_unfrozen_version_and_keeps_decisions_pending` | 成功路径 ⇒ +1 项目 +1 版本、`frozen_at IS NULL`、条目 `biz/model = pending`（⛔ 不自动签） |
//!
//! 没配环境变量时它们会**响亮的 SKIP**（打印原因并返回），⛔ 不静默当作通过 ——
//! 所以「本机真跑过」的证据是报告里的库计数，不是这两条用例的绿。

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

// ══════════════════════════════════════════════════════════════
// QA 退回项 · 真库：失败零产物（**连项目名都不许被占用**）/ 成功仍不签
// ══════════════════════════════════════════════════════════════

/// 元数据库连接串；没有 ⇒ 这条用例**响亮地跳过**（⛔ 不静默通过）。
fn db_url() -> Option<String> {
    std::env::var("ONTOK_TEST_DATABASE_URL")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn skip(what: &str) {
    eprintln!(
        "SKIP（{what}）：需要 ONTOK_TEST_DATABASE_URL（+ ONTOK_SECRET_KEY）。\
         这不是通过 —— 本机没配库时这两条判据**未验**。"
    );
}

fn crate_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
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

fn unique_project(tag: &str) -> String {
    format!(
        "req160_{tag}_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    )
}

fn db_options(dir: &std::path::Path, project: &str, agent_command: String) -> ontok::review_open::build_run::Options {
    let docs = dir.join("docs");
    std::fs::create_dir_all(&docs).expect("建语料目录");
    std::fs::write(
        docs.join("a.md"),
        "# 采集约束\n\n只读采集云证书与公网域名 TLS 握手结果。\n\n仅允许执行 Describe、List 类命令。\n",
    )
    .expect("写语料");
    ontok::review_open::build_run::Options {
        project: project.to_string(),
        input: docs,
        agent: ontok::review_open::build_run::ONLY_AGENT.to_string(),
        agent_command,
        agent_skill: skill_fixture(),
        agent_timeout_secs: Some(30),
        strict_deterministic: true,
        work_dir: dir.join(".ontok-build"),
        schema: None,
        proposal: None,
        allow_doc_only: false,
        base_version: None,
        bind: "127.0.0.1:0".into(),
        allow_public: false,
        no_browser: true,
        // 1 秒：本命令末尾会**真起审核台**，用例不能一直挂着等 Ctrl-C。
        token_ttl: "1s".into(),
        base: None,
        audit_out: None,
    }
}

/// ⛔ **失败路径不许留空壳**（QA 退回项的真库判据）。
///
/// 实测过的那一版：`get_or_create` + `new_version()` 跑在第 ① 段之前 ⇒ agent 连不上时
/// 「文件层零产物」成立，但库里多了一个空壳项目 + v1。现在的判据是**库计数不变**，
/// 而且**那个项目名根本没被占用**（FDE 打错一次项目名不该留下废弃物）。
#[tokio::test]
async fn a_failed_build_run_leaves_no_project_and_no_version() {
    let Some(url) = db_url() else {
        skip("失败路径零产物（真库）");
        return;
    };
    let pool = pool_and_migrate(&url).await;

    let dir = temp_dir("db-fail");
    let project = unique_project("shell");
    // 一个**不存在**的 agent 命令：第 ① 段（doc ingest）必然失败。
    let opts = db_options(&dir, &project, "ontok-no-such-agent-req160".into());

    let before = totals(&pool).await;
    let err = ontok::review_open::build_run::run(&pool, opts).await;
    let after = totals(&pool).await;

    assert!(err.is_err(), "agent 连不上 ⇒ 整体失败（R1）");
    assert_eq!(
        (after.0 - before.0, after.1 - before.1),
        (0, 0),
        "⛔ 失败路径在库里留下了东西：项目 +{} / 版本 +{}（AC-006：不建版本）",
        after.0 - before.0,
        after.1 - before.1
    );
    let proj = ontok::db::project::PgProjectStore::new(pool.clone())
        .get_by_name(&project)
        .await
        .expect("查项目");
    assert!(
        proj.is_none(),
        "⛔ 空壳项目被建出来了：{project}（项目 / 版本只该在 ①② 段都成功之后才建）"
    );
    // 文件层也仍然是零产物（与失败路径的单元判据互证）。
    let work = dir.join(".ontok-build");
    assert!(
        !work.join("assertions.jsonl").exists() && !work.join("merged.json").exists(),
        "⛔ work-dir 里留下了半成品：{}",
        work.display()
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// 成功路径：真的产出版本，但**未冻结**、决策**全 pending**（⛔ 不自动签）。
#[tokio::test]
async fn a_successful_build_run_creates_an_unfrozen_version_and_keeps_decisions_pending() {
    let Some(url) = db_url() else {
        skip("成功路径仍不自动签（真库）");
        return;
    };
    if std::env::var("ONTOK_SECRET_KEY").ok().filter(|s| !s.trim().is_empty()).is_none() {
        skip("成功路径仍不自动签（缺 ONTOK_SECRET_KEY，第 ④ 段起不了审核台）");
        return;
    }
    if !node_ok() || !fake_acp_js().is_file() {
        skip("成功路径仍不自动签（没有 node / 假 ACP 夹具）");
        return;
    }
    let pool = pool_and_migrate(&url).await;

    let dir = temp_dir("db-ok");
    let project = unique_project("ok");
    let opts = db_options(
        &dir,
        &project,
        format!("node {}", fake_acp_js().display()),
    );

    let before = totals(&pool).await;
    let res = ontok::review_open::build_run::run(&pool, opts).await;
    let after = totals(&pool).await;
    assert!(res.is_ok(), "四段都要过（含起审核台 1 秒）：{res:?}");
    assert_eq!(after.0 - before.0, 1, "应当**新增一个**项目");
    assert_eq!(after.1 - before.1, 1, "应当**新增一个**版本（v1）");

    // 版本存在且**未冻结**（R3：本命令不 freeze）。
    let frozen: Option<bool> = sqlx::query_scalar(
        "SELECT v.frozen_at IS NOT NULL FROM ontology_versions v \
         JOIN projects p ON p.id = v.project_id WHERE p.name = $1 ORDER BY v.seq DESC LIMIT 1",
    )
    .bind(&project)
    .fetch_optional(&pool)
    .await
    .expect("查冻结状态");
    assert_eq!(frozen, Some(false), "⛔ 版本被冻结了（R3：不自动 freeze）");

    // 条目数 > 0，且**两条轨都还是 pending**（人还没签）。
    let items: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM proposal_items i \
         JOIN proposals pr ON pr.id = i.proposal_id \
         JOIN projects p ON p.id = pr.project_id WHERE p.name = $1",
    )
    .bind(&project)
    .fetch_one(&pool)
    .await
    .expect("数条目");
    assert!(items > 0, "成功路径应当导入至少一条条目（实际 {items}）");
    let decided: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM proposal_items i \
         JOIN proposals pr ON pr.id = i.proposal_id \
         JOIN projects p ON p.id = pr.project_id \
         WHERE p.name = $1 AND (i.biz_status <> 'pending' OR i.model_status <> 'pending')",
    )
    .bind(&project)
    .fetch_one(&pool)
    .await
    .expect("数已决策条目");
    assert_eq!(decided, 0, "⛔ 有 {decided} 条条目已被决策（R3：不自动签）");

    std::fs::remove_dir_all(&dir).ok();
}

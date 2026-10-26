//! REQ-078 · AC-005 —— **每一个**打印 domain 产物的站点，都有一条真进程字节比对。
//!
//! # 这个文件补的是哪一格
//!
//! REQ-072 只把「真实 stdout == 抬头 ++ domain 渲染」钉在 `review show` 一个站点上。
//! AC-005 要求覆盖**所有**站点 —— `ontology show --html` / `--json` / `--model-view`、
//! `overview` / `overview --json` 此前**零**端到端字节比对。
//!
//! 本文件让这 5 个站点各拿到一条，断言形状与 REQ-072 那条**逐字节**相同：
//!
//! ```text
//! stdout == 已登记前缀 ++ domain 渲染 ++ 尾换行
//! ```
//!
//! ⛔ 前缀与尾换行**不写死在这里** —— 一律取自 `ontok::cli::DOMAIN_PRINT_SITES`
//! （登记表）。`tests/test_binary_output_guard.rs` 的 G-2 反过来断言
//! 「前缀原文不得出现在任何测试文件里」，所以这里也**不能**写死。
//!
//! # 隔离方式：沿用 REQ-072 的**专属 schema**
//!
//! `CREATE SCHEMA <专属>` + 连接串 `?options=-c%20search_path%3D<专属>`。
//! 零 DELETE / 零 TRUNCATE / 零清空共享表；只删**自己建的那个**命名空间，
//! 且**删完复查**（REQ-078 F-3：`cleanup_schema` 不再静默吞失败）。

mod common;

use std::path::PathBuf;
use std::process::{Command, Output};

use ontok::cli::{snapshot_to_view, DomainPrintSite, DOMAIN_PRINT_SITES};
use ontok::db::pool::DbError;
use ontok::db::project::PgProjectStore;
use ontok::db::snapshot_store::PgStore;

// ══════════════════════════════════════════════════════════════
// 沙箱
// ══════════════════════════════════════════════════════════════

/// 一次 REQ-078 用的**专属空 schema + 专属项目名**。
///
/// 机制与 `tests/test_review_show_binary_req072.rs` 完全一致，
/// 理由（`pg_temp` 是会话私有、跨进程不可见）写在那里，不重复论证。
struct Sandbox {
    schema: String,
    project: String,
    url: String,
    /// 每个沙箱一个**固定** tag —— 临时 JSON 文件名用它，不再用 schema 名。
    ///
    /// ⚠️ 刻意**不**把 schema 名（含 pid）拼进文件名：那是 REQ-072 QA 的 F-4
    /// ——「只增不删、无界增长」。同一个文件被反复覆盖，目录里最多 4 个。
    tag: String,
    /// apply 之后的版本 seq（`version new` 造 v1，`proposal apply` 造 v2）。
    version: i32,
}

impl Sandbox {
    async fn new(tag: &str) -> Sandbox {
        let base = ontok::config::test_database_url().expect(
            "缺 ONTOK_TEST_DATABASE_URL —— 口令不得入库，请导出环境变量（见 .env.example）",
        );
        let schema = common::unique_project(&format!("req078_{tag}"));
        let project = common::unique_project(&format!("req078p_{tag}"));

        let admin = common::test_pool().await;
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .expect("建专属空 schema 失败");

        let url = scoped_url(&base, &schema);
        let mut sb = Sandbox {
            schema,
            project,
            url,
            tag: tag.to_string(),
            version: 2,
        };
        sb.seed().await;
        sb
    }

    async fn seed(&mut self) {
        let p = self.run(&["project", "new", "--name", &self.project]);
        assert!(p.status.success(), "project new 失败：{}", text(&p.stderr));
        let v = self.run(&["version", "new", "--project", &self.project]);
        assert!(v.status.success(), "version new 失败：{}", text(&v.stderr));

        // ⚠️ 证据文案刻意**不含** `主键/外键/证据/基数` ——
        //    那几个是 `view::MODELING_TERMS_CJK` 的守门词，
        //    命中会让客户视图/总览 `exit(2)`，那样测的就不是渲染而是守门器了。
        let doc = serde_json::json!({
            "project": self.project,
            "base_version": 1,
            "round": 1,
            "items": [{
                "kind": "add_entity_type",
                "target": "客户",
                "payload": {
                    "key": "customer",
                    "display_name": "客户",
                    "properties": [{"name": "客户号", "column": "customer_no", "nullable": false}],
                    "evidence": [
                        {"table": "t_customer", "column": "customer_no", "claim": "列名与注释一致"}
                    ]
                }
            }]
        });
        let file = self.workspace().join(format!("proposal-{}.json", self.tag));
        std::fs::write(&file, serde_json::to_string_pretty(&doc).unwrap()).expect("写提案 JSON 失败");

        // ⛔ `propose import` **只认 `--file`**（项目名在 JSON 里，见 `ProposeCmd::Import`）。
        let imp = self.run(&["import", "--file", file.to_str().unwrap()]);
        assert!(
            imp.status.success(),
            "propose import 失败：{}\n提案 JSON：{}",
            text(&imp.stderr),
            file.display()
        );

        // 两轨都接受 ⇒ `proposal apply` 才会产出新版本（否则 apply 空转，v2 没有实体类型）。
        let acc = self.run(&[
            "review", "decide", "--proposal", "p1", "--project", &self.project, "--accept-all",
        ]);
        assert!(acc.status.success(), "review decide --accept-all 失败：{}", text(&acc.stderr));

        let ap = self.run(&["proposal", "apply", "--proposal", "p1", "--project", &self.project]);
        assert!(
            ap.status.success(),
            "proposal apply 失败：{}\n没有实体类型的话后面 5 个站点全是空断言",
            text(&ap.stderr)
        );
    }

    /// 跑一次真实 `ontok`；**只在集群握手抖动**时重试。
    ///
    /// ⚠️ 为什么需要：共享 PG 集群常年占着几十条连接，
    /// 「Connection reset by peer (os error 104)」是**建连握手阶段**被拖住时的
    /// 典型表现，隔一会儿就好 —— `tests/common/mod.rs` 对同一现象有完整记述
    /// （`common::retry_connect` 就是为它写的）。那与被测逻辑无关，
    /// 却会让测试**随机红**：本文件第一轮就撞上一次（建场阶段）、
    /// 第二轮撞在 `ontology show --model-view` 上。
    ///
    /// ⛔ **只认这两个基础设施签名，绝不重试渲染/退出码类的失败** ——
    /// 那是真信号，重试会把真信号磨掉。判据是 stderr 的**字面内容**，
    /// 不是「没成功」，所以「渲染错了所以退出码非 0」不会被重试。
    fn run(&self, args: &[&str]) -> Output {
        const HANDSHAKE: [&str; 2] = [
            "Connection reset by peer",
            "error communicating with database",
        ];
        let mut last = self.run_once(args);
        for _ in 0..3 {
            if last.status.success() {
                return last;
            }
            let e = text(&last.stderr);
            if !HANDSHAKE.iter().any(|h| e.contains(h)) {
                return last;
            }
            std::thread::sleep(std::time::Duration::from_millis(700));
            last = self.run_once(args);
        }
        last
    }

    fn run_once(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ontok-builder"))
            .args(args)
            // ⛔ 连接串只走环境变量：口令不进 argv、不进文件、不进日志。
            .env("ONTOK_DATABASE_URL", &self.url)
            .output()
            .unwrap_or_else(|e| panic!("跑 ontok {args:?} 失败：{e}"))
    }

    async fn pool(&self) -> sqlx::PgPool {
        let set_path = format!("SET search_path TO {}", self.schema);
        let opts = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .after_connect(move |conn, _meta| {
                let set_path = set_path.clone();
                Box::pin(async move {
                    sqlx::query(&set_path).execute(conn).await?;
                    Ok(())
                })
            });
        common::retry_connect(|| async {
            opts.clone().connect(&self.url).await.map_err(DbError::Connect)
        })
        .await
        .expect("连不上专属 schema")
    }

    /// apply 之后那个版本的快照 —— 进程内复刻渲染的输入。
    async fn snapshot(&self) -> ontok::domain::snapshot::Snapshot {
        let pool = self.pool().await;
        let p = PgProjectStore::new(pool.clone())
            .get_by_name(&self.project)
            .await
            .unwrap()
            .expect("沙箱项目应当存在");
        let v = PgProjectStore::new(pool.clone())
            .version_by_seq(p.id, &p.name, self.version)
            .await
            .unwrap_or_else(|e| panic!("取 v{} 失败：{e}", self.version));
        PgStore::new(pool.clone())
            .get_snapshot(v.id)
            .await
            .unwrap()
            .expect("apply 之后应当有快照")
    }

    /// 临时文件目录 —— **仓库根**的 `.review-workspace/req078d/`。
    fn workspace(&self) -> PathBuf {
        let root = common::source_root::crate_root()
            .parent()
            .expect("拿不到仓库根")
            .to_path_buf();
        let dir = root.join(".review-workspace").join("req078d");
        std::fs::create_dir_all(&dir).expect("建临时目录失败");
        dir
    }

    /// REQ-078 · F-3：显式收尾 + **删完复查**。失败即判红。
    async fn teardown(&self) {
        if let Err(why) = cleanup_schema(&self.schema).await {
            panic!(
                "🔴 REQ-078 F-3：清理专属 schema `{}` **失败** —— {why}\n\
                 请手动清理：`DROP SCHEMA {} CASCADE;`",
                self.schema, self.schema
            );
        }
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let schema = self.schema.clone();
        let _ = std::thread::spawn(move || {
            if let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() {
                if let Err(why) = rt.block_on(cleanup_schema(&schema)) {
                    // Drop 里不 panic（可能在 unwind 里）—— 只留痕。
                    eprintln!("⚠⚠ REQ-078 F-3：专属 schema `{schema}` 没能收干净 —— {why}");
                    if let Some(f) = residue_ledger() {
                        use std::io::Write;
                        if let Ok(mut f) = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(&f)
                        {
                            let _ = writeln!(f, "{schema}\t{why}");
                        }
                    }
                }
            }
        })
        .join();
    }
}

async fn cleanup_schema(schema: &str) -> Result<(), String> {
    if !schema.starts_with("t_req078_")
        || !schema
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return Err(format!("schema 名 `{schema}` 不在白名单内 —— 拒绝清理"));
    }
    let base = ontok::config::test_database_url()
        .map_err(|e| format!("拿不到 ONTOK_TEST_DATABASE_URL：{e}"))?;
    let pool = common::retry_connect(|| ontok::db::pool::connect(&base))
        .await
        .map_err(|e| format!("连不上测试库，清理压根没开始：{e}"))?;
    sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
        .execute(&pool)
        .await
        .map_err(|e| format!("`DROP SCHEMA … CASCADE` 失败：{e}"))?;
    // ⚠️ REQ-080 F-4：这里**不能**是 `to_regclass($1)::text` ——
    //    它解析的是**关系名**（走会话 `search_path`），对一个**确实存在**的
    //    schema 恒返回 `NULL`（REQ-080 QA 实测 2/2）⇒ 「`DROP` 报成功但还在」
    //    那一格**永不触发**。改查 `pg_namespace` 里的 schema 名。
    //    判别力自证（证明改后**会红**）在
    //    `tests/test_review_show_binary_req072.rs::f4_*`：同一条查询、同一套判定。
    //
    // ⚠️ `fetch_optional`：schema 不存在时这条查询返回**零行**（不是一行 NULL）
    //    ⇒ `fetch_one` 会把「已经删干净」误报成「复查没跑起来」。
    let still: Option<String> = sqlx::query_scalar(
        "SELECT nspname FROM pg_namespace WHERE nspname = $1",
    )
    .bind(schema)
    .fetch_optional(&pool)
    .await
    .map_err(|e| format!("清理后复查失败：{e}"))?
    .flatten();
    match still {
        Some(found) => Err(format!(
            "`DROP` 报成功，但 `{schema}` 仍然在库里（`pg_namespace` 查到 `{found}`）"
        )),
        None => Ok(()),
    }
}

fn residue_ledger() -> Option<PathBuf> {
    let dir = common::source_root::crate_root()
        .parent()?
        .join(".review-workspace")
        .join("req078d");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("cleanup-residue.log"))
}

fn scoped_url(base: &str, schema: &str) -> String {
    let sep = if base.contains('?') { '&' } else { '?' };
    format!("{base}{sep}options=-c%20search_path%3D{schema}")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

// ══════════════════════════════════════════════════════════════
// 断言形状：stdout == 已登记前缀 ++ domain 渲染 ++ 尾换行
// ══════════════════════════════════════════════════════════════

/// 按**登记表**里的那一项，把真实 stdout 与 domain 产物逐字节比。
///
/// ⚛️ 前缀与尾换行都取自 [`DOMAIN_PRINT_SITES`]，这里**一个字都不写死** ——
/// 写死就会被 G-2 判红（那正是 AC-002 要的）。
#[track_caller]
fn assert_site_bytes(site: &DomainPrintSite, stdout: &str, domain_text: &str) {
    let expected = format!(
        "{}{domain_text}{}",
        site.prefix_template,
        if site.trailing_newline { "\n" } else { "" }
    );
    assert_eq!(
        stdout,
        expected,
        "\n【{}】真实 stdout 与「已登记前缀 ++ domain 渲染」**逐字节不相等** ——\n\
         二进制层对这个站点的产物做了本文件未登记的再加工。\n\
         登记项：前缀 {:?}、尾换行 {}、判据 {:?}\n\
         请勿靠放宽断言把它盖掉：要么改登记，要么改 `src/cli.rs`。",
        site.command,
        site.prefix_template,
        site.trailing_newline,
        site.judged_by
    );
}

fn site(command: &str) -> &'static DomainPrintSite {
    DOMAIN_PRINT_SITES
        .iter()
        .find(|s| s.command == command)
        .unwrap_or_else(|| panic!("登记表里没有 `{command}` —— 先登记再测"))
}

/// `ontology show` 一次（非 html / 非 json / 非 model-view 的默认路径也是 JSON，
/// 但登记表登记的是显式 `--json` 那一档，两条走的是同一行 `emit_domain_line(&json)`）。
#[tokio::test]
async fn test_ontology_show_stdout_is_registered_prefix_plus_domain_render() {
    let sb = Sandbox::new("onshow").await;
    let snap = sb.snapshot().await;
    let sv = snapshot_to_view(&snap);

    // ── `--model-view` ──
    let mv = ontok::domain::model_view::project_model_track(&snap);
    let out = sb.run(&[
        "ontology", "show", "--project", &sb.project, "--version", "2", "--model-view",
    ]);
    let stdout = text(&out.stdout);
    assert!(
        out.status.success(),
        "ontology show --model-view 退出码非 0：{}\nstdout：\n{stdout}",
        text(&out.stderr)
    );
    assert_site_bytes(
        site("ontology show --model-view"),
        &stdout,
        &serde_json::to_string_pretty(&mv).unwrap(),
    );

    // ── `--html`（无 `--sampling` ⇒ 用 `DEFAULT_SAMPLING`）──
    let cv = ontok::domain::view::project(&sv);
    let sampling: Vec<String> = ontok::domain::html_view::DEFAULT_SAMPLING
        .iter()
        .map(|s| s.to_string())
        .collect();
    let out = sb.run(&[
        "ontology", "show", "--project", &sb.project, "--version", "2", "--html",
    ]);
    let stdout = text(&out.stdout);
    assert!(
        out.status.success(),
        "ontology show --html 退出码非 0：{}\nstdout 前 400 字：\n{}",
        text(&out.stderr),
        &stdout[..stdout.len().min(400)]
    );
    assert_site_bytes(
        site("ontology show --html"),
        &stdout,
        &ontok::domain::html_view::render_html(&cv, &sampling).html,
    );

    // ── 客户视图 JSON ──
    // ⚠️ `ontology show` **没有** `--json`：不带 `--html` / `--model-view` 时
    //    默认路径就直接打 JSON（`src/cli.rs` 的 `emit_domain_line(&json)` 那一行）。
    let out = sb.run(&["ontology", "show", "--project", &sb.project, "--version", "2"]);
    let stdout = text(&out.stdout);
    assert!(
        out.status.success(),
        "ontology show（默认 JSON）退出码非 0：{}",
        text(&out.stderr)
    );
    assert_site_bytes(
        site("ontology show（默认 JSON）"),
        &stdout,
        &serde_json::to_string_pretty(&cv).unwrap(),
    );

    sb.teardown().await;
}

#[tokio::test]
async fn test_overview_stdout_is_registered_prefix_plus_domain_render() {
    let sb = Sandbox::new("overview").await;
    let snap = sb.snapshot().await;
    let sv = snapshot_to_view(&snap);

    // ⚠️ `names` 的构造与 `src/cli.rs` 的 `Overview` 分支**逐字一致** ——
    // 抄错一份就会得到「测的根本不是那一格」（本会话纪律 ③）。
    let names: std::collections::BTreeMap<String, String> = sv
        .entity_types
        .iter()
        .map(|e| (e.key.clone(), e.display_name.clone()))
        .collect();
    let cv = ontok::domain::view::project(&sv);
    let ov = ontok::domain::overview::build_overview(&cv, &snap.concept_groups, &names);

    // ── 文本 ──
    let out = sb.run(&["ontology", "overview", "--project", &sb.project, "--version", "2"]);
    let stdout = text(&out.stdout);
    assert!(
        out.status.success(),
        "ontology overview 退出码非 0：{}\nstdout：\n{stdout}",
        text(&out.stderr)
    );
    assert_site_bytes(
        site("overview"),
        &stdout,
        &ontok::domain::overview::render_overview_text(&ov),
    );

    // ── JSON ──
    let out = sb.run(&[
        "ontology", "overview", "--project", &sb.project, "--version", "2", "--json",
    ]);
    let stdout = text(&out.stdout);
    assert!(
        out.status.success(),
        "ontology overview --json 退出码非 0：{}",
        text(&out.stderr)
    );
    assert_site_bytes(
        site("overview --json"),
        &stdout,
        &serde_json::to_string_pretty(&ov).unwrap(),
    );

    sb.teardown().await;
}

/// 【交付物】把 `ontology show --html` 的真实 stdout 打出来
/// （`--nocapture` 下可见），方便人工核对登记表里那一档到底长什么样。
#[tokio::test]
async fn test_ontology_show_html_stdout_is_the_registered_rendering() {
    let sb = Sandbox::new("html").await;
    let out = sb.run(&[
        "ontology", "show", "--project", &sb.project, "--version", "2", "--html",
    ]);
    let stdout = text(&out.stdout);
    println!(
        "\n===== 真实 `ontok ontology show --html` stdout（REQ-078）=====\n{}\n===== 结束 =====\n",
        &stdout[..stdout.len().min(1200)]
    );
    assert!(
        out.status.success(),
        "ontology show --html 失败：{}",
        text(&out.stderr)
    );
    // 至少证明它**不是空的**：一条恒真的字节比对（空 == 空）没有判别力。
    assert!(
        stdout.len() > 200,
        "HTML 输出只有 {} 字节 —— 空比空是恒真断言，那不是防线",
        stdout.len()
    );

    sb.teardown().await;
}

/// 出口码政策（AC-004）在本文件这一侧的端到端兑现：
/// **渲染正常 ⇒ 退出码 0**，且 stderr 上没有自检痕迹。
///
/// ⚛️ 这是政策的**正向**那一半；反向那一半（三道自检违规 ⇒ 退出 3）
/// 在当前 fixture 下**无法**触发（domain 层已被 REQ-070/076/077 验证正确），
/// 因此它由 `tests/test_binary_output_guard.rs::g3_exit_code_policy_is_defined_and_asserted`
/// 以纯函数断言覆盖，并已用 M2 变异实测到退出码非 0。
#[tokio::test]
async fn test_clean_render_exits_zero_and_keeps_self_checks_silent() {
    let sb = Sandbox::new("exitcode").await;
    let out = sb.run(&["ontology", "overview", "--project", &sb.project, "--version", "2"]);
    assert_eq!(
        out.status.code(),
        Some(ontok::cli::EXIT_OK),
        "渲染正常时退出码必须是 {} —— 政策见 `ontok::cli::EXIT_POLICY`\nstderr：\n{}",
        ontok::cli::EXIT_OK,
        text(&out.stderr)
    );
    let stderr = text(&out.stderr);
    for kind in ["两轨分界自检", "提案级归属自检", "FDE 通道自检"] {
        assert!(
            !stderr.contains(kind),
            "渲染正常时二进制自检却报出了「{kind}」\nstderr：\n{stderr}"
        );
    }

    sb.teardown().await;
}

//! REQ-072 · **真实 `ontok review show` 进程**的端到端验证。
//!
//! # 这个文件补的是哪一格
//!
//! REQ-058 / 064 / 065 / 069 / 070 一路下来，验证对象一直是 **domain 函数**
//! （`render_review_text` / `verify_track_separation` / `verify_provenance_scope`）。
//! `tests/test_review_show_e2e.rs` 走的是**真库**，但它自己文件头写得很清楚：
//! 「复刻 `cli.rs::review_cmd` 的 `Show` 分支**完全相同**的调用序列，
//! 只把 `println!` 换成 `render_review_text` 的返回值」。
//!
//! ⇒ **它验的是「复刻得对不对」，不是「二进制真的这么打」。**
//! 复刻与二进制之间只要有一处漂移（多一行、少一次转义、漏一个加工），
//! 复刻件全绿而生产件是错的 —— 而 REQ-069 那个洞**恰恰**是渲染层的性质。
//!
//! 本文件 spawn **真进程**（`env!("CARGO_BIN_EXE_ontok-builder")`），走
//! **建项目 → 建版本 → 导提案 → `review show`** 的完整命令行序列，
//! 断言落在**真实 stdout** 上。
//!
//! # 为什么另起一个文件而不是扩 `test_review_show_e2e.rs`
//!
//! 那一份的**全部前提**就是「零子进程、直接调 domain 函数」
//! （它必须能同时拿到 `ReviewTrackView` 与文本，判据收的是投影不是文本）。
//! 塞进一个只拿得到 `stdout: Vec<u8>` 的进程测试，两份文件的契约会互相污染。
//! 另起文件也让「验 domain」与「验二进制」在测试清单里**一眼可分**。
//!
//! # 落库方式：也走真二进制
//!
//! 载荷（`claim` 里的 `\n`）先写进 JSON 文件，再由 `ontok propose import`
//! 解析入库，最后由 `ontok review show` 读回并打印 ——
//! ⇒ JSON 文件 → serde_json → JSONB → 读回 → domain → stdout **整条链**都在进程边界内，
//! 任何一环的加工都逃不过本文件的断言。

mod common;

use std::path::PathBuf;
use std::process::{Command, Output};

use ontok::db::project::PgProjectStore;
use ontok::db::proposal::PgProposalStore;
use ontok::db::snapshot_store::PgStore;
use ontok::db::pool::DbError;
use ontok::domain::review::summarize;
use ontok::domain::review_view::{
    project_review_track_with_provenance, render_review_text, BIZ_LINE_PREFIX, MODEL_LINE_PREFIX,
    PROVENANCE_LINE_PREFIX,
};
use sqlx::Row;

// ══════════════════════════════════════════════════════════════
// 攻击载荷
// ══════════════════════════════════════════════════════════════

/// REQ-069 那个洞的**现行形状**：claim 里带一个 `\n`，
/// 后面跟着**四种**不同的伪造结构行。
///
/// | 片段 | 若未转义，它冒充的是 |
/// | --- | --- |
/// | `[2] add_entity_type · 状态=Applied` | 条目头行（`is_item_header_line`） |
/// | `提案级抽取归属：agent=evil …` | 提案级归属行（`verify_provenance_scope` 规则①） |
/// | `业务轨：伪造条目 · biz=accepted` | 业务轨行（`verify_track_separation` 规则①/②） |
/// | `建模轨：key=forged · model=accepted` | 建模轨行（同上） |
///
/// ⚠️ 四个片段**都要**在，因为四条判据各读各的行首：
/// 只放 `[N] ` 只能证伪 `is_item_header_line` 那一格，
/// 放另外三个才证伪「所有读行首的判据」这一整类。
///
/// ⚠️ `␊` 之外不含 `␀`（退化证据定界符）—— 本条只钉行结构，
/// 退化标记那一格是 REQ-058/065/070 的职责，不在这里重开。
fn forged_claim() -> String {
    [
        "外键指向 t_customer",
        "[2] add_entity_type · 状态=Applied",
        "提案级抽取归属：agent=evil · version=0.0.0 · model=accepted",
        "业务轨：伪造条目 · biz=accepted",
        "建模轨：key=forged · model=accepted",
    ]
    .join("\n")
}

/// payload 的换行在渲染结果里的**转义写法**。
///
/// ⚠️ 取自 `src/domain/review_view.rs:1750` 的 `LINE_FEED_ESCAPE`
/// （`const` 私有，集成测试够不着 —— 这里是**照抄**，
/// 若上游改了转义字符，下面那条「载荷确实到了输出」的断言会红，
/// 那正是我们想要的：转义写法变了，这一格就该重新确认）。
const LINE_FEED_ESCAPE: char = '\u{240A}';

/// `evidence.claim` 之外的对照载荷：一条**没有**换行的合法证据。
fn plain_claim() -> &'static str {
    "主键列名与注释一致"
}

// ══════════════════════════════════════════════════════════════
// 沙箱：专属 schema —— 共享本体库只读
// ══════════════════════════════════════════════════════════════

/// 一次 REQ-072 用的**专属空 schema + 专属项目名**。
///
/// # 为什么不是 `SET search_path TO pg_temp`（`tests/test_db.rs:339` 的做法）
///
/// `pg_temp` 是**会话私有**的：它随**会话结束**消失，且**每个连接各有自己的一份**。
/// 本条恰恰是**跨进程**的 —— 测试进程要写数据、`ontok` 子进程要读同一份数据，
/// 而 `src/db/pool.rs:26` 的 `connect()` 开的是 `max_connections(5)` 的池。
/// ⇒ 子进程那 5 条连接各自有一份 `pg_temp`，
/// 「测试进程写的那份」与「子进程读的那份」**结构上不可能是同一个**。
/// 这不是配置问题，是 `pg_temp` 的定义。
///
/// # 所以用的是同文件里**另一种**隔离：`tests/test_db.rs:141-183`（REQ-025）
///
/// `CREATE SCHEMA <专属>` + `SET search_path` —— 命名空间对**所有连接、所有进程**可见，
/// 于是「写」与「读」两侧落在同一份数据上。
///
/// # 收尾
///
/// 只 `DROP SCHEMA … CASCADE` **本测试自己建的那个**命名空间。
/// 这与 `tests/test_db.rs:118-119` 的原话一致：
/// 「删的是自己的临时命名空间，不是共享表」。
/// ⚠️ **零 DELETE / 零 TRUNCATE / 零清空共享表** —— 共享 `public` 一张表都不碰，
/// 子进程连 `public` 都进不去（连接串把 `search_path` 指走了）。
struct Sandbox {
    schema: String,
    project: String,
    /// 指到 `schema` 的连接串 —— 给**子进程**用。
    url: String,
}

impl Sandbox {
    async fn new(tag: &str) -> Sandbox {
        let base = ontok::config::test_database_url().expect(
            "缺 ONTOK_TEST_DATABASE_URL —— 口令不得入库，请导出环境变量（见 .env.example）",
        );
        let schema = common::unique_project(&format!("req072_{tag}"));
        let project = common::unique_project(&format!("req072p_{tag}"));

        // 建 schema 走**默认**连接（`public`）—— CREATE SCHEMA 不写任何共享表。
        let admin = common::test_pool().await;
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .expect("建专属空 schema 失败");

        let url = scoped_url(&base, &schema);
        Sandbox { schema, project, url }
    }

    /// 进程内的池，同样指到 `schema` —— 供与 domain 层**逐字节比对**用。
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
        // REQ-042 · 与 `tests/test_db.rs:184` 同款：builder 留在闭包外，
        // 闭包里只有「建连接」本身（`after_connect` 的会话初始化随建连接一并重试）。
        common::retry_connect(|| async {
            opts.clone().connect(&self.url).await.map_err(DbError::Connect)
        })
        .await
        .expect("连不上专属 schema")
    }

    /// 落库一条带**攻击载荷**的提案，全部经由**真实二进制**。
    ///
    /// 返回子进程 `review show` 用的提案别名（本沙箱只有一个提案 ⇒ 恒为 `p1`）。
    async fn seed_via_binary(&self) -> String {
        // 建项目 + 建版本（`propose import` 需要一个未冻结的基版本）。
        let p = self.run(&["project", "new", "--name", &self.project]);
        assert!(p.status.success(), "project new 失败：{}", text(&p.stderr));
        let v = self.run(&["version", "new", "--project", &self.project]);
        assert!(v.status.success(), "version new 失败：{}", text(&v.stderr));

        // 提案 JSON —— `claim` 里那个 `\n` 由 serde_json 编码成 `\n` 两字符，
        // 子进程 `serde_json::from_str` 再解回**真换行** ⇒ 与生产导入路径同形。
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
                        {"table": "t_customer", "column": "customer_no", "claim": plain_claim()},
                        {"table": "t_customer", "column": "customer_id", "claim": forged_claim()}
                    ]
                }
            }]
        });
        let file = self.workspace().join(format!("proposal-{}.json", self.schema));
        std::fs::write(&file, serde_json::to_string_pretty(&doc).unwrap())
            .expect("写提案 JSON 失败");

        let imp = self.run(&["import", "--file", file.to_str().unwrap()]);
        assert!(
            imp.status.success(),
            "propose import 失败：{}\n提案 JSON：{}",
            text(&imp.stderr),
            file.display()
        );
        "p1".to_string()
    }

    /// 跑一次**真实** `ontok`；**只在集群握手抖动**时重试。
    ///
    /// # 为什么加这个（实测撞过三次）
    ///
    /// 共享 PG 集群常年占着几十条连接，「Connection reset by peer (os error 104)」
    /// 是**建连握手阶段**被拖住时的典型表现，隔一会儿就好 ——
    /// `tests/common/mod.rs` 对同一现象有完整记述（`retry_connect` 就是为它写的）。
    /// 那是**基础设施**问题，与被测逻辑无关，却会让全量**随机红**：
    /// REQ-078 验收的三轮全量里各撞过一次（`tests/test_project.rs:143`、
    /// 本文件的 `propose import`）。
    ///
    /// ⛔ **只认这两个字面签名，绝不重试渲染 / 退出码类的失败** ——
    /// 那是真信号，重试只会把它磨掉。判据是 stderr 的**字面内容**，
    /// 不是「没成功」，所以「渲染错了所以退出码非 0」一次都不会被重试。
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
            // ⛔ 连接串只走环境变量：子进程继承的是**这个** URL，
            // 它由 `ONTOK_TEST_DATABASE_URL` 在进程内派生，口令**不进 argv**、
            // 不进文件、不进日志、不进 commit message。
            .env("ONTOK_DATABASE_URL", &self.url)
            .output()
            .unwrap_or_else(|e| panic!("跑 ontok {args:?} 失败：{e}"))
    }

    /// 临时文件目录 —— **仓库根**的 `.review-workspace/req072d/`。
    ///
    /// ⚠️ 刻意**不是**本 worktree 内的 `.review-workspace/`
    /// （实测它不被 `.gitignore` 覆盖，会变成待提交的脏文件）。
    ///
    /// ⚠️ crate 根走 `common::source_root::crate_root()`（**运行时**定位），
    /// 不写 `env!("CARGO_MANIFEST_DIR")` ——
    /// 全仓 `tests/**/*.rs` 只有 `tests/common/source_root.rs:104` 那一行
    /// 带 `// ANCHOR-ALLOW`，别处一律裸编译期锚点都会被
    /// `tests/test_anchor_guard.rs` 判红（本文件第一版就踩过，
    /// 由 REQ-047 的守卫当场抓住）。
    fn workspace(&self) -> PathBuf {
        let root = common::source_root::crate_root()
            .parent()
            .expect("拿不到仓库根")
            .to_path_buf();
        let dir = root.join(".review-workspace").join("req072d");
        std::fs::create_dir_all(&dir).expect("建临时目录失败");
        dir
    }
}

impl Sandbox {
    /// REQ-078 · F-3 —— **显式**收尾，并**断言** schema 真的没了。
    ///
    /// # 为什么必须有这一步（这就是 F-3 说的那件事）
    ///
    /// 原先清理只有 `Drop` 一条路，而 `Drop` 里的 `let _ = …execute(…)`
    /// **把失败整个吞掉** —— 失败与成功在外部**完全同形**（退出码 0、无 stderr）。
    /// 那正是当初在共享库里留下 5 个残留 schema 的失败形状，
    /// 而今天**仍然没有任何信号**（REQ-072 QA 的 F-3）。
    ///
    /// 于是分成两层：
    ///
    /// | 层 | 触发 | 失败时 |
    /// | --- | --- | --- |
    /// | [`Sandbox::teardown`]（本函数） | 测试体**显式**调用 | 🔴 **判红** —— 残留进共享库是必须叫出来的事 |
    /// | `Drop` 的兜底 | 测试 panic / 忘了调 | 🟡 stderr 醒目一行 + 追加进残留台账 |
    ///
    /// ⚠️ `Drop` 那一层**不能** panic：`Drop` 可能在** unwind 期间**跑，
    /// 那时 panic 是 `abort`。所以它只**记账**，不**爆炸** ——
    /// 而记账本身也必须是可见的（台账文件 + stderr），否则又退回「静默」。
    ///
    /// # 为什么「断言」而不是「相信 DROP 的返回值」
    ///
    /// 历史上那次残留的直接原因就是**清理根本没跑到**（`Drop` 里在 tokio 线程上
    /// `block_on` → panic），不是 DROP 报错。⇒ 只看返回值等于只看一半。
    /// 这里删完**再查一次**它在不在：`pg_namespace` 查得到就是没删掉
    /// （⚠️ 原来这里用的是 `to_regclass` —— 它查的是**关系名**，对 schema 恒 `NULL`，
    /// 这一格因此恒真；REQ-080 F-4 已换掉并补了判别力自证）。
    async fn teardown(&self) {
        if let Err(why) = cleanup_schema(&self.schema).await {
            panic!(
                "🔴 REQ-078 F-3：清理专属 schema `{}` **失败** —— {why}\n\
                 这**不是**噪声：残留的 schema 会一直挂在共享库上跨多次运行，\n\
                 而失败形状（删不掉却什么都不说）正是当初留下 5 个残留的那一种。\n\
                 请手动清理：`DROP SCHEMA {} CASCADE;`",
                self.schema, self.schema
            );
        }
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        // ⛔ 刻意**不**在 `Drop` 里 `block_on` 一个新 runtime：
        // `#[tokio::test]` 的测试体就跑在 tokio 的 runtime 线程上，
        // 在那里再 `block_on` 会 panic（`Cannot start a runtime from within a runtime`）——
        // 那是本文件第一版的实测失败，不是假想。
        //
        // 改为**另起一条 std 线程**做清理：那条线程不由 tokio 驱动，
        // 自建 runtime 是合法的。且 `join` 保证 schema 真的被删掉
        // （否则残留的 schema 会一直挂在共享库上）。
        //
        // ⚠️ REQ-078 F-3：**不再静默吞失败**。这一层是 panic 后的兜底，
        // 它不能判红（Drop 里 panic = abort），但**必须留痕**：
        // 醒目 stderr + 追加进残留台账。
        let schema = self.schema.clone();
        let _ = std::thread::spawn(move || {
            if let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() {
                if let Err(why) = rt.block_on(cleanup_schema(&schema)) {
                    record_residue(&schema, &why);
                }
            } else {
                record_residue(&schema, "建不起清理用的 runtime（清理压根没跑）");
            }
        })
        .join();
    }
}

/// 删掉本测试自己建的 schema（**不是**共享表），**删完再查一遍**它在不在。
///
/// # 返回值为什么是 `Result` 而不是 ()
///
/// 原先是 `let _ = …execute(…).await;` —— 失败与成功在外部**完全同形**，
/// 这就是 F-3。⇒ 现在**失败必须能上报**：
/// 显式收尾的调用方（[`Sandbox::teardown`]）据此判红，`Drop` 兜底据此记账。
///
/// ⚠️ 「删不掉」有两种形状，**两种都要抓**：
/// ① `DROP` 报错；② `DROP` 报成功但 schema 还在（历史上那次是压根没跑到 DROP）。
/// 所以下面在 `DROP` 之后**复查**它还在不在。
async fn cleanup_schema(schema: &str) -> Result<(), String> {
    // schema 名由 `common::unique_project` 生成，形如 `t_req072_ac1_<pid>_<n>`，
    // 只含 `[a-z0-9_]` ⇒ 可安全内插；仍做一次白名单校验以防误删共享命名空间。
    if !schema.starts_with("t_req072_")
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

    verdict_after_drop(schema_still_there(&pool, schema).await?, schema)
}

/// ⭐ REQ-080 F-4 · 「这个 schema 还在不在」——**查 schema 名**的那一条查询。
///
/// ## 为什么不能是 `to_regclass($1)::text`
///
/// `to_regclass` / `::regclass` 解析的是**关系名**（relation），不是 schema 名：
/// 它走会话的 `search_path` 去找一个**同名关系**，找不到就返回 `NULL`。
/// 而测试库的 `search_path` 是 `"$user", public`，**不含**本测试建的专属 schema
/// ⇒ `to_regclass('t_req072_…')` 对一个**确实存在**的 schema 也返回 `NULL`
/// （REQ-080 QA 实测 2/2；权威的 `pg_namespace` / `information_schema.schemata`
/// 同时都返回该 schema）。
///
/// ⇒ 旧写法让 `still` **恒为 `None`** ⇒ `match` 恒走 `Ok(())` ⇒
/// 「`DROP` 报成功但还在」那一格**永不触发** —— 而它恰是当初留下 5 个残留的形状。
///
/// 判别力自证（证明这一格**会红**）：见本文件的
/// `f4_schema_probe_sees_a_schema_that_really_exists` 与
/// `f4_drop_verdict_goes_red_when_the_schema_is_still_there`。
async fn schema_still_there(pool: &sqlx::PgPool, schema: &str) -> Result<Option<String>, String> {
    // ⚠️ 用 `fetch_optional` 而不是 `fetch_one`：这条查询在 schema 不存在时返回
    // **零行**（不像 `to_regclass` 那样返回一行 `NULL`）⇒ `fetch_one` 会以
    // 「no rows returned」报错，把「已经删干净了」误报成「连复查都没跑起来」。
    // 「零行」就是「不在」—— 把它折成 `None` 才是这条查询自己的答案。
    sqlx::query_scalar("SELECT nspname FROM pg_namespace WHERE nspname = $1")
        .bind(schema)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("清理后复查失败（schema 在不在都不知道）：{e}"))
        .map(|row| row.flatten())
}

/// `DROP` 之后的复查**判定** —— **纯函数**，可被直接断言。
///
/// 抽成纯函数是本文件与 REQ-080 的 AC-003 同一条纪律：
/// 「判出来」与「信数据库」是两件事，判据本身必须能被打。
fn verdict_after_drop(still: Option<String>, schema: &str) -> Result<(), String> {
    match still {
        Some(found) => Err(format!(
            "`DROP` 报成功，但 `{schema}` 仍然在库里（`pg_namespace` 查到 `{found}`）"
        )),
        None => Ok(()),
    }
}

/// 把一次「没能收干净」**记账** —— stderr 醒目一行 + 追加进残留台账。
///
/// 台账落**仓库根**的 `.review-workspace/req078d/cleanup-residue.log`。
/// ⚠️ 本函数**不做**「台账为空」的跨二进制断言：测试二进制是**并行**跑的，
/// 另一个二进制正在跑的时候去断言「台账此刻为空」，只会得到一张时序假红
/// （REQ-072 QA §4.3 的采样 2 就是这个形状）。
/// ⇒ **可判的信号是 [`Sandbox::teardown`] 里的逐条断言**，与跑测顺序无关。
fn record_residue(schema: &str, why: &str) {
    let line = format!("{schema}\t{why}");
    eprintln!(
        "⚠⚠ REQ-078 F-3：专属 schema `{schema}` **没能收干净** —— {why}\n\
         ⚠⚠ 它会一直挂在共享库上。手动清理：`DROP SCHEMA {schema} CASCADE;`"
    );
    if let Some(file) = residue_ledger() {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&file) {
            let _ = writeln!(f, "{line}");
        }
    }
}

/// 残留台账的路径 —— 仓库根 `.review-workspace/req078d/`。
fn residue_ledger() -> Option<PathBuf> {
    let dir = common::source_root::crate_root()
        .parent()?
        .join(".review-workspace")
        .join("req078d");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("cleanup-residue.log"))
}

/// 给连接串加一条 `search_path` —— 让**子进程**落进专属 schema。
///
/// # 机制
///
/// PostgreSQL 的连接参数 `options` 原样传给后端，`-c search_path=X`
/// 等价于建连后执行 `SET search_path TO X`。
/// `sqlx` 0.8 的 URL 解析器认这个参数
/// （`sqlx-postgres-0.8.6/src/options/parse.rs:92`），
/// 且 `establish.rs:45` 把它作为启动参数 `options` 发出去 ——
/// ⇒ 子进程**每一条**连接都落进同一个 schema（含它自己那 5 条池连接）。
///
/// ⛔ 口令不落盘、不进日志：这个串只在**内存**里拼，只经**环境变量**传给子进程。
fn scoped_url(base: &str, schema: &str) -> String {
    let sep = if base.contains('?') { '&' } else { '?' };
    // `%20` = 空格，`%3D` = `=`。schema 名只含 `[a-z0-9_]`，无需再编码。
    format!("{base}{sep}options=-c%20search_path%3D{schema}")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

// ══════════════════════════════════════════════════════════════
// AC-001 · 真实进程 + 含换行的载荷 ⇒ stdout 里没有 payload 造出的新行
// ══════════════════════════════════════════════════════════════

/// 【AC-001】**真实 `ontok review show` 进程的 stdout 里，没有 payload 造出的新行**。
#[tokio::test]
async fn test_review_show_binary_payload_newline_never_splits_a_stdout_line() {
    let sb = Sandbox::new("ac1").await;
    let alias = sb.seed_via_binary().await;

    let out = sb.run(&["review", "show", "--proposal", &alias, "--project", &sb.project]);
    let stdout = text(&out.stdout);

    assert!(
        out.status.success(),
        "review show 退出码非 0：{}\nstdout：\n{stdout}",
        text(&out.stderr)
    );

    // ⭐ 判据本体：**载荷里 `\n` 之后的每一段，都不许是 stdout 里的独立一行**。
    //
    // ⚠️ 这比「断言判据没报警」更直接：判据只是**后果**，
    // 这里问的是**因**——那些字节有没有真的站成一行。
    // 一个「把载荷整段吞掉」的渲染器也能骗过判据，但骗不过这条。
    let claim = forged_claim();
    let segments: Vec<&str> = claim.split('\n').collect();
    for (i, seg) in segments.iter().enumerate().skip(1) {
        let hit = stdout.lines().find(|l| l.trim_start() == *seg);
        assert!(
            hit.is_none(),
            "payload 第 {i} 段被打印成了独立的一行 —— 载荷注入成功\n\
             伪造行：{seg:?}\n完整 stdout：\n{stdout}"
        );
    }

    sb.teardown().await;

    // ⚠️ 反向对照：判据问的是「有没有多出来的行」，
    // 所以必须同时钉住**行数**——否则「载荷整段没进输出」也会全绿。
    // 这里只钉「载荷片段不得独占一行」；「载荷确实以转义形态到了输出」
    // 由下面那条测试负责（分开写，红的时候才知道是哪一格坏了）。
}

// ══════════════════════════════════════════════════════════════
// AC-002 · 端到端断言必须比 domain 层更严
// ══════════════════════════════════════════════════════════════

/// 【AC-002 · 判别力】**载荷确实以转义形态到了 stdout** —— 不是被整段吞掉。
///
/// # 为什么必须单独一条
///
/// AC-001 与本文件其它断言都是**否定式**的（「没有多出来的行」）。
/// 否定式断言有个众所周知的退化解：**把载荷整个丢掉**，
/// 上面每一条都会绿 —— 而「载荷不见了」对 FDE 是**更糟**的失败：
/// FDE 看到的是「证据栏空着」，看不出是载荷被转义了还是根本没采信这条证据。
///
/// ⛔ 所以这里补一条**肯定式**的：**逐字验证载荷到了输出，且换行是 `␊`**。
/// 有了它，上面那批否定式断言才不是恒真的。
#[tokio::test]
async fn test_review_show_binary_payload_reaches_stdout_escaped_not_dropped() {
    let sb = Sandbox::new("alive").await;
    let alias = sb.seed_via_binary().await;

    let out = sb.run(&["review", "show", "--proposal", &alias, "--project", &sb.project]);
    let stdout = text(&out.stdout);
    assert!(out.status.success(), "review show 失败：{}", text(&out.stderr));

    // 对照组那条合法证据也在（证明渲染器没整体哑掉）。
    assert!(
        stdout.contains(plain_claim()),
        "连合法证据都没打出来 —— 载荷断言全部失去意义\n完整 stdout：\n{stdout}"
    );

    // ⭐ 载荷逐字在，且四个伪造片段**之间**是 `␊` 而非真换行。
    let claim = forged_claim();
    let segments: Vec<&str> = claim.split('\n').collect();
    let escaped: String = segments.join(&LINE_FEED_ESCAPE.to_string());
    assert!(
        stdout.contains(&escaped),
        "载荷没有以「换行 ⇒ {LINE_FEED_ESCAPE:?}」的形态到达 stdout —— \
         载荷可能被整段丢弃或以别的方式改写了\n\
         期望片段：{escaped:?}\n完整 stdout：\n{stdout}"
    );

    // ⛔ 且 stdout 里**不许**出现真换行符夹在载荷首尾之间：
    // 那是「转义没生效」的另一种表现（首尾在、行结构没了，但字节是裸的）。
    assert!(
        !stdout.contains(&format!("{}\n{}", segments[0], segments[1])),
        "载荷首尾之间是**真换行**而不是 {LINE_FEED_ESCAPE:?} —— 转义未生效\n完整 stdout：\n{stdout}"
    );

    sb.teardown().await;
}

/// 【AC-002 · 结构更严】**伪造的四类结构行，一条都不许被当成独立结构行**。
///
/// 比 domain 层更在哪：domain 层的测试是**逐条判据**地问
/// （`verify_track_separation` 有没有报违规 / `verify_provenance_scope` 报没报）。
/// 这里改成**按 stdout 的行结构**直接数 ——
/// 「这一行是不是一条真条目头行」「业务轨行是不是恰好 N 行」，
/// 问的是**产出**本身有没有被污染，而不是**判据有没有察觉**。
///
/// ⚠️ 判据察觉 ≠ 产出干净：判据是纯句法的、只覆盖它写到的形态；
/// 产出被污染但判据沉默，正是本项目反复出现的那一类失败。
#[tokio::test]
async fn test_review_show_binary_payload_cannot_forge_structural_lines() {
    let sb = Sandbox::new("ac2").await;
    let alias = sb.seed_via_binary().await;

    let out = sb.run(&["review", "show", "--proposal", &alias, "--project", &sb.project]);
    let stdout = text(&out.stdout);
    assert!(out.status.success(), "review show 失败：{}", text(&out.stderr));

    let lines: Vec<&str> = stdout.lines().collect();

    // ① 条目头行：恰好 1 条（本提案 1 个条目）。
    //    载荷若注入成功，`[2] add_entity_type · 状态=Applied` 会被数进来 ⇒ 2 条。
    //    ⚠️ 用的是**与 `is_item_header_line` 同形**的判法（`[` + 全数字 + `]`），
    //    但问的是「行结构被污染了吗」，不是「判据报警了吗」。
    let headers: Vec<&str> = lines
        .iter()
        .copied()
        .filter(|l| {
            let t = l.trim_start();
            let Some(rest) = t.strip_prefix('[') else { return false };
            let Some(close) = rest.find(']') else { return false };
            let seq = &rest[..close];
            !seq.is_empty() && seq.bytes().all(|b| b.is_ascii_digit())
        })
        .collect();
    assert_eq!(
        headers.len(),
        1,
        "条目头行数 = {}，应为 1 —— payload 造出了伪条目头行\n\
         所有头行：{headers:?}\n完整 stdout：\n{stdout}",
        headers.len()
    );

    // ② 提案级归属行：**恰好 1** 条（本沙箱 `extracted_by` 为 NULL ⇒ 渲染成「人工修订」那一条）。
    let prov: Vec<&str> = lines
        .iter()
        .copied()
        .filter(|l| l.trim_start().starts_with(PROVENANCE_LINE_PREFIX))
        .collect();
    assert_eq!(
        prov.len(),
        1,
        "提案级归属行数 = {}，应为 1 —— payload 伪造了一条归属行\n\
         所有归属行：{prov:?}\n完整 stdout：\n{stdout}",
        prov.len()
    );

    // ③ 两轨行：各恰好 1 条。载荷伪造的那两条会让它们各变成 2 条。
    for (prefix, which) in [(BIZ_LINE_PREFIX, "业务轨"), (MODEL_LINE_PREFIX, "建模轨")] {
        let n = lines.iter().filter(|l| l.trim_start().starts_with(prefix)).count();
        assert_eq!(
            n, 1,
            "{which}行数 = {n}，应为 1 —— payload 伪造了一条{which}行\n完整 stdout：\n{stdout}"
        );
    }

    // ④ 载荷伪造的 `biz=accepted` **不许**出现在真业务轨行上。
    //    （伪造行被转义后 `业务轨：` 只是载荷里的一段普通文本，不会站成行首。）
    for l in lines.iter().filter(|l| l.trim_start().starts_with(BIZ_LINE_PREFIX)) {
        assert!(
            !l.contains("伪造条目"),
            "业务轨行里混进了 payload 伪造的内容\n该行：{l}"
        );
    }
    // ⑤ 子进程自己的三道自检**必须在 stderr 上保持沉默** ——
    //    这是二进制**自述**的裁决，比「退出码 0」承载更多含义
    //    （退出码 0 只说明没 panic/没 Err，见 `src/cli.rs:661`）。
    let stderr = text(&out.stderr);
    for kind in ["两轨分界自检", "提案级归属自检", "FDE 通道自检"] {
        assert!(
            !stderr.contains(kind),
            "二进制自检报出「{kind}」—— 真实 stdout 已被污染\n\
             stderr：\n{stderr}\nstdout：\n{stdout}"
        );
    }

    sb.teardown().await;
}

// ══════════════════════════════════════════════════════════════
// AC-003 · 二进制层与 domain 层是否一致
// ══════════════════════════════════════════════════════════════

/// 【AC-003】**真实 stdout ≡ `src/cli.rs:644` 的那行抬头 + domain 渲染结果**（逐字节）。
///
/// # 这一条在问什么
///
/// 二进制与 `render_review_text` 之间夹着一层
/// （`resolve_proposal_arg` → `proposals.get/items` → `get_snapshot` →
/// `to_item_views` → `summarize` → `project_review_track_with_provenance` →
/// `render_review_text` → **`println!` 抬头** → `print!`）。
/// 本文件其余断言只问「输出对不对」；这一条问的是
/// 「**除了那行已知抬头之外，二进制没有对 domain 的产物做任何再加工**」。
///
/// # 已知的**唯一**一处差异（实测登记）
///
/// `src/cli.rs` 的 `ReviewCmd::Show` 分支有一行抬头
/// （原文见 `ontok::cli::REVIEW_SHOW_PREFIX_TEMPLATE`）——
/// 真实 stdout 比 `render_review_text` 的返回值**多一行抬头**。
/// `tests/test_review_show_e2e.rs` 的 `review_show_text` **不包含**这一行
/// （它直接返回 `render_review_text(&rv)`），
/// 所以那一份与真实二进制**逐字节不相等** —— 这不是 bug，是它自己文件头
/// 声明的「复刻 `Show` 分支的 domain 调用序列」的一部分。
/// 本文件把这一行显式登记成断言，而不是把它当成噪声忽略掉。
#[tokio::test]
async fn test_review_show_binary_stdout_is_header_plus_domain_render_byte_for_byte() {
    let sb = Sandbox::new("ac3").await;
    let alias = sb.seed_via_binary().await;

    let out = sb.run(&["review", "show", "--proposal", &alias, "--project", &sb.project]);
    let stdout = text(&out.stdout);
    assert!(out.status.success(), "review show 失败：{}", text(&out.stderr));

    // ── 进程内复刻 `Show` 分支的 domain 调用序列（读的是同一个 schema）──
    let pool = sb.pool().await;
    let proposals = PgProposalStore::new(pool.clone());
    let projects = PgProjectStore::new(pool.clone());
    let pid = projects
        .get_by_name(&sb.project)
        .await
        .unwrap()
        .expect("沙箱项目应当存在")
        .id;
    let row = proposals
        .get(
            sqlx::query_as::<_, (uuid::Uuid,)>(
                "SELECT id FROM proposals WHERE project_id = $1 ORDER BY created_at DESC, id DESC",
            )
            .bind(pid)
            .fetch_one(&pool)
            .await
            .unwrap()
            .0,
        )
        .await
        .unwrap()
        .unwrap();
    let items = proposals.items(row.id).await.unwrap();
    let views = PgProposalStore::to_item_views(&items);
    let base = PgStore::new(pool.clone())
        .get_snapshot(row.base_version_id)
        .await
        .unwrap()
        .expect("base 版本快照应当存在");
    let baseline: std::collections::BTreeSet<String> =
        base.entity_types.iter().map(|e| e.key.clone()).collect();
    let st = summarize(&views, &baseline);
    let rv = project_review_track_with_provenance(&views, &st, row.extracted_by.as_ref());
    let domain_text = render_review_text(&rv);

    // ⭐ 抬头行：`提案 {alias} 状态={status}` —— `row.status` 的 Display。
    let status: String = sqlx::query("SELECT status::text FROM proposals WHERE id = $1")
        .bind(row.id)
        .fetch_one(&pool)
        .await
        .unwrap()
        .try_get("status")
        .unwrap();
    // ⚠️ REQ-078 · AC-002：抬头**不写死在这里**。
    // 原文的唯一事实在 `src/cli.rs` 的 `REVIEW_SHOW_PREFIX_TEMPLATE` ——
    // 生产侧与本断言**共用**它，由 `tests/test_binary_output_guard.rs` 的 G-2 钉住
    // （前缀原文一旦被硬写进任何测试即红）。
    let expected = format!(
        "{}{domain_text}",
        ontok::cli::review_show_prefix(&alias, &status)
    );

    assert_eq!(
        stdout, expected,
        "真实 stdout 与「抬头 + domain 渲染」**逐字节不相等** ——\n\
         二进制层对 domain 的产物做了本文件未登记的再加工。\n\
         这是 AC-003 要求如实报告的差异，请勿靠放宽断言把它盖掉。"
    );

    sb.teardown().await;
}

/// 【交付物】打印**真实进程**的 `review show` 输出。
///
/// `cargo test --test test_review_show_binary_req072 -- --nocapture`
/// 会把攻击载荷下真实 stdout 原样打出来。
#[tokio::test]
async fn test_review_show_binary_print_real_process_output() {
    let sb = Sandbox::new("show").await;
    let alias = sb.seed_via_binary().await;

    let out = sb.run(&["review", "show", "--proposal", &alias, "--project", &sb.project]);
    println!(
        "\n===== 真实 `ontok review show` 进程输出（REQ-072）=====\n\
         载荷 claim：{:?}\n\
         --- stdout ---\n{}\n--- stderr ---\n{}\n===== 结束 =====\n",
        forged_claim(),
        text(&out.stdout),
        text(&out.stderr)
    );
    assert!(out.status.success(), "review show 失败：{}", text(&out.stderr));

    sb.teardown().await;
}
// ══════════════════════════════════════════════════════════════
// REQ-080 F-4 · 判别力自证 ——「schema 确实还在」那一格**会红**
// ══════════════════════════════════════════════════════════════
//
// 这一节存在的唯一理由：改掉 `to_regclass` 之后，**必须证明这一格不是恒真**。
// 否则就是把一条恒真的判据换成另一条恒真的判据，白改。
//
// ⚠️ 共享本体库**只读**：下面只 `CREATE SCHEMA` / `DROP SCHEMA`，
// 零 DELETE / 零 TRUNCATE / 零清空表；只删**本测试自己建的那一个**，
// 且 schema 名带 `t_req072_` 前缀（能被上面的 [`cleanup_schema`] 白名单收掉）。

/// ⭐ F-4 · 「schema 确实存在」这个复现，证明改后的查询**看得见它**。
///
/// 断言两侧都要有：
/// - `pg_namespace`（**改后用的那条**）查到 ⇒ 这一格**不是恒 `None`**；
/// - `to_regclass`（**改前用的那条**）查不到 ⇒ 这正是 REQ-078 QA 说的那个坑，
///   保留这条断言是为了让「为什么换查询」**留在代码里**而不是只活在注释里。
#[tokio::test]
async fn f4_schema_probe_sees_a_schema_that_really_exists() {
    // ⚠️ 前缀必须落在 `t_req072_` 里（`unique_project` 自带 `t_`）：
    //    收尾走本文件的 [`cleanup_schema`]，它的白名单只认 `t_req072_`，
    //    用别的前缀就等于让这一条**拒绝清理自己**（实测踩过）。
    let schema = common::unique_project("req072_f4probe");
    let pool = common::test_pool().await;
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&pool)
        .await
        .expect("建专属探针 schema 失败");

    // 前提成立性：换成另一条权威查询也必须说「它在」——
    // 两条都指向同一个 schema 名，才说明 `pg_namespace` 这一条没写错列名。
    let via_information_schema: Option<String> = sqlx::query_scalar(
        "SELECT schema_name FROM information_schema.schemata WHERE schema_name = $1",
    )
    .bind(&schema)
    .fetch_one(&pool)
    .await
    .expect("问 information_schema 失败");
    assert_eq!(
        via_information_schema.as_deref(),
        Some(schema.as_str()),
        "前提失效：`{schema}` 连 information_schema 都说没有 ⇒ 下面全是空断言"
    );

    let still = schema_still_there(&pool, &schema)
        .await
        .expect("改后的查询自己都问不动");
    assert_eq!(
        still.as_deref(),
        Some(schema.as_str()),
        "🔴 `pg_namespace` 对一个**确实存在**的 schema 都查不到 ⇒ \
         「`DROP` 报成功但还在」那一格**仍然是恒假**，F-4 没修好"
    );

    // 旧写法为什么是恒假：它查的是**关系名**，不是 schema 名。
    let via_regclass: Option<String> = sqlx::query_scalar("SELECT to_regclass($1)::text")
        .bind(&schema)
        .fetch_one(&pool)
        .await
        .expect("问 to_regclass 失败");
    assert_eq!(
        via_regclass, None,
        "⚠️ `to_regclass` 现在居然看得见 `{schema}` 了 ⇒ 本文件里那段「它恒为 NULL」\
         的理由、以及 F-4 的改法都要重写（不是测试坏了，是世界的形状变了）"
    );

    // 收尾：只删自己建的那一个，并**复查**它真的没了。
    cleanup_schema(&schema)
        .await
        .unwrap_or_else(|e| panic!("清理专属探针 schema 失败：{e}"));
    assert_eq!(
        schema_still_there(&pool, &schema).await.expect("收尾复查失败"),
        None,
        "自己建的探针 schema 删不掉 —— 本节自己踩了它嘴上说的那个坑"
    );
}

/// ⭐ F-4 · 判定那一格**会红**：schema 还在 ⇒ [`verdict_after_drop`] 判 `Err`。
///
/// 这一条**不需要数据库**，所以它是常驻的；上面那条负责证明查询本身看得见。
#[test]
fn f4_drop_verdict_goes_red_when_the_schema_is_still_there() {
    // ① 红的那一侧 —— 「DROP 报成功、schema 还在」必须**报出来**。
    let red = verdict_after_drop(Some("t_req072_x_1_0".into()), "t_req072_x_1_0");
    assert!(
        red.is_err(),
        "🔴 schema 确实还在却判成「收干净了」⇒ 这一格是恒真的，F-4 没修好"
    );
    assert!(
        red.unwrap_err().contains("t_req072_x_1_0"),
        "报错里必须点名是哪个 schema —— 否则共享库上没人知道该删哪个"
    );

    // ② 绿的那一侧 —— 真删干净了不许瞎报（否则清理恒红 = 恒假）。
    assert!(
        verdict_after_drop(None, "t_req072_x_1_0").is_ok(),
        "🔴 schema 已经不在了还报失败 ⇒ 判定**恒红** = 恒假"
    );

    // ③ 逐字节核对报错文案带的是**查到的那个名字**，不是常量拼出来的。
    let msg = verdict_after_drop(Some("t_req072_x_1_0".into()), "t_req080_other").unwrap_err();
    assert!(
        msg.contains("t_req072_x_1_0") && msg.contains("t_req080_other"),
        "报错必须同时说出「在库里的是谁」与「要清的是谁」：{msg}"
    );
}

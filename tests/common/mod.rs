//! 测试公共设施 —— 连接池、建表、唯一命名。
//!
//! `mod common;` 会被**每个**测试文件各自编译一次，
//! 所以本模块里必然有「这个文件用不到」的项 —— 那是正常的，
//! 不是死代码。关掉该 lint，否则每个测试文件都报一串假警告。

#![allow(dead_code)]

// `pub` 是 REQ-047 加的：G-1/G-2/G-3 三条判据要直接断言 `source_root` 的内部
// 契约（锚点顺序、上界、两类报错抬头），而它们住在 `tests/test_anchor_guard.rs`。
pub mod source_root;

/// REQ-107 · AC-005 的 golden 比对夹具（合成客户视图 + 概念组 + 总览）。
///
/// 抽成子模块是因为**两条**判据都要用它（基线复刻版与管线生产版）——
/// 各写一份夹具一定会漂。
pub mod req107;

/// 判据读源码的**唯一入口** —— 锚点是**运行时**定位到的 crate 根，
/// 不是编译期常量。REQ-046 起，仓内**所有**读源码的判据都走这里。
#[allow(unused_imports)]
pub use source_root::{crate_root, read_source};

/// **词法级**剥注释：只删注释，**保留字符串字面量**（它们是代码）。
///
/// 与 `tests/test_binary_output_guard.rs` 的 `strip_comments` 同一套口径
/// （行注释 / 可嵌套块注释 / 普通字符串 / raw string / 字符常量）：
/// 裸 grep 会栽在「守卫被删掉、字符串还留在注释里」这个形态上（REQ-002 复审栽过），
/// 而那正是源码扫描型判据最常见的假绿来源。
///
/// ⚠️ 保留字符串字面量 = 字符串里出现判据名会被算成一次「命中」。
/// 这是**刻意**的（与既有守卫一致）：宁可假红一次让人去看，也不要放过一次。
///
/// ⚠️ 已知边界：不认 `'` 作为生命周期标记（`&'static str` 会把后面的代码吞进
/// 「字符常量」状态直到下一个 `'`）。调用方只用它扫**小片段**（出口文件），
/// 而那几个文件里 `'` 只出现在字符常量与生命周期两处 —— 判据若因此漏算，
/// 会在 `strip_comments_is_not_vacuous` 的自检里暴露。
pub fn strip_comments(src: &str) -> String {
    let cs: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0usize;
    #[derive(PartialEq)]
    enum St {
        Code,
        Line,
        Block(usize),
        Str,
        Raw(usize),
        Char,
    }
    let mut st = St::Code;
    while i < cs.len() {
        let c = cs[i];
        match st {
            St::Code => {
                if c == '/' && cs.get(i + 1) == Some(&'/') {
                    st = St::Line;
                    i += 2;
                } else if c == '/' && cs.get(i + 1) == Some(&'*') {
                    st = St::Block(1);
                    i += 2;
                } else if c == 'r' {
                    let mut j = i + 1;
                    let mut hashes = 0usize;
                    while cs.get(j) == Some(&'#') {
                        hashes += 1;
                        j += 1;
                    }
                    if cs.get(j) == Some(&'"') {
                        out.extend(cs[i..=j].iter());
                        i = j + 1;
                        st = St::Raw(hashes);
                    } else {
                        out.push(c);
                        i += 1;
                    }
                } else if c == '"' {
                    out.push(c);
                    i += 1;
                    st = St::Str;
                } else if c == '\'' {
                    // 字符常量：`'x'` / `'\n'` / `'\u{..}'`。生命周期标记
                    // （`'static`）不以 `'` 结尾，所以这里只在「真的能闭合」时进 Char。
                    let closes = cs.get(i + 2) == Some(&'\'')
                        || (cs.get(i + 1) == Some(&'\\')
                            && cs[i + 2..].iter().take(12).any(|c| *c == '\''));
                    out.push(c);
                    i += 1;
                    if closes {
                        st = St::Char;
                    }
                } else {
                    out.push(c);
                    i += 1;
                }
            }
            St::Line => {
                if c == '\n' {
                    out.push('\n');
                    st = St::Code;
                }
                i += 1;
            }
            St::Block(depth) => {
                if c == '/' && cs.get(i + 1) == Some(&'*') {
                    st = St::Block(depth + 1);
                    i += 2;
                } else if c == '*' && cs.get(i + 1) == Some(&'/') {
                    st = if depth <= 1 { St::Code } else { St::Block(depth - 1) };
                    i += 2;
                } else {
                    if c == '\n' {
                        out.push('\n');
                    }
                    i += 1;
                }
            }
            St::Str => {
                if c == '\\' {
                    out.push(c);
                    if let Some(n) = cs.get(i + 1) {
                        out.push(*n);
                    }
                    i += 2;
                } else {
                    out.push(c);
                    i += 1;
                    if c == '"' {
                        st = St::Code;
                    }
                }
            }
            St::Raw(hashes) => {
                if c == '"' {
                    let mut j = i + 1;
                    let mut seen = 0usize;
                    while seen < hashes && cs.get(j) == Some(&'#') {
                        seen += 1;
                        j += 1;
                    }
                    if seen == hashes {
                        out.extend(cs[i..j].iter());
                        i = j;
                        st = St::Code;
                        continue;
                    }
                }
                out.push(c);
                i += 1;
            }
            St::Char => {
                if c == '\\' {
                    out.push(c);
                    if let Some(n) = cs.get(i + 1) {
                        out.push(*n);
                    }
                    i += 2;
                } else {
                    out.push(c);
                    i += 1;
                    if c == '\'' {
                        st = St::Code;
                    }
                }
            }
        }
    }
    out
}

use ontok::db::pool;
use ontok::db::project::{NewProject, PgProjectStore, ProjectError, ProjectRow};
use sqlx::postgres::PgPool;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// 每个测试自建一个连接池。
///
/// **刻意不用 `OnceCell` 全局共享**：cargo test 每个 `#[tokio::test]`
/// 各起一个 runtime，全局共享的池在**先跑的测试的 runtime 被销毁后**
/// 就变成「A Tokio 1.x context was found, but it is being shutdown」——
/// 失败会随机落在后半批测试上，且与被测逻辑无关。
/// 每测试一池的开销（一次 TCP + 认证）在这个规模完全可接受。
///
/// # 但「每测试一池」不等于「每池 5 条连接」（REQ-030）
///
/// 上面否决的是**共享同一个池**，不是**放开每池的连接数**。两者要分开看：
///
/// - 共享池的风险在**池的生命周期**（跨 runtime 存活）—— 已由上面那条否决；
/// - 每池连接数的风险在**连接占用总量** —— 这是下面这条要治的。
///
/// 每测试一池 ⇒ 峰值连接数 ≈ `单个测试二进制内的并行测试数 × 每池连接数`。
/// 测试库 `ontol` 所在的 PG 是**共享集群**，`max_connections = 100` 是**集群级**
/// 的，跨库共享，与 `postgres` / `superset` 等 5 个业务库
/// 抢同一批槽位（实测那 5 个库常年占 30 条，且不归我们管）。
/// 所以测试必须把自己那份压到最小，而不是「反正有 100 条」。
///
/// `max_connections(1)` 的可行性是可查证的，不是乐观假设：本仓 `tests/` 下
/// **没有任何一处**持着 `.acquire()` / `.begin()` 的连接跨越另一个取连接的 await，
/// 绝大多数测试就是一条连接顺序跑完。**真正需要池内并发的测试已经显式改走
/// [`test_pool_with`]**，不靠调高这个默认值来获得并发。
///
/// 逃生舱口在 [`test_pool_with`]：将来若有测试真的要池内并发，
/// 显式声明它需要几条，并在那里写清理由 —— 而不是把默认值悄悄调大。
pub async fn test_pool() -> PgPool {
    test_pool_with(1).await
}

/// 每个测试自建一个连接池，且**显式**指定每池连接数。
///
/// 绝大多数测试用 [`test_pool`]（= 1 条）就够。
/// **只有**真的在同一时刻从多个 task 取连接的测试才该调这个，
/// 并在调用处写清「需要几条、为什么」——
/// 因为把池调宽是有代价的：它直接抬高共享集群上的峰值连接数。
///
/// 已知的两处（两处都在**各自独立的测试二进制**里，不会互相叠加）：
/// - `test_project.rs::test_version_seq_under_concurrency_is_unique`：8 个 task 齐步走，
///   要的就是它们**真的同时**撞 UNIQUE(project_id, seq)；
///   池只有 1 条 ⇒ 8 个 task 串行 ⇒ 那个竞争窗口根本不存在，
///   测试退化成「永远通过」的假命题（该测试的注释原本就点明了这条）。
/// - `test_extraction_agent.rs::test_registry_concurrent_same_key_never_leaks_raw_db_error`：
///   8 个 task 登记同一个 `(agent, version)`，竞争窗口在数据库侧。
pub async fn test_pool_with(max_connections: u32) -> PgPool {
    let url = ontok::config::test_database_url().expect(
        "缺 ONTOK_TEST_DATABASE_URL —— 口令不得入库，请导出环境变量（见 .env.example）",
    );
    // ══ REQ-041：重试**只**包住下面这一个 await —— 建连接本身 ══
    //
    // 括号里的闭包**就是** `pool::connect_for_tests(...)`，一个函数调用、零其他语句。
    // 这一点是被判据钉死的（`tests/test_connect_retry.rs::…_scope_is_connect_only`
    // 读本文件的源码断言），不是靠自觉：
    //
    // - **类型层**：`retry_connect` 的 `F: FnMut() -> Fut`、
    //   `Fut: Future<Output = Result<PgPool, DbError>>`。全仓只有 `pool::connect*`
    //   产出这个类型 —— 想把「连上之后的事」塞进被重试的闭包，要么类型对不上
    //   （编译不过），要么就得先改 `retry_connect` 的签名（源码判据立刻红）。
    // - **源码层**：判据直接读 `retry_connect(...)` 的实参，断言它**只**含
    //   `connect_for_tests`，且不含 `migrate` / `execute` / `query` / `fetch`。
    //
    // 迁移、断言、业务读写全在这个 `await` **之外**，照旧暴露。
    match retry_connect(|| pool::connect_for_tests(&url, max_connections)).await {
        Ok(p) => p,
        Err(e) => panic!("{e}"),
    }
}

// ══════════════════════════════════════════════════════════════
// REQ-041 · 建连接阶段的有限重试 + 退避
// ══════════════════════════════════════════════════════════════

/// 建连接的**重试**次数 —— 总尝试次数 = `CONNECT_RETRIES + 1`。
///
/// ⚠️ 这个名字刻意叫「重试」而不是「尝试」：验收标准 AC-004 的第 1 条变异是
/// 「重试次数改成 0（= 不重试，退回原状）」，所以 0 必须**恰好**等于原行为
/// （直接 `connect_for_tests(...).await.expect("连不上测试库")`），不能等于「连都不试」。
///
/// # 2 这个取值是怎么定的（不是拍脑袋）
///
/// 单次尝试的墙钟上界由 `pool::connect_for_tests` 的
/// `acquire_timeout = Duration::from_secs(10)`（`src/db/pool.rs:74`）封顶 ——
/// 实测两种错误形态里，`Connect(PoolTimedOut)` 正是把这 10s 烧满，
/// `Connection reset by peer (os error 104)` 是快速失败。
/// **所以重试成本的主导项是 10s/次，而不是退避间隔。**
///
/// 于是总上界 ≈ `(2+1) × 10s + 0.3s + 0.6s = 30.9s`，
/// 对比一轮全量测试的基线 351s（5m51s）= **8.8%**，满足 AC-002
/// 「总耗时必须显著小于一轮全量测试」。
///
/// 再往上加次数就没意义了：4 次尝试就已经 40s（11%），5 次 50s（14%），
/// 而实测集群连接常驻只有 32~38 / 100 —— **槽位并没有被占满**，
/// 失败是握手阶段被拖住（accept backlog 一类），**秒级就会恢复**，
/// 给它两个 10s 窗口（合计 30s）已经远超真实抖动窗口。超过就是纯烧时间。
pub const CONNECT_RETRIES: usize = 2;

/// 退避基数：第 n 次重试前等 `BASE × 2^(n-1)` ms，封顶 `CAP`。
///
/// 300ms → 两次重试依次等 300ms / 600ms，合计 0.9s。
/// 取指数退避而不是固定间隔：两次失败若原因是同一个（集群满），
/// 固定 300ms 的第二次重试很可能仍落在同一个抖动窗口里；
/// 翻倍让第二次落点自然后移，而总代价仍然可以忽略（0.9s / 30.9s = 2.9%）。
pub const CONNECT_BACKOFF_BASE_MS: u64 = 300;
pub const CONNECT_BACKOFF_CAP_MS: u64 = 2_000;

/// 建连接重试的**硬墙钟上界** —— 与 `CONNECT_RETRIES` 相互独立的一道闸。
///
/// 30.9s 的最坏耗时是**按当前 `acquire_timeout=10s` 算出来的**。
/// 哪天有人把 `acquire_timeout` 调大、或把 `CONNECT_RETRIES` 调大，
/// 光靠次数就拦不住总耗时了；这道闸保证「一轮全量测试不会因为建连接被拖住」。
///
/// 取 35s：比 30.9s 的实际最坏值高 4.1s（留出进程调度/认证的零头），
/// 所以**在当前配置下它永远不触发** —— 它的作用是让「重试参数被人调大」
/// 这件事在 35s 处就被掐断，而不是线性放大。
pub const CONNECT_RETRY_BUDGET: Duration = Duration::from_secs(35);

/// 重试彻底失败后的错误。**信息必须够一个人判断下一步做什么**，
/// 所以显式带上「试了几次」和「最后一次到底错在哪」。
#[derive(Debug)]
pub struct ConnectRetryError {
    /// 实际发起的尝试次数（不是重试次数）。
    pub attempts: usize,
    /// 是否是被 [`CONNECT_RETRY_BUDGET`] 掐断的（而不是次数用完）。
    pub budget_exhausted: bool,
    /// 最后一次的原始错误。
    pub last: pool::DbError,
}

impl std::fmt::Display for ConnectRetryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let why = if self.budget_exhausted {
            format!(
                "撞上 {}s 的墙钟上界（次数还没用完：上限给了 {CONNECT_RETRIES} 次重试）",
                CONNECT_RETRY_BUDGET.as_secs()
            )
        } else {
            format!("{CONNECT_RETRIES} 次重试全部用完")
        };
        write!(
            f,
            "连不上测试库：已尝试 **{}** 次（首发 + {CONNECT_RETRIES} 次重试，退避 \
             {CONNECT_BACKOFF_BASE_MS}ms 起指数退避、封顶 {CONNECT_BACKOFF_CAP_MS}ms），\
             {why}，仍然失败。\n\
             最后一次的原始错误：{}\n\
             \n\
             这一步只重试**建连接**本身 —— 连上之后的迁移/断言失败不会被重试、照旧抛出。\n\
             若本条是集群高负载导致的握手抖动，隔一会儿重跑通常就好；\
             若反复出现，先查 ONTOK_TEST_DATABASE_URL 是否可解析，再查共享集群的 \
             max_connections 与当前连接占用（业务库常年占着几十条，不归我们管）。",
            self.attempts, self.last
        )
    }
}

impl std::error::Error for ConnectRetryError {}

/// 第 `attempt_no` 次失败后（1 起）该等多久。
pub fn backoff_for(attempt_no: usize) -> Duration {
    let shift = (attempt_no - 1).min(16) as u32; // 防 shift overflow
    let ms = CONNECT_BACKOFF_BASE_MS
        .saturating_mul(1u64 << shift)
        .min(CONNECT_BACKOFF_CAP_MS);
    Duration::from_millis(ms)
}

/// 建连接阶段的**有限**重试 + 指数退避。
///
/// # 为什么签名长成这样（AC-003 的结构保证）
///
/// `F: FnMut() -> Fut` + `Fut: Future<Output = Result<PgPool, pool::DbError>>`
/// —— 「可被重试的东西」在**类型上**就只剩「产出 `PgPool` 或 `DbError` 的操作」。
/// 全仓只有 `pool::connect` / `pool::connect_from_env` / `pool::connect_for_tests`
/// 产出这个类型。想把重试范围放宽到「连上之后的失败」，唯一的路是先改这个签名，
/// 而签名一改，源码判据立刻红。
///
/// 顺带这也是**运行期判据为什么证明不了 AC-003** 的原因：
/// 单个测试里「连上之后失败」和「建连接失败」的可观测表现完全一样，
/// 差别只在被重试的**闭包有多大**，而闭包大小运行期看不见。
///
/// # 为什么必须有界（AC-002）
///
/// 无界重试会把「快速失败」变成「测试套件挂死」—— 比现在更糟。
/// 两道独立的闸：次数（[`CONNECT_RETRIES`]）与墙钟（[`CONNECT_RETRY_BUDGET`]）。
///
/// # 正常路径零开销
///
/// 第一次尝试前不退避；连接成功即返回。
pub async fn retry_connect<F, Fut>(mut attempt: F) -> Result<PgPool, ConnectRetryError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<PgPool, pool::DbError>>,
{
    let started = Instant::now();
    let mut attempts = 0usize;
    let mut last: Option<pool::DbError> = None;
    let mut budget_exhausted = false;

    while attempts <= CONNECT_RETRIES {
        if started.elapsed() >= CONNECT_RETRY_BUDGET {
            budget_exhausted = true;
            break;
        }
        attempts += 1;
        match attempt().await {
            Ok(p) => return Ok(p),
            Err(e) => {
                last = Some(e);
                if attempts > CONNECT_RETRIES {
                    break;
                }
                let wait = backoff_for(attempts);
                if started.elapsed() + wait >= CONNECT_RETRY_BUDGET {
                    budget_exhausted = true;
                    break;
                }
                tokio::time::sleep(wait).await;
            }
        }
    }

    Err(ConnectRetryError {
        attempts,
        budget_exhausted,
        // `CONNECT_RETRIES >= 0` ⇒ 循环体至少跑一次 ⇒ `last` 必为 Some。
        // 万一有人把 `while` 改成不执行，这里也不该 panic，而是给一条能读的错误。
        last: last.unwrap_or(pool::DbError::MissingEnv(
            "ONTOK_TEST_DATABASE_URL（重试一次都没发起，这本身就是缺陷）",
        )),
    })
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// 生成一个「本轮应当唯一」的 project 名。
///
/// **不做 truncate** —— 外部库多人共用，truncate 会打断别人的运行。
/// 隔离靠唯一命名：`t_{prefix}_{pid}_{n}`。
///
/// ⚠️ **这个「唯一」保证不了，撞名是有意保留的可观测故障，不是要消灭的噪声。**
///
/// 原理：`{pid}` 由 OS 分配，**OS 会回收并复用 pid**；`{n}` 是进程内计数器，
/// 每次 `cargo test` 都从 0 重新开始。于是只要有两次测试运行拿到**同一个 pid**
/// 且走过的 `unique_project` 调用**序列与 prefix 也一致**，
/// `t_{prefix}_{pid}_{n}` 就会撞上**另一轮**早已建好的项目。
///
/// 所以本模块配套的 [`new_project`] / [`fresh_project`] 一律走
/// `create`（撞名 ⇒ `NameTaken` ⇒ 响亮失败），
/// **不用 `get_or_create`**（撞名 ⇒ 静默复用旧项目 ⇒ 断言读到别人的数据）。
pub fn unique_project(prefix: &str) -> String {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let pid = std::process::id();
    format!("t_{prefix}_{pid}_{n}")
}

/// 建一个新 project，**撞名即响亮失败**。名字已由调用方算好。
///
/// # 为什么测试不能走 `get_or_create`
///
/// `get_or_create` 是 **fail-open**：名字撞了就**静默返回那个旧项目**
/// （`src/db/project.rs` 里 `get_by_name` 命中就直接 return）。
/// 测试拿到的于是是**别的进程、别的轮次**建的项目 —— 断言会去读那份脏数据。
///
/// 这不是理论风险，是已经踩过的：有人用 `get_or_create` + `unique_project`
/// 的组合跑测试，pid 撞上后断言读到的是**五天前某次 CI 建的项目**。
/// 后果是**假红**（读到别人的 seq 6，本轮只加了 3），
/// 以及更危险的**假绿嫌疑**（「失败的 apply 不得建出新版本」这类断言，
/// 被污染后读起来像「apply 失败却建了版本」—— 一个远比实际严重的指控）。
///
/// `create` 是 **fail-closed**：`ON CONFLICT (name) DO NOTHING` ⇒ 读不到行
/// ⇒ `ProjectError::NameTaken`。撞名在这里**变成一条响亮的测试失败**，
/// 而不是一段悄悄错位的断言。
///
/// ⚠️ 本函数只用于**测试侧**。`get_or_create` 作为生产 API 保持原语义
/// —— CLI 首次跑时少一步查询是它的正当用途，不在本任务范围内。
pub async fn new_project(store: &PgProjectStore, name: &str) -> ProjectRow {
    match store.create(NewProject { name: name.to_string() }).await {
        Ok(p) => p,
        Err(ProjectError::NameTaken(_)) => panic!(
            "撞名：项目 `{name}` **已经存在**（NameTaken）。\n\
             本测试要求每次运行都建**全新**项目，读到旧项目就是被污染了。\n\
             构造原理：`unique_project` 产出 `t_{{prefix}}_{{pid}}_{{n}}` —— \n\
             pid 由 OS 分配且**会被回收复用**，`{{n}}` 每次 `cargo test` 从 0 重来，\n\
             所以不同进程之间 `t_{{prefix}}_{{pid}}_{{n}}` **会真的撞名**。\n\
             这次撞名说明本轮拿到的 pid 与另一轮相同、且 unique_project 调用序也对上了。\n\
             **不要改成随机/时间戳命名把问题藏起来** —— 撞名必须留得住。\n\
             共享库里遗留的项目一律不清（外部多人共用，清它会打断别人的运行）。"
        ),
        Err(e) => panic!("建 project `{name}` 失败（**不是**撞名）：{e}"),
    }
}

/// [`new_project`] + [`unique_project`] 的合体：按 prefix 现场算名字再建。
///
/// 绝大多数「建个干净项目接着用」的测试点走这个。
pub async fn fresh_project(store: &PgProjectStore, prefix: &str) -> ProjectRow {
    new_project(store, &unique_project(prefix)).await
}

/// 建表（幂等）。直接用生产代码的 DDL —— **不另抄一份**。
///
/// 抄一份的代价是：生产侧改了 schema，测试侧还按旧的建，
/// 于是测试通过而真实运行炸掉（或者反过来）。
pub async fn migrate(pool: &PgPool) {
    ontok::db::pg::migrate(pool).await.expect("建表失败");
}

//! CLI —— 端到端演示入口（AC-012）。
//!
//! 设计取向：**每条命令都要能被人手动敲一遍看结果**，
//! 所以输出用人话 + 可 grep 的行，不刷 ANSI 花色。
//!
//! # 提案的两种指法
//!
//! AC-012 的命令里写的是 `--proposal p1`（别名），而库里是 UUID。
//! 两种都支持：`p1` = **该项目内按创建时间倒序**的第 1 个提案
//! （也就是「刚导入的那个」），`p2` = 第 2 个，以此类推；
//! 直接给 UUID 也行。别名是给人用的，UUID 是给脚本用的。
//!
//! **别名是「项目内序号」**：解析（`resolve_proposal`）与打印（`proposal_alias`）
//! 都带 `WHERE project_id = $1`，别的项目的提案既不参与计数、也解析不到。
//! 审核动作因此只会落在**本次导入的那个项目**里。

use crate::db::apply::PgApplier;

use crate::db::project::PgProjectStore;
use crate::db::proposal::{NewItem, NewProposal, PgProposalStore};
use crate::db::snapshot_store::PgStore;
use crate::domain::diff::diff_snapshots;
use crate::domain::review::{summarize, Decision, Track};
use crate::domain::snapshot::{ExtractedBy, ProvenanceSource};
use crate::domain::view::SnapshotView;
use anyhow::Context;
use clap::{Parser, Subcommand};
use serde_json::Value;
use sqlx::postgres::PgPool;
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "ontok", about = "FDE 本体论平台 · 本体内核（切片 1，只读闭环）")]
pub struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
pub enum Cmd {
    /// 新建项目
    Project { #[command(subcommand)] cmd: ProjectCmd },
    /// 版本操作
    Version { #[command(subcommand)] cmd: VersionCmd },
    /// 提案
    Propose { #[command(subcommand)] cmd: ProposeCmd },
    /// 审核
    Review { #[command(subcommand)] cmd: ReviewCmd },
    /// 提案应用
    Proposal { #[command(subcommand)] cmd: ProposalCmd },
    /// 本体查看
    Ontology { #[command(subcommand)] cmd: OntologyCmd },
    /// REQ-010 · 业务概念组（§7.1 阶段 3）
    Concept { #[command(subcommand)] cmd: ConceptCmd },
    /// REQ-108 · 置信度校准（AC-011）
    ///
    /// 把「分数」与「人工最终决定」对起来，输出**分箱命中率**。
    /// 样本不足时**明确说样本不足**，不输出 0% / 100% 冒充结论。
    Confidence { #[command(subcommand)] cmd: ConfidenceCmd },
    /// REQ-027 · 以**标准 MCP**（stdio）把**冻结快照**暴露给客户端
    ///
    /// 只服务已冻结的版本 —— 未冻结时**拒绝启动**（fail-closed）。
    McpServer {
        /// 项目名
        #[arg(long)]
        project: String,
        /// 第几版（该版必须已冻结）
        #[arg(long)]
        version: i32,
    },
    /// REQ-093 · 起一个**只读** HTTP 服务，把本体图关系用浏览器看
    ///
    /// 只服务已冻结的版本 —— 未冻结时**拒绝启动**（fail-closed）。
    /// 只接受 GET：任何「改动」类请求一律 405，没有可写的地方。
    /// 监听地址只允许回环（`127.0.0.1` / `::1`），绑别处直接拒绝启动。
    ///
    /// 【可以照抄的一整条】
    ///
    /// export ONTOK_VIEW_TOKEN='换成你自己的口令'
    /// cargo run --bin ontok -- serve \
    /// --project demo3 --version 2 --bind 127.0.0.1:8787
    ///
    /// 然后浏览器打开 http://127.0.0.1:8787/
    ///
    /// ⚠️ **三个参数缺一不可。** 少了任何一个，clap 会**直接报错退出（exit 2）**，
    /// 并把缺哪个、怎么写打出来 —— 不是安静地什么都不返回。
    ///
    /// 【还要带查询参数】页面顶部填入口令后才发请求，而请求是：
    ///
    /// /api/graph?project=demo3&version=2
    ///
    /// ⚠️ **查询参数漏了会得到一句人话 400**（同样不静默）：
    /// `要说明看哪个项目：请带上 project 参数。`
    ///
    /// ⚠️ **访问口令从环境变量 `ONTOK_VIEW_TOKEN` 读**，缺了或短于 8 个字符
    /// 都拒绝启动，**没有开发模式开关**。浏览器侧把它填进页面顶部的口令框，
    /// 它只留在内存里，不进地址栏、不进页面源码、不进浏览器存储。
    ///
    /// ⚠️ **这个身份门是临时的、刻意不完整的**：只有一个静态共享令牌，
    /// 没有过期、没有吊销、不能区分「谁在看」。完整的
    /// subject → 本体 object key 解析是 REQ-099（决策 D1），不在本需求内。
    // ⚠️ **示例必须顶格写、不能缩进** —— rustdoc 把 doc 注释里**缩进 4 格以上**
    // 的块当成 **Rust 代码块**去编译，于是 `export …` / `cargo run … \`
    // 会被当 Rust 解析、编译失败，`cargo test` 的 Doc-tests 直接红
    // （实测踩过一次：`character literal may only contain one codepoint`）。
    // 顶格写则只是普通段落，rustdoc 不碰它；`verbatim_doc_comment` 又让
    // clap 原样保留换行 ⇒ `--help` 里仍然是一整条可照抄的命令。
    #[command(verbatim_doc_comment)]
    Serve {
        /// 项目名
        #[arg(long)]
        project: String,
        /// 第几版（该版必须已冻结）
        #[arg(long)]
        version: i32,
        /// 监听地址。**只允许回环地址**（如 127.0.0.1:8787）；
        /// 绑到非回环地址会被拒绝启动
        #[arg(long, default_value = "127.0.0.1:8787")]
        bind: String,
    },
}

#[derive(Subcommand)]
pub enum ConceptCmd {
    /// 把一份**概念声明**导入成提案（走双轨审核，不直接生效）
    Import {
        #[arg(long)]
        file: PathBuf,
        /// 项目名。声明本身与项目无关（同一份声明可用于不同项目），
        /// 所以这里必须显式给 —— 免得把 A 项目的声明悄悄合进 B 项目。
        #[arg(long)]
        project: String,
        /// 基于哪个版本提案。默认 v1
        #[arg(long, default_value_t = 1)]
        base_version: i32,
        /// 轮次（写入 proposals.round）
        #[arg(long, default_value_t = 1)]
        round: i32,
    },
    /// 打印当前内容指纹，供声明作者更新 `history`
    ///
    /// **它不绕过契约**：指纹对不上时 `import` 照样拒。
    /// 它只是让「升版本」这件事不用手算哈希。
    Digest {
        #[arg(long)]
        file: PathBuf,
    },
}

#[derive(Subcommand)]
pub enum ProjectCmd {
    /// 新建项目（同名则复用）
    New {
        #[arg(long)]
        name: String,
    },
    /// 列出项目
    List,
}

#[derive(Subcommand)]
pub enum VersionCmd {
    /// 新建版本
    New {
        #[arg(long)]
        project: String,
    },
    /// 列出版本
    List {
        #[arg(long)]
        project: String,
    },
    /// 冻结版本
    Freeze {
        #[arg(long)]
        project: String,
        #[arg(long)]
        version: i32,
    },
}

#[derive(Subcommand)]
pub enum ProposeCmd {
    /// 从 JSON 文件导入提案
    Import {
        #[arg(long)]
        file: PathBuf,
    },
}

/// REQ-108 · 置信度相关命令（AC-011）。
#[derive(Subcommand)]
pub enum ConfidenceCmd {
    /// 输出**分箱命中率**（分数 vs 人工接受）
    ///
    /// 分箱：`0.00–0.30 / 0.30–0.60 / 0.60–0.75 / 0.75–0.90 / 0.90–1.00`。
    /// 人工决定取**业务轨**（`biz_status`）：关系对不对是业务判断，
    /// 建模轨判的是「建得规不规范」，不是同一件事。
    ///
    /// ⚠️ **任一分箱 < 5 条 ⇒ 整份报「样本不足」，不输出接受率结论、不算 ECE。**
    /// 空库运行同样明确提示 —— 不打印 `0%`（那是「一条都没接受」，
    /// 而事实是「一条都没判过」，两者完全不同）。
    Calibrate {
        /// 项目名（该项目的全部提案条目一起统计）
        #[arg(long)]
        project: String,
        /// 只统计挂在某个基版本上的提案？省略 = 该项目的全部提案
        #[arg(long)]
        version: Option<i32>,
    },
}

#[derive(Subcommand)]
pub enum ReviewCmd {
    /// 记录一次单轨判定
    Decide {
        #[arg(long)]
        proposal: String,
        /// 条目序号（1 起）。**与 `--accept-all` 互斥** ——
        /// 用 Option 是因为 clap 会把非 Option 参数视为「必需」，
        /// 那 `conflicts_with` 就没法让它缺席。
        #[arg(long)]
        item: Option<i32>,
        /// biz | model。与 `--accept-all` 互斥（同样因为 Option 才能缺席）。
        #[arg(long)]
        track: Option<String>,
        #[arg(long)]
        accept: bool,
        #[arg(long)]
        reject: bool,
        /// 项目名。**别名 `pN` 按项目内序号解析** —— 要审核别的项目请显式指定。
        /// 省略时作用域取「最近导入的提案所属项目」。
        #[arg(long)]
        project: Option<String>,
        /// 一次性接受**所有条目的两条轨**，不等价于逐条敲 —— 走的是同一条
        /// `set_decision` 路径与同一套领域校验。
        ///
        /// **为什么需要它**：REQ-003 从真实源库抽出 142 个条目，
        /// 逐条 `--item N` 要 284 次进程启动、每次重建连接池，
        /// E2E 根本跑不完（实测 15 分钟才走到一半就被环境中断）。
        #[arg(long, conflicts_with_all = ["item", "track", "accept", "reject"])]
        accept_all: bool,
    },
    /// 显示提案状态
    Show {
        #[arg(long)]
        proposal: String,
        /// 同 `decide`：别名按项目内序号解析
        #[arg(long)]
        project: Option<String>,
        /// REQ-108 · 一并输出**置信度分档表**（审核界面的置信度通道）。
        ///
        /// 刻意做成**开关**而不是默认输出：`review show` 的 stdout 字节
        /// 被 REQ-072/078 的真进程比对钉着（`review_show_prefix ++ 渲染文本`），
        /// 默认多打一行就会让那批判据红 —— 那不是「判据过时」，是**契约变了**。
        /// 需要看分数的审核者显式带上这个开关；客户视图与此无关（它压根不读）。
        #[arg(long)]
        confidence: bool,
    },
    /// ⭐ REQ-111 · 起一个**本地审核界面**（在 HTML 里做双轨审核）
    ///
    /// 与 `serve` 是**两个命令**，不是给 `serve` 加一个开关：
    ///
    /// | `serve` | `review serve` |
    /// | --- | --- |
    /// | 只读：任何非 GET ⇒ 405 | 可写：**唯一**写点是 `POST /api/decide` |
    /// | 只服务**已冻结**版本 | 只服务**未冻结**的提案（已冻结 ⇒ 拒绝启动） |
    ///
    /// 加开关会让「`serve` 是只读的」从**结构保证**退化成**配置保证** ——
    /// 那正是这个仓库反复栽的坑（REQ-093 的硬判据是「没有可写的地方」）。
    ///
    /// 【可以照抄的一整条】
    ///
    /// ontok review serve --project demo3 --proposal p1 --bind 127.0.0.1:8788 --token '换成你自己的口令'
    ///
    /// 然后浏览器打开 http://127.0.0.1:8788/
    ///
    /// ⚠️ **只绑回环**：`--bind 0.0.0.0:8788` 直接拒绝启动（exit≠0）。
    /// ⚠️ 口令也可以从环境变量 `ONTOK_VIEW_TOKEN` 读；两处都没有 ⇒ 拒绝启动。
    /// ⚠️ 页面壳（`/`）免口令、**不含任何数据**；`/api/*` 全部要 Bearer 口令。
    #[command(verbatim_doc_comment)]
    Serve {
        /// 项目名
        #[arg(long)]
        project: String,
        /// 提案（`p1` = 该项目内**最近导入**的那个；也可直接给 UUID）
        #[arg(long)]
        proposal: String,
        /// 监听地址。**只允许回环地址**（如 127.0.0.1:8788）
        #[arg(long, default_value = "127.0.0.1:8788")]
        bind: String,
        /// 访问口令（至少 8 个字符）。省略时读环境变量 `ONTOK_VIEW_TOKEN`
        #[arg(long)]
        token: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum ProposalCmd {
    /// 应用提案，产生新版本
    Apply {
        #[arg(long)]
        proposal: String,
        /// 同 `review decide`：别名按项目内序号解析
        #[arg(long)]
        project: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum OntologyCmd {
    /// 显示本体（默认客户视图）
    Show {
        #[arg(long)]
        project: String,
        #[arg(long)]
        version: i32,
        /// 输出建模视图（含 key / 证据），供 FDE 用
        #[arg(long)]
        model_view: bool,
        /// 输出**自包含单文件 HTML**（判读用，零外部依赖、可离线双击打开）
        #[arg(long)]
        html: bool,
        /// HTML 输出路径。省略则打到标准输出
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// 抽样高亮清单（每行一个对象名）。省略用判读手册 §3.1 的 13 个
        #[arg(long)]
        sampling: Option<PathBuf>,
    },
    /// ⭐ REQ-110 · **导出 TTL（OWL Turtle）** —— 产品自己就能导出，不再靠外部脚本。
    ///
    /// 默认导出**客户视图**（页面看到什么就导出什么，R1）；
    /// 带 `--model-view` 导出**建模视图**（含 key / 列名 / 基数 / 语义，供 FDE 用）。
    ///
    /// 【可以照抄的一整条】
    ///
    /// ontok ontology export --project demo3 --version 2 --out demo3-v2.ttl
    ///
    /// 同一版本连续导出两次**逐字节相同**（不写生成时间）；客户视图导出**过守门器**，
    /// 命中建模术语时以退出码 2 拒绝（词条只进 stderr）。文件头显式声明有损项
    /// （ActionType 未导出 / semantic 降级为注释 / cardinality 只映射两种特征）。
    Export {
        /// 项目名
        #[arg(long)]
        project: String,
        /// 第几版
        #[arg(long)]
        version: i32,
        /// 导出**建模视图**（默认是客户视图）
        #[arg(long)]
        model_view: bool,
        /// 输出路径。省略则打到标准输出
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// IRI 基（本体 IRI = `<base><project>`，前缀 = `<base><project>#`）
        #[arg(long, default_value = "https://ontok.local/")]
        base: String,
        /// 导出格式。今天只有 `ttl`（Turtle）；写别的值**直接报错**，不静默回退
        #[arg(long, default_value = "ttl")]
        format: String,
    },
    /// 比较两个版本
    Diff {
        #[arg(long)]
        project: String,
        a: i32,
        b: i32,
    },    /// REQ-010 · **业务总览**（按概念出总览；一个组都没有时退化成对象清单）
    Overview {
        #[arg(long)]
        project: String,
        #[arg(long)]
        version: i32,
        /// 输出 JSON（默认给人读的是文本）
        #[arg(long)]
        json: bool,
    },
}

/// 装一个忽略 EPIPE 的 panic hook。
///
/// **为什么必须装**：`println!` 遇到 broken pipe 会 panic，
/// 而那个 panic 发生在**子函数里的 println**，`run()` 的 `Result` 收不到 ——
/// 只在 `run` 里 try/catch 是抓不住的。唯一能统一处理的地方是 panic hook。
///
/// 场景：`ontok ontology diff ... | head -2`。
/// 用户只想看前几行，却看到一屏 std::io 栈，会误判「程序有 bug」。
pub fn install_broken_pipe_guard() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let msg = info.to_string();
        // payload 才是真正的原因；info 的 Display 未必带
        let payload = info.payload();
        if let Some(s) = payload.downcast_ref::<&str>() {
            if s.contains("Broken pipe") || s.contains("os error 32") {
                return; // 静默退出
            }
        }
        if let Some(s) = payload.downcast_ref::<String>() {
            if s.contains("Broken pipe") || s.contains("os error 32") {
                return;
            }
        }
        if msg.contains("Broken pipe") || msg.contains("os error 32") {
            return;
        }
        prev(info);
    }));
}

// ══════════════════════════════════════════════════════════════
// REQ-078 · 二进制层差分守卫的**生产侧接线**
// ══════════════════════════════════════════════════════════════
//
// # 这段代码在补哪一格
//
// REQ-072 的 QA 用变异 **M2** 证明了一件事：把 `review show` 里那行
// `print!("{text}")` 改成打印前做一次 `str::replace`
// （**domain 层一行未改**），实测：
//
// ```text
// 全量 53 个测试二进制：passed=738 / failed=4 / ignored=1
// 退出码实测 = 0
// stderr 完全为空（三道自检一声不吭）
// ```
//
// 根因**不是疏忽，是接线方式的必然**：判据
// （`verify_track_separation` / `verify_provenance_scope` / `audit_fde_channel`）
// 收的是 **domain 产物**，而打印出去的可以是它的**再加工**。
// 两者不是同一个对象 ⇒ 打印点的任何加工，对自检**结构性不可见**。
//
// ⚠️ **不承诺封堵这一类**（AC 的范围判断）。能做到的是**让它可见**：
//
// 1. [`emit_domain_text_with`] —— domain 产物通往 stdout 的**唯一**出口。
//    它写出字节，并把**同一个切片**交给判据闭包；
//    闭包**在签名上只拿得到这一个切片**。
//    ⚠️ REQ-080 F-1 实测修订：「插不进再加工」这句**不成立** —— Rust 闭包会
//    **捕获环境里的绑定**，所以
//    `emit_domain_text_with(&text.replace(..), |judged| …verify(&rv, &text))`
//    写得出来、编译得过（写出去的已加工、判据读未加工）⇒ 真 M2（M2d）。
//    那一档由 `tests/test_binary_output_guard.rs` 的 **G-1** 判红，
//    **不由** [`EXIT_POLICY`] 判红（那一档实测退出码是 **0**）。
// 2. [`DOMAIN_PRINT_SITES`] —— **显式登记表**：每一处「把 domain 产物写进
//    stdout」的站点连同它的前缀都在册。⛔ 漏登记即红
//    （`tests/test_binary_output_guard.rs` 的 G-2/G-5）。
// 3. [`EXIT_POLICY`] —— 退出码政策**写死成文字**，并由 [`self_check_exit_code`] 落地。

// ───────────────────── 退出码政策（AC-004）─────────────────────

/// 成功。
pub const EXIT_OK: i32 = 0;
/// 操作性错误：参数、连库、找不到提案/版本（`main.rs` 的 `Err` 分支）。
pub const EXIT_OPERATION_FAILED: i32 = 1;
/// 守门器判定**泄露建模术语** —— fail-closed，拒绝出图（既有码，见 [`ontology_cmd`]）。
pub const EXIT_LEAKED_MODELING_TERMS: i32 = 2;
/// ⭐ REQ-078 新增：**自检违规** —— 三道自检**从它们读到的那份串上**报出了东西。
///
/// ⚠️ REQ-080 F-1 如实修订：REQ-078 的注释写的是「渲染被再加工」，那一半
/// **实测不成立** —— 真 M2（M2d：闭包捕获外层绑定，判据读未加工那份）下
/// 三道自检**合法沉默**，退出码实测 **0**。
/// ⇒ 3 只承载「自检报了」，**不承载**「渲染被再加工」。
pub const EXIT_SELF_CHECK_VIOLATION: i32 = 3;

/// REQ-078 · 退出码政策的**唯一事实源**（文字 + 码）。
///
/// | 码 | 含义 |
/// | --- | --- |
/// | [`EXIT_OK`] | 1 / 2 / 3 都没触发；三道自检**从它们读到的那份串上**没报出违规 |
/// | [`EXIT_OPERATION_FAILED`] | 操作性错误（没进到渲染这一步） |
/// | [`EXIT_LEAKED_MODELING_TERMS`] | 守门器判定泄露建模术语 ⇒ 拒绝出图 |
/// | [`EXIT_SELF_CHECK_VIOLATION`] | 三道自检**报出了东西** |
///
/// # ⚠️ 0 **不承载**「stdout 逐字节 = 前缀 ++ 渲染」（REQ-080 F-1 实测修订）
///
/// REQ-078 那一版把 0 写成「成功，且 stdout **逐字节 = 已登记前缀 ++ domain 渲染**」。
/// 那句话在**真 M2 下是假的**，实测（REQ-080 QA，M2d）：
///
/// ```text
/// emit_domain_text_with(&text.replace('\u{240A}', "\n"), |judged| {
///     verify_track_separation(&rv, &text)   // ← 闭包**捕获**外层 `text`
/// })
/// ⇒ 写出去的已加工、判据读未加工 ⇒ 三道自检**合法沉默**
/// ⇒ self_check_exit_code(&[]) == None ⇒ 退出码 **0**，而 stdout 已被攻破
/// ```
///
/// ⇒ 「stdout == 渲染」那一格**不由退出码守**，它由
/// `tests/test_binary_output_guard.rs` 的 **G-1**（源码级接线守卫）与
/// `tests/test_domain_print_sites_binary_req078.rs` 的**真进程字节比对**守。
/// 退出码 0 是下游**最容易被当证据**的那个信号 ⇒ 它必须先说自己不承载什么。
///
/// ⛔ **拿「退出码 0」当渲染正确性的证据，在本政策下依然不成立** ——
/// REQ-072 QA 在 M2 下测到 0；REQ-080 QA 在真 M2 下复现了同一形状。
pub const EXIT_POLICY: &str = "\
退出码政策（REQ-078 AC-004；0 号码那一行由 REQ-080 F-1 如实修订）：
  0 = 1 / 2 / 3 都没触发：三道自检从**它们读到的那份串**上没报出违规
  1 = 操作性错误（参数 / 连库 / 找不到提案或版本）
  2 = 守门器判定泄露建模术语 ⇒ fail-closed，拒绝出图
  3 = 三道自检报出了东西
⛔ 0 **不承载**「stdout 逐字节 = 前缀 ++ 渲染」—— 真 M2（`emit_domain_text_with(&text.replace(..), |judged| …verify(&rv, &text))`，
   闭包捕获外层绑定）下三道自检合法沉默 ⇒ 退出码实测 **0**，而 stdout 已被攻破（REQ-080 F-1 实测）。
   ⇒ 「stdout == 渲染」由源码守卫 G-1 与真进程字节比对守，**不由退出码守**。
⛔ 拿退出码 0 当「渲染没问题」的证据，在本政策下**依然不成立**。";

/// 自检有违规 ⇒ 该退出什么码；没有 ⇒ `None`（照常 0）。
///
/// **纯函数**：没有违规就返回 `None`，**不**自己调 `exit` ——
/// 「判出来」与「退出去」是两件事，判据才可被直接断言（`tests/test_binary_output_guard.rs`）。
pub fn self_check_exit_code(violations: &[String]) -> Option<i32> {
    if violations.is_empty() {
        None
    } else {
        Some(EXIT_SELF_CHECK_VIOLATION)
    }
}

// ───────────────── 打印点登记表（AC-002 / AC-005）────────────────

/// 一个「把 domain 产物写进 stdout」的**登记项**。
///
/// # 为什么要一张表而不是散在各处的常量
///
/// REQ-072 的做法是把抬头行硬写进测试里的 `format!`
/// （`format!("提案 {alias} 状态={status}\n{domain_text}")`）——
/// 生产侧改抬头、测试侧忘了改 ⇒ **假红**；反过来测试先改 ⇒ **假绿**。
/// 常量散落在各处就会漂移，这是本会话反复出现的病。
/// ⇒ 抬头原文的**唯一**事实在 [`REVIEW_SHOW_PREFIX_TEMPLATE`]，
/// 生产与测试都从它派生前缀。
#[derive(Debug, Clone, Copy)]
pub struct DomainPrintSite {
    /// 站点所在的函数名 —— 同名绑定（如两个 `text`）靠它区分。
    pub enclosing_fn: &'static str,
    /// clap 子命令路径（人读的，判据只拿它做报错定位）。
    pub command: &'static str,
    /// 站点在 `src/cli.rs` 里把产物**写出去的那个表达式**。
    pub binding: &'static str,
    /// 允许**先写**的前缀（模板；`{alias}` / `{status}` 由 [`review_show_prefix`] 替换）。
    pub prefix_template: &'static str,
    /// 是否在产物**之后**补一个换行（`println!` 语义；`print!` 语义为 `false`）。
    pub trailing_newline: bool,
    /// ⭐ 这个站点的「同一份」靠**哪一种接线**保证 —— AC-001 的登记表版本。
    ///
    /// | 值 | 含义 | 守卫断言 |
    /// | --- | --- | --- |
    /// | [`WIRING_CLOSURE`] | 走 `emit_domain_text_with(text, \|judged\| …)`：**类型**保证 —— 闭包拿不到别的串 | 闭包体内不得再出现 `binding`；判据实参 == 闭包参数 |
    /// | [`WIRING_SAME_BINDING`] | 判据与写出读**同一个绑定**（先判后写，fail-closed） | 判据的文本实参与写出表达式**逐词元相等** |
    /// | [`WIRING_NONE`] | 刻意不过判据 | 源码里不得出现该站点的判据调用 |
    pub wiring: &'static str,
    /// 判据（守门器 / 自检）名 —— 它们必须收到**与本绑定同一份**字节。
    /// 空 = 该站点**刻意不过**判据（豁免已在 `cli.rs` 里写明理由）。
    pub judged_by: &'static [&'static str],
    /// 判**投影**（结构化数据）而不是判**渲染文本**的判据。
    ///
    /// ⚠️ 刻意与 [`Self::judged_by`] 分开：`audit_fde_channel` 读的是
    /// `ModelFacing` 的溯源（REQ-057 修的就是这一点），
    /// **它本来就不该收到渲染串** —— 把 `text` 喂给它反而是把它变成别的判据。
    /// 列在这里是为了让「哪些判据收文本、哪些收投影」成为**可读的结论**，
    /// 而不是靠读者从代码里推。
    pub projection_judged_by: &'static [&'static str],
    /// 有没有**真进程**端到端断言 `stdout == 前缀 ++ 产物 ++ 尾换行`。
    pub e2e_covered: bool,
    /// 覆盖状态的人读说明（未覆盖时**必须**写清为什么）。
    pub note: &'static str,
}

/// 接线形态：`emit_domain_text_with` 的闭包（**类型**保证）。
pub const WIRING_CLOSURE: &str = "closure";
/// 接线形态：判据与写出读**同一个绑定**（先判后写、fail-closed）。
pub const WIRING_SAME_BINDING: &str = "same_binding";
/// 接线形态：刻意不过判据。
pub const WIRING_NONE: &str = "none";
/// ⭐ REQ-107 · 接线形态：写出的是**管线产出**的包装类型（`OutletRender`）。
///
/// 与前两种的区别是**判据的位置**：判据不再在出口里、也不再是「先判后写」两条语句，
/// 而是**在管线内**执行一次，出口拿到的只能是过了闸的产物（类型层保证）。
///
/// ⇒ 判据（`check_site`）对这一族要问的不是「判据收到了什么」，
/// 而是「写出去的绑定是不是从管线产物派生的」「同一函数里还有没有判据直调」。
pub const WIRING_PIPELINE: &str = "pipeline";

/// `review show` 抬头前缀的**模板** —— 前缀原文的唯一事实源。
pub const REVIEW_SHOW_PREFIX_TEMPLATE: &str = "提案 {alias} 状态={status}\n";

/// `review show` 的抬头 —— 生产侧与端到端断言**共用**这一个函数。
pub fn review_show_prefix(alias: &str, status: &str) -> String {
    REVIEW_SHOW_PREFIX_TEMPLATE
        .replace("{alias}", alias)
        .replace("{status}", status)
}

/// **置信度分档表**（REQ-108 · AC-010 的「审核界面必须有」）。
///
/// 输入是提案条目行（`payload.confidence` 里带着分数），输出一段**纯文本**：
/// 抬头 + 四档计数 + 每条关系一行（档位、分数、关系、证据摘要、veto 理由）。
///
/// # 为什么单独一段、不并进 `review_view`
///
/// `src/domain/review_view.rs` 的渲染链是 REQ-056~078 的**字节契约**所在地，
/// 本 REQ 的红线明确禁改它。而分数是审核辅助信息、不是条目正文 ——
/// 单独一段既满足「审核界面看得见」，又不动那条被钉住的输出。
///
/// # 排序是确定的
///
/// 档位（高→拒绝）→ 分数降序 → 条目 target 升序。同一份提案两次运行
/// 输出逐字节相同（AC-005 的口径也适用于这一段）。
pub fn render_confidence_table(items: &[crate::db::proposal::ProposalItemRow]) -> String {
    use crate::domain::confidence::Band;

    #[derive(Clone)]
    struct Row {
        band: Band,
        score: f64,
        method: String,
        target: String,
        evidence: Option<(u32, u32)>,
        veto: Option<String>,
        /// REQ-109 · AI 语义判断的那一行（`fit` / `model` / `cache_hit` / `tokens`）。
        /// `None` = 这条关系**没被 AI 判断过**（AI 关 / 判失败），不打印 ——
        /// 「没判」不冒充「判了 0 分」。
        ai: Option<String>,
    }
    let mut rows: Vec<Row> = Vec::new();
    for it in items {
        let Some(v) = it.payload.get("confidence") else { continue };
        let Ok(c) = crate::domain::confidence::from_untrusted(v) else { continue };
        let ev = c
            .signals
            .iter()
            .find(|s| s.name == "sample_hit_rate")
            .and_then(|s| Some((s.sampled?, s.matched?)));
        // REQ-109 · 语义分与溯源一起打：只有分数没有溯源，审核人无从复核；
        // 只有溯源没有分数，看不出 AI 到底说了什么。
        let ai = c.ai.as_ref().map(|a| {
            let fit = c
                .signals
                .iter()
                .find(|s| s.name == crate::domain::confidence::SIGNAL_SEMANTIC_FIT)
                .map(|s| format!("{:.2}", s.value))
                .unwrap_or_else(|| "—".to_string());
            let why = a
                .rationale
                .as_deref()
                .map(|r| format!("「{r}」"))
                .unwrap_or_default();
            format!(
                "ai {} · fit {} · cache_hit {} · tokens {} {}",
                a.model, fit, a.cache_hit, a.tokens, why
            )
        });
        rows.push(Row {
            band: c.band,
            score: c.score,
            method: c.method.clone(),
            target: it.target_key.clone(),
            evidence: ev,
            veto: c.veto.as_ref().map(|v| format!("{}：{}", v.signal, v.reason)),
            ai,
        });
    }
    if rows.is_empty() {
        return "置信度：本提案没有分数（REQ-108 之前的提案 / 人工提案）".to_string();
    }
    rows.sort_by(|a, b| {
        a.band
            .cmp(&b.band)
            .then_with(|| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal))
            .then_with(|| a.target.cmp(&b.target))
    });
    let counts = crate::domain::confidence::BandCounts::of(rows.iter().map(|r| &r.band));
    // 抬头写**这一份提案实际用的方法**。AI 关时仍逐个字是 `evidence-scorecard-v1`
    // （REQ-108 的字节契约），AI 开时是 v2 —— 不写死，免得报告说假话。
    let mut methods: Vec<&str> = rows.iter().map(|r| r.method.as_str()).collect();
    methods.sort_unstable();
    methods.dedup();
    let method_label = methods.join(" / ");
    let mut out = String::new();
    out.push_str(&format!(
        "置信度（{}）：高 {} · 中 {} · 低 {} · 拒绝 {}（共 {} 条关系）\n",
        method_label,
        counts.high,
        counts.middle,
        counts.low,
        counts.rejected,
        counts.total()
    ));
    for r in &rows {
        let ev = match r.evidence {
            Some((s, m)) => format!(" · 证据 {m}/{s}"),
            None => " · 证据 无（data_evidence=none）".to_string(),
        };
        let veto = r.veto.as_deref().map(|v| format!("\n             veto {v}")).unwrap_or_default();
        let ai = r.ai.as_deref().map(|a| format!("\n             {a}")).unwrap_or_default();
        out.push_str(&format!(
            "  [{}] {:.2} {}{}{}{}\n",
            r.band.label_zh(),
            r.score,
            r.target,
            ev,
            veto,
            ai
        ));
    }
    out.trim_end().to_string()
}

/// ⭐ REQ-078 · **打印点登记表** —— 全部打印 domain 产物的站点。
///
/// ⛔ **新增 `print!` 站点必须登记**：漏登记即红（`G-2`），
/// 登记表与源码不一致即红（`G-5`）。
pub const DOMAIN_PRINT_SITES: &[DomainPrintSite] = &[
    DomainPrintSite {
        enclosing_fn: "review_cmd",
        command: "review show",
        wiring: WIRING_CLOSURE,
        binding: "text",
        prefix_template: REVIEW_SHOW_PREFIX_TEMPLATE,
        trailing_newline: false,
        judged_by: &["verify_track_separation", "verify_provenance_scope"],
        // ⚠️ REQ-057：这条判据读的是**结构化溯源**，不是渲染文本 ——
        // 把它挪进 `judged_by` 会逼它收 `text`，那正是 REQ-057 判为恒真的那种写法。
        projection_judged_by: &["audit_fde_channel"],
        e2e_covered: true,
        note: "REQ-072 的真进程字节比对 + REQ-078 的登记表前缀。",
    },
    DomainPrintSite {
        enclosing_fn: "ontology_cmd",
        command: "ontology show --model-view",
        wiring: WIRING_NONE,
        binding: "mv_json",
        prefix_template: "",
        trailing_newline: true,
        // 刻意不过判据：建模术语在这一轨**是要被裁决的内容**（`cli.rs` 的注释原话）。
        judged_by: &[],
        projection_judged_by: &[],
        e2e_covered: true,
        note: "REQ-078 起由真进程字节比对覆盖；豁免判据的理由写在 cli.rs。",
    },
    DomainPrintSite {
        enclosing_fn: "ontology_cmd",
        command: "ontology show --html",
        wiring: WIRING_PIPELINE,
        binding: "html",
        prefix_template: "",
        trailing_newline: false,
        // ⭐ REQ-107：判据在 `domain::pipeline` 里执行一次，出口只拿到过了闸的产物
        // （`OutletRender`，字段私有、无公开构造器）⇒ 出口层不再有判据调用。
        judged_by: &[],
        projection_judged_by: &[],
        e2e_covered: true,
        note: "REQ-078 起由真进程字节比对覆盖（stdout 分支；--out 分支写同一份字节）。\
         ⭐ REQ-107 起改为 `pipeline` 接线：判据在管线内、出口只写管线产物 —— \
         「判据收到的 == 打印出去的」由**类型**保证（`OutletRender` 造不出来），\
         不再靠「两条语句读同一个绑定」这条约定。独立第二道防线仍是真进程字节比对。",
    },
    DomainPrintSite {
        enclosing_fn: "ontology_cmd",
        // ⚠️ 命令名就叫这个、不叫 `--json`：`ontology show` **没有** `--json` 标志，
        //    不带 `--html` / `--model-view` 时默认路径就直接打 JSON。
        command: "ontology show（默认 JSON）",
        wiring: WIRING_PIPELINE,
        binding: "json",
        prefix_template: "",
        trailing_newline: true,
        judged_by: &[],
        projection_judged_by: &[],
        e2e_covered: true,
        note: "REQ-078 起由真进程字节比对覆盖。\
         ⭐ REQ-107 起改为 `pipeline` 接线：剔除（`filter_view`）与发布闸都在管线内，\
         出口拿到的只能是过了闸的产物 ⇒ 「先判后写」不再是一条需要人记得的时序，\
         而是一条**类型**（REQ-106 AC-001 的意图由结构承接）。\
         独立第二道防线仍是真进程字节比对。",
    },
    DomainPrintSite {
        enclosing_fn: "ontology_cmd",
        command: "overview",
        wiring: WIRING_PIPELINE,
        binding: "text",
        prefix_template: "",
        trailing_newline: false,
        judged_by: &[],
        projection_judged_by: &[],
        e2e_covered: true,
        note: "REQ-078 起由真进程字节比对覆盖。\
         ⭐ REQ-107 起改为 `pipeline` 接线（`Product::Overview`）：文本与 JSON 两条形态\
         **都**在管线内过闸（保留收口前的过度拦截，AC-005 原样不动），出口只写其中一条。",
    },
    DomainPrintSite {
        enclosing_fn: "ontology_cmd",
        command: "overview --json",
        wiring: WIRING_PIPELINE,
        binding: "json_text",
        prefix_template: "",
        trailing_newline: true,
        judged_by: &[],
        projection_judged_by: &[],
        e2e_covered: true,
        note: "REQ-078 起由真进程字节比对覆盖。\
         ⭐ REQ-107 起改为 `pipeline` 接线（`Product::Overview`）：与 `overview` 共用同一份\
         产物与同一道闸，只有写出分支不同。",
    },
    DomainPrintSite {
        enclosing_fn: "proposal_cmd",
        command: "proposal apply（未通过清单）",
        wiring: WIRING_NONE,
        binding: "b.target",
        prefix_template: "  被拦：",
        trailing_newline: true,
        judged_by: &[],
        projection_judged_by: &[],
        // ⛔ 如实登记为**未覆盖**：它逐行打 `summarize` 的阻塞项，
        // 无下游判据接线 ⇒ 真进程字节比对对它不构成防线。
        //
        // ⚠️ REQ-080 F-3：REQ-078 的 note 写「无载荷面」，**与代码不符** ——
        // `summarize` 的 `ItemVerdict.target` 是 `ItemView.target` 的克隆
        // （`src/domain/review.rs:218`），原样来自提案 payload，**未经
        // `escape_payload_text`** ⇒ payload 里带真换行就能在 stdout 上凭空
        // 多出一行 `  被拦：…`。**载荷面=有**（证据见 G-6 的运行时实测）。
        e2e_covered: false,
        note: "未覆盖（无真进程字节比对断言）：**有**文本载荷面 —— \
         `summarize` 的 `ItemVerdict.target` 来自提案 payload、未经 escape_payload_text；\
         且无判据接线。载荷面=有",
    },
    DomainPrintSite {
        enclosing_fn: "ontology_cmd",
        command: "ontology diff",
        wiring: WIRING_NONE,
        binding: "x.summary()",
        prefix_template: "  ",
        trailing_newline: true,
        judged_by: &[],
        projection_judged_by: &[],
        // ⛔ 如实登记为**未覆盖**：这一站逐条打 `Change::summary()`，
        // 无下游判据 ⇒ 真进程字节比对对本条**不构成防线**。
        //
        // ⚠️ REQ-080 F-3：REQ-078 的 note 写「无文本载荷面」，**与代码不符** ——
        // `DiffItem.target` 是快照里的实体/关系 key、`before`/`after` 是**原始
        // JSON 值**，而 `diff.rs` 的 `render()` 对 `Value::String` 是**直接
        // `clone()`**（`src/domain/diff.rs:74`）⇒ 一律**未经
        // `escape_payload_text`**，真换行能原样流进 stdout。
        // **载荷面=有**（证据见 G-6 的运行时实测）。
        //
        // 登记它是为了「漏登记即红」这条机制能覆盖到这里，而不是把它算进已覆盖。
        e2e_covered: false,
        note: "未覆盖（无真进程字节比对断言）：**有**文本载荷面 —— \
         `DiffItem.target`（快照 key）与 `before`/`after`（原始 JSON，字符串直接 clone）\
         均未经 escape_payload_text；且无判据接线。载荷面=有",
    },
];

// ───────────────────── 唯一的出口（AC-001）─────────────────────

/// 把**已过前处理与发布闸**的客户产物逐字节写进 stdout（`print!` 语义，**不加**尾换行）。
///
/// ⭐ REQ-107 第 2 轮（QA 阻断 B3 / A11 R-D）：
/// 实参类型是 [`OutletRender`] —— **拿裸 `&str` 调它编译不过**。
/// 于是「出口先把手拼的未过闸字节写出去、再去调管线判定」（REQ-106 的原始病）
/// 在出口层没有表达形式：写出去的字节只能来自管线的返回值。
///
/// ⛔ 刻意**不返回**任何派生值。
/// 建模轨（D2）的闭包式接线路过 `emit_domain_text_with(&str, …)`，见那里。
pub fn emit_domain_text(rendered: crate::domain::pipeline::OutletRender) {
    write_stdout(rendered.as_str(), false);
}

/// 同 [`emit_domain_text`]，但补一个尾换行（`println!` 语义）。
pub fn emit_domain_line(rendered: crate::domain::pipeline::OutletRender) {
    write_stdout(rendered.as_str(), true);
}

/// ⭐ REQ-078 · **判据收到的串 == 打印出去的串**，由**类型**保证。
///
/// 1. 把 `text` **逐字节**写进 stdout；
/// 2. 把**同一个切片**交给 `judge`。
///
/// # 为什么是闭包而不是「先打印、再把 `&text` 喂判据」
///
/// 旧写法（REQ-072 QA 的 M2 就是打在这儿）：
///
/// ```text
/// print!("{text}");                                  // 打印点
/// for v in verify_track_separation(&rv, &text) { … }  // 判据点：另一个 `&text`
/// ```
///
/// 两者是**两个独立取值点** —— 明天有人在打印点插一次 `str::replace`，
/// 判据点纹丝不动，**任何自检都看不见**（QA 实测：全仓 738 条全绿、
/// 退出码 0、stderr 全空）。
///
/// 这里的闭包**在签名上就拿不到别的串**：它只有 `text` 这一个 `&str` 可用。
/// 于是「判据看到了打印点之外的加工」这件事**表达不出来**。
/// （残余风险：闭包体里**引用外层同名绑定**而不是参数 —— 那一格由
/// `tests/test_binary_output_guard.rs` 的 **G-1** 源码守卫钉住。）
pub fn emit_domain_text_with<F, R>(text: &str, judge: F) -> R
where
    F: FnOnce(&str) -> R,
{
    write_stdout(text, false);
    judge(text)
}

/// 写出本体。**不静默吞失败**：写不出去要么是下游关管道（`main.rs` 负责退出 0），
/// 要么是实打实的 IO 失败 —— 后者按操作性错误 fail-closed。
///
/// ⚠️ 这里**刻意不**返回 `Result`：渲染产物已经算出来了，
/// 「写不出去」不是一个能被调用方优雅处理的分支，静默吞掉才是本仓库
/// 反复出现的那种病（F-3 同源）。
fn write_stdout(text: &str, newline: bool) {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    let r = out
        .write_all(text.as_bytes())
        .and_then(|()| if newline { out.write_all(b"\n") } else { Ok(()) })
        .and_then(|()| out.flush());
    match r {
        Ok(()) => {}
        // 下游关闭（`ontok ... | head`）：`main.rs` 末尾会 flush 失败并退出 0。
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {}
        Err(e) => {
            eprintln!("✗ 渲染产物没能写进 stdout：{e}");
            std::process::exit(EXIT_OPERATION_FAILED);
        }
    }
}

/// ⭐ REQ-107 · 客户类产物的**发布入口**（CLI 侧的唯一一处）。
///
/// # 为什么它不在 `ontology_cmd` 体内
///
/// 「拦不拦」是**判定**，在 `domain::pipeline` 里做；「命中后怎么办」是**政策**
/// （stderr 报词条 + 退出码 `EXIT_LEAKED_MODELING_TERMS`，REQ-078 AC-004）。
/// 两者分开，是为了让出口的函数体里**一条判红分支都没有** ——
/// REQ-107 的 AC-002 直接扫出口函数体（`EXIT_LEAKED_MODELING_TERMS` /
/// `std::process::exit(` / `if !leaked.is_empty()` 一律 0 命中）。
///
/// # 判据只**调用**、不复制
///
/// 本函数不含任何判据：它只把 `pipeline::render` 的 `Err` 映射成退出码。
/// 命中清单（词条本身）只进 stderr，⛔ 不进 stdout（REQ-106 AC-002）。
///
/// # ⭐ 第 2 轮：第一个参数是 [`OutletId`]，不是 spec
///
/// 出口只能交代「我是哪条出口」；轨道 / 格式 / 粒度由管线**查表**决定
/// （`pipeline::spec_of`）。于是「出口把轨道改成 `Internal` 从而关掉闸门」
/// （QA 变异 #4）在源码里**写不出来**：既没有 spec 值，也没有可写字段。
fn publish_customer(
    id: crate::domain::pipeline::OutletId,
    product: crate::domain::pipeline::Product<'_>,
) -> crate::domain::pipeline::OutletRender {
    match crate::domain::pipeline::render(id, product) {
        Ok(rendered) => rendered,
        Err(e) if e.is_leak() => {
            let hits = e.hits();
            eprintln!("⚠ 客户视图泄露了建模术语（出口 {}）：{:?}", e.outlet(), hits);
            std::process::exit(EXIT_LEAKED_MODELING_TERMS);
        }
        Err(e) => {
            // 渲染/序列化失败是 bug，不是数据问题 ⇒ 按操作性错误退出，不冒充「命中」。
            eprintln!("✗ 出口 {} 的产物生成失败：{}", e.outlet(), e.message());
            std::process::exit(EXIT_OPERATION_FAILED);
        }
    }
}

pub async fn run(cli: Cli) -> anyhow::Result<()> {
    install_broken_pipe_guard();

    let url = crate::config::require_env(crate::config::DATABASE_URL_ENV)
        .or_else(|_| crate::config::require_env(crate::config::TEST_DATABASE_URL_ENV))?;
    let pool: PgPool = crate::db::pool::connect(&url).await?;
    crate::db::pg::migrate(&pool).await?;

    match cli.cmd {
        Cmd::Project { cmd } => project_cmd(&pool, cmd).await,
        Cmd::Version { cmd } => version_cmd(&pool, cmd).await,
        Cmd::Propose { cmd } => propose_cmd(&pool, cmd).await,
        Cmd::Review { cmd } => review_cmd(&pool, cmd).await,
        Cmd::Proposal { cmd } => proposal_cmd(&pool, cmd).await,
        Cmd::Ontology { cmd } => ontology_cmd(&pool, cmd).await,
        Cmd::Concept { cmd } => concept_cmd(&pool, cmd).await,
        // REQ-108 · AC-011：分箱命中率（分数 vs 人工决定）
        Cmd::Confidence { cmd } => confidence_cmd(&pool, cmd).await,
        Cmd::McpServer { project, version } => {
            // 冻结闸门在 `mcp::prepare` 里：未冻结就 Err ⇒ 这里退出码非零。
            //
            // ⚠️ **诊断走 stderr**：stdout 是 MCP 的专属通道，
            // 规范 Transports 章禁止往 stdout 写任何非 MCP 消息。
            let args = crate::mcp::ServerArgs { project, version };
            let ctx = match crate::mcp::prepare(&pool, &args).await {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("✗ mcp-server 拒绝启动：{e}");
                    std::process::exit(2);
                }
            };
            eprintln!(
                "· mcp-server 就绪：项目 {} 第 {} 版（已冻结于 {}），{} 个工具",
                ctx.project,
                ctx.version,
                ctx.frozen_at,
                crate::mcp::tools::catalog().len()
            );
            crate::mcp::serve(ctx).await
        }
        Cmd::Serve { project, version, bind } => {
            // 绑地址先解析：给错格式要报人话，不要把 axum 的栈倒给用户。
            let addr: std::net::SocketAddr = bind.parse().with_context(|| {
                format!("监听地址看不懂：{bind:?}。要写成 127.0.0.1:8787 这样的样子。")
            })?;
            let args = crate::server::http::ServeArgs {
                project,
                version,
                bind: addr,
            };
            crate::server::http::serve(&pool, args).await
        }
    }
}

/// REQ-010 · 概念声明通道。
///
/// # 为什么必须过提案，不许「读文件直接生效」
///
/// 声明文件是**人写的**（过渡期由规则生成、将来由 HK Agent / 业务人员改）。
/// 直接生效意味着「客户看到的语义」绕过了双轨审核 —— 那正是 §2.2
/// 说的「Agent 是提议者不是决定者」被架空。所以这里只做一件事：
/// **把声明翻译成 `add_concept_group` 提案条目**，之后与实体类型条目
/// 走完全相同的 `review decide` → `proposal apply`。
/// REQ-108 · **校准命令**（AC-011）：分数 → 分箱 → 人工接受率。
///
/// # 数据从哪来
///
/// `proposal_items.confidence`（REAL）+ `biz_status`（人工决定）。两栏都在
/// 同一条提案条目上，所以不需要跨表拼「这条关系后来被接受了吗」——
/// 判定本来就是**条目级**的。
///
/// # 为什么人工决定取**业务轨**
///
/// 双轨的语义不同：业务轨判「这条关系对不对」，建模轨判「建得规不规范」。
/// 校准要的是**正确率**，所以只能取业务轨。取建模轨会把「关系对、但基数标错」
/// 的条目算成「没接受」，于是分数高低与口径无关地混在一起。
///
/// # 样本不足时**不给结论**
///
/// 设计概要 §六：任一分箱 < 5 条 ⇒ 标「样本不足」。空库同理。
/// ⛔ 绝不输出 `0%` 或 `100%` —— 那不是「准确率 0」或「准确率 100」，
/// 而是「没有样本」，两者的差别正是 AC-011 要守的。
async fn confidence_cmd(pool: &PgPool, cmd: ConfidenceCmd) -> anyhow::Result<()> {
    let ConfidenceCmd::Calibrate { project, version } = cmd;

    let projects = PgProjectStore::new(pool.clone());
    let p = projects
        .get_by_name(&project)
        .await?
        .ok_or_else(|| anyhow::anyhow!("项目不存在：{project}"))?;

    // 已决 = 业务轨有明确决定（`accepted` / `rejected`）。
    // `pending` 不算「未接受」—— 那是「还没判」，进分母会把接受率稀释成噪声。
    // 所以下面**只取** `biz_status IN ('accepted','rejected')`，
    // 另外单独查一个总数，把「待决」报出来。
    let mut sql = String::from(
        // `confidence` 列是 `REAL`（float4）；这里显式转 float8 ——
        // 评分卡用的是 f64，别在解码处留一次 f32 截断（0.15 这类值会漂）。
        "SELECT pi.confidence::float8, pi.biz_status
         FROM proposal_items pi
         JOIN proposals p ON p.id = pi.proposal_id
         WHERE p.project_id = $1
           AND pi.confidence IS NOT NULL",
    );
    if version.is_some() {
        // 只统计挂在某个基版本上的提案（`base_version_id` 对应的 seq）
        sql.push_str(
            " AND p.base_version_id IN (
                 SELECT id FROM ontology_versions WHERE project_id = $1 AND seq = $2)",
        );
    }
    let mut q = sqlx::query_as::<_, (f64, String)>(&sql).bind(p.id);
    if let Some(v) = version {
        q = q.bind(v);
    }
    let rows = q.fetch_all(pool).await?;

    let mut samples: Vec<(f64, bool)> = Vec::new();
    let mut pending = 0usize;
    for (score, status) in &rows {
        match status.as_str() {
            "accepted" => samples.push((*score, true)),
            "rejected" => samples.push((*score, false)),
            _ => pending += 1,
        }
    }

    // ⚠️ **整段只往 stdout 写一次**：REQ-107 的写点登记表（
    // `tests/test_req107_source_enumeration.rs` 的 `t1`）要求 cli.rs 里
    // **每一处** stdout 写点要么是出口、要么在白名单里。校准表是运维面，
    // 但它有十几行——一次拼好、一次写出，登记面就只剩一处。
    let mut report_lines = String::new();
    report_lines.push_str(&format!(
        "置信度校准 · project={project}{}\n",
        match version {
            Some(v) => format!(" · base_version=v{v}"),
            None => " · 全部提案".to_string(),
        }
    ));
    report_lines.push_str(&format!(
        "  方法        : {}\n", crate::domain::confidence::METHOD
    ));
    report_lines.push_str(&format!(
        "  计分条目    : {} 条（已决 {} · 待决 {} —— 待决**不进**分母）\n",
        rows.len(),
        samples.len(),
        pending
    ));

    let bins = crate::domain::confidence::calibration_bins(&samples);
    report_lines.push('\n');
    report_lines.push_str("  分箱          条数   人工接受   接受率     平均分\n");
    for b in &bins {
        let rate = match (b.acceptance_rate(), b.sufficient()) {
            (Some(r), true) => format!("{:>6.1}%", r * 100.0),
            // ⛔ 样本不足时**不印百分比**（0% / 100% 都是冒充结论）
            (Some(_), false) => "  样本不足".to_string(),
            (None, _) => "      —".to_string(),
        };
        let mean = if b.count == 0 { "—".to_string() } else { format!("{:.2}", b.mean_score) };
        report_lines.push_str(&format!(
            "  {:<12} {:>5}   {:>7}   {}   {:>6}\n",
            b.label(),
            b.count,
            b.accepted,
            rate,
            mean
        ));
    }
    report_lines.push('\n');
    if samples.is_empty() {
        report_lines.push_str(
            "  结论        : **样本不足**（没有任何已决的计分条目）—— 不输出命中率结论。\n\
             \x20             先跑一轮 propose import → review decide（业务轨），再回来校准。",
        );
    } else if let Some(ece) = crate::domain::confidence::expected_calibration_error(&bins) {
        report_lines.push_str(&format!(
            "  结论        : 样本充足（每箱 ≥ {} 条）· ECE = {:.2}\n",
            crate::domain::confidence::MIN_BIN_SAMPLES,
            ece
        ));
        report_lines.push_str(
            "  读法        : 接受率 < 平均分 = 分数偏高（该收紧）；> = 分数偏低（可以放宽），\n\
             \x20             但**调权重必须先有覆盖率**（设计概要 §九 G4：本采样只验存在性）",
        );
    } else {
        let thin: Vec<String> = bins
            .iter()
            .filter(|b| !b.sufficient())
            .map(|b| format!("{}（{} 条）", b.label(), b.count))
            .collect();
        report_lines.push_str(&format!(
            "  结论        : **样本不足** —— 以下分箱 < {} 条：{}。不输出命中率结论、不算 ECE。",
            crate::domain::confidence::MIN_BIN_SAMPLES,
            thin.join("、")
        ));
    }
    println!("{report_lines}");
    Ok(())
}

async fn concept_cmd(pool: &PgPool, cmd: ConceptCmd) -> anyhow::Result<()> {
    let ConceptCmd::Digest { file } = cmd else {
        let ConceptCmd::Import { file, project, base_version, round } = cmd else {
            unreachable!("ConceptCmd 只有两个变体")
        };
        let text = std::fs::read_to_string(&file)
            .map_err(|e| anyhow::anyhow!("读不到 {}：{e}", file.display()))?;
        let decl = crate::domain::concept::ConceptDeclaration::parse(&text)?;
        // **fail-closed**：指纹对不上 = 有人改了内容没升版本。
        // 不静默接受，也不「顺手帮他升一级」—— 那等于替他做版本决定。
        decl.verify()?;

        let projects = PgProjectStore::new(pool.clone());
        let proposals = PgProposalStore::new(pool.clone());
        let p = projects.get_or_create(&project).await?;
        let base = projects.version_by_seq(p.id, &p.name, base_version).await?;
        if base.frozen_at.is_some() {
            anyhow::bail!("base 版本 v{base_version} 已冻结，不能在其上提案");
        }

        // 抽取来源 = 声明的 `declared_by` + 版本。**不是**「我」——
        // 声明是谁做的就记谁（§8.4：版本漂移必须可见）。
        //
        // `source = Declared`（ADR-003 决策 (b) · REQ-022）：这条值是从一份
        // **外部声明文件**读进来的，系统**未**验证其真伪。
        let eb = ExtractedBy {
            agent: decl.declared_by.clone(),
            version: decl.version.clone(),
            round,
            failed_groups: vec![],
            source: Some(ProvenanceSource::Declared),
        };

        let mut items = Vec::new();
        for (i, it) in decl.to_items().iter().enumerate() {
            items.push(NewItem::from_json(it, i)?);
        }

        let row = proposals
            .create(
                p.id,
                NewProposal {
                    base_version_id: base.id,
                    round,
                    extracted_by: Some(eb.clone()),
                },
                &items,
            )
            .await?;
        let rows = proposals.items(row.id).await?;
        let alias = proposal_alias(pool, p.id, row.id).await;
        println!(
            "概念声明 v{} 已导入提案 {alias}（项目 {} · 基于 v{}）—— 共 {} 个概念组，待双轨审核",
            decl.version,
            p.name,
            base_version,
            rows.len()
        );
        println!("  聚类规则：{}", decl.rule);
        match decl.confirmed_by.as_deref() {
            Some(who) => println!("  业务确认：{who}"),
            None => println!("  ⚠ 业务尚未确认 —— 这是**过渡声明**，不是结论"),
        }
        for r in &rows {
            println!("  [{}] {} {}", r.seq, r.kind, r.target_key);
        }
        println!("  下一步：ontok review show --proposal {alias} --project {}", p.name);
        return Ok(());
    };

    let text = std::fs::read_to_string(&file)
        .map_err(|e| anyhow::anyhow!("读不到 {}：{e}", file.display()))?;
    let decl = crate::domain::concept::ConceptDeclaration::parse(&text)?;
    let d = crate::domain::concept::content_digest(&decl.groups, &decl.rule, &decl.declared_by);
    println!("内容指纹\t{d}");
    println!("当前 version\t{}", decl.version);
    println!(
        "契约\t{}",
        match decl.verify() {
            Ok(()) => "一致（可直接 import）".to_string(),
            Err(e) => format!("不一致 —— 改内容必须改版本：{e}"),
        }
    );
    Ok(())
}

async fn project_cmd(pool: &PgPool, cmd: ProjectCmd) -> anyhow::Result<()> {
    let s = PgProjectStore::new(pool.clone());
    match cmd {
        ProjectCmd::New { name } => {
            let p = s.get_or_create(&name).await?;
            println!("项目 {} id={}", p.name, p.id);
        }
        ProjectCmd::List => {
            let rows: Vec<(String, Uuid)> = sqlx::query_as("SELECT name, id FROM projects ORDER BY name")
                .fetch_all(pool)
                .await?;
            for (n, i) in rows {
                println!("{n}\t{i}");
            }
        }
    }
    Ok(())
}

async fn version_cmd(pool: &PgPool, cmd: VersionCmd) -> anyhow::Result<()> {
    let s = PgProjectStore::new(pool.clone());
    // 先取名字再 match —— 直接在 match 里 borrow 会让 cmd 被部分 move，
    // 后面再 `match cmd` 就用不了了。
    let pname = match &cmd {
        VersionCmd::New { project }
        | VersionCmd::List { project }
        | VersionCmd::Freeze { project, .. } => project.clone(),
    };
    let p = s
        .get_by_name(&pname)
        .await?
        .ok_or_else(|| anyhow::anyhow!("项目不存在：{pname}"))?;

    match cmd {
        VersionCmd::New { .. } => {
            let v = s.new_version(p.id).await?;
            println!("新版本 seq={} id={}", v.seq, v.id);
        }
        VersionCmd::List { .. } => {
            for v in s.list_versions(p.id).await? {
                println!(
                    "v{}\t{}\t{}",
                    v.seq,
                    v.id,
                    if v.frozen_at.is_some() { "已冻结" } else { "未冻结" }
                );
            }
        }
        VersionCmd::Freeze { version, .. } => {
            let v = s.version_by_seq(p.id, &p.name, version).await?;
            let f = s.freeze(v.id).await?;
            println!("已冻结 v{}", f.seq);
        }
    }
    Ok(())
}

/// AC-015 · 导入前的**顶层闸**：带 `importable: false` 的文件不是提案。
///
/// 为什么在 `cli` 里单独成一个函数而不是写在 `propose_cmd` 里：
/// 它是**所有**导入通道都该过的一道门（今天只有 `propose import` 消费这类文件），
/// 而「写在流程中间的一行 if」没法被单测钉住 —— 下一步有人重排 `propose_cmd`
/// 就可能把它挪没了。判据要能单独执行。
///
/// 语义：
/// - **缺省** ⇒ 可导入（REQ-108 之前的产物没有这个字段）；
/// - **恰好 `true`** ⇒ 可导入；
/// - 其它（`false` / `"false"` / `0` / `null`）⇒ **拒绝**（fail-closed：
///   一个说不清自己能不能导的文件，不该被猜成能导）。
pub fn ensure_importable(path: &std::path::Path, doc: &Value) -> anyhow::Result<()> {
    if let Some(v) = doc.get("importable") {
        if v.as_bool() != Some(true) {
            anyhow::bail!(
                "{} 顶层声明 importable = {} —— 这是被拒绝档，不能导入。\n\
                 拒绝档（<0.30）的关系只记 veto 理由、不进本体；可导入的是主提案 \
                 proposal.json（高/中档）与待查清单 proposal-pending.json（低档，\
                 产出时就写 importable: true）。",
                path.display(),
                v
            );
        }
    }
    Ok(())
}

async fn propose_cmd(pool: &PgPool, cmd: ProposeCmd) -> anyhow::Result<()> {
    let ProposeCmd::Import { file } = cmd;
    let text = std::fs::read_to_string(&file)
        .map_err(|e| anyhow::anyhow!("读不到 {}：{e}", file.display()))?;
    let doc: Value = serde_json::from_str(&text)?;

    // ★ AC-015：`importable: false` 必须是**硬闸**，不是文件里的约定。
    //
    // 背景（QA `qa-review.md` §7 N2）：`ontok-propose` 把「不提案」的关系写进
    // `proposal-rejected.json`，并在顶层标 `importable: false`，但 `propose import`
    // 完全不看这个字段 —— 原样导入 **EXIT=0**，10 条已知错误照样进提案表，
    // 再来一次 `review decide --accept-all` + `apply` 就进了本体。
    // **声明了却不管，比不声明更糟**：读文件的人会以为已经挡住了。
    ensure_importable(&file, &doc)?;

    let projects = PgProjectStore::new(pool.clone());
    let proposals = PgProposalStore::new(pool.clone());
    let _ = &projects;

    let pname = doc["project"].as_str().ok_or_else(|| anyhow::anyhow!("提案缺 project"))?;
    let p = projects.get_or_create(pname).await?;

    let base_seq = doc["base_version"].as_i64().unwrap_or(1) as i32;
    let base = projects.version_by_seq(p.id, &p.name, base_seq).await?;
    if base.frozen_at.is_some() {
        anyhow::bail!("base 版本 v{base_seq} 已冻结，不能在其上提案");
    }

    // ★ REQ-022 / ADR-003 (b)：**逐字段**取，并给溯源值**标注来源通道**。
    //
    // 为什么不再整块 `serde_json::from_value::<ExtractedBy>(v)`：
    // 提案侧的溯源块与快照侧**不是同一个类型**
    // （REQ-022 已把它改名 `extract::propose::ProposalProvenance`），
    // **字段名相同、类型不同**：
    //
    // | 字段 | 提案侧 | 快照侧 |
    // | --- | --- | --- |
    // | `source` | 勘查留痕**字符串**（`introspect … read_only=on`） | **来源通道枚举**（自产 / 外部声明）|
    // | `failed_groups` | 结构化**对象**数组（`{group_id, table, attempts, reason}`）| **字符串**数组 |
    //
    // 整块转换要么直接报错，要么（更糟）在某个字段类型碰巧兼容时
    // **把「勘查留痕」当成「来源通道」写进库** ——
    // 后者就是一次静默的语义错位，正是 ADR-003 要消灭的那一类。
    // 逐字段取 = 两个形状的区别**写在代码里**，不靠谁记得。
    let eb = doc.get("extracted_by").filter(|v| !v.is_null()).map(|v| -> anyhow::Result<ExtractedBy> {
        let agent = v["agent"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("提案的 extracted_by.agent 缺失或不是字符串"))?
            .to_string();
        let version = v["version"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("提案的 extracted_by.version 缺失或不是字符串（§8.4：必须是具体版本号）"))?
            .to_string();
        let round = v["round"]
            .as_i64()
            .ok_or_else(|| anyhow::anyhow!("提案的 extracted_by.round 缺失或不是整数"))?
            as i32;
        // failed_groups 两种形状都收：快照侧的字符串，或提案侧的结构化对象
        // （取 `group_id`）。收结构化对象是为了让**本工具自己产出的**提案
        // （`ontok-propose` 写出来的就是对象数组）能导得进来 ——
        // 之前这里会整块反序列化失败，于是「工具产的提案导不进去、
        // 只有手写的能导」，那是个没人明说的反向激励。
        // ⚠️ 收下不等于担保：ADR-003 D2 说得很清楚，这条通道的声明
        // **系统无法验证**，所以下面一律标 `Declared`。
        let mut failed_groups = Vec::new();
        for f in v["failed_groups"].as_array().into_iter().flatten() {
            let id = match f {
                serde_json::Value::String(s) => s.clone(),
                other => other["group_id"]
                    .as_str()
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "提案的 extracted_by.failed_groups 里有一项既不是字符串也没有 group_id：{other}"
                        )
                    })?
                    .to_string(),
            };
            failed_groups.push(id);
        }
        Ok(ExtractedBy {
            agent,
            version,
            round,
            failed_groups,
            // ★ ADR-003 (b)：这条值来自**外部 JSON 文件**，不是本工具跑出来的。
            source: Some(ProvenanceSource::Declared),
        })
    })
    .transpose()?;

    let mut items = Vec::new();
    for (i, it) in doc["items"].as_array().into_iter().flatten().enumerate() {
        items.push(NewItem::from_json(it, i)?);
    }

    let row = proposals
        .create(
            p.id,
            NewProposal { base_version_id: base.id, round: doc["round"].as_i64().unwrap_or(1) as i32, extracted_by: eb.clone() },
            &items,
        )
        .await?;

    // ── REQ-113 · 提案的输入记录（`inputs.exclusions`）落库 ─────────────
    //
    // 审核界面读的是**库里这一行**（`GET /api/review` 的 `exclusions` 块），
    // 而不是回头去解析一遍 `exclusions.json` —— 纪律 H：「哪些表被排除」
    // 只有一个实现，界面只负责显示那份结果。
    //
    // 逐条记录在提案旁边的 `proposal-exclusions.json`（提案里只留元信息，
    // 这样 AC-001 的「proposal.json 里 grep 被排除的表名 = 0」才成立）。
    // 两者由 **sha256** 绑定：[`crate::extract::exclusions::load_record`] 会先比 sha，
    // 对不上就拒绝入库 —— 「拿上一轮的记录配这一轮的提案」在页面上看不出来，
    // 只能在这种地方挡。
    if let Some(inputs) = doc.get("inputs").filter(|v| v.is_object()) {
        if inputs.get("exclusions").is_some() {
            let name = inputs["exclusions"]["record"]
                .as_str()
                .unwrap_or(crate::extract::exclusions::RECORD_FILE)
                .to_string();
            // 只允许**裸文件名**：提案是别人给的 JSON，路径不能由它决定
            // （`../../etc/passwd` 这种在评审时看不出异常，却是真的读别人的文件）。
            if name.is_empty() || name.contains('/') || name.contains('\\') || name == ".." {
                anyhow::bail!(
                    "提案的 `inputs.exclusions.record` = {name:?} 不是一个裸文件名。\n\
                     记录文件只允许放在**提案旁边**（与本文件同目录）——\
                     路径由外部 JSON 决定时，读到的可能是任意一个文件。"
                );
            }
            let dir = file.parent().map(std::path::Path::to_path_buf).unwrap_or_default();
            let record_path = dir.join(&name);
            let rec = crate::extract::exclusions::load_record(&record_path, inputs)?;
            println!(
                "  排除记录    : {} 张 · {} v{} · sha256 {}…（判的人 {}；签的人 {}）",
                rec.count(),
                rec.file,
                rec.version,
                &rec.sha256[..12.min(rec.sha256.len())],
                rec.declared_by,
                rec.reviewed_by.as_deref().unwrap_or("—")
            );
            proposals
                .set_inputs(row.id, &serde_json::json!({ "exclusions": rec }))
                .await?;
        } else {
            // 将来 `inputs` 里可能还有别的输入记录：原样留档，不丢。
            proposals.set_inputs(row.id, inputs).await?;
        }
    }

    let rows = proposals.items(row.id).await?;
    let alias = proposal_alias(pool, p.id, row.id).await;
    println!("提案 {alias} 已导入，共 {} 个条目：", rows.len());
    for r in &rows {
        println!("  [{}] {} {}", r.seq, r.kind, r.target_key);
    }
    if eb.is_none() {
        println!("  （本提案无 extracted_by = 人工修订）");
    } else {
        println!("  （抽取来源：{}）", eb.as_ref().map(|e| format!("{} {}", e.agent, e.version)).unwrap_or_default());
    }
    Ok(())
}

async fn review_cmd(pool: &PgPool, cmd: ReviewCmd) -> anyhow::Result<()> {
    let proposals = PgProposalStore::new(pool.clone());

    match cmd {
        ReviewCmd::Decide { proposal, item, track, accept, reject, project, accept_all } => {
            if accept_all {
                let p = resolve_proposal_arg(pool, &proposal, project.as_deref()).await?;
                let items = proposals.items(p).await?;
                if items.is_empty() {
                    anyhow::bail!("提案里没有任何条目");
                }
                for it in &items {
                    // 两轨都接受 —— 逐条敲 `--item N --track biz/model` 的等价物，
                    // 走的是同一条 `set_decision` 路径与同一套领域校验
                    for t in [Track::Biz, Track::Model] {
                        proposals.set_decision(it.id, t, Decision::Accepted).await?;
                    }
                }
                println!("提案 {proposal} 的 {} 个条目 × 2 轨 → accepted", items.len());
                return Ok(());
            }
            if accept == reject {
                anyhow::bail!("--accept 与 --reject 必须且只能给一个");
            }
            let item = item.context("请给 --item <序号>，或改用 --accept-all 批量接受")?;
            let d = if accept { Decision::Accepted } else { Decision::Rejected };
            let track = track.context("请给 --track <biz|model>，或改用 --accept-all 批量接受")?;
            let t = match track.as_str() {
                "biz" => Track::Biz,
                "model" => Track::Model,
                other => anyhow::bail!("--track 只能是 biz 或 model，收到 {other}"),
            };
            let p = resolve_proposal_arg(pool, &proposal, project.as_deref()).await?;
            // ⭐ REQ-111 · **不再**在这里自己查条目 + 写库：那条路径与审核界面的
            // `POST /api/decide` 必须**是同一个函数**（纪律 H）。`decide_item` 里
            // 收着「序号 → 条目 → 领域状态机 → 落库 + 审计」四步；本分支只负责
            // 解析别名与打印。页面签的和 CLI 签的因此在结构上不可能不一致。
            proposals
                .decide_item(p, item, t, d, crate::db::proposal::VIA_CLI)
                .await?;
            println!("item {} 的 {} 轨 → {}", item, track, d.as_str());
        }
        ReviewCmd::Serve { project, proposal, bind, token } => {
            // 绑地址先解析：给错格式要报人话，不要把 axum 的栈倒给用户。
            let addr: std::net::SocketAddr = bind.parse().with_context(|| {
                format!("监听地址看不懂：{bind:?}。要写成 127.0.0.1:8788 这样的样子。")
            })?;
            // 口令两种来源：显式 `--token` 优先，其次环境变量（与 `serve` 同一个变量）。
            // 两处都没有 ⇒ 拒绝启动，**没有开发模式旁路**。
            let token = match token {
                Some(t) => t,
                None => crate::config::require_env(crate::config::VIEW_TOKEN_ENV).map_err(|e| {
                    anyhow::anyhow!("{e}；也可以用 --token 显式给（至少 8 个字符）")
                })?,
            };
            let args = crate::review_ui::ReviewArgs { project, proposal, bind: addr, token };
            crate::review_ui::http::serve(pool, args).await?;
        }
        ReviewCmd::Show { proposal, project, confidence } => {
            let p = resolve_proposal_arg(pool, &proposal, project.as_deref()).await?;
            let row = proposals.get(p).await?.ok_or_else(|| anyhow::anyhow!("提案不存在"))?;
            let items = proposals.items(p).await?;
            let views = PgProposalStore::to_item_views(&items);
            let base = PgStore::new(pool.clone())
                .get_snapshot(row.base_version_id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("base 版本快照不存在"))?;
            let baseline: std::collections::BTreeSet<String> =
                base.entity_types.iter().map(|e| e.key.clone()).collect();
            let st = summarize(&views, &baseline);

            // REQ-056 · 建模轨投影（**FDE 通道**）。
            //
            // ⚠️ **刻意不过 `scan_modeling_terms`**：建模术语在这一轨
            // 是**要被裁决的内容**，不是泄露。客户视图的守门器只扫
            // 客户视图那条路径，不受这里影响（两者是独立输出类型）。
            //
            // ⚠️ REQ-057：这条豁免的前提判据**换过**。
            // 早先那版拿守门器扫渲染文本判「有没有建模术语」——
            // 而渲染器对每个条目无条件打 `key=`，`key` 又是建模术语
            // ⇒ `premise_holds` **恒为 true**，下面那句警告永远不会触发。
            //
            // 现在判据读**结构化溯源**：「有没有哪一栏建模字段
            // 既来自 payload 又非空」（[`audit_fde_channel`]）。
            // 与词表**无交集** ⇒ 往词表加词不会让自检变松
            // （而客户视图那一侧加词会变严 —— 两个方向不再共用一条路径）。
            // REQ-060：提案级 `extracted_by` 从**提案行**取（`ProposalItemRow`
            // 没有这一栏 —— 归属天生是提案级的）。
            //
            // ⚠️ 这里刻意用三参入口而不是两参的：两参会落成
            // `NotProvided`（渲染成「未提供」），而库里真的有这一栏 ——
            // 那是把「有」说成「没有」。
            let rv = crate::domain::review_view::project_review_track_with_provenance(
                &views,
                &st,
                row.extracted_by.as_ref(),
            );
            let text = crate::domain::review_view::render_review_text(&rv);
            let audit = crate::domain::review_view::audit_fde_channel(&rv);

            // 登记表里的前缀 —— 原文取自 `REVIEW_SHOW_PREFIX_TEMPLATE`，
            // 生产与端到端断言**共用**它（AC-002：⛔ 不许硬写在测试的 `format!` 里）。
            print!("{}", crate::cli::review_show_prefix(&proposal, &row.status));

            // ⚠️ REQ-078 · 打印与判据**同一份字节**。
            //
            // 旧接线是「`print!("{text}")` 在一处、三道判据各拿 `&text`」——
            // 两个独立取值点 ⇒ 在打印点插一次再加工，判据**结构性看不见**
            // （REQ-072 QA 的 M2 实测：全仓 738 条既有测试全绿、退出码 0、stderr 全空）。
            //
            // 现在判据在 [`emit_domain_text_with`] 的闭包里：闭包只拿得到
            // 刚写出去的那一个切片。判据侧与打印侧的**接线**由
            // `tests/test_binary_output_guard.rs` 的 G-1 源码守卫钉住。
            let violations = crate::cli::emit_domain_text_with(&text, |judged| {
                let mut violations: Vec<String> = Vec::new();
                if !audit.premise_holds {
                    eprintln!(
                        "⚠ FDE 通道自检：这份建模投影里没有任何来自 payload 的建模字段 —— 建模字段可能没打出来"
                    );
                    violations.push("FDE 通道自检：建模字段可能没打出来".to_string());
                }
                // 判据 ②：投影**有**数据、但渲染串轨/重复/漏打时由它报。
                for v in crate::domain::review_view::verify_track_separation(&rv, judged) {
                    eprintln!("⚠ 两轨分界自检：{v}");
                    violations.push(format!("两轨分界自检：{v}"));
                }
                // 判据 ③（REQ-060）：提案级归属的呈现 —— 缺行 / 重复打印 / 没表态时由它报。
                for v in crate::domain::review_view::verify_provenance_scope(&rv, judged) {
                    eprintln!("⚠ 提案级归属自检：{v}");
                    violations.push(format!("提案级归属自检：{v}"));
                }
                violations
            });

            // 退出码政策（AC-004）：**报了就不许当成功** —— 见 `EXIT_POLICY`。
            // 在 REQ-078 之前，这三行只是 `eprintln!`，退出码恒为 0。
            if let Some(code) = crate::cli::self_check_exit_code(&violations) {
                std::process::exit(code);
            }

            // ── REQ-108 · 置信度通道（AC-010 的「审核界面必须有」）─────────
            //
            // 为什么放在**渲染之后**、且不做进 `review_view`：
            // `src/domain/review_view.rs` 是 REQ-056~078 的原始字节契约所在
            // （那条渲染链有三道自检 + 真进程字节比对），**本 REQ 的红线禁改它**。
            // 分数是**审核辅助信息**，不是条目正文 —— 单独一段、显式开关，
            // 既让审核者看得到，又不动那条被钉住的输出。
            if confidence {
                let band_lines = render_confidence_table(&items);
                // ⚠️ 用 `println!` 而不是绕道 `write_stdout`：本仓的打印点守卫
                // （REQ-078 的 G-5）靠「认得出是 domain 产物就打红」来兜底，
                // 绕道等于让它**看不见**这个站点。这里老老实实用一个普通打印：
                // 它打的是一张分档表（不是客户产物绑定），守卫认出后**不登记**是正确的。
                println!("{band_lines}");
            }
        }
    }
    Ok(())
}

async fn proposal_cmd(pool: &PgPool, cmd: ProposalCmd) -> anyhow::Result<()> {
    let ProposalCmd::Apply { proposal, project } = cmd;
    let p = resolve_proposal_arg(pool, &proposal, project.as_deref()).await?;
    let applier = PgApplier::new(pool.clone());

    // apply 前先报一次状态，让「为什么没成功」在输出里看得见
    let proposals = PgProposalStore::new(pool.clone());
    let row = proposals.get(p).await?.ok_or_else(|| anyhow::anyhow!("提案不存在"))?;
    let items = proposals.items(p).await?;
    let views = PgProposalStore::to_item_views(&items);
    let base = PgStore::new(pool.clone()).get_snapshot(row.base_version_id).await?;
    if let Some(b) = &base {
        let baseline: std::collections::BTreeSet<String> =
            b.entity_types.iter().map(|e| e.key.clone()).collect();
        let st = summarize(&views, &baseline);
        if !st.passed {
            println!(
                "提案未通过：可应用 {} 条，被拦 {} 条，悬空关系 {} 条",
                st.applicable.len(),
                st.blocked.len(),
                st.dangling_relations.len()
            );
            for b in &st.blocked {
                println!("  被拦：{} —— {}", b.target, b.reason.as_deref().unwrap_or(""));
            }
        }
    }

    match applier.apply_proposal(p).await {
        Ok(v) => {
            let s = PgProjectStore::new(pool.clone());
            let pr = PgProposalStore::new(pool.clone());
            let pid = pr.get(p).await?.unwrap().project_id;
            let seq = s.latest_version(pid).await?.map(|v| v.seq).unwrap_or(0);
            println!("提案已应用 → v{seq}（id={v}）");
        }
        Err(e) => {
            println!("应用失败：{e}");
            std::process::exit(1);
        }
    }
    Ok(())
}

async fn ontology_cmd(pool: &PgPool, cmd: OntologyCmd) -> anyhow::Result<()> {
    let s = PgProjectStore::new(pool.clone());
    let store = PgStore::new(pool.clone());

    match cmd {
        OntologyCmd::Show { project, version, model_view, html, out, sampling } => {
            let p = s.get_by_name(&project).await?.ok_or_else(|| anyhow::anyhow!("项目不存在"))?;
            let v = s.version_by_seq(p.id, &p.name, version).await?;
            let snap = store
                .get_snapshot(v.id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("v{version} 没有快照"))?;

            if model_view {
                // REQ-055 · 建模轨投影。
                //
                // 此前这里打的是**原始快照转储**（`to_string_pretty(&snap)`）——
                // 那是**存储层形状**，不是可裁决的投影：FDE 拿到的是一份
                // 数据库快照的 dump，而不是「建模轨该看的那一份」。
                // 双轨状态机（`domain::review`）让 FDE 逐项裁决建模轨，
                // 而建模轨**没有对应的视图**可看 —— 缺的就是这一环。
                //
                // ⚠️ **刻意不过发布闸**：建模术语在这一轨是**要被裁决的内容**，
                // 不是泄露。客户视图的守门器只扫客户视图那条路径，不受这里影响
                // （两者是独立输出类型）。
                //
                // ⭐ REQ-107 · AC-007：这不是「因为它没调闸所以它是建模轨」——
                // 轨道由 `OutletSpec::D1` **显式声明**（`Track::Modeling`），
                // 是否过闸由**声明**决定。把同一份产物的 spec 换成 `Track::Customer`，
                // 它当场就会被闸拦住（`pipeline` 的单元测试与
                // `tests/test_req107_track_manifest.rs` 都钉住这一点）。
                let mv = crate::domain::model_view::project_model_track(&snap);
                let mv_json = serde_json::to_string_pretty(&mv)?;
                // ⚠️ 第 2 轮：`mv_json` 直接就是管线产物（`OutletRender`），
                // 不是「先取 &str 再交给写出」—— 写出函数只收这个类型。
                let mv_json = publish_customer(
                    crate::domain::pipeline::OutletId::D1,
                    crate::domain::pipeline::Product::Text(mv_json),
                );
                // REQ-078：登记表里的站点，走唯一的出口。
                crate::cli::emit_domain_line(mv_json);
                return Ok(());
            }

            // 客户视图
            let sv = snapshot_to_view(&snap);
            // 全路径调用：`project` 这个名字被 Show { project, .. } 的
            // 模式变量遮蔽了，写 `project(&sv)` 会变成拿 String 当函数调。
            let cv = crate::domain::view::project(&sv);

            // ── HTML 输出（判读用）──
            if html {
                // 抽样清单：省略时用判读手册 §3.1 的 13 个。
                // 读文件失败**不静默降级** —— 判读人拿到的页面若少了高亮，
                // 会误以为「这就是全部重点」，那是 fail-open。
                let sampling: Vec<String> = match &sampling {
                    Some(p) => {
                        let raw = std::fs::read_to_string(p)
                            .with_context(|| format!("读不到抽样清单 {p:?}"))?;
                        raw.lines()
                            .map(|l| l.trim().to_string())
                            .filter(|l| !l.is_empty())
                            .collect()
                    }
                    None => crate::domain::html_view::DEFAULT_SAMPLING
                        .iter()
                        .map(|s| s.to_string())
                        .collect(),
                };
                // ⭐ REQ-107 · 客户出口 C1/C2：前处理（页面级清洗）+ 发布闸 + 报告
                // **全在管线里**。出口这一步只剩「声明格式（Html）」，不再自己
                // 决定要不要剔、要不要拦、stderr 报什么形状。
                //
                // C1（stdout）与 C2（--out 文件）**共用同一份产物** `html` ——
                // 它们写的是同一批字节，不是两份各自渲染的东西。
                let html = publish_customer(
                    crate::domain::pipeline::OutletId::C1,
                    crate::domain::pipeline::Product::CustomerHtml {
                        view: &cv,
                        sampling: sampling.as_slice(),
                    },
                );
                // ⚠️ 第 2 轮：`html` 就是管线产物本身（`OutletRender`）；写文件那一支
                // 只借 `.as_str()`，stdout 那一支把产物**移进**写出函数 ——
                // 类型层保证「写出去的 == 过了闸的」，出口拿不到第二份字节。
                //
                // 回执里的对象数先取出来：⛔ 不要让回执的 `println!` 也提到 `html`
                // （REQ-078 的 G-1 会把「打印点对绑定接了方法调用」判红 —— 它是对的）。
                let dropped_objects = html.dropped_object_count();

                match &out {
                    Some(p) => {
                        std::fs::write(p, html.as_str())
                            .with_context(|| format!("写不出 {p:?}"))?;
                        println!(
                            "已写出客户视图页面：{}（{} 个对象 · 抽样高亮 {} 个 · 自包含单文件，可离线打开）",
                            p.display(),
                            cv.objects.len() - dropped_objects,
                            sampling.len()
                        );
                    }
                    // ⚠️ REQ-078：stdout 分支走登记表里那唯一的出口。
                    // 写的是管线产物本身（`OutletRender` 里的字节），⛔ 没有二次加工。
                    None => crate::cli::emit_domain_text(html),
                }
                return Ok(());
            }

            // ── REQ-107 · 客户出口 C3（JSON）──
            //
            // 收口前这里是「`filter_view` 剔除 → `to_string_pretty` → `scan` → 写出」
            // 四步各自在本函数里；现在**全部**在 `domain::pipeline` 里，顺序
            // （剔除 → 序列化 → 判定 → 才产出可写出的东西）由管线保证。
            //
            // ⚠️ 剔除发生在**守门器之前**（管线的固定顺序）：真·建模术语
            // （`主键` / `基数` / `1:N`）不是标识符形态、剔不掉，照旧到达闸门并被拒。
            let json = publish_customer(
                crate::domain::pipeline::OutletId::C3,
                crate::domain::pipeline::Product::CustomerJson(&cv),
            );

            // 走到这里 = 判定通过 ⇒ 才允许写出。stdout 上只会出现这一种格式。
            crate::cli::emit_domain_line(json);
        }
        OntologyCmd::Overview { project, version, json } => {
            let p = s.get_by_name(&project).await?.ok_or_else(|| anyhow::anyhow!("项目不存在"))?;
            let v = s.version_by_seq(p.id, &p.name, version).await?;
            let snap = store
                .get_snapshot(v.id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("v{version} 没有快照"))?;

            let sv = snapshot_to_view(&snap);
            // key → 展示名。**必须用 key 定位成员**（见 overview::build_overview 文档）
            let names: std::collections::BTreeMap<String, String> = sv
                .entity_types
                .iter()
                .map(|e| (e.key.clone(), e.display_name.clone()))
                .collect();
            let cv = crate::domain::view::project(&sv);
            let ov = crate::domain::overview::build_overview(&cv, &snap.concept_groups, &names);

            // ⭐ REQ-107 · 客户出口 C4/C5：两条形态**都**在管线内过闸
            // （保留收口前的「任一条命中就整份不发」—— 那是既有行为，AC-005 原样保留），
            // 出口只声明自己要哪一条形态。
            if json {
                let json_text = publish_customer(
                    crate::domain::pipeline::OutletId::C5,
                    crate::domain::pipeline::Product::Overview(&ov),
                );
                crate::cli::emit_domain_line(json_text);
            } else {
                let text = publish_customer(
                    crate::domain::pipeline::OutletId::C4,
                    crate::domain::pipeline::Product::Overview(&ov),
                );
                crate::cli::emit_domain_text(text);
            }
            if !ov.notes.is_empty() {
                eprintln!(
                    "⚠ 总览有 {} 处降级已写进输出（未静默略过），详见末尾说明",
                    ov.notes.len()
                );
            }
        }
        // ⭐ REQ-110 · TTL 导出。
        //
        // # 唯一的一条序列化路径（纪律 H）
        //
        // 两条分支（stdout / `--out`）都只消费 `publish_customer` 的返回值 ——
        // 序列化、前处理、发布闸**全在管线里**，本函数体里没有第二份 TTL 生成逻辑。
        // HTTP 的 `/api/ontology.ttl` 走的是**同一个** `Product::CustomerTurtle`，
        // 所以「页面下载的」与「CLI 导出的」是同一批字节（AC-007）。
        OntologyCmd::Export { project, version, model_view, out, base, format } => {
            // 格式是**显式**参数：认不出就报错，不静默回退成 ttl（fail-closed）。
            if format != "ttl" {
                anyhow::bail!(
                    "只支持 --format ttl（Turtle）。收到 {format:?} —— 本轮不做 RDF 导入，也没有别的导出格式。"
                );
            }
            let p = s.get_by_name(&project).await?.ok_or_else(|| anyhow::anyhow!("项目不存在"))?;
            let v = s.version_by_seq(p.id, &p.name, version).await?;
            let snap = store
                .get_snapshot(v.id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("v{version} 没有快照"))?;

            // 动作层条数：**只**用于文件头的有损声明（内容一个字节都不导出）。
            let action_type_count = snap.action_types.len();
            let opts = crate::domain::ttl::TtlOptions {
                project: project.as_str(),
                version,
                base: base.as_str(),
                action_type_count,
            };

            if model_view {
                // 建模视图：`Track::Modeling` ⇒ 显式豁免发布闸（`spec_of(D9)` 查表）。
                let model = crate::domain::model_view::project_model_track(&snap);
                let model_ttl = publish_customer(
                    crate::domain::pipeline::OutletId::D9,
                    crate::domain::pipeline::Product::ModelTurtle { view: &model, opts },
                );
                match &out {
                    Some(p) => {
                        std::fs::write(p, model_ttl.as_str())
                            .with_context(|| format!("写不出 {p:?}"))?;
                        // 回执走 stderr：`--out` 的 stdout 必须留空，脚本才好接。
                        eprintln!(
                            "· 已写出建模视图 TTL：{}（{} 类 / {} 关系 · 有损项见文件头）",
                            p.display(),
                            model.entity_types.len(),
                            model.link_types.len()
                        );
                    }
                    None => crate::cli::emit_domain_text(model_ttl),
                }
                return Ok(());
            }

            // 客户视图（默认）：与页面看到的是**同一份**客户视图。
            let sv = snapshot_to_view(&snap);
            let cv = crate::domain::view::project(&sv);
            let customer_ttl = publish_customer(
                crate::domain::pipeline::OutletId::C6,
                crate::domain::pipeline::Product::CustomerTurtle { view: &cv, opts },
            );
            match &out {
                Some(p) => {
                    std::fs::write(p, customer_ttl.as_str())
                        .with_context(|| format!("写不出 {p:?}"))?;
                    eprintln!(
                        "· 已写出客户视图 TTL：{}（{} 个对象 / {} 条关系 · 有损项见文件头）",
                        p.display(),
                        cv.objects.len(),
                        cv.relations.len()
                    );
                }
                None => crate::cli::emit_domain_text(customer_ttl),
            }
        }
        OntologyCmd::Diff { project, a, b } => {
            let p = s.get_by_name(&project).await?.ok_or_else(|| anyhow::anyhow!("项目不存在"))?;
            let va = s.version_by_seq(p.id, &p.name, a).await?;
            let vb = s.version_by_seq(p.id, &p.name, b).await?;
            let sa = store.get_snapshot(va.id).await?.ok_or_else(|| anyhow::anyhow!("v{a} 无快照"))?;
            let sb = store.get_snapshot(vb.id).await?.ok_or_else(|| anyhow::anyhow!("v{b} 无快照"))?;
            let d = diff_snapshots(&sa, &sb);
            if d.is_empty() {
                println!("v{a} 与 v{b} 无差异");
            } else {
                println!("v{a} → v{b} 共 {} 处变化：", d.len());
                for x in &d {
                    println!("  {}", x.summary());
                }
            }
        }
    }
    Ok(())
}

// ── 提案指法 ────────────────────────────────────────────

/// `p1` / `p2` … → **该项目内按创建时间倒序**的第 n 个提案（`p1` = 该项目里最新的那个）；
/// 也可直接给 UUID（UUID 全局唯一，不需要作用域）。
///
/// # 别名是「项目内序号」—— 必须带 `WHERE project_id = $1`
///
/// 早期版本这里是**全库**排序。铁证是 `proposal_alias` 曾有个被标成未使用的
/// `_project_id` 参数 —— 项目 id 早就传进来了，却没写进 SQL。
/// 后果不是「解析到老数据」而是更坏的：
/// 共享测试库里别的项目一有新提案，`p1` 就指向**别的项目**的提案 ——
/// 审核动作会写进别人的提案，而且照样报「成功」，
/// 只有 `review show` 里那行的条目对不上时才露出不对。
///
/// # 为什么是「倒序」而不是正序
///
/// 正序（`p1` = 最早）在本项目会踩坑：同一项目里躺着几百条历史提案，
/// `p1` 永远指向第一条老数据，AC-012 的演示命令就会作用在**老数据**上
/// —— 而且照样「成功」，只有 `review show` 里那行 `[1] a` 才露出不对。
/// 倒序让 `p1` 恒等于「刚导入的那个」，与历史无关。
///
/// # 两处 SQL 必须同作用域同排序
///
/// 与 `proposal_alias` 完全一致 —— 否则打印的 `p1`
/// 和下次 `--proposal p1` 解析到的不是同一个提案。
///
/// ⚠️ REQ-111 起是 `pub`：审核界面（`src/review_ui`）启动时用**同一个**
/// 别名解析器，不抄第二份「p1 是哪个」的 SQL。抄一份的后果是页面显示 `p1`、
/// CLI 的 `p1` 却指另一个提案 —— 那种不一致只有人肉对账才发现。
pub async fn resolve_proposal(pool: &PgPool, project_id: Uuid, spec: &str) -> anyhow::Result<Uuid> {
    if let Ok(u) = Uuid::parse_str(spec) {
        return Ok(u);
    }
    if let Some(n) = spec.strip_prefix('p').and_then(|x| x.parse::<i64>().ok()) {
        let rows: Vec<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM proposals WHERE project_id = $1 ORDER BY created_at DESC, id DESC",
        )
        .bind(project_id)
        .fetch_all(pool)
        .await?;
        return rows
            .get((n - 1) as usize)
            .map(|r| r.0)
            .ok_or_else(|| anyhow::anyhow!("别名 {spec} 越界：该项目内只有 {} 个提案", rows.len()));
    }
    anyhow::bail!("--proposal 认不出：{spec}（用 UUID 或 p1/p2…）")
}

/// 库里最近导入的提案所属项目 —— `--project` 省略时的作用域。
///
/// AC-012 的演示流程是 `propose import` 紧接着 `review decide --proposal p1`，
/// 省略 `--project` 时别名必须仍能解析到刚导入的那个，所以回落到这里。
/// **要审核历史提案请显式给 `--project`**：省略时作用域由「最新的提案」决定，
/// 它属于哪个项目不由你指定。
async fn newest_proposal_project(pool: &PgPool) -> anyhow::Result<Option<Uuid>> {
    let row: Option<(Uuid,)> =
        sqlx::query_as("SELECT project_id FROM proposals ORDER BY created_at DESC, id DESC LIMIT 1")
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|r| r.0))
}

/// `--proposal` 的值 → 提案 id（三种指法的统一入口）。
///
/// 分两步：先判指法（UUID 直接放行，**不需要作用域**），
/// 别名才去定作用域项目，然后交给 `resolve_proposal`。
async fn resolve_proposal_arg(
    pool: &PgPool,
    spec: &str,
    project: Option<&str>,
) -> anyhow::Result<Uuid> {
    if let Ok(u) = Uuid::parse_str(spec) {
        return Ok(u);
    }
    let pid = match project {
        Some(name) => PgProjectStore::new(pool.clone())
            .get_by_name(name)
            .await?
            .map(|p| p.id)
            .ok_or_else(|| anyhow::anyhow!("项目不存在：{name}"))?,
        None => newest_proposal_project(pool).await?.ok_or_else(|| {
            anyhow::anyhow!(
                "别名 {spec} 需要项目作用域，但库里还没有任何提案 —— \
                 请改用 --proposal <uuid>，或加 --project <项目名>"
            )
        })?,
    };
    resolve_proposal(pool, pid, spec).await
}

/// 给刚导入的提案算别名，用于打印。
///
/// **必须与 `resolve_proposal` 用同一套作用域与排序** —— 否则打印的 `p1`
/// 和下次 `--proposal p1` 解析到的不是同一个提案。
///
/// ⚠️ REQ-111 起是 `pub`：审核界面用它把「绑的是哪个 pN」打在启动横幅上，
/// 与 CLI 的 `review show` 抬头同源。
pub async fn proposal_alias(pool: &PgPool, project_id: Uuid, id: Uuid) -> String {
    let rows: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM proposals WHERE project_id = $1 ORDER BY created_at DESC, id DESC",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    match rows.iter().position(|r| r.0 == id) {
        Some(i) => format!("p{}", i + 1),
        None => id.to_string(),
    }
}

/// 内部快照 → 投影输入。
///
/// `ontology show` / `ontology overview` / **REQ-027 的 `mcp-server`** 共用
/// —— 抄一份就会出现「页面上的对象」和「总览里的对象」对不上，
/// 而那正是判读人会当场发现、并且当场不信我们的问题。
/// MCP 侧更不能抄：抄一份就意味着 MCP 看到的对象可能与客户看到的页面不是同一批。
///
/// ⚠️ REQ-078 起是 `pub`（原 `pub(crate)`）：端到端断言要**在测试进程里复现**
/// 这条投影，才能把真实 stdout 与「前缀 ++ domain 渲染」逐字节比。
/// 它是**纯函数**（只读 `snap`，不碰 I/O），公开它不扩大任何写路径。
pub fn snapshot_to_view(
    snap: &crate::domain::snapshot::Snapshot,
) -> SnapshotView {
    SnapshotView {
        entity_types: snap
            .entity_types
            .iter()
            .map(|e| crate::domain::view::EntityTypeView {
                key: e.key.clone(),
                display_name: e.display_name.clone(),
                description: e.description.clone(),
                aliases: e.aliases.clone(),
                parent: e.parent.clone(),
                disjoint_with: e.disjoint_with.clone(),
                properties: e
                    .properties
                    .iter()
                    .map(|pr| crate::domain::view::PropertyTypeView {
                        name: pr.name.clone(),
                        column: pr.column.clone(),
                        nullable: pr.nullable,
                        note: pr.note.clone(),
                        sample: pr.sample.clone(),
                    })
                    .collect(),
                evidence: e.evidence.iter().map(|ev| serde_json::to_value(ev).unwrap()).collect(),
            })
            .collect(),
        link_types: snap
            .link_types
            .iter()
            .map(|l| crate::domain::view::LinkTypeView {
                key: l.key.clone(),
                from_type: l.from_type.clone(),
                to_type: l.to_type.clone(),
                cardinality: l.cardinality.clone(),
                semantic: l.semantic.clone(),
            })
            .collect(),
    }
}

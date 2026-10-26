//! `ontok-builder` —— **构建面** CLI（REQ-152）。
//!
//! 这个二进制是「FDE 本机那一半」：
//!
//! | 子命令 | 干什么 | 来源 |
//! | --- | --- | --- |
//! | `introspect` | 源库只读勘查 → `schema.json` | 原 `ontok-introspect` |
//! | `propose` | `schema.json` → 本体提案 | 原 `ontok-propose` |
//! | `translate-names` | 表名 → 中文名翻译表 | 原 `ontok-translate-names` |
//! | `gate-preflight` | 门禁输入前置检查 | 原 `ontok-gate-preflight` |
//! | 其余（`project` / `version` / `import` / `review` / `proposal` / `ontology` / |
//! `r2rml` / `sparql` / `console` / `doc` / `name` / `exclude` / `access`） | 构建流水线 | 原 `ontok` 的构建子命令 |
//!
//! # 为什么不保留老命令名（`ontok-propose` …）
//!
//! 用户拍板：**不保留别名**（REQ-152 §六）。旧名字是四个独立 bin，正是「客户镜像里
//! 塞着 FDE 本机工具」的来源；保留别名等于把那条边界留成一条软线。
//!
//! # 为什么四个工具的参数解析**没有**改写成 clap 的字段
//!
//! 它们的解析器是**手写**的，而且带着**退出码语义**（例如 `translate-names` 的
//! `0/1/2/3` 四码互不重载、`gate-preflight` 的失败面），还有一批**钉住源码文本**
//! 的判据（`include_str!("ontok-gate-preflight.rs")` 读回自己的源码做结构断言）。
//! REQ-152 的要求是「业务逻辑零改动，只改 CLI 外壳」：
//!
//! - **外壳**是 clap（下面这四个子命令出现在 `ontok-builder --help` 里）；
//! - **参数解析仍只有一份**（各工具自己的 `main`，我们把 argv 原样递进去），
//!   所以不存在「clap 一份 + 手写一份」的漂移面；
//! - `--help` 原样打印工具自己的帮助（`disable_help_flag` 让 clap 不抢），
//!   信息一个字节都没少。
//!
//! ⚠️ 代价（如实登记）：工具的帮助正文里仍写着 `cargo run --bin ontok-…` 的老用法。
//! 那几行**不能改**：`gate-preflight` 的 `help_text()` 被「文案里的 flag 必须真被
//! 解析器收下」的判据逐词扫描，改成 `cargo run --bin ontok-builder` 会多出一个
//! `-builder` 词元，判据当场变红（见 `tests/test_req107_source_enumeration.rs` 与
//! `src/bin/ontok-gate-preflight.rs` 的 AC-004 测试）。新用法写在这里的 doc 里。

use clap::{Parser, Subcommand};

// ⚠️ 四个工具的**源文件路径不变**（`src/bin/ontok-*.rs`），只是不再各自成 bin：
// - `gate-preflight` 的单元测试用 `include_str!("ontok-gate-preflight.rs")` 读回自己，
//   换目录会让那批结构判据找不到文件；
// - `tests/test_agent_orchestration.rs` / `tests/test_retained_names_req084.rs` 也用
//   `include_str!("../src/bin/ontok-propose.rs")` 这类**精确路径**钉着源码文本。
// 所以这里用 `#[path]` 把它们挂成模块，`Cargo.toml` 用 `autobins = false` 让它们
// **不再各自产出二进制**。
#[path = "ontok-introspect.rs"]
mod introspect;
#[path = "ontok-propose.rs"]
mod propose;
#[path = "ontok-translate-names.rs"]
mod translate_names;
#[path = "ontok-gate-preflight.rs"]
mod gate_preflight;

#[derive(Parser)]
#[command(
    name = "ontok-builder",
    about = "FDE 本体论平台 · 构建面（勘查 / 提案 / 审核 / 导入导出 / access 验收）"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 构建流水线（原 `ontok` 的构建子命令；服务面只剩 `query serve` / `mcp-server` / `serve`）
    ///
    /// 这些子命令的**参数、帮助与退出码逐字未变** —— 它们只是从 `ontok` 挪到了
    /// `ontok-builder`（REQ-152 ⑤：两个二进制，服务面 / 构建面）。
    #[command(flatten)]
    Build(ontok::cli::Cmd),

    /// 源库只读勘查 → `schema.json`（原 `ontok-introspect`）
    ///
    /// 用法：ontok-builder introspect --out out/schema.json
    ///
    /// 环境变量：ONTOK_SOURCE_DATABASE_URL（源库连接串，口令不入库）。
    /// 连接串会被强制加上 `default_transaction_read_only=on`，且勘查前校验服务端
    /// 确实是只读（不是就直接报错退出）。
    ///
    /// `--help` 打印原工具的完整帮助；参数与原命令**逐字相同**。
    #[command(disable_help_flag = true)]
    Introspect {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0..)]
        args: Vec<String>,
    },

    /// `schema.json` → 本体提案（原 `ontok-propose`；默认路径不连库）
    ///
    /// 用法：ontok-builder propose --schema out/schema.json --names out/table_names.json \
    ///　　　　--out out/proposal.json --stats out/extract-stats.json
    ///
    /// ⚠️ `--names` **必填**（没有默认值）。
    /// `--help` 打印原工具的完整帮助；参数与退出码与原命令**逐字相同**。
    #[command(disable_help_flag = true)]
    Propose {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0..)]
        args: Vec<String>,
    },

    /// 表名 → 中文名翻译表生成器（原 `ontok-translate-names`）
    ///
    /// 用法：ontok-builder translate-names --schema out/schema.json --out out/table_names.json
    ///
    /// ⚠️ 退出码四码互不重载（`0` 成功 / `1` 改命令 / `2` 数据不全没写产物 /
    /// `3` 产物已写但要人过目），与原命令**完全相同**。
    /// `--help` 打印原工具的完整帮助。
    #[command(disable_help_flag = true)]
    TranslateNames {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0..)]
        args: Vec<String>,
    },

    /// 门禁输入前置检查（原 `ontok-gate-preflight`）
    ///
    /// 用法：ontok-builder gate-preflight [--verify-only] [--out <path>] [--baseline <path>]
    ///
    /// 环境变量：ONTOK_SOURCE_DATABASE_URL（源库连接串）。
    /// `--help` 打印原工具的完整帮助；参数与退出码与原命令**逐字相同**。
    #[command(disable_help_flag = true)]
    GatePreflight {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0..)]
        args: Vec<String>,
    },
}

/// 把「工具自己的 `main`」的失败按 **`fn main() -> Result<()>` 的原样**收尾。
///
/// 那四个工具原来是独立 bin，`main` 返回 `Result` ⇒ Rust 的 `Termination` 会打
/// `Error: <Debug>` 并以 **1** 退出。这里逐字复刻，⛔ 不换成 `✗ {e}`：
/// 失败信息与退出码都属于「CLI 外壳」，不该因为合并而变。
fn finish(r: anyhow::Result<()>) {
    if let Err(e) = r {
        eprintln!("Error: {e:?}");
        std::process::exit(1);
    }
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    match cli.cmd {
        // 与原来的 `ontok` 主入口逐字相同（`cli::run` 自己装 EPIPE hook）。
        Cmd::Build(cmd) => {
            if let Err(e) = ontok::cli::run(ontok::cli::Cli { cmd }).await {
                eprintln!("✗ {e}");
                std::process::exit(1);
            }
            if std::io::stdout().flush().is_err() {
                std::process::exit(0);
            }
        }
        Cmd::Introspect { args } => finish(introspect::main(args).await),
        Cmd::Propose { args } => finish(propose::main(args).await),
        Cmd::TranslateNames { args } => translate_names::main(args),
        Cmd::GatePreflight { args } => finish(gate_preflight::main(args).await),
    }
}

use std::io::Write;

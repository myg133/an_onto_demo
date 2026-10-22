//! 门禁输入前置检查（REQ-048）
//!
//! **为什么有这个命令**：`tests/drop_rate.rs` 与 `tests/test_fk_naming.rs` 都是
//! **fail-closed** 的回归门禁 —— 读不到脱敏 `schema.json` 就判失败，而不是安静跳过。
//! 这道纪律是对的，但它把「输入没了」这件事推迟到**跑测试时**才暴露，
//! 而那个输入**不在版本控制内**（`.review-workspace/` 在仓库外、被 git 忽略），
//! 清理临时目录就会没。
//!
//! 本命令把这件事**提前**：跑全量测试**之前**先跑它，一次搞定三件事 ——
//! 从源库现拉、落到门禁实际会去读的位置、校验产物像个真 schema。
//!
//! # 用法
//!
//! ```bash
//! export ONTOK_SOURCE_DATABASE_URL="postgresql://…"   # 口令只走环境变量
//! cargo run --bin ontok-gate-preflight                # 现拉 + 落盘 + 校验
//! cargo run --bin ontok-gate-preflight -- --verify-only   # 只校验已有产物（不连库）
//! ```
//!
//! # 门禁靠什么找到产物
//!
//! 两个门禁的查找顺序一致：**先 `ONTOK_SCHEMA_JSON`，再各自的缺省路径**。
//! 缺省路径写成了两种字面量，但解析后指向同一个文件：
//!
//! | 门禁 | 缺省字面量 | 解析后 |
//! | --- | --- | --- |
//! | `tests/drop_rate.rs:29` | `../.review-workspace/fix-props/schema.json` | 相对 crate root（`cargo test` 的 CWD） |
//! | `tests/test_fk_naming.rs:312` | `/home/…/Ontology/.review-workspace/fix-props/schema.json` | 绝对路径 |
//!
//! 所以本命令**默认写到那个绝对路径**：一个位置同时喂饱两个门禁，不用设任何环境变量。
//! 若当前工作目录让两种字面量**解析到不同文件**（例如在本仓之外运行），
//! 本命令**拒绝猜**，直接报错要求设 `ONTOK_SCHEMA_JSON` —— 否则会出现
//! 「我拉好了，门禁还是读不到」这种最难查的假象。
//!
//! # ⚠️ 全量 `cargo test` 的运行前提：**实测样本**（REQ-066 · AC-002）
//!
//! 本文件的单元测试里有两条要读**实测样本**，它们是**客户源库的脱敏 schema**：
//!
//! | 角色 | 缺省路径（**本机**） | 覆盖口（环境变量） |
//! | --- | --- | --- |
//! | 登记基线 | `<仓库根>/.review-workspace/fix-props/schema.json` | [`SCHEMA_JSON_ENV`] |
//! | 现拉产物 | `<仓库根>/.review-workspace/req062d/live-pulled.json` | `ONTOK_LIVE_SCHEMA_JSON` |
//!
//! **怎么准备**：登记基线那份由**人**建立 —— 跑一次现拉（`cargo run --bin ontok-gate-preflight`），
//! 人工复核后显式放到该路径；现拉那份放进 `.review-workspace/req066d/` 之类的工作目录即可。
//!
//! ⛔ **这两份文件不在版本控制内**（`.gitignore` 的 `/*` 覆盖 `.review-workspace/`），
//! **源库信息不得入库** —— 它们是客户源库的 schema。
//!
//! **缺了会怎样**：下面三条**故意**红（fail-closed，**不是回归**）：
//! `tests/drop_rate.rs` / `tests/test_fk_naming.rs`（两个门禁读不到输入）
//! 与 `real_pair_five_missing_columns_are_named`。
//! 别的机器 / CI 上**不需要**有权限拿样本：把上面两个环境变量指向一份
//! **已授权的脱敏副本**即可（覆盖口照 `tests/drop_rate.rs:68` 的既有约定，
//! 缺省值一个字都没改 ⇒ 本机零影响）。
//!
//! **承重的判据不依赖任何文件**：[`synthetic_pair_reproduces_the_real_drift_claims`]
//! 在**任何**环境下都真跑，逐条对应上面那一条的结论。
//!
//! # REQ-062：登记基线与现拉产物**必须是两个文件**
//!
//! 2026-10-10 实测确认的旧行为：`--out` 的缺省值就是登记基线那个文件，
//! 于是「现拉 → 校验」之后 `write_atomic` 把它**覆盖写回**——
//! 门禁拿一份**待审对象**当**登记事实**写了下去。
//! 落盘前唯一的拦截是体量区间（80–400 表 / 900–3000 列），
//! 实测的 96 表 / 1111 列落在区间正中 ⇒ **必然放行并覆盖**，
//! 而那份文件**不在版本控制内**，没有 git 兜底。
//!
//! 所以现在有**两个互不相同的角色**：
//!
//! | 角色 | 参数 | 缺省 | 本命令能对它做什么 |
//! | --- | --- | --- | --- |
//! | **登记基线**（登记事实） | `--baseline` | 两个门禁实际会读的那份 | **只读** |
//! | **现拉落盘目标**（待审对象） | `--out` | `.review-workspace/gate-preflight/incoming-schema.json` | 写 |
//!
//! 两者解析到同一文件 ⇒ **报错退出**，理由是 **路径**，**不是内容**
//! （内容一样也报错：危害在「这次运行会去写登记事实」，与内容无关）。
//!
//! 门禁读的那份**不再由本命令产出**。想让门禁看到新的 schema，
//! 必须由**人**复核后显式覆盖登记基线 —— 本命令不自动同步基线，
//! 「有差异就更新基线」正是让判据恒真的做法。
//!
//! # REQ-062：对账（现拉 vs 登记基线）
//!
//! 体量区间对「少 5 列」**完全不敏感**，所以区间判红 ≠ 对过账。
//! 另有一路逐项对账，四个形态分开点名（详见 [`reconcile`]）：
//! 缺表 / 缺列 / 列类型变了 **阻断**，注释没了 **降级为 warn**；
//! 反方向（现拉多出表、列、注释）**只 warn，绝不阻断** ——
//! 源库合法新增（备份表、同步簿记表）不该被当成事故。
//!
//! `server_version` **只比主版本**：`16.9 → 16.14` 是补丁更新，不该阻断。
//!
//! # REQ-063：角色写进**类型**，参数对调**编译不过**
//!
//! 原来 `fn reconcile(base: &SchemaReport, subj: &SchemaReport)` **两个参数同类型** ——
//! 类型系统天然区分不了谁是谁，于是「把两个实参对调」是一个**合法可编译**的程序。
//! 现在 `Baseline` / `Subject` 是两个**不同的 newtype**（不是 type alias），
//! 对调直接编译失败：这类变异从「被监视」升级成「**不可构造**」。
//!
//! **路径也必须带角色**（`BaselinePath` / `SubjectPath`）。只给数据套壳**堵不住**
//! 「两条加载语句角色对调」那一类：`load_baseline` 若只吃 `&Path`，
//! 写成 `load_baseline(&roles.out)` 照样编译通过，拿到的却是待审区那份。
//! 所以角色一路标到 `Roles` 的字段类型上，两个加载点各自只收自己那一种。
//!
//! # 本命令验证了什么（以及**明确不**验证什么 · REQ-049）
//!
//! 本命令只回答一个问题：**两个门禁能不能读到并解析这份文件。**
//! 它的通过**不等于**门禁会通过 —— 这两件事必须分开讲，否则就是假信心。
//!
//! | | 门禁真正会做的事 | 本命令跑了吗 |
//! | --- | --- | --- |
//! | 读产物 | 读 `schema.json`，反序列化成 `SchemaReport` | ✅ 就是这个 |
//! | 翻译表 | 加载第二个输入 `src/extract/table_names.json` | ❌ |
//! | 抽取 | 对产物跑抽取器 `extract()` | ❌ |
//! | 投影 + 判定 | 跑 `project()`，对**抽出来的属性**判丢弃率 / 外键命名 | ❌ |
//!
//! 实测过的假信心（2026-10-10）：把真产物 91 张表**改名成同一个名字**、
//! 列内容原样保留 —— 形状上完全合法（表数 / 列数 / 计数一致性全对），
//! **本命令 EXIT=0**，而 `drop_rate` 门禁**丢弃率 96.9%、判红**。
//!
//! ⇒ 所以本命令的结论只能写成「门禁**读得到**」，
//! 永远不能写成「门禁**会过**」。想确认红不红，去跑门禁。
//!
//! # 安全
//!
//! - 勘查走 `introspect()`，即 AC-101 那条**强制只读**路径（只读
//!   `information_schema` / `pg_catalog`，零业务数据行）。
//! - **口令绝不回显**：本命令不打印连接串；库侧报错也过一遍 [`sanitize`]，
//!   把 URL 里的 userinfo 打码。

use anyhow::{anyhow, bail, Context, Result};
use ontok::extract::introspect::{introspect, SchemaReport, SOURCE_DATABASE_URL_ENV};
use std::ops::Deref;
use std::path::{Component, Path, PathBuf};

/// 覆盖门禁查找路径的环境变量（两个门禁都认）。
const SCHEMA_JSON_ENV: &str = "ONTOK_SCHEMA_JSON";

/// `tests/drop_rate.rs:29` 的缺省字面量 —— 相对 crate root（`cargo test` 的 CWD）。
const DROP_RATE_DEFAULT: &str = "../.review-workspace/fix-props/schema.json";

/// `tests/test_fk_naming.rs:312` 的缺省字面量 —— 绝对路径。
const FK_NAMING_DEFAULT: &str =
    "/home/myg133/Projects/Anchnet/Ontology/.review-workspace/fix-props/schema.json";

/// **登记基线**的缺省路径 —— 就是 [`FK_NAMING_DEFAULT`] 那个文件，
/// 也就是两个门禁实际会读的那份。**登记事实，本命令只读不写。**
const BASELINE_DEFAULT: &str = FK_NAMING_DEFAULT;

/// **现拉落盘目标**的缺省路径 —— 刻意落在**另一个文件**上（REQ-062 · AC-006）。
///
/// 放在 `gate-prefix`… 之外的 `gate-preflight/` 目录，是为了让「待审产物」
/// 与「登记基线」在目录上一眼可分；不放在 `fix-props/` 里，
/// 免得待审产物和门禁输入混在一起看不出谁是谁。
const OUT_DEFAULT: &str =
    "/home/myg133/Projects/Anchnet/Ontology/.review-workspace/gate-preflight/incoming-schema.json";

/// 逐项对账时，每条形态最多**点名**多少个对象（超出只报总数）。
///
/// 点名是硬要求（AC-005），但「现拉多出 80 列」不该把终端刷爆；
/// 于是**命名的部分一定列出**，另附「另有 N 项」——
/// 绝不退化成「缺失项 N 条」这种把后果合并掉的说法（AC-002）。
const MAX_NAMED: usize = 12;

/// 表数 / 列数的预期区间。
///
/// - **下界 `80`**：直接对齐 `tests/drop_rate.rs:64` 的 `MIN_TABLES` ——
///   本命令应当在**门禁判红之前**就判红，而不是等跑测试时才发现。
/// - **下界 `900` 列**：按真实产物标定（实测 `1051` 列 / `91` 表），
///   留约 15% 余量。
/// - **上界是防「拉错库 / 拉到垃圾」的护栏，不是增长限额**：
///   取实测值的 4 倍（表）/ 3 倍（列）。正常演化不会碰它，
///   但「连到了另一个库」或「写进了二进制噪声」会立刻撞上。
struct Limits {
    min_tables: usize,
    max_tables: usize,
    min_columns: usize,
    max_columns: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            min_tables: 80,
            max_tables: 400,
            min_columns: 900,
            max_columns: 3000,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 角色带类型（REQ-063 · AC-001 / AC-005）
//
// # 为什么要类型而不是判据
//
// 「参数对调」在两个参数**同类型**时是一个合法可编译的程序：类型系统天然
// 区分不了谁是谁，所以能挡住它的只能是「读源码断言调用点文本」那种监视式判据 ——
// 而监视式判据结构上管不到「名字没变、数据换了角色」。
//
// 换成 newtype 之后，对调**编译不过**：变异不是被抓到，而是**根本构造不出来**。
//
// ⚠️ 这两个类型**不是** `type Baseline = SchemaReport;` —— 那样等于什么都没做，
// 它们是各自独立的 struct，只是 `Deref` 到里面那份报告。
// ─────────────────────────────────────────────────────────────────────────────

/// **登记基线**（登记事实）：本命令**只读**它，绝不写。
#[derive(Debug)]
struct Baseline(SchemaReport);

/// **被校验的那一份**（现拉落盘目标 / 从磁盘重读回来的那份）。
#[derive(Debug)]
struct Subject(SchemaReport);

/// 登记基线的**路径**。只能喂给**只读基线**的那些函数。
#[derive(Debug, Clone)]
struct BaselinePath(PathBuf);

/// 现拉落盘目标（待审对象）的**路径**。只能喂给**写 / 校验被校验方**的那些函数。
#[derive(Debug, Clone)]
struct SubjectPath(PathBuf);

impl Deref for Baseline {
    type Target = SchemaReport;
    fn deref(&self) -> &SchemaReport {
        &self.0
    }
}

impl Deref for Subject {
    type Target = SchemaReport;
    fn deref(&self) -> &SchemaReport {
        &self.0
    }
}

// ⚠️ 路径**刻意不实现 Deref** —— 这是**纵深防御，不是承重件**（REQ-066 更正）。
//
// 承重的是「[`BaselinePath`] 与 [`SubjectPath`] 是**两个互不相同的 struct 类型**」：
// 形参要 `&BaselinePath`，实参给 `&SubjectPath` 就**编译不过**。
// QA 实测（REQ-063 · §2.3）：给两者补上 `impl Deref<Target = Path>` **之后**，
// 角色对调（M8）**仍然**是编译错误 —— Rust 的 deref 强转只会把
// `&SubjectPath` 变成 `&Path`，而 `&Path` 再也强转**不回去**。
//
// 也就是说：「刻意不 Deref」**不是**当年封住 M8 的理由，别把它当成
// 「加了 Deref 也不会破、可以放心加」的许可证。
// 那它留着有什么用？挡住将来有人写一个 `fn f(p: &Path)` 把两边糊在一起的写法 ——
// 那是真实存在的一类退化，但与 M8 的编译拒绝**是两件事**。
impl BaselinePath {
    fn new(p: impl Into<PathBuf>) -> Self {
        Self(p.into())
    }
    fn as_path(&self) -> &Path {
        &self.0
    }
}

impl SubjectPath {
    fn new(p: impl Into<PathBuf>) -> Self {
        Self(p.into())
    }
    fn as_path(&self) -> &Path {
        &self.0
    }
}

/// 本命令**真正接受**的参数（`parse_args` 的收单范围）。
///
/// 这张表存在的意义是给文案当**钉子**：文案里出现的每个 flag 必须能被
/// [`parse_args`] 收下，解析器收下的每个 flag 也必须在这张表里 ——
///
/// ```text
/// 两者各说各话 ⇒ REQ-063 · AC-004 的「残留旧文案」会以另一种形式长回来。
/// ```
///
/// 表与解析器由 `every_flag_in_the_messages_is_actually_accepted` 绑在一起。
/// 「未知参数」的报错**直接从这张表生成** —— 于是这张表在生产构建里也是活的，
/// 不是只为测试存在的摆设（否则编译器会报 dead_code，signal 会被噪音淹没）。
const ACCEPTED_ARGS: &[&str] = &[
    "--out",
    "--baseline",
    "--verify-only",
    "--min-tables",
    "--max-tables",
    "--min-columns",
    "--max-columns",
    "-h",
    "--help",
];

/// 凡是文案提到 [`SCHEMA_JSON_ENV`]，都必须同时给出这句**免责**。
///
/// 由来：REQ-062 让 `--out` **不再认**这个变量（继续认它 = 把覆盖路径留在默认路上），
/// 它现在只用于**显示**。而它又是两个门禁真正的输入开关 —— 两边各说一半，
/// 于是 `:357` 那句「设它指向你要的那个文件」就变成了残留旧文案：
/// 照做的人会以为本命令的基线跟着变了，实际**一点都不会变**。
const SCHEMA_JSON_DISCLAIMER: &str = "两个门禁都会读它，但它**不参与**本命令的角色解析";

#[tokio::main]
async fn main() -> Result<()> {
    let cli = parse_args(std::env::args().skip(1))?;
    if cli.help {
        print_help();
        return Ok(());
    }
    let Cli {
        out: out_arg,
        baseline: baseline_arg,
        verify_only,
        limits,
        ..
    } = cli;

    // 角色解析**先于连库**：落盘目标与基线撞成同一个文件时，
    // 那是个配置错误，不必等一次勘查往返、更不必碰库。
    let roles = resolve_roles(out_arg.as_deref(), baseline_arg.as_deref())?;
    println!("现拉落盘目标：{}", roles.out.as_path().display());
    println!("  来源        ：{}", roles.out_origin);
    println!("登记基线    ：{}", roles.baseline.as_path().display());
    println!("  来源        ：{}", roles.baseline_origin);
    if let Ok(v) = std::env::var(SCHEMA_JSON_ENV) {
        if !v.trim().is_empty() {
            println!("门禁读取位置：{v}（{}）", schema_json_note());
        }
    }

    if verify_only {
        println!("模式          ：--verify-only（不连源库、不写盘）");
        let (base, report, warn) = load_both(&roles, &limits, "--out 指向的那份")?;
        let rec = reconcile(&base, &report);
        finish(&roles, &report, &base, "--verify-only", &warn, &rec)?;
        return Ok(());
    }

    // ── 现拉 ─────────────────────────────────────────────────────────
    // 库侧的错一律过 sanitize：口令绝不回显（口令只走环境变量）。
    let url = std::env::var(SOURCE_DATABASE_URL_ENV).map_err(|_| {
        anyhow!(
            "缺少环境变量 {SOURCE_DATABASE_URL_ENV}（源库连接串，口令不入库）。\n\
             补救：从仓库根的 .env 取该变量导出，或直接给出连接串。\n\
             注意：源库连接串与 {SCHEMA_JSON_ENV}（测试库）刻意分开命名，别拿错。\n{}",
            guidance_hint(
                roles.out.as_path(),
                roles.baseline.as_path()
            )
        )
    })?;
    if url.trim().is_empty() {
        bail!(
            "环境变量 {SOURCE_DATABASE_URL_ENV} 是空字符串。\n{}",
            guidance_hint(roles.out.as_path(), roles.baseline.as_path())
        );
    }

    println!("模式          ：现拉（只读勘查 → 校验 → 落盘 → 对账）");
    let report = introspect(&url).await.map_err(|e| {
        anyhow!(
            "源库不可达 / 勘查失败：{}\n\
             本命令**不**接受「跳过」——门禁读不到输入时是 fail-closed panic，\
             跑全量测试只会得到一个看不懂的失败。\n{}",
            sanitize(&format!("{e:#}")),
            guidance_hint(roles.out.as_path(), roles.baseline.as_path())
        )
    })?;

    // 先在**内存里**校验：不合格就**不落盘**，保住已有的好产物。
    // （否则一次失败的现拉会把待审区里能用的产物覆盖成坏文件。）
    // 这里丢弃 `warn`：落盘后会从磁盘重读一遍，那次的 `warn` 才是权威的。
    let Findings { bad, .. } = validate(&report, &limits);
    if !bad.is_empty() {
        bail!(
            "现拉到了，但产物**不像一个真 schema**，已拒绝落盘（原有产物保持不变）：\n  - {}\n{}",
            bad.join("\n  - "),
            guidance_hint(roles.out.as_path(), roles.baseline.as_path())
        );
    }

    // 落盘目标是**待审区**，不是登记基线 —— 上面 resolve_roles 已经挡住撞车。
    // 对账失败时这份产物**留在磁盘上**：它正是「差异长什么样」的证据，
    // 删掉等于把问题变成无证可查。
    write_atomic(roles.out.as_path(), &report)?;

    // 再从**磁盘**读回验一次：挡住「写坏了 / 写半截 / 写到别处去了」。
    let (base, on_disk, warn) = load_both(&roles, &limits, "刚落盘的那份")?;
    let rec = reconcile(&base, &on_disk);
    finish(&roles, &on_disk, &base, "现拉", &warn, &rec)?;
    Ok(())
}

/// 命令行参数（[`parse_args`] 的产物）。
struct Cli {
    out: Option<String>,
    baseline: Option<String>,
    verify_only: bool,
    limits: Limits,
    help: bool,
}

/// 解析命令行。
///
/// 抽成独立函数**不是为了好看**，是为了让「文案里出现的 flag 必须真被收下」
/// 这条钉子（AC-004）能被写出来：测试直接把一个 flag 喂进来，
/// 用**真解析器**判它收不收，而不是在文案里 grep 一遍。
fn parse_args(args: impl Iterator<Item = String>) -> Result<Cli> {
    let mut cli = Cli {
        out: None,
        baseline: None,
        verify_only: false,
        limits: Limits::default(),
        help: false,
    };
    let mut args = args;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--out" => cli.out = Some(args.next().context("--out 需要一个路径参数")?),
            "--baseline" => {
                cli.baseline = Some(args.next().context("--baseline 需要一个路径参数")?)
            }
            "--verify-only" => cli.verify_only = true,
            "--min-tables" => {
                cli.limits.min_tables = parse_num(&mut args, "--min-tables")?;
            }
            "--max-tables" => {
                cli.limits.max_tables = parse_num(&mut args, "--max-tables")?;
            }
            "--min-columns" => {
                cli.limits.min_columns = parse_num(&mut args, "--min-columns")?;
            }
            "--max-columns" => {
                cli.limits.max_columns = parse_num(&mut args, "--max-columns")?;
            }
            // 见到 help 就立刻返回：后面的参数不再看（旧行为如此）。
            "-h" | "--help" => return Ok(Cli { help: true, ..cli }),
            other => bail!(
                "未知参数 {other}\n\
                 本命令接受：{}\n{}",
                ACCEPTED_ARGS.join(" / "),
                guidance_hint(Path::new(OUT_DEFAULT), Path::new(BASELINE_DEFAULT))
            ),
        }
    }
    Ok(cli)
}

fn parse_num(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<usize> {
    let raw = args.next().context(format!("{flag} 需要一个数字参数"))?;
    raw.parse::<usize>()
        .with_context(|| format!("{flag} 的值 {raw:?} 不是非负整数"))
}

/// 本次运行里两个**互不相同**的角色。
///
/// 拆成两个字段不是为了好看：旧代码只有一个 `--out`，
/// 它的缺省值恰好就是登记基线，于是「校验完写回去」=
/// 把待审对象写成登记事实（AC-000）。
///
/// ⚠️ 字段类型**带角色**（`SubjectPath` / `BaselinePath`，REQ-063 · AC-001）：
/// 正因为两个字段类型不同，「这条路径是哪一边」不能靠变量名承载，
/// 编译器和 [`load_both`] 一起把它钉住。
#[derive(Debug)]
struct Roles {
    /// 现拉落盘目标（待审对象）—— 本命令**写**它。
    out: SubjectPath,
    out_origin: String,
    /// 登记基线（登记事实）—— 本命令**只读**它。
    baseline: BaselinePath,
    baseline_origin: String,
}

/// 决定「现拉落到哪」与「跟谁对账」，并**拒绝两者是同一个文件**。
///
/// # 撞车判定**只看路径，不看内容**
///
/// 这是 AC-006 明写的要求：危害在「这次运行会去写登记事实」，
/// 与两个文件当前内容是否相同**无关**。
/// 若写成「内容一样就放过」，那么「还没现拉到东西」的时序下
/// 判据根本打不出问题 —— 门禁就能在被绕过的那条路上继续走。
/// 所以这里一次文件都不读、一次内容都不比。
fn resolve_roles(out_arg: Option<&str>, baseline_arg: Option<&str>) -> Result<Roles> {
    let (out, out_origin) = match out_arg {
        Some(p) => (
            SubjectPath::new(p),
            "--out 显式指定".to_string(),
        ),
        None => (
            SubjectPath::new(OUT_DEFAULT),
            "缺省：待审区（刻意 ≠ 登记基线，见 REQ-062 · AC-006）".to_string(),
        ),
    };
    let (baseline, baseline_origin) = match baseline_arg {
        Some(p) => (BaselinePath::new(p), "--baseline 显式指定".to_string()),
        None => default_baseline()?,
    };

    if same_file(&out, &baseline) {
        bail!(
            "落盘目标与登记基线**解析到同一个文件**，已拒绝执行：\n  \
             落盘目标：{}\n  \
             登记基线：{}\n\
             \x20 危害不是「内容会变」，是「这次运行会去写登记基线」——\
             门禁读的那份是**登记事实**，被一份待审对象覆盖之后，\
             差异的证据连同基线一起消失（该文件不在版本控制内，没有 git 兜底）。\
             \n\
             \x20 请二选一：\n  \
             \x20 1) 给这次现拉另指定落盘目标（待审区，与登记基线分开）：\n  \
             \x20    --out <另一个文件>\n  \
             \x20 2) 或给对账指定另一份登记基线：\n  \
             \x20    --baseline <另一个文件>\n\
             \x20 登记基线只能由**人**在复核之后显式更新；本命令不自动同步基线。",
            out.as_path().display(),
            baseline.as_path().display(),
        );
    }

    Ok(Roles {
        out,
        out_origin,
        baseline,
        baseline_origin,
    })
}

/// 登记基线的**缺省**位置，并保住原有的一条纪律：
/// 两个门禁的缺省路径必须落在同一个文件上。
///
/// 那条检查以前服务于「写一份喂饱两个门禁」；现在本命令不写那个文件了，
/// 但它**仍然有意义**：基线若有两个候选，「跟谁对账」就不确定，
/// 拿不准就不猜。
fn default_baseline() -> Result<(BaselinePath, String)> {
    let abs = PathBuf::from(BASELINE_DEFAULT);
    let cwd = std::env::current_dir().context("取不到当前工作目录")?;
    let rel_resolved = lexical_normalize(&cwd.join(DROP_RATE_DEFAULT));
    if rel_resolved != abs {
        bail!("{}", baseline_divergence_hint(&rel_resolved, &abs));
    }
    Ok((
        BaselinePath::new(abs),
        "缺省路径（两个门禁解析到同一文件）".to_string(),
    ))
}

/// 两个门禁的缺省路径在当前目录下解析不到同一个文件时的**报错正文**。
///
/// 抽成纯函数（不吃 cwd、不碰文件）是为了让文案能被测试直接断言 ——
/// 这条文案腐烂过一次：它建议「设 `ONTOK_SCHEMA_JSON` 指向你要的那个文件」，
/// 而 REQ-062 之后**设了它对本命令的基线毫无影响**，照做的人只会白折腾
/// （见 [`SCHEMA_JSON_DISCLAIMER`] 与 AC-004）。
fn baseline_divergence_hint(rel_resolved: &Path, abs: &Path) -> String {
    format!(
        "两个门禁的缺省路径在当前目录下**解析到不同文件**，不知道该跟哪份登记基线对账：\n  \
         drop_rate.rs      → {rel}\n  \
         test_fk_naming.rs → {abs}\n\
         本命令不猜哪一个是登记基线。请二选一：\n\
         1) 用 --baseline 显式指定登记基线（**本命令只认这个参数**，不拿环境变量定角色）：\n\
         \x20    --baseline <另一个文件>\n\
         2) 或在本仓的 crate root 目录下重跑本命令，让两个缺省路径重新对齐。\n\
         \x20 注意：{SCHEMA_JSON_ENV} {SCHEMA_JSON_DISCLAIMER} —— \
         设它不会改变「跟谁对账」，也不会改变「落到哪」。",
        rel = rel_resolved.display(),
        abs = abs.display(),
    )
}

/// 两个路径是不是同一个文件。
///
/// 三层判定，从强到弱，**只看路径，一个字节的内容都不读**（AC-006）：
///
/// 1. 两边都 `canonicalize` 得动（文件已存在）时比**解析后的真实路径**
///    —— 符号链接、`.`/`..`、相对路径 vs 绝对路径都算同一；
/// 2. 比 `(st_dev, st_ino)` —— **硬链接**（REQ-063 · AC-003）。
///    POSIX 说硬链接就是「同一个文件」，但 `canonicalize` **只解符号链接**，
///    看不见同一个 inode 的另一个名字。漏掉这一层，
///    `--out <登记基线的硬链接>` 就从「拒绝执行」那道拦截底下溜了过去；
/// 3. 任一边不存在（还没落盘的那份）时退化成**词法**比较。
///
/// 危害边界（如实写明，别让后来者重新论证一遍）：`write_atomic` 走 `rename`，
/// 而 `rename` 替换的是**目录项**、不穿透 inode ⇒ 登记基线的内容**不会**被改写
/// （QA 已实测）。第 2 层补的是「拒绝执行」这个**信号**，不是数据安全。
fn same_file(out: &SubjectPath, baseline: &BaselinePath) -> bool {
    let (a, b) = (out.as_path(), baseline.as_path());
    if let (Ok(x), Ok(y)) = (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        if x == y {
            return true;
        }
    }
    if let (Ok(x), Ok(y)) = (std::fs::metadata(a), std::fs::metadata(b)) {
        #[cfg(unix)] use std::os::unix::fs::MetadataExt;
        #[cfg(unix)] if x.dev() == y.dev() && x.ino() == y.ino() {
            return true;
        }
    }
    abs_lexical(a) == abs_lexical(b)
}

/// 相对当前目录补全并做词法规整（不碰文件系统）。
fn abs_lexical(p: &Path) -> PathBuf {
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(p)
    };
    lexical_normalize(&abs)
}

/// 词法规整（不碰文件系统）：把 `a/b/../c` 折成 `a/c`。
fn lexical_normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// 读回 + 校验**磁盘上**那份被校验对象。
///
/// `label` 是给报错用的称呼（「刚落盘的那份」/「--out 指向的那份」），
/// 免得两个角色同时出现在一个文件里时，报错让人分不清在讲谁。
///
/// 返回 `(产物, 警告)`：警告不改变退出码，由 [`print_ok`] 转述。
///
/// 解析用的是门禁**同一个** `SchemaReport` 类型 —— 判据不是「看起来像 JSON」，
/// 而是「门禁真能把它反序列化成 `SchemaReport`」。
///
/// **fail-closed 保持不变**：读不到 / 反序列化不了 / 计数不自洽 / 体量越界
/// 一律走 `bad` ⇒ 退出码非 0。本命令没有、也不会新增「读不到就跳过」的分支。
///
/// ⚠️ `path` 是 [`SubjectPath`] 而不是 `&Path`（REQ-063 · AC-001）：
/// 「被校验的那一份」这个角色必须写进参数类型，否则
/// `load_and_verify(&roles.baseline)` 是一个**能编译**的程序 ——
///
/// 拿待审区的内容当基线去读，语义整个反过来，而编译一声不响。
fn load_and_verify(
    subject: &SubjectPath,
    limits: &Limits,
    label: &str,
) -> Result<(Subject, Vec<String>)> {
    let path = subject.as_path();
    let raw = std::fs::read_to_string(path).map_err(|e| {
        anyhow!(
            "读不到{label}：{}（{e}）\n\
             这个文件**不在版本控制内**（`.review-workspace/` 在仓库外且被忽略），\
             清理临时目录后必须重新拉。\n{}",
            path.display(),
            guidance_hint(path, Path::new(BASELINE_DEFAULT))
        )
    })?;
    let report: SchemaReport = serde_json::from_str(&raw).map_err(|e| {
        anyhow!(
            "产物不是合法的 schema JSON（门禁会在这里直接 panic）：{}（{e}）\n{}",
            path.display(),
            guidance_hint(path, Path::new(BASELINE_DEFAULT))
        )
    })?;
    let Findings { bad, warn } = validate(&report, limits);
    if !bad.is_empty() {
        bail!(
            "{label}校验不通过（{}）：\n  - {}\n{}",
            path.display(),
            bad.join("\n  - "),
            guidance_hint(path, Path::new(BASELINE_DEFAULT))
        );
    }
    Ok((Subject(report), warn))
}

/// 把**两个角色**一起从磁盘读进来。
///
/// 刻意**只写一遍**：原先在 `main` 的两条分支里各写了一遍
/// （`load_and_verify` + `load_baseline`），于是「把两条加载语句的角色对调」
/// 是一个能编译的写法（QA 的 M8）—— 调用点一个字符都不用改。
///
/// 现在两条分支共用这一个入口，返回值顺序由类型钉死（`(Baseline, Subject, …)`），
/// 而「基线读哪个文件、被校验方读哪个文件」由下面这两行**唯一**决定：
/// [`load_baseline`] 只收 [`BaselinePath`]，[`load_and_verify`] 只收 [`SubjectPath`]，
/// 写反了**编译不过**。
///
/// 顺带拿到一条**行为**判据：`load_both_roles_keep_their_own_file` 真的把
/// 两个内容不同的文件喂进来，断言回来的 `Baseline` 里装的是**基线那份**的内容。
fn load_both(
    roles: &Roles,
    limits: &Limits,
    subject_label: &str,
) -> Result<(Baseline, Subject, Vec<String>)> {
    let (subj, warn) = load_and_verify(&roles.out, limits, subject_label)?;
    let base = load_baseline(&roles.baseline)?;
    Ok((base, subj, warn))
}

/// 读**登记基线**。
///
/// **只解析，不套体量区间** —— 区间是给「这次现拉的产物」定的护栏，
/// 拿它去卡一份历史登记事实，要么误伤（老版本基线本来就少几列），
/// 要么变成「改基线来绕开区间」的后门。基线只要求**能被解析成 SchemaReport**。
///
/// 基线不存在 ⇒ 报错退出（fail-closed）：没有登记事实就对不了账，
/// 「跳过对账」等于把「判了通过却没对过账」放回来。
///
/// ⚠️ `path` 是 [`BaselinePath`]：登记基线是**只读**角色，
/// 不接受一条待审区的路径（见 [`load_and_verify`] 里同一件事的另一面）。
fn load_baseline(path: &BaselinePath) -> Result<Baseline> {
    let path = path.as_path();
    let raw = std::fs::read_to_string(path).map_err(|e| {
        anyhow!(
            "读不到登记基线：{}（{e}）\n\
             没有登记事实就**对不了账** —— 本命令不提供「跳过对账」，\
             否则「判了通过却没对过账」这条假信心就又回来了。\n\
             \x20 登记基线只能由**人**建立：跑一次现拉，人工复核后显式放到该路径。\n\
             \x20 临时换个位置用 --baseline 指定也可以。",
            path.display()
        )
    })?;
    serde_json::from_str(&raw).map_err(|e| {
        anyhow!(
            "登记基线不是合法的 schema JSON：{}（{e}）\n\
             \x20 登记基线必须与门禁读的是**同一个 SchemaReport 结构**（现拉产出的那份即可）。",
            path.display()
        )
    })
    .map(Baseline)
}

/// 逐项对账的结果 —— 与 [`Findings`] 一样分两路：
/// 「阻断」与「只提示」不许合并，否则「多判一条」就会多出一条没人论证过的失败路径。
#[derive(Debug)]
struct Reconcile {
    /// 缺表 / 缺列 / 类型变了 / 主版本不符 —— 任一条 ⇒ 非零退出码。
    bad: Vec<String>,
    /// 注释没了 / 现拉多出 —— 一律不影响退出码。
    warn: Vec<String>,
}

/// 现拉结果 vs 登记基线，逐项对账（REQ-062 · AC-002 / AC-003）。
///
/// # 方向：**基线里的东西，在现拉结果里没了** ⇒ 阻断
///
/// 反方向（现拉多出来）**只 warn**。这不是宽松，是因为源库**本来就会合法演进**：
/// 备份表、同步簿记表、派生视图都是正常产物，96 表 vs 91 表不该是事故
/// （AC-001 明写这一点）。把「多出来」也判成阻断，
/// 就会在第一次正常演进时把人逼到「改基线来消差异」——
/// 那正好把判据变成恒真。
///
/// # 四个形态必须分开点名，不能合并计数
///
/// | 形态 | 业务后果 | 去向 |
/// | --- | --- | --- |
/// | 缺表 | 抽取器**根本不会去碰**这张表 | bad |
/// | 表在、缺列 | 抽取器**会去碰**它，但拿不到该属性 | bad |
/// | 列在、类型变了 | 拿到的是另一套语义，静默走偏 | bad |
/// | 列在、类型同、注释没了 | 注释是抽取最重要的信息源之一，但不至于错 | warn |
///
/// 合并成「缺失项 N 条」就等于把上面四种后果也合并了：
/// 「表没了」和「少了 3 列」要办的事根本不是一回事。
///
/// # 「多出注释」（形态 g）不报
///
/// 基线没注释、现拉有注释 —— 那是源库补全了信息，没有可担心的后果，
/// 连 warn 都不必给，免得噪声淹没真信号。
///
/// ⚠️ 两个参数**类型不同**（[`Baseline`] / [`Subject`]，REQ-063 · AC-001）：
/// 旧签名 `(&SchemaReport, &SchemaReport)` 两个实参同类型，于是「把两个实参对调」
/// 是一个合法可编译的程序 —— 判据只能去监视源码文本，
/// 而监视式判据结构上管不到「名字没变、数据换了角色」（QA 的 M8）。
/// 换成 newtype 之后对调**编译不过**：这类变异不是被抓到，而是不可构造。
fn reconcile(base: &Baseline, subj: &Subject) -> Reconcile {
    let mut bad = Vec::new();
    let mut warn = Vec::new();

    // ── AC-003：server_version 只比主版本 ──────────────────────────
    match (
        major_version(&base.server_version),
        major_version(&subj.server_version),
    ) {
        (Some(bm), Some(sm)) if bm != sm => bad.push(format!(
            "PostgreSQL 主版本不符：基线 {bm}（{}）vs 现拉 {sm}（{}）。\
             理由不是「版本必须一样」，是**服务端语义可能不同**\
             （information_schema 的呈现、类型名写法、约束推导）——\
             这类差异**不体现在表名列数上**，只会静默改变抽取结果。",
            base.server_version, subj.server_version
        )),
        (Some(_), Some(_)) => {}
        (b, s) => bad.push(format!(
            "读不出 PostgreSQL 主版本，对不了账（fail-closed）：基线 {:?} / 现拉 {:?}。\
             \x20 版本读不出来时「主版本相同」无法成立，\
             本命令不拿「读不到」当「没问题」。",
            b.map(|v| v.to_string()).unwrap_or_else(|| base.server_version.clone()),
            s.map(|v| v.to_string())
                .unwrap_or_else(|| subj.server_version.clone()),
        )),
    }

    // ── 基线 → 现拉：少了就是少了（四个形态分开点名）──────────────
    for bt in &base.tables {
        let Some(st) = subj.tables.iter().find(|t| t.name == bt.name) else {
            // 形态 a：整张表没了。**不再逐列报这张表** ——
            // 否则形态 b 会把它整个重报一遍，两个形态的意义又糊在一起。
            bad.push(format!(
                "缺表：基线登记的表 {} 在现拉结果里不存在 —— \
                 这张表整个没了，抽取器根本不会去碰它",
                bt.name
            ));
            continue;
        };
        for bc in &bt.columns {
            let Some(sc) = st.columns.iter().find(|c| c.name == bc.name) else {
                bad.push(format!(
                    "缺列：{}.{} —— 表 {} 还在，但基线登记的这一列在现拉结果里没有\
                     （抽取器会去碰这张表，却拿不到该属性）",
                    bt.name, bc.name, bt.name
                ));
                continue;
            };
            if sc.data_type != bc.data_type {
                bad.push(format!(
                    "列类型变了：{}.{}：{} → {} —— 列还在，但抽取会按另一套语义处理",
                    bt.name, bc.name, bc.data_type, sc.data_type
                ));
            } else if has_comment(bc.comment.as_deref()) && !has_comment(sc.comment.as_deref()) {
                // 形态 d：降级为 warn —— 注释是重要信息源，但缺注释不等于抽错。
                warn.push(format!(
                    "注释没了：{}.{} —— 基线登记有注释，现拉结果里没有（不阻断）",
                    bt.name, bc.name
                ));
            }
        }
    }

    // ── 现拉 → 基线：多出来的一律只 warn ──────────────────────────
    // 按形态聚成两条（每条点名到上限），而不是每列一行：
    // 「现拉合法新增了几十列」是常态，不该把真信号刷掉。
    let mut extra_objects: Vec<String> = Vec::new();
    let mut extra_columns: Vec<String> = Vec::new();
    for st in &subj.tables {
        let Some(bt) = base.tables.iter().find(|t| t.name == st.name) else {
            extra_objects.push(st.name.clone());
            continue;
        };
        for sc in &st.columns {
            if !bt.columns.iter().any(|c| c.name == sc.name) {
                extra_columns.push(format!("{}.{}", st.name, sc.name));
            }
        }
    }
    if !extra_objects.is_empty() {
        warn.push(format!(
            "现拉多出 {} 个对象（基线没有登记）：{} —— 源库合法新增，不阻断",
            extra_objects.len(),
            named(&extra_objects)
        ));
    }
    if !extra_columns.is_empty() {
        warn.push(format!(
            "现拉多出 {} 列（基线没有登记）：{} —— 源库合法新增，不阻断",
            extra_columns.len(),
            named(&extra_columns)
        ));
    }

    Reconcile { bad, warn }
}

/// 点名若干对象：**先列出来**，超出的只报数量。
///
/// 绝不退化成「N 项」而不点名 —— 点名是硬要求（AC-005）。
fn named(items: &[String]) -> String {
    let shown: Vec<&str> = items.iter().take(MAX_NAMED).map(String::as_str).collect();
    if shown.len() < items.len() {
        format!("{}（另 {} 项未列出）", shown.join("、"), items.len() - shown.len())
    } else {
        shown.join("、")
    }
}

/// 注释算不算「有」：空白串与缺省同义。
fn has_comment(c: Option<&str>) -> bool {
    c.map(|s| !s.trim().is_empty()).unwrap_or(false)
}

/// 取 `server_version` 里的**主版本号**。
///
/// 取**第一段连续数字**：`PostgreSQL 17.4 (Debian 17.4-1…)` → `17`，
/// `16.14 (Debian …)` → `16`。刻意**不**比较完整版本串：
/// `16.9 → 16.14` 是补丁更新，抽取所依赖的服务端语义没变，不该阻断（AC-003）。
fn major_version(s: &str) -> Option<u32> {
    let b = s.as_bytes();
    let start = b.iter().position(|c| c.is_ascii_digit())?;
    let mut end = start;
    while end < b.len() && b[end].is_ascii_digit() {
        end += 1;
    }
    s[start..end].parse::<u32>().ok()
}

/// 对账 + 出结果：**有阻断项就非零退出**，否则打印通过提示。
///
/// 单独抽成函数，是为了让「非零退出码」这条判据（AC-005）
/// 能被**真的调用到**并断言，而不是只靠眼看。
fn finish(
    roles: &Roles,
    subj: &Subject,
    base: &Baseline,
    mode: &str,
    warn: &[String],
    rec: &Reconcile,
) -> Result<()> {
    if !rec.bad.is_empty() {
        bail!(
            "对账不通过（{} 张表 / {} 列）vs 登记基线（{} 张表 / {} 列）：\n  - {}\n{}\n  \
             \x20 被校验的那份：{}\n  \
             登记基线：{}\n\
             \x20 差异逐条列在上面了。**先分清哪个形态**：\n\
             \x20 · 缺表 / 缺列 / 类型变了 —— 抽取结果会与登记事实不一致，先查源库是不是指的同一个环境；\n\
             \x20 · 主版本不符 —— 服务端语义可能不同，这类差异不体现在表名列数上。\n\
             \x20 本命令**不会**自动更新登记基线：基线是登记事实，只能由人复核后显式更新。",
            subj.table_count,
            subj.column_count,
            base.table_count,
            base.column_count,
            rec.bad.join("\n  - "),
            // warn 也一并转述：判据「一次算完再报」——只报失败项，
            // 会让人修完失败项再跑一次才发现还有一堆 warn，来回试。
            if rec.warn.is_empty() {
                String::new()
            } else {
                format!("\n  （以下只提示、不阻断）\n  - {}", rec.warn.join("\n  - "))
            },
            roles.out.as_path().display(),
            roles.baseline.as_path().display(),
        );
    }
    print_ok(
        roles.out.as_path(),
        subj,
        mode,
        warn,
        &rec.warn,
        roles.baseline.as_path(),
    );
    Ok(())
}

/// 校验结果分两路 —— **失败**与**警告**必须分开，否则「多判一条」就会
/// 多出一条没人论证过的失败路径。
///
/// # 为什么表名唯一性只落在 `warn`
///
/// 本命令对「门禁会不会红」**没有发言权**：它从不调用 `extract()`，
/// 从不加载 `src/extract/table_names.json`，也从不调用 `project()`（见文件头
/// 「本命令验证了什么」）。于是重复表名只能当**信号**，不能当**判据**：
///
/// - 已实测的只有「91 张**全**同名 + 完整列 ⇒ `drop_rate` 判红（丢弃率 96.9%）」；
/// - 「两张同名 ⇒ 必红」**已被实测否证**：91 张里只有 2 张同名时
///   （`activity` ×2），`drop_rate` 门禁**照样 PASS（EXIT=0）**。
///
/// 所以这里选 **fail-open**：重复表名 ⇒ 警告 + 退出码仍是 0。
/// 放进 `bad` 就是拿一个**已被反例推翻**的断言去拦人 —— 会误伤合法产物。
/// 这**不是**削弱门禁的 fail-closed —— 门禁自身的「读不到就失败」在
/// [`load_and_verify`] 里原样保留，本检查也从不参与退出码。
struct Findings {
    /// 判失败的那些（缺任一条 ⇒ 不落盘 / 退出码非 0）。
    bad: Vec<String>,
    /// 只提示，不影响退出码。
    warn: Vec<String>,
}

/// 全部判据**一次算完再报** —— 只报第一个错等于让人来回试。
///
/// 内部一致性（`tables.len() == table_count`、列数求和相等）不是凑数：
/// 它正是「拉了个空壳也算通过」的解药 —— 一个 `tables: []` 却自称
/// `table_count: 91` 的文件，光看计数骗得过，只看一致性骗不过。
fn validate(r: &SchemaReport, lim: &Limits) -> Findings {
    let mut bad = Vec::new();
    let mut warn = Vec::new();

    if r.transaction_read_only != "on" {
        bad.push(format!(
            "只读留痕不对：transaction_read_only = {:?}（应为 \"on\"）—— 这份产物不是只读勘查来的",
            r.transaction_read_only
        ));
    }
    if r.tables.len() != r.table_count {
        bad.push(format!(
            "空壳迹象：tables 里 {} 张表，表头却写 table_count = {}",
            r.tables.len(),
            r.table_count
        ));
    }

    let sum_cols: usize = r.tables.iter().map(|t| t.columns.len()).sum();
    let sum_declared: usize = r.tables.iter().map(|t| t.column_count).sum();
    let sum_commented: usize = r.tables.iter().map(|t| t.commented_count).sum();
    if sum_cols != r.column_count {
        bad.push(format!(
            "列数对不上：表头 column_count = {}，tables 实际加起来 = {}",
            r.column_count, sum_cols
        ));
    }
    if sum_declared != r.column_count {
        bad.push(format!(
            "各表自称的列数加起来 = {}，与表头 column_count = {} 不符",
            sum_declared, r.column_count
        ));
    }
    if sum_commented != r.commented_column_count {
        bad.push(format!(
            "各表自称的注释列加起来 = {}，与表头 commented_column_count = {} 不符",
            sum_commented, r.commented_column_count
        ));
    }
    for t in &r.tables {
        if t.column_count != t.columns.len() {
            bad.push(format!(
                "表 {} 自称 {} 列，columns 里只有 {} 个",
                t.name, t.column_count, t.columns.len()
            ));
        }
    }

    if r.table_count < lim.min_tables {
        bad.push(format!(
            "表数异常偏少：{} 张 < 下界 {}（对齐 drop_rate.rs 的 MIN_TABLES）—— 疑似空壳或连错了库",
            r.table_count, lim.min_tables
        ));
    } else if r.table_count > lim.max_tables {
        bad.push(format!(
            "表数异常偏多：{} 张 > 上界 {}（实测约 90）—— 疑似连错了库",
            r.table_count, lim.max_tables
        ));
    }
    if r.column_count < lim.min_columns {
        bad.push(format!(
            "列数异常偏少：{} 列 < 下界 {}（实测约 1050）—— 疑似空壳或连错了库",
            r.column_count, lim.min_columns
        ));
    } else if r.column_count > lim.max_columns {
        bad.push(format!(
            "列数异常偏多：{} 列 > 上界 {}（实测约 1050）—— 疑似连错了库",
            r.column_count, lim.max_columns
        ));
    }

    // ── 表名唯一性：启发（warn），不是判据（bad）────────────────
    //
    // 依据是可查的：勘查器按表名把行折叠成表
    // （`src/extract/introspect.rs` 里 `tables.iter_mut().find(|t| t.name == tname)`），
    // **同一个勘查器不可能吐出两个同名表**。所以重复表名说明这份产物
    // 不是这条路径产出的（手改过 / 别的写入方 / 被换过），值得人看一眼。
    //
    // ⚠️ 但它**不判失败**：同名表在形状上完全合法（表数、列数、
    // 各处计数一致性全对），门禁红不红取决于抽取器怎么合并这些表，
    // 而那件事本命令管不着、也没验证过。
    let dups = duplicate_table_names(r);
    if !dups.is_empty() {
        let shown: Vec<String> = dups
            .iter()
            .take(5)
            .map(|(n, c)| format!("{n} ×{c}"))
            .collect();
        warn.push(format!(
            "表名不唯一：{} 个表名在 tables 里重复出现（{}{}）。\
             勘查器按表名折叠，正常不会产出重复表名，所以这**值得人工核对产物是怎么来的**。\n\
             \x20     但请注意：**重复表名 ≠ 门禁会红**。同名表在形状上完全合法\
             （表数 / 列数 / 计数一致性全对），门禁红不红取决于抽取器怎么处理，\
             本命令不跑抽取器、无法判断。要确认只能跑门禁。",
            dups.len(),
            shown.join("、"),
            if dups.len() > shown.len() { " 等" } else { "" }
        ));
    }

    Findings { bad, warn }
}

/// 按首次出现顺序列出重复的表名及其次数。
fn duplicate_table_names(r: &SchemaReport) -> Vec<(String, usize)> {
    let mut seen: Vec<(&str, usize)> = Vec::new();
    for t in &r.tables {
        match seen.iter_mut().find(|(n, _)| *n == t.name.as_str()) {
            Some((_, c)) => *c += 1,
            None => seen.push((t.name.as_str(), 1)),
        }
    }
    seen.into_iter()
        .filter(|(_, c)| *c > 1)
        .map(|(n, c)| (n.to_string(), c))
        .collect()
}

/// 原子落盘：先写同目录临时文件再 `rename`。
/// 门禁是 fail-closed 的，半截文件等于门禁必红 —— 不给它这个机会。
fn write_atomic(path: &Path, r: &SchemaReport) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("建不出目录 {}", parent.display()))?;
        }
    }
    let file_name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "schema.json".to_string());
    let tmp = path.with_file_name(format!(".{file_name}.tmp"));

    std::fs::write(&tmp, serde_json::to_string_pretty(r)?)
        .with_context(|| format!("写不出 {}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .with_context(|| format!("{} 落不到 {}", tmp.display(), path.display()))?;
    Ok(())
}

/// 把 URL 里的 userinfo（含口令）打码。
///
/// 库侧报错可能把连接串带出来；口令绝不允许出现在终端、报告或 commit 里。
fn sanitize(msg: &str) -> String {
    msg.split_whitespace()
        .map(|tok| {
            let Some(scheme_end) = tok.find("://") else {
                return tok.to_string();
            };
            let after = &tok[scheme_end + 3..];
            let seg_end = after.find('/').unwrap_or(after.len());
            let (authority, rest) = after.split_at(seg_end);
            match authority.find('@') {
                Some(at) => format!(
                    "{}://<userinfo-redacted>@{}{}",
                    &tok[..scheme_end],
                    &authority[at + 1..],
                    rest
                ),
                None => tok.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// 关于 [`SCHEMA_JSON_ENV`] 的**唯一**说法。
///
/// 凡是提到这个变量的文案都从这里取，并自动带上 [`SCHEMA_JSON_DISCLAIMER`] ——
/// 分散写就会出现「有的地方说了免责、有的地方只说「门禁也认它」」，
/// 而读后者的人会以为设了它就能改本命令的行为。REQ-063 · AC-004 的
/// 残留旧文案正是这么长回来的。
fn schema_json_note() -> String {
    format!(
        "环境变量 {SCHEMA_JSON_ENV}：{SCHEMA_JSON_DISCLAIMER} —— \
         设它不会改变「跟谁对账」，也不会改变「落到哪」。"
    )
}

/// 出错时的补救提示 —— 一次把**两个角色**都说清楚。
///
/// 旧文案只说「门禁会读的位置」，那是「输入」与「输出」还是同一个文件时代的说法；
/// 现在两者是两份文件，只说一个会让人以为工具还在写门禁读的那份。
fn guidance_hint(subject: &Path, baseline: &Path) -> String {
    format!(
        "  这次被校验的那份：{SUB}\n  \
         登记基线（只读）  ：{BASE}\n\
         \n\
         下一步（三选一）：\n\
         \x20 1) 现拉一份新的（本命令自带勘查，落在**待审区**，不碰登记基线）：\n\
         \x20    export {SRC}=\"postgresql://…\"\n\
         \x20    cargo run --bin ontok-gate-preflight -- --out <待审区文件> --baseline {BASE}\n\
         \x20 2) 或用现有勘查器单独拉：\n\
         \x20    cargo run --bin ontok-introspect -- --out {SUB}\n\
         \x20 3) 或用 --baseline 显式指定另一份登记基线。\n\
         \n\
         注意：两份文件都**不在版本控制内**，清理临时目录后**必须**重新拉；\n\
         \x20 登记基线只能由人复核后显式更新，本命令不自动同步它。\n\
         \x20 {NOTE}",
        SUB = subject.display(),
        BASE = baseline.display(),
        SRC = SOURCE_DATABASE_URL_ENV,
        NOTE = schema_json_note(),
    )
}

/// 两个门禁的**测试函数名** —— 成功提示要点名它们，
/// 不能只丢一句「跑全量」让人自己猜哪两条才是门禁。
const DROP_RATE_GATE: &str = "real_schema_drop_rate_report"; // tests/drop_rate.rs
const FK_NAMING_GATE: &str = "test_real_schema_customer_id_has_exactly_one_display_name"; // tests/test_fk_naming.rs

/// 成功提示（REQ-049 · AC-001 · REQ-062）。
///
/// # 这里最容易犯的错：把「读得到」写成「门禁会过」
///
/// 本命令**只**回答两个问题：这份文件**能不能被解析**，
/// 以及它与**登记基线**对不对得上。
/// 门禁红不红由另外三件事决定，而这三件本命令**一件都没跑**：
///
/// | 门禁真正会做的事 | 本命令跑了吗 |
/// | --- | --- |
/// | 加载第二个输入 `src/extract/table_names.json` | ❌ |
/// | 对产物跑抽取器 `extract()` | ❌ |
/// | 跑投影 `project()` 并对**抽出来的属性**判丢弃率 / 外键命名 | ❌ |
///
/// 所以正确说法是「**读得到**」，不是「**门禁会过**」。
/// 实测过的假信心：91 张同名表 + 完整列 ⇒ 本命令 EXIT=0，
/// 而 `drop_rate` 门禁丢弃率 96.9%、判红（2026-10-10 实测，
/// 人造产物：把真产物 91 张表改名成同一个名字、列内容原样保留）。
fn print_ok(
    path: &Path,
    r: &SchemaReport,
    mode: &str,
    warn: &[String],
    reconcile_warn: &[String],
    baseline: &Path,
) {
    println!();
    println!("前置检查通过（{mode}）");
    println!("  表 / 列      : {} / {}", r.table_count, r.column_count);
    println!(
        "  有注释列 / 外键: {} / {}",
        r.commented_column_count, r.foreign_key_count
    );
    println!("  schema        : {}", r.schema);
    println!("  只读留痕      : transaction_read_only = {}", r.transaction_read_only);
    println!("  勘查时刻      : {}", r.introspected_at);
    println!("  被校验的那份  : {}", path.display());
    println!("  登记基线      : {}", baseline.display());

    if !warn.is_empty() {
        println!();
        println!("⚠️ 启发式提示（不影响退出码，也不是门禁判据）：");
        for w in warn {
            println!("  · {w}");
        }
    }

    if !reconcile_warn.is_empty() {
        println!();
        println!("⚠️ 对账提示（**不影响退出码**：源库多出来的东西是合法演进，不是事故）：");
        for w in reconcile_warn {
            println!("  · {w}");
        }
    }

    println!();
    println!("── 这一步证明了什么 ────────────────────────────────");
    println!("  ✅ 这份文件能被解析（与门禁同一个 SchemaReport），计数自洽，体量在区间内，");
    println!("     并且与登记基线**逐项对过账**：基线登记的东西一个都没丢。");
    println!("     （登记表少、缺列、类型变了、主版本不符 —— 任何一条都会让本命令非零退出。）");
    println!();
    println!("  ⚠️ **本次产物没有进入门禁读取位置。**");
    println!("     门禁读的是登记基线，而本命令**只读它、不写它** ——");
    println!("     登记基线是登记事实，只能由人复核后显式更新。");
    println!("     要让门禁看到这份产物，请自行把它放到门禁实际会读的位置。");
    println!();
    println!("  ⚠️ **读得到 ≠ 门禁会通过。**");
    println!("     门禁红不红取决于**内容**，而那三件事本命令一件都没跑：");
    println!("       · 门禁还要第二个输入 src/extract/table_names.json（本命令不碰它）");
    println!("       · 门禁会对这份文件跑抽取器 extract()（本命令不跑）");
    println!("       · 门禁会跑投影 project()，再对**抽出来的属性**判丢弃率 / 外键命名");
    println!("     所以本命令**无法**告诉你门禁会不会红，只能告诉你输入到位了。");
    println!("     实测过的假信心：91 张同名表 + 完整列 ⇒ 本命令通过，drop_rate 门禁仍判红。");
    println!();
    println!("── 下一步：直接跑这两个门禁，别只看本命令 ──────────────");
    println!("     cargo test --test drop_rate                    # {DROP_RATE_GATE}");
    println!("     cargo test --test test_fk_naming              # {FK_NAMING_GATE}");
    println!("     cargo test --no-fail-fast                     # 合起来一次跑完");
    println!();
    println!("⚠️ 这两份文件都不在版本控制内 —— 清理临时目录后必须重新跑本命令。");
}

fn print_help() {
    println!("{}", help_text());
}

/// `--help` 的正文。抽成纯函数，好让 AC-004 的「文案 ↔ 参数集合」钉子
/// 能直接断言它，而不必去截 stdout。
#[allow(clippy::needless_raw_string_hashes)]
fn help_text() -> String {
    format!(
        "门禁输入前置检查（REQ-048 · REQ-062 · REQ-063）\n\n\
         用法：\n  \
         cargo run --bin ontok-gate-preflight                    现拉 + 校验 + 落盘 + 对账\n  \
         cargo run --bin ontok-gate-preflight -- --verify-only   只校验已有产物（不连库）\n\n\
         两个角色，两个独立参数，**刻意不是同一个文件**（REQ-062 · AC-006）：\n  \
         · 登记基线（登记事实，本命令只读）  {BASE}\n  \
         · 现拉落盘目标（待审对象，本命令写）{OUT}\n  \
         两者解析到同一文件 ⇒ **报错退出**，只看路径不看内容。\n\
         门禁读的是登记基线；想让门禁看到新产物，须由人复核后显式更新基线。\n\n\
         门禁靠什么找到产物（两个门禁同款顺序）：\n  \
         1. 环境变量 {SCHEMA_JSON_ENV}\n  \
         2. 缺省路径 {ABS}（与 drop_rate.rs 的 {REL} 解析到同一文件）\n\
         \x20 {NOTE}\n\n\
         本命令自己的环境变量：{SRC}（源库连接串，口令不入库）\n\
         参数：\n  \
         \x20--out <path>        现拉落盘目标（默认：待审区，与登记基线分开）\n  \
         \x20--baseline <path>   登记基线（默认：门禁实际会读的那份，只读不写）\n  \
         \x20--verify-only       不连源库、不写盘：只校验 --out 那份并对账\n  \
         \x20--min-tables N      表数下界（默认 80，对齐 drop_rate.rs 的 MIN_TABLES）\n  \
         \x20--max-tables N      表数上界（默认 400，防拉错库）\n  \
         \x20--min-columns N     列数下界（默认 900）\n  \
         \x20--max-columns N     列数上界（默认 3000，防拉错库）",
        NOTE = schema_json_note(),
        ABS = BASELINE_DEFAULT,
        OUT = OUT_DEFAULT,
        BASE = BASELINE_DEFAULT,
        REL = DROP_RATE_DEFAULT,
        SRC = SOURCE_DATABASE_URL_ENV,
    )
}
// ─────────────────────────────────────────────────────────────────────────────
// 测试（REQ-062 · REQ-063）
//
// 这里刻意做成**同文件内的单元测试**：对账逻辑只被这个 bin 用，
// 放进 `tests/` 的集成测试反而要为了「能 import 到 bin 的私有函数」
// 把逻辑挪进 lib —— 那是为测试而搬代码，不是为可测性设计。
//
// 夹具原则：**造文件，不造库**。全部在内存里拼 `SchemaReport`，
// 一个字节都不连库、不碰登记基线。
// ─────────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use ontok::extract::introspect::{ColumnMeta, TableMeta};

    fn col(name: &str, ty: &str, comment: Option<&str>) -> ColumnMeta {
        ColumnMeta {
            name: name.to_string(),
            data_type: ty.to_string(),
            nullable: true,
            comment: comment.map(str::to_string),
            is_primary_key: false,
        }
    }

    fn tbl(name: &str, columns: Vec<ColumnMeta>) -> TableMeta {
        let commented_count = columns.iter().filter(|c| has_comment(c.comment.as_deref())).count();
        TableMeta {
            name: name.to_string(),
            comment: None,
            column_count: columns.len(),
            commented_count,
            columns,
        }
    }

    /// 拼一份计数自洽的报告 —— `validate()` 对计数不自洽是 fail-closed 的，
    /// 夹具若不自洽，失败会来自体量/一致性而不是对账，判据就被噪声盖住了。
    fn rep(tables: Vec<TableMeta>, ver: &str) -> SchemaReport {
        SchemaReport {
            server_version: ver.to_string(),
            transaction_read_only: "on".to_string(),
            schema: "public".to_string(),
            table_count: tables.len(),
            column_count: tables.iter().map(|t| t.columns.len()).sum(),
            commented_column_count: tables.iter().map(|t| t.commented_count).sum(),
            foreign_key_count: 0,
            tables,
            introspected_at: "2026-10-10T00:00:00+08:00".to_string(),
        }
    }

    /// 一份最小的「三张表、每表两列」基线：形状像真的，规模小到能一眼看全。
    fn sample_baseline() -> SchemaReport {
        rep(
            vec![
                tbl(
                    "users",
                    vec![
                        col("id", "uuid", Some("主键")),
                        col("name", "character varying", Some("姓名")),
                    ],
                ),
                tbl(
                    "orders",
                    vec![
                        col("id", "uuid", Some("主键")),
                        col("amount", "numeric", Some("金额")),
                    ],
                ),
                tbl(
                    "items",
                    vec![
                        col("id", "uuid", Some("主键")),
                        col("sku", "text", Some("编码")),
                    ],
                ),
            ],
            "PostgreSQL 17.4 on x86_64-pc-linux-gnu, compiled by gcc 8.3.0, 64-bit",
        )
    }

    /// 临时目录 —— 默认走 `ONTOK_TEST_TMPDIR`（本仓约定：仓库根的
    /// `.review-workspace/req063d/`，因为 worktree 内的 `.review-workspace/`
    /// **不被 gitignore 忽略**，写进去会让 `git status -uall` 变脏）。
    fn tmp_dir(tag: &str) -> PathBuf {
        let base = std::env::var("ONTOK_TEST_TMPDIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir().join("ontok-req062d"));
        let dir = base.join(tag);
        std::fs::create_dir_all(&dir).expect("建得出临时目录");
        dir
    }

    fn roles_out(out: &Path, baseline: &Path) -> Roles {
        Roles {
            out: SubjectPath::new(out),
            out_origin: "测试".to_string(),
            baseline: BaselinePath::new(baseline),
            baseline_origin: "测试".to_string(),
        }
    }

    /// 把裸夹具按**角色**装箱（REQ-063 · AC-001）。
    ///
    /// 刻意不让这层装箱替调用点决定谁是谁：第一个实参恒为登记基线，
    /// 角色由**调用点**显式写出来 —— 这正是 newtype 要固定下来的那件事。
    fn rc(base: SchemaReport, subj: SchemaReport) -> Reconcile {
        reconcile(&Baseline(base), &Subject(subj))
    }

    /// 把一份夹具写到磁盘（`load_both` / `same_file` 这类真读文件的用例用）。
    fn write_fixture(path: &Path, r: &SchemaReport) {
        std::fs::write(path, serde_json::to_string_pretty(r).expect("夹具能序列化"))
            .expect("夹具能落盘");
    }

    /// 体量下限全开的 [`Limits`]：合成夹具只有 3~4 张表，
    /// 够不着生产区间（80 表 / 900 列起）——
    /// 用生产下限会把失败原因盖成「体量越界」，判据就被噪声埋了。
    fn open_limits() -> Limits {
        Limits {
            min_tables: 0,
            max_tables: 1_000,
            min_columns: 0,
            max_columns: 100_000,
        }
    }

    // ── AC-006：两个角色不许解析到同一个文件 ──────────────────────────

    #[test]
    fn resolve_roles_accepts_two_distinct_files() {
        let dir = tmp_dir("distinct");
        let roles = resolve_roles(
            Some(dir.join("incoming.json").to_str().unwrap()),
            Some(dir.join("schema.json").to_str().unwrap()),
        )
        .expect("两个不同文件应当允许");
        assert_eq!(roles.out.as_path(), dir.join("incoming.json"));
        assert_eq!(roles.baseline.as_path(), dir.join("schema.json"));
    }

    /// M5：连**缺省值**都必须是两个不同的文件。
    ///
    /// 旧代码的 `--out` 缺省值**就是**登记基线，于是「什么都不传跑一次」
    /// 必然覆盖登记基线 —— 这正是 AC-000。
    /// 没有这条，`OUT_DEFAULT` 被改回 `BASELINE_DEFAULT` 时不会有任何测试变红。
    #[cfg(unix)] #[test]
    fn default_roles_are_two_different_files() {
        let roles = resolve_roles(None, None).expect("缺省值不该撞车");
        assert_eq!(roles.baseline.as_path(), Path::new(BASELINE_DEFAULT), "基线缺省＝门禁读的那份");
        assert_ne!(
            roles.out.as_path(),
            roles.baseline.as_path(),
            "缺省的落盘目标必须与登记基线分开"
        );
        assert!(!same_file(&roles.out, &roles.baseline));
    }

    /// M5：把落盘目标指回基线（模拟今天的行为）⇒ 必须报错退出。
    #[test]
    fn resolve_roles_rejects_out_equal_baseline() {
        let dir = tmp_dir("m5");
        let p = dir.join("schema.json");
        let err = resolve_roles(Some(p.to_str().unwrap()), Some(p.to_str().unwrap()))
            .expect_err("落盘目标＝基线必须报错");
        let msg = format!("{err:#}");
        assert!(msg.contains("解析到同一个文件"), "报错要说清撞车：{msg}");
    }

    /// M6：同一路径、**内容完全相同** ⇒ 仍必须报错。
    ///
    /// 这是 M5 最容易漏的变体：若实现写成「内容一样就放过」，
    /// 那么「还没现拉到东西」的时序下判据根本打不出问题。
    #[test]
    fn resolve_roles_rejects_same_file_even_when_contents_identical() {
        let dir = tmp_dir("m6");
        let p = dir.join("schema.json");
        let body = serde_json::to_string_pretty(&sample_baseline()).unwrap();
        std::fs::write(&p, &body).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), body, "前提：两份内容确实一样");

        let err = resolve_roles(Some(p.to_str().unwrap()), Some(p.to_str().unwrap()))
            .expect_err("内容相同也必须报错 —— 危害在路径相同，不在内容不同");
        assert!(
            format!("{err:#}").contains("解析到同一个文件"),
            "报错要说清撞车"
        );
    }

    /// 撞车判定必须经得起「相对路径 / `..` / 符号链接」的写法差异。
    #[cfg(unix)] #[test]
    fn resolve_roles_rejects_alias_of_baseline() {
        let dir = tmp_dir("alias");
        let real = dir.join("fix-props").join("schema.json");
        std::fs::create_dir_all(real.parent().unwrap()).unwrap();
        std::fs::write(&real, "{}").unwrap();
        let via_dotdot = dir.join("fix-props").join("..").join("fix-props").join("schema.json");
        let err = resolve_roles(Some(via_dotdot.to_str().unwrap()), Some(real.to_str().unwrap()))
            .expect_err("`..` 写法指向同一文件，必须报错");
        assert!(format!("{err:#}").contains("解析到同一个文件"));

        let link = dir.join("link.json");
        // 夹具必须可重复跑：上一轮留下的符号链接会让 symlink() 直接失败。
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let err = resolve_roles(Some(link.to_str().unwrap()), Some(real.to_str().unwrap()))
            .expect_err("符号链接指向同一文件，必须报错");
        assert!(format!("{err:#}").contains("解析到同一个文件"));
    }

    /// AC-003：`--out` 指向**登记基线的硬链接**（`ln` 而不是 `ln -s`）⇒ 必须报错。
    ///
    /// 补 `(st_dev, st_ino)` 之前这一对是**放行**的：
    /// `canonicalize` 只解符号链接，两个硬链接名各自 canonicalize 成自己。
    /// AC-006 写的是「二者解析到**同一个文件** ⇒ 拒绝执行」——
    /// POSIX 说硬链接就是同一个文件，所以这里**必须**拒绝。
    #[cfg(unix)] #[test]
    fn resolve_roles_rejects_hard_link_of_baseline() {
        let dir = tmp_dir("hardlink");
        let real = dir.join("schema.json");
        std::fs::write(&real, "{}").unwrap();
        let link = dir.join("hardlink-to-baseline.json");
        // 夹具必须可重复跑：上一轮留下的硬链接会让 hard_link() 直接失败。
        let _ = std::fs::remove_file(&link);
        std::fs::hard_link(&real, &link).unwrap();

        // 前提：两个名字路径不同、canonicalize 也不同 ——
        // ⇒ 挡住它的只能是 inode 比对，前面两层都拦不住（否则本条测不到点子上）。
        assert_ne!(link, real);
        assert_ne!(
            std::fs::canonicalize(&link).unwrap(),
            std::fs::canonicalize(&real).unwrap(),
            "前提不成立：canonicalize 已经把两个硬链接认成同一个路径，那就不必测 inode 这一层了"
        );
        use std::os::unix::fs::MetadataExt;
        assert_eq!(
            std::fs::metadata(&link).unwrap().ino(),
            std::fs::metadata(&real).unwrap().ino(),
            "前提不成立：两个名字不在同一个 inode 上，那就不是硬链接"
        );

        let err = resolve_roles(Some(link.to_str().unwrap()), Some(real.to_str().unwrap()))
            .expect_err("硬链接指向同一个文件，必须拒绝执行");
        assert!(
            format!("{err:#}").contains("解析到同一个文件"),
            "报错要说清撞车：{err:#}"
        );
    }

    /// AC-003 的**反向**：加了 inode 比对之后，两份**各是各的**文件仍必须放行。
    ///
    /// 挡住「同一个 inode」很容易，挡住「内容一样」则不行 ——
    /// 危害在同路径，不在同内容（AC-006）。这条钉住后者没被顺手做掉。
    #[test]
    fn same_content_two_distinct_files_are_still_allowed() {
        let dir = tmp_dir("same-content");
        let a = dir.join("a.json");
        let b = dir.join("b.json");
        let body = serde_json::to_string_pretty(&sample_baseline()).unwrap();
        std::fs::write(&a, &body).unwrap();
        std::fs::write(&b, &body).unwrap();

        let roles = resolve_roles(Some(a.to_str().unwrap()), Some(b.to_str().unwrap()))
            .expect("内容相同不是撞车 —— 危害在「同路径」，不在「同内容」");
        assert_eq!(roles.out.as_path(), a);
        assert_eq!(roles.baseline.as_path(), b);
    }

    // ── AC-002：四个形态分开点名 ───────────────────────────────────────

    /// 形态 a：表整个没了 ⇒ 阻断 + 点名。
    #[test]
    fn missing_table_blocks_and_names_it() {
        let base = sample_baseline();
        let mut subj = sample_baseline();
        subj.tables.retain(|t| t.name != "orders");
        subj.table_count = subj.tables.len();

        let rec = rc(base, subj);
        assert_eq!(rec.bad.len(), 1, "应只有 1 条阻断：{rec:?}");
        assert!(rec.bad[0].contains("orders"), "必须点名缺哪张表：{}", rec.bad[0]);
        assert!(rec.bad[0].contains("缺表"), "形态 a 要有自己的说法");
    }

    /// 形态 b：表在、缺列 ⇒ 阻断 + 点名「哪张表.哪列」。
    #[test]
    fn missing_column_blocks_and_names_table_dot_column() {
        let base = sample_baseline();
        let mut subj = sample_baseline();
        let t = subj.tables.iter_mut().find(|t| t.name == "orders").unwrap();
        t.columns.retain(|c| c.name != "amount");
        t.column_count = t.columns.len();

        let rec = rc(base, subj);
        assert_eq!(rec.bad.len(), 1, "应只有 1 条阻断：{rec:?}");
        assert!(rec.bad[0].contains("orders.amount"), "必须点名 表.列：{}", rec.bad[0]);
        assert!(rec.bad[0].contains("缺列"), "形态 b 要有自己的说法");
    }

    /// a 与 b **不许合并**：表整个没了，不能再把它 15 列逐条报成「缺列」。
    #[test]
    fn missing_table_is_not_also_reported_as_missing_columns() {
        let base = rep(
            vec![tbl(
                "orders",
                (0..15)
                    .map(|i| col(&format!("c{i}"), "text", Some("注")))
                    .collect(),
            )],
            "PostgreSQL 17.4",
        );
        let subj = rep(vec![], "PostgreSQL 17.4");
        let rec = rc(base, subj);
        assert_eq!(rec.bad.len(), 1, "1 张缺表就只该报 1 条，不能滚成 15 条缺列：{rec:?}");
        assert!(rec.bad[0].contains("缺表"));
    }

    /// 四种形态混在一起时，各自计数仍然分得开。
    #[test]
    fn four_shapes_are_counted_separately() {
        let base = rep(
            vec![
                tbl("gone", vec![col("a", "text", None)]),
                tbl("kept", vec![col("a", "text", None), col("b", "text", None)]),
            ],
            "PostgreSQL 17.4",
        );
        let subj = rep(
            vec![
                tbl("kept", vec![col("a", "integer", None), col("b", "text", None)]),
            ],
            "PostgreSQL 17.4",
        );
        let rec = rc(base, subj);
        assert_eq!(rec.bad.len(), 2, "1 缺表 + 1 类型变更：{rec:?}");
        assert_eq!(rec.bad.iter().filter(|m| m.contains("缺表")).count(), 1);
        assert_eq!(rec.bad.iter().filter(|m| m.contains("类型变了")).count(), 1);
        assert!(rec.bad.iter().any(|m| m.contains("kept.a") && m.contains("text → integer")));
    }

    /// 形态 c：类型变了 ⇒ 阻断 + 「A → B」。
    #[test]
    fn type_change_blocks_and_shows_both_types() {
        let base = sample_baseline();
        let mut subj = sample_baseline();
        let t = subj.tables.iter_mut().find(|t| t.name == "items").unwrap();
        t.columns[1].data_type = "varchar".to_string();

        let rec = rc(base, subj);
        assert_eq!(rec.bad.len(), 1, "{rec:?}");
        assert!(rec.bad[0].contains("items.sku"), "{}", rec.bad[0]);
        assert!(rec.bad[0].contains("text → varchar"), "要写出 A→B：{}", rec.bad[0]);
    }

    /// 形态 d：注释没了 ⇒ **只 warn**，不阻断。
    #[test]
    fn comment_loss_is_warn_only() {
        let base = sample_baseline();
        let mut subj = sample_baseline();
        let t = subj.tables.iter_mut().find(|t| t.name == "users").unwrap();
        t.columns[1].comment = None;
        t.commented_count = t.columns.iter().filter(|c| has_comment(c.comment.as_deref())).count();

        let rec = rc(base, subj);
        assert!(rec.bad.is_empty(), "注释没了不许阻断：{rec:?}");
        assert_eq!(rec.warn.len(), 1, "{rec:?}");
        assert!(rec.warn[0].contains("users.name"), "{}", rec.warn[0]);
    }

    /// 空白注释串与「没注释」同义（否则清一色补个空串就能躲过判据）。
    #[test]
    fn blank_comment_counts_as_missing_comment() {
        let base = sample_baseline();
        let mut subj = sample_baseline();
        let t = subj.tables.iter_mut().find(|t| t.name == "users").unwrap();
        t.columns[1].comment = Some("   ".to_string());
        t.commented_count = t.columns.iter().filter(|c| has_comment(c.comment.as_deref())).count();

        let rec = rc(base, subj);
        assert!(rec.bad.is_empty());
        assert!(rec.warn.iter().any(|m| m.contains("users.name")), "{rec:?}");
    }

    // ── AC-002 反方向：只 warn，绝不阻断（M3）──────────────────────────

    /// 形态 e：现拉多出表/视图 ⇒ 只 warn。
    #[test]
    fn extra_table_is_warn_only() {
        let base = sample_baseline();
        let mut subj = sample_baseline();
        subj.tables
            .push(tbl("llm_provider_back", vec![col("id", "uuid", None)]));
        subj.table_count = subj.tables.len();

        let rec = rc(base, subj);
        assert!(rec.bad.is_empty(), "源库合法新增表不许阻断：{rec:?}");
        assert!(
            rec.warn.iter().any(|m| m.contains("llm_provider_back")),
            "要点名：{rec:?}"
        );
    }

    /// 形态 f：现拉多出列 ⇒ 只 warn。
    #[test]
    fn extra_column_is_warn_only() {
        let base = sample_baseline();
        let mut subj = sample_baseline();
        let t = subj.tables.iter_mut().find(|t| t.name == "orders").unwrap();
        t.columns.push(col("channel_code", "text", None));
        t.column_count = t.columns.len();

        let rec = rc(base, subj);
        assert!(rec.bad.is_empty(), "源库合法新增列不许阻断：{rec:?}");
        assert!(rec.warn.iter().any(|m| m.contains("orders.channel_code")), "{rec:?}");
    }

    /// 形态 g：现拉多出注释 ⇒ **连 warn 都不报**。
    #[test]
    fn extra_comment_is_not_reported() {
        let base = sample_baseline();
        let mut subj = sample_baseline();
        let t = subj.tables.iter_mut().find(|t| t.name == "items").unwrap();
        t.columns[1].comment = Some("新加的注释".to_string());
        t.commented_count = t.columns.iter().filter(|c| has_comment(c.comment.as_deref())).count();

        let rec = rc(base, subj);
        assert!(rec.bad.is_empty() && rec.warn.is_empty(), "补注释是好事，不该出声：{rec:?}");
    }

    /// M3 端到端：现拉比基线**新**（多表多列多注释），退出码必须仍是 0。
    /// 「不该阻断的也阻断」不会被任何正向判据发现，只能这样单独钉住。
    #[test]
    fn ahead_of_baseline_still_passes() {
        let base = sample_baseline();
        let mut subj = sample_baseline();
        subj.tables
            .push(tbl("llm_provider_back", vec![col("id", "uuid", Some("备份"))]));
        subj.tables.push(tbl("_volc_dts_pg_ddl_abc", vec![col("id", "uuid", None)]));
        subj.table_count = subj.tables.len();
        let t = subj.tables.iter_mut().find(|t| t.name == "orders").unwrap();
        t.columns.push(col("channel_code", "text", Some("渠道")));
        t.column_count = t.columns.len();
        subj.column_count = subj.tables.iter().map(|t| t.columns.len()).sum();
        subj.commented_column_count = subj.tables.iter().map(|t| t.commented_count).sum();

        let (base, subj) = (Baseline(base), Subject(subj));
        let rec = reconcile(&base, &subj);
        let roles = roles_out(Path::new("/tmp/subject.json"), Path::new("/tmp/baseline.json"));
        finish(&roles, &subj, &base, "测试", &[], &rec)
            .expect("源库领先于基线是合法演进，绝不可阻断");
    }

    /// 完全一致 ⇒ 零发现（防止有人把「对账」写成比表数/比大小）。
    #[test]
    fn identical_reports_produce_no_findings() {
        let r = sample_baseline();
        let rec = rc(r.clone(), r);
        assert!(rec.bad.is_empty() && rec.warn.is_empty(), "{rec:?}");
    }

    /// 反过来也不能只比计数：列全换一遍、表名一样，必须被点名。
    #[test]
    fn same_counts_but_different_columns_is_still_caught() {
        let base = sample_baseline();
        let mut subj = sample_baseline();
        for t in subj.tables.iter_mut() {
            for c in t.columns.iter_mut() {
                c.name = format!("x_{}", c.name);
            }
        }
        let rec = rc(base, subj);
        assert_eq!(rec.bad.len(), 6, "3 表 × 2 列 = 6 条缺列，一对一：{rec:?}");
        assert!(rec.bad.iter().all(|m| m.contains("缺列")));
    }

    // ── AC-003：server_version 只比主版本 ──────────────────────────────

    #[test]
    fn server_version_patch_bump_passes() {
        let base = sample_baseline();
        let subj = rep(base.tables.clone(), "PostgreSQL 17.4.1 on x86_64-pc-linux-gnu");
        let rec = rc(base, subj);
        assert!(rec.bad.is_empty(), "17.4 → 17.4.1 是补丁更新，不该阻断：{rec:?}");
    }

    #[test]
    fn server_version_major_mismatch_blocks() {
        let base = sample_baseline();
        let subj = rep(
            base.tables.clone(),
            "PostgreSQL 16.14 (Debian 16.14-1.pgdg13+1) on x86_64-pc-linux-gnu",
        );
        let rec = rc(base, subj);
        assert_eq!(rec.bad.len(), 1, "{rec:?}");
        assert!(rec.bad[0].contains("主版本"), "{}", rec.bad[0]);
    }

    #[test]
    fn server_version_major_comparison_survives_vendor_suffix() {
        // 实测登记基线与活库的两种写法，解析出来的都该是主版本。
        assert_eq!(major_version("PostgreSQL 17.4 on x86_64-pc-linux-gnu"), Some(17));
        assert_eq!(major_version("PostgreSQL 16.14 (Debian 16.14-1.pgdg13+1)"), Some(16));
        assert_eq!(major_version("PostgreSQL 9.6.24 on x86_64"), Some(9));
        assert_eq!(major_version("unknown"), None);
    }

    #[test]
    fn server_version_unreadable_fails_closed() {
        let base = sample_baseline();
        let subj = rep(base.tables.clone(), "unknown");
        let rec = rc(base, subj);
        assert_eq!(rec.bad.len(), 1, "读不出主版本不能当「没问题」：{rec:?}");
        assert!(rec.bad[0].contains("fail-closed"), "{}", rec.bad[0]);
    }

    // ── AC-005：退出码与点名 ────────────────────────────────────────────

    #[test]
    fn finish_fails_when_reconcile_has_bad_findings() {
        let base = sample_baseline();
        let mut subj = sample_baseline();
        subj.tables.retain(|t| t.name != "users");
        subj.table_count = subj.tables.len();
        let (base, subj) = (Baseline(base), Subject(subj));
        let rec = reconcile(&base, &subj);
        let roles = roles_out(Path::new("/tmp/subject.json"), Path::new("/tmp/baseline.json"));
        let err = finish(&roles, &subj, &base, "测试", &[], &rec)
            .expect_err("对账失败必须非零退出，而不是 warn 后仍 exit 0");
        let msg = format!("{err:#}");
        assert!(msg.contains("users"), "报错必须点名具体对象：{msg}");
    }

    // ── 实测样本对账（2026-10-10 · VPN 实例 vs 登记基线）────────────────
    //
    // 这两条样本**不在版本控制内**（`.review-workspace/` 被 gitignore），
    // 所以新克隆里必然缺失。旧写法在缺失时只说明原因、不断言 ——
    // 那就是**零断言却记 passed**，让通过计数虚高（REQ-063 · AC-002 / F1）。
    //
    // 现在分工明确：**合成夹具那条永远真跑**（承重的判据），
    // 实测那条缺样本时**判红**，而不是假装通过。
    // ⚠️ **运行前提**（REQ-066 · AC-002，完整版在文件头的模块文档里）：
    // 下面这两份是**客户源库的脱敏 schema**，**不在版本控制内**
    // （`.gitignore` 的 `/*` 覆盖它们）⇒ **源库信息不得入库**。
    //
    // 缺省值 = 本机既有绝对路径，**一个字都没改** ⇒ 本机零影响；
    // 别的机器 / CI 用环境变量指向一份**已授权的脱敏副本**（见下）。
    //
    // 覆盖口照本仓既有约定（`tests/drop_rate.rs:68`）：**先读环境变量、
    // 没设才回落缺省**。现拉那份没有既有约定，按同族命名取 `ONTOK_LIVE_SCHEMA_JSON`
    // —— 与 `ONTOK_SCHEMA_JSON` 同为「哪份 schema JSON」的读法，不自创风格。
    const REAL_BASELINE_DEFAULT: &str =
        "/home/myg133/Projects/Anchnet/Ontology/.review-workspace/fix-props/schema.json";
    const REAL_LIVE_DEFAULT: &str =
        "/home/myg133/Projects/Anchnet/Ontology/.review-workspace/req062d/live-pulled.json";
    const REAL_LIVE_ENV: &str = "ONTOK_LIVE_SCHEMA_JSON";

    /// 取实测样本路径：**环境变量优先，没设才回落缺省**。
    ///
    /// [`SCHEMA_JSON_ENV`] 复用门禁自己的那个常量、不另起名字 ——
    /// 它指的本来就是「两个门禁实际会读的那份登记基线」，同一个值才对得上。
    fn real_sample_path(env: &str, default: &str) -> String {
        resolve_sample_path(std::env::var(env).ok(), default)
    }

    /// 解析逻辑单独抽出来，**不碰进程环境** ⇒ 测试里没有并行干扰。
    fn resolve_sample_path(from_env: Option<String>, default: &str) -> String {
        from_env.unwrap_or_else(|| default.to_string())
    }

    #[test]
    fn sample_path_override_reads_env_first_and_keeps_the_default_verbatim() {
        // ① 解析规则本身（纯函数，**不碰进程环境** ⇒ 测试并行不会互相干扰）。
        assert_eq!(resolve_sample_path(None, REAL_LIVE_DEFAULT), REAL_LIVE_DEFAULT);
        assert_eq!(resolve_sample_path(Some("/tmp/a.json".into()), REAL_LIVE_DEFAULT), "/tmp/a.json");
        // ② 进程里真设了就用它、没设就回落缺省 —— **两种环境下本条都成立**，
        //    所以「设了变量跑」和「不设跑」守的是同一条口径。
        //    `unchanged` 是**今天**的缺省值：只在「没设变量、缺省真的是生效值」
        //    时才断言它 —— 设了变量时缺省本就没在生效路径上，不该拿它说事
        //    （否则「覆盖口生效」这件事反而会被缺省的变化掩盖掉）。
        let pairs = [
            (SCHEMA_JSON_ENV, REAL_BASELINE_DEFAULT, "/home/myg133/Projects/Anchnet/Ontology/.review-workspace/fix-props/schema.json"),
            (REAL_LIVE_ENV, REAL_LIVE_DEFAULT, "/home/myg133/Projects/Anchnet/Ontology/.review-workspace/req062d/live-pulled.json"),
        ];
        for (env, default, unchanged) in pairs {
            match std::env::var(env) {
                Ok(v) => assert_eq!(real_sample_path(env, default), v, "{env} 设了却没被读"),
                Err(_) => {
                    assert_eq!(real_sample_path(env, default), default, "{env} 没设却没用缺省");
                    assert_eq!(default, unchanged, "{env} 没设 ⇒ 缺省必须与今天逐字节一致");
                }
            }
        }
        // 覆盖口接在**真正的读取路径**上，不是加了个没人调的口子：
        // 两份样本都必须写成「环境变量优先 + 缺省回落」这一对。
        let src = include_str!("ontok-gate-preflight.rs");
        let body = unique_fn_body(src, "real_pair_five_missing_columns_are_named");
        for wiring in [
            "real_sample_path(SCHEMA_JSON_ENV, REAL_BASELINE_DEFAULT)",
            "real_sample_path(REAL_LIVE_ENV, REAL_LIVE_DEFAULT)",
        ] {
            assert!(
                body.contains(wiring),
                "real_pair_five_missing_columns_are_named 必须写成 `{wiring}` —— \
                 直连缺省常量就等于把覆盖口架空：{body}"
            );
        }
    }

    #[cfg(unix)] #[test]
    fn real_pair_five_missing_columns_are_named() {
        // ⚠️ 本条**不再软跳过**（REQ-063 · AC-002）。
        //
        // ⛔ 不用 `#[ignore]`（既有纪律：任何测试不许删、不许 ignore）。
        // 改成把「样本缺失」本身**判成红**：缺样本 ⇒ 这条红，
        // 而不是一条零断言的绿灯 —— 计数因此是诚实的。
        //
        // 这与本仓 fail-closed 的既有口径一致：
        // `tests/drop_rate.rs` 与 `tests/test_fk_naming.rs` 读不到
        // `schema.json` 时同样判失败（见本文件头）。
        //
        // 路径**先过覆盖口**（REQ-066 · AC-002）：没设环境变量就是本机既有绝对路径。
        let baseline = real_sample_path(SCHEMA_JSON_ENV, REAL_BASELINE_DEFAULT);
        let live = real_sample_path(REAL_LIVE_ENV, REAL_LIVE_DEFAULT);
        assert!(
            Path::new(&baseline).exists() && Path::new(&live).exists(),
            "实测样本缺失 —— 本条**不能**因此记通过：\n  \
             基线 {baseline}（覆盖口：环境变量 {SCHEMA_JSON_ENV}）\n  \
             现拉 {live}（覆盖口：环境变量 {REAL_LIVE_ENV}）\n  \
             （两份都不在版本控制内；同形状的合成夹具版见 \
             synthetic_pair_reproduces_the_real_drift_claims）"
        );
        let base: SchemaReport =
            serde_json::from_str(&std::fs::read_to_string(&baseline).unwrap()).unwrap();
        let subj: SchemaReport =
            serde_json::from_str(&std::fs::read_to_string(&live).unwrap()).unwrap();

        let rec = rc(base, subj);
        // 基线里的表**一张都没少**（91/91 全中）⇒ 形态 a 必然是 0 条。
        // 这条断言同时否掉 AC-004 里「M1 必须报缺 5 张表」的写法：
        // 那 5 张表是**现拉多出来的**，按 AC-001/AC-002 只能 warn。
        assert_eq!(rec.bad.iter().filter(|m| m.contains("缺表")).count(), 0, "{rec:?}");
        for c in [
            "deductions.model_id",
            "deductions.provider",
            "package_templates.sale_limit",
            "service_config_peak.status",
            "service_config_peak.weekdays",
        ] {
            assert!(
                rec.bad.iter().any(|m| m.contains(c)),
                "必须点名少掉的列 {c}：{rec:?}"
            );
        }
        assert!(
            rec.bad.iter().any(|m| m.contains("主版本")),
            "17.4 vs 16.14 必须被报出来：{rec:?}"
        );
        // 现拉多出的 5 张表 + 65 列：只 warn。
        for t in ["_volc_dts_pg_ddl_18e4b12d3d9d40aa81853a72d73fcd3c", "llm_provider_back"] {
            assert!(rec.warn.iter().any(|m| m.contains(t)), "多出的表要点名 {t}：{rec:?}");
        }
    }
    /// AC-002（选 a）：**随仓库走的合成夹具**上，断言与实测样本**同等强度**的结论。
    ///
    /// # 为什么必须有这一条
    ///
    /// 实测样本不在版本控制内 ⇒ 「只有实测那一条能证这个需求」的写法，
    /// 换个新克隆就变成零断言的绿灯。本条**一个字节都不碰文件系统**，
    /// 任何环境下都真跑，而且断言逐条对应实测那一条的每一条结论。
    ///
    /// # 形状照抄实测那一对
    ///
    /// 基线 3 张表一张不少（⇒ 缺表必然 0 条）、5 个列少在现拉、
    /// 主版本 17.4 vs 16.14、现拉多出 2 张表和 1 列。
    /// 其中「表一张不少」这条同时否掉 AC-004 里「M1 必须报缺 5 张表」的写法。
    #[test]
    fn synthetic_pair_reproduces_the_real_drift_claims() {
        let base = rep(
            vec![
                tbl(
                    "deductions",
                    vec![
                        col("id", "uuid", Some("主键")),
                        col("model_id", "uuid", Some("模型")),
                    ],
                ),
                tbl(
                    "package_templates",
                    vec![
                        col("id", "uuid", Some("主键")),
                        col("sale_limit", "integer", Some("限购")),
                    ],
                ),
                tbl(
                    "service_config_peak",
                    vec![
                        col("id", "uuid", Some("主键")),
                        col("status", "integer", Some("状态")),
                        col("weekdays", "text", Some("星期")),
                    ],
                ),
            ],
            "PostgreSQL 17.4 on x86_64-pc-linux-gnu",
        );

        let mut subj = base.clone();
        subj.server_version = "PostgreSQL 16.14 on x86_64-pc-linux-gnu".to_string();
        for (tname, dropped) in [
            ("deductions", vec!["model_id"]),
            ("package_templates", vec!["sale_limit"]),
            ("service_config_peak", vec!["status", "weekdays"]),
        ] {
            let t = subj.tables.iter_mut().find(|t| t.name == tname).unwrap();
            t.columns.retain(|c| !dropped.contains(&c.name.as_str()));
            t.column_count = t.columns.len();
        }
        subj.tables.push(tbl("llm_provider_back", vec![col("id", "uuid", None)]));
        subj
            .tables
            .push(tbl("_volc_dts_pg_ddl_18e4b12d", vec![col("id", "uuid", None)]));
        subj.table_count = subj.tables.len();
        let t = subj.tables.iter_mut().find(|t| t.name == "deductions").unwrap();
        t.columns.push(col("channel_code", "text", None));
        t.column_count = t.columns.len();
        subj.column_count = subj.tables.iter().map(|x| x.columns.len()).sum();
        subj.commented_column_count = subj.tables.iter().map(|x| x.commented_count).sum();

        let rec = rc(base, subj);
        assert_eq!(
            rec.bad.iter().filter(|m| m.contains("缺表")).count(),
            0,
            "基线里的表一张都没少 ⇒ 缺表必须 0 条（反向也成立：多出的表只 warn）：{rec:?}"
        );
        for c in [
            "deductions.model_id",
            "package_templates.sale_limit",
            "service_config_peak.status",
            "service_config_peak.weekdays",
        ] {
            assert!(
                rec.bad.iter().any(|m| m.contains(c)),
                "必须点名少掉的列 {c} —— 这正是 M2（注掉 b 类判据）唯一能打中的路径：{rec:?}"
            );
        }
        assert!(
            rec.bad.iter().any(|m| m.contains("主版本")),
            "17.4 vs 16.14 必须被报出来：{rec:?}"
        );
        for t in ["_volc_dts_pg_ddl_18e4b12d", "llm_provider_back"] {
            assert!(rec.warn.iter().any(|m| m.contains(t)), "多出的表要点名 {t}：{rec:?}");
        }
        assert!(
            rec.warn.iter().any(|m| m.contains("deductions.channel_code")),
            "多出的列要点名：{rec:?}"
        );
    }

    /// 防「软跳过复活」（AC-002）。
    ///
    /// ⚠️ **能力有限，如实说明**：它只认「那一句标记文案」，
    /// 换一种措辞重写软跳过它抓不到。它防的是**同一处**的回归立刻变红，
    /// 绝不是「软跳过不可能再回来」的保证 ——
    /// 真正承重的是 `synthetic_pair_reproduces_the_real_drift_claims`
    /// （不依赖任何文件，永远真跑）与 `real_pair_*` 的前提断言。
    ///
    /// 探针**必须用片段拼**：这里若直接写出一个连着的字面量，
    /// 它自己就在本文件里，`matches(..)` 会把自己算进去 ⇒ 恒真。
    ///
    /// 形状是**集合相等**而不是「计数 == 0」（REQ-066 · AC-001）：
    /// 期望清单是**空集** —— 软跳过一个都不许有。
    /// 判据自己被换成恒真的事，由
    /// [`structural_criteria_cannot_be_swapped_for_a_tautology`] 盯着。
    #[test]
    fn real_pair_has_no_soft_skip_left() {
        let src = include_str!("ontok-gate-preflight.rs");
        assert!(
            no_soft_skip_match(&matched_lines(src, &real_soft_skip_marker())),
            "软跳过（一条断言都没跑却记 passed）复活了 —— \
             样本缺失时本条必须**判红**，见 real_pair_five_missing_columns_are_named 的前提断言"
        );
    }

    // ── REQ-063 · AC-005：角色流向（newtype 之外的那一半）───────────────
    //
    // newtype 让「两条加载语句角色对调」**编译不过**（M8 不可构造）。
    // 但编译期只封住了**写法**；内容层面仍要有东西钉着 ——
    // 所以这里再钉一条**运行期**的：真的造两个内容**明显不同**的文件走一遍
    // [`load_both`]，断言回来的 `Baseline` 里装的是**基线那份**。
    //
    // 为什么不能靠方案 A：它钉的是**调用点文本**，管不到
    // 「名字没变、数据换了角色」；方案 D（子进程集成测试）本条明确不做。
    // 于是剩下唯一能落到运行期的就是「真的读一遍，看回来的是不是那一份」。
    #[test]
    fn load_both_roles_keep_their_own_file() {
        let dir = tmp_dir("load-both");
        let bp = dir.join("baseline.json");
        let sp = dir.join("incoming.json");

        // 登记基线：3 张表、主版本 17.4。
        let base = sample_baseline();
        write_fixture(&bp, &base);

        // 待审那份：4 张表（多一张备份表）、多一列、主版本 16.14。
        let mut ahead = base.clone();
        ahead
            .tables
            .push(tbl("llm_provider_back", vec![col("id", "uuid", Some("备份"))]));
        ahead.table_count = ahead.tables.len();
        let t = ahead.tables.iter_mut().find(|t| t.name == "orders").unwrap();
        t.columns.push(col("channel_code", "text", Some("渠道")));
        t.column_count = t.columns.len();
        ahead.column_count = ahead.tables.iter().map(|x| x.columns.len()).sum();
        ahead.commented_column_count = ahead.tables.iter().map(|x| x.commented_count).sum();
        let ahead = rep(ahead.tables, "PostgreSQL 16.14 on x86_64-pc-linux-gnu");
        write_fixture(&sp, &ahead);

        // 前提：两份文件的**可辨特征**真的不同（否则下面的断言是恒真）。
        assert_ne!(base.server_version, ahead.server_version, "前提：两份要能一眼认出谁是谁");
        assert_ne!(base.table_count, ahead.table_count, "前提：两份要能一眼认出谁是谁");

        let roles = roles_out(&sp, &bp);
        let (got_base, got_subj, _warn) = load_both(&roles, &open_limits(), "待审")
            .expect("两份都是合法 schema，读得回来");

        assert_eq!(
            got_base.server_version, base.server_version,
            "Baseline 位必须装着**登记基线那份**（17.4）—— 装成待审那份就是 M8"
        );
        assert_eq!(got_base.table_count, 3, "Baseline 位是 3 张表那份");
        assert_eq!(
            got_subj.server_version, ahead.server_version,
            "Subject 位必须装着**待审那份**（16.14）"
        );
        assert_eq!(got_subj.table_count, 4, "Subject 位是 4 张表那份");

        // ── 场景 ②：**两个加载器的不对称**才是「谁读了谁」的可观测后果 ──
        //
        // 场景 ① 只在两份文件都合法时成立 —— 此时无论用哪个加载器读，
        // 拿到的内容都一样 ⇒ 「对调了但结果一样」是**等价变异**，抓不到。
        //
        // 两个加载器唯一的**可观测**差别是 [`validate`]：待审那份要过校验，
        // 登记基线**只解析、不套体量区间**（[`load_baseline`] 的既有纪律，
        // 「拿区间去卡一份历史登记事实」要么误伤、要么变成改基线的后门）。
        //
        // 于是把登记基线做成「解析得动、但 validate 会判红」的那份：
        //   正确实现 ⇒ load_baseline 只解析 ⇒ 成功；
        //   基线位错走 load_and_verify ⇒ 校验不过 ⇒ 报错 ⇒ 本条变红。
        let mut unread_only_off = base.clone();
        unread_only_off.transaction_read_only = "off".to_string();
        let bp_off = dir.join("baseline-readonly-off.json");
        write_fixture(&bp_off, &unread_only_off);
        // 前提：这份基线**确实**过不了 validate（否则场景 ② 是恒真）。
        let findings = validate(&unread_only_off, &open_limits());
        assert!(
            !findings.bad.is_empty(),
            "前提不成立：这份基线竟然通过了 validate ⇒ 场景 ② 抓不到任何东西：{:?}", findings.bad
        );

        let roles_off = roles_out(&sp, &bp_off);
        let (got_base_off, _got_subj_off, _w) =
            load_both(&roles_off, &open_limits(), "待审")
                .expect("登记基线只要求「能被解析成 SchemaReport」，**不套体量区间** —— \
                         若这里报错，说明基线位走了 load_and_verify，那正是 M8");
        assert_eq!(
            got_base_off.transaction_read_only, "off",
            "基线位确实装的是那份「只读留痕不对」的登记基线"
        );
    }

    // ── REQ-063 · AC-004：文案 ↔ 实现真正认的东西，必须钉在一起 ──────────

    /// 从一段**运行期文案**里抠出所有 `ONTOK_…` 环境变量名（去重、保序）。
    ///
    /// 抠的是**运行期字符串**而不是源码：变量名在文案里是插值出来的
    /// （`{SCHEMA_JSON_ENV}`），源码里 grep 到的只是那个标识符本身。
    fn env_vars_in(text: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let b = text.as_bytes();
        let mut i = 0;
        while i < b.len() {
            // ⚠️ 必须先落在字符边界上：文案是中文，`b[i]` 命中不代表能切片。
            if text.is_char_boundary(i) && text[i..].starts_with("ONTOK_") {
                let mut j = i + "ONTOK_".len();
                while j < b.len() && (b[j] == b'_' || b[j].is_ascii_alphanumeric()) {
                    j += 1;
                }
                let name = text[i..j].to_string();
                if !out.contains(&name) {
                    out.push(name);
                }
                i = j;
            } else {
                i += 1;
            }
        }
        out
    }

    /// 从一段**运行期文案**里抠出**本命令自己**的 flag。
    ///
    /// ⚠️ 两条边界，缺一条就假阳性：
    ///
    /// 1. 只认 `--long-flag` 与**单个**字母的 `-h`。不设这条的话，
    ///    `REQ-048` 里的 `-048` 会被当成 flag。
    /// 2. `cargo ...` 那几行只取 `--` 分隔符**之后**的部分。
    ///    `cargo run --bin x -- --verify-only` 里 `--bin` 是 **cargo 的**参数，
    ///    本命令的承诺只有 `--` 之后那些。
    fn flags_in(text: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for line in text.lines() {
            let scan_from = if line.contains("cargo ") {
                match line.rfind(" -- ") {
                    Some(i) => i + 4,
                    None => continue,
                }
            } else {
                0
            };
            let b = line.as_bytes();
            let mut i = scan_from;
            while i < b.len() {
                let long = b[i] == b'-'
                    && i + 1 < b.len()
                    && b[i + 1] == b'-'
                    && i + 2 < b.len()
                    && b[i + 2].is_ascii_alphanumeric();
                // 单字母：`-h` 之后必须**不是**字母数字也不是 `-`。
                let short = b[i] == b'-'
                    && i + 1 < b.len()
                    && b[i + 1].is_ascii_alphabetic()
                    && (i + 2 >= b.len()
                        || !(b[i + 2].is_ascii_alphanumeric() || b[i + 2] == b'-'));
                if long || short {
                    let mut j = i;
                    while j < b.len() && (b[j] == b'-' || b[j].is_ascii_alphanumeric()) {
                        j += 1;
                    }
                    let f = line[i..j].to_string();
                    if !out.contains(&f) {
                        out.push(f);
                    }
                    i = j;
                } else {
                    i += 1;
                }
            }
        }
        out
    }

    /// 受检的四段文案 —— 全部是纯函数，不吃 cwd、不碰文件系统。
    ///
    /// ⚠️ 覆盖面如实写明：这四段就是**会给出补救建议**的全部地方
    /// （help、报错补救、缺省分歧提示、变量说明）。
    /// `main` 里那几条内联 `bail!` 只按构造走 `guidance_hint()`；
    /// 唯一例外是「源库连接串缺失」那条顺带提了一嘴 `ONTOK_SCHEMA_JSON`，
    /// 它在那里已经带了同一句免责（见 `SCHEMA_JSON_DISCLAIMER`）。
    fn user_facing_texts() -> Vec<String> {
        vec![
            help_text(),
            guidance_hint(Path::new("/tmp/subject.json"), Path::new("/tmp/baseline.json")),
            baseline_divergence_hint(
                Path::new("/tmp/rel/schema.json"),
                Path::new("/tmp/abs/schema.json"),
            ),
            schema_json_note(),
        ]
    }

    /// AC-004：**文案里出现的环境变量名**，不许与「实现真正接受的入口」脱节。
    ///
    /// 这条文案腐烂过一次（原 `:357` 建议「设 `ONTOK_SCHEMA_JSON` 指向你要的那个文件」），
    /// 而 REQ-062 之后设了它对本命令的基线**毫无影响** ——
    /// 照做的人只会白折腾，还以为已经生效了。
    ///
    /// ⛔ 恒真入口：把 `|| t.contains(SCHEMA_JSON_DISCLAIMER)` 拆掉、
    /// 或改成只问「文案里有没有这个变量名」，本条立刻变成恒真 ——
    /// 那恰好是它要防的那件事的反面。见断言消息。
    #[test]
    fn remedy_text_never_offers_an_env_var_the_command_does_not_read() {
        for t in user_facing_texts() {
            for v in env_vars_in(&t) {
                // 本命令**拿来定角色**的环境变量只有源库连接串。
                // 其余每一个只要出现在文案里，就必须带着免责句。
                assert!(
                    v == SOURCE_DATABASE_URL_ENV || t.contains(SCHEMA_JSON_DISCLAIMER),
                    "文案提到了环境变量 {v}，但这段文案既没说它参与角色解析，\
                     也没带免责句「{SCHEMA_JSON_DISCLAIMER}」。\n\
                     ⛔ 恒真入口：不许把这条放宽成「只检查文案里有没有这个变量名」——\
                     那恰好是本条要防的反面。\n文案：\n{t}"
                );
            }
        }
        // 前提自检：这四段文案里**真的**提到了那个变量名（否则上面整条判据是空的）。
        let all: String = user_facing_texts().concat();
        assert!(
            env_vars_in(&all).contains(&SCHEMA_JSON_ENV.to_string()),
            "前提不成立：四段文案里一个 ONTOK_ 变量都没提到 ⇒ 本条判据恒真"
        );
    }

    /// AC-004：**文案里出现的 flag**，必须真被 `parse_args` 收下。
    ///
    /// 钉的是「文案承诺的东西必须存在」。判据靠**真喂解析器**，
    /// 不是 grep 白名单 —— [`ACCEPTED_ARGS`] 与解析器互相锁：
    /// 解析器改名 ⇒ 本条红；白名单漏登记 ⇒ 本条红；文案写了个不存在的 flag ⇒ 本条红。
    fn parse_args_accepts(flag: &str) -> bool {
        let owned: Vec<String> = match flag {
            "--out" | "--baseline" | "--min-tables" | "--max-tables" | "--min-columns"
            | "--max-columns" => vec![flag.to_string(), "1".to_string()],
            other => vec![other.to_string()],
        };
        parse_args(owned.into_iter()).is_ok()
    }

    #[test]
    fn every_flag_in_the_messages_is_actually_accepted() {
        for t in user_facing_texts() {
            for f in flags_in(&t) {
                assert!(
                    parse_args_accepts(&f),
                    "文案提到 {f}，但 parse_args 不收它 —— 文案在承诺一个不存在的参数"
                );
                assert!(
                    ACCEPTED_ARGS.contains(&f.as_str()),
                    "文案提到 {f}，它不在 ACCEPTED_ARGS 里 —— 白名单与文案各说各话"
                );
            }
        }
        // 反向：解析器收下的每个 flag 都得在白名单里登记（改名/新增不许漏）。
        for f in ACCEPTED_ARGS {
            assert!(
                parse_args_accepts(f),
                "ACCEPTED_ARGS 列了 {f}，parse_args 却收不下 —— 白名单与解析器各说各话"
            );
        }
        // 前提自检：文案里**真的**提到了 flag（否则上面整条判据是空的）。
        assert!(
            flags_in(&help_text()).contains(&"--baseline".to_string()),
            "前提不成立：help 里连 --baseline 都没有 ⇒ 本条判据恒真"
        );
    }

    // ── AC-001（REQ-066）：M7 的判据形状换成**集合相等** ──────────────
    //
    // `fn reconcile(base: &SchemaReport, subj: &SchemaReport)` 两个参数同类型，
    // 类型系统天然区分不了谁是谁。把 `main` 里两处调用的参数**对调**，
    // 编译照过、语义整个反过来（拿基线当现拉结果去比），
    // 而上面每一条测试都自建夹具直调 `reconcile`，**根本不经过那两行** ⇒ 全绿。
    //
    // 所以这条判据**不测行为、只测结构**：`include_str!` 读回本文件源码，
    // 断言那两个调用点存在、且方向是对的。
    //
    // # 为什么必须是「集合相等」，不是「计数 == 1」（REQ-066 · AC-001）
    //
    // 上一轮的形状是 `assert_eq!(src.matches(pat).count(), 1)`。
    // 它**有放宽轴**：`>= 0` / `>= 1` / `!= 1` 都是**一行合法改动**，
    // 而 `1` 读起来只是个「计数」，把它换成恒真**看不出哪里变了**。
    // QA 实测：四条全改 ⇒ 33/0 **全绿**；**只改其中一条** ⇒ 同样全绿 ⇒ 洞没堵上。
    //
    // 旧形状里「基线在前参数位」这件事藏在**算子**后面；现在它被摊成
    // [`EXPECTED_RECONCILE_SITES`] 里的**一项**。放宽它必须**动那张表** ——
    // 给一张清单**加一行**，diff 里一眼可见，而且第二层判据会当场变红。
    //
    // ⚠️ **诚实的边界**（别把这条读成「恒真不可能再进来」）：
    // 判据自己当然也能被整条替换掉 —— 任何判据都能。
    // 真正封住那个方向的是**第二层**：
    // [`structural_criteria_cannot_be_swapped_for_a_tautology`] 要求这些判据
    // **必须**引用自己的期望清单、且**不得**出现任何放宽轴算子。
    // 逃逸的代价从「换一个比较符」升到「重写函数体 + 删掉常量引用」——
    // 那是**看得见**的改动，不再是一行。
    const EXPECTED_RECONCILE_SITES: &[&str] = &["&base, &report", "&base, &on_disk"];

    /// 软跳过期望清单：**空集** —— 一个都不许有（`real_pair_has_no_soft_skip_left`）。
    const EXPECTED_SOFT_SKIP_SITES: &[&str] = &[];

    /// **闸门（seam）**：判据与期望清单之间**唯一**的通道（REQ-066 · QA · BLK-1/5）。
    ///
    /// # 为什么是 `-> bool` 而不是把清单交出去给判据比
    ///
    /// 上一版把 `EXPECTED_RECONCILE_SITES` **交给判据自己比**，
    /// 于是「只比 `.len()`」「只比 `[0]`」都是**合法且能编译**的写法 ——
    /// QA 实测两条都 35/0 全绿，**叠加真实的 M7 参数对调也照样全绿**。
    /// 病根不在黑名单漏了哪个算子，而在**清单是可以被拆开比**的：
    /// 清单一旦可拆，「比全量」和「比一项」在类型上就长得一模一样。
    ///
    /// 现在清单**只存在于闸门内部**，对外只暴露一个 `bool`：
    /// 调用方能做的**只有**把**整个** `found` 交进去。
    /// 「只比一项」不再是「另一种写法」，而是**算错了值** ⇒ 运行时判红。
    ///
    /// ⚠️ 闸门自己被改写（`==` 改 `len()`）仍可能。
    /// 那由 [`seams_reject_known_wrong_inputs`] 用**执行**堵住，
    /// 不是靠再读一遍闸门的源码 —— 那样就回到「读文本」的老路上了。
    fn call_sites_match(found: &[String]) -> bool {
        found == EXPECTED_RECONCILE_SITES
    }

    /// [`call_sites_match`] 的软跳过版（期望是空集）。
    fn no_soft_skip_match(found: &[&str]) -> bool {
        found == EXPECTED_SOFT_SKIP_SITES
    }

    /// 软跳过标记串**由闸门产出**，判据不自己拿字面量（QA · BLK-2）。
    ///
    /// 病根同上：标记串是判据的**输入**，而上一版输入就写在判据体内 ——
    /// 改成任何一个不存在的串，软跳过真的复活了也查不出来（1 行、全绿）。
    fn real_soft_skip_marker() -> String {
        ["eprintln!(\n                \"跳过：实测样本不在", "（不在版本控制内"].concat()
    }

    /// 断言消息（把四条 `MSG_*` 收拢，判据体内因此只剩一句 `assert!`）。
    fn call_sites_failure_message() -> String {
        [MSG_FWD_REPORT, MSG_FWD_ON_DISK, MSG_REV_REPORT, MSG_REV_ON_DISK].join("\n")
    }

    /// 「放宽轴」清单：结构判据里**不得**出现它们（白名单之外的兜底）。
    ///
    /// ⚠️ 这一层是**兜底，不是主力**。上一版把它当主力，结果被
    /// 「不在名单上」的写法整类绕过（QA · BLK-1/2/3/4/5）。
    /// 主力是闸门：清单不可拆分 ⇒ 「弱化」不再是写法问题而是算错。
    const RELAXATION_AXES: &[&str] = &[
        "||", "&&", "!=", ">=", "<=", ">", ".contains(", ".any(", ".all(", ".is_empty()",
        "[..", "= true", ".unwrap_or(", ".unwrap(true",
    ];

    /// 从本文件源码里抄出 `fn <name>` 的函数体（**不含**签名行）。
    ///
    /// 收尾认的是**4 空格缩进的 `}`**：本仓 `rustfmt` 保证模块内 `fn`
    /// 一律收在 4 空格，嵌套块更深 ⇒ 不会把下一个函数一起吞进来。
    ///
    /// ⚠️ **必须唯一**（QA · BLK-4）：上一版取**首个**匹配，
    /// 于是放一个同名诱饵函数在前面，这条探针就去看诱饵了（4 行、全绿）。
    /// 现在出现次数 ≠ 1 一律 panic ⇒ 盯着它的那条测试变红。
    ///
    /// 判据被整条删掉时同样 panic（不是静静放过）。
    fn unique_fn_body<'a>(src: &'a str, name: &str) -> &'a str {
        let needle = format!("fn {name}(");
        let hits: Vec<usize> = src.match_indices(needle.as_str()).map(|(at, _)| at).collect();
        assert_eq!(
            hits.len(),
            1,
            "`fn {name}(` 在本文件里出现了 {} 次（位置 {:?}）—— 必须**恰好一次**：\
             多个就是同名诱饵/复制品，唯一匹配那条会被绕过",
            hits.len(),
            hits
        );
        let from_fn = &src[hits[0]..];
        let end = from_fn
            .find("\n    }\n")
            .unwrap_or_else(|| panic!("判据 {name} 的函数体没有 4 空格缩进的收尾"));
        &from_fn[..end]
    }

    /// 抄出源码里所有命中 `mark` 的**整行原文**（保序）。
    fn matched_lines<'a>(src: &'a str, mark: &str) -> Vec<&'a str> {
        src.match_indices(mark)
            .map(|(at, _)| {
                let start = src[..at].rfind('\n').map_or(0, |i| i + 1);
                let end = src[at..].find('\n').map_or(src.len(), |i| at + i);
                src[start..end].trim()
            })
            .collect()
    }

    /// 从源码抄出 `main()` 体内全部 `reconcile(...)` 调用点的实参（保序）。
    ///
    /// 只扫 `main` 的函数体范围：`fn reconcile(...)` 的签名、模块文档里
    /// 抄写的那份签名、测试里 `fn rc(...)` 的包装调用都在范围外 ——
    /// 本条要盯的是**主流程那两处**。
    ///
    /// 探针**必须用片段拼**：这里若直接写出 `reconcile(` 字面量，
    /// 它自己就在本文件里，会被当成第三个调用点 ⇒ 恒真。
    fn reconcile_call_sites_in_main(src: &str) -> Vec<String> {
        let needle = ["reconcile", "("].concat();
        let main_at = src.find("fn main(").expect("本文件有 fn main");
        let from_main = &src[main_at..];
        let body_end = from_main
            .find("\n}\n")
            .expect("fn main 的体以行首 `}` 收尾");
        let body = &from_main[..body_end];

        let mut sites = Vec::new();
        let mut rest = body;
        while let Some(at) = rest.find(needle.as_str()) {
            let after = &rest[at + needle.len()..];
            let close = after.find(')').expect("reconcile( 的实参后面跟着 )");
            sites.push(after[..close].split_whitespace().collect::<Vec<_>>().join(" "));
            rest = &after[close..];
        }
        sites
    }

    // ⚠️ **恒真入口必须写在断言消息里**（REQ-063 · AC-006 / F7，REQ-066 保留）：
    // 四条消息提成了常量，`assertion_messages_name_the_tautology_escape_hatches`
    // 盯的就是「消息里有没有点名」。它守的是**消息文本**，
    // 「比较符有没有真被放宽」由上面的第二层判据守 —— 两件事，两条判据。
    const MSG_FWD_REPORT: &str = concat!(
        "`--verify-only` 那处必须**恰好**一个、且基线在前参数位（现拉结果在后）。\n",
        "少了＝调用点被重构/删掉，多了＝多出第二个调用点。\n",
        "⛔ 恒真入口，别从这儿进来：**不许**把集合相等换成 contains()，\n",
        "**不许**换成 >= 0 / >= 1 / != 0 这类比较，也**不许**只比清单里的一项。\n",
        "若只是改名或提取成辅助函数，请改 EXPECTED_RECONCILE_SITES 去匹配新结构。"
    );
    const MSG_FWD_ON_DISK: &str = concat!(
        "现拉那处必须**恰好**一个、且基线在前参数位（落盘重读的那份在后）。\n",
        "同上：⛔ 不许 contains()，不许 >= 0 / >= 1 / != 0，也不许只比一项；\
         改名/重构时改 EXPECTED_RECONCILE_SITES。"
    );
    const MSG_REV_REPORT: &str = concat!(
        "参数对调（M7）：基线必须在前参数位 —— 后位是**被校验的那份**。\n",
        "反向调用点一出现，清单里就多出 '&report, &base' 一项，集合相等立刻变红。\n",
        "⛔ 同上：不许 contains()，不许 >= 0 / != 0，也不许只比一项。"
    );
    const MSG_REV_ON_DISK: &str = concat!(
        "参数对调（M7）：基线必须在前参数位 —— 后位是**被校验的那份**。\n",
        "反向调用点一出现，清单里就多出 '&on_disk, &base' 一项，集合相等立刻变红。\n",
        "⛔ 同上：不许 contains()，不许 >= 0 / != 0，也不许只比一项。"
    );

    /// ⛔ **判据不是唯一防线**（REQ-066 · QA · S-b 形态）：
/// 本条**只**在「喂进来的源码就是本文件源码」时才有意义 ——
/// 单独把它喂一段**假源码**，它可以判绿而**无人报警**。
///
/// ⚠️ **哪条轴归谁守 —— 别读反**（QA 第 3 轮实测纠正过一次；写反了比不写更坏）：
///
/// | 轴 | 守它的 | 实测 |
/// | --- | --- | --- |
/// | **实参顺序（M7）** | **本条 + 闸门守卫** | 纯 M7 ⇒ 这里**红** |
/// | **加载角色（M8）写法** | newtype **编译拒绝** | `E0308` ×2 |
/// | **加载角色（M8）内容层** | `load_both_roles_keep_their_own_file` | 纯 M7 下它**照样绿** |
///
/// ⛔ **`load_both_roles_keep_their_own_file` 抓不到 M7**（我曾在这里把它写成
/// 运行期兜底，QA 实测证伪）。它**直调** [`load_both`]、**根本不经过 `main`**，
/// 而 M7 活在 `main` 的 `reconcile(&report, &base)` 两处调用点上 ——
/// 两者在结构上**不相交**。它守的是 **`load_both` 的角色归属**（M8 的内容层），
/// 与实参顺序**无关**。
///
/// ⇒ 现状是：**M7 只有判据/守卫这一条线，运行期那条并不响。**
/// 别把它读成「判据不要紧、有运行期兜底」—— 恰好说反了。
///
/// 这与本命令开头那条纪律是同一件事：
/// **一个测试给的信号不承载它名字暗示的含义，除非另有一条不依赖它的线。**
/// 写清楚比让人默认「这条就是防线」好。
#[test]
    fn both_reconcile_call_sites_pass_baseline_first() {
        let src = include_str!("ontok-gate-preflight.rs");
        assert!(
            call_sites_match(&reconcile_call_sites_in_main(src)),
            "{}",
            call_sites_failure_message()
        );
    }

    /// AC-001 **第二层**：这些结构判据**自己**不许被换成恒真（REQ-066）。
    ///
    /// ① **必须经过自己的闸门** [`call_sites_match`] / [`no_soft_skip_match`] ——
    ///    闸门内部才拿得到期望清单，所以「弱化成只比一项 / 只比长度」不再是
    ///    一种**写法**，而是**算错了值**（由闸门那条执行判据当场变红）；
    /// ② **不得**出现 [`RELAXATION_AXES`] 里任何一个放宽轴算子
    ///    （尤其 `||` / `&&` —— `call_sites_match(..) || true` 这种
    ///    「经过闸门但再补一个恒真」必须挡住）；
    /// ③ 判据**必须唯一**（[`unique_fn_body`]）—— 同名诱饵函数无用；
    /// ④ 判据被整条删掉 ⇒ panic ⇒ 本条变红。
    ///
    /// ⚠️ **这一层仍是兜底**（QA · BLK 轮证明过：纯读文本的规则会被整类绕过）。
    /// 真正承重的是**闸门 + 执行**：清单不可拆分，采集器与标记串另有执行判据。
    #[test]
    fn structural_criteria_cannot_be_swapped_for_a_tautology() {
        let src = include_str!("ontok-gate-preflight.rs");
        for (name, seam) in [
            ("both_reconcile_call_sites_pass_baseline_first", "call_sites_match"),
            ("real_pair_has_no_soft_skip_left", "no_soft_skip_match"),
        ] {
            let body = unique_fn_body(src, name);
            assert!(
                body.contains(seam),
                "判据 {name} 没有经过闸门 {seam} —— 期望清单只在闸门内部，\
                 绕开它就等于没人比：{body}"
            );
            for axis in RELAXATION_AXES {
                assert!(
                    !body.contains(axis),
                    "判据 {name} 里出现放宽轴 `{axis}` —— 闸门被旁路或被补成恒真：{body}"
                );
            }
        }
        // ⚠️ 守卫**自己**也要唯一（BLK-4 的同族）：否则放一个同名诱饵在前，
        // 把 `seams_reject_known_wrong_inputs` 顶成一个什么都不查的空壳，
        // 整条执行防线就静悄悄没了。这里不做内容检查，只钉「恰好一次」。
        for guard in [
            "seams_reject_known_wrong_inputs",
            "call_site_collector_is_verbatim_and_ordered",
            "soft_skip_marker_is_pinned_and_can_actually_match",
        ] {
            let hits = src.matches(&format!("fn {guard}(")).count();
            assert_eq!(hits, 1, "守卫 `{guard}` 出现了 {hits} 次 —— 必须**恰好一次**（同名诱饵？）");
        }
    }

    /// 闸门本身**必须**拒掉已知的错输入 —— 用**执行**堵，不靠读闸门源码。
    ///
    /// 上一版闸门不存在（清单直接交给判据比），于是
    /// 「只比 `.len()`」「只比 `[0]`」两条都是 35/0 全绿，
    /// **叠加真实的 M7 参数对调也全绿**（QA · BLK-1/BLK-5，已独立复现）。
    ///
    /// 现在清单锁在闸门里：任何**局部**比较传进去都**算错** ⇒ 这里的 `!` 断言变红。
    #[test]
    fn seams_reject_known_wrong_inputs() {
        // 前提自检：闸门对**正确**输入必须放行，否则下面全是恒真。
        let src = include_str!("ontok-gate-preflight.rs");
        assert!(
            call_sites_match(&reconcile_call_sites_in_main(src)),
            "前提不成立：主判据当前都不成立 ⇒ 下面那些 `!` 断言是恒真"
        );
        let s = |x: &str| x.to_string();

        // 少一个调用点（「只比 [0]」能过的形态，BLK-5）。
        assert!(!call_sites_match(&[]), "空集合竟然判相等");
        assert!(!call_sites_match(&[s("&base, &report")]), "只剩一条竟然判相等");
        // ⚠️ **生成式**，不是手挑几个向量（QA · BLK-8）：
        // 手挑 ⇒ **枚举没覆盖到的向量就是活的**。QA 实测的完整 M7
        // `["&report, &base", "&on_disk, &base"]` 正好是「两处**都**换」那个组合，
        // 而上一版只钉了「只换第一处」⇒ 漏网。
        // 这里按**规则**把「每一处调用点的两种实参顺序」的**全部 2^n 组合**造出来：
        // n=2 时是 4 个（00/01/10/11，跳过 00 = 正确向量本身），
        // 完整 M7（11）**自动在内**，不需要谁记得补。
        // 将来清单长到 3 条 ⇒ 自动扩到 8 个向量，这段一个字不用改。
        for mask in 1..(1usize << EXPECTED_RECONCILE_SITES.len()) {
            let mutant: Vec<String> = EXPECTED_RECONCILE_SITES
                .iter()
                .enumerate()
                .map(|(i, site)| {
                    let mut args: Vec<&str> = site.split(", ").collect();
                    if mask >> i & 1 == 1 {
                        args.reverse();
                    }
                    args.join(", ")
                })
                .collect();
            assert!(
                !call_sites_match(&mutant),
                "实参对调向量（掩码 {mask}）{mutant:?} 竟然判相等 —— 那就是 M7 全绿"
            );
        }
        // 多出第三个调用点。
        assert!(
            !call_sites_match(&[s("&base, &report"), s("&base, &on_disk"), s("&on_disk, &base")]),
            "多出一个调用点竟然判相等"
        );
        // 顺序洗过（同样两项、内容集合相同、顺序不同）。
        assert!(
            !call_sites_match(&[s("&base, &on_disk"), s("&base, &report")]),
            "顺序被洗掉竟然判相等 ⇒ 对调实参会看不出来"
        );

        // 软跳过闸门：期望是空集，任何非空都必须判否。
        assert!(no_soft_skip_match(&[]));
        assert!(!no_soft_skip_match(&["eprintln!(\"跳过\")"]), "有软跳过竟然判相等");
    }

    /// 采集器必须**原样、保序** —— 这是 BLK-3 的正面对手。
    ///
    /// 病根：上一版第二层只监视**判据函数体**，**不监视采集器**，
    /// 于是「在采集 helper 里把实参顺序归一化」是 3 行、全绿，
    /// **叠加完整 M7 仍全绿**（QA · BLK-3）。
    /// 现在用手工构造的输入直接**执行**它：顺序、反序、main 之外的调用点。
    #[test]
    fn call_site_collector_is_verbatim_and_ordered() {
        let probe = |body: &str| format!("fn main() -> Result<()> {{\n{body}\n}}\n");
        let v = |x: &[&str]| x.iter().map(|s| s.to_string()).collect::<Vec<_>>();

        assert_eq!(
            reconcile_call_sites_in_main(&probe("    reconcile(&base, &report);\n    reconcile(&base, &on_disk);")),
            v(&["&base, &report", "&base, &on_disk"])
        );
        // 参数对调必须**原样**抄出 —— 不许归一化 / 排序 / 去重。
        assert_eq!(
            reconcile_call_sites_in_main(&probe("    reconcile(&report, &base);")),
            v(&["&report, &base"])
        );
        // 保序：同样两项、顺序相反，采集结果必须不同。
        assert_ne!(
            reconcile_call_sites_in_main(&probe("    reconcile(&base, &on_disk);\n    reconcile(&base, &report);")),
            reconcile_call_sites_in_main(&probe("    reconcile(&base, &report);\n    reconcile(&base, &on_disk);")),
            "采集器把顺序洗掉了 ⇒ 实参对调会看不出来"
        );
        // main 之外的调用点不得被算进来。
        assert_eq!(
            reconcile_call_sites_in_main(
                "fn helper() {\n    reconcile(&x, &y);\n}\nfn main() -> Result<()> {\n    reconcile(&base, &report);\n}\n"
            ),
            v(&["&base, &report"])
        );
    }

    /// 软跳过标记串必须钉死 —— 这是 BLK-2 的正面对手。
    ///
    /// 病根：上一版标记串就写在判据**体内**，是判据的**输入**，
    /// 改成任何一个**不存在的串**就是 1 行、全绿，
    /// **软跳过真的复活了也查不出来**（QA · BLK-2）。
    /// 现在标记由闸门产出、并在这里钉成字面量；再加一条**正向对照**，
    /// 确保「找不到」不是因为标记压根匹配不到（否则那条判据是恒真）。
    #[test]
    fn soft_skip_marker_is_pinned_and_can_actually_match() {
        assert_eq!(
            real_soft_skip_marker(),
            "eprintln!(\n                \"跳过：实测样本不在（不在版本控制内",
            "软跳过标记串被改了 ⇒ 判据的**输入**被换掉，真软跳过复活时查不出来"
        );
        // 正向对照：这个标记在一段合成的软跳过源码里**必须**能被找到。
        let fake = format!("        if x {{\n            {}\n", real_soft_skip_marker());
        assert_eq!(
            matched_lines(&fake, &real_soft_skip_marker()),
            vec![fake.split('\n').nth(1).unwrap().trim()],
            "正向对照失败：标记压根匹配不到 ⇒ `real_pair_has_no_soft_skip_left` 是恒真"
        );
    }

    /// AC-006 / F7：恒真入口必须**在断言消息里被点名**。
    ///
    /// 起源（REQ-063）：上一轮的消息点名了 `contains()` 却没点名 `>= 0` ——
    /// 「一行可补」，而这一行不补，下一次「让它别那么吵」就会从这条路进来。
    /// 本条把那句话变成**可断言的**事实：消息常量抽出来之后，
    /// 「消息里有没有点名」就是一条会响的判据，而不是只能靠人读。
    ///
    /// ⚠️ **它守的始终只是「消息文本」**（REQ-063 · AC-006 的字面要求）。
    /// QA 实测：四条判据真被放宽成 `>= 0` 时，本条**照样全绿** —— 守消息
    /// 守不住比较符。
    ///
    /// ✅ 那个缺口已由 REQ-066 · AC-001 补上，**不是**由本条补的：
    /// 判据形状换成了**集合相等**（放宽必须改期望清单），
    /// 外加第二层 [`structural_criteria_cannot_be_swapped_for_a_tautology`]
    /// —— 那条**确实**看得见判据自己的源码，所以「没有自动判据能管」
    /// 这个说法在 REQ-066 之后已经不成立，别再照抄。
    /// 两条各守一件：本条守消息不腐烂，那条守比较符没被换掉。
    #[test]
    fn assertion_messages_name_the_tautology_escape_hatches() {
        for m in [MSG_FWD_REPORT, MSG_FWD_ON_DISK, MSG_REV_REPORT, MSG_REV_ON_DISK] {
            assert!(
                m.contains("contains()"),
                "恒真入口之一是「把 count()==1 换成 contains()」—— 消息里必须点名它：{m}"
            );
            assert!(
                m.contains(">= 0"),
                "恒真入口之一是「把 == 1 / == 0 放宽成 >= 0」—— 消息里必须点名它：{m}"
            );
        }
    }
}

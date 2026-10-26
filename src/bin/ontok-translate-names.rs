//! 表名→中文名翻译表**生成器**（REQ-090）
//!
//! ```text
//! ontok-introspect  ──▶ schema.json ──▶ ontok-translate-names ──▶ 翻译表 JSON
//!                                                            └─▶ ontok-propose --names
//! ```
//!
//! # 为什么需要它（全仓此前**没有**任何程序生产这份表）
//!
//! `ontok-propose` 的 `--names` 要吃一份「表名 → 中文显示名」翻译表，而实测
//! 源库 **91 张表里只有 3 张有表注释** —— 中文名不外挂，抽取出的实体**整批消失**
//! （`propose.rs` 把查不到中文名的表记进 `stats.tables_without_cn_name`，
//! 空名过不了 `usable_as_human_name`，**没有 stderr、没有非零退出码**）。
//! 这份表过去每次换源库都要人手写一遍全量 JSON。
//!
//! # 本文件的四条纪律
//!
//! ## 一 · 校验**不重写**，只调 [`TableNames::load`]
//!
//! 「空名 / 同名 / 空表」这三条判据有**唯一真身**：`TableNames::load`。
//! 本生成器在写文件**之前**把自己拼出来的候选串交给它，**过不了就不写文件**
//! （fail-closed）。本文件**没有**第二份校验实现 —— 复制即漂。
//!
//! ⚠️ 这不是形式主义：`load` 任一条不过就 `bail` **整份表**（不是丢一张表），
//! 所以「先生成、后校验」会让下游 `ontok-propose` 整轮失败。
//!
//! ## 二 · 已有翻译是**知识资产**，默认**不覆盖**（AC-003）
//!
//! | 情形 | 本工具的行为 |
//! | --- | --- |
//! | schema 新增的表 | 翻译并**新增** |
//! | schema 里还在的旧表 | **原样保留**（除非 `--overwrite` / `--regenerate-all`）|
//! | schema 里**已消失**的表 | 进缺口清单，**绝不静默删除** |
//!
//! 删除必须是人的**显式动作**：静默删除等于「这条业务对象从世界上消失了，
//! 且没人知道」。`--regenerate-all` 也不例外 —— 它只管**重翻**，不管**删**。
//!
//! ## 三 · 覆盖率是**显式出口**，不是「产物里少几条」
//!
//! 一行汇总 `translated N / total M` + 产物里机器可读的 `_coverage`
//! （`untranslated` / `absent_from_schema` / `degenerate_tokens` 三个数组）。
//! 缺口默认**硬失败且不写文件**（退出码 **2**，与「命令写错了」的 **1** 分开），
//! 要出残缺产物必须显式 `--allow-gaps`，且此时退出码是 **3**
//! ——「60 条对 31 条」不算成功。
//!
//! ⚠️ 四个退出码**互不重载**，脚本只看退出码就能决定处置方向：
//! `0` 成功 / `1` 改命令 / `2` 数据不全没写产物 / `3` 产物已写但要人过目。
//!
//! ⚠️ **执行顺序是踩过坑才写下来的**：缺口闸与 `translated N / total M`
//! 汇总都必须排在 `TableNames::load` **之前**。反过来排，
//! **100% 缺口**（一张都没翻出来 ⇒ `names` 为空）会被 `load` 的「翻译表是空的」
//! 先 `bail` 掉 ⇒ 归成 **1**（=「我的命令写错了」），而事实是
//! 「命令完全正确，是数据一条都没翻出来」—— **1 与 2 重载，正是本设计要消灭的情况**。
//! 代价还不止重载：那条路径上 `load` 直接短路返回，
//! **最坏的情况（一条都没翻出来）产出的信息最少** —— 既没有汇总行，也没有缺口清单。
//!
//! ⚠️ **退化自检命中不算成功**：`degenerate_tokens` 里的中文名过不了
//! `usable_as_human_name`，下游会把这些对象**静默丢掉** —— 与缺口同一类危害，
//! 所以也归入 **3**（产物已写出但要人过目），**不是 0**。
//!
//! ⚠️ 代价：`ontok-propose` 载入后**会丢弃 `_coverage`**（结构未标
//! `deny_unknown_fields`，多带字段能反序列化，但消费方拿不到）。
//! 它是给人与脚本看的**同批产物**，不是抽取器的输入契约。
//!
//! ## 四 · 溯源字段记**事实**，不记占位
//!
//! `agent` 由**真正跑起来的这段代码自报**（`module_path!()`，不是谁"登记"它是什么），
//! 口径对齐 `ontok-propose` 的 `inproc:ontok::agent::RuleBasedAgent`。
//! ⚠️ 基线上吃过亏：`ontok-propose` 的 `--agent` 曾硬编码 `"mcode-agent"`、
//! `--agent-version` 默认 `"unknown"` 且**被翻译表的版本号顶替** ——
//! 产物里记着一个与执行者无关的**自证假溯源**。本文件不抄那个坑：
//! 凭据只走 [`ontok::agent::llm::LlmConfig::from_env`]，
//! **不读 `.env` 文件、不读命令行参数、不新增任何环境变量**。

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{bail, Context, Result};
use ontok::agent::llm::protocol::Message;
use ontok::agent::llm::{
    completion_text, ChatRequest, HttpTransport, LlmConfig, LlmTransport, API_KEY_ENV,
    MAX_TOKENS_ENV, MODEL_ENV, TIMEOUT_ENV,
};
use ontok::extract::introspect::SchemaReport;
use ontok::extract::names::TableNames;

/// 退出码 · 成功且**全覆盖、无残留**。
const EXIT_OK: i32 = 0;
/// 退出码 · **命令/环境/数据形状错误**（缺参、读不到文件、JSON 非法、模型调用失败、
/// 产物被 `TableNames::load` 拒绝）—— 处置是**去修命令**，重跑即可。
///
/// ⛔ 它**不再**兼表「有缺口」：两者处置完全相反（改命令 vs 处理数据），
/// 共用一个码等于逼脚本去猜。上一版就是重载的（缺口也是 1），已拆开。
const EXIT_COMMAND_ERROR: i32 = 1;
/// 退出码 · 命令**没问题**，但数据**不完整**：有表没翻出来，且**没给** `--allow-gaps`
/// ⇒ **没有写出任何文件**。处置是**看 `_coverage.untranslated` 或显式承认缺口**。
const EXIT_GAPS_NOT_ACKNOWLEDGED: i32 = 2;
/// 退出码 · 产物**已写出**，但仍有**缺口 / 残留 / 退化**要人过目
/// （`--allow-gaps` 生效、或没有缺口只是有已消失的表、
/// 或退化自检命中了词元 = 实体 key 的中文名）。
/// 处置是**读产物里的 `_coverage`**。
///
/// ⚠️ 退化自检归到这里、**不新造一档**：三者的脚本处置**完全相同**
/// （读产物里的 `_coverage`），新造一码只会把「一个动作」拆成两个码，
/// 逼脚本多记一条规则却不换来任何处置上的区别 —— 而本设计要的正是
/// 「只看退出码就能决定处置方向」。
const EXIT_REVIEW_NEEDED: i32 = 3;

/// 退出码的**唯一实现**（纯函数，好让单测真的在测这条路径，
/// 而不是测一份「长得一样但没接线」的第二实现）。
///
/// `written` = 产物是否已经落盘。它决定「有缺口」落在 2（没写）还是 3（写了）。
fn coverage_exit(has_gaps: bool, has_stale: bool, has_degenerate: bool, written: bool) -> i32 {
    if has_gaps && !written {
        EXIT_GAPS_NOT_ACKNOWLEDGED
    } else if has_gaps || has_stale || has_degenerate {
        EXIT_REVIEW_NEEDED
    } else {
        EXIT_OK
    }
}

/// 缺口闸的**唯一实现**（纯函数），返回**该不该拦下来、什么都不写**。
///
/// 闸门在 [`TableNames::load`] **之前**跑（见 `run()`）——
/// 否则 100% 缺口会被 `load` 的「翻译表是空的」先 `bail` 掉归成 1，
/// 于是「命令写错了」与「数据一条都没翻出来」共用一个码（脚本无法区分处置方向），
/// 而且那条路径**连汇总行与缺口清单都没有**（AC-002 要求人和脚本**同时**能读到缺口）。
///
/// `product_is_empty` 是第二道保险：**即使给了 `--allow-gaps`**，
/// `names` 为空时也**写不出**任何合法产物（`load` 必然拒），
/// 此时报 3 是撒谎（3 的语义是「产物**已写出**」），报 1 是错分类（命令没问题）
/// ⇒ 只能仍是 **2**（数据不完整、没有写出任何文件）。
fn gap_gate_fires(has_gaps: bool, allow_gaps: bool, product_is_empty: bool) -> bool {
    has_gaps && (!allow_gaps || product_is_empty)
}

/// 每次请求送多少张表。一张表的 schema（表注释 + 列注释）是几十行，
/// 一次全塞进去会让正文被截断（`completion_text` 对 `finish_reason = "length"` 报错）。
const DEFAULT_BATCH_SIZE: usize = 20;

/// 每张表最多送几列的注释 —— 送全量对「这张表是干什么的」没有增量信息。
const MAX_COLUMNS_IN_PROMPT: usize = 8;

/// **本程序自报的实现身份**（AC-004）。
///
/// `module_path!()` 展开成「编译进这个二进制的到底是哪段代码」——
/// 与 `ontok-propose` 的 `inproc:ontok::agent::RuleBasedAgent` 同一口径。
/// 它**不可能**指向别的实现：改成别的名字就得改这一行，而这一行就在产物里看得见。
const REG090_MODULE_PATH: &str = module_path!();

/// REQ-152：本文件不再自己成 bin —— 它是 `ontok-builder translate-names` 的实现。
/// argv 由外层的 clap 子命令**原样**递进来（`args` 就是去掉程序名之后的参数），
/// 参数解析、退出码与产物一个字节都没变。
pub fn main(args: Vec<String>) {
    let code = match run(args) {
        Ok(code) => code,
        Err(err) => {
            // `{:#}` 打出 anyhow 的**整条因果链** ——
            // `TableNames::load` 报错的原因必须在链尾可见（AC-001 要求错误指名道姓是哪张表）。
            eprintln!("错误：{err:#}");
            EXIT_COMMAND_ERROR
        }
    };
    std::process::exit(code);
}

// ══════════════════════════════════════════════════════════════
// 参数
// ══════════════════════════════════════════════════════════════

#[derive(Debug, Default)]
struct Args {
    schema: Option<String>,
    out: Option<String>,
    endpoint: Option<String>,
    names: Option<String>,
    source: Option<String>,
    overwrite: BTreeSet<String>,
    regenerate_all: bool,
    allow_gaps: bool,
    batch_size: usize,
}

fn parse_args(args: Vec<String>) -> Result<Option<Args>> {
    let mut a = Args {
        batch_size: DEFAULT_BATCH_SIZE,
        ..Default::default()
    };
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        let mut val = || {
            it.next()
                .with_context(|| format!("{arg} 需要一个参数"))
        };
        match arg.as_str() {
            "--schema" => a.schema = Some(val()?),
            // ⛔ **无默认**（AC-008）：翻译表是**部署方资产**，程序不许往任何
            // 受版本控制的路径写 —— 尤其不是 `src/extract/table_names.json`。
            "--out" => a.out = Some(val()?),
            "--endpoint" => a.endpoint = Some(val()?),
            "--names" => a.names = Some(val()?),
            "--source" => a.source = Some(val()?),
            "--overwrite" => {
                let t = val()?;
                a.overwrite.insert(t.clone());
            }
            "--regenerate-all" => a.regenerate_all = true,
            "--allow-gaps" => a.allow_gaps = true,
            "--batch-size" => {
                a.batch_size = val()?
                    .parse()
                    .context("--batch-size 要一个正整数（每次请求送多少张表）")?
            }
            "-h" | "--help" => {
                print_help();
                return Ok(None);
            }
            other => bail!("未知参数 {other}（--help 列全部参数）"),
        }
    }
    if a.batch_size == 0 {
        bail!("--batch-size 必须是正整数（0 意味着一个请求都不发）");
    }
    Ok(Some(a))
}

fn print_help() {
    println!("表名→中文名翻译表生成器（REQ-090）：schema JSON → TableNames JSON");
    println!();
    println!("用法：ontok-translate-names --schema <path> --out <path> [--endpoint <url>] [选项]");
    println!();
    println!("必填：");
    println!("  --schema <path>     ontok-introspect 产出的 schema JSON（SchemaReport：");
    println!("                      server_version / schema / tables[] 等）");
    println!("  --out <path>        翻译表 JSON 的写出路径。**无默认值**：翻译表是部署方资产，");
    println!("                      程序不往任何受版本控制的路径写（尤其不是 src/extract/table_names.json）。");
    println!();
    println!("翻译（需要调用模型时才必填）：");
    println!("  --endpoint <url>    OpenAI 兼容端点的 base_url（**登记/部署事实，不写死**），");
    println!("                      实际请求打到 {{endpoint}}/chat/completions。");
    println!("                      没有要翻的表时**不需要**它。");
    println!("  --batch-size <n>    每次请求送多少张表（默认 {DEFAULT_BATCH_SIZE}）");
    println!();
    println!("增量（一条翻译是**人工确认过的知识资产**，不是机器输出）：");
    println!("  --names <path>      已有翻译表，做增量（缺省 = 从零生成）");
    println!("  --overwrite <table> 显式重翻这张表（可重复）。⛔ 不给就是**原样保留**；");
    println!("                      表名不在 schema 里 ⇒ 直接报错（防拼错）。");
    println!("  --regenerate-all    重翻 schema 里的**全部**表。⚠️ **仍然不删**任何条目：");
    println!("                      schema 里已消失的表只进清单，删除必须是人的显式动作。");
    println!("  --allow-gaps        允许写出**有缺口**的翻译表。默认不许（有缺口 ⇒ 不写文件、退出码 2）。");
    println!("                      ⚠️ 唯一例外：一张都没翻出来时**依然写不出**（空表过不了校验），");
    println!("                         仍退出 2 —— --allow-gaps 买不到「空表也能落盘」。");
    println!();
    println!("溯源：");
    println!("  --source <str>      产物里 `source` 字段的值。**不给就用事实性描述**：");
    println!("                      \"<schema> schema, <N> tables (introspected <时刻>)\"。");
    println!("                      ⚠️ SchemaReport 里**没有数据库名**（只有 pg schema 名），");
    println!("                      所以没有、也**不许编**一个看起来像库名的默认值。");
    println!("  产物里的 `agent` 由**本程序自报**（编译进这个二进制的实现路径），不是占位串；");
    println!("  `version` 是本 crate 的版本 + 真正调用的模型名。");
    println!();
    println!("输出格式（两份都在同一个文件里）：");
    println!("  · TableNames 本体：agent / version / generated_at / source / names");
    println!("  · 额外字段 `_coverage`：机器可读的缺口清单 ——");
    println!("    untranslated[]      schema 里有、但没拿到中文名的表");
    println!("    absent_from_schema[] names 里有、但 schema 已没有的表（**没有被删**）");
    println!("    degenerate_tokens[] 中文名词元撞上实体 key 的退化自检（AC-007）；**非空 ⇒ 退出码 {}**，", EXIT_REVIEW_NEEDED);
    println!("                      不是 0（这样的名字会被下游静默丢弃）");
    println!("  ⚠️ `ontok-propose` 载入后会**丢弃** `_coverage`：结构未标 deny_unknown_fields，");
    println!("     多带字段能反序列化，但消费方拿不到这份清单。");
    println!();
    println!("stdout 一行汇总：translated <N> / total <M>");
    println!("退出码（四个值互不重载，脚本可只看退出码就决定处置方向）：");
    println!("  {EXIT_OK} = 成功：**全覆盖、无残留、无退化**（唯一能拿 0 的情形）");
    println!("  {EXIT_COMMAND_ERROR} = 命令/环境/数据形状错误 ⇒ **去修命令**，产物没写");
    println!("  {EXIT_GAPS_NOT_ACKNOWLEDGED} = 命令没问题但**数据不完整**（有表没翻出来）⇒ 没写产物；");
    println!("       处置是看 _coverage.untranslated 或显式加 --allow-gaps");
    println!("       ⚠️ **一张都没翻出来（100% 缺口）也是这个码，不是 {EXIT_COMMAND_ERROR}** ——");
    println!("          那种情况命令完全正确，缺的是数据；此时连 --allow-gaps 也写不出产物");
    println!("          （空表过不了 TableNames::load），仍报本码。");
    println!("  {EXIT_REVIEW_NEEDED} = 产物**已写出**，但仍有缺口/残留/退化要人过目 ⇒ 读产物的 _coverage");
    println!("       ⚠️ **退化自检命中也归本码，不是 {EXIT_OK}**：这样的中文名过不了下游的");
    println!("          usable_as_human_name，对象会被**静默丢掉** —— 报成功等于骗人。");
    println!();
    println!("环境变量（凭据与模型名**只走环境变量**：不入库、不进日志、不回显值）：");
    println!("  {API_KEY_ENV}      模型 api key（必填）");
    println!("  {MODEL_ENV}        模型名（必填）");
    println!("  {MAX_TOKENS_ENV}   输出上限（可选，默认 {}）", LlmConfig::DEFAULT_MAX_TOKENS);
    println!("  {TIMEOUT_ENV}      单次请求超时秒数（可选，默认 {}）", LlmConfig::DEFAULT_TIMEOUT_SECS);
    println!();
    println!("非交互：不读 stdin、不依赖 TTY、不弹确认，可被脚本 / CI 无人值守调用。");
}

// ══════════════════════════════════════════════════════════════
// 翻译计划（纯函数 —— 增量语义只在这里有第二处实现，因此可测）
// ══════════════════════════════════════════════════════════════

/// 这一轮**要送去翻译**的表名（已排序）与随之而来的记账。
#[derive(Debug, Default, PartialEq)]
struct Plan {
    to_translate: Vec<String>,
    /// 被 `--overwrite` / `--regenerate-all` 点名要重翻的表（即**重写**旧条目的那些）。
    rewritten: Vec<String>,
}

/// **增量语义**（AC-003）的唯一实现。
///
/// 「旧条目一条不落地留着」这件事是**结构上**保证的：
/// `to_translate` 只决定**送模型哪些表**，合并时旧条目原样带过去，
/// 这里**没有任何一条路径会产出「要删除的表」**。
fn plan_translations(
    schema_tables: &[String],
    existing: &BTreeMap<String, String>,
    overwrite: &BTreeSet<String>,
    regenerate_all: bool,
) -> Result<Plan> {
    for t in overwrite {
        if !schema_tables.iter().any(|s| s == t) {
            bail!(
                "--overwrite 点了表 {t:?}，但它**不在** schema 里。\n\
                 schema 共 {} 张表。拼错表名会让「显式重翻」变成「什么都没做」，\
                 而你以为重翻过了 —— 所以这里直接报错。",
                schema_tables.len()
            );
        }
    }
    let to_translate: Vec<String> = schema_tables
        .iter()
        .filter(|t| regenerate_all || overwrite.contains(*t) || !existing.contains_key(*t))
        .cloned()
        .collect();
    let rewritten: Vec<String> = to_translate
        .iter()
        .filter(|t| existing.contains_key(*t))
        .cloned()
        .collect();
    Ok(Plan {
        to_translate,
        rewritten,
    })
}

/// 合并：**旧条目原样留下 + 新翻译写入**。
///
/// ⛔ 本函数**没有删除分支** —— 已从 schema 消失的表因此必然留在 `names` 里，
/// 由 [`coverage`] 把它列进 `absent_from_schema` 让**人**决定去留。
fn merge_names(
    existing: &BTreeMap<String, String>,
    translated: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut merged = existing.clone();
    for (k, v) in translated {
        merged.insert(k.clone(), v.clone());
    }
    merged
}

/// 缺口清单：`untranslated`（schema 里有、没名字）与
/// `absent_from_schema`（names 里有、schema 已无）。两个数组都**排序**，
/// 这样同一次输入的产物逐字节可比（diff 才说得出变化）。
fn coverage(
    schema_tables: &[String],
    names: &BTreeMap<String, String>,
) -> (Vec<String>, Vec<String>) {
    let untranslated: Vec<String> = schema_tables
        .iter()
        .filter(|t| !names.contains_key(*t))
        .cloned()
        .collect();
    let absent: Vec<String> = names
        .keys()
        .filter(|k| !schema_tables.iter().any(|s| s == *k))
        .cloned()
        .collect();
    (untranslated, absent)
}

// ══════════════════════════════════════════════════════════════
// 退化自检（AC-007：只查退化，不查雅俗）
// ══════════════════════════════════════════════════════════════

/// 按**非字母数字/下划线**切分并转小写 —— 与 `src/domain/view.rs` 的 `tokenize`
/// **同名同规则**的副本。
///
/// ⚠️⚠️ **这是 AC-006 逼出来的强制重复，不是设计选择**：
/// `tokenize` 在 `view.rs` 里是**私有**函数，而 `view.rs` 是本需求的**红线只读**文件
/// （改它就得动一个禁改文件），所以「复用原件」在物理上不可行。
///
/// **⛔ 副本与原件的「相等性」没有任何测试覆盖** —— 下面那条单测钉的是**这个副本自己**
/// 的行为，两份实现可以各自全绿而悄悄漂移，**没有任何测试能抓住**。
/// 这就是纪律 H 说的「忠实的重复实现任何测试都抓不住」，登记为已知限制，不写成已封堵。
///
/// 漂移的**后果有界**：只影响退化自检的**告警**（可能漏报一条退化名），
/// **不影响** `TableNames::load` 的三条校验 —— 那是真身，本需求直接调用。
fn tokenize(s: &str) -> Vec<String> {
    s.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|t| !t.is_empty())
        .map(|t| t.to_ascii_lowercase())
        .collect()
}

/// 中文名里**恰好等于某个实体 key** 的词元 —— 这些名字会被
/// `usable_as_human_name` 判为退化值，在客户视图里**整个对象静默消失**。
///
/// 典型触发场景：中英混排的「订单 Order」，而库里恰有 key `order`。
///
/// ⛔ 这里**只**查「词元恰好相等」这一种退化。**不加**「中文名不得包含其他表名」
/// 之类的雅俗检查 —— 那会误杀绝大多数正常翻译（`客户订单` 里就有 `订单`）。
fn degenerate_tokens(cn_name: &str, keys: &BTreeSet<String>) -> Vec<String> {
    tokenize(cn_name)
        .into_iter()
        .filter(|t| keys.contains(t))
        .collect()
}

/// 实体 key 集合 = **全部表名转小写**。
///
/// 口径依据：`propose.rs` 里 `EntityPayload.key = t.name`（表名原样），
/// `view.rs:763` 再 `to_ascii_lowercase()` 收成 `keys`。
/// 本生成器看不到快照，但表名就是未来的实体 key，所以判据用同一个集合。
///
/// ⚠️ **这个集合是下游 `keys` 的子集，不是相等**：下游的 `keys` 来自**快照里的实体类型**
/// （可能还有视图、视图族等非表来源）。所以退化自检是**窄于**下游判据的 ——
/// 存在「过得了自检、下游照样丢掉」的名字。这是**漏报**方向，
/// 不影响「检测到的都会反映到退出码」这条契约，但**不要**把它当成已封堵：
/// 要真正对齐，得等生成器能看到快照。
///
/// 另外它只查 `schema_tables` 里**还在**的表：已从 schema 消失的残留条目不参与退化自检。
fn entity_keys(schema_tables: &[String]) -> BTreeSet<String> {
    schema_tables
        .iter()
        .map(|t| t.to_ascii_lowercase())
        .collect()
}

// ══════════════════════════════════════════════════════════════
// 翻译（复用库里已有的 LLM 通道：凭据/模型/传输层一律不新造）
// ══════════════════════════════════════════════════════════════

const SYSTEM_PROMPT: &str = "\
你是企业数据库的术语翻译。为每张**业务表**起一个中文显示名。
规则：
1. 简体中文短语，通常 2–12 个字；
2. 必须是业务人员日常的说法（“客户”“销售订单”“收货地址”）；
3. ⛔ 不要用建模用语（“快照”“实例”“记录表”“主数据”）——
   这些词会让页面把真实业务对象整块剔除；
4. ⛔ 不要中英混排、不要把表名或列名原样抄进来；
5. ⛔ 不要输出表名本身或空字符串 —— 那等于没翻；
6. 只输出一个 JSON 对象，不要解释、不要 Markdown 代码块：
   {\"names\": {\"<表名>\": \"<中文名>\"}}
7. 表名与输入**逐字一致**，不要增删条目；输入里的**每一张表**都要给出译名。";

/// 把一批表投影成送进 prompt 的精简 schema（表注释 + 前 N 列的列注释）。
fn prompt_batch(report: &SchemaReport, batch: &[String]) -> String {
    let items: Vec<serde_json::Value> = batch
        .iter()
        .filter_map(|name| report.tables.iter().find(|t| &t.name == name))
        .map(|t| {
            let columns: Vec<serde_json::Value> = t
                .columns
                .iter()
                .filter(|c| c.comment.as_deref().map(str::trim).unwrap_or("").len() > 1)
                .take(MAX_COLUMNS_IN_PROMPT)
                .map(|c| {
                    serde_json::json!({
                        "column": c.name,
                        "comment": c.comment.as_deref().unwrap_or("").trim(),
                    })
                })
                .collect();
            serde_json::json!({
                "table": t.name,
                "table_comment": t.comment.as_deref().unwrap_or("").trim(),
                "columns": columns,
            })
        })
        .collect();
    serde_json::to_string_pretty(&items).unwrap_or_else(|_| "[]".to_string())
}

/// 剥掉模型爱加的 ```json 围栏，再取 `names` 对象。
///
/// ⛔ **只取** `batch` 里出现过的表名：**模型编出来的表名一律丢掉** ——
/// 编出来的表名会在缺口清单里凭空多出一条「没翻的表」，比没有更坏。
fn parse_translations(body: &str, batch: &[String]) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let cleaned = body
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    let Ok(v) = serde_json::from_str::<serde_json::Value>(cleaned) else {
        return out;
    };
    let Some(obj) = v.get("names").and_then(|n| n.as_object()) else {
        return out;
    };
    for (k, val) in obj {
        let Some(cn) = val.as_str() else { continue };
        let cn = cn.trim();
        if cn.is_empty() || !batch.iter().any(|b| b == k) {
            continue;
        }
        out.insert(k.clone(), cn.to_string());
    }
    out
}

/// 真的去调模型，返回 `表名 → 中文名`（**可能缺项** —— 缺的就是缺口，不猜）。
///
/// ⚠️ 凭据与模型只走 [`LlmConfig::from_env`]；传输层用库里的
/// [`HttpTransport`]。**不读 `.env` 文件、不读命令行参数、不新增环境变量**。
fn translate(
    transport: &mut dyn LlmTransport,
    model: &str,
    max_tokens: u32,
    report: &SchemaReport,
    batch: &[String],
) -> Result<BTreeMap<String, String>> {
    let req = ChatRequest {
        model: model.to_string(),
        messages: vec![
            Message::system(SYSTEM_PROMPT),
            Message::user(prompt_batch(report, batch)),
        ],
        max_tokens,
    };
    // ⚠️ 错误信息里**不回显请求体**（它含客户 schema）与 api key。
    let resp = transport
        .complete(&req)
        .with_context(|| format!("第 {} 张表这一批调用模型失败", batch.len()))?;
    let body = completion_text(&resp)?;
    Ok(parse_translations(&body, batch))
}

// ══════════════════════════════════════════════════════════════
// 主流程
// ══════════════════════════════════════════════════════════════

fn run(args: Vec<String>) -> Result<i32> {
    let Some(args) = parse_args(args)? else {
        return Ok(EXIT_OK);
    };
    // ⛔ `--out` **必填、无默认**（AC-008）。翻译表是部署方资产，
    // 程序不替人决定往哪个受版本控制的路径写。
    let out = args
        .out
        .as_deref()
        .context(
            "缺 --out：翻译表的写出路径**必填且无默认值**。\n\
             为什么不给默认：翻译表是**部署方资产**，默认路径会让人不小心把产物写进 \
             src/extract/table_names.json 之类受版本控制的位置（那是部署方的东西，不是本仓库的）。",
        )?
        .to_string();
    let schema_path = args
        .schema
        .as_deref()
        .context("缺 --schema：ontok-introspect 产出的 schema JSON 路径")?
        .to_string();

    let raw = std::fs::read_to_string(&schema_path)
        .with_context(|| format!("读不到 {schema_path}（先跑 ontok-introspect 产出它）"))?;
    let report: SchemaReport =
        serde_json::from_str(&raw).with_context(|| format!("{schema_path} 不是合法的 SchemaReport JSON"))?;

    // 已有翻译表：**走 `TableNames::load` 读**，不自己解析 ——
    // 一份坏掉的旧表必须在这里就炸，而不是被"宽容地"当成空表重新生成
    // （那等于用一份新生成物**悄悄覆盖**掉人确认过的知识资产）。
    let existing_table: Option<TableNames> = match &args.names {
        Some(p) => {
            let raw = std::fs::read_to_string(p).with_context(|| format!("读不到已有翻译表 {p}"))?;
            Some(
                TableNames::load(&raw)
                    .with_context(|| format!("已有翻译表 {p} 未通过 TableNames::load 的校验"))?,
            )
        }
        None => None,
    };
    let existing: BTreeMap<String, String> = existing_table
        .as_ref()
        .map(|t| t.names.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();

    let mut schema_tables: Vec<String> = report.tables.iter().map(|t| t.name.clone()).collect();
    schema_tables.sort();
    schema_tables.dedup();

    let plan = plan_translations(&schema_tables, &existing, &args.overwrite, args.regenerate_all)?;

    // ── 调模型（只在真有活要干时才要凭据）──
    let mut translated: BTreeMap<String, String> = BTreeMap::new();
    let mut model_used: Option<String> = None;
    if !plan.to_translate.is_empty() {
        let endpoint = args.endpoint.as_deref().with_context(|| {
            format!(
                "这次有 {} 张表要翻译，缺 --endpoint。\n\
                 端点是**部署/登记的事实**，不写死也不给默认值（换个环境就得改代码）。",
                plan.to_translate.len()
            )
        })?;
        // ⛔ 凭据只走环境变量（复用 `from_env`，不新增读 key 的途径）。
        let cfg = LlmConfig::from_env()?;
        let mut transport = HttpTransport::new(endpoint, &cfg.api_key, cfg.timeout)
            .context("构造 LLM HTTP 传输失败")?;
        for batch in plan.to_translate.chunks(args.batch_size) {
            let got = translate(&mut transport, &cfg.model, cfg.max_tokens, &report, batch)?;
            translated.extend(got);
        }
        model_used = Some(cfg.model);
    }

    let merged = merge_names(&existing, &translated);
    let (untranslated, absent) = coverage(&schema_tables, &merged);
    let translated_count = schema_tables.len() - untranslated.len();
    // 记账口径（⛔ 不能用 `schema_tables.len() - rewritten.len()` 那种减法）：
    // 那个式子把「**压根没翻出来**」的表也算成「沿用旧译」——
    // 实测过：从零生成、5 张表只翻出 3 张时，它会报「沿用旧译 5」，
    // 而那份表**根本不存在**。虚高的账比没有账更坏。
    // 正确口径：schema 里**有名字**的表 = 本轮真翻出来的 + 沿用旧译的。
    let regenerated = translated.len();
    let carried_over = translated_count - regenerated;

    // ── 溯源：三个字段都记**事实**（AC-004）──
    //
    // `agent` = **本程序自报**的实现身份：`module_path!()` 展开成
    // 「编译进这个二进制的到底是哪段代码」，它**不可能**指向别的实现 ——
    // 与 `ontok-propose` 的 `inproc:ontok::agent::RuleBasedAgent` 同一口径，
    // 刻意带前缀好让人认出「这是执行者自报，不是某个部署侧登记名」。
    let agent = format!("bin:{REG090_MODULE_PATH}");
    // `version` = 本 crate 版本 + **这次真的调用的**模型名。
    // ⛔ 没调模型时不写模型名 —— 记一个没跑过的模型就是**假溯源**，
    // 正是 REQ-059 在 `ontok-propose` 上吃过的那口亏。
    let version = match &model_used {
        Some(m) => format!("{}+llm.{m}", env!("CARGO_PKG_VERSION")),
        None => env!("CARGO_PKG_VERSION").to_string(),
    };
    let source = match args.source.as_deref().map(str::trim) {
        Some(s) if !s.is_empty() => s.to_string(),
        Some(_) => bail!("--source 给了但**去掉空白后是空的** —— 空字符串不是源库标识"),
        // ⚠️ SchemaReport 里**没有数据库名**（只有 pg schema 名）。
        // 所以不给 --source 时只写**确实是事实**的描述，
        // ⛔ 绝不编一个看起来像库名的默认值。
        None => format!(
            "{} schema, {} tables (introspected {})",
            report.schema, report.tables.len(), report.introspected_at
        ),
    };

    let keys = entity_keys(&schema_tables);
    let degenerate: Vec<serde_json::Value> = schema_tables
        .iter()
        .filter_map(|t| {
            let cn = merged.get(t)?;
            let hit = degenerate_tokens(cn, &keys);
            if hit.is_empty() {
                return None;
            }
            Some(serde_json::json!({
                "table": t,
                "cn_name": cn,
                "tokens_equal_to_entity_key": hit,
            }))
        })
        .collect();

    let table = TableNames {
        agent: agent.clone(),
        version: version.clone(),
        generated_at: chrono::Utc::now().to_rfc3339(),
        source: source.clone(),
        names: merged.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
    };

    // `_coverage` 是**额外字段**：`TableNames` 未标 `deny_unknown_fields`
    // ⇒ 消费方仍能原样反序列化（本文件**不改** `names.rs` 的结构定义）。
    let mut doc = serde_json::to_value(&table)?;
    let cov = serde_json::json!({
        "generator": agent,
        "generator_version": version,
        "model": model_used,
        "schema": report.schema,
        "schema_tables": schema_tables.len(),
        "translated": translated_count,
        "carried_over": carried_over,
        "attempted": plan.to_translate.len(),
        "regenerated": regenerated,
        "rewritten": plan.rewritten,
        "regenerate_all": args.regenerate_all,
        "untranslated": untranslated,
        "absent_from_schema": absent,
        "degenerate_tokens": degenerate,
    });
    doc.as_object_mut()
        .context("产物不是 JSON 对象")
        .expect("TableNames 序列化后必是对象")
        .insert("_coverage".to_string(), cov);
    let text = serde_json::to_string_pretty(&doc)?;

    // ── 覆盖率汇总：**在缺口闸与 `TableNames::load` 之前**打印 ──
    //
    // 这一行必须在**每一条**提前返回的路径之前出现 ——
    // 否则缺口那条路径（EXIT=2）脚本什么也读不到，「覆盖率可数」就只在成功路径成立；
    // 而校验失败那条路径（EXIT=1）同样读不到覆盖率，等于「最坏的情况信息最少」。
    println!("translated {translated_count} / total {}", schema_tables.len());

    // ── 缺口闸：**必须排在 `TableNames::load` 之前** ──
    //
    // ⚠️ 顺序是有语义的，不是风格问题：
    // 100% 缺口 ⇒ `names` 为空 ⇒ `load` 先 `bail("翻译表是空的")`
    // ⇒ 被 `main` 归成 **1**（=「我的命令写错了，去改命令行」），
    // 而**命令完全正确**，是数据一条都没翻出来 —— 1 与 2 重载，
    // 正是本需求要消灭的情况；而且那条路径上汇总行与缺口清单**双双消失**。
    //
    // 闸门走**独立的退出码 2**（不 bail：bail 会被 `main` 归成 1），
    // 并把缺口清单打到 stderr —— 没写产物时，stderr 是脚本唯一能读到的出口。
    let has_gaps = !untranslated.is_empty();
    if gap_gate_fires(has_gaps, args.allow_gaps, merged.is_empty()) {
        // `--allow-gaps` 已经给过时，「加 --allow-gaps」是**错的方向** ⇒ 换一句说。
        let advice = if args.allow_gaps {
            "处置：--allow-gaps 已经给了也**依然写不出产物**（原因见下方 ⚠）——\n\
             要落盘，就得让模型至少给出**一条**可用译名，\n\
             或先用 --names 喂一份手工旧译做增量。"
                .to_string()
        } else {
            format!(
                "处置：查上面的表名（模型没给出可用译名），或显式加 --allow-gaps 先落盘\n\
                 再人工补译 —— 届时退出码是 {EXIT_REVIEW_NEEDED}（产物已写出但仍需人过目），不是 0。"
            )
        };
        eprintln!(
            "有 {} 张表没翻出来 ⇒ **不写文件**（退出码 {}：命令没问题，不完整的是数据）。\n\
             表名：{:?}\n\
             \n\
             为什么默认硬失败：`ontok-propose` 对查不到中文名的表是**静默丢弃**\n\
             （记进 stats.tables_without_cn_name，没有 stderr、没有非零退出码），\n\
             一份残缺的翻译表会让抽取器整批丢实体而**没人看见**。\n\
             \n\
             {}",
            untranslated.len(),
            EXIT_GAPS_NOT_ACKNOWLEDGED,
            untranslated,
            advice
        );
        // 退化自检**同样**要在这条路径上可见：这里没写产物，
        // `_coverage.degenerate_tokens` 也就无处可读 ⇒ 只能落 stderr（AC-002：
        // 缺口与退化都必须**人和脚本同时**能读到，不得只能靠人眼看日志）。
        if !degenerate.is_empty() {
            eprintln!(
                "⚠ 同时有 {} 条中文名退化（词元撞上实体 key，**下游会把这些对象整个丢掉**）：",
                degenerate.len()
            );
            for d in &degenerate {
                eprintln!("    {} → {}", d["table"], d["cn_name"]);
            }
        }
        if merged.is_empty() {
            eprintln!(
                "⚠ 本次**一张都没翻出来**（translated 0 / {}）。这不是命令写错了 —— \
                 命令是对的，是模型一条可用译名都没给出。\n\
                 ⚠ 即便加了 --allow-gaps 也**写不出产物**：空表过不了 TableNames::load \
                 （「翻译表是空的」），所以没有 `_coverage` 可读 —— \
                 上面的缺口清单与本行就是脚本在这个场景下**全部**能拿到的东西。",
                schema_tables.len()
            );
        }
        return Ok(EXIT_GAPS_NOT_ACKNOWLEDGED);
    }

    // ★ AC-001：本文件**唯一**的校验来源。写文件之前先过它 ——
    // `load` 不过就不写，坏产物连落地都不落地。
    //
    // ⚠️ 它排在缺口闸**之后**是有意的：两者同时成立时先报缺口（2），
    // 因为缺口是更可操作的那个；补齐缺口后重跑，`load` 的结论自然会浮出来（1）。
    TableNames::load(&text).context("产物未通过 TableNames::load 的校验（**未写出任何文件**）")?;

    if let Some(parent) = std::path::Path::new(&out).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("建不出目录 {}", parent.display()))?;
        }
    }
    std::fs::write(&out, &text).with_context(|| format!("写不出 {out}"))?;

    // ── 人看的明细（stdout 上方那一行是脚本读的）──
    eprintln!("产出      : {out}");
    eprintln!("来源      : {source}");
    eprintln!("翻译来源  : {agent} v{version}");
    eprintln!(
        "沿用旧译 / 本轮新翻 / 送模型张数 : {} / {} / {}",
        carried_over,
        regenerated,
        plan.to_translate.len()
    );
    if !untranslated.is_empty() {
        eprintln!(
            "未翻译（{} 张，已写进 _coverage.untranslated）: {}",
            untranslated.len(),
            untranslated.join(", ")
        );
    }
    if !absent.is_empty() {
        eprintln!(
            "⚠ schema 里已消失但**仍保留在 names 里**（{} 张，删除需人工处理，见 _coverage.absent_from_schema）: {}",
            absent.len(),
            absent.join(", ")
        );
    }
    if !degenerate.is_empty() {
        eprintln!(
            "⚠ 退化自检：{} 条中文名的词元撞上实体 key，客户视图里会**整个对象消失**（见 _coverage.degenerate_tokens）",
            degenerate.len()
        );
        for d in &degenerate {
            eprintln!("    {} → {}", d["table"], d["cn_name"]);
        }
    }
    eprintln!(
        "注：本次译文**未经人工确认**。质量只能人看（AC-007 / 限制 L-1）—— 清单见 _coverage 与产物本体。"
    );

    // ⚠️ 退化自检**进**退出码：`degenerate_tokens` 非空 ⇒ 不是 0。
    // 这样的中文名过不了 `usable_as_human_name`，下游会把这些表对应的对象
    // **静默丢掉**（只往 stats 塞计数，不报错）——
    // 明知道会导致下游丢表却报成功，与缺口报 0 是同一类毛病。
    Ok(coverage_exit(
        !untranslated.is_empty(),
        !absent.is_empty(),
        !degenerate.is_empty(),
        true,
    ))
}

// ══════════════════════════════════════════════════════════════
// 单测（纯函数层 —— 不联网、不花钱、不碰客户资产）
// ══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn tabs(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    /// AC-003 · 新表补、旧表留：只在**缺名**的表上翻译。
    #[test]
    fn plan_translates_only_missing_tables_and_never_asks_to_delete() {
        let schema = tabs(&["t_a", "t_b", "t_c"]);
        let existing = BTreeMap::from([
            ("t_a".to_string(), "甲".to_string()),
            ("t_b".to_string(), "乙".to_string()),
        ]);
        let plan = plan_translations(&schema, &existing, &BTreeSet::new(), false).unwrap();
        assert_eq!(plan.to_translate, tabs(&["t_c"]));
        assert!(plan.rewritten.is_empty(), "新表不是「重翻」");
    }

    /// AC-003 · 旧条目**逐条留下**：`merge_names` 没有任何删除分支。
    #[test]
    fn merge_keeps_entries_absent_from_schema() {
        let existing = BTreeMap::from([
            ("t_a".to_string(), "甲".to_string()),
            ("t_gone".to_string(), "已删表".to_string()),
        ]);
        let translated = BTreeMap::from([("t_new".to_string(), "新表".to_string())]);
        let merged = merge_names(&existing, &translated);
        assert_eq!(merged.get("t_a"), Some(&"甲".to_string()));
        assert_eq!(
            merged.get("t_gone"),
            Some(&"已删表".to_string()),
            "schema 里已消失的表必须**留在 names 里**，删除是人的显式动作"
        );
        assert_eq!(merged.get("t_new"), Some(&"新表".to_string()));
    }

    /// AC-003 · 同一份输入跑两遍，增量后旧条目一个字节都不变。
    #[test]
    fn second_run_leaves_earlier_translations_byte_identical() {
        let schema1 = tabs(&["t_a", "t_b", "t_c"]);
        let existing = BTreeMap::from([("t_a".to_string(), "甲".to_string())]);
        let plan1 = plan_translations(&schema1, &existing, &BTreeSet::new(), false).unwrap();
        let r1 = translate_from_map(plan1, [("t_b", "乙"), ("t_c", "丙")]);
        let merged1 = merge_names(&existing, &r1);

        // 第二遍：schema 加了 t_d / t_e，**删了** t_legacy
        let schema2 = tabs(&["t_a", "t_b", "t_d", "t_e"]);
        let plan2 = plan_translations(&schema2, &merged1, &BTreeSet::new(), false).unwrap();
        let r2 = translate_from_map(plan2, [("t_d", "丁"), ("t_e", "戊")]);
        let merged2 = merge_names(&merged1, &r2);

        for t in ["t_a", "t_b", "t_c"] {
            assert_eq!(
                merged1.get(t),
                merged2.get(t),
                "旧翻译 {t} 在增量后必须**逐字节未变**"
            );
        }
        assert_eq!(merged2.get("t_d"), Some(&"丁".to_string()));
        assert_eq!(merged2.get("t_e"), Some(&"戊".to_string()));
        assert_eq!(
            merged2.get("t_c"),
            Some(&"丙".to_string()),
            "schema 里已消失的表仍留在 names 里"
        );
        let (untranslated, absent) = coverage(&schema2, &merged2);
        assert!(untranslated.is_empty());
        assert_eq!(absent, tabs(&["t_c"]), "且它必须出现在清单里");
    }

    fn translate_from_map(
        plan: Plan,
        pairs: impl IntoIterator<Item = (&'static str, &'static str)>,
    ) -> BTreeMap<String, String> {
        let r: BTreeMap<String, String> =
            pairs.into_iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        r.into_iter()
            .filter(|(k, _)| plan.to_translate.contains(k))
            .collect()
    }

    /// AC-003 · `--regenerate-all` 重翻全部，但**已消失的表照样不删**。
    #[test]
    fn regenerate_all_retranslates_but_never_deletes() {
        let schema = tabs(&["t_a", "t_b"]);
        let existing = BTreeMap::from([
            ("t_a".to_string(), "旧甲".to_string()),
            ("t_gone".to_string(), "已删表".to_string()),
        ]);
        let plan = plan_translations(&schema, &existing, &BTreeSet::new(), true).unwrap();
        assert_eq!(plan.to_translate, tabs(&["t_a", "t_b"]));
        assert_eq!(plan.rewritten, tabs(&["t_a"]));
        let merged = merge_names(&existing, &BTreeMap::from([("t_a".to_string(), "新甲".to_string())]));
        assert_eq!(merged.get("t_a"), Some(&"新甲".to_string()));
        assert_eq!(
            merged.get("t_gone"),
            Some(&"已删表".to_string()),
            "--regenerate-all 只管**重翻**，删除不是它的职责"
        );
    }

    /// `--overwrite` 只重翻点名的那张；拼错的表名直接报错（防"以为重翻过了"）。
    #[test]
    fn overwrite_is_scoped_and_typos_are_rejected() {
        let schema = tabs(&["t_a", "t_b"]);
        let existing = BTreeMap::from([
            ("t_a".to_string(), "甲".to_string()),
            ("t_b".to_string(), "乙".to_string()),
        ]);
        let plan = plan_translations(&schema, &existing, &BTreeSet::from(["t_b".to_string()]), false).unwrap();
        assert_eq!(plan.to_translate, tabs(&["t_b"]));
        assert_eq!(plan.rewritten, tabs(&["t_b"]));
        let err = plan_translations(&schema, &existing, &BTreeSet::from(["t_typo".to_string()]), false)
            .expect_err("拼错的表名必须报错");
        assert!(format!("{err:#}").contains("t_typo"));
    }

    /// AC-001 · 缺口清单：两个数组各自算各自的，**互不吞并**。
    #[test]
    fn coverage_reports_untranslated_and_absent_separately() {
        let schema = tabs(&["t_a", "t_b", "t_c"]);
        let names = BTreeMap::from([
            ("t_a".to_string(), "甲".to_string()),
            ("t_gone".to_string(), "没了".to_string()),
        ]);
        let (untranslated, absent) = coverage(&schema, &names);
        assert_eq!(untranslated, tabs(&["t_b", "t_c"]));
        assert_eq!(absent, tabs(&["t_gone"]));
    }

    /// 退出码语义：「命令错了」与「数据不完整」**不许共用一个码** ——
    /// 两者的处置方向完全相反（改命令 vs 处理数据），脚本只凭退出码就该能分开。
    /// 另外「有缺口但**没写**产物」与「有缺口且**已写**产物」也分开，
    /// 「退化自检命中」也不许拿 0（下游会静默丢对象）。
    #[test]
    fn exit_codes_separate_command_error_from_data_incompleteness() {
        // 全覆盖无残留无退化 —— **唯一**能拿 0 的情形
        assert_eq!(super::coverage_exit(false, false, false, true), EXIT_OK);
        // 有缺口且已写出（--allow-gaps）⇒ 3
        assert_eq!(super::coverage_exit(true, false, false, true), EXIT_REVIEW_NEEDED);
        // 有缺口但**没写**（未承认）⇒ 2，与上一行**不同**
        assert_eq!(super::coverage_exit(true, false, false, false), EXIT_GAPS_NOT_ACKNOWLEDGED);
        // 无缺口但有残留（已消失的表）⇒ 3
        assert_eq!(super::coverage_exit(false, true, false, true), EXIT_REVIEW_NEEDED);
        // 无缺口无残留，**只**有退化自检命中 ⇒ 3，⛔ 绝不是 0（QA 的 F6）
        assert_eq!(super::coverage_exit(false, false, true, true), EXIT_REVIEW_NEEDED);
        assert_ne!(
            super::coverage_exit(false, false, true, true),
            EXIT_OK,
            "退化自检命中时报成功 = 骗人：这些名字过不了 usable_as_human_name，下游会静默丢表"
        );
        // 四个码两两不同 —— 重载即红
        let all = [EXIT_OK, EXIT_COMMAND_ERROR, EXIT_GAPS_NOT_ACKNOWLEDGED, EXIT_REVIEW_NEEDED];
        for (i, a) in all.iter().enumerate() {
            for (j, b) in all.iter().enumerate() {
                assert!(i == j || a != b, "退出码 {a} 与 {b} 重载了");
            }
        }
    }

    /// QA 的 F2：**100% 缺口**（一张都没翻出来）必须走缺口闸，
    /// 绝不能让 `TableNames::load` 的「翻译表是空的」先 `bail` 掉 ⇒ 退出码 1。
    /// 1 的语义是「去修命令」，而这里命令完全正确 —— 那是 1/2 重载。
    #[test]
    fn gap_gate_fires_on_total_translation_failure_so_it_is_never_exit_1() {
        // 100% 缺口：没给 --allow-gaps ⇒ 闸门拦下 ⇒ 2（**不是 1**）
        assert!(
            super::gap_gate_fires(true, false, true),
            "一张都没翻出来 ⇒ 拦下来，不写任何文件"
        );
        // 100% 缺口 + --allow-gaps：**仍然拦下**。空表过不了 load，写不出产物，
        // 所以既不是 3（3 = 产物已写出，这是撒谎）也不是 1（命令没问题）⇒ 仍是 2。
        assert!(
            super::gap_gate_fires(true, true, true),
            "--allow-gaps 买不到「空表也能落盘」；无产物可写就只能是 2"
        );
        // 部分缺口 + --allow-gaps ⇒ 放行（写残缺产物，退出码 3）
        assert!(!super::gap_gate_fires(true, true, false));
        // 部分缺口 + 没承认 ⇒ 拦下
        assert!(super::gap_gate_fires(true, false, false));
        // 无缺口 ⇒ 闸门永不触发（哪怕给了 --allow-gaps）
        assert!(!super::gap_gate_fires(false, false, false));
        assert!(!super::gap_gate_fires(false, true, false));
    }

    /// F2 的完整链路（纯函数层）：3 张表、模型一条都没回 ⇒
    /// `untranslated` 覆盖全部 3 张、`merged` 为空（`names` 必是空表）、
    /// 闸门拦下 ⇒ 退出码 2，而不是被空表校验归成 1。
    #[test]
    fn total_translation_failure_yields_code_2_and_a_full_gap_list() {
        let schema = tabs(&["t_a", "t_b", "t_c"]);
        let merged = merge_names(&BTreeMap::new(), &BTreeMap::new());
        let (untranslated, _absent) = coverage(&schema, &merged);
        assert_eq!(untranslated.len(), 3, "缺口清单必须**完整**（含全部 3 张表）");
        assert!(merged.is_empty(), "names 为空 ⇒ 空表过不了 TableNames::load");
        assert!(super::gap_gate_fires(!untranslated.is_empty(), false, merged.is_empty()));
        // 闸门拦下 ⇒ 没写产物 ⇒ 落在 2；⛔ 绝不是 1（= 去修命令）
        assert_eq!(
            super::coverage_exit(!untranslated.is_empty(), false, false, false),
            EXIT_GAPS_NOT_ACKNOWLEDGED
        );
    }

    /// 记账不许虚高：从零生成、5 张表只翻出 3 张时，
    /// 「沿用旧译」必须**是 0 而不是 5**（压根没有旧译可沿用）。
    #[test]
    fn carried_over_never_inflates_when_there_is_no_previous_table() {
        let schema = tabs(&["t_a", "t_b", "t_c", "t_d", "t_e"]);
        let plan = plan_translations(&schema, &BTreeMap::new(), &BTreeSet::new(), false).unwrap();
        assert_eq!(plan.to_translate.len(), 5);
        let translated = BTreeMap::from([
            ("t_a".to_string(), "甲".to_string()),
            ("t_b".to_string(), "乙".to_string()),
            ("t_c".to_string(), "丙".to_string()),
        ]);
        let merged = merge_names(&BTreeMap::new(), &translated);
        let (untranslated, _absent) = coverage(&schema, &merged);
        let translated_count = schema.len() - untranslated.len();
        assert_eq!(translated_count, 3);
        assert_eq!(translated_count - translated.len(), 0, "无旧译 ⇒ 沿用数必须是 0");
    }

    // ── AC-007 · 只查退化 ──

    /// `tokenize` 与 `view.rs` 同规则：CJK **不**被切碎。
    /// ⛔ 这条只钉**副本自己**的规则，**不**证明它与 `src/domain/view.rs` 的原件相等
    /// —— 两份可以各自全绿而漂移（见 [`tokenize`] 的限制说明）。
    #[test]
    fn tokenize_keeps_han_runs_whole() {
        assert_eq!(tokenize("客户订单"), vec!["客户订单"]);
        assert_eq!(tokenize("订单 Order"), vec!["订单", "order"]);
        assert_eq!(
            tokenize("robot_device"),
            vec!["robot_device"],
            "**下划线不是分隔符**（view.rs 的同一规则）——所以 key `t_customer` 是**一个**词元"
        );
    }

    /// 中英混排 + 库里恰有 key `order` ⇒ 整个对象会被 `usable_as_human_name` 丢弃。
    #[test]
    fn degenerate_detects_token_equal_to_entity_key() {
        let keys = entity_keys(&tabs(&["order", "t_customer"]));
        assert_eq!(degenerate_tokens("订单 Order", &keys), vec!["order"]);
        assert!(degenerate_tokens("订单", &keys).is_empty());
        assert!(
            degenerate_tokens("客户订单", &keys).is_empty(),
            "「客户订单」是正常翻译 —— **不许**因为含「订单」就判退化"
        );
    }

    /// `TableNames` 未标 `deny_unknown_fields` ⇒ 额外字段能被消费方原样吞下。
    /// 这正是 `_coverage` 的承载依据（AC-002 / 限制 L-4）。
    #[test]
    fn extra_coverage_field_still_deserializes_as_table_names() {
        let doc = serde_json::json!({
            "agent": "bin:x",
            "version": "0.1.0",
            "generated_at": "2026-01-01T00:00:00Z",
            "source": "public schema, 1 tables (introspected t)",
            "names": {"t_a": "甲"},
            "_coverage": {"untranslated": []},
        });
        let text = serde_json::to_string(&doc).unwrap();
        let tn = TableNames::load(&text).expect("带额外字段仍必须能被 load 接受");
        assert_eq!(tn.lookup("t_a"), Some("甲"));
    }

    /// 解析模型正文：剥代码块围栏、丢掉**编造的表名**、丢掉空名。
    #[test]
    fn parse_translations_drops_fences_blank_and_invented_tables() {
        let body = "```json\n{\"names\": {\"t_a\": \" 甲 \", \"t_b\": \"  \", \"t_ghost\": \"幽灵\"}}\n```";
        let got = parse_translations(body, &tabs(&["t_a", "t_b"]));
        assert_eq!(got.get("t_a"), Some(&"甲".to_string()));
        assert!(!got.contains_key("t_b"), "空名等于没翻，不收");
        assert!(
            !got.contains_key("t_ghost"),
            "模型编出来的表名会凭空多出一条缺口，必须丢掉"
        );
    }
}

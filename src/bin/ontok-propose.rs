//! 抽取 CLI：schema JSON → 本体提案（REQ-003）
//!
//! ```bash
//! cargo run --bin ontok-propose -- \
//!     --schema out/schema.json \
//!     --names   src/extract/table_names.json \
//!     --out     out/proposal.json \
//!     --stats   out/extract-stats.json
//! ```
//!
//! 默认路径**只读本地文件，不连任何数据库** —— 网络访问发生在上一步 `ontok-introspect`。
//!
//! ⚠️ **REQ-019 起有一条例外**：第 2 轮（`--round ≥ 2`）会连
//! `ONTOK_DATABASE_URL` **只读**基版本的 `object_types`，填进阶段 1 的
//! 「已有类型清单」。不载入的话，REQ-014 的 key 双向核对在生产上一次都跑不到
//! （清单恒空 ⇒ 三种违规不可达）。第 1 轮仍然完全不连库。
//!
//! # REQ-059：`--agent` 不再是标签，而是**断言**
//!
//! 过去 `--agent` 默认 `"mcode-agent"`、`--agent-version` 默认 `"unknown"`
//! （且那个 `"unknown"` 会被**翻译表的版本号**顶替），而这条路径真正跑的是
//! `inproc`。于是产物里 `extracted_by` 记着两个**与执行者无关**的值：
//! `agent` 是硬编码占位，`version` 直接借自翻译表 —— 一份**自证**的假溯源，
//! 偏偏它的名字还与「翻译表由谁产出」长得几乎一样。
//!
//! 现在：
//!
//! - **不带** `--agent-registry`（默认路径）⇒ `extracted_by.agent` / `.version`
//!   记的是**真正跑起来的那个实现自报的身份**
//!   （[`ontok::agent::factory::INPROC_EXECUTOR_ID`] 及其版本），不连库；
//! - **带** `--agent-registry` ⇒ 仍是库里登记的**真实登记信息**
//!   （规格 §8.4：「具体版本号，不是『最新版』」）；
//! - `--agent` / `--agent-version` **不再是标签而是断言**：给出就必须与事实相符，
//!   对不上 ⇒ **拒绝出产物**（不再「只警告、产物照出」）。
//!
//! 与 `--agent-registry` 同时给会**直接报错**：两套来源同时存在时，
//! 溯源到底是按谁的记就成了猜 —— 溯源猜错正是 §8.4 要防的事故，所以宁可拒绝跑。
//!
//! ⚠️ **这一条会改变产物内容**：`proposal.json` 的 `extracted_by.agent` /
//! `extracted_by.version` 两个字段的取值变了（`items` / `stats` 一个字节都不动）。

use anyhow::{Context, Result};
use ontok::agent::factory::{
    executor_name_collision, identity_cross_check, INPROC_EXECUTOR_ID, INPROC_EXECUTOR_VERSION,
};
use ontok::agent::orchestrate::{
    orchestrate, ExtractionRun, OrchestrationInput, DEFAULT_ATTEMPTS, DEFAULT_BASE_VERSION,
};
use ontok::agent::registry::{AgentIdentity, AgentRegistration};
use ontok::agent::{build_agent, Assembly, ExistingType, UsageBudget};
use ontok::extract::exclusions::{Entry, Exclusions};
use ontok::extract::introspect::{KeyCatalog, SchemaReport, SOURCE_DATABASE_URL_ENV};
use ontok::extract::names::TableNames;

#[tokio::main]
async fn main() -> Result<()> {
    let mut schema_path = String::from("out/schema.json");
    let mut names_path = String::from("src/extract/table_names.json");
    let mut out = String::from("out/proposal.json");
    let mut stats_out = String::from("out/extract-stats.json");
    let mut project = String::from("extracted");
    // REQ-059：`--agent` / `--agent-version` 不再有**默认值**。
    //
    // 过去这里是 `String::from("mcode-agent")` / `String::from("unknown")`，
    // 然后 `unknown` 还会被**翻译表的版本号**顶替（见下面注释）——
    // 两个都不是执行者的事实，却以「默认如此」的姿态进了产物。
    // 现在不传就是 `None`，「调用方没声明」与「调用方声明了」因此可分辨，
    // 而无声明时的身份由**真正跑起来的那个实现**给出（`INPROC_EXECUTOR_ID`）。
    // 叫 `declared_*` 而不是 `agent` / `agent_version`：下面 `let agent = build_agent(..)`
    // 会**遮蔽**同名变量。REQ-059 之前这个遮蔽是无害的（参数只在遮蔽前被 clone 走），
    // 现在需要在**造出实现之后**拿声明值去比对实际值，遮蔽就会把声明值吃掉 ——
    // 而且吃掉的报错是 `Box<dyn ExtractionAgent>` 上没有 `as_deref`，很难一眼看懂。
    let mut declared_agent: Option<String> = None;
    let mut declared_version: Option<String> = None;
    // REQ-013：agent/版本/轮次改由 registry 给出时为真。
    let mut registry_agent: Option<String> = None;
    let mut registry_version: Option<String> = None;
    let mut round: i32 = 1;
    // REQ-019：每组尝试次数**真的可配**。REQ-015 时这一行被钉死成 1，
    // 于是 `drive_group` 的重试在生产路径上从不生效（详见 DEFAULT_ATTEMPTS）。
    let mut attempts: u32 = DEFAULT_ATTEMPTS;
    // REQ-022 · 成本闸门：外置 agent 必须提前设闸（规格 §7.1 / ADR-002 N4）。
    // 不给就是「不设闸」—— 对零成本实现（`inproc`）合理；
    // 对声明了「我会上报用量」的实现，编排入口会**直接拒绝**（义务不靠自觉）。
    let mut budget_tokens: Option<u64> = None;
    // REQ-108 · 置信度接线（AC-002 / AC-003 / AC-006）
    //
    // 采样**默认开**：不给 `--no-sample` 就试着采。但「试着」不等于「必须成功」——
    // 没有源库连接串时**不报错**（既有调用点 `ontok-propose` 一轮都不连库），
    // 而是所有关系标 `data_evidence: none`，并且**大声说出来**。
    // 为什么是「说」而不是「静默继续」：缺少证据 ≠ 有证据（R1 ④），
    // 静默继续会让一份「没验过」的提案看起来和验过的一样。
    let mut no_sample = false;
    let mut src_url: Option<String> = None;
    // REQ-108 · AC-009：待查清单 / 拒绝清单的落点。默认与 `--out` 同目录。
    let mut pending_out: Option<String> = None;
    let mut rejected_out: Option<String> = None;
    // REQ-109 · AI 语义项（**默认关**，设计概要 §五）
    let mut llm = false;
    let mut refresh = false;
    // 分阶段上线用：只判关系键包含该子串的那些（阶段 1 的「先小样本」）。
    // 不改变任何判据 —— 没被判的关系语义缺失（不是 0 分）。
    let mut llm_only: Option<String> = None;
    // 分阶段上线用：最多送前 N 条关系给 AI（**只截断请求**，不改变判据）。
    let mut llm_limit: Option<usize> = None;
    // REQ-113 · 排除表：**数据**由产品执行，不是调用方先跑脚本过滤一遍。
    //
    // 不给 `--exclude` ⇒ 行为与 REQ-112 逐字节相同（提案里没有 `inputs.exclusions`）。
    // ⭐ REQ-118 · A-1 起，`inputs.names` **恒出现**（每次抽取都基于某份翻译表）；
    // 这是有意的契约变更：名字的溯源不能再无据可查。`inputs.exclusions` 仍只在
    // 给了 `--exclude` 时出现。
    let mut exclude_path: Option<String> = None;
    // 陈旧排除（文件里点名、schema 里没有）默认只警告；这个开关把它变成**硬失败**。
    let mut strict_exclusions = false;
    // ── REQ-120 · FK 边补全（三段式：提名 → 值域重叠 → 提案 → 人签 → 落库）──
    //
    // `--fk-out`    : 生成 `fk-proposals.json`（**未签**，不进快照）
    // `--fk-sign`   : 读一份**签过字**的同一形状文件，把边注入本轮提案
    // `--fk-unresolved-out`: 未解析清单落点（默认与 --fk-out 同目录）
    //
    // 三者都**默认不出现在命令行** ⇒ 行为与 REQ-119 逐字节相同（除了下面
    // 那条「唯一列 ⇒ 1:1」的基数修正，它是 R4 的硬要求，不带开关）。
    let mut fk_out: Option<String> = None;
    let mut fk_sign: Option<String> = None;
    let mut fk_unresolved_out: Option<String> = None;

    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut val = || args.next().with_context(|| format!("{a} 需要一个参数"));
        match a.as_str() {
            "--schema" => schema_path = val()?,
            "--names" => names_path = val()?,
            "--out" => out = val()?,
            "--stats" => stats_out = val()?,
            "--project" => project = val()?,
            "--agent" => declared_agent = Some(val()?),
            "--agent-version" => declared_version = Some(val()?),
            "--agent-registry" => registry_agent = Some(val()?),
            "--agent-registry-version" => registry_version = Some(val()?),
            "--round" => {
                round = val()?.parse().context("--round 要一个整数（第几轮抽取）")?;
                if round < 1 {
                    anyhow::bail!("--round 必须是 ≥1 的整数（规格 §8.4 的 round 是「第几轮抽取」）");
                }
            }
            // REQ-022 · 成本闸门
            "--budget-tokens" => {
                budget_tokens = Some(
                    val()?
                        .parse()
                        .context("--budget-tokens 要一个非负整数（累计用量上限，单位由协议自己定）")?,
                );
            }
            // REQ-108 · R1 ④：整个采样可关（关掉时 confidence 只由结构信号算）
            "--no-sample" => no_sample = true,
            // REQ-108 · 源库连接串（**只从参数/环境变量来，绝不写进任何文件**）
            "--src" => src_url = Some(val()?),
            // REQ-108 · AC-009：两个清单的落点（默认与 --out 同目录）
            "--pending-out" => pending_out = Some(val()?),
            "--rejected-out" => rejected_out = Some(val()?),
            // REQ-109 · AI 语义项（默认关；开就必须有三件环境变量 + 预算）
            "--llm" => llm = true,
            "--refresh" => refresh = true,
            "--llm-only" => {
                let v = val()?;
                if v.trim().is_empty() {
                    anyhow::bail!("--llm-only 需要非空的关系键子串（例如 llm_model）");
                }
                llm_only = Some(v);
            }
            "--llm-limit" => {
                let n: usize = val()?.parse().context("--llm-limit 要一个正整数（最多送几条关系给 AI）")?;
                if n == 0 {
                    anyhow::bail!("--llm-limit 必须 ≥1（0 等于「一条都不判」，那与不开 AI 混淆）");
                }
                llm_limit = Some(n);
            }
            // REQ-113 · 排除表
            "--exclude" => exclude_path = Some(val()?),
            "--strict-exclusions" => strict_exclusions = true,
            // REQ-120 · FK 边补全
            "--fk-out" => fk_out = Some(val()?),
            "--fk-sign" => fk_sign = Some(val()?),
            "--fk-unresolved-out" => fk_unresolved_out = Some(val()?),
            // REQ-019 · 规格 §7.1 设计点⑤：「校验失败重试 1–2 次，仍失败才标记抽取失败」。
            // REQ-015 把 attempts 钉死在 1，那条规格在生产上就没兑现过。
            "--attempts" => {
                attempts = val()?
                    .parse()
                    .context("--attempts 要一个整数（每组抽取的尝试次数，含第一次）")?;
                if attempts < 1 {
                    anyhow::bail!("--attempts 必须是 ≥1 的整数（0 次等于「根本没抽过却报成功」，设计点⑤禁止）");
                }
            }
            "-h" | "--help" => {
                println!("schema JSON → 本体提案（本地计算，不连库）\n  --schema / --names / --out / --stats / --project");
                println!("REQ-118 · 名字溯源（P4）：\n  --names <file>  翻译表。提案元信息**恒记** inputs.names =\n                  {{file, sha256, agent, version, reviewed_by}}（+ 有 _proposal 时带上原始提案的 file/sha256）；\n                  reviewed_by 取翻译表 `_proposal.reviewed_by`，没有这一块就是 null（字段仍存在）");
                println!("REQ-059 抽取归属（extracted_by 记的是**真实执行者**，不是调用方填的占位）:\n  --agent <name> / --agent-version <ver>\n                现在是**断言**而不是标签：不带就由真正跑起来的实现自报（{INPROC_EXECUTOR_ID} v{INPROC_EXECUTOR_VERSION}）；\n                带上就必须与事实相符，对不上**拒绝出产物**。\n                要登记一个自己的名字，请用下面的 --agent-registry（查得到才放行）。");
                println!("REQ-013 agent-registry（带上才连 ONTOK_DATABASE_URL）:\n  --agent-registry <name> / --agent-registry-version <ver> / --round <n>");
                println!("REQ-019:\n  --attempts <n>  每组抽取的尝试次数（默认 {DEFAULT_ATTEMPTS}）\n  --round ≥2 时会连库载入基版本的已有类型（规格 §7.1 阶段 1 的五个输入之一）");
                println!("REQ-022:\n  --budget-tokens <n>  成本闸门：累计用量上限，超了**停**并上报\n                （外置 agent 的强制义务；没设闸又声明会上报用量 ⇒ 入口拒绝）");
                println!("REQ-108 置信度:\n  --src <url>  源库连接串（只读会话；也可用环境变量 SRC_URL / {SOURCE_DATABASE_URL_ENV}）\n  --no-sample  关闭采样：confidence 只由结构信号算，且全部标 data_evidence=none（缺数据证据不进主提案）\n  --pending-out / --rejected-out  待查清单 / 拒绝清单的落点（默认与 --out 同目录的 proposal-pending.json / proposal-rejected.json）");
                println!("REQ-113 排除表（**数据**由产品执行，不是先跑脚本过滤）:\n  --exclude <file>      排除文件（version / rule / declared_by / exclude:[\
{{table, category, reason, decided_by}}]）；被排除的表**不建实体**，且在抽取前就剔除\n\
                        \x20 理由为空 / 类别不在枚举内 / 缺决策人 ⇒ **拒绝文件**（exit≠0）\n\
                        \x20 stats.json 记 tables_seen / tables_excluded / tables_extracted，且必须满足\n\
                        \x20 seen - excluded == extracted（不闭合 ⇒ 不写出统计）\n\
                        \x20 提案元信息记 inputs.exclusions = {{file, version, sha256}}（+ 逐条记录）\n\
                  --strict-exclusions  文件里点名、schema 里没有的表（陈旧排除）⇒ exit≠0；\n\
                        \x20 不给这个开关时只警告 + 记进 stats.stale_exclusions");
                println!("REQ-109 AI 语义项（**默认关**）:\n  --llm        打开 AI 语义判断（semantic_fit 权重 0.15，method 变 evidence-scorecard-v2）\n               开 ⇒ 必须有 {base} / {key} / {model}，否则 exit≠0 并点名缺哪个（R4）\n  --budget-tokens <n>  --llm 时**必填且 >0**；超预算即停并按「已用/上限/停在第几条」报告（R5）\n  --refresh    忽略已有缓存并覆盖（迭代提示词时用）；会打印「强制刷新 N 条」（R3）\n  --llm-only <substr>  只判关系键包含该子串的关系（分阶段上线的闸，其余按语义缺失处理）\n  --llm-limit <n>      最多送 n 条关系给 AI（分阶段上线的闸，**只截请求**，不改变判据）",
                    base = ontok::extract::ai_semantic::BASE_URL_ENV,
                    key = ontok::agent::llm::API_KEY_ENV,
                    model = ontok::agent::llm::MODEL_ENV);
                println!(
                    "REQ-120 FK 边补全（**默认关**；三段式：提名 → 值域重叠 → 提案 → 人签 → 落库）:\n\
                     \x20 --fk-out <file>          对每个 *_id 列生成候选 + 采样验命中率，写 ADR-005 提案（**未签**，不进快照）；需要 --src\n\
                     \x20 --fk-sign <file>         读一份**签过字**（reviewed_by 非空）的同形状文件，把边注入本轮提案（未签的一条不进）\n\
                     \x20 --fk-unresolved-out <f>  未解析清单落点（默认与 --fk-out 同目录的 fk-unresolved.json）\n\
                     \x20 判据：命中率 ≥0.95 strong · 0.5–0.95 medium（需人签）· <0.5 **不产出**；样本 <3 条 ⇒ insufficient_sample\n\
                     \x20 值域重叠是硬证据：⛔ 名字相似度只负责**提名**，不负责判决（AC-002）"
                );
                return Ok(());
            }
            other => anyhow::bail!("未知参数 {other}"),
        }
    }

    // ── REQ-113 · 排除文件：**先校验输入，再干活** ─────────────────────
    //
    // 位置是刻意的：在 schema / 翻译表 / LLM 三件套之前就把文件判掉。
    // 一份理由为空、类别拼错的 `exclusions.json` 若等到抽完才报，
    // 那一轮的代价（时间，开了 --llm 时还有钱）就白付了 —— 而它压根不是
    // 「跑起来才会知道」的问题。校验规则只有一处：
    // [`ontok::extract::exclusions::Exclusions::load`]。
    if strict_exclusions && exclude_path.is_none() {
        anyhow::bail!(
            "--strict-exclusions 只在 --exclude 下有意义：它说的是「陈旧排除（文件里点名、\
             但这份 schema 里没有）要直接失败」。没给排除文件时不存在陈旧项，\
             给这个开关只会让人以为发生过一次检查。"
        );
    }
    let exclusions = match &exclude_path {
        Some(p) => Some(Exclusions::load(p)?),
        None => None,
    };

    // ── REQ-109 · AI 开关的前置校验（**在干活之前**失败）────────────
    //
    // 三条都必须在读 schema 之前判完。AC-002 要的不是「抽完之后才发现没 key」，
    // 而是「开口就说清缺哪个变量、exit≠0」—— 晚失败会让一次运行白跑，
    // 更糟的是可能已经花掉了别的钱。
    //
    // 顺序是有意的：**先判环境变量**（R4 的「不许静默降级」），再判预算（R5），
    // 最后才要求产品库（缓存的落点）。这样「三件套缺一件」永远报得出来，
    // 不会被「你还没给预算」抢先盖住。
    let mut ai_budget: Option<u64> = None;
    let llm_settings = if llm {
        let settings = ontok::extract::ai_semantic::LlmSettings::from_env()?;
        ai_budget = Some(ontok::extract::ai_semantic::require_ai_budget(budget_tokens)?);
        ontok::config::require_env(ontok::config::DATABASE_URL_ENV).context(
            "--llm 需要 ONTOK_DATABASE_URL：AI 判断必须落 `ai_cache` 表，\
             否则「同指纹命中就不再调用」这条（R1/R2）无处安放 —— \
             每次重跑都会为同一批关系再付一次钱。",
        )?;
        Some(settings)
    } else {
        if refresh {
            anyhow::bail!(
                "--refresh 只在 --llm 下有意义：它说的是「忽略 AI 缓存并覆盖」。\n\
                 AI 没开时缓存压根不参与本轮，给这个开关只会让人以为发生过一次刷新。"
            );
        }
        None
    };

    let schema_raw = std::fs::read_to_string(&schema_path)
        .with_context(|| format!("读不到 {schema_path}（先跑 ontok-introspect）"))?;
    let mut report: SchemaReport =
        serde_json::from_str(&schema_raw).context("schema JSON 解析失败")?;

    // ── REQ-113 · 排除表：**在抽取之前**剔除（AC-001 / AC-002 / AC-005）──
    //
    // 为什么在这里、而不是「抽完再从 items 里删」：
    // 1. 抽完再删要保证实体、属性、关系、证据、置信度、TTL 六处都不剩 —— 漏一处
    //    就是噪声进了本体；
    // 2. 剔除后这些表也不在「已知表集合」里，于是没有 `xxx_id` 会再推断出一条
    //    指向噪声表的关系（抽完再删会留下悬空关系）。
    //
    // 判定本身**只有一处**：`Exclusions::apply`（纪律 H）。本文件只负责
    // 「给它的输入对不对」（文件已在参数解析后校验过）与「把结果记下来」。
    let tables_seen = report.tables.len();
    let mut exclusion_applied: Vec<Entry> = Vec::new();
    let mut exclusion_stale: Vec<String> = Vec::new();
    let mut exclusion_flagged: Vec<ontok::extract::exclusions::Flagged> = Vec::new();
    if let Some(ex) = &exclusions {
        let run = ex.apply(&report);
        // R4 · 陈旧排除：默认**警告 + 计数**，`--strict-exclusions` ⇒ 当场失败。
        //
        // 为什么默认不硬失败：库结构会变，硬失败会让一份老文件一夜之间不能用，
        // 于是人的反应是「把它从命令行摘掉」—— 那正好让排除**消失**。
        // 警告 + 计数仍然把「这条排除从没生效过」说出来。
        if strict_exclusions && !run.stale.is_empty() {
            anyhow::bail!(
                "陈旧排除 {} 条（--strict-exclusions ⇒ 失败）：{}\n\
                 \n\
                 这些表在 {} 里被点名排除，但这份 schema 里**没有**它们。\n\
                 两种可能，都得人来判：\n\
                 ① 源库已删掉 / 改名了这些表 —— 排除该跟着删（留着就是一份过期的决定）；\n\
                 ② 表名写错了 —— 这条排除**从来没生效过**，那批被排除的表里可能一直\
                 被当成业务表抽进了本体。\n\
                 \n\
                 不加 --strict-exclusions 时这只是警告（R4：库结构会变，硬失败会把\
                 老文件逼出命令行）。",
                run.stale.len(),
                run.stale.join("、"),
                ex.file(),
            );
        }
        if let Some(w) = run.stale_warning(ex.file()) {
            eprintln!("{w}");
        }
        exclusion_applied = run.applied.clone();
        exclusion_stale = run.stale.clone();
        exclusion_flagged = run.flagged.clone();
        // 之后的一切（分组、采样、AI、统计）都只看**这个作用域**。
        report = run.report;
    }

    // 翻译表是**知识资产**：它自己的 agent/version 记的是「这份翻译表由谁产出」，
    // 与「这次抽取由谁执行」是**两件不相干的事**。
    //
    // ⚠️ REQ-059：这里原来有一段
    //   `if agent_version == "unknown" { agent_version = names.version.clone(); }`
    // 把**翻译表的版本号**顶替成抽取 agent 的版本号，于是产物里
    // `extracted_by.version` 记的是「翻译表的版本」。
    // 同一屏输出里，「翻译表：mcode-agent (MiniMax Code) v1.2.0」与
    // 「extracted_by = mcode-agent / 1.2.0」于是**两处都像是同一件事** ——
    // 归属与知识来源互相冒充。现已删掉：抽取身份只能来自执行者或登记，
    // 翻译表的版本号不再流进 `extracted_by`。
    let names_raw = std::fs::read_to_string(&names_path)
        .with_context(|| format!("读不到翻译表 {names_path}"))?;
    let names = TableNames::load(&names_raw)?;
    // ⭐ REQ-118 · A-1（P4）：**产物侧**必须能查到「这份提案用的是哪份翻译表、谁签的」。
    //
    // 只把 `<file, sha256, agent, version, reviewed_by>` 记进 `inputs.names`——
    // ⛔ 不记整份 `names` 映射：`inputs` 是**元信息**段，不是第二份知识资产
    // （也免得被排除的表名从这一段漏进提案文本）。
    //
    // `reviewed_by` 从翻译表的 `_proposal` 块取（`ontok name suggest --as-names`
    // 写的那一块）。老翻译表没有这块 ⇒ `null`，但**字段必须存在**：
    // 「没人签过」与「没有这一栏」在评审记录里是两件事。
    let names_sha256 = ontok::extract::sample::sha256_hex(names_raw.as_bytes());
    let names_proposal = serde_json::from_str::<serde_json::Value>(&names_raw)
        .ok()
        .and_then(|v| v.get("_proposal").cloned());
    let names_reviewed_by = names_proposal
        .as_ref()
        .and_then(|p| p.get("reviewed_by"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    // 原始**提案文件**（`names.proposal.json`）的引用：`_proposal` 里有就带上。
    // `--names` 吃的是**转换后**的 TableNames，它的 sha 与提案文件的 sha 不同 ——
    // 两个都记，才知道「名字来自哪份提案、经由哪份输入进去」。
    let names_proposal_ref = names_proposal.as_ref().and_then(|p| {
        let file = p.get("file").and_then(|v| v.as_str())?;
        let sha = p.get("sha256").and_then(|v| v.as_str())?;
        Some(serde_json::json!({ "file": file, "sha256": sha }))
    });

    // ── REQ-015：造实现 + 编排，不再直接 RuleBasedAgent::new(…).propose(…) ──
    //
    // 两条路径的差别**只在登记从哪来**，之后完全一样：
    //   带 --agent-registry ⇒ 登记来自库里（fail-closed，查不到就报错）
    //   不带                ⇒ 由 CLI 参数合成，行为与 REQ-013 之前逐字节一致
    // 然后一律：`build_agent`（按登记的 protocol 造实现，找不到就报错，
    // **不回落**规则实现）→ `orchestrate`（分组 drive_group + 阶段 2 核对）。
    //
    // ⚠️ 这里拿到的**只有一个 `Box<dyn ExtractionAgent>`** ——
    // 生产入口不再知道任何具体类型，所以「接第二个 agent」不需要改这个文件。
    let registration = match &registry_agent {
        Some(name) => {
            if declared_agent.is_some() || declared_version.is_some() {
                anyhow::bail!(
                    "--agent-registry 与 --agent/--agent-version 不能同时给：\
                     两套来源并存时「这条本体是谁抽的」就成了猜（规格 §8.4）"
                );
            }
            let url = ontok::config::require_env(ontok::config::DATABASE_URL_ENV)?;
            let pool = ontok::db::pool::connect(&url).await?;
            ontok::db::pg::migrate(&pool).await.context("迁移 agent-registry 建表失败")?;
            let registry = ontok::db::PgAgentRegistry::new(&pool);

            // 取**完整登记**（含 protocol），不是只取身份 ——
            // 「用哪个实现」由登记的 protocol 决定（factory 的分派依据），
            // 只拿到 AgentIdentity 就没法分派，那正是 REQ-013 之前的形状。
            // 查不到就报错，不回落默认值（config.rs / introspect.rs 的同一纪律）。
            let reg = match &registry_version {
                Some(v) => registry
                    .require(name, v)
                    .await
                    .with_context(|| format!("解析 agent 身份失败：{name} v{v}"))?,
                None => registry
                    .require_any(name)
                    .await
                    .with_context(|| format!("解析 agent 身份失败：{name}"))?,
            };

            // §4.1「记录每轮迭代」。重复同一轮幂等（重放）。
            registry
                .record_round(&reg.agent, &reg.version, round, Some(&report.schema))
                .await
                .context("记录抽取轮次失败")?;
            reg
        }
        None => AgentRegistration {
            // ⚠️ REQ-059：这两个字段以前是 `agent.clone()` / `agent_version.clone()`，
            // 也就是**调用方随手填的字符串**（默认 `"mcode-agent"` /
            // `"unknown"`→翻译表版本）。产物里 `extracted_by.agent` 记的是
            // 那个占位，而真正跑的是 `inproc` —— 一份**自证**的假溯源。
            //
            // 现在这里记的是**这一条路径上真正会跑的那个实现**：
            // 协议固定 `"inproc"` 是事实陈述（同下），实现自报
            // [`INPROC_EXECUTOR_ID`] / [`INPROC_EXECUTOR_VERSION`]。
            agent: INPROC_EXECUTOR_ID.to_string(),
            version: INPROC_EXECUTOR_VERSION.to_string(),
            // protocol 固定 "inproc" 是**事实陈述**：这条路径跑的本来就是
            // 进程内规则实现，不是「默认挑一个实现」。
            protocol: "inproc".to_string(),
            endpoint: None,
            capabilities: Vec::new(),
            // 不连库 ⇒ 没有登记时刻。**留空**，不用 `introspected_at`
            // 冒充 —— 那是勘查时刻，不是登记时刻，填上去就是假的溯源。
            registered_at: String::new(),
        },
    };

    let agent = build_agent(&registration, names.clone(), round)?;

    // ── REQ-059 · 抽取归属可信化：三道闸，全在**出产物之前** ──
    //
    // 过去这里只有一道闸，且那一道只是用 eprintln 打一行告警就往下走 ——
    // 「让人看见但不阻止」。§8.4 要防的是版本漂移，而 `extracted_by`
    // 正是版本快照里「这条本体是谁抽的」那一栏，它不可信时整条归因链都不可信。
    // 所以现在不一致一律**拒绝出产物**（`bail!`），三条各管一件不同的事：

    // 闸一：调用方**声明**的身份 vs 造出来的实现**自报**的身份。
    //
    // `--agent` 的语义因此从「贴一个标签」变成「**断言**执行者是谁」。
    // 这是 REQ-059 第一次让这两个值来自**真正独立**的两个来源：
    // 声明来自命令行（人），实际来自 `build_agent` 造出的那个实现（代码）。
    // REQ-059 之前，`identity_cross_check` 拿
    // `AgentIdentity::from_registration(&registration)` 与 `agent.identity()`
    // 比，而 inproc 分支的身份**正是前者的同源拷贝** —— 拿一个值跟自己比，
    // 恒为「一致」，那条告警分支在默认路径上**结构上不可达**。
    require_declared_matches_actual("agent", declared_agent.as_deref(), &agent.identity().agent)?;
    require_declared_matches_actual("agent-version", declared_version.as_deref(), &agent.identity().version)?;

    // 闸二：登记身份 vs 握手身份（`--agent-registry` 路径才可能不一致）。
    //
    // 处置从「打一行 ⚠ 然后照常出产物」改成**拒绝**：不一致就意味着
    // `extraction_rounds` 与 `extracted_by` 会记下两个不同的名字，
    // 而产物里没有任何东西能让人看出这一点。
    if let Some(mismatch) =
        identity_cross_check(&AgentIdentity::from_registration(&registration, round), agent.identity())
    {
        anyhow::bail!(
            "拒绝出产物：抽取身份两处对不上。\n\
             {mismatch}\n"
        );
    }

    // 闸三：抽取执行者名 vs 翻译表来源名**不许读起来像同一个东西**（AC-003）。
    //
    // 基线上的实况正是这样：翻译表登记的产出方叫
    // `"mcode-agent (MiniMax Code)"`，而 `extracted_by.agent` 叫
    // `"mcode-agent"` —— 两件不相干的事，在同一屏输出里长得几乎一样。
    if let Some(ambiguous) = executor_name_collision(&names.agent, &agent.identity().agent) {
        anyhow::bail!("拒绝出产物：抽取归属与翻译表来源混名。\n{ambiguous}\n");
    }

    // ── REQ-019：第 2 轮起从**基版本**载入已有类型 ──
    //
    // REQ-015 时这里从不填 `existing_types`，于是 REQ-014 刚加的 key 双向核对
    // （规格 §7.1 阶段 1 输入之四）在生产上**一次都没跑过**：
    //   ExistingKeyDeclaredAsNew / ExistingDisplayNameChanged —— 没有「已有类型」可比
    //   ForbiddenTerm —— 那要 constraints，不在这里，但同理无据可依
    //
    // 取**该 project 的最新版本**作为基版本：「第 2 轮要接着长」的那一版，
    // 它的 `object_types` 就是阶段 1 要比对的「已知类型」。
    //
    // ⚠️ REQ-019 版本归因修复：这里**曾经**写着「提案落库时挂的基版本正是它
    // （`proposals.base_version_id`）」—— **那句是错的**，害得 `base_version`
    // 一直写死 1：落库挂哪一版由 `ontok propose import` 读产物里的
    // `base_version` 决定，与本函数取哪一版**毫无关系**。两者各说各话时
    // 「阶段 1 拿 v2 验的、apply 的 diff 却挂在 v1 上」，且错得无声。
    // 现在 `base_seq` 一路带进提案（`OrchestrationInput::base_version`），
    // 声明的就是**这一轮实际比对了的那一版**，两边不可能再分叉。
    //
    // ⚠️ 由此产生的一条硬事实：**第 2 轮起必须连库**。缺 `ONTOK_DATABASE_URL`
    // 或 project 不存在 ⇒ 直接报错，绝不退回「空清单假装第 2 轮」——
    // 那正是让三种违规**静默不可达**的老路。
    let (base_seq, existing_types) = load_existing_types(round, &project).await?;
    if round > 1 {
        println!(
            "  已有类型    : {} 个（来自 project {project:?} 的基版本 v{base_seq}）",
            existing_types.len()
        );
    }

    let mut orchestration = OrchestrationInput::new(&names, &project);
    orchestration.attempts = attempts;
    orchestration.existing_types = existing_types;
    // 与上面那个 `base_seq` **同一个值**（不是另解析一次）——
    // 提案里声明的基版本必须就是清单实际从哪一版载入的。
    orchestration.base_version = base_seq;
    // REQ-022：闸门由**装配层**给（CLI 是装配层）；编排层只判「越没越」。
    orchestration.budget = budget_tokens.map(|max_used| UsageBudget { max_used, max_cost: None });
    let ExtractionRun { mut outcome, stage } = orchestrate(&*agent, &report, &orchestration)?;
    // REQ-108：`stats` 先克隆一份 —— 下面要给 `outcome.proposal.items` 贴置信度
    // （可变借用），而统计只读。克隆一个小结构比把 20 处 `s.xxx` 改成
    // 「先取值再打印」安全得多。
    let s = outcome.stats.clone();

    // ── REQ-108 · 置信度：采样 → 评分 → 贴回条目 ────────────────────
    //
    // 顺序不能换：采样只产出聚合量 → 评分是纯函数 → 贴分数**只新增一个键**。
    // 「抽取结果不被置信度改动」这条（AC-005）在这个顺序下是结构性的：
    // 评分函数的输入里根本没有 items 本身，只有坐标与聚合量。
    let keys = KeyCatalog::from_product(
        &serde_json::from_str::<serde_json::Value>(&schema_raw)
            .context("schema JSON 解析失败（键目录）")?,
    );

    // ══════════════════════════════════════════════════════════════
    // ⭐ REQ-120 · FK 边补全（在**评分之前**：注入的边也要被采样与评分）
    // ══════════════════════════════════════════════════════════════
    //
    // 位置是**结构性**的，三条都不能挪：
    // 1. 在 `link_candidates` **之前** —— 否则注入的新边拿不到置信度，
    //    会被当成「没评分的关系」扔进待查清单（`unscored_links`）；
    // 2. 在 `s = outcome.stats.clone()` **之后** —— 下面的统计打印读的是 `s`；
    // 3. 在写任何文件**之前** —— 签字产物有问题时一个字节都不该落盘。
    let src_url_resolved = src_url
        .clone()
        .or_else(|| std::env::var("SRC_URL").ok().filter(|s| !s.trim().is_empty()))
        .or_else(|| std::env::var(SOURCE_DATABASE_URL_ENV).ok().filter(|s| !s.trim().is_empty()));
    // **要连库的是 `--fk-out`**（值域重叠只能从库里读）；`--fk-sign` 是**纯本地**的：
    // 它读一份已经签好的文件、把边注入条目 —— 不需要再连一次源库。
    // 早先版本把两者绑在一起，等于要求「签字时必须能连源库」，那是无谓的耦合。
    if fk_out.is_some() {
        if src_url_resolved.is_none() {
            anyhow::bail!(
                "REQ-120：--fk-out 需要源库连接串（--src 或环境变量 SRC_URL / \
                 {SOURCE_DATABASE_URL_ENV}）—— 值域重叠只能从库里读，不能猜"
            );
        }
        if keys.is_empty() {
            anyhow::bail!(
                "REQ-120：这份 schema.json 没有 primary_key / unique_columns —— \
                 候选表用哪一列验命中是**猜不出来**的（R3）。先重跑 ontok-introspect"
            );
        }
    }
    if fk_out.is_none() && fk_unresolved_out.is_some() {
        anyhow::bail!(
            "REQ-120：--fk-unresolved-out 只在 --fk-out 下有意义 —— \
             未解析清单与提案是**同一次运行**的产物，不许各跑一次"
        );
    }
    if let Some(fk_path) = fk_out.as_deref() {
        let url = src_url_resolved.as_deref().expect("上面已判过");
        let pool = ontok::extract::sample::sample_pool(url)
            .await
            .context("连不上源库（FK 值域重叠采样）")?;
        let source = ontok::extract::fk::FkSource {
            schema_file: schema_path.clone(),
            schema_sha256: ontok::extract::sample::sha256_hex(schema_raw.as_bytes()),
            schema: report.schema.clone(),
            // 只读凭据：与 REQ-003 AC-101 同一份事实（勘查报告里读回的那个值）。
            read_only: report.transaction_read_only.clone(),
            tables: report.tables.len(),
            columns: report.tables.iter().map(|t| t.columns.len()).sum(),
        };
        let fk_report = ontok::extract::fk::verify(
            &pool,
            &report,
            &keys,
            &names,
            source,
            ontok::extract::fk::FK_TIMEOUT,
        )
        .await?;
        pool.close().await;
        if let Some(parent) = std::path::Path::new(fk_path).parent() {
            std::fs::create_dir_all(parent).ok();
        }
        std::fs::write(
            fk_path,
            ontok::extract::fk::render_report(&fk_report)?,
        )
        .with_context(|| format!("写不出 {fk_path}（FK 提案）"))?;
        // 未解析清单默认与提案同目录：**和提案同源**（同一次运行、同一份数据）。
        let unresolved_path = fk_unresolved_out.clone().unwrap_or_else(|| {
            let p = std::path::Path::new(fk_path);
            p.with_file_name("fk-unresolved.json").to_string_lossy().to_string()
        });
        if let Some(parent) = std::path::Path::new(&unresolved_path).parent() {
            std::fs::create_dir_all(parent).ok();
        }
        std::fs::write(&unresolved_path, ontok::extract::fk::render_unresolved(&fk_report)?)
            .with_context(|| format!("写不出 {unresolved_path}（未解析清单）"))?;

        let st = &fk_report.stats;
        println!();
        println!("REQ-120 FK 边补全（值域重叠 · {}）", ontok::extract::fk::FK_METHOD);
        println!(
            "  待判列      : {} 个 *_id（候选 {} 个 · 实测 {} 个）",
            st.columns_considered, st.candidates_generated, st.candidates_tested
        );
        println!(
            "  补出关系    : {} 条（strong {} · medium {} · inherit {}，**未签，不进快照**）",
            st.proposals, st.proposals_strong, st.proposals_medium, st.proposals_inherited
        );
        if st.proposals_inherited > 0 || st.inherit_conflicts > 0 {
            println!(
                "  继承（E-2） : {} 条（同名同义列，confidence 0.60–0.75，必须人签）· \
                 目标不唯一被拒 {} 列",
                st.proposals_inherited, st.inherit_conflicts
            );
        }
        println!(
            "  值域否决    : {} 个候选（命中率 < 0.5，一条关系都不产出）",
            st.rejected
        );
        println!("  未解析      : {} 列（逐条见 {unresolved_path}）", st.unresolved);
        for (code, n) in fk_report.unresolved_counts() {
            println!("    · {code:<20} {n}");
        }
        if !st.close() {
            anyhow::bail!(
                "FK 统计不闭合：{} 列 ≠ 提案 {} + 未解析 {}",
                st.columns_considered,
                st.proposals,
                st.unresolved
            );
        }
    }
    if let Some(sign_path) = fk_sign.as_deref() {
        let raw = std::fs::read_to_string(sign_path)
            .with_context(|| format!("读不出签字产物 {sign_path}"))?;
        let signed = ontok::extract::fk::load_signed(&raw)?;
        let known: std::collections::BTreeSet<String> =
            report.tables.iter().map(|t| t.name.clone()).collect();
        let st = ontok::extract::fk::inject_signed(&mut outcome.proposal.items, &signed, &known)?;
        println!();
        println!("REQ-120 签字产物（{sign_path}）");
        println!(
            "  注入关系    : {} 条新增 + {} 条既有边补验证标记（未签 {} 条已忽略）",
            st.added, st.marked_existing, st.skipped_unsigned
        );
    }
    // R4 / REQ-119 N-1：**单列唯一列上的 N:1 改成 1:1**。
    // 不带开关：这不是可选项，是「`unique_columns` 明明有约束却只标一半」的修正。
    let promoted = ontok::extract::fk::promote_unique_cardinality(&mut outcome.proposal.items, &keys);
    if !promoted.is_empty() {
        println!();
        println!("REQ-120 基数修正（R4：单列唯一列 ⇒ 1:1）");
        for s in &promoted {
            println!("  · {s}");
        }
    }
    // AC-007：名字推断的边一律盖 `join_verified: false`，签过字的边保留 true。
    let stamped = ontok::extract::fk::stamp_naming_only(&mut outcome.proposal.items);
    if stamped > 0 || fk_sign.is_some() {
        println!("  连接验证标记: naming-only {stamped} 条（value-overlap 由 --fk-sign 给）");
    }

    let candidates = ontok::extract::propose::link_candidates(&outcome.proposal.items, &keys);
    let mut evidence: std::collections::HashMap<
        usize,
        ontok::extract::sample::SampleEvidence,
    > = std::collections::HashMap::new();

    // R1 ④：`--no-sample` 关的是**置信度采样**（REQ-108 的那一段）。
    // ⚠️ 它**不**关 REQ-120 的 FK 采样 —— 值域重叠不是「打分素材」，
    // 是产生关系的**判决依据**；关掉它还能补出关系，等于退回「只看名字」。
    let sample_url = if no_sample { None } else { src_url_resolved };
    let mut sampled_relations = 0usize;
    if let Some(url) = sample_url {
        if keys.is_empty() {
            eprintln!(
                "⚠ 这份 schema.json 没有 primary_key / unique_columns（REQ-108 之前的产品）——\
                 采样无法确定「目标表用哪一列验命中」，本轮**不采样**。\
                 先重跑 ontok-introspect 生成新产物。"
            );
        } else {
            let pool = ontok::extract::sample::sample_pool(&url)
                .await
                .context("连不上源库（只读采样）")?;
            for c in &candidates {
                match c.sample_spec() {
                    Some(spec) => {
                        let e = ontok::extract::sample::sample_one(
                            &pool,
                            &spec,
                            ontok::extract::sample::SAMPLE_TIMEOUT,
                        )
                        .await;
                        if e.sampled > 0 {
                            sampled_relations += 1;
                        }
                        evidence.insert(c.item_index, e);
                    }
                    // 目标表没有单列键（复合主键 / 无主键）⇒ 不能采样，但**要留痕**
                    None => {
                        evidence.insert(
                            c.item_index,
                            ontok::extract::sample::SampleEvidence::none("no_key_column"),
                        );
                    }
                }
            }
            pool.close().await;
        }
    } else {
        eprintln!(
            "⚠ 未采样（{}）：本轮置信度只由结构信号算，全部标 data_evidence=none。\
             要给数据证据请传 --src <url> 或环境变量 SRC_URL / {SOURCE_DATABASE_URL_ENV}。",
            if no_sample { "给了 --no-sample" } else { "没有源库连接串" }
        );
    }
    // ── REQ-109 · AI 语义判断（可选、带缓存、带预算）───────────────────
    //
    // 位置是**结构性**的：采样已经跑完（评分需要数据证据），评分还没发生
    // （语义分要作为评分卡的输入）。三条都在同一个循环里，顺序不能换。
    //
    // 缓存流程（`db::ai_cache` 模块头有完整论证）：
    //   预加载（async）→ 内存编排（sync，命中即返回、绝不调用）→ 一次性回写（async）
    let mut ai_judged: Option<
        std::collections::HashMap<usize, ontok::extract::ai_semantic::Judgement>,
    > = None;
    if let Some(settings) = &llm_settings {
        use ontok::extract::ai_semantic as ai;

        let prompts = ai::Prompts::standard();
        let budget = ai_budget.expect("--llm 已在入口校验过预算");
        let mut reqs = ontok::extract::propose::ai_requests(&candidates, &report, llm_only.as_deref());
        let skipped = candidates.len().saturating_sub(reqs.len());
        // 分阶段上线的「只跑前 N 条」：**只截请求**，不改任何判据。
        // 没送的关系语义缺失（不是 0 分），与「AI 判失败」同义。
        if let Some(n) = llm_limit {
            reqs.truncate(n);
        }

        let db_url = ontok::config::require_env(ontok::config::DATABASE_URL_ENV)?;
        let pool = ontok::db::pool::connect(&db_url)
            .await
            .context("连不上产品库（ai_cache 所在库）")?;
        ontok::db::pg::migrate(&pool)
            .await
            .context("迁移失败（建 ai_cache 表）")?;
        let store = ontok::db::PgAiCache::new(pool.clone());
        let fps: Vec<String> = reqs
            .iter()
            .map(|r| {
                r.fingerprint(
                    &prompts.sha256,
                    &settings.config.model,
                    ai::AGENT_VERSION,
                )
            })
            .collect();
        let mut cache = store.preload(&fps).await.context("读 ai_cache 失败")?;
        let mut transport = settings.transport()?;
        let (judged, stats) = ai::judge_all(
            transport.as_mut(),
            &mut cache,
            &reqs,
            &settings.config.model,
            prompts.clone(),
            budget,
            refresh,
            settings.config.max_tokens,
        );
        // ★ 先回写、再判预算：**已经花掉的钱对应的判断必须落盘**。
        // 反过来的话，一次越预算的终止会让这一轮的所有调用白花（下次重跑再付一次）。
        let written = store.persist(&cache).await.context("写 ai_cache 失败")?;
        pool.close().await;

        let judge_ok = stats.judged;
        println!();
        println!("AI 语义（{}）", ontok::domain::confidence::METHOD_V2);
        println!(
            "  模型        : {}（prompt sha256 {}…，agent {}）",
            settings.config.model,
            &prompts.sha256[..12.min(prompts.sha256.len())],
            ai::AGENT_VERSION
        );
        let filtered_note = {
            let mut parts: Vec<String> = Vec::new();
            if let Some(f) = llm_only.as_deref() {
                parts.push(format!("--llm-only {f}"));
            }
            if let Some(n) = llm_limit {
                parts.push(format!("--llm-limit {n}"));
            }
            if parts.is_empty() {
                "无目标键列".to_string()
            } else {
                format!("无目标键列 / {}", parts.join(" / "))
            }
        };
        println!(
            "  请求        : {} 条（未送 AI {} 条：{}）",
            reqs.len(),
            skipped,
            filtered_note
        );
        println!(
            "  判定        : {} 条（缓存命中 {} · 实际调用 {}{}）",
            judge_ok,
            stats.cache_hits,
            stats.calls,
            if stats.rejected_cache_hits > 0 {
                format!(" · 其中作废记录回放 {} 条", stats.rejected_cache_hits)
            } else {
                String::new()
            }
        );
        println!(
            "  token       : 用 {}{}",
            stats.tokens_spent,
            match stats.cache_hit_rate() {
                Some(r) => format!(
                    " / 上限 {budget} · 缓存命中率 {:.0}% · 新写入缓存 {written} 条",
                    r * 100.0
                ),
                None => format!(" / 上限 {budget} · 缓存命中率 —（没有判定）· 新写入缓存 {written} 条"),
            }
        );
        if stats.rejected() > 0 || stats.failed_calls > 0 {
            println!(
                "  ⚠ 作废      : {} 条（解析 {} · choice {} · fit {} · rationale {} · cites {}）· 调用失败 {}",
                stats.rejected(),
                stats.rejected_parse,
                stats.rejected_choice,
                stats.rejected_fit,
                stats.rejected_rationale,
                stats.rejected_cites,
                stats.failed_calls
            );
            for s in stats.rejection_samples.iter().take(3) {
                println!("                {s}");
            }
        }
        if refresh {
            println!(
                "  ⚠ 强制刷新  : {} 条（--refresh：忽略已有缓存并覆盖）",
                stats.refresh_ignored
            );
        }

        // ── 预算闸：越了 ⇒ **不出产物**（AC-006）──
        //
        // 为什么不写一份「importable: false」的提案就退出：主提案的契约里
        // 没有 importable 这一栏（只有待查/拒绝清单有），写一份**看起来正常、
        // 其实语义判断只做了一半**的提案，正是 R4 要防的「静默降级」。
        if stats.over_budget() {
            // 「停在第几条」两种说法都要有，且都必须是真的：
            // · 还有关系没判 ⇒ 就停在那个位置；
            // · 全判完了但用量越顶 ⇒ 停在「最后一条之后」，不许说成停在第 1 条。
            let stop_where = match stats.budget_stopped {
                Some(n) => format!("停在第 {} 条（共 {} 条关系）", n, reqs.len()),
                None => format!(
                    "停在第 {} 条之后（{} 条已全部判完，用量越过了上限）",
                    reqs.len(),
                    reqs.len()
                ),
            };
            println!(
                "  ✗ 预算超限  : 已用 {} / 上限 {}，{}",
                stats.tokens_spent, budget, stop_where
            );
            eprintln!(
                "✗ AI 预算超限：已用 {} token / 上限 {} token，{}。",
                stats.tokens_spent, budget, stop_where
            );
            anyhow::bail!(
                "AI 预算超限 —— 本轮**不产出任何提案文件**（AC-006）。\n\
                 \n\
                 已用 / 上限 / 停在第几条：{} / {} / {}。\n\
                 \n\
                 为什么不在超预算时照出产物：主提案没有 importable 栏，\
                 一份「语义只判了一半」的提案看起来与完整的一模一样 ——\
                 那正是 R4「不许静默降级」要防的东西。\n\
                 已经花掉的那些判断**已经写进 ai_cache**，放宽预算重跑会直接命中，不重复计费。",
                stats.tokens_spent,
                budget,
                stop_where
            );
        }

        ai_judged = Some(judged);
    }

    let scored = ontok::extract::propose::score_links_with_ai(
        &outcome.proposal.items,
        &report,
        &keys,
        &evidence,
        ai_judged.as_ref(),
    );
    ontok::extract::propose::attach_confidence(&mut outcome.proposal.items, &scored);
    let scored_by_band = ontok::domain::confidence::BandCounts::of(scored.iter().map(|(_, c)| &c.band));

    // 本轮实际用的评分方法：AI 关 = v1（与 REQ-108 逐字节相同），开 = v2。
    let score_method = if llm_settings.is_some() {
        ontok::domain::confidence::METHOD_V2
    } else {
        ontok::domain::confidence::METHOD
    };

    println!();
    println!("置信度（{score_method}）");
    println!(
        "  关系评分    : {} 条（其中 {} 条真的读到了样本）",
        scored.len(),
        sampled_relations
    );
    println!(
        "  分档        : 高 {} · 中 {} · 低 {} · 拒绝 {}",
        scored_by_band.high, scored_by_band.middle, scored_by_band.low, scored_by_band.rejected
    );
    let no_evidence = scored
        .iter()
        .filter(|(_, c)| c.data_evidence == ontok::domain::confidence::DataEvidence::None)
        .count();
    if no_evidence > 0 {
        println!("  ⚠ 无数据证据: {no_evidence} 条（data_evidence=none，不会进主提案）");
    }

    println!();
    println!("抽取完成");
    println!("  源库        : {}（勘查于 {}，只读={}）", report.schema, report.introspected_at, report.transaction_read_only);
    // ⚠️ REQ-059：这一行与下面「抽取执行者」那行是**两件不同的东西**，
    // 措辞必须让读的人一眼分得开 —— 翻译表是**知识资产**（谁产的翻译），
    // 抽取执行者是**这份提案的 items 由谁抽**。基线上这两行读起来像同一件事。
    println!("  翻译表来源   : agent={} v{}（知识资产：**谁产的这份翻译表**，不是抽取执行者）", names.agent, names.version);
    println!("  抽取执行者   : agent={} v{}（协议 {}）—— **这一行才是 extracted_by 记的**", agent.identity().agent, agent.identity().version, registration.protocol);
    println!("  翻译覆盖    : {} / {} 表", names.coverage(&report.tables.iter().map(|t| t.name.clone()).collect::<Vec<_>>()).0, report.tables.len());
    // ── REQ-113 · 排除表：**三计数必须同屏可见**（AC-002）──────
    //
    // 这一段与 stats.json 里的三个数同源（`tables_seen` / `excluded` / `extracted`），
    // 但 `extracted` 的**实测值**要等分档路由之后才定，所以这里先报
    // 「勘查 / 排除 / 作用域」三行，闭合断言在写盘前做（见下面的 `stamp_counts`）。
    if let Some(ex) = &exclusions {
        println!(
            "  排除        : {} 张（{} v{} · sha256 {}… · 决策人 {}）",
            exclusion_applied.len(),
            ex.file(),
            ex.version(),
            &ex.sha256()[..12.min(ex.sha256().len())],
            ex.declared_by()
        );
        println!(
            "  表          : 勘查 {tables_seen} → 排除 {} → 抽取作用域 {}（陈旧 {}）",
            exclusion_applied.len(),
            report.tables.len(),
            exclusion_stale.len()
        );
    } else {
        // 没给 --exclude：**明说**这一轮没有排除，而不是让人以为「排了 0 张」。
        // 这两件事在产物里也必须分得开（没有 `inputs` 块 vs `inputs.exclusions` 存在）。
        println!("  排除        : 未启用（没给 --exclude：本轮把 {tables_seen} 张表全当业务表）");
    }
    let n_ent = outcome.proposal.items.iter().filter(|i| i["kind"] == "add_entity_type").count();
    let n_link = outcome.proposal.items.iter().filter(|i| i["kind"] == "add_link_type").count();
    // 与上面「已有类型 …基版本 v{base_seq}」是**同一个值**。
    // 原来是写死的 `v1` 字面量，于是第 2 轮同屏打印出
    // 「基版本 v2」（载入清单那行）与「基版本 v1」（本行）两个互相打架的数字。
    println!("  提案项目    : {project}（基版本 v{base_seq}）");
    println!("  items       : {} 条（实体 {n_ent} · 关系 {n_link}）", outcome.proposal.items.len());
    println!("  实体        : {n_ent} 个（中文名 {} 个）", s.entities_with_cn_name);
    println!("  属性        : {} 个（来自列注释 {} · 走词表兜底 {}）",
        s.total_properties, s.props_via_comment, s.props_via_wordlist);
    // REQ-005：低质量注释被主动降级 —— 必须报出来，不许静默（fail-open 比 fail-closed 危险）
    if s.props_comment_rejected > 0 {
        println!("  ⚠ 低质量注释: {} 条（无汉字且非枚举值域，已回退词表/形态兜底）", s.props_comment_rejected);
    }
    println!("  关系        : {} 条（悬空 {}）", s.links_inferred, s.links_dangling);
    if !s.tables_without_cn_name.is_empty() {
        println!("  ⚠ 无中文名（会被客户视图过滤）: {} 张 — {}", s.tables_without_cn_name.len(), s.tables_without_cn_name.join(", "));
    }

    // ── 阶段 1/2 的执行结果（REQ-015）：分组数、失败组、映射违规 ──
    //
    // 设计点⑤「不静默跳过」：失败组与阶段 2 的违规项**必须显式报出**。
    // 它们**不进** proposal.json / stats.json —— 那是 REQ-002 的既有契约，
    // 加字段就破坏下游解析；`failed_groups` 已经进了提案元数据，
    // 违规项按既定语义「只报错不丢弃」，在这里报给人看。
    //
    // ⚠️ REQ-019 措辞修正：这一行**不能**读成「阶段 2 核过了、没问题」。
    // 规则实现下 `ColumnRef` 与提案 `items` 取自**同一份 schema**，
    // 是拿结果核对结果 —— 违规数**结构上不可能非 0**，所以那个 0 不携带
    // 任何「证据核对通过」的信息。措辞必须让人一眼看出这是
    // 「规则实现跑通」，不是「核对通过」。
    println!(
        "  阶段1/2     : {} 组（规则实现跑通 {} · 失败 {}）· 候选类型 {} · 候选属性 {}",
        stage.groups,
        stage.groups_ok,
        stage.failed_groups.len(),
        stage.candidate_types,
        stage.candidate_properties
    );
    if stage.violations.is_empty() {
        println!("  ⚠ 阶段 2 违规 0 条 —— 规则实现下这是**自证**（声明与 items 同源），只说明「跑通了」，不说明「核对通过」");
    } else {
        println!("  ⚠ 阶段 2 违规 {} 条 —— 规则实现下不该发生，请查是不是 agent 不是规则实现", stage.violations.len());
    }
    // ── REQ-022：阶段 3+4 走的是哪条组装路径（不许静默）────────────
    println!(
        "  阶段3/4     : {}",
        match stage.assembly {
            Assembly::RulePipeline => "规则路径（items = 本地规则管线的产物）",
            Assembly::RebuiltFromCandidates => "重建路径（items = 由 agent 候选组装，规格 §7.1 阶段 4）",
        }
    );
    if stage.candidate_links > 0 {
        println!("  候选关系    : {} 条（阶段 3 的跨组合成输入）", stage.candidate_links);
    }

    // ── REQ-022：成本闸门 —— 三栏分清「跑完 / 没跑 / 为什么停」────────
    if stage.budget.configured() {
        match &stage.budget.limit_cost {
            Some(c) => println!(
                "  成本闸      : 用量上限 {} · 费用上限 {:.6} {}",
                stage.budget.limit_used.unwrap_or_default(),
                c.amount,
                c.currency
            ),
            None => println!(
                "  成本闸      : 用量上限 {}（本轮已用 {}）",
                stage.budget.limit_used.unwrap_or_default(),
                stage.budget.used
            ),
        }
    } else {
        println!("  成本闸      : 未设（本实现零成本时合理；声明了会上报用量的 agent 会被入口拒绝）");
    }
    if stage.budget.stopped {
        println!(
            "  ⚠ 预算超限  : {} —— **agent 阶段**已跑完 {}/{} 组，未跑 {} 组",
            stage.budget.reason.as_deref().unwrap_or("(原因未知)"),
            stage.groups_ok + stage.failed_groups.len(),
            stage.groups,
            stage.budget.skipped_groups.len(),
        );
        // **关键**：没跑的组记 skipped_groups，**不进 failed_groups**。
        // 「钱用完了」与「这张表抽不出来」是两回事，混在一起会让下游
        // 把一次花钱决策读成一次抽取质量事故。
        println!("               未跑（**不是抽取失败**）: {}", stage.budget.skipped_groups.join(", "));
        // ⚠ REQ-022 复审 F1：「提案是部分的」这句话**只在重建路径上为真**。
        //
        // 两条组装路径对「预算截断了什么」的含义完全不同（`orchestrate` 末尾
        // 的分支）：重建路径的 items **只由跑完那些组的候选**组装 ⇒ 确实残缺；
        // 规则路径的 items 来自 `extract_with_context(report, …)`，而 `report`
        // 是**全量**勘查结果、不受预算影响 ⇒ items 覆盖全部表，**并不残缺**。
        //
        // 原来这里无条件打「本轮提案是部分的」+ 下面无条件说「不要当完整提案
        // import」，于是规则路径下一份**完整可用**的提案被贴上残缺标签、
        // 并被建议「放宽预算重抽」—— 与成本闸门的目的正好相反。
        // 措辞必须按**实际语义**分叉，否则报告本身就在说假话。
        match stage.assembly {
            Assembly::RebuiltFromCandidates => println!(
                "               本轮提案是**部分**的：items 只由上面跑完那些组的候选重建而成，未跑的 {} 组**没有**进候选",
                stage.budget.skipped_groups.len()
            ),
            Assembly::RulePipeline => println!(
                "               本轮提案**不是部分**的：items 来自**全量报告**的本地规则管线（覆盖 {}/{} 张表），\n\
                 \x20             越闸只截断了**阶段 1 的 agent 抽取**（那 {} 组没被 agent 抽过），不影响 items 的覆盖面。\n\
                 \x20             判据是**这份提案能不能用**，不是「要不要放宽预算重抽」—— 规则路径下放宽预算换不来更多 items。",
                s.tables_seen,
                report.tables.len(),
                stage.budget.skipped_groups.len()
            ),
        }
    }
    for f in &stage.failed_groups {
        // 失败的组会让 `extracted_by.failed_groups` 非空 ⇒ 提案元数据里看得见，
        // 且**不参与收敛判定**（否则「什么都没抽」会被误判成「已收敛」）。
        eprintln!("  ✗ 抽取失败: {}（表 {} · 重试 {} 次）—— {}", f.group_id, f.table, f.attempts, f.reason);
    }
    for v in &stage.violations {
        // 阶段 2 的代码侧核对（规格架构红线：证据核对不可外包）。
        // **只报错不丢弃**：删不删是人的决定，不是代码替他做的。
        eprintln!("  ⚠ 映射声明核不过去: [{} / 表 {}] {:?}", v.group_id, v.table, v.violation);
    }

    for p in [&out, &stats_out] {
        if let Some(parent) = std::path::Path::new(p).parent() {
            std::fs::create_dir_all(parent).ok();
        }
    }

    // ── REQ-108 · AC-009 分档路由 ─────────────────────────────────
    //
    // 三份 items **互不相交**：高+中进提案 · 低进待查 · 拒绝不提案。
    // 「不提案」档必须**在 proposal.json 里一条都找不到**（AC-009 的原话），
    // 所以这里不是「标记一下」，而是真的把它们从 items 里移出去。
    let mut routing = ontok::extract::propose::route_items(outcome.proposal.items, &scored);
    // ⭐ REQ-120 rev.2 · **人签过的边不参与按置信度丢弃**（E-2 的必要条件）：
    // `api_call_logs` 那种 0 行表永远拿不到实测证据 ⇒ 打分必然低 ⇒ 会被静默
    // 丢进待查清单。签字是人做的判断，不该被分诊器推翻。
    let rescued = ontok::extract::fk::retain_signed(&mut routing);
    if rescued > 0 {
        println!();
        println!("REQ-120 保留已签字的边: {rescued} 条（分档器本会把它们丢进待查/拒绝）");
    }
    outcome.proposal.items = routing.kept;
    let base_version = outcome.proposal.base_version;
    let pending_path = pending_out.clone().unwrap_or_else(|| {
        let p = std::path::Path::new(&out);
        p.with_file_name("proposal-pending.json").to_string_lossy().to_string()
    });
    let rejected_path = rejected_out.clone().unwrap_or_else(|| {
        let p = std::path::Path::new(&out);
        p.with_file_name("proposal-rejected.json").to_string_lossy().to_string()
    });

    // 待查 / 拒绝清单都**不是主提案**：它们没有 `extracted_by`。
    //
    // ★ AC-015：`importable` 必须与「这份文件到底能不能导」**同真同假** ——
    // `propose import` 现在把它当硬闸（不是约定）。
    //
    // | 文件 | 能不能导 | 为什么 |
    // | --- | --- | --- |
    // | `proposal-rejected.json` | **不能**（`false`） | 拒绝档（<0.30）连提案都不是；一次 `--accept-all` 就进本体 |
    // | `proposal-pending.json` | **能**（`true`） | 待查清单就是要进审核界面的「证据不足」折叠区（设计概要 §五），只是不进主提案 |
    //
    // 之前两栏都写 `false`：那既让「带 false 的文件导得进去」（声明不生效），
    // 又让待查清单**永远导不进来**（低档因此无法被人工审核）—— 两件事都是 bug。
    let side_file =
        |items: &[serde_json::Value], importable: bool, counts_note: &str| -> serde_json::Value {
            serde_json::json!({
                "project": project,
                "base_version": base_version,
                "round": round,
                "importable": importable,
                "method": score_method,
                "note": counts_note,
                "items": items,
            })
        };
    let pending_doc = side_file(
        &routing.pending,
        true,
        "待查清单（0.30–0.60）：不进主提案；证据不足，需人工判断（可导入审核，但不自动进提案）",
    );
    let rejected_doc = side_file(
        &routing.rejected,
        false,
        "被拒绝（<0.30）：不提案；每条都带 veto 理由。importable: false ⇒ propose import 会拒绝导入",
    );
    for p in [&pending_path, &rejected_path] {
        if let Some(parent) = std::path::Path::new(p).parent() {
            std::fs::create_dir_all(parent).ok();
        }
    }
    std::fs::write(&pending_path, serde_json::to_string_pretty(&pending_doc)?)
        .with_context(|| format!("写不出 {pending_path}"))?;
    std::fs::write(&rejected_path, serde_json::to_string_pretty(&rejected_doc)?)
        .with_context(|| format!("写不出 {rejected_path}"))?;

    println!();
    println!("分档路由（AC-009）");
    // 主提案里的**实测**关系条数（= 真正会进快照的那一批）。
    // ⚠️ 不能只报 `counts.high + counts.middle`：`--fk-sign` 之后，人签过字的边
    // 会从待查/拒绝档被捞回来（`retain_signed`），它们的 band 仍然是 low/rejected。
    // 只报分档数会打出「主提案 0 条关系」而实际有 74 条 —— 那是**误导**。
    let kept_links = outcome
        .proposal
        .items
        .iter()
        .filter(|i| i["kind"] == "add_link_type")
        .count();
    println!(
        "  主提案      : {} 条关系（高 {} · 中 {} · 已签字保留 {}）+ 非关系条目",
        kept_links, routing.counts.high, routing.counts.middle, rescued
    );
    println!("  待查清单    : {} 条（低档，不进主提案）→ {pending_path}", routing.counts.low);
    println!("  不提案      : {} 条（拒绝档，proposal.json 里一条都没有）→ {rejected_path}", routing.counts.rejected);
    if routing.unscored_links > 0 {
        println!(
            "  ⚠ 未评分关系: {} 条（条目缺列名/两端）—— 放进待查清单，不冒充「高」",
            routing.unscored_links
        );
    }
    debug_assert_eq!(
        routing.counts.total() + routing.unscored_links,
        scored.len(),
        "四档条数 + 未评分 == 评分过的关系数（口径必须对得上）"
    );

    // ── REQ-113 · 三计数闭合（AC-002 / R5）：**写盘之前**的最后一道闸 ──────
    //
    // `tables_extracted` 取**实测**：最终提案（分档路由之后）里 `add_entity_type`
    // 的条数。它与 `tables_seen - tables_excluded` 是两个**独立**来源的数 ——
    // 反推出来的差永远等于差，那样的「断言」不携带任何信息。
    //
    // 路由从不搬动实体条目（`route_items` 只对 `add_link_type` 分档），
    // 所以这个实测值就是「真的建成本体的表数」。
    let extracted = outcome
        .proposal
        .items
        .iter()
        .filter(|i| i["kind"] == "add_entity_type")
        .count();
    // 「作用域里有、却没产出实体」的表名 —— 只用于闭合失败时的**报错正文**。
    // BA 裁定：失败要能说清是哪几张表没了，只报数字不等等于让人去猜。
    let produced: std::collections::HashSet<&str> = outcome
        .proposal
        .items
        .iter()
        .filter(|i| i["kind"] == "add_entity_type")
        .filter_map(|i| i["payload"]["key"].as_str())
        .collect();
    let missing: Vec<String> = report
        .tables
        .iter()
        .filter(|t| !produced.contains(t.name.as_str()))
        .map(|t| t.name.clone())
        .collect();
    outcome.stats.stamp_counts(
        tables_seen,
        &exclusion_applied,
        &exclusion_stale,
        &exclusion_flagged,
        extracted,
        &missing,
    )?;

    // ── REQ-118 · A-1（P4）· 溯源：**输入侧两份文件都要落进产物** ────────
    //
    // `inputs.names` 恒存在（`--names` 必然给过，默认路径也算）：
    // `{file, sha256, agent, version, reviewed_by}` —— 与 `inputs.exclusions` 同构。
    // `reviewed_by = null` 是**允许且必须存在**的：提案还没签，但将来签了要有地方记。
    //
    // ⚠️ 这改变了 REQ-112 时期「不给 --exclude ⇒ 提案里连 inputs 都不出现」的口径：
    // 那是当时还没有名字溯源。现在**每一次**抽取都基于某份翻译表，
    // 不记它等于「抽出来的名字无据可查」—— 正是 REQ-118 要修的那一格。
    {
        let mut inputs = serde_json::Map::new();
        let mut names_rec = serde_json::Map::new();
        names_rec.insert("file".into(), serde_json::Value::from(names_path.clone()));
        names_rec.insert("sha256".into(), serde_json::Value::from(names_sha256.clone()));
        names_rec.insert("agent".into(), serde_json::Value::from(names.agent.clone()));
        names_rec.insert(
            "version".into(),
            serde_json::Value::from(names.version.clone()),
        );
        names_rec.insert(
            "reviewed_by".into(),
            match &names_reviewed_by {
                Some(s) => serde_json::Value::from(s.clone()),
                None => serde_json::Value::Null,
            },
        );
        if let Some(pref) = &names_proposal_ref {
            names_rec.insert("proposal".into(), pref.clone());
        }
        inputs.insert("names".into(), serde_json::Value::Object(names_rec));
        // ── REQ-113 · 排除审计段（沿用既有形状，**一个文件 = 一条完整审计链**）──
        //
        // `inputs.exclusions` 是完整审计段：
        // `{file, version, sha256, declared_by, reviewed_by, rule, counts, excluded[], flagged[], stale[]}`。
        //
        // 为什么把逐条明细也放进来（而不是侧车文件）：
        // 用户要搬的**只有一个** proposal.json；侧车会让「丢一个文件就导不进」，
        // 产品还多一个产出文件。AC-001 的判据随之精确化：
        // `items` 段里 grep 被排除表名 = 0，且整份文件里这些表名**只允许**出现在这一段内。
        // 这一段是**元信息**（谁判的、依据什么、排了谁），不是实体。
        if let Some(ex) = &exclusions {
            let counts = ontok::extract::exclusions::Counts::of(&outcome.stats);
            let rec = ex.record(
                ontok::extract::exclusions::Displayed {
                    applied: &exclusion_applied,
                    stale: &exclusion_stale,
                    flagged: &exclusion_flagged,
                },
                counts,
            );
            inputs.insert(
                "exclusions".into(),
                serde_json::to_value(rec).unwrap_or(serde_json::Value::Null),
            );
        }
        outcome.proposal.inputs = Some(serde_json::Value::Object(inputs));
    }

    std::fs::write(&out, serde_json::to_string_pretty(&outcome.proposal)?)
        .with_context(|| format!("写不出 {out}"))?;
    std::fs::write(&stats_out, serde_json::to_string_pretty(&outcome.stats)?)?;
    println!();
    println!("排除台账（AC-002 / AC-003）");
    println!(
        "  三计数      : 勘查 {} − 排除 {} = 抽取 {}（闭合 = {}）",
        outcome.stats.tables_seen,
        outcome.stats.tables_excluded,
        outcome.stats.tables_extracted,
        outcome.stats.counts_close()
    );
    if outcome.stats.excluded_tables.is_empty() {
        println!("  逐条        : 无（本轮没有排除）");
    } else {
        for e in &outcome.stats.excluded_tables {
            println!(
                "  · {:<40} {:<22} {}（{} · 置信度 {}）",
                e.table,
                e.category.as_str(),
                e.reason,
                e.decided_by,
                match e.confidence {
                    Some(c) => format!("{c:.2}"),
                    None => "—".to_string(),
                }
            );
        }
    }
    // 「保留但存疑」**单独一段**（⛔ 不与上面的排除合并）：
    // 上面是「已经不建实体」的决定，这里是「照常建、但请人看一眼」。
    if !outcome.stats.flagged_tables.is_empty() {
        println!(
            "  待裁决      : {} 张（**没有被排除**，已照常进本体；只是有疑虑，见审核界面）",
            outcome.stats.flagged_tables.len()
        );
        for f in &outcome.stats.flagged_tables {
            println!("  ? {:<40} {}", f.table, f.why);
        }
    }
    if !outcome.stats.stale_exclusions.is_empty() {
        eprintln!(
            "  ⚠ 陈旧排除 {} 条已记进 stats.stale_exclusions：{}",
            outcome.stats.stale_exclusions.len(),
            outcome.stats.stale_exclusions.join("、")
        );
    }

    // 成本超限 ⇒ **非零退出**（产物已写出，方便核对「钱花到哪了」）。
    // 为什么不是「写完就当没事」：超预算是**需要人知道**的事实，
    // 脚本里 `ontok-propose && ontok propose import` 因此会停在这里。
    //
    // ⚠ 退出码**两条路径都非零**，但**理由不同**（REQ-022 复审 F1）——
    // 「非零」在这里表示「你设的预算被越过了、这一轮按预算提前收了尾」，
    // **不**表示「产物残缺」。产物残不残缺由组装路径决定，两者在下面分开说：
    if stage.budget.stopped {
        match stage.assembly {
            // 重建路径：items 真的只含跑完那些组 ⇒ 这份提案**确实不能当完整的用**。
            Assembly::RebuiltFromCandidates => anyhow::bail!(
                "预算超限，已在第 {}/{} 组停：{}。未跑的 {} 组**没有**进 failed_groups（那不是抽取失败）。\n\
                 产物已写出（{} 与 {}）供核对，但**不要**当完整提案 import —— items 只由跑完那些组的候选重建。\n\
                 先决定：放宽预算 / 分轮抽 / 就用已跑完的这部分。",
                stage.groups_ok + stage.failed_groups.len(),
                stage.groups,
                stage.budget.reason.as_deref().unwrap_or("(原因未知)"),
                stage.budget.skipped_groups.len(),
                out,
                stats_out,
            ),
            // 规则路径：items 来自全量报告的规则管线，**覆盖全部 {}/{} 张表** ⇒
            // 提案**完整可用**。非零退出只表示「预算被越过了」。
            // 原话「不要当完整提案 import / 放宽预算」在这里是**错的**：
            // 放宽预算换不来更多 items（items 压根不看预算），只会多花钱多花时间。
            Assembly::RulePipeline => anyhow::bail!(
                "预算超限，已在第 {}/{} 组停：{}。未跑的 {} 组**没有**进 failed_groups（那不是抽取失败）。\n\
                 产物已写出（{} 与 {}）。\n\
                 ⚠ 这份提案的 items **是完整的**：规则路径跑的是全量报告，覆盖 {}/{} 张表。\n\
                 越闸只截断了**阶段 1 的 agent 抽取**（那 {} 组没被 agent 抽过）—— 那些组在规则路径下\n\
                 仍由本地规则覆盖，所以**放宽预算换不来更多 items**。\n\
                 非零退出在这里只表示「你设的预算被越过了」：**要不要 import 由你判断这份提案能不能用**，\n\
                 而不是由「它残不残缺」决定（它不残缺）。",
                stage.groups_ok + stage.failed_groups.len(),
                stage.groups,
                stage.budget.reason.as_deref().unwrap_or("(原因未知)"),
                stage.budget.skipped_groups.len(),
                out,
                stats_out,
                s.tables_seen,
                report.tables.len(),
                stage.budget.skipped_groups.len(),
            ),
        }
    }

    // 悬空关系必须为 0 —— 非 0 意味着提案在审核期会被 AC-007 拦，直接失败退出
    if s.links_dangling > 0 {
        anyhow::bail!("提案含 {} 条悬空关系，AC-007 会在审核期拦截", s.links_dangling);
    }
    println!("  提案        : {out}");
    println!("  统计        : {stats_out}");
    Ok(())
}

/// **`--agent` / `--agent-version` 是「断言」，不是「标签」**（REQ-059 · AC-002）。
///
/// 调用方声明了什么执行者，这一轮就必须真的是那个执行者。对不上 ⇒
/// **拒绝出产物**，不告警、不照发。
///
/// # 为什么不给「不一致也照发，只打一行 ⚠」留位置
///
/// `extracted_by` 不是给人看的备注，它是**版本快照**里「这条本体是谁抽的」
/// 那一栏（规格 §8.4）。而 §8.4 存在的全部意义就是版本锁定与 diff 归因 ——
/// 哪天换了个 agent，抽出来的差异会被读成「本体演进」。
///
/// 在这条链上**让人看见但不阻止**，等于发一份**自己都说不清谁抽的**提案出去，
/// 而它看起来与一份可信的提案**完全一样**。本会话连续三次认定的那条判断
/// （「猜错归属比不给更危险」）在这里是同一件事：宁可不出。
///
/// # 为什么能这么做而 REQ-059 之前做不到
///
/// 现在声明来自**命令行**（人），实际来自 `build_agent` 造出来的那个实现
/// （代码），两个值**来源真正独立**，比得才有意义。
/// 之前那次比对是拿 `from_registration(&registration)` 与 `agent.identity()` ——
/// 而 `inproc` 分支的身份**正是前者的同源拷贝**，恒为「一致」，
/// 那道闸门在默认路径上从来没有真正合上过。
fn require_declared_matches_actual(
    field: &str,
    declared: Option<&str>,
    actual: &str,
) -> Result<()> {
    let Some(declared) = declared else {
        return Ok(()); // 没声明 = 不置喙，身份由执行者自报
    };
    if declared == actual {
        return Ok(());
    }
    anyhow::bail!(
        "拒绝出产物：--{field} 声明的执行者是 {declared:?}，\
         而这一轮**真正跑起来的实现**是 {actual:?}。\n\
         \n\
         为什么必须拒：`extracted_by` 记的是「这份提案的 items 由谁抽」，\
         它会进版本快照（规格 §8.4）。声明与事实不符时，产物里记下的是\
         一个**从没跑过**的名字，而 diff 归因会拿它当依据 —— 版本漂移因此\
         被算成「本体演进」，而且**静默**。\n\
         \n\
         怎么办（两条）：\n\
         1) 不传 --{field}：身份就由真正跑起来的实现自报，那**一定**是真的。\n\
         2) 确实要用自己的名字：先在 agent-registry 里登记它，再用 \
         `--agent-registry <name>`（查得到才放行，fail-closed）。\n\
            ⚠️ 本 CLI **目前没有**登记 agent 的子命令，只能经库写入登记。",
        field = field,
        declared = declared,
        actual = actual,
    )
}

/// **第 2 轮起载入「已有类型清单」**（设计点① + 规格 §7.1 阶段 1 的五个输入之四）。
///
/// 返回 `(基版本 seq, 已有类型)`。`round == 1` 返回
/// `([DEFAULT_BASE_VERSION], 空)` —— 第 1 轮没有基版本，「已有类型」按定义为空，
/// 声明 1 就是如实的（也正是 REQ-002 契约里第 1 轮提案 `base_version` 的既有取值）。
///
/// 返回类型是 `u32` 而不是 `i32`：提案里 `Proposal::base_version` 就是 `u32`，
/// 中间不留一次可能变成 `0` / `-1` 的转换 —— `0` 号版本在库里不存在，
/// 一个能被表示出来的假基版本号就是下一次静默错挂的入口。
///
/// # 为什么这个返回值是**提案 `base_version` 的唯一来源**
///
/// 这一版既是清单的来源，也是提案声明的基版本（`OrchestrationInput::base_version`
/// → [`Proposal::base_version`][crate::extract::propose::Proposal::base_version]）。
/// 二者同源于**一次**解析，所以不可能分叉。
/// 曾经它们各说各话（清单取 latest、声明写死 1），代价是：
/// 阶段 1 拿 v2 的类型核 key、apply 的 diff 却挂在 v1 上，
/// 且 §8.4 的版本锁定归因指向了错的那一版，**全程无声**。
///
/// # 为什么这一段在 CLI 而不在 `src/agent/`
///
/// `src/agent/` 有零 I/O 纪律（`tests/test_agent_orchestration.rs` 的
/// `test_agent_modules_have_no_direct_db_access` 盯着）：抽取 agent 只管
/// 「怎么想」，连库是**装配层**的事。而 `orchestrate` 本身收的是
/// `&OrchestrationInput`，那份输入里已经有 `existing_types` —— 所以
/// 「从库里把它取出来」天然属于装配层，也就是这里。
///
/// # 为什么取**最新**版本而不是让用户指定
///
/// 提案落库时挂的基版本（`proposals.base_version_id`）是调用方在
/// `ontok-propose import` 那一步**读产物里的 `base_version`** 选定的；
/// `ontok-propose` 这一步只看得到 `--project`，所以取该 project 的**最新**版本
/// —— 那就是「第 2 轮要接着长」的那一版。
/// 选别的基版本属于**另一个动作**（import），不该在这一步猜。
///
/// ⚠️ 这个「取最新」是**既有语义**，REQ-019 / REQ-020 都不改它：
/// 改的从来不是「选哪一版」，而是「拿这一版做什么」。
///
/// **REQ-020 · 最新一版已被冻结 ⇒ 在这里就停**（此前记录在案的已知风险，已修）：
/// `latest_version` 的 `WHERE` 只有 `project_id`、不过滤 `frozen_at`，
/// 所以取到的可能是一版**冻结**的版本。而冻结检查原先是**事后**的，
/// 只在 import（`src/cli.rs`）与 apply（`src/db/apply.rs`）两处。
/// 于是那一轮的抽取会**照常跑完**（实测 6.9s 量级、3 条规则产物），
/// 直到 `ontok-propose import` 才硬失败 —— **白跑一趟，且失败得很晚**。
/// 本函数在**载入之后立刻**校验（见下面的 `frozen_at` 分支），
/// 让「白跑 + 晚失败」变成「开口就说清」。
///
/// 为什么在这里查、而不是给 `latest_version` 加 `AND frozen_at IS NULL`：
/// 那会改掉「latest 版本」的既有语义，而它有**三个**调用方与另外三个用途
/// （`src/cli.rs` 的 apply 回显、`tests/test_project.rs`、
/// `tests/test_extraction_agent.rs`），其中 `test_project.rs:114` 明确断言
/// 「拿到的就是 seq 最大的那一版」。**可提案性**是这一个调用点的需求，
/// 不是这个查询的语义 —— 混进 SQL 就会波及另外三个调用方。
///
/// # ⚠️ 绝不退回空清单
///
/// 冻结**不许**被当成「没有基版本」而放行空清单：那会让
/// [`MappingViolation::ExistingKeyDeclaredAsNew`] 等三种违规**静默不可达** ——
/// 这正是 REQ-015 的老病根，宁可失败也不许静默。
///
/// # 缺项目 / 缺环境变量 ⇒ 报错，绝不退回空清单
///
/// 悄悄给一份空清单看起来「跑通了」，但那等于把
/// [`MappingViolation::ExistingKeyDeclaredAsNew`]、
/// [`MappingViolation::ExistingDisplayNameChanged`] 重新变成**不可达** ——
/// 这正是 REQ-015 之后 key 双向核对在生产上一次都没跑过的原因。
async fn load_existing_types(round: i32, project: &str) -> Result<(u32, Vec<ExistingType>)> {
    if round <= 1 {
        return Ok((DEFAULT_BASE_VERSION, Vec::new()));
    }
    let url = ontok::config::require_env(ontok::config::DATABASE_URL_ENV)
        .context("第 2 轮起必须从基版本载入「已有类型清单」⇒ 需要 ONTOK_DATABASE_URL")?;
    let pool = ontok::db::pool::connect(&url).await?;
    let projects = ontok::db::project::PgProjectStore::new(pool.clone());

    let Some(p) = projects.get_by_name(project).await? else {
        anyhow::bail!(
            "第 {round} 轮需要「已有类型清单」才能做 key 双向核对（规格 §7.1 阶段 1 的输入之四），\
             但库里没有 project {project:?} —— 没有基版本就比不了「这一轮是新增还是改名」。\
             先把上一轮提案 import 落库，或确认 --project 给对了。"
        );
    };
    let Some(base) = projects.latest_version(p.id).await? else {
        anyhow::bail!(
            "project {project:?} 下还没有任何版本 —— 第 {round} 轮无从取基版本。\
             先跑 ontok-propose import 落一版，再抽第 2 轮。"
        );
    };
    // ── REQ-020：基版本冻结 ⇒ **立刻**停，不要白跑一趟抽取 ──
    //
    // 冻结检查原先只在 import / apply 两处（事后），于是这一轮会照常跑完
    // 才在 import 硬失败。显式的失败、数据都不受损，但「白跑 + 晚失败」
    // 是坏体验：几分钟算力换一句「不能在其上提案」。
    //
    // 这里**不复用** `is_frozen()`：同一次查询已经取回了 `frozen_at`，
    // 再查一遍既多一次往返，也留下一个「两处判断可能漂移」的口子。
    if base.frozen_at.is_some() {
        anyhow::bail!(
            "第 {round} 轮抽不了：project {project:?} 的**最新**版本 v{} 已被冻结，\
             不能在冻结版本上提案（冻结版是判据，AC-006 —— 它的内容不再接受变更）。\n\
             \n\
             这一轮本会把提案挂在这个 base 版本上，所以现在停：跑完再被 import 拒绝是白跑。\n\
             \n\
             怎么处理（三条，按常见程度排）：\n\
             1. 在冻结版上提案本来就不合规范 ⇒ 先开一版新的当基线：\
             `ontok version new --project {project}`，再重跑第 {round} 轮（清单与提案会一起指向那版）。\n\
             2. 若你确实要在这版上提案，先确认 v{} 是被**误冻**的：\
             `ontok version list --project {project}` 看冻结时刻。冻结是显式动作、不可撤销，\
             解冻属人工决策 —— 本步不替你自动绕过。\n\
             3. 若你想要的基线是 v{} **之前**的某一版，那是**另一个动作**\
             （import 时选 base_version），不在本步「取最新」的语义里 ——\
             本步刻意不替你猜是哪一版，也不悄悄改去查上一版。",
            base.seq,
            base.seq,
            base.seq,
        );
    }
    let base_seq = u32::try_from(base.seq).with_context(|| {
        format!(
            "基版本 seq={} 不是合法版本号（库里的版本号从 1 起），\
             拿它当提案的 base_version 会挂到一个不存在的版本上",
            base.seq
        )
    })?;
    let snap = ontok::db::PgStore::new(pool)
        .get_snapshot(base.id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("取 project {project:?} 基版本 v{} 的快照失败", base.seq))?;

    let types = snap
        .entity_types
        .iter()
        .map(|e| ExistingType {
            key: e.key.clone(),
            display_name: e.display_name.clone(),
        })
        .collect();
    // 清单与提案声明共用这一个 `base_seq` —— 同源，不给第二次解析留口子。
    Ok((base_seq, types))
}

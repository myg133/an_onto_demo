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
                println!("REQ-059 抽取归属（extracted_by 记的是**真实执行者**，不是调用方填的占位）:\n  --agent <name> / --agent-version <ver>\n                现在是**断言**而不是标签：不带就由真正跑起来的实现自报（{INPROC_EXECUTOR_ID} v{INPROC_EXECUTOR_VERSION}）；\n                带上就必须与事实相符，对不上**拒绝出产物**。\n                要登记一个自己的名字，请用下面的 --agent-registry（查得到才放行）。");
                println!("REQ-013 agent-registry（带上才连 ONTOK_DATABASE_URL）:\n  --agent-registry <name> / --agent-registry-version <ver> / --round <n>");
                println!("REQ-019:\n  --attempts <n>  每组抽取的尝试次数（默认 {DEFAULT_ATTEMPTS}）\n  --round ≥2 时会连库载入基版本的已有类型（规格 §7.1 阶段 1 的五个输入之一）");
                println!("REQ-022:\n  --budget-tokens <n>  成本闸门：累计用量上限，超了**停**并上报\n                （外置 agent 的强制义务；没设闸又声明会上报用量 ⇒ 入口拒绝）");
                println!("REQ-108 置信度:\n  --src <url>  源库连接串（只读会话；也可用环境变量 SRC_URL / {SOURCE_DATABASE_URL_ENV}）\n  --no-sample  关闭采样：confidence 只由结构信号算，且全部标 data_evidence=none（缺数据证据不进主提案）\n  --pending-out / --rejected-out  待查清单 / 拒绝清单的落点（默认与 --out 同目录的 proposal-pending.json / proposal-rejected.json）");
                return Ok(());
            }
            other => anyhow::bail!("未知参数 {other}"),
        }
    }

    let schema_raw = std::fs::read_to_string(&schema_path)
        .with_context(|| format!("读不到 {schema_path}（先跑 ontok-introspect）"))?;
    let report: SchemaReport = serde_json::from_str(&schema_raw).context("schema JSON 解析失败")?;

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
    let candidates = ontok::extract::propose::link_candidates(&outcome.proposal.items, &keys);
    let mut evidence: std::collections::HashMap<
        usize,
        ontok::extract::sample::SampleEvidence,
    > = std::collections::HashMap::new();

    let sample_url = if no_sample {
        None
    } else {
        src_url
            .clone()
            .or_else(|| std::env::var("SRC_URL").ok().filter(|s| !s.trim().is_empty()))
            .or_else(|| std::env::var(SOURCE_DATABASE_URL_ENV).ok().filter(|s| !s.trim().is_empty()))
    };
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
    let scored = ontok::extract::propose::score_links(
        &outcome.proposal.items,
        &report,
        &keys,
        &evidence,
    );
    ontok::extract::propose::attach_confidence(&mut outcome.proposal.items, &scored);
    let scored_by_band = ontok::domain::confidence::BandCounts::of(scored.iter().map(|(_, c)| &c.band));

    println!();
    println!("置信度（evidence-scorecard-v1）");
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
    println!("  翻译覆盖    : {} / {} 表", names.coverage(&report.tables.iter().map(|t| t.name.clone()).collect::<Vec<_>>()).0, s.tables_seen);
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
    let routing = ontok::extract::propose::route_items(outcome.proposal.items, &scored);
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

    // 待查 / 拒绝清单**不是提案**：它们没有 `extracted_by`，也不该被 import。
    // 显式写 `"importable": false`，免得有人拿它们去 `propose import`
    // 然后在 `propose import` 里再撞一次「项目不存在」以外的怪错误。
    let side_file = |items: &[serde_json::Value], counts_note: &str| -> serde_json::Value {
        serde_json::json!({
            "project": project,
            "base_version": base_version,
            "round": round,
            "importable": false,
            "method": ontok::domain::confidence::METHOD,
            "note": counts_note,
            "items": items,
        })
    };
    let pending_doc = side_file(
        &routing.pending,
        "待查清单（0.30–0.60）：不进主提案；证据不足，需人工判断",
    );
    let rejected_doc = side_file(
        &routing.rejected,
        "被拒绝（<0.30）：不提案；每条都带 veto 理由",
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
    println!(
        "  主提案      : {} 条关系（高 {} · 中 {}）+ 非关系条目",
        routing.counts.high + routing.counts.middle,
        routing.counts.high,
        routing.counts.middle
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

    std::fs::write(&out, serde_json::to_string_pretty(&outcome.proposal)?)
        .with_context(|| format!("写不出 {out}"))?;
    std::fs::write(&stats_out, serde_json::to_string_pretty(&outcome.stats)?)?;

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

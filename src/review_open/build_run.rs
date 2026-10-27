//! ⭐ REQ-163 · **`ontok-builder build run`** —— 一条命令从文档跑到**本机评审台**（纯本地）。
//!
//! ```text
//! ontok-builder build run --input docs/ --work-dir demo/
//!     --agent acp --agent-command pi-acp --agent-skill Deploy/skills/ontok-extract/SKILL.md
//! ```
//!
//! 四段（`①②③④`，与 `--help` 里的编号一致）：
//!
//! | 段 | 做什么 | 连库？ | 产物 |
//! | --- | --- | --- | --- |
//! | ① | `crate::cli::doc_ingest_cmd`（ACP 抽取） | ⛔ | `<work-dir>/assertions.jsonl` |
//! | ② | `crate::cli::doc_merge_cmd`（机械合并） | ⛔ | `<work-dir>/merged.json` |
//! | ③ | 把合并结果写成**草稿** | ⛔ | `<work-dir>/out/proposal.json` |
//! | ④ | 起**本机评审台**（console v2） | ⛔ | 无（`GET /api/state` 看得见草稿） |
//!
//! # ⭐ REQ-163：整条链**一个字节都不写库、不需要任何环境变量**
//!
//! REQ-162 把 `console serve` 变成了纯本地评审台（`--dir`、无口令、无库），但本命令当时
//! 还是「plan 查库 → ①② → 建项目/版本 → import 写库 → review open 要口令 + 要库」——
//! 一跑 `build run` 就掉回旧台。现在：
//!
//! | 旧 | 新 |
//! | --- | --- |
//! | `plan_base_seq`（查库） | [`plan_base_seq`]（**读目录**：`1` 或「最新定版 + 1」） |
//! | `ensure_base`（建项目 / 版本） | **删掉** —— 本地没有「项目表」，工作目录就是项目 |
//! | `import_proposal_core`（写库） | [`write_draft`]（写 `<work-dir>/out/proposal.json`） |
//! | `review_open::open_with`（口令 + 库） | `console::serve::serve`（**同一条**启动路径） |
//!
//! ⇒ `build run` **全程不连库**：`ONTOK_DATABASE_URL` / `ONTOK_SECRET_KEY` 缺了也照常跑通
//! （`tests/test_req163_local_chain.rs` 的真进程判据把这两个变量从子进程环境里删掉再跑）。
//!
//! # 每一段调的都是**既有实现**（⛔ 不复制）
//!
//! | 段 | 调的东西 | 为什么是它 |
//! | --- | --- | --- |
//! | ① 抽取 | [`crate::cli::doc_ingest_cmd`] | 预算闸 / 指纹缓存 / ACP 出产物闸 / `--strict-deterministic` 全在里面；抄一份 = 判据分叉 |
//! | ② 合并 | [`crate::cli::doc_merge_cmd`] | 纯机械对齐 + 输出撞车检查在同一处 |
//! | ③ 草稿 | [`crate::console::local::write_atomic`] + `local::DRAFT_REL` | 与评审台读的**同一个**路径、同一份原子落盘（半份 JSON 会让页面 500） |
//! | ④ 评审台 | [`crate::console::serve::serve`] | 与 `console serve --dir` **同一条**启动路径（回环闸 / 横幅 / 路由全复用）；⛔ 本文件没有第二个 HTTP server |
//!
//! # 项目名与基版本：`--project` / `--base-version` 都只是**可选覆盖**
//!
//! | 值 | 缺省 | 从哪来 |
//! | --- | --- | --- |
//! | 项目名 | **工作目录名** | [`crate::console::local::project_name_of_workdir`]（与评审台同一份 sanitize，⛔ 没有第二份） |
//! | 基版本 | `1`，或「最新定版 + 1」 | [`plan_base_seq`]（只读 `<work-dir>/.ontok/versions/`） |
//!
//! 基版本的含义：草稿标的是「这一份**将成为**哪一版」（定版时评审台取「最新 + 1」），
//! 所以缺省值与 `--base-version` 的校验都按这个口径（≤ 已定版的最新版 ⇒ exit 2）。
//!
//! # 四条裁定落在哪
//!
//! | 裁定 | 落点 |
//! | --- | --- |
//! | **R1** 任一阶段失败 ⇒ 整体失败、⛔ **不产半成品** | 每段失败都 `return Err`，后面的段一个都不跑；③ 只在 ①② 都成功之后（结构判据 `the_draft_write_and_the_desk_serve_happen_after_both_stages`） |
//! | **R2** 末尾**自动**起评审台；`--no-browser` 只影响浏览器 | [`run`] 的最后一步无条件走 `console::serve::serve` |
//! | **R3** ⛔ **不自动 apply / freeze** | [`run`] 的终点是「人看到待审清单」；本文件没有任何签名动作（结构判据 `the_orchestrator_never_applies_or_freezes`） |
//! | **R4** 中间产物落 `--work-dir`（默认 `.ontok-build/`），可重跑、缓存命中即 0 token | [`artifacts`] 是**唯一**的路径取值点 |
//! | **R5** 只做**文档路径** | `--agent` 只认 `acp` |
//!
//! # ⭐ QA 退回项（第 2 轮）：`--bind` 的闸**前移到入参闸**
//!
//! 非回环地址（`0.0.0.0:8899` / 内网 / 公网）必须在 [`validate`] 里就拒：
//! 否则它要等 ①②③ **全跑完**（已落盘、已烧掉 ACP token）才在 ④ 段被拒，
//! 而且退出码会变成 `1`（环境错）而不是 `2`（入参错）—— 与 `--input` / `--agent`
//! 这些同级参数不一致。闸门本身**复用** [`crate::console::serve::check_bind`]
//! （⛔ 不写第二份「什么算回环」），但**文案是 `build run` 自己的**：本命令没有放行开关，
//! ⛔ 不许在报错里教人「加 `--allow-public`」（那个开关 REQ-163 已从本命令删掉）。

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::console::local::{self, DRAFT_REL};
use crate::console::serve::{check_bind, ServeArgs};
use crate::query::serve::parse_bind;

use super::bad;

/// `--work-dir` 的默认值（R4）。
pub const DEFAULT_WORK_DIR: &str = ".ontok-build";

/// 目前唯一支持的 `--agent` 取值（R5：只做文档路径的 ACP 通道）。
pub const ONLY_AGENT: &str = "acp";

/// ⭐ REQ-170 · `--dsn` 勘查出来的 schema 落在**工作目录**里的文件名（`<work-dir>/schema.json`）。
///
/// 为什么是这个名字：`r2rml generate --schema` / `doc merge --schema` / `ontok-introspect --out`
/// 的默认值都是 `out/schema.json` 或 `schema.json`，同一个人工链路里不出现第二个名字。
/// 放在工作目录（而不是 `out/`）：它同时是第 ① 段的输入，`out/` 是**产物**目录。
pub const SCHEMA_REL: &str = "schema.json";

/// `build run` 的参数（已从 clap 拆出来：本模块不认识 clap）。
#[derive(Debug, Clone)]
pub struct Options {
    /// 项目名覆盖。`None` ⇒ **工作目录名**（[`project_name`]）。
    pub project: Option<String>,
    /// 文档目录或单个 `.md` / `.txt`。
    pub input: PathBuf,
    /// `acp`（唯一取值）。
    pub agent: String,
    pub agent_command: String,
    pub agent_skill: PathBuf,
    /// 单轮往返截止（秒）。省略用 `doc ingest` 的默认值。
    pub agent_timeout_secs: Option<u64>,
    pub strict_deterministic: bool,
    /// **工作目录**（中间产物 + 草稿 + `.ontok/`；默认 [`DEFAULT_WORK_DIR`]）。
    pub work_dir: PathBuf,
    /// 真实 schema（`doc merge` 的严格校验）。
    pub schema: Option<PathBuf>,
    /// ⭐ REQ-170 · 源库连接串。给了它就在第 0 段调 [`crate::extract::introspect`]
    /// 勘查出 `<work-dir>/schema.json`，再当 `--schema` 用。
    ///
    /// ⛔ 与 `schema` **互斥**（同时给 ⇒ exit 2，见 [`validate`]）。
    /// ⛔ 本命令**不写任何 DB 连接代码**：勘查只有 `extract::introspect` 那一份实现。
    pub dsn: Option<String>,
    /// 基础提案（省略 = 在工作目录里生成一份空提案，并自动开纯文档通路）。
    pub proposal: Option<PathBuf>,
    /// 强制开纯文档通路（`doc merge --allow-doc-only`）。省略时：生成了空提案就开。
    pub allow_doc_only: bool,
    /// 基版本序号覆盖。`None` ⇒ `1` 或「最新定版 + 1」（[`plan_base_seq`]）。
    pub base_version: Option<i32>,
    /// ⭐ REQ-172 · `propose` 打底：`None` = **自动**（给了 `--dsn` 就开）。
    ///
    /// 为什么自动：库驱动打底的意义是**96 张全覆盖** —— 不给 `--dsn` 时它无事可做，
    /// 所以默认关（纯文档路径逐字节不变）；给了 `--dsn` 却不开，R2RML 仍然只有文档
    /// 命中的那 7 个类，那等于把本需求的目的关掉了。
    pub propose: Option<bool>,
    /// ⭐ REQ-172 · 关系判定契约（`Deploy/skills/ontok-relate/SKILL.md`）。
    ///
    /// 给了它才跑 AI 两轮（A 判定 + B 补漏）；⛔ 路径不写死在代码里。
    /// 省略 ⇒ 只做「规则 + 值域」候选（零 token），并在屏幕上说明「AI 两轮未启用」。
    pub relate_skill: Option<PathBuf>,
    /// 评审台的监听地址（与 `console serve` 同一条语义；默认内核分配端口）。
    pub bind: String,
    pub no_browser: bool,
}

/// 本命令的**项目名**：`--project` 给了就用它，否则 = 工作目录名。
///
/// ⚠️ 缺省分支走 [`local::project_name_of_workdir`] —— 与 `console serve --dir` / 页面标题 /
/// 包 manifest 的**同一个**派生点。抄一份 sanitize 的后果是「CLI 说项目 A、页面说项目 B」。
pub fn project_name(o: &Options) -> String {
    match &o.project {
        Some(p) => p.clone(),
        None => local::project_name_of_workdir(&o.work_dir),
    }
}

/// 草稿落点：`<work-dir>/out/proposal.json`。
///
/// ⚠️ 用 `local::DRAFT_REL` 而不是字面量：这个路径是评审台**读**的那一个
/// （`console serve --dir <work-dir>` 打开的就是它），两处各写一遍迟早会漂。
pub fn draft_path(o: &Options) -> PathBuf {
    o.work_dir.join(DRAFT_REL)
}

/// 四段产物在 `--work-dir` 里的**唯一**落点（R4）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifacts {
    pub work_dir: PathBuf,
    /// ① 的产物：断言 JSONL。
    pub assertions: PathBuf,
    pub assertions_stats: PathBuf,
    /// ② 的输入：基础提案（人给的，或本命令生成的空提案）。
    pub proposal_in: PathBuf,
    /// 基础提案是不是**本命令生成的**（决定要不要自动开纯文档通路）。
    pub proposal_generated: bool,
    /// ② 的产物：合并结果（与 `proposal.json` 同形状）。
    pub merged: PathBuf,
    pub merged_stats: PathBuf,
    /// ⭐ REQ-172 · `propose` 打底后的提案（`<work-dir>/proposal.proposed.json`）。
    pub proposed: PathBuf,
    /// ⭐ REQ-172 · 关系候选（`<work-dir>/relation-candidates.json`）。
    pub candidates: PathBuf,
    /// ⭐ REQ-172 · AI 判定结果（`<work-dir>/relations.json`）。
    pub relations: PathBuf,
    /// ⭐ REQ-172 · 合并了实体打底 + 关系条目的提案（第 ② 段的**实际输入**）。
    pub proposal_final: PathBuf,
    /// ③ 的产物：**草稿**（评审台读的那一份）。
    pub draft: PathBuf,
}

/// 路径表（**纯函数**：判据不需要真跑一遍抽取）。
pub fn artifacts(o: &Options) -> Artifacts {
    let work = o.work_dir.clone();
    let proposal_in = o
        .proposal
        .clone()
        .unwrap_or_else(|| work.join("proposal.json"));
    Artifacts {
        assertions: work.join("assertions.jsonl"),
        assertions_stats: work.join("assertions.stats.json"),
        proposal_generated: o.proposal.is_none(),
        proposal_in,
        merged: work.join("merged.json"),
        merged_stats: work.join("merge.stats.json"),
        proposed: work.join("proposal.proposed.json"),
        candidates: work.join("relation-candidates.json"),
        relations: work.join("relations.json"),
        proposal_final: work.join("proposal.final.json"),
        draft: work.join(DRAFT_REL),
        work_dir: work,
    }
}

/// 一份**空提案**（REQ-143 的「纯文档通路」起点）。
///
/// 为什么需要它：`doc merge` 要两份输入（断言 + 提案），而「从零开始的项目」里
/// 还没有任何提案文件。空提案 = 「本体里现在什么都没有」，于是合并结果全是
/// `origin: doc` 的新条目 —— 那正是「从文档长出本体」的形状。
/// ⛔ 它不是「导入一个空提案」：合并会往里加条目，没加到的（`doc_unmapped`）会被计数。
pub fn empty_proposal_json(project: &str, base_version: i32) -> String {
    let doc = serde_json::json!({
        "project": project,
        "base_version": base_version,
        "round": 1,
        "items": [],
    });
    let mut text = serde_json::to_string_pretty(&doc).expect("空提案序列化失败");
    text.push('\n');
    text
}

/// 入参闸（**纯函数**：不建目录、不写文件、不连库）。
///
/// 为什么单独暴露：`cli::run` 在**任何活儿之前**先调它 —— `--input` 打错时应该看到
/// 「读不到 --input」（exit 2），而不是先看到一段抽取流水账。与 `doc ingest` /
/// `console serve` 在干活之前判入参同一条纪律。
///
/// # ⭐ `--bind` 也在这里判（QA 退回项 F3：它曾要等 ①②③ 跑完才在 ④ 段被拒）
///
/// 这条闸的三个理由，缺一个都不行：
///
/// | 理由 | 后果 |
/// | --- | --- |
/// | **在 ①②③ 之前** | 地址写错时不该先烧一遍 ACP token、还往 `--work-dir` 落一堆文件 |
/// | **exit 2**（[`super::BadInput`]） | 「地址不合法」是**入参错**，与 `--input` / `--agent` 同级；④ 段那条路是 exit 1（环境错），码不一样下游会误判 |
/// | **文案不带 `--allow-public`** | 本命令**没有**放行开关（REQ-163 删掉了）；教人加一个不存在的参数比不报更坏 |
///
/// 判「什么算回环」复用 [`crate::console::serve::check_bind`]（⛔ 不写第二份）。
pub fn validate(o: &Options) -> anyhow::Result<()> {
    let meta = std::fs::metadata(&o.input).map_err(|e| {
        bad(format!(
            "读不到 --input {}：{e}\n  ⛔ 本命令只吃 `.md` / `.txt` 的文档路径（R5）——\
             库路径（`introspect` / `propose`）不在本命令里。",
            o.input.display()
        ))
    })?;
    if !(meta.is_file() || meta.is_dir()) {
        return Err(bad(format!(
            "--input {} 既不是文件也不是目录",
            o.input.display()
        )));
    }
    if o.agent.trim() != ONLY_AGENT {
        return Err(bad(format!(
            "--agent 只认 `{ONLY_AGENT}`（收到 {:?}）。\n  R5：本命令只做**文档路径**\
             （`doc ingest` 的 ACP 通道）；库路径请用 `introspect` + `propose` 那条链。",
            o.agent
        )));
    }
    if o.agent_command.trim().is_empty() {
        return Err(bad(
            "--agent-command 是空的：要写成 `pi-acp`（或绝对路径）。⛔ 没有默认值。",
        ));
    }
    if !o.agent_skill.is_file() {
        return Err(bad(format!(
            "读不到 --agent-skill {}：它必须是存在的文件（或目录里的 `SKILL.md`）。\n  \
             ⛔ 抽取契约不写死在代码里：它随每一块下发给 agent。",
            o.agent_skill.display()
        )));
    }
    // ── ④ 段的地址闸**前移到这里**（F3）：地址错了不该等 ①②③ 跑完 ──
    let addr = parse_bind(&o.bind).map_err(|e| bad(format!("--bind {e}")))?;
    check_bind(addr, false).map_err(|_| {
        // ⚠️ 公文沿用 `check_bind` 的判据、但**换成 `build run` 自己的说法**：
        // 那条文案会教人「加 --allow-public」，而本命令**没有**这个开关（F4）。
        bad(format!(
            "--bind {addr} 不是回环地址：`build run` **只绑回环**（127.0.0.1 / ::1），\
             而且**没有**放行非回环的开关。\n  \
             只给本机：`--bind 127.0.0.1:0`（端口 0 = 内核分配）。\n  \
             要在局域网 / 远程暴露评审台：用 `ontok-builder console serve --dir <工作目录>`\
             并自行加反向代理 + TLS（它同样只绑回环，⛔ 不会替你开口子）。"
        ))
    })?;
    if let Some(p) = &o.proposal {
        if !p.is_file() {
            return Err(bad(format!(
                "读不到 --proposal {p}：它必须是存在的提案文件（`ontok-builder propose` 的产物）",
                p = p.display()
            )));
        }
    }
    // ── ⭐ REQ-170 · 库结构的两个入口**互斥**，且点名 ────────────────
    //
    // 为什么要在这里（入参闸）而不是第 0 段：同时给两个来源时，**哪个说了算**没有
    // 正确答案 —— 先勘查再拿文件盖掉、或者反过来，产出的 schema.json 都取决于顺序，
    // 而用户以为两者等价。所以它是**入参错（exit 2）**，要在烧掉 ACP token 之前报。
    if o.dsn.is_some() && o.schema.is_some() {
        return Err(bad(
            "`--dsn` 与 `--schema` **互斥**：一个是「连库现勘」，一个是「用现成文件」。\n  \
             只给 `--dsn <库 URL>` ⇒ 第 0 段调 introspect（只读 fail-closed）写 \
             `<work-dir>/schema.json`；\n  \
             只给 `--schema <schema.json>` ⇒ 直接用那份文件（人已经跑过 introspect）。\n  \
             ⛔ 两个都给时没有「谁优先」的正确答案 —— 产出的 schema 会取决于顺序。",
        ));
    }
    if let Some(dsn) = &o.dsn {
        if dsn.trim().is_empty() {
            return Err(bad(
                "`--dsn` 是空的：要给一条库连接串（如 postgresql://user:pass@127.0.0.1:5432/db）。\n  \
                 不想连库就别给这个参数（不给 = 纯文档路径，与 REQ-163 逐字节相同）。",
            ));
        }
    }
    // ── ⭐ REQ-172 · 关系探索的两个参数闸（都在烧 token 之前）────────
    if o.propose == Some(true) && o.dsn.is_none() && o.schema.is_none() {
        return Err(bad(
            "`--with-propose` 需要 `--dsn` 或 `--schema`：propose 打底要读一份真实的库结构。\n  \
             两个都没给时没有「拿什么打底」的答案 —— ⛔ 不猜。",
        ));
    }
    if let Some(skill) = &o.relate_skill {
        if o.dsn.is_none() {
            return Err(bad(
                "`--relate-skill` 需要 `--dsn`：判定轮要给两端取样本、判完还要落库对账。\n  \
                 只给 `--schema` 时没有库可读 —— 请改用 `relation judge --candidates ... --dsn ...`。",
            ));
        }
        let ok = skill.is_file() || (skill.is_dir() && skill.join("SKILL.md").is_file());
        if !ok {
            return Err(bad(format!(
                "读不到 `--relate-skill` {}：它必须是存在的 `.md` 文件，或里面装着 `SKILL.md` 的目录。\n  \
                 ⛔ 判定契约不写死在代码里 —— 它是 BA 维护的文档，也是缓存指纹的一部分。",
                skill.display()
            )));
        }
    }
    Ok(())
}
/// ⭐ REQ-170 · 第 0 段：**勘查**（只有给了 `--dsn` 才跑）→ `<work-dir>/schema.json`。
///
/// # 为什么是「第 0 段」而不是塞进 ①
///
/// `doc ingest` 的 prompt 要带库结构，`doc merge` 要用 schema 做严格校验 ——
/// 两者都得**先有 schema**。而勘查失败（连不上 / 不是只读 / 库名不对）属于
/// **环境错**：此时一个字节都还没写（连 `--work-dir` 都不建），⛔ 不留半份草稿。
///
/// # 为什么调 `introspect_with_keys` 而不是自己写 SQL
///
/// 平台里接触客户库的代码**只有** `extract::introspect` 一份（AC-101 的只读红线在
/// 那里）。它同时给出键事实（主键 / 唯一列）—— `r2rml generate` 要这些，缺了它
/// 会把所有类都判成「没有主键」而跳过。⛔ 这里不新增任何一条 SQL、不新建连接池。
///
/// # 与「人先跑一遍 introspect 再 `--schema`」的关系
///
/// 产物**同源**：`ontok-builder introspect --out` 走的也是
/// `introspect_with_keys` + `KeyCatalog::inject_into_product`。所以同一库的
/// `--dsn` 与 `--schema` 得到同一份 `schema.json`（逐字节），下游产物自然一致。
pub async fn resolve_schema(o: &Options) -> anyhow::Result<Option<PathBuf>> {
    let Some(dsn) = o.dsn.as_deref() else {
        // 没给 --dsn：有 --schema 就用它，都没有就 None（老路）。
        return Ok(o.schema.clone());
    };
    if o.schema.is_some() {
        // validate 已经拦过；这里是「不靠调用顺序也安全」的第二道。
        return Err(bad("`--dsn` 与 `--schema` 互斥（内部：勘查前复核）。"));
    }

    eprintln!("── 第 0/4 段：勘查源库（introspect，只读 fail-closed）──────────────");
    let (report, keys) = crate::extract::introspect::introspect_with_keys(dsn)
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "第 0/4 段（勘查）失败：{e:#}\n\
                 ⛔ 后面的四段一个都不会跑 —— 本轮**不产出**断言、不写草稿、不起评审台。\n\
                 排查：库地址 / 只读账号 / 库名；或改用人已经跑好的 `--schema <schema.json>`。",
            )
        })?;
    if report.table_count == 0 {
        anyhow::bail!(
            "第 0/4 段（勘查）失败：源库里**一张表都没有**（schema = {}）—— \
             这样的 schema 没法跟文档对齐，也没法给 r2rml 用。\n  \
             检查 `--dsn` 指向的库对不对（勘查只读 public schema）。",
            report.schema
        );
    }

    // 产物形状与 `ontok-builder introspect --out` **同一份**（SchemaReport + 键字段）。
    let mut product = serde_json::to_value(&report)
        .map_err(|e| anyhow::anyhow!("内部错误：SchemaReport 序列化失败：{e}"))?;
    keys.inject_into_product(&mut product);
    let text = serde_json::to_string_pretty(&product)
        .map_err(|e| anyhow::anyhow!("内部错误：schema.json 序列化失败：{e}"))?;

    // ⚠️ 工作目录在**勘查成功之后**才建：勘查失败时盘上不该出现任何东西。
    if !o.work_dir.as_os_str().is_empty() {
        std::fs::create_dir_all(&o.work_dir).map_err(|e| {
            anyhow::anyhow!(
                "建不出 --work-dir {}：{e}（schema.json 要写在这里）",
                o.work_dir.display()
            )
        })?;
    }
    let path = o.work_dir.join(SCHEMA_REL);
    local::write_atomic(&path, text.as_bytes()).map_err(|e| {
        anyhow::anyhow!(
            "写不出 {}：{}（改法：{}）",
            path.display(),
            e.message,
            e.hint
        )
    })?;
    let single_pk = report
        .tables
        .iter()
        .filter(|t| {
            keys.get(&t.name)
                .and_then(|k| k.primary_key.as_ref())
                .is_some()
        })
        .count();
    println!(
        "源库勘查完成（只读）：{} 张表 / {} 列 · 单列主键 {} / {} · 真实外键 {} 条 → {}",
        report.table_count,
        report.column_count,
        single_pk,
        report.table_count,
        report.foreign_key_count,
        path.display(),
    );
    Ok(Some(path))
}

/// ① ② 两段：**不碰数据库**，只吃本地文件、只产本地文件。
///
/// 拆出来的理由（也是「失败零产物」判据的入口）：这两段的失败面**完全不需要库**
/// （agent 连不上 / 文档格式不对 / 合并撞车），所以那条判据可以在没有任何元数据库的
/// 机器上真跑一遍，而不是标成「未验」。
pub async fn build_artifacts(o: &Options, base_version: i32) -> anyhow::Result<Artifacts> {
    // ── 入参闸（⛔ 全部在干活之前判完）──────────────────────────
    validate(o)?;

    // ── ⭐ REQ-170 · 第 0 段：勘查（只有给了 --dsn 才跑）─────────────
    //
    // 位置在 ①② 之前、`--work-dir` 建立之前：勘查失败 ⇒ 盘上一个字节都没有。
    // 给了 `--schema` 时这里只是把那个路径递下去（不读、不复制）。
    let schema_path = resolve_schema(o).await?;

    if !o.work_dir.as_os_str().is_empty() {
        std::fs::create_dir_all(&o.work_dir).map_err(|e| {
            anyhow::anyhow!(
                "建不出 --work-dir {}：{e}（换一个可写路径）",
                o.work_dir.display()
            )
        })?;
    }

    let art = artifacts(o);

    if let Some(p) = &schema_path {
        eprintln!(
            "· 库结构：{}（第 ① 段随每块下发摘要 · 第 ② 段拿它做严格校验）",
            p.display()
        );
    }

    // ── ① 抽取（既有装配点，一行实现都不抄）────────────────────
    eprintln!("── 第 1/4 段：doc ingest ───────────────────────────────────────");
    crate::cli::doc_ingest_cmd(
        o.input.clone(),
        art.assertions.clone(),
        Some(art.assertions_stats.clone()),
        None, // 预算：用 doc ingest 的默认上限（与单独跑时同一口径）
        None, // 不分块截断
        None, // 缓存落点：默认 `<out>.cache.json` ⇒ 重跑命中即 0 token（R4）
        false,
        Some(o.agent.clone()),
        Some(o.agent_command.clone()),
        Some(o.agent_skill.clone()),
        o.agent_timeout_secs,
        o.strict_deterministic,
        schema_path.clone(), // ⭐ REQ-170：库结构（None ⇒ 与 REQ-163 逐字节相同）
        None,                // 表数闸：用 doc ingest 的默认值（DEFAULT_MAX_TABLES）
        // ⭐ REQ-171 · 选表策略三件套：全表阈值 / 追加垃圾表模式 / 是否保留垃圾表。
        // `build run` 不另设开关 —— 与单独跑 `doc ingest` 用**同一套默认值**
        // （小库全表清单、垃圾表排除），否则同一条链在第 ① 段与第 ② 段会看到不同的表。
        crate::extract::schema_digest::DEFAULT_FULL_LIST_MAX,
        Vec::new(),
        false,
    )
    .await
    .map_err(|e| {
        anyhow::anyhow!(
            "第 1/4 段（doc ingest）失败：{e:#}\n\
             ⛔ R1：后面的三段一个都不会跑 —— 本轮**不产出**合并结果、不写草稿、不起评审台。\
             中间产物留在 {}（重跑会命中指纹缓存，成功的块不重复计费）。",
            art.work_dir.display()
        )
    })?;

    // ── ② 的输入：人给的提案，或本命令生成的空提案 ─────────────
    //
    // ⚠️ 位置在**抽取成功之后**（QA 退回项）：空提案只是第 ② 段的输入，
    // 第 ① 段失败时连它都不该落盘 —— 「失败零产物」在文件层也要成立。
    if art.proposal_generated {
        let project = project_name(o);
        let text = empty_proposal_json(&project, base_version);
        local::write_atomic(&art.proposal_in, text.as_bytes()).map_err(|e| {
            anyhow::anyhow!(
                "写不出基础提案 {}：{}（改法：{}）",
                art.proposal_in.display(),
                e.message,
                e.hint
            )
        })?;
        eprintln!(
            "· 基础提案：本命令生成了空提案 {}（项目 {} · 基版本 v{base_version}）—— \
             纯文档通路会自动打开（新建实体 / 关系的档位是 low，仍需人签）",
            art.proposal_in.display(),
            project
        );
    } else {
        eprintln!("· 基础提案：{}（人给的）", art.proposal_in.display());
    }

    // ── ⭐ REQ-172 · 库驱动打底 + 关系探索（只有 schema 路径存在时才跑）──
    //
    // 顺序是**判据**：先 `propose` 打底（96 张全覆盖）→ probe（零 token 值域）
    // → judge（可选，AI 两轮）→ 把关系条目并进同一份提案 → 第 ② 段。
    // 任何一步失败 ⇒ 整条链失败（R1），⛔ 不写草稿、不起评审台。
    let mut merge_input = art.proposal_in.clone();
    if let Some(sp) = schema_path.as_deref() {
        let schema_file = crate::extract::relation::load_schema(sp).map_err(|e| {
            anyhow::anyhow!(
                "第 0.5/4 段（读 schema）失败：{e:#}\n\
                 ⛔ 后面的段一个都不会跑 —— 本轮不产出提案、不写草稿。"
            )
        })?;
        let base_text = std::fs::read_to_string(&art.proposal_in).map_err(|e| {
            anyhow::anyhow!(
                "读不到基础提案 {}：{e}（第 ② 段的输入）",
                art.proposal_in.display()
            )
        })?;
        let mut proposal_doc: serde_json::Value =
            serde_json::from_str(&base_text).map_err(|e| {
                anyhow::anyhow!("基础提案 {} 不是合法 JSON：{e}", art.proposal_in.display())
            })?;
        let project = project_name(o);
        let with_propose = match o.propose {
            Some(v) => v,
            None => o.dsn.is_some(),
        };

        // ── 0.5 段：propose 打底（纯本地，零 token）──
        if with_propose {
            eprintln!("── 第 0.5/4 段：propose 打底（把schema变成实体/属性/命名关系）────────");
            let names = crate::extract::relation::names_from_comments(&schema_file.report);
            let outcome = crate::extract::propose::extract(
                &schema_file.report,
                &names,
                &project,
                "rule:propose",
                env!("CARGO_PKG_VERSION"),
            )
            .map_err(|e| {
                anyhow::anyhow!(
                    "第 0.5/4 段（propose 打底）失败：{e:#}\n\
                     ⛔ 不写草稿。可用 `--without-propose` 跳过打底（但那样 R2RML 仍只映射文档命中的表）。"
                )
            })?;
            let mut typed = serde_json::to_value(&outcome.proposal)
                .map_err(|e| anyhow::anyhow!("内部错误：propose 产物序列化失败：{e}"))?;
            typed["base_version"] = serde_json::json!(base_version as u32);
            let added = merge_proposal_docs(&mut proposal_doc, &typed)?;
            let text = format!(
                "{}\n",
                serde_json::to_string_pretty(&proposal_doc)
                    .map_err(|e| anyhow::anyhow!("内部错误：提案序列化失败：{e}"))?
            );
            local::write_atomic(&art.proposed, text.as_bytes()).map_err(|e| {
                anyhow::anyhow!(
                    "写不出打底提案 {}：{}（改法：{}）",
                    art.proposed.display(),
                    e.message,
                    e.hint
                )
            })?;
            merge_input = art.proposed.clone();
            eprintln!(
                "· propose 打底：并入 {} 条（门面 {} 张表；提案现共 {} 条）→ {}",
                added,
                outcome.stats.tables_extracted,
                proposal_doc
                    .get("items")
                    .and_then(|v| v.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0),
                art.proposed.display()
            );
        }

        // ── 0.6 段：关系探索（probe 一定跑；judge 只在给了 --relate-skill 时）──
        if let Some(dsn) = o.dsn.as_deref() {
            eprintln!("── 第 0.6/4 段：relation probe（只读值域验证，零 token）──────");
            let pool = crate::extract::sample::sample_pool(dsn)
                .await
                .map_err(|e| {
                    anyhow::anyhow!(
                        "第 0.6/4 段（relation probe）连不上源库：{e:#}\n\
                         ⛔ 不退化只做命名。"
                    )
                })?;
            let probe_opts = crate::extract::relation::ProbeOptions {
                sample_rows: crate::extract::relation::DEFAULT_SAMPLE_ROWS,
                max_pairs: crate::extract::relation::DEFAULT_MAX_PAIRS,
                max_targets_per_column: crate::extract::relation::MAX_TARGETS_PER_COLUMN,
                schema_file: sp.display().to_string(),
                schema_sha256: schema_file.sha256.clone(),
                dsn: dsn.to_string(),
            };
            let probe = crate::extract::relation::probe(&pool, &schema_file, &probe_opts)
                .await
                .map_err(|e| {
                    anyhow::anyhow!(
                        "第 0.6/4 段（relation probe）失败：{e:#}\n\
                         ⛔ 不写草稿。"
                    )
                })?;
            let cand_text = probe.render()?;
            local::write_atomic(&art.candidates, cand_text.as_bytes()).map_err(|e| {
                anyhow::anyhow!(
                    "写不出关系候选 {}：{}（改法：{}）",
                    art.candidates.display(),
                    e.message,
                    e.hint
                )
            })?;
            eprintln!(
                "· 值域验证：{} 条候选（值域确认 {} · 否定 {} · 未测 {}）→ {}",
                probe.stats.candidates_kept,
                probe.stats.domain_confirmed,
                probe.stats.domain_rejected,
                probe.stats.domain_tested
                    - probe.stats.domain_confirmed
                    - probe.stats.domain_rejected,
                art.candidates.display()
            );

            let items = if let Some(skill_path) = &o.relate_skill {
                eprintln!("── 第 0.7/4 段：relation judge（AI 两轮：判定 + 补漏）──────────");
                let skill = crate::extract::doc_acp::load_skill(skill_path)?;
                let skill_sha = crate::extract::sample::sha256_hex(skill.as_bytes());
                let (program, args) = crate::extract::doc_acp::resolve_command(&o.agent_command)?;
                let cfg = crate::agent::acp::AcpConfig {
                    program: program.clone(),
                    args: args.clone(),
                    cwd: crate::extract::doc_acp::session_cwd()?,
                    timeout: crate::extract::doc_acp::resolve_timeout(o.agent_timeout_secs)?,
                };
                let mut session = crate::extract::doc_acp::connect(cfg)?;
                eprintln!(
                    "· 判定实现：acp（{}）—— 握手身份 {} v{}",
                    o.agent_command, session.agent, session.version
                );
                let mut extra: std::collections::BTreeMap<
                    String,
                    std::collections::BTreeSet<String>,
                > = std::collections::BTreeMap::new();
                for c in &probe.candidates {
                    extra
                        .entry(c.child_table.clone())
                        .or_default()
                        .insert(c.child_column.clone());
                    extra
                        .entry(c.parent_table.clone())
                        .or_default()
                        .insert(c.parent_column.clone());
                }
                let (samples, failures) = crate::extract::relation_judge::fetch_samples(
                    &pool,
                    &probe.source.schema,
                    &probe.tables,
                    &extra,
                    crate::extract::relation_judge::DEFAULT_SAMPLE_ROWS,
                )
                .await;
                if !probe.tables.is_empty() && failures == probe.tables.len() {
                    anyhow::bail!(
                        "第 0.7/4 段（relation judge）名片样本一张表都取不到 —— \
                         ⛔ 不空跑模型（白烧 token）。"
                    );
                }
                let model = crate::extract::doc_acp::model_id(
                    &program,
                    &args,
                    &session.agent,
                    &session.version,
                );
                let meta = crate::extract::relation_judge::JudgeOptions {
                    sample_rows: crate::extract::relation_judge::DEFAULT_SAMPLE_ROWS,
                    cache: art.relations.with_file_name("relations.json.cache.json"),
                    out: art.relations.clone(),
                    candidates_file: art.candidates.display().to_string(),
                    candidates_sha256: crate::extract::sample::sha256_hex(cand_text.as_bytes()),
                    skill_file: skill_path.display().to_string(),
                    skill_sha256: skill_sha,
                };
                let mut judged = crate::extract::relation_judge::run(
                    &probe,
                    &skill,
                    &model,
                    &meta,
                    samples,
                    failures,
                    &mut session.transport,
                )
                .map_err(|e| {
                    anyhow::anyhow!(
                        "第 0.7/4 段（relation judge）失败：{e:#}\n\
                         ⛔ 不写草稿、不起评审台；可用 `--relate-skill` 去掉这一轮（只做规则+值域）。"
                    )
                })?;
                judged.source.agent = session.agent.clone();
                judged.source.agent_version = session.version.clone();
                let text = judged.render()?;
                local::write_atomic(&art.relations, text.as_bytes()).map_err(|e| {
                    anyhow::anyhow!(
                        "写不出关系判定 {}：{}（改法：{}）",
                        art.relations.display(),
                        e.message,
                        e.hint
                    )
                })?;
                eprintln!(
                    "· AI 判定：{} 条关系（A {} 批 / B {} 批 · token A {} + B {}）→ {}",
                    judged.stats.relations,
                    judged.round_a.batches,
                    judged.round_b.batches,
                    judged.round_a.tokens,
                    judged.round_b.tokens,
                    art.relations.display()
                );
                judged.link_items(&probe)
            } else {
                eprintln!(
                    "· AI 两轮：**未启用**（没给 --relate-skill）—— 只落规则 + 值域候选；\
                     这**不是**模型失败后的降级，是显式参数缺席。"
                );
                crate::extract::relation::probe_link_items(&probe)
            };
            pool.close().await;

            let (added, replaced) =
                crate::extract::relation::merge_into_proposal(&mut proposal_doc, items)?;
            let text = format!(
                "{}\n",
                serde_json::to_string_pretty(&proposal_doc)
                    .map_err(|e| anyhow::anyhow!("内部错误：提案序列化失败：{e}"))?
            );
            local::write_atomic(&art.proposal_final, text.as_bytes()).map_err(|e| {
                anyhow::anyhow!(
                    "写不出关系提案 {}：{}（改法：{}）",
                    art.proposal_final.display(),
                    e.message,
                    e.hint
                )
            })?;
            merge_input = art.proposal_final.clone();
            eprintln!(
                "· 关系落提案：并入 {} 条 / 替换同名 {} 条 → {}",
                added,
                replaced,
                art.proposal_final.display()
            );
        }
    }

    // ── ② 合并（机械对齐，不调模型）──────────────────────────
    eprintln!("── 第 2/4 段：doc merge ────────────────────────────────────────");
    let doc_only = o.allow_doc_only || art.proposal_generated;
    if doc_only && !o.allow_doc_only {
        eprintln!(
            "· 纯文档通路：开（因为基础提案是本命令生成的空提案；\
             要关掉请给 --proposal <既有提案>）"
        );
    }
    crate::cli::doc_merge_cmd(
        &art.assertions,
        &merge_input,
        &art.merged,
        Some(&art.merged_stats),
        schema_path.as_deref(),
        doc_only,
    )
    .await
    .map_err(|e| {
        anyhow::anyhow!(
            "第 2/4 段（doc merge）失败：{e:#}\n\
             ⛔ R1：不写草稿、不起评审台。断言文件仍在 {}（可单独重跑 `doc merge`）。",
            art.assertions.display()
        )
    })?;

    Ok(art)
}

/// ⭐ REQ-172 · 把 `add` 提案里的条目并进 `base`：**按 `kind + payload.key` 去重**。
///
/// ⛔ 不覆盖人给的条目：`--proposal` 是人的输入，打底只补它没提到的表/边。
/// 返回并入的条数（屏幕上要看得见「补了多少」）。
fn merge_proposal_docs(
    base: &mut serde_json::Value,
    add: &serde_json::Value,
) -> anyhow::Result<usize> {
    fn key_of(it: &serde_json::Value) -> Option<String> {
        let kind = it.get("kind").and_then(|v| v.as_str())?;
        let key = it.pointer("/payload/key").and_then(|v| v.as_str())?;
        Some(format!("{kind}:{key}"))
    }
    let base_items = base
        .get_mut("items")
        .and_then(|v| v.as_array_mut())
        .ok_or_else(|| anyhow::anyhow!("基础提案缺少 `items` 数组"))?;
    let existing: BTreeSet<String> = base_items.iter().filter_map(key_of).collect();
    let add_items = add
        .get("items")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow::anyhow!("合并源缺少 `items` 数组"))?;
    let mut added = 0usize;
    for it in add_items {
        if let Some(k) = key_of(it) {
            if existing.contains(&k) {
                continue;
            }
        }
        base_items.push(it.clone());
        added += 1;
    }
    Ok(added)
}

/// **只读**地算「这一轮草稿标哪一版」—— 读工作目录，⛔ 一个字节都不写、不连库。
///
/// # 口径（与评审台的定版规则对齐）
///
/// | 情形 | 结果 |
/// | --- | --- |
/// | 工作目录里还没有任何定版 | **1**（第一次定版会产出 v1） |
/// | 已有最新定版 v`N` | **`N + 1`**（评审台定版时取「最新 + 1」） |
/// | 给了 `--base-version v` | `v`，但必须 **> 最新定版号**（否则 exit 2） |
///
/// 为什么要先算：`doc merge` 产出的合并结果顶层要带 `base_version`（草稿的抬头）。
/// 为什么**只读**：算错了不该在盘上留下任何东西 —— 它是第 ① 段之前唯一的「计划」动作，
/// 与 REQ-160 的「建项目 / 版本只能在两段成功之后」是同一条判断（AC-006：失败零产物）。
pub fn plan_base_seq(o: &Options) -> anyhow::Result<i32> {
    let latest = local::latest_version_seq_of(&o.work_dir);
    match (o.base_version, latest) {
        (Some(v), _) if v < 1 => Err(bad("`--base-version` 不合法：定版号从 1 开始。")),
        (Some(v), Some(l)) if v <= l => Err(bad(format!(
            "`--base-version {v}` 不大于已定版的最新版本 v{l}（工作目录 {}）。\n  \
             草稿的 `base_version` 标的是「这一份**将成为**哪一版」，而评审台定版时取\
             「最新定版 + 1」⇒ 至少是 v{}。\n  \
             不给 `--base-version` 就用这个缺省值 v{}。",
            o.work_dir.display(),
            l + 1,
            l + 1
        ))),
        (Some(v), _) => Ok(v),
        (None, Some(l)) => Ok(l + 1),
        (None, None) => Ok(1),
    }
}

/// ③ 段产出的草稿（人看一眼就够的三栏：路径 / 条数 / 字节数）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    pub path: PathBuf,
    /// `items` 的条数（评审台第一屏就是「共 N 条，已签 0」）。
    pub items: usize,
    pub bytes: usize,
}

/// ③ **把合并结果写成草稿**：`<work-dir>/out/proposal.json`。
///
/// # 为什么是「复制字节」而不是「重新序列化」
///
/// 草稿的 sha256 会进 `decisions.json`（「裁决是对哪一份草稿签的」）与定版的
/// `meta.json`。重新序列化一轮（不同 pretty / 键序）就会让「同一份合并结果」算出
/// 两个 sha —— 人会在页面上看到莫名其妙的 `stale`。⇒ 这里**逐字节**搬 `merged.json`。
///
/// # 为什么先校验再落盘
///
/// 评审台对草稿只有两个要求：合法 JSON + 有 `items` 数组（`parse_draft`）。在这里
/// 先判一遍，失败时能点名「是合并产物坏了」；否则要等人在浏览器里点开页面才看到 400。
///
/// ⚠️ 位置是承重的：**只能在 ①② 都成功之后**（调用点在 [`run_draft`]；
/// 结构判据 `the_draft_write_and_the_desk_serve_happen_after_both_stages`）。
pub fn write_draft(art: &Artifacts) -> anyhow::Result<Draft> {
    let text = std::fs::read_to_string(&art.merged).map_err(|e| {
        anyhow::anyhow!(
            "读不到合并结果 {}：{e}（先跑 `ontok-builder doc merge` 产出它）",
            art.merged.display()
        )
    })?;
    let doc: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
        anyhow::anyhow!(
            "合并结果 {} 不是合法 JSON：{e}（这是第 ② 段的产物，请重跑 `doc merge`）",
            art.merged.display()
        )
    })?;
    let items = doc.get("items").and_then(|v| v.as_array()).ok_or_else(|| {
        anyhow::anyhow!(
            "合并结果 {} 里没有 `items` 数组 —— 评审台读不懂这样的草稿\
                 （草稿形状是 `{{project, base_version?, items:[{{kind,target,payload}}]}}`）",
            art.merged.display()
        )
    })?;
    local::write_atomic(&art.draft, text.as_bytes()).map_err(|e| {
        anyhow::anyhow!(
            "写不出草稿 {}：{}（改法：{}）",
            art.draft.display(),
            e.message,
            e.hint
        )
    })?;
    Ok(Draft {
        path: art.draft.clone(),
        items: items.len(),
        bytes: text.len(),
    })
}

/// ①②③ 三段（**不连库、不起服务**）：算基版本 → 抽取 → 合并 → 写草稿。
///
/// 单独暴露是为了让「失败零产物」与「草稿在 ①② 之后」这两条判据能在**不起服务**
/// 的前提下真跑（真进程的 ④ 段要 Ctrl-C 才结束）。
///
/// # 顺序是**判据**
///
/// ```text
/// ① plan_base_seq（只读）→ ② 抽取 → ③ 合并 → ④ 写草稿
/// ```
///
/// ④ 不能提前到 ①② 之前：抽取失败（agent 连不上 / 文档格式不对 / 合并撞车）时
/// 连草稿都不该落盘 —— 评审台会把一份半成品当成「人待审的清单」，而它其实什么都没抽到。
pub async fn run_draft(o: &Options) -> anyhow::Result<Artifacts> {
    // ── 计划基版本（**只读**；exit 2 的情形都在这里，一个字节都还没写）──
    let base_seq = plan_base_seq(o)?;

    // ── ① ②（既有装配点）────────────────────────────────────
    let art = build_artifacts(o, base_seq).await?;

    // ── ③ 写草稿（⚠️ 位置承重：只能在 ①② 都成功之后）────────────
    eprintln!("── 第 3/4 段：写草稿（out/proposal.json）───────────────────────");
    let draft = write_draft(&art).map_err(|e| {
        anyhow::anyhow!(
            "第 3/4 段（写草稿）失败：{e:#}\n\
             ⛔ R1：不起评审台。合并结果仍在 {}（可单独重跑 `doc merge`）。",
            art.merged.display()
        )
    })?;
    println!(
        "草稿已写出：{}（{} 条，项目 {}）",
        draft.path.display(),
        draft.items,
        project_name(o)
    );
    if draft.items == 0 {
        eprintln!(
            "⚠ 这一份草稿 **0 条**：评审台上没有可签的东西。\
             多半是文档里没有可对齐的断言（看 {} 里的未映射原因）。",
            art.merged_stats.display()
        );
    }
    // ⛔ R3：这里是**终点**。下面只起评审台，不签、不冻。
    println!(
        "⛔ 本命令**不自动 apply / freeze**：终点是「你在页面上看到待审清单」（R3）。\
         决策只在页面上做。"
    );

    Ok(art)
}

/// 四段串起来：`run_draft` + **起本机评审台**（④）。
///
/// `run` 的函数体只做两件事，顺序是判据：先 [`run_draft`]（算基版本 / ①②③），
/// 全部成功之后才起服务（`console::serve::serve`）—— ①② 失败时端口一个都不占，
/// 更不会出现「页面开着、草稿不存在」那种自相矛盾的界面。
///
/// ⚠️ `--bind` **不在这里判**：它属于入参（[`validate`]），比 ①②③ 还早 ——
/// 地址写错时连 token 都不该烧。
///
/// ⚠️ ④ 是**前台常驻**：它与 `ontok-builder console serve --dir <work-dir>` 起的是
/// 同一个服务（同一份回环闸 / 同一张路由表），Ctrl-C 结束。
pub async fn run(o: Options) -> anyhow::Result<()> {
    // ── ①②③ ───────────────────────────────────────────────
    let art = run_draft(&o).await?;

    // ── ④ 起本机评审台（**同一条**启动路径，⛔ 不另写 HTTP server）──
    eprintln!("── 第 4/4 段：起本机评审台（console v2）────────────────────────");
    eprintln!(
        "· 工作目录 {} 就是这项工作区（草稿 {}）",
        art.work_dir.display(),
        local::DRAFT_REL
    );
    eprintln!(
        "· 等价命令：`ontok-builder console serve --dir {}`（Ctrl-C 结束）",
        art.work_dir.display()
    );
    crate::console::serve::serve(ServeArgs {
        dir: art.work_dir.clone(),
        bind: o.bind.clone(),
        no_browser: o.no_browser,
    })
    .await
}

/// 本文件源码 —— 「⛔ 不自动签」与「③④ 只能在 ①② 之后」的结构判据扫它。
#[cfg(test)]
const MODULE_SRC: &str = include_str!("build_run.rs");

// ══════════════════════════════════════════════════════════════
// 判据（①②③ 真跑；④ 段由 `tests/test_req163_local_chain.rs` 用真进程验）
// ══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "ontok-req163-build-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&d).expect("临时目录");
        d
    }

    fn options(dir: &Path) -> Options {
        Options {
            project: None,
            input: dir.join("docs"),
            agent: ONLY_AGENT.into(),
            agent_command: "ontok-agent-that-does-not-exist-req163".into(),
            agent_skill: dir.join("SKILL.md"),
            agent_timeout_secs: Some(5),
            strict_deterministic: true,
            work_dir: dir.join(".ontok-build"),
            schema: None,
            dsn: None,
            proposal: None,
            allow_doc_only: false,
            base_version: None,
            propose: None,
            relate_skill: None,
            bind: super::super::DEFAULT_BIND.into(),
            no_browser: true,
        }
    }

    /// 造一份最小输入：一个 `.md` + 一份 skill。
    fn write_input(dir: &Path) {
        let docs = dir.join("docs");
        std::fs::create_dir_all(&docs).expect("docs");
        std::fs::write(
            docs.join("a.md"),
            "# 客户\n\n订单属于客户。\n\n## 订单\n\n一张订单只属于一个客户。\n",
        )
        .expect("写文档");
        std::fs::write(dir.join("SKILL.md"), "# 抽取契约（测试用）\n").expect("写 skill");
    }

    /// 取一个 `pub (async) fn` 的函数体（从签名到顶格的 `}`）—— 结构判据的公共入口。
    fn fn_body(name: &str) -> &'static str {
        let needle = format!("fn {name}(");
        let start = MODULE_SRC
            .find(&needle)
            .unwrap_or_else(|| panic!("`fn {name}` 不见了"));
        let rest = &MODULE_SRC[start..];
        let end = rest.find("\n}\n").map(|i| i + 2).unwrap_or(rest.len());
        &rest[..end]
    }

    // ── 失败零产物（R1）────────────────────────────────────────

    #[tokio::test]
    async fn a_failed_ingest_leaves_no_assertions_and_no_merged_proposal() {
        let dir = temp_dir("fail");
        write_input(&dir);
        let o = options(&dir);
        let art = artifacts(&o);

        let err = build_artifacts(&o, 1)
            .await
            .expect_err("agent 连不上时整条链必须失败");
        let msg = format!("{err:#}");
        assert!(msg.contains("第 1/4 段"), "失败要点名是哪一段：{msg}");
        assert!(
            msg.contains("后面的三段一个都不会跑") || msg.contains("不产出"),
            "失败要说清 R1 的后果：{msg}"
        );
        // ⛔ 零产物：断言文件与合并结果都不存在（缓存文件允许存在 —— 那是「花掉的钱」）。
        assert!(
            !art.assertions.exists(),
            "⛔ 抽取失败却留下了断言文件：{}",
            art.assertions.display()
        );
        assert!(
            !art.merged.exists(),
            "⛔ 第 2 段没跑却留下了合并结果：{}",
            art.merged.display()
        );
        assert!(
            !art.draft.exists(),
            "⛔ 第 ①② 段没成功却留下了**草稿**：{}（评审台会把半成品当成待审清单）",
            art.draft.display()
        );
        assert!(
            !art.draft.parent().map(|p| p.exists()).unwrap_or(false),
            "⛔ 连 `out/` 目录都不该建（③ 段没跑过）"
        );
        assert!(
            art.work_dir.is_dir(),
            "工作目录应当建好（可重跑、缓存可复用）"
        );
        // ⚠️ 空提案（第 ② 段的输入）现在也写在**抽取成功之后**：
        // 第 ① 段失败时它不该落盘（「失败零产物」在文件层也要成立）。
        assert!(
            !art.proposal_in.exists(),
            "⛔ 抽取失败却留下了基础提案：{}（它只是第 2 段的输入，不该提前落盘）",
            art.proposal_in.display()
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn a_bad_input_or_a_bad_agent_never_touches_the_work_dir() {
        let dir = temp_dir("badinput");
        write_input(&dir);

        // --input 不存在 ⇒ exit 2，且一个文件都不写（连 work-dir 都不该建）。
        let mut o = options(&dir);
        o.input = dir.join("no-such-docs");
        let e = build_artifacts(&o, 1)
            .await
            .expect_err("输入不存在必须失败");
        assert_eq!(
            super::super::exit_code_for(&e),
            super::super::EXIT_BAD_INPUT,
            "入参不合法必须是 2"
        );

        // --agent 不是 acp ⇒ exit 2（R5：只做文档路径）。
        let mut o = options(&dir);
        o.agent = "library".into();
        let e = build_artifacts(&o, 1)
            .await
            .expect_err("库路径不在本命令里");
        assert_eq!(
            super::super::exit_code_for(&e),
            super::super::EXIT_BAD_INPUT
        );
        assert!(format!("{e:#}").contains("acp"), "{e:#}");

        // --agent-command 为空 ⇒ exit 2（⛔ 没有默认值）。
        let mut o = options(&dir);
        o.agent_command = "  ".into();
        let e = build_artifacts(&o, 1).await.expect_err("空命令必须失败");
        assert_eq!(
            super::super::exit_code_for(&e),
            super::super::EXIT_BAD_INPUT
        );

        // --agent-skill 不存在 ⇒ exit 2。
        let mut o = options(&dir);
        o.agent_skill = dir.join("nope.md");
        let e = build_artifacts(&o, 1)
            .await
            .expect_err("skill 不存在必须失败");
        assert_eq!(
            super::super::exit_code_for(&e),
            super::super::EXIT_BAD_INPUT
        );

        // ⭐ `--bind` 非回环 ⇒ **入参错**（exit 2），而且**在 ①② 之前**：
        // 地址写错时不该先烧一遍 ACP token（QA 退回项 F3）。
        for bad_bind in ["0.0.0.0:8899", "192.168.1.9:8899", "8.8.8.8:8899"] {
            let mut o = options(&dir);
            o.bind = bad_bind.into();
            let e = build_artifacts(&o, 1)
                .await
                .expect_err("非回环地址必须在入参闸就拒");
            assert_eq!(
                super::super::exit_code_for(&e),
                super::super::EXIT_BAD_INPUT,
                "{bad_bind} 是入参错（不是 ④ 段的环境错）"
            );
            let msg = format!("{e:#}");
            assert!(msg.contains("只绑回环"), "要说明本命令的边界：{msg}");
            assert!(msg.contains("--bind"), "要点名那个参数：{msg}");
            // ⛔ F4：文案不许教人加一个**本命令没有**的开关。
            assert!(
                !msg.contains("请加 `--allow-public`") && !msg.contains("加 --allow-public"),
                "⛔ 报错在建议一个已删掉的开关：{msg}"
            );
            assert!(
                !msg.contains("第 1/4 段"),
                "⛔ 地址错了却已经开始抽取（说明闸在 ①② 之后）：{msg}"
            );
        }
        // 反空转：回环地址必须放行（否则上面那条可能只是「什么都拒」）。
        let mut o = options(&dir);
        o.bind = "127.0.0.1:0".into();
        o.agent_command = "ontok-agent-that-does-not-exist-req163".into();
        let e = build_artifacts(&o, 1)
            .await
            .expect_err("回环地址要放行到 ①② 段（那里才因为 agent 不存在而失败）");
        let msg = format!("{e:#}");
        assert!(
            msg.contains("第 1/4 段"),
            "回环地址必须过闸、走到 ① 段：{msg}"
        );
        assert_eq!(
            super::super::exit_code_for(&e),
            1,
            "① 段的失败是环境错（1）"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    // ── R4 · 路径表 ───────────────────────────────────────────

    #[test]
    fn the_work_dir_layout_is_one_place_and_matches_the_documented_names() {
        let dir = PathBuf::from("/tmp/x");
        let o = Options {
            work_dir: dir.clone(),
            ..options(Path::new("."))
        };
        let art = artifacts(&o);
        assert_eq!(art.assertions, dir.join("assertions.jsonl"));
        assert_eq!(art.assertions_stats, dir.join("assertions.stats.json"));
        assert_eq!(art.merged, dir.join("merged.json"));
        assert_eq!(art.merged_stats, dir.join("merge.stats.json"));
        assert!(art.proposal_generated, "没给 --proposal ⇒ 生成空提案");
        assert_eq!(art.proposal_in, dir.join("proposal.json"));
        // ⭐ ③ 的落点必须与评审台读的那一个**同一个**（`out/proposal.json`）。
        assert_eq!(art.draft, dir.join("out/proposal.json"));
        assert_eq!(art.draft, draft_path(&o), "路径表与 draft_path 不许漂");

        let given = PathBuf::from("/tmp/my-proposal.json");
        let o2 = Options {
            work_dir: dir.clone(),
            proposal: Some(given.clone()),
            ..options(Path::new("."))
        };
        let art2 = artifacts(&o2);
        assert_eq!(art2.proposal_in, given, "人给的提案不许被改写");
        assert!(!art2.proposal_generated);
        assert_eq!(DEFAULT_WORK_DIR, ".ontok-build");
    }

    #[test]
    fn the_generated_empty_proposal_is_importable_shaped() {
        let text = empty_proposal_json("demo", 3);
        let v: serde_json::Value = serde_json::from_str(&text).expect("必须是合法 JSON");
        assert_eq!(v["project"], "demo");
        assert_eq!(v["base_version"], 3);
        assert_eq!(v["round"], 1);
        assert_eq!(v["items"].as_array().map(|a| a.len()), Some(0));
        // ⛔ 不预填任何条目，也不夹带状态字段。
        assert!(v.get("importable").is_none());
        // 确定性：同输入两次生成逐字节相同（重跑可比对）。
        assert_eq!(text, empty_proposal_json("demo", 3));
        assert!(text.ends_with('\n'), "落盘文件以换行收尾");
    }

    // ── ⭐ REQ-163 · 项目名 / 基版本：都在**目录**上算，不需要库 ──────

    #[test]
    fn the_project_name_defaults_to_the_work_dir_name_and_can_be_overridden() {
        let dir = temp_dir("Project Name!");
        let o = Options {
            work_dir: dir.clone(),
            ..options(&dir)
        };
        // 缺省 = 目录名（与 `console serve --dir` 同一份 sanitize）
        assert_eq!(
            project_name(&o),
            crate::console::local::project_name_of_workdir(&dir)
        );
        assert_eq!(
            project_name(&o),
            crate::console::local::project_name_of(&std::fs::canonicalize(&dir).unwrap()),
            "两条派生必须一致（否则 CLI 与页面会报不同的项目名）"
        );
        // 目录名里的空格 / 大写 / `!` 都不合法 ⇒ 被 sanitize 掉，且仍是合法项目名
        assert!(
            crate::bundle::pack::is_valid_project_name(&project_name(&o)),
            "sanitize 之后必须是合法项目名：{}",
            project_name(&o)
        );
        // `--project` 是**可选覆盖**
        let o2 = Options {
            project: Some("my_project".into()),
            ..options(&dir)
        };
        assert_eq!(project_name(&o2), "my_project");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_project_name_is_derivable_before_the_dir_exists() {
        // `build run` 在**抽取之前**就要知道项目名：那时 `--work-dir` 可能还不存在。
        let base = temp_dir("notyet");
        let dir = base.join("Demo Workspace");
        assert!(!dir.exists(), "这条判据要的是「目录还不存在」");
        let o = Options {
            work_dir: dir.clone(),
            ..options(&base)
        };
        let name = project_name(&o);
        assert_eq!(name, "demo_workspace", "目录名要按同一份规则 sanitize");
        assert!(crate::bundle::pack::is_valid_project_name(&name));
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn the_base_version_is_one_or_the_latest_finalized_plus_one() {
        let dir = temp_dir("base");
        let o = Options {
            work_dir: dir.clone(),
            ..options(&dir)
        };
        // 一版都没有 ⇒ 1
        assert_eq!(plan_base_seq(&o).unwrap(), 1);

        // 已有 v1 / v2（只读地看目录）⇒ v3
        for seq in [1, 2] {
            let v = dir.join(format!(".ontok/versions/v{seq}"));
            std::fs::create_dir_all(&v).unwrap();
            std::fs::write(v.join("snapshot.json"), b"{}").unwrap();
        }
        assert_eq!(plan_base_seq(&o).unwrap(), 3, "缺省 = 最新定版 + 1");

        // `--base-version` 覆盖：> 最新 才允许
        let ok = Options {
            work_dir: dir.clone(),
            base_version: Some(7),
            ..options(&dir)
        };
        assert_eq!(plan_base_seq(&ok).unwrap(), 7);
        let next = Options {
            work_dir: dir.clone(),
            base_version: Some(3),
            ..options(&dir)
        };
        assert_eq!(plan_base_seq(&next).unwrap(), 3, "恰好是最新 + 1 也允许");

        // ≤ 最新定版 ⇒ exit 2（草稿会标一个不可能成为的版本号）
        for stale in [1, 2] {
            let o2 = Options {
                work_dir: dir.clone(),
                base_version: Some(stale),
                ..options(&dir)
            };
            let e = plan_base_seq(&o2).expect_err("不能低于已定版的最新版本");
            assert_eq!(
                super::super::exit_code_for(&e),
                super::super::EXIT_BAD_INPUT
            );
            let msg = format!("{e:#}");
            assert!(msg.contains("v2"), "要点名最新定版号：{msg}");
            assert!(msg.contains("base_version"), "要解释这个字段的含义：{msg}");
        }
        let zero = Options {
            work_dir: dir.clone(),
            base_version: Some(0),
            ..options(&dir)
        };
        let e = plan_base_seq(&zero).expect_err("0 不是定版号");
        assert_eq!(
            super::super::exit_code_for(&e),
            super::super::EXIT_BAD_INPUT
        );

        // ⛔ 一个字节都没写（这条函数只读目录）。
        assert!(!dir.join(".ontok/versions/v3").exists());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_draft_is_written_atomically_from_the_merged_bytes() {
        let dir = temp_dir("draft");
        let o = options(&dir);
        let art = artifacts(&o);
        std::fs::create_dir_all(&art.work_dir).unwrap();
        let merged = "{\n  \"project\": \"req163_demo\",\n  \"base_version\": 1,\n  \"items\": [\n    {\"kind\": \"add_entity_type\", \"target\": \"客户\", \"payload\": {\"key\": \"customer\"}},\n    {\"kind\": \"add_entity_type\", \"target\": \"订单\", \"payload\": {\"key\": \"order\"}}\n  ]\n}\n";
        std::fs::write(&art.merged, merged).unwrap();

        let draft = write_draft(&art).expect("合并结果是合法的 ⇒ 应当写出草稿");
        assert_eq!(draft.path, dir.join(".ontok-build/out/proposal.json"));
        assert_eq!(draft.items, 2);
        assert_eq!(draft.bytes, merged.len());
        // ⚠️ **逐字节**搬（草稿的 sha256 会进裁决与定版 meta，重排一次就多一个 sha）
        assert_eq!(std::fs::read(&draft.path).unwrap(), merged.as_bytes());
        // 原子落盘：不留 `.tmp`
        assert!(
            !draft.path.with_extension("json.tmp").exists(),
            "落盘之后不该留下临时文件"
        );

        // 合并结果没有 `items` ⇒ 明确报错（评审台读不懂这样的草稿）
        std::fs::write(&art.merged, br#"{"project":"x"}"#).unwrap();
        let e = write_draft(&art).expect_err("没有 items 必须失败");
        assert!(format!("{e:#}").contains("items"), "{e:#}");

        // 合并结果不是 JSON ⇒ 明确报错
        std::fs::write(&art.merged, b"not json").unwrap();
        let e = write_draft(&art).expect_err("坏 JSON 必须失败");
        assert!(format!("{e:#}").contains("不是合法 JSON"), "{e:#}");

        std::fs::remove_dir_all(&dir).ok();
    }

    // ── R3 · 不自动签（结构判据）────────────────────────────────

    /// ⛔ **不自动 apply / freeze**。
    ///
    /// 为什么是源码判据：这两个动作在这条纯本地链上**根本不存在**（没有库），
    /// 所以没有可跑的行为面；结构上能答「这个文件有没有可能调它们」——
    /// 一句 `if` 忘了删也会被抓到。
    ///
    /// ⚠️ 探针用 `concat!` 拼：把那些标识符原样写进本文件会让 `include_str!`
    /// 读到的源码里**恰好含有**探针（自指），判据就永远红了。
    #[test]
    fn the_orchestrator_never_applies_or_freezes() {
        for forbidden in [
            concat!("Pg", "Applier"),
            concat!("apply_", "proposal"),
            concat!("freeze_", "version_core"),
            concat!(".", "freeze("),
            concat!("set_", "decision"),
        ] {
            assert!(
                !MODULE_SRC.contains(forbidden),
                "⛔ R3：`build run` 不许调用 `{forbidden}`（终点是人在页面上审）"
            );
        }
        // 反向判据：这一段**必须**把「不自动签」说出来（屏幕上要能读到）。
        assert!(
            MODULE_SRC.contains("不自动 apply / freeze"),
            "R3 要在输出里明说"
        );
        // 而且终点必须是本机评审台（不是「写完草稿就结束」）。
        assert!(
            MODULE_SRC.contains(concat!("console::serve::", "serve(")),
            "R2：末尾必须自动起评审台"
        );
    }

    /// 四段都调的是**既有实现**（⛔ 不复制一份），而且 ⛔ 没有第二个 HTTP server。
    #[test]
    fn the_four_stages_reuse_the_existing_entry_points() {
        for needle in [
            concat!("crate::cli::doc_", "ingest_cmd("),
            concat!("crate::cli::doc_", "merge_cmd("),
            // ③：草稿的路径与落盘都来自 `console::local`（⛔ 不写字面量路径 / 自己的 fs::write）
            concat!("local::", "write_atomic("),
            concat!("local::", "DRAFT_REL"),
            // ④：与 `console serve --dir` 同一条启动路径
            concat!("crate::console::serve::", "serve("),
        ] {
            assert!(MODULE_SRC.contains(needle), "没复用既有实现：缺 {needle}");
        }
        // ⛔ 不 shell out 去调自己（探针同样避开自指）。
        for forbidden in [
            concat!("Command::new(\"", "ontok"),
            concat!("cargo", " run"),
            concat!("std::process::", "Command"),
        ] {
            assert!(
                !MODULE_SRC.contains(forbidden),
                "⛔ 不 spawn 子进程调自己：{forbidden}"
            );
        }
        // ⛔ **不另写 HTTP server**（R1-4）：评审台只有 `console::serve` 那一份。
        for forbidden in [
            concat!("ax", "um::"),
            concat!("Router::", "new("),
            concat!("Tcp", "Listener"),
            concat!("hyper", "::"),
        ] {
            assert!(
                !MODULE_SRC.contains(forbidden),
                "⛔ 本文件不许自己起服务：出现 `{forbidden}` 就说明有第二个 HTTP server"
            );
        }
        // 缓存：给 None ⇒ doc ingest 用默认 `<out>.cache.json`（重跑 0 token）。
        assert!(
            MODULE_SRC.contains("None, // 缓存落点"),
            "缓存落点必须留给 doc ingest 的默认值（R4）"
        );
    }

    // ── ⭐ REQ-170 · 库结构的两个入口（`--dsn` | `--schema`）──────────

    /// `--dsn` 与 `--schema` **互斥** ⇒ exit 2，且报错**点名两个参数**。
    ///
    /// 为什么是入参错：同时给两个来源时「谁优先」没有正确答案 —— 产出的 schema.json
    /// 会取决于顺序，而人以为两者等价。这里还顺手钉住「没有写任何东西」：
    /// 这条闸在 ①②（以及第 0 段勘查）之前。
    #[test]
    fn dsn_and_schema_are_mutually_exclusive_and_the_error_names_both() {
        let dir = temp_dir("dsn-exclusive");
        write_input(&dir);
        let mut o = options(&dir);
        o.dsn = Some("postgresql://readonly:pw@127.0.0.1:5432/src".into());
        o.schema = Some(dir.join("schema.json"));
        let e = validate(&o).expect_err("两个都给必须失败");
        assert_eq!(
            super::super::exit_code_for(&e),
            super::super::EXIT_BAD_INPUT,
            "库结构的两个入口互斥属于入参错（2）"
        );
        let msg = format!("{e:#}");
        for needle in ["--dsn", "--schema", "互斥"] {
            assert!(msg.contains(needle), "要点名 {needle:?}：{msg}");
        }
        assert!(
            !o.work_dir.exists(),
            "入参闸不该建工作目录（那是第 0/① 段的事）"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// `--dsn ""` ⇒ exit 2（⛔ 不要拿空串去连库，那会报一条与真原因无关的错）。
    #[test]
    fn an_empty_dsn_is_an_input_error_and_points_at_the_flag() {
        let dir = temp_dir("dsn-empty");
        write_input(&dir);
        let mut o = options(&dir);
        o.dsn = Some("   ".into());
        let e = validate(&o).expect_err("空 dsn 必须失败");
        assert_eq!(
            super::super::exit_code_for(&e),
            super::super::EXIT_BAD_INPUT
        );
        assert!(format!("{e:#}").contains("--dsn"), "{e:#}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 都不给 ⇒ 第 0 段是**空操作**（⛔ 不建工作目录、⛔ 不读任何文件）。
    ///
    /// 这条判据守的是零回归的**前提**：`build run`（无库）路径上不许出现任何
    /// 与库结构有关的 I/O 或输出。
    #[tokio::test]
    async fn without_dsn_or_schema_the_schema_step_is_a_noop() {
        let dir = temp_dir("no-schema");
        write_input(&dir);
        let o = options(&dir);
        assert_eq!(resolve_schema(&o).await.unwrap(), None, "都不给 ⇒ None");
        assert!(!o.work_dir.exists(), "⛔ 空操作不许建工作目录");

        // 给了 `--schema` ⇒ 原样递下去（**不读、不复制**：文件由人维护）
        let p = dir.join("given-schema.json");
        std::fs::write(&p, r#"{"tables":[]}"#).unwrap();
        let mut o2 = options(&dir);
        o2.schema = Some(p.clone());
        assert_eq!(resolve_schema(&o2).await.unwrap(), Some(p));
        std::fs::remove_dir_all(&dir).ok();
    }

    // ── QA 退回项（REQ-160）· 等价判据：③④ 只能在 ①② 之后 ──────────

    /// ⛔ **结构判据**：写草稿 / 起评审台**只能在 ①② 都成功之后**。
    ///
    /// # 它承接的是 REQ-160 的 `the_preparatory_write_happens_after_both_stages`
    ///
    /// 那条老判据钉的是「`ensure_base`（建项目 / 建版本）只能在 ①② 之后」——
    /// 它防的事故是：agent 连不上时库里多一个**空壳项目 + v1**（FDE 打错一次项目名
    /// 就多一个没有回收口的废弃物）。REQ-163 把 `ensure_base` **整段删掉**了
    /// （纯本地链没有项目表），所以那条判据字面上无处可钉。
    ///
    /// ⚠️ 但**事故面没有消失**，只是换了宿主：现在「提前落盘」的后果是评审台里出现一份
    /// **半成品草稿**（人打开页面，看到一份其实什么都没抽到的待审清单，还以为能签）。
    /// ⇒ 判据等价替换成同一条纪律的两个新落点：
    ///
    /// | 挪法 | 哪条断言红 |
    /// | --- | --- |
    /// | `write_draft` 挪进 `build_artifacts`（第 ① 段之前） | `build < write` |
    /// | 把 ③ 内联回 `run_draft` 的**开头** | `build < write` |
    /// | `run_draft(&o)` 挪到起服务那一步之后 | `draft < serve` |
    ///
    /// 行为面的一半在「失败时草稿与 `out/` 都不存在」（见本模块的
    /// `a_failed_ingest_leaves_no_assertions_and_no_merged_proposal`）与
    /// `tests/test_req163_local_chain.rs`（真进程）。
    #[test]
    fn the_draft_write_and_the_desk_serve_happen_after_both_stages() {
        // ── `run`：先跑完 ①②③，才起服务 ────────────────────────
        let run = fn_body("run");
        let draft_phase = run
            .find(concat!("run_draft(&", "o)"))
            .expect("`run` 必须先跑 ①②③（`run_draft`）");
        let serve = run
            .find(concat!("console::serve::", "serve("))
            .expect("`run` 必须起本机评审台");
        assert!(
            draft_phase < serve,
            "⛔ 起评审台**只能在** ①②③ 成功之后 —— 现在的顺序是先起服务：\
             抽取失败时页面已经开着，而草稿根本不存在"
        );

        // ── `run_draft`：算基版本 → ①② → 写草稿 ────────────────
        let draft_fn = fn_body("run_draft");
        let plan = draft_fn
            .find(concat!("plan_base_seq(o)"))
            .expect("`run_draft` 必须**只读**地先算基版本（草稿要填 base_version）");
        let build = draft_fn
            .find(concat!("build_artifacts(o, ", "base_seq)"))
            .expect("`run_draft` 必须调 ①② 段");
        let write = draft_fn
            .find(concat!("write_draft(&", "art)"))
            .expect("`run_draft` 必须写草稿");
        assert!(
            plan < build,
            "算基版本必须在抽取之前（否则草稿的 base_version 没法填）"
        );
        assert!(
            build < write,
            "⛔ 写草稿必须在 ①② 段**成功之后** —— 现在的顺序是先写草稿：\
             agent 连不上时评审台里会出现一份半成品草稿"
        );

        // ── ② 段自己绝不许写草稿（写草稿只能收在 `run_draft` 的 ③）────
        let build_fn = fn_body("build_artifacts");
        for w in [concat!("write_", "draft("), concat!("DRAFT_", "REL")] {
            assert!(
                !build_fn.contains(w),
                "⛔ `build_artifacts` 的函数体里出现了 `{w}` —— ①② 段不许碰草稿，\
                 否则它又跑在两段成功之前"
            );
        }
        // 同理：起服务只许出现在 `run` 里（`run_draft` 不许起服务 —— 那是判据跑得动的前提）。
        let all_serve_calls = MODULE_SRC
            .matches(concat!("console::serve::", "serve("))
            .count();
        assert_eq!(
            all_serve_calls, 1,
            "起评审台只许有**一处**调用（现在 {all_serve_calls} 处）"
        );
    }
}

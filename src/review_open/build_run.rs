//! ⭐ REQ-160 · **`ontok-builder build run`** —— 一条命令从文档跑到审核台。
//!
//! ```text
//! ontok-builder build run --project demo --input docs/ \
//!     --agent acp --agent-command pi-acp --agent-skill Deploy/skills/ontok-extract/SKILL.md
//! ```
//!
//! # 它只是**便利入口**，不是唯一入口（风险 2 的缓解）
//!
//! 四段各自仍然可以单独跑（`doc ingest` / `doc merge` / `propose import` / `review open`）：
//! `--work-dir` 把中间产物留在盘上，所以「某一阶段想单独重跑」不需要重跑整条链。
//!
//! # 每一段调的都是**既有实现**（⛔ 不复制）
//!
//! | 段 | 调的函数 | 为什么是它 |
//! | --- | --- | --- |
//! | ① 抽取 | [`crate::cli::doc_ingest_cmd`]（`doc ingest` 的装配点） | 预算闸 / 指纹缓存 / ACP 出产物闸 / `--strict-deterministic` 全在里面；抄一份 = 判据分叉 |
//! | ② 合并 | [`crate::cli::doc_merge_cmd`]（`doc merge` 的装配点） | 纯机械对齐 + 输出撞车检查在同一处 |
//! | ③ 导入 | [`crate::cli::import_proposal_core`]（`propose import` / `POST /api/import` 的同一个） | 「不猜项目 / `importable` 硬闸 / 基版本未冻结」只有一份 |
//! | ④ 收口 | [`super::open`]（`review open` 的实现） | 本需求的**唯一出口** |
//!
//! ⛔ 本文件**不 shell out 去调自己**：那是「两条实现」的开始（argv 一变就漂）。
//!
//! # 四条裁定落在哪
//!
//! | 裁定 | 落点 |
//! | --- | --- |
//! | **R1** 任一阶段失败 ⇒ 整体失败、⛔ **不产半成品** | 每段失败都 `return Err`，后面的段一个都不跑；产物文件由各段自己的闸保证「要么完整、要么不存在」（`doc ingest` 的预算/严格闸、`doc merge` 的先合并后写） |
//! | **R2** 末尾**自动** `review open`；`--no-browser` 只影响浏览器 | [`run`] 的最后一步无条件调 [`super::open`]；`--no-browser` 只传给它的 `no_browser` |
//! | **R3** ⛔ **不自动 apply / freeze** | [`run`] 的终点是「人看到待审清单」；本文件**没有**任何 apply / freeze 调用（结构判据 `the_orchestrator_never_applies_or_freezes`） |
//! | **R4** 中间产物落 `--work-dir`（默认 `.ontok-build/`），可重跑、缓存命中即 0 token | [`artifacts`] 是**唯一**的路径取值点；缓存文件由 `doc ingest` 的老规矩产生（`<out>.cache.json`），重跑直接命中 |
//! | **R5** 只做**文档路径** | `--agent` 只认 `acp`；库路径（`introspect` / `propose`）不在本命令里，也不接受别的取值 |
//!
//! # 基版本（`base_version`）从哪来 —— 这是本命令**唯一**的写库动作（不是 apply）
//!
//! `propose import` 要求提案挂在一个**已存在且未冻结**的版本上（`import_proposal_core`
//! 的 `version_by_seq`）。所以本命令需要两件准备：**知道**落点、**建出**落点。
//!
//! ⚠️ **这两件事是分开的，顺序是判据**（QA REQ-160 退回项）：
//!
//! | 步骤 | 函数 | 写不写库 | 什么时候 |
//! | --- | --- | --- | --- |
//! | ① 算序号 | [`plan_base_seq`] | **只读** | 第 ① 段抽取**之前**（空提案要填 `base_version`） |
//! | ② 建项目 / 建版本 | [`ensure_base`] | **写** | 第 ① ② 段**都成功之后**、导入之前 |
//!
//! 为什么不能把 ② 提前到 ①：抽取失败（agent 连不上 / 文档格式不对 / 合并撞车）时
//! **连项目名都不该被占用** —— FDE 打错一次项目名就多一个空壳项目 + v1，而空壳没有
//! 回收入口。AC-006 的门面正是「不产出断言文件、**不建版本**」。
//! 结构判据：`the_preparatory_write_happens_after_both_stages`（把它挪回开头即红）。
//!
//! 两者都只建「空容器」，⛔ 不产生任何本体内容；建了什么会**打在屏幕上**。
//! 最新一版**已冻结**时**不**自动再建一版：那是「项目在生产中」的信号，该由人决定
//! （报错并给出 `version new` 的写法）。

use std::path::PathBuf;

use sqlx::postgres::PgPool;

use crate::db::project::PgProjectStore;

use super::{bad, OpenArgs};

/// `--work-dir` 的默认值（R4）。
pub const DEFAULT_WORK_DIR: &str = ".ontok-build";

/// 目前唯一支持的 `--agent` 取值（R5：只做文档路径的 ACP 通道）。
pub const ONLY_AGENT: &str = "acp";

/// `build run` 的参数（已从 clap 拆出来：本模块不认识 clap）。
#[derive(Debug, Clone)]
pub struct Options {
    pub project: String,
    /// 文档目录或单个 `.md` / `.txt`。
    pub input: PathBuf,
    /// `acp`（唯一取值）。
    pub agent: String,
    pub agent_command: String,
    pub agent_skill: PathBuf,
    /// 单轮往返截止（秒）。省略用 `doc ingest` 的默认值。
    pub agent_timeout_secs: Option<u64>,
    pub strict_deterministic: bool,
    /// 中间产物落点（默认 [`DEFAULT_WORK_DIR`]）。
    pub work_dir: PathBuf,
    /// 真实 schema（`doc merge` 的严格校验 + 审核台的声明校验）。
    pub schema: Option<PathBuf>,
    /// 基础提案（省略 = 在工作目录里生成一份空提案，并自动开纯文档通路）。
    pub proposal: Option<PathBuf>,
    /// 强制开纯文档通路（`doc merge --allow-doc-only`）。省略时：生成了空提案就开。
    pub allow_doc_only: bool,
    /// 基版本序号。省略 = 该项目最新版本（没有就建 v1）。
    pub base_version: Option<i32>,
    /// 透传给 `review open` 的那几个开关。
    pub bind: String,
    pub allow_public: bool,
    pub no_browser: bool,
    pub token_ttl: String,
    pub base: Option<String>,
    pub audit_out: Option<PathBuf>,
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
    /// ② 的产物：合并结果（与 `proposal.json` 同形状，可直接 `propose import`）。
    pub merged: PathBuf,
    pub merged_stats: PathBuf,
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
/// 为什么单独暴露：`build run` 的 ③④ 段要元数据库，命令整体在**连库之后**分派；
/// 若入参校验也放在那里，没配库的机器上 `--input` 打错也只会看到「ONTOK_DATABASE_URL
/// 未设置」——报错指向错的原因，退出码也从 2 变成 1。所以 CLI 在连库之前先调本函数
/// （与 `doc ingest` / `review open` 在连库之前分派同一条纪律）。
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
    if let Some(p) = &o.proposal {
        if !p.is_file() {
            return Err(bad(format!(
                "读不到 --proposal {p}：它必须是存在的提案文件（`ontok-builder propose` 的产物）",
                p = p.display()
            )));
        }
    }
    Ok(())
}

/// ① ② 两段：**不碰数据库**，只吃本地文件、只产本地文件。
///
/// 拆出来的理由（也是判据 6 的入口）：这两段的失败面**完全不需要库**
/// （agent 连不上 / 文档格式不对 / 合并撞车），所以「失败零产物」可以在没有元数据库的
/// 机器上真跑一遍，而不是标成「未验」。
pub async fn build_artifacts(o: &Options, base_version: i32) -> anyhow::Result<Artifacts> {
    // ── 入参闸（⛔ 全部在干活之前判完）──────────────────────────
    validate(o)?;
    if !o.work_dir.as_os_str().is_empty() {
        std::fs::create_dir_all(&o.work_dir).map_err(|e| {
            anyhow::anyhow!(
                "建不出 --work-dir {}：{e}（换一个可写路径）",
                o.work_dir.display()
            )
        })?;
    }

    let art = artifacts(o);

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
    )
    .await
    .map_err(|e| {
        anyhow::anyhow!(
            "第 1/4 段（doc ingest）失败：{e:#}\n\
             ⛔ R1：后面的三段一个都不会跑 —— 本轮**不产出**合并结果、不建提案、不起审核台。\
             中间产物留在 {}（重跑会命中指纹缓存，成功的块不重复计费）。",
            art.work_dir.display()
        )
    })?;

    // ── ② 的输入：人给的提案，或本命令生成的空提案 ─────────────
    //
    // ⚠️ 位置在**抽取成功之后**（QA 退回项）：空提案只是第 ② 段的输入，
    // 第 ① 段失败时连它都不该落盘 —— 「失败零产物」在文件层也要成立。
    if art.proposal_generated {
        let text = empty_proposal_json(&o.project, base_version);
        std::fs::write(&art.proposal_in, text).map_err(|e| {
            anyhow::anyhow!("写不出基础提案 {}：{e}", art.proposal_in.display())
        })?;
        eprintln!(
            "· 基础提案：本命令生成了空提案 {}（项目 {} · 基版本 v{base_version}）—— \
             纯文档通路会自动打开（新建实体 / 关系的档位是 low，仍需人签）",
            art.proposal_in.display(),
            o.project
        );
    } else {
        eprintln!("· 基础提案：{}（人给的）", art.proposal_in.display());
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
        &art.proposal_in,
        &art.merged,
        Some(&art.merged_stats),
        o.schema.as_deref(),
        doc_only,
    )
    .await
    .map_err(|e| {
        anyhow::anyhow!(
            "第 2/4 段（doc merge）失败：{e:#}\n\
             ⛔ R1：不导入、不起审核台。断言文件仍在 {}（可单独重跑 `doc merge`）。",
            art.assertions.display()
        )
    })?;

    Ok(art)
}

/// **只读**地算「这一轮会落在哪个基版本上」（⛔ 一个字节都不写）。
///
/// # 为什么必须先算、但不能先建（REQ-160 · QA 退回项）
///
/// `doc merge` 产出的 `merged.json` 顶层要带 `base_version`（`propose import`
/// 按它找基版本），所以第 ② 段**之前**就得知道这个序号。但**知道**与**创建**
/// 是两件事：
///
/// | | 什么时候做 | 为什么 |
/// | --- | --- | --- |
/// | 算序号（本函数） | 第 ① 段**之前** | 只读查询；给空提案填 `base_version` |
/// | 建项目 / 建版本（[`ensure_base`]） | 第 ① ② 段**都成功之后** | 只有「确实要导入」时才需要落库 |
///
/// 反例（QA 实测的那次）：agent 连不上 ⇒ 第 ① 段就失败 ⇒ 若提前 `get_or_create`
/// + `new_version()`，库里会多出一个**空壳项目 + v1**。FDE 打错一次项目名，
/// 就永久多一个空壳；而 AC-006 的门面是「不建版本」。所以创建挪到后面。
///
/// 序号怎么算（全部只读）：
/// - 项目不存在 ⇒ **1**（真建的时候是 v1）；
/// - 项目存在但没有版本 ⇒ **1**；
/// - 给了 `--base-version` ⇒ 它必须在、且未冻结，否则 exit 2（此时**什么都还没写**）；
/// - 否则取最新一版：已冻结 ⇒ exit 2（⛔ 不自动开新版，那是人的决定）。
pub async fn plan_base_seq(pool: &PgPool, o: &Options) -> anyhow::Result<i32> {
    let projects = PgProjectStore::new(pool.clone());
    let Some(proj) = projects
        .get_by_name(&o.project)
        .await
        .map_err(|e| anyhow::anyhow!("查项目 {} 失败：{e}", o.project))?
    else {
        // 项目还不存在：真要导入时会建它 + v1，所以这里的计划序号是 1。
        if let Some(v) = o.base_version {
            if v != 1 {
                return Err(bad(format!(
                    "项目 {} 还不存在，`--base-version {v}` 无从谈起 —— \
                     新建的项目只有 v1（要别的序号请先建项目 / 版本）。",
                    o.project
                )));
            }
        }
        return Ok(1);
    };
    let versions = projects
        .list_versions(proj.id)
        .await
        .map_err(|e| anyhow::anyhow!("查项目 {} 的版本失败：{e}", o.project))?;
    match o.base_version {
        Some(v) => {
            let row = versions.iter().find(|x| x.seq == v).ok_or_else(|| {
                bad(format!(
                    "版本 v{v} 不存在（项目 {}）。现有：{}",
                    o.project,
                    if versions.is_empty() {
                        "（一版都没有）".to_string()
                    } else {
                        versions
                            .iter()
                            .map(|x| format!("v{}", x.seq))
                            .collect::<Vec<_>>()
                            .join(" / ")
                    }
                ))
            })?;
            if row.frozen_at.is_some() {
                return Err(bad(format!(
                    "版本 v{v} 已冻结（项目 {}）：⛔ 冻结的版本只读，不能在它上面提新提案。\n  \
                     先新开一版：`ontok-builder version new --project {}`，\
                     再用 `--base-version <新序号>` 重跑（⛔ 本命令不自动开新版本）。",
                    o.project, o.project
                )));
            }
            Ok(v)
        }
        None => match versions.last() {
            Some(v) if v.frozen_at.is_none() => Ok(v.seq),
            Some(v) => Err(bad(format!(
                "最新版本 v{} 已冻结（项目 {}）。\n  \
                 ⛔ 本命令**不自动**在冻结版本之上开新版本（那是人的决定）：\
                 先 `ontok-builder version new --project {}` 再重跑。",
                v.seq, o.project, o.project
            ))),
            None => Ok(1),
        },
    }
}

/// **准备性写库**（⛔ 不是 apply / freeze）：确保项目存在、且有一个**未冻结**的基版本。
///
/// ⚠️ 这是本命令唯一的两处写库，**只在第 ① ② 段都成功之后**才被调用
/// （调用点在 [`run`]；判据 `the_preparatory_write_happens_after_both_stages`）。
///
/// 返回真实基版本序号。若与 [`plan_base_seq`] 算出的不同（并发下别人先建了版本），
/// **不猜**：直接失败并让人重跑 —— 把提案挂到没算过的版本上比报错危险。
pub async fn ensure_base(
    pool: &PgPool,
    o: &Options,
    planned: i32,
) -> anyhow::Result<crate::db::project::ProjectRow> {
    let projects = PgProjectStore::new(pool.clone());
    let proj = projects
        .get_or_create(&o.project)
        .await
        .map_err(|e| anyhow::anyhow!("项目 {} 取得 / 新建失败：{e}", o.project))?;
    let versions = projects
        .list_versions(proj.id)
        .await
        .map_err(|e| anyhow::anyhow!("查项目 {} 的版本失败：{e}", o.project))?;
    let base_seq = match versions.last() {
        Some(v) if v.frozen_at.is_none() => Some(v.seq),
        Some(v) => {
            return Err(bad(format!(
                "最新版本 v{} 已冻结（项目 {}）。\n  ⛔ 本命令**不自动**在冻结版本之上开新版本：\
                 先 `ontok-builder version new --project {}` 再重跑。",
                v.seq, o.project, o.project
            )));
        }
        None => None,
    };
    let actual = match base_seq {
        Some(seq) => seq,
        None => {
            let v = projects
                .new_version(proj.id)
                .await
                .map_err(|e| anyhow::anyhow!("给项目 {} 建 v1 失败：{e}", o.project))?;
            eprintln!(
                "· 项目 {} 还没有任何版本 ⇒ 已建 v{}（**未冻结**、无内容；\
                 ⛔ 本命令不 apply / freeze）",
                o.project, v.seq
            );
            v.seq
        }
    };
    if actual != planned {
        return Err(anyhow::anyhow!(
            "基版本在本次运行期间变了（计划 v{planned}，实际 v{actual}）—— \
             多半是另一个进程刚建了版本。⛔ 不把提案挂到没算过的版本上：\
             请重跑一次（`--work-dir` 里的产物可以直接复用）。"
        ));
    }
    Ok(proj)
}

/// 四段串起来（R1 的「整体失败」由 `?` 保证；R2 的「末尾自动 review open」在最后）。
///
/// # 顺序是**判据**（QA 退回项）：准备性写库在两段成功之后
///
/// ```text
/// ① plan_base_seq（只读）→ ② 抽取 → ③ 合并 → ④ ensure_base（建项目/版本）→ ⑤ 导入 → ⑥ 起审核台
/// ```
///
/// 为什么 ④ 不能提前到 ①：抽取失败时**连项目名都不该被占用**（AC-006 的门面是
/// 「不产出断言文件、**不建版本**」）—— FDE 打错一次项目名就多一个空壳，
/// 而空壳没有任何回收入口。结构判据见
/// `the_preparatory_write_happens_after_both_stages`（把 `ensure_base` 挪回开头即红）。
pub async fn run(pool: &PgPool, o: Options) -> anyhow::Result<()> {
    // ── 计划基版本（**只读**；exit 2 的三种情形都在这里，一个字节都还没写）──
    let base_seq = plan_base_seq(pool, &o).await?;

    // ── ① ② ────────────────────────────────────────────────
    let art = build_artifacts(&o, base_seq).await?;

    // ── 准备性写库：项目 + 未冻结的基版本（⛔ 不是 apply / freeze）──
    // ⚠️ 位置是承重的：它在两段成功**之后**（见函数文档与那条结构判据）。
    ensure_base(pool, &o, base_seq).await?;

    // ── ③ 导入（与 `propose import` / `POST /api/import` **同一个**函数）──
    eprintln!("── 第 3/4 段：proposal import ──────────────────────────────────");
    let text = std::fs::read_to_string(&art.merged)
        .map_err(|e| anyhow::anyhow!("读不到合并结果 {}：{e}", art.merged.display()))?;
    let doc: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| anyhow::anyhow!("合并结果 {} 不是合法 JSON：{e}", art.merged.display()))?;
    let receipt = crate::cli::import_proposal_core(
        pool,
        &doc,
        &art.merged,
        Some(&o.project),
        true,
    )
    .await
    .map_err(|e| {
        anyhow::anyhow!(
            "第 3/4 段（proposal import）失败：[{}] {}\n  {}\n\
             ⛔ R1：不起审核台。合并结果仍在 {}（可单独重跑 `import --file`）。",
            e.code,
            e.message,
            e.hint,
            art.merged.display()
        )
    })?;
    println!(
        "提案 {} 已导入，共 {} 个条目（基版本 v{base_seq}）：",
        receipt.alias,
        receipt.items.len()
    );
    for r in receipt.items.iter().take(20) {
        println!("  [{}] {} {}", r.seq, r.kind, r.target_key);
    }
    if receipt.items.len() > 20 {
        println!("  …（其余 {} 条在页面上看）", receipt.items.len() - 20);
    }
    // ⛔ R3：这里是**终点**。下面只起审核台，不 apply、不 freeze。
    println!(
        "⛔ 本命令**不自动 apply / freeze**：终点是「你在页面上看到待审清单」（R3）。\
         决策只在页面上做。"
    );

    // ── ④ 收口（**同进程**调用，不 spawn 子进程）──────────────────
    eprintln!("── 第 4/4 段：review open ──────────────────────────────────────");
    super::open_with(
        pool,
        OpenArgs {
            project: o.project.clone(),
            version: Some(base_seq),
            bind: o.bind.clone(),
            allow_public: o.allow_public,
            no_browser: o.no_browser,
            token_ttl: o.token_ttl.clone(),
            schema: o.schema.clone(),
            base: o.base.clone(),
            audit_out: o.audit_out.clone(),
        },
    )
    .await
}

/// 本文件源码 —— 「⛔ 不自动签」的结构判据扫它。
#[cfg(test)]
const MODULE_SRC: &str = include_str!("build_run.rs");

// ══════════════════════════════════════════════════════════════
// 判据（① ② 段真跑；③ ④ 段需要库 ⇒ 结构判据 + 如实标注）
// ══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "ontok-req160-build-{tag}-{}-{}",
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
            project: "req160_demo".into(),
            input: dir.join("docs"),
            agent: ONLY_AGENT.into(),
            agent_command: "ontok-agent-that-does-not-exist-req160".into(),
            agent_skill: dir.join("SKILL.md"),
            agent_timeout_secs: Some(5),
            strict_deterministic: true,
            work_dir: dir.join(".ontok-build"),
            schema: None,
            proposal: None,
            allow_doc_only: false,
            base_version: None,
            bind: super::super::DEFAULT_BIND.into(),
            allow_public: false,
            no_browser: true,
            token_ttl: super::super::DEFAULT_TOKEN_TTL.into(),
            base: None,
            audit_out: None,
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

    // ── 测试 6 · build run 失败零产物 ─────────────────────────

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
        assert!(
            msg.contains("第 1/4 段"),
            "失败要点名是哪一段：{msg}"
        );
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
        let e = build_artifacts(&o, 1).await.expect_err("输入不存在必须失败");
        assert_eq!(
            super::super::exit_code_for(&e),
            super::super::EXIT_BAD_INPUT,
            "入参不合法必须是 2"
        );

        // --agent 不是 acp ⇒ exit 2（R5：只做文档路径）。
        let mut o = options(&dir);
        o.agent = "library".into();
        let e = build_artifacts(&o, 1).await.expect_err("库路径不在本命令里");
        assert_eq!(super::super::exit_code_for(&e), super::super::EXIT_BAD_INPUT);
        assert!(format!("{e:#}").contains("acp"), "{e:#}");

        // --agent-command 为空 ⇒ exit 2（⛔ 没有默认值）。
        let mut o = options(&dir);
        o.agent_command = "  ".into();
        let e = build_artifacts(&o, 1).await.expect_err("空命令必须失败");
        assert_eq!(super::super::exit_code_for(&e), super::super::EXIT_BAD_INPUT);

        // --agent-skill 不存在 ⇒ exit 2。
        let mut o = options(&dir);
        o.agent_skill = dir.join("nope.md");
        let e = build_artifacts(&o, 1).await.expect_err("skill 不存在必须失败");
        assert_eq!(super::super::exit_code_for(&e), super::super::EXIT_BAD_INPUT);

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
        // ⛔ 不夹带 `importable: false`（那会被导入闸挡下），也不预填任何条目。
        assert!(v.get("importable").is_none());
        // 确定性：同输入两次生成逐字节相同（重跑可比对）。
        assert_eq!(text, empty_proposal_json("demo", 3));
        assert!(text.ends_with('\n'), "落盘文件以换行收尾");
    }

    // ── R3 · 不自动签（结构判据）────────────────────────────────

    /// ⛔ **不自动 apply / freeze**。
    ///
    /// 为什么是源码判据：这两个动作需要真库才能「跑完再看版本状态」，
    /// 而本机没有元数据库（派单 §4 的如实登记）。源码判据不依赖环境，
    /// 且**结构上**能答「这个文件有没有可能调它们」—— 一句 `if` 忘了删也会被抓到。
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
        // 而且终点必须是 review open（不是「导入完就结束」）。
        assert!(
            MODULE_SRC.contains("super::open_with("),
            "R2：末尾必须自动起审核台"
        );
    }

    /// 四段都调的是**既有实现**（⛔ 不复制一份）。
    #[test]
    fn the_four_stages_reuse_the_existing_entry_points() {
        for needle in [
            concat!("crate::cli::doc_", "ingest_cmd("),
            concat!("crate::cli::doc_", "merge_cmd("),
            concat!("crate::cli::import_", "proposal_core("),
        ] {
            assert!(MODULE_SRC.contains(needle), "第几段没复用既有实现：缺 {needle}");
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
        // 缓存：给 None ⇒ doc ingest 用默认 `<out>.cache.json`（重跑 0 token）。
        assert!(
            MODULE_SRC.contains("None, // 缓存落点"),
            "缓存落点必须留给 doc ingest 的默认值（R4）"
        );
    }

    // ── QA 退回项 · 准备性写库必须在两段成功之后 ──────────────────

    /// ⛔ **结构判据**：`get_or_create` / `new_version` 只许出现在 [`ensure_base`] 里，
    /// 且它的**调用点**必须在 ①② 段成功之后、导入之前。
    ///
    /// # 为什么这条必须能红
    ///
    /// QA 真库实测过一版：写库跑在抽取之前 ⇒ agent 连不上时「文件层零产物」成立，
    /// 但库里多了一个**空壳项目 + v1**（FDE 打错一次项目名就多一个废弃物）。
    /// 三种挪法都会让这条红：
    ///
    /// | 挪法 | 哪条断言红 |
    /// | --- | --- |
    /// | `ensure_base(...)` 挪到 `build_artifacts` 之前 | `build < ensure` |
    /// | 把 `get_or_create` 内联回 `run` 开头 | `run` 体里出现写库调用 |
    /// | 把 `new_version(` 内联回 `run` | 同上 |
    ///
    /// （探针用 `concat!` 拼：源码判据扫的是本文件，字面量探针会自指。）
    #[test]
    fn the_preparatory_write_happens_after_both_stages() {
        let run_start = MODULE_SRC
            .find(concat!("pub async fn ", "run("))
            .expect("`run` 不见了");
        let tail = MODULE_SRC[run_start..]
            .find(concat!("#[cfg", "(test)]"))
            .expect("`run` 之后应当紧跟判据模块");
        let body = &MODULE_SRC[run_start..run_start + tail];

        let plan = body
            .find(concat!("plan_base_seq(pool, &o)"))
            .expect("`run` 必须**只读**地先算基版本（空提案要填 base_version）");
        let build = body
            .find(concat!("build_artifacts(&o, ", "base_seq)"))
            .expect("`run` 必须调 ①② 段");
        let ensure = body
            .find(concat!("ensure_base(pool, &o, ", "base_seq)"))
            .expect("`run` 必须调准备性写库（项目 / 版本）");
        let import = body
            .find(concat!("import_proposal_core", "("))
            .expect("`run` 必须调导入");

        assert!(
            plan < build,
            "算基版本必须在抽取之前（否则空提案的 base_version 没法填）"
        );
        assert!(
            build < ensure,
            "⛔ 准备性写库（建项目 / 建版本）必须在 ①② 段**成功之后** —— \
             现在的顺序是写库在前：agent 连不上就会在库里留下空壳项目 + v1（AC-006）"
        );
        assert!(ensure < import, "准备性写库必须在导入之前（import 要求基版本已存在）");

        // 两处写库只许收在 `ensure_base` 里：`run` 自己的函数体不许出现它们。
        for w in [concat!("get_or_", "create("), concat!("new_", "version(")] {
            assert!(
                !body.contains(w),
                "⛔ `run` 的函数体里出现了 `{w}` —— 写库必须收在 `ensure_base` 里，\
                 否则它又会跑在两段之前（QA 实测过的那次空壳项目就是它）"
            );
        }
    }
}

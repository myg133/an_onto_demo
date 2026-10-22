//! **`ontok concept suggest --ai`**（REQ-118 · 决策点三）—— 产品内置的 AI 语义分组。
//!
//! # 它替代的是哪一段对话
//!
//! REQ-115 给了产品一个**算法**版分组（`domain::concept_suggest`，贪心模块度），
//! 但那份 8 组的业务语义分组是在**对话里**做的（`concept-declaration-ai-v2.json` 的
//! `declared_by` 写着 `ai-concept-grouper / qwen36-27B`）。本模块把它变成产品能力：
//! 输入是一版快照（实体 + 关系），输出是一份**可直接 `concept import`** 的
//! [`ConceptDeclaration`]（AC-004），外加 ADR-005 形状的 `proposals[]`（AC-001）。
//!
//! # 两种分组并存，⛔ 不互相冒充（AC-007）
//!
//! | 命令 | `source` | 谁算的 | 进指纹 |
//! | --- | --- | --- | --- |
//! | `concept suggest`（算法版，REQ-115） | **`rule`** | 贪心模块度（整数、无随机数） | 无需（确定性） |
//! | `concept suggest --ai` | **`ai`** | 模型语义判断 | ✅ |
//!
//! ⚠️ REQ-115 时期的 `source` 值叫 `algorithm-suggest`；REQ-118 的 AC-007 明确要求
//! **降级路径标 `rule`**，所以常量值改成了 `rule`。`declared_by` 仍写着算法名
//! （`concept-suggest/greedy-modularity-v1`）—— 那是「谁算的」的**细粒度**出处，
//! 与「来源通道」是两栏。改值不是退步：`source` 是给机器分流的，`declared_by` 是给人追的。
//!
//! # 为什么产物是「声明 + 提案」两份数据装在一个文件里
//!
//! - AC-004 要 `concept import` 直接吃（`version / declared_by / rule / history / groups`，
//!   `digest` 正确）；
//! - AC-001 要每条提案有 `claim / evidence / confidence / reason / decided_by / reviewed_by`。
//!
//! `ConceptGroup` 没有 confidence / reason 这两栏，硬塞进去会让 `content_digest`
//! 与「组内容」搅在一起。所以文件顶层的 `proposals[]` 承载 ADR-005 形状，
//! `groups[]` 承载声明本身；`ConceptDeclaration` 没有 `deny_unknown_fields`，
//! `concept import` 只读它认识的那几栏（多余的 `proposals` / `stats` 原样忽略）。
//!
//! **AC-001 的判据范围（BA 裁定 A-3）**：六字段（claim / evidence / confidence /
//! reason / decided_by / reviewed_by）适用于顶层的 `proposals[]`；
//! `groups[]` 的字段由 **REQ-010 的 [`ConceptGroup`]** 定义，**不叠加**这六栏。
//! ⛔ 不要因为 `groups[]` 里没有 `confidence` 就判 AC-001 不通过 —— 两处看的不是同一个数组。
//!
//! # 已知限制（A-5 · 登记，不是已修）
//!
//! **一次请求装下整个快照**：96 个实体（带描述与前 6 个属性名）实测 7,842 token 输出、
//! 一次调用成功；但 `--ai` 的通道没有分块 ——
//!
//! - **触发条件**：`Σ(实体描述长度) × 成员数` 让模型输出超过 `max_tokens`（默认 4096），
//!   `completion_text` 会判 `finish_reason = "length"` ⇒ **整份回答作废**（不是丢几个组）；
//!   粗略地说，实体数超过 ~150–200 或每个实体描述很长时就会撞上。
//! - **将来修法**：两阶段「分片 + 合并」—— 先按业务域/关系连通分量把实体分片
//!   （每片一次调用，指纹按片算），再用跨片关系把同域组并起来；或先让模型只给
//!   「组名 + 种子成员」，再逐组补齐成员。**本轮不做**，因为分片的合并规则本身就是
//!   另一个决策点（会引入新的语义判断），不该在 REQ-118 里顺带决定。
//!
//! ⛔ 在此之前，大 schema 上 `concept suggest --ai` 可能整份失败；失败时**不产出文件**
//! （`run_batches` 把该批记 `truncated`，预算/产物闸照常拦），不会给出半份分组。
//!
//! # P2：这份声明**仍然没被签**
//!
//! `reviewed_by` / `confirmed_by` 恒 `null`；`source = "ai"`。
//! `concept import` 只是把它变成**提案**，之后仍要双轨审核 + `proposal apply` 才进本体
//! —— 这条链路是 REQ-010 既有的，本模块**不新增任何捷径**。
//!
//! # 校验比算法版更严的三处
//!
//! 1. 成员必须在实体集合里（模型编造的 key 直接剔掉，剔空则整组丢弃）；
//! 2. 一个实体只能属于一个组（重叠整组丢弃 —— `validate_groups` 会判 `OverlappingMembership`）；
//! 3. 组名不许撞客户发布闸（`domain::pipeline::group_name_hits`）——
//!    组名会进 `/api/graph` 与业务总览，撞闸会让**每次**出图都 500，
//!    所以撞闸的组在这里就被丢弃并计数（⛔ 不改写模型给的名字）。

use crate::agent::llm::LlmTransport;
use crate::domain::concept::{
    content_digest, scan_group_names, ConceptDeclaration, ConceptGroup, DeclarationRevision,
};
use crate::domain::snapshot::Snapshot;
use crate::extract::ai_semantic::Prompts;
use crate::extract::proposal::{
    decided_by_ai, finalize_item, parse_proposals, run_batches, sha256_text,
    BatchRequest, Drop, ProposalCache, ProposalHeader, ProposalItem,
    ProposalStats, LOW_CONFIDENCE_THRESHOLD, SOURCE_AI,
};

// ──────────────────────────────────────────────────────────
// 常量
// ──────────────────────────────────────────────────────────

/// 提案逻辑版本 —— **进指纹**。
pub const AGENT_VERSION: &str = "concept-ai-1";

/// 声明版本。**规则不变就固定**：它记录的是「这一份建议」，不是「第几次跑」。
pub const DECLARATION_VERSION: &str = "1.0.0";

/// `source` 的取值（AC-007）：这份声明是**AI 提的**，不是算法算的。
pub const SOURCE_AI_DECLARATION: &str = "ai";

/// 聚类规则的**一句话说明**（进 `rule` → 进 `content_digest`）。
///
/// ⚠️ 改措辞 = 改声明内容，必须升 `DECLARATION_VERSION`（`ConceptDeclaration::verify` 会拒）。
pub const RULE: &str = "AI 语义分组（`concept suggest --ai`）：按客户可感知的业务域把实体归组——同一次业务动作里会被一起提及的对象放一起；组内关系密度只用于校验分得对不对，不做机械社区检测；拿不准的组标 low_confidence，拿不准的实体宁可不分组。";

/// 每个实体最多送几条属性名（给模型一点业务上下文，但不送整份 schema）。
pub const MAX_PROPERTIES_IN_PROMPT: usize = 6;

/// 固定提示词 —— **改动会改变 `prompt_sha256`，从而让所有缓存失效**（P3）。
pub const SYSTEM_PROMPT: &str = r#"你是企业本体建模的业务概念分组器。你的唯一任务：把给定的实体（表）按**客户能感知的业务概念**分组，并给出可核实的依据。

输入是一个 JSON：{"entities": [{"key": "实体 key", "display_name": "展示名", "description": "说明或空串", "properties": ["属性名"]}], "links": [{"from": "实体 key", "to": "实体 key", "count": 关系条数}], "task": "..."}

输出必须是严格 JSON（不要 markdown 代码块、不要任何解释文字）：
{"proposals": [{"subject": "组编号（自取，如 customer_account）", "name": "组名", "members": ["实体 key"], "claim": "一句话说明这组是什么", "evidence": ["可核实的依据"], "confidence": 0.0到1.0, "reason": "一句话说明为什么这几个是一组"}]}

分组规则：
1. 组数通常在 3–10 之间；**只有 ≥2 个成员的组**。一个成员的「组」不是归并，不要输出。
2. members 只能是输入 entities 里的 key，**逐字一致**；⛔ 不许编造 key。
3. **一个实体只能出现在一个组里**。拿不准归哪组的实体，宁可不分组，也不要硬塞。
4. name 是 2–12 个简体中文字，业务人员日常说法（「客户与子账号」「用量、扣费与余额」）。
   ⛔ 不许用建模术语（主键、外键、字段、实体、实例、记录表、主数据、基数、维度表）。
5. claim 一句话说明这组是什么业务；reason 说明**为什么这几个对象**是同一个业务概念。
6. evidence 给 1–3 条，**每一条都要引用输入里真实存在的实体 key 或关系（from → to）**，
   例如「customers 与 customer_account_binding 有 belongs_to 关系」。
   ⛔ 不许写「模型认为」「推测」「按经验」这类不可核实的话。
7. confidence 是 0–1：证据充分给 0.8 以上；只靠名字相似给 0.6 以下。
8. 关系很少时（整个图几乎是孤点），按业务域语义分组并在 reason 里说明依据是实体名/属性名。
"#;

/// 每条请求里固定的 task 文案（进 user 消息，不进模板哈希）。
const TASK_TEXT: &str = "把这些实体按业务概念分组，成员必须来自输入的 key，按协议返回严格 JSON。";

// ──────────────────────────────────────────────────────────
// 输入
// ──────────────────────────────────────────────────────────

/// 送进提示词的实体切片。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct EntitySlice {
    pub key: String,
    pub display_name: String,
    pub description: String,
    pub properties: Vec<String>,
}

/// 分组输入 = 实体 + 关系（两端 key）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupInput {
    pub entities: Vec<EntitySlice>,
    pub links: Vec<(String, String)>,
}

impl GroupInput {
    /// 从快照摊平：**只搬运，不判断**。
    ///
    /// 实体去重（同 key 只留第一条），按 key 排序；关系保留两端（悬空端在计算
    /// 提示词时剔掉 —— 悬空关系对分组没有信息量）。
    pub fn from_snapshot(snap: &Snapshot) -> Self {
        let mut entities: Vec<EntitySlice> = Vec::new();
        for e in &snap.entity_types {
            if entities.iter().any(|x| x.key == e.key) {
                continue;
            }
            let mut properties: Vec<String> = e
                .properties
                .iter()
                .map(|p| {
                    let n = p.name.trim();
                    if n.is_empty() {
                        p.column.clone()
                    } else {
                        n.to_string()
                    }
                })
                .filter(|s| !s.trim().is_empty())
                .take(MAX_PROPERTIES_IN_PROMPT)
                .collect();
            properties.sort();
            properties.dedup();
            entities.push(EntitySlice {
                key: e.key.clone(),
                display_name: e.display_name.trim().to_string(),
                description: e.description.as_deref().unwrap_or("").trim().to_string(),
                properties,
            });
        }
        entities.sort_by(|a, b| a.key.cmp(&b.key));
        let mut links: Vec<(String, String)> = snap
            .link_types
            .iter()
            .map(|l| (l.from_type.clone(), l.to_type.clone()))
            .collect();
        links.sort();
        Self { entities, links }
    }

    /// 已知实体 key 集合（校验用）。
    pub fn known(&self) -> std::collections::BTreeSet<&str> {
        self.entities.iter().map(|e| e.key.as_str()).collect()
    }

    /// 输入的规范化指纹（进文件头，也是指纹的一部分）。
    pub fn digest(&self) -> String {
        let mut entities: Vec<&EntitySlice> = self.entities.iter().collect();
        entities.sort_by(|a, b| a.key.cmp(&b.key));
        let mut links: Vec<&(String, String)> = self.links.iter().collect();
        links.sort();
        let canon = serde_json::json!({
            "entities": entities,
            "links": links.iter().map(|(a, b)| serde_json::json!([a, b])).collect::<Vec<_>>(),
        });
        sha256_text(&canon.to_string())
    }

    /// 构造唯一一次请求。
    pub fn build_request(&self) -> anyhow::Result<BatchRequest> {
        let known = self.known();
        let mut counts: std::collections::BTreeMap<(&str, &str), usize> =
            std::collections::BTreeMap::new();
        for (a, b) in &self.links {
            if !known.contains(a.as_str()) || !known.contains(b.as_str()) {
                continue; // 悬空关系：对分组没有信息量
            }
            *counts.entry((a.as_str(), b.as_str())).or_insert(0) += 1;
        }
        let body = serde_json::json!({
            "entities": self.entities,
            "links": counts
                .iter()
                .map(|((a, b), n)| serde_json::json!({"from": a, "to": b, "count": n}))
                .collect::<Vec<_>>(),
            "task": TASK_TEXT,
        });
        let user_message = serde_json::to_string(&body)?;
        Ok(BatchRequest {
            key: "snapshot".to_string(),
            item_count: self.entities.len(),
            input_digest: sha256_text(&user_message),
            user_message,
        })
    }
}

// ──────────────────────────────────────────────────────────
// 解析 + 校验
// ──────────────────────────────────────────────────────────

/// 一个通过校验的组（**声明视图**：key 已分配、成员已排序）。
#[derive(Debug, Clone, PartialEq)]
pub struct GroupProposal {
    pub key: String,
    pub display_name: String,
    pub members: Vec<String>,
    pub item: ProposalItem,
}

/// 校验模型回答：丢弃 + 计数，绝不修正模型输出。
///
/// 顺序是刻意的：**先过滤成员 → 再判重叠 → 再分配 key → 最后过客户闸**。
/// 换顺序会让计数口径说不清（例如「重叠」与「成员编造」同时成立时算哪一类）。
pub fn validate(
    body: &str,
    input: &GroupInput,
    decided_by: &str,
    stats: &mut ProposalStats,
) -> std::result::Result<Vec<GroupProposal>, String> {
    let parsed = parse_proposals(body)?;
    for m in &parsed.malformed {
        stats.reject(Drop::ResponseParse, m.clone());
    }
    let known = input.known();
    let mut candidates: Vec<(String, Vec<String>, ProposalItem)> = Vec::new();
    for raw in parsed.proposals {
        let name = raw.name.as_deref().map(str::trim).unwrap_or("").to_string();
        if name.is_empty() {
            stats.reject(Drop::MissingName, raw.subject.clone());
            continue;
        }
        let mut members: Vec<String> = Vec::new();
        let mut unknown = 0usize;
        for m in &raw.members {
            let m = m.trim();
            if m.is_empty() {
                continue;
            }
            if !known.contains(m) {
                unknown += 1;
                continue;
            }
            if !members.iter().any(|x| x == m) {
                members.push(m.to_string());
            }
        }
        if unknown > 0 {
            // 编造的成员剔掉，但**计数**：静默剔除会让「模型给了 8 个成员、实际 5 个」
            // 变成一件没人知道的事。
            stats.reject(
                Drop::UnknownSubject,
                format!("组 {name:?} 有 {unknown} 个成员不在实体集合里"),
            );
        }
        members.sort();
        if members.is_empty() {
            stats.reject(Drop::NoMembers, name);
            continue;
        }
        let item = match finalize_item(&raw, decided_by) {
            Ok(i) => i,
            Err(d) => {
                stats.reject(d, name);
                continue;
            }
        };
        // 组证据的落点只有两种：组名，或某个成员 key。
        let lower_evidence: Vec<String> = item.evidence.iter().map(|e| e.to_lowercase()).collect();
        let name_lower = name.to_lowercase();
        let anchored = lower_evidence.iter().any(|e| e.contains(&name_lower))
            || members
                .iter()
                .any(|m| lower_evidence.iter().any(|e| e.contains(&m.to_lowercase())));
        if !anchored {
            stats.reject(
                Drop::EvidenceNotFromInput,
                format!("组 {name:?}：{:?}", item.evidence),
            );
            continue;
        }
        candidates.push((name, members, item));
    }

    // 稳定顺序：按 (组名, 成员) 排序后再分配 key —— 两次调用（含缓存回放）字节一致。
    candidates.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

    let mut accepted: Vec<GroupProposal> = Vec::new();
    let mut home: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    let mut used_keys: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let entity_keys = input.known();
    for (name, members, mut item) in candidates {
        if let Some(hit) = members.iter().find(|m| home.contains_key(m.as_str())) {
            stats.reject(
                Drop::Overlap,
                format!(
                    "组 {name:?} 的成员 {hit:?} 已在组 {:?} 里",
                    home.get(hit.as_str())
                ),
            );
            continue;
        }
        let key = fresh_group_key(&entity_keys, &mut used_keys);
        item.subject = key.clone();
        for m in &members {
            home.insert(m.clone(), key.clone());
        }
        accepted.push(GroupProposal {
            key,
            display_name: name,
            members,
            item,
        });
    }

    // 客户发布闸：组名/组 key 会进 `/api/graph` 与业务总览。
    let cgroups: Vec<ConceptGroup> = accepted.iter().map(to_concept_group).collect();
    let hits = scan_group_names(&cgroups);
    if !hits.is_empty() {
        let bad: std::collections::BTreeSet<String> = hits.iter().map(|h| h.group.clone()).collect();
        for h in &hits {
            stats.reject(Drop::CustomerGate, format!("{h}"));
        }
        accepted.retain(|g| !bad.contains(&g.key));
    }

    accepted.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(accepted)
}

/// 分配一个不与实体 key 冲突的组 key（`concept_01`、`concept_02` …）。
fn fresh_group_key(
    entity_keys: &std::collections::BTreeSet<&str>,
    used: &mut std::collections::BTreeSet<String>,
) -> String {
    let mut n = used.len() + 1;
    loop {
        let cand = format!("concept_{n:02}");
        n += 1;
        if entity_keys.contains(cand.as_str()) || !used.insert(cand.clone()) {
            continue;
        }
        return cand;
    }
}

/// 组 → `ConceptGroup`（声明里的那一形状）。
///
/// `scope_hint` **不写**：它是**客户可见**的组说明（业务总览的 `summary`），
/// 而「AI 提的 / 待人审」是内部备注 —— 属于 `declared_by` / `rule` / `proposals[]`
/// （与 REQ-115 的 C-2 同一条裁定）。
fn to_concept_group(g: &GroupProposal) -> ConceptGroup {
    ConceptGroup {
        key: g.key.clone(),
        display_name: g.display_name.clone(),
        members: g.members.clone(),
        scope_hint: None,
        low_confidence: Some(g.item.confidence < LOW_CONFIDENCE_THRESHOLD),
        extracted_by: None,
        evidence: vec![],
    }
}

/// 跑唯一一次请求（缓存 / 预算 / 记账走三命令共用的 [`run_batches`]）。
pub fn suggest_all(
    transport: &mut dyn LlmTransport,
    cache: &mut dyn ProposalCache,
    input: &GroupInput,
    model: &str,
    prompts: &Prompts,
    budget: u64,
    max_tokens: u32,
) -> anyhow::Result<(Vec<GroupProposal>, ProposalStats)> {
    let request = input.build_request()?;
    let decided_by = decided_by_ai(model);
    let (mut groups, mut stats) = run_batches(
        transport,
        cache,
        std::slice::from_ref(&request),
        model,
        prompts,
        AGENT_VERSION,
        budget,
        max_tokens,
        |_i, body, stats| validate(body, input, &decided_by, stats),
    );
    groups.sort_by(|a, b| a.key.cmp(&b.key));
    stats.proposed = groups.len();
    stats.low_confidence = groups
        .iter()
        .filter(|g| g.item.is_low_confidence())
        .count();
    Ok((groups, stats))
}

// ──────────────────────────────────────────────────────────
// 声明 + 渲染
// ──────────────────────────────────────────────────────────

/// 把通过的组组装成一份**可直接 `concept import`** 的声明（AC-004）。
///
/// `declared_by = ai:<model>`：声明的**作者**是可追的（P4）。
/// `reviewed_by` / `confirmed_by` 恒 `None`：提案永不权威（P2）。
pub fn build_declaration(groups: &[GroupProposal], model: &str) -> ConceptDeclaration {
    let cgroups: Vec<ConceptGroup> = groups.iter().map(to_concept_group).collect();
    let declared_by = decided_by_ai(model);
    let digest = content_digest(&cgroups, RULE, &declared_by);
    ConceptDeclaration {
        version: DECLARATION_VERSION.to_string(),
        declared_by,
        rule: RULE.to_string(),
        confirmed_by: None,
        source: Some(SOURCE_AI_DECLARATION.to_string()),
        reviewed_by: None,
        history: vec![DeclarationRevision {
            version: DECLARATION_VERSION.to_string(),
            content_digest: digest,
            note: "由 concept suggest --ai 语义分组生成（未审核，不是结论）".to_string(),
        }],
        groups: cgroups,
    }
}

/// 提案文件头。
pub fn header(model: &str, prompt_sha: &str, input_sha256: String) -> ProposalHeader {
    ProposalHeader {
        kind: "concept",
        source: SOURCE_AI,
        model: Some(model.to_string()),
        prompt_sha: Some(prompt_sha.to_string()),
        agent_version: AGENT_VERSION.to_string(),
        input_sha256,
        decided_by: decided_by_ai(model),
    }
}

/// 渲染：**声明字段 + ADR-005 提案字段**装进同一个 JSON 对象。
///
/// 落盘前自检三件事（任何一条不过 ⇒ 报错、不写文件）：
/// 1. 声明契约 `verify()`（改内容必须改版本 / digest 正确）；
/// 2. 组字段过客户发布闸（`scan_group_names` 为空）；
/// 3. 每组至少一个成员（`validate_groups` 的 `EmptyGroup` 不可能发生 —— 这里仍断言）。
pub fn render_proposal(
    decl: &ConceptDeclaration,
    groups: &[GroupProposal],
    header: &ProposalHeader,
    stats: &ProposalStats,
) -> anyhow::Result<String> {
    decl.verify()
        .map_err(|e| anyhow::anyhow!("内部错误：生成的声明过不了版本契约 —— **未写出**：{e}"))?;
    let hits = scan_group_names(&decl.groups);
    if !hits.is_empty() {
        anyhow::bail!(
            "内部错误：生成的声明有组字段撞客户发布闸 —— **未写出**：{}",
            hits.iter().map(|h| h.to_string()).collect::<Vec<_>>().join("；")
        );
    }
    let items: Vec<ProposalItem> = groups.iter().map(|g| g.item.clone()).collect();
    let mut doc = match serde_json::to_value(decl)? {
        serde_json::Value::Object(m) => m,
        _ => anyhow::bail!("内部错误：ConceptDeclaration 不是 JSON 对象"),
    };
    // 公共头部（与 `proposal::render_document` 的字段同名同义；这里直接拼进声明对象，
    // 因为 `render_document` 会另起一个 `proposals` 数组，而声明本身就是顶层对象）。
    doc.insert(
        "proposal_format".to_string(),
        serde_json::Value::from(crate::extract::proposal::PROPOSAL_FORMAT),
    );
    doc.insert("kind".to_string(), serde_json::Value::from(header.kind));
    doc.insert(
        "model".to_string(),
        match &header.model {
            Some(m) => serde_json::Value::from(m.clone()),
            None => serde_json::Value::Null,
        },
    );
    doc.insert(
        "prompt_sha".to_string(),
        match &header.prompt_sha {
            Some(s) => serde_json::Value::from(s.clone()),
            None => serde_json::Value::Null,
        },
    );
    doc.insert(
        "agent_version".to_string(),
        serde_json::Value::from(header.agent_version.clone()),
    );
    doc.insert(
        "input_sha256".to_string(),
        serde_json::Value::from(header.input_sha256.clone()),
    );
    doc.insert(
        "decided_by".to_string(),
        serde_json::Value::from(header.decided_by.clone()),
    );
    doc.insert(
        "low_confidence_threshold".to_string(),
        serde_json::json!(LOW_CONFIDENCE_THRESHOLD),
    );
    doc.insert("proposals".to_string(), serde_json::to_value(items)?);
    doc.insert("stats".to_string(), stats.content_json());
    let mut text = serde_json::to_string_pretty(&serde_json::Value::Object(doc))?;
    text.push('\n');
    Ok(text)
}

/// 便于测试/报告：组成员表（`组名 → 成员`）。
pub fn member_map(groups: &[GroupProposal]) -> std::collections::BTreeMap<String, Vec<String>> {
    groups
        .iter()
        .map(|g| (g.display_name.clone(), g.members.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::llm::protocol::{Choice, Usage};
    use crate::agent::llm::{ChatResponse, ScriptedTransport};
    use crate::extract::proposal::MemoryProposalCache;

    fn input() -> GroupInput {
        GroupInput {
            entities: vec![
                EntitySlice {
                    key: "customers".into(),
                    display_name: "客户".into(),
                    description: "客户主档".into(),
                    properties: vec!["客户标识".into()],
                },
                EntitySlice {
                    key: "customer_account_binding".into(),
                    display_name: "账号绑定".into(),
                    description: String::new(),
                    properties: vec![],
                },
                EntitySlice {
                    key: "usage_details".into(),
                    display_name: "用量明细".into(),
                    description: String::new(),
                    properties: vec![],
                },
                EntitySlice {
                    key: "deductions".into(),
                    display_name: "扣费明细".into(),
                    description: String::new(),
                    properties: vec![],
                },
                EntitySlice {
                    key: "lonely".into(),
                    display_name: "孤点".into(),
                    description: String::new(),
                    properties: vec![],
                },
            ],
            links: vec![
                ("customers".into(), "customer_account_binding".into()),
                ("usage_details".into(), "deductions".into()),
                ("deductions".into(), "ghost".into()),
            ],
        }
    }

    fn reply(content: &str, tokens: u64) -> ChatResponse {
        ChatResponse {
            choices: vec![Choice {
                message: crate::agent::llm::protocol::Message_ {
                    content: Some(content.to_string()),
                    reasoning_content: None,
                },
                finish_reason: Some("stop".to_string()),
            }],
            usage: Some(Usage {
                prompt_tokens: tokens.saturating_sub(1),
                completion_tokens: 1,
                total_tokens: Some(tokens),
                completion_tokens_details: None,
            }),
        }
    }

    fn answer() -> String {
        serde_json::json!({"proposals": [
            {"subject": "customer_account", "name": "客户与子账号",
             "members": ["customers", "customer_account_binding"],
             "claim": "客户主档与账号绑定同属「客户是谁」",
             "evidence": ["customers → customer_account_binding 有 belongs_to 关系"],
             "confidence": 0.9, "reason": "两者在关系图里直接相连，业务上一个是客户、一个是它的账号归属"},
            {"subject": "billing", "name": "用量、扣费与余额",
             "members": ["usage_details", "deductions"],
             "claim": "用量明细与扣费明细是同一笔业务动作的两端",
             "evidence": ["usage_details → deductions 有 belongs_to 关系"],
             "confidence": 0.72, "reason": "用量产生扣费，两者在关系图里直接相连"}
        ]})
        .to_string()
    }

    #[test]
    fn validate_assigns_stable_keys_and_ignores_singletons() {
        let mut stats = ProposalStats::default();
        let groups = validate(&answer(), &input(), "ai:m", &mut stats).unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].key, "concept_01");
        assert_eq!(groups[1].key, "concept_02");
        assert_eq!(groups[0].members, vec!["customer_account_binding", "customers"], "成员排序");
        assert!(groups.iter().all(|g| g.members.len() >= 2));
        assert_eq!(stats.rejected_total(), 0);
        // `lonely` 没有被塞进任何组（模型没给）
        let all: Vec<&String> = groups.iter().flat_map(|g| g.members.iter()).collect();
        assert!(!all.iter().any(|m| m.as_str() == "lonely"));
    }

    #[test]
    fn unknown_members_are_removed_and_counted_overlap_is_rejected() {
        let mut stats = ProposalStats::default();
        let body = serde_json::json!({"proposals": [
            {"subject": "a", "name": "组甲", "members": ["customers", "ghost1", "ghost2"],
             "claim": "x", "evidence": ["customers 与 ghost1 有关系"],
             "confidence": 0.9, "reason": "编造了两个成员但还有一个真的"},
            {"subject": "b", "name": "组乙", "members": ["customers", "deductions"],
             "claim": "x", "evidence": ["customers 与 deductions 有关系"],
             "confidence": 0.9, "reason": "与组甲重叠"}
        ]})
        .to_string();
        let groups = validate(&body, &input(), "ai:m", &mut stats).unwrap();
        assert_eq!(groups.len(), 1, "重叠的组整组丢弃");
        // 排序按组名字节序：乙(U+4E59) < 甲(U+7532)，所以先接受「组乙」的两个成员，
        // 后到的「组甲」与它重叠 ⇒ 整组丢弃（与迭代顺序无关：谁先谁后都只留一组）。
        assert_eq!(groups[0].members, vec!["customers", "deductions"]);
        assert_eq!(stats.rejected.get("unknown_subject").copied(), Some(1), "两个编造成员计一次");
        assert_eq!(stats.rejected.get("overlap").copied(), Some(1));
    }

    #[test]
    fn group_name_hitting_customer_gate_is_dropped() {
        let mut stats = ProposalStats::default();
        let body = serde_json::json!({"proposals": [
            {"subject": "a", "name": "主键与字段", "members": ["customers", "deductions"],
             "claim": "x", "evidence": ["customers 与 deductions 有关系"],
             "confidence": 0.9, "reason": "组名撞了客户发布闸的建模术语"}
        ]})
        .to_string();
        let groups = validate(&body, &input(), "ai:m", &mut stats).unwrap();
        assert!(groups.is_empty());
        assert_eq!(stats.rejected.get("customer_gate").copied(), Some(1));
    }

    #[test]
    fn declaration_is_import_ready_with_correct_digest() {
        let mut stats = ProposalStats::default();
        let groups = validate(&answer(), &input(), "ai:m", &mut stats).unwrap();
        let decl = build_declaration(&groups, "m");
        assert_eq!(decl.verify(), Ok(()), "AC-004：digest 必须自洽");
        assert_eq!(decl.source.as_deref(), Some("ai"), "AC-007");
        assert_eq!(decl.reviewed_by, None, "P2");
        assert_eq!(decl.confirmed_by, None);
        assert_eq!(decl.declared_by, "ai:m");
        assert!(decl.rule.contains("AI 语义分组"));
        assert_eq!(decl.groups.len(), 2);
        assert!(decl.groups.iter().all(|g| g.scope_hint.is_none()), "C-2：不写客户可见说明");
    }

    #[test]
    fn rendered_file_is_still_a_valid_declaration_and_has_proposals() {
        let mut stats = ProposalStats::default();
        let groups = validate(&answer(), &input(), "ai:m", &mut stats).unwrap();
        stats.proposed = groups.len();
        let decl = build_declaration(&groups, "m");
        let prompts = Prompts::from_system(SYSTEM_PROMPT);
        let h = header("m", &prompts.sha256, input().digest());
        let text = render_proposal(&decl, &groups, &h, &stats).unwrap();
        // AC-004：带 `proposals` 额外字段仍能被 concept import 解析并验签
        let parsed = ConceptDeclaration::parse(&text).expect("必须能解析");
        assert_eq!(parsed.verify(), Ok(()), "带 proposals 后 digest 仍一致");
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["source"], "ai");
        assert_eq!(v["reviewed_by"], serde_json::Value::Null);
        assert_eq!(v["proposals"].as_array().unwrap().len(), 2);
        for p in v["proposals"].as_array().unwrap() {
            for k in ["claim", "evidence", "confidence", "reason", "decided_by", "reviewed_by"] {
                assert!(p.get(k).is_some(), "AC-001：每条都要有 {k}");
            }
            assert_eq!(p["reviewed_by"], serde_json::Value::Null);
        }
    }

    #[test]
    fn cache_makes_second_run_free_and_identical() {
        let prompts = Prompts::from_system(SYSTEM_PROMPT);
        let mut transport = ScriptedTransport::new(vec![Ok(reply(&answer(), 700))]);
        let mut cache = MemoryProposalCache::new();
        let input = input();
        let (g1, s1) = suggest_all(&mut transport, &mut cache, &input, "m", &prompts, 10_000, 300)
            .unwrap();
        let decl1 = build_declaration(&g1, "m");
        let h = header("m", &prompts.sha256, input.digest());
        let a = render_proposal(&decl1, &g1, &h, &s1).unwrap();
        let (g2, s2) = suggest_all(&mut transport, &mut cache, &input, "m", &prompts, 10_000, 300)
            .unwrap();
        let decl2 = build_declaration(&g2, "m");
        let b = render_proposal(&decl2, &g2, &h, &s2).unwrap();
        assert_eq!(s2.tokens_spent, 0);
        assert_eq!(s2.calls, 0);
        assert_eq!(a, b, "第二次逐字节相同");
    }

    #[test]
    fn input_digest_ignores_order_but_tracks_content() {
        let a = input();
        let mut b = input();
        b.entities.reverse();
        b.links.reverse();
        assert_eq!(a.digest(), b.digest(), "同一份输入换个顺序 ⇒ 同一指纹");
        let mut c = input();
        c.entities[0].display_name = "改了".into();
        assert_ne!(a.digest(), c.digest());
    }
}





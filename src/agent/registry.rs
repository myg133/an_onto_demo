//! 抽取 agent 的**登记信息**（纯数据，零 I/O）。
//!
//! 规格 §4.1 给 agent-registry 定的职责：
//!
//! > 注册抽取 agent（自研的与外部的）、锁定版本、记录每轮迭代
//! > **版本锁定就是它的硬职责**（详见 §8.4 `extracted_by`）
//!
//! 形状**照抄 `extract::names::TableNames`**（REQ-003 的既有模式：
//! 「LLM 离线产出物 + agent/version 标记 + `load()` 校验」）——
//! 那是本项目里「一份 agent 产出的知识资产」的既有形态，
//! registry 记录与它是同一类东西，不该另发明一套。

use anyhow::Context;
use serde::{Deserialize, Serialize};

/// 一个登记在册的抽取 agent。
///
/// **「协议」「端点」是数据，不是代码**（本任务的范围边界）：
/// 规格 §7.1 的协议选型还没拍板（ACP 已于 2025-08 并入 A2A，
/// 而实际要接的 mcode ACP 模式与该说法矛盾），所以 `protocol` / `endpoint`
/// **一律存成字符串**。将来选了协议、加了 adapter，这个字段填什么值都行，
/// **不需要改这里的类型，也不需要动 trait**。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRegistration {
    /// 登记名（`kh-core` / `vendor-extractor` …）。与版本合成注册键。
    pub agent: String,
    /// **锁定的具体版本号**，不是「最新版」（§8.4 硬要求）。
    pub version: String,
    /// 协议标识。`inproc` = 进程内规则实现；将来填 `a2a` / `mcp` / …
    pub protocol: String,
    /// 端点。进程内实现为 `None`。
    #[serde(default)]
    pub endpoint: Option<String>,
    /// 能力标签 —— 存数据，供编排按能力路由，不进 `if` 分支。
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// 登记时刻（ISO-8601）。
    pub registered_at: String,
}

impl AgentRegistration {
    /// 载入并做基本校验（照 `TableNames::load` 的 fail-closed 纪律）。
    ///
    /// 校验点：登记名非空、版本非空、协议非空，且**版本不许是「最新版」这类
    /// 模糊说法** —— §8.4 说死了「具体版本号，不是『最新版』」，
    /// 那是 diff 分不清「本体演进」还是「agent 换人」的唯一原因。
    pub fn load(json: &str) -> anyhow::Result<Self> {
        let r: Self = serde_json::from_str(json).context("agent 登记 JSON 解析失败")?;
        r.validate()?;
        Ok(r)
    }

    /// 校验。空串与「latest」之类的模糊版本一律拒绝。
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.agent.trim().is_empty() {
            anyhow::bail!("agent 登记名为空");
        }
        if self.version.trim().is_empty() {
            anyhow::bail!(
                "agent {} 的版本为空 —— §8.4 要求记具体版本，不能留空",
                self.agent
            );
        }
        if is_vague_version(&self.version) {
            anyhow::bail!(
                "agent {} 的版本 {:?} 是模糊说法 —— §8.4 要求具体版本号，\
                 否则 diff 变了分不清是「本体演进」还是「agent 换人」",
                self.agent,
                self.version
            );
        }
        if self.protocol.trim().is_empty() {
            anyhow::bail!("agent {} 未登记协议", self.agent);
        }
        Ok(())
    }
}

/// 「最新版」这类**不算具体版本**的写法。登记时直接拒。
///
/// 刻意列全几种常见写法而不是只判 `"latest"`：
/// 漏一种就等于给「记了个假版本号」留了个门，而假版本号会让 §8.4 的
/// 收敛判定与 diff 归因**整体失效** —— 这是静默的坏处，不是报错。
pub fn is_vague_version(v: &str) -> bool {
    let s = v.trim().to_ascii_lowercase();
    matches!(
        s.as_str(),
        "latest" | "newest" | "current" | "head" | "main" | "master" | "dev" | "nightly"
    )
}

/// 一次抽取的**身份** —— agent + 锁定版本 + 轮次。
///
/// 这正是 [`crate::extract::propose::ExtractedBy`] 的三个字段。
/// 存在的意义是把「谁抽的」这件事**从 CLI 默认值里搬到 registry 登记**：
/// 现状是 `agent` 取自 `--agent`（默认 `"mcode-agent"`）、
/// `version` 取自 `--agent-version`（默认 `"unknown"`）、
/// `round` 硬编码 1 —— 三个都是「没查到就编一个」。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentIdentity {
    pub agent: String,
    /// 锁定版本。
    pub version: String,
    /// 第几轮抽取（§8.4）。
    pub round: i32,
}

impl AgentIdentity {
    /// 从一条登记记录构造身份。轮次由调用方给定（轮次是**运行**的属性，
    /// 不是 agent 的属性 —— 同一个 agent 会跑很多轮）。
    pub fn from_registration(reg: &AgentRegistration, round: i32) -> Self {
        Self {
            agent: reg.agent.clone(),
            version: reg.version.clone(),
            round: round.max(1),
        }
    }

    /// **既有 CLI 默认路径专用的构造**，只为保持逐字节兼容。
    ///
    /// ⚠️ 这不是「兜底」，是**显式的兼容层**：`ontok-propose` 不带
    /// `--agent-registry` 跑时走这里，`round` 恒为 1、`version` 恒为
    /// 调用方传入值（CLI 默认 `"unknown"`）—— 与 REQ-013 之前一模一样。
    /// 走 registry 的路径请用 [`AgentIdentity::from_registration`]。
    pub fn legacy_cli(agent: &str, version: &str) -> Self {
        Self {
            agent: agent.to_string(),
            version: version.to_string(),
            round: 1,
        }
    }
}

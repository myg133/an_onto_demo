//! `agent-registry`（规格 §4.1）—— **只服务建侧**。
//!
//! 规格原文：
//!
//! > `agent-registry` **只服务建侧**，图上没有出边是刻意的。它承载 FDE 反复调整
//! > 抽取方法的闭环：**注册抽取 agent（自研的与外部的）、锁定版本、记录每轮迭代**。
//! > **读本体的外部 agent 不经过它** —— 那些 agent 只经 `mcp-server` 进来消费本体。
//! > 两者名字里都有 agent，但一个是造本体的、一个是读本体的
//!
//! **本模块只写「造本体」那一侧。** 读侧不经过这里，所以本文件里
//! 没有一条查询是「给外部 agent 读本体」的。
//!
//! # 版本锁定是硬职责（§8.4）
//!
//! 「一轮抽取一旦开始，agent 与其版本在整个抽取会话内锁定」——
//! 所以 [`PgAgentRegistry::register`] 对已登记的 `(agent, version)`
//! **不允许改写**：同键重复登记会拿新内容去比对，内容不一致直接报错。
//! 这不是幂等友好的设计，是**故意的** —— 改写等于偷偷换版本，
//! 而 diff 分不清「本体演进」还是「agent 换人」正是 §8.4 要防的事故。
//!
//! # 共享本体库**只允许 INSERT**
//!
//! 本表是**审计资产**：谁在什么时候登记了哪个 agent 的哪个版本。
//! 本文件**没有 DELETE / TRUNCATE / UPDATE 语句**，
//! `tests/test_extraction_agent.rs::test_registry_source_has_no_destructive_sql`
//! 用 `include_str!` 把这条纪律钉成可执行断言。

use sqlx::postgres::PgPool;

use crate::agent::registry::{AgentIdentity, AgentRegistration};

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("数据库错误：{0}")]
    Db(#[from] sqlx::Error),

    /// **fail-closed**（照 `config::require_env` / `introspect::ensure_read_only` 的纪律）：
    /// 查不到就报错，**绝不**静默返回一个「看起来像」的默认值。
    #[error("agent-registry 里没有 {agent} 的登记 —— 不许回落默认值，抽不出来就说抽不出来")]
    AgentNotRegistered { agent: String },

    #[error("agent-registry 里没有 {agent} {version} 的登记")]
    VersionNotRegistered { agent: String, version: String },

    /// 版本锁定被绕过 —— 同键重登记但内容不同。
    ///
    /// 只带 `agent` / `version` / `detail` 三项，不把两边各三个字段都搬进来：
    /// 那样 `Err` 变体会大到超过 `result_large_err` 的阈值，
    /// 而这个错误**只在被人读到时才需要细节**。
    #[error("agent {agent} {version} 已登记且版本已锁定，不许改写：{detail}。要换实现请登记**新版本号**")]
    VersionLocked { agent: String, version: String, detail: String },

    /// **撞上唯一约束，却读不回那一行**（REQ-025 的 fail-closed 兜底）。
    ///
    /// `register` 的并发路径靠 `ON CONFLICT (agent, version) DO NOTHING`
    /// 判同键：冲突了说明同键已存在，读回来比对内容。
    /// 而「读不回来」在本库纪律下**不可解释** —— 本模块无破坏性 SQL，
    /// 登记是审计资产，行不会凭空消失。所以这不是「当作成功」的理由，
    /// 按 fail-closed（`config::require_env` / `introspect::ensure_read_only` 同一纪律）
    /// 报错，**不猜**它到底是消失还是看不见。
    #[error("agent {agent} {version} 撞上唯一约束冲突，却读不回那一行（本模块无破坏性 SQL，登记不会消失）—— 不可解释的并发状态，拒绝记账而不是当成成功")]
    ConcurrentRowVanished { agent: String, version: String },

    #[error("登记不合法：{0}")]
    Invalid(String),

    #[error("轮次必须是 ≥1 的整数（规格 §8.4 的 round 是「第几轮抽取」）")]
    BadRound(i32),
}

/// 建表 DDL（幂等）—— 由 [`crate::db::pg::migrate`] 执行。
///
/// 与 `SCHEMA` 分开一个常量而不是拼进去：`SCHEMA` 是本 MVP 首批的 9 张表，
/// REQ-013 是**后加**的。
///
/// ⚠️ **REQ-025：本常量是纯 DDL，不再自带 `BEGIN` / 咨询锁 / `COMMIT`。**
///
/// 建表为什么需要锁、锁键是什么、为什么用交易级而不是会话级，
/// 现在**只写在** [`crate::db::pg`] 一处 —— 那把锁的覆盖面已经是
/// 「`migrate()` 的**全部** DDL」（9 张基表 + 2 条 ALTER + 这两张表），
/// 所以锁必须与所有 DDL 待在**同一个 batch**：
/// 锁是交易级（`pg_advisory_xact_lock`），拆成两条 batch 就是两个事务、
/// 两把各自立即释放的锁 = 锁根本没起作用。
/// 要单看这两张表的 DDL（本文件的职责），这里就是全部。
pub const SCHEMA_AGENT_REGISTRY: &str = r#"
-- REQ-013 · agent-registry（规格 §4.1 / §8.4）
-- 只服务建侧：注册抽取 agent、锁定版本。没有出边到本体图 —— 刻意的。
CREATE TABLE IF NOT EXISTS extraction_agents (
  id            UUID PRIMARY KEY,
  agent         TEXT NOT NULL,
  version       TEXT NOT NULL,                -- 锁定的具体版本，不是「最新版」
  protocol      TEXT NOT NULL DEFAULT 'inproc',  -- 协议是**数据**，不进代码
  endpoint      TEXT,                         -- 进程内实现为 NULL
  capabilities  JSONB NOT NULL DEFAULT '[]',  -- 能力标签，按能力路由用
  registered_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (agent, version)
);

-- 每轮迭代的记录（§4.1「记录每轮迭代」）。
-- 与 extraction_agents 分表：agent 是「是什么」，轮次是「跑了几次」，
-- 塞进同一行就得每次抽取都改 agent 行 —— 那会毁掉版本锁定的审计价值。
CREATE TABLE IF NOT EXISTS extraction_rounds (
  id          UUID PRIMARY KEY,
  agent       TEXT NOT NULL,
  version     TEXT NOT NULL,
  round_no    INT  NOT NULL,                 -- ≥ 1
  source      TEXT,                          -- 勘查留痕
  started_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (agent, version, round_no)
);
"#;

/// 一行登记记录 —— 起个别名，别把六元组写在两处。
type RegRow = (
    String,
    String,
    String,
    Option<String>,
    serde_json::Value,
    chrono::DateTime<chrono::Utc>,
);

/// 共享本体库上的抽取 agent 注册表。
pub struct PgAgentRegistry<'a> {
    pool: &'a PgPool,
}

impl<'a> PgAgentRegistry<'a> {
    pub fn new(pool: &'a PgPool) -> Self {
        Self { pool }
    }

    /// **注册**一个抽取 agent（自研的与外部的一视同仁 —— `protocol` 只是数据）。
    ///
    /// **版本锁定**：`(agent, version)` 已登记时**不改写**。
    /// 内容一致 → 当作幂等重放，返回 `Ok(false)`；
    /// 内容不一致 → [`RegistryError::VersionLocked`]，要求登记**新版本号**。
    ///
    /// # REQ-025 · 并发重登记走**同一条判据**（不再有 TOCTOU）
    ///
    /// 原实现是「先 `find()` 查一遍，没有再 `INSERT`」，**两步之间没有锁**：
    /// 两个连接并发登记同一 `(agent, version)` 时两边都可能 `find()` 到 `None`，
    /// 随后一个 INSERT 成功、另一个撞裸的唯一约束 ⇒ 漏出的是
    /// `RegistryError::Db(unique constraint)`，而不是本该给的
    /// [`RegistryError::VersionLocked`]。
    /// 那时 [`RegistryError::VersionLocked`] 的 `detail`（「同键重登记但内容不同」）
    /// 在并发路径上就成了**说假话**：明明是并发撞键，调用方收到的却是一句
    /// 「数据库错误」，查不出是版本锁定被绕过还是别的什么。
    ///
    /// 现在把判同键这件事**交给数据库**：`INSERT … ON CONFLICT (agent, version) DO NOTHING`
    /// （与 `record_round` 同一形状），
    /// - `rows_affected() > 0` ⇒ 本行是第一个写入者 ⇒ `Ok(true)`；
    /// - `rows_affected() == 0` ⇒ 同键已存在（**无论是不是本进程写的**）⇒
    ///   读回那一行，按内容走**既定的同一条判据**：`Ok(false)` 或 `VersionLocked`。
    ///
    /// 冲突后一定读得到：`ON CONFLICT DO NOTHING` 撞上推测插入时会等对方事务
    /// 结束（COMMIT 才「什么都不做」，ROLLBACK 则自己插入成功），
    /// 等到 COMMIT 那条行就已提交、随即可见。
    ///
    /// 之所以读得到，靠的是**下面这两句各自 autocommit**（都跑在 `self.pool` 上、
    /// 没有显式 `BEGIN`）：读回那条语句会另起一个事务、**取新快照**。
    /// **READ COMMITTED 与 REPEATABLE READ 两种隔离级别下均已实测**读得到。
    ///
    /// ⚠️ 反过来说，「一定读得到」的**前提是「读回那条语句开新事务」**，
    /// 不是「PG 在任何隔离级别下都读得到」：若把这两句塞进**同一个显式
    /// REPEATABLE READ 事务**、且该事务的快照早于对方 COMMIT，就**不成立** ——
    /// `find()` 会拿到 `None`，落进 `ConcurrentRowVanished` 分支。
    ///
    /// ⚠️ **所以「幂等」语义一点没变**：内容相同仍是 `Ok(false)`，
    /// 不是「一律报 `VersionLocked`」—— 那是另一处回归。
    pub async fn register(&self, reg: &AgentRegistration) -> Result<bool, RegistryError> {
        reg.validate()
            .map_err(|e| RegistryError::Invalid(e.to_string()))?;
        if reg.registered_at.trim().is_empty() {
            return Err(RegistryError::Invalid("registered_at 不能为空".into()));
        }

        // REQ-025 · 单步判同键（锁/序列化交给唯一约束，不靠「先查后写」）。
        let res = sqlx::query(
            r#"INSERT INTO extraction_agents (id, agent, version, protocol, endpoint, capabilities, registered_at)
               VALUES ($1, $2, $3, $4, $5, $6, $7::timestamptz)
               ON CONFLICT (agent, version) DO NOTHING"#,
        )
        .bind(uuid::Uuid::new_v4())
        .bind(&reg.agent)
        .bind(&reg.version)
        .bind(&reg.protocol)
        .bind(&reg.endpoint)
        .bind(serde_json::to_value(&reg.capabilities).unwrap_or_else(|_| serde_json::json!([])))
        .bind(&reg.registered_at)
        .execute(self.pool)
        .await?;

        if res.rows_affected() > 0 {
            return Ok(true); // 本行是第一个写入者
        }

        // 冲突 ⇒ 同键已存在。读回比对，走与「已登记」完全相同的判据。
        // registered_at 不参与比对（与原实现一致）：它记的是「谁先登记」，
        // 不是版本内容；拿它当内容会把同键重放误判成改写。
        let existing = self
            .find(reg.agent.as_str(), Some(reg.version.as_str()))
            .await?
            .ok_or_else(|| RegistryError::ConcurrentRowVanished {
                agent: reg.agent.clone(),
                version: reg.version.clone(),
            })?;
        let same = existing.protocol == reg.protocol
            && existing.endpoint == reg.endpoint
            && existing.capabilities == reg.capabilities;
        if same {
            return Ok(false); // 幂等重放
        }
        Err(RegistryError::VersionLocked {
            agent: reg.agent.clone(),
            version: reg.version.clone(),
            detail: format!(
                "已登记 protocol={} endpoint={:?} capabilities={:?}；\
                 本次登记 protocol={} endpoint={:?} capabilities={:?}",
                existing.protocol,
                existing.endpoint,
                existing.capabilities,
                reg.protocol,
                reg.endpoint,
                reg.capabilities,
            ),
        })
    }

    /// **fail-closed** 查询：指定版本查不到就报错，不回落默认值。
    pub async fn require(
        &self,
        agent: &str,
        version: &str,
    ) -> Result<AgentRegistration, RegistryError> {
        self.find(agent, Some(version))
            .await?
            .ok_or_else(|| RegistryError::VersionNotRegistered {
                agent: agent.to_string(),
                version: version.to_string(),
            })
    }

    /// **fail-closed** 查询：只给登记名，取登记的版本。
    ///
    /// 取「登记时刻**最后**写入的那条」而不是 `MAX(version)` ——
    /// 版本号不是数字序，`2.3.1` 与 `10.0.0` 字符串比较会得出反直觉的结果。
    /// 「最新」由登记顺序决定，与 §8.4「具体版本，不是最新版」不冲突：
    /// 这里选一条**具体的**版本填进 `extracted_by.version`。
    pub async fn require_any(&self, agent: &str) -> Result<AgentRegistration, RegistryError> {
        self.find(agent, None)
            .await?
            .ok_or_else(|| RegistryError::AgentNotRegistered {
                agent: agent.to_string(),
            })
    }

    /// 解析身份（登记 + 轮次）。轮次必须 ≥1。
    pub async fn resolve_identity(
        &self,
        agent: &str,
        version: Option<&str>,
        round: i32,
    ) -> Result<AgentIdentity, RegistryError> {
        if round < 1 {
            return Err(RegistryError::BadRound(round));
        }
        let reg = match version {
            Some(v) => self.require(agent, v).await?,
            None => self.require_any(agent).await?,
        };
        Ok(AgentIdentity::from_registration(&reg, round))
    }

    /// **记录一轮迭代**（§4.1「记录每轮迭代」）。
    ///
    /// 重复记录同一 `(agent, version, round)` **幂等**（当作重放）——
    /// 这一条与 `register` 的严格锁定**故意不同**：轮次是「跑过几次」的日志，
    /// 同一轮重放一次不构成「偷偷换版本」。
    pub async fn record_round(
        &self,
        agent: &str,
        version: &str,
        round: i32,
        source: Option<&str>,
    ) -> Result<bool, RegistryError> {
        if round < 1 {
            return Err(RegistryError::BadRound(round));
        }
        // 版本必须已登记 —— 否则记下来的轮次指向一个不存在的 agent。
        self.require(agent, version).await?;
        let res = sqlx::query(
            r#"INSERT INTO extraction_rounds (id, agent, version, round_no, source)
               VALUES ($1, $2, $3, $4, $5)
               ON CONFLICT (agent, version, round_no) DO NOTHING"#,
        )
        .bind(uuid::Uuid::new_v4())
        .bind(agent)
        .bind(version)
        .bind(round)
        .bind(source)
        .execute(self.pool)
        .await?;
        Ok(res.rows_affected() > 0)
    }

    /// 该 agent 已记录的最大轮次（没有则 0）。
    pub async fn max_round(&self, agent: &str, version: &str) -> Result<i32, RegistryError> {
        let row: Option<(Option<i32>,)> = sqlx::query_as(
            r#"SELECT MAX(round_no) FROM extraction_rounds WHERE agent = $1 AND version = $2"#,
        )
        .bind(agent)
        .bind(version)
        .fetch_optional(self.pool)
        .await?;
        Ok(row.and_then(|r| r.0).unwrap_or(0))
    }

    /// 底层查询。`version = None` 时取登记顺序上的最后一条。
    pub async fn find(
        &self,
        agent: &str,
        version: Option<&str>,
    ) -> Result<Option<AgentRegistration>, RegistryError> {
        let row: Option<RegRow> = match version {
            Some(v) => {
                sqlx::query_as(
                    r#"SELECT agent, version, protocol, endpoint, capabilities, registered_at
                           FROM extraction_agents WHERE agent = $1 AND version = $2"#,
                )
                .bind(agent)
                .bind(v)
                .fetch_optional(self.pool)
                .await?
            }
            None => {
                sqlx::query_as(
                    r#"SELECT agent, version, protocol, endpoint, capabilities, registered_at
                           FROM extraction_agents WHERE agent = $1
                           ORDER BY registered_at DESC, id DESC LIMIT 1"#,
                )
                .bind(agent)
                .fetch_optional(self.pool)
                .await?
            }
        };

        Ok(row.map(|(a, v, p, e, c, t)| AgentRegistration {
            agent: a,
            version: v,
            protocol: p,
            endpoint: e,
            capabilities: serde_json::from_value(c).unwrap_or_default(),
            registered_at: t.to_rfc3339(),
        }))
    }
}

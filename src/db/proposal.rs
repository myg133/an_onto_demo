//! 提案的存储。
//!
//! 提案是**审核的输入**，不是本体本身。条目内容放在 `payload`（JSONB）里，
//! 状态（`biz_status` / `model_status`）用独立的列 —— 因为审核查询
//! 总是「哪些还没决」，而 payload 可能有几 KB，不该被反复读出来。
//!
//! **本层是 `db` 与 `domain::review` 的接缝**：`load_items` 把行转成
//! `ItemView`（纯内存结构），状态机因此不需要知道数据库长什么样。

use crate::db::pg::StoreError;
use crate::domain::review::{Decision, ItemKind, ItemView};
use crate::domain::snapshot::ExtractedBy;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::postgres::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct ProposalRow {
    pub id: Uuid,
    pub project_id: Uuid,
    pub base_version_id: Uuid,
    pub round: i32,
    /// **可空**：None = 人工修订的提案（ADR-001）
    pub extracted_by: Option<Value>,
    /// `pending` | `applied` | `rejected`
    pub status: String,
    pub created_at: DateTime<Utc>,
}

/// ⭐ REQ-111 · 条目表的**唯一**列清单。
///
/// 两处 `SELECT`（`items` / `item_by_seq`）共用它：`ProposalItemRow` 加了审计两栏
/// 之后，抄两份列清单迟早会出现「一处带 via、一处不带」—— 那正是
/// 「页面签的看得到、CLI 签的看不到」这类不一致的来源（AC-004 双向一致要防的）。
const ITEM_COLUMNS: &str = "id, proposal_id, seq, kind, target_key, payload, \
                            biz_status, model_status, \
                            biz_decided_at, biz_via, model_decided_at, model_via";

#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct ProposalItemRow {
    pub id: Uuid,
    pub proposal_id: Uuid,
    /// 提案内序号（1 起）。**给人看的稳定编号** —— CLI 的 `--item 1` 指它
    pub seq: i32,
    pub kind: String,
    pub target_key: String,
    pub payload: Value,
    pub biz_status: String,
    pub model_status: String,
    /// ⭐ REQ-111 · 决策审计（AC-009）：**最后一次**改判的时间与入口。
    ///
    /// 两轨各两栏（不是一张追加日志表）—— 因为 AC-005 要求同 `item+track`
    /// 连签只留**终态、不产生两条记录**。审计放在同一行上，两条判据才不互相打架。
    /// `*_decided_at` 为 `NULL` = 这一轨还没人签过（与 `pending` 同源，不是零值）。
    pub biz_decided_at: Option<DateTime<Utc>>,
    /// `cli` / `ui`（见 [`VIA_CLI`] / [`VIA_UI`]）
    pub biz_via: Option<String>,
    pub model_decided_at: Option<DateTime<Utc>>,
    pub model_via: Option<String>,
}

/// ⭐ REQ-111 · 决策入口标识（写进 `proposal_items.*_via`）。
///
/// 为什么是**字符串常量**而不是枚举：这一栏是**审计面**（人查库时读它），
/// 不是状态机的输入 —— 状态机只看 `Decision`。加一个枚举会让人以为
/// 「via 也参与判定」，而它不参与。
pub const VIA_CLI: &str = "cli";
pub const VIA_UI: &str = "ui";

/// 一次决策后该条目的**双轨状态**（来自 `UPDATE … RETURNING`，不另查一次）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecisionOutcome {
    pub biz: Decision,
    pub model: Decision,
}


/// 新提案的输入。
#[derive(Debug, Clone)]
pub struct NewProposal {
    pub base_version_id: Uuid,
    pub round: i32,
    /// 提案携带的抽取来源。`None` = 人工修订（ADR-001）
    pub extracted_by: Option<ExtractedBy>,
}

/// 一个待写入的条目。
#[derive(Debug, Clone)]
pub struct NewItem {
    pub kind: ItemKind,
    pub target_key: String,
    pub payload: Value,
}

impl NewItem {
    /// 提案 JSON 里的一条 item → [`NewItem`]。
    ///
    /// 提在存储层是因为「JSON 形状 → 领域枚举」是这一层的接缝职责：
    /// `cli` 的两条入口（`propose import` 与 REQ-010 的 `concept import`）
    /// 都要过它，抄两份就会漂。
    ///
    /// # REQ-108 · AC-008：`method` 必填
    ///
    /// 条目带 `confidence` 却没有 `method` ⇒ **这里就拒绝**（`import` 因此非 0 退出）。
    /// 报错信息直接写明「没有 method 的分数不可复核」——
    /// 这句话是 AC 的原话，也是拒绝的**理由**，不是一句形式化的校验文案。
    ///
    /// # 为什么 confidence 是**搬进 payload**，而不是加一个结构体字段
    ///
    /// 两个原因，都不是风格问题：
    ///
    /// 1. `apply` 那条路只拿得到 `ItemView.payload`（`domain::review` 的条目视图
    ///    没有独立的置信度栏），不并进 payload 的话分数就进不了快照，
    ///    `--model-view` 也就无从显示；
    /// 2. `NewItem` 的字面量构造散布在 10+ 个测试里（`test_apply` / `test_proposal` /
    ///    `test_review_show_e2e` …），加字段会让它们全部编译不过 ——
    ///    那是为一件「搬家」改十处测试，收益为零而噪声很大。
    ///
    /// 所以：**条目顶层**的 `confidence`（设计概要 §2.1 的形状）在**入口处**
    /// 校验一次，然后搬进 `payload.confidence`。落库时三栏从同一个值取，
    /// **不是两个真相**。
    pub fn from_json(it: &Value, idx: usize) -> anyhow::Result<Self> {
        let kind_s = it["kind"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("item {} 缺 kind", idx + 1))?;
        let target = it["target"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("item {} 缺 target", idx + 1))?;
        let kind = match kind_s {
            "add_entity_type" => ItemKind::AddEntityType,
            "add_property" => ItemKind::AddProperty,
            "add_link_type" => ItemKind::AddLinkType,
            "remove_entity_type" => ItemKind::RemoveEntityType,
            "modify_entity_type" => ItemKind::ModifyEntityType,
            // REQ-010 · 阶段 3 的产出走**同一条**提案通道
            "add_concept_group" => ItemKind::AddConceptGroup,
            other => anyhow::bail!("未知 kind：{other}"),
        };
        let mut payload = it["payload"].clone();
        match it.get("confidence") {
            None | Some(Value::Null) => {}
            Some(v) => {
                let c = crate::domain::confidence::from_untrusted(v)
                    .map_err(|e| anyhow::anyhow!("item {} 的 confidence 不合法：{e}", idx + 1))?;
                if let Some(obj) = payload.as_object_mut() {
                    obj.insert(
                        "confidence".to_string(),
                        serde_json::to_value(&c).unwrap_or(Value::Null),
                    );
                }
            }
        }
        Ok(NewItem { kind, target_key: target.to_string(), payload })
    }

    /// payload 里带的置信度（**同一份值**，不另存）。
    ///
    /// 出错时返回 `StoreError::BadConfidence` —— 走到这里还读不懂，
    /// 说明 `payload` 被别的工具改过（`from_json` 已经校验过一次）。
    fn confidence(&self) -> Result<Option<crate::domain::confidence::Confidence>, StoreError> {
        match self.payload.get("confidence") {
            None | Some(Value::Null) => Ok(None),
            Some(v) => crate::domain::confidence::from_untrusted(v)
                .map(Some)
                .map_err(|e| StoreError::BadConfidence(format!("{}：{e}", self.target_key))),
        }
    }
}

#[derive(Clone)]
pub struct PgProposalStore {
    pool: PgPool,
}

impl PgProposalStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn migrate(&self) -> Result<(), StoreError> {
        crate::db::pg::migrate(&self.pool).await?;
        Ok(())
    }

    /// 建提案 + 一次性写入全部条目（单事务）。
    pub async fn create(
        &self,
        project_id: Uuid,
        p: NewProposal,
        items: &[NewItem],
    ) -> Result<ProposalRow, StoreError> {
        if items.is_empty() {
            return Err(StoreError::NothingApplicable);
        }
        let mut tx = self.pool.begin().await?;
        let id = Uuid::new_v4();

        let row: ProposalRow = sqlx::query_as(
            "INSERT INTO proposals (id, project_id, base_version_id, round, extracted_by, status)
             VALUES ($1,$2,$3,$4,$5,'pending')
             RETURNING id, project_id, base_version_id, round, extracted_by, status, created_at",
        )
        .bind(id)
        .bind(project_id)
        .bind(p.base_version_id)
        .bind(p.round)
        .bind(p.extracted_by.as_ref().and_then(|e| serde_json::to_value(e).ok()))
        .fetch_one(&mut *tx)
        .await?;

        for (i, it) in items.iter().enumerate() {
            // **关系类条目的 payload 必须带 from_type / to_type** ——
            // Task 6 的状态机靠它判断两端是否可用。缺了就等于声明不完整，
            // 会被按「悬空关系」拦下（宁可拦住，不要静默放行）。
            if it.kind.is_relational() {
                for f in ["from_type", "to_type"] {
                    if it.payload.get(f).and_then(|v| v.as_str()).is_none() {
                        return Err(StoreError::Db(sqlx::Error::Protocol(
                            format!("关系类条目的 payload 缺 {f}：target={}", it.target_key),
                        )));
                    }
                }
            }
            // REQ-108 · AC-008：`method` 必填 —— 这里是**第二道**（`from_json` 是第一道），
            // 因为直连本函数的调用方可能自己拼 `NewItem`。读不懂 ⇒ 拒绝入库。
            let confidence = it.confidence()?;
            sqlx::query(
                "INSERT INTO proposal_items
                   (id, proposal_id, seq, kind, target_key, payload, biz_status, model_status,
                    confidence, confidence_method, confidence_signals)
                 VALUES ($1,$2,$3,$4,$5,$6,'pending','pending',$7,$8,$9)",
            )
            .bind(Uuid::new_v4())
            .bind(id)
            .bind(i as i32 + 1)
            .bind(it.kind.as_str())
            .bind(&it.target_key)
            .bind(&it.payload)
            // REQ-108 · 三栏置信度；`None` ⇒ SQL NULL（实体条目没有分数，不是 0 分）
            .bind(confidence.as_ref().map(|c| c.score))
            .bind(confidence.as_ref().map(|c| c.method.clone()))
            .bind(
                confidence
                    .as_ref()
                    .and_then(|c| serde_json::to_value(c).ok()),
            )
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(row)
    }

    pub async fn get(&self, id: Uuid) -> Result<Option<ProposalRow>, StoreError> {
        let r = sqlx::query_as(
            "SELECT id, project_id, base_version_id, round, extracted_by, status, created_at
             FROM proposals WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(r)
    }

    pub async fn items(&self, proposal_id: Uuid) -> Result<Vec<ProposalItemRow>, StoreError> {
        let rows = sqlx::query_as(&format!(
            "SELECT {ITEM_COLUMNS} FROM proposal_items WHERE proposal_id = $1 ORDER BY seq"
        ))
        .bind(proposal_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// 按 `seq` 取单个条目（CLI `--item 1` 走这里）。
    pub async fn item_by_seq(
        &self,
        proposal_id: Uuid,
        seq: i32,
    ) -> Result<Option<ProposalItemRow>, StoreError> {
        let r = sqlx::query_as(&format!(
            "SELECT {ITEM_COLUMNS} FROM proposal_items WHERE proposal_id = $1 AND seq = $2"
        ))
        .bind(proposal_id)
        .bind(seq)
        .fetch_optional(&self.pool)
        .await?;
        Ok(r)
    }

    /// ⭐ REQ-111 · **决策的唯一实现**（CLI 的 `review decide` 与审核界面的
    /// `POST /api/decide` 都走这里）。
    ///
    /// # 为什么必须只剩这一个入口（纪律 H）
    ///
    /// 「提案内序号 → 条目 → 单轨判定 → 落库」这条路径此前只住在
    /// `cli.rs::review_cmd` 的分支里。审核界面若在 HTTP 层再写一遍 UPDATE，
    /// 就成了**第二份决策逻辑**：两份永远一致只是巧合，而「同 item+track
    /// 连签只留终态」（AC-005）与「审计带 via」（AC-009）会先在其中一份上漂。
    ///
    /// 所以：**序号解析、领域更新、落库、审计**四步全部收进本函数，
    /// 两个入口只差一个 `via` 参数。`set_decision_via` 是**唯一**的 UPDATE 点；
    /// `git grep 'set_decision_via'` 只会命中「定义 + 本函数 + 兼容旧的
    /// `set_decision`」三处。
    ///
    /// # 为什么还调一次 `domain::review::apply_decision`
    ///
    /// 状态机是纯函数（`domain::review`，零 I/O）。让**它**算下一态、
    /// 存储层只负责落库，等于把「判定」与「持久化」的边界写清楚：
    /// 将来状态机加规则（例如「已否决能否被抹掉」），两个入口同时生效。
    pub async fn decide_item(
        &self,
        proposal_id: Uuid,
        seq: i32,
        track: crate::domain::review::Track,
        d: Decision,
        via: &str,
    ) -> Result<DecisionOutcome, StoreError> {
        let row = self
            .item_by_seq(proposal_id, seq)
            .await?
            .ok_or(StoreError::ItemNotFound { seq })?;
        // 领域层算下一态（`apply_decision` 是纯函数：只改被签的那一轨）。
        let view = Self::to_item_views(std::slice::from_ref(&row))
            .pop()
            .ok_or(StoreError::ItemNotFound { seq })?;
        let next = crate::domain::review::apply_decision(&view, track, d);
        let status = match track {
            crate::domain::review::Track::Biz => next.biz_status,
            crate::domain::review::Track::Model => next.model_status,
        };
        self.set_decision_via(row.id, track, status, via).await
    }

    /// 写入单轨判定 —— **全仓唯一**的决策 UPDATE。
    ///
    /// `via` 与 `now()` 一起落库（AC-009）：`decided_at` 由**数据库**取时间，
    /// 不用进程时钟（多进程下进程时钟会给出先后矛盾的审计）。
    ///
    /// 返回更新后的双轨状态，调用方不必再查一次。
    pub async fn set_decision_via(
        &self,
        item_id: Uuid,
        track: crate::domain::review::Track,
        d: Decision,
        via: &str,
    ) -> Result<DecisionOutcome, StoreError> {
        // 三列名都是上面 match 出来的字面量，不来自用户输入 —— 无注入面。
        let (col, at_col, via_col) = match track {
            crate::domain::review::Track::Biz => ("biz_status", "biz_decided_at", "biz_via"),
            crate::domain::review::Track::Model => {
                ("model_status", "model_decided_at", "model_via")
            }
        };
        let row: (String, String) = sqlx::query_as(&format!(
            "UPDATE proposal_items
                SET {col} = $2, {at_col} = now(), {via_col} = $3
              WHERE id = $1
          RETURNING biz_status, model_status"
        ))
        .bind(item_id)
        .bind(d.as_str())
        .bind(via)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(StoreError::Db(sqlx::Error::RowNotFound))?;
        Ok(DecisionOutcome {
            biz: parse_decision(&row.0),
            model: parse_decision(&row.1),
        })
    }

    /// 写入单轨判定（**兼容旧签名**：默认入口记 `cli`）。
    ///
    /// 保留它是因为 `proposal apply`（`db/apply.rs`）与一批既有测试直接调它；
    /// 生产上的两条**人工**入口都走 [`Self::decide_item`] / [`Self::set_decision_via`]。
    /// 语义仍是「后写覆盖」，审计栏随之更新。
    ///
    /// **不校验是否已 rejected** —— 那正是 G-5 待拍板的语义问题：
    /// 同轨的 `accepted` 能否抹掉显式的 `rejected`？
    /// 当前实现**允许**（后写覆盖，AC-005 要求的正是「连签 = 改判」）。
    pub async fn set_decision(
        &self,
        item_id: Uuid,
        track: crate::domain::review::Track,
        d: Decision,
    ) -> Result<(), StoreError> {
        self.set_decision_via(item_id, track, d, VIA_CLI)
            .await
            .map(|_| ())
    }

    pub async fn set_status(&self, id: Uuid, status: &str) -> Result<(), StoreError> {
        sqlx::query("UPDATE proposals SET status = $2 WHERE id = $1")
            .bind(id)
            .bind(status)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// 行 → 状态机的输入结构。**这是两层接缝所在。**
    ///
    /// `id` 用 `"seq:kind:target"` 合成 —— 稳定、可读、跨进程一致
    /// （UUID 每次导入都变，不能当外部可见的标识）。
    pub fn to_item_views(rows: &[ProposalItemRow]) -> Vec<ItemView> {
        rows.iter()
            .map(|r| ItemView {
                id: format!("{}:{}:{}", r.seq, r.kind, r.target_key),
                kind: parse_kind(&r.kind),
                target: r.target_key.clone(),
                from_type: r
                    .payload
                    .get("from_type")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                to_type: r
                    .payload
                    .get("to_type")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                biz_status: parse_decision(&r.biz_status),
                model_status: parse_decision(&r.model_status),
                payload: r.payload.clone(),
            })
            .collect()
    }
}

/// DB 文本 → 枚举。**未知值落 `AddProperty`**（见下方「已知问题」）。
pub fn parse_kind(s: &str) -> ItemKind {
    match s {
        "add_entity_type" => ItemKind::AddEntityType,
        "add_property" => ItemKind::AddProperty,
        "add_link_type" => ItemKind::AddLinkType,
        "remove_entity_type" => ItemKind::RemoveEntityType,
        "modify_entity_type" => ItemKind::ModifyEntityType,
        // REQ-010：阶段 3 的产出与其它 kind **同表同状态机**。
        // 这一臂是**必须显式写**的 —— 少了它，概念组条目会从库里读成
        // 「加属性」，然后 apply 报一个毫不相干的 `property.column 缺失`。
        "add_concept_group" => ItemKind::AddConceptGroup,
        // ⚠ 已知问题（REQ-010 发现，**本轮不改**）：未知 kind 落 `AddProperty`。
        // 落点「不沾关系分支」是有意的（`tests/test_proposal.rs` 有断言守着），
        // 但它**会走写入分支** —— apply 的 AddProperty 分支会去 payload 取
        // `parent_key` 与 `column`，取到就真的往实体上加一条属性。
        // 即：**一个拼错的 kind 会改本体**，方向是 fail-open。
        //
        // 为什么不在这轮改：`ItemKind` 与 `parse_kind` 的兜底语义是
        // 已过四轮独立审核的基础设施，本轮的范围是概念层。
        // 建议单开一项：给未知值一个**不写本体**的落点（如 `Unknown`），
        // 并同步改 `test_proposal_parse_kind_known_and_unknown` 的断言。
        _ => ItemKind::AddProperty,
    }
}

pub fn parse_decision(s: &str) -> Decision {
    match s {
        "accepted" => Decision::Accepted,
        "rejected" => Decision::Rejected,
        _ => Decision::Pending,
    }
}

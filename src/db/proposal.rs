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
        Ok(NewItem { kind, target_key: target.to_string(), payload: it["payload"].clone() })
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
            sqlx::query(
                "INSERT INTO proposal_items
                   (id, proposal_id, seq, kind, target_key, payload, biz_status, model_status)
                 VALUES ($1,$2,$3,$4,$5,$6,'pending','pending')",
            )
            .bind(Uuid::new_v4())
            .bind(id)
            .bind(i as i32 + 1)
            .bind(it.kind.as_str())
            .bind(&it.target_key)
            .bind(&it.payload)
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
        let rows = sqlx::query_as(
            "SELECT id, proposal_id, seq, kind, target_key, payload, biz_status, model_status
             FROM proposal_items WHERE proposal_id = $1 ORDER BY seq",
        )
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
        let r = sqlx::query_as(
            "SELECT id, proposal_id, seq, kind, target_key, payload, biz_status, model_status
             FROM proposal_items WHERE proposal_id = $1 AND seq = $2",
        )
        .bind(proposal_id)
        .bind(seq)
        .fetch_optional(&self.pool)
        .await?;
        Ok(r)
    }

    /// 写入单轨判定。
    ///
    /// **不校验是否已 rejected** —— 那正是 G-5 待拍板的语义问题：
    /// 同轨的 `accepted` 能否抹掉显式的 `rejected`？
    /// 当前实现**允许**（后写覆盖），并在 `decisions_log` 留痕。
    /// 拍板后这里要加守卫。
    pub async fn set_decision(
        &self,
        item_id: Uuid,
        track: crate::domain::review::Track,
        d: Decision,
    ) -> Result<(), StoreError> {
        let col = match track {
            crate::domain::review::Track::Biz => "biz_status",
            crate::domain::review::Track::Model => "model_status",
        };
        // col 是上面 match 出来的字面量，不来自用户输入 —— 无注入面
        sqlx::query(&format!(
            "UPDATE proposal_items SET {col} = $2 WHERE id = $1"
        ))
        .bind(item_id)
        .bind(d.as_str())
        .execute(&self.pool)
        .await?;
        Ok(())
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

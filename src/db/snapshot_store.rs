//! 快照的存取。
//!
//! **单事务铁律**（Task 4 Step 3）：删旧行 → 写新行 → 更新版本行
//! 必须在**同一个事务**里。否则中途失败会留下半截快照 ——
//! 而半截快照比没有快照更坏：它看起来是有效的。

use crate::db::pg::StoreError;
use crate::domain::concept::ConceptGroup;
use crate::domain::snapshot::{
    ActionType, EntityType, ExtractedBy, Function, LinkType, Snapshot,
};
use serde_json::Value;
use sqlx::postgres::PgPool;
use uuid::Uuid;

#[derive(Clone)]
pub struct PgStore {
    pool: PgPool,
}

impl PgStore {
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

    /// 存快照。
    ///
    /// **版本已冻结则拒绝**（AC-006）—— 这��写路径的闸门。
    pub async fn save_snapshot(&self, snap: &Snapshot) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await?;

        // ① 冻结闸门 —— 在事务内查，避免 TOCTOU
        let frozen: Option<bool> =
            sqlx::query_scalar("SELECT frozen_at IS NOT NULL FROM ontology_versions WHERE id = $1")
                .bind(snap.version_id)
                .fetch_optional(&mut *tx)
                .await?;
        match frozen {
            None => {
                return Err(StoreError::Db(sqlx::Error::RowNotFound));
            }
            Some(true) => {
                return Err(StoreError::VersionFrozen { seq: snap.seq });
            }
            Some(false) => {}
        }

        // ② 删旧行 —— REQ-033：五表循环收进共享层 `delete_snapshot_rows`。
        //
        // ⚠️ **冻结闸门在 ①，必须留在它前面**：闸门没放行之前不许有任何
        // DELETE，否则等于在冻结版本上删数据。本调用点的**位置未动**，
        // 共享层内部也不做任何准入判断（只拿调用方的 tx，只删，不开事务）。
        // `tests/test_snapshot_write_paths.rs` 里
        // `test_freeze_gate_precedes_delete_and_delete_covers_five_tables` 钉的就是这个次序。
        //
        // 顺序说明（先删 link —— 它按 key 引用两端，不受 FK 约束保护；
        // `concept_groups` 排最后：它没有外键指向 object_types，
        // 概念组是独立的展示层声明，删旧行的顺序不牵涉红线）。
        delete_snapshot_rows(&mut tx, snap.version_id).await?;

        // ③ 写新行 —— REQ-012：五栏 INSERT 只有一份实现（见
        // `insert_snapshot_rows`）。**本入口的错误映射保持原样**：
        // unique 冲突 ⇒ `StoreError::KeyConflict(key)`，其余 ⇒ `StoreError::Db`。
        insert_snapshot_rows(&mut tx, snap)
            .await
            .map_err(|e| map_key_conflict(e.source, &e.key))?;

        // ④ 版本行：`extracted_by` 与快照**同时**写入
        //
        // 刻意不放成独立方法只由 apply 调 —— 那样 `save_snapshot` 存完的
        // 快照取回来 `extracted_by` 恒为 None，往返测试立刻失败。
        // 它是**版本的属性**，与快照同生共死。
        //
        // REQ-033：SQL 字面量、参数顺序、`None → SQL NULL` 的序列化三者
        // 已收进 `update_version_extracted_by`（全仓只有一份），
        // 这里只剩「拿本事务去执行」。
        update_version_extracted_by(snap.version_id, &snap.extracted_by)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn get_snapshot(&self, version_id: Uuid) -> Result<Option<Snapshot>, StoreError> {
        let Some((seq, eb)) = sqlx::query_as::<_, (i32, Option<Value>)>(
            "SELECT seq, extracted_by FROM ontology_versions WHERE id = $1",
        )
        .bind(version_id)
        .fetch_optional(&self.pool)
        .await?
        else {
            return Ok(None);
        };

        let snap = Snapshot {
            version_id,
            seq,
            // SQL NULL → None（人工修订）；有值 → 反序列化
            extracted_by: eb.and_then(|j| serde_json::from_value(j).ok()),
            entity_types: self.load_entities(version_id).await?,
            link_types: self.load_links(version_id).await?,
            action_types: self.load_actions(version_id).await?,
            functions: self.load_functions(version_id).await?,
            concept_groups: self.load_concept_groups(version_id).await?,
        };
        Ok(Some(snap))
    }

    /// `extracted_by` 存在版本行上（**可空**：人工修订记 NULL，ADR-001）。
    pub async fn get_extracted_by(&self, version_id: Uuid) -> Result<Option<Option<ExtractedBy>>, StoreError> {
        let row: Option<(Option<Value>,)> = sqlx::query_as(
            "SELECT extracted_by FROM ontology_versions WHERE id = $1",
        )
        .bind(version_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some((v,)) = row else { return Ok(None) };
        Ok(Some(match v {
            None => None,
            Some(j) => serde_json::from_value(j).ok(),
        }))
    }

    /// 写 `extracted_by`。**`None` 表示人工修订，落成 SQL NULL**。
    ///
    /// **版本已冻结则拒绝**（AC-006）—— 与 `save_snapshot` 同款闸门。
    pub async fn set_extracted_by(
        &self,
        version_id: Uuid,
        v: &Option<ExtractedBy>,
    ) -> Result<(), StoreError> {
        // REQ-038 · 补冻结闸门（**本条不是新增 API，是给既有 API 补前置检查**）。
        //
        // 收口前这里是**裸 pool UPDATE，零前置检查**。同一个 `PgStore` 上
        // `save_snapshot` 走的是「begin → 事务内查 frozen_at → Some(true) ⇒
        // VersionFrozen」，本函数完全绕过。QA 实测的后果：
        //   冻结版本上 save_snapshot    → VersionFrozen            ✔ 被挡住
        //   冻结版本上 set_extracted_by → Ok，extracted_by 抹成 NULL ✘ 没挡住
        //
        // 而 ADR-001 里 `None` = **人工修订**。所以这条路径能把冻结版本的
        // 溯源改写成「人工修订」—— 凭空造出一个人工修订标记，
        // 是 AC-006「版本已冻结则拒绝写入」的破口。
        //
        // **为什么走事务，而不是「在 pool 上先查再 UPDATE」**：
        // 单线程下两者行为相同，但后者把「查」与「写」拆成两个自动提交语句，
        // 中间留窗口 —— 并发 `freeze()` 可以插在两者之间落定，
        // 于是「查时未冻结 / 写时已冻结」照样写进去（TOCTOU）。
        // 放进同一事务后，闸门与写入共享一个快照/锁边界，窗口消失；
        // 这与 `save_snapshot` ① 步的形状一致，不留第二种写法。
        // 归属说明：本缺口来自 `32dc515`（REQ-012），**不是 REQ-033 引入**；
        // REQ-033 的 QA 判定应当收口，故在本轮补上。
        let mut tx = self.pool.begin().await?;

        // ① 冻结闸门 —— 在事务内查，避免 TOCTOU。错误映射与 `save_snapshot` 逐字同形：
        //    版本不存在 ⇒ `Db(RowNotFound)`；已冻结 ⇒ `VersionFrozen { seq }`。
        //    `seq` 一并取回：`set_extracted_by` 没有 `Snapshot` 可借，
        //    只能自己查（`save_snapshot` 用的是 `snap.seq`）。
        let gate: Option<(i32, bool)> = sqlx::query_as(
            "SELECT seq, frozen_at IS NOT NULL FROM ontology_versions WHERE id = $1",
        )
        .bind(version_id)
        .fetch_optional(&mut *tx)
        .await?;
        match gate {
            None => {
                return Err(StoreError::Db(sqlx::Error::RowNotFound));
            }
            Some((seq, true)) => {
                return Err(StoreError::VersionFrozen { seq });
            }
            Some((_, false)) => {}
        }

        // ② 版本行 UPDATE —— 仍在**本事务内**执行（不能退回 `self.pool`，
        //    那样又变成「查在事务里、写在事务外」的半拉子）。
        //
        // REQ-033 · 与 `save_snapshot` / `write_snapshot_in` 共用同一个构造点。
        // 原文这里自己算了一遍序列化，且用的是 `.unwrap()`；共享层统一用
        // `.ok()`。两者在本类型上**不可达地等价**：`ExtractedBy` 的字段只有
        // `String` / `i32` / `Vec<String>` 和一个无字段枚举，
        // 没有 map 键、没有 f64/NaN，`to_value` 不会失败。
        // 所以这不是对外行为的改动，只是把「DB 写路径上不该 panic」这条
        // 统一掉了 —— 语义仍是 ADR-001：`None` = 人工修订 ⇒ SQL NULL。
        update_version_extracted_by(version_id, v)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    // ── 各表的读取 ──────────────────────────────────────

    async fn load_entities(&self, vid: Uuid) -> Result<Vec<EntityType>, StoreError> {
        let rows: Vec<(String, String, Option<String>, Value, Option<String>, Value, Value, Value)> =
            sqlx::query_as(
                "SELECT key, display_name, description, aliases, parent_key,
                        disjoint_with, properties, evidence
                 FROM object_types WHERE version_id = $1 ORDER BY ordinal",
            )
            .bind(vid)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|(key, dn, desc, al, pk, dw, props, ev)| EntityType {
                key,
                display_name: dn,
                description: desc, // ← NULL 还原成 None（三态）
                aliases: from_json(al),
                parent: pk,
                disjoint_with: from_json(dw),
                properties: serde_json::from_value(props).unwrap_or_default(),
                evidence: serde_json::from_value(ev).unwrap_or_default(),
            })
            .collect())
    }

    async fn load_links(&self, vid: Uuid) -> Result<Vec<LinkType>, StoreError> {
        // REQ-108 · 三栏 confidence 与 `LinkType::confidence` 一一对应。
        // ⚠️ 读的是**整份评分卡**（`confidence_signals` 里存的是完整对象，
        // 含 signals / veto / band）—— score 与 method 两栏是给 SQL 查询与
        // 校准用的冗余列，不参与重建。
        let rows: Vec<(String, String, String, String, String, Value, Option<Value>)> = sqlx::query_as(
            "SELECT key, from_type, to_type, cardinality, semantic, evidence, confidence_signals
             FROM link_types WHERE version_id = $1 ORDER BY ordinal",
        )
        .bind(vid)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(key, f, t, c, s, ev, conf)| LinkType {
                key,
                from_type: f,
                to_type: t,
                cardinality: c,
                semantic: s,
                evidence: serde_json::from_value(ev).unwrap_or_default(),
                // NULL ⇒ None（REQ-108 之前的版本 / 人工修订）——
                // **不伪造分数**：不知道就是不知道。
                confidence: conf.and_then(|j| crate::domain::confidence::from_untrusted(&j).ok()),
            })
            .collect())
    }

    async fn load_actions(&self, vid: Uuid) -> Result<Vec<ActionType>, StoreError> {
        let rows: Vec<(String, String, Value, Value, Value, Value, Value, Value)> = sqlx::query_as(
            "SELECT key, target, parameters, preconditions, effects, permission, audit_policy, evidence
             FROM action_types WHERE version_id = $1 ORDER BY ordinal",
        )
        .bind(vid)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(key, target, p, pre, eff, perm, audit, ev)| ActionType {
                key,
                target,
                parameters: p,
                preconditions: pre,
                effects: serde_json::from_value(eff).unwrap_or_default(),
                permission: perm,
                audit_policy: audit,
                evidence: serde_json::from_value(ev).unwrap_or_default(),
            })
            .collect())
    }

    /// REQ-010 · 读概念组。
    ///
    /// `scope_hint` / `extracted_by` **可空**（三态之一：NULL = 还没写
    /// 说明 / 人工修订），`members` / `evidence` 是 JSONB 数组。
    async fn load_concept_groups(&self, vid: Uuid) -> Result<Vec<ConceptGroup>, StoreError> {
        let rows: Vec<(String, String, Value, Option<String>, Option<Value>, Option<bool>, Value)> =
            sqlx::query_as(
                "SELECT key, display_name, members, scope_hint, extracted_by,
                        low_confidence, evidence
                 FROM concept_groups WHERE version_id = $1 ORDER BY ordinal",
            )
            .bind(vid)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|(key, dn, members, hint, eb, lc, ev)| ConceptGroup {
                key,
                display_name: dn,
                members: from_json(members),
                scope_hint: hint,
                extracted_by: eb.and_then(|j| serde_json::from_value(j).ok()),
                // REQ-028 · F4：NULL 读成 None（「没声明过」），
                // **不回填成 Some(false)** —— 回填等于替业务侧做了一个
                // 从没做过的断言，那正是本 REQ 要拔掉的 fail-open。
                low_confidence: lc,
                evidence: serde_json::from_value(ev).unwrap_or_default(),
            })
            .collect())
    }

    async fn load_functions(&self, vid: Uuid) -> Result<Vec<Function>, StoreError> {
        let rows: Vec<(String, String, Value, Value)> = sqlx::query_as(
            "SELECT key, signature, allowed_types, evidence FROM functions WHERE version_id = $1 ORDER BY ordinal",
        )
        .bind(vid)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(key, sig, at, ev)| Function {
                key,
                signature: sig,
                allowed_types: from_json(at),
                evidence: serde_json::from_value(ev).unwrap_or_default(),
            })
            .collect())
    }
}

// ── REQ-012 · 快照行写入的**唯一**实现 ──────────────────────

/// REQ-012 · **两处入口都不许自己写 INSERT。**
///
/// # 为什么有这个东西
///
/// REQ-011 的缺陷本体是「两份同构实现」：`PgStore::save_snapshot`
/// （`ontology snapshot save` 走的那条）与 `PgApplier` → `write_snapshot_in`
/// （`propose apply` 走的那条，**生产写路径**）各自抄了一份五栏 INSERT。
/// 抄的时候生产那份漏了 `action_types` / `functions` ——
/// 一个版本一次 apply 就把这两栏静默清空，事务内不报任何错。
///
/// REQ-011 补上了那两段 INSERT，**病根没除**：两份实现仍然并存，
/// 下一次再加一栏还是同样的漏法。这里把「行写入」收成一份。
///
/// # 共享什么、不共享什么（逐条对着 diff 定的）
///
/// |                        | `save_snapshot` | `write_snapshot_in` |
/// |------------------------|-----------------|---------------------|
/// | INSERT 主体（SQL / bind / `execute`） | 13 行 | **逐行相同** |
/// | 冲突时的错误映射        | `map_key_conflict(err, key)` ⇒ `StoreError::KeyConflict` | `?` ⇒ `StoreError::Db` |
///
/// 所以**只共享 INSERT 主体**：本函数返回原始 [`sqlx::Error`] **加上出错的 key**，
/// 由两个调用方**各自**映射（见两处调用点）。错误语义逐字未变，
/// 一个 unique 冲突在 save 上仍是 `KeyConflict`，在 apply 上仍是 `Db`。
///
/// ⚠️ 刻意**不**做成「`write_snapshot_in` 转手调 `save_snapshot`」：
/// 那样 apply 会背上 save 的五段重排 + 冻结检查，错误映射也就真的被改了。
///
/// ⚠️ 序列化统一用 [`json_of`] / [`evidence_json`] / `properties_json`。
/// `write_snapshot_in` 原先在 object/link/concept 三处用的是
/// `serde_json::to_value(..).unwrap_or_default()` —— 那里的 fallback 是
/// `Value::Null`，这三个是 `Value::Array([])`。**在本仓这些类型上
/// `to_value` 不会失败**（`Vec<String>` / `Vec<Property>` / `Vec<Evidence>`
/// 全是结构体与字符串数组，无 map 键、无 f64/NaN），所以两者不可达地一致；
/// 统一到 `json_of` 是为了让「同一份序列化」名副其实（REQ-011 已为
/// action/functions 两栏做过同样的统一，本次把剩下三栏补齐）。
pub(crate) async fn insert_snapshot_rows(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    snap: &Snapshot,
) -> Result<(), SnapshotRowError> {
    for (i, e) in snap.entity_types.iter().enumerate() {
        sqlx::query(
            "INSERT INTO object_types
               (id, version_id, key, display_name, description, aliases,
                parent_key, disjoint_with, properties, evidence, ordinal)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
        )
        .bind(Uuid::new_v4())
        .bind(snap.version_id)
        .bind(&e.key)
        .bind(&e.display_name)
        .bind(e.description.as_deref()) // ← None 必须落成 NULL，不是 ''
        .bind(json_of(&e.aliases))
        .bind(e.parent.as_deref())
        .bind(json_of(&e.disjoint_with))
        .bind(properties_json(e))
        .bind(evidence_json(&e.evidence))
        .bind(i as i32)
        .execute(&mut **tx)
        .await
        .map_err(|source| SnapshotRowError {
            table: "object_types",
            key: e.key.clone(),
            source,
        })?;
    }

    for (i, l) in snap.link_types.iter().enumerate() {
        // REQ-108 · 关系行携带三栏置信度（score / method / 整份评分卡）。
        //
        // `confidence` 为 `None` ⇒ 三栏都落 SQL NULL（旧数据 / 人工修订），
        // **不回填 0.0** —— 0.0 是「确定它不成立」，None 是「不知道」。
        sqlx::query(
            "INSERT INTO link_types
               (id, version_id, key, from_type, to_type, cardinality, semantic, evidence, ordinal,
                confidence, confidence_method, confidence_signals)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
        )
        .bind(Uuid::new_v4())
        .bind(snap.version_id)
        .bind(&l.key)
        .bind(&l.from_type)
        .bind(&l.to_type)
        .bind(&l.cardinality)
        .bind(&l.semantic)
        .bind(evidence_json(&l.evidence))
        .bind(i as i32)
        .bind(l.confidence.as_ref().map(|c| c.score))
        .bind(l.confidence.as_ref().map(|c| c.method.clone()))
        .bind(
            l.confidence
                .as_ref()
                .and_then(|c| serde_json::to_value(c).ok()),
        )
        .execute(&mut **tx)
        .await
        .map_err(|source| SnapshotRowError {
            table: "link_types",
            key: l.key.clone(),
            source,
        })?;
    }

    // REQ-011 补的那两栏。搬到这里之后，它们与另外三栏**同处一个函数** ——
    // 「再加一栏忘了同步两份」这个失败模式从结构上被取消。
    for (i, a) in snap.action_types.iter().enumerate() {
        sqlx::query(
            "INSERT INTO action_types
               (id, version_id, key, target, parameters, preconditions,
                effects, permission, audit_policy, evidence, ordinal)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
        )
        .bind(Uuid::new_v4())
        .bind(snap.version_id)
        .bind(&a.key)
        .bind(&a.target)
        .bind(&a.parameters)
        .bind(&a.preconditions)
        .bind(json_of(&a.effects))
        .bind(&a.permission)
        .bind(&a.audit_policy)
        .bind(evidence_json(&a.evidence))
        .bind(i as i32)
        .execute(&mut **tx)
        .await
        .map_err(|source| SnapshotRowError {
            table: "action_types",
            key: a.key.clone(),
            source,
        })?;
    }

    for (i, f) in snap.functions.iter().enumerate() {
        sqlx::query(
            "INSERT INTO functions
               (id, version_id, key, signature, allowed_types, evidence, ordinal)
             VALUES ($1,$2,$3,$4,$5,$6,$7)",
        )
        .bind(Uuid::new_v4())
        .bind(snap.version_id)
        .bind(&f.key)
        .bind(&f.signature)
        .bind(json_of(&f.allowed_types))
        .bind(evidence_json(&f.evidence))
        .bind(i as i32)
        .execute(&mut **tx)
        .await
        .map_err(|source| SnapshotRowError {
            table: "functions",
            key: f.key.clone(),
            source,
        })?;
    }

    // REQ-010 · 概念组。与实体行同事务写，保证「一个版本 =
    // 一份完整状态」—— 不会出现「实体升了、概念组还是上一版的」。
    for (i, c) in snap.concept_groups.iter().enumerate() {
        sqlx::query(
            "INSERT INTO concept_groups
               (id, version_id, key, display_name, members, scope_hint,
                extracted_by, low_confidence, evidence, ordinal)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
        )
        .bind(Uuid::new_v4())
        .bind(snap.version_id)
        .bind(&c.key)
        .bind(&c.display_name)
        .bind(json_of(&c.members))
        .bind(c.scope_hint.as_deref())
        .bind(c.extracted_by.as_ref().and_then(|e| serde_json::to_value(e).ok()))
        .bind(c.low_confidence) // REQ-028 · F4：三态原样落库，None 就是 NULL
        .bind(evidence_json(&c.evidence))
        .bind(i as i32)
        .execute(&mut **tx)
        .await
        .map_err(|source| SnapshotRowError {
            table: "concept_groups",
            key: c.key.clone(),
            source,
        })?;
    }

    Ok(())
}

/// 行写入失败的定位信息：**原始 `sqlx::Error` + 出错的那一栏 + 它的 key**。
///
/// **刻意不预先映射成 [`StoreError`]** —— 两个入口对同一个 DB 错误的
/// 对外语义不同（`save_snapshot` 要 `KeyConflict`，`write_snapshot_in` 要 `Db`），
/// 映射必须留在各自调用方，见 [`insert_snapshot_rows`] 的说明。
pub struct SnapshotRowError {
    pub table: &'static str,
    pub key: String,
    pub source: sqlx::Error,
}

// ── REQ-033 · 「删旧行」与「版本行 UPDATE」的**唯一**实现 ──────────
//
// REQ-012 把五栏 INSERT 收成了唯一一份，但当时 BA 的裁定范围只到「那 5 段
// INSERT」，于是同一条落库链上的另外两步仍各有两份 / 三份抄写。
// 本节把这两步也收口，是 REQ-011 那个病根的**剩余部分**。
//
// 三步落库链的归属，现在是这样：
//
// | 步骤 | 唯一实现 | 状态 |
// |------|----------|------|
// | ① 冻结闸门 | 只在 [`PgStore::save_snapshot`] | **刻意不共享**（见下） |
// | ② 删旧行   | [`delete_snapshot_rows`] | REQ-033 收口 |
// | ③ 写新行   | [`insert_snapshot_rows`] | REQ-012 已收口 |
// | ④ 版本行   | [`update_version_extracted_by`] | REQ-033 收口 |

/// 五张快照表清空的**唯一**顺序。
///
/// 顺序有讲究（先删 `link_types`），所以这份清单全仓只存这一处 ——
/// 循环体在 [`delete_snapshot_rows`]。
const SNAPSHOT_TABLES: [&str; 5] = [
    "link_types",
    "object_types",
    "action_types",
    "functions",
    "concept_groups",
];

/// REQ-033 · **删旧行**的唯一实现。
///
/// # 为什么这一步也必须只有一份
///
/// REQ-011 的病因不是「INSERT 写错了」，是**同一条链被抄了两份**：
/// [`PgStore::save_snapshot`] 与 `db::apply::write_snapshot_in` 各抄一份
/// 五表 DELETE 循环，抄漏的那一份就等于「一栏表被清空却什么都不写回来」。
/// REQ-012 治好了 INSERT 那一半，DELETE 这一半原样留着 —— 那等于
/// 把同一个病根换了个地方再种一次。
///
/// 现在五张表的清单只在本模块存一份。谁漏表、漏顺序、漏一处同步，
/// 编译不过或结构判据变红。
///
/// # 顺序：为什么先删 `link_types`
///
/// 它按 `key` 引用两端（存的是 key 不是 id），**不受外键约束保护** ——
/// 先删它，后面的表删空时不会留下悬空引用行。
/// `concept_groups` 排最后：它没有外键指向 `object_types`，
/// 概念组是独立的展示层声明，删旧行的顺序不牵涉红线。
///
/// # ⚠️ 冻结闸门**不在**这里，这是刻意的
///
/// 本函数**只删**：不做任何准入判断，也不自己开事务 —— 它拿的是调用方
/// 已经 `begin()` 的那个 `&mut Transaction`。冻结闸门（AC-006）在
/// [`PgStore::save_snapshot`] 里、**本函数的调用之前**。
/// 谁把调用点提到闸门之前，就是在冻结版本上删数据；
/// `test_freeze_gate_precedes_delete_and_delete_covers_five_tables` 就是钉这一点的。
pub(crate) async fn delete_snapshot_rows(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    version_id: Uuid,
) -> Result<(), sqlx::Error> {
    for t in SNAPSHOT_TABLES {
        sqlx::query(&format!("DELETE FROM {t} WHERE version_id = $1"))
            .bind(version_id)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

/// 版本行 `extracted_by` 更新的 SQL —— **全仓唯一**一份字面量。
const UPDATE_VERSION_EXTRACTED_BY_SQL: &str =
    "UPDATE ontology_versions SET extracted_by = $2 WHERE id = $1";

/// REQ-033 · 版本行 `extracted_by` 更新的**唯一**构造点（原本有 3 份）。
///
/// # 为什么返回 query 而不是写成 `async fn`
///
/// 三个调用点的差别**只在「拿哪个 executor 执行」**：REQ-038 起三者
/// **全在事务里**（`save_snapshot` / `write_snapshot_in` / `set_extracted_by`
/// —— 最后一个原本直接打在 pool 上，但它要带冻结闸，就一并收进事务）。
/// SQL 字面量、参数顺序、`None → SQL NULL` 的序列化三者全同，
/// 所以把它们收进这一个函数，加一列时就不存在「忘了同步另外两处」。
///
/// ⚠️ **刻意不写成 `impl Executor` 泛型**：sqlx 0.8 下
/// `&mut PgConnection` 的 `Executor<'c>` 是 HRTB，会让 future **丢掉 `Send`**
/// （同一个坑见 [`crate::db::pg::migrate`] 的注释，那里已经踩过一次）。
/// 返回 query 让每个调用点自己 `execute`，`Send` 与事务边界都不变。
///
/// 语义（ADR-001）：`None` = 人工修订，落成 SQL NULL。
/// **不继承 base、不推断** —— 继承在这里根本没有代码路径可走。
pub(crate) fn update_version_extracted_by(
    version_id: Uuid,
    v: &Option<ExtractedBy>,
) -> sqlx::query::Query<'static, sqlx::Postgres, sqlx::postgres::PgArguments> {
    sqlx::query(UPDATE_VERSION_EXTRACTED_BY_SQL)
        .bind(version_id)
        .bind(v.as_ref().and_then(|e| serde_json::to_value(e).ok()))
}

// ── 辅助 ────────────────────────────────────────────────

fn map_key_conflict(err: sqlx::Error, key: &str) -> StoreError {
    if let sqlx::Error::Database(ref d) = err {
        // 23505 = unique_violation
        if d.code().as_deref() == Some("23505") {
            return StoreError::KeyConflict(key.to_string());
        }
    }
    StoreError::Db(err)
}

// REQ-011 · `json_of` / `evidence_json` 对 `db::apply` 也开放。
// apply 的落库路径要写同样几张表，序列化必须**同一份实现** ——
// 两处各写一遍就是下一次分叉的起点（REQ-011 的根因）。
pub(crate) fn json_of<T: serde::Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Array(vec![]))
}

fn properties_json(e: &EntityType) -> Value {
    serde_json::to_value(&e.properties).unwrap_or(Value::Array(vec![]))
}

pub(crate) fn evidence_json(e: &[crate::domain::snapshot::Evidence]) -> Value {
    serde_json::to_value(e).unwrap_or(Value::Array(vec![]))
}

/// JSON 数组 → `Vec<String>`。空/null 都给空 vec。
fn from_json(v: Value) -> Vec<String> {
    serde_json::from_value(v).unwrap_or_default()
}

//! **建模轨投影（FDE 视角）** —— 纯逻辑，**零 I/O**。
//!
//! 规格 §8.3 拆双轨：**客户没能力判断建模质量**（主键、基数、规范化），
//! 让他审只会随便点「通过」。于是业务轨给客户看、**建模轨给 FDE 看** ——
//! 本模块就是**建模轨的那个投影**。
//!
//! # 为什么「再加一个投影」就是全部缺的那一环
//!
//! 双轨状态机（[`crate::domain::review`] 的 `Track` / `apply_decision`）
//! 早就在按轨分别写 `biz_status` / `model_status`，而**全仓只有一个投影**
//! —— `crate::domain::view::project` 的客户视图。
//! 于是状态机让 FDE 逐项裁决建模轨，**FDE 却拿不到建模轨的数据**。
//! 缺的不是判据、不是数据，是**第三个投影**。
//!
//! # 与客户视图的分离：靠「类型不同 + 通道不同」，不靠运行时开关
//!
//! | | 客户视图 | 建模轨（本模块） |
//! | --- | --- | --- |
//! | 输出类型 | [`crate::domain::view::CustomerView`] | [`ModelTrackView`] |
//! | 函数 | [`crate::domain::view::project`] | [`project_model_track`] |
//! | 术语守门 | **过** [`crate::domain::view::scan_modeling_terms`]（命中即拒绝发布）| **不过** —— 建模术语在这里是**要被看见的内容**，不是泄露 |
//! | 消费方 | 客户 / 判读手册 / HTML | FDE 裁决建模质量 |
//!
//! **两个输出类型互不相干**，建模轨那一份**从不经过**客户视图的守门器 ——
//! 所以「客户视图不许出现建模术语」这条不变量**没有被本模块放宽一分**：
//! 它压根不参与那条路径。
//!
//! # 「刻意保留」的清单 —— 正是客户视图逐条注明丢弃的那些
//!
//! `key` / `evidence` / `column` / `cardinality` / `disjoint_with` /
//! `extracted_by` / `action_types` / `functions`。
//! 客户视图丢它们是对的（客户不认 `robot_device`）；建模轨留它们也是对的
//! （FDE 判断的正是这些）。**同一份数据，两种取舍** —— 缺的是第二个取舍。
//!
//! # 本模块**不**做什么
//!
//! - **不碰抽取逻辑** —— 产物 sha 应当不变；
//! - **不伪造快照层没有的东西** —— 例如快照层 `Property` 没有独立的 `pk` 位，
//!   本模块就**不给** `pk` 字段，而是把 `column` 原样带出（见 [`ModelPropertyView`]）；
//! - **不引入任何 I/O** —— 领域层零 I/O（AC-002）。

use serde::{Deserialize, Serialize};

use crate::domain::snapshot::{ActionType, Evidence, ExtractedBy, Function, Snapshot};

/// 建模轨视图 —— **FDE 逐项裁决建模质量时看的那一份**。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelTrackView {
    /// 第几版（对应快照的 `seq`）—— 裁决要能指明「我裁的是哪一版」
    pub seq: i32,
    /// **抽取归属**（§8.4）：谁产的、哪一版工具、哪一轮、哪些分组失败了。
    ///
    /// 三态语义与快照层一致（`None` = 旧数据 / 人工修订，**不猜**）——
    /// 见 [`crate::domain::snapshot::ExtractedBy`]。
    pub extracted_by: Option<ExtractedBy>,
    pub entity_types: Vec<ModelEntityView>,
    pub link_types: Vec<ModelLinkView>,
    /// **切片 1 恒空，但结构必须在**（§5.5 占位）—— 与快照层同一条理由：
    /// 「只建不用」的东西最容易被后人当垃圾清理掉，而它是切片 2 的地基。
    pub action_types: Vec<ActionType>,
    /// 同上（§5.5 · Function 占位）。
    pub functions: Vec<Function>,
}

/// 建模轨里的一个对象类型。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelEntityView {
    /// **系统标识** —— 客户视图刻意丢弃这一栏；FDE 判断「两条提议是不是同一个东西」靠它。
    pub key: String,
    pub display_name: String,
    pub description: Option<String>,
    pub aliases: Vec<String>,
    /// **父类的 key** —— 建模轨判断继承/规范化时看它。
    pub parent: Option<String>,
    /// **与之互斥的类型 key** —— 建模轨判断「这两个类型会不会同时成立」的唯一依据。
    pub disjoint_with: Vec<String>,
    pub properties: Vec<ModelPropertyView>,
    /// **证据** —— 审计用。客户视图明确丢弃（「不是给人看的」），FDE 恰恰要靠它复核。
    pub evidence: Vec<Evidence>,
}

/// 建模轨里的一个属性。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelPropertyView {
    pub name: String,
    /// **列映射** —— 建模术语本体。
    ///
    /// ⚠️ **本模块不给 `pk` 字段**：快照层的 `Property` **没有独立的 `pk` 位**
    /// （`src/domain/snapshot.rs`），凭空造一个只会给 FDE 一个假的确定性信号。
    /// FDE 判主键能拿到的全部就是这两样：`column` 本身，以及下面的 `nullable`。
    /// 真要一张显式的主键声明，那是**补数据**（另一个需求），不是投影能变出来的。
    pub column: String,
    /// 是否可空 —— 与 `column` 合看才能推断「哪一列大概率是主键」。
    pub nullable: bool,
    pub note: Option<String>,
    pub sample: Option<String>,
}

/// 建模轨里的一个关系类型。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelLinkView {
    pub key: String,
    pub from_type: String,
    pub to_type: String,
    /// **关系基数** —— 客户视图把它翻成人话（`1:N` → 「一对多」），
    /// 建模轨**保留原记号**：基数本身就是 FDE 要裁决的对象，翻掉就没有可裁决的东西了。
    pub cardinality: String,
    /// **关系语义** —— 同样保留原记号（`has` / `belongs_to` / …）。
    pub semantic: String,
    pub evidence: Vec<Evidence>,
}

/// 把内部快照投影成**建模轨视图**。
///
/// 与 [`crate::domain::view::project`] 是**两个并列的投影**，不是一个的开关：
/// 输入同一份 [`Snapshot`]，输出两种取舍，**互不调用**。
///
/// # 为什么不入参收窄成一份新的窄视图
///
/// `crate::domain::view::SnapshotView` 只声明了客户视图用得到的字段 ——
/// 它装不下 `extracted_by` / `action_types` / `functions`。
/// 照着它再造一份「建模轨版窄视图」等于把 [`Snapshot`] 近乎镜像一遍，
/// 而 `view.rs` 自己就告诫过「不是 Snapshot 的镜像，避免契约一改投影跟着改」。
/// 这里直接取 `&Snapshot`（`domain::diff::diff_snapshots` 同样这么做），
/// 把取舍**只做在输出这一侧**。
pub fn project_model_track(snapshot: &Snapshot) -> ModelTrackView {
    ModelTrackView {
        seq: snapshot.seq,
        extracted_by: snapshot.extracted_by.clone(),
        entity_types: snapshot
            .entity_types
            .iter()
            .map(|e| ModelEntityView {
                key: e.key.clone(),
                display_name: e.display_name.clone(),
                description: e.description.clone(),
                aliases: e.aliases.clone(),
                parent: e.parent.clone(),
                disjoint_with: e.disjoint_with.clone(),
                properties: e
                    .properties
                    .iter()
                    .map(|p| ModelPropertyView {
                        name: p.name.clone(),
                        column: p.column.clone(),
                        nullable: p.nullable,
                        note: p.note.clone(),
                        sample: p.sample.clone(),
                    })
                    .collect(),
                evidence: e.evidence.clone(),
            })
            .collect(),
        link_types: snapshot
            .link_types
            .iter()
            .map(|l| ModelLinkView {
                key: l.key.clone(),
                from_type: l.from_type.clone(),
                to_type: l.to_type.clone(),
                cardinality: l.cardinality.clone(),
                semantic: l.semantic.clone(),
                evidence: l.evidence.clone(),
            })
            .collect(),
        action_types: snapshot.action_types.clone(),
        functions: snapshot.functions.clone(),
    }
}

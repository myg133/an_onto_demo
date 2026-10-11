//! 表名→中文名翻译表（REQ-003 · AC-104 的前提）
//!
//! # 为什么需要它
//!
//! 真实源库 `<业务库名>` 的实测：**91 张表里只有 3 张有表注释**。
//! 而 REQ-002 的 `usable_as_human_name` 规定「词元命中任一实体 key 的值不可用」，
//! 于是把英文表名当 `display_name` 会让实体**整批消失** ——
//! REQ-002 评估里的「场景 A：91 张表存活 0（0%）」。
//!
//! 所以抽取器必须为每张表准备中文名。这份翻译**由 LLM 产出、随代码入库、
//! 带 agent 与版本标记** —— 正是 REQ-002 的 AC-011/AC-014 想解决的问题：
//! 「谁在哪个版本翻的」，否则供应商悄悄升模型后 diff 变化无法解释。
//!
//! # 维护方式
//!
//! 翻译表是**知识资产**，不是代码逻辑。改动流程：
//! 1. 改 `table_names.json`
//! 2. **必须**同时更新 `version`（否则 diff 说不清是哪次翻译变��的）
//! 3. 重跑 `ontok-propose`，diff 提案看变化是否可解释


use anyhow::Context;
use serde::{Deserialize, Serialize};

/// 翻译表本体。对应 `table_names.json`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableNames {
    /// 产出这张表的 agent 标识。
    pub agent: String,
    /// **具体版本**。改翻译就改它 —— 与 REQ-002 AC-011 同一原则。
    pub version: String,
    pub generated_at: String,
    /// 源库标识，便于追溯是哪套 schema。
    pub source: String,
    /// 表名 → 中文显示名。
    pub names: std::collections::HashMap<String, String>,
}

impl TableNames {
    /// 载入并做基本校验。
    ///
    /// 校验点：中文名不能为空、不能等于表名本身（等于就等于没翻，
    /// 会被 `usable_as_human_name` 当成退化值过滤）。
    pub fn load(json: &str) -> anyhow::Result<Self> {
        let tn: Self = serde_json::from_str(json).context("翻译表 JSON 解析失败")?;
        if tn.names.is_empty() {
            anyhow::bail!("翻译表是空的");
        }
        for (k, v) in &tn.names {
            if v.trim().is_empty() {
                anyhow::bail!("表 {k} 的中文名为空 —— 等于没翻，会被 usable_as_human_name 过滤");
            }
            if v.trim().to_lowercase() == k.to_lowercase() {
                anyhow::bail!("表 {k} 的中文名与表名相同 —— 会被判定为退化值而丢弃");
            }
        }
        Ok(tn)
    }

    /// 查一个表的中文名。查不到返回 `None` ——
    /// **这是显式的失败**：调用方会记进 `tables_without_cn_name`，
    /// E2E 报告能数出「有几张表没翻出来」，而不是静默丢在客户视图里。
    pub fn lookup(&self, table: &str) -> Option<&str> {
        self.names.get(table).map(|s| s.as_str())
    }

    /// 覆盖率 —— 翻译表落后于源库 schema 时会掉下来。
    pub fn coverage(&self, tables: &[String]) -> (usize, usize) {
        let hit = tables.iter().filter(|t| self.lookup(t).is_some()).count();
        (hit, tables.len())
    }

    /// **全部中文显示名**的集合 —— 供「这句话说的是不是一个我们叫得出名字的对象」判定。
    ///
    /// REQ-008：判断一条列注释是不是**只是在复述这个外键**时要用
    /// （`客户标识` 说的就是「客户」，`子账号ID` 说的是另一个主体）。
    pub fn display_name_set(&self) -> std::collections::BTreeSet<String> {
        self.names.values().map(|v| v.trim().to_string()).collect()
    }
}

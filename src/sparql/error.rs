//! REQ-122 · 查询错误 —— **可读 + 可行动**（R1 / R2 / R3 的共同要求）。
//!
//! 数字员工拿不到栈，只能读一句话。所以本类型的每一条消息都必须回答三件事：
//! **错在哪**（点名到 表.列 / 属性 IRI / 字符位置）、**为什么不支持**（不是「非法」二字）、
//! **下一步怎么办**（改哪一行声明 / 用哪条命令查可用值）。

use std::fmt;

/// 查询失败。只有一句话 —— 那句话就是全部信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryError {
    message: String,
}

impl QueryError {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }

    /// 带**字符位置**的错误（位置从 1 数起 —— 人不是从 0 数的）。
    pub fn at(message: &str, pos: usize) -> Self {
        Self { message: format!("{message}（位置 {}）", pos + 1) }
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn into_message(self) -> String {
        self.message
    }
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for QueryError {}

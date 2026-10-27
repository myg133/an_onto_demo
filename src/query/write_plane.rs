//! ⭐ REQ-159 P1c-1 · **写面在 `query serve` 里的进程级状态**。
//!
//! # 为什么写面要自己一份状态，而不是塞进 `QueryService`
//!
//! `QueryService` 是**一份快照**的投影（模型 / 目录 / 两视图 TTL / R2RML），它随
//! `version_id` 变、被 `multi::MultiProject` 按版本缓存。写面要的三样东西却都是
//! **进程级、与版本无关**的：
//!
//! | 依赖 | 是什么 | 为什么不能进缓存 |
//! | --- | --- | --- |
//! | `pool` | **元数据库**连接池 | 写的是 projects / versions / keys / DSN 密文，不是某一版本的服务内容 |
//! | `cipher` | [`SecretCipher`]（`ONTOK_SECRET_KEY`） | 密钥随快照缓存多份 = 多 N 份明文密钥常驻（⛔ 没有理由） |
//! | `audit` | 控制面审计（[`AuditLog`]） | 一个进程**一份**审计，旧版本对象不该各写一条 |
//!
//! 于是它被单独构造一次、由 `build_router*` 装进请求扩展（见
//! [`crate::query::http::inject_write_plane`]）。⛔ 缓存里装的东西仍然只有
//! `Arc<QueryService>`（P1b 的性质不变：**缓存里没有 DSN、没有 token**）。
//!
//! # 密钥：缺了就**拒绝启动**，而且在**绑 socket 之前**
//!
//! 判据（AC-c4）：`query serve` 在没有 `ONTOK_SECRET_KEY` 时必须退出，并且
//! ① 说完「缺什么」② 说完「怎么补」③ 说完「换密钥的后果」④ ⛔ 不许出现「就绪」横幅。
//! 位置：`serve` / `multi::serve` 里读 token 表**之后**、绑定地址**之前**
//! （token 表自身的错先报 —— 那是同一张表的两件事，先报更基础的那件）。

use sqlx::postgres::PgPool;

use super::audit::AuditSink;
use crate::console::audit::AuditLog;
use crate::provision::dsn::{SecretCipher, SECRET_KEY_ENV};
use crate::console::provision_api::Deps;

/// 写面的进程级依赖（见模块头）。
pub struct WritePlane {
    /// **元数据库**（⛔ 不是客户源库：写面只写元数据）。
    pool: PgPool,
    /// DSN 与 query key 的加密列。
    cipher: SecretCipher,
    /// 控制面审计（行形状与 console 同一份，落点走同一个 [`AuditSink`]）。
    audit: AuditLog,
}

impl std::fmt::Debug for WritePlane {
    /// ⛔ 不打印连接串 / 密钥（`SecretCipher` 自己也是 `<redacted>`，这里再兜一层）。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WritePlane").finish_non_exhaustive()
    }
}

impl WritePlane {
    /// 组装写面。`sink` 与查询面**共用**同一个审计落点（一个进程一份审计）。
    pub fn new(pool: PgPool, cipher: SecretCipher, sink: AuditSink) -> Self {
        Self {
            pool,
            cipher,
            audit: AuditLog::new(sink),
        }
    }

    /// 借用成 8 条写端点要的形状（[`Deps`]）。
    pub fn deps(&self) -> Deps<'_> {
        Deps {
            pool: &self.pool,
            cipher: &self.cipher,
            audit: &self.audit,
        }
    }

    /// 审计（判据用：证明「每个写操作一行」）。
    pub fn audit(&self) -> &AuditLog {
        &self.audit
    }
}

/// 读密钥（**纯函数内核**：判据不去改进程环境，见 `provision::dsn::SecretCipher::from_lookup`）。
///
/// 返回 `anyhow::Result` 而不是 `DsnError`：调用点是启动闸门，要把「为什么拒绝启动」
/// 与「怎么补」写成一段人话（`center` 的错误契约是面向调用方的，这里面向运维）。
pub fn make_cipher<F>(lookup: F) -> anyhow::Result<SecretCipher>
where
    F: Fn(&str) -> Option<String>,
{
    SecretCipher::from_lookup(lookup).map_err(|e| {
        anyhow::anyhow!(
            "缺 {SECRET_KEY_ENV}，拒绝启动 `ontok query serve`：\n{e}\n  \
             ⚠️ 从 REQ-159 P1c-1 起，`query serve` 承载**写面**（建项目 / 上传构建包 / \
             解析 / 激活 / 查看 key，Studio 调）—— 那 8 条要把 **DSN 与 query key** \
             加密后落库，所以这个进程必须有那把密钥。\n  \
             设置方式（当前会话）：\n    \
             PowerShell : $env:{SECRET_KEY_ENV} = '<64 位十六进制>'\n    \
             bash/zsh   : export {SECRET_KEY_ENV}=<64 位十六进制>\n  \
             生成一个：`openssl rand -hex 32`。\n  \
             ⚠️ 它与 `ontok console serve` 用的**是同一把**：只在一边设过、另一边换过，\
             那么 `GET /api/projects/:p/key` 解出来的密文会对不上（v1 不做密钥轮换）。\n  \
             ⛔ 没有「先跳过后补」的开关：带着一把坏密钥起来，等于让写端点在一个**会失败**\
             的状态下对外可用（用户传完包、点开 DSN 弹窗才收到 500）。"
        )
    })
}

/// 生产路径：从环境变量读（[`SECRET_KEY_ENV`] 与兼容名）。
pub fn load_secret_cipher() -> anyhow::Result<SecretCipher> {
    make_cipher(|name| std::env::var(name).ok())
}

// ══════════════════════════════════════════════════════════════
// 判据（纯函数：不连库、不绑 socket）
// ══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    /// **AC-c4** · 两个名字都没设 ⇒ 拒绝启动，且话里四件事都在：
    /// ① 缺的是哪个环境变量 ② 怎么补（含生成命令）③ 换密钥的后果 ④ ⛔ 没有「跳过」开关。
    ///
    /// 变异：把 `make_cipher` 改成「缺了就返回一把默认密钥」⇒ 本测试红
    /// （那正是「默认密钥等于没有加密」）。
    #[test]
    fn a_missing_secret_key_refuses_to_start_with_an_actionable_message() {
        let e = make_cipher(|_| None).expect_err("没有密钥必须拒绝启动");
        let text = e.to_string();
        for needle in [
            "ONTOK_SECRET_KEY",
            "拒绝启动",
            "openssl rand -hex 32",
            "同一把",
            "没有「先跳过后补」的开关",
            "写面",
        ] {
            assert!(text.contains(needle), "缺 {needle:?}：\n{text}");
        }
        // ⛔ 不许在这段话里出现「就绪」——启动还没走到那一步。
        assert!(!text.contains("就绪"), "{text}");
    }

    /// 两个名字设了**不同**的值 ⇒ 拒绝（与 `console serve` 同一条：不猜哪把对）。
    #[test]
    fn two_different_keys_are_refused_instead_of_guessed() {
        let e = make_cipher(|name| match name {
            SECRET_KEY_ENV => Some("11".repeat(32)),
            crate::provision::dsn::LEGACY_SECRET_KEY_ENV => Some("22".repeat(32)),
            _ => None,
        })
        .expect_err("两把不同的密钥必须拒绝");
        assert!(e.to_string().contains("值不一样"), "{e}");
    }

    /// 64 位十六进制 ⇒ 通过；形状不对 ⇒ 拒绝（转手的是 `SecretCipher::from_hex` 的判据，
    /// 这里只钉「启动闸门没有把它吞掉」）。
    #[test]
    fn a_well_formed_key_builds_and_a_short_one_does_not() {
        let ok = make_cipher(|name| (name == SECRET_KEY_ENV).then(|| "ab".repeat(32)));
        assert!(ok.is_ok(), "{:?}", ok.err().map(|e| e.to_string()));
        let bad = make_cipher(|name| (name == SECRET_KEY_ENV).then(|| "abcd".to_string()));
        assert!(bad.is_err(), "32 位十六进制不是 32 字节");
    }
}

//! REQ-126 · 门户的**进程内并发闸** —— 每张口令一个名额池，拿不到就 **429**，不排队。
//!
//! # 为什么门户也需要它（与 `query serve` 保持一致）
//!
//! 门户的场景是「客户点一张卡片」，但**风险模型不是场景**：
//! 同一张口令可以被脚本复用 —— 一个人点卡片 ≠ 一个客户端只发一个请求。
//! `query serve`（REQ-123）早就有每 token 的并发上限（`tokens.json` 的
//! `max_concurrency`）；门户比它弱，就是一条**可被免费放大的**取数通道：
//! 一句 `for i in $(seq 1 200); do curl … & done` 就能把源库的只读连接池（2 条）
//! 堵满，让**别的客户的页面**排队等 `acquire_timeout`。
//!
//! # 四条硬性质（判据见文末单测与 `tests/test_req126_portal.rs`）
//!
//! | # | 性质 | 为什么 |
//! | --- | --- | --- |
//! | 1 | 上限就是**口令里那一行**的 `max_concurrency` | 调用方抬不高（请求里没有这个字段） |
//! | 2 | 第 N+1 个**立刻** 429，⛔ 不排队等超时 | 「等 10 秒再告诉你太忙」比直接说更糟 |
//! | 3 | 名额按 token 隔离 | A 打满不影响 B（不同客户互不拖累） |
//! | 4 | `Drop` 归还（含 panic） | 泄漏一个名额 = 那张口令永久少一个并发 |
//!
//! # 与 REQ-123 的 `Limiter` 是什么关系
//!
//! 同一套算法（`Mutex<HashMap<digest, usize>>` + `Drop` 归还），但**不是同一个类型**：
//! 那一份还管着 SSE 连接与全局上限（对齐源库连接池），门户没有 SSE、也没有
//! 「全局上限」这个口径（并发总量的天然上界 = 口令数 × 各自上限，是运维写死的）。
//! 把两者并成一个类型，会让门户顺手获得一组它**不该有**的开关。
//!
//! # 已知边界（⛔ 不写成「已封堵」）
//!
//! - 计数器是**进程内**的：多副本部署时每个副本各算一份（要全局口径得上 Redis /
//!   PG advisory lock，那是另一条需求）；
//! - 只卡「同时几个查询在跑」，**不卡速率**（每秒多少次是另一件事）；
//! - 中毒的锁不会让服务停摆（取回内部值继续）：计数器的语义是「尽力」。

use std::collections::HashMap;
use std::sync::Mutex;

/// 每 token 的名额池。
#[derive(Debug, Default)]
pub struct Limiter {
    inflight: Mutex<HashMap<String, usize>>,
}

/// 一次占用的凭据。`Drop` 负责归还（panic 也不泄漏名额）。
#[derive(Debug)]
pub struct Permit<'a> {
    limiter: &'a Limiter,
    key: String,
}

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        self.limiter.release(&self.key);
    }
}

/// 中毒的锁不该让服务停摆：计数器的语义是「尽力」，取回内部值继续。
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match m.lock() {
        Ok(g) => g,
        Err(e) => e.into_inner(),
    }
}

impl Limiter {
    /// 占一个名额；**占不到就是 `None`**（⛔ 不等待、不排队）。
    ///
    /// `limit` 来自口令那一行（启动时已校验 ≥1）；传 0 等于「一个都不许」。
    pub fn acquire(&self, key: &str, limit: usize) -> Option<Permit<'_>> {
        if limit == 0 {
            return None;
        }
        let mut m = lock(&self.inflight);
        let used = m.get(key).copied().unwrap_or(0);
        if used >= limit {
            return None;
        }
        m.insert(key.to_string(), used + 1);
        Some(Permit {
            limiter: self,
            key: key.to_string(),
        })
    }

    /// 归还一个名额（`Permit::drop` 调它）。计数归零时**删掉键**：
    /// 不删的话，长跑的服务会为每张注销过的口令留一个 0（与 REQ-123 同一处理）。
    fn release(&self, key: &str) {
        let mut m = lock(&self.inflight);
        match m.get_mut(key) {
            Some(n) if *n > 1 => *n -= 1,
            Some(_) => {
                m.remove(key);
            }
            None => {}
        }
    }

    /// 某张口令当前占了几个名额（诊断 / 判据用）。
    pub fn inflight(&self, key: &str) -> usize {
        lock(&self.inflight).get(key).copied().unwrap_or(0)
    }

    /// 表里还记着几个键（判据用：全部归还后必须是 0，否则说明名额泄漏）。
    pub fn tracked_keys(&self) -> usize {
        lock(&self.inflight).len()
    }
}

// ══════════════════════════════════════════════════════════════
// 判据
// ══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn n_permits_then_the_next_one_is_refused() {
        let l = Limiter::default();
        let a = l.acquire("tok-a", 2).expect("第 1 个名额");
        let b = l.acquire("tok-a", 2).expect("第 2 个名额");
        assert!(l.acquire("tok-a", 2).is_none(), "第 3 个必须被拒（N+1 ⇒ 429）");
        assert_eq!(l.inflight("tok-a"), 2);
        drop(a);
        assert_eq!(l.inflight("tok-a"), 1, "归还一个");
        assert!(l.acquire("tok-a", 2).is_some(), "腾出来的名额可以再用");
        drop(b);
    }

    #[test]
    fn different_tokens_never_interfere() {
        let l = Limiter::default();
        let _a1 = l.acquire("tok-a", 1).expect("A 的名额");
        assert!(l.acquire("tok-a", 1).is_none(), "A 已经满了");
        let _b = l.acquire("tok-b", 1).expect("A 打满不该影响 B");
        assert_eq!(l.inflight("tok-a"), 1);
        assert_eq!(l.inflight("tok-b"), 1);
    }

    #[test]
    fn dropping_every_permit_leaves_no_kept_keys() {
        let l = Limiter::default();
        {
            let _a = l.acquire("tok-a", 3).unwrap();
            let _b = l.acquire("tok-a", 3).unwrap();
            let _c = l.acquire("tok-b", 3).unwrap();
            assert_eq!(l.tracked_keys(), 2);
        }
        assert_eq!(l.tracked_keys(), 0, "全部归还后不许留下 0 计数的键");
    }

    #[test]
    fn a_zero_limit_is_closed_not_open() {
        let l = Limiter::default();
        assert!(l.acquire("tok-a", 0).is_none(), "0 等于「一个都不许」，⛔ 不是「不限」");
    }

    #[test]
    fn a_panicking_holder_does_not_leak_the_slot() {
        let l = Limiter::default();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _p = l.acquire("tok-a", 1).expect("名额");
            panic!("持有者炸了");
        }));
        assert!(r.is_err());
        assert_eq!(l.inflight("tok-a"), 0, "panic 也必须归还名额（RAII）");
        assert!(l.acquire("tok-a", 1).is_some(), "名额必须还能再拿");
    }

    #[test]
    fn the_limit_is_read_per_call_not_cached() {
        // 上限是**每次调用时**传进来的（口令那一行）；换一个上限立刻生效。
        let l = Limiter::default();
        let p = l.acquire("tok-a", 1).unwrap();
        assert!(l.acquire("tok-a", 1).is_none());
        assert!(l.acquire("tok-a", 2).is_some(), "上限 2 时同一个池还能再拿一个");
        drop(p);
    }
}

//! 破损管道（EPIPE）的 panic hook —— **两个 bin 共用的一份实现**（REQ-152）。
//!
//! # 为什么它不在 `cli.rs` 里了
//!
//! 原来这个 hook 只装在 `ontok`（`cli::run` 的第一行）。REQ-152 把 CLI 拆成
//! 服务面（`ontok`）与构建面（`ontok-builder`）之后，**两个 bin 都要装它** ——
//! 而服务面绝不能引用 `crate::cli`（那会把执行器链进 `ontok`，见 `src/service.rs`
//! 模块头）。所以实现搬到这里，`cli::install_broken_pipe_guard` 保留为**转发**：
//! 名字与调用点不变（`tests/test_req107_source_enumeration.rs` 的出口函数登记表
//! 仍对得上），实现只有一份。
//!
//! **为什么必须装**：`println!` 遇到 broken pipe 会 panic，
//! 而那个 panic 发生在**子函数里的 println**，`run()` 的 `Result` 收不到 ——
//! 只在 `run` 里 try/catch 是抓不住的。唯一能统一处理的地方是 panic hook。
//!
//! 场景：`ontok ontology diff ... | head -2`。
//! 用户只想看前几行，却看到一屏 std::io 栈，会误判「程序有 bug」。

/// 装一个忽略 EPIPE 的 panic hook（幂等：重复装只是再包一层，行为不变）。
pub fn install_broken_pipe_guard() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let msg = info.to_string();
        // payload 才是真正的原因；info 的 Display 未必带
        let payload = info.payload();
        if let Some(s) = payload.downcast_ref::<&str>() {
            if s.contains("Broken pipe") || s.contains("os error 32") {
                return; // 静默退出
            }
        }
        if let Some(s) = payload.downcast_ref::<String>() {
            if s.contains("Broken pipe") || s.contains("os error 32") {
                return;
            }
        }
        if msg.contains("Broken pipe") || msg.contains("os error 32") {
            return;
        }
        prev(info);
    }));
}

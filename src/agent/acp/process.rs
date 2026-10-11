//! **ACP 子进程的生命周期** —— 拉起 `mcode acp`、握手、跑一轮 prompt、关掉。
//!
//! # ★ 为什么这一层**完全不用 tokio**（REQ-023 的同步/异步裁定）
//!
//! 事实链（BA 逐行核实过）：
//!
//! | 事实 | 位置 |
//! | --- | --- |
//! | `ontok-propose` 是 `#[tokio::main] async fn main()` | `src/bin/ontok-propose.rs:39` |
//! | `orchestrate(...)` 在 **async main 内部**被调用 | `src/bin/ontok-propose.rs:229` |
//! | `trait ExtractionAgent` 的四个方法**全是同步** | `src/agent/mod.rs:310-364` |
//!
//! 于是 ADR-002 的 N3 那条坑就在这儿：**同步方法在 async 上下文里 `block_on`
//! 会直接 panic**。要不就改 trait（那会让 REQ-013~022 整串返工），
//! 要不就别在那个上下文里 `block_on`。
//!
//! ## 本模块的答案：**一条专用线程 + 纯阻塞 I/O，一个 `block_on` 都不写**
//!
//! BA 给的两条路线各有一半是对的，这里把两半合起来：
//!
//! - **A 的机制**：子进程活在一根**专用线程**上，同步方法通过 channel 与它往返；
//! - **B 的 I/O**：`std::process` + `std::io`，**不碰 tokio 一根汗毛**。
//!
//! 为什么这个组合比「照 A 做」更好，有三条：
//!
//! 1. **A 自己也没能真的消灭 `block_on`。** A 要求在专用线程里起一个 tokio
//!    runtime，然后用 `block_on` 去驱动它 —— `block_on` 这个词照样出现在代码里，
//!    只是「不在已有 runtime 上下文里」。本方案**一个都不写**，
//!    于是那条不变量可以**用 grep 证明**，而不是靠读代码判断。
//! 2. **A 要补三个 tokio feature（`process` / `io-util` / `time`），
//!    换来的只是把同一段阻塞逻辑写成异步形式。** 协议是一条 stdio 上的
//!    严格串行请求/响应流，本来就没有并发可利用 —— 异步在这里是纯开销。
//!    不补 feature 就不用改 `Cargo.toml`，也就不用碰依赖。
//! 3. **专用线程解决的是「子进程管理」**：`extract_group` 会阻塞（ACP 一轮实测
//!    11.7 秒），而拉起进程 / stdio 读写 / 超时 / 收尾都搬到了**不是** runtime
//!    worker 的线程上 —— 不借 runtime 的手，于是不需要 runtime 上下文。
//!    ⚠️ **但它不解决「不阻塞调用方」**：这一轮往返期间**调用方线程是停摆的**
//!    （QA-A 实测：传输模拟 300ms 时，`extract_group` 占用**调用方线程** 300.55ms）。
//!    在 `#[tokio::main]` 里调进来，占住的就是那根 tokio worker —— 同 worker 上的
//!    其它 future 照样饿死。**这根线程买的不是「异步安全」，是「子进程不借 runtime」。**
//!
//! # 两条线程，各有一个说得清的理由
//!
//! | 线程 | 为什么必须有它 |
//! | --- | --- |
//! | **reader** | 超时。`BufRead::read_line` 一旦阻塞就无法打断，**只能**靠另一个线程把行递出来，调用方用 [`std::sync::mpsc::Receiver::recv_timeout`] 自己捏着截止时间 |
//! | **worker** | 子进程管理（拉起 / stdio / 超时 / 收尾）不借 runtime 的手。见上第 3 条 —— ⚠️ 它**不**解除调用方的阻塞 |
//!
//! 没有 reader 就没有超时：一个卡住的 `mcode acp` 会让 `ontok` **永远挂在那里**，
//! 而 `drive_group` 的重试逻辑会围着它空转。

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::collections::VecDeque;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::Mutex;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use serde_json::Value;

use super::protocol::{self, AgentInfo, Incoming, NewSessionResult, RpcRequest, TurnOutcome};

/// ACP 会话超时：**180 秒**。
///
/// 依据是实测的 11.7 秒（一次「只回两个字」的完整往返），留了十几倍余量。
/// 为什么要有上限而不是「一直等」：抽取一次真实分组要塞进整张表的 schema 与样本，
/// 比那句话重得多，但仍不该是分钟级；而一个**卡死**的子进程如果被无限等待，
/// 产物里就永远不会有「这一组失败」这一行 —— **静默的挂起比报错更难查**。
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(180);

/// 拉起 ACP 子进程需要的全部配置。
#[derive(Debug, Clone)]
pub struct AcpConfig {
    /// 可执行文件名或路径（`"mcode"` 走 `PATH`）。
    pub program: String,
    /// 参数（`["acp"]` —— 实测跑通的调用形式）。
    pub args: Vec<String>,
    /// `session/new` 的 `cwd`。**必须存在**（协议要求在那里起会话）。
    pub cwd: String,
    /// 单次请求的截止时间。
    pub timeout: Duration,
}

impl Default for AcpConfig {
    fn default() -> Self {
        Self {
            program: "mcode".to_string(),
            args: vec!["acp".to_string()],
            // ⚠️ 不是 `/tmp`，也不是 home：默认落在**当前工作目录**，
            // 让「会话建在哪」是调用方看得见、说得出的事。
            cwd: std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|e| format!("<拿不到当前目录：{e}>")),
            timeout: DEFAULT_TIMEOUT,
        }
    }
}

/// 握手的结果 —— `initialize` + `session/new` 两步的合并。
///
/// ⚠️ `session_id` 来自 `session/new`，而 `agent_info` 来自 `initialize`
/// （★ F-3：两者**分属不同的响应**）。把 `agentInfo` 从 `session/new`
/// 的 result 里读会得到空 —— 而空身份会让 §8.4 的溯源变成假的。
#[derive(Debug, Clone)]
pub struct Handshake {
    pub protocol_version: i64,
    pub agent_info: AgentInfo,
    pub agent_capabilities: Value,
    pub session: NewSessionResult,
}

/// **子进程生命周期的抽象** —— 让 adapter 能在不起真进程的情况下被测。
///
/// # 为什么需要这个 trait
///
/// 测试纪律要求**不许**真的去调 `mcode acp`：一次 11.7 秒、依赖登录态、
/// CI 上不可复现。若 adapter 直接持有 [`AcpProcess`]，那么 adapter 的每一处
/// 逻辑（累积、解析、用量记账、能力声明）都只能在真进程上测。
/// 有了这个 trait，`AcpAgent` 接受任意 `Box<dyn AcpTransport>`，
/// 测试喂一段**录好的** [`TurnOutcome`] 就能把整条路走通。
pub trait AcpTransport: Send {
    /// `initialize` + `session/new`。
    fn handshake(&mut self) -> Result<Handshake>;
    /// 跑一轮 `session/prompt`，返回**累积**后的正文 + 用量。
    fn prompt(&mut self, text: &str) -> Result<TurnOutcome>;
    /// 收掉子进程（幂等）。
    fn shutdown(&mut self);
}

// ──────────────────────────────────────────────────────────
// 真进程
// ──────────────────────────────────────────────────────────

/// **真的** `mcode acp` 子进程：stdin 写请求，stdout 读行。
pub struct AcpProcess {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    /// reader 线程递过来的行。见模块头「两条线程」。
    lines: Receiver<String>,
    reader: Option<JoinHandle<()>>,
    next_id: i64,
    handshake: Option<Handshake>,
    cfg: AcpConfig,
}

impl AcpProcess {
    /// 拉起子进程。**不**做握手 —— 握手要往返，单独一步更清楚。
    pub fn spawn(cfg: AcpConfig) -> Result<Self> {
        if !std::path::Path::new(&cfg.cwd).is_dir() {
            bail!(
                "ACP 会话目录 {:?} 不存在 —— session/new 要求 cwd 是一个真实目录",
                cfg.cwd
            );
        }
        let mut child = Command::new(&cfg.program)
            .args(&cfg.args)
            .current_dir(&cfg.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // ⚠️ stderr **必须接到 somewhere**：`Stdio::null()` 不做，
            // 直接继承父进程 —— 子进程往 stderr 写的诊断信息会混进
            // ontok 自己的输出，让 CLI 的 stdout 契约变得不可解析。
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| {
                format!(
                    "拉起 ACP 子进程 {:?} {:?} 失败 —— \
                     没有回落到任何别的实现（回落 = 溯源造假，见 factory 模块头）",
                    cfg.program, cfg.args
                )
            })?;

        let stdin = child
            .stdin
            .take()
            .context("子进程没有可用的 stdin（协议要靠它发请求）")?;
        let stdout = child
            .stdout
            .take()
            .context("子进程没有可用的 stdout（协议要靠它回消息）")?;

        let (tx, rx) = channel::<String>();
        let reader = std::thread::Builder::new()
            .name("acp-reader".into())
            .spawn(move || {
                let mut r = BufReader::new(stdout);
                loop {
                    let mut line = String::new();
                    match r.read_line(&mut line) {
                        Ok(0) => break, // EOF：子进程死了
                        Ok(_) => {
                            let t = line.trim();
                            if t.is_empty() {
                                continue;
                            }
                            // 发送失败只说明没人要了（worker 已退出），不是错误。
                            if tx.send(t.to_string()).is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
            })
            .context("ACP reader 线程起不来")?;

        Ok(Self {
            child: Some(child),
            stdin: Some(stdin),
            lines: rx,
            reader: Some(reader),
            next_id: 1,
            handshake: None,
            cfg,
        })
    }

    /// 发一条请求，**只**把 id 匹配的那条响应当成「结束了」。
    ///
    /// 途中遇到的通知全部累积（正文就在通知里，★ F-1）；
    /// 途中遇到的**过期**响应（id 对不上）丢掉并计数（超时重同步会撞上）。
    fn call(&mut self, method: &str, params: Value) -> Result<(protocol::PromptAccumulator, protocol::RpcResponse)> {
        let id = self.next_id;
        self.next_id += 1;
        let req = RpcRequest::new(id, method, params).encode_line()?;

        {
            let stdin = self
                .stdin
                .as_mut()
                .context("子进程 stdin 已关闭 —— 没法发请求（子进程是不是退了？）")?;
            stdin
                .write_all(req.as_bytes())
                .context("写 ACP 请求失败（子进程是不是崩了？）")?;
            stdin.flush().context("flush ACP 请求失败")?;
        }

        let deadline = Instant::now() + self.cfg.timeout;
        let mut acc = protocol::PromptAccumulator::new();
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                bail!(
                    "ACP 请求 {method:?}（id={id}）在 {:?} 内没有收到终响应。\n\
                     \n\
                     这一轮可能对端卡住了，也可能正文已经在通知里发完了但终响应没来。\n\
                     已收到 {notifications} 条通知 / {chunks} 条带文本的 chunk。\n\
                     \n\
                     为什么这里必须超时而不是一直等：无限等待会让「这一组失败」永远\
                     不会出现在 failed_groups 里 —— 一个卡死的子进程在产物里**不留任何痕迹**，\
                     那是比报错更难查的故障。",
                    self.cfg.timeout,
                    notifications = acc.notifications(),
                    chunks = acc.chunks(),
                );
            }
            let line = match self.lines.recv_timeout(remaining) {
                Ok(l) => l,
                Err(RecvTimeoutError::Timeout) => continue, // 上面的 deadline 会接手
                Err(RecvTimeoutError::Disconnected) => bail!(
                    "ACP 子进程的 stdout 提前关闭（reader 线程已退出）—— \
                     子进程多半崩了。已收到 {notifications} 条通知 / {chunks} 条带文本的 chunk。",
                    notifications = acc.notifications(),
                    chunks = acc.chunks(),
                ),
            };
            let msg = protocol::decode_line(&line)?;
            match msg {
                Incoming::Notification(_) => acc.absorb(&msg)?,
                Incoming::Response(r) => {
                    if r.matches_id(id) {
                        return Ok((acc, r));
                    }
                    // 过期响应：绝不能当成「这一轮结束了」。
                    acc.note_stale_response();
                }
            }
        }
    }

    /// 子进程是否还活着（诊断用）。
    pub fn is_running(&mut self) -> bool {
        self.child
            .as_mut()
            .is_some_and(|c| matches!(c.try_wait(), Ok(None)))
    }
}

impl AcpTransport for AcpProcess {
    fn handshake(&mut self) -> Result<Handshake> {
        if let Some(h) = &self.handshake {
            return Ok(h.clone());
        }
        let (_, resp) = self.call("initialize", protocol::initialize_params())?;
        let init = protocol::decode_initialize(&resp.into_result()?)?;

        let (_, resp) = self.call("session/new", protocol::new_session_params(&self.cfg.cwd))?;
        // ★ F-3：这一步的结果形状与上一步**不同**，类型也不同。
        let session = protocol::decode_new_session(&resp.into_result()?)?;

        let h = Handshake {
            protocol_version: init.protocol_version,
            agent_info: init.agent_info,
            agent_capabilities: init.agent_capabilities,
            session,
        };
        self.handshake = Some(h.clone());
        Ok(h)
    }

    fn prompt(&mut self, text: &str) -> Result<TurnOutcome> {
        let session_id = self
            .handshake
            .as_ref()
            .context("还没握手就发 prompt —— sessionId 只能从 session/new 的响应里拿")?
            .session
            .session_id
            .clone();
        let (acc, resp) = self.call("session/prompt", protocol::prompt_params(&session_id, text))?;
        // ★ 这一行里才有正文；`resp` 自己不带正文（见 protocol 模块的 F-1 段落）。
        acc.finish(resp)
    }

    fn shutdown(&mut self) {
        self.kill();
    }
}

impl AcpProcess {
    fn kill(&mut self) {
        // 先关 stdin：给对端一个「输入流结束了」的正常退出信号。
        self.stdin.take();
        if let Some(mut child) = self.child.take() {
            // 等一小会儿让它自己退；不退就杀。**不留孤儿进程** ——
            // 一个没人管的 `mcode acp` 会一直挂着，占着内存与文件锁。
            let deadline = Instant::now() + Duration::from_millis(500);
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) if Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    Ok(None) => {
                        let _ = child.kill();
                        let _ = child.wait();
                        break;
                    }
                    Err(_) => break,
                }
            }
        }
        // reader 线程会因 stdout EOF 自行退出。
        self.reader.take();
    }
}

impl Drop for AcpProcess {
    fn drop(&mut self) {
        self.kill();
    }
}

// ──────────────────────────────────────────────────────────
// 专用线程外壳 —— 同步方法拿到「不在 tokio worker 上」的阻塞
// ──────────────────────────────────────────────────────────

/// 专用线程上的工作指令。
enum Cmd {
    Handshake(Sender<Result<Handshake>>),
    Prompt {
        text: String,
        reply: Sender<Result<TurnOutcome>>,
    },
    Shutdown(Sender<()>),
}

/// [`AcpTransport`] 的**同步外壳** —— 子进程在专用线程上。
///
/// # 「为什么这根线程合法」的完整论证（ADR-002 · N3）
///
/// 调用链是 `#[tokio::main] async main` → `orchestrate` → `drive_group`
/// → `AcpAgent::extract_group`（**同步**）→ 本类型。
/// `extract_group` 必须阻塞到这一轮结束（ACP 一轮 11.7 秒）。
///
/// 危险在于**这个阻塞本身**：阻塞发生在 tokio 的 worker 线程上时，同一个 worker
/// 上的其它 future 就全部饿死；而 tokio 的多线程 runtime 只有有限个 worker，
/// 91 个组顺序跑下来只是把 runtime 占满 —— CLI 里没有别的活，
/// 于是「看起来能跑」，但任何后台任务（未来接的进度上报、健康检查）都会静默饿死。
///
/// 本类型只能拆掉其中一半，且必须说清是哪一半 ——
///
/// 1. **解决的是「子进程管理」**：子进程与它的读写**全部**在专用线程上，
///    那**不是** tokio worker，于是子进程这一侧**不占** runtime 的手；
/// 2. 本线程只做 [`Sender::send`] + [`Receiver::recv_timeout`] ——
///    纯 std 阻塞原语，**不涉及 runtime，不调用 `block_on`**；
/// 3. 因此**没有任何一处会在已有 runtime 上下文里 `block_on`** ——
///    这条不变量可以用一条 grep 证明（模块头承诺的就是这一点）。
///
/// # ⚠️ 代价：**调用方线程在整轮往返期间是阻塞的**
///
/// 实测（QA-A）：传输模拟 300ms 时，`extract_group` 占用**调用方线程** 300.55ms。
/// 「子进程不借 runtime」**不等于**「不阻塞调用方」—— 从 `#[tokio::main]` 的
/// async main 调进来，占住的就是那根 tokio worker，同 worker 上的其它 future
/// 照样饿死。上一节把「不占 tokio worker」列成本类型的功劳，是**说过头了**：
/// 不占的是子进程那一侧，**调用方该等还是等**。
///
/// 今天这条路径没人走：`build_agent` 的 `other` 分支对 `acp` 直接 `bail!`
/// （`src/agent/factory.rs:91`），adapter 未接线。接线那天若需要真正的
/// 「不阻塞 worker」，得改的是 trait 契约（异步化）或把调用整体挪出 async 上下文，
/// 不是靠这根线程。
pub struct ThreadTransport {
    tx: Option<Sender<Cmd>>,
    worker: Option<JoinHandle<()>>,
    handshake: Option<Handshake>,
    /// 一旦出错就**粘住**：JSON-RPC 是一条严格串行的流，
    /// 一次超时之后这一轮的响应可能已经在路上了，
    /// 继续往下发请求会让「上一轮的响应」与「这一轮的响应」**错位**。
    ///
    /// 与其猜哪一条是哪一个，不如**从此拒绝**，让 `drive_group` 的重试
    /// 显式失败、让人看见。宁可重跑整轮，也不要一份错位的抽取。
    poisoned: Option<String>,
    /// worker 线程的看门狗时限。
    ///
    /// 它**不是**协议超时（那个由 [`AcpConfig::timeout`] 在 worker 内部逐请求管），
    /// 而是「worker 线程本身死了/卡死了」的兜底。取协议超时的两倍再加 30 秒：
    /// 内层已经给了两倍时间还没出结果的话，多等那 30 秒也没有意义。
    watchdog: Duration,
}

impl ThreadTransport {
    /// 拉起子进程并在专用线程上接管它。
    pub fn spawn(cfg: AcpConfig) -> Result<Self> {
        let watchdog = cfg.timeout * 2 + Duration::from_secs(30);
        let (tx, rx) = channel::<Cmd>();
        // 通道里传 `String` 而不是 `anyhow::Error`：这个 `Result` 只用来
        // 回答「子进程拉起来了没有」，错误文本直接进 `bail!`，不需要 error 链。
        let (ready_tx, ready_rx) = channel::<std::result::Result<(), String>>();
        let worker = std::thread::Builder::new()
            .name("acp-worker".into())
            .spawn(move || {
                let mut proc = match AcpProcess::spawn(cfg) {
                    Ok(p) => p,
                    Err(e) => {
                        let _ = ready_tx.send(Err(format!("{e:#}")));
                        return;
                    }
                };
                if ready_tx.send(Ok(())).is_err() {
                    return;
                }
                while let Ok(cmd) = rx.recv() {
                    match cmd {
                        Cmd::Handshake(reply) => {
                            let _ = reply.send(proc.handshake());
                        }
                        Cmd::Prompt { text, reply } => {
                            let _ = reply.send(proc.prompt(&text));
                        }
                        Cmd::Shutdown(reply) => {
                            let _ = reply.send(());
                            // 退出循环 → `proc` 被 drop → `AcpProcess::drop` 杀子进程。
                            break;
                        }
                    }
                }
                // rx 断开（handle 被 drop）也会走到这里，子进程照样被收掉。
                proc.shutdown();
            })
            .context("ACP worker 线程起不来")?;

        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                tx: Some(tx),
                worker: Some(worker),
                handshake: None,
                poisoned: None,
                watchdog,
            }),
            Ok(Err(e)) => bail!("ACP 子进程拉不起来：{e}"),
            Err(_) => bail!("ACP worker 线程在就绪前就退出了"),
        }
    }

    /// 已经握手过的那份握手结果（[`AcpTransport::handshake`] 跑过之后才有）。
    pub fn handshake_snapshot(&self) -> Option<&Handshake> {
        self.handshake.as_ref()
    }

    /// 是不是已经因为一次失败而**粘住**了。
    pub fn is_poisoned(&self) -> bool {
        self.poisoned.is_some()
    }

    fn roundtrip<T>(
        &mut self,
        make: impl FnOnce(Sender<Result<T>>) -> Cmd,
    ) -> Result<T> {
        if let Some(why) = &self.poisoned {
            bail!("ACP 会话此前已失败，不许继续用（{why}）—— 重建会话或降低单组负载");
        }
        let (rtx, rrx) = channel();
        let tx = self.tx.as_ref().context("ACP worker 通道已关闭")?;
        let timeout = self.watchdog;
        tx.send(make(rtx))
            .map_err(|_| anyhow::anyhow!("ACP worker 线程已退出"))?;
        match rrx.recv_timeout(timeout) {
            Ok(Ok(v)) => Ok(v),
            Ok(Err(e)) => {
                self.poisoned = Some(format!("{e:#}"));
                Err(e)
            }
            Err(_) => {
                // 收不到回音 = worker 卡死或死了。此后这一条会话不可信。
                self.poisoned = Some("worker 线程无响应".into());
                bail!("ACP worker 线程在 {timeout:?} 内没有回音")
            }
        }
    }
}

impl AcpTransport for ThreadTransport {
    fn handshake(&mut self) -> Result<Handshake> {
        let h = self.roundtrip(Cmd::Handshake)?;
        self.handshake = Some(h.clone());
        Ok(h)
    }

    fn prompt(&mut self, text: &str) -> Result<TurnOutcome> {
        self.roundtrip(|reply| Cmd::Prompt {
            text: text.to_string(),
            reply,
        })
    }

    fn shutdown(&mut self) {
        if let Some(tx) = self.tx.take() {
            let (rtx, rrx) = channel();
            if tx.send(Cmd::Shutdown(rtx)).is_ok() {
                // 有界等待：不让 `Drop` 变成一次无限阻塞。
                // 等不到就直接放弃（丢 handle = 分离线程），
                // 子进程仍会被 worker 退出时的 drop 收掉。
                let _ = rrx.recv_timeout(Duration::from_millis(1000));
            }
        }
        if let Some(w) = self.worker.take() {
            if !w.is_finished() {
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

impl Drop for ThreadTransport {
    fn drop(&mut self) {
        self.shutdown();
    }
}

// ──────────────────────────────────────────────────────────
// 假传输 —— 测试与回放用（不起进程、不需要登录态）

/// **录好的**传输：按预设脚本回 [`TurnOutcome`]。
///
/// # 为什么它不是「顺手加的测试便利」，而是测试纪律的一部分
///
/// 测试纪律明写：**不许**真的去调 `mcode acp` —— 一次 11.7 秒、依赖登录态、
/// CI 上不可复现。若 [`AcpAgent`](super::AcpAgent) 直接持有 [`AcpProcess`]，
/// 那么它的每一处逻辑（累积、解析、用量记账、能力声明）都只能在真进程上测；
/// 那种测试的特征是「本地过、CI 挂、或者干脆没人跑」。
///
/// 有了它，`AcpAgent` 接受任意 `Box<dyn AcpTransport>`，
/// 测试喂一段录好的 [`TurnOutcome`] 就能把整条路走完 —— 毫秒级、可复现、离线。
pub struct ScriptedTransport {
    pub handshake: Handshake,
    /// 排队待发的回应。用完就 pop —— 所以「第 N 组失败」这类脚本能自然写出来。
    turns: Mutex<VecDeque<Result<TurnOutcome>>>,
    /// 队列空时的兜底（`always_answer` 用它）。`None` = 队列用完即报错。
    fallback: Option<Box<dyn Fn(&str) -> Result<TurnOutcome> + Send>>,
    /// 收到过的 prompt 原文 —— 测试用来断言「发出去的确实是契约里那五个输入」。
    pub prompts: Mutex<Vec<String>>,
}

impl ScriptedTransport {
    /// 按脚本依次回 [`TurnOutcome`]。
    pub fn new(handshake: Handshake, turns: Vec<Result<TurnOutcome>>) -> Self {
        Self {
            handshake,
            turns: Mutex::new(turns.into_iter().collect()),
            fallback: None,
            prompts: Mutex::new(Vec::new()),
        }
    }

    /// 无论问什么都按同一个函数回答（多数测试要的是这个）。
    pub fn always_answer(
        handshake: Handshake,
        answer: impl Fn(&str) -> Result<TurnOutcome> + Send + 'static,
    ) -> Self {
        Self {
            handshake,
            turns: Mutex::new(VecDeque::new()),
            fallback: Some(Box::new(answer)),
            prompts: Mutex::new(Vec::new()),
        }
    }
}

impl AcpTransport for ScriptedTransport {
    fn handshake(&mut self) -> Result<Handshake> {
        Ok(self.handshake.clone())
    }

    fn prompt(&mut self, text: &str) -> Result<TurnOutcome> {
        self.prompts.lock().expect("prompts 锁").push(text.to_string());
        if let Some(next) = self.turns.lock().expect("turns 锁").pop_front() {
            return next;
        }
        match &self.fallback {
            Some(f) => f(text),
            None => bail!(
                "ScriptedTransport 的回应队列已空 —— 这次抽取没有预录的回应。\n\
                 队列按调用顺序消耗，所以「第 N 组失败」这类脚本是**故意**这么写的。"
            ),
        }
    }

    fn shutdown(&mut self) {}
}

//! **`mcp-server` 的进程外壳**（REQ-027）。
//!
//! # 三件事，按顺序
//!
//! 1. **解析并锁定一份冻结快照**（[`prepare`]）—— 不冻结就**拒绝启动**。
//! 2. **stdio 行循环**（[`serve`]）—— 读一行、回一行。
//! 3. **每个返回值过守门器**（`tools::guard`）。
//!
//! # 为什么「只能服务冻结快照」这条约束得由这一层自己加
//!
//! 摸底结论：**冻结闸门只在写路径**。`save_snapshot` 在事务内查
//! `frozen_at IS NOT NULL` 命中即 `VersionFrozen`；而 `get_snapshot` /
//! `load_*` 这些读路径**一处检查都没有**。所以「只读冻结版」今天
//! **在代码里并不存在** —— 不加这个闸，`ontok ontology show` 读的就是
//! 一个还能被改的版本，而客户以为那是定稿。
//!
//! 这正是本项目一贯的做法：**约束要么落在代码里，要么就不存在**。
//! 一句写在文档里的「请勿对未冻结版本使用」等于没有 ——
//! fail-closed 必须在启动时就把进程拒掉。

use std::io::{BufRead, Write};
use std::sync::mpsc;

use anyhow::{bail, Context};
use serde_json::Value;

use crate::db::project::PgProjectStore;
use crate::db::snapshot_store::PgStore;
use sqlx::postgres::PgPool;

use super::protocol as p;
use super::tools::{self, OntologyContext};

/// `mcp-server` 的启动参数。
#[derive(Debug, Clone)]
pub struct ServerArgs {
    pub project: String,
    pub version: i32,
}

/// **启动闸门**：解析项目/版本 → 查 `frozen_at` → **非 NULL 才继续**。
///
/// 返回构建好的只读上下文。**未冻结即 `Err`**，调用方据此 fail-closed 退出。
pub async fn prepare(pool: &PgPool, args: &ServerArgs) -> anyhow::Result<OntologyContext> {
    let ps = PgProjectStore::new(pool.clone());
    let store = PgStore::new(pool.clone());

    let project = ps
        .get_by_name(&args.project)
        .await?
        .with_context(|| format!("项目 {:?} 不存在", args.project))?;

    let version = ps
        .version_by_seq(project.id, &project.name, args.version)
        .await
        .with_context(|| format!("项目 {:?} 没有第 {} 版", args.project, args.version))?;

    // ── 冻结闸门（fail-closed）────────────────────────────
    //
    // 读路径本来不查冻结 —— 见模块头。**非 NULL 才对外服务**；
    // 未冻结不是「降级服务」，是**拒绝启动**。
    let Some(frozen_at) = version.frozen_at else {
        bail!(
            "项目 {:?} 的第 {} 版**尚未冻结**，拒绝对外服务。\
             mcp-server 只服务冻结快照（业务模型定稿后才给客户看）。\
             冻结请先执行：`ontok version freeze --project {} --version {}`",
            args.project,
            args.version,
            args.project,
            args.version
        );
    };

    let snapshot = store
        .get_snapshot(version.id)
        .await?
        .with_context(|| format!("第 {} 版没有快照", args.version))?;

    let sv = crate::cli::snapshot_to_view(&snapshot);

    // key → 展示名。**必须用 key 定位成员**（与 `ontology overview` 同一条理由：
    // 两个成员可能翻成同一个展示名，按名字反查会张冠李戴）。
    let names: std::collections::BTreeMap<String, String> = sv
        .entity_types
        .iter()
        .map(|e| (e.key.clone(), e.display_name.clone()))
        .collect();

    let view = crate::domain::view::project(&sv);

    // ── REQ-104 · 出口层属性级剔除 ──
    //
    // 判据在 `identifier_shape::has_raw_identifier`（全仓一份），本处**只调用**。
    // 放在 `project()` 之后、`build_overview()` 之前：这样**五个工具共用这一份视图**，
    // 而不是每个工具各写一遍过滤（纪律 H）。
    //
    // ⚠️ 本模块此前那道 `scrub_identifiers`（**替换**成 `…`）**保留不动** ——
    // 它是 B1 的职责、动作不同（保留周围业务文字）。本轮加的是**剔除**：
    // 名字是标识符的整条属性不上，名字干净而说明脏的只隐去说明。
    // 两者先后无冲突（替换过之后 `has_raw_identifier` 更不容易命中）。
    //
    // ⭐ REQ-107：前处理与它的 stderr 报告**都收进管线**（`prepare_view`）——
    // 与 CLI / HTTP 共用同一份前处理与同一种报告形状。
    let filtered = crate::domain::pipeline::prepare_view(&view);
    let view = filtered.view;

    let overview =
        crate::domain::overview::build_overview(&view, &snapshot.concept_groups, &names);

    // 概念显示名 → 确认状态。**重名取先声明的**，并在 notes 里报出来 ——
    // 静默取一个会让「这个确认状态到底属于哪个分组」变成不可知。
    let mut confirmation = std::collections::BTreeMap::new();
    let mut name_clashes = 0usize;
    for g in &snapshot.concept_groups {
        if confirmation.contains_key(&g.display_name) {
            name_clashes += 1;
            continue;
        }
        confirmation.insert(g.display_name.clone(), tools::confirmation_of(g));
    }
    let mut overview = overview;
    if name_clashes > 0 {
        overview.notes.push(format!(
            "有 {name_clashes} 个概念组的显示名与前面的重复，确认状态只按先声明的那个取值"
        ));
    }

    Ok(OntologyContext {
        project: args.project.clone(),
        version: args.version,
        frozen_at: frozen_at.to_rfc3339(),
        view,
        overview,
        names,
        confirmation,
    })
}

/// stdio 服务循环。
///
/// ⚠️ **stdout 只许写 MCP 消息** —— 规范 Transports 章：「server MUST NOT
/// write anything to its stdout that is not a valid MCP message」。
/// 所以诊断信息一律走 `stderr`（规范允许 server 往 stderr 写日志）。
///
/// 读 stdin 用**独立线程 + `std::sync::mpsc`**，主循环 `spawn_blocking`
/// 等行：tokio 当前只开了 `rt-multi-thread`/`macros`，**没有 `sync`**，
/// 用 `tokio::sync::mpsc` 就得改 `Cargo.toml`（范围外）。
/// `spawn_blocking` 属 `rt`，现有 feature 就够，且不会在 current_thread
/// runtime 上 panic。
pub async fn serve(ctx: OntologyContext) -> anyhow::Result<()> {
    let (tx, rx) = mpsc::channel::<String>();

    std::thread::Builder::new()
        .name("mcp-stdin".into())
        .spawn(move || {
            let stdin = std::io::stdin();
            for line in stdin.lock().lines() {
                match line {
                    Ok(l) => {
                        let t = l.trim().to_string();
                        if t.is_empty() {
                            continue; // 空行不是消息，规范没定义，跳过
                        }
                        if tx.send(t).is_err() {
                            break; // 服务端已退出
                        }
                    }
                    Err(_) => break, // EOF / 管道关闭 = 客户端断开，正常收工
                }
            }
        })
        .context("mcp-server 的 stdin 读取线程起不来")?;

    // `recv()` 会把 Receiver 消耗掉，而循环每轮都要用它一次。
    // 包进 `Arc<Mutex<..>>`：每轮把一个**克隆的 Arc** 交给 spawn_blocking，
    // 真正的 Receiver 始终留在本函数里。
    //
    // 不用 `block_in_place`：它在 current_thread runtime 上会 panic，
    // 而 `#[tokio::test]` 默认就是 current_thread —— 那样本函数就没法测了。
    let rx = std::sync::Arc::new(std::sync::Mutex::new(rx));

    loop {
        // 等下一行 —— 用 spawn_blocking 把「阻塞等待」挪出 async worker
        let rx2 = rx.clone();
        let line = tokio::task::spawn_blocking(move || {
            rx2.lock().ok()?.recv().ok()
        })
        .await
        .context("等 stdin 那一行的任务本身出错了")?;

        // 读取线程结束 / channel 关闭 = EOF = 客户端关闭 stdin = 规范里的正常关闭路径
        let Some(line) = line else {
            return Ok(());
        };

        let out = handle_line(&ctx, &line);
        // 只有请求才有响应；通知**必须**静默
        if let Some(resp) = out {
            let mut stdout = std::io::stdout();
            stdout
                .write_all(resp.as_bytes())
                .and_then(|_| stdout.write_all(b"\n"))
                .and_then(|_| stdout.flush())
                .context("写 stdout 失败 —— 客户端可能已经断开")?;
        }
    }
}

/// 处理一行入站消息，返回**该回的那一行**（通知返回 `None`）。
///
/// 单独成函数是为了能**用假 JSON-RPC 字节直接测它**，
/// 不必真的起进程、更不必真的连 MCP 客户端。
pub fn handle_line(ctx: &OntologyContext, line: &str) -> Option<String> {
    match p::decode_line(line) {
        p::Incoming::Request(req) => Some(dispatch(ctx, req.method.as_str(), &req.params, &req.id)),

        p::Incoming::Notification(n) => {
            // `notifications/initialized` 是生命周期的第三步，收到即认为握手完成。
            // **不回任何东西** —— 通知有响应就是协议违规。
            if n.method != "notifications/initialized" {
                eprintln!("· 收到通知 {}，按规范不回响应", n.method);
            }
            None
        }

        p::Incoming::Invalid { id, code, message } => {
            // 无法确定 id 时回 `id: null`，**不许不回**（JSON-RPC 2.0 要求）。
            Some(match id {
                Some(id) => p::encode_error_line(&id, code, &message, None),
                None => p::encode_error_no_id(code, &message),
            })
        }
    }
}

/// 请求分发 —— 纯函数般的同步逻辑，便于直接测。
fn dispatch(ctx: &OntologyContext, method: &str, params: &Value, id: &Value) -> String {
    match method {
        "initialize" => {
            let requested = params
                .get("protocolVersion")
                .and_then(Value::as_str);
            let agreed = p::negotiate_protocol_version(requested);
            // ⭐ REQ-107 · AC-008（Z3）：`initialize` 的**整段 result** 过运行时闸。
            //
            // 收口前只有**项目名**过闸（`tools::instructions` 里那一句），
            // 模板正文（「该版本已冻结」「confirmation…」）与 `serverInfo`
            // **从不过闸** —— 往模板里塞一个守门词不会有任何测试红。
            // 现在整段 result（模板正文 + serverInfo + 已过闸的项目名）一起判，
            // 与 `tools/call` 的返回值同一条外送路径、同一道闸。
            let result = p::initialize_result(agreed, &tools::instructions(ctx));
            match crate::domain::pipeline::publish_value(crate::domain::pipeline::OutletSpec::Z3, result) {
                Ok(rendered) => p::encode_result(id, rendered.into_value()),
                Err(e) => p::encode_error_line(id, p::INTERNAL_ERROR, e.message(), None),
            }
        }

        "notifications/initialized" => {
            // 以请求形式到达的初始化通知（部分客户端这么发）—— 回空结果，
            // 因为对方带 id 等着响应。
            p::encode_result(id, p::empty_result())
        }

        "ping" => p::encode_result(id, p::empty_result()),

        "tools/list" => {
            let tools: Vec<Value> = tools::catalog()
                .into_iter()
                .map(|t| {
                    serde_json::json!({
                        "name": t.name,
                        "title": t.title,
                        "description": t.description,
                        "inputSchema": t.input_schema,
                    })
                })
                .collect();
            // ⭐ REQ-107 · AC-008（Z2）：`tools/list` 过运行时闸。
            //
            // `description` / `title` 从**真实响应体**里发出去，进的是客户端的
            // 工具面板与下游 LLM 的提示词素材 —— 与 `tools/call` 的返回值同一条外送路径。
            // 收口前这条路径**零闸**（而且 `description`/`title` 连断言都没有）。
            let result = p::tools_list_result(Value::Array(tools));
            match crate::domain::pipeline::publish_value(crate::domain::pipeline::OutletSpec::Z2, result) {
                Ok(rendered) => p::encode_result(id, rendered.into_value()),
                Err(e) => p::encode_error_line(id, p::INTERNAL_ERROR, e.message(), None),
            }
        }

        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);

            if !tools::catalog().iter().any(|t| t.name == name) {
                // 目录里没有 ⇒ 未知工具。规范 Tools 章：这是**协议错误** -32602。
                //
                // ⚠️ `name` 是**客户端传进来的字符串**，原样回显等于
                // 把守门词从错误路径送出去 —— 成功路径过了闸，错误路径没过，
                // 闸门就从这里漏。所以与下面两处一样走 `safe_error`。
                return p::encode_error_line(
                    id,
                    p::INVALID_PARAMS,
                    &tools::safe_error(&format!("没有这个工具：{name}")),
                    None,
                );
            }

            match tools::call(ctx, name, &arguments) {
                // ── 发布闸（REQ-107 · AC-001/002/003）── 对**每一个**返回值无条件执行，
                // 且只有**一个**调用点 —— 五个工具（M1–M5）共用它。
                //
                // 判定在管线里（`publish_value` 复用 `tools::guard` 本体，不抄判据）；
                // 出口这一步只把结果映射成协议形状。
                Ok(value) => match crate::domain::pipeline::publish_value(
                    crate::domain::pipeline::OutletSpec::MCP_TOOLS,
                    value,
                ) {
                    Ok(rendered) => {
                        let text = rendered.text().to_string();
                        let structured = rendered.into_value();
                        p::encode_result(id, p::tool_call_result(&text, structured))
                    }
                    // 命中 ⇒ `isError: true`，**不给数据**（fail-closed）。
                    // 命中词条只进 stderr，见 `tools::guard` 的模块内说明。
                    Err(e) => p::encode_result(id, p::tool_call_error_result(e.message())),
                },
                Err(e) => match e {
                    // 参数不合法 / 工具不存在 ⇒ 协议错误（连接坏了）
                    tools::ToolError::Unknown(_) | tools::ToolError::BadParams(_) => {
                        p::encode_error_line(
                            id,
                            p::INVALID_PARAMS,
                            &tools::safe_error(&e.to_string()),
                            None,
                        )
                    }
                    // 业务上取不到 ⇒ 执行错误（连接是好的，只是这一问没答案）
                    tools::ToolError::NotFound(_) => {
                        p::encode_result(
                            id,
                            p::tool_call_error_result(&tools::safe_error(&e.to_string())),
                        )
                    }
                },
            }
        }

        other => {
            // 规范 Tools 章「Error Handling」：方法不认识 ⇒ JSON-RPC 错误 -32601。
            //
            // ⚠️ `other` 同样是**客户端传进来的方法名**，会原样回显 ——
            // 与上面「未知工具」同一条理由：错误路径也必须过 `safe_error`。
            p::encode_error_line(
                id,
                p::METHOD_NOT_FOUND,
                &tools::safe_error(&format!("不支持的方法：{other}")),
                None,
            )
        }
    }
}
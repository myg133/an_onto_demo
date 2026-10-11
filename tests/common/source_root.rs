//! REQ-046 · 判据读源码时的 **crate 根解析** —— **运行时**，不依赖编译期常量。
//!
//! # 这份文件在修什么
//!
//! REQ-045 把判据从 `include_str!`（编译期内嵌，二进制**自包含**）改成
//! 运行时读盘 + `env!("CARGO_MANIFEST_DIR")`。回归就出在后者：
//! **`CARGO_MANIFEST_DIR` 是编译期常量** —— 二进制一旦在 A 目录编好，
//! 被搬到 B 处直跑，它仍然去读 **A** 的源码。
//!
//! REQ-045 合并后第一次全量即 **506 passed / 30 failed**，形态集中：
//!
//! ```text
//! panicked at tests/test_acp_extraction.rs:378
//! 读不到 /home/…/verify-REQ-044/src/agent/acp/mod.rs：No such file or directory
//! ```
//!
//! `verify-REQ-044` 是一个**早已回收的 worktree**。⇒ 二进制在依赖一个
//! **不存在的目录**，而报错把它说成「**文件不存在**」——
//! **这是那次事故里最有害的部分**：它把人引向「源码被删了？」这个错误方向。
//!
//! # 锚点改成「当前实际的那个 crate 根」
//!
//! [`read_source`] 每次运行都**重新**定位，顺序如下：
//!
//! 1. **运行时工作目录**（含最多 [`MAX_WALK_UP`] 层上溯）—— 找到
//!    `Cargo.toml` 且其中 `[package] name` **等于本 crate 的包名**的目录；
//! 2. 编译期目录 —— **仅当它此刻仍在、且仍是本 crate 的根**时兜底。
//!
//! `cargo test` 起的测试进程 cwd 就是包根，所以**正常路径下 1 与 2 等价**，
//! 本模块不改变任何既有判据的取值。真正被改变的是**事故路径**：
//! 判据从此跟着**你脚下那棵树**走，而不是跟着**编译它时那棵树**走。
//!
//! # 为什么必须核对**包名**，光看「有没有 `Cargo.toml`」不够
//!
//! 少这一步，`cwd` 恰好落在**另一个 crate** 里时，判据会**静默读错那棵树**
//! —— 那是**假绿**，比红着报更坏。核对包名把它变成一次响的失败。
//!
//! # 为什么不用 `std::env::current_exe()` 上溯做**锚点**
//!
//! 试过，它当锚点会引入一个**更隐蔽**的洞：二进制被复制到任意目录后，
//! 从它上溯可能落到一个**毫不相干**的 crate 根上；而判据本来就该看
//! 「你正在检验的那棵树」，不是「二进制恰好躺在哪」。
//!
//! 所以本模块把 `current_exe()` 降级为**只做诊断**：它出现在报错里，
//! 帮人一眼看出「这个二进制是从哪儿拿到的」，但**不参与选根**。
//!
//! # 为什么不做「构建时记 `git rev-parse HEAD`、不匹配就拒绝运行」
//!
//! (a) 它**解决不了**本次事故 —— 事故是「目录没了」，不是「代码版本不同」；
//! (b) 要在编译期拿到 HEAD 就得引 `build.rs`，而 `build.rs` 仍是编译期求值，
//!     它对「二进制已经编好、文件再改」这条真正的失效模式**一样无能为力**
//!     （`test_connect_retry.rs` 文件头已用 A/B 实测钉死这一点）；
//! (c) 它凭空多出一个「必须同时是合法 git 工作区」的运行期前提，
//!     而判据只该要求「有一棵可读的源码树」。
//!
//! ⇒ 记录为**已考虑并否决**，不是「没想过」。
//!
//! # REQ-047 封口：两条登记订正
//!
//! ## 🔴 已知洞 F-2（假绿）：**核对包名挡不住「同一 crate 的另一个 worktree」**
//!
//! 上文「为什么必须核对包名」那段论证**只对「别的 crate」成立**。
//! 本仓**每个 worktree 的 `Cargo.toml` 都写着 `name = "ontok"`** ⇒ 包名核对
//! **全部通过** ⇒ cwd 落在**另一个 `ontok` worktree** 时，
//! [`crate_root_from_cwd`] 会把它当成「本 crate 的根」。
//!
//! 后果（QA 实跑确认：**测试 passed**）：
//! 1. 候选列表 = `[另一个 worktree 的根, 编译期根]`；
//! 2. 目标文件**不在**另一个 worktree 里 ⇒ 第一个候选失败；
//! 3. 回落到**编译期根**并**读成功** ⇒ [`read_source`] **返回成功**。
//!
//! ⇒ 你以为在检验 B 树，实际读到的是 **A 树**，且**没有任何信号**。
//!
//! **本需求（REQ-047）明确不修**：修它要改 [`order_candidates`] 的**语义**
//! （例如 cwd 命中同名 crate 时直接 fail-closed，而不是回落），
//! 那超出「加守卫」的范围，且会改变既有判据的取值。
//! **登记为已知洞**，留给后续需求。
//!
//! ## 🟢 F-5 撤销：报错根与 IO 错误**结构上不可能不一致**
//!
//! 此前登记「`last_err` 取**最后一个**候选、而报错根取**第一个**，二者可能不一致」
//! —— **与代码不符，本条撤销**。理由：
//!
//! ```
//! Err(e) => last_err = Some((root.path.clone(), e))   // :264 — root 与 e 打进同一元组
//! let (root, e) = last_err.expect(...)                 // :273 — 取出的 root 正是产生该 e 的候选
//! ```
//!
//! （行号对应 REQ-047 提交 `fff0938` 的后续修订；**以代码为准**，行号会随编辑漂移。）
//!
//! ⇒ 二者**永远来自同一候选**。`roots.first()` 只用于判 `None`/`Some`
//! （类别 A/B 之分），**不参与选根**。
//!
//! ⚠️ 真正存在（但**展示层**、非正确性）的边界：两个候选**都**失败时报出的 root 是
//! **最后一个**候选（编译期树）而非主锚（cwd 树），可能把人引去看错的那棵树。

use std::path::{Path, PathBuf};

/// 编译期常量。**只作兜底与诊断用**，不作默认锚点 —— 见文件头。
///
/// ⚠️ `// ANCHOR-ALLOW` —— REQ-047 起，**全仓 `tests/**/*.rs` 只有这一行**
/// 允许出现裸 `env!("CARGO_MANIFEST_DIR")`（G-1 语法层判据 `g1_…` 逐行扫代码行）。
/// 要加第二处之前先问：为什么不能走 [`read_source`] / [`crate_root`]？
const BUILD_MANIFEST_DIR: &str = env!("CARGO_MANIFEST_DIR"); // ANCHOR-ALLOW

/// 本 crate 的包名。用来确认「找到的那个 `Cargo.toml` 就是本 crate 的」。
const PKG_NAME: &str = env!("CARGO_PKG_NAME");

/// 从工作目录往上最多回溯几层找 crate 根。
///
/// 有上界是刻意的：上溯无界会让 `/tmp` 这种地方一路爬到某个大仓库的根上，
/// 判据于是「跑在哪个目录」与「读到谁的源码」都变得不可预期。
pub(crate) const MAX_WALK_UP: usize = 8;

// ══════════════════════════════════════════════════════════════
// 定位
// ══════════════════════════════════════════════════════════════

/// 从 `Cargo.toml` 里抠出 `[package] name` 的值。
///
/// 刻意**不引 toml 解析依赖**：判据底座不该为读一个字段拖进一个新 crate。
/// 本仓的 `Cargo.toml` 是规范写法（`[package]` 段里 `name = "ontok"`），
/// 抠不出来时返回 `None` —— 那一律按「不是本 crate 的根」处理，**偏保守**。
fn manifest_package_name(manifest: &Path) -> Option<String> {
    let text = std::fs::read_to_string(manifest).ok()?;
    let mut in_package = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
            continue;
        }
        if !in_package {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            if k.trim() == "name" {
                return Some(v.trim().trim_matches('"').to_string());
            }
        }
    }
    None
}

/// 该目录是不是**本 crate** 的根（而不是「某个」crate 的根）。
fn is_this_crate_root(dir: &Path) -> bool {
    manifest_package_name(&dir.join("Cargo.toml")).as_deref() == Some(PKG_NAME)
}

/// 从 `start` 往上最多回溯 `limit` 层（**含起点本身，共 `limit + 1` 次试探**），
/// 第一个满足 `is_root` 的目录。**纯函数**，不读任何全局状态。
///
/// 抽成纯函数是为了让「上溯有界」这条**可被断言**：REQ-046 留下的两条存活变异里，
/// M1 之所以能全绿，正是因为正常路径下锚点的**取值**与编译期常量相同
/// （`cargo test` 的 cwd 恒等于包根）—— 只有把「顺序」「上界」这类
/// **与取值无关的性质**抽出来断言，才杀得掉它。见 `tests/test_anchor_guard.rs`。
pub(crate) fn walk_up_from<F>(start: &Path, limit: usize, is_root: F) -> Option<PathBuf>
where
    F: Fn(&Path) -> bool,
{
    let mut cur: Option<&Path> = Some(start);
    for _ in 0..=limit {
        let dir = cur?;
        if is_root(dir) {
            return Some(dir.to_path_buf());
        }
        cur = dir.parent();
    }
    None
}

/// 从 `start` 上溯找**本 crate** 的根（包名须等于 [`PKG_NAME`]）。
pub(crate) fn crate_root_from(start: &Path) -> Option<PathBuf> {
    walk_up_from(start, MAX_WALK_UP, is_this_crate_root)
}

/// 从运行时工作目录上溯，找本 crate 的根。
fn crate_root_from_cwd() -> Option<PathBuf> {
    crate_root_from(&std::env::current_dir().ok()?)
}

/// 编译期那个目录**此刻是否仍可用**。
pub(crate) fn build_root_alive() -> bool {
    is_this_crate_root(Path::new(BUILD_MANIFEST_DIR))
}

/// 一个候选根是**怎么**找着的 —— 让「顺序」本身可断言，而不是只能看取值。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RootOrigin {
    /// 运行时工作目录上溯（**REQ-046 的实质贡献**，删掉它 = 还原 REQ-045）。
    Cwd,
    /// 编译期常量兜底。
    BuildManifest,
}

/// 候选根 + 它的来源。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RootCandidate {
    pub(crate) path: PathBuf,
    pub(crate) origin: RootOrigin,
}

/// 本次运行该按哪个顺序试的候选根 —— **纯函数**。
///
/// # 顺序是 REQ-046 的**全部**内容，不是排版
///
/// 运行时 cwd 必须在**前**：事故场景下编译期目录已不存在，锚点只能靠 cwd。
/// 把它挪到兜底之后，判据就会重新依赖编译期常量 —— 即 **M1 变异**。
///
/// ⚠️ **空 vec ⇒ 一个可用的 crate 根都没定位到 ⇒ 这是「目录不对」那一类失败。**
pub(crate) fn order_candidates(
    cwd_root: Option<PathBuf>,
    build_alive: bool,
) -> Vec<RootCandidate> {
    let mut roots = Vec::new();
    if let Some(r) = cwd_root {
        roots.push(RootCandidate {
            path: r,
            origin: RootOrigin::Cwd,
        });
    }
    if build_alive {
        roots.push(RootCandidate {
            path: PathBuf::from(BUILD_MANIFEST_DIR),
            origin: RootOrigin::BuildManifest,
        });
    }
    roots
}

/// [`order_candidates`] 的真实接线：把**实际探测**结果喂进去。
pub(crate) fn candidate_roots() -> Vec<RootCandidate> {
    order_candidates(crate_root_from_cwd(), build_root_alive())
}

// ══════════════════════════════════════════════════════════════
// 对外：读源码 / 取 crate 根
// ══════════════════════════════════════════════════════════════

/// 判据读源码的**唯一入口**：相对**运行时定位到的 crate 根**读一个文本文件。
///
/// # fail-closed（REQ-045 立，REQ-046 保留并细化）
///
/// 读不到就 panic，**不存在「读不到就当没有」这条分支**。
/// `include_str!` 的隐性好处是「文件不在就编不过」；改成运行时读之后这个保证
/// 消失了，必须显式补回来 —— 否则「文件被改名 / 移动」会让判据**悄悄不再检查**
/// 它本该检查的源码，那比「判据检查过期源码」更难发现。
///
/// # REQ-046 的关键改动：报错**分两类**，不混成一条 `No such file or directory`
///
/// | 类别 | 触发条件 | 报错说什么 |
/// | --- | --- | --- |
/// | **B · 目录不对** | 一个可用的 crate 根都定位不到 | 编译期目录已不在 + 你在哪跑的 + 二进制在哪 + 怎么修 |
/// | **A · 文件真不存在** | crate 根找对了，文件不在 | 根是哪来的（cwd 还是编译期）+ 相对路径 + 绝对路径 + IO 错误 |
///
/// REQ-045 之前两者都退化成同一句 `No such file or directory`，于是
/// 「目录不对」被读成「文件被删了」—— **那是事故里最有害的部分**。
pub fn read_source(rel_to_crate: &str) -> String {
    let roots = candidate_roots();
    let mut last_err = None;

    for root in &roots {
        match std::fs::read_to_string(root.path.join(rel_to_crate)) {
            Ok(text) => return text,
            Err(e) => last_err = Some((root.path.clone(), e)),
        }
    }

    match roots.first() {
        // 压根定位不到 crate 根 ⇒ 这不是「文件丢了」，是**目录不对**。
        None => panic!("{}", mismatch_diagnostic(rel_to_crate)),
        // 根定位到了、文件不在 ⇒ 文件**真的**不存在。
        Some(_) => {
            let (root, e) = last_err.expect("roots 非空 ⇒ 循环里至少记过一次失败");
            panic!(
                "{}",
                missing_file_diagnostic(rel_to_crate, &root, &e, build_root_alive())
            );
        }
    }
}

/// 判据要**遍历目录**时用的 crate 根（`src/`、`src/agent/`、`src/domain/` …）。
///
/// 定位不到时报**类别 B**，与 [`read_source`] 同一套措辞 ——
/// 「根都找不着」和「根找着了但子目录不在」是两回事，必须分开说。
pub fn crate_root() -> PathBuf {
    match candidate_roots().into_iter().next() {
        Some(root) => root.path,
        None => panic!("{}", mismatch_diagnostic("（此处要的是 crate 根本身，不涉及具体文件）")),
    }
}

// ══════════════════════════════════════════════════════════════
// 两类报错的措辞
// ══════════════════════════════════════════════════════════════

/// 这个 crate 根是怎么找着的 —— 写进报错，让人知道判据读的是哪棵树。
fn root_source_desc(root: &Path) -> String {
    if root == Path::new(BUILD_MANIFEST_DIR) {
        format!("编译期目录 `CARGO_MANIFEST_DIR`（你不在本 crate 的根目录下运行）")
    } else {
        format!("**运行时工作目录上溯**匹配到 `name = \"{PKG_NAME}\"` 的 manifest")
    }
}

/// **类别 B** —— 二进制与当前目录不匹配。
fn mismatch_diagnostic(rel_to_crate: &str) -> String {
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "<读取失败>".to_string());
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "<读取失败>".to_string());
    let build_alive = build_root_alive();
    let build_state = if build_alive {
        "该目录**还在**，但里面已不是 `name = \"ontok\"` 的 crate 根".to_string()
    } else {
        "**该目录已不存在**（worktree 被回收 / 仓库被搬走 / 构建产物被跨目录复用）".to_string()
    };

    format!(
        "{header}\
         │\n\
         │  · 本二进制**编译时**的 crate 根：{BUILD_MANIFEST_DIR}\n\
         │    现状：{build_state}\n\
         │  · 你**运行它时**的工作目录：{cwd}\n\
         │    现状：该目录及其上溯 {MAX_WALK_UP} 层内，没有 `name = \"{PKG_NAME}\"` 的 `Cargo.toml`\n\
         │  · 本二进制**自身**位置：{exe}\n\
         │  · 本来想读的源码（相对 crate 根）：{rel_to_crate}\n\
         │\n\
         └─ 【怎么修】二选一：\n\
         \x20  1. `cd` 到本仓库的 crate 根目录，再运行该二进制；\n\
         \x20  2. 在你想检验的那棵树上执行 `cargo test` 重编 —— **直跑\
         `target/debug/deps/` 下的旧二进制时 cargo 不会替你重编**，\
         而那正是 REQ-046 事故的机制（cargo 误判新鲜、复用了旧二进制）。\n\
         \n\
         ⚠️ 判别提示：**类别 B 的特征是「crate 根本身就没找到」**。\
         若报错里出现「已定位到 crate 根」，那说明根找对了、\
         是**文件**的问题（类别 A），两条不要混看。",
        header = header_mismatch()
    )
}

/// **类别 A 的抬头** —— crate 根找对了、文件真的不在。
///
/// 抽成**纯函数**（不读任何环境状态）是 REQ-047 的 G-3：REQ-046 把两类报错
/// 分开了，但**全仓没有任何一条断言去区分它们** ⇒ 把 B 的抬头换成 A 的文案
/// （**M2 变异**，即「两类合并」）是 536 条测试全绿。**措辞本身就是契约**。
///
/// 判别短语见 [`header_discriminators`]，由 `g3_…` 断言钉住。
pub(crate) fn header_missing_file() -> &'static str {
    "判据读不到源码文件 ⇒ **fail-closed**，此处不静默跳过。\n\
     \n\
     ┌─ 【失败类型】**文件真的不存在**（crate 根是**对**的，文件不在）\n"
}

/// **类别 B 的抬头** —— 一个可用的 crate 根都定位不到。
///
/// 纯函数，理由同 [`header_missing_file`]。
pub(crate) fn header_mismatch() -> &'static str {
    "判据定位不到「本 crate 的根」⇒ **fail-closed**，此处不静默跳过。\n\
     \n\
     ┌─ 【失败类型】**二进制与当前目录不匹配** —— **这不是「源码文件被删了」**\n"
}

/// 两类抬头各自的**判别短语**（必须互不相同，且只属于自己那一类）。
///
/// 这是「两类不可合并」的**可执行定义**：只要各自还带着自己的判别短语，
/// 把 B 整个换成 A（M2）就会既撞上「互不相等」又撞上「短语丢失」。
pub(crate) const fn header_discriminators() -> [(&'static str, &'static str); 2] {
    [
        ("A", "文件真的不存在"),
        ("B", "二进制与当前目录不匹配"),
    ]
}

/// **类别 A** —— crate 根找对了，文件真的不存在。
fn missing_file_diagnostic(
    rel_to_crate: &str,
    root: &Path,
    err: &std::io::Error,
    build_alive: bool,
) -> String {
    let abs = root.join(rel_to_crate);
    let tail = if build_alive {
        String::new()
    } else {
        format!(
            "\n  · 附带信息：编译期目录 `{BUILD_MANIFEST_DIR}` **已不存在** —— \
             本次是靠运行时工作目录找到 crate 根的（这正是 REQ-046 期望的行为）。"
        )
    };

    format!(
        "{header}\
         │\n\
         │  · 已定位到 crate 根：{}\n\
         │    这个根怎么来的：{}\n\
         │  · 相对 crate 根：{rel_to_crate}\n\
         │  · 绝对路径：{}\n\
         │  · 底层 IO 错误：{err}\n\
         │{tail}\n\
         │\n\
         └─ 【怎么修】确认路径拼写；确属改名 / 移动，则同步更新调用它的那条判据。\n\
         \n\
         ⚠️ **别把它读成 REQ-046 事故那种「目录不对」**：这里 crate 根已经找对了，\
         问题在文件本身（被改名、被移动或拼错）。",
        root.display(),
        root_source_desc(root),
        abs.display(),
        header = header_missing_file()
    )
}
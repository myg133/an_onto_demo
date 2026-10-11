//! REQ-041 · 建连接重试的判据（AC-001 / AC-002 / AC-003 / AC-004）。
//!
//! 这份测试**不碰数据库**：它用假闭包驱动 `retry_connect`，所以判据本身
//! 既不受共享集群负载影响、也只在毫秒~秒级跑完 —— 这是「判重试」能稳定
//! 自动化的前提。用真库去试重试，等于把判据的可靠性绑到集群负载上。
//!
//! # ⚠️ REQ-045 · 读源码一律走 [`read_source`]（**运行时**），不许再用 `include_str!`
//!
//! 本文件要读 4 个源码文件（`tests/common/mod.rs`、`tests/test_db.rs`、
//! `src/db/pool.rs`、`src/db/apply.rs`）。REQ-045 之前它们全是 `include_str!` ——
//! 即**编译期**把内容内嵌进本判据二进制。
//!
//! ## 判据曾检查过期源码，且**一声不响**（QA 报告 + BA 复核，`0c00428` 基线）
//!
//! | 观察 | 内容 |
//! | --- | --- |
//! | develop 工作区 | `git status` **空**，裸 grep `rawconnect` **计数 0** |
//! | 判据输出 | 报出一段**只在 QA 探针污染过的文件里才存在**的 `rawconnect` 代码并红 |
//! | 强制重编译 | `touch tests/test_connect_retry.rs` 后 ⇒ **12 passed / 0 failed** |
//!
//! ## ⚠️ 事故的**机制**必须照实说（REQ-045 实测，别再照抄旧说法）
//!
//! REQ-044/045 的文档曾把这归因为「`include_str!` **不建立 cargo 的文件依赖追踪**」。
//! **本机实测（cargo 1.98.1 / rustc 1.98.1）不成立**：
//!
//! | 实验 | 观察 |
//! | --- | --- |
//! | `include_str!` 的目标是否进 dep-info | **进了** —— `target/debug/deps/test_connect_retry-*.d` 明确列出 `tests/test_db.rs` / `src/db/pool.rs` / `src/db/apply.rs` |
//! | 只改 `tests/test_db.rs`（**不动**判据文件） | **确实重编译** —— 输出 `Compiling ontok`，判据二进制 mtime 由 `15:27:30` 变为 `15:27:38` |
//! | `touch` 一个**无关**文件 `tests/test_apply.rs` | **不重编译** —— `Finished in 0.14s`，二进制 mtime 不变 |
//!
//! 第三行说明 cargo 是按 **dep-info** 判定的，不是「包内任一文件」。
//! ⇒ 在**本工具链**上，改 `test_db.rs` 会触发重编译，那条路径**没有复现**。
//!
//! **但「判据检查过期源码」这个症状本身复现了，而且与工具链无关** ——
//! 根子是「内嵌内容只在**编译那一刻**取一次」。不经 cargo、直接执行已编译的二进制时：
//!
//! | 运行方式 | 同一份**已被变异**的 `tests/test_db.rs` 在磁盘上 | 结果 |
//! | --- | --- | --- |
//! | 旧版判据二进制（干净树编译，`include_str!`） | 是 | **12 passed / 0 failed** —— 漏网 |
//! | 新版判据二进制（[`read_source`]，同样直跑） | 是 | **11 passed / 1 failed** —— 当场抓到 |
//!
//! ## 事故的另一条机制（**转述 BA/QA 结论，本会话未独立复现**）
//!
//! BA 曾用 `target` 符号链接让子 agent 的 worktree 与 develop **共享构建产物**，
//! 两侧 `CARGO_MANIFEST_DIR` 不同而 fingerprint 互相覆盖 ⇒ 跑的是**另一个目录**
//! 编出来的二进制，它内嵌的是**那个目录**的 `test_db.rs`。
//! 关键在于 **dep-info 里的路径是相对 manifest 根的**（`.d` 里写的是 `tests/test_db.rs`），
//! 共享 `target` 时就解析到了别的目录的文件。**本会话按 briefing 要求没有建符号链接，
//! 故此项未复现**，如实标注。
//!
//! ## 改成运行时读，顺带消掉的三点
//!
//! 1. 内容与**编译时刻**解耦 —— 每次运行都取当前磁盘内容；
//! 2. ⚠️ **本条已被 REQ-046 订正**。原文写的是「路径锚定 `env!("CARGO_MANIFEST_DIR")`
//!    = 本 crate 的根 ⇒ 即使二进制被跨 worktree 复用也不会读到邻居目录的源码」。
//!    **前半句对、后半句错**：`CARGO_MANIFEST_DIR` 确实是本 crate 的根，
//!    但它**同时是编译期常量** —— 它锚的是「**编译时**那棵树」，
//!    不是「**你跑它时**那棵树」。worktree 一回收，它就指向**不存在的目录**，
//!    而那正是 REQ-045 合并后 30 条失败的根因。
//!    现在的锚点是**每次运行重新定位**（cwd 优先、核对包名、编译期目录兜底），
//!    见 `tests/common/source_root.rs`；
//! 3. 给 AC-002 的 **fail-closed** 提供落点 —— `include_str!` 的「文件不在就编不过」
//!    是隐性保证，改成运行时读必须显式补回来（见 [`read_source`]）。
//!
//! 本仓其它判据（`test_no_write_path.rs` / `test_review_state.rs` …）仍在用
//! `include_str!` —— 那是它们的既有约定，**本需求不动它们**；这里只把本文件读的那几处换掉。

// ══════════════════════════════════════════════════════════════
// REQ-044 封口 / REQ-045 订正 · 判据完备性**决策记录**
// ══════════════════════════════════════════════════════════════
//
// # 为什么要有这一节
//
// REQ-041 / 042 / 043 连续三轮都在给判据加补丁，**每一轮 QA 都能找到新的绕过**
// —— 这是无底洞信号。所以 REQ-044 明确**只还一笔欠账**（补完退避判据），
// 其余已知洞**逐条标注为「已知且接受」**。
//
// > **封口比补洞更重要**：不封口，每个人都会以为那是待办。
// > **失真的封口比没有封口更糟**：它会让下一个人基于错误前提放弃修，或者放过真洞。
//
// # ⚠️ REQ-045 · 编号约定（本节裸编号在**文件内唯一**）
//
// 本节的 `F-1` / `F-3` / `F-4` / `F-5` / `C4` **只在本节内有效**。
// REQ-041~044 各轮自己用过的局部编号一律写成 **`REQ-0xx-Fn`**（带轮次前缀）。
//
// **为什么加前缀**：REQ-044 之前 `F-1` 同时指「raw string 未处理」与
// 「REQ-043 的换行/注释括号」，`F-3` 同时指「白名单按名字放行」与
// 「REQ-043 的字面量匹配」—— 本节自称唯一权威决策记录，却和正文撞车，
// 读者无法确定某个 `F-1` 说的是哪一个洞。
//
// # ⚠️ REQ-045 · 判据改成二维：「自然度 × 修复成本」
//
// REQ-044 用的是**一维**判据：「触发它需要刻意构造不自然的源码形状 ⇒ 是决策」。
// **QA 指出反例**：**C4 只要把 helper 叫 `raw_connect`（日常命名）就不满足
// 「刻意构造」，却被算作决策**。一维判据把「是否刻意」当唯一支点，
// 于是**只要能在日常写法里触发就自动豁免**，完全不看这条洞值不值得修 ——
// 而且它压根看不见「判据自己检查的是过期源码」这种**零刻意**就踩的洞。
//
// 改成两维，各自可查证：
//
// | 维度 | 问什么 | 档位 |
// | --- | --- | --- |
// | **自然度** | 触发它需要多不自然的源码形状？ | **高** = 日常改代码就会踩；**中** = 写法本身日常，但要放进特定位置；**低** = 必须刻意写反常源码 |
// | **修复成本** | 在本仓判据层修掉它要多少？ | **低** = 十几行、复用已有底座；**中** = 同一手法要在多处重复实现，或要先定一个范围；**高** = 要引入新解析层/外部依赖 |
//
// **取二维里更紧的那一维**：
//
// - **高自然度 + 低成本** ⇒ **当场就该修**，登记为「**欠账**」，不算「决策」；
// - **低自然度** ⇒ 可登记为**决策**（不论成本）；
// - **中自然度** ⇒ 看成本：低成本就修，**中/高成本才可登记为决策**，且**必须写明成本估算依据**。
//
// **追加一条方向性约束**：能造出**假绿**的洞，即使登记为决策，
// 也**必须写明「造出这个假绿需要什么具体构造」** —— 否则下一任无从判断它是否真被接受过。
//
// # 五条已知洞 · 用二维判据重新评估（REQ-045）
//
// ⚠️ 下面每条的「修复成本」都是本仓**可查证**的结论；
// REQ-044 那版有**三条**的现象或成本描述**已被 QA 实测推翻**（逐条见下）。
//
// | 编号 | 现象 | 自然度 | 修复成本 | 二维结论 |
// | --- | --- | --- | --- | --- |
// | **F-1** | raw string 未处理（[`mask_noncode`] 与 [`balanced_call`] 都是） | 中 | 中 | 决策（**但理由已作废重写**） |
// | **F-3** | 白名单 [`CALLED_IDENTS_ALLOWED`] **只按名字放行**，注释里的「标准」代码从不校验 | 低 | 中 | 决策（理由未变） |
// | **F-4** | [`fn_body`] **首个匹配即返回** + 在未屏蔽视图里找 `fn` | 低 | **低**（十余行） | 决策（**成本栏已改**） |
// | **C4** | [`connect_sites`] 豁免**任何** `_` 结尾前缀 | **高**（叫 `raw_connect` 即可） | **低**（一处前缀枚举） | ⚠️ **不是决策，是欠账** |
// | **F-5** | 覆盖判据 [`test_every_test_side_connect_site_goes_through_retry`] **只枚举两个测试文件** | **高**（新增测试文件即漏） | **低**（AC-001 改造后可在运行时枚举） | ⚠️ **不是决策，是欠账** |
//
// ⚠️ **C4 与 F-5 在二维判据下从「决策」降级为「欠账」。**
// 本需求 AC-003/004 只要求**改文字**，AC-006 又钉住测试条数不得增加，
// 所以它们**不进本轮**；但封口**必须照实写** —— 继续记成
// 「刻意构造所以是决策」就是在重复 REQ-044 的失真。
//
// # REQ-045 本身：判据把源码**内嵌在编译期**（不在上表，因为它已被修掉）
//
// 按同一判据留个坐标：自然度**高**（判据的内容只在**编译那一刻**取一次 ——
// 不经 cargo 直跑已编译二进制、或构建产物被跨 worktree 复用时，
// 它就在检查过期源码，且**一声不响**）、修复成本**低**（改一处读取方式）
// ⇒ **高 + 低，当场就该修**，已在本需求修掉。
// 它此前被漏掉，原因**不是**它「需要刻意构造」——恰恰相反，它**零刻意**；
// 而是**没人用这两维去看它**，只顺手用一维判据扫了一遍「刻意构造」。
// 这也是 REQ-044 一维判据失效的第一个实证。

// ## F-1 · raw string 未处理 —— REQ-045 订正：REQ-044 的理由**已被实测推翻**
//
// **REQ-044 原话（作废）**：「本仓实参无 raw string … 对本判据已经够用
// （raw string 里的 `for` / `sleep(` **不会露出来**）」。
//
// **QA 探针 P7d / P7 实测（二进制 `test_connect_retry`）**：
//
// | 构造 | 实测结果 | 性质 |
// | --- | --- | --- |
// | **P7d**：raw string 内**引号数为偶数** | `sleep(` **泄进屏蔽视图** ⇒ 退避判据**判绿**；真身是 **3 次尝试、零退避** | **假绿**（最危险的方向） |
// | **P7**：raw string 内**引号数为奇数** | 扫描器**跨行失步** ⇒ 直接 **panic** | 方向保守，但**失败模式失控**（不是「报红并说明是哪一步看不懂」） |
//
// ⇒ 「raw string 里的 `sleep(` 不会露出来」**是错的**，且错的方向恰好是**假绿**。
// 「本仓实参无 raw string」只能说明**今天**为真，推不出「无害」——
// 这与 REQ-043 拆掉的那条「判据不能依赖『现在恰好没有』」是同一条教训。
//
// **二维评估**：自然度**中**（raw string 是 Rust 日常写法，但要放进
// `retry_connect` 实参 / 重试循环体里才触发）；修复成本**中**
// （`balanced_call` 与 `mask_noncode` 各加一个 raw string 分支；
// 保守近似即可 —— 判「`r#`/`br#` 后紧跟 `"` 进 raw 态、按 `#` 数匹配 `"#`」，
// 与 [`is_char_literal`] 同一套保守取舍，**不必**引入完整 Rust 词法分析）。
// ⇒ 中 × 中 ⇒ 登记为决策，但按上面那条方向性约束，
// **假绿的构造已写明**：raw string 里放一个引号数为偶数、含 `sleep(` 的片段。
//
// # 这五条与 REQ-044 修掉的那两条（W5 / W5b）有什么不同
//
// W5 / W5b **不需要刻意构造**就能撞上：任何一次「把退避挪走」或「把重试挪走」的重构
// 都会踩中 —— 那是真实开发里最常见的两种改动形态。二维判据下它们的坐标是
// **自然度 高 × 修复成本 低（REQ-044 已修）**，正是「当场就该修」的那一格。
//
// ## F-4 · `fn_body` 首个匹配即返回 —— REQ-045 订正：**成本论证是错的**
//
// **REQ-044 原话（作废）**：「修好它需要**完整 Rust 解析器**，
// **远超本仓判据的合理成本**」。
//
// **QA 反驳**：**成本论证错误** —— REQ-044 自己刚在 [`all_retry_loops`] 用
// 「**枚举全部、不取第一个**」杀掉 W5b，**同一手法套到 [`fn_body`] 上仅十余行**。
// 既然同一手法在本文件里已被证明可行且便宜，「需要完整 Rust 解析器」就不成立。
//
// **现象本身仍成立**（静态核算，**未实测**，依据是 `fn_body` 的实际代码）：
// - `fn_body` 在 [`normalize`] 处理过的 `pool.rs` 上找 `fn <name>`，而 `normalize`
//   **只丢整行 `//` 注释** —— **行尾注释与字符串字面量仍在视图里**
//   ⇒ 一行 `const … = "fn connect_for_tests(&url) {"` 能被当成定义抢先匹配；
// - 「首个匹配」要靠**同名**定义抢先才会错，而同名 `fn` 在同一 crate 里**编译不过**
//   ⇒ **这一半在今天的 `pool.rs` 上几乎不可触发**，真实触发面是上一条。
//
// **二维评估**：自然度**低**（要写出 `const … = "fn …"` 这类反常形状）
// ⇒ 维持决策；但**成本栏必须写「低」**，别再用「需要完整 Rust 解析器」吓人。
//
// ## C4 · `_` 结尾前缀豁免 —— REQ-045 订正：**描述被缩小，且与代码路径对不上**
//
// **REQ-044 原话（作废）**：「豁免是**刻意设计**（允许 `_unused` 这类惯用法）」。
//
// **QA 反驳**（读 `connect_sites` 里 `before.ends_with("_")` 那一行）：
// 1. **豁免范围被缩小地描述**：代码是 `ends_with("_")`，豁免**任何** `_` 结尾前缀
//    —— `raw_connect(` / `naked_connect(` 一律被放过，远不止「`_unused` 惯用法」；
// 2. **与代码路径对不上**：[`connect_sites`] 只认 `connect(`，而 `ends_with("_")`
//    把 `retry_connect(` / `after_connect(` 这两条**本来就该豁免**的**顺带**吞了 ——
//    拿「顺带覆盖」当「刻意设计」的证据站不住。
//
// **二维评估**：自然度**高** —— 只要把 helper 命名为 `raw_connect`
// （**日常命名**，「裸连接」的本意）就成立，**不需要任何刻意构造**；
// 修复成本**低** —— 把 `ends_with("_")` 换成显式的 `retry_` / `after_` 前缀枚举
// （**一处、一行**）。⇒ **高 + 低，按本节判据必须当场修**。
// 本需求 AC 只要求改文字，所以它**不进本轮**，但它是**欠账**而非**决策**。
//
// ## F-5 · 覆盖判据只枚举两个测试文件 —— REQ-045 订正：从决策**降为欠账**
//
// 现象不变：[`test_every_test_side_connect_site_goes_through_retry`] 的 `files`
// 数组**逐个列**出两个文件。新增第三个测试文件并在里头裸 `connect(&url)`
// ⇒ 判据**根本不读那个文件**，一声不响。
//
// **REQ-044 原话**：「枚举全仓的**误报面大于收益**」。
// **REQ-045 订正**：它把成本栏算错了。REQ-045 把读取改成**运行时**之后
// （见 [`read_source`]），「枚举 `tests/` 下的 `.rs`」不再需要 `include_str!` 的
// 字面量清单 ⇒ 成本从「不可持续地手工维护」降到**低**；而自然度是**高**
// ——「加一个新测试文件」是最日常的改动之一。⇒ **高 + 低 ⇒ 欠账，不是决策。**
//
// ## F-3 · 白名单按名字放行 —— 二维评估后**维持决策**（理由未变）
//
// [`CALLED_IDENTS_ALLOWED`] 只按**名字**放行，它上面那句注释
// 「标准是它不是本仓定义的函数」**代码从不校验**。
//
// 自然度**低**：触发它必须在本仓**定义一个与预置构造器同名的函数**
// （如自定义 `Ok` / `Err`），既要刻意、又要编译得过，等于「有意才能改」。
// 修复成本**中**：校验「它确实不是本仓定义的」要先定下本仓的解析范围
// （`pool.rs`？整个 `src/`？），这一步定范围比 F-4/C4 的十几行贵。
// ⇒ 低自然度 ⇒ 维持决策。

mod common;

use common::{
    backoff_for, retry_connect, ConnectRetryError, CONNECT_BACKOFF_BASE_MS,
    CONNECT_BACKOFF_CAP_MS, CONNECT_RETRIES, CONNECT_RETRY_BUDGET,
};
use ontok::db::pool;
use sqlx::postgres::{PgPool, PgPoolOptions};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// 假「建连接」闭包的类型别名 —— 只是让 `flaky()` 的签名能读。
type ConnectFut = Pin<Box<dyn Future<Output = Result<PgPool, pool::DbError>> + Send>>;

// ══════════════════════════════════════════════════════════════
// REQ-045 · AC-001 / AC-002 · 读「**当前**磁盘上的」源码
// ══════════════════════════════════════════════════════════════

/// 读 **crate 根相对路径** 指向的源码文件，返回其**当前**内容。
///
/// # 为什么必须是**运行时**读，而不是 `include_str!`（REQ-045 · AC-001 · 核心）
///
/// `include_str!` 把源码在**编译期**内嵌进本判据二进制。实测（cargo 1.98.1）：
/// `include_str!` 的目标**确实**进了 dep-info，所以改 `tests/test_db.rs` 会触发重编译
/// —— ⚠️ **「`include_str!` 不建立 cargo 的文件依赖追踪」在本工具链上不成立**，
/// 详见文件头的实测表，本函数不沿用那个说法。
///
/// 但内嵌的**真正**弱点与工具链无关：**内容只在编译那一刻取一次**。
/// 只要不重编译（最常见的是**直接执行** `target/debug/deps/test_connect_retry-<hash>`，
/// 绕开 cargo），判据读到的就是**过期源码**，且**一声不响**。实测 A/B：
///
/// | 二进制 | 磁盘上的 `tests/test_db.rs` 已被变异（摘掉 `retry_connect`） | 结果 |
/// | --- | --- | --- |
/// | 旧版（`include_str!`，干净树编译） | 是 | **12 passed / 0 failed** |
/// | 本文件（运行时读） | 是 | **11 passed / 1 failed** |
///
/// # ⚠️ REQ-046 · 锚点从「编译期常量」改成「运行时定位」—— **本段替换了 REQ-045 的原论证**
///
/// REQ-045 在这里写过这么一段（**已被证伪，如实保留结论以便追溯**）：
///
/// > 「第二条独立理由是**跨目录复用**……本函数按 `env!("CARGO_MANIFEST_DIR")`
/// > 取**本 crate 的根**，即使二进制被跨 worktree 复用也读不到邻居的源码。」
/// > 「它是编译期常量，但指向本 crate 的根，不是运行时的 cwd。」
/// > 「⚠️ 诚实说明一处残留约束：它是编译期常量，所以『编译之后把整个仓库搬走』
/// > 会指向旧路径。」
///
/// **那段论证是错的，而且错在最关键的地方。** `CARGO_MANIFEST_DIR` 确实是
/// 「本 crate 的根」，但它**同时也是编译期常量** —— 这两件事合起来意味着：
/// 它锚定的是「**编译它时**的那棵树」，不是「**你正在跑它时**的那棵树」。
///
/// 后果不是理论上的。REQ-045 合并后第一次全量即 **506 passed / 30 failed**，
/// `CARGO_MANIFEST_DIR` 指向 `verify-REQ-044` —— **一个早已回收的 worktree**。
/// REQ-045 把这一条归类成「诚实说明」，**BA 定性：一定会咬人的已知限制是缺陷，不是说明**。
///
/// ⇒ 现在的锚点（见 `tests/common/source_root.rs`）是**每次运行重新定位**：
/// 先找**运行时工作目录**（上溯至多 8 层、且必须核对 `name` 与本 crate 相同），
/// 编译期目录**仅作兜底**。`cargo test` 起的测试进程 cwd 就是包根，
/// 所以**正常路径下两者等价、判据取值不变**；变的是**事故路径**。
///
/// # 不用 `build.rs` + `cargo:rerun-if-changed`
///
/// `build.rs` 仍然是**编译期**求值：它最多把「编译时取当前内容」做得更准，
/// **解决不了**「二进制已经编好、文件再改」这个真正的失效模式
/// （上表 A 行就是这种情形：不经 cargo 直跑时，`build.rs` 一样救不了）。
/// AC-001 要的是「**每次运行都取当前内容**」，只有运行时读给得起。
///
/// # AC-002 · **fail-closed**：读不到就报错，绝不静默跳过
///
/// `include_str!` 有个隐性好处：**文件不在就编不过**。改成运行时读之后
/// 这个保证消失了，**必须显式补回来** —— 否则「文件被改名 / 移动」会让判据
/// **悄悄不再检查**它本该检查的源码，那比「判据检查过期源码」更难发现。
///
/// 所以这里**不存在「读不到就当没有」这条分支**：读失败直接 panic。
///
/// # ⚠️ REQ-046 · fail-closed **不许退化**，且必须**分两类**报错
///
/// REQ-045 的 fail-closed 只有一句 `No such file or directory` ——
/// 它把「**目录不对**」和「**文件真不存在**」混成了同一句，
/// 于是「二进制在已回收的 worktree 里编的」被读成「源码被删了？」。
/// **那是那次事故里最有害的部分**：报错把人引向错误方向。
///
/// 所以现在两类分开说：
///
/// | 类别 | 触发条件 | 报错开头 |
/// | --- | --- | --- |
/// | **B · 目录不对** | 定位不到任何可用的 crate 根 | 【失败类型】**二进制与当前目录不匹配** |
/// | **A · 文件真不存在** | 根找对了、文件不在 | 【失败类型】**文件真的不存在** |
///
/// 类别 B 额外打印**编译期目录**（及其是否还在）、**你运行它时的 cwd**、
/// **二进制自身位置**与修法 —— 让人一眼看出「这是目录不对，不是文件丢了」。
///
/// 实测：把 `tests/test_db.rs` 移走再跑判据 ⇒ `test_every_test_side_connect_site_goes_through_retry`
/// 立刻 panic；**类别 A**。把整个 worktree 回收后从别处直跑 ⇒
/// 同样是立刻 panic，但打的是**类别 B**。
///
/// ⚠️ 一处粒度上的诚实说明：fail-closed 是**按测试**的 —— 只有**读那个文件**的
/// 那一条测试会红（移走 `test_db.rs` 时是 12 条里的 1 条红），不是整个二进制全红。
/// 这是**响的**（红 + 精确路径），但不是「一条不读全崩」。
///
/// # ⚠️ REQ-046 · 为什么本文件不再自带实现
///
/// 函数体迁到 `tests/common/source_root.rs`，本文件只导入。
/// 同一个错锚点在仓内共有 **6 处** —— 各自带一份实现，就意味着下一次
/// 只改一处、另外五处照旧。**唯一入口**才有唯一可审计的行为。
///
/// 上文关于「为什么运行时读」的论证**依然成立、且未被削弱**：
/// A/B 表证明的是「内嵌内容只在编译那一刻取一次」这个弱点，
/// REQ-046 改的是**在哪个目录取** —— 两者互补，不是二选一。
use common::read_source;


// ══════════════════════════════════════════════════════════════
// 夹具
// ══════════════════════════════════════════════════════════════

/// 造一个**不连任何东西**的池 —— `connect_lazy` 只解析 URL，不建连接。
/// 本文件的所有判据只需要一个「成功」的返回值当占位符。
fn lazy_pool() -> PgPool {
    PgPoolOptions::new()
        .connect_lazy("postgres://127.0.0.1:1/none")
        .expect("URL 合法即可 —— connect_lazy 不发起连接")
}

/// 造一个「建连接失败」的错误，形态取自实测高负载时的两种之一。
fn connect_err() -> pool::DbError {
    pool::DbError::Connect(sqlx::Error::PoolTimedOut)
}

/// 「前 `fail_first` 次失败、之后成功」的假建连接；返回实际被调用的次数。
fn flaky(fail_first: usize) -> (Arc<AtomicUsize>, impl FnMut() -> ConnectFut) {
    let calls = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&calls);
    let f = move || {
        let n = c.fetch_add(1, Ordering::SeqCst) + 1;
        Box::pin(async move {
            if n <= fail_first {
                Err(connect_err())
            } else {
                Ok(lazy_pool())
            }
        }) as ConnectFut
    };
    (calls, f)
}

// ══════════════════════════════════════════════════════════════
// AC-001 · 前 N 次失败能重试成功
// ══════════════════════════════════════════════════════════════

/// AC-001：`test_pool()` 在前 N 次建连接失败时**重试并拿到连接**，
/// 而不是像修复前那样直接 `panic!("连不上测试库")`。
///
/// 判别力：把 `CONNECT_RETRIES` 改成 0（本文件的变异 1）⇒
/// `fail_first = 2` 时闭包只会被调 1 次，返回 `Err` ⇒ 本条立刻红。
#[tokio::test]
async fn test_connect_retry_succeeds_after_transient_failures() {
    let (calls, mut attempt) = flaky(2);
    let r = retry_connect(&mut attempt).await;
    assert!(r.is_ok(), "前 2 次失败、第 3 次成功时必须拿到连接，实际 {:?}", r.err());
    assert_eq!(calls.load(Ordering::SeqCst), 3, "应当首发 + 2 次重试");
}

/// 边界：**恰好**用完重试次数（失败 2 次 = 上限）也必须成功。
/// 少试一次就会红 —— 这条专门卡「次数算错一位」。
#[tokio::test]
async fn test_connect_retry_uses_exactly_the_budget() {
    let (calls, mut attempt) = flaky(CONNECT_RETRIES);
    let r = retry_connect(&mut attempt).await;
    assert!(r.is_ok(), "失败次数正好等于重试上限时应成功");
    assert_eq!(calls.load(Ordering::SeqCst), CONNECT_RETRIES + 1);
}

/// 正常路径**零额外开销**：第一次就成功时闭包只跑一次，且不引入退避等待。
///
/// 这是 AC-005「耗时不显著劣化」在测试侧的兜底：加退避最容易犯的错是
/// 在**第一次尝试之前**也睡一下，等于给每条测试无条件加了 300ms。
#[tokio::test]
async fn test_connect_retry_does_not_retry_or_wait_on_first_success() {
    let (calls, mut attempt) = flaky(0);
    let t0 = Instant::now();
    let r = retry_connect(&mut attempt).await;
    let spent = t0.elapsed();
    assert!(r.is_ok());
    assert_eq!(calls.load(Ordering::SeqCst), 1, "首次成功时不得再调第二次");
    assert!(
        spent < Duration::from_millis(100),
        "首次成功的路径不该有退避等待，实测花了 {spent:?}"
    );
}

// ══════════════════════════════════════════════════════════════
// AC-002 · 有界 + 可读 + 不挂死
// ══════════════════════════════════════════════════════════════

/// AC-002 之一：**一直**失败时，有限次后**仍然失败**（而不是无限重试）。
///
/// ⚠️ 这条判据同时是 AC-004 变异 3（重试次数写成极大值 / 无界）的**杀手**：
/// 变异版本会一直重试下去，被下面那条 `timeout` 掐住 ⇒ 红。
#[tokio::test]
async fn test_connect_retry_gives_up_on_persistent_failure() {
    let calls = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&calls);
    let r = retry_connect(|| {
        let c = Arc::clone(&c);
        async move {
            c.fetch_add(1, Ordering::SeqCst);
            Err::<PgPool, _>(connect_err())
        }
    })
    .await;
    assert!(r.is_err(), "一直失败时必须最终失败，不能无限重试");
    assert_eq!(
        calls.load(Ordering::SeqCst),
        CONNECT_RETRIES + 1,
        "失败时只应尝试首发 + {CONNECT_RETRIES} 次重试"
    );
}

/// AC-002 之二：**不得让测试挂死**。
///
/// 判别力：把重试次数改成极大值 / 无界 ⇒ 闭包每次耗时 200ms，
/// 无界版本永远跑不完 ⇒ 这里 12s 的 `timeout` 触发 ⇒ **红**，
/// 而不是把测试套件卡死（那正是 AC-002 要防的事）。
#[tokio::test]
async fn test_connect_retry_never_hangs_under_persistent_failure() {
    let r = tokio::time::timeout(
        Duration::from_secs(12),
        retry_connect(|| async {
            tokio::time::sleep(Duration::from_millis(200)).await;
            Err::<PgPool, _>(connect_err())
        }),
    )
    .await
    .expect("12s 内必须返回 —— 无界重试会让测试套件卡住，那比现在更糟");
    assert!(r.is_err(), "一直失败时最终必须失败");
}

/// AC-002 之三：墙钟上界确实**大于**实际最坏耗时 ⇒ 当前配置下它不误伤。
///
/// `connect_for_tests` 的 `acquire_timeout=10s` 封顶单次尝试，
/// 所以最坏 = (2+1)×10s + 0.3s + 0.6s = 30.9s。这条把「35s 不是拍脑袋」
/// 钉成可执行断言：哪天有人把预算调到 30s 以下而没改 `acquire_timeout`，
/// 本条立刻红（预算会把第 3 次尝试掐掉）。
#[test]
fn test_retry_budget_exceeds_the_realistic_worst_case() {
    let per_attempt = Duration::from_secs(10); // connect_for_tests 的 acquire_timeout
    let worst = per_attempt * (CONNECT_RETRIES as u32 + 1)
        + backoff_for(1)
        + backoff_for(2);
    assert!(
        CONNECT_RETRY_BUDGET > worst,
        "墙钟上界 {CONNECT_RETRY_BUDGET:?} 必须大于实际最坏耗时 {worst:?}，\
         否则它会在正常参数下就掐断最后一次尝试"
    );
}

/// AC-002 之四：**错误可读** —— 能看出试了几次、最终原因是什么。
///
/// 判别力：把错误信息里的次数去掉、或把「最后一次原始错误」吞掉，本条立刻红。
#[tokio::test]
async fn test_connect_retry_error_is_readable() {
    let e: ConnectRetryError = retry_connect(|| async { Err::<PgPool, _>(connect_err()) })
        .await
        .expect_err("一直失败必须返回 Err");
    let msg = e.to_string();

    assert_eq!(e.attempts, CONNECT_RETRIES + 1, "错误里要能取到尝试次数");
    assert!(
        msg.contains(&format!("已尝试 **{}** 次", CONNECT_RETRIES + 1)),
        "错误信息里必须写出尝试次数，实际：\n{msg}"
    );
    assert!(
        msg.contains(&connect_err().to_string()),
        "错误信息里必须原样带出最后一次的底层错误（`{}`），实际：\n{msg}",
        connect_err()
    );
    assert!(
        msg.contains(&CONNECT_RETRIES.to_string()) && msg.contains("重试全部用完"),
        "错误信息里必须写出重试次数与「为什么停了」，实际：\n{msg}"
    );
}

/// 退避是**指数**的、有封顶，且与常量自洽。
#[test]
fn test_backoff_is_exponential_and_capped() {
    assert_eq!(backoff_for(1), Duration::from_millis(CONNECT_BACKOFF_BASE_MS));
    assert_eq!(backoff_for(2), Duration::from_millis(CONNECT_BACKOFF_BASE_MS * 2));
    assert_eq!(backoff_for(3), Duration::from_millis(CONNECT_BACKOFF_BASE_MS * 4));
    // 封顶
    assert_eq!(backoff_for(20), Duration::from_millis(CONNECT_BACKOFF_CAP_MS));
    // 不许溢出 panic（shift 溢出是这一类代码的经典坑）
    assert_eq!(backoff_for(usize::MAX), Duration::from_millis(CONNECT_BACKOFF_CAP_MS));
}

// ══════════════════════════════════════════════════════════════
// AC-003 · 重试范围只限建连接
// ══════════════════════════════════════════════════════════════

/// 读 `tests/common/mod.rs` 的**代码**（跳过整行的 `//` 注释与空行，逐行归一化）。
/// 沿用 `test_snapshot_write_paths.rs::code_text` 的既有做法。
///
/// ⚠️ REQ-045：`include_str!` → [`read_source`]，取的是**运行时的当前内容**。
fn code_text() -> String {
    normalize(&read_source("tests/common/mod.rs"))
}

/// # 为什么**保留换行**（REQ-043-F1）
///
/// 原来是 `join(" ")` —— 所有行被压成一行。压行本身对 `contains` 类断言无害，
/// 但它会**毁掉行注释的语义**：`balanced_call` 的行注释状态靠 `\n` 收尾，
/// 源码里的换行一旦被换成空格，`// 注释` 就会把**后面所有代码**当成注释吞掉，
/// 于是配平永不归零 ⇒ 良性代码被判成红（正是 BC 那条假红）。
///
/// 所以这里改成 `join("\n")`：仍然逐行 trim、仍然丢掉整行注释，
/// 但**行边界还在**，行注释才有一个可收尾的边界。
/// 代价是 `normalize` 不再幂等于「一整行」，但它对 `normalize(·)` 二次调用是幂等的
/// （trim 过的行再 trim 不变），`fn_body` 里那层 `normalize(body)` 仍安全。
///
/// 行**内**注释（`code; // …`）与块注释 `/* … */` 都不在这里剥。
///
/// # REQ-044 · 订正这段理由的因果（**论证错了，设计不用改**）
///
/// 原文写的是「剥掉它们会让『禁用词写在注释里』从『红』变成『绿』，那等于新开一个假绿洞」。
/// **这个因果是反的**：注释里的 `migrate` **根本不执行**，剥掉它只会**减少假红**
/// （少一处无意义的告警），**不可能造出假绿**。拿「防假绿」当不剥的理由站不住。
///
/// 那么**为什么仍然不剥**？真实理由是另外两条：
///
/// 1. **判据问的是「源码里有没有写过这个词」，那是可审计的文本事实。**
///    注释是源码的一部分，而「这行注释是否在误导后来的人」正是评审要看的东西；
///    判据在这里的价值是**把可疑文本摆到人眼前**（宁可假红去多看一眼），
///    而不是替人判断它会不会执行。执行与否的判定交给**另一条**判据去做 ——
///    见 [`mask_noncode`]：判「退避有没有真的发生」时用的是屏蔽视图。
/// 2. **成本与收益不对称**：在这里剥注释要再引入一套状态机，而它对**任何真实逃逸**
///    的检出率**零提升** —— 注释里的禁用词本来就不是真实逃逸。
///
/// ⚠️ 别把这段与 [`balanced_call`] 里的注释处理混为一谈：**那里**必须在状态机里处理注释，
/// 理由是注释里的裸括号会**破坏配平**（`// )` 让窗口提前截断 ⇒ 假绿；`// (` 让配平永不归零 ⇒ panic），
/// 那是**配平正确性**问题，与「禁用词按文本出现即红」是两回事。
///
/// 两条判据的方向是**互补**的，不是同一个取向：禁用词 ⇒ 看文本（保守，宁假红）；
/// 退避 ⇒ 看执行（注释里的 `sleep` 不算）。REQ-044 之前缺的正是后半截。
fn normalize(src: &str) -> String {
    src.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

// ══════════════════════════════════════════════════════════════
// REQ-042 · 结构化提取（替掉 REQ-041 的定长 200 字符裸切片）
// ══════════════════════════════════════════════════════════════

/// 从 `src` 的 `open` 位置（一个 `(` 或 `{`）起，返回**括号配平**的那一段（含首尾）。
///
/// # 为什么必须是配平而不是定长切片
///
/// REQ-041 的判据取的是「`retry_connect(||` 之后的 200 字符」。实测三条绕过全活：
/// - **H2**：把 `migrate` 用填充顶到 200 字符窗口之外 —— 窗口大小是任意的，
///   填 30 个空格就出去了。定长窗口与「代码里有没有 migrate」**无任何逻辑关系**。
/// - **H4**：在更早处放一个诱饵 `retry_connect(|| pool::connect_for_tests(..))`，
///   而真正被放宽的那个在后面 —— `find` 只认第一个。
/// - **H1**：helper 名字里带 `connect_for_tests` 子串 —— `contains` 是子串匹配。
///
/// 配平提取把「实参」定义成**语法上的那个调用表达式**：
/// 无论里面填多少空格、多少行、还是嵌套多深，取到的都是同一个东西。
///
/// # 注释里的括号（REQ-043-F1 / REQ-043-F2）
///
/// 本函数原来只处理**字符串字面量**，**完全没处理注释** ——
/// 而同一个文件 `:248-252` 当时专门论证过「判据不能依赖『现在恰好没有』」。
/// 字符串防了、注释没防，是同一类陷阱漏了一半。实测后果两条方向都有：
///
/// | 构造 | 旧行为 | 性质 |
/// | --- | --- | --- |
/// | 行尾注释里的裸 `)`（`// )`） | 配平**提前归零** → 窗口截断 → 真正的 `migrate` 落在窗口外 | **假绿**（BA/BB：真的放宽了重试范围却全绿） |
/// | 块注释里的裸 `)`（`/* ) */`） | 同上 | **假绿**（BB） |
/// | 良性注释里的裸 `(`（`// (`） | 配平**永不归零** → panic | **假红**（BC：没问题的代码被判红） |
///
/// 假绿比假红危险，所以这次把注释当成**语法的一部分**处理：
/// `//` 到行尾、`/* … */` 到闭合（**Rust 的块注释可嵌套**，所以要计数，不能只找第一个 `*/`）。
/// 注释在**计数**时被跳过，但注释**文本仍留在返回值里** ——
/// 这样「禁用词写在注释里」依旧是红的（保守方向），不会因为剥注释而新开假绿。
///
/// # `'` 是字符字面量还是生命周期（`'_` / `'static`）
///
/// Rust 里 `'` 有两义：`')'` 是字符字面量，`'a` 是生命周期标注。
/// 判据必须区分，否则要么漏跳字符字面量（`')'` 把配平算歪 = BE 那条洞），
/// 要么把生命周期误当字面量、**一路吞到下一个 `'`**，把本该计入配平的括号吞掉 ——
/// 那会**藏起**禁用词，是最坏的方向。所以判据**保守**：
/// 只有「后面紧跟闭合单引号」的确定形式才算字面量，其余一律按生命周期处理、
/// 只跳过这一个字节。见 [`is_char_literal`]。
fn balanced_call(src: &str, open: usize) -> &str {
    let bytes = src.as_bytes();
    debug_assert!(
        matches!(bytes[open], b'(' | b'{'),
        "balanced_call 只能从开括号/开花括号起算"
    );
    let (o, cl) = match bytes[open] {
        b'(' => (b'(', b')'),
        b'{' => (b'{', b'}'),
        _ => unreachable!(),
    };
    let mut depth = 0usize;
    let mut i = open;
    let mut in_str = false;
    let mut escaped = false;
    let mut in_line_comment = false; // `// …` 到行尾（依赖 normalize 保留了换行）
    let mut block_depth = 0usize;     // `/* … */`，Rust 允许**嵌套** ⇒ 用计数而非布尔
    let mut in_char = false;
    let mut char_escaped = false;
    while i < bytes.len() {
        let c = bytes[i];

        // ── 注释态：最先判，且**不参与**括号计数 ──
        if in_line_comment {
            if c == b'\n' {
                in_line_comment = false;
            }
            i += 1;
            continue;
        }
        if block_depth > 0 {
            if c == b'/' && bytes.get(i + 1) == Some(&b'*') {
                block_depth += 1;
                i += 2;
                continue;
            }
            if c == b'*' && bytes.get(i + 1) == Some(&b'/') {
                block_depth -= 1;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }

        // ── 字符串字面量（原有逻辑，逐字保留）──
        if in_str {
            if escaped {
                escaped = false;
            } else if c == b'\\' {
                escaped = true;
            } else if c == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }

        // ── 字符字面量 ──
        if in_char {
            if char_escaped {
                char_escaped = false;
            } else if c == b'\\' {
                char_escaped = true;
            } else if c == b'\'' {
                in_char = false;
            }
            i += 1;
            continue;
        }

        // ── 代码态 ──
        match c {
            // 注释必须在字符串**之前**判：`//` 在字符串里是普通字面，
            // 反过来 `"` 在注释里也不该开字符串 —— 两个态互斥，顺序即正确性。
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                in_line_comment = true;
                i += 2;
                continue;
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                block_depth = 1;
                i += 2;
                continue;
            }
            b'"' => {
                in_str = true;
                i += 1;
                continue;
            }
            b'\'' if is_char_literal(bytes, i) => {
                in_char = true;
                i += 1;
                continue;
            }
            _ if c == o => depth += 1,
            _ if c == cl => {
                depth -= 1;
                if depth == 0 {
                    return &src[open..=i];
                }
            }
            _ => {}
        }
        i += 1;
    }
    panic!("从第 {open} 个字节起的括号没有配平 —— 判据读到的是残缺源码");
}

/// `bytes[i]` 位于**代码态**且是 `'`：它到底是字符字面量还是生命周期标注？
///
/// # 判据为什么必须**保守**（只朝一个方向错）
///
/// 误判的代价是**不对称**的：
/// - 把**字面量**误当生命周期 ⇒ 只多跳一个字节，退化成 REQ-043 之前的行为，不新增洞；
/// - 把**生命周期**误当字面量 ⇒ 扫描器会一路吞到下一个 `'`，**吞掉本该计入配平的括号**
///   —— 那会把禁用词藏起来，直接造出**假绿**。
///
/// 所以只有**确定**是字面量时才进字面量态：
/// - `'\…'`：反斜杠后必然是转义（生命周期标注不可能以 `\` 跟随）；
/// - `'x'`：恰好一个字节后紧跟闭合单引号（生命周期标注后面永远不是 `'`）。
///
/// 落在这两条之外的（`'static` / `'_` / `'a` / 标签 `'outer:`）一律按生命周期处理。
fn is_char_literal(bytes: &[u8], i: usize) -> bool {
    match bytes.get(i + 1) {
        Some(&b'\\') => true,
        Some(_) => bytes.get(i + 2) == Some(&b'\''),
        None => false,
    }
}

/// 找出**每一个** `name(` 的调用点（不是定义），返回各自配平的实参。
///
/// # 为什么是「每一个」而不是 `find` 的第一个
///
/// H4 就是靠「只认第一个」活下来的：更早处放一个干净的诱饵，
/// 真正被放宽的调用点排在后面，判据压根没看见它。
/// 这里把**所有**调用点都取出来逐个检查 —— 诱饵不仅要自己干净，
/// 后面那个真身也逃不掉。
fn all_call_args(src: &str, name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = src[from..].find(name) {
        let at = from + rel;
        from = at + name.len();
        // `retry_connect` 后面必须紧跟 `(` 才算调用；
        // `fn retry_connect<F, Fut>(` 这种定义不算。
        if src[from..].starts_with('(') {
            out.push(balanced_call(src, from).to_string());
        }
    }
    assert!(
        !out.is_empty(),
        "`{name}(` 在 common/mod.rs 里一个调用点都没有 —— 判据锚定的东西没了"
    );
    out
}

/// AC-003：**只有建连接被重试**，「连上之后的任何失败」照旧暴露。
///
/// # 判据钉的是「被重试的代码里不许有什么」，不是「文本长什么样」
///
/// REQ-041 的判据是「`retry_connect(||` 之后 200 字符里不许出现 `migrate`」。
/// 那是**在文本形状上**找证据，于是：
/// - 填空格就能把 `migrate` 顶出窗口（H2）；
/// - 在更早处放个诱饵就能让判据看错对象（H4）；
/// - `contains("connect_for_tests")` 是**子串**匹配，所以一个叫
///   `connect_for_tests_but_migrate` 的 helper 能同时满足「含 connect_for_tests」
///   和让行为面真被绕过（H1）。
///
/// 现在判据由三条组成，**每条都以「不许有什么」的形式表达**：
///
/// 1. **实参里不许出现** `migrate` / `execute` / `query` / `fetch` / `begin` / `commit`
///    —— 但取的是**配平后的整个实参**，填多少空格都出不去（H2 死）。
/// 2. **每一个**调用点都要过这一关 —— 诱饵救不了真身（H4 死）。
/// 3. 被调用的那个函数**在源码里必须真的是「建连接」**：
///    实参里出现的标识符，逐个回到 `src/db/pool.rs` 找它的定义，
///    取**配平的函数体**，断言那个体里**也不许**出现上面那 6 个词 ——
///
///    第 3 条是唯一能杀死 **H1** 的一条。H1 的本质是：
///    「重试的范围」不由**调用点**决定，而由**被调用函数的体**决定。
///    判据只看调用点，就永远看不见 helper 体内藏着的 `migrate`。
///
///    REQ-043：这条原来是**半文本** —— 它靠 `ident.contains("connect")` 决定追不追，
///    于是「helper 起个不含 `connect` 的名字」就成了一条现成绕过（实测 R3b 全绿）。
///    现在改成**白名单制**：每个被调标识符都必须解析到干净体，**解析不到就红**，
///    豁免只能由显式的 [`CALLED_IDENTS_ALLOWED`] 逐条给出。见该常量的注释。
#[test]
fn test_retry_scope_is_connect_only() {
    let code = code_text();
    let pool_src = normalize(&read_source("src/db/pool.rs"));
    const FORBIDDEN: [&str; 6] = ["migrate", "execute", "query", "fetch", "begin", "commit"];

    for arg in all_call_args(&code, "retry_connect") {
        // 反向确认（防空断言）：实参里**确实**是在建连接
        assert!(
            arg.contains("connect"),
            "被重试的实参必须是在建连接，实际拿到：\n{arg}"
        );
        // 第 1 条：整个配平实参里不许有「连上之后」的动作
        for forbidden in FORBIDDEN {
            assert!(
                !arg.contains(forbidden),
                "重试范围被放宽了：`retry_connect` 的实参里出现了 `{forbidden}` —— \
                 只有**建连接**阶段的失败可以重试，连上之后的迁移/查询失败必须照旧暴露。\
                 实测实参（配平后的完整调用表达式）：\n{arg}"
            );
        }

        // 第 3 条：实参里**被调用**的标识符，逐个必须解析到一个**干净体**（白名单制）。
        //
        // 只看「被调用」的（紧跟 `(` 的）标识符 —— 变量名（`max_connections`）
        // 不是被重试的行为，追它的定义毫无意义。
        for ident in called_idents(&arg) {
            if CALLED_IDENTS_ALLOWED.contains(&ident.as_str()) {
                continue;
            }
            // REQ-043 · AC-004：这里原来是「名字不含 connect 就放过」。
            // 那是个**按名字开洞**的判据：helper 起个不含 `connect` 的名字（`qa_hook`）
            // 就能整体藏进被重试的范围，而判据连追都不追（实测 R3b 全绿）。
            // 改成白名单：**解析不到就红**，能不能豁免由显式白名单说了算，
            // 而不是由名字长得像不像决定。
            let Some(body) = fn_body(&pool_src, &ident) else {
                panic!(
                    "被重试的实参调用了 `{ident}`，但它在 `src/db/pool.rs` 里**解析不到干净函数体** —— \
                     「重试的范围」由**被调函数的体**决定，判据看不见这个体就等于没判定，\
                     建连接入口也可能被挪到别处藏住 migrate。\
                     修法：把它收进 `src/db/pool.rs` 的具名函数（像 `connect_for_tests` 那样），\
                     或若它确实是标准库方法，加进本文件底下的 `CALLED_IDENTS_ALLOWED` 并写明理由。\
                     实测实参（配平后的完整调用表达式）：\n{arg}"
                );
            };
            for forbidden in FORBIDDEN {
                assert!(
                    !body.contains(forbidden),
                    "重试范围被放宽了：被重试的实参调用了 `{ident}`，而它在 \
                     `src/db/pool.rs` 里的**函数体**里出现了 `{forbidden}` —— \
                     建连接这一步内部做了「连上之后」的事，于是「连上之后的失败」\
                     也会被重试（实测已被重试 3 次）。实测函数体：\n{body}"
                );
            }
        }
    }
}

/// 被重试的实参里、**允许**解析不到 `pool.rs` 干净体的标识符。
///
/// # 为什么是白名单，而不是「名字含 connect 才追」
///
/// 旧判据按**名字**决定追不追，于是「起个不含 `connect` 的名字」就是一条现成的绕过
/// （实测 R3b：`pg::qa_hook` 里藏 migrate，全绿）。名字是**攻击者可控**的，
/// 拿它当放行条件等于把洞交给对方。
///
/// # 每一项都必须写明「为什么它不可能是藏 migrate 的地方」
///
/// 白名单的标准是**它不是本仓定义的函数** —— Rust 预置的枚举构造器 / 标准库方法
/// 没有「本仓的函数体」可追，也就没有本仓的 `migrate` 能藏进去。
///
/// **注意豁免不等于放行**：第 ① 条（整个实参不许出现 6 个禁用词）照旧覆盖实参全文，
/// 注释与字面量里的禁用词一样算命中；而任何**本仓定义**的 helper 只要被调用，
/// 它自己就是一个独立的被调标识符、必须解析得到。所以「往 `Ok(...)` 里塞个
/// `qa_hook(...)`」骗不过本条 —— `qa_hook` 自己会被拦。
const CALLED_IDENTS_ALLOWED: &[&str] = &[
    // Rust 预置的枚举构造器：不是函数、没有函数体。
    // 实测 `BG` 控制轮证明漏掉它们会**假红** ——
    // 任何写成 `|| async { … Ok(p) }` 的合法闭包里都会跟着一个 `Ok(`。
    "Ok", "Err", "Some", "None",
];

/// 实参里出现的**被调用标识符** —— 即紧跟 `(` 的那些名字。
///
/// 变量名（`&url` / `max_connections`）不是被调用的行为，排除掉：
/// 否则 `max_connections` 之类会被当成「被重试的函数」去 pool.rs 里找定义。
fn called_idents(arg: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = arg.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_alphabetic() || c == b'_' {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            // 紧跟 `(` 才算「被调用」；`::` 前缀取最后一段（`pool::connect_for_tests` → `connect_for_tests`）
            if bytes.get(i) == Some(&b'(') {
                out.push(arg[start..i].to_string());
            }
        } else {
            i += 1;
        }
    }
    out
}

/// 取 `src` 里 `fn <name>`（含 `async fn`）的**配平函数体**；找不到返回 `None`。
fn fn_body(src: &str, name: &str) -> Option<String> {
    let pat = format!("fn {name}");
    let mut from = 0usize;
    while let Some(rel) = src[from..].find(&pat) {
        let at = from + rel;
        from = at + pat.len();
        let rest = &src[from..];
        // `fn connect_for_tests` 的下一个非空白字符必须是 `(` / `<` / `(` 前的空格，
        // 否则 `fn connect_for_tests_and_migrate` 会误匹配 `connect_for_tests`。
        let trimmed = rest.trim_start();
        if !(trimmed.starts_with('(') || trimmed.starts_with('<')) {
            continue;
        }
        // 跳过泛型参数与参数列表，找到函数体的 `{`
        let body_open = find_body_open(src, from)?;
        let body = balanced_call(src, body_open);
        return Some(normalize(body));
    }
    None
}

/// 从 `from`（刚过 `fn <name>`）起，找到函数体开头的 `{`。
fn find_body_open(src: &str, from: usize) -> Option<usize> {
    let bytes = src.as_bytes();
    let mut i = from;
    let mut paren = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => paren += 1,
            b')' => paren = paren.saturating_sub(1),
            // `;` 说明这是个声明/签名而不是定义
            b';' => return None,
            b'{' if paren == 0 => return Some(i),
            _ => {}
        }
        i += 1;
    }
    None
}

/// AC-001：**每一个测试侧的建连接点都必须落在重试之内**。
///
/// # 这条是 AC-004「把重试摘掉 ⇒ 须红」所依赖的判据
///
/// AC-001 给 4 处补了重试，但**补了不等于钉住了**：实测把
/// `tests/test_db.rs` 里 `test_pool_migrate_race` 的 `retry_connect(...)`
/// 摘掉退回裸 `connect(&url)`，`test_db` 二进制**11 条全绿** ——
/// 因为 AC-002 的判据只管「重试的范围别被放宽」，**没有一条判据管
/// 「该重试的地方有没有重试」**。覆盖缺口就是这么漏过去的。
///
/// 本条把「不许漏」钉死：逐个文件枚举出**所有**建连接调用点，
/// 断言每一个都被 `retry_connect` 包住。
///
/// # 为什么是「枚举 + 计数」而不是纯搜索
///
/// 纯搜索（`assert!(!src.contains("connect(&url)"))`）会因为
/// `retry_connect` 自己的**实参里就含有** `connect(...)` 而永远红。
/// 所以这里取 `retry_connect` 配平实参的**区间**，再断言
/// 「所有建连接调用的位置都落在某个区间内」—— 区间外的建连接点即漏网。
///
/// # 允许的例外只有一个，且必须写明理由
///
/// [`UNWRAPPED_ALLOWED`] 里每一项都必须带一句「为什么不套」。
/// 新增例外 = 新增一个可查证的决策点，而不是「悄悄不重试」。
///
/// # 为什么 `src/db/apply.rs` 不在本条范围内
///
/// 它在 `#[cfg(test)] mod tests` 里，**不是生产代码**，而且集成测试
/// 够不着它（库不能依赖 `tests/`），所以它用的是**就地重写**的一份。
/// 它的覆盖由 [`test_lib_unit_test_connect_also_retries`] 单独钉。
#[test]
fn test_every_test_side_connect_site_goes_through_retry() {
    // (文件, 该文件里建连接调用点的绝对偏移区间, ...) —— 逐个文件查
    // (文件源码, 文件名) —— 逐个列出来（REQ-045 起改为运行时读，见 [`read_source`]；
    // 这条列表同时就是 F-5「只枚举两个测试文件」的现场，处置见顶部决策记录）
    let files = [
        (normalize(&read_source("tests/test_db.rs")), "tests/test_db.rs"),
        (normalize(&read_source("tests/common/mod.rs")), "tests/common/mod.rs"),
    ];
    for (src, rel) in files {

        // 收集所有 retry_connect 实参的区间
        //
        // ⚠️ 区间必须以 **`retry_connect` 标识符的起点**为基准，
        // 与下面 `connect_sites` 用的基准一致 ——
        // 否则内层 `connect(` 落在区间外，判据对自己的调用点也会红。
        let mut covered: Vec<(usize, usize)> = Vec::new();
        let mut from = 0usize;
        while let Some(rel_i) = src[from..].find("retry_connect") {
            let at = from + rel_i;
            let after = at + "retry_connect".len();
            if src[after..].starts_with('(') {
                let arg = balanced_call(&src, after);
                covered.push((at, at + arg.len() + "retry_connect".len()));
                from = after;
            } else {
                from = after;
            }
        }

        // 枚举建连接调用点
        for (pos, line) in connect_sites(&src) {
            if covered.iter().any(|(s, e)| pos >= *s && pos < *e) {
                continue;
            }
            // 例外白名单
            if UNWRAPPED_ALLOWED.iter().any(|(f, needle, _)| *f == rel && line.contains(needle)) {
                continue;
            }
            panic!(
                "`{rel}` 里有一处建连接**没有**被 `retry_connect` 包住 —— \
                 建连接阶段的抖动会直接打脸这条测试。实测那一行：\n{line}\n\
                 修法：把它包进 `common::retry_connect(|| …)`。\
                 确实不能重试的话，加进本文件底下的 `UNWRAPPED_ALLOWED` 并写明理由。"
            );
        }
    }
}

/// 确实不套重试的建连接点 —— **每一项都必须写明理由**。
///
/// 空表：REQ-042 结束时，测试侧**没有任何一处**建连接是故意不重试的。
const UNWRAPPED_ALLOWED: &[(&str, &str, &str)] = &[];

/// 在（已去注释的）源码里找出所有**建连接调用点**：(绝对偏移, 所在行文本)。
fn connect_sites(src: &str) -> Vec<(usize, String)> {
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(rel_i) = src[from..].find("connect") {
        let at = from + rel_i;
        let after = at + "connect".len();
        from = after;
        // 只认「被调用」的：`connect(` —— 排除 `connect_for_tests` 的定义处等
        if bytes.get(from) != Some(&b'(') {
            continue;
        }
        // ⚠️ 排除 `retry_connect(` 自身（它的实参区间由上面单独算）
        // 与 `after_connect(` —— 后者是**每条连接建好之后**跑的回调，
        // 不是建连接动作，把它算进来是假阳性。
        let before = &src[..at];
        if before.ends_with("retry_") || before.ends_with("after_") || before.ends_with("_") {
            continue;
        }
        // 取所在行（`normalize` 保留了换行，所以「行」= 到下一个
        // 分号为止的片段 —— 本函数只在报错信息与例外白名单里用它）
        let seg_end = src[at..].find(';').map(|i| at + i).unwrap_or(src.len());
        out.push((at, src[at..seg_end].to_string()));
    }
    out
}

/// REQ-042 · `src/db/apply.rs` 的 `#[cfg(test)]` 建连接也必须重试。
///
/// 它是 lib 自己的单元测试，集成测试够不着，所以用的是就地重写的一份
/// （2 次重试 + 300/600ms 退避）。本条钉住「那份重试不许被摘掉」。
///
/// # REQ-043-F3：钉的是**性质**，不是 `for attempt in 0..=` 这段文本
///
/// 原来这里是 `body.contains("for attempt in 0..=")` —— **字面量匹配**，于是两条真实退化都绿：
/// - **C3**：把上界 `2` 改成 `0`（= **只尝试一次、零重试**）→ 仍含 `for attempt in 0..=` ⇒ 仍绿；
/// - **C3b**：把退避整段删掉 → 循环壳还在 ⇒ 仍绿。
///
/// 字面量匹配的通病是：**它只认写法，不认含义**。改成从源码里**推导出两个性质**：
///
/// | 性质 | 怎么推 | 杀掉谁 |
/// | --- | --- | --- |
/// | **有界且至少重试一次**：总尝试次数 ≥ 2 | 从 `for x in A..=B` 的头部**解析出上界**，加上「首次 + N 次重试」 | **C3** |
/// | **尝试次数有天花板**：≤ [`RETRY_ATTEMPTS_CEILING`] | 同上 | 防止有人改成 100 次把套件烧穿 |
/// | **存在退避**：循环体里有 sleep | 取**配平的循环体**再找 `sleep`（REQ-044 起，见下） | **C3b** |
///
/// 三条都落在「不许有什么 / 不得小于几」的形式上，而不是「必须长这样」。
///
/// # REQ-044：把「退避」这一半也补成性质，并修掉「一个半边解析、另一个半边字面量」的分裂
///
/// REQ-043 只改了**次数**那一半，退避仍是 `loop_body.contains("sleep")` —— **字面量匹配**。
/// 字面量匹配的通病是**只认写法、不认含义**，实测两条都活（两条都在**未修复的树上跑过**，
/// 二进制 `test_connect_retry`，各 12 passed / 0 failed）：
///
/// | 变异 | 构造 | 真实退化 | 未修复时 |
/// | --- | --- | --- | --- |
/// | **W5** | 退避整段删掉，用**行尾注释**里的 `sleep` 顶替 | 3 次尝试、**零退避** | **绿（假绿）** |
/// | **W5b** | 一条**诱饵 `for`**（体里只有一行注释）+ 注释里的 `sleep`；真身退回单次尝试 | **1 次尝试、零退避** | **绿（假绿）** |
///
/// W5b 尤其说明问题：它用**一条谁也不执行的注释**同时顶替了「次数」与「退避」两个性质 ——
/// 诱饵 `for` 之所以能生效，是因为 `retry_loop` 只取 `body.find("for ")` 的**第一个**，
/// 与 REQ-042 早就拆掉的 `find` 第一个调用点（H4）是同一类病。
///
/// 现在三条性质共用**同一套解析底座** [`mask_noncode`]：
///
/// 1. **定位**：在**屏蔽掉注释与字面量**的视图里枚举**每一个** `for … in … {`，
///    只保留**循环体里真的调用了 `connect_for_tests(`** 的那些 —— 诱饵 `for` 的体是空的，
///    顶替不了真身；真身若退回单次尝试，则**没有任何循环包住建连接**，直接报红。
/// 2. **次数**：循环头从**同一个屏蔽视图**里取（注释伪造不出 `0..=2`），仍由
///    [`attempts_from_for_header`] 解析 —— 解析不出 ⇒ cannot certify ⇒ 变红。
/// 3. **退避**：在循环体的**屏蔽视图**里找**调用形状**的退避函数
///    （路径末段恰为 `sleep`、其后紧跟 `(`）⇒ 行尾注释 / 字符串里的 `sleep` 一律不算。
///
/// # 这次**堵住**了什么 / **没堵住**什么
///
/// **堵住**：
/// - W5：注释或字符串里的 `sleep` 不再能满足退避判据（`mask_noncode` 把它们替成空格）；
/// - W5b：诱饵 `for` 必须**自己包住建连接**才算数；真身退回「1 次尝试零退避」时
///   报「建连接不落在任何重试循环内」⇒ 变红；
/// - `sleep` 作为更长标识符的一部分（`my_sleep(` / `asleep(`）不算退避；
/// - 循环头里的注释不能伪造上界（循环头取自屏蔽视图）。
///
/// **没堵住**（刻意不追，见「判据完备性封口」）：
/// - 退避**只要在循环体里被调用**即可，不校验它是否真的被 `.await`、等待时长是否非零、
///   是否位于重试路径上（写在死分支 `if false { sleep(…) }` 里照样算）；
/// - 退避函数名必须**恰为** `sleep`（末段），自建的 `my_sleep(…)` helper 不被认作退避
///   —— 这一侧只会**假红**（写法不常见），不会假绿；
/// - 嵌套循环里「外层 `for` 包住内层 `for`」的形状只按外层判读；
/// - 判定粒度仍是**文本级**：不证明 sleep 与建连接处在同一条控制流路径上。
#[test]
fn test_lib_unit_test_connect_also_retries() {
    let src = normalize(&read_source("src/db/apply.rs"));
    // 只看 `#[cfg(test)] mod tests` 之后的部分 —— 前面的都是生产代码
    let at = src
        .find("mod tests")
        .expect("apply.rs 应有 #[cfg(test)] mod tests");
    let test_mod = &src[at..];

    let f = test_mod
        .find("fn pool()")
        .expect("测试模块里应有 pool() 辅助函数");
    let body_open = find_body_open(test_mod, f + "fn pool()".len()).expect("pool() 应有函数体");
    let body = balanced_call(test_mod, body_open);

    assert!(
        body.contains("connect_for_tests"),
        "apply.rs 的测试 pool() 应当建连接，实测函数体：\n{body}"
    );

    // ① 重试循环：**循环体里真的调用了建连接**，且有界、**至少重试一次**。
    //
    // REQ-044：原来这里是 `retry_loop(body)` —— 只取 `body.find("for ")` 的**第一个**，
    // 于是一条**诱饵 `for`** 就能把「次数」与「退避」两个性质同时顶替掉（实测 W5b 全绿）。
    // 现在只认**循环体里真的调用了 `connect_for_tests(`** 的循环，且判定一律走**屏蔽视图**。
    let loops = loops_around_connect(body, "connect_for_tests");
    assert!(
        !loops.is_empty(),
        "apply.rs 测试 pool() 的体里找不到**包住建连接**的 `for … in 0..=N` 形状的重试循环 —— \
         建连接必须落在**有界重试**里，否则共享集群的握手抖动会直接打脸这条单元测试。\
         ⚠️ **空循环 / 诱饵循环不算数**：判据只认循环体里**真的调用了 `connect_for_tests(`** 的那个 \
         （在屏蔽注释与字面量的视图里判定），所以「造一条空 `for` 顶在真身前面」骗不过它。\
         若循环被改成了别的形状，请让本判据跟上（它推的是「尝试次数」这个性质）。\
         实测函数体：\n{body}"
    );

    for lv in &loops {
        let header = &lv.header;
        let loop_body = &lv.body;
        // 次数：从循环头**解析**出总尝试次数，解析不出 ⇒ cannot certify ⇒ 变红
        let attempts = attempts_from_for_header(header).unwrap_or_else(|| {
            panic!(
                "重试循环 `{header}` 的上界推不出「尝试次数」—— 本判据只能处理 `for x in 0..=N` / \
                 `0..N` 且 N 是整数字面量的形状。**推不出 ⇒ 判据无法认证「有界重试」这个性质**，\
                 属于不可判定，必须报出来而不是默认放行。实测循环体：\n{loop_body}"
            )
        });
        assert!(
            attempts >= 2,
            "重试至少要有一次重试（= 总尝试次数 ≥ 2），实测只推导出 {attempts} 次尝试 —— \
         也就是**没有重试**，等于退回裸 connect。实测循环头：`{header}`"
        );
        assert!(
            attempts <= RETRY_ATTEMPTS_CEILING,
            "重试尝试次数 {attempts} 超过上限 {RETRY_ATTEMPTS_CEILING} —— \
         单次尝试被 `acquire_timeout=10s` 封顶，次数越大越像在烧墙钟时间而不是在等抖动恢复。\
         实测循环头：`{header}`"
        );
        // ② 退避必须存在 —— 无背压的紧循环重试等于把集群的抖动放大成风暴。
        //
        // REQ-044：原来是 `loop_body.contains("sleep")` —— **字面量匹配**，于是行尾注释里的
        // `sleep` 就能顶替真正的退避（实测 W5 全绿）。现在改成：在**屏蔽注释与字面量**的视图里
        // 找**调用形状**的退避函数（路径末段恰为 `sleep`、其后紧跟 `(`）。
        assert!(
            !lv.backoff_calls().is_empty(),
            "重试循环体里**没有退避**（在屏蔽注释与字面量后的代码里找不到 `sleep(...)` 调用）—— \
         无背压的紧循环重试会在集群抖动时把它放大成连接风暴。\
         ⚠️ 行尾注释 / 块注释 / 字符串里的 `sleep` **不算退避**（它们不执行），\
         退避必须是**真的被调用**的 `tokio::time::sleep(…)` 这类形状。\
         实测循环头：`{header}`，实测循环体：\n{loop_body}"
        );
    }
    // 不许是无界重试
    assert!(
        !body.contains("loop {"),
        "重试循环不许是无界 `loop {{` —— 那会把「快速失败」变成「套件挂死」。\
         实测函数体：\n{body}"
    );
}

/// `apply.rs` 测试 `pool()` 的重试尝试次数上限。
///
/// 单次尝试被 `pool::connect_for_tests` 的 `acquire_timeout = 10s` 封顶，
/// 所以 3 次 ≈ 30.9s 已是「超过真实抖动窗口」；再往上加次数是纯烧墙钟时间。
/// 与 `tests/common/mod.rs` 的 `CONNECT_RETRIES = 2`（= 3 次尝试）保持同一量级。
const RETRY_ATTEMPTS_CEILING: usize = 5;

/// 一个 `for … in … {` 循环的三份视图（REQ-044）。
///
/// - `header`：取自**屏蔽视图** ⇒ 注释伪造不出上界，解析次数时用的是它；
/// - `body`：**原文**切片 ⇒ 报错信息里是人读的源码；
/// - `masked_body`：与 `body` **逐字节等长**的屏蔽视图 ⇒ 一律用它判「有没有真的执行」。
struct RetryLoopView {
    header: String,
    body: String,
    masked_body: String,
}

impl RetryLoopView {
    /// 循环体里**真的被调用**的退避调用（在**屏蔽视图**上匹配，见 [`call_sites`]）。
    ///
    /// 注释 / 块注释 / 字符串里的 `sleep` 一律**不算** —— 它们不执行（W5）。
    fn backoff_calls(&self) -> Vec<String> {
        BACKOFF_CALLEE_SEGMENTS
            .iter()
            .flat_map(|seg| call_sites(&self.masked_body, seg))
            .collect()
    }
}

/// 退避函数的**末段名**：路径末段恰为它、且其后紧跟 `(` 才算一次退避调用。
const BACKOFF_CALLEE_SEGMENTS: &[&str] = &["sleep"];

/// `body` 里**每一个** `for … in … {` 循环（REQ-044 · W5b：不再是 `find` 的**第一个**）。
///
/// 枚举与定位全在**屏蔽视图**上做，切原文时用**相同偏移**（[`mask_noncode`] 保持长度不变）。
fn all_retry_loops(body: &str) -> Vec<RetryLoopView> {
    let masked = mask_noncode(body);
    let bytes = masked.as_bytes();
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = masked[from..].find("for ") {
        let at = from + rel;
        from = at + "for ".len();
        // `for` 必须真的是关键字：前面若是标识符字符，那是别的名字的一部分
        if at > 0 && is_ident_byte(bytes[at - 1]) {
            continue;
        }
        let rest = &masked[from..];
        let Some(in_rel) = rest.find(" in ") else { continue };
        let Some(brace_rel) = rest[in_rel..].find('{') else { continue };
        let header_end = from + in_rel + brace_rel;
        let Some(body_open) = find_body_open(&masked, header_end) else {
            continue;
        };
        let masked_body = balanced_call(&masked, body_open).to_string();
        out.push(RetryLoopView {
            header: masked[at..header_end].trim().to_string(),
            body: body[body_open..body_open + masked_body.len()].to_string(),
            masked_body,
        });
    }
    out
}

/// 只留下**循环体里真的调用了 `anchor`** 的循环 —— W5b 的直接解药。
///
/// 诱饵 `for` 的体是空的，顶替不了真身；真身若退回「单次尝试」，
/// 则**没有任何循环包住建连接**，调用方据此报红。
/// 判定在**屏蔽视图**上做：注释里写一句「这里调 `connect_for_tests`」不算数。
fn loops_around_connect(body: &str, anchor: &str) -> Vec<RetryLoopView> {
    all_retry_loops(body)
        .into_iter()
        .filter(|lv| !call_sites(&lv.masked_body, anchor).is_empty())
        .collect()
}

/// 在**屏蔽视图**里找出所有**调用形状**的 `name`，返回片段（供报错与判空）。
///
/// # 「算一次调用」的三个条件
///
/// 1. **在代码里** —— 调用方必须已用 [`mask_noncode`] 屏蔽注释与字面量（**W5 就死在这**）；
/// 2. **是调用** —— 其后（允许空白）紧跟 `(`，所以 `let x = sleep;` 这种**取值**不算；
/// 3. **末段恰为该名** —— `tokio::time::sleep(` 算，`my_sleep(` / `asleep(` **不算**。
///
/// # 没堵住什么（刻意不追）
///
/// 只看「**有没有被调用**」，**不**校验它是否真的被 `.await`、等待时长是否非零、
/// 是否落在重试路径上（死分支里的 `sleep(…)` 一样算）。
fn call_sites(masked: &str, name: &str) -> Vec<String> {
    let bytes = masked.as_bytes();
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = masked[from..].find(name) {
        let at = from + rel;
        from = at + name.len();
        // 条件 3：前一个字节不能是标识符字符（`my_sleep` / `asleep` 不算）
        if at > 0 && is_ident_byte(bytes[at - 1]) {
            continue;
        }
        // 条件 2：其后紧跟 `(`（允许空白 —— 屏蔽视图里空白可能来自被屏蔽的注释）
        let mut j = from;
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        if bytes.get(j) == Some(&b'(') {
            let end = masked[j..].find(')').map(|i| j + i + 1).unwrap_or(masked.len());
            out.push(masked[at..end].to_string());
        }
    }
    out
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// 把 `src` 里**注释与字面量**的内容替换成**等长空格**（换行原样保留），
/// 得到「只会执行的代码」视图 —— REQ-044 的解析底座。
///
/// # 三个必须成立的性质
///
/// 1. **长度不变**（按字节替成空格）⇒ 掩码视图与原文**偏移一一对应**，
///    于是可以在掩码上定位、在原文上取报错信息；
/// 2. **换行保留** ⇒ 行注释 `//` 有可收尾的边界（与 [`normalize`] 保留换行同一理由）；
/// 3. 保持**合法 UTF-8** ⇒ 被屏蔽的多字节注释（如中文）整段变空格，不会切出半个字符。
///
/// # 只在「有没有**真的执行**」这一个方向使用
///
/// 这与本文件「禁用词一律按文本出现即红」的保守取向**不冲突**，两者问的是不同的问题：
/// 判 `migrate` 有没有被写进源码 ⇒ 看文本；判 `sleep` 有没有真的让循环等一下 ⇒ 看执行。
/// 拿**文本**判「有没有退避」本身就是错的方向 —— 注释里的 `sleep` 一行都不执行（**W5**）。
///
/// # 已知不处理：raw string（**F-1**）—— 处置见本文件顶部「决策记录」
///
/// 本函数把 `r#"…"#` 当**普通字符串**处理：从第一个 `"` 屏蔽到下一个 `"`。
/// [`balanced_call`] 是同一套处理。
///
/// ⚠️ **REQ-045 订正**：REQ-044 在这里写的「对本判据已经够用
/// （raw string 里的 `for` / `sleep(` **不会露出来**）」**已被 QA 实测推翻**：
///
/// | 构造 | 本函数的实际行为 | 性质 |
/// | --- | --- | --- |
/// | raw string 内**引号数为偶数**（探针 **P7d**） | `sleep(` **泄进屏蔽视图** ⇒ 退避判据**判绿** | **假绿**（真身是 3 次尝试、零退避） |
/// | raw string 内**引号数为奇数**（探针 **P7**） | 扫描器**跨行失步** | 直接 **panic** —— 方向保守，但**失败模式失控** |
///
/// ⇒ 「本仓实参无 raw string」只能说明**今天**为真，推不出「无害」——
/// 这与 REQ-043 拆掉的「判据不能依赖『现在恰好没有』」是同一条教训。
/// 二维判据（**自然度 中 × 修复成本 中**）下它仍是决策，但登记理由
/// 已不是「够用」，而是「修复要两个函数各加一个 raw string 分支」。
fn mask_noncode(src: &str) -> String {
    let b = src.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0usize;
    let mut in_line_comment = false;
    let mut block_depth = 0usize;
    let mut in_str = false;
    let mut escaped = false;
    let mut in_char = false;
    let mut char_escaped = false;
    while i < b.len() {
        let c = b[i];
        // 换行**原样保留**：行注释靠它收尾，且长度必须不变
        if c == b'\n' {
            out.push(b'\n');
            in_line_comment = false;
            i += 1;
            continue;
        }
        // ── 注释态：不参与任何判定 ──
        if in_line_comment {
            out.push(b' ');
            i += 1;
            continue;
        }
        if block_depth > 0 {
            out.push(b' ');
            if c == b'/' && b.get(i + 1) == Some(&b'*') {
                block_depth += 1;
                out.push(b' ');
                i += 2;
                continue;
            }
            if c == b'*' && b.get(i + 1) == Some(&b'/') {
                block_depth -= 1;
                out.push(b' ');
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        // ── 字面量态 ──
        if in_str {
            out.push(b' ');
            if escaped {
                escaped = false;
            } else if c == b'\\' {
                escaped = true;
            } else if c == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if in_char {
            out.push(b' ');
            if char_escaped {
                char_escaped = false;
            } else if c == b'\\' {
                char_escaped = true;
            } else if c == b'\'' {
                in_char = false;
            }
            i += 1;
            continue;
        }
        // ── 代码态 ──
        match c {
            b'/' if b.get(i + 1) == Some(&b'/') => {
                in_line_comment = true;
                out.extend_from_slice(b"  ");
                i += 2;
                continue;
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                block_depth = 1;
                out.extend_from_slice(b"  ");
                i += 2;
                continue;
            }
            b'"' => {
                in_str = true;
                out.push(b' ');
                i += 1;
                continue;
            }
            b'\'' if is_char_literal(b, i) => {
                in_char = true;
                out.push(b' ');
                i += 1;
                continue;
            }
            _ => {}
        }
        out.push(c);
        i += 1;
    }
    // 被屏蔽的字节一律替成 ASCII 空格、其余字节原样 ⇒ 必然仍是合法 UTF-8
    String::from_utf8(out).expect("掩码只把字节替成 ASCII 空格、保留换行与原文字节，UTF-8 必然合法")
}

/// 从 `for x in <range>` 的循环头里**推导总尝试次数**：`0..=N` ⇒ N+1，`0..N` ⇒ N。
///
/// 推不出来返回 `None` —— 调用方据此判「不可判定」并报红，而不是默认放行。
/// 这是本判据与旧版最大的区别：**宁可在看不懂时喊出来，也不要装作看懂了**。
fn attempts_from_for_header(header: &str) -> Option<usize> {
    let range = header.split(" in ").nth(1)?.trim();
    let (lo, hi, inclusive) = match (range.split_once("..="), range.split_once("..")) {
        (Some((l, h)), _) => (l.trim(), h.trim(), true),
        (None, Some((l, h))) => (l.trim(), h.trim(), false),
        (None, None) => return None,
    };
    if lo != "0" {
        return None;
    }
    // 剥掉 `2usize` / `2u32` 这类整数字面量后缀
    let hi = hi.trim_end_matches(|c: char| c.is_ascii_alphabetic());
    let hi: usize = hi.parse().ok()?;
    Some(if inclusive { hi + 1 } else { hi })
}

/// AC-003 的第二道：类型层 + `migrate` 不落在重试之内。
///
/// 全仓能产出 `Result<PgPool, DbError>` 的只有 `pool::connect*`；
/// 把别的东西塞进 `retry_connect` 会**编译不过**。
#[test]
fn test_retry_helper_is_still_pinned_to_connect() {
    let code = code_text();
    assert!(
        code.contains("Fut: Future<Output = Result<PgPool, pool::DbError>>"),
        "`retry_connect` 的签名必须仍被钉在 Result<PgPool, DbError> 上 —— \
         放宽它就等于把「连上之后的失败」也纳入了可重试范围"
    );

    // `migrate()` 是「连上之后」的第一个真实动作：它必须在重试之外、照旧 expect。
    // ⚠️ REQ-042：原来这里也是定长 200 字符切片（同样的 H2/H4 洞），
    // 改成「取 `pub async fn migrate` 的**配平函数体**，整个体里不许有 `retry_connect`」。
    let mig = code
        .find("pub async fn migrate")
        .expect("common/mod.rs 里应有 migrate");
    let body_open = find_body_open(&code, mig + "pub async fn migrate".len())
        .expect("migrate 应有函数体");
    let mig_body = balanced_call(&code, body_open);
    assert!(
        !mig_body.contains("retry_connect"),
        "`migrate` 的函数体里出现了 `retry_connect` —— 迁移失败不得被重试。\
         实测函数体：\n{mig_body}"
    );
}

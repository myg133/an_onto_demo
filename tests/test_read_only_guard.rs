//! AC-101 · 只读保证的**防回归**（REQ-003 · S1）
//!
//! ## 为什么要有这个文件
//!
//! 勘查器是整个平台里**唯一**接触客户数据的代码，安全红线是「只读」。
//! 独立审核用变异证明这道红线**零防回归**：
//!
//! | 变异 | 修复前 |
//! | --- | --- |
//! | 删掉 `after_connect` 里的 `SET` | 148/148 全绿 |
//! | 阉割 fail-closed 校验 | 148/148 全绿 |
//! | 删掉 `force_read_only` 的调用点 | 148/148 全绿 |
//!
//! 根因：**`force_read_only` 原本只测了纯函数，没人测它有没有被真的调用。**
//!
//! ## 两层判据
//!
//! 1. **行为层**（`src/extract/introspect.rs` 的 `mod tests`）：
//!    - `ensure_read_only_rejects_anything_but_on` —— 执行「拒绝」这个决定；
//!    - `read_only_pool_really_reads_back_on` —— 拿测试库连一次，读回
//!      `transaction_read_only`。**删掉 SET 会被它抓住**（读回 `off`）。
//! 2. **接线层**（本文件）：前两条抓不到的两种变异在这里抓 ——
//!    删掉 `ensure_read_only(...)` 的**调用点**、删掉
//!    `force_read_only(...)` 的**调用点**（这两个不是行为差异，
//!    库读回来照样是 `on`，只能看接线）。
//!
//! ## 接线层为什么不是「又来一遍 grep」
//!
//! 判据跑在**剥掉注释之后**的代码上。REQ-002 复审栽过的形态正是
//! 「守卫被删掉、字符串留在注释里」—— 裸 grep 挡不住它，本文件能挡住。
//! `stripper_strips_comments_before_matching` 是这条判据自身的自检。
//!
//! ## 为什么「数出现次数」和「语义断言」必须同时留着（S1'）
//!
//! 计数判据（`matches("force_read_only(").count() >= 2`）判的是
//! **有没有出现过**；它判不出**出现在哪、错误有没有被传播**。实测两个
//! 诱饵变异能骗过 160 个测试：
//!
//! | 变异 | 只留计数判据 | 计数 + 语义（本文件的现状） |
//! | --- | --- | --- |
//! | R4 池构造删掉真调用，`introspect` 里补一次「像防御纵深」的调用 | 160/160 全绿 | 🔴 红 |
//! | R5 `ensure_read_only(&read_only)?` 改成 `.ok()`（错误被丢弃） | 160/160 全绿 | 🔴 红 |
//!
//! 复审 agent 自己第一版就是「只加语义断言、把计数判据替换掉」，**结果漏了
//! R1**（删 `after_connect` 里的 SET）：语义断言只盯着两个函数体，
//! 盯不到 `read_only_pool` 里的连接串构造。**两类判据各管各的，都得留。**
//!
//! 所以下面两个测试里既有计数、也有语义断言 —— 删掉任何一类都会掉判别力。

/// 勘查器源码（编译期快照）。
const INTROSPECTOR: &str = include_str!("../src/extract/introspect.rs");

/// 剥掉 Rust 注释（`//` 与 `/* */`，块注释按嵌套计数），**保留字符串字面量**。
///
/// 保留字符串是必须的：`SET default_transaction_read_only = on` 本身就是
/// 一个字符串字面量，把它一起剥掉就等于把判据的核心证据也删了。
fn strip_rust_comments(src: &str) -> String {
    let b: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(b.len());
    let mut block = 0usize; // 块注释嵌套深度
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        let next = b.get(i + 1).copied();
        if block > 0 {
            if c == '/' && next == Some('*') {
                block += 1;
                i += 2;
            } else if c == '*' && next == Some('/') {
                block -= 1;
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if c == '/' && next == Some('/') {
            while i < b.len() && b[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && next == Some('*') {
            block = 1;
            i += 2;
            continue;
        }
        // 字符字面量（'x' / '\n'）原样保留；`'a` 这种生命周期因为
        // 找不到配对的第二个引号而自然落到「当普通字符处理」。
        if c == '\'' && b.get(i + 2) == Some(&'\'') {
            out.push(c);
            out.push(b[i + 1]);
            out.push('\'');
            i += 3;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

/// 勘查器的**可执行代码**（注释已剥，且**只看生产代码**）。
///
/// 为什么要砍掉 `#[cfg(test)]` 之后的部分：`force_read_only` 自己的三个单元测试
/// 也调 `force_read_only(...)`，不砍的话「调用点还在不在」会被**测试代码顶包**
/// —— 删掉生产接线，判据照样绿。**绿但没判别力的判据比没有判据更糟。**
fn code() -> String {
    let stripped = strip_rust_comments(INTROSPECTOR);
    let production = match stripped.find("#[cfg(test)]") {
        Some(i) => stripped[..i].to_string(),
        None => stripped,
    };
    // 锚点自检：砍错地方就当场报错，绝不静默放行
    assert!(
        production.contains("pub async fn introspect"),
        "生产代码区里找不到 `pub async fn introspect` —— #[cfg(test)] 锚点可能变了"
    );
    production
}

/// 判据自检：注释里的字面量**必须**被剥掉。
///
/// 没有这一条，上面三条断言就可能退化成「注释里还留着那句话 ⇒ 假通过」——
/// 正是 REQ-002 复审栽过的形态。
#[test]
fn stripper_strips_comments_before_matching() {
    let src = "// default_transaction_read_only=on\n/* SET default_transaction_read_only = on */\nlet x = 1;\n";
    let out = strip_rust_comments(src);
    assert!(
        !out.contains("default_transaction_read_only"),
        "注释没被剥掉 —— 接线判据会退化成存在性 grep：{out}"
    );
    assert!(out.contains("let x = 1;"), "真代码不该被误剥：{out}");
    // 同时：字符串字面量里的内容必须留着（否则 SET 的证据被自己删了）
    let kept = strip_rust_comments("sqlx::query(\"SET a = on\")");
    assert!(kept.contains("SET a = on"), "字符串字面量不该被剥：{kept}");
}

/// 去掉**全部空白** —— 让语义断言与代码排版无关。
///
/// `ensure_read_only(&read_only)?;` 与 `ensure_read_only( &read_only )? ;`
/// 是同一件事；只改排版就判红，那是白噪声，不是防回归。
/// 也因此语义判据一律用 `contains` 而不是等值比较：调用写在语句里任何
/// 位置都算数，「有没有」由形态决定，「在哪、有没有吞错误」由排版无关的
/// 形态串决定。
fn squashed(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// `read_only_pool` 的**函数体**（生产代码区内）。
///
/// 起点是签名、终点是下一个生产函数 `introspect` —— 于是 `force_read_only`
/// 自己的单元测试（也在本文件的后半）天然落在体外，顶不了包。
fn read_only_pool_fn() -> String {
    code()
        .split("pub(crate) async fn read_only_pool")
        .nth(1)
        .and_then(|s| s.split("pub async fn introspect").next())
        .expect("找不到 read_only_pool 函数体")
        .to_string()
}

/// 变异 ①：删掉 `after_connect` 里的 `SET`。
/// 行为层已能抓（读回 `off`），这里守的是「回调整个被删掉」这一形态。
#[test]
fn after_connect_read_only_set_is_in_executable_code() {
    let c = code();
    assert!(
        c.contains("after_connect("),
        "只读池构造里必须有 after_connect 回调"
    );
    assert!(
        c.contains("SET default_transaction_read_only = on"),
        "after_connect 里必须执行 SET default_transaction_read_only = on（只靠连接串参数实测不设防）"
    );
}

/// 变异 ② + R4：删掉 `force_read_only` 的**调用点**。
///
/// 判据有**两条，各管各的，都得留**：
/// - **计数**（`>= 2`）：判「生产代码里至少出现过一次调用」。判据只跑
///   **生产代码区**（见 [`code`]）—— 否则本文件与单元测试自己的
///   `force_read_only(...)` 调用会顶包，删掉生产接线也照样绿。
/// - **语义**（池构造函数体内必须有 `force_read_only(url)?;`）：判「真调用
///   确实在**池构造里**、且错误用 `?` 传播」。
///
/// **只留计数会被 R4 骗过**：把池构造里的真调用删掉，另在 `introspect` 里
/// 补一次「看起来像防御纵深」的调用 —— 出现次数不变、计数照样绿，
/// 而池构造已经不再强制只读了（AC-101 红线）。
#[test]
fn force_read_only_is_actually_called_by_the_pool_builder() {
    let c = code();
    let calls = c.matches("force_read_only(").count();
    assert!(
        calls >= 2,
        "生产代码里 force_read_only 只有 {calls} 处出现 —— 调用点被删了？\
         （1 处只是函数签名，池构造必须真的调用它）"
    );
    // 语义断言（R4）：调用必须在**池构造体内**，且错误用 `?` 传播
    let pool_fn = squashed(&read_only_pool_fn());
    assert!(
        pool_fn.contains("force_read_only(url)?;"),
        "read_only_pool 函数体内必须有 `force_read_only(url)?;` —— \
         强制只读连接串是池构造的职责。把它挪到别的函数里、再补一次调用，\
         骗得过计数判据，但池构造已不再强制只读（AC-101 红线）"
    );
}

/// 变异 ③ + R5：阉割 fail-closed —— 删掉 `ensure_read_only(...)` 的**调用点**。
///
/// 判据同样有**两条，都得留**：
/// - **计数**（`>= 2`）
/// - **语义**（`introspect` 函数体内必须有 `ensure_read_only(&read_only)?;`）
///
/// **只留计数会被 R5 骗过** —— 把 `ensure_read_only(&read_only)?;` 换成
/// `ensure_read_only(&read_only).ok();`（错误被 `.ok()` 吞掉）之后，
/// 出现次数一点没变、计数照样绿，而 fail-closed 已经被改成了
/// **fail-open**：读回不是 `on` 也照样勘查客户库。AC-101 红线真失效。
/// `?` 就是这里的关键，也是计数判据永远看不见的东西。
#[test]
fn fail_closed_check_is_actually_called_by_introspect() {
    let c = code();
    let calls = c.matches("ensure_read_only(").count();
    assert!(
        calls >= 2,
        "生产代码里 ensure_read_only 只有 {calls} 处出现 —— 调用点被删了？\
         （1 处只是函数签名，introspect 必须在勘查前真的调它）"
    );
    // 语义断言（R5）：`?` 必须真的传播错误，`.ok()` / 丢弃返回值都算阉割
    let intro_fn = squashed(
        code()
            .split("pub async fn introspect")
            .nth(1)
            .expect("找不到 introspect 函数体"),
    );
    assert!(
        intro_fn.contains("ensure_read_only(&read_only)?;"),
        "introspect 函数体内必须有 `ensure_read_only(&read_only)?;` —— \
         `?` 不可省。写成 `.ok()` 或丢掉返回值，就把 fail-closed 改成了 \
         fail-open（AC-101 红线），而 `ensure_read_only` 的出现次数一点没变"
    );
    // 判据本身也不能被改成放行一切
    assert!(
        c.contains("!= \"on\""),
        "fail-closed 判据里必须真的拿读回值与 \"on\" 比较"
    );
}

/// 反向：只读**不该**变成「凡连必拒」——真只读连接必须放行。
/// 守的是别把判据写成「永远 bail」这种同样错误的形态。
#[test]
fn read_only_pool_itself_is_not_wired_to_reject_everything() {
    // 池构造里不应出现 fail-closed 拒绝（否则连不上库就没法勘查）
    let pool_fn = read_only_pool_fn();
    assert!(
        !pool_fn.contains("ensure_read_only("),
        "池构造阶段不应调 fail-closed（那是勘查前、读回设置之后的判据）"
    );
}

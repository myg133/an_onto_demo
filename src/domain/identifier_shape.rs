//! 源库标识符的**形态判据** —— 页面层与 MCP 层**共用的一份**实现（REQ-029）。
//!
//! # 为什么是**一个**文件而不是两个函数
//!
//! REQ-029 之前，HTML 层（[`crate::domain::html_view`] 的 `has_raw_identifier`）
//! 与 MCP 层（[`crate::mcp::tools`] 的 `scrub_identifiers`）把同一套形态思路
//! **各写了一遍**，且**已经漂移**：实测三处可观测差异 ——
//!
//! | 输入 | 旧 HTML 判据 | 旧 MCP 判据 |
//! | --- | --- | --- |
//! | `对应abc.def` | **漏抓**（点路径判定跑在 `split_whitespace` 产出的词上） | 命中（变成 `对应…`） |
//! | `key` / `KEY` | 命中（多一条字面量规则） | 不命中（由 [`crate::domain::view::scan_modeling_terms`] 兜住） |
//! | 命中之后 | **整条丢弃** | **局部替换**成 `…` |
//!
//! 前两行是**判据**的分叉，第三行是**动作**的差异（动作本来就该不同）。
//! 漂移的代价是实打实的：`对应abc.def` 在页面上**漏抓**，客户看得见的
//! 名字里会留着 `对应abcdef`。
//!
//! ⇒ 判据必须**只有一份**，两侧都调它；**动作**留在各自调用方。
//! 「改一处、同步另一处」在这里不成立 —— 改共享函数才会同时生效。
//!
//! # 判据本体（**形状**，不是词表）
//!
//! 一个 ASCII 词元满足下列任一条即为**源库标识符形态**：
//!
//! 1. **点路径** `a.b` —— 切分后 ≥2 段，每段非空、只含 `[A-Za-z0-9_]`，
//!    且**段里至少有一个 ASCII 字母**；
//! 2. **snake_case** —— 含 `_`，切分后 ≥2 段非空，且**段里至少有一个 ASCII 字母**。
//!
//! # ⚠️「段里至少有一个字母」是 REQ-029 补的，修的是**静默吃客户文字**
//!
//! 补之前两条规则**都不要求任何一段含字母**，于是「纯数字 + 分隔符」被当成
//! 标识符：**业务文字被静默吃掉**。真实语料（主仓 `e2e-schema.json` 的
//! 681 条列注释）里当前是 **0 例**，但判据是**活的** —— 换源库、换业务就会出现：
//!
//! | 输入 | 旧判据 | 动作 | 客户看到 |
//! | --- | --- | --- | --- |
//! | `版本号 1.5 的订单` | 命中 | 替换/丢弃 | `版本号 … 的订单` |
//! | `折扣 0.8 折` | 命中 | 替换/丢弃 | `折扣 … 折` |
//! | `生效于 3.14 之后` | 命中 | 替换/丢弃 | `生效于 … 之后` |
//! | `见 2024.10 的公告` | 命中 | 替换/丢弃 | `见 … 的公告` |
//! | `金额 1_000_000` | 命中（snake_case 规则）| 替换/丢弃 | `金额 …` |
//!
//! 这是 **fail-open 的反面**：不是漏掉标识符，而是**把客户看得懂的业务文字删掉**，
//! 且**无声**（页面侧甚至整条属性消失，只在 withheld 清单里留个名字）。
//! 漏网一个列名，运维还看得出来；误伤一条业务说明，没人知道少了东西。
//!
//! # ⚠️ 本模块**只保留一条**原则（原先这里同时写着两条、互相打架，QA · F3 已收敛）
//!
//! > **宁可误伤，不许漏网**（fail-closed 方向）。
//! >
//! > 判据一旦命中，就当它是源标识符；**不以「看起来像正常文字」为由放过**。
//!
//! ⚠️ 原先这两句是并列的、且**互相矛盾**，下一个人不知道该听哪条：
//!
//! | 原先写的 | 说的是 |
//! | --- | --- |
//! | 「误伤比漏网的代价更高」 | 倾向**少误伤** |
//! | 「漏网是头号失败模式」 | 倾向**少漏网** |
//!
//! ⇒ 两条并存时，判据该往哪边偏是**无解的**。现在只留后一条。
//!
//! 这条原则能同时解释「补字母要求」和「取至少一段」，是因为它只对
//! **已经命中的串**表态（「命中了就别放」），**不决定命中谁**
//! （那是判据本体的活）。两个取舍因此落在同一个方向上，且**没有一条要求为误伤让路**：
//!
//! - **修小数误伤**不是「放水」：被吃的 `0.8` / `2024.10` / `1_000_000` 是
//!   **纯数字段**，压根不是标识符形态 —— 旧判据把它们算作命中，是**判据不精确**，
//!   不是这条原则要求保留的代价。修它**不违反**本原则。
//! - **取「至少一段」**才是本原则的**直接后果**：它让真实表名
//!   `llm_provider_back_20260701` 不漏网，而付出的代价（`No.1` / `v1.2`
//!   多半是版本号）**正是**本原则愿意付的那一类误伤。
//!
//! ⚠️ 反过来说：**凡是要靠「放行某个形态」来降低误伤的改动，都要先回答**
//! 「它会不会同时放走真标识符」—— 会，就是拿本原则换数字，不做。
//!
//! # 为什么是「**至少一段**含字母」而不是「**每段**都含字母」
//!
//! 两者都能挡住上表各例（它们的段**全是**纯数字），但在真实标识符上分道扬镳：
//!
//! | 真实串 | 每段含字母 | 至少一段含字母 |
//! | --- | --- | --- |
//! | `llm_provider_back_20260701`（**真实表名**，主仓 `e2e-schema.json` 有） | ❌ 漏 | ✅ 抓 |
//! | `x_1` / `level_3` 这类带序号段的列名 | ❌ 漏 | ✅ 抓 |
//!
//! 「每段」规则会在真实语料里**漏掉 1 张表**（1051 列名实测漏 0、表名实测漏 1）。
//! 漏网一个源标识符，代价是整个模块存在的理由被推翻；代价是 `No.1` / `v1.2`
//! 这类「版本号/序位号」仍会命中 —— 这**正是**上面那条「宁可误伤，不许漏网」
//! 原则愿意付的代价，不是违背它（见下「残留风险」）。
//!
//! # 两侧的**组合**差异（唯一保留的一处，且是**有意**的）
//!
//! 形状判据两侧完全一致；差别只在**要不要叠加 `key` 字面量**：
//!
//! - [`has_raw_identifier`]（页面层用）= 形状 ∪ `key` 字面量；
//! - [`contains_identifier_shape`] / [`scrub_identifiers`]（MCP 层用）= 只有形状。
//!
//! # ⚠️「`key` 字面量只进页面层」**并没有消除误伤面，只是把它挪了地方**
//! # （QA · F2 更正；原先的写法读起来像「页面层更全面，所以那样更好」）
//!
//! **为什么不把字面量也并进 MCP 侧**：实测（`baseline-proposal.json` 的 582 条
//! 真实 `note` + 主仓 `e2e-schema.json` 的 681 条列注释）——
//! 并进去会让真实列注释 `license key`（`licenses.license_key`、
//! `activity_keys.license_key`）被吃成 `license …`。「License 密钥」是**业务说法**，
//! 不是列名；把它吃掉正是本模块要消灭的那种误伤。
//! 而 `key` 在 MCP 侧**并不漏网**：[`crate::domain::view::scan_modeling_terms`]
//! 已经兜住整份视图的发布闸门（`note: "这个字段叫 key"` ⇒ 直接 `Err`）。
//! 两者是**机制不同，不是缺失**。
//!
//! ⚠️⚠️ 但上面这段**只解释了「为什么 MCP 侧不含」，没解释「这是否更干净」**
//! —— 而后者才是容易被顺手当成结论的地方。准确后果是：
//!
//! - 页面层对 **`note` 也用含 `key` 字面量的判据** —— [`has_raw_identifier`]；
//! - 且命中后的动作是**整条丢弃**（[`crate::domain::html_view::sanitize_display`]
//!   对 `note` 返回 `None`，说明文字从页面上整体消失）；
//! - 所以 `license key` 这类误伤**在 note 上仍然存在**，而且**动作更重**
//!   （整条丢 > 局部替换成 `…`）。
//!
//! ⇒ 把 `key` 挡在 MCP 侧之外，换来的**不是**「误伤面更小」，而是
//! **「误伤只发生在一个动作更重的出口上」**。当前语料里 `license key` 实测 **0 例**
//! 出现在被页面层清洗的 note 上，属**既有行为、本轮未改**（改它要动页面层判据的
//! 组成，超出 REQ-029 范围）；这里如实记下，别让后人读成「页面层这套更干净」。
//!
//! # ⚠️ 残留风险（如实记在这，**没修**）
//!
//! 「至少一段含字母」管住的是「**全数字**」，管不住「**数字 + 字母**」的缩写/版本号：
//! `U.S.` / `i.e.` / `e.g` / `No.1` / `v1.2` 在两侧**仍然命中**。
//!
//! # ⚠️「收紧会让 `a.b` 也放过去」这句话**只对一半的收紧成立**（QA · F4 更正）
//!
//! 收紧有**两条互不相同**的路，只有一条会伤 `a.b`：
//!
//! | 收紧方式 | 会漏 `a.b` 吗 | 说明 |
//! | --- | --- | --- |
//! | 「**每段**都含字母」 | ❌ **不漏** | `a` / `b` 两段**各自都有字母**，仍命中 —— 这条收紧**不伤** `a.b` |
//! | 「**至少一段 ≥2 字符**」 | ✅ **漏** | `a` / `b` 都只有 1 个字符，全段不达标 ⇒ 直接放过去 |
//!
//! ⇒ 「伤 `a.b`」的是**段长下限**（`≥2 字符`），**与「每段 vs 至少一段」无关**。
//! 上表已证明「每段含字母」这条收紧在 `a.b` 上是安全的 —— 真正被这条原则否掉的
//! 是**段长**收紧，而不是「每段」。
//!
//! `a.b` 是「判据认形状、不认词表」的活证据，让它漏过去是**削弱**闸门，
//! 不在 REQ-029 范围内。实测真实语料（964 条属性名 + 582 条 `note` +
//! 681 条列注释 + 1051 个列名 + 91 个表名）命中这 5 个串的记录数为 **0**，
//! 属**潜在**风险而非已发生事故。

/// 一个 ASCII 词元切出来的**标识符候选**（`a.b` / `customer_id` 会是**一个**词元）。
///
/// **两侧共用这一份切分** —— 旧实现里页面层跑的是 `split_whitespace` 词、
/// MCP 层跑的是含 `.` 的 ASCII 连缀词，`对应abc.def` 的漏抓就是这么来的。
pub(crate) fn identifier_tokens(s: &str) -> impl Iterator<Item = &str> {
    s.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.'))
        .filter(|t| !t.is_empty())
}

/// 这段字符串里有没有 ASCII 字母。
fn has_ascii_alpha(seg: &str) -> bool {
    seg.bytes().any(|b| b.is_ascii_alphabetic())
}

/// 切出来的段是否**合格**：非空、只含 ASCII 字母数字/下划线，且**至少一段含字母**。
///
/// 最后一条是 REQ-029 补的（见模块注释）：旧实现**不要求任何一段含字母**，
/// 于是 `0.8` / `3.14` / `2024.10` / `1_000_000` 这类「纯数字串」被当成
/// 标识符，业务文字被静默吃掉。取「至少一段」而不是「每段」的理由见模块注释
/// （`llm_provider_back_20260701` 这类**真实表名**带日期后缀）。
fn segments_qualify(segs: &[&str]) -> bool {
    let valid: Vec<&str> = segs
        .iter()
        .copied()
        .filter(|s| {
            !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        })
        .collect();
    valid.len() >= 2 && valid.iter().copied().any(has_ascii_alpha)
}

/// 这个**词元**是不是源库标识符的形态（点路径 / snake_case）。
///
/// **判据本体，全项目只有这一份实现** —— 页面层与 MCP 层都调它。
pub fn is_identifier_token(token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    // ① 点路径 `a.b`
    let segs: Vec<&str> = token.split('.').collect();
    if segments_qualify(&segs) {
        return true;
    }
    // ② snake_case `customer_id`
    if token.contains('_') {
        let parts: Vec<&str> = token.split('_').collect();
        if segments_qualify(&parts) {
            return true;
        }
    }
    false
}

/// 这个**词元**是不是建模术语**字面量**（`key` / `KEY`）。
///
/// 与形状判据分开放：它**是词表**（建模术语就那么几个），
/// 而形状判据是**判据**。两者组合起来才是页面层要的完整判据。
pub fn is_modeling_literal(token: &str) -> bool {
    token.eq_ignore_ascii_case("key")
}

/// 这段文本里有没有**源库原始标识符**（形状 ∪ 字面量）—— **页面层判据**。
///
/// 命中就整条丢弃：这些串**没有任何办法翻成人话**，
/// 硬改只会造出「关联模型（对应 llm 模型 模型编号）」这种半吊子。
pub fn has_raw_identifier(s: &str) -> bool {
    identifier_tokens(s).any(|t| is_identifier_token(t) || is_modeling_literal(t))
}

/// 这段文本里有没有标识符**形态**（不含字面量）—— **MCP 层判据**。
///
/// **快路径**：没有就整段原样返回，绝大多数 `note` 走这条。
pub fn contains_identifier_shape(s: &str) -> bool {
    identifier_tokens(s).any(is_identifier_token)
}

/// 清洗自由文本里的**源库标识符** —— **MCP 层动作**。
///
/// # 为什么是**替换**而不是整条丢掉
///
/// `note` 是**数据库列注释原文**，是真实业务信息（属性说明）。
/// BA 裁定：不许用「把 `note` 整个丢掉」绕过 —— 丢了会伤回答质量。
/// 所以命中的标识符串被换成 `…`，**周围的业务文字原样保留**：
/// `关联模型ID(对应t052.model_id)` → `关联模型ID(对应…)`。
///
/// # 只在**整个词元**命中时才替换
///
/// 半吊子替换（「关联模型(对应…模型编号)」那种）会造出业务上不通的句子。
pub fn scrub_identifiers(text: &str) -> String {
    if !contains_identifier_shape(text) {
        return text.to_string(); // 绝大多数 note 走这条快路
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    // 逐个 ASCII 词元切出来处理，中文与标点原样带走
    while let Some(pos) =
        rest.find(|c: char| c.is_ascii_alphanumeric() || c == '_' || c == '.')
    {
        out.push_str(&rest[..pos]);
        let tail = &rest[pos..];
        let end = tail
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.'))
            .unwrap_or(tail.len());
        let token = &tail[..end];
        out.push_str(if is_identifier_token(token) {
            "…"
        } else {
            token
        });
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// REQ-029 主项：小数/日期/千分位**不许**被当成标识符吃掉。
    ///
    /// 这批串在旧判据下**全部命中**（`0.8` 是「两段纯数字」），
    /// 动作是把客户看得懂的业务文字换成 `…`。
    ///
    /// ⚠️ 这**不是**违反「宁可误伤，不许漏网」（见模块注释那条原则）：
    /// 这些串的段**一个字母都没有**，压根不是标识符形态，
    /// 旧判据把它们算命中是**判据不精确**，修它两头都不亏。
    #[test]
    fn decimals_and_dates_are_not_identifiers() {
        for keep in [
            "版本号 1.5 的订单",
            "折扣 0.8 折",
            "生效于 3.14 之后",
            "见 2024.10 的公告",
            "金额 1_000_000 元",
            "QPS 限制 100.5",
            "2.5倍积分",
            "1_000_000",
        ] {
            assert_eq!(
                scrub_identifiers(keep),
                keep,
                "小数/日期/千分位是业务文字，不该被清洗：{keep}"
            );
            assert!(
                !contains_identifier_shape(keep),
                "判据不该把纯数字段当成标识符：{keep}"
            );
        }
    }

    /// 修小数**不能**顺手把真标识符放过去 —— 真阳性一个都不能少。
    #[test]
    fn real_identifiers_still_caught() {
        for hit in [
            "关联模型ID(对应t052.model_id)",
            "核销人customer_id",
            "供应商配置(base_url, api_key等)",
            "功能编码，如 jd_api",
            "a.b",
            "v2_model",
            "指向 billing_account",
            "zzz_qqq",
            "t.c",
            "x_1",
            "llm_provider_back_20260701", // **真实表名**（主仓 e2e-schema.json 里有）
        ] {
            assert!(
                is_identifier_token(hit) || contains_identifier_shape(hit),
                "真标识符形态不该被放过去：{hit}"
            );
        }
        assert!(is_identifier_token("llm_provider_back_20260701"));
        assert_eq!(scrub_identifiers("核销人customer_id"), "核销人…");
        assert_eq!(scrub_identifiers("a.b"), "…");
    }

    /// 「不是词表，是形状」：没在真实数据里出现过的串也必须被抓。
    #[test]
    fn shape_not_wordlist() {
        assert!(is_identifier_token("qqq_zzz"));
        assert!(is_identifier_token("z.y.x"));
        assert!(!is_identifier_token("customer"));
        assert!(!is_identifier_token("GLM"));
        assert!(!is_identifier_token("2024"));
    }

    /// ⚠️ **残留风险**：缩写/版本号（`U.S.` / `No.1` / `v1.2`）**仍然命中**。
    ///
    /// ⚠️ **「收紧会放走 `a.b`」只对段长收紧成立**（QA · F4）：
    /// 要伤 `a.b` 的是「**至少一段 ≥2 字符**」（`a` `b` 都只有 1 字符）；
    /// 而「**每段都含字母**」这条收紧**保得住** `a.b`（两段各自都有字母），
    /// 也保不住 `llm_provider_back_20260701`（末尾段 `20260701` 无字母）——
    /// 那才是「取至少一段」的理由，与 `a.b` 无关。
    /// 这条测试的作用是把残留风险**钉成显式的**，将来谁想收紧/放宽，一眼能看到影响面。
    #[test]
    fn known_residual_risk_is_pinned() {
        for risky in ["U.S.", "i.e.", "e.g", "No.1", "v1.2"] {
            assert!(
                contains_identifier_shape(risky),
                "缩写/版本号当前仍命中（已知残留风险，见模块注释）: {risky}"
            );
        }
        // 与之相对：**全数字**的串必须被放过去
        for safe in ["1.5", "3.14", "2024.10", "0.8", "2.5倍"] {
            assert!(
                !contains_identifier_shape(safe),
                "全数字串不该命中: {safe}"
            );
        }
    }

    /// 页面层判据（形状 ∪ `key` 字面量）。
    #[test]
    fn html_criterion_includes_key_literal() {
        assert!(has_raw_identifier("这个字段叫 key"));
        assert!(has_raw_identifier("这个字段叫 KEY"));
        assert!(has_raw_identifier("核销人customer_id"));
        // ⚠️ 页面层此前**漏抓**的串：中文前缀粘在点路径上
        assert!(
            has_raw_identifier("对应abc.def"),
            "中文前缀不再把点路径甩掉 —— REQ-029 修的真实泄漏"
        );
        assert!(!has_raw_identifier("扣款币种"));
        assert!(!has_raw_identifier("折扣 0.8 折"));
    }

    /// MCP 侧**不**并入 `key` 字面量：并进去会把真实业务说法 `license key`
    /// 吃成 `license …`（实测 `licenses.license_key` 的列注释原文就是它）。
    #[test]
    fn mcp_criterion_does_not_eat_key_word() {
        assert_eq!(
            scrub_identifiers("license key"),
            "license key",
            "「License 密钥」是业务说法，不是列名"
        );
        assert!(!contains_identifier_shape("license key"));
    }

    /// ⚠️ **钉住「接线」**，不只是判据本身。
    ///
    /// 变异自证 M2 的教训：只测本模块的函数，**页面层退回它自己的本地副本时
    /// 一个测试都不会红**（判据分叉正是这样悄悄回来的）。
    /// 所以这里从**页面层的公开出口** `html_view::sanitize_display` 打进去 ——
    /// 它必须真的走共享判据。
    #[test]
    fn html_view_actually_calls_the_shared_criterion() {
        // 页面层此前**漏抓**的串：中文前缀不再把点路径甩掉
        assert_eq!(
            crate::domain::html_view::sanitize_display("对应abc.def"),
            None,
            "页面层必须走共享判据，否则会漏抓 —— 客户看得见 `对应abcdef`"
        );
        assert_eq!(
            crate::domain::html_view::sanitize_display("核销人customer_id"),
            None
        );
        // 小数/日期**不许**让整条属性从页面上消失（REQ-029 主项）。
        // ⚠️ 这里只用**不含页面禁用词**的样本（`版本号` 是页面禁用词，
        // 命中的是 `PAGE_FORBIDDEN_CJK`，与标识符判据无关 —— 别把它算进本测试）。
        for keep in ["折扣 0.8 折", "见 2024.10 的公告", "生效 3.14 之后"] {
            assert_eq!(
                crate::domain::html_view::sanitize_display(keep).as_deref(),
                Some(keep),
                "小数/日期是业务文字，属性不该被整条丢弃：{keep}"
            );
        }
        assert_eq!(
            crate::domain::html_view::sanitize_display("扣款币种").as_deref(),
            Some("扣款币种")
        );
    }

    /// **证明这次是加强而不是等价改写**：新判据在旧判据的真阳性上**全中**
    /// （漏网为零），新增（漏抓修复）至少一处；旧判据独有的命中**仅限全数字串**
    /// ——即本轮要修的误伤，可穷举。
    #[test]
    fn new_criterion_is_a_strengthening_of_the_old_one() {
        // 旧判据（形状，**不要求任何一段含字母**）
        fn old(token: &str) -> bool {
            let segs: Vec<&str> = token
                .split('.')
                .filter(|s| {
                    !s.is_empty()
                        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                })
                .collect();
            if segs.len() >= 2 {
                return true;
            }
            if token.contains('_') {
                return token.split('_').filter(|p| !p.is_empty()).count() >= 2;
            }
            false
        }
        // 真阳性样本：旧判据命中 ⇒ 新判据必须也命中（漏网为零）
        for tok in [
            "a.b", "z.y.x", "customer_id", "base_url", "api_key", "jd_api",
            "t052.model_id", "v2_model", "x_1", "t.c", "billing_account",
            "llm_provider_back_20260701", "rewrite_times",
        ] {
            assert!(old(tok), "样本应当是旧判据的真阳性: {tok}");
            assert!(is_identifier_token(tok), "新判据漏了真阳性: {tok}");
        }
        // 旧判据独有的命中：逐个确认**段全是纯数字**（即本轮修掉的误伤）
        for tok in ["1.5", "3.14", "2024.10", "0.8", "1_000_000", "2024_10_01"] {
            assert!(old(tok), "样本应当是旧判据的误伤: {tok}");
            assert!(!is_identifier_token(tok), "全数字串不该再命中: {tok}");
            assert!(
                tok.split(['.', '_']).all(|s| s.bytes().all(|b| b.is_ascii_digit())),
                "误伤样本应当每段都是纯数字: {tok}"
            );
        }
    }
}
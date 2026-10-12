//! **REQ-087 · 用 `is_primary_key` 产出 `1:1`**（AC rev.2 · 规则入库，**暂不接线**）
//!
//! # 一句话说清状态
//!
//! **本规则写好了、测好了，但【尚未接线】** —— [`WIRED`] 当前是 `false`，
//! 所以 [`crate::extract::propose::infer_link`] 的产出与基线 `35e4a09`
//! **逐字节一致**（80 条关系的基数、语义、方向一条不变）。
//!
//! # 为什么不接线（BA rev.2 裁定，2026-10-12）
//!
//! 裁定是「**收益留、风险封**」：多库场景下 `1:1` 是真需求，规则**不该删**；
//! 但接线的唯一前置是
//! [`crate::domain::view::semantic_cn`] **能翻译 `corresponds_to`**。
//!
//! ## 这条前置为什么是硬的
//!
//! `semantic_cn` 只认 6 个语义（`has` / `belongs_to` / `depends_on` /
//! `references` / `records` / `derived_from`），其余走 `_ => s.to_string()`
//! **原样透出**，紧接着被第四档守门 `untranslated_semantic_hit`
//! （判据 = `meaning` 值里出现**任何 ASCII 字母**）抓住
//! ⇒ **`cli.rs` 拒绝发布整份客户视图**。
//!
//! 而 `corresponds_to` 是**本 REQ 新引入的**第五类产出路径：
//! 基线 `REL_SUFFIXES` 四条全部产出 `1:N` / `N:1`，
//! **没有任何路径能产出 `1:1`** ⇒ 那是新失败模式，不是既存问题。
//!
//! ⚠️ 那个开关在**红线** `src/domain/view.rs` 里，本 REQ 禁改
//! ⇒ 已另立 **REQ-089**（特批一行）。
//!
//! ## 接线有多贵
//!
//! **一个字符**：把 [`WIRED`] 改成 `true`。调用点已经在位
//! （`propose.rs` 的循环头与 `evidence.claim`），届时无需再动 `propose.rs`
//! —— 也就不必再碰 REQ-084 钉死的 130 个行号。
//!
//! # 为什么它住在独立文件，而不是 `propose.rs`
//!
//! **不是因为整洁，是因为门禁。** REQ-084 的保留项登记
//! （`tests/retained_names.req084.tsv`）把 `src/extract/propose.rs` 里
//! 130 处客户真表名的**行号 + 列号**逐个钉死，
//! 由 `tests/test_retained_names_req084.rs` 的锁 ② 实物核对。
//!
//! ⇒ **在 `propose.rs` 里增删任何一行**，那 130 行里位于其后的部分全部平移，
//! 门禁立刻变红（本人第一版就是这么变红的：新增 22 处真名 + 行号平移，
//! 两条断言同时挂掉）。
//!
//! 所以：规则、说明与它的测试全部落在本文件；`propose.rs` 只做
//! **等行数的改写**，不增一行、不删一行。
//! 这不是绕过门禁，是**不去动另一个 REQ 的成果**。
//! ⚠️ AC-004 写的是「登记在 `infer_link` 的文档注释里」，
//! 这里改成了「登记在本模块，`propose.rs:224`（`REL_SUFFIXES` 的文档注释）里加一行指向这里」——
//! 偏离与理由如实登记，BA 已判**保留此处理**。
//!
//! （QA 复核纠正：**本行原先写「`infer_link` 文档里」是错的**，指针实际落在 `REL_SUFFIXES`
//! 的文档注释上。`REL_SUFFIXES` 反而是更好的锚点——它正是「按列名后缀复述」这件事的定义处。）

/// **接线闸 —— 本 REQ 唯一的开关。**
///
/// # 当前值 `false` 的含义（不是「忘了接」）
///
/// 这是 BA rev.2 的**裁定结果**，不是遗漏：
/// 规则本体与测试都在库里，但**在 [`semantic_cn`] 能翻译 `corresponds_to`
/// 之前不得接线**（REQ-089）。接线的前置一旦具备，改成 `true` 即可。
///
/// # 为什么闸要同时管住「基数」与「证据」两处
///
/// 接线只改一半会造成**自证假溯源**：若基数仍是 `N:1` 而 `evidence.claim`
/// 写「基数由本表主键判为 1:1」，那条证据就在说假话 ——
/// 与 REQ-019 修掉的 `extracted_by` 占位名同一类病。
/// 所以 [`evidence_claim`] 也必须过同一个闸，否则现在就会开始说谎
/// （今天 80 条关系的映射列里主键 0 条，所以尚未被观测到）。
pub(crate) const WIRED: bool = false;

/// **本表主键列给出的基数** —— `1:1`。
///
/// # 「不新造第四个词」：这是**既有词形**
///
/// AC-003 点名的失败模式是「`count_phrase` 认不出的词形落到兜底分支 ⇒
/// 页面说『多个』而实际是 1 个」。逐条核对下来，`1:1` 走完整条链
/// **不需要任何改动**：
///
/// | 环节 | 位置 | 现状 |
/// | --- | --- | --- |
/// | 客户视图翻基数 | `domain::view::cardinality_cn` | 已有 `"1:1" => "唯一对应"` |
/// | 数量词出口 | `domain::html_view::count_phrase` | 已有 `("唯一对应", _) => one` |
/// | 总览复用 | `domain::overview` | 调的是**同一个** `count_phrase` |
///
/// ⇒ 基数侧**没有**风险；风险全在语义侧（见模块头 REQ-089）。
pub(crate) const CARD_ONE_TO_ONE: &str = "1:1";

/// **本表主键列给出的语义** —— `corresponds_to`（唯一对应）。
///
/// # ⚠️ 本词**尚未**进入客户视图的翻译词表（这就是不能接线的原因）
///
/// `semantic_cn` 的六个分支里没有它 ⇒ `_ =>` 兜底原样透出
/// ⇒ 第四档守卫抓住 ⇒ **整份客户视图拒绝发布**。
/// 是 **fail-closed**（拒发，不是把错数据发给客户），方向与本模块
/// 「少一条胜过错一条」的纪律同向；但它仍是一颗会卡住发布的雷。
/// 补法在 **REQ-089**（红线 `view.rs` 特批一行）。
pub(crate) const SEM_CORRESPONDS_TO: &str = "corresponds_to";

/// **规则本体（不含接线闸）** —— 主键 ⇒ 该给什么基数与语义。
///
/// # 判据
///
/// **主键 ⇒ 一行里恰好一个值**（单值、非空、表内唯一）。
/// 关系方向是「子表 → 主表」，于是「站在子表这一端看过去，
/// 有几个父表行」**至多 1 个** ⇒ `1:1`。
///
/// # 为什么**无条件覆盖**四种后缀，含 `_ids` 的 `1:N`
///
/// 「既是主键又叫 `x_ids`」是**输入自相矛盾**：主键不可能多值。
/// 此时以约束为准。这不违反「少一条胜过错一条」——
/// 矛盾的是**输入**，不是产出。
///
/// # 它**不**做什么
///
/// 它不判断目标表对不对。`*_id` 到底指哪张表，仍由 `infer_link` 的
/// 两档判据 + 四道闸决定；本函数只在**那条关系已经解出来之后**
/// 改写它的基数与语义。⇒ 闸 A/B/C/D 一条都没被放宽。
///
/// # 为什么单独抽出来
///
/// 它**不含接线闸**，所以规则本身的正确性在 `WIRED = false` 时
/// **仍然可以被测**（见 `test_rule_body_*`）。否则「暂不接线」会把
/// 规则的判别力一并关掉，只剩一个永远为真的测试。
pub(crate) fn decide(is_primary_key: bool) -> Option<(&'static str, &'static str)> {
    is_primary_key.then_some((CARD_ONE_TO_ONE, SEM_CORRESPONDS_TO))
}

/// 后缀定下的 `(suffix, cardinality, semantic)`，必要时按主键覆写后两项。
///
/// `suffix` 原样带回，是为了让调用点能直接写出这个三元组，
/// **不必在调用点新增任何一行**（见模块头的门禁说明）。
///
/// 取 [`WIRED`] 当前值 ⇒ 今天的行为与基线**逐字节一致**。
pub(crate) fn card_for(
    suffix: &'static str,
    suffix_card: &'static str,
    suffix_sem: &'static str,
    is_primary_key: bool,
) -> (&'static str, &'static str, &'static str) {
    card_for_gated(suffix, suffix_card, suffix_sem, is_primary_key, WIRED)
}

/// [`card_for`] 的**闸显式化**版本：把「接不接线」变成参数。
///
/// 生产路径只经 [`card_for`]（固定用 [`WIRED`]）；
/// 这个版本**只给测试**用，好让「接上了会怎样」在闸关闭期间
/// 仍可被断言 —— 否则规则的一半行为永远测不到。
pub(crate) fn card_for_gated(
    suffix: &'static str,
    suffix_card: &'static str,
    suffix_sem: &'static str,
    is_primary_key: bool,
    wired: bool,
) -> (&'static str, &'static str, &'static str) {
    if wired {
        if let Some((card, sem)) = decide(is_primary_key) {
            return (suffix, card, sem);
        }
    }
    (suffix, suffix_card, suffix_sem)
}

/// `evidence[0].claim` 的原文。`is_tail` = 词干走的是尾缀档（闸 A 之后那档）。
///
/// # 为什么 `claim` 必须按事实改口径
///
/// 这条 `claim` 是**评审者唯一能看到的推断依据**。
/// 基数明明来自 `is_primary_key`，却写成「按 `*_id` 命名约定推断」，
/// 那就是**自证假溯源** —— 与 REQ-019 修掉的 `extracted_by` 占位名同一类病。
/// 而「目标表怎么找到的」与「基数怎么定的」是两件独立的事，各有各的措辞。
///
/// # 为什么它也过 [`WIRED`] 闸
///
/// 今天没接线，基数就仍是列名后缀的复述 —— 证据**必须**照实这么写。
/// 若证据先于基数说「主键判的 1:1」，那是在替一个还没发生的接线作证。
pub(crate) fn evidence_claim(is_tail: bool, by_pk: bool) -> &'static str {
    evidence_claim_gated(is_tail, by_pk, WIRED)
}

/// [`evidence_claim`] 的闸显式化版本，**只给测试**用（理由同 [`card_for_gated`]）。
pub(crate) fn evidence_claim_gated(is_tail: bool, by_pk: bool, wired: bool) -> &'static str {
    if wired && by_pk {
        return if is_tail {
            "零外键库：目标表按 *_id 命名约定命中（词干是已知表名的**唯一**尾缀匹配，表名带业务前缀）；\
             **基数由本表主键判为 1:1**（主键一行恰好一个值 ⇒ 子表→主表方向上「1 个」）"
        } else {
            "零外键库：目标表按 *_id 命名约定命中（词干直接展开命中已知表名）；\
             **基数由本表主键判为 1:1**（主键一行恰好一个值 ⇒ 子表→主表方向上「1 个」）"
        };
    }
    if is_tail {
        "零外键库，按 *_id 命名约定推断（词干是已知表名的**唯一**尾缀匹配，表名带业务前缀）"
    } else {
        "零外键库，按 *_id 命名约定推断（词干直接展开命中已知表名）"
    }
}

// ─────────────────────────────────────────────────────────────
// 已知限制登记（AC-004）
// ─────────────────────────────────────────────────────────────
//
// ⚠️ **本 REQ 不解决「基数无依据」这件事**，别把它读成解决了。
//
// 1. **`N:1` 仍是列名后缀的复述，不是观测结论。**
//    一个 `_id` 结尾的列名**说不出**「本表多行是不是都指向同一行父表」。
//    本 REQ 只在「该列恰好是本表主键」这一小类里有办法换掉这个说法；
//    而按 rev.2 裁定**尚未接线**，所以今天连这一小类也没有换 ——
//    其余 80 条仍是**无依据的**。
//
// 2. **判 `N:1` vs `1:1` 需要 UNIQUE 约束或值域，当前架构两个都拿不到。**
//    - `ColumnMeta` 只采 `data_type` / `nullable` / `comment` / `is_primary_key`，
//      **全文无 `unique` 字段**；
//    - 源库只读 `information_schema` / `pg_catalog`，**零业务数据行**
//      ⇒ 值域重叠也测不出来。
//    这是**架构限制**，不是本 REQ 能修的。
//
// 3. **源库零外键 ⇒ 目标表命中靠列名/词干匹配，也可能命中错。**
//    「词干是不是某张表的直接展开」是**命名启发式**，不是数据库声明的引用。
//    闸 B/C/D 挡掉的是**歧义**，挡不掉「唯一命中但命中错」。
//
// 4. **本词 `corresponds_to` 未进 `semantic_cn`** ⇒ 见 [`SEM_CORRESPONDS_TO`]。
//    这是**暂不接线**的直接原因，补法在 REQ-089。
//
// 5. **规则尚未接线** ⇒ 见 [`WIRED`]。今天它对产物零影响。
//
// # 实测：这条规则在当前语料下**零触发**（即使接线也是零触发）
//
// 可复跑口径：
// - 登记基线里 `is_primary_key == true` 的列 = **93**
//   （`jq '[.tables[].columns[]|select(.is_primary_key==true)]|length]' schema.json`）
// - 80 条关系的映射列（`evidence[0].column`）里是主键的 = **0**
//   （按 `(from_type, column)` 与上表求交集）
//
// ⇒ **80 条基数逐条不变，两个产物文件的 sha 不变。**
//
// # 同 REQ-066 残留 S6 的处置口径
//
// **按指定口径登记，不写成「已封堵」。** 上面第 1~3 条没有任何一条被本 REQ
// 消除，它们只是被**写下来**了；第 4、5 条则明确写为**未接线**。

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::html_view::count_phrase;

    /// **规则本体**：主键 ⇒ `1:1` / `corresponds_to`；非主键 ⇒ 不动。
    ///
    /// ⚠️ 刻意**不经过接线闸**（直接测 [`decide`]）：
    /// 否则闸一关，规则本身的判别力就跟着一起关掉了，
    /// 剩下的只是一个恒真的测试。
    #[test]
    fn test_rule_body_maps_primary_key_to_one_to_one() {
        assert_eq!(
            decide(true),
            Some((CARD_ONE_TO_ONE, SEM_CORRESPONDS_TO)),
            "★ 主键 ⇒ 1:1 / corresponds_to，这是规则本体的全部内容"
        );
        assert_eq!(decide(false), None, "非主键不该给出任何覆写");
    }

    /// **当前状态 = 未接线**：生产路径 [`card_for`] 拿到主键也**原样返回**。
    ///
    /// 这条测试守着「暂不接线」这个裁定：接线前它绿；哪天有人把
    /// [`WIRED`] 改成 `true` 却没同步处理语义翻译（REQ-089），它立刻变红。
    #[test]
    fn test_rule_is_banked_but_not_wired_yet() {
        assert!(!WIRED, "BA rev.2 裁定：规则入库、暂不接线。接线前置 = REQ-089");
        for (suffix, card, sem) in [
            ("_id", "N:1", "belongs_to"),
            ("_ids", "1:N", "has"),
            ("_no", "N:1", "references"),
            ("_code", "N:1", "references"),
        ] {
            assert_eq!(
                card_for(suffix, card, sem, true),
                (suffix, card, sem),
                "未接线时主键列必须拿到**原样**的 (suffix, card, sem) —— \
                 否则产物就变了，AC-005 的「零变化」不成立"
            );
        }
    }

    /// **接线后会怎样**：四个后缀全部覆写，含 `_ids` 的 `1:N`。
    ///
    /// 「既是主键又叫 `x_ids`」是输入自相矛盾（主键不可能多值），以约束为准。
    #[test]
    fn test_wiring_overrides_every_suffix_including_plural() {
        assert_eq!(
            card_for_gated("_id", "N:1", "belongs_to", true, true),
            ("_id", CARD_ONE_TO_ONE, SEM_CORRESPONDS_TO)
        );
        assert_eq!(
            card_for_gated("_ids", "1:N", "has", true, true),
            ("_ids", CARD_ONE_TO_ONE, SEM_CORRESPONDS_TO),
            "主键不可能多值 ⇒ 1:N 也必须被覆写成 1:1"
        );
    }

    /// **反向对照**：接线了也**不得**波及非主键列。
    ///
    /// 变异 = 把判据写成「无条件覆写」时，这条会红。
    #[test]
    fn test_wiring_never_touches_a_non_primary_key_column() {
        for (suffix, card, sem) in [
            ("_id", "N:1", "belongs_to"),
            ("_ids", "1:N", "has"),
            ("_no", "N:1", "references"),
            ("_code", "N:1", "references"),
        ] {
            assert_eq!(
                card_for_gated(suffix, card, sem, false, true),
                (suffix, card, sem),
                "{suffix} + 非主键 ⇒ 必须原样返回，覆写不得扩大"
            );
        }
    }

    /// **闸对两处同时生效**：基数与证据要么都接、要么都不接。
    ///
    /// 今天（未接线）证据**必须**照旧写「按命名约定推断」——
    /// 若它抢先写「主键判的 1:1」，就是**替一个还没发生的接线作证**。
    #[test]
    fn test_unwired_evidence_does_not_claim_a_primary_key_basis() {
        for is_tail in [false, true] {
            assert!(
                !evidence_claim_gated(is_tail, true, false).contains("主键"),
                "未接线时证据不得出现「主键」口径（否则证据在与基数说不同的话）"
            );
            assert!(evidence_claim_gated(is_tail, false, false).contains("零外键库"));
        }
        // 接上线之后，证据才跟着基数改口
        assert!(evidence_claim_gated(false, true, true).contains("本表主键"));
        assert!(evidence_claim_gated(true, true, true).contains("本表主键"));
        // 非主键两档措辞必须**逐字节**等于基线原文（80 条既有关系依赖它）
        assert!(evidence_claim_gated(false, false, true).contains("直接展开"));
        assert!(evidence_claim_gated(true, false, true).contains("尾缀"));
    }

    /// **证据与基数必须永远同口径**（枚举 4 后缀 × 2 主键 × 2 闸态 = 16 组）。
    ///
    /// 守的是「**只接一半**」这个失败模式：基数接了而证据没接，
    /// 证据就会说「主键判的 1:1」而基数还是 `N:1`；
    /// 反过来证据先接则会在**还没接线时**就开始说假话。
    /// 两种都靠 [`WIRED`] 同时管住两处来杜绝 —— 这条测试把那个
    /// 「两处必须同步」的不变量显式钉住。
    #[test]
    fn test_evidence_never_contradicts_the_cardinality() {
        for (suffix, card, sem) in [
            ("_id", "N:1", "belongs_to"),
            ("_ids", "1:N", "has"),
            ("_no", "N:1", "references"),
            ("_code", "N:1", "references"),
        ] {
            for is_pk in [false, true] {
                for wired in [false, true] {
                    let (_, got_card, _) = card_for_gated(suffix, card, sem, is_pk, wired);
                    let claim = evidence_claim_gated(false, is_pk, wired);
                    let evidence_says_pk = claim.contains("本表主键");
                    assert_eq!(
                        evidence_says_pk,
                        got_card == CARD_ONE_TO_ONE,
                        "证据与基数必须同口径：{suffix} / is_pk={is_pk} / wired={wired} 下\
                         基数是 {got_card}，而证据的主张是 {evidence_says_pk}"
                    );
                }

                // ⚠️ 上面那段查的是**闸显式化**版本；这一段查**生产入口**。
                // 少了它，「只给证据接线、基数不接」这种半接线就抓不住
                // —— 实测变异 `evidence_claim(…, true)` 确实曾从这里溜过去。
                let (_, prod_card, _) = card_for(suffix, card, sem, is_pk);
                let prod_claim = evidence_claim(false, is_pk);
                assert_eq!(
                    prod_claim.contains("本表主键"),
                    prod_card == CARD_ONE_TO_ONE,
                    "生产入口两处必须同口径：{suffix} / is_pk={is_pk} 下\
                     基数是 {prod_card}，证据却主张了另一回事"
                );
            }
        }
    }

    /// **AC-003 的下游口径自查**：`1:1` 是**既有词形**，不是新造的第四个词。
    ///
    /// 若有人把产出改成 `1-1` / `one_to_one` 之类的新词形，
    /// `count_phrase` 会掉进 `_ => many` 兜底，页面就会说「多个」
    /// 而实际是 1 个（AC-003 点名的失败模式）。
    ///
    /// ⚠️ 「`1:1` → 「唯一对应」」那一半**不由本测试守**：`cardinality_cn` 是
    /// `domain/view.rs` 的私有权函数，而该文件是红线（禁改），
    /// 从这里调不到、也不该为它开口子。改由既有两处守：
    /// `view` 模块自带的基数译词测试，与 `tests/test_view.rs` 的
    /// `KNOWN_CARDINALITIES`。
    #[test]
    fn test_one_to_one_is_an_already_known_cardinality_token() {
        assert_eq!(CARD_ONE_TO_ONE, "1:1");
        // 数量词出口侧：`唯一对应` 两端都答「1 个」，不会落到 `many` 兜底
        assert_eq!(count_phrase("唯一对应", true), "1 个");
        assert_eq!(count_phrase("唯一对应", false), "1 个");
        // 反向对照：`N:1` 走的是 `many` 那一支（站在子表这端看是「多个」）
        assert_eq!(count_phrase("多对一", false), "多个");
    }
}

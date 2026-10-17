//! REQ-104 · 客户视图的**属性级剔除** —— 出口层动作，**全仓只有这一份**。
//!
//! # 这道剔除是什么
//!
//! 客户视图里的属性说明是**源库列注释原文**。原文里夹着技术串与英文守门词
//! （`Kafka key`、`API Key`、`CDC/table/op/pk/lsn`），于是同一份视图在不同出口
//! 有**两种结果**：页面出口早就把它们剔除、整份守门通过；JSON 出口与只读图服务
//! 不剔，守门器逐字扫到就拒发 —— `demo3` 第 2 版有 3 个节点点不开（HTTP 500）。
//!
//! # 判据**不在本文件**
//!
//! 判据只有一份：[`crate::domain::identifier_shape::has_raw_identifier`]
//! （源库标识符的**形态** ∪ `key` 字面量）。页面层与 MCP 层此前各持副本且已漂移，
//! REQ-029 把两份副本都合并掉了 —— **本文件只调用，不复写**（纪律 H：
//! 抄一份判据就是制造下一次漂移）。
//!
//! # 三处出口共用这一个函数
//!
//! - [`crate::cli`]：`ontology show` 的 **JSON** 出口；
//! - [`crate::server`]：`/api/node` 的节点视图；
//! - [`crate::mcp::tools`]：工具返回的属性清单。
//!
//! 页面出口（`ontology show --html`）**不调本函数** —— 它在
//! [`crate::domain::html_view`] 里已有自己的一套展示级清洗，本轮不动它。
//!
//! ⚠️ **`ontology overview` 也不调本函数，但原因不同**：它在
//! [`crate::domain::overview`] 里复用 `html_view::sanitize_display`
//! （`overview.rs` 顶部 `use`，用在对象说明与属性名两处），
//! 而 `sanitize_display` **是 `has_raw_identifier` 的超集**
//! （还多一道中文建模词判定 + `ID` 记号摘除）。
//! ⇒ overview **本来就已覆盖**，**不是缺口**。
//!
//! ⚠️ 本模块与 `sanitize_display` 的剔除方向也**不会打架**：
//! 前者是后者的**真子集** —— 本模块剔掉的，`sanitize_display` 一定也剔。
//! 两者只在**改名**（`strip_id_token`：`客户ID` → `客户`）上不同，
//! 而那是 `sanitize_display` **早就有**的展示行为，与本模块无关。
//!
//! # ⛔ 守门器一行没改，两种口径的差异原样保留
//!
//! [`crate::mcp::tools::guard`] 保持原样。所以：
//! - `ontology show`（JSON）仍是**整份** fail-closed，命中即 `exit 2`；
//! - `/api/node` 仍是**逐节点**判，一个节点脏只拒发那一个节点。
//!
//! 本模块只在这道闸门**之前**减掉属性，**不放宽**任何一条判据。
//!
//! # 为什么只剔属性名，不拿它去顶替守门器
//!
//! 真·建模术语（`主键` / `基数` / `1:N` / `cardinality`）**不是标识符形态**，
//! [`crate::domain::identifier_shape::has_raw_identifier`] 对它们**一律不命中**
//! ⇒ 它们照旧到达守门器、照旧被拒。
//!
//! ⇒ 「客户视图不得含建模术语」这个不变量没有被削弱：
//! **被剔的只是客户标签里放不进去的源库标识符**（`customer_pk`、`belongs_to`），
//! 剔掉之后视图里根本没有它，不存在「漏出去」这回事。
//! 失败的粒度从「整份视图拒发」挪到「一个属性不上」，与页面出口今天的行为一致。
//!
//! # ⛔ 已知限制 · 丢弃率门禁的覆盖面缩小 · **未封堵**（纪律 H）
//!
//! `tests/drop_rate.rs` 的语义是「**抽出来的属性**里有多少在**投影成客户视图**时被丢弃」。
//! 本模块**位于 `view::project()` 之后**，所以那门禁调 `project()` 时**看不见**本模块的剔除。
//!
//! | 口径 | 门禁测量到的丢弃率 | 本模块额外剔除 |
//! | --- | --- | --- |
//! | 门禁默认 fixture（仓库根脱敏件 · 91 表） | 7.78%（75/964） | +6 条（0.62pp） |
//! | 活体源库（`ontok-introspect` · 96 表） | 8.11%（83/1024） | +20 条（1.95pp） |
//!
//! ⛔ **因此门禁的语义已从「客户可见的丢弃」窄化为「投影内部的丢弃」。**
//! 这**不是**门禁坏了 —— 门禁仍然钉住 `view::project()` 里那些丢弃；
//! 它不再覆盖出口层这约 20 条。
//!
//! - **信息没有完全丢失**：页面出口的 stderr 一直在报
//!   「有 N 条属性带技术用语，没上页面」；本模块也返回同口径的剔除清单，
//!   由各出口自己报到 stderr（内容不进响应体，理由同守门器）。
//! - **本限制为什么留着**：把剔除放进 `view::project()` 会让门禁分子 +20，
//!   活体口径下变成 **10.06%**，**撞破 `MAX_DROP_RATE = 0.10`**；
//!   而 `tests/drop_rate.rs` 是禁改文件，抬不了阈值。负责人裁定取「出口层 + 登记」。
//! - **何时需要重新评估**：
//! ① 门禁口径本身调整（分母/分子定义变化）时，必须把出口层的剔除**计入**，
//!    否则门禁会长期低报客户真实可见的字段损失；
//! ② 届时若活体口径仍在 10% 附近，应当重新权衡「放进投影」还是「调阈值」，
//!    而不是让两套口径继续各测各的。
//!
//! ⛔ 以上是**登记**，不是「已封堵」。任何把本段改写成「门禁已覆盖」的说法都是错的。
//!
//! # ⛔ 已知限制 · 三条护栏缺口 · **未封堵**（REQ-104 · QA 复审登记）
//!
//! 以下三条由 QA 独立复审发现。**都只登记，没修** —— 每条要么超出本轮范围，
//! 要么已被明确划给别的需求。**三条都是登记，不是已封堵。**
//!
//! ## ① 「整份 fail-closed vs 逐节点判」的差异**零测试覆盖**
//!
//! **事实**：本轮刻意保留了这个差异 —— `ontology show`（JSON）命中即整份 `exit 2`，
//! `/api/node` 命中只拒发那一个节点（HTTP 500）。
//!
//! **缺口**：**全 `tests/` 无任何断言钉住它**。
//! 搜过什么：`grep -rn 'EXIT_LEAKED_MODELING_TERMS|exit\(2\)' tests/`
//! ⇒ 只有 `test_binary_output_guard.rs` 在断言**常量被接线**（`:1253`）、
//! 以及 `test_domain_print_sites_binary_req078.rs:90` 在**回避** `exit(2)` 场景；
//! **没有任何一条断言这两种口径的行为差异**。
//!
//! **为什么难钉**：剔除之后整份视图往往就干净了，差异**看不出来**；
//! 要钉住它，必须构造一份「只有其中一个节点脏」的视图，
//! 然后同时断言「整份拒发」与「另一个节点仍打得开」—— 两个断言缺一不可。
//!
//! **何时需要补测试**：① 有人改动 `guard()` 的调用粒度（整份 → 逐份）；
//! ② 有人改动任一出口的剔除位置；③ 本模块被下线或搬家。
//! 在那之前，这个差异**只由文档（本段 + `server/mod.rs`）保证**。
//!
//! ## ② `customer_pk` 这类**由 fail-closed 变成静默丢弃** —— 方向性变化
//!
//! **事实**：QA 用 7 个对照串实测，本模块的剔除**吃掉**了
//! `belongs_to`、`customer_pk`（snake_case 形态）；
//! 仍到达守门器并被拒的是 `主键服务中心`、`主键标识`、`外键指向客户`、
//! `1:N`、`cardinality`。
//!
//! **变化方向**：这些串**以前**让整份视图 `exit 2`（fail-closed，客户拿不到任何数据），
//! **现在**变成「那一条属性静默消失、其余照发」（静默丢弃）。
//! **这是放宽，不是收紧** —— 判读人/客户拿到的是**少一条属性**，
//! 而不是「明确知道这份数据没过闸」。`customer_pk` 正是 AC-002 的对照材料之一。
//!
//! **影响面**：只波及**恰好带源库标识符形态**的那几条属性；
//! 真实 `demo3` v2 上是 19 条（属性）+ 2 条（对象说明），
//! 每条都按页面出口同口径报到 stderr（见上方「信息没有完全丢失」）。
//!
//! **为什么留着**：页面出口**早就是这个行为**（`html_view::sanitize_display`），
//! 本轮只是让其余出口**与之一致**。要改成「先拒发再让人去修」，
//! 得先决定「拒发整份」还是「拒发单条」—— 那是产品裁定，不是本轮能定的。
//!
//! ## ③ 三处出口共用一个函数这条判据是**单向**的
//!
//! **事实**：`all_three_outlets_call_the_one_shared_function`
//! （`tests/test_property_filter.rs`）只断言「这三个文件**调了**共享函数」。
//!
//! **它钉得住**：删调用、改名、三处各写各的。
//! **它钉不住**：**保留共享调用、同时另加一份真在生效的本地判据**。
//! QA 变异 **M3** 实测：在 `cli.rs` 保留共享调用、另加一份本地判据 ⇒ **9 条全绿**。
//! 这正是纪律 H 点名的盲区 ——「看起来测了、其实测不全」。
//!
//! **已划走**：加固守卫**已并入 REQ-103**，本轮不做。
//! ⇒ **别把这条测试当成「防住多一份判据」的东西。**
//! 要真正防住，得改成「三个文件里 `has_raw_identifier` 的调用点数各为 0」
//! 或反向的「除 `identifier_shape` 外全仓无第二处判据实现」这类**否定式**断言。
//!
//! ⛔ 以上三条**均未封堵**。任何把本段改写成「已补测试」「已改 fail-closed」
//! 或「结构守卫已覆盖多份判据」的说法都是错的。

use crate::domain::identifier_shape::has_raw_identifier;
use crate::domain::view::{CustomerView, EntityView, PropertyView};

/// 一条剔除记录 —— **只进 stderr，不进响应体**。
///
/// 理由与守门器同一条：命中词本身是要被挡在外面的东西，
/// 把它写进返回给客户端的响应等于从错误信息里开后门。
/// 清单里的对象名/属性名都是**已经判过「不是技术串」**的那些，
/// 但仍按运维口径走 stderr，客户看不见。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DroppedProp {
    /// 所属对象的人话名。
    pub object: String,
    /// 属性人话名。
    pub property: String,
    /// `true` = 整条属性被剔除；`false` = 只剔了说明文字，属性还在。
    pub whole_property: bool,
}

impl DroppedProp {
    /// stderr 上的一行 —— 与页面出口那句「有 N 条属性带技术用语」同口径。
    pub fn line(&self) -> String {
        if self.whole_property {
            format!("{}/{}", self.object, self.property)
        } else {
            format!("{}/{}（说明文字带技术用语）", self.object, self.property)
        }
    }
}

/// 剔除后的视图 + 剔除清单。
#[derive(Debug, Clone, PartialEq)]
pub struct FilteredView {
    pub view: CustomerView,
    /// 被剔除的属性 / 说明文字，按视图里的出现顺序。
    pub dropped: Vec<DroppedProp>,
    /// 对象说明被整段隐去的对象名（来源是**表注释原文**，同样可能带技术串）。
    pub silenced_objects: Vec<String>,
}

impl FilteredView {
    /// 剔除总条数（属性 + 说明文字）。
    pub fn dropped_len(&self) -> usize {
        self.dropped.len()
    }

    /// stderr 报告文本（**纯字符串**；各出口自己 `eprintln!`，domain 层不做 I/O）。
    ///
    /// 口径与页面出口那句「有 N 条属性带技术用语，没上页面」对齐，
    /// 但这里说得更准：**分两种动作**（整条剔除 / 只隐去说明），
    /// 不报一个含混的总数 —— 判读人分不清「少了一条属性」和「少了一句说明」。
    ///
    /// 前 20 条明细，超出只给计数（与页面出口同一条上限）。
    pub fn report(&self) -> String {
        let whole = self.dropped.iter().filter(|d| d.whole_property).count();
        let notes = self.dropped.len() - whole;
        if self.dropped.is_empty() && self.silenced_objects.is_empty() {
            return String::new();
        }
        let mut out = format!(
            "⚠ 有 {} 条属性带技术用语，没进客户视图（整条剔除 {} · 只隐去说明 {}）",
            self.dropped.len(),
            whole,
            notes
        );
        if !self.silenced_objects.is_empty() {
            out.push_str(&format!(
                "；另有 {} 个对象的说明文字同样带技术用语，已整段隐去：{}",
                self.silenced_objects.len(),
                self.silenced_objects.join("、")
            ));
        }
        let shown: Vec<String> = self.dropped.iter().take(20).map(DroppedProp::line).collect();
        if shown.is_empty() {
            out.push_str("。");
        } else {
            out.push_str(&format!("。前 {} 条：{}", shown.len(), shown.join("、")));
            if self.dropped.len() > shown.len() {
                out.push_str(&format!("（其余 {} 条省略）", self.dropped.len() - shown.len()));
            }
        }
        out
    }
}

/// 这段说明文字是不是带技术串。
fn note_is_technical(note: &str) -> bool {
    has_raw_identifier(note)
}

/// 剔除**一个**对象的属性清单。
///
/// 分两种动作，与页面出口一致：
/// - 属性名脏 ⇒ **整条属性**剔除（名字没法只留一半）；
/// - 属性名干净、只有说明脏 ⇒ **只隐去说明**，属性还在。
fn filter_properties(object: &str, props: &[PropertyView]) -> (Vec<PropertyView>, Vec<DroppedProp>) {
    let mut kept = Vec::with_capacity(props.len());
    let mut dropped = Vec::new();
    for p in props {
        if has_raw_identifier(&p.name) {
            dropped.push(DroppedProp {
                object: object.to_string(),
                property: p.name.clone(),
                whole_property: true,
            });
            continue;
        }
        let note = match p.note.as_deref() {
            Some(n) if !n.trim().is_empty() && note_is_technical(n) => {
                dropped.push(DroppedProp {
                    object: object.to_string(),
                    property: p.name.clone(),
                    whole_property: false,
                });
                None
            }
            other => other.map(str::to_string),
        };
        kept.push(PropertyView {
            name: p.name.clone(),
            note,
            required: p.required,
            sample: p.sample.clone(),
        });
    }
    (kept, dropped)
}

/// 对**整份客户视图**做属性级剔除。
///
/// **幂等**：对已剔除过的视图再跑一次，返回值逐字相等（测试钉住）。
/// 「幂等」不是洁癖 —— 三个出口都可能拿到上游已处理过的视图，
/// 两套实现叠加出「跑一次少一条」是最难查的一类偏差。
pub fn filter_view(view: &CustomerView) -> FilteredView {
    let mut dropped: Vec<DroppedProp> = Vec::new();
    let mut silenced: Vec<String> = Vec::new();
    let mut objects: Vec<EntityView> = Vec::with_capacity(view.objects.len());

    for o in &view.objects {
        // 对象说明来自**表注释原文**，同样可能带技术串。页面出口隐去它，
        // 这里保持同一口径：不硬改、整段不显示。
        let note = match o.note.as_deref() {
            Some(n) if !n.trim().is_empty() && note_is_technical(n) => {
                silenced.push(o.name.clone());
                None
            }
            other => other.map(str::to_string),
        };
        let (properties, mut here) = filter_properties(&o.name, &o.properties);
        dropped.append(&mut here);
        objects.push(EntityView {
            name: o.name.clone(),
            note,
            belongs_to: o.belongs_to.clone(),
            aliases: o.aliases.clone(),
            properties,
        });
    }

    FilteredView {
        view: CustomerView {
            objects,
            relations: view.relations.clone(),
        },
        dropped,
        silenced_objects: silenced,
    }
}

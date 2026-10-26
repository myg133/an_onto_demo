//! 客户视图投影 —— 纯逻辑，**零 I/O**。
//!
//! 规格 §8.3：客户看到的必须是**业务语言**，不能出现任何建模术语
//! （主键 / 基数 / 外键 / ordinal / key …）。
//!
//! 验收 AC-005。**本模块是「判据」本身的实现** ——
//! REQ-001 spike 的 AC-005 就是「客户能不能一眼判读」，而这份输出就是给人看的那份。

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// 系统标识 → 客户能听懂的说法。**外键列名只出现它的目标对象名。**
const COLUMN_LABELS: &[(&str, &str)] = &[
    ("customer_id", "所属客户"),
    ("cust_id", "所属客户"),
    ("main_customer_id", "主客户"),
    ("device_id", "所属设备"),
    ("dev_id", "所属设备"),
    ("product_id", "所属产品"),
    ("package_id", "所属套餐"),
    ("quota_id", "所属额度"),
    ("license_id", "所属授权"),
    ("subscription_id", "所属订阅"),
    ("parent_id", "上级"),
];

/// 外键属性的显示名 —— **从关系推导**（REQ-008 · 任务 1）。
///
/// # 为什么不能从列注释拿
///
/// 真实源库里同一个 `customer_id`，37 张表的注释各写各的：
/// `客户标识`（10 处）/ `客户ID` / `用户id` / `账号ID（余额所属账号）` / `子账号ID` ……
/// 判读人打开「客户」这一屏，会看到**同一个东西 4 种叫法**，
/// 其中一种还跟**对象名**重名（「客户」既是对象、又是它的属性）。
/// 根因是外键属性名走了「注释优先」这条路 —— 而**外键是关系，不是自由文本**。
///
/// # 判据
///
/// 显示名只由「谁指向谁」决定：
/// 1. 词表里**人工确认过**的外键名优先（`customer_id → 所属客户`）；
/// 2. 否则 `所属 + 目标对象的人话名`（`admin_user_id → 所属后台管理员`）。
///
/// **拿不到目标的中文名就返回 `None`** —— 由调用方回落注释路径，
/// 绝不拼出「所属 customers」这种半吊子（S-2）。
pub fn fk_property_name(col: &str, target_cn: &str) -> Option<String> {
    let low = col.trim().to_ascii_lowercase();
    for (k, v) in COLUMN_LABELS {
        if low == *k {
            return Some((*v).to_string());
        }
    }
    let target = target_cn.trim();
    if target.is_empty() || !target.chars().all(is_han) {
        return None;
    }
    Some(format!("所属{target}"))
}

/// 通用业务词表 —— 供「剥掉 `_id`/`_no`/`_num`/`_code`/`_key` 技术后缀后」按词根查表。
///
/// 存在的意义：`order_no` 这类列不在 `COLUMN_LABELS` 里，但剥掉 `_no`
/// 得到词根 `order`，查本表能翻成「订单编号」。**表里没有的词根一律
/// 返回 `None`**，绝不把原始列名透给客户（见 S-2）。
const BUSINESS_WORDS: &[(&str, &str)] = &[
    // 通用名
    ("name", "名称"),
    ("title", "标题"),
    ("type", "类型"),
    ("kind", "类别"),
    ("status", "状态"),
    ("state", "状态"),
    ("remark", "备注"),
    ("note", "备注"),
    ("description", "描述"),
    ("tag", "标签"),
    ("level", "等级"),
    ("version", "版本"),
    ("source", "来源"),
    ("target", "目标"),
    // 业务名词
    ("user", "用户"),
    ("customer", "客户"),
    ("supplier", "供应商"),
    ("device", "设备"),
    ("product", "产品"),
    ("package", "套餐"),
    ("order", "订单"),
    ("quota", "额度"),
    ("license", "授权"),
    ("subscription", "订阅"),
    // 常见复合列（整词命中，无技术后缀）
    ("create_time", "创建时间"),
    ("update_time", "更新时间"),
    ("create_by", "创建人"),
    ("update_by", "更新人"),
    ("deleted_at", "删除时间"),
    // 数值/金额
    ("amount", "金额"),
    ("price", "价格"),
    ("total", "总额"),
    ("count", "数量"),
    ("num", "数量"),
    // 联系方式
    ("phone", "电话"),
    ("mobile", "手机"),
    ("email", "邮箱"),
    ("address", "地址"),
    // ── REQ-003 增补（AC-104）────────────────────────────────
    // 动机：拿真实源库 `<业务库名>`（91 表 / 1051 列）实测，
    // 原词表**只覆盖 18%** 的列，81% 被丢弃。丢弃量最大的恰恰是
    // 最标准、最该被翻译的审计字段 —— 因为**形态对不上**：
    //   词表写 `create_time` ↔ 库里是 `created_at`  （×83）
    //   词表写 `create_by`   ↔ 库里是 `created_by`  （×52，少一个 d）
    //   词表写 `deleted_at`  ↔ 库里是 `is_deleted`  （布尔软删标志，×4）
    // 词表里根本没有 `id` —— 而 `id` 是全库最常见的列（×88）。
    // 词表是照中文 demo fixture 手工编的，真实库的命名约定跟它不一样。
    //
    // 补的是**形态**（ed/过去式、is_ 前缀、id 单独），不是无脑加词 ——
    // 宁可漏，也绝不能把技术标识符当业务名透给客户（见 S-2）。
    ("id", "编号"),
    ("created_at", "创建时间"),
    ("updated_at", "更新时间"),
    ("created_by", "创建人"),
    ("updated_by", "更新人"),
    ("is_deleted", "是否删除"),
    ("is_enabled", "是否启用"),
    ("enabled", "是否启用"),
    ("disabled", "是否禁用"),
    ("remark", "备注"),
    // 时间类：真实库用 `*_at` 结尾，词表原来只有 `create_time` 一种形态
    ("begin_at", "开始时间"),
    ("end_at", "结束时间"),
    ("expires_at", "过期时间"),
    ("expired_at", "过期时间"),
    ("deleted_at", "删除时间"),
    ("login_at", "登录时间"),
    ("register_at", "注册时间"),
    ("paid_at", "支付时间"),
    ("sort_order", "排序"),
    ("ordinal", "排序"),
    ("priority", "优先级"),
    ("currency", "币种"),
    ("balance", "余额"),
    ("amount", "金额"),
    ("times", "次数"),
    ("count", "数量"),
    ("rate", "费率"),
    ("rate_limit", "限流"),
    ("interest", "利息"),
    ("rebate", "返利"),
    ("deduction", "扣款"),
    ("transaction", "交易"),
    ("channel", "渠道"),
    ("provider", "服务商"),
    ("permission", "权限"),
    ("role", "角色"),
    ("admin", "管理员"),
    ("config", "配置"),
    ("template", "模板"),
    ("log", "日志"),
    ("record", "记录"),
    ("cache", "缓存"),
    // REQ-005 · 真实源库 `instance_manager.session_id` 的注释是「SessionId」（纯英文，
    // 被质量门槛挡下回退词表）—— 但词表里**没有** `session`，于是回退返回 None，
    // 属性被 S-2 丢弃、客户视图少一项。
    //
    // 这里加词是**词表真实缺词**，不是「形态能解决却用词表打补丁」——
    // `session_id → 会话` 是语义映射，没有形态规则能推出来。
    ("session", "会话"),
    ("policy", "策略"),
    ("policies", "策略"),
    ("model", "模型"),
    ("key", "密钥"),
    ("token", "令牌"),
    ("secret", "密钥"),
    ("app", "应用"),
    ("aisearch", "AI 搜索"),
    // ── REQ-006 · 真实源库「词根真缺」的高频业务词 ──────────
    //
    // **这一段是词表层（开放类），不是形态层。** 上面的 [`FORM_SUFFIXES`]
    // 解决的是「形态对不上」，这里解决的是「**词根本来就没有**」——
    // `expire` / `retry` / `billing` 不是构词后缀，是实义词。
    // **判据**：`resolve_stem` 剥完后缀查不到词根时丢的是什么，就补什么，
    // 且**只补真实源库里高频出现、且翻译无歧义的**。
    //
    // 刻意**不补**的一类（补了也看不懂，等于把技术串换层皮）：
    // `payload` / `headers` / `idempotency_key` / `kafka_offset` /
    // `aggregate_key` / `llm_metadata` / `hash` / `projection`。
    // 宁可继续丢弃，也不让客户视图里出现「幂等密钥 标识」这种假人话。
    //
    // 生命周期 / 状态动作
    ("expire", "过期"),
    ("expired", "过期"),
    ("effective", "生效"),
    ("cancel", "取消"),
    ("cancelled", "取消"),
    ("retry", "重试"),
    ("lock", "锁定"),
    ("locked", "锁定"),
    ("unlock", "解锁"),
    ("clear", "清除"),
    ("cleared", "清除"),
    ("publish", "发布"),
    ("trigger", "触发"),
    ("start", "开始"),
    ("finish", "完成"),
    ("end", "结束"),
    ("read", "已读"),
    ("visible", "可见"),
    ("enable", "启用"),
    ("disable", "禁用"),
    ("recommend", "推荐"),
    ("recommended", "推荐"),
    ("hit", "命中"),
    ("family", "系列"),
    ("allow", "允许"),
    ("forbid", "禁止"),
    ("limit", "限制"),
    ("threshold", "阈值"),
    ("auto", "自动"),
    ("manual", "手动"),
    ("default", "默认"),
    ("current", "当前"),
    ("last", "最近"),
    ("next", "下一个"),
    ("previous", "上一个"),
    ("total", "总计"),
    ("used", "已用"),
    ("available", "可用"),
    ("remain", "剩余"),
    ("whitelist", "白名单"),
    ("blacklist", "黑名单"),
    // 计费 / 商务
    ("billing", "计费"),
    ("cycle", "周期"),
    ("renew", "续费"),
    ("reset", "重置"),
    ("upgrade", "降级"),
    ("refund", "退款"),
    ("discount", "折扣"),
    ("paid", "已付"),
    ("price", "价格"),
    ("currency", "币种"),
    ("rmb", "人民币"),
    ("usd", "美元"),
    ("balance", "余额"),
    ("cost", "成本"),
    ("income", "收入"),
    ("consume", "消耗"),
    ("point", "积分"),
    ("score", "评分"),
    ("rank", "排名"),
    ("tier", "档次"),
    // 用量 / 配额
    ("usage", "用量"),
    ("quota", "额度"),
    ("quota_used", "已用额度"),
    ("llm", "大模型"),
    ("tokenhub", "令牌中心"),
    ("rpm", "每分钟请求数"),
    ("tpm", "每分钟令牌数"),
    ("speed", "速度"),
    ("concurrency", "并发数"),
    // 业务对象
    ("region", "地区"),
    ("country", "国家"),
    ("province", "省份"),
    ("city", "城市"),
    ("industry", "行业"),
    ("role", "角色"),
    ("operator", "操作员"),
    ("customer_service", "客服"),
    ("account", "账户"),
    ("organization", "组织"),
    ("team", "团队"),
    ("group", "分组"),
    ("category", "分类"),
    ("scene", "场景"),
    ("channel", "渠道"),
    ("notice", "公告"),
    ("home", "首页"),
    ("advisor", "顾问"),
    ("image", "图片"),
    ("icon", "图标"),
    ("file", "文件"),
    ("filename", "文件名"),
    ("ip", "IP 地址"),
    ("agent", "代理"),
    ("service", "服务"),
    ("instance", "实例"),
    ("project", "项目"),
    ("task", "任务"),
    ("job", "作业"),
    ("queue", "队列"),
    ("event", "事件"),
    ("subject", "主题对象"),
    // ⚠️ **刻意不收 `snapshot`**：「快照」是客户视图页面明令禁止的建模术语
    // （判读手册 §材料清单）。把它当业务词翻出来，会让 `quota_snapshot`
    // 拼成「额度快照」直接漏到页面上。建模术语永远不进业务词表。
    ("progress", "进度"),
    ("result", "结果"),
    ("reason", "原因"),
    ("error", "错误"),
    ("content", "内容"),
    ("extra", "附加信息"),
    ("remark", "备注"),
    ("timezone", "时区"),
    ("weekday", "工作日"),
    ("hour", "小时"),
    ("minute", "分钟"),
    ("second", "秒"),
    ("day", "天"),
    ("month", "月"),
    ("year", "年"),
    ("week", "周"),
];

/// 技术后缀 → 中文尾词。剥掉后缀后按词根查 `BUSINESS_WORDS`。
const TECH_SUFFIXES: &[(&str, &str)] = &[
    ("_id", ""),
    ("_no", "编号"),
    ("_num", "数量"),
    ("_code", "编码"),
    ("_key", "标识"),
];

/// **形态后缀** → 中文尾词（REQ-006 · 任务 B）
///
/// # 为什么是这张表，而不是「再加 147 个词条」
///
/// 独立审核实测：真实源库 964 个属性里 **172 个（17.8%）**因词表认不出被**静默丢弃**。
/// 缺口不是「词不够多」，是**形态处理器不存在**：
///
/// | 形态 | 例子 | 处理 |
/// | --- | --- | --- |
/// | `*_at` 时间 | `expire_at` / `locked_at` | 剥 `_at` → `expire` 查词表 |
/// | `*_count` 数量 | `retry_count` / `hit_count` | 剥 `_count` → 查词表 |
/// | `*_by` 操作者 | `locked_by` / `cleared_by` | 剥 `_by` → 查词表 |
/// | `*_rate` 比率 | `discount_rate` | 剥 `_rate` → 查词表 |
/// | `*_cycle` 周期 | `billing_cycle` / `reset_cycle` | 剥 `_cycle` → 查词表 |
///
/// # 这张表**不是**「换个地方堆词」
///
/// 这是本项目第四次吃同一条纪律的亏，判据必须说清楚：
///
/// | 层次 | 性质 | 能否穷举 | 本文件的做法 |
/// | --- | --- | --- | --- |
/// | 业务**词汇**（`retry` / `billing` / `quota`） | **开放类** | 不能 —— 下个客户库还有新词 | [`BUSINESS_WORDS`] 枚举 |
/// | 英文**构词后缀**（`-at` / `-count` / `-by`） | **封闭类** | 能 —— 语言的构词法是有限集 | 本表枚举，**且只收语法层** |
///
/// 所以**判据是规则不是词**：「剥掉已知形态后缀，剩下的词根查词表；词根还是复合的，
/// 就按词元逐个查，每个词元都认识才拼」。加了 `hit` 一个词根，
/// `hit_count` / `last_hit_at` / `daily_hit_count` 全部自动认得 ——
/// **这是规则会自己长出来的结果，不是逐个补的**。
///
/// ⚠️ 反过来做（把 `_expire` / `_retry` / `_billing` 这类词根也列进表里）
/// 就是把词表换个地方堆：下个客户库换个词根又漏。
/// **凡是能用「剥后缀 + 查词根」解决的，一律不许进这张表。**
const FORM_SUFFIXES: &[(&str, &str)] = &[
    // 时间类
    ("_at", "时间"),
    ("_time", "时间"),
    ("_date", "日期"),
    ("_until", "截止时间"),
    // 计量类
    ("_count", "数量"),
    ("_times", "次数"),
    ("_num", "数量"),
    ("_amount", "金额"),
    ("_total", "合计"),
    ("_rate", "比率"),
    ("_ratio", "比例"),
    ("_percent", "百分比"),
    ("_unit", "单位"),
    // 周期类
    ("_cycle", "周期"),
    ("_period", "周期"),
    // 操作者类
    ("_by", "操作人"),
    // 状态类
    ("_status", "状态"),
    ("_type", "类型"),
    ("_name", "名称"),
    ("_version", "版本"),
    ("_size", "大小"),
];

/// **屈折还原**（REQ-008 · 任务 3a）—— 英文构词里**封闭**的一层规则，
/// 不是词表。
///
/// # 为什么不能靠补词
///
/// 这是本项目第五次吃「往词表里加词」的亏。真实源库 81 条丢弃里，
/// `prices` / `models` / `available_cycles` / `weekdays` / `tokenhub_notices`
/// 全都是**同一个 `customer_id` 之外的形态问题**：
/// 词表里已经有 `price → 价格`、`model → 模型`、`available → 可用`、
/// `cycle → 周期`、`weekday → 工作日`、`notice → 公告`，
/// 只是源库写的是**屈折形式**，查表查不到。
///
/// 补 `prices → 价格` 这类词条，等于把同一个意思再抄一遍 ——
/// 下一个客户库写 `price_list` / `model_count` / `available_quota`，
/// 又要再抄一遍。**判据必须是规则**。
///
/// # 规则（英文屈折的封闭类）
///
/// | 形态 | 例 | 还原 |
/// | --- | --- | --- |
/// | 名词复数 `-s` | `cycles → cycle` | 去掉 `s` |
/// | 名词复数 `-es` | `prices → price` | 去掉 `es` 后仍要试 `s` |
/// | 名词复数 `-ies` | `categories → category` | `ies → y` |
///
/// ⚠️ **不碰的三类**（去掉后缀会得到另一个词，反而是错的）：
/// - 以 `ss` 结尾（`address` —— 去掉 `s` 得到 `addres`，不是词）；
/// - 以 `us` / `is` 结尾（`status` / `analysis` —— 复数是 `statuses`，
///   去掉 `s` 得到的是错的形态）；
/// - 长度 ≤ 3（`gas` / `bus` / `is` —— 去掉就没有词根了）。
///
/// # 为什么 fail-closed 仍然成立
///
/// 还原出来的词根**照样要过 `lookup_word`**：不认识就返回 `None`。
/// 所以这条规则**只会把「本来认不出」变成「认得出」**，
/// 绝不会把「本来认不出」变成「拼出半吊子人话」——
/// `blob_ref_xyz` 里 `ref` 不认识，仍然整体丢弃（S-2 不变）。
///
/// **变异自证**：把 `inflect_variants` 改成返回空 vec ⇒
/// `tests/test_prop_name_quality.rs::test_inflection_is_morphology_not_wordlist`
/// 与 `tests/drop_rate.rs` 的丢弃率上限立刻变红。
fn inflect_variants(tok: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |s: &str| {
        if !s.is_empty() && !out.iter().any(|x| x == s) {
            out.push(s.to_string());
        }
    };
    // `-ies → -y`：`categories → category`
    if tok.len() > 4 && tok.ends_with("ies") {
        push(&tok[..tok.len() - 3]);
        push(&format!("{}y", &tok[..tok.len() - 3]));
    }
    // `-es → `` / `-s → ``：`prices → price`、`boxes → box`、`cycles → cycle`
    if tok.len() > 3 && tok.ends_with("es") {
        push(&tok[..tok.len() - 2]);
    }
    if tok.len() > 3 && tok.ends_with('s') && !tok.ends_with("ss") && !tok.ends_with("us") {
        push(&tok[..tok.len() - 1]);
    }
    out
}

/// 列名 → 「最后一个词元屈折还原后」的候选形态。
///
/// `available_cycles` → `available_cycle`（于是能命中 `_cycle` 形态后缀）。
/// 只动**最后一个**词元：复合词里前面的词元是限定语（`original_prices` 的
/// `original` 不是复数），还原它们没有依据。
fn inflected_column_forms(low: &str) -> Vec<String> {
    let Some(idx) = low.rfind('_') else { return Vec::new() };
    let (head, last) = low.split_at(idx + 1);
    inflect_variants(last).into_iter().map(|v| format!("{head}{v}")).collect()
}

/// 这个英文词元**是不是我们认识的列名用词**（`name` / `email` / `type` …）。
///
/// 用途有两处，都是「按形态判是不是字段名」而不是「按词表猜意思」：
/// - [`crate::extract::propose::is_enum_domain`]：一串原子**全都**是列名用词
///   时，它多半是**字段名列表**（`name,email`）而不是**枚举值域**（`Y/N`）；
/// - 外键属性名的「注释只是在复述这个外键」判定。
///
/// 判的是**键**（英文源词）不是值：值域里出现 `type` 这样的**值**很正常，
/// 但 `name,email` 这种**字段名列表**里的每个原子都是能当列名用的词。
pub fn is_known_column_word(tok: &str) -> bool {
    let t = tok.trim().to_ascii_lowercase();
    if t.is_empty() {
        return false;
    }
    COLUMN_LABELS.iter().any(|(k, _)| *k == t) || lookup_word(&t).is_some()
}

/// 这个中文串**是不是一个我们叫得出名字的对象/人**。
///
/// 判据：全是汉字，且命中**任一**已知实体显示名（对象名）或业务词表的中文值。
/// 用途：判断一段列注释是不是**只是在复述这个外键本身**
/// （`客户标识` / `用户id` → 说的就是「客户」，没有新信息），
/// 而 `子账号ID` / `账号ID（余额所属账号）` / `被代扣Open用户ID`
/// 都在**换主体或加限定**，必须保留 —— 那是关系表达不了的信息。
pub fn is_known_entity_name(s: &str, object_names: &BTreeSet<String>) -> bool {
    let v = s.trim();
    if v.is_empty() || !v.chars().all(is_han) {
        return false;
    }
    object_names.contains(v) || BUSINESS_WORDS.iter().any(|(_, val)| *val == v)
}

/// 词根 → 中文。查不到返回 `None`。
///
/// **REQ-008**：先按原形查，**再按屈折变体查**（[`inflect_variants`]）——
/// 规则层，不是新增词条。
fn lookup_word(w: &str) -> Option<&'static str> {
    if let Some(v) = BUSINESS_WORDS.iter().find(|(k, _)| *k == w).map(|(_, v)| *v) {
        return Some(v);
    }
    for v in inflect_variants(w) {
        if let Some(hit) = BUSINESS_WORDS.iter().find(|(k, _)| *k == v).map(|(_, v)| *v) {
            return Some(hit);
        }
    }
    None
}

/// **复合词根解析** —— REQ-006 形态处理器的第二级。
///
/// 剥掉形态后缀后词根可能还是复合的：`last_retry` / `next_hit` / `auto_renew`。
/// 规则（**不是查表**）：
///
/// 1. 先按**整词根**查 [`BUSINESS_WORDS`]（`create_time` / `sort_order` 这类）；
/// 2. 整词根查不到才按 `_` 切词元，**逐个查**；
/// 3. **每个词元都认识才拼** —— 任一不认识就整体返回 `None`。
///
/// 第 3 条的「全有或全无」是 **S-2 的守门**：宁可整个属性丢弃，
/// 也不拼出「客户未知 额度 策略」这种半吊子人话。
/// 也正是它让 `blob_ref_xyz` / `zzz_unmapped_token` 继续被拒 ——
/// 前者 `ref` 不认识，后者 `zzz` 不认识。
fn resolve_stem(stem: &str) -> Option<String> {
    if stem.is_empty() {
        return None;
    }
    if let Some(w) = lookup_word(stem) {
        return Some(w.to_string());
    }
    let toks: Vec<&str> = stem.split('_').filter(|t| !t.is_empty()).collect();
    if toks.len() < 2 {
        return None;
    }
    let mut parts: Vec<&str> = Vec::with_capacity(toks.len());
    for t in toks {
        // `?` = 任一词元不认识就整体放弃
        parts.push(lookup_word(t)?);
    }
    Some(parts.concat())
}


/// 列名 → 人话。**查不到返回 `None`，绝不原样透出原始列名。**
///
/// 规则：
/// 1. 精确命中 `COLUMN_LABELS`（外键等已知列）；
/// 2. 剥掉技术后缀，按词根查 `BUSINESS_WORDS`（`order_no` → 订单编号）；
/// 3. 无技术后缀时整词查 `BUSINESS_WORDS`（`status` → 状态）；
/// 4. **REQ-006**：剥掉**形态后缀**（[`FORM_SUFFIXES`]）后按词根解析 ——
///    `expire_at` → `expire`+时间 → 过期时间；`retry_count` → 重试数量；
/// 5. 认不出来 → `None`。
///
/// **S-2 修法**：早期版本第 4 步是 `col.to_string()` —— 把 `blob_ref_xyz`
/// 这类表外列原样塞进客户视图，等于把技术标识符当业务名给人看。
/// 现在「认不出」是显式的失败，由调用方决定丢弃。
///
/// **REQ-006 提升为 `pub`**：丢弃率探针（`tests/drop_rate.rs`）要**逐属性**
/// 报出「哪些列没进客户视图」—— 那正是判读人问「这对象属性怎么这么少」
/// 时的答案。只报总数不够定位。只读纯函数，不引入 I/O。
pub fn humanize_column(col: &str) -> Option<String> {
    let low = col.to_ascii_lowercase();

    // 1) 精确命中（外键等已知列）
    for (k, v) in COLUMN_LABELS {
        if low == *k {
            return Some((*v).to_string());
        }
    }

    // 2) 剥技术后缀 → 词根查业务词表
    for (suffix, tail) in TECH_SUFFIXES {
        if let Some(stem) = low.strip_suffix(suffix) {
            if let Some(w) = lookup_word(stem) {
                return Some(format!("{w}{tail}"));
            }
            // 词根是复合的（`related_customer` / `peak_config`）时，
            // 与形态后缀走**同一套**复合解析，别让 `_id` 成为绕过形态处理器的
            // 暗门 —— 那是「两条路各修一半」的老毛病。
            if let Some(w) = resolve_stem(stem) {
                return Some(format!("{w}{tail}"));
            }
            // 剥了后缀但词根不认识 → 同样认不出，返回 None
            return None;
        }
    }

    // 3) 无技术后缀 → 整词查业务词表
    if let Some(w) = lookup_word(&low) {
        return Some(w.to_string());
    }

    // 4) **REQ-006 形态处理器**：剥形态后缀 → 解析词根。
    //    走不到这里的都是「整词和技术后缀都不认得的复合形态」——
    //    正是 `*_at` / `*_count` / `*_by` / `*_rate` / `*_cycle` 那一批。
    //    **注意这一步仍然可能返回 None** —— 词根不认识就是不认识（S-2）。
    //
    //    **REQ-008**：先按**原形**试后缀；原形一个都不匹配时，
    //    再把**最后一个词元屈折还原**后试一次 ——
    //    `available_cycles` → `available_cycle` → 命中 `_cycle` → 可用周期。
    //    这是「剥后缀」这条规则自己长出来的能力，不是新加的词条。
    for (suffix, tail) in FORM_SUFFIXES {
        if let Some(stem) = low.strip_suffix(suffix) {
            // 词根认不出来时**不继续试其他后缀**：`retry_count` 剥掉 `_count`
            // 认不出，就该丢弃；再拿它去撞 `_unt` 之类只会拼出怪东西。
            return resolve_stem(stem).map(|w| format!("{w}{tail}"));
        }
    }
    for inflected in inflected_column_forms(&low) {
        for (suffix, tail) in FORM_SUFFIXES {
            if let Some(stem) = inflected.strip_suffix(suffix) {
                if let Some(w) = resolve_stem(stem) {
                    return Some(format!("{w}{tail}"));
                }
            }
        }
    }

    // 5) **布尔前缀形态**（REQ-006）：`is_` 开头是「是/否」的构词形态，
    //    不是一个词。`is_read` / `is_visible` / `is_recommended` 剥掉 `is_`
    //    后按词根翻译再冠「是否」。`is_deleted` / `is_enabled` 早已整词收录，
    //    在第 3 步就返回了，走不到这里 —— 两条路结果一致，不冲突。
    if let Some(stem) = low.strip_prefix("is_") {
        return resolve_stem(stem).map(|w| format!("是否{w}"));
    }

    // 6) **无后缀的复合词**（REQ-006）：`auto_renew` / `package_family` /
    //    `customer_quota_policies` 这一类既没有技术后缀也没有形态后缀。
    //    形态处理器不能只认「剥后缀」——**复合本身就是一种形态**。
    //    `resolve_stem` 的「全词元都认识才拼」保证这里仍然是 fail-closed：
    //    `blob_ref_xyz` / `zzz_unmapped_token` / `aisearch_times_internal_v2`
    //    都因为**至少一个词元不认识**而继续被拒（S-2 回归点，见测试）。
    //    走到这里还认不出 → **None，绝不原样透出**（S-2）。
    resolve_stem(&low)
}

/// 把一个值按非字母数字/下划线切成词元（统一小写）。
fn tokenize(s: &str) -> Vec<String> {
    s.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|t| !t.is_empty())
        .map(|t| t.to_ascii_lowercase())
        .collect()
}

/// **S-1 防御**：这个值能不能当人话给人看。
///
/// 判为不可用的三种情形：
/// - 空白；
/// - 任一词元恰好等于某个实体 key（如 `display_name` 退化成了 `robot_device`）；
/// - 整个值（小写后）恰好等于某个 key。
///
/// 用「**词元相等**」而非「子串包含」，避免 key=`id` 时把
/// `customer` 之类的正常中文/英文词误伤 —— 客户视图宁可漏也不误杀。
///
/// 【REQ-010 只放宽可见性，逻辑一个字没动】总览里的**概念名**要过同一把尺
/// （概念显示名退化成了实体 key 时同样不能给客户看）。抄一份判据就会漂，
/// 而「客户视图里叫得出、总览里叫不出」是判读人当场能发现的矛盾。
pub fn usable_as_human_name(value: &str, keys: &BTreeSet<String>) -> bool {
    let v = value.trim();
    if v.is_empty() {
        return false;
    }
    if keys.contains(&v.to_ascii_lowercase()) {
        return false;
    }
    !tokenize(v).iter().any(|t| keys.contains(t))
}

/// 客户视图里的一个对象
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntityView {
    pub name: String,
    pub note: Option<String>,
    /// 上层对象的**名字**（不是 key）—— 客户不认识 `robot_device`
    pub belongs_to: Option<String>,
    pub aliases: Vec<String>,
    pub properties: Vec<PropertyView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PropertyView {
    pub name: String,
    pub note: Option<String>,
    pub required: bool,
    /// 样例值 —— 脱敏后的，客户靠它判断「这是不是我要的东西」
    pub sample: Option<String>,
}

/// 客户视图里的一个关系
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RelationView {
    pub from: String,
    pub to: String,
    /// 基数，转成人话
    pub how_many: String,
    /// 语义，转成人话
    pub meaning: String,
}

/// 整份客户视图
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomerView {
    pub objects: Vec<EntityView>,
    pub relations: Vec<RelationView>,
}

/// 语义 → 人话
///
/// `_ =>` 分支**刻意保留原样**（不静默改写成别的含义），理由与
/// [`cardinality_cn`] 的兜底完全同构：认不出的语义原样进入客户视图，
/// 然后被**第四档**守门（[`untranslated_semantic_hit`]，按**形态**判定
/// 「这是不是内部标识符而不是中文人话」）当场抓住 → `cli.rs` 拒绝发布
/// 整份视图。也就是「漏翻 = 拒绝出图」，而不是「漏翻 = 悄悄给客户看
/// `belongs_to`」。
///
/// B1 修之前这里是真盲区：复审实测把真实产物 51 条 `meaning` 全改回
/// `belongs_to`（= `_ =>` 兜底的形态）仍 exit 0 —— `belongs_to` 不被
/// 任何守门词覆盖。
fn semantic_cn(s: &str) -> String {
    match s {
        "has" => "拥有".to_string(),
        "belongs_to" => "归属于".to_string(),
        "depends_on" => "依赖".to_string(),
        "references" => "关联到".to_string(),
        "records" => "记录了".to_string(),
        "derived_from" => "算自".to_string(),
        // ── ⭐ REQ-156 · 真实语料里另外 9 种动词（15 种中的其余部分）──
        // 同义归一：`binds` / `binds_to` 是同一件事，给**同一个**标签。
        "binds_to" | "binds" => "绑定到".to_string(),
        // 同义归一：`attaches_to` / `attached_to` 是同一件事，给**同一个**标签。
        "attached_to" | "attaches_to" => "挂载于".to_string(),
        "notifies_via" => "通知到".to_string(),
        "applies_to" => "适用于".to_string(),
        "targets" => "指向".to_string(),
        "bills" => "计费到".to_string(),
        "based_on" => "基于".to_string(),
        "deployed_to" => "部署于".to_string(),
        // ⚠️ 泛词（语义太宽，几乎可以套在任何关系上）—— **REQ-157 会清掉**。
        // 现在仍要给中文：否则 `semantic_label` 会把英文 key 漏给客户/Studio。
        "uses" => "使用".to_string(),
        // ⚠️ 泛词（REQ-157 会清掉）。
        "contains" => "包含".to_string(),
        // ⚠️ 泛词（REQ-157 会清掉）。
        "associated_with" => "关联".to_string(),
        // ⛔ 兜底**刻意不动**：`untranslated_semantic_hit` 靠「原样返回英文」来判定漏翻。
        _ => s.to_string(),
    }
}

/// 关系语义 → **可选的中文标签**（REQ-156）。
///
/// 与 [`semantic_cn`] 的区别只有一个：认不出的动词返回 `None`，而不是把英文 key
/// 原样当成「标签」递出去（catalog 的 `semantic_label` 用这个）。
///
/// 判据与 [`untranslated_semantic_hit`] **同一条**：标签里出现任何 ASCII 字母
/// ⇒ 它就不是中文标签。空语义（映射侧 `MappedJoin` 没有语义）同样返回 `None`。
/// ⛔ 这里**不编**任何兜底中文 —— 认不出就是认不出。
pub fn semantic_label(s: &str) -> Option<String> {
    if s.trim().is_empty() {
        return None;
    }
    let label = semantic_cn(s);
    if label.chars().any(|c| c.is_ascii_alphabetic()) {
        None
    } else {
        Some(label)
    }
}

/// 基数 → 人话。
///
/// **REQ-003 补 `N:1`（B1）**：真实 E2E 产出的 51 条关系 `how_many`
/// **无一例外**是 `N:1`（`活动关键词 → 活动` 这类 `*_id` 外键），
/// 而原表只认 `1:1`/`1:N`/`N:M` —— `N:1` 落进 `_ =>` 分支，
/// **把内部记号原样透给了客户**（AC-005/AC-106 破防）。
///
/// `_ =>` 分支**刻意保留原样**（不静默改写成别的含义）：认不出的记号
/// 原样进入客户视图，然后被第三档守门词（[`cardinality_notation_hit`]，
/// 按**形态**而非词表判定）当场抓住 → `cli.rs` 拒绝发布整份视图。
/// 也就是说「漏翻 = 拒绝出图」，而不是「漏翻 = 悄悄给客户看错的东西」。
/// 这条不变量由 `tests/test_view.rs::test_view_no_cardinality_notation_passes_silently` 守着。
fn cardinality_cn(c: &str) -> String {
    match c {
        "1:1" => "唯一对应".to_string(),
        "1:N" => "一对多".to_string(),
        "N:M" => "多对多".to_string(),
        // 真实库的主力形态：多个从属行指向同一个父行（`活动关键词 → 活动`）
        "N:1" => "多对一".to_string(),
        _ => c.to_string(),
    }
}

/// 把内部快照投影成客户视图。
///
/// **刻意丢弃的东西**（每一个都是「建模术语」或「内部实现」）：
/// - `key` —— 系统标识，客户不认 `robot_device`，只认「设备」
/// - `evidence` —— 审计用，不是给人看的
/// - `ordinal` —— 显示顺序
/// - `extracted_by` / `proposed_by` / `approved_by` —— 治理元数据
/// - `action_types` / `functions` —— 切片 1 的占位，客户视图里**根本不该有这个字段**
/// - `disjoint_with` —— 审核提示，不是客户要理解的东西
pub fn project(snapshot: &SnapshotView) -> CustomerView {
    // 全部实体 key 的集合 —— S-1 防御的判据来源
    let keys: BTreeSet<String> =
        snapshot.entity_types.iter().map(|e| e.key.to_ascii_lowercase()).collect();

    // 只有「能当人话用」的 display_name 才允许进映射表。
    // 退化成了 key 的对象（抽取侧漏翻人话）直接不发布：
    // 关系/父对象一旦指到它就查不到 → 被丢弃，不会泄露标识符。
    let by_key: BTreeMap<&str, &str> = snapshot
        .entity_types
        .iter()
        .filter(|e| usable_as_human_name(&e.display_name, &keys))
        .map(|e| (e.key.as_str(), e.display_name.as_str()))
        .collect();

    let objects = snapshot
        .entity_types
        .iter()
        .filter(|e| usable_as_human_name(&e.display_name, &keys))
        .map(|e| EntityView {
            name: e.display_name.clone(),
            note: e.description.clone(),
            belongs_to: e.parent.as_ref().and_then(|p| by_key.get(p.as_str()).map(|s| s.to_string())),
            aliases: e.aliases.clone(),
            properties: e
                .properties
                .iter()
                .filter_map(|p| {
                    // 抽取侧给的是中文属性名；漏给时用列名人话化兜底。
                    // **兜底也认不出（None）就丢弃这个属性** —— 宁可少一个属性，
                    // 也不能把 `blob_ref_xyz` 这种技术串当成业务名展示。
                    let name = if p.name.trim().is_empty() {
                        humanize_column(&p.column)?
                    } else {
                        p.name.clone()
                    };
                    // S-1：属性名同样不许是 key
                    if !usable_as_human_name(&name, &keys) {
                        return None;
                    }
                    Some(PropertyView {
                        name,
                        note: p.note.clone(),
                        required: !p.nullable,
                        sample: p.sample.clone(),
                    })
                })
                .collect(),
        })
        .collect();

    let relations = snapshot
        .link_types
        .iter()
        .filter_map(|l| {
            let from = by_key.get(l.from_type.as_str())?;
            let to = by_key.get(l.to_type.as_str())?;
            Some(RelationView {
                from: (*from).to_string(),
                to: (*to).to_string(),
                how_many: cardinality_cn(&l.cardinality),
                meaning: semantic_cn(&l.semantic),
            })
        })
        .collect();

    CustomerView { objects, relations }
}

// ──────────────────────────────────────────────────────────
// 投影所需的输入形状
//
// 只声明投影真正用到的字段 —— 不是 Snapshot 的镜像，
// 避免「契约一改投影跟着改」。
// ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SnapshotView {
    pub entity_types: Vec<EntityTypeView>,
    pub link_types: Vec<LinkTypeView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntityTypeView {
    pub key: String,
    pub display_name: String,
    pub description: Option<String>,
    pub aliases: Vec<String>,
    pub parent: Option<String>,
    pub disjoint_with: Vec<String>,
    pub properties: Vec<PropertyTypeView>,
    pub evidence: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PropertyTypeView {
    pub name: String,
    pub column: String,
    pub nullable: bool,
    pub note: Option<String>,
    pub sample: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinkTypeView {
    pub key: String,
    pub from_type: String,
    pub to_type: String,
    pub cardinality: String,
    pub semantic: String,
}

/// **建模术语黑名单** —— 验收 AC-005 的守门词。
///
/// 出现在客户视图里的任何一处都算失败（连注释/键名也算）。
///
/// 拆分成两档：中文用包含匹配，英文用词元/整体匹配。
/// 基数记号走**第三档 —— 按形态判定**，见 [`cardinality_notation_hit`]。
///
/// 原来是一律子串匹配（`json.contains(t)`），实测在真实源库上会**误杀**：
/// `<业务库名>` 里有 `license_key`、`public_key`、`config_key`、
/// `aggregate_key`、`idempotency_key`、`keywords` —— `license_key`（授权密钥）
/// 是正常业务概念，却因为含子串 `key` 被判成建模术语泄露。
/// 一旦抽取侧把它翻成「License Key」当属性名，`cli.rs` 的运行时守门会
/// `exit 2` 关闭**整个**客户视图（AC-105 的拆分动机）。
///
/// 拆分理由：
/// - **英文守门词是「标识符」** —— 泄露形态是「有个东西就叫 key」。
///   所以要判「**整体就是**这个词（或这个词组）」，而不是「包含」。
///   `Key` / `CARDINALITY` / `primary_key` 照样被抓（S-3 的大小写绕过也在）；
///   `License Key`（两个词、整体不等于 key）不再误伤。
/// - **中文守门词是「术语」** —— 中文不分词，`基数` 出现在句中也该被抓，
///   所以保持**包含匹配**。
/// - **基数记号是「形态」** —— `1:1`/`1:N`/`N:1`/`N:M`/`1..*` 都是
///   「单字符 分隔符 单字符」，**判定它的不是词表而是形状**。
///   词表只能覆盖写下来的那几种拼法，形态判定才能覆盖「将来新增的记号」。
pub const MODELING_TERMS: &[&str] = &[
    "pk", "primary_key", "cardinality", "ordinal", "evidence", "disjoint",
    "extracted_by", "proposed_by", "approved_by", "key",
    "外键", "基数", "主键", "证据",
];

/// 中文守门词 —— 用**包含**匹配。
const MODELING_TERMS_CJK: &[&str] = &["外键", "基数", "主键", "证据"];

/// 基数记号的**形态**守门 —— 第三档，不靠词表，靠形状。
///
/// 记号形态 = **单字符** +（`:` 或 `..`）+ **单字符**，两侧取自 `[0-9A-Za-z*]`。
/// 命中 `1:1` / `1:N` / `N:1` / `N:M` / `0:1` / `1..*` / `0..1`。
///
/// **为什么不写进 `MODELING_TERMS` 词表**（这是审核指出的结构性失明）：
/// - `normalize_term` 会把 `N:1` 压成 `n_1` —— 与真实标识符 `n_1` 无法区分；
///   而「整体归一化后等于 `n_1`」这条路又永远不成立（整体是整段 JSON）。
/// - 就算硬塞进词表，也只覆盖 `N:1` 这一种拼法，`1:M` / `M:N` / `1..*` 照样漏过去。
///   词表是**枚举**，形态是**判定** —— 只有后者能兑现「任何没被翻译的记号
///   都被当场抓住」。
///
/// **为什么不误杀时刻/日期**（属性样例里很常见）：两侧都必须是**单字符**且
/// 前后不能再接同类字符，所以 `09:30`、`12:30:00`、`2026-10-03T10:41:55Z`
/// 全部不合 —— 时刻是**两字符**的时分秒。`1:12` 也因右侧越界被排除。
/// 实测真实 137KB 客户视图里本档只命中 51 处，全是 `N:1`，无误杀。
fn cardinality_notation_hit(text: &str) -> Option<String> {
    let b: Vec<char> = text.chars().collect();
    let side = |c: char| c.is_ascii_alphanumeric() || c == '*';
    for i in 0..b.len() {
        if !side(b[i]) || (i > 0 && side(b[i - 1])) {
            continue; // 左端必须是独立的一个字符
        }
        let sep = if i + 1 < b.len() && b[i + 1] == ':' {
            1
        } else if i + 2 < b.len() && b[i + 1] == '.' && b[i + 2] == '.' {
            2
        } else {
            continue; // 分隔符只能是 `:` 或 `..`
        };
        let j = i + 1 + sep; // 跳过分隔符后的右端字符下标
        if j >= b.len() || !side(b[j]) || (j + 1 < b.len() && side(b[j + 1])) {
            continue; // 右端也必须是独立单字符
        }
        return Some(b[i..=j].iter().collect());
    }
    None
}

/// 是不是**汉字**。
///
/// 不直接用 `char::is_alphabetic()` —— 它把假名（`カナ`）、谚文（`한글`）
/// 一起放进来，而 AC-005 要的是**中文**人话。
///
/// **REQ-005 提升为 `pub`**：抽取器判「列注释能不能当客户可见的属性名」
/// （`extract::propose::classify_comment`）要的是同一个判据 ——
/// 客户视图的终局要求是中文，抽取入口就该用同一把尺子量，
/// 而不是让两处各写一份、日后各改各的。
pub fn is_han(c: char) -> bool {
    matches!(c as u32, 0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0xF900..=0xFAFF)
}

/// 关系语义字段（`meaning`）的**取值** —— 只认值位置，不认键名。
///
/// **为什么必须按字段取值，不能全文本扫**：客户视图里**有一个键就叫
/// `belongs_to`**（`EntityView::belongs_to`，见 [`EntityView`]）——
/// 全文本扫会把它当成语义泄漏，于是**每一份视图**都误杀。
/// 基数记号能全文本扫是因为它的形态（`单字符:单字符`）在键名位置凑巧不成立；
/// 语义标识符会。
///
/// 顺带把转义处理掉（`\"` / `\\`）：不然 `\"` 就能提前收尾、伪造一个假的
/// 键名位置骗过判据。
fn meaning_values(json: &str) -> Vec<String> {
    const KEY: &str = "\"meaning\"";
    let mut out = Vec::new();
    let mut pos = 0usize;
    while let Some(rel) = json[pos..].find(KEY) {
        // 键名之后只认 `:` + 字符串；否则这不是键，继续往后找
        let mut cur = &json[pos + rel + KEY.len()..];
        cur = cur.trim_start();
        if !cur.starts_with(':') {
            pos = json.len() - cur.len();
            continue;
        }
        cur = cur[1..].trim_start();
        if let Some(body) = cur.strip_prefix('"') {
            let mut val = String::new();
            let mut escaped = false;
            for ch in body.chars() {
                if escaped {
                    val.push(ch);
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    break;
                } else {
                    val.push(ch);
                }
            }
            out.push(val);
        }
        pos = json.len() - cur.len() + 1;
    }
    out
}

/// 关系语义漏翻的**形态**守门 —— 第四档，不靠词表，靠形状。
///
/// 形态 = **内部标识符**而不是中文人话：**一个汉字都没有，却含 ASCII 字母
/// 或下划线**。`belongs_to` / `depends_on` / `derived_from` / `has` 以及
/// 将来抽取器产出的任何新语义，全都落在这个形态里。
///
/// **为什么不写进 `MODELING_TERMS` 词表**（与 [`cardinality_notation_hit`]
/// 同一条理由）：词表只能覆盖写下来的那几种拼法，`aggregates` / `is_a_of` /
/// `consists_of` 照样原样透给客户。**词表是枚举，形态是判定。**
///
/// **误杀面**：这条判据**只看 `meaning` 字段的取值**，不看键名、不看
/// `note`/`sample` 自由文本。译文恒为中文（`拥有`/`归属于`/`依赖`…），
/// 所以真实产物上零误杀。
///
/// 修的是同一形状的病：`semantic_cn` 的 `_ => s.to_string()` 与 B1 修之前的
/// `cardinality_cn` 一模一样 —— 未识别的语义**原样透出**。今天真实产物的
/// `meaning` 全是「归属于」，零泄漏是**运气**，不是防线：抽取器换一个语义
/// 就原样漏出去，而复审实测 `belongs_to` **不被任何守门词覆盖**。
/// 第四档：未翻译的关系语义。
///
/// 判据是「`meaning` 的值里**出现任何 ASCII 字母**就抓」。
///
/// **为什么这么判**：`semantic_cn` 的所有分支输出都是中文（「归属于」「拥有」「多对多」…），
/// 唯一能产出英文的路径是它的兜底 `_ => c.to_string()` —— 那正是我们要抓的。
/// 所以「值里有英文字母」≈ 「走了兜底」，不需要枚举具体词形。
///
/// ⚠️ 曾经的判据是「**一个汉字都没有**却含 ASCII 词元」——
/// **错在只要有一个汉字就放行**：「拥有 belongs_to」里的「拥有」把它放了，
/// `belongs_to` 原样透给客户（收尾复审 V1）。
/// 而且那个方向的失败模式是 **fail-open（发错数据给客户）**，
/// 与前三轮挖到的 fail-closed（拒发）相反 —— 更危险，因为它**不会自己暴露**。
fn untranslated_semantic_hit(json: &str) -> Option<String> {
    meaning_values(json)
        .into_iter()
        .find(|v| v.chars().any(|c| c.is_ascii_alphabetic()))
        .map(|v| format!("语义值含未翻译的英文：{v}"))
}

/// 把一段文本归一化成「可整体比对的词」：小写 + 非字母数字/下划线一律转下划线 + 压空白。
///
/// `License Key` → `license_key`；`CARDINALITY` → `cardinality`；`  Key  ` → `key`。
fn normalize_term(s: &str) -> String {
    let lowered = s.to_lowercase();
    let mut out = String::with_capacity(lowered.len());
    let mut prev_sep = false;
    for ch in lowered.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            out.push(ch);
            prev_sep = false;
        } else if !prev_sep {
            out.push('_');
            prev_sep = true;
        }
    }
    out.trim_matches('_').to_string()
}

/// 英文守门词在文本里是否出现。
///
/// 判据：把文本按「非字母数字/下划线」切词元；若**任一词元**等于某个**单词**守门词则命中；
/// 若文本**整体归一化**后等于某个**词组**守门词（`primary_key` / `extracted_by`）也命中。
///
/// 这样 `License Key` 的词元是 `license` / `key`，其中 `key` 仍会命中…——
/// **所以单词守门词走的是「整体词元边界 + 排除已知业务前缀」**。
/// 见下方 `BUSINESS_ALLOWED_PREFIXES`。
const BUSINESS_ALLOWED_PREFIXES: &[&str] = &[
    "license", "public", "private", "config", "cache", "aggregate", "idempotency",
    "record", "business", "keyword", "api", "access", "secret", "sort", "partition",
];

fn english_term_hit(text: &str) -> Option<&'static str> {
    // 1) 词组整体命中（primary_key / extracted_by / proposed_by / approved_by）
    let whole = normalize_term(text);
    for t in MODELING_TERMS {
        if t.is_ascii() && whole == *t {
            return Some(t);
        }
    }
    // 2) 词元命中。单词守门词（pk/key/…）要求「该词元就是它自己」，
    //    且前一个词元不在业务白名单里 —— 否则 `license_key` 里的 `key`
    //    会被误判（`license_key` 归一化后整体也不是 `key`）。
    let toks: Vec<String> = text
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|t| !t.is_empty())
        .map(|t| t.to_lowercase())
        .collect();
    for (i, tok) in toks.iter().enumerate() {
        for t in MODELING_TERMS {
            if !t.is_ascii() {
                continue;
            }
            if tok == t {
                // 前词在业务白名单 → 这是业务概念不是建模术语
                if i > 0 && BUSINESS_ALLOWED_PREFIXES.contains(&toks[i - 1].as_str()) {
                    continue;
                }
                return Some(t);
            }
        }
    }
    None
}

/// 扫描序列化结果，列出泄露的建模术语。空 = 通过。
///
/// **S-3 修法**：早期版本用 `json.contains(*t)` 大小写敏感地匹配 ——
/// 视图里出现 `"CARDINALITY"` 或 `"Key"` 就绕过了守门词，
/// 守门器形同虚设。改为**两侧都小写后再匹配**。
/// 返回的是黑名单里的原始词（便于报告指出违规的是哪个词）。
///
/// **REQ-003 改法（AC-105）**：中文词包含匹配、英文词按词元/整体匹配；
/// 基数记号按**形态**匹配，见 [`cardinality_notation_hit`]。
pub fn scan_modeling_terms(json: &str) -> Vec<String> {
    let mut hits: Vec<String> = MODELING_TERMS_CJK
        .iter()
        .filter(|t| json.contains(**t))
        .map(|s| s.to_string())
        .collect();
    if let Some(t) = english_term_hit(json) {
        if !hits.iter().any(|h| h == t) {
            hits.push(t.to_string());
        }
    }
    // 第三档：任何没被翻译的基数记号都当场报出。
    // 有了这一档，`cardinality_cn` 的 `_ =>` 兜底才是安全的 ——
    // 漏翻的记号不会悄悄进客户视图，而是让整份视图拒绝发布。
    if let Some(tok) = cardinality_notation_hit(json) {
        let label = format!("未翻译的基数记号 {tok}");
        if !hits.iter().any(|h| *h == label) {
            hits.push(label);
        }
    }
    // 第四档：任何没被翻译的关系语义（`belongs_to` 这类内部标识符）当场报出。
    // `semantic_cn` 的 `_ =>` 兜底与 B1 修之前的 `cardinality_cn` 同型，
    // 这里给它补上同型的防线 —— 否则「今天全是『归属于』」只是运气。
    if let Some(v) = untranslated_semantic_hit(json) {
        let label = format!("未翻译的关系语义 {v}");
        if !hits.iter().any(|h| *h == label) {
            hits.push(label);
        }
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_humanize_column_known() {
        assert_eq!(humanize_column("customer_id").as_deref(), Some("所属客户"));
        assert_eq!(humanize_column("dev_id").as_deref(), Some("所属设备"));
    }

    #[test]
    fn test_humanize_column_via_business_word() {
        // 剥掉技术后缀后按词根查表
        assert_eq!(humanize_column("order_no").as_deref(), Some("订单编号"));
        assert_eq!(humanize_column("status").as_deref(), Some("状态"));
        assert_eq!(humanize_column("create_time").as_deref(), Some("创建时间"));
    }

    #[test]
    fn test_humanize_column_unknown_returns_none_not_raw() {
        // **S-2 反例**：表外的 *_id 不许原样透出，必须是 None
        assert_eq!(humanize_column("blob_ref_xyz_id"), None);
        assert_eq!(humanize_column("mystery"), None);
    }

    #[test]
    fn test_semantic_and_cardinality_cn() {
        assert_eq!(semantic_cn("has"), "拥有");
        assert_eq!(cardinality_cn("1:N"), "一对多");
    }

    // ── ⭐ REQ-156 · 15 种真实动词全部有人话，且标签里不许有 ASCII ──────
    //
    // 真实快照（cloud-resource v3）的 34 条关系一共 15 种 semantic：
    // belongs_to 16 / references 4 / binds_to 2 / attached_to 1 / attaches_to 1 /
    // notifies_via 1 / applies_to 1 / targets 1 / binds 1 / uses 1 / contains 1 /
    // associated_with 1 / bills 1 / based_on 1 / deployed_to 1。
    // 补课前的映射只有 6 个（其中 4 个在本套数据里根本没出现）⇒ 13 种 key 原样
    // 漏进客户视图 TTL 的 `rdfs:comment`（`语义：applies_to（多对多）`）。
    const REAL_VERBS: [(&str, &str); 15] = [
        ("belongs_to", "归属于"),
        ("references", "关联到"),
        ("binds_to", "绑定到"),
        ("attached_to", "挂载于"),
        ("attaches_to", "挂载于"),
        ("notifies_via", "通知到"),
        ("applies_to", "适用于"),
        ("targets", "指向"),
        ("binds", "绑定到"),
        ("uses", "使用"),
        ("contains", "包含"),
        ("associated_with", "关联"),
        ("bills", "计费到"),
        ("based_on", "基于"),
        ("deployed_to", "部署于"),
    ];

    #[test]
    fn test_semantic_cn_covers_all_15_real_verbs_and_labels_are_chinese() {
        for (raw, want) in REAL_VERBS {
            assert_eq!(semantic_cn(raw), want, "{raw} 的中文映射不对");
        }
        // 同义归一：两个 key **同一个**标签（AC-003）。
        assert_eq!(semantic_cn("binds"), semantic_cn("binds_to"));
        assert_eq!(semantic_cn("attaches_to"), semantic_cn("attached_to"));

        // AC-002 的判据与 `untranslated_semantic_hit` **同一条**：
        // 标签里出现任何 ASCII 字母 ⇒ 它不是中文标签。
        for (raw, _) in REAL_VERBS {
            let label = semantic_label(raw).unwrap_or_else(|| panic!("{raw} 应当有中文标签"));
            assert!(
                !label.chars().any(|c| c.is_ascii_alphabetic()),
                "{raw} 的标签含 ASCII 字母：{label}"
            );
            assert!(!label.is_empty(), "{raw} 的标签不许是空串");
        }

        // 已有映射仍在（⛔ 没被这轮改动弄丢）。
        for (raw, want) in [
            ("has", "拥有"),
            ("depends_on", "依赖"),
            ("records", "记录了"),
            ("derived_from", "算自"),
        ] {
            assert_eq!(semantic_cn(raw), want);
        }
    }

    /// AC-004 · 认不出的动词 ⇒ `None`（⛔ 不编中文、⛔ 不塞空串）；
    /// 空语义（映射侧没有 semantic）同样 `None`。
    #[test]
    fn test_semantic_label_is_none_for_unknown_or_empty() {
        assert_eq!(semantic_label("xyz_unknown"), None);
        assert_eq!(semantic_label("aggregates"), None);
        assert_eq!(semantic_label(""), None);
        assert_eq!(semantic_label("   "), None);
        // `semantic_cn` 的兜底**没变**（守着 `untranslated_semantic_hit` 的形态判据）。
        assert_eq!(semantic_cn("xyz_unknown"), "xyz_unknown");
    }

    // ── REQ-003 · 基数记号零透出（B1）─────────────────────
    //
    // 真实 E2E 的 51 条关系 `how_many` **全是** `N:1`，而词表只认三种
    // → 51 条全部落进 `_ =>` 原样透出。这里锁住四种已知记号都翻成人话。

    #[test]
    fn test_cardinality_cn_translates_every_known_notation() {
        for (raw, want) in [
            ("1:1", "唯一对应"),
            ("1:N", "一对多"),
            ("N:M", "多对多"),
            ("N:1", "多对一"),
        ] {
            let got = cardinality_cn(raw);
            assert_eq!(got, want, "基数记号 {raw} 应当翻成人话");
            assert!(
                !got.contains(':') && !got.contains(".."),
                "翻译结果不该还留着记号形态：{got}"
            );
        }
    }

    /// 第三档守门词：**任何**记号形态都要被抓 —— 不只是 `N:1`。
    /// 少一个「已知记号」或多一种「未知拼法」这里都会红。
    #[test]
    fn test_guard_catches_any_cardinality_notation() {
        for bad in ["N:1", "n:1", "1:M", "M:N", "0:1", "1:0", "1..*", "0..1", "N..M"] {
            let hit = scan_modeling_terms(&format!(r#"{{"how_many":"{bad}"}}"#));
            assert!(
                hit.iter().any(|h| h.contains(bad)),
                "未翻译的基数记号「{bad}」必须被守门器抓住：{hit:?}"
            );
        }
    }

    /// 反向：**不能**把时刻/日期/版本号误判成基数记号。
    /// 属性样例里出现 `09:30` 是常态，误杀会直接关掉整份客户视图。
    #[test]
    fn test_guard_does_not_mistake_timestamps_for_notation() {
        for ok in [
            "09:30", "12:30:00", "23:59", "2026-10-03T10:41:55Z", "1.5", "v1.2.3",
            "http://example.com", "一对多", "多对一", "唯一对应", "多对多",
        ] {
            let hit = scan_modeling_terms(&format!(r#"{{"sample":"{ok}"}}"#));
            assert!(hit.is_empty(), "「{ok}」被误判为建模术语：{hit:?}");
        }
    }

    // ── REQ-003 · 词表增补（AC-104）────────────────────
    //
    // 动机：原词表在真实源库 `<业务库名>`（91 表 / 1051 列）
    // 上只覆盖 18%，81% 被丢弃。丢得最多的恰恰是最标准的审计字段 ——
    // 词表写 `create_time`/`create_by`，库里是 `created_at`/`created_by`
    // （少个 d），**形态对不上，不是词不够**。

    #[test]
    fn test_humanize_real_world_audit_columns() {
        for (col, want) in [
            ("id", "编号"),
            ("created_at", "创建时间"),
            ("updated_at", "更新时间"),
            ("created_by", "创建人"),
            ("is_deleted", "是否删除"),
        ] {
            assert_eq!(
                humanize_column(col).as_deref(),
                Some(want),
                "真实库高频列 {col} 应当能翻成人话"
            );
        }
    }

    /// **S-2 回归点不能丢**：词表扩大后，认不出的技术串仍必须返回 `None`。
    /// 若某次增补写得太宽，把 `blob_ref_xyz` 之类也收进词表，
    /// 客户视图就会重新开始透出技术标识符。
    #[test]
    fn test_humanize_still_refuses_unknown_columns() {
        for bad in [
            "blob_ref_xyz",
            "mystery",
            "zzz_unmapped_token",
            "aisearch_times_internal_v2",
        ] {
            assert_eq!(
                humanize_column(bad),
                None,
                "认不出的列 {bad} 必须返回 None 而不是原样透出"
            );
        }
    }

    // ── REQ-003 · 关系语义零透出（X2 · 与 B1 同构）──────────

    /// 第四档守门：**任何**内部标识符形态的语义都要被抓 —— 不只是 `belongs_to`。
    /// 变异：把第四档（语义形态判定）删掉或削弱 ⇒ 本测试立刻变红。
    ///
    /// 第 7-12 个是**词表里没有**的拼法：这一档判的是**形态**
    /// （一个汉字都没有、却含 ASCII 词元），写成枚举的话它们会全部漏过去。
    #[test]
    fn test_guard_catches_untranslated_semantic_by_form() {
        for bad in [
            "has", "belongs_to", "depends_on", "references", "records", "derived_from",
            "aggregates", "is_a_of", "consists_of", "memberOf", "PART_OF", "linked_to",
        ] {
            let hit = scan_modeling_terms(&format!(r#"{{"meaning":"{bad}"}}"#));
            assert!(
                hit.iter().any(|h| h.contains(bad)),
                "未翻译的关系语义「{bad}」必须被守门器抓住：{hit:?}"
            );
        }
    }

    /// 反向：已翻成中文的语义不该被抓；`belongs_to` 作为**键名**
    /// （`EntityView::belongs_to`）也不该被抓 —— 抓了就是永远出不了图。
    #[test]
    fn test_guard_allows_translated_semantic_and_its_own_key_name() {
        for ok in ["归属于", "拥有", "依赖", "关联到", "记录了", "算自"] {
            let hit = scan_modeling_terms(&format!(r#"{{"meaning":"{ok}"}}"#));
            assert!(
                hit.is_empty(),
                "已翻成中文的语义「{ok}」不该被报出来：{hit:?}"
            );
        }
        let view = r#"{"objects":[{"name":"活动","belongs_to":"活动类型","properties":[]}],"relations":[{"from":"活动","to":"活动类型","how_many":"多对一","meaning":"归属于"}]}"#;
        let hit = scan_modeling_terms(view);
        assert!(
            hit.is_empty(),
            "belongs_to 作为键名不该被判成语义泄漏（否则每份视图都误杀）：{hit:?}"
        );
    }

    /// REQ-005 · 真实源库上「词表翻不出」的两类缺口之一：简单缺词。
    ///
    /// `instance_manager.session_id` 的注释是纯英文 `SessionId`，被抽取层的注释质量门槛
    /// 挡下、回退到词表 —— 但词表原本**没有** `session`，回退返回 `None`，
    /// 属性被 S-2 丢弃、客户视图少一项。
    /// `session_id → 会话` 是语义映射，没有形态规则能推出来，所以只能补词条。
    #[test]
    fn test_humanize_words_missing_from_real_source_db() {
        assert_eq!(humanize_column("session_id").as_deref(), Some("会话"));
        assert_eq!(humanize_column("policy").as_deref(), Some("策略"));
        assert_eq!(humanize_column("policies").as_deref(), Some("策略"));
    }

    // ── REQ-006 · 形态处理器 ────────────────────────────────
    //
    // **下面是判别力所在**：每个测试都对着形态机制的一级破坏。
    // 少了它们，把 `FORM_SUFFIXES` 整个短路掉测试照样全绿 ——
    // 那就是「绿但没判别力」，比明说失败糟糕得多。

    /// 第一级：**形态后缀**（`*_at` / `*_count` / `*_by` / `*_rate` / `*_cycle`）。
    /// 变异：短路 `FORM_SUFFIXES` 循环 ⇒ 本测试立刻变红。
    #[test]
    fn test_humanize_form_suffix_time_and_metric() {
        for (col, want) in [
            ("expire_at", "过期时间"),
            ("effective_at", "生效时间"),
            ("start_at", "开始时间"),
            ("cancelled_at", "取消时间"),
            ("retry_count", "重试数量"),
            ("hit_count", "命中数量"),
            ("locked_by", "锁定操作人"),
            ("cleared_by", "清除操作人"),
            ("discount_rate", "折扣比率"),
            ("billing_cycle", "计费周期"),
            ("reset_cycle", "重置周期"),
        ] {
            assert_eq!(
                humanize_column(col).as_deref(),
                Some(want),
                "形态后缀 {col} 应当能翻成人话"
            );
        }
    }

    /// 第二级：**布尔前缀** `is_`。变异：删掉 `strip_prefix("is_")` 分支 ⇒ 变红。
    #[test]
    fn test_humanize_boolean_prefix_form() {
        for (col, want) in [
            ("is_read", "是否已读"),
            ("is_visible", "是否可见"),
            ("is_recommended", "是否推荐"),
        ] {
            assert_eq!(humanize_column(col).as_deref(), Some(want), "{col} 应当翻成是否句");
        }
    }

    /// 第三级：**无后缀复合词**（`auto_renew` / `package_family`）。
    /// 变异：把结尾的 `resolve_stem(&low)` 换成 `None` ⇒ 本测试立刻变红。
    #[test]
    fn test_humanize_bare_compound_form() {
        assert_eq!(humanize_column("auto_renew").as_deref(), Some("自动续费"));
        assert_eq!(humanize_column("package_family").as_deref(), Some("套餐系列"));
        assert_eq!(
            humanize_column("customer_quota_policies").as_deref(),
            Some("客户额度策略")
        );
    }

    /// **S-2 的命门**：复合词「全有或全无」——
    /// 只要有一个词元不认识，整个属性必须丢弃，**绝不拼出半吊子人话**。
    ///
    /// 变异：把 `parts.push(lookup_word(t)?)` 改成「认识的留、不认识的跳过再拼」⇒ 变红。
    /// 那个方向的失败是 **fail-open（把技术串发���客户）**，比 fail-closed 危险得多。
    #[test]
    fn test_humanize_compound_is_all_or_nothing() {
        for bad in [
            "customer_xyz_policies",   // 只有 policies 认识
            "zzz_unmapped_token",       // 只有 token 认识
            "aisearch_times_internal_v2", // 只有 aisearch / times 认识
            "blob_ref_xyz",             // 一个都不认识
        ] {
            assert_eq!(
                humanize_column(bad),
                None,
                "复合词 {bad} 里有词元不认识，必须整体丢弃而不是拼出半吊子人话"
            );
        }
    }

    /// 形态后缀**不认识词根时也必须丢弃** ——
    /// 变异：把 `return resolve_stem(stem).map(...)` 改成「继续试下一个后缀 / 回退原文」⇒ 变红。
    #[test]
    fn test_humanize_form_suffix_with_unknown_root_is_dropped() {
        for bad in ["mystery_at", "wombat_count", "zzz_by", "qqq_rate"] {
            assert_eq!(
                humanize_column(bad),
                None,
                "形态后缀认得出、词根认不出的 {bad} 必须丢弃"
            );
        }
    }
}

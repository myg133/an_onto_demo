//! CLI —— 端到端演示入口（AC-012）。
//!
//! 设计取向：**每条命令都要能被人手动敲一遍看结果**，
//! 所以输出用人话 + 可 grep 的行，不刷 ANSI 花色。
//!
//! # 提案的两种指法
//!
//! AC-012 的命令里写的是 `--proposal p1`（别名），而库里是 UUID。
//! 两种都支持：`p1` = **该项目内按创建时间倒序**的第 1 个提案
//! （也就是「刚导入的那个」），`p2` = 第 2 个，以此类推；
//! 直接给 UUID 也行。别名是给人用的，UUID 是给脚本用的。
//!
//! **别名是「项目内序号」**：解析（`resolve_proposal`）与打印（`proposal_alias`）
//! 都带 `WHERE project_id = $1`，别的项目的提案既不参与计数、也解析不到。
//! 审核动作因此只会落在**本次导入的那个项目**里。

use crate::db::apply::PgApplier;
use crate::console::OpError;

use crate::db::project::PgProjectStore;
use crate::db::proposal::{NewItem, NewProposal, PgProposalStore};
use crate::db::snapshot_store::PgStore;
use crate::domain::diff::diff_snapshots;
use crate::domain::review::{summarize, Decision, Track};
use crate::domain::snapshot::{ExtractedBy, ProvenanceSource};
use crate::domain::view::SnapshotView;
use anyhow::Context;
use clap::{Parser, Subcommand};
use serde_json::Value;
use sqlx::postgres::PgPool;
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "ontok", about = "FDE 本体论平台 · 本体内核（切片 1，只读闭环）")]
pub struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
pub enum Cmd {
    /// 新建项目
    Project { #[command(subcommand)] cmd: ProjectCmd },
    /// 版本操作
    Version { #[command(subcommand)] cmd: VersionCmd },
    /// 提案
    Propose { #[command(subcommand)] cmd: ProposeCmd },
    /// 审核
    Review { #[command(subcommand)] cmd: ReviewCmd },
    /// 提案应用
    Proposal { #[command(subcommand)] cmd: ProposalCmd },
    /// 本体查看
    Ontology { #[command(subcommand)] cmd: OntologyCmd },
    /// REQ-010 · 业务概念组（§7.1 阶段 3）
    Concept { #[command(subcommand)] cmd: ConceptCmd },
    /// REQ-108 · 置信度校准（AC-011）
    ///
    /// 把「分数」与「人工最终决定」对起来，输出**分箱命中率**。
    /// 样本不足时**明确说样本不足**，不输出 0% / 100% 冒充结论。
    Confidence { #[command(subcommand)] cmd: ConfidenceCmd },
    /// REQ-116 / REQ-129 · **文档 → 本体**的三步（摄取 / 合并 / 签字）
    ///
    /// 当前本体的关系只有归属（79/82 条 `belongs_to`），因为源库没有外键、
    /// 只能按 `*_id` 推断。**关系语义在文档里，不在 DDL 里。**
    ///
    /// | 子命令 | 做什么 | 调模型？ | 连库？ |
    /// | --- | --- | --- | --- |
    /// | `doc ingest` | `.md` / `.txt` → 带证据片段的 concept / relation 断言 | 是 | 否 |
    /// | `doc merge` | 断言 → 机械并进一份提案（同 `proposal.json` 形状） | 否 | 否 |
    /// | `doc sign` | 给一条 doc/both 条目签字（只写 `reviewed_by` + `signed_at` + `signed_at_source`） | 否 | 否 |
    ///
    /// 【可以照抄的一整条】
    ///
    /// ontok doc ingest --input docs/ --out out/assertions.jsonl --stats out/stats.json
    ///
    /// ontok doc merge --assertions out/assertions.jsonl --proposal out/proposal.json --out out/merged.json --schema out/schema.json
    ///
    /// ontok doc sign --file out/merged.json --item relation:订单→套餐模板 --by qa-li --out out/signed.json
    ///
    /// ⚠️ 只吃 `.md` / `.txt`（UTF-8）。`.pdf` / `.docx` / `.doc` **直接报错并点名格式**，
    /// 请先用外部工具转成文本（转换后请记下 hash）。⛔ 不静默跳过。
    /// ⚠️ 每条断言都带 `evidence.quote`，它必须能在对应块的原文里**逐字**找到；
    /// 找不到的断言**丢弃并计数**（⛔ 不修正 quote）。
    /// ⚠️ 两条命令都**不连产品库、不写本体、不出图**：输入是本地文件、产物是本地文件，
    /// 指纹缓存也是本地文件（默认 `<out>.cache.json`）。
    #[command(verbatim_doc_comment)]
    Doc { #[command(subcommand)] cmd: DocCmd },
    /// REQ-118 · **名字提案**（决策点一：表名 / 属性名的中文名）
    ///
    /// ADR-005：涉及决策的先让 AI 提案，**提案永不权威**。本命令只产出提案文件，
    /// ⛔ 不写库、不改本体、不喂任何下游 —— 名字要进本体，必须人评审后
    /// 显式走 `ontok-propose --names` 与既有的双轨审核链路。
    ///
    /// 【可以照抄的一整条】
    ///
    /// ontok name suggest --schema out/schema.json --out out/names.proposal.json --as-names out/table_names.json
    ///
    /// ⚠️ 每条提案都带 `claim / evidence / confidence / reason / decided_by / reviewed_by`；
    /// `evidence` 必须是**输入里能查到**的表名 / 列名 / 注释片段（对不上的条目丢弃并计数）。
    /// ⚠️ `reviewed_by` 恒为 `null`（P2）；`decided_by` 是 `ai:<model>`（P4）。
    /// ⚠️ 同一份 schema 第二次运行 **0 token**、输出**逐字节相同**（指纹缓存，P3）。
    /// ⚠️ `--as-names` 输出的 `TableNames` 形状可直接喂 `ontok-propose --names`（AC-002）。
    #[command(verbatim_doc_comment)]
    Name { #[command(subcommand)] cmd: NameCmd },
    /// REQ-118 · **排除提案**（决策点二：哪些表不进本体）
    ///
    /// ADR-005：AI 逐表判「该不该进本体」，判断落成**可评审的提案**；
    /// 本命令 ⛔ 不写库、⛔ 不直接过滤任何东西 —— 要真的排除，仍需把产物喂给
    /// `ontok-propose --exclude`（决定是人的显式动作）。
    ///
    /// 【可以照抄的一整条】
    ///
    /// ontok exclude suggest --schema out/schema.json --out out/excl.proposal.json
    /// ontok-propose --schema out/schema.json --names out/table_names.json --exclude out/excl.proposal.json --out out/proposal.json --no-sample
    ///
    /// ⚠️ 一份文件装两种形状：`proposals[]`（ADR-005 提案）与 `exclude[]`
    /// （REQ-113 的 `--exclude` 形状）—— 后者可被 `ontok-propose` **直接消费**（AC-003）。
    /// ⚠️ `category` 只能是 REQ-113 的七类枚举之一；⛔ 不许自由发挥。
    /// ⚠️ 「保留」也是判断，也会进 `proposals[]`；拿不准的保留标 `low_confidence`。
    #[command(verbatim_doc_comment)]
    Exclude { #[command(subcommand)] cmd: ExcludeCmd },
    /// ⭐ REQ-121 · **R2RML 映射生成**（本体 ↔ 关系库的桥）
    ///
    /// 数字员工链路是「TTL → Agent 写 SPARQL → 服务端按 R2RML 查库 → Agent 解释」。
    /// 本命令产出中间那份**按 R2RML 查库**的映射：每个类一张表、每个数据类型属性一列、
    /// 每条**验过的**关系一个 join。
    ///
    /// 【可以照抄的一整条】
    ///
    /// ontok r2rml generate --project anspire_grouped --version 3 --out mapping.ttl --schema code/out/schema.json
    ///
    /// ⚠️ 三态决定生成什么：`true` ⇒ 生成 join；`inherit` ⇒ **默认不生成**，
    /// `--include-inherit` 才生成且带 `ontok:joinState "inherit"`；
    /// `false` ⇒ ⛔ **永不生成**，只写进文件尾的注释（含原因）。
    /// ⚠️ `--schema` **必填**：主键 / 复合主键 / 列存在性只能来自真实 schema（不猜）。
    /// 对账失败会**点名到 表.列**并以非零码退出，⛔ 不写出半份映射。
    /// ⚠️ 同一 project+version 两次生成**逐字节相同**（不写时间）。
    #[command(verbatim_doc_comment)]
    R2rml { #[command(subcommand)] cmd: R2rmlCmd },
    /// ⭐ REQ-122 · **SPARQL 查询**（按 R2RML 映射生成 SQL + 租户强制注入）
    ///
    /// 数字员工写 SPARQL，服务端把它变成一条**只读** SQL，并在 `WHERE` 最前面
    /// 按声明注入租户条件 —— Agent 看不见也改不了。
    ///
    /// 【可以照抄的一整条】
    ///
    /// ontok sparql query --project anspire_r2rml_excl --version 2 --schema code/out/schema.json --policy query-policy.json --tenant <租户id> --query 'SELECT ?o ?a WHERE { ?o :orders__amount ?a }'
    ///
    /// ⚠️ **映射来自 `r2rml generate` 的同一个函数**（`r2rml::build_mapping`），
    /// ⛔ 不解析生成的 TTL —— 两份实现会漂移，而漂移的那次不报错。
    /// ⚠️ **租户口径在声明里**（`--policy query-policy.json`，ADR-006）：租户主体、
    /// 逐表过滤列、公共维度 + 理由。⛔ 代码里没有 `customer_id` 这类字面量；
    /// 换库只改声明。没登记、又不在公共维度的表 ⇒ **整条查询拒绝**（点名表）。
    /// ⚠️ 不支持的语法（OPTIONAL / UNION / 子查询 / BIND / 属性路径 / CONSTRUCT / ASK /
    /// REGEX / 函数）**一律报错**，不会静默忽略。
    /// ⚠️ **覆盖范围也会报警**：声明里标了 `coverage: "main_only"` 的表（例如子账号产生的
    /// 扣费主账号看不到），命中时 `warnings[]` 会点名 —— 别把偏小的金额读成「没有业务」。
    /// ⚠️ 源库连接串走环境变量 `ONTOK_SOURCE_DATABASE_URL`（口令不入库），
    /// 连上后**读回** `transaction_read_only = on` 才执行。
    #[command(verbatim_doc_comment)]
    Sparql { #[command(subcommand)] cmd: SparqlCmd },
    /// ⭐ REQ-124 · **控制面 API**（管理流水线的 HTTP 化）
    ///
    /// 把 `propose import` / `proposal apply` / `version freeze` / `ontology export` /
    /// `r2rml generate` / `review show|decide` 各变成一个端点。
    /// ⛔ **不是后门**：只暴露既有流程（import → review → apply → freeze），
    /// 没有「直接改对象」的端点，也没有抽取（抽取属任务系统，另开需求）。
    ///
    /// 【可以照抄的一整条】
    ///
    /// ontok console serve --bind 127.0.0.1:8899 --tokens admin-tokens.json --project anspire_r2rml_excl --schema code/out/schema.json
    ///
    /// ⚠️ `--tokens` 里装**两套**凭据（`kind: "admin" | "query"`）：query token 调管理端点
    /// ⇒ **403**（不是 401），没有 token ⇒ 401；⛔ 同一个 token 不许两用（加载时拒绝）。
    /// ⚠️ 默认只绑**回环**：`0.0.0.0` / 任何非回环地址都要显式 `--allow-public`（启动警告）。
    /// ⚠️ 三份声明（`--exclusions` / `--policy` / `--tenancy`）的 PUT **保存前校验**：
    /// 表 / 列必须存在于 `--schema`；`main_only` 必须带 `reason`；`publicDimensions`
    /// 必须带 `reason`；exclusions 的 `category` 要在枚举内且 `reason`/`decided_by` 非空。
    /// 失败 ⇒ 400 + 点名，**原文件一个字节都不变**。
    /// ⚠️ `GET /api/export/ttl` 与 `POST /api/r2rml` 的产物与对应 CLI 命令**逐字节相同**
    /// （同一个渲染/生成函数，不是第二次实现）。
    /// ⚠️ 审计：每个写操作一行（含失败），token 只记摘要。
    #[command(verbatim_doc_comment)]
    Console { #[command(subcommand)] cmd: ConsoleCmd },
    /// ⭐ REQ-126 · **客户门户**（租户锁死、只能问预置问题）
    ///
    /// 给**客户本人**用的服务：他不需要会写 SPARQL，也不该能写。
    /// 门户只提供**预置问题卡片** —— 每个问题的 SPARQL 由声明文件给出，
    /// 参数（租户）由服务端从**访问口令**里取，⛔ 请求体 / URL 带上 tenant 会被拒。
    ///
    /// 【可以照抄的一整条】
    ///
    /// ontok portal serve --bind 127.0.0.1:8900 --tokens portal-tokens.json --project anspire_r2rml_excl --version 2 --schema code/out/schema.json --policy query-policy.json --questions portal-questions.json
    ///
    /// ⚠️ 端点**只有四个**：`GET /`（页面壳，零数据）、`GET /api/questions`（问题列表，
    /// 不含 SPARQL）、`POST /q/<id>`（执行一个问题）、`GET /api/health`（免 token）。
    /// ⛔ **没有** `/api/sparql`、没有写操作、不能切项目/版本（进程级固定）。
    /// ⚠️ 凭据是**另一套**：`--tokens` 里的 `kind` 必须是 `"portal"`，每张口令绑定一个租户
    /// （admin / query 凭据不能当门户口令用，加载时就整份拒绝）。
    /// ⚠️ `portal-questions.json` **启动时逐题校验**（fail-closed）：`reviewed_by` 为空、
    /// 表没在 `--policy` 里登记、属性在目录里不存在、结果列是实例 IRI 或与真实列名同名
    /// ⇒ **拒绝启动并点名**那一条，⛔ 不会「跳过它继续跑」。
    /// ⚠️ 结果只给客户看**业务名 / 覆盖警告 / 截断说明**：⛔ 不返回 `sql_echo`、
    /// 表名、列名、属性 IRI。`coverage: main_only` 的表会在页面上**显著告警**。
    /// ⚠️ 每次问题执行一行审计（口令只记摘要；⛔ 无 SQL、无查询原文）。
    #[command(verbatim_doc_comment)]
    Portal { #[command(subcommand)] cmd: PortalCmd },
    /// ⭐ REQ-123 · **查询服务对外**（REST + 远程 MCP；一个进程、两种协议、一份鉴权）
    ///
    /// 数字员工链路的最后一块：把 REQ-122 的 `sparql query` 做成**服务**，
    /// 让 Agent 通过 HTTP 或 MCP 调用。⛔ 租户只能来自 token —— 请求里带 tenant
    /// 只用于核对，不一致直接 403。
    ///
    /// 【可以照抄的一整条】
    ///
    /// ontok query serve --bind 10.0.0.5:8800 --tokens tokens.json --project anspire_r2rml_excl --version 2 --schema code/out/schema.json --policy query-policy.json
    ///
    /// ⚠️ **默认只绑内网/本机**：`127.0.0.1` / `10.x` / `172.16–31.x` / `192.168.x` / `fc00::/7`。
    /// 绑 `0.0.0.0`（或任何公网地址）**必须**显式加 `--allow-public`，启动时打警告。
    /// ⚠️ token 表是**静态文件**（`--tokens`）：`token → {tenant, max_rows, max_concurrency}`；
    /// 启动时读一次（改 token 要重启）。示例见 `tokens.example.json`，⛔ 里面的 token 是假值。
    /// ⚠️ 租户声明 `--policy` **必填**（ADR-006：机制在代码、口径在声明）。
    /// ⚠️ 版本必须**已冻结**（与 `serve` / `mcp-server` 同一条闸门）。
    #[command(verbatim_doc_comment)]
    Query { #[command(subcommand)] cmd: QueryCmd },
    /// REQ-027 · 以**标准 MCP**（stdio）把**冻结快照**暴露给客户端
    ///
    /// 只服务已冻结的版本 —— 未冻结时**拒绝启动**（fail-closed）。
    McpServer {
        /// 项目名
        #[arg(long)]
        project: String,
        /// 第几版（该版必须已冻结）
        #[arg(long)]
        version: i32,
    },
    /// REQ-093 · 起一个**只读** HTTP 服务，把本体图关系用浏览器看
    ///
    /// 只服务已冻结的版本 —— 未冻结时**拒绝启动**（fail-closed）。
    /// 只接受 GET：任何「改动」类请求一律 405，没有可写的地方。
    /// 监听地址只允许回环（`127.0.0.1` / `::1`），绑别处直接拒绝启动。
    ///
    /// 【可以照抄的一整条】
    ///
    /// export ONTOK_VIEW_TOKEN='换成你自己的口令'
    /// cargo run --bin ontok -- serve \
    /// --project demo3 --version 2 --bind 127.0.0.1:8787
    ///
    /// 然后浏览器打开 http://127.0.0.1:8787/
    ///
    /// ⚠️ **三个参数缺一不可。** 少了任何一个，clap 会**直接报错退出（exit 2）**，
    /// 并把缺哪个、怎么写打出来 —— 不是安静地什么都不返回。
    ///
    /// 【还要带查询参数】页面顶部填入口令后才发请求，而请求是：
    ///
    /// /api/graph?project=demo3&version=2
    ///
    /// ⚠️ **查询参数漏了会得到一句人话 400**（同样不静默）：
    /// `要说明看哪个项目：请带上 project 参数。`
    ///
    /// ⚠️ **访问口令从环境变量 `ONTOK_VIEW_TOKEN` 读**，缺了或短于 8 个字符
    /// 都拒绝启动，**没有开发模式开关**。浏览器侧把它填进页面顶部的口令框，
    /// 它只留在内存里，不进地址栏、不进页面源码、不进浏览器存储。
    ///
    /// ⚠️ **这个身份门是临时的、刻意不完整的**：只有一个静态共享令牌，
    /// 没有过期、没有吊销、不能区分「谁在看」。完整的
    /// subject → 本体 object key 解析是 REQ-099（决策 D1），不在本需求内。
    // ⚠️ **示例必须顶格写、不能缩进** —— rustdoc 把 doc 注释里**缩进 4 格以上**
    // 的块当成 **Rust 代码块**去编译，于是 `export …` / `cargo run … \`
    // 会被当 Rust 解析、编译失败，`cargo test` 的 Doc-tests 直接红
    // （实测踩过一次：`character literal may only contain one codepoint`）。
    // 顶格写则只是普通段落，rustdoc 不碰它；`verbatim_doc_comment` 又让
    // clap 原样保留换行 ⇒ `--help` 里仍然是一整条可照抄的命令。
    #[command(verbatim_doc_comment)]
    Serve {
        /// 项目名
        #[arg(long)]
        project: String,
        /// 第几版（该版必须已冻结）
        #[arg(long)]
        version: i32,
        /// 监听地址。**只允许回环地址**（如 127.0.0.1:8787）；
        /// 绑到非回环地址会被拒绝启动
        #[arg(long, default_value = "127.0.0.1:8787")]
        bind: String,
    },
}

#[derive(Subcommand)]
pub enum ConceptCmd {
    /// 把一份**概念声明**导入成提案（走双轨审核，不直接生效）
    Import {
        #[arg(long)]
        file: PathBuf,
        /// 项目名。声明本身与项目无关（同一份声明可用于不同项目），
        /// 所以这里必须显式给 —— 免得把 A 项目的声明悄悄合进 B 项目。
        #[arg(long)]
        project: String,
        /// 基于哪个版本提案。默认 v1
        #[arg(long, default_value_t = 1)]
        base_version: i32,
        /// 轮次（写入 proposals.round）
        #[arg(long, default_value_t = 1)]
        round: i32,
    },
    /// 打印当前内容指纹，供声明作者更新 `history`
    ///
    /// **它不绕过契约**：指纹对不上时 `import` 照样拒。
    /// 它只是让「升版本」这件事不用手算哈希。
    Digest {
        #[arg(long)]
        file: PathBuf,
    },
    /// REQ-115 / REQ-118 · 从关系拓扑算**建议**分组（不写库，只产出一份声明文件）
    ///
    /// 产物可以直接喂给 `concept import`，之后仍需双轨审核 + `proposal apply`
    /// 才会进本体。⛔ 本命令**不写库**：跑前跑后库里的概念组/提案/版本行不变。
    ///
    /// 【可以照抄的一整条】（算法版 = 降级路径，`source = "rule"`）
    ///
    /// ontok concept suggest --project demo3 --version 2 --out d.json
    /// ontok concept import --file d.json --project demo3 --base-version 3
    ///
    /// 【可以照抄的一整条】（AI 语义版，`source = "ai"`，REQ-118 · AC-004/AC-007）
    ///
    /// ontok concept suggest --ai --project demo3 --version 2 --out groups.proposal.json --budget-tokens 100000
    /// ontok concept import --file groups.proposal.json --project demo3 --base-version 3
    ///
    /// ⚠️ 建议 ≠ 结论：声明文件里 `reviewed_by` / `confirmed_by` = `null`。
    /// 算法版的 `source` 是 `rule`；`--ai` 版是 `ai` —— ⛔ 算法版不得冒充 `ai`。
    /// ⚠️ `--ai` 时 `--out` **必填**（ADR-005 P1：提案必须落盘）；
    /// 算法版省略 `--out` 时把声明打到 stdout（管道友好）。
    #[command(verbatim_doc_comment)]
    Suggest {
        /// 项目名
        #[arg(long)]
        project: String,
        /// 从哪一版的关系拓扑算（该版必须有快照）
        #[arg(long)]
        version: i32,
        /// 输出路径。`--ai` 时必填；算法版省略则把声明 JSON 打到标准输出
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// ⭐ REQ-118：用**模型**做语义分组（AC-004），而不是算法版的贪心模块度。
        /// 需要 `ONTOK_LLM_BASE_URL` / `ONTOK_LLM_API_KEY` / `ONTOK_LLM_MODEL` 三件套。
        #[arg(long)]
        ai: bool,
        /// 预算闸（token，仅 `--ai`）。越限即停且不产出声明文件（AC-006）。
        #[arg(long)]
        budget_tokens: Option<u64>,
        /// 指纹缓存落点（仅 `--ai`）。默认 `<out>.cache.json`。
        #[arg(long)]
        cache: Option<PathBuf>,
    },
}

/// REQ-116 · 文档摄取（`ontok doc <子命令>`）。
///
/// 与 `serve` / `review serve` 那种「一个命令 + 开关」的取舍同理：
/// 将来还会有「文档概念 ↔ 表/字段」的对齐（REQ-117），那是**另一件事**，
/// 所以从第一天就分成子命令，而不是给 `ingest` 加一个 `--align` 开关。
#[derive(Subcommand)]
pub enum DocCmd {
    /// 从 `.md` / `.txt` 抽**带证据**的断言，产出 JSONL（+ stats.json）
    ///
    /// 分块是确定性的（标题层级 + 段落）；抽取靠指纹缓存：
    /// 同一份输入第二次运行 **0 token**、JSONL **逐字节相同**。
    Ingest {
        /// 输入：一个 `.md`/`.txt` 文件，或一个目录（递归）。
        /// ⛔ `.pdf` / `.docx` / `.doc` 直接报错并点名格式。
        #[arg(long)]
        input: PathBuf,
        /// 产物：断言 JSONL（带 `--chunks-only` 时是块清单 JSONL）
        #[arg(long)]
        out: PathBuf,
        /// 统计：`stats.json` 的落点。省略则不写（屏幕上报的同一批数字）
        #[arg(long)]
        stats: Option<PathBuf>,
        /// 预算闸（token）。越限即**停**并按「已用 / 上限 / 停在第几块」报告，
        /// 且**不产出断言文件**（AC-007 选的是「不产出半份 JSONL」）。
        /// 省略时用默认上限（`doc_ingest::DEFAULT_BUDGET_TOKENS`）。
        #[arg(long)]
        budget_tokens: Option<u64>,
        /// 只跑前 N 块（分阶段上线用）。`stats.truncated` 会标 true。
        #[arg(long)]
        limit_chunks: Option<usize>,
        /// 指纹缓存的落点。默认 `<out>.cache.json`。
        /// 删掉它 ⇒ 下次重新调用模型（重新计费）；保留它 ⇒ 同输入 0 token。
        #[arg(long)]
        cache: Option<PathBuf>,
        /// **只分块、不调用模型**：把块清单写成 JSONL（判 AC-002 的确定性用）。
        /// 不需要 LLM 三件套、不需要预算。
        #[arg(long)]
        chunks_only: bool,
    },
    /// ⭐ REQ-129 · 把断言**机械地**并进一份提案（产出与 `proposal.json` 同形状）
    ///
    /// 这一步是「文档 → 本体」断掉的那一节：`doc ingest` 产出带证据的断言，
    /// 本命令把其中**已对齐**的部分并进提案，于是链路继续：
    /// `propose import → review decide → proposal apply → version freeze`。
    ///
    /// 【可以照抄的一整条】
    ///
    /// ontok doc merge --assertions out/assertions.jsonl --proposal out/proposal.json --out out/merged.json --stats out/merge.stats.json
    ///
    /// ⚠️ **只并 `alignable: true`**：`future` / `example` 一律排除并计数（那是文档里的设想/举例）。
    /// ⚠️ **只做机械对齐**：断言自带 `maps_to`（表的列 / 关系的 child 列 + parent 列）就校验后并入；
    /// 没有映射的关系 ⇒ `doc_unmapped`，⛔ **不按名字猜表**；概念只按「精确名字/别名命中」对齐。
    /// ⚠️ doc 条目 `band` 最高 `medium`、`reviewed_by` 恒 `null` ⇒ **未签的不会被 apply 采用**。
    /// ⚠️ 基数冲突的条目 `applicable: false` 且两侧说法都在 `conflicts[]` 里 ——
    /// `proposal apply` 遇到它会**拒绝并点名**（不是忽略）。
    /// ⚠️ 同一份输入两次运行 ⇒ 输出**逐字节相同**（无时间戳、无随机、稳定排序）。
    /// ⚠️ ⛔ **输出不许指向输入，也不许互相撞**：`--out` / `--stats` 与
    /// `--assertions` / `--proposal`（以及两者彼此）撞了就 **exit≠0 并点名**。
    /// 静默覆盖是数据丢失（`--stats` 指向提案会把提案写成一坨统计 JSON）。
    /// ⚠️ 重跑合并会让**之前的签字失效**（`reviewed_by` / `signed_at` 一并清掉，
    /// `stats.reviewed_by_reset` 会记数）—— 内容变了签名就该作废，这是刻意的。
    /// ⚠️ 本命令**不连产品库、不调模型**：输入两份本地文件、产物也是本地文件。
    #[command(verbatim_doc_comment)]
    Merge {
        /// 断言 JSONL（`ontok doc ingest` 的产物）
        #[arg(long)]
        assertions: PathBuf,
        /// 提案 JSON（`ontok-propose` 的产物）
        #[arg(long)]
        proposal: PathBuf,
        /// 合并结果：与 `proposal.json` 同形状（可直接 `propose import`）
        #[arg(long)]
        out: PathBuf,
        /// 统计：`stats.json` 的落点（五项 + 明细）。省略则不写（屏幕上报同一批数字）
        #[arg(long)]
        stats: Option<PathBuf>,
        /// ⭐ 真实 schema（`ontok-introspect` 产出的 `schema.json`）。**可选**。
        ///
        /// 给了 ⇒ **严格**校验 `maps_to` 的每个表/列（**含父端主键列**，如 `customers.id`），
        /// 任何一个不存在 ⇒ `doc_unmapped`（AC-004：给了 schema 就 0 例外）。
        /// 不给 ⇒ 退回 fallback（列必须在提案的 `properties` 里；父端列额外接受约定名 `id`），
        /// 并在 stats 里记 `parent_key_id_convention` + stderr 警告 —— **可数、可见**。
        #[arg(long)]
        schema: Option<PathBuf>,
    },
    /// ⭐ REQ-129 · 给一条 `doc` / `both` 条目**签字**（写 `reviewed_by` + `signed_at`）
    ///
    /// # 为什么需要一条命令
    ///
    /// 规则 7 说「未签 ⇒ apply 时忽略」；而「签」在 REQ-118 里是**评审流程写进提案**的动作。
    /// 没有这条命令时，唯一办法是手工改 JSON 里的 `reviewed_by` —— 那不是产品能力，
    /// 也没人挡得住手滑改错条目。
    ///
    /// 【可以照抄的一整条】
    ///
    /// ontok doc sign --file out/merged.json --item relation:订单→套餐模板 --by qa-li --out out/signed.json
    ///
    /// ⚠️ `--item` 认三种写法：**1 基序号**（`136`，与 `review decide --item N` 同一个 N）、
    /// **稳定键**（`relation:订单→套餐模板` / `entity:customers` /
    /// `relation:order_template_id__package_templates`）、以及 `apply` 报错里那个
    /// `136:add_link_type:订单→套餐模板`。⛔ 不认数组下标（顺序会变，按它签迟早签错人）。
    /// ⚠️ 只签 `origin: doc | both` 的条目；DB 轨条目请走 `review decide`（两条轨有意分开）。
    /// ⚠️ 只写 `reviewed_by` + `signed_at` + `signed_at_source` 三栏，**其余字节不变**；
    /// 重复签同一个人 ⇒ 幂等。
    /// ⚠️ `signed_at_source` 标的是时间来源，**三值**（与模块头一致）：
    /// 给了 `--at` ⇒ `arg`（人给的声明，可复现）；省略 ⇒ `system`（本次运行的系统时钟，
    /// 记录）；`legacy` = REQ-129 的老产物没有这一栏，读回时按缺省处理
    /// （不报错、⛔ 不补写，也⛔ 不写进产物）。
    /// ⚠️ ⛔ 不就地改输入：必须给 `--out`（或显式 `--in-place`）。
    /// ⚠️ 签字时间是事实，所以默认取当前 UTC 时间；要逐字节复现就给 `--at <RFC3339>`。
    #[command(verbatim_doc_comment)]
    Sign {
        /// 要签的提案文件（`ontok doc merge` 的产物）
        #[arg(long)]
        file: PathBuf,
        /// 签哪一条：1 基序号 / `relation:订单→套餐模板` / `136:add_link_type:订单→套餐模板`
        #[arg(long)]
        item: String,
        /// 签字人（**必填**，空/空白 ⇒ 拒绝：⛔ 不许匿名签）
        #[arg(long)]
        by: String,
        /// 签完的输出（⛔ 与 `--in-place` 互斥；两个都不给 ⇒ 拒绝）
        #[arg(long)]
        out: Option<PathBuf>,
        /// 显式就地覆盖输入（不加这个开关时 `--out` 指向输入文件也会被拒）
        #[arg(long)]
        in_place: bool,
        /// 签字时间（RFC3339，如 `2026-10-24T04:00:00Z`）。省略 = 当前 UTC 时间
        #[arg(long)]
        at: Option<String>,
    },
}

/// REQ-118 · 名字提案（`ontok name suggest`）。
#[derive(Subcommand)]
pub enum NameCmd {
    /// AI 为每张表提一个**带证据与置信度**的中文名（只产提案，不写库）
    ///
    /// 与 `ontok-translate-names`（REQ-090）的分工：那条命令维护**增量翻译表**
    /// （保护人工旧译、缺口闸、退化自检），本命令产出**从零评审的提案**
    /// （每条可 diff、可签字）。两者通过 `--as-names` 搭桥。
    Suggest {
        /// 源库勘查产物（`ontok-introspect` 写出的 `schema.json`）
        #[arg(long)]
        schema: PathBuf,
        /// 提案落点（**必填**：ADR-005 P1 —— 提案必须落盘、可 diff、可评审）
        #[arg(long)]
        out: PathBuf,
        /// 同时把名字转成 `ontok-propose --names` 可直接消费的 `TableNames` 形状（AC-002）
        #[arg(long)]
        as_names: Option<PathBuf>,
        /// 预算闸（token）。越限即**停**并按「已用 / 上限 / 停在第几批」报告，
        /// 且**不产出提案文件**（AC-006：不产半份文件）。
        #[arg(long)]
        budget_tokens: Option<u64>,
        /// 指纹缓存的落点。默认 `<out>.cache.json`。
        /// 删掉它 ⇒ 下次重新调用模型（重新计费）；保留它 ⇒ 同输入 0 token。
        #[arg(long)]
        cache: Option<PathBuf>,
        /// 每次请求送几张表（默认 12）。只影响请求切分，不改变判据。
        #[arg(long)]
        batch_size: Option<usize>,
    },
}

/// REQ-118 · 排除提案（`ontok exclude suggest`）。
#[derive(Subcommand)]
pub enum ExcludeCmd {
    /// AI 逐表判「这张表该不该进本体」，产出可评审、且能被 `--exclude` 直接消费的提案
    Suggest {
        /// 源库勘查产物（`schema.json`）
        #[arg(long)]
        schema: PathBuf,
        /// 提案落点（**必填**：P1）
        #[arg(long)]
        out: PathBuf,
        /// 已有排除清单（REQ-113 形状）：其中**仍在 schema 里**的条目原样沿用、
        /// 不再送模型；schema 里已消失的条目进 `stale_excluded_tables`（⚠️ 只报不删）。
        #[arg(long)]
        exclusions: Option<PathBuf>,
        /// 预算闸（token）。越限即停且不产出提案文件（AC-006）。
        #[arg(long)]
        budget_tokens: Option<u64>,
        /// 指纹缓存落点。默认 `<out>.cache.json`。
        #[arg(long)]
        cache: Option<PathBuf>,
        /// 每次请求送几张表（默认 10）。
        #[arg(long)]
        batch_size: Option<usize>,
    },
}

#[derive(Subcommand)]
pub enum ProjectCmd {
    /// 新建项目（同名则复用）
    New {
        #[arg(long)]
        name: String,
    },
    /// 列出项目
    List,
}

#[derive(Subcommand)]
pub enum VersionCmd {
    /// 新建版本
    New {
        #[arg(long)]
        project: String,
    },
    /// 列出版本
    List {
        #[arg(long)]
        project: String,
    },
    /// 冻结版本
    Freeze {
        #[arg(long)]
        project: String,
        #[arg(long)]
        version: i32,
    },
}

#[derive(Subcommand)]
pub enum ProposeCmd {
    /// 从 JSON 文件导入提案
    Import {
        #[arg(long)]
        file: PathBuf,
    },
}

/// ⭐ REQ-121 · R2RML 映射生成。
#[derive(Subcommand)]
pub enum R2rmlCmd {
    /// 从**建模视图**生成 R2RML（Turtle）：类 → 表、属性 → 列、验过的关系 → join
    ///
    /// 输入全是**已有的产物**：快照（`--project/--version`）+ `--schema`
    /// （`ontok-introspect` 的 schema.json）+ 可选的 `--tenancy`（租户列）。
    /// ⛔ 不跑 SQL、不推断新的列、不发明关系 —— 只把已签的证据翻译成 R2RML。
    Generate {
        /// 项目名
        #[arg(long)]
        project: String,
        /// 第几版（该版必须已有快照）
        #[arg(long)]
        version: i32,
        /// **必填**：真实 schema（`ontok-introspect` 产物）。
        ///
        /// 主键 / 复合主键 / 列存在性 / 唯一列**只能**来自它 —— 没有它就没法
        /// 与库对账（⛔ 不猜主键，也不拿启发式顶替）。
        #[arg(long)]
        schema: PathBuf,
        /// 输出路径。省略则打到标准输出
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// 生成 `joinVerified "inherit"` 的边（默认**不生成**）
        ///
        /// 生成时该 `rr:predicateObjectMap` 会带 `ontok:joinState "inherit"` +
        /// `ontok:joinEvidence` —— 让下游看得见「这条 join 本表没实测」。
        #[arg(long)]
        include_inherit: bool,
        /// IRI 基（本体 IRI = `<base><project>`，前缀 = `<base><project>#`）
        #[arg(long, default_value = "https://ontok.local/")]
        base: String,
        /// 租户声明（与 `ontology export --model-view` 同一份口径）。
        ///
        /// 已签（`reviewed_by` 非空）且命中时，类的租户列写进 TriplesMap 的
        /// `ontok:tenantColumn`，供 REQ-122 注入过滤。省略 ⇒ 文件头 + stderr 明说「未标注」。
        #[arg(long)]
        tenancy: Option<PathBuf>,
        /// **审计**（A-2.4）：`exclusions.json`。给了才核对「已排除的表是否混进映射」。
        ///
        /// ⛔ 不会因此删掉任何 TriplesMap —— 它只做**可见性**：
        /// 映射里出现已排除表 ⇒ stderr 警告 + 文件头点名「本映射包含 N 张已排除的表」。
        /// 生产口径应当是**快照**里就不含这些表（`ontok-propose --exclude`）。
        #[arg(long)]
        exclude: Option<PathBuf>,
        /// **审计**（A-2.3）：已签字的 FK 提案（`fk-proposals.json`）。
        ///
        /// 给了才点名「端点被排除、因而没有生成」的已签关系 —— 这些边在快照里
        /// 已经不存在（`apply` 拒绝悬空关系），只有这份文件还留着它们。
        #[arg(long)]
        fk_signed: Option<PathBuf>,
    },
}

/// ⭐ REQ-122 · SPARQL 查询。
#[derive(Subcommand)]
pub enum SparqlCmd {
    /// 执行一条 SPARQL SELECT（v1 子集），返回 JSON（`sql_echo` 里能看见注入条件）
    ///
    /// 输出（stdout）：`{columns, rows, row_count, truncated, sql_echo, tenant_applied[],
    /// tables[], public_dimensions[], warnings[]}`。
    /// 诊断一律走 stderr（映射规模 / 声明摘要 / 注入了哪几张表 / 只读回执）。
    ///
    /// ⚠️ **`sql_echo` 就是真正执行的那条 SQL**（字面量已内联），所以
    /// 「租户条件在不在里面」用文本断言即可，不需要相信任何注释。
    /// ⚠️ `LIMIT` 是 `--max-rows + 1`：第 N+1 行只用来判断 `truncated`，不进结果。
    /// ⚠️ 超时有**两道**：服务端 `statement_timeout` + 客户端兜底（+2s）。
    Query {
        /// 项目名
        #[arg(long)]
        project: String,
        /// 第几版
        #[arg(long)]
        version: i32,
        /// **必填**：真实 schema（`ontok-introspect` 产物）。
        ///
        /// 主键 / 列存在性 / 列类型都只能来自它（⛔ 不猜；数值去重、类型不匹配也要靠它）。
        #[arg(long)]
        schema: PathBuf,
        /// 租户声明（`query-policy.json`，ADR-006）—— **没有默认值**。
        ///
        /// 租户主体 / 逐表过滤列 / 公共维度 + 理由。省略 ⇒ **整条拒绝**并打印模板
        /// （没有声明就不知道哪张表按哪一列过滤 —— ⛔ 不许猜一个列名）。
        #[arg(long)]
        policy: Option<PathBuf>,
        /// 租户 id（会原样写进 `WHERE <声明里的列> = '<这个值>'`）。
        ///
        /// ⛔ 不是「可选过滤」：空值直接拒绝（`--tenant` 必填）。
        #[arg(long)]
        tenant: String,
        /// SPARQL 查询（v1 子集：BGP / 基本 FILTER / LIMIT / OFFSET / ORDER BY 单列 /
        /// COUNT·SUM·MIN·MAX·AVG + GROUP BY 单列；唯一的清单外额外项是 `SELECT DISTINCT`，
        /// 它只去重、不改变「取哪些行」）。
        #[arg(long)]
        query: String,
        /// 最多返回多少行（超出 ⇒ `truncated: true` 并明确告知）。
        ///
        /// 这是**硬上限**：查询里写更大的 LIMIT 会被压到这个值并给 warning。
        #[arg(long, default_value_t = 1000)]
        max_rows: u64,
        /// 超时（毫秒）。服务端 `statement_timeout` 用它，客户端兜底 +2s。
        #[arg(long, default_value_t = 10_000)]
        timeout_ms: u64,
        /// IRI 基（本体 IRI = `<base><project>`，默认前缀 `:` = 本体 IRI + `#`）。
        ///
        /// 必须与 `ontok r2rml generate` 用的是同一个值，否则属性 IRI 对不上。
        #[arg(long, default_value = "https://ontok.local/")]
        base: String,
        /// 把 `joinVerified "inherit"` 的边也生成 join（默认不生成）。
        ///
        /// 生成时会带 `ontok:joinState "inherit"` 标（这些 join 本表没实测）。
        #[arg(long)]
        include_inherit: bool,
    },
}

/// ⭐ REQ-123 · 查询服务（REST + 远程 MCP）。
#[derive(Subcommand)]
pub enum QueryCmd {
    /// 起查询服务：REST（/api/sparql /catalog /describe /health）+ MCP over HTTP（/mcp）
    ///
    /// 端点：
    ///
    /// POST /api/sparql         body {query, max_rows?}（tenant 只能来自 token）
    /// GET  /api/catalog?page=1&page_size=50
    /// GET  /api/describe?name=:customers
    /// GET  /api/ontology.ttl   ⭐整份 OWL（Turtle）：默认**客户视图**；`?view=model` 建模视图
    /// GET  /api/health         免 token，只给版本与语法版本
    /// POST /mcp                JSON-RPC 2.0（initialize / tools/list / tools/call / resources/list / resources/read）
    /// GET  /mcp                SSE 流（服务端消息；空闲超时会主动断开）
    ///
    /// MCP 工具：catalog / describe / sparql_query / sample。
    /// MCP 资源：`ontok://ontology/customer.ttl`（客户视图）/ `ontok://ontology/model.ttl`（建模视图），
    /// 用 `resources/read` 取原文 —— 与 HTTP 同视图的字节**逐字节相同**。
    ///
    /// ⚠️ `/api/ontology.ttl` 的字节与 CLI 导出**同参数**时 sha256 相等：
    /// 客户视图 = `ontok ontology export`（不带 `--schema`；客户视图结构上不含列名，
    /// `--schema` 只改文件头一句说明）；建模视图 = `ontok ontology export --model-view
    /// --schema <schema> --tenancy <file>`。⛔ 客户视图不含 `ontok:tenant*` / 列名 / 主键 /
    /// 枚举 / join 证据；要这些必须显式 `?view=model`（R1）。
    ///
    /// ⚠️ 回执里的 `join_states[]` 是**什么范围**（⛔ 别读成「本次查询走到的边」）：
    ///
    /// 范围 = 「**查询涉及的表之间**的全部映射边」。回执里由 `join_states_scope` 标明，
    /// 值恒为 `all_edges_between_involved_tables`；`join_states_scope_note` 是同一件事的
    /// 人话说明。只要两张表都出现在 `tables[]` 里，声明/映射里它们之间的**每一条** join
    /// 都会列出来 —— 不论这次查询有没有用到它。
    ///
    /// 每条边带 `used`：`false` = 「涉及但本次未用」（例：三表查询
    /// `?l :licenses__customers ?c . ?b :balances__customers__customer_id ?c` 会顺带列出
    /// `balances__customers__source_customer_id`，本次没走到它），`true` = 该边的谓词写进了
    /// 查询。要「只列走过的边」是后续需求（planner 的 `Planned` 暂未暴露用过的边）。
    ///
    /// ⚠️ 鉴权只认 `Authorization: Bearer <token>`。⛔ 放 query string / cookie **不生效**。
    /// ⚠️ 限额：`max_rows`（token 级上限，调用方只能往下压）、超时、每 token 并发、
    /// SSE 连接数 + 空闲超时；超限 **429**。
    /// ⚠️ 审计：每次调用一行（时间 / token 摘要 / tenant / 查询哈希 / 行数 / 耗时 / code），
    /// ⛔ 不记 token 明文、不记查询原文。
    #[command(verbatim_doc_comment)]
    Serve {
        /// 监听地址。默认只允许内网/回环；绑 0.0.0.0 或公网地址要显式 `--allow-public`
        #[arg(long)]
        bind: String,
        /// 静态 token 表（JSON，见 `tokens.example.json`；示例里的 token 是假值）
        #[arg(long)]
        tokens: PathBuf,
        /// 项目名
        #[arg(long)]
        project: String,
        /// 第几版（该版必须已冻结）
        #[arg(long)]
        version: i32,
        /// 真实 schema（`ontok-introspect` 产物）—— 列存在性 / 列类型只能来自它
        #[arg(long)]
        schema: PathBuf,
        /// 租户声明（`query-policy.json`，ADR-006）—— **没有默认值**
        #[arg(long)]
        policy: PathBuf,
        /// ⭐ 租户标注声明（与 `ontology export --tenancy` 同一份）—— **只在 `?view=model` 生效**。
        ///
        /// 省略时自动取 `--policy` 同目录的 `tenancy.json`（存在才用；取不到会在启动时警告，
        /// 建模视图因此不含 `ontok:tenant*`）。客户视图结构上不含租户信息，不受它影响。
        #[arg(long)]
        tenancy: Option<PathBuf>,
        /// 允许绑非内网地址（0.0.0.0 / 公网）。⛔ 不加就只能绑回环或内网
        #[arg(long)]
        allow_public: bool,
        /// 单次查询超时（毫秒）：服务端 statement_timeout + 客户端兜底都按它
        #[arg(long, default_value_t = 10_000)]
        timeout_ms: u64,
        /// SSE 空闲超时（毫秒）：到点主动断开并记审计（≥ 1000）
        #[arg(long, default_value_t = 60_000)]
        sse_idle_timeout_ms: u64,
        /// 全局 SSE 连接上限（超限 429）
        #[arg(long, default_value_t = 32)]
        sse_max_connections: usize,
        /// 全局并发查询上限（按源库只读池的连接数取，默认 2）
        #[arg(long, default_value_t = 2)]
        max_concurrent_queries: usize,
        /// 审计落点（追加写）。省略 = 只写 stderr；stderr **恒写**
        #[arg(long)]
        audit_out: Option<PathBuf>,
        /// IRI 基（必须与 `ontok r2rml generate` 用的是同一个）
        #[arg(long, default_value = "https://ontok.local/")]
        base: String,
        /// 把 `joinVerified "inherit"` 的边也生成 join（默认不生成，与 `r2rml generate` 一致）
        #[arg(long)]
        include_inherit: bool,
    },
}

/// ⭐ REQ-124 · 控制面（`ontok console serve`）。
#[derive(Subcommand)]
pub enum ConsoleCmd {
    /// 起控制面 API：管理流水线的每一步一个端点（⛔ 只暴露既有流程）
    ///
    /// 端点：
    ///
    /// GET  /                                 ⭐REQ-125 工作台页面壳（免 token；壳内零数据）
    /// GET  /api/health                       免 token，只给版本
    /// ⚠️ 本服务**不含查询端点**：POST /api/sparql、GET /api/catalog、GET /api/describe 属于 `ontok query serve`（另一进程）；
    ///    查询台 / 目录浏览需由反向代理把两者挂到**同一个源**下（本机 `tests/fixtures/req125/dev_proxy.ps1`，部署见 BA/runbook/ontology-e2e.md §7.3）。
    /// GET  /api/projects                     项目 / 版本 / 冻结状态 / 对象·关系计数
    /// POST /api/import                       body {project, path} 或 {project, proposal}
    /// POST /api/apply                        body {project, proposal}
    /// POST /api/freeze                       body {project, version}
    /// GET  /api/export/ttl                   ?project&version&model_view=1&schema=&tenancy=
    /// POST /api/r2rml                        body {project, version, schema, exclude?, include_inherit?}
    /// GET/PUT /api/config/{exclusions|policy|tenancy}   读原文 / 保存前校验后写入
    /// GET  /api/audit                        ?limit=
    /// GET  /api/review                       ?project&proposal
    /// POST /api/decide                       body {project, proposal, item, track, decision}
    ///
    /// ⚠️ 鉴权只认 `Authorization: Bearer <token>`。`--tokens` 里装两套凭据
    /// （`kind: "admin" | "query"`）：query token 调管理端点 ⇒ **403**；无 token ⇒ 401。
    /// ⛔ 同一个 token 不许两用 —— 加载时直接拒绝整份表。
    /// ⚠️ 默认只绑回环；`0.0.0.0` / 公网地址必须显式 `--allow-public`（启动警告）。
    /// ⚠️ 声明 PUT 的校验对照 `--schema`（真实 schema）：表 / 列必须存在；
    /// `main_only` 与 `publicDimensions` 必须带 `reason`；exclusions 的 `category`
    /// 要在枚举内且 `reason` / `decided_by` 非空。校验失败 ⇒ 400 + 点名，
    /// **原文件一个字节都不变**（校验在写之前）。
    /// ⚠️ `GET /api/export/ttl` / `POST /api/r2rml` 与 CLI 的产物**逐字节相同**。
    #[command(verbatim_doc_comment)]
    Serve {
        /// 监听地址。默认 `127.0.0.1:8899`；非回环地址必须加 `--allow-public`
        #[arg(long, default_value = "127.0.0.1:8899")]
        bind: String,
        /// 两套凭据的静态表（`admin-tokens.json`；示例见 `admin-tokens.example.json`）
        #[arg(long)]
        tokens: PathBuf,
        /// 本服务只服务这一个项目
        #[arg(long)]
        project: String,
        /// 真实 schema（`ontok-introspect` 产物）—— 声明校验的「表 / 列必须存在」只能来自它
        #[arg(long, default_value = crate::console::DEFAULT_SCHEMA)]
        schema: PathBuf,
        /// 排除声明（GET / PUT 的目标）
        #[arg(long, default_value = crate::console::DEFAULT_EXCLUSIONS)]
        exclusions: PathBuf,
        /// 租户声明（GET / PUT 的目标）
        #[arg(long, default_value = crate::console::DEFAULT_POLICY)]
        policy: PathBuf,
        /// 租户标注声明（GET / PUT 的目标）
        #[arg(long, default_value = crate::console::DEFAULT_TENANCY)]
        tenancy: PathBuf,
        /// 允许绑非回环地址（`0.0.0.0` / 公网）。⛔ 不加就只能绑回环
        #[arg(long)]
        allow_public: bool,
        /// 审计落点（追加写）。省略 = 只写 stderr；stderr **恒写**
        #[arg(long)]
        audit_out: Option<PathBuf>,
        /// IRI 基（必须与 `r2rml generate` / `ontology export` 用的是同一个）
        #[arg(long, default_value = "https://ontok.local/")]
        base: String,
    },
}

/// ⭐ REQ-126 · 客户门户（`ontok portal serve`）。
///
/// ⛔ 与 `console` / `query` 是三个独立服务：**各自一套凭据、各自一个端口**
/// （`kind` 互斥，见 `portal::tokens`）。
#[derive(Subcommand)]
pub enum PortalCmd {
    /// 起客户门户：预置问题卡片 + 结果解释（⛔ 没有自由查询、没有写操作）
    ///
    /// 端点（**只有四个**）：
    ///
    /// GET  /                                 页面壳（免 token；壳内**零数据**）
    /// GET  /api/questions                    问题列表（标题 + 说明；⛔ 不含 SPARQL）
    /// POST /q/<id>                           执行一个问题（租户只来自口令）
    /// GET  /api/health                       免 token，只给版本
    ///
    /// ⛔ 没有 `/api/sparql`（自由查询）、没有 `/api/catalog`、没有任何写操作、
    ///    不能切项目 / 版本（一个进程只服务一个已冻结版本）。
    /// ⚠️ `--tokens` 是**第三套凭据**：`kind` 必须是 `"portal"`，每张口令绑定一个租户；
    ///    admin / query 凭据不能当门户口令用（加载时整份拒绝）。
    /// ⚠️ 请求体 / URL 带 `tenant` ⇒ **400 / 403**（拒绝，不是忽略）；没有口令 ⇒ 401。
    /// ⚠️ `--questions` 在启动时**逐题校验**（fail-closed）：`reviewed_by` 为空 /
    ///    表没在 `--policy` 里登记 / 属性不存在 / 结果列是实例 IRI 或与真实列名同名
    ///    ⇒ 拒绝启动并点名那一条。
    /// ⚠️ 结果只给：业务对象名（不是表名）+ 覆盖警告（`main_only` 显著提示）+ 截断说明；
    ///    ⛔ 不返回 `sql_echo` / 表名 / 列名 / 属性 IRI。
    /// ⚠️ 默认只绑内网 / 本机；通配或公网地址必须 `--allow-public`（启动警告）。
    /// ⚠️ 审计：每次问题执行一行（口令只记摘要；⛔ 无 SQL、无查询原文）。
    #[command(verbatim_doc_comment)]
    Serve {
        /// 监听地址。默认 `127.0.0.1:8900`；非内网地址必须加 `--allow-public`
        #[arg(long, default_value = "127.0.0.1:8900")]
        bind: String,
        /// 门户口令表（`portal-tokens.json`；示例见 `portal-tokens.example.json`）
        ///
        /// 每条口令绑定一个租户，并可带两个限额：`max_rows`（默认 **200**，
        /// 上限 10000）= 单次问题最多几行；`max_concurrency`（默认 **2**，上限 64）
        /// = 同一口令同时最多几个问题在跑 —— 超限立刻 **429** `TOO_MANY_CONCURRENT`，
        /// ⛔ 不排队；不同口令之间互不影响。`kind` 必须是 `"portal"`。
        #[arg(long)]
        tokens: PathBuf,
        /// 本门户只服务这一个项目
        #[arg(long)]
        project: String,
        /// 第几版（该版必须**已冻结**）
        #[arg(long)]
        version: i32,
        /// 真实 schema（`ontok-introspect` 产物）—— 「变量名不与列名同名」只能来自它
        #[arg(long, default_value = crate::portal::DEFAULT_SCHEMA)]
        schema: PathBuf,
        /// 租户声明（`query-policy.json`）—— 问题涉及的表必须在这里登记
        #[arg(long, default_value = crate::portal::DEFAULT_POLICY)]
        policy: PathBuf,
        /// 预置问题声明（`portal-questions.json`）；启动时逐题校验
        #[arg(long, default_value = crate::portal::DEFAULT_QUESTIONS)]
        questions: PathBuf,
        /// 允许绑非内网地址（`0.0.0.0` / 公网）。⛔ 不加就只能绑内网 / 本机
        #[arg(long)]
        allow_public: bool,
        /// 审计落点（追加写）。省略 = 只写 stderr；stderr **恒写**
        #[arg(long)]
        audit_out: Option<PathBuf>,
        /// 单个问题的超时（毫秒）
        #[arg(long, default_value_t = crate::portal::DEFAULT_TIMEOUT_MS)]
        timeout_ms: u64,
        /// IRI 基（必须与 `r2rml generate` / `query serve` 用的是同一个）
        #[arg(long, default_value = "https://ontok.local/")]
        base: String,
        /// 把 `joinVerified "inherit"` 的边也生成 join（默认不生成，与 CLI 一致）
        #[arg(long)]
        include_inherit: bool,
    },
}

/// REQ-108 · 置信度相关命令（AC-011）。
#[derive(Subcommand)]
pub enum ConfidenceCmd {
    /// 输出**分箱命中率**（分数 vs 人工接受）
    ///
    /// 分箱：`0.00–0.30 / 0.30–0.60 / 0.60–0.75 / 0.75–0.90 / 0.90–1.00`。
    /// 人工决定取**业务轨**（`biz_status`）：关系对不对是业务判断，
    /// 建模轨判的是「建得规不规范」，不是同一件事。
    ///
    /// ⚠️ **任一分箱 < 5 条 ⇒ 整份报「样本不足」，不输出接受率结论、不算 ECE。**
    /// 空库运行同样明确提示 —— 不打印 `0%`（那是「一条都没接受」，
    /// 而事实是「一条都没判过」，两者完全不同）。
    Calibrate {
        /// 项目名（该项目的全部提案条目一起统计）
        #[arg(long)]
        project: String,
        /// 只统计挂在某个基版本上的提案？省略 = 该项目的全部提案
        #[arg(long)]
        version: Option<i32>,
    },
}

#[derive(Subcommand)]
pub enum ReviewCmd {
    /// 记录一次单轨判定
    Decide {
        #[arg(long)]
        proposal: String,
        /// 条目序号（1 起）。**与 `--accept-all` 互斥** ——
        /// 用 Option 是因为 clap 会把非 Option 参数视为「必需」，
        /// 那 `conflicts_with` 就没法让它缺席。
        #[arg(long)]
        item: Option<i32>,
        /// biz | model。与 `--accept-all` 互斥（同样因为 Option 才能缺席）。
        #[arg(long)]
        track: Option<String>,
        #[arg(long)]
        accept: bool,
        #[arg(long)]
        reject: bool,
        /// 项目名。**别名 `pN` 按项目内序号解析** —— 要审核别的项目请显式指定。
        /// 省略时作用域取「最近导入的提案所属项目」。
        #[arg(long)]
        project: Option<String>,
        /// 一次性接受**所有条目的两条轨**，不等价于逐条敲 —— 走的是同一条
        /// `set_decision` 路径与同一套领域校验。
        ///
        /// **为什么需要它**：REQ-003 从真实源库抽出 142 个条目，
        /// 逐条 `--item N` 要 284 次进程启动、每次重建连接池，
        /// E2E 根本跑不完（实测 15 分钟才走到一半就被环境中断）。
        #[arg(long, conflicts_with_all = ["item", "track", "accept", "reject"])]
        accept_all: bool,
    },
    /// 显示提案状态
    Show {
        #[arg(long)]
        proposal: String,
        /// 同 `decide`：别名按项目内序号解析
        #[arg(long)]
        project: Option<String>,
        /// REQ-108 · 一并输出**置信度分档表**（审核界面的置信度通道）。
        ///
        /// 刻意做成**开关**而不是默认输出：`review show` 的 stdout 字节
        /// 被 REQ-072/078 的真进程比对钉着（`review_show_prefix ++ 渲染文本`），
        /// 默认多打一行就会让那批判据红 —— 那不是「判据过时」，是**契约变了**。
        /// 需要看分数的审核者显式带上这个开关；客户视图与此无关（它压根不读）。
        #[arg(long)]
        confidence: bool,
    },
    /// ⭐ REQ-111 · 起一个**本地审核界面**（在 HTML 里做双轨审核）
    ///
    /// 与 `serve` 是**两个命令**，不是给 `serve` 加一个开关：
    ///
    /// | `serve` | `review serve` |
    /// | --- | --- |
    /// | 只读：任何非 GET ⇒ 405 | 可写：**唯一**写点是 `POST /api/decide` |
    /// | 只服务**已冻结**版本 | 只服务**未冻结**的提案（已冻结 ⇒ 拒绝启动） |
    ///
    /// 加开关会让「`serve` 是只读的」从**结构保证**退化成**配置保证** ——
    /// 那正是这个仓库反复栽的坑（REQ-093 的硬判据是「没有可写的地方」）。
    ///
    /// 【可以照抄的一整条】
    ///
    /// ontok review serve --project demo3 --proposal p1 --bind 127.0.0.1:8788 --token '换成你自己的口令'
    ///
    /// 然后浏览器打开 http://127.0.0.1:8788/
    ///
    /// ⚠️ **只绑回环**：`--bind 0.0.0.0:8788` 直接拒绝启动（exit≠0）。
    /// ⚠️ 口令也可以从环境变量 `ONTOK_VIEW_TOKEN` 读；两处都没有 ⇒ 拒绝启动。
    /// ⚠️ 页面壳（`/`）免口令、**不含任何数据**；`/api/*` 全部要 Bearer 口令。
    #[command(verbatim_doc_comment)]
    Serve {
        /// 项目名
        #[arg(long)]
        project: String,
        /// 提案（`p1` = 该项目内**最近导入**的那个；也可直接给 UUID）
        #[arg(long)]
        proposal: String,
        /// 监听地址。**只允许回环地址**（如 127.0.0.1:8788）
        #[arg(long, default_value = "127.0.0.1:8788")]
        bind: String,
        /// 访问口令（至少 8 个字符）。省略时读环境变量 `ONTOK_VIEW_TOKEN`
        #[arg(long)]
        token: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum ProposalCmd {
    /// 应用提案，产生新版本
    Apply {
        #[arg(long)]
        proposal: String,
        /// 同 `review decide`：别名按项目内序号解析
        #[arg(long)]
        project: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum OntologyCmd {
    /// 显示本体（默认客户视图）
    Show {
        #[arg(long)]
        project: String,
        #[arg(long)]
        version: i32,
        /// 输出建模视图（含 key / 证据），供 FDE 用
        #[arg(long)]
        model_view: bool,
        /// 输出**自包含单文件 HTML**（判读用，零外部依赖、可离线双击打开）
        #[arg(long)]
        html: bool,
        /// HTML 输出路径。省略则打到标准输出
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// 抽样高亮清单（每行一个对象名）。省略用判读手册 §3.1 的 13 个
        #[arg(long)]
        sampling: Option<PathBuf>,
    },
    /// ⭐ REQ-110 · **导出 TTL（OWL Turtle）** —— 产品自己就能导出，不再靠外部脚本。
    ///
    /// 默认导出**客户视图**（页面看到什么就导出什么，R1）；
    /// 带 `--model-view` 导出**建模视图**（含 key / 列名 / 基数 / 语义，供 FDE 用）。
    ///
    /// ⭐ REQ-119 · 两版都带：反向属性（`owl:inverseOf` 双向 + domain/range 对调）、
    /// xsd range（`xsd:decimal` / `integer` / `dateTime` / `date` / `boolean` / `string`）、
    /// 每类一条 `ontok:instanceIri` 模板。`--tenancy <file>` 的租户标注**只在建模视图**；
    /// 未标注时导出仍成功，但文件头 + `ontok:tenantUnannotated` + stderr 三处警告。
    ///
    /// 【可以照抄的一整条】
    ///
    /// ontok ontology export --project demo3 --version 2 --out demo3-v2.ttl
    ///
    /// 同一版本连续导出两次**逐字节相同**（不写生成时间）；客户视图导出**过守门器**，
    /// 命中建模术语时以退出码 2 拒绝（词条只进 stderr）。文件头显式声明有损项
    /// （ActionType 未导出 / semantic 降级为注释 / cardinality 只映射两种特征）。
    Export {
        /// 项目名
        #[arg(long)]
        project: String,
        /// 第几版
        #[arg(long)]
        version: i32,
        /// 导出**建模视图**（默认是客户视图）
        #[arg(long)]
        model_view: bool,
        /// 输出路径。省略则打到标准输出
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// IRI 基（本体 IRI = `<base><project>`，前缀 = `<base><project>#`）
        #[arg(long, default_value = "https://ontok.local/")]
        base: String,
        /// 导出格式。今天只有 `ttl`（Turtle）；写别的值**直接报错**，不静默回退
        #[arg(long, default_value = "ttl")]
        format: String,
        /// ⭐ REQ-119 · 租户声明文件（JSON：`{columns, tables?, reviewed_by}`）。
        ///
        /// **只在 `--model-view` 生效**：命中列标 `ontok:tenantKey`、其类标
        /// `ontok:tenantColumn`。客户视图结构上不含租户标注（R4）。
        /// 省略 ⇒ 导出仍成功，但三处标记「未标注」（文件头 / `ontok:tenantUnannotated` /
        /// stderr 警告）。`reviewed_by` 为空 ⇒ 视为**未签**，不落标注。
        #[arg(long)]
        tenancy: Option<PathBuf>,
        /// ⭐ REQ-119 A-1 · **真实列类型**（`ontok-introspect` 产出的 schema JSON）。
        ///
        /// 按「实体 key = 表名 + 属性列名」查 `data_type`：命中即以真实类型为准，
        /// 属性标 `ontok:rangeSource "schema"`；未命中/未给 ⇒ 启发式 + `"heuristic"`，
        /// 文件头与 stderr 会**明说**类型来源。给了本参数后建模视图应当 0 条启发式。
        #[arg(long)]
        schema: Option<PathBuf>,
    },
    /// 比较两个版本
    Diff {
        #[arg(long)]
        project: String,
        a: i32,
        b: i32,
    },    /// REQ-010 · **业务总览**（按概念出总览；一个组都没有时退化成对象清单）
    Overview {
        #[arg(long)]
        project: String,
        #[arg(long)]
        version: i32,
        /// 输出 JSON（默认给人读的是文本）
        #[arg(long)]
        json: bool,
    },
}

/// 装一个忽略 EPIPE 的 panic hook。
///
/// **为什么必须装**：`println!` 遇到 broken pipe 会 panic，
/// 而那个 panic 发生在**子函数里的 println**，`run()` 的 `Result` 收不到 ——
/// 只在 `run` 里 try/catch 是抓不住的。唯一能统一处理的地方是 panic hook。
///
/// 场景：`ontok ontology diff ... | head -2`。
/// 用户只想看前几行，却看到一屏 std::io 栈，会误判「程序有 bug」。
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

// ══════════════════════════════════════════════════════════════
// REQ-078 · 二进制层差分守卫的**生产侧接线**
// ══════════════════════════════════════════════════════════════
//
// # 这段代码在补哪一格
//
// REQ-072 的 QA 用变异 **M2** 证明了一件事：把 `review show` 里那行
// `print!("{text}")` 改成打印前做一次 `str::replace`
// （**domain 层一行未改**），实测：
//
// ```text
// 全量 53 个测试二进制：passed=738 / failed=4 / ignored=1
// 退出码实测 = 0
// stderr 完全为空（三道自检一声不吭）
// ```
//
// 根因**不是疏忽，是接线方式的必然**：判据
// （`verify_track_separation` / `verify_provenance_scope` / `audit_fde_channel`）
// 收的是 **domain 产物**，而打印出去的可以是它的**再加工**。
// 两者不是同一个对象 ⇒ 打印点的任何加工，对自检**结构性不可见**。
//
// ⚠️ **不承诺封堵这一类**（AC 的范围判断）。能做到的是**让它可见**：
//
// 1. [`emit_domain_text_with`] —— domain 产物通往 stdout 的**唯一**出口。
//    它写出字节，并把**同一个切片**交给判据闭包；
//    闭包**在签名上只拿得到这一个切片**。
//    ⚠️ REQ-080 F-1 实测修订：「插不进再加工」这句**不成立** —— Rust 闭包会
//    **捕获环境里的绑定**，所以
//    `emit_domain_text_with(&text.replace(..), |judged| …verify(&rv, &text))`
//    写得出来、编译得过（写出去的已加工、判据读未加工）⇒ 真 M2（M2d）。
//    那一档由 `tests/test_binary_output_guard.rs` 的 **G-1** 判红，
//    **不由** [`EXIT_POLICY`] 判红（那一档实测退出码是 **0**）。
// 2. [`DOMAIN_PRINT_SITES`] —— **显式登记表**：每一处「把 domain 产物写进
//    stdout」的站点连同它的前缀都在册。⛔ 漏登记即红
//    （`tests/test_binary_output_guard.rs` 的 G-2/G-5）。
// 3. [`EXIT_POLICY`] —— 退出码政策**写死成文字**，并由 [`self_check_exit_code`] 落地。

// ───────────────────── 退出码政策（AC-004）─────────────────────

/// 成功。
pub const EXIT_OK: i32 = 0;
/// 操作性错误：参数、连库、找不到提案/版本（`main.rs` 的 `Err` 分支）。
pub const EXIT_OPERATION_FAILED: i32 = 1;
/// 守门器判定**泄露建模术语** —— fail-closed，拒绝出图（既有码，见 [`ontology_cmd`]）。
pub const EXIT_LEAKED_MODELING_TERMS: i32 = 2;
/// ⭐ REQ-078 新增：**自检违规** —— 三道自检**从它们读到的那份串上**报出了东西。
///
/// ⚠️ REQ-080 F-1 如实修订：REQ-078 的注释写的是「渲染被再加工」，那一半
/// **实测不成立** —— 真 M2（M2d：闭包捕获外层绑定，判据读未加工那份）下
/// 三道自检**合法沉默**，退出码实测 **0**。
/// ⇒ 3 只承载「自检报了」，**不承载**「渲染被再加工」。
pub const EXIT_SELF_CHECK_VIOLATION: i32 = 3;

/// REQ-078 · 退出码政策的**唯一事实源**（文字 + 码）。
///
/// | 码 | 含义 |
/// | --- | --- |
/// | [`EXIT_OK`] | 1 / 2 / 3 都没触发；三道自检**从它们读到的那份串上**没报出违规 |
/// | [`EXIT_OPERATION_FAILED`] | 操作性错误（没进到渲染这一步） |
/// | [`EXIT_LEAKED_MODELING_TERMS`] | 守门器判定泄露建模术语 ⇒ 拒绝出图 |
/// | [`EXIT_SELF_CHECK_VIOLATION`] | 三道自检**报出了东西** |
///
/// # ⚠️ 0 **不承载**「stdout 逐字节 = 前缀 ++ 渲染」（REQ-080 F-1 实测修订）
///
/// REQ-078 那一版把 0 写成「成功，且 stdout **逐字节 = 已登记前缀 ++ domain 渲染**」。
/// 那句话在**真 M2 下是假的**，实测（REQ-080 QA，M2d）：
///
/// ```text
/// emit_domain_text_with(&text.replace('\u{240A}', "\n"), |judged| {
///     verify_track_separation(&rv, &text)   // ← 闭包**捕获**外层 `text`
/// })
/// ⇒ 写出去的已加工、判据读未加工 ⇒ 三道自检**合法沉默**
/// ⇒ self_check_exit_code(&[]) == None ⇒ 退出码 **0**，而 stdout 已被攻破
/// ```
///
/// ⇒ 「stdout == 渲染」那一格**不由退出码守**，它由
/// `tests/test_binary_output_guard.rs` 的 **G-1**（源码级接线守卫）与
/// `tests/test_domain_print_sites_binary_req078.rs` 的**真进程字节比对**守。
/// 退出码 0 是下游**最容易被当证据**的那个信号 ⇒ 它必须先说自己不承载什么。
///
/// ⛔ **拿「退出码 0」当渲染正确性的证据，在本政策下依然不成立** ——
/// REQ-072 QA 在 M2 下测到 0；REQ-080 QA 在真 M2 下复现了同一形状。
pub const EXIT_POLICY: &str = "\
退出码政策（REQ-078 AC-004；0 号码那一行由 REQ-080 F-1 如实修订）：
  0 = 1 / 2 / 3 都没触发：三道自检从**它们读到的那份串**上没报出违规
  1 = 操作性错误（参数 / 连库 / 找不到提案或版本）
  2 = 守门器判定泄露建模术语 ⇒ fail-closed，拒绝出图
  3 = 三道自检报出了东西
⛔ 0 **不承载**「stdout 逐字节 = 前缀 ++ 渲染」—— 真 M2（`emit_domain_text_with(&text.replace(..), |judged| …verify(&rv, &text))`，
   闭包捕获外层绑定）下三道自检合法沉默 ⇒ 退出码实测 **0**，而 stdout 已被攻破（REQ-080 F-1 实测）。
   ⇒ 「stdout == 渲染」由源码守卫 G-1 与真进程字节比对守，**不由退出码守**。
⛔ 拿退出码 0 当「渲染没问题」的证据，在本政策下**依然不成立**。";

/// 自检有违规 ⇒ 该退出什么码；没有 ⇒ `None`（照常 0）。
///
/// **纯函数**：没有违规就返回 `None`，**不**自己调 `exit` ——
/// 「判出来」与「退出去」是两件事，判据才可被直接断言（`tests/test_binary_output_guard.rs`）。
pub fn self_check_exit_code(violations: &[String]) -> Option<i32> {
    if violations.is_empty() {
        None
    } else {
        Some(EXIT_SELF_CHECK_VIOLATION)
    }
}

// ───────────────── 打印点登记表（AC-002 / AC-005）────────────────

/// 一个「把 domain 产物写进 stdout」的**登记项**。
///
/// # 为什么要一张表而不是散在各处的常量
///
/// REQ-072 的做法是把抬头行硬写进测试里的 `format!`
/// （`format!("提案 {alias} 状态={status}\n{domain_text}")`）——
/// 生产侧改抬头、测试侧忘了改 ⇒ **假红**；反过来测试先改 ⇒ **假绿**。
/// 常量散落在各处就会漂移，这是本会话反复出现的病。
/// ⇒ 抬头原文的**唯一**事实在 [`REVIEW_SHOW_PREFIX_TEMPLATE`]，
/// 生产与测试都从它派生前缀。
#[derive(Debug, Clone, Copy)]
pub struct DomainPrintSite {
    /// 站点所在的函数名 —— 同名绑定（如两个 `text`）靠它区分。
    pub enclosing_fn: &'static str,
    /// clap 子命令路径（人读的，判据只拿它做报错定位）。
    pub command: &'static str,
    /// 站点在 `src/cli.rs` 里把产物**写出去的那个表达式**。
    pub binding: &'static str,
    /// 允许**先写**的前缀（模板；`{alias}` / `{status}` 由 [`review_show_prefix`] 替换）。
    pub prefix_template: &'static str,
    /// 是否在产物**之后**补一个换行（`println!` 语义；`print!` 语义为 `false`）。
    pub trailing_newline: bool,
    /// ⭐ 这个站点的「同一份」靠**哪一种接线**保证 —— AC-001 的登记表版本。
    ///
    /// | 值 | 含义 | 守卫断言 |
    /// | --- | --- | --- |
    /// | [`WIRING_CLOSURE`] | 走 `emit_domain_text_with(text, \|judged\| …)`：**类型**保证 —— 闭包拿不到别的串 | 闭包体内不得再出现 `binding`；判据实参 == 闭包参数 |
    /// | [`WIRING_SAME_BINDING`] | 判据与写出读**同一个绑定**（先判后写，fail-closed） | 判据的文本实参与写出表达式**逐词元相等** |
    /// | [`WIRING_NONE`] | 刻意不过判据 | 源码里不得出现该站点的判据调用 |
    pub wiring: &'static str,
    /// 判据（守门器 / 自检）名 —— 它们必须收到**与本绑定同一份**字节。
    /// 空 = 该站点**刻意不过**判据（豁免已在 `cli.rs` 里写明理由）。
    pub judged_by: &'static [&'static str],
    /// 判**投影**（结构化数据）而不是判**渲染文本**的判据。
    ///
    /// ⚠️ 刻意与 [`Self::judged_by`] 分开：`audit_fde_channel` 读的是
    /// `ModelFacing` 的溯源（REQ-057 修的就是这一点），
    /// **它本来就不该收到渲染串** —— 把 `text` 喂给它反而是把它变成别的判据。
    /// 列在这里是为了让「哪些判据收文本、哪些收投影」成为**可读的结论**，
    /// 而不是靠读者从代码里推。
    pub projection_judged_by: &'static [&'static str],
    /// 有没有**真进程**端到端断言 `stdout == 前缀 ++ 产物 ++ 尾换行`。
    pub e2e_covered: bool,
    /// 覆盖状态的人读说明（未覆盖时**必须**写清为什么）。
    pub note: &'static str,
}

/// 接线形态：`emit_domain_text_with` 的闭包（**类型**保证）。
pub const WIRING_CLOSURE: &str = "closure";
/// 接线形态：判据与写出读**同一个绑定**（先判后写、fail-closed）。
pub const WIRING_SAME_BINDING: &str = "same_binding";
/// 接线形态：刻意不过判据。
pub const WIRING_NONE: &str = "none";
/// ⭐ REQ-107 · 接线形态：写出的是**管线产出**的包装类型（`OutletRender`）。
///
/// 与前两种的区别是**判据的位置**：判据不再在出口里、也不再是「先判后写」两条语句，
/// 而是**在管线内**执行一次，出口拿到的只能是过了闸的产物（类型层保证）。
///
/// ⇒ 判据（`check_site`）对这一族要问的不是「判据收到了什么」，
/// 而是「写出去的绑定是不是从管线产物派生的」「同一函数里还有没有判据直调」。
pub const WIRING_PIPELINE: &str = "pipeline";

/// `review show` 抬头前缀的**模板** —— 前缀原文的唯一事实源。
pub const REVIEW_SHOW_PREFIX_TEMPLATE: &str = "提案 {alias} 状态={status}\n";

/// `review show` 的抬头 —— 生产侧与端到端断言**共用**这一个函数。
pub fn review_show_prefix(alias: &str, status: &str) -> String {
    REVIEW_SHOW_PREFIX_TEMPLATE
        .replace("{alias}", alias)
        .replace("{status}", status)
}

/// **置信度分档表**（REQ-108 · AC-010 的「审核界面必须有」）。
///
/// 输入是提案条目行（`payload.confidence` 里带着分数），输出一段**纯文本**：
/// 抬头 + 四档计数 + 每条关系一行（档位、分数、关系、证据摘要、veto 理由）。
///
/// # 为什么单独一段、不并进 `review_view`
///
/// `src/domain/review_view.rs` 的渲染链是 REQ-056~078 的**字节契约**所在地，
/// 本 REQ 的红线明确禁改它。而分数是审核辅助信息、不是条目正文 ——
/// 单独一段既满足「审核界面看得见」，又不动那条被钉住的输出。
///
/// # 排序是确定的
///
/// 档位（高→拒绝）→ 分数降序 → 条目 target 升序。同一份提案两次运行
/// 输出逐字节相同（AC-005 的口径也适用于这一段）。
pub fn render_confidence_table(items: &[crate::db::proposal::ProposalItemRow]) -> String {
    use crate::domain::confidence::Band;

    #[derive(Clone)]
    struct Row {
        band: Band,
        score: f64,
        method: String,
        target: String,
        evidence: Option<(u32, u32)>,
        veto: Option<String>,
        /// REQ-109 · AI 语义判断的那一行（`fit` / `model` / `cache_hit` / `tokens`）。
        /// `None` = 这条关系**没被 AI 判断过**（AI 关 / 判失败），不打印 ——
        /// 「没判」不冒充「判了 0 分」。
        ai: Option<String>,
    }
    let mut rows: Vec<Row> = Vec::new();
    for it in items {
        let Some(v) = it.payload.get("confidence") else { continue };
        let Ok(c) = crate::domain::confidence::from_untrusted(v) else { continue };
        let ev = c
            .signals
            .iter()
            .find(|s| s.name == "sample_hit_rate")
            .and_then(|s| Some((s.sampled?, s.matched?)));
        // REQ-109 · 语义分与溯源一起打：只有分数没有溯源，审核人无从复核；
        // 只有溯源没有分数，看不出 AI 到底说了什么。
        let ai = c.ai.as_ref().map(|a| {
            let fit = c
                .signals
                .iter()
                .find(|s| s.name == crate::domain::confidence::SIGNAL_SEMANTIC_FIT)
                .map(|s| format!("{:.2}", s.value))
                .unwrap_or_else(|| "—".to_string());
            let why = a
                .rationale
                .as_deref()
                .map(|r| format!("「{r}」"))
                .unwrap_or_default();
            format!(
                "ai {} · fit {} · cache_hit {} · tokens {} {}",
                a.model, fit, a.cache_hit, a.tokens, why
            )
        });
        rows.push(Row {
            band: c.band,
            score: c.score,
            method: c.method.clone(),
            target: it.target_key.clone(),
            evidence: ev,
            veto: c.veto.as_ref().map(|v| format!("{}：{}", v.signal, v.reason)),
            ai,
        });
    }
    if rows.is_empty() {
        return "置信度：本提案没有分数（REQ-108 之前的提案 / 人工提案）".to_string();
    }
    rows.sort_by(|a, b| {
        a.band
            .cmp(&b.band)
            .then_with(|| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal))
            .then_with(|| a.target.cmp(&b.target))
    });
    let counts = crate::domain::confidence::BandCounts::of(rows.iter().map(|r| &r.band));
    // 抬头写**这一份提案实际用的方法**。AI 关时仍逐个字是 `evidence-scorecard-v1`
    // （REQ-108 的字节契约），AI 开时是 v2 —— 不写死，免得报告说假话。
    let mut methods: Vec<&str> = rows.iter().map(|r| r.method.as_str()).collect();
    methods.sort_unstable();
    methods.dedup();
    let method_label = methods.join(" / ");
    let mut out = String::new();
    out.push_str(&format!(
        "置信度（{}）：高 {} · 中 {} · 低 {} · 拒绝 {}（共 {} 条关系）\n",
        method_label,
        counts.high,
        counts.middle,
        counts.low,
        counts.rejected,
        counts.total()
    ));
    for r in &rows {
        let ev = match r.evidence {
            Some((s, m)) => format!(" · 证据 {m}/{s}"),
            None => " · 证据 无（data_evidence=none）".to_string(),
        };
        let veto = r.veto.as_deref().map(|v| format!("\n             veto {v}")).unwrap_or_default();
        let ai = r.ai.as_deref().map(|a| format!("\n             {a}")).unwrap_or_default();
        out.push_str(&format!(
            "  [{}] {:.2} {}{}{}{}\n",
            r.band.label_zh(),
            r.score,
            r.target,
            ev,
            veto,
            ai
        ));
    }
    out.trim_end().to_string()
}

/// ⭐ REQ-078 · **打印点登记表** —— 全部打印 domain 产物的站点。
///
/// ⛔ **新增 `print!` 站点必须登记**：漏登记即红（`G-2`），
/// 登记表与源码不一致即红（`G-5`）。
pub const DOMAIN_PRINT_SITES: &[DomainPrintSite] = &[
    DomainPrintSite {
        enclosing_fn: "review_cmd",
        command: "review show",
        wiring: WIRING_CLOSURE,
        binding: "text",
        prefix_template: REVIEW_SHOW_PREFIX_TEMPLATE,
        trailing_newline: false,
        judged_by: &["verify_track_separation", "verify_provenance_scope"],
        // ⚠️ REQ-057：这条判据读的是**结构化溯源**，不是渲染文本 ——
        // 把它挪进 `judged_by` 会逼它收 `text`，那正是 REQ-057 判为恒真的那种写法。
        projection_judged_by: &["audit_fde_channel"],
        e2e_covered: true,
        note: "REQ-072 的真进程字节比对 + REQ-078 的登记表前缀。",
    },
    DomainPrintSite {
        enclosing_fn: "ontology_cmd",
        command: "ontology show --model-view",
        wiring: WIRING_NONE,
        binding: "mv_json",
        prefix_template: "",
        trailing_newline: true,
        // 刻意不过判据：建模术语在这一轨**是要被裁决的内容**（`cli.rs` 的注释原话）。
        judged_by: &[],
        projection_judged_by: &[],
        e2e_covered: true,
        note: "REQ-078 起由真进程字节比对覆盖；豁免判据的理由写在 cli.rs。",
    },
    DomainPrintSite {
        enclosing_fn: "ontology_cmd",
        command: "ontology show --html",
        wiring: WIRING_PIPELINE,
        binding: "html",
        prefix_template: "",
        trailing_newline: false,
        // ⭐ REQ-107：判据在 `domain::pipeline` 里执行一次，出口只拿到过了闸的产物
        // （`OutletRender`，字段私有、无公开构造器）⇒ 出口层不再有判据调用。
        judged_by: &[],
        projection_judged_by: &[],
        e2e_covered: true,
        note: "REQ-078 起由真进程字节比对覆盖（stdout 分支；--out 分支写同一份字节）。\
         ⭐ REQ-107 起改为 `pipeline` 接线：判据在管线内、出口只写管线产物 —— \
         「判据收到的 == 打印出去的」由**类型**保证（`OutletRender` 造不出来），\
         不再靠「两条语句读同一个绑定」这条约定。独立第二道防线仍是真进程字节比对。",
    },
    DomainPrintSite {
        enclosing_fn: "ontology_cmd",
        // ⚠️ 命令名就叫这个、不叫 `--json`：`ontology show` **没有** `--json` 标志，
        //    不带 `--html` / `--model-view` 时默认路径就直接打 JSON。
        command: "ontology show（默认 JSON）",
        wiring: WIRING_PIPELINE,
        binding: "json",
        prefix_template: "",
        trailing_newline: true,
        judged_by: &[],
        projection_judged_by: &[],
        e2e_covered: true,
        note: "REQ-078 起由真进程字节比对覆盖。\
         ⭐ REQ-107 起改为 `pipeline` 接线：剔除（`filter_view`）与发布闸都在管线内，\
         出口拿到的只能是过了闸的产物 ⇒ 「先判后写」不再是一条需要人记得的时序，\
         而是一条**类型**（REQ-106 AC-001 的意图由结构承接）。\
         独立第二道防线仍是真进程字节比对。",
    },
    DomainPrintSite {
        enclosing_fn: "ontology_cmd",
        command: "overview",
        wiring: WIRING_PIPELINE,
        binding: "text",
        prefix_template: "",
        trailing_newline: false,
        judged_by: &[],
        projection_judged_by: &[],
        e2e_covered: true,
        note: "REQ-078 起由真进程字节比对覆盖。\
         ⭐ REQ-107 起改为 `pipeline` 接线（`Product::Overview`）：文本与 JSON 两条形态\
         **都**在管线内过闸（保留收口前的过度拦截，AC-005 原样不动），出口只写其中一条。",
    },
    DomainPrintSite {
        enclosing_fn: "ontology_cmd",
        command: "overview --json",
        wiring: WIRING_PIPELINE,
        binding: "json_text",
        prefix_template: "",
        trailing_newline: true,
        judged_by: &[],
        projection_judged_by: &[],
        e2e_covered: true,
        note: "REQ-078 起由真进程字节比对覆盖。\
         ⭐ REQ-107 起改为 `pipeline` 接线（`Product::Overview`）：与 `overview` 共用同一份\
         产物与同一道闸，只有写出分支不同。",
    },
    DomainPrintSite {
        enclosing_fn: "proposal_cmd",
        command: "proposal apply（未通过清单）",
        wiring: WIRING_NONE,
        binding: "b.target",
        prefix_template: "  被拦：",
        trailing_newline: true,
        judged_by: &[],
        projection_judged_by: &[],
        // ⛔ 如实登记为**未覆盖**：它逐行打 `summarize` 的阻塞项，
        // 无下游判据接线 ⇒ 真进程字节比对对它不构成防线。
        //
        // ⚠️ REQ-080 F-3：REQ-078 的 note 写「无载荷面」，**与代码不符** ——
        // `summarize` 的 `ItemVerdict.target` 是 `ItemView.target` 的克隆
        // （`src/domain/review.rs:218`），原样来自提案 payload，**未经
        // `escape_payload_text`** ⇒ payload 里带真换行就能在 stdout 上凭空
        // 多出一行 `  被拦：…`。**载荷面=有**（证据见 G-6 的运行时实测）。
        e2e_covered: false,
        note: "未覆盖（无真进程字节比对断言）：**有**文本载荷面 —— \
         `summarize` 的 `ItemVerdict.target` 来自提案 payload、未经 escape_payload_text；\
         且无判据接线。载荷面=有",
    },
    DomainPrintSite {
        enclosing_fn: "ontology_cmd",
        command: "ontology diff",
        wiring: WIRING_NONE,
        binding: "x.summary()",
        prefix_template: "  ",
        trailing_newline: true,
        judged_by: &[],
        projection_judged_by: &[],
        // ⛔ 如实登记为**未覆盖**：这一站逐条打 `Change::summary()`，
        // 无下游判据 ⇒ 真进程字节比对对本条**不构成防线**。
        //
        // ⚠️ REQ-080 F-3：REQ-078 的 note 写「无文本载荷面」，**与代码不符** ——
        // `DiffItem.target` 是快照里的实体/关系 key、`before`/`after` 是**原始
        // JSON 值**，而 `diff.rs` 的 `render()` 对 `Value::String` 是**直接
        // `clone()`**（`src/domain/diff.rs:74`）⇒ 一律**未经
        // `escape_payload_text`**，真换行能原样流进 stdout。
        // **载荷面=有**（证据见 G-6 的运行时实测）。
        //
        // 登记它是为了「漏登记即红」这条机制能覆盖到这里，而不是把它算进已覆盖。
        e2e_covered: false,
        note: "未覆盖（无真进程字节比对断言）：**有**文本载荷面 —— \
         `DiffItem.target`（快照 key）与 `before`/`after`（原始 JSON，字符串直接 clone）\
         均未经 escape_payload_text；且无判据接线。载荷面=有",
    },
    // ⭐ REQ-121 · `r2rml generate` 的 stdout（内部轨：表名 / 列名 / join 证据）。
    //
    // 为什么登记：它把 domain 产物（R2RML Turtle）逐字节写进 stdout —— 不登记就是
    // 「新增写点没人管」（REQ-107 收口的那个洞）。为什么**不是** WIRING_PIPELINE：
    // 内部轨不过客户发布闸（`spec_of(OutletId::I11)` 是 `Track::Internal`），
    // 走 `publish_customer` 反而会让 T8 把它算成一条「没人认领的闸调用」。
    DomainPrintSite {
        enclosing_fn: "r2rml_cmd",
        command: "r2rml generate",
        wiring: WIRING_NONE,
        binding: "r2rml_doc",
        prefix_template: "",
        trailing_newline: false,
        judged_by: &[],
        projection_judged_by: &[],
        // ⛔ 未覆盖：本命令要连库 + 要 `--schema`，真进程字节比对需要一套
        // 专属 schema 夹具；本轮把「两次生成逐字节相同」交给 `tests/test_req121_r2rml.rs`
        // 的纯函数判据 + 手工 sha256（见交付物）。
        //
        // ⚠️ 载荷面=有：表名 / 列名 / `ontok:joinEvidence` 原文都来自快照，
        // 其中证据串原样进注释行（`one_line` 只压换行，不改内容）。
        e2e_covered: false,
        note: "未覆盖（无真进程字节比对断言）：R2RML Turtle 里含表名 / 列名 / \
         join 证据原文（快照载荷经 one_line 后原样进注释行与 ontok:joinEvidence）—— \
         载荷面=有；内部轨，只登记在 OUTLETS 的 I11/I12。",
    },
];

// ───────────────────── 唯一的出口（AC-001）─────────────────────

/// 把**已过前处理与发布闸**的客户产物逐字节写进 stdout（`print!` 语义，**不加**尾换行）。
///
/// ⭐ REQ-107 第 2 轮（QA 阻断 B3 / A11 R-D）：
/// 实参类型是 [`OutletRender`] —— **拿裸 `&str` 调它编译不过**。
/// 于是「出口先把手拼的未过闸字节写出去、再去调管线判定」（REQ-106 的原始病）
/// 在出口层没有表达形式：写出去的字节只能来自管线的返回值。
///
/// ⛔ 刻意**不返回**任何派生值。
/// 建模轨（D2）的闭包式接线路过 `emit_domain_text_with(&str, …)`，见那里。
pub fn emit_domain_text(rendered: crate::domain::pipeline::OutletRender) {
    write_stdout(rendered.as_str(), false);
}

/// 同 [`emit_domain_text`]，但补一个尾换行（`println!` 语义）。
pub fn emit_domain_line(rendered: crate::domain::pipeline::OutletRender) {
    write_stdout(rendered.as_str(), true);
}

/// ⭐ REQ-078 · **判据收到的串 == 打印出去的串**，由**类型**保证。
///
/// 1. 把 `text` **逐字节**写进 stdout；
/// 2. 把**同一个切片**交给 `judge`。
///
/// # 为什么是闭包而不是「先打印、再把 `&text` 喂判据」
///
/// 旧写法（REQ-072 QA 的 M2 就是打在这儿）：
///
/// ```text
/// print!("{text}");                                  // 打印点
/// for v in verify_track_separation(&rv, &text) { … }  // 判据点：另一个 `&text`
/// ```
///
/// 两者是**两个独立取值点** —— 明天有人在打印点插一次 `str::replace`，
/// 判据点纹丝不动，**任何自检都看不见**（QA 实测：全仓 738 条全绿、
/// 退出码 0、stderr 全空）。
///
/// 这里的闭包**在签名上就拿不到别的串**：它只有 `text` 这一个 `&str` 可用。
/// 于是「判据看到了打印点之外的加工」这件事**表达不出来**。
/// （残余风险：闭包体里**引用外层同名绑定**而不是参数 —— 那一格由
/// `tests/test_binary_output_guard.rs` 的 **G-1** 源码守卫钉住。）
pub fn emit_domain_text_with<F, R>(text: &str, judge: F) -> R
where
    F: FnOnce(&str) -> R,
{
    write_stdout(text, false);
    judge(text)
}

/// 写出本体。**不静默吞失败**：写不出去要么是下游关管道（`main.rs` 负责退出 0），
/// 要么是实打实的 IO 失败 —— 后者按操作性错误 fail-closed。
///
/// ⚠️ 这里**刻意不**返回 `Result`：渲染产物已经算出来了，
/// 「写不出去」不是一个能被调用方优雅处理的分支，静默吞掉才是本仓库
/// 反复出现的那种病（F-3 同源）。
fn write_stdout(text: &str, newline: bool) {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    let r = out
        .write_all(text.as_bytes())
        .and_then(|()| if newline { out.write_all(b"\n") } else { Ok(()) })
        .and_then(|()| out.flush());
    match r {
        Ok(()) => {}
        // 下游关闭（`ontok ... | head`）：`main.rs` 末尾会 flush 失败并退出 0。
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {}
        Err(e) => {
            eprintln!("✗ 渲染产物没能写进 stdout：{e}");
            std::process::exit(EXIT_OPERATION_FAILED);
        }
    }
}

/// ⭐ REQ-119 · 把管线挂在产物上的 range 分布打到 stderr。
///
/// 分布是序列化器**从最终正文数出来的**（`TtlDocument::stats`），出口这里只读，
/// 不碰产物字节、也不重算 —— 客户视图在序列化前有属性级剔除，按视图重算会多算。
pub(crate) fn eprint_ttl_stats(
    track: &str,
    stats: Option<&crate::domain::ttl::RangeStats>,
    schema_given: bool,
) {
    let Some(stats) = stats else {
        return;
    };
    eprintln!(
        "· xsd range（{track}）：{}（数值列 decimal+integer = {}）",
        stats.summary(),
        stats.numeric()
    );
    if !schema_given {
        eprintln!("⚠ xsd 类型来源：未读真实列类型，全部来自启发式（--schema <schema.json> 可消除）");
    } else if stats.source("heuristic") > 0 {
        let list = if stats.heuristic.is_empty() {
            "（清单为空）".to_string()
        } else {
            stats.heuristic.join("、")
        };
        eprintln!(
            "⚠ --schema 未匹配/类型认不出 {} 个属性，已回退启发式：{list}",
            stats.source("heuristic")
        );
    }
}

/// ⭐ REQ-107 · 客户类产物的**发布入口**（CLI 侧的唯一一处）。
///
/// # 为什么它不在 `ontology_cmd` 体内
///
/// 「拦不拦」是**判定**，在 `domain::pipeline` 里做；「命中后怎么办」是**政策**
/// （stderr 报词条 + 退出码 `EXIT_LEAKED_MODELING_TERMS`，REQ-078 AC-004）。
/// 两者分开，是为了让出口的函数体里**一条判红分支都没有** ——
/// REQ-107 的 AC-002 直接扫出口函数体（`EXIT_LEAKED_MODELING_TERMS` /
/// `std::process::exit(` / `if !leaked.is_empty()` 一律 0 命中）。
///
/// # 判据只**调用**、不复制
///
/// 本函数不含任何判据：它只把 `pipeline::render` 的 `Err` 映射成退出码。
/// 命中清单（词条本身）只进 stderr，⛔ 不进 stdout（REQ-106 AC-002）。
///
/// # ⭐ 第 2 轮：第一个参数是 [`OutletId`]，不是 spec
///
/// 出口只能交代「我是哪条出口」；轨道 / 格式 / 粒度由管线**查表**决定
/// （`pipeline::spec_of`）。于是「出口把轨道改成 `Internal` 从而关掉闸门」
/// （QA 变异 #4）在源码里**写不出来**：既没有 spec 值，也没有可写字段。
fn publish_customer(
    id: crate::domain::pipeline::OutletId,
    product: crate::domain::pipeline::Product<'_>,
) -> crate::domain::pipeline::OutletRender {
    match crate::domain::pipeline::render(id, product) {
        Ok(rendered) => rendered,
        Err(e) if e.is_leak() => {
            let hits = e.hits();
            eprintln!("⚠ 客户视图泄露了建模术语（出口 {}）：{:?}", e.outlet(), hits);
            std::process::exit(EXIT_LEAKED_MODELING_TERMS);
        }
        Err(e) => {
            // 渲染/序列化失败是 bug，不是数据问题 ⇒ 按操作性错误退出，不冒充「命中」。
            eprintln!("✗ 出口 {} 的产物生成失败：{}", e.outlet(), e.message());
            std::process::exit(EXIT_OPERATION_FAILED);
        }
    }
}

pub async fn run(cli: Cli) -> anyhow::Result<()> {
    install_broken_pipe_guard();

    // ⭐ REQ-116 · `doc ingest` **在连库之前**分派。
    //
    // 为什么把它提到连接池前面：这条命令**一处都不碰产品库**（AC-008：不写库）。
    // 输入是本地文件、产物是本地文件、指纹缓存也是本地文件（`<out>.cache.json`）。
    // 若仍先建连接池，`ONTOK_DATABASE_URL` 没配就**跑不了格式闸**（AC-001 的
    // `.pdf` 用例会先被「连不上库」挡住），而「这个格式不能吃」与「库连不上」
    // 是两件完全不相干的事 —— 报错必须指向真原因。
    let cmd = match cli.cmd {
        Cmd::Doc { cmd } => return doc_cmd(cmd).await,
        // ⭐ REQ-118：两条**提案**命令也**不连产品库**（ADR-005 P2：提案不写库）。
        // 与 `doc ingest` 同一条理由：输入是本地 schema 文件、产物是本地文件、
        // 指纹缓存也是本地文件 —— 若仍先建连接池，「没配库」会抢在
        // 「没配 LLM 三件套」前面报，而后者才是真原因。
        Cmd::Name { cmd } => return name_cmd(cmd).await,
        Cmd::Exclude { cmd } => return exclude_cmd(cmd).await,
        cmd => cmd,
    };

    let url = crate::config::require_env(crate::config::DATABASE_URL_ENV)
        .or_else(|_| crate::config::require_env(crate::config::TEST_DATABASE_URL_ENV))?;
    let pool: PgPool = crate::db::pool::connect(&url).await?;
    crate::db::pg::migrate(&pool).await?;

    match cmd {
        Cmd::Project { cmd } => project_cmd(&pool, cmd).await,
        Cmd::Version { cmd } => version_cmd(&pool, cmd).await,
        Cmd::Propose { cmd } => propose_cmd(&pool, cmd).await,
        Cmd::Review { cmd } => review_cmd(&pool, cmd).await,
        Cmd::Proposal { cmd } => proposal_cmd(&pool, cmd).await,
        Cmd::Ontology { cmd } => ontology_cmd(&pool, cmd).await,
        // ⭐ REQ-121 · R2RML 映射（内部轨：表名/列名/join 证据 —— 不是客户视图）
        Cmd::R2rml { cmd } => r2rml_cmd(&pool, cmd).await,
        // ⭐ REQ-122 · SPARQL → SQL（按声明注入租户；只读源库由本命令自己连）
        Cmd::Sparql { cmd } => sparql_cmd(&pool, cmd).await,
        // ⭐ REQ-123 · 查询服务对外（REST + MCP over HTTP；一个进程、一份鉴权）
        Cmd::Query { cmd } => query_cmd(&pool, cmd).await,
        // ⭐ REQ-124 · 控制面 API（管理流水线的 HTTP 化；⛔ 只暴露既有流程）
        Cmd::Console { cmd } => console_cmd(&pool, cmd).await,
        // ⭐ REQ-126 · 客户门户（租户锁死、只能问预置问题；⛔ 没有自由查询）
        Cmd::Portal { cmd } => portal_cmd(&pool, cmd).await,
        Cmd::Concept { cmd } => concept_cmd(&pool, cmd).await,
        // REQ-108 · AC-011：分箱命中率（分数 vs 人工决定）
        Cmd::Confidence { cmd } => confidence_cmd(&pool, cmd).await,
        // ⛔ 不可达：`Cmd::Doc` 在函数开头就已返回（不连库）。这里**不**写
        //    `unreachable!()`，而是报一句人话 —— 万一将来有人把上面的提前返回
        //    删掉，得到的是一条「内部错误」而不是一个 panic 栈。
        Cmd::Doc { .. } => anyhow::bail!(
            "内部错误：`doc` 子命令应当在连库之前处理（它不需要产品库）。\
             请把 `run()` 开头的提前返回恢复回去。"
        ),
        // 同上：这两条也在连库之前返回。
        Cmd::Name { .. } | Cmd::Exclude { .. } => anyhow::bail!(
            "内部错误：`name` / `exclude` 子命令应当在连库之前处理（它们不写库）。\
             请把 `run()` 开头的提前返回恢复回去。"
        ),
        Cmd::McpServer { project, version } => {
            // 冻结闸门在 `mcp::prepare` 里：未冻结就 Err ⇒ 这里退出码非零。
            //
            // ⚠️ **诊断走 stderr**：stdout 是 MCP 的专属通道，
            // 规范 Transports 章禁止往 stdout 写任何非 MCP 消息。
            let args = crate::mcp::ServerArgs { project, version };
            let ctx = match crate::mcp::prepare(&pool, &args).await {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("✗ mcp-server 拒绝启动：{e}");
                    std::process::exit(2);
                }
            };
            eprintln!(
                "· mcp-server 就绪：项目 {} 第 {} 版（已冻结于 {}），{} 个工具",
                ctx.project,
                ctx.version,
                ctx.frozen_at,
                crate::mcp::tools::catalog().len()
            );
            crate::mcp::serve(ctx).await
        }
        Cmd::Serve { project, version, bind } => {
            // 绑地址先解析：给错格式要报人话，不要把 axum 的栈倒给用户。
            let addr: std::net::SocketAddr = bind.parse().with_context(|| {
                format!("监听地址看不懂：{bind:?}。要写成 127.0.0.1:8787 这样的样子。")
            })?;
            let args = crate::server::http::ServeArgs {
                project,
                version,
                bind: addr,
            };
            crate::server::http::serve(&pool, args).await
        }
    }
}

/// REQ-116 · **文档摄取**（`ontok doc ingest`）—— 唯一一个**不连产品库**的子命令。
///
/// # 它为什么在 `run()` 的连库之前就被分派
///
/// 见 [`run`] 开头的注释：输入/产物/缓存全是本地文件，格式闸（AC-001）
/// 必须能在「没配库」的机器上照常给出**真原因**（格式不支持），
/// 而不是被「连不上库」抢先盖住。
///
/// # 顺序是刻意的
///
/// | 步骤 | 为什么在这个位置 |
/// | --- | --- |
/// | 读文件 + 格式闸 | 最便宜的失败先报（`.pdf` ⇒ 立刻点名格式） |
/// | `--chunks-only` | 不碰模型、不需要凭据与预算（AC-002 的判据用它） |
/// | 预算 → LLM 三件套 | 缺口必须在**干活之前**报出来（不白跑一趟） |
/// | 读缓存 → 抽取 → **先写缓存** | 花掉的钱对应的回答必须落盘，否则越预算后重跑要再付一次 |
/// | 预算闸 → 产物 | 越预算 ⇒ **不产出任何断言文件**（AC-007 的裁定，见下） |
///
/// # 越预算时为什么**一个产物文件都不写**
///
/// AC-007 允许二选一：「不产出半份 JSONL」或「产出但标 `truncated: true`」。
/// 这里选**前者**：主产物 `assertions.jsonl` 的契约里没有 `truncated` 这一栏，
/// 一份「只跑了前 K 块」的 JSONL 与完整的那份**长得一模一样**，下游（REQ-117）
/// 会把它当全量来对齐。宁可什么都不出，也不要出一份看起来完整的一半。
/// （`--limit-chunks` 是**显式**截断，那种情况允许出产物，并在 `stats.truncated`
/// 里标 true —— 人知道自己只要前 N 块。）
async fn doc_cmd(cmd: DocCmd) -> anyhow::Result<()> {
    match cmd {
        // ⭐ REQ-129 · 合并是**第二条** `doc` 子命令（拆开而不是给 ingest 加开关：
        // 摄取要调模型、合并只做机械对齐，两者的失败面与预算面完全不同）。
        DocCmd::Merge {
            assertions,
            proposal,
            out,
            stats,
            schema,
        } => {
            return doc_merge_cmd(
                &assertions,
                &proposal,
                &out,
                stats.as_deref(),
                schema.as_deref(),
            )
            .await
        }
        // ⭐ REQ-129 · 签字（规则 7 的另一半）：只写两栏、⛔ 不就地改输入。
        DocCmd::Sign {
            file,
            item,
            by,
            out,
            in_place,
            at,
        } => {
            return doc_sign_cmd(&file, &item, &by, out.as_deref(), in_place, at.as_deref())
                .await
        }
        DocCmd::Ingest {
            input,
            out,
            stats,
            budget_tokens,
            limit_chunks,
            cache,
            chunks_only,
        } => {
            return doc_ingest_cmd(
                input,
                out,
                stats,
                budget_tokens,
                limit_chunks,
                cache,
                chunks_only,
            )
            .await
        }
    }
}

/// REQ-116 · `ontok doc ingest` 的装配点（唯一一个调模型的 `doc` 子命令）。
async fn doc_ingest_cmd(
    input: PathBuf,
    out: PathBuf,
    stats_path: Option<PathBuf>,
    budget_tokens: Option<u64>,
    limit_chunks: Option<usize>,
    cache: Option<PathBuf>,
    chunks_only: bool,
) -> anyhow::Result<()> {
    use crate::extract::doc_ingest as di;

    if limit_chunks == Some(0) {
        anyhow::bail!("--limit-chunks 是 0：0 块等于「什么都没跑」，那与「不跑」混淆。要跑就给 ≥1。");
    }

    // ① 输入 + 格式闸（AC-001）。`load_docs` 是**唯一**判「这个扩展名能不能吃」的地方。
    let docs = di::load_docs(&input)?;
    let all_chunks = di::chunk_all(&docs);

    // ② `--chunks-only`：只产块清单，一次模型都不调（AC-002 的确定性判据用它比对）。
    if chunks_only {
        let manifest = di::render_chunks_jsonl(&all_chunks)?;
        write_local(&out, manifest.as_bytes())?;
        eprintln!("✓ 只分块（--chunks-only）：{} 个文件 → {} 块", docs.len(), all_chunks.len());
        eprintln!("  产物：{}（同一份输入两次运行必须逐字节相同）", out.display());
        return Ok(());
    }

    // ③ 预算 + LLM 三件套（**在干活之前**判完）。
    //
    // 三件套的校验**复用 REQ-109 的 `LlmSettings::from_env`**（不抄第二份判据）：
    // 它缺哪个变量就点名哪个，且绝不回显凭据。
    let budget = di::require_budget(budget_tokens)?;
    if budget_tokens.is_none() {
        eprintln!(
            "· 未给 --budget-tokens，用默认上限 {budget}（超过即停，且不产出断言文件）"
        );
    }
    let settings = crate::extract::ai_semantic::LlmSettings::from_env()?;
    let mut transport = settings.transport()?;
    let model = settings.config.model.clone();
    // 输出上限：本通道的默认值比 REQ-109 的单条判定大得多（理由见
    // `doc_ingest::DEFAULT_MAX_TOKENS`）。运维显式设了 ONTOK_LLM_MAX_TOKENS 就以它为准。
    let max_tokens_set = std::env::var(crate::agent::llm::MAX_TOKENS_ENV)
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false);
    let max_tokens = di::resolve_max_tokens(settings.config.max_tokens, max_tokens_set);

    // ④ 指纹缓存：**本地 JSON 文件**（⛔ 不是 `ai_cache` 表 —— 本命令不写库）。
    let cache_path = cache.unwrap_or_else(|| {
        let mut s = out.clone().into_os_string();
        s.push(".cache.json");
        PathBuf::from(s)
    });
    let mut mem_cache = match std::fs::read_to_string(&cache_path) {
        Ok(text) => di::cache_from_json(&text)
            .with_context(|| format!("指纹缓存读不懂：{}", cache_path.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => di::MemoryDocCache::new(),
        Err(e) => {
            return Err(anyhow::Error::new(e))
                .with_context(|| format!("读不到指纹缓存 {}", cache_path.display()))
        }
    };

    // ⑤ 分阶段上线：只跑前 N 块（**显式**截断，stats.truncated 会标 true）。
    let chunks: Vec<di::Chunk> = match limit_chunks {
        Some(n) if n < all_chunks.len() => all_chunks[..n].to_vec(),
        _ => all_chunks.clone(),
    };
    let prompts = di::Prompts::standard();
    let (assertions, stats) = di::ingest_all(
        transport.as_mut(),
        &mut mem_cache,
        &chunks,
        &model,
        &prompts,
        budget,
        max_tokens,
    );

    // ⑥ **先落缓存、再判预算**：已经花掉的钱对应的回答必须留在盘上，
    //    否则一次越预算的终止会让这一轮所有调用白花（下次重跑再付一次）。
    let cache_json = di::cache_to_json(&mem_cache)?;
    write_local(&cache_path, cache_json.as_bytes())?;

    // ⑦ 预算闸（AC-007）。
    if stats.over_budget() {
        let stop_where = match stats.budget_stopped {
            Some(n) => format!("停在第 {n} 块（共 {} 块）", chunks.len()),
            None => format!(
                "停在第 {} 块之后（{} 块已全部跑完，用量越过了上限）",
                chunks.len(),
                chunks.len()
            ),
        };
        eprintln!(
            "✗ 预算超限：已用 {} / 上限 {} token，{}",
            stats.tokens_spent, budget, stop_where
        );
        anyhow::bail!(
            "doc ingest 预算超限 —— 本轮**不产出任何断言文件**（AC-007：不产出半份 JSONL）。\n\
             \n\
             已用 / 上限 / 停在第几块：{} / {} / {}。\n\
             \n\
             为什么越预算时一个产物都不写：`assertions.jsonl` 没有「这只是前 K 块」这一栏，\
             一份半份的 JSONL 与完整的那份**长得一模一样** —— 下游对齐会把残缺当全量。\n\
             已经花掉的那些回答**已经写进本地缓存**（{}），放宽预算重跑会直接命中、\
             不重复计费。",
            stats.tokens_spent,
            budget,
            stop_where,
            cache_path.display()
        );
    }

    // ⑧ 产物：断言 JSONL（稳定排序）+ stats.json。
    let jsonl = di::render_jsonl(&assertions)?;
    let stats_json = di::render_stats(
        &input.display().to_string(),
        &docs,
        &all_chunks,
        &stats,
        limit_chunks,
        &model,
        &prompts.sha256,
    )?;
    write_local(&out, jsonl.as_bytes())?;
    if let Some(p) = &stats_path {
        write_local(p, stats_json.as_bytes())?;
    }

    // ⑨ 屏幕报告（走 stderr：stdout 留给「要不要接管道」的选择，本命令默认不写 stdout）。
    let mut future = 0usize;
    let mut example = 0usize;
    for a in &assertions {
        match a.modality() {
            di::Modality::Future => future += 1,
            di::Modality::Example => example += 1,
            _ => {}
        }
    }
    let relations_total = stats.relations;
    // ⭐ N-2：两个占比都要报 —— 只看「活下来的」那个会恒为 0%（恒真判据）。
    let alive = stats.predicate_share("belongs_to");
    let raw = stats.raw_predicate_share("belongs_to");
    let fmt_share = |s: di::PredicateShare| match s.share {
        Some(v) => format!("{}/{}={:.1}%", s.count, s.total, v * 100.0),
        None => format!("{}/{}（分母 0）", s.count, s.total),
    };
    let preds: Vec<String> = stats
        .predicate_counts
        .iter()
        .map(|(p, n)| format!("{p} {n}"))
        .collect();
    let raw_preds: Vec<String> = stats
        .raw_predicate_counts
        .iter()
        .map(|(p, n)| format!("{p} {n}"))
        .collect();

    eprintln!("✓ 文档摄取");
    eprintln!(
        "  输入        : {}（{} 个文件 / {} 块{}）",
        input.display(),
        docs.len(),
        chunks.len(),
        match limit_chunks {
            Some(n) if n < all_chunks.len() => format!("，--limit-chunks {n} ⇒ 截断（truncated）"),
            _ => String::new(),
        }
    );
    eprintln!(
        "  断言        : {} 条（概念 {} · 关系 {}）",
        stats.assertions, stats.concepts, stats.relations
    );
    eprintln!(
        "  ⚠ 丢弃      : {} 条（证据对不上 {} · 模态 {} · 谓词 {} · 字段空 {} · 解析 {}）",
        stats.rejected(),
        stats.rejected_no_evidence,
        stats.rejected_modality,
        stats.rejected_predicate,
        stats.rejected_empty,
        stats.rejected_parse
    );
    for s in stats.rejection_samples.iter().take(3) {
        eprintln!("                {s}");
    }
    eprintln!(
        "  缓存        : 命中 {} / {} 块（实际调用 {} 次 · 强制失败 {} 次）",
        stats.cache_hits, chunks.len(), stats.calls, stats.failed_calls
    );
    eprintln!(
        "  token       : 本轮 {} / 上限 {}（回放历史 {}）· 输出上限 {max_tokens}",
        stats.tokens_spent, budget, stats.tokens_replayed
    );
    eprintln!("  概念（去重）: {} 个（⛔ 不合并别名 —— 实体消解留到对齐轮）", stats.distinct_concepts());
    eprintln!(
        "  谓词        : 活下来 {}（{} 条关系）",
        if preds.is_empty() { "（无）".to_string() } else { preds.join(" · ") },
        relations_total
    );
    // ⭐ N-2：原始分布 + 谓词闸的丢弃 + 两个 belongs_to 占比（AC-004 看 raw）。
    eprintln!(
        "  谓词（原始）: {}（{} 条关系，含被谓词闸丢掉的 {} 条）",
        if raw_preds.is_empty() { "（无）".to_string() } else { raw_preds.join(" · ") },
        stats.raw_relations(),
        stats.rejected_by_predicate_gate
    );
    eprintln!(
        "  belongs_to  : 原始 {} ← **AC-004 判据** · 活下来 {}",
        fmt_share(raw),
        fmt_share(alive)
    );
    if !stats.rejected_predicate_counts.is_empty() {
        let rej: Vec<String> = stats
            .rejected_predicate_counts
            .iter()
            .map(|(p, n)| format!("{p} {n}"))
            .collect();
        eprintln!(
            "  谓词闸丢弃  : {} 条（{}）",
            stats.rejected_by_predicate_gate,
            rej.join(" · ")
        );
    }
    eprintln!(
        "  未对齐      : future {future} · example {example}（JSONL 里保留，alignable=false）"
    );
    eprintln!("  产物        : {}", out.display());
    if let Some(p) = &stats_path {
        eprintln!("  统计        : {}", p.display());
    }
    eprintln!("  缓存文件    : {}", cache_path.display());
    eprintln!(
        "  提示        : 本命令不写库、不改本体、不出图；产物供下一轮「断言 ↔ 表/字段」对齐使用"
    );
    Ok(())
}

/// ⭐ REQ-129 · `ontok doc merge` 的装配点（**不连产品库、不调模型**）。
///
/// # 顺序是刻意的
///
/// | 步骤 | 为什么在这个位置 |
/// | --- | --- |
/// | 读两份输入 | 最便宜的失败先报（文件不在 / 不是合法 JSON） |
/// | 拒绝原地覆盖 | `--out` 指向输入文件 ⇒ 先报，别把提案写坏 |
/// | 纯合并（`extract::doc_merge`） | 零 I/O 的唯一实现；失败时**一个字节都不写** |
/// | 写合并结果 → 写统计 | 主产物先落盘；统计是附件 |
/// | stderr 报告五项 | stdout 留给「要不要接管道」的选择（本命令不写 stdout） |
///
/// # 为什么不需要 LLM 三件套、不需要预算
///
/// 它是**机械对齐**：输入是两份本地文件，判断依据全是文件里的字段（`maps_to` /
/// 名字 / 别名）。⛔ 没有一次网络调用，所以没有 token 可花，也就没有预算闸。
async fn doc_merge_cmd(
    assertions: &std::path::Path,
    proposal: &std::path::Path,
    out: &std::path::Path,
    stats_path: Option<&std::path::Path>,
    schema: Option<&std::path::Path>,
) -> anyhow::Result<()> {
    use crate::extract::doc_merge as dm;

    let assertions_text = std::fs::read_to_string(assertions).with_context(|| {
        format!(
            "读不到断言 JSONL：{}（先跑 `ontok doc ingest` 产出它）",
            assertions.display()
        )
    })?;
    let proposal_text = std::fs::read_to_string(proposal).with_context(|| {
        format!(
            "读不到提案 JSON：{}（先跑 `ontok-propose` 产出它）",
            proposal.display()
        )
    })?;
    let proposal_doc: Value = serde_json::from_str(&proposal_text)
        .with_context(|| format!("提案 {} 不是合法 JSON", proposal.display()))?;

    // ⛔ **不原地覆盖 —— 输出 × {输入 ∪ 另一个输出}，全部查**（QA N1）。
    //
    // | 撞法 | 后果 |
    // | --- | --- |
    // | `--out` == `--assertions` / `--proposal` | 输入被合并结果盖掉，没法再复核「合并前是什么样」 |
    // | `--stats` == `--assertions` / `--proposal` | **提案/断言被写成一坨统计 JSON**（静默数据丢失） |
    // | `--stats` == `--out` | 主产物被统计文件顶掉，而屏幕上会报「产物已写出」 |
    //
    // 上一版只守了 `--out` ⇒ `--stats` 指向提案时 exit 0 且提案被覆盖（QA 实测）。
    // 静默覆盖是**数据丢失**，所以这里一律 exit≠0 并**点名哪两个参数撞了**。
    let ins: [(&str, &std::path::Path); 2] =
        [("--assertions", assertions), ("--proposal", proposal)];
    let mut outs: Vec<(&str, &std::path::Path)> = vec![("--out", out)];
    if let Some(p) = stats_path {
        outs.push(("--stats", p));
    }
    for (i, (oname, opath)) in outs.iter().enumerate() {
        for (iname, ipath) in ins.iter() {
            if same_path(opath, ipath) {
                anyhow::bail!(
                    "{oname} 与 {iname} 是同一个文件（{}）。\n\
                     ⛔ 不原地覆盖：{iname} 是**输入**，覆盖后就没法再拿它复核「合并前是什么样」。\
                     请给 {oname} 换一个路径。",
                    opath.display()
                );
            }
        }
        for (oname2, opath2) in outs.iter().skip(i + 1) {
            if same_path(opath, opath2) {
                anyhow::bail!(
                    "{oname} 与 {oname2} 是同一个文件（{}）。\n\
                     ⛔ 两个产物不能写同一个文件：一份会**静默盖掉**另一份，\
                     而屏幕上还会报「两个都写了」。请各给一个路径。",
                    opath.display()
                );
            }
        }
        crate::extract::proposal::require_out_is_file(opath)
            .map_err(|e| anyhow::anyhow!("{oname}：{e}"))?;
    }

    // ⭐ `--schema`（可选）：给了就走**严格**校验面（AC-004 订正后：给了就 0 例外）。
    let opts = match schema {
        Some(p) => {
            let report = load_schema_report(p)?;
            dm::MergeOptions {
                schema: Some(dm::SchemaIndex::from_report(&report)),
            }
        }
        None => dm::MergeOptions::default(),
    };

    // 纯合并：失败 ⇒ 一个字节都不写（不产半份提案）。
    let outcome = dm::merge_with(&assertions_text, &proposal_doc, &opts)?;
    let stats_json = dm::render_stats(&outcome.stats)?;
    write_local(out, outcome.rendered.as_bytes())?;
    if let Some(p) = stats_path {
        write_local(p, stats_json.as_bytes())?;
    }

    // 屏幕报告（stderr）：AC-008 的五项逐行点名，外加未映射的原因分档。
    let s = &outcome.stats;
    eprintln!("✓ 文档断言合并（机械对齐，⛔ 不做语义猜测）");
    eprintln!(
        "  输入        : {} 行断言（概念 {} · 关系 {}） × 提案 {} 条",
        s.assertions_read, s.assertions_concept, s.assertions_relation, s.items_in
    );
    eprintln!(
        "  排除不可对齐: {}（future {} · example {}{}）",
        s.excluded_not_alignable,
        s.excluded_future,
        s.excluded_example,
        if s.excluded_modality_conflict > 0 {
            format!(" · 模态与标记矛盾 {}", s.excluded_modality_conflict)
        } else {
            String::new()
        }
    );
    eprintln!("  并入        : {}", s.merged);
    eprintln!(
        "  未映射      : {}{}",
        s.doc_unmapped,
        if s.unmapped_reasons.is_empty() {
            String::new()
        } else {
            format!(
                "（{}）",
                s.unmapped_reasons
                    .iter()
                    .map(|(k, n)| format!("{k} {n}"))
                    .collect::<Vec<_>>()
                    .join(" · ")
            )
        }
    );
    eprintln!("  冲突        : {}（两侧说法都在 conflicts[]，条目 applicable=false）", s.conflicts);
    eprintln!(
        "  合并为 both : {} · doc-only 条目 {}（新增 {} 条）",
        s.merged_both, s.merged_doc_only, s.items_added
    );
    if s.duplicate_claims > 0 {
        eprintln!("  重复断言    : {} 条（同实体/同一条边逐条去重后只留一条证据）", s.duplicate_claims);
    }
    if s.band_capped_strong > 0 {
        eprintln!(
            "  ⚠ 压档      : {} 条（输入档位强于 {} ⇒ 压到 medium）",
            s.band_capped_strong, dm::DOC_BAND_CAP
        );
    }
    if s.reviewed_by_reset > 0 {
        eprintln!(
            "  ⚠ 签名重置  : {} 条（doc 条目必须未签；重跑合并会让之前的签字作废）",
            s.reviewed_by_reset
        );
    }
    if s.parent_key_id_convention > 0 {
        eprintln!(
            "  ⚠ 父端列    : {} 条只靠约定主键名 `{}` 通过（提案里没有主键列；给 `--schema` 可严格校验）",
            s.parent_key_id_convention,
            dm::PK_CONVENTION
        );
    }
    if s.schema_checked {
        eprintln!(
            "  严格校验    : 是（schema.json {} 张表）· 父端列靠 schema 通过 {} 条",
            s.schema_tables, s.parent_column_schema_only
        );
    } else {
        eprintln!(
            "  严格校验    : 否（没给 --schema ⇒ 父端列只能靠约定名；\
             要 0 例外请给 --schema <schema.json>）"
        );
    }
    eprintln!("  产物        : {}", out.display());
    if let Some(p) = stats_path {
        eprintln!("  统计        : {}", p.display());
    }
    eprintln!(
        "  提示        : 产出与 proposal.json 同形状；未签的 doc 条目 apply 时会被忽略，\
         冲突条目会让 apply 拒绝并点名"
    );
    Ok(())
}

/// ⭐ REQ-129 · `ontok doc sign` 的装配点（**不连产品库、不调模型**）。
///
/// # 三条「不做」落在哪
///
/// | ⛔ | 落点 |
/// | --- | --- |
/// | 不匿名签 | `sign()` 里对 `--by` 的空串判断（CLI 只负责原样传） |
/// | 不就地改输入 | 这里：`--out` 与 `--in-place` 二选一；`--out` 指向输入也拒 |
/// | 不签 DB 轨条目 | `sign()` 里对 `origin` 的判断（CLI 不重复判） |
///
/// 时间戳默认取 UTC：签字时间是**事实**，不是渲染选项。
/// 要逐字节复现（AC 的确定性判据）就显式给 `--at`。
async fn doc_sign_cmd(
    file: &std::path::Path,
    item: &str,
    by: &str,
    out: Option<&std::path::Path>,
    in_place: bool,
    at: Option<&str>,
) -> anyhow::Result<()> {
    use crate::extract::doc_merge as dm;

    // ⑤/⑥ · 出口：`--out` 与 `--in-place` 二选一，且 ⛔ 不许拿 `--out` 悄悄就地覆盖。
    let target: &std::path::Path = match (out, in_place) {
        (Some(_), true) => anyhow::bail!(
            "--out 与 --in-place 只能给一个：给了 --out 就写到新文件，\
             要覆盖输入请显式用 --in-place"
        ),
        (Some(p), false) => {
            if same_path(p, file) {
                anyhow::bail!(
                    "--out 指向的正是输入文件（{}）。\n\
                     ⛔ 不就地改输入：请写到另一个文件；确实要覆盖请显式加 `--in-place`。\n\
                     理由：签字前后的两份文件是**审计对象**（谁签的、改了什么）——\
                     留不下原件就没法复核。",
                    p.display()
                );
            }
            p
        }
        (None, true) => file,
        (None, false) => anyhow::bail!(
            "缺 --out：签字**必须落盘**到另一个文件（或显式给 `--in-place`）。\n\
             ⛔ 不提供「默认就地改」：签字前后的两份文件是审计对象，\
             留不下原件就没法复核「谁签的、除了这两栏还改了什么」。"
        ),
    };

    let text = std::fs::read_to_string(file)
        .with_context(|| format!("读不到提案文件：{}", file.display()))?;
    // ⭐ REQ-130 · 时间的**来源**必须跟时间一起记下来：
    // `--at` 给了 ⇒ `arg`（人给的**声明**，可复现）；省略 ⇒ `system`（本次取的**记录**）。
    // ⛔ 这一栏不改变 `--at` 可选（Q-3），只是把「这个时间是谁定的」写进产物。
    let (at, at_source) = match at {
        Some(t) => (t.to_string(), dm::AT_SOURCE_ARG),
        None => (
            chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            dm::AT_SOURCE_SYSTEM,
        ),
    };

    // 纯签字：失败 ⇒ 一个字节都不写。
    let outcome = dm::sign(&text, item, by, &at, at_source)?;
    write_local(target, outcome.rendered.as_bytes())?;

    eprintln!("✓ 文档条目签字");
    eprintln!(
        "  条目        : 第 {} 条 {}（origin {}）",
        outcome.seq, outcome.label, outcome.origin
    );
    eprintln!(
        "  签字        : {} @ {}{}",
        by.trim(),
        outcome.signed_at,
        if outcome.already_signed {
            "（幂等：本来就已由同一个人签过，值不变）"
        } else {
            ""
        }
    );
    eprintln!(
        "  时间来源    : {}{}",
        outcome.signed_at_source,
        match outcome.signed_at_source.as_str() {
            dm::AT_SOURCE_ARG => "（--at 给的：声明，可复现）",
            dm::AT_SOURCE_SYSTEM => "（本次运行的系统时钟：记录，不可复现为同一秒）",
            _ => "（老产物没有 signed_at_source：按缺省处理，⛔ 不补写）",
        }
    );
    eprintln!("  其余字节    : 未变（只写了 reviewed_by / signed_at / signed_at_source 三栏）");
    if !outcome.input_canonical {
        eprintln!(
            "  ⚠ 格式提示  : 输入不是规范格式（键序/缩进），本次输出已规范化 —— 条目内容未变"
        );
    }
    eprintln!("  产物        : {}", target.display());
    eprintln!(
        "  下一步      : ontok propose import --file {}（未签的 doc 条目 apply 时会被忽略）",
        target.display()
    );
    Ok(())
}

/// 两个路径是否是同一个文件（**只做字面比较**：本命令不做符号链接解析）。
fn same_path(a: &std::path::Path, b: &std::path::Path) -> bool {
    a == b
        || match (a.canonicalize(), b.canonicalize()) {
            (Ok(x), Ok(y)) => x == y,
            _ => false,
        }
}

/// 本地产物的**唯一写出点**（REQ-116）。
///
/// 三份文件共用它：断言 JSONL（主产物）、`stats.json`、本地指纹缓存。
/// 收在一个函数里是刻意的 —— 出口登记表（`domain::pipeline::OUTLETS`）里
/// 「一处写点」对应的是「这条命令会往盘上写东西」这个事实，而不是三份文件各一条。
/// `path.parent()` 不存在时先建目录（否则第一次跑 `--out out/x.jsonl` 会 ENOENT）。
fn write_local(path: &std::path::Path, bytes: &[u8]) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("建不出产物目录 {}", dir.display()))?;
        }
    }
    std::fs::write(path, bytes).with_context(|| format!("写不出去：{}", path.display()))?;
    Ok(())
}

// ══════════════════════════════════════════════════════════════
// REQ-118 · AI 提案（ADR-005）
//
// 两条命令（`name suggest` / `exclude suggest`）**不连产品库**：
// 输入是本地 `schema.json`、产物是本地文件、指纹缓存也是本地文件。
// 它们在 `run()` 里**连库之前**就被分派（与 `doc ingest` 同一个理由）——
// 否则「没配 ONTOK_DATABASE_URL」会抢在「没配 LLM 三件套」前面报，而后者才是真原因。
//
// 落盘顺序是刻意的：**先写缓存、再判预算、最后写产物**。
// 已花掉的钱对应的回答必须落盘（否则越预算后重跑要再付一次）；
// 越预算则**一个产物都不写**（AC-006：不产半份文件）。
// ══════════════════════════════════════════════════════════════

/// 读 `schema.json` 的**唯一**入口（两条提案命令共用）。
fn load_schema_report(
    path: &std::path::Path,
) -> anyhow::Result<crate::extract::introspect::SchemaReport> {
    let raw = std::fs::read_to_string(path).with_context(|| {
        format!(
            "读不到 schema JSON：{}（先跑 ontok-introspect 产出它）",
            path.display()
        )
    })?;
    serde_json::from_str(&raw)
        .with_context(|| format!("{} 不是合法的 SchemaReport JSON", path.display()))
}

/// `<out>.cache.json` —— 两条提案命令共用的默认缓存落点。
fn default_cache_path(out: &std::path::Path) -> PathBuf {
    let mut s = out.to_path_buf().into_os_string();
    s.push(".cache.json");
    PathBuf::from(s)
}

/// 提案通道的输出上限：运维显式设了 `ONTOK_LLM_MAX_TOKENS` 就以它为准（部署事实），
/// 否则用 `doc_ingest` 那条通道的默认值（4096）—— **不另立第二份默认值**。
fn proposal_max_tokens(configured: u32) -> u32 {
    let explicitly_set = std::env::var(crate::agent::llm::MAX_TOKENS_ENV)
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false);
    crate::extract::doc_ingest::resolve_max_tokens(configured, explicitly_set)
}

/// 打开 LLM 三件套 + 真 HTTP 传输（校验复用 REQ-109 的 `LlmSettings::from_env`）。
struct ProposalLlm {
    transport: Box<dyn crate::agent::llm::LlmTransport>,
    model: String,
    max_tokens: u32,
}

fn open_proposal_llm() -> anyhow::Result<ProposalLlm> {
    let settings = crate::extract::ai_semantic::LlmSettings::from_env()?;
    let transport = settings.transport()?;
    let max_tokens = proposal_max_tokens(settings.config.max_tokens);
    Ok(ProposalLlm {
        transport,
        model: settings.config.model.clone(),
        max_tokens,
    })
}

/// 置信度分箱一行（三条命令同一句话 —— 唯一实现在 `proposal` 模块）。
fn confidence_line(values: &[f64]) -> String {
    crate::extract::proposal::confidence_histogram_line(values)
}

/// **`ontok name suggest`** —— 决策点一：中文名提案。
async fn name_cmd(cmd: NameCmd) -> anyhow::Result<()> {
    use crate::extract::name_suggest as ns;
    use crate::extract::proposal as prop;

    let NameCmd::Suggest {
        schema,
        out,
        as_names,
        budget_tokens,
        cache,
        batch_size,
    } = cmd;

    prop::require_out_is_file(&out)?;
    let report = load_schema_report(&schema)?;
    if report.tables.is_empty() {
        anyhow::bail!(
            "schema JSON 里一张表都没有（{}）—— 没有可提案的对象。\
             先确认 ontok-introspect 跑在对的源库/schema 上。",
            schema.display()
        );
    }
    let budget = prop::require_proposal_budget(budget_tokens)?;
    if budget_tokens.is_none() {
        eprintln!("· {}", prop::default_budget_notice(budget));
    }
    let batches = ns::plan_batches(&report, batch_size.unwrap_or(ns::DEFAULT_BATCH_SIZE))?;
    let mut llm = open_proposal_llm()?;
    let cache_path = cache.unwrap_or_else(|| default_cache_path(&out));
    let mut mem_cache = prop::load_cache_file(&cache_path)?;
    let prompts = crate::extract::ai_semantic::Prompts::from_system(ns::SYSTEM_PROMPT);

    let (suggestions, stats) = ns::suggest_all(
        llm.transport.as_mut(),
        &mut mem_cache,
        &batches,
        &llm.model,
        &prompts,
        budget,
        llm.max_tokens,
    );

    // ① 先落缓存（花掉的钱对应的回答必须留在盘上）。
    let cache_json = prop::cache_to_json(&mem_cache)?;
    write_local(&cache_path, cache_json.as_bytes())?;

    // ② 预算闸：越限 ⇒ **不产出任何提案文件**（AC-006）。
    prop::ensure_within_budget("name suggest", &stats, budget, &cache_path)?;

    // ②' N-1 失败闸（全部批次失败）：**一个字节都不写**。
    // 空提案的 JSON 完全合法，导入方会当成「真的、恰好 0 条」—— 那是装作处理过。
    if stats.completeness() == prop::Completeness::AllFailed {
        return Err(prop::all_batches_failed_error(
            "name suggest",
            &stats,
            &cache_path,
        ));
    }

    // ③ 产物：提案（主）+ `--as-names`（可选，AC-002）。
    let header = ns::header(&report, &llm.model, &prompts.sha256)?;
    let text = ns::render_proposal(&suggestions, &report, &header, &stats)?;
    let proposal_sha = prop::sha256_text(&text);
    write_local(&out, text.as_bytes())?;

    // ③' N-1 失败闸（部分批次失败）：文件可留（里面 `complete: false`），但退出码非零，
    // 且 **`--as-names` 不落盘** —— 一份缺了 N 批的名字表喂给 `ontok-propose`
    // 会让那些表被静默丢弃（REQ-090 的老毛病），正是 N-1 要堵的 fail-open。
    if stats.completeness() == prop::Completeness::Partial {
        return Err(prop::partial_failure_error("name suggest", &stats, &out));
    }

    let mut names_written: Option<PathBuf> = None;
    if let Some(p) = &as_names {
        let names_text = ns::as_names(
            &suggestions,
            &report,
            &llm.model,
            &out.display().to_string(),
            &proposal_sha,
            &prompts.sha256,
        )?;
        write_local(p, names_text.as_bytes())?;
        names_written = Some(p.clone());
    }

    // ④ 屏幕报告（stderr：本命令默认不写 stdout）。
    let values: Vec<f64> = suggestions.iter().map(|s| s.item.confidence).collect();
    let low: Vec<String> = suggestions
        .iter()
        .filter(|s| s.item.is_low_confidence())
        .map(|s| format!("{}={:.2}", s.table, s.item.confidence))
        .collect();
    let missing = ns::missing_tables(&report, &suggestions);
    eprintln!("✓ 名字提案（AI 先提，人审核后才会进本体）");
    eprintln!(
        "  输入        : {}（{} 张表 → {} 批 · 每批 {} 张）",
        schema.display(),
        report.tables.len(),
        batches.len(),
        batch_size.unwrap_or(ns::DEFAULT_BATCH_SIZE)
    );
    eprintln!(
        "  提案        : {} 条（丢 {} 条）"
        , stats.proposed, stats.rejected_total()
    );
    if !stats.rejected.is_empty() {
        let parts: Vec<String> = stats
            .rejected
            .iter()
            .map(|(k, n)| format!("{k} {n}"))
            .collect();
        eprintln!("  丢弃明细    : {}", parts.join(" · "));
        for s in stats.rejection_samples.iter().take(3) {
            eprintln!("                {s}");
        }
    }
    eprintln!(
        "  置信度      : {}（低置信 < {}：{} 条）",
        confidence_line(&values),
        prop::low_confidence_label(),
        stats.low_confidence
    );
    if !low.is_empty() {
        eprintln!("  低置信清单  : {}", low.join(" · "));
    }
    eprintln!(
        "  缓存        : 命中 {} / {} 批（调用 {} 次 · 失败 {} 次）",
        stats.cache_hits, stats.requested, stats.calls, stats.failed_calls
    );
    eprintln!(
        "  token       : 本轮 {} / 上限 {}（回放历史 {}）· 输出上限 {}",
        stats.tokens_spent, budget, stats.tokens_replayed, llm.max_tokens
    );
    eprintln!(
        "  完整性      : complete = true（失败批次 0 / {}）",
        stats.requested
    );
    eprintln!("  模型        : {}（decided_by = ai:{}）", llm.model, llm.model);
    if !missing.is_empty() {
        eprintln!(
            "  ⚠ 未提案    : {} 张（{}）—— ⛔ 静默丢表比错名更坏，这里点名",
            missing.len(),
            missing.join(", ")
        );
    }
    eprintln!("  产物        : {}", out.display());
    eprintln!("  提案 sha256 : {proposal_sha}");
    eprintln!("  缓存文件    : {}", cache_path.display());
    if let Some(p) = &names_written {
        eprintln!("  名字表      : {}", p.display());
        if !missing.is_empty() {
            eprintln!(
                "  ⚠ 喂给 --names 前请注意：有 {} 张表没有提案，\
                 `ontok-propose` 会把这些表**静默丢弃**（它只在 stats 里计数）",
                missing.len()
            );
        }
    }
    eprintln!(
        "  提示        : 未签字的提案不权威（reviewed_by = null）；\
         要生效请显式走 ontok-propose --names + 双轨审核"
    );
    Ok(())
}

/// **`ontok exclude suggest`** —— 决策点二：该不该进本体。
async fn exclude_cmd(cmd: ExcludeCmd) -> anyhow::Result<()> {
    use crate::extract::exclude_suggest as es;
    use crate::extract::proposal as prop;

    let ExcludeCmd::Suggest {
        schema,
        out,
        exclusions,
        budget_tokens,
        cache,
        batch_size,
    } = cmd;

    prop::require_out_is_file(&out)?;
    let report = load_schema_report(&schema)?;
    if report.tables.is_empty() {
        anyhow::bail!(
            "schema JSON 里一张表都没有（{}）—— 没有可判定的对象。",
            schema.display()
        );
    }
    let existing = match &exclusions {
        Some(p) => {
            let s = p
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("--exclusions 路径不是合法 UTF-8：{p:?}"))?;
            Some(crate::extract::exclusions::Exclusions::load(s)?)
        }
        None => None,
    };
    let budget = prop::require_proposal_budget(budget_tokens)?;
    if budget_tokens.is_none() {
        eprintln!("· {}", prop::default_budget_notice(budget));
    }
    let (batches, carried, stale) = es::plan_batches(
        &report,
        existing.as_ref(),
        batch_size.unwrap_or(es::DEFAULT_BATCH_SIZE),
    )?;
    let cache_path = cache.unwrap_or_else(|| default_cache_path(&out));
    let mut mem_cache = prop::load_cache_file(&cache_path)?;

    let (decisions, stats, model, prompt_sha) = if batches.is_empty() {
        // 全部沿用已有清单 ⇒ **一次模型都不调**（也不要求 LLM 三件套）。
        eprintln!("· 全部表都已在 --exclusions 里 ⇒ 不调用模型（沿用 {} 条）", carried.len());
        (
            Vec::new(),
            prop::ProposalStats {
                budget,
                ..Default::default()
            },
            None,
            None,
        )
    } else {
        let mut llm = open_proposal_llm()?;
        let prompts = crate::extract::ai_semantic::Prompts::from_system(es::SYSTEM_PROMPT);
        let (decisions, stats) = es::suggest_all(
            llm.transport.as_mut(),
            &mut mem_cache,
            &batches,
            &llm.model,
            &prompts,
            budget,
            llm.max_tokens,
        );
        // 先落缓存（即使后面越预算，这一轮的钱也不白花）。
        let cache_json = prop::cache_to_json(&mem_cache)?;
        write_local(&cache_path, cache_json.as_bytes())?;
        prop::ensure_within_budget("exclude suggest", &stats, budget, &cache_path)?;
        (
            decisions,
            stats,
            Some(llm.model.clone()),
            Some(prompts.sha256.clone()),
        )
    };

    let model_text = model.clone().unwrap_or_else(|| "（未调用模型）".to_string());
    let input_sha = crate::extract::name_suggest::schema_digest(&report)?;
    // 全部沿用已有清单时**没有 AI 提案**：`source` / `decided_by` 必须说 `rule`，
    // ⛔ 不许写成 `ai:（未调用模型）` —— 那是把「没跑模型」伪装成「模型提的」。
    let header = match (&model, &prompt_sha) {
        (Some(m), Some(p)) => es::header(m, p, input_sha),
        _ => prop::ProposalHeader {
            kind: "exclude",
            source: prop::SOURCE_RULE,
            model: None,
            prompt_sha: None,
            agent_version: es::AGENT_VERSION.to_string(),
            input_sha256: input_sha,
            decided_by: prop::RULE_DECIDED_BY.to_string(),
        },
    };

    // N-1：全部批次失败 ⇒ **一个字节都不写**。
    if stats.completeness() == prop::Completeness::AllFailed {
        return Err(prop::all_batches_failed_error(
            "exclude suggest",
            &stats,
            &cache_path,
        ));
    }

    let text = es::render_proposal(&decisions, &carried, &stale, &header, &stats)?;
    let proposal_sha = prop::sha256_text(&text);
    write_local(&out, text.as_bytes())?;

    // N-1：部分批次失败 ⇒ 文件可留（`complete: false`），但退出码非零。
    if stats.completeness() == prop::Completeness::Partial {
        return Err(prop::partial_failure_error("exclude suggest", &stats, &out));
    }

    let excluded = es::excluded_tables(&decisions);
    let kept = es::kept_tables(&decisions);
    let values: Vec<f64> = decisions.iter().map(|d| d.item.confidence).collect();
    let low: Vec<String> = decisions
        .iter()
        .filter(|d| d.item.is_low_confidence())
        .map(|d| {
            format!(
                "{}={:.2}{}",
                d.table,
                d.item.confidence,
                if d.excluded { "（排除）" } else { "（保留）" }
            )
        })
        .collect();
    eprintln!("✓ 排除提案（AI 先判，人审核后才会真的被排除）");
    eprintln!(
        "  输入        : {}（{} 张表 → 待判 {} 张 / 沿用 {} 张{}）",
        schema.display(),
        report.tables.len(),
        report.tables.len() - carried.len(),
        carried.len(),
        if stale.is_empty() {
            String::new()
        } else {
            format!(" / 陈旧 {} 张", stale.len())
        }
    );
    eprintln!(
        "  判定        : {} 张（排除 {} · 保留 {}）· 丢 {} 条",
        decisions.len(),
        excluded.len(),
        kept.len(),
        stats.rejected_total()
    );
    if !stats.rejected.is_empty() {
        let parts: Vec<String> = stats.rejected.iter().map(|(k, n)| format!("{k} {n}")).collect();
        eprintln!("  丢弃明细    : {}", parts.join(" · "));
        for s in stats.rejection_samples.iter().take(3) {
            eprintln!("                {s}");
        }
    }
    if !excluded.is_empty() {
        eprintln!("  排除清单    : {}", excluded.join(", "));
    }
    if !stale.is_empty() {
        let names: Vec<String> = stale.iter().map(|e| e.table.clone()).collect();
        eprintln!(
            "  ⚠ 陈旧排除  : {} 张在 schema 里已不存在（{}）—— 只报不删，删除是人的显式动作",
            stale.len(),
            names.join(", ")
        );
    }
    eprintln!(
        "  置信度      : {}（低置信 < {}：{} 条）",
        confidence_line(&values),
        prop::low_confidence_label(),
        stats.low_confidence
    );
    if !low.is_empty() {
        eprintln!("  低置信清单  : {}", low.join(" · "));
    }
    eprintln!(
        "  缓存        : 命中 {} / {} 批（调用 {} 次 · 失败 {} 次）",
        stats.cache_hits, stats.requested, stats.calls, stats.failed_calls
    );
    eprintln!(
        "  token       : 本轮 {} / 上限 {}（回放历史 {}）",
        stats.tokens_spent, budget, stats.tokens_replayed
    );
    eprintln!(
        "  完整性      : complete = true（失败批次 0 / {}）",
        stats.requested
    );
    eprintln!("  模型        : {model_text}");
    eprintln!("  产物        : {}", out.display());
    eprintln!("  提案 sha256 : {proposal_sha}");
    eprintln!("  缓存文件    : {}", cache_path.display());
    if excluded.is_empty() && carried.is_empty() {
        eprintln!(
            "  ⚠ 本次没有排除项：文件仍是一份合法提案，但 `ontok-propose --exclude` 会拒绝空 exclude\
             （REQ-113：一份不排除任何表的文件等于没做出决定）"
        );
    } else {
        eprintln!(
            "  下一步      : ontok-propose --schema <same> --names <names.json> \
             --exclude {} --out out/proposal.json --no-sample",
            out.display()
        );
    }
    eprintln!("  提示        : reviewed_by = null；⛔ 本命令没有过滤任何表，也没有写库");
    Ok(())
}

/// REQ-010 · 概念声明通道。
///
/// # 为什么必须过提案，不许「读文件直接生效」
///
/// 声明文件是**人写的**（过渡期由规则生成、将来由 HK Agent / 业务人员改）。
/// 直接生效意味着「客户看到的语义」绕过了双轨审核 —— 那正是 §2.2
/// 说的「Agent 是提议者不是决定者」被架空。所以这里只做一件事：
/// **把声明翻译成 `add_concept_group` 提案条目**，之后与实体类型条目
/// 走完全相同的 `review decide` → `proposal apply`。
/// REQ-108 · **校准命令**（AC-011）：分数 → 分箱 → 人工接受率。
///
/// # 数据从哪来
///
/// `proposal_items.confidence`（REAL）+ `biz_status`（人工决定）。两栏都在
/// 同一条提案条目上，所以不需要跨表拼「这条关系后来被接受了吗」——
/// 判定本来就是**条目级**的。
///
/// # 为什么人工决定取**业务轨**
///
/// 双轨的语义不同：业务轨判「这条关系对不对」，建模轨判「建得规不规范」。
/// 校准要的是**正确率**，所以只能取业务轨。取建模轨会把「关系对、但基数标错」
/// 的条目算成「没接受」，于是分数高低与口径无关地混在一起。
///
/// # 样本不足时**不给结论**
///
/// 设计概要 §六：任一分箱 < 5 条 ⇒ 标「样本不足」。空库同理。
/// ⛔ 绝不输出 `0%` 或 `100%` —— 那不是「准确率 0」或「准确率 100」，
/// 而是「没有样本」，两者的差别正是 AC-011 要守的。
async fn confidence_cmd(pool: &PgPool, cmd: ConfidenceCmd) -> anyhow::Result<()> {
    let ConfidenceCmd::Calibrate { project, version } = cmd;

    let projects = PgProjectStore::new(pool.clone());
    let p = projects
        .get_by_name(&project)
        .await?
        .ok_or_else(|| anyhow::anyhow!("项目不存在：{project}"))?;

    // 已决 = 业务轨有明确决定（`accepted` / `rejected`）。
    // `pending` 不算「未接受」—— 那是「还没判」，进分母会把接受率稀释成噪声。
    // 所以下面**只取** `biz_status IN ('accepted','rejected')`，
    // 另外单独查一个总数，把「待决」报出来。
    let mut sql = String::from(
        // `confidence` 列是 `REAL`（float4）；这里显式转 float8 ——
        // 评分卡用的是 f64，别在解码处留一次 f32 截断（0.15 这类值会漂）。
        "SELECT pi.confidence::float8, pi.biz_status
         FROM proposal_items pi
         JOIN proposals p ON p.id = pi.proposal_id
         WHERE p.project_id = $1
           AND pi.confidence IS NOT NULL",
    );
    if version.is_some() {
        // 只统计挂在某个基版本上的提案（`base_version_id` 对应的 seq）
        sql.push_str(
            " AND p.base_version_id IN (
                 SELECT id FROM ontology_versions WHERE project_id = $1 AND seq = $2)",
        );
    }
    let mut q = sqlx::query_as::<_, (f64, String)>(&sql).bind(p.id);
    if let Some(v) = version {
        q = q.bind(v);
    }
    let rows = q.fetch_all(pool).await?;

    let mut samples: Vec<(f64, bool)> = Vec::new();
    let mut pending = 0usize;
    for (score, status) in &rows {
        match status.as_str() {
            "accepted" => samples.push((*score, true)),
            "rejected" => samples.push((*score, false)),
            _ => pending += 1,
        }
    }

    // ⚠️ **整段只往 stdout 写一次**：REQ-107 的写点登记表（
    // `tests/test_req107_source_enumeration.rs` 的 `t1`）要求 cli.rs 里
    // **每一处** stdout 写点要么是出口、要么在白名单里。校准表是运维面，
    // 但它有十几行——一次拼好、一次写出，登记面就只剩一处。
    let mut report_lines = String::new();
    report_lines.push_str(&format!(
        "置信度校准 · project={project}{}\n",
        match version {
            Some(v) => format!(" · base_version=v{v}"),
            None => " · 全部提案".to_string(),
        }
    ));
    report_lines.push_str(&format!(
        "  方法        : {}\n", crate::domain::confidence::METHOD
    ));
    report_lines.push_str(&format!(
        "  计分条目    : {} 条（已决 {} · 待决 {} —— 待决**不进**分母）\n",
        rows.len(),
        samples.len(),
        pending
    ));

    let bins = crate::domain::confidence::calibration_bins(&samples);
    report_lines.push('\n');
    report_lines.push_str("  分箱          条数   人工接受   接受率     平均分\n");
    for b in &bins {
        let rate = match (b.acceptance_rate(), b.sufficient()) {
            (Some(r), true) => format!("{:>6.1}%", r * 100.0),
            // ⛔ 样本不足时**不印百分比**（0% / 100% 都是冒充结论）
            (Some(_), false) => "  样本不足".to_string(),
            (None, _) => "      —".to_string(),
        };
        let mean = if b.count == 0 { "—".to_string() } else { format!("{:.2}", b.mean_score) };
        report_lines.push_str(&format!(
            "  {:<12} {:>5}   {:>7}   {}   {:>6}\n",
            b.label(),
            b.count,
            b.accepted,
            rate,
            mean
        ));
    }
    report_lines.push('\n');
    if samples.is_empty() {
        report_lines.push_str(
            "  结论        : **样本不足**（没有任何已决的计分条目）—— 不输出命中率结论。\n\
             \x20             先跑一轮 propose import → review decide（业务轨），再回来校准。",
        );
    } else if let Some(ece) = crate::domain::confidence::expected_calibration_error(&bins) {
        report_lines.push_str(&format!(
            "  结论        : 样本充足（每箱 ≥ {} 条）· ECE = {:.2}\n",
            crate::domain::confidence::MIN_BIN_SAMPLES,
            ece
        ));
        report_lines.push_str(
            "  读法        : 接受率 < 平均分 = 分数偏高（该收紧）；> = 分数偏低（可以放宽），\n\
             \x20             但**调权重必须先有覆盖率**（设计概要 §九 G4：本采样只验存在性）",
        );
    } else {
        let thin: Vec<String> = bins
            .iter()
            .filter(|b| !b.sufficient())
            .map(|b| format!("{}（{} 条）", b.label(), b.count))
            .collect();
        report_lines.push_str(&format!(
            "  结论        : **样本不足** —— 以下分箱 < {} 条：{}。不输出命中率结论、不算 ECE。",
            crate::domain::confidence::MIN_BIN_SAMPLES,
            thin.join("、")
        ));
    }
    println!("{report_lines}");
    Ok(())
}

async fn concept_cmd(pool: &PgPool, cmd: ConceptCmd) -> anyhow::Result<()> {
    // ⭐ REQ-115 · **建议层**先分流：它只读快照、只产出声明文件，绝不写库。
    // 放在这里而不是塞进下面那段 import 逻辑里，是为了让「建议」与「权威」
    // 在代码上也分得开（(R1)：算法不得冒充人签过的声明）。
    if let ConceptCmd::Suggest {
        project,
        version,
        out,
        ai,
        budget_tokens,
        cache,
    } = &cmd
    {
        if *ai {
            // REQ-118 · AC-004 / AC-007：AI 语义分组。
            return concept_suggest_ai(
                pool,
                project,
                *version,
                out.as_deref(),
                *budget_tokens,
                cache.as_deref(),
            )
            .await;
        }
        // 算法版是**降级路径**：预算/缓存参数只对 `--ai` 有意义。
        // ⛔ 不静默忽略 —— 给了就必须起作用，否则调用方会以为这些闸生效了。
        if budget_tokens.is_some() || cache.is_some() {
            anyhow::bail!(
                "--budget-tokens / --cache 只对 `concept suggest --ai` 有意义：\n\
                 算法版（`source = rule`）是**确定性**的，不调用模型、没有 token 可省、\
                 也没有指纹缓存可命中。\n\
                 要跑 AI 语义分组请加 `--ai`（同时需要 --out 与 LLM 三件套）。"
            );
        }
        return concept_suggest(pool, project, *version, out.as_deref()).await;
    }

    let ConceptCmd::Digest { file } = cmd else {
        let ConceptCmd::Import { file, project, base_version, round } = cmd else {
            unreachable!("ConceptCmd 只有三个变体；Suggest 已在上面分流")
        };
        // ⭐ REQ-118 · A-1（P4）：溯源要记**文件本身**的 sha256，所以先按字节读，
        // 再解析。`read_to_string` 拿不到「文件原始字节」这个身份（BOM/行尾差异会漂）。
        let file_bytes = std::fs::read(&file)
            .map_err(|e| anyhow::anyhow!("读不到 {}：{e}", file.display()))?;
        let file_sha256 = crate::extract::sample::sha256_hex(&file_bytes);
        let text = String::from_utf8(file_bytes).map_err(|e| {
            anyhow::anyhow!("概念声明 {} 不是 UTF-8 文本，拒绝解析：{e}", file.display())
        })?;
        let decl = crate::domain::concept::ConceptDeclaration::parse(&text)?;
        // **fail-closed**：指纹对不上 = 有人改了内容没升版本。
        // 不静默接受，也不「顺手帮他升一级」—— 那等于替他做版本决定。
        decl.verify()?;

        // ⭐ REQ-115 · C-1：组字段是**客户可见**的（`/api/graph` 的 `groups[]` 与
        // 业务总览的 `summary`），所以在**导入时**就过客户发布闸 —— 坏声明根本进不来，
        // 而不是流到快照/出图阶段才让整份 500。⛔ 不静默剔除、⛔ 不改写字段。
        let bad_names = crate::domain::concept::scan_group_names(&decl.groups);
        if !bad_names.is_empty() {
            let listed: Vec<String> = bad_names.iter().map(|h| format!("  - {h}")).collect();
            anyhow::bail!(
                "概念声明被拒绝导入：有 {} 处组字段会撞上客户发布闸（这些字段会出现在 \
                 /api/graph 的 groups[] 与业务总览的 summary 里）——\n{}\n\
                 请改措辞后重算指纹（ontok concept digest --file …）再导入。\
                 ⛔ 不会静默剔除，也不会替你改写。",
                bad_names.len(),
                listed.join("\n")
            );
        }

        let projects = PgProjectStore::new(pool.clone());
        let proposals = PgProposalStore::new(pool.clone());
        let p = projects.get_or_create(&project).await?;
        let base = projects.version_by_seq(p.id, &p.name, base_version).await?;
        if base.frozen_at.is_some() {
            anyhow::bail!("base 版本 v{base_version} 已冻结，不能在其上提案");
        }

        // 抽取来源 = 声明的 `declared_by` + 版本。**不是**「我」——
        // 声明是谁做的就记谁（§8.4：版本漂移必须可见）。
        //
        // `source = Declared`（ADR-003 决策 (b) · REQ-022）：这条值是从一份
        // **外部声明文件**读进来的，系统**未**验证其真伪。
        let eb = ExtractedBy {
            agent: decl.declared_by.clone(),
            version: decl.version.clone(),
            round,
            failed_groups: vec![],
            source: Some(ProvenanceSource::Declared),
        };

        let mut items = Vec::new();
        for (i, it) in decl.to_items().iter().enumerate() {
            items.push(NewItem::from_json(it, i)?);
        }

        let row = proposals
            .create(
                p.id,
                NewProposal {
                    base_version_id: base.id,
                    round,
                    extracted_by: Some(eb.clone()),
                },
                &items,
            )
            .await?;

        // ⭐ REQ-118 · A-1（P4）：**产物侧**记下「这份提案用的是哪份声明、谁签的」。
        //
        // 形状与 `inputs.exclusions` 同构：`file` / `sha256` / `declared_by` / `reviewed_by`，
        // 外加声明自己的 `version` 与 `confirmed_by`。四个「签名类」字段**必须存在**，
        // 值可以是 `null`（现在没人签）—— 「没人签」与「没有这一栏」是两件事。
        proposals
            .set_inputs(
                row.id,
                &serde_json::json!({
                    "concepts": {
                        "file": file.display().to_string(),
                        "sha256": file_sha256,
                        "version": &decl.version,
                        "declared_by": &decl.declared_by,
                        "confirmed_by": &decl.confirmed_by,
                        "reviewed_by": &decl.reviewed_by,
                    }
                }),
            )
            .await?;
        let rows = proposals.items(row.id).await?;
        let alias = proposal_alias(pool, p.id, row.id).await;
        println!(
            "概念声明 v{} 已导入提案 {alias}（项目 {} · 基于 v{}）—— 共 {} 个概念组，待双轨审核",
            decl.version,
            p.name,
            base_version,
            rows.len()
        );
        println!("  聚类规则：{}", decl.rule);
        match decl.confirmed_by.as_deref() {
            Some(who) => println!("  业务确认：{who}"),
            None => println!("  ⚠ 业务尚未确认 —— 这是**过渡声明**，不是结论"),
        }
        for r in &rows {
            println!("  [{}] {} {}", r.seq, r.kind, r.target_key);
        }
        println!("  下一步：ontok review show --proposal {alias} --project {}", p.name);
        return Ok(());
    };

    let text = std::fs::read_to_string(&file)
        .map_err(|e| anyhow::anyhow!("读不到 {}：{e}", file.display()))?;
    let decl = crate::domain::concept::ConceptDeclaration::parse(&text)?;
    let d = crate::domain::concept::content_digest(&decl.groups, &decl.rule, &decl.declared_by);
    println!("内容指纹\t{d}");
    println!("当前 version\t{}", decl.version);
    println!(
        "契约\t{}",
        match decl.verify() {
            Ok(()) => "一致（可直接 import）".to_string(),
            Err(e) => format!("不一致 —— 改内容必须改版本：{e}"),
        }
    );
    Ok(())
}

/// REQ-115 · `concept suggest` —— **建议层**：从关系拓扑算候选分组。
///
/// # 它**不写库**（AC-004）
///
/// 全程只有两条库调用：`get_by_name`（项目）与 `version_by_seq` + `get_snapshot`
/// （读快照）。没有 `INSERT` / `UPDATE`，也没有借道 `proposals`。
/// 这条不是靠人记得 —— `tests/test_req115_groups.rs` 会对真库前后比对
/// `concept_groups` / `proposals` / `ontology_versions` 的行与内容。
///
/// # 为什么产物必须「可直接 import」
///
/// 建议的价值在于**省打字**，不在于绕过审核。所以产物是完整的
/// [`crate::domain::concept::ConceptDeclaration`]（含 `history` 的内容指纹），
/// 直接喂 `concept import` 即可 —— 但 import 之后仍然要双轨签 + `apply`，
/// 才可能出现在图上（AC-005 的往返）。
///
/// # 两种输出
///
/// - `--out <file>`：写文件 + 打一行回执（路径 / 组数 / 对象数）；
/// - 省略：**只**把声明 JSON 打到 stdout（管道友好，回执会把文件弄脏）。
///
/// 两种输出的字节都只来自 [`crate::domain::concept_suggest::render`] ——
/// 同一份建议不会有两个序列化点（AC-004 的逐字节相同）。
async fn concept_suggest(
    pool: &PgPool,
    project: &str,
    version: i32,
    out: Option<&std::path::Path>,
) -> anyhow::Result<()> {
    let projects = PgProjectStore::new(pool.clone());
    let store = PgStore::new(pool.clone());

    let p = projects
        .get_by_name(project)
        .await?
        .ok_or_else(|| anyhow::anyhow!("项目不存在：{project}"))?;
    let v = projects.version_by_seq(p.id, &p.name, version).await?;
    let snap = store
        .get_snapshot(v.id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("v{version} 没有快照 —— 先从源库抽取并落库"))?;

    // 摊平快照：实体（key + 展示名）与关系（两端 key）。**只搬运，不判断。**
    let entities: Vec<(String, String)> = snap
        .entity_types
        .iter()
        .map(|e| (e.key.clone(), e.display_name.clone()))
        .collect();
    let links: Vec<(String, String)> = snap
        .link_types
        .iter()
        .map(|l| (l.from_type.clone(), l.to_type.clone()))
        .collect();

    let decl = crate::domain::concept_suggest::suggest(
        &crate::domain::concept_suggest::SuggestInput {
            entities: &entities,
            links: &links,
        },
    );
    // 序列化只有这一处（AC-004）。
    let text = crate::domain::concept_suggest::render(&decl)?;

    match out {
        Some(path) => {
            std::fs::write(path, text.as_bytes())
                .with_context(|| format!("写不出 {path:?}"))?;
            // 回执一次拼好、一次写出：stdout 写点因此只有一处（REQ-107 的写点登记表）。
            let mut receipt = String::new();
            receipt.push_str(&format!(
                "已写出建议声明：{}（项目 {} · 基于 v{} · {} 个组 / {} 个对象 · **未写库**）\n",
                path.display(),
                p.name,
                version,
                decl.groups.len(),
                entities.len()
            ));
            for g in &decl.groups {
                receipt.push_str(&format!("  {} ← {} 个对象\n", g.display_name, g.members.len()));
            }
            receipt.push_str(&format!(
                "  来源：{}（source = rule · reviewed_by = null，没人审过）\n",
                decl.declared_by
            ));
            receipt.push_str(&format!(
                "  下一步：ontok concept import --file {} --project {} --base-version <未冻结的版本>\n",
                path.display(),
                p.name
            ));
            println!("{receipt}");
        }
        // 管道友好：只打 JSON 本体（回执会混进 `> d.json`）。
        None => print!("{text}"),
    }
    Ok(())
}

// ══════════════════════════════════════════════════════════════
// REQ-118 · `concept suggest --ai` —— 决策点三：AI 语义分组
//
// 与上一条（算法版）的**唯一**相同点是「读同一版快照、产同一形状的声明」；
// 其余的差别都要在产物里看得见（AC-007）：`source = "ai"`、`decided_by = "ai:<model>"`、
// `reviewed_by = null`、文件顶层多一个 `proposals[]`（ADR-005 的形状）。
//
// ⛔ 它**不写库**：只有 get_by_name / version_by_seq / get_snapshot 三条读。
// ══════════════════════════════════════════════════════════════

/// **`concept suggest --ai`** —— AI 语义分组提案。
async fn concept_suggest_ai(
    pool: &PgPool,
    project: &str,
    version: i32,
    out: Option<&std::path::Path>,
    budget_tokens: Option<u64>,
    cache: Option<&std::path::Path>,
) -> anyhow::Result<()> {
    use crate::extract::ai_group as ag;
    use crate::extract::proposal as prop;

    // P1：提案必须落盘。算法版可以只打 stdout，AI 版不行 —— 一份没落盘的提案
    // 进不了评审记录，而 `reviewed_by` 要能回到一个具体文件。
    let out = out.ok_or_else(|| prop::missing_out_error("concept suggest --ai"))?;
    prop::require_out_is_file(out)?;
    let budget = prop::require_proposal_budget(budget_tokens)?;
    if budget_tokens.is_none() {
        eprintln!("· {}", prop::default_budget_notice(budget));
    }

    let projects = PgProjectStore::new(pool.clone());
    let store = PgStore::new(pool.clone());
    let p = projects
        .get_by_name(project)
        .await?
        .ok_or_else(|| anyhow::anyhow!("项目不存在：{project}"))?;
    let v = projects.version_by_seq(p.id, &p.name, version).await?;
    let snap = store
        .get_snapshot(v.id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("v{version} 没有快照 —— 先从源库抽取并落库"))?;
    if snap.entity_types.is_empty() {
        anyhow::bail!(
            "v{version} 的快照里没有任何实体 —— 没有可分组的对象。\
             先确认抽取真的产出了实体（看 proposal.json / stats）；空快照上跑分组\
             会得到一份「0 个组」的声明，那不是判断，是没数据。"
        );
    }

    let input = ag::GroupInput::from_snapshot(&snap);
    let cache_path = cache
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| default_cache_path(out));
    let mut mem_cache = prop::load_cache_file(&cache_path)?;
    let mut llm = open_proposal_llm()?;
    let prompts = crate::extract::ai_semantic::Prompts::from_system(ag::SYSTEM_PROMPT);

    let (groups, stats) = ag::suggest_all(
        llm.transport.as_mut(),
        &mut mem_cache,
        &input,
        &llm.model,
        &prompts,
        budget,
        llm.max_tokens,
    )?;
    // 先落缓存，再判预算（花掉的钱对应的回答必须留在盘上）。
    let cache_json = prop::cache_to_json(&mem_cache)?;
    write_local(&cache_path, cache_json.as_bytes())?;
    prop::ensure_within_budget("concept suggest --ai", &stats, budget, &cache_path)?;

    // N-1：全部批次失败 ⇒ 不写声明（空声明 = 0 个组，导入后会显示成功）。
    if stats.completeness() == prop::Completeness::AllFailed {
        return Err(prop::all_batches_failed_error(
            "concept suggest --ai",
            &stats,
            &cache_path,
        ));
    }

    let decl = ag::build_declaration(&groups, &llm.model);
    let header = ag::header(&llm.model, &prompts.sha256, input.digest());
    let text = ag::render_proposal(&decl, &groups, &header, &stats)?;
    write_local(out, text.as_bytes())?;

    // N-1：部分失败 ⇒ 文件留、退出码非零；⛔ 下面「可直接 import」的提示也不会打印。
    if stats.completeness() == prop::Completeness::Partial {
        return Err(prop::partial_failure_error("concept suggest --ai", &stats, out));
    }

    let members_total: usize = groups.iter().map(|g| g.members.len()).sum();
    let values: Vec<f64> = groups.iter().map(|g| g.item.confidence).collect();
    let low: Vec<String> = groups
        .iter()
        .filter(|g| g.item.is_low_confidence())
        .map(|g| format!("{}={:.2}", g.display_name, g.item.confidence))
        .collect();
    let ungrouped: Vec<&str> = {
        let grouped: std::collections::BTreeSet<&str> = groups
            .iter()
            .flat_map(|g| g.members.iter().map(String::as_str))
            .collect();
        input
            .entities
            .iter()
            .map(|e| e.key.as_str())
            .filter(|k| !grouped.contains(k))
            .collect()
    };
    eprintln!("✓ AI 语义分组提案（AI 先提，人审核后才会进本体）");
    eprintln!(
        "  输入        : 项目 {} v{}（{} 个实体 · {} 条关系）",
        p.name,
        version,
        input.entities.len(),
        input.links.len()
    );
    eprintln!(
        "  提案        : {} 个组 / {} 个成员（丢 {} 条）",
        groups.len(),
        members_total,
        stats.rejected_total()
    );
    if !stats.rejected.is_empty() {
        let parts: Vec<String> = stats.rejected.iter().map(|(k, n)| format!("{k} {n}")).collect();
        eprintln!("  丢弃明细    : {}", parts.join(" · "));
        for s in stats.rejection_samples.iter().take(3) {
            eprintln!("                {s}");
        }
    }
    for g in &groups {
        eprintln!("    {} ← {} 个对象", g.display_name, g.members.len());
    }
    if !ungrouped.is_empty() {
        eprintln!(
            "  未分组      : {} 个（模型没把握的不硬凑 —— 图上照常显示为未分组）",
            ungrouped.len()
        );
    }
    eprintln!(
        "  置信度      : {}（低置信 < {}：{} 条）",
        confidence_line(&values),
        prop::low_confidence_label(),
        stats.low_confidence
    );
    if !low.is_empty() {
        eprintln!("  低置信清单  : {}", low.join(" · "));
    }
    eprintln!(
        "  缓存        : 命中 {} / {} 批（调用 {} 次 · 失败 {} 次）",
        stats.cache_hits, stats.requested, stats.calls, stats.failed_calls
    );
    eprintln!(
        "  token       : 本轮 {} / 上限 {}（回放历史 {}）· 输出上限 {}",
        stats.tokens_spent, budget, stats.tokens_replayed, llm.max_tokens
    );
    eprintln!(
        "  完整性      : complete = true（失败批次 0 / {}）",
        stats.requested
    );
    eprintln!(
        "  模型        : {}（source = ai · decided_by = ai:{} · reviewed_by = null）",
        llm.model, llm.model
    );
    eprintln!("  声明契约    : digest 一致（可直接 concept import）");
    eprintln!("  产物        : {}", out.display());
    eprintln!("  提案 sha256 : {}", prop::sha256_text(&text));
    eprintln!("  缓存文件    : {}", cache_path.display());
    eprintln!(
        "  下一步      : ontok concept import --file {} --project {} --base-version <未冻结的版本>",
        out.display(),
        p.name
    );
    Ok(())
}

async fn project_cmd(pool: &PgPool, cmd: ProjectCmd) -> anyhow::Result<()> {
    let s = PgProjectStore::new(pool.clone());
    match cmd {
        ProjectCmd::New { name } => {
            let p = s.get_or_create(&name).await?;
            println!("项目 {} id={}", p.name, p.id);
        }
        ProjectCmd::List => {
            let rows: Vec<(String, Uuid)> = sqlx::query_as("SELECT name, id FROM projects ORDER BY name")
                .fetch_all(pool)
                .await?;
            for (n, i) in rows {
                println!("{n}\t{i}");
            }
        }
    }
    Ok(())
}

async fn version_cmd(pool: &PgPool, cmd: VersionCmd) -> anyhow::Result<()> {
    let s = PgProjectStore::new(pool.clone());
    // 先取名字再 match —— 直接在 match 里 borrow 会让 cmd 被部分 move，
    // 后面再 `match cmd` 就用不了了。
    let pname = match &cmd {
        VersionCmd::New { project }
        | VersionCmd::List { project }
        | VersionCmd::Freeze { project, .. } => project.clone(),
    };
    let p = s
        .get_by_name(&pname)
        .await?
        .ok_or_else(|| anyhow::anyhow!("项目不存在：{pname}"))?;

    match cmd {
        VersionCmd::New { .. } => {
            let v = s.new_version(p.id).await?;
            println!("新版本 seq={} id={}", v.seq, v.id);
        }
        VersionCmd::List { .. } => {
            for v in s.list_versions(p.id).await? {
                println!(
                    "v{}\t{}\t{}",
                    v.seq,
                    v.id,
                    if v.frozen_at.is_some() { "已冻结" } else { "未冻结" }
                );
            }
        }
        VersionCmd::Freeze { version, .. } => {
            // ⭐ REQ-124 · 与 `POST /api/freeze` **同一个**实现
            // （[`crate::console::ops::freeze_version_core`]，内部调
            // `PgProjectStore::freeze`）。重复冻结在 CLI 仍是幂等成功（历史行为），
            // HTTP 层把它翻成 409（R5）。
            let out = crate::console::ops::freeze_version_core(pool, &pname, version).await?;
            println!("已冻结 v{}", out.version);
        }
    }
    Ok(())
}

/// AC-015 · 导入前的**顶层闸**：带 `importable: false` 的文件不是提案。
///
/// 为什么在 `cli` 里单独成一个函数而不是写在 `propose_cmd` 里：
/// 它是**所有**导入通道都该过的一道门（今天只有 `propose import` 消费这类文件），
/// 而「写在流程中间的一行 if」没法被单测钉住 —— 下一步有人重排 `propose_cmd`
/// 就可能把它挪没了。判据要能单独执行。
///
/// 语义：
/// - **缺省** ⇒ 可导入（REQ-108 之前的产物没有这个字段）；
/// - **恰好 `true`** ⇒ 可导入；
/// - 其它（`false` / `"false"` / `0` / `null`）⇒ **拒绝**（fail-closed：
///   一个说不清自己能不能导的文件，不该被猜成能导）。
pub fn ensure_importable(path: &std::path::Path, doc: &Value) -> anyhow::Result<()> {
    if let Some(v) = doc.get("importable") {
        if v.as_bool() != Some(true) {
            anyhow::bail!(
                "{} 顶层声明 importable = {} —— 这是被拒绝档，不能导入。\n\
                 拒绝档（<0.30）的关系只记 veto 理由、不进本体；可导入的是主提案 \
                 proposal.json（高/中档）与待查清单 proposal-pending.json（低档，\
                 产出时就写 importable: true）。",
                path.display(),
                v
            );
        }
    }
    Ok(())
}

async fn propose_cmd(pool: &PgPool, cmd: ProposeCmd) -> anyhow::Result<()> {
    let ProposeCmd::Import { file } = cmd;
    let text = std::fs::read_to_string(&file)
        .map_err(|e| anyhow::anyhow!("读不到 {}：{e}", file.display()))?;
    let doc: Value = serde_json::from_str(&text)?;

    // ⭐ REQ-124 · 主体逻辑搬到 [`import_proposal_core`]（控制面的 `POST /api/import`
    // 调**同一个**函数）。本函数只剩「读文件 → 打印回执」：打印的几行与搬走前逐字相同
    // （`tests/test_req107_source_enumeration.rs` 的 `NON_CUSTOMER_WRITES` 钉着它们）。
    let receipt = import_proposal_core(pool, &doc, &file, None, false).await?;

    if let Some(rec) = &receipt.exclusions {
        println!(
            "  排除记录    : {} 张 · {} v{} · sha256 {}…（判的人 {}；签的人 {}）",
            rec.count,
            rec.file,
            rec.version,
            &rec.sha256[..12.min(rec.sha256.len())],
            rec.declared_by,
            rec.reviewed_by.as_deref().unwrap_or("—")
        );
    }
    println!(
        "提案 {alias} 已导入，共 {} 个条目：",
        receipt.items.len(),
        alias = receipt.alias
    );
    for r in &receipt.items {
        println!("  [{}] {} {}", r.seq, r.kind, r.target_key);
    }
    match &receipt.extracted_by {
        None => println!("  （本提案无 extracted_by = 人工修订）"),
        Some(eb) => println!("  （抽取来源：{} {}）", eb.agent, eb.version),
    }
    Ok(())
}

/// 一条已导入条目的**回执**（序号 / 类型 / 目标 key）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct ImportItemReceipt {
    pub seq: i32,
    pub kind: String,
    pub target_key: String,
}

/// 提案里带的**抽取来源**（`extracted_by` 的两栏）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct ExtractedByReceipt {
    pub agent: String,
    pub version: String,
}

/// 提案里带的**排除记录摘要**（REQ-113 的 `inputs.exclusions`）。
///
/// 只放**溯源元数据**（文件 / 版本 / sha / 判的人 / 签的人 / 条数），
/// 不放逐条明细（那是提案载荷的一部分，`GET /api/review` 的 `exclusions` 块才给全）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct ImportExclusionsReceipt {
    pub count: usize,
    pub file: String,
    pub version: String,
    pub sha256: String,
    pub declared_by: String,
    pub reviewed_by: Option<String>,
}

/// `propose import` / `POST /api/import` 的**同一个**实现。
///
/// # 参数
///
/// - `source`：提案来源（文件路径，或 `"<inline>"`）—— 只用于错误文案与 `importable` 闸；
/// - `expected_project`：控制面给的 `{project}`。给了就**必须**与文件里的 `project`
///   一致（⛔ 不静默把 A 项目的提案导进 B 项目）；CLI 传 `None`（项目名只在文件里）；
/// - `strict`：控制面为 `true` —— AC-003 要求「提案缺 `project` / `items` ⇒ 400 + 点名」。
///   CLI 保持历史行为（缺 `items` = 空提案），所以这个差异是**请求契约**的差异，
///   不是两份实现。
///
/// # 它做的每一件事都是原来 `propose_cmd` 里的
///
/// ① `importable: false` 硬闸（AC-015）；② 项目取得/新建；③ 基版本未冻结；
/// ④ `extracted_by` 逐字段取并标 `Declared`；⑤ 条目 → `NewItem`；
/// ⑥ `PgProposalStore::create`；⑦ `inputs`（含 `exclusions`）再过一遍
/// `Record::from_inputs` 后落库。
pub async fn import_proposal_core(
    pool: &PgPool,
    doc: &Value,
    source: &std::path::Path,
    expected_project: Option<&str>,
    strict: bool,
) -> Result<ImportReceipt, crate::console::OpError> {
    use crate::console::OpError;

    // ★ AC-015：`importable: false` 必须是**硬闸**，不是文件里的约定。
    //
    // 背景（QA `qa-review.md` §7 N2）：`ontok-propose` 把「不提案」的关系写进
    // `proposal-rejected.json`，并在顶层标 `importable: false`，但 `propose import`
    // 完全不看这个字段 —— 原样导入 **EXIT=0**，10 条已知错误照样进提案表，
    // 再来一次 `review decide --accept-all` + `apply` 就进了本体。
    // **声明了却不管，比不声明更糟**：读文件的人会以为已经挡住了。
    ensure_importable(source, doc).map_err(|e| {
        OpError::bad_request(
            "PROPOSAL_NOT_IMPORTABLE",
            format!("{e:#}"),
            "这是一份**被拒绝档**的产物（`importable: false`）。\
             可导入的是主提案 `proposal.json` 与待查清单 `proposal-pending.json`。",
        )
    })?;

    let projects = PgProjectStore::new(pool.clone());
    let proposals = PgProposalStore::new(pool.clone());

    let pname = doc["project"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            OpError::bad_request(
                "PROPOSAL_MISSING_PROJECT",
                format!("提案 {} 缺顶层 `project`（或它不是非空字符串）。", source.display()),
                "提案 JSON 顶层要写 `project: \"<项目名>\"`。⛔ 不猜项目 —— \
                 猜错会把 A 的提案导进 B 的项目。",
            )
        })?;
    if let Some(want) = expected_project {
        if want.trim() != pname {
            return Err(OpError::bad_request(
                "PROJECT_MISMATCH",
                format!(
                    "请求里的 project = {want:?}，而提案里的 project = {pname:?} —— 不一致。"
                ),
                "控制面只服务 `--project` 指定的那个项目：把提案的 `project` 改成它，\
                 或换一个 `--project` 再起服务。⛔ 不会把 A 的提案静默导进 B。",
            ));
        }
    }
    if strict && !doc.get("items").is_some_and(|v| v.is_array()) {
        return Err(OpError::bad_request(
            "PROPOSAL_MISSING_ITEMS",
            format!("提案 {} 缺 `items`（或它不是数组）。", source.display()),
            "提案 JSON 顶层要有 `items: [...]`；空提案不是「导入成功」，是「还没做决定」。",
        ));
    }

    let p = projects.get_or_create(pname).await.map_err(OpError::db)?;

    let base_seq = doc["base_version"].as_i64().unwrap_or(1) as i32;
    let base = projects
        .version_by_seq(p.id, &p.name, base_seq)
        .await
        .map_err(OpError::db)?;
    if base.frozen_at.is_some() {
        return Err(OpError::conflict(
            "BASE_VERSION_FROZEN",
            format!("提案基于 v{base_seq}，而 v{base_seq} 已冻结，不能在其上提案。"),
            "冻结的版本只读。新开一版再提案：`ontok version new --project …`，\
             然后把提案的 `base_version` 改成新版本。",
        ));
    }

    // ★ REQ-022 / ADR-003 (b)：**逐字段**取，并给溯源值**标注来源通道**。
    //
    // 为什么不再整块 `serde_json::from_value::<ExtractedBy>(v)`：
    // 提案侧的溯源块与快照侧**不是同一个类型**
    // （REQ-022 已把它改名 `extract::propose::ProposalProvenance`），
    // **字段名相同、类型不同**：
    //
    // | 字段 | 提案侧 | 快照侧 |
    // | --- | --- | --- |
    // | `source` | 勘查留痕**字符串**（`introspect … read_only=on`） | **来源通道枚举**（自产 / 外部声明）|
    // | `failed_groups` | 结构化**对象**数组（`{group_id, table, attempts, reason}`）| **字符串**数组 |
    //
    // 整块转换要么直接报错，要么（更糟）在某个字段类型碰巧兼容时
    // **把「勘查留痕」当成「来源通道」写进库** ——
    // 后者就是一次静默的语义错位，正是 ADR-003 要消灭的那一类。
    // 逐字段取 = 两个形状的区别**写在代码里**，不靠谁记得。
    let eb = doc
        .get("extracted_by")
        .filter(|v| !v.is_null())
        .map(|v| -> Result<ExtractedBy, crate::console::OpError> {
            let bad = |msg: String| {
                OpError::bad_request(
                    "PROPOSAL_BAD_EXTRACTED_BY",
                    format!("提案 {} 的 {msg}", source.display()),
                    "`extracted_by` 要么整块省略（= 人工修订），要么给全 \
                     `{agent, version, round, failed_groups?}`；`version` 必须是具体版本号。",
                )
            };
            let agent = v["agent"]
                .as_str()
                .ok_or_else(|| bad("extracted_by.agent 缺失或不是字符串".into()))?
                .to_string();
            let version = v["version"]
                .as_str()
                .ok_or_else(|| bad("extracted_by.version 缺失或不是字符串（§8.4：必须是具体版本号）".into()))?
                .to_string();
            let round = v["round"]
                .as_i64()
                .ok_or_else(|| bad("extracted_by.round 缺失或不是整数".into()))? as i32;
            // failed_groups 两种形状都收：快照侧的字符串，或提案侧的结构化对象
            // （取 `group_id`）。收结构化对象是为了让**本工具自己产出的**提案
            // （`ontok-propose` 写出来的就是对象数组）能导得进来 ——
            // 之前这里会整块反序列化失败，于是「工具产的提案导不进去、
            // 只有手写的能导」，那是个没人明说的反向激励。
            // ⚠️ 收下不等于担保：ADR-003 D2 说得很清楚，这条通道的声明
            // **系统无法验证**，所以下面一律标 `Declared`。
            let mut failed_groups = Vec::new();
            for f in v["failed_groups"].as_array().into_iter().flatten() {
                let id = match f {
                    serde_json::Value::String(s) => s.clone(),
                    other => other["group_id"]
                        .as_str()
                        .ok_or_else(|| {
                            bad(format!(
                                "extracted_by.failed_groups 里有一项既不是字符串也没有 group_id：{other}"
                            ))
                        })?
                        .to_string(),
                };
                failed_groups.push(id);
            }
            Ok(ExtractedBy {
                agent,
                version,
                round,
                failed_groups,
                // ★ ADR-003 (b)：这条值来自**外部 JSON 文件**，不是本工具跑出来的。
                source: Some(ProvenanceSource::Declared),
            })
        })
        .transpose()?;

    let mut items = Vec::new();
    for (i, it) in doc["items"].as_array().into_iter().flatten().enumerate() {
        items.push(NewItem::from_json(it, i).map_err(|e| {
            OpError::bad_request(
                "PROPOSAL_BAD_ITEM",
                format!("提案 {} 的第 {} 条条目不合法：{e:#}", source.display(), i + 1),
                "条目的形状由 `ontok-propose` 决定：kind / target_key / payload 三类字段；\
                 手改过的提案请对照一份工具产物。",
            )
        })?);
    }

    let row = proposals
        .create(
            p.id,
            NewProposal {
                base_version_id: base.id,
                round: doc["round"].as_i64().unwrap_or(1) as i32,
                extracted_by: eb.clone(),
            },
            &items,
        )
        .await
        .map_err(OpError::db)?;

    // ── REQ-113 · 提案的排除审计段（`inputs.exclusions`）落库 ─────────────
    //
    // 审核界面读的是**库里这一行**（`GET /api/review` 的 `exclusions` 块），
    // 而不是回头去解析一遍 `exclusions.json` —— 纪律 H：「哪些表被排除」
    // 只有一个实现，界面只负责显示那份结果。
    //
    // ⭐ BA 裁定「单一文件」：逐条明细**就在 proposal.json 里**
    // （`inputs.exclusions = {file, version, sha256, declared_by, reviewed_by,
    //   rule, counts, excluded[], flagged[], stale[]}`）。
    // 所以这里**没有**侧车、也没有第二个文件的 sha 绑定 —— 只读那一份。
    //
    // ⚠️ 落库前**再过一遍校验**（与 `--exclude` 那侧同一套判据，`Record::from_inputs`）：
    // 这份 JSON 是**别人给的**，可能被手改过。一条 `reason` 被清空、
    // 或 `counts` 与明细对不上的记录若进了库，审核界面会把一份假审计当成真的显示出来。
    let mut exclusions = None;
    if let Some(inputs) = doc.get("inputs").filter(|v| v.is_object()) {
        let rec = crate::extract::exclusions::Record::from_inputs(inputs).map_err(|e| {
            OpError::bad_request(
                "PROPOSAL_BAD_INPUTS",
                format!("提案 {} 的 `inputs.exclusions` 不合法：{e:#}", source.display()),
                "排除记录必须与抽取那一轮一致（条数 / sha256 / 逐条理由）。\
                 ⛔ 被手改过的记录不能进库 —— 界面会把它当成真审计显示。",
            )
        })?;
        match rec {
            Some(rec) => {
                exclusions = Some(ImportExclusionsReceipt {
                    count: rec.count(),
                    file: rec.file.clone(),
                    version: rec.version.clone(),
                    sha256: rec.sha256.clone(),
                    declared_by: rec.declared_by.clone(),
                    reviewed_by: rec.reviewed_by.clone(),
                });
                // ⭐ REQ-118 · A-1：**保留 `inputs` 里的其它键**（`names` 等）。
                // 上一版这里只写 `{ "exclusions": rec }` —— 于是「既给 --names 又给
                // --exclude」的提案，落库后**名字溯源整块消失**（只剩排除那一段）。
                // 现在把校验后的 `exclusions` 写回**原对象**，其它输入记录原样保留。
                let mut stored = inputs.as_object().cloned().unwrap_or_default();
                stored.insert(
                    "exclusions".to_string(),
                    serde_json::to_value(&rec)
                        .context("排除记录序列化失败")
                        .map_err(OpError::db)?,
                );
                proposals
                    .set_inputs(row.id, &Value::Object(stored))
                    .await
                    .map_err(OpError::db)?;
            }
            None => {
                // 有 `inputs` 但没有 `exclusions`（例如只有 `names`）：原样留档，不丢。
                proposals
                    .set_inputs(row.id, inputs)
                    .await
                    .map_err(OpError::db)?;
            }
        }
    }

    let rows = proposals.items(row.id).await.map_err(OpError::db)?;
    let alias = proposal_alias(pool, p.id, row.id).await;
    Ok(ImportReceipt {
        project: p.name,
        proposal_id: row.id,
        alias,
        items: rows
            .iter()
            .map(|r| ImportItemReceipt {
                seq: r.seq,
                kind: r.kind.clone(),
                target_key: r.target_key.clone(),
            })
            .collect(),
        extracted_by: eb.map(|e| ExtractedByReceipt {
            agent: e.agent,
            version: e.version,
        }),
        exclusions,
    })
}

/// `propose import` / `POST /api/import` 的回执（**同一个**类型）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct ImportReceipt {
    pub project: String,
    pub proposal_id: Uuid,
    /// 提案别名（`p1` / `p2` …，与 `review show` 的抬头同源）。
    pub alias: String,
    pub items: Vec<ImportItemReceipt>,
    pub extracted_by: Option<ExtractedByReceipt>,
    pub exclusions: Option<ImportExclusionsReceipt>,
}

async fn review_cmd(pool: &PgPool, cmd: ReviewCmd) -> anyhow::Result<()> {
    let proposals = PgProposalStore::new(pool.clone());

    match cmd {
        ReviewCmd::Decide { proposal, item, track, accept, reject, project, accept_all } => {
            if accept_all {
                let p = resolve_proposal_arg(pool, &proposal, project.as_deref()).await?;
                let items = proposals.items(p).await?;
                if items.is_empty() {
                    anyhow::bail!("提案里没有任何条目");
                }
                for it in &items {
                    // 两轨都接受 —— 逐条敲 `--item N --track biz/model` 的等价物，
                    // 走的是同一条 `set_decision` 路径与同一套领域校验
                    for t in [Track::Biz, Track::Model] {
                        proposals.set_decision(it.id, t, Decision::Accepted).await?;
                    }
                }
                println!("提案 {proposal} 的 {} 个条目 × 2 轨 → accepted", items.len());
                return Ok(());
            }
            if accept == reject {
                anyhow::bail!("--accept 与 --reject 必须且只能给一个");
            }
            let item = item.context("请给 --item <序号>，或改用 --accept-all 批量接受")?;
            let d = if accept { Decision::Accepted } else { Decision::Rejected };
            let track = track.context("请给 --track <biz|model>，或改用 --accept-all 批量接受")?;
            let t = match track.as_str() {
                "biz" => Track::Biz,
                "model" => Track::Model,
                other => anyhow::bail!("--track 只能是 biz 或 model，收到 {other}"),
            };
            let p = resolve_proposal_arg(pool, &proposal, project.as_deref()).await?;
            // ⭐ REQ-111 · **不再**在这里自己查条目 + 写库：那条路径与审核界面的
            // `POST /api/decide` 必须**是同一个函数**（纪律 H）。`decide_item` 里
            // 收着「序号 → 条目 → 领域状态机 → 落库 + 审计」四步；本分支只负责
            // 解析别名与打印。页面签的和 CLI 签的因此在结构上不可能不一致。
            proposals
                .decide_item(p, item, t, d, crate::db::proposal::VIA_CLI)
                .await?;
            println!("item {} 的 {} 轨 → {}", item, track, d.as_str());
        }
        ReviewCmd::Serve { project, proposal, bind, token } => {
            // 绑地址先解析：给错格式要报人话，不要把 axum 的栈倒给用户。
            let addr: std::net::SocketAddr = bind.parse().with_context(|| {
                format!("监听地址看不懂：{bind:?}。要写成 127.0.0.1:8788 这样的样子。")
            })?;
            // 口令两种来源：显式 `--token` 优先，其次环境变量（与 `serve` 同一个变量）。
            // 两处都没有 ⇒ 拒绝启动，**没有开发模式旁路**。
            let token = match token {
                Some(t) => t,
                None => crate::config::require_env(crate::config::VIEW_TOKEN_ENV).map_err(|e| {
                    anyhow::anyhow!("{e}；也可以用 --token 显式给（至少 8 个字符）")
                })?,
            };
            let args = crate::review_ui::ReviewArgs { project, proposal, bind: addr, token };
            crate::review_ui::http::serve(pool, args).await?;
        }
        ReviewCmd::Show { proposal, project, confidence } => {
            let p = resolve_proposal_arg(pool, &proposal, project.as_deref()).await?;
            let row = proposals.get(p).await?.ok_or_else(|| anyhow::anyhow!("提案不存在"))?;
            let items = proposals.items(p).await?;
            let views = PgProposalStore::to_item_views(&items);
            let base = PgStore::new(pool.clone())
                .get_snapshot(row.base_version_id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("base 版本快照不存在"))?;
            let baseline: std::collections::BTreeSet<String> =
                base.entity_types.iter().map(|e| e.key.clone()).collect();
            let st = summarize(&views, &baseline);

            // REQ-056 · 建模轨投影（**FDE 通道**）。
            //
            // ⚠️ **刻意不过 `scan_modeling_terms`**：建模术语在这一轨
            // 是**要被裁决的内容**，不是泄露。客户视图的守门器只扫
            // 客户视图那条路径，不受这里影响（两者是独立输出类型）。
            //
            // ⚠️ REQ-057：这条豁免的前提判据**换过**。
            // 早先那版拿守门器扫渲染文本判「有没有建模术语」——
            // 而渲染器对每个条目无条件打 `key=`，`key` 又是建模术语
            // ⇒ `premise_holds` **恒为 true**，下面那句警告永远不会触发。
            //
            // 现在判据读**结构化溯源**：「有没有哪一栏建模字段
            // 既来自 payload 又非空」（[`audit_fde_channel`]）。
            // 与词表**无交集** ⇒ 往词表加词不会让自检变松
            // （而客户视图那一侧加词会变严 —— 两个方向不再共用一条路径）。
            // REQ-060：提案级 `extracted_by` 从**提案行**取（`ProposalItemRow`
            // 没有这一栏 —— 归属天生是提案级的）。
            //
            // ⚠️ 这里刻意用三参入口而不是两参的：两参会落成
            // `NotProvided`（渲染成「未提供」），而库里真的有这一栏 ——
            // 那是把「有」说成「没有」。
            let rv = crate::domain::review_view::project_review_track_with_provenance(
                &views,
                &st,
                row.extracted_by.as_ref(),
            );
            let text = crate::domain::review_view::render_review_text(&rv);
            let audit = crate::domain::review_view::audit_fde_channel(&rv);

            // 登记表里的前缀 —— 原文取自 `REVIEW_SHOW_PREFIX_TEMPLATE`，
            // 生产与端到端断言**共用**它（AC-002：⛔ 不许硬写在测试的 `format!` 里）。
            print!("{}", crate::cli::review_show_prefix(&proposal, &row.status));

            // ⚠️ REQ-078 · 打印与判据**同一份字节**。
            //
            // 旧接线是「`print!("{text}")` 在一处、三道判据各拿 `&text`」——
            // 两个独立取值点 ⇒ 在打印点插一次再加工，判据**结构性看不见**
            // （REQ-072 QA 的 M2 实测：全仓 738 条既有测试全绿、退出码 0、stderr 全空）。
            //
            // 现在判据在 [`emit_domain_text_with`] 的闭包里：闭包只拿得到
            // 刚写出去的那一个切片。判据侧与打印侧的**接线**由
            // `tests/test_binary_output_guard.rs` 的 G-1 源码守卫钉住。
            let violations = crate::cli::emit_domain_text_with(&text, |judged| {
                let mut violations: Vec<String> = Vec::new();
                if !audit.premise_holds {
                    eprintln!(
                        "⚠ FDE 通道自检：这份建模投影里没有任何来自 payload 的建模字段 —— 建模字段可能没打出来"
                    );
                    violations.push("FDE 通道自检：建模字段可能没打出来".to_string());
                }
                // 判据 ②：投影**有**数据、但渲染串轨/重复/漏打时由它报。
                for v in crate::domain::review_view::verify_track_separation(&rv, judged) {
                    eprintln!("⚠ 两轨分界自检：{v}");
                    violations.push(format!("两轨分界自检：{v}"));
                }
                // 判据 ③（REQ-060）：提案级归属的呈现 —— 缺行 / 重复打印 / 没表态时由它报。
                for v in crate::domain::review_view::verify_provenance_scope(&rv, judged) {
                    eprintln!("⚠ 提案级归属自检：{v}");
                    violations.push(format!("提案级归属自检：{v}"));
                }
                violations
            });

            // 退出码政策（AC-004）：**报了就不许当成功** —— 见 `EXIT_POLICY`。
            // 在 REQ-078 之前，这三行只是 `eprintln!`，退出码恒为 0。
            if let Some(code) = crate::cli::self_check_exit_code(&violations) {
                std::process::exit(code);
            }

            // ── REQ-108 · 置信度通道（AC-010 的「审核界面必须有」）─────────
            //
            // 为什么放在**渲染之后**、且不做进 `review_view`：
            // `src/domain/review_view.rs` 是 REQ-056~078 的原始字节契约所在
            // （那条渲染链有三道自检 + 真进程字节比对），**本 REQ 的红线禁改它**。
            // 分数是**审核辅助信息**，不是条目正文 —— 单独一段、显式开关，
            // 既让审核者看得到，又不动那条被钉住的输出。
            if confidence {
                let band_lines = render_confidence_table(&items);
                // ⚠️ 用 `println!` 而不是绕道 `write_stdout`：本仓的打印点守卫
                // （REQ-078 的 G-5）靠「认得出是 domain 产物就打红」来兜底，
                // 绕道等于让它**看不见**这个站点。这里老老实实用一个普通打印：
                // 它打的是一张分档表（不是客户产物绑定），守卫认出后**不登记**是正确的。
                println!("{band_lines}");
            }
        }
    }
    Ok(())
}

async fn proposal_cmd(pool: &PgPool, cmd: ProposalCmd) -> anyhow::Result<()> {
    let ProposalCmd::Apply { proposal, project } = cmd;
    let p = resolve_proposal_arg(pool, &proposal, project.as_deref()).await?;
    let applier = PgApplier::new(pool.clone());

    // apply 前先报一次状态，让「为什么没成功」在输出里看得见
    let proposals = PgProposalStore::new(pool.clone());
    let row = proposals.get(p).await?.ok_or_else(|| anyhow::anyhow!("提案不存在"))?;
    let items = proposals.items(p).await?;
    let views = PgProposalStore::to_item_views(&items);
    let base = PgStore::new(pool.clone()).get_snapshot(row.base_version_id).await?;
    if let Some(b) = &base {
        let baseline: std::collections::BTreeSet<String> =
            b.entity_types.iter().map(|e| e.key.clone()).collect();
        let st = summarize(&views, &baseline);
        if !st.passed {
            println!(
                "提案未通过：可应用 {} 条，被拦 {} 条，悬空关系 {} 条",
                st.applicable.len(),
                st.blocked.len(),
                st.dangling_relations.len()
            );
            for b in &st.blocked {
                println!("  被拦：{} —— {}", b.target, b.reason.as_deref().unwrap_or(""));
            }
        }
    }

    // ⭐ REQ-129 · 规则 7 的可见性：未签的 doc-only 条目在 apply 时被**忽略**。
    // 走 stderr 是把「没那么做」说清楚 —— 静默跳过会让「签了一条」与「什么都没签」
    // 在输出上长得一模一样（而 AC-007 判的正是这两者的差别）。
    let ignored = crate::domain::apply::unsigned_doc_only(&views);
    if !ignored.is_empty() {
        eprintln!(
            "· 未签的 doc 条目 {} 条 ⇒ apply 时**忽略**（规则 7：未签不进快照）",
            ignored.len()
        );
        for (_, target) in ignored.iter().take(8) {
            eprintln!("    {target}");
        }
    }

    match applier.apply_proposal(p).await {
        Ok(v) => {
            let s = PgProjectStore::new(pool.clone());
            let pr = PgProposalStore::new(pool.clone());
            let pid = pr.get(p).await?.unwrap().project_id;
            let seq = s.latest_version(pid).await?.map(|v| v.seq).unwrap_or(0);
            println!("提案已应用 → v{seq}（id={v}）");
        }
        Err(e) => {
            println!("应用失败：{e}");
            std::process::exit(1);
        }
    }
    Ok(())
}

/// ⭐ REQ-124 · `ontology export` / `GET /api/export/ttl` 的**共用参数**。
///
/// `tenancy` / `schema` 是**服务端路径**：CLI 来自 `--tenancy` / `--schema`，
/// 控制面来自 query string 的同名参数（文档里写明它是服务端路径）。
pub struct TtlExportParams<'a> {
    pub project: &'a str,
    pub version: i32,
    pub model_view: bool,
    pub base: &'a str,
    pub tenancy: Option<&'a std::path::Path>,
    pub schema: Option<&'a std::path::Path>,
}



/// ⭐ REQ-124 · `ontology export` 与 `GET /api/export/ttl` 的**共用读取段**。
///
/// 顺序与搬走前的 `ontology_cmd` Export 分支逐项一致（失败都是 fail-closed）：
/// 项目 → 版本 → 快照 → 租户声明 → schema → 投影。
///
/// 返回的 [`crate::console::ops::TtlPrepared`] 只装**已经准备好的输入**；
/// 渲染在两侧各自发生（CLI 走 `publish_customer`，控制面走
/// `console::ops::render_ttl_export`），但输入是同一份、渲染器是同一个。
pub async fn prepare_ttl_export(
    pool: &PgPool,
    p: TtlExportParams<'_>,
) -> Result<crate::console::ops::TtlPrepared, OpError> {
    let s = PgProjectStore::new(pool.clone());
    let store = PgStore::new(pool.clone());

    let proj = s
        .get_by_name(p.project)
        .await
        .map_err(OpError::db)?
        .ok_or_else(|| {
            OpError::not_found(
                "PROJECT_NOT_FOUND",
                format!("项目 {:?} 不存在。", p.project),
                "检查 `--project`；项目要先由 `ontok project new` 建出来。",
            )
        })?;
    let v = s
        .version_by_seq(proj.id, &proj.name, p.version)
        .await
        .map_err(|e| {
            OpError::not_found(
                "VERSION_NOT_FOUND",
                format!("项目 {:?} 没有第 {} 版：{e}", p.project, p.version),
                "`GET /api/projects` 里有每一版的 seq。",
            )
        })?;
    let snap = store
        .get_snapshot(v.id)
        .await
        .map_err(OpError::db)?
        .ok_or_else(|| {
            OpError::not_found(
                "SNAPSHOT_NOT_FOUND",
                format!("v{} 没有快照，导不出东西。", p.version),
                "快照由 `proposal apply` 产出；空版本导出的「空本体」会被当成真的没有业务。",
            )
        })?;

    // ⭐ REQ-119 · 租户声明：**读文件 + 解析**在这里，判定/落点全在
    // `domain::ttl`（纯函数）。解析失败 ⇒ 直接报错（fail-closed），
    // 不静默当成「没有声明」——那会产出一份看着有标注的本体。
    let tenancy = match p.tenancy {
        Some(path) => {
            let raw = std::fs::read_to_string(path).map_err(|e| {
                OpError::bad_request(
                    "DECLARATION_UNREADABLE",
                    format!("读不出租户声明文件 {path:?}：{e}"),
                    "`tenancy` 是**服务端路径**；文件不存在时不会退回「未标注」——\
                     那会产出一份看着像没要求标注的本体。",
                )
            })?;
            Some(crate::domain::ttl::TenancyDecl::parse(&raw).map_err(|e| {
                OpError::bad_request(
                    "DECLARATION_INVALID",
                    format!("租户声明文件 {path:?} 不合法：{e}"),
                    "形状：{\"columns\":[\"…\"],\"tables\":[\"…\"]?,\"reviewed_by\":\"…\"?}。",
                )
            })?)
        }
        None => None,
    };

    // ⭐ REQ-119 A-1 · 真实列类型（`schema`）。解析失败 ⇒ 直接报错（fail-closed）：
    // 悄悄退回启发式会让调用方以为「用的是真实类型」。
    let schema = match p.schema {
        Some(path) => {
            let raw = std::fs::read_to_string(path).map_err(|e| {
                OpError::bad_request(
                    "SCHEMA_UNREADABLE",
                    format!("读不出 schema 文件 {path:?}：{e}"),
                    "`schema` 是**服务端路径**（`ontok-introspect` 的产物）。",
                )
            })?;
            Some(crate::domain::ttl::SchemaTypes::parse(&raw).map_err(|e| {
                OpError::bad_request(
                    "SCHEMA_INVALID",
                    format!("schema 文件 {path:?} 不合法：{e}"),
                    "schema 必须是 `ontok-introspect` 写出的形状（tables[].columns[]）。",
                )
            })?)
        }
        None => None,
    };
    if schema.as_ref().is_some_and(|s| s.is_empty()) {
        eprintln!("⚠ --schema 里一列都没读到：本次仍全部走启发式（检查 tables/columns）");
    }

    // 客户视图：`--tenancy` / `--schema` 对它不生效 —— 在**渲染之前**说清楚，
    // 不让它们看起来「生效了」（R4 / AC-007）。
    if !p.model_view {
        if tenancy.is_some() {
            eprintln!("⚠ --tenancy 只在建模视图生效：客户视图 TTL 不含租户标注（R4）");
        }
        if schema.is_some() {
            eprintln!(
                "⚠ 客户视图不含列名，--schema 无法按列匹配：本文件仍走启发式（类型以建模视图为准）"
            );
        }
    }

    let model = if p.model_view {
        Some(crate::domain::model_view::project_model_track(&snap))
    } else {
        None
    };
    let customer = if p.model_view {
        None
    } else {
        let sv = snapshot_to_view(&snap);
        Some(crate::domain::view::project(&sv))
    };

    Ok(crate::console::ops::TtlPrepared {
        project: proj.name,
        version: p.version,
        base: p.base.to_string(),
        model_view: p.model_view,
        snapshot: snap,
        tenancy,
        schema,
        model,
        customer,
    })
}

async fn ontology_cmd(pool: &PgPool, cmd: OntologyCmd) -> anyhow::Result<()> {
    let s = PgProjectStore::new(pool.clone());
    let store = PgStore::new(pool.clone());

    match cmd {
        OntologyCmd::Show { project, version, model_view, html, out, sampling } => {
            let p = s.get_by_name(&project).await?.ok_or_else(|| anyhow::anyhow!("项目不存在"))?;
            let v = s.version_by_seq(p.id, &p.name, version).await?;
            let snap = store
                .get_snapshot(v.id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("v{version} 没有快照"))?;

            if model_view {
                // REQ-055 · 建模轨投影。
                //
                // 此前这里打的是**原始快照转储**（`to_string_pretty(&snap)`）——
                // 那是**存储层形状**，不是可裁决的投影：FDE 拿到的是一份
                // 数据库快照的 dump，而不是「建模轨该看的那一份」。
                // 双轨状态机（`domain::review`）让 FDE 逐项裁决建模轨，
                // 而建模轨**没有对应的视图**可看 —— 缺的就是这一环。
                //
                // ⚠️ **刻意不过发布闸**：建模术语在这一轨是**要被裁决的内容**，
                // 不是泄露。客户视图的守门器只扫客户视图那条路径，不受这里影响
                // （两者是独立输出类型）。
                //
                // ⭐ REQ-107 · AC-007：这不是「因为它没调闸所以它是建模轨」——
                // 轨道由 `OutletSpec::D1` **显式声明**（`Track::Modeling`），
                // 是否过闸由**声明**决定。把同一份产物的 spec 换成 `Track::Customer`，
                // 它当场就会被闸拦住（`pipeline` 的单元测试与
                // `tests/test_req107_track_manifest.rs` 都钉住这一点）。
                let mv = crate::domain::model_view::project_model_track(&snap);
                let mv_json = serde_json::to_string_pretty(&mv)?;
                // ⚠️ 第 2 轮：`mv_json` 直接就是管线产物（`OutletRender`），
                // 不是「先取 &str 再交给写出」—— 写出函数只收这个类型。
                let mv_json = publish_customer(
                    crate::domain::pipeline::OutletId::D1,
                    crate::domain::pipeline::Product::Text(mv_json),
                );
                // REQ-078：登记表里的站点，走唯一的出口。
                crate::cli::emit_domain_line(mv_json);
                return Ok(());
            }

            // 客户视图
            let sv = snapshot_to_view(&snap);
            // 全路径调用：`project` 这个名字被 Show { project, .. } 的
            // 模式变量遮蔽了，写 `project(&sv)` 会变成拿 String 当函数调。
            let cv = crate::domain::view::project(&sv);

            // ── HTML 输出（判读用）──
            if html {
                // 抽样清单：省略时用判读手册 §3.1 的 13 个。
                // 读文件失败**不静默降级** —— 判读人拿到的页面若少了高亮，
                // 会误以为「这就是全部重点」，那是 fail-open。
                let sampling: Vec<String> = match &sampling {
                    Some(p) => {
                        let raw = std::fs::read_to_string(p)
                            .with_context(|| format!("读不到抽样清单 {p:?}"))?;
                        raw.lines()
                            .map(|l| l.trim().to_string())
                            .filter(|l| !l.is_empty())
                            .collect()
                    }
                    None => crate::domain::html_view::DEFAULT_SAMPLING
                        .iter()
                        .map(|s| s.to_string())
                        .collect(),
                };
                // ⭐ REQ-107 · 客户出口 C1/C2：前处理（页面级清洗）+ 发布闸 + 报告
                // **全在管线里**。出口这一步只剩「声明格式（Html）」，不再自己
                // 决定要不要剔、要不要拦、stderr 报什么形状。
                //
                // C1（stdout）与 C2（--out 文件）**共用同一份产物** `html` ——
                // 它们写的是同一批字节，不是两份各自渲染的东西。
                let html = publish_customer(
                    crate::domain::pipeline::OutletId::C1,
                    crate::domain::pipeline::Product::CustomerHtml {
                        view: &cv,
                        sampling: sampling.as_slice(),
                    },
                );
                // ⚠️ 第 2 轮：`html` 就是管线产物本身（`OutletRender`）；写文件那一支
                // 只借 `.as_str()`，stdout 那一支把产物**移进**写出函数 ——
                // 类型层保证「写出去的 == 过了闸的」，出口拿不到第二份字节。
                //
                // 回执里的对象数先取出来：⛔ 不要让回执的 `println!` 也提到 `html`
                // （REQ-078 的 G-1 会把「打印点对绑定接了方法调用」判红 —— 它是对的）。
                let dropped_objects = html.dropped_object_count();

                match &out {
                    Some(p) => {
                        std::fs::write(p, html.as_str())
                            .with_context(|| format!("写不出 {p:?}"))?;
                        println!(
                            "已写出客户视图页面：{}（{} 个对象 · 抽样高亮 {} 个 · 自包含单文件，可离线打开）",
                            p.display(),
                            cv.objects.len() - dropped_objects,
                            sampling.len()
                        );
                    }
                    // ⚠️ REQ-078：stdout 分支走登记表里那唯一的出口。
                    // 写的是管线产物本身（`OutletRender` 里的字节），⛔ 没有二次加工。
                    None => crate::cli::emit_domain_text(html),
                }
                return Ok(());
            }

            // ── REQ-107 · 客户出口 C3（JSON）──
            //
            // 收口前这里是「`filter_view` 剔除 → `to_string_pretty` → `scan` → 写出」
            // 四步各自在本函数里；现在**全部**在 `domain::pipeline` 里，顺序
            // （剔除 → 序列化 → 判定 → 才产出可写出的东西）由管线保证。
            //
            // ⚠️ 剔除发生在**守门器之前**（管线的固定顺序）：真·建模术语
            // （`主键` / `基数` / `1:N`）不是标识符形态、剔不掉，照旧到达闸门并被拒。
            let json = publish_customer(
                crate::domain::pipeline::OutletId::C3,
                crate::domain::pipeline::Product::CustomerJson(&cv),
            );

            // 走到这里 = 判定通过 ⇒ 才允许写出。stdout 上只会出现这一种格式。
            crate::cli::emit_domain_line(json);
        }
        OntologyCmd::Overview { project, version, json } => {
            let p = s.get_by_name(&project).await?.ok_or_else(|| anyhow::anyhow!("项目不存在"))?;
            let v = s.version_by_seq(p.id, &p.name, version).await?;
            let snap = store
                .get_snapshot(v.id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("v{version} 没有快照"))?;

            let sv = snapshot_to_view(&snap);
            // key → 展示名。**必须用 key 定位成员**（见 overview::build_overview 文档）
            let names: std::collections::BTreeMap<String, String> = sv
                .entity_types
                .iter()
                .map(|e| (e.key.clone(), e.display_name.clone()))
                .collect();
            let cv = crate::domain::view::project(&sv);
            let ov = crate::domain::overview::build_overview(&cv, &snap.concept_groups, &names);

            // ⭐ REQ-107 · 客户出口 C4/C5：两条形态**都**在管线内过闸
            // （保留收口前的「任一条命中就整份不发」—— 那是既有行为，AC-005 原样保留），
            // 出口只声明自己要哪一条形态。
            if json {
                let json_text = publish_customer(
                    crate::domain::pipeline::OutletId::C5,
                    crate::domain::pipeline::Product::Overview(&ov),
                );
                crate::cli::emit_domain_line(json_text);
            } else {
                let text = publish_customer(
                    crate::domain::pipeline::OutletId::C4,
                    crate::domain::pipeline::Product::Overview(&ov),
                );
                crate::cli::emit_domain_text(text);
            }
            if !ov.notes.is_empty() {
                eprintln!(
                    "⚠ 总览有 {} 处降级已写进输出（未静默略过），详见末尾说明",
                    ov.notes.len()
                );
            }
        }
        // ⭐ REQ-110 · TTL 导出。
        //
        // # 唯一的一条序列化路径（纪律 H）
        //
        // 两条分支（stdout / `--out`）都只消费 `publish_customer` 的返回值 ——
        // 序列化、前处理、发布闸**全在管线里**，本函数体里没有第二份 TTL 生成逻辑。
        // HTTP 的 `/api/ontology.ttl` 走的是**同一个** `Product::CustomerTurtle`，
        // 所以「页面下载的」与「CLI 导出的」是同一批字节（AC-007）。
        OntologyCmd::Export { project, version, model_view, out, base, format, tenancy, schema } => {
            // 格式是**显式**参数：认不出就报错，不静默回退成 ttl（fail-closed）。
            if format != "ttl" {
                anyhow::bail!(
                    "只支持 --format ttl（Turtle）。收到 {format:?} —— 本轮不做 RDF 导入，也没有别的导出格式。"
                );
            }
            // ⭐ REQ-124 · **读取/投影**在 [`prepare_ttl_export`]（`GET /api/export/ttl`
            // 调**同一个**函数）⇒ 两份出口拿到的是同一份投影 + 同一个 `TtlOptions`。
            // ⚠️ 渲染这一步留在本函数里：T2/T8 要求「写出的绑定直接来自
            // `publish_customer(…)`，且 OutletId 的管线调用与写点同函数」。
            let prepared = prepare_ttl_export(
                pool,
                TtlExportParams {
                    project: &project,
                    version,
                    model_view,
                    base: &base,
                    tenancy: tenancy.as_deref(),
                    schema: schema.as_deref(),
                },
            )
            .await?;
            let object_count = prepared.object_count();
            let relation_count = prepared.relation_count();

            if prepared.model_view {
                // 建模视图：`Track::Modeling` ⇒ 显式豁免发布闸（`spec_of(D9)` 查表）。
                let model_ttl = publish_customer(
                    crate::domain::pipeline::OutletId::D9,
                    crate::domain::pipeline::Product::ModelTurtle {
                        view: prepared.model(),
                        opts: prepared.opts(),
                    },
                );
                crate::console::ops::report_ttl_export(&prepared, model_ttl.range_stats());
                match &out {
                    Some(p) => {
                        std::fs::write(p, model_ttl.as_str())
                            .with_context(|| format!("写不出 {p:?}"))?;
                        // 回执走 stderr：`--out` 的 stdout 必须留空，脚本才好接。
                        eprintln!(
                            "· 已写出建模视图 TTL：{}（{} 类 / {} 关系 · 有损项见文件头）",
                            p.display(),
                            object_count,
                            relation_count
                        );
                    }
                    None => crate::cli::emit_domain_text(model_ttl),
                }
                return Ok(());
            }

            // 客户视图（默认）：与页面看到的是**同一份**客户视图。
            let customer_ttl = publish_customer(
                crate::domain::pipeline::OutletId::C6,
                crate::domain::pipeline::Product::CustomerTurtle {
                    view: prepared.customer(),
                    opts: prepared.opts(),
                },
            );
            crate::console::ops::report_ttl_export(&prepared, customer_ttl.range_stats());
            match &out {
                Some(p) => {
                    std::fs::write(p, customer_ttl.as_str())
                        .with_context(|| format!("写不出 {p:?}"))?;
                    eprintln!(
                        "· 已写出客户视图 TTL：{}（{} 个对象 / {} 条关系 · 有损项见文件头）",
                        p.display(),
                        object_count,
                        relation_count
                    );
                }
                None => crate::cli::emit_domain_text(customer_ttl),
            }
        }
        OntologyCmd::Diff { project, a, b } => {
            let p = s.get_by_name(&project).await?.ok_or_else(|| anyhow::anyhow!("项目不存在"))?;
            let va = s.version_by_seq(p.id, &p.name, a).await?;
            let vb = s.version_by_seq(p.id, &p.name, b).await?;
            let sa = store.get_snapshot(va.id).await?.ok_or_else(|| anyhow::anyhow!("v{a} 无快照"))?;
            let sb = store.get_snapshot(vb.id).await?.ok_or_else(|| anyhow::anyhow!("v{b} 无快照"))?;
            let d = diff_snapshots(&sa, &sb);
            if d.is_empty() {
                println!("v{a} 与 v{b} 无差异");
            } else {
                println!("v{a} → v{b} 共 {} 处变化：", d.len());
                for x in &d {
                    println!("  {}", x.summary());
                }
            }
        }
    }
    Ok(())
}

// ── R2RML 映射（REQ-121）────────────────────────────────

/// ⭐ REQ-124 · `r2rml generate` / `POST /api/r2rml` 的**共用参数**。
pub struct R2rmlParams<'a> {
    pub project: &'a str,
    pub version: i32,
    /// **必填**：主键 / 唯一列 / 列存在性只能来自真实 schema。
    pub schema: &'a std::path::Path,
    pub include_inherit: bool,
    pub base: &'a str,
    pub tenancy: Option<&'a std::path::Path>,
    pub exclude: Option<&'a std::path::Path>,
    pub fk_signed: Option<&'a std::path::Path>,
}

/// 生成结果：**同一份 Turtle 字节** + 生成它的项目/版本（回执用）。
pub struct R2rmlOut {
    pub project: String,
    pub version: i32,
    pub doc: crate::domain::r2rml::R2rmlDocument,
}

/// ⭐ REQ-124 · `r2rml generate` 与 `POST /api/r2rml` 的**同一个**实现。
///
/// ⛔ 不写任何字节：stdout / `--out` 仍是出口的事（I11 / I12 的登记点在
/// [`r2rml_cmd`]）。诊断（三态统计 / 排除表 / 租户未标注）走 stderr ——
/// 两个入口看到的是同一批事实，不是「HTTP 版少报几条警告」。
///
/// ⛔ 对账失败 ⇒ 返回错误，**一个字节都不产出**（fail-closed）：半份映射比没有映射
/// 更危险 —— 它会被 R2RML 引擎当成完整映射用。
#[allow(clippy::too_many_arguments)]
pub async fn r2rml_core(
    pool: &PgPool,
    p: R2rmlParams<'_>,
) -> Result<R2rmlOut, OpError> {
    let projects = PgProjectStore::new(pool.clone());
    let store = PgStore::new(pool.clone());
    let proj = projects
        .get_by_name(p.project)
        .await
        .map_err(OpError::db)?
        .ok_or_else(|| {
            OpError::not_found(
                "PROJECT_NOT_FOUND",
                format!("项目 {:?} 不存在。", p.project),
                "检查 `--project`。",
            )
        })?;
    let v = projects
        .version_by_seq(proj.id, &proj.name, p.version)
        .await
        .map_err(|e| {
            OpError::not_found(
                "VERSION_NOT_FOUND",
                format!("项目 {:?} 没有第 {} 版：{e}", p.project, p.version),
                "`GET /api/projects` 里有每一版的 seq。",
            )
        })?;
    let snap = store
        .get_snapshot(v.id)
        .await
        .map_err(OpError::db)?
        .ok_or_else(|| {
            OpError::not_found(
                "SNAPSHOT_NOT_FOUND",
                format!("v{} 没有快照，生成不了映射。", p.version),
                "快照由 `proposal apply` 产出。",
            )
        })?;

    // 租户声明与 `ontology export --model-view` **同一份解析**（fail-closed：
    // 解析失败不当成「没有声明」，否则会得到一份看着有标注的映射）。
    let tenancy_decl = match p.tenancy {
        Some(path) => {
            let raw = std::fs::read_to_string(path).map_err(|e| {
                OpError::bad_request(
                    "DECLARATION_UNREADABLE",
                    format!("读不出租户声明文件 {path:?}：{e}"),
                    "`tenancy` 是**服务端路径**；读不到时不会当成「未标注」。",
                )
            })?;
            Some(
                crate::domain::ttl::TenancyDecl::parse(&raw).map_err(|e| {
                    OpError::bad_request(
                        "DECLARATION_INVALID",
                        format!("租户声明文件 {path:?} 不合法：{e}"),
                        "形状：{\"columns\":[\"…\"],\"tables\":[\"…\"]?,\"reviewed_by\":\"…\"?}。",
                    )
                })?,
            )
        }
        None => None,
    };

    // `--schema` 必填：主键 / 唯一列 / 列存在性只能来自真实。
    let raw = std::fs::read_to_string(p.schema).map_err(|e| {
        OpError::bad_request(
            "SCHEMA_UNREADABLE",
            format!("读不出 schema 文件 {:?}：{e}", p.schema),
            "`schema` 是**服务端路径**（`ontok-introspect` 的产物），必填：\
             ⛔ 不猜主键，也不拿启发式顶替。",
        )
    })?;
    let schema_types = crate::domain::ttl::SchemaTypes::parse(&raw).map_err(|e| {
        OpError::bad_request(
            "SCHEMA_INVALID",
            format!("schema 文件 {:?} 不合法：{e}", p.schema),
            "schema 必须是 `ontok-introspect` 写出的形状（tables[].columns[]）。",
        )
    })?;
    if schema_types.is_empty() {
        return Err(OpError::bad_request(
            "SCHEMA_EMPTY",
            "--schema 里一列都没读到（检查 tables/columns）—— 这份 schema 无法对账。",
            "换一份真实 schema：主键 / 唯一列 / 列存在性只能来自它。",
        ));
    }

    let opts = crate::domain::ttl::TtlOptions {
        project: p.project,
        version: p.version,
        base: p.base,
        action_type_count: snap.action_types.len(),
        tenancy: tenancy_decl.as_ref(),
        schema: Some(&schema_types),
    };
    let model = crate::domain::model_view::project_model_track(&snap);

    // ── A-2.4 · 审计输入（只影响文件头/文件尾，不改变映射）──────────
    //
    // `--exclude` 是**核对**，不是过滤：生产口径应当让快照本身不带这些表
    // （`ontok-propose --exclude`）。这里能做的只是「有人拿未排除的快照来生成」
    // 时把它**喊出来**（不阻断，但必须让人看见）。
    let excluded_names: Option<Vec<String>> = match p.exclude {
        Some(path) => {
            let ex = crate::extract::exclusions::Exclusions::load(&path.to_string_lossy())
                .map_err(|e| {
                    OpError::bad_request(
                        "EXCLUSIONS_UNREADABLE",
                        format!("读不出排除文件 {path:?}：{e:#}"),
                        "`exclude` 是**服务端路径**；给了它就会核对「已排除的表是否混进映射」。",
                    )
                })?;
            Some(ex.entries().iter().map(|e| e.table.clone()).collect())
        }
        None => None,
    };
    // `--fk-signed` 是**审计**：端点被排除的已签关系在快照里已经不存在
    // （`apply` 拒绝悬空关系），只有这份签字产物还留着它们。
    let signed_edges: Option<Vec<(String, String)>> = match p.fk_signed {
        Some(path) => {
            let raw = std::fs::read_to_string(path).map_err(|e| {
                OpError::bad_request(
                    "FK_SIGNED_UNREADABLE",
                    format!("读不出签字产物 {path:?}：{e}"),
                    "`fk_signed` 是**服务端路径**。",
                )
            })?;
            let value: serde_json::Value = serde_json::from_str(&raw).map_err(|e| {
                OpError::bad_request(
                    "FK_SIGNED_INVALID",
                    format!("签字产物 {path:?} 不是合法 JSON：{e}"),
                    "签字产物是 `fk proposals` 的 JSON。",
                )
            })?;
            let mut edges: Vec<(String, String)> = Vec::new();
            if let Some(arr) = value.get("proposals").and_then(|x| x.as_array()) {
                for prop in arr {
                    // 只认**签过字**的（`reviewed_by` 非空）—— 未签的本来就不进快照。
                    let signed = prop.get("reviewed_by").is_some_and(|x| !x.is_null());
                    if !signed {
                        continue;
                    }
                    let subject = prop.get("subject").and_then(|x| x.as_str()).unwrap_or("");
                    let to = prop
                        .pointer("/facts/to_table")
                        .and_then(|x| x.as_str())
                        .unwrap_or("");
                    if let Some((from, _)) = subject.split_once('.') {
                        if !from.is_empty() && !to.is_empty() {
                            edges.push((from.to_string(), to.to_string()));
                        }
                    }
                }
            }
            edges.sort();
            edges.dedup();
            Some(edges)
        }
        None => None,
    };

    // 对账失败 ⇒ Display 里逐条点名（表.列），**不写文件**。
    let doc = crate::domain::r2rml::generate_audited(
        &model,
        &opts,
        p.include_inherit,
        crate::domain::r2rml::R2rmlAudit {
            excluded_tables: excluded_names.as_deref(),
            signed_edges: signed_edges.as_deref(),
        },
    )
    .map_err(|e| {
        OpError::bad_request(
            "R2RML_RECONCILE_FAILED",
            format!("R2RML 生成失败（⛔ 一个字节都没产出）：{e}"),
            "映射与 schema 对不上：按错误里点名的 表.列 修 schema 或快照。\
             ⛔ 不会写出半份映射 —— 那会被当成完整映射用，查出来的结果是错的。",
        )
    })?;

    let st = doc.stats();
    // ⛔ `false` 与 `未标` 分开报：前者是**有据**的「仅名字推断」，后者是
    // 「没有验证信息」（含旧版布尔 false 无证据的遗留）。合并报会让数字员工
    // 把 52 条「没验」读成「52 条验过但失败」。
    // ⚠️ joinCondition 数在 `summary()` 里已经有了 —— 不再重复一遍。
    eprintln!(
        "· R2RML（{} v{}）：{} · \
         三态 true {} / inherit {}（生成 {}）/ false（仅名字推断，有据）{} · \
         未标（无验证信息）{} · 复合主键跳过 {} · 跳过已签关系 {} · schema 对账 0 失败",
        p.project,
        p.version,
        st.summary(),
        st.verified_edges,
        st.inherited_edges,
        st.inherited_emitted,
        st.naming_only_edges,
        st.unverified_edges,
        st.skipped_classes.len(),
        st.skipped_signed_edges.len()
    );
    // ── A-2.4 · fail-visible：映射里混进了已排除表 ──────────────
    if !st.excluded_tables_in_mapping.is_empty() {
        eprintln!(
            "⚠ 排除表：本映射包含 {} 张已排除的表（{}）—— 这份快照没带 --exclude；\
             生产口径应重建快照（本文件仅供对照，不阻断）",
            st.excluded_tables_in_mapping.len(),
            st.excluded_tables_in_mapping.join("、")
        );
    }
    if !crate::domain::ttl::tenancy_report(&model, tenancy_decl.as_ref()).annotated() {
        eprintln!(
            "⚠ 租户列未标注：TriplesMap 上一条 ontok:tenantColumn 都没有 —— \
             这份映射不能直接给数字员工用（--tenancy <file>）"
        );
    }

    Ok(R2rmlOut {
        project: proj.name,
        version: p.version,
        doc,
    })
}

/// ⭐ REQ-121 · `r2rml generate` 的**唯一入口**。
///
/// 生成逻辑全在 [`crate::domain::r2rml`]（纯函数、零 I/O）；这里只做三件事：
/// 读快照 / schema / 租户声明，把**对账失败**变成非零退出，写出产物。
///
/// ⛔ 对账失败时**一个字节都不写**（fail-closed）：半份映射比没有映射更危险 ——
/// 它会被 R2RML 引擎当成完整映射用，查出来的结果是错的，而且不报错。
///
/// ⛔ stdout 与 `--out` 写的是**同一份**字节（`r2rml_doc`）：出口只有一处序列化，
/// 与 C6/C7、D9/D10 同一条口径。
///
/// ⭐ REQ-124 · 生成整段在 [`r2rml_core`]：CLI 与 `POST /api/r2rml` 调**同一个**函数

async fn r2rml_cmd(pool: &PgPool, cmd: R2rmlCmd) -> anyhow::Result<()> {
    let R2rmlCmd::Generate {
        project,
        version,
        schema,
        out,
        include_inherit,
        base,
        tenancy,
        exclude,
        fk_signed,
    } = cmd;

    // ⭐ REQ-124 · 生成整段在 [`r2rml_core`]（`POST /api/r2rml` 调**同一个**函数）
    // ⇒ 两个出口的字节由同一份代码产生（AC-006：sha256 相等）。
    // ⛔ 本分支只负责「写到哪」（stdout / `--out`）。
    let generated = r2rml_core(
        pool,
        R2rmlParams {
            project: &project,
            version,
            schema: &schema,
            include_inherit,
            base: &base,
            tenancy: tenancy.as_deref(),
            exclude: exclude.as_deref(),
            fk_signed: fk_signed.as_deref(),
        },
    )
    .await?;
    let R2rmlOut { doc, .. } = generated;
    let st = doc.stats();
    let r2rml_doc = doc.doc();
    match &out {
        Some(path) => {
            std::fs::write(path, r2rml_doc.as_bytes())
                .with_context(|| format!("写不出 {path:?}"))?;
            // 回执走 stderr：`--out` 的 stdout 必须留空，脚本才好接。
            eprintln!(
                "· 已写出 R2RML 映射：{}（{} 字节 / {} 个 TriplesMap · 有损项见文件头）",
                path.display(),
                doc.len(),
                st.triples_maps
            );
        }
        None => print!("{}", r2rml_doc),
    }
    Ok(())
}

// ══════════════════════════════════════════════════════════════
// REQ-122 · SPARQL 查询
// ══════════════════════════════════════════════════════════════

/// `ontok sparql query` —— 规划 + 注入 + 只读执行。
///
/// # 顺序是刻意的（每一条失败都要**在干活之前**报出来）
///
/// | # | 步骤 | 为什么在这个位置 |
/// | --- | --- | --- |
/// | ① | 参数闸（`--max-rows` / `--timeout-ms` / `--tenant`） | 最便宜的失败先报 |
/// | ② | 读声明并解析 | 没有声明就**没有口径** —— 此时连库、算什么都在浪费，而且会给出「看起来成功」的空结果 |
/// | ③ | 项目 / 版本 / 快照 | 元数据库只读 |
/// | ④ | `--schema` 对账 | 列存在性 / 类型只能来自真实 schema |
/// | ⑤ | **映射**（与 `r2rml generate` 同一个 `build_mapping`） | ⛔ 不解析 TTL；同源才不会有第二份读法 |
/// | ⑥ | 连**源库**（只读，读回 `transaction_read_only` / `standard_conforming_strings`） | 客户资产：只读不成立就一条都不查；回执里那两个值就是**这里读回来的**，⛔ 不写死 |
/// | ⑦ | 规划（含注入）→ 执行 | 注入在 SQL 生成之后、执行之前 |
///
/// # 为什么先连元数据库再连源库
///
/// 元数据库（`ONTOK_DATABASE_URL`）只存本体快照，源库（`ONTOK_SOURCE_DATABASE_URL`）
/// 才是客户数据。前者连不上是「本体没准备好」，后者连不上是「数据源不可用」——
/// 报错必须指向真原因，不能一句「连不上库」把两者混在一起。
async fn sparql_cmd(pool: &PgPool, cmd: SparqlCmd) -> anyhow::Result<()> {
    use crate::sparql::{plan::PlanInput, policy::QueryPolicy};

    let SparqlCmd::Query {
        project,
        version,
        schema,
        policy,
        tenant,
        query,
        max_rows,
        timeout_ms,
        base,
        include_inherit,
    } = cmd;

    // ── ① 参数闸 ────────────────────────────────────────────
    if max_rows == 0 {
        anyhow::bail!(
            "--max-rows 是 0：0 行等于「什么都没查」，与「查了没有」混淆。要查就给 ≥1。"
        );
    }
    if timeout_ms == 0 {
        anyhow::bail!("--timeout-ms 是 0：等于「不许它跑」，那是禁用不是超时。要跑就给 ≥1。");
    }
    if tenant.trim().is_empty() {
        anyhow::bail!(
            "--tenant 是空的：⛔ 没有租户就没有隔离。本命令不会退化成「查全部」——\
             宁可拒绝，也不给出一个看起来正常的全量结果。"
        );
    }

    // ── ② 声明（缺失时给模板，而不是「猜一个列」）──────────────
    let policy = match &policy {
        Some(path) => {
            let raw = std::fs::read_to_string(path)
                .with_context(|| format!("读不出租户声明 {path:?}"))?;
            QueryPolicy::parse(&raw)
                .map_err(|e| anyhow::anyhow!("租户声明 {path:?} 不合法：{e}"))?
        }
        None => anyhow::bail!(
            "缺少租户声明（--policy）—— ⛔ 没有声明 ⇒ 所有表不可查（不会猜一个列名，也不会不过滤放行）。\n\
             {}\n\
             模板（{}）：\n{}",
            "声明决定「哪张表按哪一列过滤 / 哪张表是公共维度」。请把它另存为 query-policy.json 后改写：",
            crate::sparql::policy::TEMPLATE_FILE,
            crate::sparql::policy::TEMPLATE
        ),
    };

    // ── ③ 项目 / 版本 / 快照 ─────────────────────────────────
    let projects = PgProjectStore::new(pool.clone());
    let store = PgStore::new(pool.clone());
    let p = projects
        .get_by_name(&project)
        .await?
        .ok_or_else(|| anyhow::anyhow!("项目 {project:?} 不存在"))?;
    let v = projects.version_by_seq(p.id, &p.name, version).await?;
    let snap = store
        .get_snapshot(v.id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("{project} v{version} 没有快照"))?;

    // ── ④ schema ────────────────────────────────────────────
    let raw = std::fs::read_to_string(&schema)
        .with_context(|| format!("读不出 schema 文件 {schema:?}"))?;
    let schema_types = crate::domain::ttl::SchemaTypes::parse(&raw)
        .map_err(|e| anyhow::anyhow!("schema 文件 {schema:?} 不合法：{e}"))?;
    if schema_types.is_empty() {
        anyhow::bail!("--schema 里一列都没读到 —— 这份 schema 无法对账（⛔ 不猜列类型）");
    }

    // ── ⑤ 映射（**与 `r2rml generate` 同一个函数**）────────────
    let opts = crate::domain::ttl::TtlOptions {
        project: project.as_str(),
        version,
        base: base.as_str(),
        action_type_count: snap.action_types.len(),
        tenancy: None,
        schema: Some(&schema_types),
    };
    let model = crate::domain::model_view::project_model_track(&snap);
    let mapping = crate::domain::r2rml::build_mapping(
        &model,
        &opts,
        include_inherit,
        crate::domain::r2rml::R2rmlAudit::default(),
    )
    .map_err(|e| {
        anyhow::anyhow!(
            "按 R2RML 生成 SQL 的前置对账失败（映射与 schema 不一致，⛔ 一个字节都不产出）：\n{e}"
        )
    })?;

    // ── ⑥ 源库（只读；读回校验）───────────────────────────────
    let url = crate::config::require_env(crate::extract::introspect::SOURCE_DATABASE_URL_ENV)
        .map_err(|e| anyhow::anyhow!("{e}（SPARQL 查的是源库，不是元数据库）"))?;
    let src = crate::sparql::exec::connect_read_only(&url)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    // ── ⑦ 规划 + 执行 ───────────────────────────────────────
    let ontology_iri = opts.ontology_iri();
    let input = PlanInput {
        mapping: &mapping,
        schema: &schema_types,
        ontology_iri: &ontology_iri,
        policy: &policy,
        tenant: tenant.as_str(),
        max_rows,
    };
    let result = match crate::sparql::run_query(&src.pool, &input, &query, timeout_ms).await {
        Ok(r) => r,
        Err(e) => {
            src.pool.close().await;
            return Err(anyhow::anyhow!("{e}"));
        }
    };
    src.pool.close().await;

    // ── ⑧ 回执（stderr 给人，stdout 给脚本）──────────────────
    let st = mapping.stats();
    eprintln!(
        "· SPARQL（{project} v{version}）：映射 {} —— 与 r2rml generate 同源（build_mapping）",
        st.summary()
    );
    eprintln!(
        "· 声明：v{} · 租户主体 {}.{}（{}）· 逐表登记 {} 张 · 公共维度 {} 张 · reviewed_by {}",
        policy.version,
        policy.tenant_subject,
        policy.tenant_id_column,
        tenant,
        policy.per_table.len(),
        policy.public_dimensions.len(),
        policy.reviewed_by.as_deref().unwrap_or("（未签）")
    );
    if result.tenant_applied.is_empty() {
        eprintln!(
            "· 注入：0 张表（这条查询只碰公共维度 {}）",
            result
                .public_dimensions
                .iter()
                .map(|p| p.table.as_str())
                .collect::<Vec<_>>()
                .join("、")
        );
    } else {
        eprintln!(
            "· 注入 {} 张表：{}",
            result.tenant_applied.len(),
            result
                .tenant_applied
                .iter()
                .map(|t| format!("{}.{} = {:?}", t.table, t.column, t.value))
                .collect::<Vec<_>>()
                .join("、")
        );
    }
    // ⛔ 这一行**不许**写死 `on`（REQ-122 · N3）：两个会话值都来自 ⑥ 里
    // `SHOW` 在**这条**连接上读回的结果（`ReadOnlyConnection::facts`）。
    // 曾经的「只读会话：transaction_read_only = on」是硬编码字符串 —— 那是假回执。
    eprintln!("{}", src.facts.receipt(timeout_ms, max_rows));
    eprintln!(
        "· 返回 {} 行（truncated: {}）· 涉及 {} 张表",
        result.row_count,
        result.truncated,
        result.tables.len()
    );
    for w in &result.warnings {
        eprintln!("⚠ {w}");
    }

    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

/// ⭐ REQ-123 · `ontok query serve` —— 参数只在这里翻译，逻辑全在 `query::serve`。
///
/// ⚠️ 这里**不做**任何鉴权 / 限额 / 绑定判断：那些在 `query::serve::serve` 与
/// `query::http` 里（一处实现）。本函数只做「clap 参数 → ServeArgs」的搬运。
async fn query_cmd(pool: &PgPool, cmd: QueryCmd) -> anyhow::Result<()> {
    let QueryCmd::Serve {
        bind,
        tokens,
        project,
        version,
        schema,
        policy,
        tenancy,
        allow_public,
        timeout_ms,
        sse_idle_timeout_ms,
        sse_max_connections,
        max_concurrent_queries,
        audit_out,
        base,
        include_inherit,
    } = cmd;

    let args = crate::query::serve::ServeArgs {
        bind,
        tokens,
        project,
        version,
        schema,
        policy,
        tenancy,
        allow_public,
        timeout_ms,
        sse_idle_timeout_ms,
        sse_max_connections,
        max_concurrent_queries,
        audit_out,
        base,
        include_inherit,
    };
    crate::query::serve::serve(pool, args).await
}

/// ⭐ REQ-124 · `ontok console serve` —— 参数只在这里翻译，逻辑全在 `console::serve`。
///
/// ⚠️ 这里**不做**任何鉴权 / 绑定判断：那些在 `console::http` 与 `console::serve`
/// 里（一处实现）。本函数只做「clap 参数 → ServeArgs」的搬运 —— 与 `query_cmd` 同一条分工。
async fn console_cmd(pool: &PgPool, cmd: ConsoleCmd) -> anyhow::Result<()> {
    let ConsoleCmd::Serve {
        bind,
        tokens,
        project,
        schema,
        exclusions,
        policy,
        tenancy,
        allow_public,
        audit_out,
        base,
    } = cmd;
    let args = crate::console::serve::ServeArgs {
        bind,
        tokens,
        project,
        schema,
        exclusions,
        policy,
        tenancy,
        allow_public,
        audit_out,
        base,
    };
    crate::console::serve::serve(pool, args).await
}

/// ⭐ REQ-126 · `ontok portal serve` —— 参数只在这里翻译，逻辑全在 `portal::serve`。
///
/// ⚠️ 这里**不做**任何鉴权 / 绑定 / 问题校验：那些在 `portal::http` 与
/// `portal::serve`（一处实现）。本函数只做「clap 参数 → ServeArgs」的搬运 ——
/// 与 `query_cmd` / `console_cmd` 同一条分工。
async fn portal_cmd(pool: &PgPool, cmd: PortalCmd) -> anyhow::Result<()> {
    let PortalCmd::Serve {
        bind,
        tokens,
        project,
        version,
        schema,
        policy,
        questions,
        allow_public,
        audit_out,
        timeout_ms,
        base,
        include_inherit,
    } = cmd;
    let args = crate::portal::serve::ServeArgs {
        bind,
        tokens,
        project,
        version,
        schema,
        policy,
        questions,
        allow_public,
        audit_out,
        timeout_ms,
        base,
        include_inherit,
    };
    crate::portal::serve::serve(pool, args).await
}

// ── 提案指法 ────────────────────────────────────────────/// `p1` / `p2` … → **该项目内按创建时间倒序**的第 n 个提案（`p1` = 该项目里最新的那个）；/// 也可直接给 UUID（UUID 全局唯一，不需要作用域）。
///
/// # 别名是「项目内序号」—— 必须带 `WHERE project_id = $1`
///
/// 早期版本这里是**全库**排序。铁证是 `proposal_alias` 曾有个被标成未使用的
/// `_project_id` 参数 —— 项目 id 早就传进来了，却没写进 SQL。
/// 后果不是「解析到老数据」而是更坏的：
/// 共享测试库里别的项目一有新提案，`p1` 就指向**别的项目**的提案 ——
/// 审核动作会写进别人的提案，而且照样报「成功」，
/// 只有 `review show` 里那行的条目对不上时才露出不对。
///
/// # 为什么是「倒序」而不是正序
///
/// 正序（`p1` = 最早）在本项目会踩坑：同一项目里躺着几百条历史提案，
/// `p1` 永远指向第一条老数据，AC-012 的演示命令就会作用在**老数据**上
/// —— 而且照样「成功」，只有 `review show` 里那行 `[1] a` 才露出不对。
/// 倒序让 `p1` 恒等于「刚导入的那个」，与历史无关。
///
/// # 两处 SQL 必须同作用域同排序
///
/// 与 `proposal_alias` 完全一致 —— 否则打印的 `p1`
/// 和下次 `--proposal p1` 解析到的不是同一个提案。
///
/// ⚠️ REQ-111 起是 `pub`：审核界面（`src/review_ui`）启动时用**同一个**
/// 别名解析器，不抄第二份「p1 是哪个」的 SQL。抄一份的后果是页面显示 `p1`、
/// CLI 的 `p1` 却指另一个提案 —— 那种不一致只有人肉对账才发现。
pub async fn resolve_proposal(pool: &PgPool, project_id: Uuid, spec: &str) -> anyhow::Result<Uuid> {
    if let Ok(u) = Uuid::parse_str(spec) {
        return Ok(u);
    }
    if let Some(n) = spec.strip_prefix('p').and_then(|x| x.parse::<i64>().ok()) {
        let rows: Vec<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM proposals WHERE project_id = $1 ORDER BY created_at DESC, id DESC",
        )
        .bind(project_id)
        .fetch_all(pool)
        .await?;
        return rows
            .get((n - 1) as usize)
            .map(|r| r.0)
            .ok_or_else(|| anyhow::anyhow!("别名 {spec} 越界：该项目内只有 {} 个提案", rows.len()));
    }
    anyhow::bail!("--proposal 认不出：{spec}（用 UUID 或 p1/p2…）")
}

/// 库里最近导入的提案所属项目 —— `--project` 省略时的作用域。
///
/// AC-012 的演示流程是 `propose import` 紧接着 `review decide --proposal p1`，
/// 省略 `--project` 时别名必须仍能解析到刚导入的那个，所以回落到这里。
/// **要审核历史提案请显式给 `--project`**：省略时作用域由「最新的提案」决定，
/// 它属于哪个项目不由你指定。
async fn newest_proposal_project(pool: &PgPool) -> anyhow::Result<Option<Uuid>> {
    let row: Option<(Uuid,)> =
        sqlx::query_as("SELECT project_id FROM proposals ORDER BY created_at DESC, id DESC LIMIT 1")
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|r| r.0))
}

/// `--proposal` 的值 → 提案 id（三种指法的统一入口）。
///
/// 分两步：先判指法（UUID 直接放行，**不需要作用域**），
/// 别名才去定作用域项目，然后交给 `resolve_proposal`。
async fn resolve_proposal_arg(
    pool: &PgPool,
    spec: &str,
    project: Option<&str>,
) -> anyhow::Result<Uuid> {
    if let Ok(u) = Uuid::parse_str(spec) {
        return Ok(u);
    }
    let pid = match project {
        Some(name) => PgProjectStore::new(pool.clone())
            .get_by_name(name)
            .await?
            .map(|p| p.id)
            .ok_or_else(|| anyhow::anyhow!("项目不存在：{name}"))?,
        None => newest_proposal_project(pool).await?.ok_or_else(|| {
            anyhow::anyhow!(
                "别名 {spec} 需要项目作用域，但库里还没有任何提案 —— \
                 请改用 --proposal <uuid>，或加 --project <项目名>"
            )
        })?,
    };
    resolve_proposal(pool, pid, spec).await
}

/// 给刚导入的提案算别名，用于打印。
///
/// **必须与 `resolve_proposal` 用同一套作用域与排序** —— 否则打印的 `p1`
/// 和下次 `--proposal p1` 解析到的不是同一个提案。
///
/// ⚠️ REQ-111 起是 `pub`：审核界面用它把「绑的是哪个 pN」打在启动横幅上，
/// 与 CLI 的 `review show` 抬头同源。
pub async fn proposal_alias(pool: &PgPool, project_id: Uuid, id: Uuid) -> String {
    let rows: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM proposals WHERE project_id = $1 ORDER BY created_at DESC, id DESC",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    match rows.iter().position(|r| r.0 == id) {
        Some(i) => format!("p{}", i + 1),
        None => id.to_string(),
    }
}

/// 内部快照 → 投影输入。
///
/// `ontology show` / `ontology overview` / **REQ-027 的 `mcp-server`** 共用
/// —— 抄一份就会出现「页面上的对象」和「总览里的对象」对不上，
/// 而那正是判读人会当场发现、并且当场不信我们的问题。
/// MCP 侧更不能抄：抄一份就意味着 MCP 看到的对象可能与客户看到的页面不是同一批。
///
/// ⚠️ REQ-078 起是 `pub`（原 `pub(crate)`）：端到端断言要**在测试进程里复现**
/// 这条投影，才能把真实 stdout 与「前缀 ++ domain 渲染」逐字节比。
/// 它是**纯函数**（只读 `snap`，不碰 I/O），公开它不扩大任何写路径。
pub fn snapshot_to_view(
    snap: &crate::domain::snapshot::Snapshot,
) -> SnapshotView {
    SnapshotView {
        entity_types: snap
            .entity_types
            .iter()
            .map(|e| crate::domain::view::EntityTypeView {
                key: e.key.clone(),
                display_name: e.display_name.clone(),
                description: e.description.clone(),
                aliases: e.aliases.clone(),
                parent: e.parent.clone(),
                disjoint_with: e.disjoint_with.clone(),
                properties: e
                    .properties
                    .iter()
                    .map(|pr| crate::domain::view::PropertyTypeView {
                        name: pr.name.clone(),
                        column: pr.column.clone(),
                        nullable: pr.nullable,
                        note: pr.note.clone(),
                        sample: pr.sample.clone(),
                    })
                    .collect(),
                evidence: e.evidence.iter().map(|ev| serde_json::to_value(ev).unwrap()).collect(),
            })
            .collect(),
        link_types: snap
            .link_types
            .iter()
            .map(|l| crate::domain::view::LinkTypeView {
                key: l.key.clone(),
                from_type: l.from_type.clone(),
                to_type: l.to_type.clone(),
                cardinality: l.cardinality.clone(),
                semantic: l.semantic.clone(),
            })
            .collect(),
    }
}



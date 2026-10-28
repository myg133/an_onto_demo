//! `ontok` —— **服务面** CLI（REQ-152）。
//!
//! 三个入口：`query serve` / `mcp-server` / `serve`。构建面（project / version /
//! import / review / freeze / export / access / …）在 `ontok-builder`，见 `src/cli.rs`。
//!
//! # 为什么单独一个模块（而不是留在 `cli.rs` 里）
//!
//! `ontok` 二进制**编译期不链**执行器（`access::net` / `access::tc3` / `ureq`）。
//! 链接器按**可达符号**收对象：只要本模块不引用 `crate::cli`，`cli` 及其下游的
//! 执行器就不会进 `ontok`。这条不是约定而是结构 —— 一旦本文件出现 `crate::cli`
//! 或 `access::net`，`strings target/release/ontok | grep X-TC-Action` 立刻非 0。
//! 守卫见 `tests/test_req152_bins.rs`。

use anyhow::Context;
use clap::{Parser, Subcommand};
use sqlx::postgres::PgPool;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "ontok", about = "FDE 本体论平台 · 本体内核（切片 1，只读闭环）")]
pub struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
pub enum Cmd {
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
pub enum QueryCmd {
    /// 起查询服务：REST（/api/sparql /catalog /describe /health）+ MCP over HTTP（/mcp）
    ///
    /// 端点：
    ///
    /// POST /api/sparql         body {query, max_rows?, format?}（tenant 只能来自 token；
    ///                          ⭐ REQ-142：**默认回执是 JSON-LD 上下文子图**，要行回执传 `?format=rows`
    ///                          或 `Accept: application/json`，MCP 传 `format:"rows"`）
    /// GET  /api/catalog?page=1&page_size=50
    /// GET  /api/describe?name=:customers
    /// GET  /api/ontology.ttl   ⭐整份 OWL（Turtle）：默认**客户视图**；`?view=model` 建模视图
    /// GET  /api/health         免 token：产品版本 + 项目名 + **本体**版本 + 语法版本（两种模式都有）
    /// POST /mcp                JSON-RPC 2.0（initialize / tools/list / tools/call / resources/list / resources/read）
    /// GET  /mcp                SSE 流（服务端消息；空闲超时会主动断开）
    ///
    /// ⭐ REQ-139 · `/api/health` 里的 `version` 与 `ontology_version` 是**两件事**，⛔ 别混读：
    ///
    /// | 字段 | 含义 | 例 |
    /// | --- | --- | --- |
    /// | `version` | **产品**版本 = 这个 `ontok` 二进制的 `CARGO_PKG_VERSION`（升级软件才变） | `"0.1.0"` |
    /// | `ontology_version` | **本体**版本 = `--version` 指向的、已冻结的快照版本号（整数，`ontok version freeze` 才变） | `2` |
    /// | `project` | 项目名（`--project`）；它本来就出现在 TTL 与 `/api/catalog` 里 | `"ops360"` |
    ///
    /// `project` / `ontology_version` 与 `GET /api/catalog` 的 `project` / `version` 是**同一对
    /// 字段**（同一来源，⛔ 不在两处各算一次）；普通模式与 `--ontology-only` **都有**这两个字段。
    /// ⛔ health 仍然**不给**租户 id / 连接串 / 表名 / 口令（能判断「是哪套部署」就够了）。
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
    /// ⭐ **本体服务模式（`--ontology-only`，REQ-136）** —— 没有业务库也能起服务：
    ///
    /// ```text
    /// ontok query serve --bind 0.0.0.0:8800 --allow-public --tokens tokens.json \
    ///   --project ops360 --version 2 --ontology-only
    /// ```
    ///
    /// 该模式**不需要** `ONTOK_SOURCE_DATABASE_URL`、**不需要** `--schema`、**不需要** `--policy`
    /// （给了 `--schema` 反而拒绝启动：R1 互斥）；仍然需要 `--project` / `--version`（版本必须
    /// 已冻结）与 `--tokens`。各端点在两种模式下的差别：
    ///
    /// | 端点 | 普通模式 | `--ontology-only` |
    /// | --- | --- | --- |
    /// | `GET /api/health` | 200 + `project` / `ontology_version` | 200 + `project` / `ontology_version` + `"mode":"ontology-only"` |
    /// | `GET /api/catalog` | 只列**可查**的类/关系（schema + R2RML 对账） | **直接读快照**：类/属性/关系**全列**；关系带 `evidence_level`（= `ontok:joinVerified`：`true`/`inherit`/`false`/**`unverified`**），类带 `display_name`；`counts` 里**没有** `excluded_relations` |
    /// | `GET /api/describe` | 类/属性详情（含租户口径） | 同上，另给 `display_name` / `key`；`tenant.mode = not_applicable`（没有数据源） |
    /// | `GET /api/ontology.ttl` | 同 CLI 导出 | **与 `ontok ontology export --project <p> --version <v>`（不带 `--schema`）逐字节相同**；`?view=model` 是同一份无 schema/tenancy 的建模视图 |
    /// | `POST /api/sparql` | 真查 | **需要库的查询 ⇒ 503 `NO_DATA_SOURCE`**（⛔ 不是 500、⛔ 不是 200 空结果；hint 说明「重试无效」）；**纯 api 类**（`--access-mapping` 里声明的类）⇒ 与普通模式**同一份** `@pending` 取数指引（`format=jsonld` 200 / `format=rows` 400 `NOT_DB_SOURCE`） |
    /// | MCP `sparql_query` / `sample` | 真查 | 同一个 `code` / `hint`（JSON-RPC 信封里 `isError:true`；HTTP 仍是 200 —— 传输没坏，是工具执行失败）；`sample` 在本模式**恒 503**（取样必须真连库） |
    /// | MCP `catalog` / `describe` / `resources/read` | 正常 | 正常（与 REST 同一段代码） |
    ///
    /// ⛔ 该模式**一处都不连业务库**（启动时不连，任何请求也不会连 —— `QueryService::source`
    /// 是 `None`，唯一取池入口 `QueryService::source_pool` 在本模式恒 `None`）；
    /// `--include-inherit` 不生效（不做对账）。
    ///
    /// ⭐ REQ-151 · SPARQL 不再「恒 503」：**需要库**的查询（库源类 / 混合 / 没有
    /// `--access-mapping`）仍然 503 `NO_DATA_SOURCE`；**纯 api 类**（映射里声明的类）
    /// 返回取数指引 —— `format=jsonld` ⇒ 200 + `@graph:[]` + `@pending`（与普通模式同一形状）；
    /// `format=rows` ⇒ 400 `NOT_DB_SOURCE`（表挂不住「少了一块」）。`sample` 仍 503。
    /// catalog 的类/关系由**同一个**函数从快照取（`query::catalog::build_items`），
    /// 两个模式只在**过滤**处分叉（纪律 H）。
    ///
    /// ⚠️ `max_rows=0` 是**参数错误（400 `BAD_REQUEST`）**，与部署模式无关。注意判定顺序：
    /// ⓪ 模式闸（503）先于 ② 限额校验（400）—— **需要库**的查询在本体模式仍先得到
    /// 503 `NO_DATA_SOURCE`；**纯 api 类**会通过闸门，然后才因 `max_rows=0` 得到 400。
    /// 旧版本本体模式**一切查询**都先 503，看不到这个参数错误；这是 REQ-151 把闸门挪到
    /// 「源判定之后」的有意结果（⛔ 不是 500，也不是 200 空结果）。
    ///
    /// ⚠️ 本体模式的 `@pending[].get.filter` 可能是 `{}`：那表示**平台无法替你确定取数标识**
    /// （本体模式没有 R2RML / 库侧主键，认不出 `filter.ids`）。⛔ **不要直接全量拉取**
    /// （账单、告警一类接口会烧钱并触发限流）：先用该类的实例标识属性（`GET /api/describe`
    /// 看属性，如 `:cvm_instance__instance_id`）在**你自己的取数侧**拿到标识，再带
    /// `get.filter.ids` 重新组织取数；暂时拿不到标识时，至少用 `get.pushdown` 里的字段
    /// 收窄范围，⛔ 不要无过滤拉全量。
    ///
    /// ⚠️ 鉴权只认 `Authorization: Bearer <token>`。⛔ 放 query string / cookie **不生效**。
    /// ⚠️ 限额：`max_rows`（token 级上限，调用方只能往下压）、超时、每 token 并发、
    /// SSE 连接数 + 空闲超时；超限 **429**。
    /// ⚠️ 审计：每次调用一行（时间 / token 摘要 / tenant / 查询哈希 / 行数 / 耗时 / code），
    /// ⛔ 不记 token 明文、不记查询原文。
    ///
    /// ⭐ **两种部署模式（REQ-159 P1b）** —— 判据只有一个：给没给 `--project`。
    ///
    /// ```text
    /// 单项目（--project <名> --version <第几版>）    启动时加载一份快照；与 REQ-123 逐字节一致
    /// 多项目（两个都不给）                          按 key 反查项目 → active_version_id → 按 version_id 缓存
    /// ```
    ///
    /// 多项目模式是「一套部署 · 多套本体」：数字员工**不需要知道项目名和版本号**，换项目 =
    /// 换 key。`activate` 之后**下一次请求**就生效（不需要重启、不需要跨进程通知）——
    /// 因为缓存键是 `version_id`，而 `active_version_id` 每次请求都重读。
    ///
    /// 多项目模式的 token 表（`version: 2`）里**每一条**都要写
    /// `"project": "<项目名>"`（缺一条 ⇒ **拒绝启动**并点名第几条）；`version: 1` 的老表
    /// 在单项目模式下仍然照常可用。
    ///
    /// ⚠️ 多项目模式下 token 绑的项目**不在元数据库里**（改名 / 删除 / 抄错）⇒
    /// **404 `project.not_found`**（配置错，⛔ 不是 500：服务没有坏，要改的是 token 表）。
    /// 「项目没有生效版本」仍是 **503 `project.no_active_version`**。
    ///
    /// ⭐ **建项目发的 key 也能鉴权（REQ-175）** —— 静态 token 表没命中时，身份门按
    /// `sha256(token)` 查 `project_keys`（60s 缓存，键 = 摘要）：
    ///
    /// - 命中 ⇒ 视作 `kind = "query"`、项目 = 该 key 绑的项目（多项目模式照常解析
    ///   `active_version_id`）；⛔ 这把 key 不能调那 8 条写端点（403 `key.not_admin`）；
    /// - 未命中 / 已吊销 ⇒ **401**（不区分「没有这把 key」与「已吊销」—— 403 会泄露）；
    /// - **单项目模式**下它绑的项目 ≠ `--project` ⇒ **403 `key.project_mismatch`**；
    /// - **静态 token 表优先**：表里有的 token，行为与本需求之前逐字节相同。
    ///
    /// ⭐ **写面（REQ-159 P1c-1 / ADR-013）** —— 这个进程现在也能**写元数据库**了：
    ///
    /// ```text
    /// POST /api/projects                          建项目 + 发一把 query key（201）
    /// POST /api/projects/:p/bundles               上传构建包（application/octet-stream，body 就是包字节）
    /// GET  /api/projects/:p/bundles/:id           包状态与组件清单
    /// POST /api/projects/:p/bundles/:id/import    解析入库（单事务，不冻结）
    /// POST /api/projects/:p/bundles/:id/activate  冻结 + 写生效指针
    /// PUT  /api/projects/:p/database              配 DSN（加密列；响应里没有明文）
    /// POST /api/projects/:p/database/test         测连通（失败也 200，由 ok 区分）
    /// GET  /api/projects/:p/key                   再次查看 query key（每次写审计）
    /// ```
    ///
    /// - 这几条**只认 `kind = "admin"` 的 key**；query key 调它们 ⇒ **403 `key.not_admin`**；
    /// - 它们原来在 `ontok console serve`（FDE 本地）—— 那里**仍然保留**，与这里共用
    ///   同一个适配层（状态码 / 审计 action / 错误码只有一份）；
    /// - ⛔ 只写**元数据库**（projects / versions / snapshots / keys / DSN 密文），
    ///   ⛔ 不碰客户源库（`database/test` 只连一次、只 `SELECT version()`）；
    /// - ⚠️ 需要环境变量 **`ONTOK_SECRET_KEY`**（64 位十六进制 = 32 字节；DSN 与 query key
    ///   的加密列）。**缺了就拒绝启动**（在绑 socket 之前），没有「先跳过后补」的开关。
    ///
    /// token 表里 `kind` 的规矩（ADR-013 §三）：
    ///
    /// | 写法 | 结果 |
    /// | --- | --- |
    /// | 不写 `kind` | 视为 `"query"`（**向后兼容**：既有 token 表一个字节不用改） |
    /// | `"kind": "admin"` | 能调上面 8 条；**必须不绑项目**（写了 `project` ⇒ 整份拒绝启动） |
    /// | `"kind": "query"` | 只能读；多项目模式下必须有 `project` |
    ///
    /// ⭐ **`?project=<名>`（REQ-159 P1c-1 追加）** —— Studio 顶栏切项目：
    /// `GET /api/catalog` / `GET /api/describe` / `GET /api/ontology.ttl` /
    /// `POST /api/sparql` 接受这个**可选**参数，**只有 admin** 能用（query key 带它 ⇒
    /// **403 `key.project_pinned`**：数字员工的项目只能来自 key，跳项目就是串包）。
    /// **不给这个参数时行为完全不变**（单项目模式按 `--project`、多项目模式按 key）。
    #[command(verbatim_doc_comment)]
    Serve {
        /// 监听地址。默认只允许内网/回环；绑 0.0.0.0 或公网地址要显式 `--allow-public`
        #[arg(long)]
        bind: String,
        /// 静态 token 表（JSON，见 `tokens.example.json`；示例里的 token 是假值）
        #[arg(long)]
        tokens: PathBuf,
        /// 项目名。⭐ **省略 = 多项目模式**（每次请求按 key 绑定项目 → 该项目的
        /// `active_version_id`）；给了就必须同时给 `--version`。
        ///
        /// 单项目模式（`--project` + `--version`）与 REQ-123 起的行为**逐字节一致**：
        /// 启动时加载一份快照，token 只做鉴权、⛔ 不按 token 切项目。
        ///
        /// 多项目模式（两个都不给）：一个进程服务多套本体 —— 启动时**一份快照都不加载**，
        /// 每次请求按 `Authorization: Bearer <key>` 反查项目，再取
        /// `projects.active_version_id` 指向的那一版。换项目 = 换 key；activate 之后
        /// **下一次请求**即生效（不需要重启、不需要任何跨进程通知）。
        ///
        /// ⚠️ 多项目模式下 token 表里**每一条**都必须写 `"project": "<项目名>"`
        /// （否则**拒绝启动**并点名第几条）；`version: 2` 起支持这个字段，`version: 1`
        /// 的老表在单项目模式下仍然照常可用。
        #[arg(long)]
        project: Option<String>,
        /// 第几版（该版必须已冻结）。**与 `--project` 成对**；多项目模式下省略。
        #[arg(long)]
        version: Option<i32>,
        /// 真实 schema（`ontok-introspect` 产物）—— 列存在性 / 列类型只能来自它。
        ///
        /// **普通模式必填**；`--ontology-only` 下**互斥**（同时给 ⇒ 拒绝启动，R1）。
        #[arg(long)]
        schema: Option<PathBuf>,
        /// 租户声明（`query-policy.json`，ADR-006）—— **普通模式必填**。
        ///
        /// `--ontology-only` 下可省（没有数据源 ⇒ 没有租户口径）。
        #[arg(long)]
        policy: Option<PathBuf>,
        /// ⭐ **本体服务模式**：不连业务库、不做 schema / R2RML 对账。
        ///
        /// 只要 `--project` / `--version`（已冻结）+ `--tokens`；catalog/describe 直接读快照、
        /// `/api/ontology.ttl` 与 CLI 导出逐字节相同、
        /// **需要库的** SPARQL/取样恒 503 `NO_DATA_SOURCE`。
        /// ⭐ REQ-151 · 若同时给了 `--access-mapping`，**纯 api 类**的查询返回取数指引
        /// （jsonld ⇒ 200 + `@pending`；rows ⇒ 400 `NOT_DB_SOURCE`），混合查询仍 503。
        /// ⛔ 与 `--schema` 互斥（同时给 ⇒ 拒绝启动并说明原因）。
        ///
        /// ⭐ REQ-137 · 本模式的 token 表里**不许写 `tenant`**（必须缺省）：这里没有业务库、
        /// 没有租户概念，填了 ⇒ 拒绝启动。⛔ 不要用全 0 UUID 之类的假租户让它起来 ——
        /// 那会让审计日志里出现一个「看起来真实」的租户。
        #[arg(long)]
        ontology_only: bool,
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
        /// 全局并发查询上限（按源库只读池的连接数取，默认 2；本体模式无意义）
        #[arg(long, default_value_t = 2)]
        max_concurrent_queries: usize,
        /// 审计落点（**追加**写）。⚠️ 它不是审计的开关：省略时审计仍写 **stdout**（容器日志 → 集群采集）；给了它 = stdout **+** 这个文件（同时写）
        #[arg(long)]
        audit_out: Option<PathBuf>,
        /// IRI 基（必须与 `ontok r2rml generate` 用的是同一个）
        #[arg(long, default_value = "https://ontok.local/")]
        base: String,
        /// 把 `joinVerified "inherit"` 的边也生成 join（默认不生成，与 `r2rml generate` 一致）
        ///
        /// 只在普通模式生效；`--ontology-only` 下不做对账，本开关被忽略（启动会提示）。
        #[arg(long)]
        include_inherit: bool,
        /// ⭐ REQ-145 · **access 映射**（API 源声明）：哪些类来自云 API、按什么取。
        ///
        /// ⛔ ⭐ REQ-177 · **只在单项目模式可用**。多项目模式（`--project` / `--version`
        /// 都不给）下给这个参数 ⇒ **连库之前就拒绝启动**（退出码 2）：每套本体的 AM
        /// 随构建包入库（REQ-174），按 `active_version_id` 各自加载；进程级一份会让
        /// 「没有 AM 的项目」回落到别人的映射并 bind 失败。
        ///
        /// 省略 ⇒ 与不带本参数时**逐字节相同**（零回归）：TTL 无 `ontok:sourceKind`、
        /// `catalog`/`describe` 无 `access`、SPARQL 无 `@pending` / `complete` / `mapping_version`。
        ///
        /// 给了 `--access-mapping <file>` 之后（四个产物各自的变化）：
        ///
        /// - `GET /api/ontology.ttl`（**两视图**）：api 类多一行 `ontok:sourceKind "api"`；
        ///   库源类**不写**（默认 db）；⛔ 不写端点 / 参数 / 凭据
        /// - `GET /api/catalog` / `GET /api/describe`：每个类多一个 `access`
        ///   （api ⇒ `{kind:"api", queryable_by:["fetch"], pushdown_fields:[…], freshness:…}`；
        ///   库源 ⇒ `{kind:"db", queryable_by:["sparql"]}`）
        /// - `POST /api/sparql`（默认 JSON-LD）：只碰 api 类 ⇒ `@graph: []` +
        ///   `@pending[]`（带 `get`）+ `meta.complete:false` + `meta.unresolved_classes`
        /// - `POST /api/sparql?format=rows`：只碰 api 类 ⇒ **400 `NOT_DB_SOURCE`**
        ///   （⛔ 不返回空表 —— 空表会被读成「查了没有」）
        /// - 跨源边（一端库、一端 api）：**不下发** `get`；`@pending` 那条改给 `ref` +
        ///   `local_hint`（库侧哪一列有 FK 值 + 可直接照抄的取数语句）+ 三步 `hint`；
        ///   要**遍历**它 ⇒ **400 `CROSS_SOURCE_UNSUPPORTED`**（拆成两段查，别重试）
        ///
        /// ⚠️ **已知限制（R-3）**：一条查询**同时**涉及库源实体与 API 类时，整个查询
        /// 返回 pending（`meta.complete:false`）、**一行 SQL 都不执行**、`@graph` 恒空 ——
        /// ⛔ 不会塞半份库源结果伪装成「算完了」。部分求值（库源那半照算）要拆查询计划，
        /// **是阶段 2 的事**；本期请拆成两条查询（先库源，再按 `get` / `ref` 取 API 那半）。
        ///
        /// 文件形状（**只认这几个字段**；契约里其余字段读进来就丢，⛔ 不转发、不执行）：
        ///
        /// ```json
        /// { "contract_version": 1, "mapping_version": 2,
        ///   "entities": [
        ///     { "id": "Customer", "class": "customers",
        ///       "capability": { "pushdown": ["ids"] }, "freshness": "realtime" } ] }
        /// ```
        ///
        /// - `mapping_version` **必填** ⇒ `meta.mapping_version` 与 `@pending[].get.ref`
        ///   的 `@v<n>`（契约规则 3：映射是人签的，要能追到版本）；
        /// - `entities[].class` 是**本体里的类 key**（`catalog` 里 class 项的 `table`）；
        ///   必须在这一版里真实存在，否则**拒绝启动**（点名 + 给相近项）；
        /// - `entities[].id` 是 access 契约里的条目名（`get.ref` = `access#<id>@v<n>`）；
        /// - `capability.pushdown` ⇒ `access.pushdown_fields` / `get.pushdown`（省略 = `[]`，
        ///   ⛔ 不替你猜一个能力）；`freshness` 可选（省略 ⇒ `access` 里不出现）。
        ///
        /// ⭐ REQ-152 · `freshness` **两种写法都收**（一份 schema，部署侧不再需要投影脚本）：
        /// 字符串 `"freshness": "realtime"`，或 access 契约 / 全量 AM 的对象
        /// `"freshness": { "mode": "realtime", "note": "…" }` —— 统一取 `mode`。
        /// 对象里没有 `mode` ⇒ 与省略同义（⛔ 不编一个「实时」）。
        ///
        /// ⛔ 平台**不调用**任何 API（凭据不出客户边界）、**不跨源 join**、**不回填**；
        /// `@pending[].get` 只是**取数指引**。⛔ 客户侧部署不要加载映射：不加载就天然没有 `get`。
        ///
        /// ⚠️ 跨源边**不在**映射文件里声明：它由「本体里的边 + 映射里的源」交叉推出
        /// （两端一库一 API ⇒ 跨源），免得同一件事写两份、改一处不会报错。
        ///
        /// ⚠️ **已知限制（R-G）**：客户视图按 **`display_name`** 匹配 api 类（客户视图的类
        /// 身份就是人读名，结构上没有 key）⇒ **`display_name` 为空的 api 类会在客户视图里
        /// 漏标** `ontok:sourceKind "api"`（建模视图按 key 匹配，不受影响）。
        /// 这是**漏标不是错标**：那个类在客户视图里看起来像库源，查下去会收到
        /// `NOT_DB_SOURCE`（jsonld 是 `@pending`）。给那个类补一个人读名即可消除。
        #[arg(long)]
        access_mapping: Option<PathBuf>,
    },
}

pub async fn run(cli: Cli) -> anyhow::Result<()> {
    crate::pipe_guard::install_broken_pipe_guard();

    // ⭐ REQ-177 · **连库之前的参数自检**（AC-004）：多项目模式（`--project` /
    // `--version` 都不给）+ `--access-mapping` ⇒ 参数错，退出码 **2**。
    //
    // 位置：必须在取连接串 / 连产品库 / 跑迁移之前。给一个连不上的
    // `ONTOK_DATABASE_URL` 时，用户该看到的是「这个参数组合不允许」，而不是
    // 「连不上库」—— 后者会把人带去查网络与凭据，而真正要改的是启动命令。
    if let Cmd::Query {
        cmd:
            QueryCmd::Serve {
                project,
                version,
                access_mapping,
                ..
            },
    } = &cli.cmd
    {
        if let Err(e) = crate::query::serve::preflight(
            project.as_deref(),
            *version,
            access_mapping.as_deref(),
        ) {
            eprintln!("✗ query serve 拒绝启动：{e}");
            std::process::exit(2);
        }
    }

    // 与 `cli::run` **同一条顺序**：先取连接串、连产品库、跑迁移，再分派。
    // 服务面没有「连库之前就返回」的子命令（那些都在构建面）。
    let url = crate::config::require_env(crate::config::DATABASE_URL_ENV)
        .or_else(|_| crate::config::require_env(crate::config::TEST_DATABASE_URL_ENV))?;
    let pool: PgPool = crate::db::pool::connect(&url).await?;
    crate::db::pg::migrate(&pool).await?;

    match cli.cmd {
        Cmd::Query { cmd } => query_cmd(&pool, cmd).await,
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

async fn query_cmd(pool: &PgPool, cmd: QueryCmd) -> anyhow::Result<()> {
    let QueryCmd::Serve {
        bind,
        tokens,
        project,
        version,
        schema,
        policy,
        ontology_only,
        tenancy,
        allow_public,
        timeout_ms,
        sse_idle_timeout_ms,
        sse_max_connections,
        max_concurrent_queries,
        audit_out,
        base,
        include_inherit,
        access_mapping,
    } = cmd;

    let args = crate::query::serve::ServeArgs {
        bind,
        tokens,
        project,
        version,
        schema,
        policy,
        ontology_only,
        tenancy,
        allow_public,
        timeout_ms,
        sse_idle_timeout_ms,
        sse_max_connections,
        max_concurrent_queries,
        audit_out,
        base,
        include_inherit,
        access_mapping,
    };
    crate::query::serve::serve(pool, args).await
}

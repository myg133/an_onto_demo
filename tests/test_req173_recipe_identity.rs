//! REQ-173 · 让取数配方**自足**：`@pending.get` 必须带 `identity` 与 `iri_template`。
//!
//! # 为什么这条不改就是废配方
//!
//! 取数脚本要把「一行」变成「一个节点」，就必须有 `@id` ⇒ 需要 `identity.key_path`。
//! `@pending.get` 里没有它时，脚本只能要求调用方传 `--id-property <fields 里的属性名>`，
//! 而 `fields` 里是**本体属性名**、`identity.key_path` 是**响应记录里的键名** ——
//! 两者经常不同（实测：属性 `instance_id`，记录里叫 `InstanceId`）⇒ 平台上的 agent
//! 只能猜，`--format rows` 下漏传还会让每行被 `skipped_no_identity` 静默丢掉
//! （**0 行且不报错**，最坏的一种）。本文件判的就是「配方自己把这一环带上」。
//!
//! | # | 判据 | 层 |
//! | --- | --- | --- |
//! | **AC-001** | 真进程 + 真库：`@pending[0].get.identity.key_path` == AM 里该 entity 的 `identity.key_path`（夹具里它 **≠** `fields` 的属性名） | 契约层 + 真进程 |
//! | **AC-002** | `@pending[0].get.iri_template` == AM 里的 `iri_template`（逐字，⛔ 不按 class 拼） | 契约层 + 真进程 |
//! | **AC-003** | v1 AM ⇒ `@pending.get` **仍然恰好 4 键**（class / filter / pushdown / ref，逐键比对；AC-103 不回退） | 契约层 + 真进程 |
//! | **AC-004** | AM 里没写 `identity` ⇒ 配方里**没有** `identity` 键（⛔ 不塞 `{}`、⛔ 不从 `fields` 挑主键） | 契约层 + 真进程 |
//! | **AC-005** | 端到端：真回执**原样**存盘 ⇒ `access_fetch.py --recipe <file> --format jsonld`（**不传** `--id-property`）不再报 `recipe.no_identity`，一路走到凭据/网络层 | 真脚本 |
//!
//! # 红绿自证
//!
//! 把 `build_recipe()` 里填 `identity` 的那一行（`identity: …`）去掉 ⇒
//! **AC-001 红**（`get.identity` 不在）且 **AC-005 红**（脚本在 `recipe.no_identity` 那道闸
//! 就 exit 2，走不到网络层）。AC-003 / AC-004 保持绿 —— 它们判的是「不多出键」，
//! 与本改动方向相反的那一半。
//!
//! ⚠️ AC-005 的判据**不是**「脚本 exit 0」：夹具的 `base_url` 指向一个**刚释放的本地端口**
//! （连不上），所以它必然以 `net.error`（exit 7）结束。能走到那一步，本身就证明
//! 「`recipe.no_identity`」和 `run_fetch` 里那道 `am.invalid`（`identity.key_path`）都过了。
//!
//! # 环境
//!
//! | 变量 | 用途 | 缺了怎样 |
//! | --- | --- | --- |
//! | `ONTOK_TEST_DATABASE_URL`（或 `ONTOK_DATABASE_URL`） | 元数据库（项目 / 版本 / 快照） | **响亮跳过**（契约层的用例照跑） |
//! | `ONTOK_SECRET_KEY` | `query serve` 起进程的写面密钥闸门 | **响亮跳过** |
//! | `ONTOK_ACCESS_FETCH` | `access_fetch.py` 的路径（覆盖默认的 `../Deploy/...`） | 跳过 AC-005（⛔ 不假装通过） |
//!
//! ⚠️ 真进程那几条都起**真二进制**（`CARGO_BIN_EXE_ontok`）：`@pending` 的字节是
//! 「进程怎么装配服务」的产物，在进程内测不到那一层。

mod common;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use ontok::db::project::PgProjectStore;
use ontok::db::snapshot_store::PgStore;
use ontok::domain::model_view::{ModelEntityView, ModelLinkView, ModelTrackView};
use ontok::domain::snapshot::{EntityType, Snapshot};
use ontok::query::source_gate::{AccessMapping, SourceCheck};
use ontok::sparql::query::parse as parse_query;
use serde_json::{json, Value};
use sqlx::postgres::PgPool;

/// 查询 key（≥ 24 字符；判据只要「有且形状对」，⛔ 不是真凭据）。
const TOK: &str = "FAKE-REQ173-QUERY-aaaaaaaaaaaaaaaaaaaa";

/// 本体里的类 key（= AM `entities[].class`）。
const CLASS: &str = "cvm_instance";
/// 本体属性名（= `fields` 的键）—— 它**必须**与下面那个 `key_path` 不同。
const PROPERTY: &str = "instance_id";
/// `identity.key_path` = **响应记录里的键名**（腾讯云 `InstanceId`）。
///
/// ⚠️ 刻意让 `PROPERTY`（`instance_id`，本体属性名）≠ `KEY_PATH`（`InstanceId`，记录键名）：
/// 于是「配方里出现 `InstanceId`」只可能来自 AM 的 `identity.key_path`，
/// 任何「从 `fields` 挑一个属性名当主键」的实现都会给出 `instance_id` ⇒ 断言立刻红。
const KEY_PATH: &str = "InstanceId";
/// `identity` 里额外的一个键：证明透出是**原样**（整个对象），不是只抠 `key_path`。
const TENANT_PATH: &str = "AppId";
/// AM 里写的 IRI 模板（含 `{id}` 占位符）。
const IRI_TEMPLATE: &str = "ontok:cvm_instance/{id}";
/// 「按 class 拼一个」会长的样子（红证：配方里出现它 = 有人在猜）。
const FABRICATED_IRI: &str = "ontok:cvm_instance/{id}";
/// `fields` 归一化后的路径（`PROPERTY` 指向记录里的哪个键）。
const FIELD_PATH: &str = "InstanceId";

const IRI: &str = "https://ontok.local/demo";

// ══════════════════════════════════════════════════════════════
// 夹具：两版 AM + 一条「只看契约」的取配方辅助
// ══════════════════════════════════════════════════════════════

/// 一份最小 AM（tc3 源 + 一条 api 类）。
///
/// - `contract_version = 1` ⇒ 服务端**不建配方**（`Recipe` 全 `None`）⇒ `get` 只有 4 键；
/// - `contract_version = 2` ⇒ 配方摊平进 `get`；
/// - `identity = false` / `iri_template = false` ⇒ **不写**那两个键（AC-004 的夹具）。
fn am(contract_version: u32, identity: bool, iri_template: bool, base_url: &str) -> String {
    let mut ent = json!({
        "id": "CvmInstance",
        "class": CLASS,
        "source": "cvm",
        "capability": { "pushdown": ["ids"] },
        "freshness": "realtime",
        "operation": {
            "action": "DescribeInstances",
            "version": "2017-03-12",
            "method": "POST",
            "content_type": "application/json"
        },
        "request": { "template": { "Limit": 100 } },
        "iterator": "$.Response.InstanceSet[*]",
        "fields": {
            PROPERTY: { "path": FIELD_PATH },
            "instance_name": { "path": "InstanceName" }
        }
    });
    if identity {
        // ⚠️ 多带一个 `tenant_path`：判据是「整个对象原样搬运」，不是只搬 `key_path`。
        ent["identity"] = json!({ "key_path": KEY_PATH, "tenant_path": TENANT_PATH });
    }
    if iri_template {
        ent["iri_template"] = json!(IRI_TEMPLATE);
    }
    json!({
        "contract_version": contract_version,
        "mapping_version": 3,
        "sources": [{
            "id": "cvm",
            "base_url": base_url,
            "auth": {
                "mode": "tc3-hmac-sha256",
                "service": "cvm",
                "credential_env": ["TENCENT_SECRET_ID", "TENCENT_SECRET_KEY"],
                "signed_headers": ["content-type", "host", "x-tc-action"],
                "timestamp": { "header": "X-TC-Timestamp", "format": "unix-seconds" },
                "scope": ["date", "service", "tc3_request"]
            }
        }],
        "entities": [ent]
    })
    .to_string()
}

/// 只跑契约层（不起进程、不连库）：`parse` → `bind` → `check` ⇒ 第 0 条 pending 的 `get`。
fn pending_get(mapping: &str) -> ontok::query::source_gate::GetInstruction {
    let model = model();
    let m = AccessMapping::parse(mapping)
        .expect("AM 形状合法")
        .bind(&model)
        .expect("AM 里的类必须在这一版本体里");
    let q = parse_query(&format!("SELECT ?c WHERE {{ ?c a :{CLASS} }}"), IRI).expect("查询合法");
    let c = m.check(&q, IRI, None);
    assert!(matches!(c, SourceCheck::Api { .. }), "{c:?}");
    c.pending()[0].get.clone().expect("按类点名必须给 get")
}

/// 把服务端那份 `get` 摊平成**线上形状**（`jsonld::PendingGet`）再序列化。
///
/// ⚠️ 必须走这个类型：AC-003 / AC-004 判的是**摊平之后**的键集合
/// （`#[serde(flatten)]` + `skip_serializing_if` 的合力），⛔ 不是 `GetInstruction` 的
/// Rust 字段 —— 后者没有 `Serialize`，也不代表客户端看到的东西。
fn wire_get_json(get: &ontok::query::source_gate::GetInstruction) -> String {
    let pg = ontok::query::jsonld::PendingGet {
        class: get.class.clone(),
        filter: ontok::query::jsonld::PendingFilter { ids: get.ids.clone() },
        pushdown: get.pushdown.clone(),
        reference: get.reference.clone(),
        recipe: get.recipe.clone().unwrap_or_default(),
    };
    serde_json::to_string(&pg).expect("序列化")
}

/// 序列化后的**键名列表**（排序后比较，与键序无关）。
fn keys_of(v: &Value) -> Vec<String> {
    let mut k: Vec<String> = v
        .as_object()
        .unwrap_or_else(|| panic!("不是 JSON 对象：{v}"))
        .keys()
        .cloned()
        .collect();
    k.sort();
    k
}

/// 线上 JSON 文本里**顶层键的字符序**（用于判「字段声明在末尾」）。
///
/// 为什么不用 `serde_json::Value` 判序：本仓没开 `preserve_order`，`Value::Object`
/// 落的是 `BTreeMap` ⇒ 键被排序，原始顺序在 `Value` 里已经丢了。只有序列化后的
/// **文本**还留着线上那份字节顺序。
fn key_order(text: &str, keys: &[&str]) -> Vec<usize> {
    keys.iter()
        .map(|k| {
            text.find(&format!("\"{k}\":"))
                .unwrap_or_else(|| panic!("序列化文本里没有键 {k}：{text}"))
        })
        .collect()
}

/// v1 的四个旧键（顺序无关，比较前排序）。
fn legacy_keys() -> Vec<String> {
    ["class", "filter", "pushdown", "ref"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

fn model() -> ModelTrackView {
    ModelTrackView {
        seq: 1,
        extracted_by: None,
        entity_types: vec![ModelEntityView {
            key: CLASS.to_string(),
            display_name: "云服务器实例".to_string(),
            description: None,
            aliases: vec![],
            parent: None,
            disjoint_with: vec![],
            properties: vec![],
            evidence: vec![],
        }],
        link_types: Vec::<ModelLinkView>::new(),
        action_types: vec![],
        functions: vec![],
    }
}

/// 契约层夹具用的 `base_url`（不起网络，值与判据无关）。
const NO_NET: &str = "https://cvm.tencentcloudapi.com";

// ══════════════════════════════════════════════════════════════
// 契约层（不需要库 / 不需要进程）
// ══════════════════════════════════════════════════════════════

/// **AC-001（契约层）** · v2 + `identity` ⇒ 配方里**整个对象原样**（`key_path` 逐字）。
#[test]
fn ac001_contract_v2_carries_identity_verbatim() {
    let get = pending_get(&am(2, true, true, NO_NET));
    let identity = get
        .recipe
        .as_ref()
        .and_then(|r| r.identity.as_ref())
        .unwrap_or_else(|| panic!("v2 的配方必须带 identity：{get:?}"));
    assert_eq!(
        identity["key_path"], KEY_PATH,
        "`key_path` 必须是 AM 里写的那个值：{identity}"
    );
    // 整个对象都搬（多出来的 `tenant_path` 不许被裁掉）。
    assert_eq!(
        identity,
        &json!({ "key_path": KEY_PATH, "tenant_path": TENANT_PATH }),
        "identity 要**原样**透出，不是只抠 key_path：{identity}"
    );
    // 反证：`fields` 的属性名与 `key_path` 不同 —— 「从 fields 挑一个」会给出 PROPERTY。
    let fields = get.recipe.as_ref().and_then(|r| r.fields.as_ref()).expect("有 fields");
    assert!(fields.contains_key(PROPERTY), "{fields:?}");
    assert_ne!(PROPERTY, KEY_PATH);
    assert!(
        !fields.contains_key(KEY_PATH),
        "夹具要保证 `key_path` **不是** fields 的键：{fields:?}"
    );

    let text = wire_get_json(&get);
    assert_eq!(
        serde_json::from_str::<Value>(&text).unwrap()["identity"]["key_path"],
        KEY_PATH,
        "摊平之后 get.identity.key_path 必须还在：{text}"
    );
    eprintln!("· AC-001（契约层）· get.identity = {identity}");
}

/// **AC-002（契约层）** · v2 + `iri_template` ⇒ 逐字透出（⛔ 不按 class 拼）。
#[test]
fn ac002_contract_v2_carries_iri_template_verbatim() {
    // 换一个**不是**「按 class 拼出来」的模板：猜出来的实现会给出 `FABRICATED_IRI`。
    let other = "https://cvm.example/instance/{id}";
    let base = am(2, true, true, NO_NET);
    let doc = base.replace(IRI_TEMPLATE, other);
    assert!(doc.contains(other), "夹具替换失败");

    for want in [IRI_TEMPLATE, other] {
        let m = if want == IRI_TEMPLATE { base.clone() } else { doc.clone() };
        let get = pending_get(&m);
        let recipe = get.recipe.as_ref().expect("v2 有配方");
        assert_eq!(
            recipe.iri_template.as_deref(),
            Some(want),
            "`iri_template` 必须是 AM 里写的那个值（⛔ 不按 class 拼）：{recipe:?}"
        );
        let v: Value = serde_json::from_str(&wire_get_json(&get)).unwrap();
        assert_eq!(v["iri_template"], want);
    }
    // 红证：那个「按 class 拼」的常量与夹具里写的模板长得一样也没关系 ——
    // 上面第二个值（`https://cvm.example/…`）不是任何拼接规则能产出的。
    assert_ne!(other, FABRICATED_IRI);
}

/// **AC-003（契约层）** · v1 ⇒ `get` 摊平后**恰好 4 键**，逐键比对。
///
/// ⚠️ v1 的 AM 里**照样**写着 `identity` / `iri_template`：证明「4 键」不是靠夹具里
/// 没有这两项混过去的，而是 v1 契约确实不建配方（AC-103 不回退）。
#[test]
fn ac003_contract_v1_get_has_exactly_the_four_legacy_keys() {
    let with_extras = am(1, true, true, NO_NET);
    assert!(with_extras.contains("identity"), "夹具里必须有 identity");
    let get = pending_get(&with_extras);
    assert!(get.recipe.is_none(), "v1 不建配方（AC-103）");

    let text = wire_get_json(&get);
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        keys_of(&v),
        legacy_keys(),
        "v1 的 `@pending.get` 必须逐键等于旧 4 键（一个都不多）：{text}"
    );
    // 逐键再 assert 一次（不是只比列表）：
    for k in legacy_keys() {
        assert!(v.get(&k).is_some(), "缺旧键 {k}：{text}");
    }
    assert!(v.get("identity").is_none(), "{text}");
    assert!(v.get("iri_template").is_none(), "{text}");
    assert!(!text.contains("\"identity\""), "{text}");
    assert!(!text.contains("\"iri_template\""), "{text}");
    // 键序也不许动：v1 的四个键按声明顺序排。
    let pos = key_order(&text, &["class", "filter", "pushdown", "ref"]);
    assert!(pos.windows(2).all(|w| w[0] < w[1]), "v1 键序被打乱：{text}");
    eprintln!("· AC-003（契约层）· v1 @pending.get = {text}");
}

/// **AC-004（契约层）** · AM 里没写 `identity` ⇒ 配方里**没有**这个键（⛔ 不塞 `{}`）。
///
/// `iri_template` 同时**在**：证明两项各自由「AM 写没写」决定，不是绑在一起的开关。
#[test]
fn ac004_contract_without_identity_has_no_identity_key() {
    let get = pending_get(&am(2, false, true, NO_NET));
    let recipe = get.recipe.as_ref().expect("v2 有配方");
    assert!(
        recipe.identity.is_none(),
        "AM 没写 identity ⇒ 配方里必须没有它（⛔ 不塞 {{}}、⛔ 不从 fields 挑主键）：{recipe:?}"
    );
    assert_eq!(recipe.iri_template.as_deref(), Some(IRI_TEMPLATE));

    let text = wire_get_json(&get);
    assert!(!text.contains("\"identity\""), "摊平后出现了 identity 键：{text}");
    assert!(text.contains("\"iri_template\""), "iri_template 不该被连坐：{text}");
    eprintln!("· AC-004（契约层）· 无 identity 的 get = {text}");

    // 反过来：两项都没写 ⇒ 两个键都不出现，且配方其余部分照旧。
    let bare = pending_get(&am(2, false, false, NO_NET));
    let text = wire_get_json(&bare);
    assert!(!text.contains("\"identity\""), "{text}");
    assert!(!text.contains("\"iri_template\""), "{text}");
    assert!(text.contains("\"fields\""), "其余配方段照旧：{text}");
}

/// **REQ-173 的结构约束** · 两个新字段**声明在末尾**：`identity` 在 `auth` 之后、
/// `iri_template` 在 `identity` 之后 ⇒ v2 既有那几段的键序一个字符不动（只做加法）。
#[test]
fn new_fields_are_appended_after_the_existing_recipe_keys() {
    let get = pending_get(&am(2, true, true, NO_NET));
    let text = wire_get_json(&get);
    let pos = key_order(
        &text,
        &[
            "class",
            "filter",
            "pushdown",
            "ref",
            "source",
            "operation",
            "request",
            "iterator",
            "fields",
            "auth",
            "identity",
            "iri_template",
        ],
    );
    assert!(
        pos.windows(2).all(|w| w[0] < w[1]),
        "v2 的键序必须是「旧 4 键 → 既有配方段 → identity → iri_template」：{text}"
    );
    eprintln!("· 键序 · {text}");
}

// ══════════════════════════════════════════════════════════════
// 真进程 + 真库
// ══════════════════════════════════════════════════════════════

fn ontok_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_ontok"))
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "ontok-req173-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&d).expect("建临时目录");
    d
}

fn secret_key() -> String {
    // 64 位十六进制（32 字节）。⛔ 不写死真密钥 —— 判据只要「有且形状对」。
    "ab".repeat(32)
}

/// 一个「可服务」的最小项目：一条 api 类 + 已冻结的第 1 版。
struct Live {
    pool: PgPool,
    project: String,
    version: i32,
    dir: PathBuf,
}

/// 起库夹具。缺环境 ⇒ **响亮跳过**并返回 `None`（⛔ 不假装通过）。
async fn live(tag: &str, test: &str) -> Option<Live> {
    if ontok::config::test_database_url().is_err() {
        common::skip::missing("ONTOK_TEST_DATABASE_URL", test);
        return None;
    }
    // `query serve` 单项目模式**无条件**加载写面密钥（缺 ⇒ 拒绝启动），所以这里先判。
    if std::env::var("ONTOK_SECRET_KEY")
        .map(|v| v.trim().is_empty())
        .unwrap_or(true)
    {
        common::skip::missing("ONTOK_SECRET_KEY", test);
        return None;
    }
    let pool = common::test_pool().await;
    common::migrate(&pool).await;

    let projects = PgProjectStore::new(pool.clone());
    let project = common::unique_project(&format!("req173{tag}"));
    let p = common::new_project(&projects, &project).await;
    let v = projects.new_version(p.id).await.expect("新版本");

    let mut snap = Snapshot::new(v.id, v.seq);
    snap.entity_types = vec![EntityType {
        key: CLASS.to_string(),
        display_name: "云服务器实例".to_string(),
        description: None,
        aliases: vec![],
        parent: None,
        disjoint_with: vec![],
        properties: vec![],
        evidence: vec![],
    }];
    PgStore::new(pool.clone())
        .save_snapshot(&snap)
        .await
        .expect("存快照");
    // 单项目模式的 `query serve` 只服务**已冻结**的版本（与 `serve` 同一条闸门）。
    projects.freeze(v.id).await.expect("冻结");

    Some(Live { pool, project, version: v.seq, dir: temp_dir(tag) })
}

async fn cleanup(live: &Live) {
    let _ = sqlx::query("DELETE FROM projects WHERE name = $1")
        .bind(&live.project)
        .execute(&live.pool)
        .await;
    let _ = std::fs::remove_dir_all(&live.dir);
}

struct Server {
    child: Child,
    port: u16,
    stderr: PathBuf,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Server {
    /// 进程的 stderr（起不来时用它给出**人话**，而不是一句「连不上端口」）。
    fn stderr_text(&self) -> String {
        std::fs::read_to_string(&self.stderr).unwrap_or_default()
    }
}

fn free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").expect("要一个空闲端口");
    let p = l.local_addr().expect("addr").port();
    drop(l);
    p
}

fn tokens_json() -> String {
    format!(r#"{{"version":"1.0.0","tokens":[{{"label":"agent","token":"{TOK}"}}]}}"#)
}

/// 起 **真进程**：`ontok query serve --ontology-only --project … --version … --access-mapping …`。
///
/// `--ontology-only`：判据不需要客户源库（纯 api 类的 `@pending` 不碰库）。
fn spawn(live: &Live, am_text: &str) -> Server {
    let am_path = live.dir.join("access-mapping.json");
    std::fs::write(&am_path, am_text).expect("写 AM");
    let tokens = live.dir.join("tokens.json");
    std::fs::write(&tokens, tokens_json()).expect("写 token 表");
    let stderr_path = live.dir.join("serve.stderr.log");
    let port = free_port();

    let child = Command::new(ontok_bin())
        .args([
            "query",
            "serve",
            "--bind",
            &format!("127.0.0.1:{port}"),
            "--tokens",
            tokens.to_str().unwrap(),
            "--ontology-only",
            "--project",
            &live.project,
            "--version",
            &live.version.to_string(),
            "--access-mapping",
            am_path.to_str().unwrap(),
        ])
        .env("ONTOK_DATABASE_URL", ontok::config::test_database_url().unwrap())
        .env("ONTOK_SECRET_KEY", secret_key())
        .stdout(Stdio::null())
        .stderr(Stdio::from(
            std::fs::File::create(&stderr_path).expect("建 stderr 日志"),
        ))
        .spawn()
        .expect("起 ontok 子进程");

    let srv = Server { child, port, stderr: stderr_path };
    wait_ready(&srv);
    srv
}

fn wait_ready(srv: &Server) {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(30) {
        if let Ok((200, _)) = std::panic::catch_unwind(|| {
            http(srv.port, "GET", "/api/health", &Vec::new(), &[])
        }) {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!(
        "服务在 30 秒内没有就绪（端口 {}）。stderr：\n{}",
        srv.port,
        srv.stderr_text()
    );
}

/// 裸 HTTP/1.1（`Connection: close`）：返回 `(状态码, 正文)`。
fn http(
    port: u16,
    method: &str,
    path: &str,
    headers: &[(String, String)],
    body: &[u8],
) -> (u16, Vec<u8>) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).expect("连上服务");
    s.set_read_timeout(Some(Duration::from_secs(20)))
        .expect("设读超时");
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n");
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    if !body.is_empty() {
        req.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    req.push_str("\r\n");
    s.write_all(req.as_bytes()).expect("写请求头");
    s.write_all(body).expect("写请求体");
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).expect("读响应");

    let pos = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("响应头结束标记");
    let head = String::from_utf8_lossy(&buf[..pos]).to_string();
    let status: u16 = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse().ok())
        .expect("状态行");
    let raw = &buf[pos + 4..];
    let chunked = head.to_ascii_lowercase().contains("transfer-encoding: chunked");
    (status, if chunked { dechunk(raw) } else { raw.to_vec() })
}

fn dechunk(mut raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let Some(eol) = raw.windows(2).position(|w| w == b"\r\n") else {
            break;
        };
        let size_txt = String::from_utf8_lossy(&raw[..eol]).to_string();
        let size = usize::from_str_radix(size_txt.trim().split(';').next().unwrap_or("0"), 16)
            .unwrap_or(0);
        raw = &raw[eol + 2..];
        if size == 0 {
            break;
        }
        out.extend_from_slice(&raw[..size.min(raw.len())]);
        raw = &raw[(size + 2).min(raw.len())..];
    }
    out
}

/// 打一条「按类点名」的查询，拿回 JSON-LD 回执（`@pending` 在里面）。
fn sparql_jsonld(srv: &Server) -> Value {
    serde_json::from_str(&sparql_jsonld_raw(srv).1)
        .unwrap_or_else(|e| panic!("不是 JSON（{e}）"))
}

/// 同上，但同时返回**原始正文**（AC-005 要「原样存盘」，⛔ 不重打一遍）。
fn sparql_jsonld_raw(srv: &Server) -> (Value, String) {
    let body = json!({ "query": format!("SELECT ?c WHERE {{ ?c a :{CLASS} }}") }).to_string();
    let headers = vec![
        ("Authorization".to_string(), format!("Bearer {TOK}")),
        ("Content-Type".to_string(), "application/json".to_string()),
    ];
    let (status, raw) = http(
        srv.port,
        "POST",
        "/api/sparql?format=jsonld",
        &headers,
        body.as_bytes(),
    );
    let text = String::from_utf8_lossy(&raw).to_string();
    assert_eq!(
        status, 200,
        "纯 api 类的 jsonld 查询应当 200 且带 @pending。stderr：\n{}\nbody：{text}",
        srv.stderr_text()
    );
    let v = serde_json::from_str(&text).unwrap_or_else(|e| panic!("不是 JSON（{e}）：{text}"));
    (v, text)
}

/// 回执里的 `@pending[0].get`。
fn pending_get_of(receipt: &Value) -> Value {
    let pending = receipt["@pending"]
        .as_array()
        .unwrap_or_else(|| panic!("回执里没有 @pending 数组：{receipt}"));
    assert_eq!(pending.len(), 1, "只点名了一个类：{receipt}");
    pending[0]["get"].clone()
}

/// 一个**刚释放**的本地端口：连不上，但错误发生在网络层（AC-005 用它证明「走过了闸门」）。
fn closed_port() -> u16 {
    free_port()
}

/// **AC-001（真进程 + 真库）** · v2 AM（含 `identity`）⇒
/// `@pending[0].get.identity.key_path` **等于 AM 里的值**，且它**不是** `fields` 的属性名。
#[tokio::test]
async fn ac001_live_pending_get_identity_key_path() {
    let test = "ac001_live_pending_get_identity_key_path";
    let Some(live) = live("a1", test).await else { return };
    let srv = spawn(&live, &am(2, true, true, NO_NET));

    let receipt = sparql_jsonld(&srv);
    let get = pending_get_of(&receipt);
    let identity = get
        .get("identity")
        .unwrap_or_else(|| panic!("v2 的 get 必须有 identity：{get}"));
    assert_eq!(
        identity["key_path"], KEY_PATH,
        "`@pending[0].get.identity.key_path` 必须等于 AM 里该 entity 的值 —— \
         没有它，取数脚本只能让调用方猜 `--id-property`（这就是本需求的全部）：{identity}"
    );
    // ⭐ 防「猜对了也算」：属性名叫 `instance_id`，`key_path` 叫 `InstanceId`。
    let fields = get.get("fields").unwrap_or_else(|| panic!("get 必须有 fields：{get}"));
    let field_keys: Vec<&str> = fields.as_object().unwrap().keys().map(String::as_str).collect();
    assert!(field_keys.contains(&PROPERTY), "{fields}");
    assert!(
        !field_keys.contains(&KEY_PATH),
        "`fields` 里不该有 {KEY_PATH}（那是**记录**里的键名，不是本体属性名）：{fields}"
    );
    // ⚠️ 这条判据有牙：AM 里 fixture 的 identity 整个对象都在。
    assert_eq!(
        identity,
        &json!({ "key_path": KEY_PATH, "tenant_path": TENANT_PATH }),
        "identity 要原样透出（含额外的 tenant_path）：{identity}"
    );
    eprintln!("· AC-001 · @pending[0].get.identity = {identity}");
    eprintln!("· AC-001 · @pending[0].get.fields = {fields}");

    drop(srv);
    cleanup(&live).await;
}

/// **AC-002（真进程 + 真库）** · `@pending[0].get.iri_template` 逐字等于 AM 里的模板。
#[tokio::test]
async fn ac002_live_pending_get_iri_template() {
    let test = "ac002_live_pending_get_iri_template";
    let Some(live) = live("a2", test).await else { return };
    let srv = spawn(&live, &am(2, true, true, NO_NET));

    let receipt = sparql_jsonld(&srv);
    let get = pending_get_of(&receipt);
    assert_eq!(
        get["iri_template"], IRI_TEMPLATE,
        "`@pending[0].get.iri_template` 必须等于 AM 里写的模板（⛔ 不按 class 拼）：{get}"
    );
    eprintln!("· AC-002 · @pending[0].get.iri_template = {}", get["iri_template"]);

    drop(srv);
    cleanup(&live).await;
}

/// **AC-003（真进程 + 真库）** · v1 AM ⇒ `@pending.get` 仍然**只有 4 键**（逐键比对）。
#[tokio::test]
async fn ac003_live_v1_pending_get_has_only_the_four_legacy_keys() {
    let test = "ac003_live_v1_pending_get_has_only_the_four_legacy_keys";
    let Some(live) = live("a3", test).await else { return };
    // 契约 v1 但 AM 里**照样写着** identity / iri_template：证明「4 键」不是靠夹具里
    // 没有这两项混过去的。
    let am_text = am(1, true, true, NO_NET);
    assert!(am_text.contains("identity"), "夹具里必须有 identity");
    let srv = spawn(&live, &am_text);

    let receipt = sparql_jsonld(&srv);
    let get = pending_get_of(&receipt);
    assert_eq!(
        keys_of(&get),
        legacy_keys(),
        "v1 的 `@pending.get` 必须逐键等于那 4 个（AC-103 不回退）：{get}"
    );
    for k in legacy_keys() {
        assert!(get.get(&k).is_some(), "缺旧键 {k}：{get}");
    }
    let text = serde_json::to_string(&get).unwrap();
    assert!(!text.contains("\"identity\""), "{text}");
    assert!(!text.contains("\"iri_template\""), "{text}");
    eprintln!("· AC-003 · v1 @pending[0].get = {text}");

    drop(srv);
    cleanup(&live).await;
}

/// **AC-004（真进程 + 真库）** · AM 里没写 `identity` ⇒ 配方里**没有**这个键。
#[tokio::test]
async fn ac004_live_without_identity_has_no_identity_key() {
    let test = "ac004_live_without_identity_has_no_identity_key";
    let Some(live) = live("a4", test).await else { return };
    let srv = spawn(&live, &am(2, false, true, NO_NET));

    let receipt = sparql_jsonld(&srv);
    let get = pending_get_of(&receipt);
    assert!(
        get.get("identity").is_none(),
        "AM 没写 identity ⇒ 配方里不许出现它（⛔ 不塞 {{}}、⛔ 不从 fields 挑主键）：{get}"
    );
    // `iri_template` 同时在：两项各自由「AM 写没写」决定。
    assert_eq!(get["iri_template"], IRI_TEMPLATE, "{get}");
    let text = serde_json::to_string(&get).unwrap();
    assert!(!text.contains("\"identity\""), "{text}");
    eprintln!("· AC-004 · 无 identity 的 get = {text}");

    drop(srv);
    cleanup(&live).await;
}

// ══════════════════════════════════════════════════════════════
// AC-005 · 端到端：真回执**原样存盘** ⇒ 真脚本不传 `--id-property`
// ══════════════════════════════════════════════════════════════

/// `access_fetch.py` 在哪：环境变量优先，否则取**部署树里的那一份**
/// （`<仓库根>/../Deploy/skills/ontok-query/scripts/access_fetch.py`）。
fn access_fetch() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("ONTOK_ACCESS_FETCH") {
        let p = PathBuf::from(p);
        return p.is_file().then_some(p);
    }
    let root = common::crate_root();
    let p = root
        .parent()
        .unwrap_or(&root)
        .join("Deploy")
        .join("skills")
        .join("ontok-query")
        .join("scripts")
        .join("access_fetch.py");
    p.is_file().then_some(p)
}

/// 找一个能跑的 Python（`python` / `py`）。找不到 ⇒ 返回 `None`。
fn python() -> Option<&'static str> {
    for exe in ["python", "py"] {
        if Command::new(exe)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            return Some(exe);
        }
    }
    None
}

/// **AC-005** · 把**真进程**发出的回执**原样**存盘（一个字都不改），然后
/// `access_fetch.py --recipe <file> --format jsonld`（**不传** `--id-property`、不传 `--entity`）：
/// 不许再报 `recipe.no_identity`，且必须走到凭据/网络层。
///
/// 判据是三件事同时成立：
/// ① 输出里**没有** `recipe.no_identity`（脚本自己的闸门码）；
/// ② 输出里**没有** `identity.key_path` 的拒绝原文（`run_fetch` 里那道 `am.invalid`）；
/// ③ exit == 7（`EXIT_NETWORK`）且输出里有 `net.` 错误码 —— 证明它真的**发出去了请求**，
///    而不是卡在输入校验上。夹具的 `base_url` 指向一个刚释放的本地端口 ⇒ 连不上但立刻失败。
#[tokio::test]
async fn ac005_live_receipt_saved_verbatim_feeds_access_fetch_without_id_property() {
    let test = "ac005_live_receipt_saved_verbatim_feeds_access_fetch_without_id_property";
    let Some(live) = live("a5", test).await else { return };
    let Some(script) = access_fetch() else {
        common::skip::missing(
            "Deploy/skills/ontok-query/scripts/access_fetch.py（可用 ONTOK_ACCESS_FETCH 指定）",
            test,
        );
        cleanup(&live).await;
        return;
    };
    let Some(py) = python() else {
        common::skip::missing("可执行的 Python（python / py）", test);
        cleanup(&live).await;
        return;
    };

    // `base_url` 指向一个刚释放的端口：网络层立刻失败，判据与真实云 API 无关。
    let dead = closed_port();
    let base_url = format!("http://127.0.0.1:{dead}");
    let srv = spawn(&live, &am(2, true, true, &base_url));
    let (receipt, raw) = sparql_jsonld_raw(&srv);

    // 前提：这一段确实带了 identity / iri_template（否则本用例测的就不是 AC-001 的产物）。
    let get = pending_get_of(&receipt);
    assert_eq!(get["identity"]["key_path"], KEY_PATH, "{get}");
    assert_eq!(get["iri_template"], IRI_TEMPLATE, "{get}");

    // 「原样存盘」= 把服务端响应的**原始正文**写进文件（⛔ 重打一遍会丢/改字段）。
    let recipe_path = live.dir.join("receipt.json");
    std::fs::write(&recipe_path, &raw).expect("原样存盘");
    let saved = std::fs::read_to_string(&recipe_path).expect("读回");
    assert_eq!(saved, raw, "存盘的内容必须与真回执逐字节相同");
    eprintln!("· AC-005 · 原样存盘 = {}", recipe_path.display());

    let out = run_access_fetch(py, &script, &recipe_path);
    let all = format!("{}\n{}", out.0, out.1);
    eprintln!("· AC-005 · exit = {:?}", out.2);
    eprintln!("· AC-005 · stdout:\n{}", out.0);
    if !out.1.trim().is_empty() {
        eprintln!("· AC-005 · stderr:\n{}", out.1);
    }

    // ① + ②：两道 identity 闸门都没响。
    for bad in ["recipe.no_identity", "recipe.no_fixture"] {
        assert!(
            !all.contains(bad),
            "配方自带了 identity，不该再报 {bad}：\n{all}"
        );
    }
    assert!(
        !all.contains("identity.key_path"),
        "脚本仍在抱怨 identity.key_path（说明 identity 没过去）：\n{all}"
    );
    // ③：走到了网络层（连不上是夹具安排的）。
    assert_eq!(
        out.2,
        Some(7),
        "应当以 `net.*`（exit 7）结束 —— 那才证明它过了输入校验、真发了请求：\n{all}"
    );
    assert!(
        all.contains("net."),
        "输出里没有网络层错误码（net.error / net.timeout）：\n{all}"
    );
    // 反证：`--format jsonld` 这条路**确实**需要 @id（否则本用例是空跑）。
    assert!(
        all.contains("连不上") || all.contains("网络") || all.contains("超时"),
        "没看到网络失败的原文：\n{all}"
    );

    drop(srv);
    cleanup(&live).await;
}

/// 跑一次 `--recipe <file> --format jsonld`（**刻意不传** `--id-property` / `--entity`）。
///
/// 返回 `(stdout, stderr, exit_code)`。假凭据：判据是「过不过 identity 闸门」，不是真取数。
fn run_access_fetch(py: &str, script: &Path, recipe: &Path) -> (String, String, Option<i32>) {
    let out = Command::new(py)
        .arg(script)
        .args(["--recipe"])
        .arg(recipe)
        .args(["--format", "jsonld", "--timeout", "5"])
        .env("TENCENT_SECRET_ID", "FAKE-REQ173-SECRET-ID")
        .env("TENCENT_SECRET_KEY", "FAKE-REQ173-SECRET-KEY")
        // 本地端口不该走代理（有代理的机器上这会改变失败点）。
        .env("NO_PROXY", "127.0.0.1,localhost")
        .env("no_proxy", "127.0.0.1,localhost")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("跑 access_fetch.py");
    (
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
        out.status.code(),
    )
}

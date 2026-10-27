//! ⭐ REQ-162 · **本地评审台**的领域层与 HTTP 面（草稿 → 人签 → 定版 → 导出包）。
//!
//! ```text
//! ontok-builder console serve --dir <工作目录> [--bind 127.0.0.1:8899] [--no-browser]
//! ```
//!
//! # 它是**纯文件**的，一行业务 SQL 都没有
//!
//! | 概念 | 落在哪 |
//! | --- | --- |
//! | 草稿（agent 产出，**只读**） | `<dir>/out/proposal.json` |
//! | 裁决（人签，唯一可写状态） | `<dir>/.ontok/decisions.json` |
//! | 定版（冻结） | `<dir>/.ontok/versions/v<N>/{snapshot,meta}.json` |
//! | 构建包（只含定版） | `<dir>/.ontok/bundles/<project>-v<N>.ontokbundle` |
//!
//! # 两个变换都是**现成的纯函数**（本文件不重写建模规则）
//!
//! - 定版：`crate::domain::apply::apply(ApplyInput{..}) -> Snapshot` —— 零 I/O（`src/domain/apply.rs`）；
//! - 打包：`crate::bundle::pack::{PackInput, assemble}` —— 零 I/O（`src/bundle/pack.rs`）。
//!
//! ⛔ 本模块**没有** `PgPool` / `sqlx` / 网络；`ONTOK_DATABASE_URL` / `ONTOK_SECRET_KEY`
//! 缺了也照常起（AC-007）。判据见 `tests/test_req162_console.rs` 的真进程用例。
//!
//! # 与旧「控制面」的关系（诚实登记）
//!
//! REQ-124/159 的库版路由（`console::http::build_router`）**仍然存在**，但它只服务
//! `ontok-builder review open`（REQ-160 的一次性口令审核台，不在本单可改范围）。
//! `console serve` **不再挂载**那份路由：它只挂本文件的 6+1 条纯文件端点。
//! 详见 `BA/.local/dispatch/REQ-162-dev-changelog.md`。
//!
//! # 定版的语义
//!
//! - **签完才能定版**：任一轨还是 `pending` ⇒ 400（`DRAFT_NOT_READY`），页面也点不动；
//! - **拒绝的条目只是不进这一版**：定版时只把两轨都 `accepted` 的条目交给 `apply`；
//! - **定版后草稿再变，旧定版不跟着变**：要改就再定一版（vN+1），旧目录一个字节不动；
//! - `decisions.proposal_sha256` 与当前草稿不符 ⇒ `state.stale = true`；定版会拒
//!   （`DECISIONS_STALE`）—— 人要么重新签，要么把草稿恢复成上一份。
//!
//! # 端点（6 + 1）
//!
//! | # | 端点 | 行为 |
//! | --- | --- | --- |
//! | 1 | `GET /api/state` | 目录 · 草稿进度 · 最近定版 · 最近包 |
//! | 2 | `GET /api/draft` | 草稿全文 + 每条当前裁决 + 进度 |
//! | 3 | `POST /api/decide` | `{key, biz?, model?, reason?}` → 写 `decisions.json` |
//! | 4 | `POST /api/finalize` | 要求 `pending == 0`；算快照 → `versions/v<N>/` |
//! | 5 | `GET /api/versions` | 定版列表（倒序） |
//! | 6 | `POST /api/bundle` | `{seq?}`（默认最新定版）→ 写 `.ontok/bundles/` |
//! | 7 | `GET /api/bundle/download?seq=` | `application/octet-stream` 原始字节 |
//!
//! 错误体沿用既有的 `{code, message, hint}` 三件套（`console::http::fail` 是唯一出口）。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::OpError;
use crate::bundle::pack::{assemble, PackInput};
use crate::domain::apply::{apply, is_doc_origin, reviewed_by_of, ApplyError, ApplyInput};
use crate::domain::review::{summarize, Decision, ItemView};
use crate::domain::snapshot::{ExtractedBy, Snapshot};

/// 草稿相对工作目录的落点（agent 的产物，**只读**）。
pub const DRAFT_REL: &str = "out/proposal.json";
/// 裁决相对工作目录的落点（页面唯一可写的状态）。
pub const DECISIONS_REL: &str = ".ontok/decisions.json";
/// 定版目录（`v<N>/` 在这里面）。
pub const VERSIONS_REL: &str = ".ontok/versions";
/// 构建包目录。
pub const BUNDLES_REL: &str = ".ontok/bundles";
/// 单次请求体上限（裁决请求只有几十字节；给 1 MiB 已经非常宽松）。
const MAX_BODY_BYTES: usize = 1024 * 1024;
/// `POST /api/finalize` 的响应里快照的 id 取值前缀（审计/排障时能看出是本机评审台产的）。
const SIGNED_BY: &str = "local-review";

// ══════════════════════════════════════════════════════════════
// 工作目录 + 项目名（一个工作目录 = 一个项目）
// ══════════════════════════════════════════════════════════════

/// 一个工作目录就是**一个项目**（用户口径：不建项目、不选项目）。
#[derive(Debug, Clone)]
pub struct LocalConsole {
    /// 已经 canonicalize 过的工作目录（横幅 / 响应里用它）。
    dir: PathBuf,
    /// 包 manifest 里的项目名（目录名 sanitize 到 schema 的 pattern）。
    project: String,
}

impl LocalConsole {
    /// canonicalize 工作目录并派生项目名。
    ///
    /// ⚠️ 这里**不创建**任何目录：只读目录（或暂时没写权限）也应当能把页面打开、
    /// 能看草稿。`.ontok/` 在第一次写裁决 / 定版 / 打包时才建。
    pub fn open(dir: impl AsRef<Path>) -> Result<Self, OpError> {
        let raw = dir.as_ref();
        let dir = std::fs::canonicalize(raw).map_err(|e| {
            OpError::not_found(
                "WORKDIR_NOT_FOUND",
                format!("工作目录不存在或不可读：{}（{e}）", raw.display()),
                "传一个已经存在的目录：`console serve --dir <工作目录>`。\
                 草稿约定放在 `<工作目录>/out/proposal.json`（agent 的产物）。",
            )
        })?;
        if !dir.is_dir() {
            return Err(OpError::bad_request(
                "WORKDIR_NOT_DIR",
                format!("`--dir` 指向的不是目录：{}", dir.display()),
                "`--dir` 要一个**目录**（一个工作目录 = 一个项目），不是 proposal.json 本身。",
            ));
        }
        let name = project_name_of(&dir);
        Ok(Self { dir, project: name })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn project(&self) -> &str {
        &self.project
    }

    fn draft_path(&self) -> PathBuf {
        self.dir.join(DRAFT_REL)
    }

    fn decisions_path(&self) -> PathBuf {
        self.dir.join(DECISIONS_REL)
    }

    fn versions_dir(&self) -> PathBuf {
        self.dir.join(VERSIONS_REL)
    }

    fn bundles_dir(&self) -> PathBuf {
        self.dir.join(BUNDLES_REL)
    }

    fn version_dir(&self, seq: i32) -> PathBuf {
        self.versions_dir().join(format!("v{seq}"))
    }

    fn snapshot_path(&self, seq: i32) -> PathBuf {
        self.version_dir(seq).join("snapshot.json")
    }

    fn meta_path(&self, seq: i32) -> PathBuf {
        self.version_dir(seq).join("meta.json")
    }

    fn bundle_path(&self, seq: i32) -> PathBuf {
        self.bundles_dir()
            .join(format!("{}-v{seq}.ontokbundle", self.project))
    }

    /// 草稿字节（`None` = 文件不存在；**不**把「不存在」当错误，页面要给可执行提示）。
    fn read_draft(&self) -> Result<Option<Vec<u8>>, OpError> {
        let path = self.draft_path();
        match std::fs::read(&path) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(OpError::internal(format!(
                "读不出草稿 {}：{e}。改法：确认这个文件对当前用户可读；\
                 草稿是 agent 的产物（`ontok-builder propose` 写到 out/proposal.json），评审台不会改它。",
                path.display()
            ))),
        }
    }

    /// 裁决文件（缺失 = 全 pending；**损坏要报错**，⛔ 不静默覆盖人的签字）。
    fn read_decisions(&self) -> Result<DecisionsFile, OpError> {
        let path = self.decisions_path();
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(DecisionsFile::default())
            }
            Err(e) => {
                return Err(OpError::internal(format!(
                    "读不出裁决 {}：{e}。改法：确认这个文件对当前用户可读；\
                     要重来就把 .ontok/decisions.json 移走（页面会从全 pending 开始）。",
                    path.display()
                )))
            }
        };
        serde_json::from_slice(&bytes).map_err(|e| {
            OpError::internal(format!(
                "裁决文件 {} 不是合法 JSON：{e}。\
                 ⛔ 评审台不会静默覆盖它（那会丢掉人的签字）。\
                 改法：修好这个文件，或把它移走再重新签（页面从全 pending 开始）。",
                path.display()
            ))
        })
    }

    /// 最近一个定版号（`None` = 一个都还没有）。
    fn latest_seq(&self) -> Option<i32> {
        let mut seqs = self.version_seqs();
        seqs.pop()
    }

    /// 已经落盘的定版号，**升序**。
    fn version_seqs(&self) -> Vec<i32> {
        let dir = self.versions_dir();
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Vec::new();
        };
        let mut out: Vec<i32> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name();
                let name = name.to_string_lossy();
                let rest = name.strip_prefix('v')?;
                rest.parse::<i32>().ok().filter(|n| *n >= 1)
            })
            .collect();
        out.sort_unstable();
        out
    }

    /// 横幅用的一行概况（**永不失败**：读不出来就报 0，横幅不能把服务拦住）。
    ///
    /// 返回值：`(草稿, 条数, 已签, 最新定版, 构建包)` —— 全是零业务数据的计数/文件名。
    pub(crate) fn draft_summary(&self) -> (String, usize, usize, String, String) {
        let draft = if self.draft_path().is_file() {
            DRAFT_REL.to_string()
        } else {
            format!("{DRAFT_REL}（缺失）")
        };
        let (mut total, mut signed) = (0usize, 0usize);
        if let Ok(Some(bytes)) = self.read_draft() {
            if let Ok((_, mut items)) = parse_draft(&bytes) {
                if let Ok(decisions) = self.read_decisions() {
                    merge_decisions(&mut items, &decisions);
                }
                let progress = Progress::of(&items);
                total = progress.total;
                signed = progress.signed;
            }
        }
        let latest = self
            .latest_seq()
            .map(|s| format!("v{s}"))
            .unwrap_or_else(|| "还没有".to_string());
        let bundle = self
            .latest_seq()
            .map(|s| self.bundle_path(s))
            .filter(|p| p.is_file())
            .map(|p| file_name_of(&p))
            .unwrap_or_else(|| "还没有".to_string());
        (draft, total, signed, latest, bundle)
    }

    fn load_snapshot(&self, seq: i32) -> Result<Snapshot, OpError> {
        let path = self.snapshot_path(seq);
        let bytes = std::fs::read(&path).map_err(|e| {
            OpError::internal(format!(
                "读不出定版快照 {}：{e}。改法：这一版目录可能被手工动过；\
                 删掉 .ontok/versions/v{seq} 后重新定版（旧版本丢了就重签一次）。",
                path.display()
            ))
        })?;
        serde_json::from_slice(&bytes).map_err(|e| {
            OpError::internal(format!(
                "定版快照 {} 不是合法的 Snapshot JSON：{e}。\
                 改法：删掉 .ontok/versions/v{seq} 后重新定版。",
                path.display()
            ))
        })
    }
}

/// 目录名 → 包 manifest 的项目名（`^[a-z0-9][a-z0-9_-]{1,62}$`）。
///
/// 为什么不是「直接用目录名」：`bundle::pack` 的项目名 pattern 只接受小写字母 /
/// 数字 / `-` / `_`，而工作目录可能是 `My Project`（空格）、`本体`（非 ASCII）、
/// `a`（太短）。**确定性**映射（同一目录永远同一个名字）比「猜一个像样的名字」重要：
/// 包名、定版目录与页面标题都从这一个函数来。
pub fn project_name_of(dir: &Path) -> String {
    let raw = dir
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut s: String = raw
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    // pattern 的第三个条件：长度 2..=63。
    s.truncate(63);
    if s.is_empty() || !s.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit()) {
        s.insert(0, 'p');
    }
    if s.len() < 2 {
        s.push('_');
    }
    s
}

// ══════════════════════════════════════════════════════════════
// 存储形状
// ══════════════════════════════════════════════════════════════

/// `decisions.json` —— 人的裁决（**唯一可写状态**）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DecisionsFile {
    /// 写这份裁决时的草稿 sha256（用于判「草稿变了，裁决可能过期」）。
    #[serde(default)]
    pub proposal_sha256: String,
    #[serde(default)]
    pub items: BTreeMap<String, StoredDecision>,
}

/// 一条裁决（两轨 + 理由 + 时间）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StoredDecision {
    #[serde(default)]
    pub biz: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub decided_at: String,
}

impl StoredDecision {
    fn biz_decision(&self) -> Decision {
        decision_of(&self.biz)
    }
    fn model_decision(&self) -> Decision {
        decision_of(&self.model)
    }
    fn signed(&self) -> bool {
        self.biz_decision() != Decision::Pending && self.model_decision() != Decision::Pending
    }
}

fn decision_of(s: &str) -> Decision {
    match s {
        "accepted" => Decision::Accepted,
        "rejected" => Decision::Rejected,
        _ => Decision::Pending,
    }
}

fn decision_name(d: Decision) -> &'static str {
    d.as_str()
}

/// 一条草稿条目 + 它当前的裁决（页面直接消费的形状）。
#[derive(Debug, Clone, Serialize)]
pub struct DraftItem {
    /// 稳定 key（`seq:kind:target`，与库版 `ItemView::id` 同一个命名法）。
    pub key: String,
    pub seq: usize,
    pub kind: String,
    pub target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_type: Option<String>,
    pub payload: Value,
    pub biz: String,
    pub model: String,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decided_at: Option<String>,
    pub signed: bool,
}

/// `{total, signed, pending}`。
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Progress {
    pub total: usize,
    pub signed: usize,
    pub pending: usize,
}

impl Progress {
    fn of(items: &[DraftItem]) -> Self {
        let total = items.len();
        let signed = items.iter().filter(|i| i.signed).count();
        Self {
            total,
            signed,
            pending: total - signed,
        }
    }
}

/// 定版的元信息（`meta.json`，与页面/接口共用同一形状）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionMeta {
    pub seq: i32,
    pub created_at: String,
    /// 这一版里**实际进了快照**的条目数（两轨 accepted 的那些）。
    pub items: usize,
    pub snapshot_sha256: String,
    pub proposal_sha256: String,
}

// ══════════════════════════════════════════════════════════════
// 草稿解析（**只读**；⛔ 不写 proposal.json）
// ══════════════════════════════════════════════════════════════

/// 草稿条目里允许出现的 kind（与 `domain::review::ItemKind` 一一对应）。
///
/// ⛔ **不**复用 `db::proposal::parse_kind` 的兜底：那个函数把不认识的 kind 落成
/// `AddProperty`（历史包袱，见它的注释）。评审台要 fail-closed —— 一个拼错的 kind
/// 必须当场点名，不能变成一条「加属性」混进定版。
const KNOWN_KINDS: &[&str] = &[
    "add_entity_type",
    "add_property",
    "add_link_type",
    "remove_entity_type",
    "modify_entity_type",
    "add_concept_group",
];

fn kind_of(raw: &str) -> Result<crate::domain::review::ItemKind, OpError> {
    use crate::domain::review::ItemKind;
    let kind = match raw {
        "add_entity_type" => ItemKind::AddEntityType,
        "add_property" => ItemKind::AddProperty,
        "add_link_type" => ItemKind::AddLinkType,
        "remove_entity_type" => ItemKind::RemoveEntityType,
        "modify_entity_type" => ItemKind::ModifyEntityType,
        "add_concept_group" => ItemKind::AddConceptGroup,
        other => {
            return Err(OpError::bad_request(
                "DRAFT_UNKNOWN_KIND",
                format!(
                    "草稿里有认不出的条目类型 {other:?}（只认：{}）。",
                    KNOWN_KINDS.join(" / ")
                ),
                "草稿是 `ontok-builder propose` 的产物；如果是手改的，把 kind 改成上面五个之一。\
                 ⛔ 评审台不会把不认识的 kind 猜成「加属性」再放进定版。",
            ))
        }
    };
    Ok(kind)
}

/// 解析草稿 → 条目列表（纯函数：入参是字节，出参是结构；不碰磁盘）。
fn parse_draft(bytes: &[u8]) -> Result<(Value, Vec<DraftItem>), OpError> {
    let doc: Value = serde_json::from_slice(bytes).map_err(|e| {
        OpError::bad_request(
            "DRAFT_BAD_JSON",
            format!("草稿不是合法 JSON：{e}"),
            "草稿约定是 `ontok-builder propose` 产出的 `out/proposal.json`；\
             手改坏了就重新跑一次 propose（评审台不会改它）。",
        )
    })?;
    let items = doc.get("items").and_then(|v| v.as_array()).ok_or_else(|| {
        OpError::bad_request(
            "DRAFT_NO_ITEMS",
            "草稿里没有 `items` 数组 —— 这不是一份提案。",
            "提案的形状是 `{project, base_version?, extracted_by?, items:[{kind,target,payload}]}`；\
             重新跑 `ontok-builder propose` 会生成它。",
        )
    })?;
    let mut out = Vec::with_capacity(items.len());
    for (idx, raw) in items.iter().enumerate() {
        let obj = raw.as_object().ok_or_else(|| {
            OpError::bad_request(
                "DRAFT_BAD_ITEM",
                format!("草稿第 {} 条不是 JSON 对象。", idx + 1),
                "提案的 `items[]` 每条都要是一个对象（`{kind,target,payload}`）。",
            )
        })?;
        let kind = obj
            .get("kind")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                OpError::bad_request(
                    "DRAFT_BAD_ITEM",
                    format!("草稿第 {} 条缺 `kind`。", idx + 1),
                    "每条都要有 `kind`（add_entity_type / add_link_type / …）。",
                )
            })?
            .to_string();
        kind_of(&kind)?;
        let payload = obj.get("payload").cloned().unwrap_or(Value::Null);
        let target = obj
            .get("target")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .or_else(|| {
                payload
                    .get("key")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| format!("(第 {} 条)", idx + 1));
        let from_type = payload
            .get("from_type")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let to_type = payload
            .get("to_type")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        out.push(DraftItem {
            key: format!("{}:{}:{}", idx + 1, kind, target),
            seq: idx + 1,
            kind,
            target,
            from_type,
            to_type,
            payload,
            biz: Decision::Pending.as_str().to_string(),
            model: Decision::Pending.as_str().to_string(),
            reason: String::new(),
            decided_at: None,
            signed: false,
        });
    }
    Ok((doc, out))
}

/// 把 `decisions.json` 合并到条目上（**不丢未匹配的裁决** —— 它们留在文件里，
/// 以便草稿恢复原样后还能用；`stale` 负责提示「这份裁决是给旧草稿的」）。
fn merge_decisions(items: &mut [DraftItem], decisions: &DecisionsFile) {
    for item in items.iter_mut() {
        let Some(d) = decisions.items.get(&item.key) else {
            continue;
        };
        item.biz = decision_name(d.biz_decision()).to_string();
        item.model = decision_name(d.model_decision()).to_string();
        item.reason = d.reason.clone();
        item.decided_at = (!d.decided_at.is_empty()).then(|| d.decided_at.clone());
        item.signed = d.signed();
    }
}

/// 草稿 + 裁决 → `ItemView`（只收**两轨都 accepted** 的条目；拒绝/未签的不进定版）。
///
/// ⭐ doc-only 条目的 `reviewed_by` 由**这次人工评审**补上：规则 7（`domain::apply`）
/// 要求 doc 条目先有人签字才进快照，而草稿是只读的 —— 人的签字在 `decisions.json` 里。
/// 这里只在**内存中**给 payload 补 `reviewed_by`，`out/proposal.json` 一个字节都不动。
fn accepted_views(items: &[DraftItem]) -> Vec<ItemView> {
    items
        .iter()
        .filter(|it| it.signed && it.biz == "accepted" && it.model == "accepted")
        .map(|it| {
            let mut view = ItemView {
                id: it.key.clone(),
                // 已在 `parse_draft` 里验过 kind；这里的兜底臂不会命中。
                kind: kind_of(&it.kind).unwrap_or(crate::domain::review::ItemKind::AddProperty),
                target: it.target.clone(),
                from_type: it.from_type.clone(),
                to_type: it.to_type.clone(),
                biz_status: Decision::Accepted,
                model_status: Decision::Accepted,
                payload: it.payload.clone(),
            };
            if is_doc_origin(&view) && reviewed_by_of(&view).is_none() {
                if let Some(obj) = view.payload.as_object_mut() {
                    obj.insert("reviewed_by".to_string(), json!(SIGNED_BY));
                }
            }
            view
        })
        .collect()
}

/// 空基线：v1 的 base（`seq = 0`，四类集合全空；⛔ 不编一个假版本号）。
fn empty_snapshot() -> Snapshot {
    Snapshot::new(uuid::Uuid::nil(), 0)
}

fn baseline_types(snapshot: &Snapshot) -> std::collections::BTreeSet<String> {
    snapshot
        .entity_types
        .iter()
        .map(|e| e.key.clone())
        .collect()
}

fn extracted_by_of(doc: &Value) -> Option<ExtractedBy> {
    doc.get("extracted_by")
        .filter(|v| !v.is_null())
        .and_then(|v| serde_json::from_value(v.clone()).ok())
}

// ══════════════════════════════════════════════════════════════
// 磁盘写入（**唯一**一处 `fs::write`，便于出口登记对账）
// ══════════════════════════════════════════════════════════════

/// 原子落盘：先写同目录的 `.tmp`，再 `rename` 覆盖。
///
/// 为什么不是直接写目标：一份写了一半的 `decisions.json` / `snapshot.json`
/// 比没有更糟（下次读会报「不是合法 JSON」，而人以为数据还在）。`.tmp` + rename
/// 在同一个文件系统上是原子的。
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), OpError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            OpError::internal(format!(
                "建不出目录 {}：{e}。改法：确认工作目录可写（这是本机唯一的写入面）。",
                parent.display()
            ))
        })?;
    }
    let tmp = path.with_extension(format!(
        "{}tmp",
        path.extension()
            .and_then(|s| s.to_str())
            .map(|s| format!("{s}."))
            .unwrap_or_default()
    ));
    std::fs::write(&tmp, bytes).map_err(|e| {
        OpError::internal(format!(
            "写不出 {}：{e}。改法：确认工作目录可写、磁盘没满。",
            tmp.display()
        ))
    })?;
    std::fs::rename(&tmp, path).map_err(|e| {
        OpError::internal(format!(
            "落盘 {} 失败：{e}（临时文件 {} 还在，可以手工检查）。",
            path.display(),
            tmp.display()
        ))
    })
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

// ══════════════════════════════════════════════════════════════
// 路由
// ══════════════════════════════════════════════════════════════

/// 组装本地评审台的**唯一**一张路由表（6 + 1 条纯文件端点 + 页面 + health）。
pub fn build_router(svc: Arc<LocalConsole>) -> Router {
    Router::new()
        // ⭐ 页面壳：同一个 `console::http::index`（I22 的唯一写点，零数据、免口令）。
        .route("/", get(super::http::index))
        .route("/api/health", get(health))
        .route("/api/state", get(api_state))
        .route("/api/draft", get(api_draft))
        .route("/api/decide", post(api_decide))
        .route("/api/finalize", post(api_finalize))
        .route("/api/versions", get(api_versions))
        .route("/api/bundle", post(api_bundle))
        .route("/api/bundle/download", get(api_bundle_download))
        .fallback(not_found)
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .with_state(svc)
}

async fn not_found() -> Response {
    super::http::fail(crate::query::error::ApiError::with_code(
        StatusCode::NOT_FOUND,
        "NOT_FOUND",
        "没有这个路径 —— 本机评审台只做四件事：读草稿 · 记裁决 · 定版 · 导出包。",
        "可用：GET /（页面）、GET /api/health、GET /api/state、GET /api/draft、\
         POST /api/decide、POST /api/finalize、GET /api/versions、POST /api/bundle、\
         GET /api/bundle/download?seq=。\
         没有登录 / 项目 / 上传 / DSN / 入库 / 激活 / key / 审计端点（那些是 Studio 的事）。",
    ))
}

async fn health() -> Response {
    super::http::respond(
        StatusCode::OK,
        json!({
            "service": "ontok-console",
            "mode": "local-review",
            "version": env!("CARGO_PKG_VERSION"),
            "api": "v2",
        }),
    )
}

// ── ① GET /api/state ─────────────────────────────────────────

async fn api_state(State(svc): State<Arc<LocalConsole>>) -> Response {
    match state_json(&svc) {
        Ok(v) => super::http::respond(StatusCode::OK, v),
        Err(e) => super::http::fail_op(e),
    }
}

fn state_json(svc: &LocalConsole) -> Result<Value, OpError> {
    let draft = svc.read_draft()?;
    let decisions = svc.read_decisions()?;
    let latest = svc.latest_seq();

    let mut draft_block = json!({
        "exists": draft.is_some(),
        "items": 0,
        "signed": 0,
        "pending": 0,
        "stale": false,
    });
    if let Some(bytes) = &draft {
        let (_, mut items) = parse_draft(bytes)?;
        merge_decisions(&mut items, &decisions);
        let progress = Progress::of(&items);
        let sha = crate::query::audit::sha256_hex(bytes);
        draft_block = json!({
            "exists": true,
            "items": progress.total,
            "signed": progress.signed,
            "pending": progress.pending,
            "stale": is_stale(&decisions, &sha),
        });
    }

    let latest_version = latest
        .map(|seq| read_meta(svc, seq))
        .transpose()?
        .flatten()
        .map(|m| {
            json!({
                "seq": m.seq,
                "created_at": m.created_at,
                "items": m.items,
            })
        });

    let bundle = latest.and_then(|seq| {
        let path = svc.bundle_path(seq);
        let bytes = std::fs::metadata(&path).ok()?.len();
        Some(json!({
            "file": file_name_of(&path),
            "bytes": bytes,
        }))
    });

    Ok(json!({
        "dir": display_path(svc.dir()),
        "project": svc.project(),
        "draft": draft_block,
        "latest_version": latest_version,
        "bundle": bundle,
    }))
}

/// 给人看的路径：Windows 上 `canonicalize` 会给出 `\\?\D:\…`（扩展长度路径）——
/// 页面上显示它既长又像乱码。这里**只影响显示**（横幅 / 响应），实际读写仍用 `Path`。
pub fn display_path(p: &Path) -> String {
    let s = p.display().to_string();
    match s.strip_prefix(r"\\?\") {
        Some(rest) => rest.to_string(),
        None => s,
    }
}

fn is_stale(decisions: &DecisionsFile, draft_sha: &str) -> bool {
    !decisions.proposal_sha256.is_empty() && decisions.proposal_sha256 != draft_sha
}

fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default()
}

fn read_meta(svc: &LocalConsole, seq: i32) -> Result<Option<VersionMeta>, OpError> {
    let path = svc.meta_path(seq);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(OpError::internal(format!(
                "读不出定版元信息 {}：{e}。改法：删掉 .ontok/versions/v{seq} 后重新定版。",
                path.display()
            )))
        }
    };
    serde_json::from_slice(&bytes).map(Some).map_err(|e| {
        OpError::internal(format!(
            "定版元信息 {} 不是合法 JSON：{e}。改法：删掉 .ontok/versions/v{seq} 后重新定版。",
            path.display()
        ))
    })
}

// ── ② GET /api/draft ─────────────────────────────────────────

async fn api_draft(State(svc): State<Arc<LocalConsole>>) -> Response {
    match draft_json(&svc) {
        Ok(v) => super::http::respond(StatusCode::OK, v),
        Err(e) => super::http::fail_op(e),
    }
}

fn draft_json(svc: &LocalConsole) -> Result<Value, OpError> {
    let Some(bytes) = svc.read_draft()? else {
        return Err(OpError::not_found(
            "DRAFT_MISSING",
            format!(
                "工作目录里没有草稿：{} 不存在。",
                svc.draft_path().display()
            ),
            "先让 agent 产出草稿：`ontok-builder propose --schema out/schema.json \
             --names out/table_names.json --out out/proposal.json`（在同一个工作目录里跑）。\
             评审台只读它，不会替你生成草稿。",
        ));
    };
    let decisions = svc.read_decisions()?;
    let (doc, mut items) = parse_draft(&bytes)?;
    merge_decisions(&mut items, &decisions);
    let sha = crate::query::audit::sha256_hex(&bytes);
    let progress = Progress::of(&items);
    Ok(json!({
        "proposal": doc,
        "proposal_sha256": sha,
        "stale": is_stale(&decisions, &sha),
        "items": items,
        "progress": progress,
    }))
}

// ── ③ POST /api/decide ───────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DecideRequest {
    key: String,
    #[serde(default)]
    biz: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    reason: Option<String>,
}

async fn api_decide(
    State(svc): State<Arc<LocalConsole>>,
    body: Bytes,
) -> Response {
    match decide(&svc, &body) {
        Ok(v) => super::http::respond(StatusCode::OK, v),
        Err(e) => super::http::fail_op(e),
    }
}

fn decide(svc: &LocalConsole, body: &Bytes) -> Result<Value, OpError> {
    if body.is_empty() {
        return Err(OpError::bad_request(
            "EMPTY_BODY",
            "请求体是空的。",
            "body 形如 {\"key\":\"1:add_entity_type:客户\",\"biz\":\"accepted\",\"model\":\"accepted\"}。",
        ));
    }
    let req: DecideRequest = serde_json::from_slice(body).map_err(|e| {
        OpError::bad_request(
            "BAD_JSON",
            format!("请求体不是合法 JSON（或字段不对）：{e}"),
            "body 形如 {\"key\":\"…\",\"biz\":\"accepted|rejected|pending\",\
             \"model\":\"accepted|rejected|pending\",\"reason\":\"可选\"}。",
        )
    })?;
    let track_value = |name: &str, raw: &Option<String>| -> Result<Option<String>, OpError> {
        let Some(raw) = raw else { return Ok(None) };
        let t = raw.trim();
        match t {
            "accepted" | "rejected" | "pending" => Ok(Some(t.to_string())),
            other => Err(OpError::bad_request(
                "BAD_DECISION",
                format!("{name} 只认 accepted / rejected / pending，收到 {other:?}。"),
                "两轨都给了值（accepted/rejected）才算签完；`pending` = 撤回这一签。",
            )),
        }
    };
    let biz = track_value("biz", &req.biz)?;
    let model = track_value("model", &req.model)?;
    if biz.is_none() && model.is_none() && req.reason.is_none() {
        return Err(OpError::bad_request(
            "NOTHING_TO_DECIDE",
            "至少给 biz / model / reason 之一。",
            "只想写理由：给 `reason`；想签业务轨：给 `biz`。",
        ));
    }

    let Some(bytes) = svc.read_draft()? else {
        return Err(OpError::not_found(
            "DRAFT_MISSING",
            format!("工作目录里没有草稿：{} 不存在。", svc.draft_path().display()),
            "先跑 `ontok-builder propose … --out out/proposal.json` 生成草稿。",
        ));
    };
    let sha = crate::query::audit::sha256_hex(&bytes);
    let (_, mut items) = parse_draft(&bytes)?;
    let mut decisions = svc.read_decisions()?;
    merge_decisions(&mut items, &decisions);

    let exists = items.iter().any(|i| i.key == req.key);
    if !exists {
        let sample = items
            .first()
            .map(|i| i.key.clone())
            .unwrap_or_default();
        return Err(OpError::not_found(
            "ITEM_NOT_FOUND",
            format!("草稿里没有 key = {:?} 的条目（草稿现在 {} 条）。", req.key, items.len()),
            format!(
                "key 用 `GET /api/draft` 的 `items[].key`（形状 `seq:kind:target`）；\
                 第一条长这样：{sample:?}。草稿变了的话 key 也会变。"
            ),
        ));
    }

    let mut stored = decisions
        .items
        .get(&req.key)
        .cloned()
        .unwrap_or_default();
    if let Some(v) = &biz {
        stored.biz = v.clone();
    }
    if let Some(v) = &model {
        stored.model = v.clone();
    }
    if let Some(v) = &req.reason {
        stored.reason = v.clone();
    }
    stored.decided_at = now_rfc3339();
    decisions.items.insert(req.key.clone(), stored.clone());
    // 人在**当前**草稿上签字 ⇒ 把这份裁决的基准 sha 更新到现在（不做别的「自动修复」：
    // 没签过的条目仍然是 pending，页面会把它们列出来）。若草稿随后又变，`stale` 会再亮。
    decisions.proposal_sha256 = sha;
    save_decisions(svc, &decisions)?;

    if let Some(item) = items.iter_mut().find(|i| i.key == req.key) {
        item.biz = decision_name(decision_of(&stored.biz)).to_string();
        item.model = decision_name(decision_of(&stored.model)).to_string();
        item.reason = stored.reason.clone();
        item.decided_at = Some(stored.decided_at.clone());
        item.signed = stored.signed();
    }
    let progress = Progress::of(&items);
    let (biz_name, model_name, signed) = (
        decision_name(decision_of(&stored.biz)),
        decision_name(decision_of(&stored.model)),
        stored.signed(),
    );
    Ok(json!({
        "ok": true,
        "key": req.key,
        "biz": biz_name,
        "model": model_name,
        "signed": signed,
        "progress": progress,
        "stale": false,
    }))
}

fn save_decisions(svc: &LocalConsole, decisions: &DecisionsFile) -> Result<(), OpError> {
    let bytes = serde_json::to_vec_pretty(decisions).map_err(|e| {
        OpError::internal(format!("裁决序列化失败：{e}（这是内部错误，不是请求写法问题）。"))
    })?;
    write_atomic(&svc.decisions_path(), &bytes)
}

// ── ④ POST /api/finalize ─────────────────────────────────────

async fn api_finalize(State(svc): State<Arc<LocalConsole>>) -> Response {
    match finalize(&svc) {
        Ok(v) => super::http::respond(StatusCode::OK, v),
        Err(e) => super::http::fail_op(e),
    }
}

fn finalize(svc: &LocalConsole) -> Result<Value, OpError> {
    let Some(bytes) = svc.read_draft()? else {
        return Err(OpError::not_found(
            "DRAFT_MISSING",
            format!("工作目录里没有草稿：{} 不存在。", svc.draft_path().display()),
            "先跑 `ontok-builder propose … --out out/proposal.json` 生成草稿，再来定版。",
        ));
    };
    let sha = crate::query::audit::sha256_hex(&bytes);
    let decisions = svc.read_decisions()?;
    if is_stale(&decisions, &sha) {
        return Err(OpError::conflict(
            "DECISIONS_STALE",
            "草稿在裁决之后变过 —— 这份裁决可能对不上当前草稿，拒绝定版。",
            "页面会提示「草稿变了」。重新过一遍（任意一条重新签一次会把裁决基准更新到当前草稿），\
             或者把 out/proposal.json 恢复成裁决时的那一份。⛔ 评审台不会自动清空人的签字。",
        ));
    }
    let (doc, mut items) = parse_draft(&bytes)?;
    merge_decisions(&mut items, &decisions);
    let progress = Progress::of(&items);
    if progress.pending > 0 {
        return Err(OpError::bad_request(
            "DRAFT_NOT_READY",
            format!(
                "还有 {} 条没签完（共 {} 条，已签 {} 条）—— 签完才能定版。",
                progress.pending, progress.total, progress.signed
            ),
            "两轨（业务 / 建模）都给了 accepted 或 rejected 才算签完；\
             页面上的「全选接受 / 全选拒绝」可以一把签完。",
        ));
    }
    if !items.iter().any(|i| i.biz == "accepted" && i.model == "accepted") {
        return Err(OpError::bad_request(
            "NOTHING_ACCEPTED",
            "所有条目都被拒绝了 —— 没有可以定版的内容。",
            "至少接受一条（两轨都 accepted）再定版；或者这份草稿就不要了。",
        ));
    }

    let base_seq = svc.latest_seq();
    let base = match base_seq {
        Some(seq) => svc.load_snapshot(seq)?,
        None => empty_snapshot(),
    };
    let baseline = baseline_types(&base);
    let views = accepted_views(&items);

    // 先把「依赖没定版」的关系 / 概念组条目**点名**出来（apply 只给计数，给不出是谁）。
    let status = summarize(&views, &baseline);
    if !status.blocked.is_empty() {
        let names: Vec<String> = status
            .blocked
            .iter()
            .map(|v| {
                format!(
                    "{}（{}）",
                    v.target,
                    v.reason.clone().unwrap_or_else(|| "依赖未满足".to_string())
                )
            })
            .collect();
        return Err(OpError::conflict(
            "FINALIZE_BLOCKED",
            format!("{} 条条目的依赖没有定版：{}", names.len(), names.join("；")),
            "关系条目的两端、概念组的每个成员都必须是**这一版里接受了的**实体。\
             要么把缺的那个实体也接受，要么把这些条目也拒绝（拒绝的不进定版）。",
        ));
    }

    let snapshot = apply(ApplyInput {
        base: &base,
        proposal_extracted_by: extracted_by_of(&doc),
        items: &views,
        baseline_types: &baseline,
    })
    .map_err(apply_error)?;

    // ⚠️ 快照的**字节**只在这里产生一次：`snapshot.json` 与包里 `ontology/snapshot.json`
    // 必须逐字节相同（AC-006）。包那边是 `serde_json::to_vec(&Snapshot)`，这里用同一个写法。
    let snapshot_bytes = serde_json::to_vec(&snapshot).map_err(|e| {
        OpError::internal(format!("快照序列化失败：{e}（这是内部错误，不是请求写法问题）。"))
    })?;
    let seq = base_seq.unwrap_or(0) + 1;
    let snapshot_sha = crate::query::audit::sha256_hex(&snapshot_bytes);
    let created_at = now_rfc3339();
    let meta = VersionMeta {
        seq,
        created_at: created_at.clone(),
        items: views.len(),
        snapshot_sha256: snapshot_sha.clone(),
        proposal_sha256: sha,
    };
    let meta_bytes = serde_json::to_vec_pretty(&meta).map_err(|e| {
        OpError::internal(format!("定版元信息序列化失败：{e}（这是内部错误，不是请求写法问题）。"))
    })?;
    write_atomic(&svc.snapshot_path(seq), &snapshot_bytes)?;
    write_atomic(&svc.meta_path(seq), &meta_bytes)?;

    Ok(json!({
        "seq": seq,
        "created_at": created_at,
        "items": meta.items,
        "snapshot_sha256": snapshot_sha,
        "proposal_sha256": meta.proposal_sha256,
    }))
}

fn apply_error(e: ApplyError) -> OpError {
    let msg = e.to_string();
    match e {
        ApplyError::NotPassed { .. } | ApplyError::NothingApplicable => OpError::conflict(
            "FINALIZE_BLOCKED",
            format!("定版失败：{msg}"),
            "两轨都 accepted 的条目才能进定版；被拒绝的条目不会进（要改就再定一版）。",
        ),
        ApplyError::KeyConflict(_) | ApplyError::RemovingMissing(_) => OpError::conflict(
            "FINALIZE_CONFLICT",
            format!("定版失败：{msg}"),
            "草稿里有与基线冲突的 key（可能重复定版了同一份草稿）。\
             改草稿里的 key 后再定版；⛔ 评审台不会替你改草稿。",
        ),
        ApplyError::DanglingLink(_) => OpError::conflict(
            "FINALIZE_DANGLING",
            format!("定版失败：{msg}"),
            "关系条目的两端必须都在这一版里；把指向的实体也接受，或拒绝这条关系。",
        ),
        ApplyError::BadConfidence(_) | ApplyError::BadConceptGroup(_) => {
            OpError::bad_request("FINALIZE_BAD_ITEM", format!("定版失败：{msg}"), "这是草稿内容问题：按点名的那条修草稿后重新跑 propose。")
        }
        ApplyError::NotApplicable { .. } => OpError::conflict(
            "FINALIZE_INAPPLICABLE",
            format!("定版失败：{msg}"),
            "草稿把这条标成了 `applicable: false`（冲突条目）：人要在冲突两边挑一个，\
             或者把这条也拒绝；⛔ 不能「都接受」替人挑边。",
        ),
        ApplyError::DocBandStrong { .. } => OpError::bad_request(
            "FINALIZE_DOC_BAND",
            format!("定版失败：{msg}"),
            "doc 来源的条目 band 不许是 strong（规则 2）；把 band 改成 medium/low 后重新跑 propose。",
        ),
    }
}

// ── ⑤ GET /api/versions ──────────────────────────────────────

async fn api_versions(State(svc): State<Arc<LocalConsole>>) -> Response {
    match versions_json(&svc) {
        Ok(v) => super::http::respond(StatusCode::OK, v),
        Err(e) => super::http::fail_op(e),
    }
}

fn versions_json(svc: &LocalConsole) -> Result<Value, OpError> {
    let mut seqs = svc.version_seqs();
    seqs.sort_unstable_by(|a, b| b.cmp(a));
    let mut versions = Vec::new();
    for seq in seqs {
        if let Some(meta) = read_meta(svc, seq)? {
            versions.push(meta);
        }
    }
    Ok(json!({ "versions": versions }))
}

// ── ⑥ POST /api/bundle · ⑦ GET /api/bundle/download ───────────

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleRequest {
    #[serde(default)]
    seq: Option<i32>,
}

async fn api_bundle(State(svc): State<Arc<LocalConsole>>, body: Bytes) -> Response {
    let req = if body.is_empty() {
        BundleRequest::default()
    } else {
        match serde_json::from_slice::<BundleRequest>(&body) {
            Ok(r) => r,
            Err(e) => {
                return super::http::fail_op(OpError::bad_request(
                    "BAD_JSON",
                    format!("请求体不是合法 JSON（或字段不对）：{e}"),
                    "body 形如 {\"seq\":1}，或干脆空 body（= 最新定版）。",
                ))
            }
        }
    };
    match bundle(&svc, req.seq) {
        Ok(v) => super::http::respond(StatusCode::OK, v),
        Err(e) => super::http::fail_op(e),
    }
}

fn bundle(svc: &LocalConsole, wanted: Option<i32>) -> Result<Value, OpError> {
    let seq = match wanted {
        Some(seq) => {
            if seq < 1 {
                return Err(OpError::bad_request(
                    "BAD_SEQ",
                    format!("seq 必须 ≥ 1，收到 {seq}。"),
                    "定版号从 1 开始；不给 seq = 用最新定版。",
                ));
            }
            if !svc.version_seqs().contains(&seq) {
                return Err(OpError::not_found(
                    "VERSION_NOT_FOUND",
                    format!("没有第 {seq} 版定版。"),
                    "先 `GET /api/versions` 看有哪些；导出的是**定版**，不是草稿。",
                ));
            }
            seq
        }
        None => svc.latest_seq().ok_or_else(|| {
            OpError::bad_request(
                "NO_VERSION",
                "还没有任何定版 —— 导出的是定版，草稿不能直接打包。",
                "先把草稿签完（两轨都给值）再点「定版」，然后再导出。",
            )
        })?,
    };

    let snapshot = svc.load_snapshot(seq)?;
    let built_at = now_rfc3339();
    let bytes = assemble(&PackInput::new(
        svc.project().to_string(),
        seq,
        built_at,
        snapshot,
    ))
    .map_err(|e| {
        OpError::bad_request(
            "BUNDLE_PACK_FAILED",
            format!("打包失败：{e}"),
            "这是包格式的要求（项目名 / 版本号 / 时间 / 快照）；\
             目录名会 sanitize 成合法的项目名，若仍失败把这条错误原文报给维护者。",
        )
    })?;
    let path = svc.bundle_path(seq);
    write_atomic(&path, &bytes)?;
    let sha = crate::query::audit::sha256_hex(&bytes);
    Ok(json!({
        "seq": seq,
        "file": file_name_of(&path),
        "path": path.display().to_string(),
        "bytes": bytes.len(),
        "sha256": sha,
    }))
}

async fn api_bundle_download(
    State(svc): State<Arc<LocalConsole>>,
    Query(q): Query<BTreeMap<String, String>>,
) -> Response {
    let mut seq: Option<i32> = None;
    for (k, v) in &q {
        match k.as_str() {
            "seq" => {
                match v.trim().parse::<i32>() {
                    Ok(n) if n >= 1 => seq = Some(n),
                    _ => {
                        return super::http::fail_op(OpError::bad_request(
                            "BAD_SEQ",
                            format!("seq 要写一个 ≥1 的整数，收到 {v:?}。"),
                            "例：`GET /api/bundle/download?seq=1`；不给 = 最新定版。",
                        ))
                    }
                }
            }
            other => {
                return super::http::fail_op(OpError::bad_request(
                    "BAD_QUERY",
                    format!("不认识的查询参数 {other:?}。"),
                    "只接受 `seq`（定版号，省略 = 最新）。",
                ))
            }
        }
    }
    let seq = match seq.or_else(|| svc.latest_seq()) {
        Some(seq) => seq,
        None => {
            return super::http::fail_op(OpError::bad_request(
                "NO_VERSION",
                "还没有任何定版 —— 没有可下载的包。",
                "先定版，再 `POST /api/bundle` 生成包。",
            ))
        }
    };
    let path = svc.bundle_path(seq);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return super::http::fail_op(OpError::not_found(
                "BUNDLE_MISSING",
                format!("第 {seq} 版的包还没生成：{} 不存在。", path.display()),
                "先 `POST /api/bundle`（body `{\"seq\":…}` 或空 = 最新）打一次包，再下载。",
            ))
        }
        Err(e) => {
            return super::http::fail_op(OpError::internal(format!(
                "读不出包 {}：{e}。改法：确认文件可读；要重打就再 POST /api/bundle。",
                path.display()
            )))
        }
    };
    let disposition = HeaderValue::from_str(&format!(
        "attachment; filename=\"{}\"",
        file_name_of(&path)
    ))
    .unwrap_or_else(|_| HeaderValue::from_static("attachment"));
    (
        StatusCode::OK,
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/octet-stream"),
            ),
            (header::CONTENT_DISPOSITION, disposition),
        ],
        bytes,
    )
        .into_response()
}

// ══════════════════════════════════════════════════════════════
// 判据（纯函数：不连库、不绑 socket）
// ══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "ontok-req162-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&d).expect("建临时目录");
        d
    }

    const DRAFT: &str = r#"{
      "extracted_by": {"agent":"demo","version":"0.1.0","round":1,"failed_groups":[]},
      "items": [
        {"kind":"add_entity_type","target":"客户","payload":{"key":"customer","display_name":"客户"}},
        {"kind":"add_entity_type","target":"订单","payload":{"key":"order","display_name":"订单"}},
        {"kind":"add_link_type","target":"客户→订单","payload":{"key":"c_of_o","from_type":"customer","to_type":"order"}}
      ]
    }"#;

    #[test]
    fn project_name_is_deterministic_and_legal() {
        assert_eq!(project_name_of(Path::new("C:/x/anspire_real")), "anspire_real");
        assert_eq!(project_name_of(Path::new("C:/x/My Project")), "my_project");
        assert_eq!(project_name_of(Path::new("C:/x/本体")), "p__");
        assert_eq!(project_name_of(Path::new("C:/x/a")), "a_");
        assert!(crate::bundle::pack::is_valid_project_name(&project_name_of(
            Path::new("C:/x/feature-REQ-162")
        )));
    }

    #[test]
    fn parse_draft_rejects_unknown_kind() {
        let bad = br#"{"items":[{"kind":"add_nonsense","target":"x"}]}"#;
        let e = parse_draft(bad).unwrap_err();
        assert_eq!(e.code, "DRAFT_UNKNOWN_KIND");
    }

    #[test]
    fn draft_bytes_stay_read_only_through_a_full_cycle() {
        let dir = temp_dir("cycle");
        std::fs::create_dir_all(dir.join("out")).unwrap();
        let draft = dir.join(DRAFT_REL);
        std::fs::write(&draft, DRAFT).unwrap();
        let before = std::fs::read(&draft).unwrap();
        let svc = LocalConsole::open(&dir).unwrap();

        let doc = draft_json(&svc).unwrap();
        assert_eq!(doc["progress"]["total"], 3);
        assert_eq!(doc["progress"]["pending"], 3);
        let keys: Vec<String> = doc["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["key"].as_str().unwrap().to_string())
            .collect();

        // 全拒绝：一条都不进定版 ⇒ finalize 报 NOTHING_ACCEPTED。
        for key in &keys {
            let body = Bytes::from(
                json!({"key": key, "biz":"rejected", "model":"rejected"}).to_string(),
            );
            decide(&svc, &body).expect("写裁决");
        }
        let e = finalize(&svc).unwrap_err();
        assert_eq!(e.code, "NOTHING_ACCEPTED");

        // 全接受 ⇒ 定版 v1（关系条目两端都在，能过）。
        for key in &keys {
            let body = Bytes::from(
                json!({"key": key, "biz":"accepted", "model":"accepted"}).to_string(),
            );
            decide(&svc, &body).expect("写裁决");
        }
        let v = finalize(&svc).unwrap();
        assert_eq!(v["seq"], 1);
        assert_eq!(v["items"], 3);
        assert!(svc.snapshot_path(1).is_file(), "定版快照要落盘");

        // 导出包 → 自检 ok，且包内快照与 snapshot.json 逐字节相同。
        let b = bundle(&svc, None).unwrap();
        let bytes = std::fs::read(svc.bundle_path(1)).unwrap();
        assert_eq!(b["bytes"], bytes.len());
        let report = crate::bundle::verify(&bytes);
        assert!(report.ok, "包必须通过自检：{:?}", report.problems);
        let parsed = crate::bundle::zip::read(&bytes).expect("读包");
        let entry = parsed
            .iter()
            .find(|e| e.path == crate::bundle::SNAPSHOT_PATH)
            .expect("包里要有快照");
        assert_eq!(
            entry.data,
            std::fs::read(svc.snapshot_path(1)).unwrap(),
            "包里快照必须与定版 snapshot.json 逐字节相同（AC-006）"
        );

        // 草稿一个字节都没变（AC-003）。
        assert_eq!(std::fs::read(&draft).unwrap(), before, "评审台不许改草稿");
    }

    #[test]
    fn stale_decisions_are_detected_and_block_finalize() {
        let dir = temp_dir("stale");
        std::fs::create_dir_all(dir.join("out")).unwrap();
        std::fs::write(dir.join(DRAFT_REL), DRAFT).unwrap();
        let svc = LocalConsole::open(&dir).unwrap();
        let sha = crate::query::audit::sha256_hex(DRAFT.as_bytes());
        let mut d = DecisionsFile {
            proposal_sha256: "0".repeat(64),
            ..DecisionsFile::default()
        };
        for item in draft_json(&svc).unwrap()["items"].as_array().unwrap() {
            d.items.insert(
                item["key"].as_str().unwrap().to_string(),
                StoredDecision {
                    biz: "accepted".into(),
                    model: "accepted".into(),
                    ..StoredDecision::default()
                },
            );
        }
        save_decisions(&svc, &d).unwrap();
        let state = state_json(&svc).unwrap();
        assert_eq!(state["draft"]["stale"], true, "sha 不符必须 stale");
        assert_eq!(finalize(&svc).unwrap_err().code, "DECISIONS_STALE");

        // 改回真 sha ⇒ 不再 stale，可以定版。
        d.proposal_sha256 = sha;
        save_decisions(&svc, &d).unwrap();
        assert_eq!(state_json(&svc).unwrap()["draft"]["stale"], false);
        assert_eq!(finalize(&svc).unwrap()["seq"], 1);
    }

    #[test]
    fn finalize_is_gated_on_pending_items() {
        let dir = temp_dir("gate");
        std::fs::create_dir_all(dir.join("out")).unwrap();
        std::fs::write(dir.join(DRAFT_REL), DRAFT).unwrap();
        let svc = LocalConsole::open(&dir).unwrap();
        // 只想签一条：别的仍是 pending。
        let first = draft_json(&svc).unwrap()["items"][0]["key"]
            .as_str()
            .unwrap()
            .to_string();
        decide(
            &svc,
            &Bytes::from(json!({"key": first, "biz":"accepted","model":"accepted"}).to_string()),
        )
        .unwrap();
        let e = finalize(&svc).unwrap_err();
        assert_eq!(e.code, "DRAFT_NOT_READY");
        assert!(e.message.contains("2"), "要点名还差几条：{}", e.message);
        assert!(!svc.snapshot_path(1).exists(), "门禁拒绝时不许落任何定版");
    }

    #[test]
    fn bundle_before_finalize_is_rejected() {
        let dir = temp_dir("nobundle");
        std::fs::create_dir_all(dir.join("out")).unwrap();
        std::fs::write(dir.join(DRAFT_REL), DRAFT).unwrap();
        let svc = LocalConsole::open(&dir).unwrap();
        assert_eq!(bundle(&svc, None).unwrap_err().code, "NO_VERSION");
    }
}

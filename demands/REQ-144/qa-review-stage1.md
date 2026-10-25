# REQ-144 阶段 1 · Pre-merge QA 审核

> **审核对象**：`feature-REQ-144b` · 分支 `feature/REQ-144-stage1` · 提交 `1d48791`
> **基线**：`develop` `6f4987d`（自建，`git archive 6f4987d`，不从 Dev 处取）
> **依据**：`demand.md` §六（阶段 1 规格）· `acceptance.md` §一 9 条 AC + §四 阶段 1 裁定 R-A~R-F
> **审核方式**：**不服单测**。签名用 Python `hashlib/hmac` 独立重算；密码学原语用 5+8 组向量对拍
> 被测源码（`#[path]` 直接编译**原文**，不是副本）；网络行为用**自己的**假端点 + 裸 TCP 监听器；
> 零回归用**自建基线二进制**逐字节对拍。
> **证据目录**：`feature-REQ-144b/.local/qa-req144s1/`（⛔ 不在 `%TEMP%`，见 §11）
> **审核时间**：2026-10-25 · QA agent `qa-req144-stage1`

---

## 1 · 结论

**PASS 带非阻断**（可合并）。

| # | P0 项 | 结果 |
| --- | --- | --- |
| 1 | 签名独立重算（官方向量 `cvm`/`1551113065`） | ✅ **逐字节相同**，且我比它多验了全链（见 §2 F1） |
| 2 | 手写 SHA-256 / HMAC-SHA256（5 随机 + 3 边界） | ✅ 81 个字段对拍，**0 处不同** |
| 3 | 不给 `--allow-network` 确实不联网 | ✅ 裸监听器 **accept 0 / 0 字节**；不可解析域名 0 秒差异 |
| 4 | 零回归（8 case vs 基线二进制） | ✅ **7/8 逐字节相同**，1 个是已裁定的 R-C 文案变更（码/退出码未变） |
| 5 | 五类错误分类 + 变异 | ✅ 6 场景全对（含重试次数）；变异 **必红** |
| 6 | 凭据不外泄 | ✅ 14 份产物 **0 命中**；`Debug` 手写 `<redacted>` |
| 7 | 零新增依赖 | ✅ `git diff develop..HEAD -- Cargo.toml Cargo.lock` **为空** |
| 8 | 阶段 0 九条 AC 仍绿 | ✅ 9/9 绿，连带 145 live 9/9、四红线 2/2、source_enumeration 10/10 |
| 9 | 不退化（全量逐 target） | ✅ **0 新增失败**（基线自己实测：7 红；3 条「只在 feature 红」已 A/B 复跑证伪为共用库前置缺失，详见 §10） |

**非阻断发现 3 条**（不拦合并，建议下一轮做）：

| # | 级别 | 发现 |
| --- | --- | --- |
| F1 | 建议 | `tc3.rs` 测试注释说「SecretKey 打码 ⇒ 从 SecretKey 走派生链无法与文档对拍」——**这个论断不成立**。文档里的打码 SecretKey 就是字面 32 个 `*`，用它能把**整条链**（含第一跳 `HMAC("TC3"+SK, Date)`）跑到文档公布的 `Signature`。它现在从 `SecretSigning` 接上，少验了第一跳。我已独立验证全链（§2）。 |
| F2 | 流程（R-D） | **Dev 没落实 R-D**：`feature-REQ-144b/.local/req144-stage1/` **不存在**，证据仍只在 `%TEMP%\ontok-req144-stage1\`；仓库里也搜不到「原临时路径作废」那一行。详见 §11。 |
| F3 | 覆盖 | 三个错误码**没有任何测试覆盖**：`UPSTREAM_UNREACHABLE`、`API_ERROR`（4xx/5xx 分支）、`AUTH_MODE_UNSUPPORTED`。我用真实 CLI 跑了 DNS 失败 → `UPSTREAM_UNREACHABLE` 正常；另发现「连接被拒」被归到 `UPSTREAM_TIMEOUT`（kind=Connect）而不是 `UNREACHABLE` —— 可行动，但码名有点误导。详见 §6.3。 |

---

## 2 · 独立重算的签名（P0-1）

### 2.1 我用的方法与官方文档原文

官方文档 <https://cloud.tencent.com/document/api/213/30654>（《签名方法 v3》），2026-10-25 抓取，
下面这些值**逐字**来自文档 HTML（不是从被测代码里抄的）：

| 文档公布项 | 值 |
| --- | --- |
| `HashedRequestPayload` | `35e9c5b0e3ae67532d3c9f17ead6c90222632e5b1ff7f6e89887f1398934f064` |
| `HashedCanonicalRequest` | `7019a55be8395899b900fb5564e4200d984910f34794a27cb3fb7d10ff6a1e84` |
| `SecretDate` | `da98fb70dcf6b112dc21038d1eeeb3a95c74b4dcb12c1131f864f6066bd02be0` |
| `SecretService` | `8d70cbefb03939f929db64d32dc2ba89b1095620119fe3e050e2b18c5bd2752f` |
| `SecretSigning` | `b596b923aad85185e2d1f6659d2a062e0a86731226e021e61bfe06f7ed05f5af` |
| `Signature` | `10b1a37a7301a02ca19a647ad722d5e43b4b3cff309d421d85b46093f6ab6c4f` |
| `Authorization` | `TC3-HMAC-SHA256 Credential=AKID********************************/2019-02-25/cvm/tc3_request, SignedHeaders=content-type;host;x-tc-action, Signature=10b1a37a…6ab6c4f` |

我的实现：`.local/qa-req144s1/tc3_ref.py`（纯 `hashlib` / `hmac`，60 行，按文档四步手写，
**不 import 被测代码、不抄它的测试常量**）。

### 2.2 独立算出的 `Signature` vs 它的期望值

| 量 | 我（Python）算出的 | 它测试里的期望值（`src/access/tc3.rs`） | 相同？ |
| --- | --- | --- | --- |
| `Date` | `2019-02-25` | `2019-02-25`（:478） | ✅ |
| `HashedRequestPayload` | `35e9c5b0e3ae67532d3c9f17ead6c90222632e5b1ff7f6e89887f1398934f064` | 同（:439） | ✅ |
| `CanonicalRequest` | `POST\n/\n\ncontent-type:application/json; charset=utf-8\nhost:cvm.tencentcloudapi.com\nx-tc-action:describeinstances\n\ncontent-type;host;x-tc-action\n35e9c5b0…` | 同（:492） | ✅ |
| `HashedCanonicalRequest` | `7019a55be8395899b900fb5564e4200d984910f34794a27cb3fb7d10ff6a1e84` | 同（:442） | ✅ |
| `StringToSign` | `TC3-HMAC-SHA256\n1551113065\n2019-02-25/cvm/tc3_request\n7019a55be…` | 同（:496） | ✅ |
| **`Signature`** | **`10b1a37a7301a02ca19a647ad722d5e43b4b3cff309d421d85b46093f6ab6c4f`** | 同（:451） | ✅ |
| `Authorization` | `TC3-HMAC-SHA256 Credential=AKID********************************/2019-02-25/cvm/tc3_request, SignedHeaders=content-type;host;x-tc-action, Signature=10b1a37a…` | 同（:453） | ✅ |

### 2.3 ⭐ 我比它的测试多走一步：**全链**（F1）

文档把 SecretKey 打成 `********************************`（32 个 `*`）。我注意到：**这个打码串就是被计算的密钥本身**
（文档生成时用的就是它）。用它从第一跳开始算，**三把派生密钥与最终签名全部命中文档公布值**：

```
SecretDate    = da98fb70dcf6b112dc21038d1eeeb3a95c74b4dcb12c1131f864f6066bd02be0   ← 文档公布值
SecretService = 8d70cbefb03939f929db64d32dc2ba89b1095620119fe3e050e2b18c5bd2752f   ← 文档公布值
SecretSigning = b596b923aad85185e2d1f6659d2a062e0a86731226e021e61bfe06f7ed05f5af   ← 文档公布值
Signature     = 10b1a37a7301a02ca19a647ad722d5e43b4b3cff309d421d85b46093f6ab6c4f   ← 文档公布值
Authorization = 与文档 §4 那一整串**逐字节相同**
```

⇒ 它的 `official_vector_1_cvm_1551113065_is_byte_identical` 只从 `SecretSigning` 接上，
**第一跳 `HMAC("TC3"+SK, Date)` 没被官方向量钉住**（那条链只靠 `official_vector_1_derived_key_chain_replays`
在 `SecretDate` 之后自洽）。我实测：**第一跳是对的**，所以这不是缺陷，只是判据比可能的弱。
（建议下一轮把 `doc_input()` 的 `secret_key` 改成 `"*"*32` 直接走 `sign()`，并删掉「无法对拍」那句注释。）

### 2.4 第二组向量（`monitor` / `1620000000`）

它注释如实写了「不是文档公布值，是文档同一份 Python 实现复算」。我用**自己的**实现独立复算：

| 量 | 我算出 | 它的期望（:576-596） |
| --- | --- | --- |
| `HashedPayload` | `73dbfc14f5c9c9661954ef0061726727018795229097c3fdfa9bc0e4d2aaca1b` | 同 ✅ |
| `HashedCanonicalRequest` | `c2592b8f255720debe999f1f415c9e3175496a43696159c3d9c952a51bdd51e1` | 同 ✅ |
| `SecretSigning` | `9a7de508fd240de15561f552c9a3aa786895ea714475d4710667b7775c1889ff` | 同 ✅ |
| `Signature` | `694b814cbf240e7ec8c361c69c6fb049fc82b8dce207a6a07486f5f671163905` | 同 ✅ |

⇒ 它的「如实登记来源」是真的：第二组的期望值与独立实现**一致**，不是"自己证明自己"。

### 2.5 签名在**线上**也被独立复算过（见 §6）

假端点收到每个真实请求后，用同一个 `tc3_ref.py` 按收到的 `Host / X-TC-Action / X-TC-Timestamp /
Content-Type / body` 重算 `Authorization`，与客户端发来的头**逐字节**比较 ⇒ 5 个场景、9 个请求
**全部 `True`**。这证明的不是"签名函数对"，而是"**签的就是发的那一份 body / 那一套头**"。

---

## 3 · 手写密码学原语对拍（P0-2）

被测代码**没引** `sha2`/`hmac`，手写了 SHA-256（`tc3.rs:199-299`）与 HMAC-SHA256（`:306-326`）。
对拍方式：`.local/qa-req144s1/harness` 用 `#[path]` 直接编译**原文** `src/access/tc3.rs`
（不是复制品，改原文它立即跟着变），只补一个最小 `AccessError` 桩；
期望值全部来自 `.local/qa-req144s1/gen_vectors.py`（`hashlib` / `hmac`，固定种子可复现）。

### 3.1 SHA-256：5 组随机 + 6 个填充边界（含要求的 55/56/64）

| 向量 | 输入长度 | 期望（Python） | 被测（Rust） | 相同 |
| --- | --- | --- | --- | --- |
| random-1 | 1 | `7a4a4b50f5121ed5310ece45a7eeb7af5545af63ee2ae52add4f37788f075b1d` | 同 | ✅ |
| random-2 | 37 | `831dba148d7dda1083db32db845ea403e8f223ed5d1f9260318ad21a93d2c9f0` | 同 | ✅ |
| random-3 | 0 | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` | 同 | ✅ |
| random-4 | 63 | `0c10462f4a3a2ee700a934c3275380bd48c9062972b54e5ef9dedfa9ca81bcb9` | 同 | ✅ |
| random-5 | 65 | `3eddc8e9dd93fd00ba97264f30304cfde0a03dd0a363a43f42bd7a6fbbd1a73a` | 同 | ✅ |
| **空输入** | 0 | `e3b0c442…b855` | 同 | ✅ |
| **正好 55** | 55 | `9f4390f8d30c2dd92ec9f095b65e2b9ae9b0a925a5258e241c9f1e910f734318` | 同 | ✅ |
| **正好 56** | 56 | `b35439a4ac6f0948b6d6f9e3c6af0f5f590ce20f1bde7090ef7970686ec6738a` | 同 | ✅ |
| **正好 64** | 64 | `ffe054fe7ae0cb6dc65c3af9b61d5209f439851db43d0ba5997337df154668eb` | 同 | ✅ |
| 119（64+55） | 119 | `31eba51c313a5c08226adf18d4a359cfdfd8d2e816b13f4af952f7ea6584dcfb` | 同 | ✅ |
| 120（64+56） | 120 | `2f3d335432c70b580af0e8e1b3674a7c020d683aa5f73aaaedfdc55af904c21c` | 同 | ✅ |

### 3.2 HMAC-SHA256：5 组随机 + 3 个 key 边界（含 key > 64 先哈希）

| 向量 | key / msg 长度 | 期望（Python hmac） | 被测（Rust） | 相同 |
| --- | --- | --- | --- | --- |
| random-1 | 16 / 64 | `be900aebe121643fae05bc9b19ac43626fc82d18a643595c66049d79fd066c56` | 同 | ✅ |
| random-2 | 64 / 55 | `1af2ddce5784fa5e2ca64d93a04e275011dc32cd505d07fa2a50aacf5e64f10b` | 同 | ✅ |
| random-3 | 65 / 56 | `c4505d8fd9806a93a14024da1e213ca0198c11f8d98e2845f5dd266e39934eb7` | 同 | ✅ |
| random-4 | 32 / 55 | `98f779ff87d07d2de1573789fcf1714252f4bbd0846cc85681080dbe8e3e95f0` | 同 | ✅ |
| random-5 | 32 / 64 | `a2188d96e0a167271d88bc11cb84cec62ca3e28999f45acea526f1c036c6da78` | 同 | ✅ |
| **key = 0**（空 key） | 0 / 12 | `e7021e77d246c02a6cc6ceea8bd62442c82db65c2954a5da96b54685c05597e9` | 同 | ✅ |
| **key = 64**（不哈希） | 64 / 12 | `9da41635d4442933e878147e7b6ddf88a5401c18aa8e78686c7a7b696d0985a4` | 同 | ✅ |
| **key = 65**（先哈希） | 65 / 12 | `c965336253f2a0a377e94e899e075d85f56dba0422ffa09fa362df94fa2385b7` | 同 | ✅ |
| key = 131 | 131 / 12 | `984567aa82a01a6ef82c35cea1c63f7e175e4f9063a510ed45a5b7f2884f645b` | 同 | ✅ |
| 空消息 | 20 / 0 | `98588483fbb4a9279759e99cfe75c7dc56fe3a29983900aade97422e57b642f2` | 同 | ✅ |

### 3.3 合计

**81 个字段对拍（11 个 SHA + 10 个 HMAC + 5 个签名向量 ×12 个中间量）⇒ 不一致 0 处。**

```
合计 81 个字段对拍；不一致 0 处
全部一致 = ALL MATCH
```

日志：`.local/qa-req144s1/logs/primitive-compare.log`（含每组的 python / rust 双列原文）。
边界结论：**55/56 交界与 64 的整块填充都对**（`tc3.rs:282` 的 `if rest.len() < 56 { 1 } else { 2 }` 正确）；
**key > 64 先 SHA-256 也对**（`tc3.rs:308`）。

---

## 4 · 未联网证明（P0-3）

方法：本机起一个**裸 TCP 监听器**（只 `accept` + 记字节，不实现任何协议），把契约 `base_url`
指到它；进程只要碰了网络，accept 计数立刻 > 0。再叠加「不可解析域名」做第二重证明。
脚本：`.local/qa-req144s1/no_network.py`，日志：`logs/no-network.log`。

| 场景 | 退出码 | 监听器 accept | 收到字节 | 判定 |
| --- | --- | --- | --- | --- |
| `fetch --dry-run-fixture`（base_url 指着监听器） | 0 | **0** | **0** | ✅ 没联网 |
| `fetch` 两个开关都不给 | 1（`DRY_RUN_REQUIRED`） | **0** | **0** | ✅ 没联网 |
| `validate` | 0 | **0** | **0** | ✅ 没联网 |
| `publish` | 0 | **0** | **0** | ✅ 没联网 |
| `--dry-run-fixture` + `http://no-such-host-qa144.invalid` | 0 | — | — | ✅ 输出与正常域名**逐字节相同**，耗时 0.05s vs 0.05s（差 0.00s） |

**默认通路联网次数合计 = 0。**

代码层面为什么不可能联网（静态）：

```
ureq::                      → 只出现在 src/access/net.rs（3 处，:273 / :290 / :369）
net::fetch_network(         → 只有 1 个调用点 src/cli.rs:3080，
                              位于 `if dry_run_fixture { … return }` 之后、`--allow-network` 分支内
cli.rs:2988                 → if !dry_run_fixture && !allow_network { bail!(DRY_RUN_REQUIRED) }
clap                        → --dry-run-fixture 与 --allow-network `conflicts_with`（实测同时给 ⇒ 退出码 2）
```

另外实测：同时给两个开关 ⇒ `error: the argument '--dry-run-fixture' cannot be used with '--allow-network'`，**exit 2**。

---

## 5 · 零回归 8 case（P0-4 / R1 / R6）

**基线二进制是我自己起的**：`git archive 6f4987d` → `.local/qa-req144s1/base-6f4987d/`，
独立 `CARGO_TARGET_DIR=.local/cargo-qa144s1-base`。两个二进制：

| 二进制 | sha256 |
| --- | --- |
| feature `1d48791`（`.local/cargo-qa144s1/debug/ontok.exe`） | `C71BA83A291962847B2BB3BB543EE775842E1DA3936981B17DD98B4A3D50D2CC` |
| 基线 `6f4987d`（`.local/cargo-qa144s1-base/debug/ontok.exe`） | `0C1A032C94FB30D7D50F7FAC2B3B168A57B50E884F7D7339CA444A528F58CC59` |

（两个 target dir 在跑完全部对拍后已清理，见 §12；源码未变，`cargo build --locked` 可原样重建。）

归一化白名单**只有** `freshness.fetched_at`（本机时钟）。它出现在两处：JSON 的
`meta.freshness.fetched_at` 与 stderr 的 `· 时点：<RFC3339>` 回显（同一个值的回显）⇒ 一并归一化。
**其余逐字节比较**（stdout / stderr / publish 产物 / 退出码）。

| # | case | 退出码 f/base | sha(stdout) f / base | sha(stderr) f / base | 判定 |
| --- | --- | --- | --- | --- | --- |
| 1 | fetch `--filter {"status":"STOPPED"}` `--limit 50`（本地过滤） | 0 / 0 | `05b036cccf17fb0e` / 同 | `d23b6e0d42f8dfee` / 同 | ✅ 逐字节相同（17611 B） |
| 2 | fetch `--entity alarm` `--limit 10`（无过滤） | 0 / 0 | `80c3274b5c0bc275` / 同 | `09a66130bd620587` / 同 | ✅ 逐字节相同（1438 B） |
| 3 | fetch `--filter {"zone":"ap-guangzhou-2"}` `--limit 200`（下推+截断） | 0 / 0 | `ab4134943bca19d2` / 同 | `1a6248e8a7ab27e7` / 同 | ✅ 逐字节相同（42533 B） |
| 4 | `validate`（好契约） | 0 / 0 | `4d50e19a5249966c` / 同 | `dd2a98cf89c55649` / 同 | ✅ 逐字节相同（1601 B） |
| 5 | `validate`（R4 重叠的坏契约） | 1 / 1 | `95c21b784bcff307` / 同 | `57995d3800dfe63f` / 同 | ✅ 逐字节相同（2750 B） |
| 6 | `publish --out <同一路径>` | 0 / 0 | （空） | `20c8519aecfdefe1` / 同 | ✅ 发布产物 `324624a5da5e9660` 两侧相同（7093 B） |
| 7 | `fetch` 不给任何开关 | 1 / 1 | （空） | `f5c5e581fe1e09c3` / `ae5a6c90681c17e6` | ⚠️ **仅文案不同**（见下），码/退出码相同 |
| 8 | `fetch --filter {"bogus":"x"}`（未知过滤名） | 1 / 1 | （空） | `1be384187c4023a0` / 同 | ✅ 逐字节相同（313 B） |

**7/8 逐字节相同**。第 7 个的差异**就是 R-C 已裁定的那句文案变更**（"本程序里没有那条路径" 变成假话 ⇒ 必须改）：

```diff
- ✗ DRY_RUN_REQUIRED · 阶段 0 **不发任何网络请求**：请显式加 `--dry-run-fixture` 用夹具跑。
- 真实调用（TC3 签名 / 凭据 / 限流退避）是阶段 1 的事，本程序里**没有**那条路径。
+ ✗ DRY_RUN_REQUIRED · 请显式选一条通路：`--dry-run-fixture`（读本地夹具，⛔ 不发网络请求）或 `--allow-network`（真调用：TC3 签名 / 凭据 / 限流退避）。
+ ⛔ 两个都不给时本程序既不联网，也不静默降级成「读 fixture 冒充真数据」。
```

**码（`DRY_RUN_REQUIRED`）与退出码（1）都没变**，与 R-C 裁定一致 ⇒ 不算回归。

另外我确认了 `meta.network` 这一栏**在 fixture 通路不出现**（`Meta.network: Option<…>` +
`skip_serializing_if = "Option::is_none"`，`fetch.rs:190-196`）：case 1/2/3 的 stdout 逐字节相同就是硬证据。

---

## 6 · 五类错误分类 + 变异（P0-5）

**用我自己的假端点**（`.local/qa-req144s1/error_classes.py`，进程内 `ThreadingHTTPServer`，
⛔ 没用它的 `fake_server.py`）。占位凭据：`AKIDqatest` / `secretqatest`（⛔ 不是真凭据）。
每个真实请求都由**独立实现**复算 `Authorization` 并比对（§2.5）。

| 场景 | 伪端点脚本 | 退出码 | 服务端收到 | code | 重试 | 可行动 hint | 签名独立复算 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| A · 200 两页 | `[200,200]` | 0 | **2**（1 页 1 次） | —（成功） | 0 | — | ✅ 2/2 |
| B · 429 后成功 | `[429,200]` | 0 | **2** | —（重试后成功） | **1** | — | ✅ 2/2 |
| C · 401 被拒 | `[401]` | 1 | **1**（⛔ 不重试） | `AUTH_REJECTED` | **0** | "不重试" / "5 分钟" | ✅ 1/1 |
| D · 200 缺字段 | `[200 无 InstanceSet]` | 1 | **1**（⛔ 不重试） | `FIELD_PATH_MISSING` | **0** | "契约写错了" | ✅ 1/1 |
| E · 超时 | `[hang×4]` | 1 | **3**（上限 3 次） | `UPSTREAM_TIMEOUT` | **2** | "不返回空结果" / 已尝试 3 次 | ✅ 3/3 |
| F · 凭据缺失（补充） | `[200]` | 1 | **0**（发请求之前就判） | `AUTH_MISSING` | — | 变量名 + `dry-run-fixture` | — |

**6/6 PASS**。要点：

- **A**：`meta.network = {"host":"127.0.0.1:58371","service":"127","attempts":2,"retries":0,"http_status":200,"signed_headers":"content-type;host;x-tc-action"}`，`pages=2 / rows_scanned=3 / @graph=3`，分页 `Offset 0 → 2`、`Limit=2`（来自 `paging.max_limit`），`Region` 在**头**里不在 body 里。
- **B**：`attempts=2 / retries=1`，**恰好重试 1 次**（退避 200ms）。
- **C**：`AUTH_REJECTED`，服务端**只看到 1 次**请求 —— 401 不重试是硬判据。
- **E**：3 次尝试后报 `UPSTREAM_TIMEOUT`（kind=Global），**不会**静默成"没数据"。
- **F**：**0 次请求**就报 `AUTH_MISSING` —— 不拿 401 当答案，也不浪费配额。

### 6.1 变异：`AUTH_REJECTED` 改成会重试 ⇒ 必须有测试红

在**副本**（`.local/qa-req144s1/mutants/retry-auth/`，`git archive HEAD` 出来的，⛔ 没碰 feature 分支）
里把 `classify_http` 与 `classify_api_error_code` 两处 `Verdict::Fatal(auth_rejected…)` 改成 `Verdict::Retry(…)`：

```diff
-        401 | 403 => Verdict::Fatal(auth_rejected(
+        401 | 403 => Verdict::Retry(auth_rejected(
-        return Verdict::Fatal(auth_rejected(status, &detail));
+        return Verdict::Retry(auth_rejected(status, &detail));
```

结果：`tests/test_req144_network.rs::auth_rejected_is_not_retried` **变红**（红行见 §6.2）。

### 6.2 变异红行

```
     Running tests\test_req144_network.rs (…cargo-qa144s1-mut\debug\deps\test_req144_network-…exe)

running 1 test
test auth_rejected_is_not_retried ... FAILED

failures:

---- auth_rejected_is_not_retried stdout ----

thread 'auth_rejected_is_not_retried' (59252) panicked at tests\test_req144_network.rs:518:5:
assertion `left == right` failed: API_ERROR · 服务端返回码 `FakeScriptExhausted`：脚本用完了（HTTP 500）。
  left: "API_ERROR"
 right: "AUTH_REJECTED"

failures:
    auth_rejected_is_not_retried

test result: FAILED. 0 passed; 1 failed; 10 filtered out; finished in 0.02s
```

红行原文：**`tests\test_req144_network.rs:518:5`**（`assert_eq!(err.code, CODE_AUTH_REJECTED)`）。
变异体在 401 上重试了 3 次，把假端点的脚本耗尽 ⇒ 最后拿到 500 `FakeScriptExhausted` ⇒ 码变成 `API_ERROR`
⇒ 断言立刻红。（第 519 行 `server.seen().len() == 1` 的「⛔ 不许重试」断言同样会红。）
完整日志：`.local/qa-req144s1/logs/mutant-auth-retry.log`。

### 6.3 F3：三个码没有测试覆盖（非阻断）

| 码 | 有测试吗 | 我的实测 |
| --- | --- | --- |
| `UPSTREAM_UNREACHABLE` | ⛔ 无断言 | DNS 失败 → `UPSTREAM_UNREACHABLE`（"io: 不知道这样的主机。(os error 11001)"），3 次尝试后报出 ✅ 行为正确 |
| `API_ERROR`（4xx/5xx 无信封分支） | ⛔ 无 | 未单独构造（走同一 `classify_http` 分支，风险低） |
| `AUTH_MODE_UNSUPPORTED` | ⛔ 无 | 未构造（`Credentials::resolve` 里 `:129` 分支） |
| **连接被拒**（127.0.0.1:1） | ⛔ 无 | 归到 `UPSTREAM_TIMEOUT`（kind=Connect）而**不是** `UPSTREAM_UNREACHABLE` —— 文案仍可行动（"确认到 `127.0.0.1:1` 的网络 / 代理是否通"），只是码名与直觉不符。ureq 3.4.2 把 connect 失败统一报成 `Error::Timeout(Connect)`，所以 `UPSTREAM_UNREACHABLE` 实际上只覆盖 DNS/TLS 一类。**建议**：要么把 `Timeout(Connect)` 归到 `UPSTREAM_UNREACHABLE`，要么在文档里写清这条边界。 |

---

## 7 · 凭据不外泄（P0-6）

脚本 `.local/qa-req144s1/cred_leak.py`（日志 `logs/credential-leak.log`）。
grep 对象：**产物**的 stdout / stderr（成功两页、429、401、缺字段、超时、连接被拒、凭据缺失 14 份文件）。

```
grep 占位凭据 'AKIDqatest' / 'secretqatest'
  [OK ] 1-connection-refused.stdout / .stderr
  [OK ] A-200-two-pages.stdout.json / .stderr.txt
  [OK ] B-429-then-success.stdout.json / .stderr.txt
  [OK ] C-401-auth-rejected.stdout.json / .stderr.txt
  [OK ] D-200-missing-iterator.stdout.json / .stderr.txt
  [OK ] E-timeout.stdout.json / .stderr.txt
  [OK ] F-auth-missing-before-any-request.stdout.json / .stderr.txt
命中数 = 0（必须 0）· 扫描文件数 = 14
```

`Debug` 打印（静态 + 行为两道）：

| 判据 | 结果 |
| --- | --- |
| `Credentials` 是否 `derive(Debug)` | **否**（`net.rs:87-101` 手写 `impl Debug`） |
| 手写 `Debug` 里 `<redacted>` | `Credentials { secret_id: "<redacted>", secret_key: "<redacted>" }`（两字段各一次） |
| `SignInput` 是否 `Debug` | **否**（`tc3.rs:63`：「不要给它 `Debug`：里面有 SecretKey」） |
| 凭据能不能从命令行给 | **不能**：clap 里没有 `--secret-id` / `--secret-key`（只有 `auth.credential_env` 的**变量名**出现在文档注释里） |
| 凭据能不能写进契约 | **不能**：`Source.auth` 只有 `mode` + `credential_env`（变量名） |
| `Authorization` 会不会被打印 | 不会：只作 HTTP 头发送（`net.rs:733`），stderr 只打 `signed_headers` 名字 |
| 错误信息里有没有值 | 没有：`AUTH_MISSING` 只点名**变量名**（`QA144_AK / TENCENTCLOUD_SECRET_ID、…`） |

---

## 8 · 零新增依赖（P0-7）

```
$ git diff develop..HEAD -- Cargo.toml Cargo.lock | Measure-Object -Line
Lines: 0
DIFF IS EMPTY: Cargo.toml + Cargo.lock 零差异
```

⇒ 手写 SHA-256/HMAC 的代价（R-B）是**真实存在**的，但**确实没新增依赖**。`ureq` 是 develop 上已有的那一个
（`Cargo.toml:50` 在基线里就有，diff 为空可证）。

---

## 9 · 阶段 0 九条 AC 仍绿（P0-8）

| 套件 | 结果 | 说明 |
| --- | --- | --- |
| `test_req144_access`（阶段 0 的 9 条） | **9 passed / 0 failed** | `ac001…ac008` + `extra_published_jsonld_is_queryable` |
| `test_req145_live`（平台侧消费） | **9 passed / 0 failed** | `ac001…ac008` + `r2_audit_…` |
| `test_req107_red_lines`（四红线） | **2 passed / 0 failed** | `test_req107_red_lines_and_guard_are_untouched` / `the_red_line_scanners_are_not_vacuous` |
| `test_req107_source_enumeration` | **10 passed / 0 failed** | 含 `t6_every_module_is_registered` ⇒ **新模块 `net` / `tc3` 已登记**（`tests/test_req107_source_enumeration.rs:1986` 起的 `MODULES` 表 +5 行） |
| `test_req144_network`（阶段 1 新增） | **11 passed / 0 failed** | 它自己的 11 条 |

`t6_every_module_is_registered` 绿 + diff 里那两行登记 ⇒ 「新增模块即红」这条闸门**确实被喂了新模块**，
不是靠"没有新模块所以不用改"混过去的。

---

## 10 · 全量逐 target（P0-9）

命令（两侧完全相同）：`cargo test --no-fail-fast --locked`，各自独立 `CARGO_TARGET_DIR`；
`ONTOK_TEST_DATABASE_URL` 指向元数据库（从 `kubectl -n anspire-ontok-uat get secret ontok-db` 解出口令，
**未落盘、未写进本报告**）。

> 基线数字**我自己实测**（`git archive 6f4987d` + 独立 target dir），不是采信 Dev 自报。
> 详细逐 target 表见 `.local/qa-req144s1/logs/test-compare.log`。

### 10.1 总量（两侧各自实测）

| | 测试 target 数 | passed | failed |
| --- | --- | --- | --- |
| 基线 `6f4987d` | 106 | **1866** | **7** |
| feature `1d48791` | 107（+`test_req144_network`） | **1883** | **9** |

`+17 passed` 的构成：`test_req144_network` 11 条（新）+ `tc3.rs` 单测 8 条（新）
+ `test_project` 1 条（基线因连接池超时 flaky 红，feature 绿）
− 3 条 live 用例在 feature 侧真的执行了（基线侧跳过，见 §10.2）。

### 10.2 失败**名字**集合对比（比 per-target 计数硬）

> ⚠️ 方法说明：`--no-fail-fast` 下多个测试二进制的输出会出现行交错，个别 target 的
> `test result:` 行可能被记到相邻 target 上（我第一版 per-target 表里 `test_req126_live`
> 两侧出现 0/0 就是这个原因）。**失败的名字不会串**，所以最终判据用集合对比。
> 逐 target 明细（含每格 P/F）仍在 `logs/test-compare.log`。

| | 失败名字 |
| --- | --- |
| 基线（7） | `real_schema_drop_rate_report` · `test_cli_end_to_end_output_is_byte_identical_to_legacy_extract` · `test_db_pg_is_postgres_16` · `test_real_schema_customer_id_has_exactly_one_display_name` · `test_req084_every_registered_name_is_still_literally_at_its_line` · `test_req084_no_unregistered_real_name_remains_in_the_registered_files` · `test_version_seq_under_concurrency_is_unique` |
| feature（9） | 上面 7 条里去掉 `test_version_seq_under_concurrency_is_unique`（基线 flaky：`pool timed out`），加上 **3 条 live**：`live_export_and_r2rml_…` · `live_two_views_…` · `live_auth_etag_audit_and_mcp_resource` |

**5 条两侧同红（基线也红 ⇒ 不是新增）的原因**（全部是环境/前置，不是本提交）：

| 用例 | 为什么红 |
| --- | --- |
| `real_schema_drop_rate_report` / `test_real_schema_customer_id_…` | 读不到脱敏 `schema.json`（`.review-workspace/fix-props/schema.json` 被 gitignore，本机没有）⇒ 门禁按纪律**必须红** |
| `test_db_pg_is_postgres_16` | 共享 UAT 库实际是 **PostgreSQL 17.10**，用例期望 16 |
| `test_req084_*`（2 条） | 客户表名登记表过期（`src/domain/concept.rs` 实际 9 处、登记 5 处）——**基线同红** |
| `test_cli_end_to_end_…_legacy_extract` | 抽取值与黄金基线不一致（基线同红） |

### 10.3 3 条「只在 feature 红」的 live 用例：**A/B 复跑证伪**

它们都死在同一个前置上：`✗ 项目 "anspire_r2rml_excl" 不存在。`（共享 UAT 库里没有那份冻结项目 v2）。

我做了**同一 DB、同一套显式 env** 的 A/B 复跑（`ONTOK_REQ124_SCHEMA` / `ONTOK_REQ127_SCHEMA` 都指到
`<repo>/code/out/schema.json`）：

| 侧 | 结果 |
| --- | --- |
| feature `1d48791` | `test result: FAILED. 1 passed; 1 failed` · `panicked at test_req124_live.rs:448` · `✗ 项目 "anspire_r2rml_excl" 不存在。` |
| 基线 `6f4987d` | `test result: FAILED. 1 passed; 1 failed` · `panicked at test_req124_live.rs:448` · **同一句话** |
| 基线（127） | `test result: FAILED. 0 passed; 2 failed` · `panicked at test_req127_live.rs:196` · 同一句话 |

⇒ **基线二进制跑同一批用例、同样的 env，红得一模一样** ⇒ 不是本提交引入的回归。

**那为什么全量那一轮基线显示 2/2 绿？** 因为这两个用例的 skip 条件是
`repo_root()/code/out/schema.json` 存不存在：我提取的基线树在 `.local/qa-req144s1/base-6f4987d/`，
`repo_root()` 指向 `.local/qa-req144s1/`，那里**没有** `code/out/schema.json` ⇒ 走
`*** SKIP: live_export_r2rml -- 缺 ONTOK_REQ124_SCHEMA -- 这条判据没有执行，不是通过 ***`
（skip 计为 passed）；而 feature 树就在仓库根，路径解析到了 ⇒ 真的执行了。
**这是基线 harness 的观感差，不是代码差**（我把它如实写出来，而不是把 3 条红算成回归）。

### 10.4 逐 target 明细（节选；完整见 `logs/test-compare.log`）

| target | 基线 P/F | feature P/F | 判定 |
| --- | --- | --- | --- |
| `test_req144_access`（阶段 0 九条） | 9/0 | 9/0 | OK |
| `test_req144_network`（阶段 1 新增） | —（基线无此文件） | 11/0 | 新增且全绿 |
| `test_req145_live` | 9/0 | 9/0 | OK |
| `test_req107_red_lines` | 2/0 | 2/0 | OK |
| `test_req107_source_enumeration` | 10/0 | 10/0 | OK |
| `unittests:src/lib` | 790/0 | **798/0** | +8（`tc3.rs` 单测） |
| `test_project` | 9/1（pool timeout flaky） | 10/0 | FIXED（非本提交功劳） |
| 其余 90+ 个 target | 全绿 | 全绿 | OK |

### 10.5 结论

**0 个新增失败**（扣除已 A/B 证伪的 3 条共用库前置后）。
基线 7 红、feature 9 红，**红的名字集合差集全部可归因于环境**。
`+17 passed` 的账：`+19`（11 条网络新测试 + 8 条 `tc3` 单测）`− 3`（3 条 live 用例在 feature 侧真的跑了：
基线侧走 `*** SKIP` 记成 passed、feature 侧执行后红）`+ 1`（`test_project` 那条基线 flaky 由红转绿）= **17**，对得上。


---

## 11 · R-D 落实检查（证据是否已在 `.local/`）

| 检查 | 结果 |
| --- | --- |
| `feature-REQ-144b/.local/req144-stage1/` 是否存在 | **❌ 不存在**（`.local/` 下只有 `cargo-req144b/`（Dev 的编译缓存）与我建的 `qa-req144s1/`） |
| `%TEMP%\ontok-req144-stage1\` 是否还在 | **✅ 还在**（`after/` `after-commit/` `after-final/` `baseline/` `demo/` + 4 份 `.log`） |
| 仓库里有没有「原临时路径作废」那一行 | **❌ 搜不到**（`git grep -i "req144-stage1\|%TEMP%\|AppData.Local.Temp"` → 0 命中） |

⇒ **R-D 没有落实。** 按 `acceptance.md` §四 R-D 的裁定「不阻断合并」，但它要求的两件事
（复制证据进 `.local/req144-stage1/` + 交付里写一行「原临时路径作废」）**都没做**。

**QA 已代为保全证据**（防止 `%TEMP%` 被清掉导致下一轮无据可查）：
复制到 `feature-REQ-144b/.local/req144-stage1-from-temp/`（**2.0 MB / 130 个文件**，
仍是 `.local/` 下、仍被 git 忽略：`git check-ignore -v` 命中 `$GIT_DIR/info/exclude:8:/.local/`）。
⚠️ 这**不能替代** Dev 的那一行声明 —— 归属与「作废」表述必须由 Dev/BA 在交付里写。

（Dev 的编译缓存 `.local/cargo-req144b/` 已 18.6 GB，属 R-D 同类的磁盘卫生问题，建议一并清理。）

---

## 12 · 我的临时目录 + 是否清理

全部在 **worktree 的 `.local/` 内**（`git check-ignore -v` 确认被 `$GIT_DIR/info/exclude:8:/.local/` 忽略）：

| 目录 | 用途 | 清理前 | 处置 |
| --- | --- | --- | --- |
| `.local/qa-req144s1/` | **证据**：脚本 / 日志 / artifacts / vectors / harness 源码 / 基线源码树 | 31.9 MB | **保留**（16.8 MB / 908 文件） |
| `.local/cargo-qa144s1/` | feature 编译缓存 | 16,736.9 MB | **已删** |
| `.local/cargo-qa144s1-base/` | 基线编译缓存 | 16,566.7 MB | **已删** |
| `.local/cargo-qa144s1-mut/` | 变异体编译缓存 | 3,582.3 MB | **已删** |
| `.local/cargo-qa144s1-harness/` | 对拍 harness 编译缓存 | 27.0 MB | **已删** |
| `base-6f4987d.tar` / `head.tar` | 两个 `git archive` 中间件 | 15.9 MB | **已删**（可随时 `git archive` 重生成） |

**合计释放 ≈ 36.0 GB**（删的是**我自己的** `cargo-qa144s1*` 四个 target dir + 两个 tar；
`git status --porcelain` 在 feature 分支上为空 ⇒ 没有改 feature 分支、也没有 push）。
⚠️ 未删 **Dev 的** `.local/cargo-req144b/`（18.6 GB）—— 不属我处置范围，但建议 Dev/BA 清掉。

`D:\` 根下**没有**我建的任何目录（`Get-ChildItem D:\ -Directory` 里没有 `qa*`/`144*`/`ontok*`）。
说明：二进制在收集完 sha 与全部对拍结果后随 target dir 一起删除，**可用
`cargo build --locked` 在同一个 `.local/cargo-qa144s1` 里原样重建**（源码未变，HEAD 仍是 `1d48791`）。


---

## 13 · 发现清单

| # | 级别 | 位置 | 发现 | 建议 |
| --- | --- | --- | --- | --- |
| F1 | 建议（提高判据强度） | `src/access/tc3.rs:426-432, 546-551` | 注释称「SecretKey 打码 ⇒ 派生链第一跳无法对拍」**不成立**；文档的打码串就是密钥本身（32 个 `*`）。我已用全链复算命中官方 `Signature` | 把 `doc_input().secret_key` 改成 `"*".repeat(32)` 直接走 `sign()`，删掉「无法对拍」的论断；这样 `official_vector_1` 能覆盖**四步全链** |
| F2 | **流程（必须补，不阻断）** | `.local/`·交付说明 | R-D 未落实：证据仍在 `%TEMP%`，`.local/req144-stage1/` 不存在，交付里也没有「原临时路径作废」 | Dev 复制证据 + 补那一行；BA 把它记入 `dispatch/rules.md` 复述清单 |
| F3 | 建议 | `src/access/net.rs:419-459, 369-395` | `UPSTREAM_UNREACHABLE` / `API_ERROR`(4xx/5xx) / `AUTH_MODE_UNSUPPORTED` **零测试覆盖**；且「连接被拒」被归到 `UPSTREAM_TIMEOUT`(kind=Connect) 而非 `UNREACHABLE` | 补 3 条断言（都能离线造）；决定 connect 失败该归哪个码，并写进 `cli.rs` 的码表 |
| O1 | 观察 | `src/access/net.rs:874-879` | `truncated` 用 `page_size`（`paging.max_limit`）而非**本页实际请求的** `page_limit`；当 `--limit` 小于 `max_limit` 时可能漏判/误判截断 | 非本轮回归（新增代码），建议下一轮用 `page_limit` 对齐 |
| O2 | 观察 | `src/access/net.rs:56-59` | `X-TC-Region` 缺省时只进 warning 不报错 —— 区域性接口会在服务端失败（报 `API_ERROR` 4xx），归因链条较长 | 可考虑在 `--allow-network` 时把「契约有 `region` 下推字段但没 `defaults.Region`」升级为启动期错误 |

---

## 14 · 证据索引（全部在 `.local/qa-req144s1/`）

| 文件 | 内容 |
| --- | --- |
| `tc3_ref.py` | 我的 TC3 独立实现（hashlib/hmac） |
| `gen_vectors.py` / `vectors.json` | 5+8 SHA、5+3 HMAC、5 个签名向量的期望值生成器与产物 |
| `harness/` | 用 `#[path]` 编译**原文** `tc3.rs` 的对拍 harness |
| `compare.py` → `logs/primitive-compare.log` | 81 字段对拍（python / rust 双列） |
| `logs/doc-vector-independent.log` | 官方向量全链复算（含打码 key = 32 个 `*`） |
| `no_network.py` → `logs/no-network.log` | 裸 TCP 监听器 + 不可解析域名的未联网证明 |
| `regression.py` → `logs/regression-8cases.log` | 8 case 逐字节对拍（feature vs 基线二进制） |
| `error_classes.py` → `logs/error-classes.log` / `error-classes-summary.json` | 五类错误 + 凭据缺失（含每个请求的独立签名复算） |
| `mutants/retry-auth/` + `logs/mutant-auth-retry.log` | AUTH_REJECTED 变异体与其变红结果 |
| `cred_leak.py` → `logs/credential-leak.log` | 凭据外泄 grep（14 份产物，0 命中） |
| `logs/dns-fail.log` | DNS 失败 → `UPSTREAM_UNREACHABLE` 实测 |
| `logs/build-feature.log` / `build-base.log` | 两个二进制的构建记录 |
| `logs/test-feature.log` / `test-base.log` / `test-compare.log` | 全量测试原始日志与逐 target 对比 |
| `logs/test-failset.log` | 失败**名字**集合对比 + 3 条 live 用例的 A/B 复跑结论 |
| `logs/retest-schema-explicit-feature.log` / `-base.log` | 同一 DB、同一套显式 env 下的 A/B 复跑原文 |
| `logs/no-network.log` 等 | 见上（每个 P0 项一份日志） |
| `artifacts/` | 8 case 的 stdout/stderr/publish 产物、假端点请求记录、离线契约 |

# 需求：REQ-152 · 本体服务只做「描述」，把执行器与厂商细节移出产品

> 合并原 REQ-150（两份 mapping schema）与"`get` 缺配方"（原拟的 REQ-152）。

## 基本信息

| 项 | 值 |
| --- | --- |
| 需求编号 | REQ-152 |
| 优先级 | **P0**（决定"接第二家云要不要改代码"） |
| 状态 | 已就绪 |
| 基线 | `v0.1.8`（develop `6682621`）· **接入期冻结，不合入 v0.1.8** |
| 触发 | 真实巡检复盘：数字员工拿 `@pending.get` 后**必须自己重建整个请求**（44 次 eval 里过半在造签名/翻页/参数），且**猜错了接口版本** |

## 一 · 三个问题，一个根因

| # | 现象 | 根因 |
| --- | --- | --- |
| 1 | `@pending.get` 只有 `{class, filter, pushdown, ref}`，**没有 action / version / host / iterator / 字段映射** | 描述**不完整**——客户端只能靠自己的云知识补齐 |
| 2 | `query serve --access-mapping` 与 `ontok access validate` 认**两份 schema**（`freshness` 对象 vs 字符串），部署靠临时投影脚本 | 同一份契约两个解析器 |
| 3 | **产品二进制里带着 3,093 行执行器**（`net.rs` 1755 + `tc3.rs` 465 + `fetch.rs` 873），含签名与凭据读取 | 越界：我们只该描述 |

**根因是同一个**：**"描述"和"执行"没有被真正分开**。

## 二 · 要做（六项）

### ① 一份 mapping schema（消灭双形状）

- `query serve` 与 `access validate` 解析**同一份**文件（`freshness` 两种写法都接受，内部归一）
- **删除** `Deploy/k8s/overlays/uat-ops360/gen-serve-mapping.py` 与 `am-tencent.serve.json`

### ② `operation` 泛化成 `headers{}` + `vars{}`（去厂商特化）

**现在**（腾讯风味，写死在字段名里）：

```json
"operation": { "action": "DescribeInstances", "version": "2017-03-12", "method": "POST", "content_type": "…" }
```

**目标**（厂商概念降级为 header 模板）：

```json
"operation": {
  "method": "POST", "path": "/",
  "headers": {
    "Content-Type":   "application/json; charset=utf-8",
    "X-TC-Action":    "{action}",
    "X-TC-Version":   "{version}",
    "X-TC-Timestamp": "{timestamp}",
    "X-TC-Region":    "{region}"
  },
  "vars": { "action": "DescribeInstances", "version": "2017-03-12" }
}
```

- **判据**：`grep -iE 'X-TC-|X-Amz-|X-Aliyun' src/` 在**产品路径**（`src/query/`、`src/access/{mod,validate,publish}.rs`）里 = **0**
- ⛔ 不许为"某家云"加 if/else

### ③ `@pending.get` 暴露**完整配方**

```json
"get": {
  "class": "cvm_instance", "filter": {}, "pushdown": ["region","zone"],
  "source": { "base_url": "https://cvm.tencentcloudapi.com", "auth_mode": "tc3-hmac-sha256" },
  "operation": { "method": "POST", "path": "/", "headers": { … } },
  "request": { "template": {"Region":"{region}"}, "required": ["region"], "paging": { … } },
  "iterator": "…InstanceSet[*]",
  "fields": { "name": "InstanceName", "status": "InstanceState", … },
  "auth": { "credential_env": ["…"], "signed_headers": ["content-type","host","x-tc-action"],
            "timestamp": {"header":"X-TC-Timestamp","format":"unix-seconds"},
            "scope": ["date","service","tc3_request"] },
  "ref": "access#CvmInstance@v1"
}
```

**⛔ 绝对不给**：任何凭据值、任何签名实现。

### ④ `auth` 写成**可实现规范**（给客户端，不是给代码）

- 新增 `skills/ontok-query/AUTH-SPEC.md`：算法名 / 签名头 / 时间戳格式 / scope / 规范化规则
- 附录 A 的 60 行参考实现作为**示例**（已用官方向量验签）
- **判据**：一个没接过腾讯云的人照这份文档 + 附录 A，能自己实现并通过我们的对拍用例

### ⑤ **二进制合并成 2 个**（服务面 / 构建面），执行器归构建面

**现状：5 个 bin，镜像里塞了 4 个**——客户镜像里带着勘查 / 提案 / 翻译表工具，这些是 FDE 本机的活。

| 现状 | 归属 |
| --- | --- |
| `ontok`（主 CLI：serve/query/project/version/access…） | **拆开**：服务面 → `ontok`；构建面 → `ontok-build` |
| `ontok-introspect` | → `ontok-build introspect` |
| `ontok-propose` | → `ontok-build propose` |
| `ontok-translate-names` | → `ontok-build translate-names` |
| `ontok-gate-preflight` | → `ontok-build gate-preflight` |

**目标：两个 bin**

```
ontok        服务面：query serve / mcp + catalog/describe/TTL 渲染      ← 唯一进镜像
ontok-build  构建面：introspect / propose / translate-names / gate-preflight /
                     project / version / import / review / freeze / export /
                     access validate & fetch（含签名与凭据读取）
```

**这一刀切在 ADR-007 的 build/consume 线上**：执行器不需要单独造 bin，它天然属于构建面。

**判据（能红能绿）**：

```bash
# ① 只有两个 bin
ls target/release/ontok target/release/ontok-build && ls target/release/ontok-{introspect,propose,translate-names,gate-preflight} 2>/dev/null | wc -l   # ⇒ 0
# ② 产品 bin 里没有签名代码
strings target/release/ontok | grep -c 'X-TC-Action'                                   # ⇒ 0
# ③ 产品 bin 里没有真调用入口
target/release/ontok access fetch --allow-network 2>&1 | grep -q 'unrecognized|不认识的'  # ⇒ 命中
# ④ 构建 bin 仍能真调（验收能力不丢）
target/release/ontok-build access fetch --mapping … --allow-network                     # ⇒ 正常
# ⑤ 镜像里只有一个 bin
podman run --rm <image> sh -c 'ls /usr/local/bin | grep -c ontok'                       # ⇒ 1
```

### ⑥ 契约 v1 → v2（加法式）

- `contract_version: 2`
- `get` 新增字段**全部可选**；老客户端忽略即可 ⇒ 不破坏
- `contract_version: 1` 的映射仍可加载（缺字段时退回 v1 行为）

## 三 · 不做

| ⛔ | 理由 |
| --- | --- |
| 把签名算法写成配置（嵌入式脚本沙箱） | 复杂度爆炸，且没有第二家云的真实需求 |
| 支持多厂商签名算法的**实现** | 执行器移出产品后，它只需服务"验收"这一个用途 |
| 改本体内容 | 那是 REQ-153（补地域/可用区）的事 |
| 动 `v0.1.8` 基线 | 接入期冻结 |

## 四 · 关键裁定

| # | 裁定 |
| --- | --- |
| R1 | **服务面（serve/MCP/HTTP）零签名代码**——这是本需求的核心，不是副产品 |
| R2 | 配方的**描述**进产品，配方的**执行**出产品 |
| R3 | 厂商差异只能出现在**映射文件**与**执行器（已移出）**里，⛔ 不出现在产品代码 |
| R4 | 契约加法式升级：老映射、老客户端都要能继续跑 |
| R5 | 验收能力不许丢（真调是对拍映射唯一可靠的手段） |
| R6 | 质量闸新增：产品路径 grep 厂商 token = 0（现在会红 10 处） |

## 五 · 风险

| # | 风险 | 缓解 |
| --- | --- | --- |
| 1 | 摘执行器时把"验收能力"也摘了 | R5 + 一条对拍用例（用真凭据跑 CVM 一次） |
| 2 | `get` 变大 ⇒ 响应体积上升 | 14 个 entity × 几个类，量级可接受；必要时按 `class` 分页返回 |
| 3 | 客户端不兼容新 `get` | 加法式 + 老字段保留一版 |
| 4 | 接入期改这个会影响接入方 | **不合入 v0.1.8**，接入验收通过后发 `v0.2.0` |

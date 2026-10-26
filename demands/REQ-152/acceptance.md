# 验收标准：REQ-152

> 基线 `v0.1.8`（`6682621`）· **不合入 v0.1.8**（接入期冻结），目标是 `v0.2.0`

## 一 · AC

### AC-001 · 一份 schema（`freshness` 双形状消失）

- `query serve --access-mapping` 与 `ontok access validate` 解析**同一份** `am-tencent.json`
- `freshness` 写成对象 `{mode,note}` 或字符串 `"realtime"` **都能加载**
- `gen-serve-mapping.py` 与 `am-tencent.serve.json` **已删除**
- 判据：用**同一份全量 AM** 启动 serve（不经过投影）+ validate 通过

### AC-002 · 产品路径零厂商 token（QG，**现在会红**）

```bash
grep -riE 'X-TC-|X-Amz-|X-Aliyun|tencent|aws|aliyun' \
     src/query/ src/access/{mod,validate,publish,path}.rs
```

- 期望 **0 命中**（允许：错误码文案里不出现厂商名）
- 原型先跑一次，**贴出红行**（证明这条判据能红）
- 判据：红 → 绿

### AC-003 · `operation` 数据驱动

- 用一份**非腾讯形状**的映射（如 `X-Amz-Target` + `aws-sigv4` 声明）构造 body：
  - 断言发出的 header 与映射声明**逐字一致**
  - 断言产品代码里没有任何分支因厂商而变
- 判据：body 原文 + `git diff --stat src/query/` 里无厂商判断

### AC-004 · `@pending.get` 配方完整

- 对 14 个 entity：`get` 必须含 `source.base_url` / `operation.{method,path,headers}` /
  `request.{template,required,paging}` / `iterator` / `fields` / `auth.{credential_env,signed_headers,timestamp,scope}` / `ref`
- **⛔ 不含**：任何凭据值、任何密钥、任何签名实现
- 判据：一个 entity 的响应原文 + `grep -c 'SECRET_KEY\|AKID'`（响应里）= 0

### AC-005 · 契约 v2 加法式

| 场景 | 期望 |
| --- | --- |
| 老映射（`contract_version: 1`，没有 headers） | 仍能加载，`get` 退回 v1 形状（只 4 个键） |
| 新映射（v2） | `get` 完整配方 |
| 老客户端只看 4 个键 | 不报错（多余键忽略） |
| 判据 | 三种组合各一次 |

### AC-006 · 两个 bin（服务面 / 构建面），产品 bin 无签名代码

| 判据 | 期望 |
| --- | --- |
| `target/release/ontok` 与 `ontok-builder` 存在 | ✅ 两个 |
| `ontok-introspect` / `ontok-propose` / `ontok-translate-names` / `ontok-gate-preflight` | **不再产出**（已合并为 `ontok-builder` 的子命令） |
| `strings target/release/ontok \| grep -c 'X-TC-Action'` | **0** |
| `target/release/ontok access fetch --allow-network` | **不认识**（子命令不在服务面） |
| `podman run --rm <image> ls /usr/local/bin` | **只有 `ontok`** |
| 判据 | 五条实测 |

### AC-007 · 验收能力不丢（R5）

- `ontok-builder access fetch --mapping am-tencent.json --entity cvm_instance --allow-network`
  用**只读凭据**真调一次，返回 ≥1 行且 `rows_returned > 0`
- 判据：命令 + 输出行数（凭据由 BA 提供，QA 不落地）

### AC-008 · `auth` 可实现规范（④）

- `skills/ontok-query/AUTH-SPEC.md` 存在，含：算法名 / 签名头 / 时间戳格式 / scope / 规范化规则 / 参考实现入口
- **判据（人判 + 机判）**：
  - 机判：附录 A 的参考实现对**官方向量**验签一致（沿用现有用例）
  - 人判：由 QA 按文档**独立**实现一遍签名（不看附录 A），能通过同一个向量

### AC-009 · 零回归

- 服务面（serve/MCP/HTTP）行为：除 `get` 变丰（加法）外，`catalog`/`describe`/`health`/`TTL`/503/400 码与文案逐字节不变
- 本体 TTL（客户视图 + 建模视图）**逐字节不变**（`79741a7f…` / `e78e296e…`）
- 判据：8 case sha 对拍 + TTL sha

### AC-010 · 不退化

| 项 | 约束 |
| --- | --- |
| ⛔ 禁改 | 四红线 + `tools.rs` + `guard()` + `property_filter.rs` |
| ⛔ 禁 | 新依赖（摘代码是减法，更不该加）；删测试；放宽断言 |
| 全量 | 三列（passed/failed/SKIP）+ 逐条归因；基线必须**含生成物** |

## 二 · 纪律

- **G** 变异：①把 `get` 的配方字段删掉 ⇒ AC-004 红 ②把厂商 header 写回 `net.rs`（若还在产品里）⇒ AC-002 红
  ③把执行器塞回产品二进制 ⇒ AC-006 红
- **S** AC 也会错，发现直接说


---

## 二 · 阶段 2 的 AC（契约：`get` 给配方）

### AC-101 · `get` 完整配方（contract v2）

对 14 个 entity：`get` 必须能给出下列键（映射里有的都要给）：

| 键 | 内容 |
| --- | --- |
| `source` | `base_url` · `auth_mode` |
| `operation` | `method` · `path` · `headers{}` · `vars{}` |
| `request` | `template` · `required` · `paging` |
| `iterator` | 列表在响应里的路径 |
| `fields` | 本体属性 → 响应字段 |
| `auth` | `credential_env`（**只有变量名**）· `signed_headers` · `timestamp` · `scope` |

判据：贴 1 个类的 `get` 原文 + 14 个类的键齐全度表。

### AC-102 · `get` 里**零凭据值**

- `grep -E 'AKID|SECRET_KEY\s*[:=]\s*"[^"]'` 在响应里 = 0
- 只允许出现**环境变量名**（如 `TENCENT_SECRET_ID`）
- 判据：一次响应全文 + grep

### AC-103 · 契约 v1 向后兼容（R2）

| 映射 | `get` |
| --- | --- |
| `contract_version: 1` / 不写 | **旧 4 键，一个不多** |
| `contract_version: 2` | 完整配方 |

- 判据：同一份 AM 改一行版本号，两次 `get` 原文对比

### AC-104 · 已签 AM（旧形状）照常工作

- `am-tencent.json` **一个字不改**，serve 能起、`get` 走 v1 分支
- 判据：起服务 + 一次查询

### AC-105 · `operation` 泛化（无厂商分支）

- 映射里写任意 header 名（例如 `X-Amz-Target`），`get.operation.headers` **原样透出**
- `grep -riE 'X-TC-|X-Amz-|tencent|aws' src/query/ src/access/{mod,validate,publish,path}.rs` = **0**（映射文件里的不算）
- 判据：造一份假厂商映射跑一次 + grep

### AC-106 · `auth` 只透传，不推断

- 映射 `sources[].auth` 不给 `signed_headers` ⇒ `get.auth` **没有这个键**（⛔ 不许按 `mode` 猜一个默认值）
- 判据：删掉该字段再跑一次

### AC-107 · `get` 体积上限

- 单类 `get` ≤ **12 KB**；超了要报（说明 `fields` 过肥）
- 判据：14 个类的体积表

### AC-108 · 服务面零回归（R1）

- 8 case（health / catalog / describe / pending / rows 400 / 503 / TTL 两视图）与 `8efd323` **逐字节相同**
- 判据：sha 表

### AC-109 · 产品 bin 仍不含执行器（R4）

- `strings target/release/ontok | grep -c 'X-TC-Action'` = **0**
- `ls target/release | grep -c '^ontok'` = **2**（`ontok` / `ontok-builder`）
- 判据：两条命令

### AC-110 · `auth` 可实现规范

- worktree 里 `docs/AUTH-SPEC.md` 存在，含：算法 / 规范化规则 / 时间戳 / scope / 签名字符串结构 / 自测向量
- **人判**：QA 只读这份文档（不看源码）能独立实现并通过同一个向量
- 判据：QA 的独立实现 + 向量比对

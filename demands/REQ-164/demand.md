# REQ-164 · AM 取数脚本进 skill（数字员工别再手写）

- 状态：**已就绪**（P1）
- 提出：2026-10-27（用户：「AM 的查询脚本做到 skill 内，这样数字员工拿到 AM 就不用去自己做查询脚本了，还总出错」）
- 负责：join（`Deploy/skills/ontok-query/`）
- 关联：REQ-144（AM 契约）、REQ-145（API 源）、AUTH-SPEC（TC3 签名）

---

## 一 · 一句话

**把「读 AM → 签名 → 分页取数 → 映射成 JSON-LD」这一整套做成 skill 里的一个脚本**，
数字员工**调用它**，而不是照着 AUTH-SPEC 自己写一遍（那件事他们总写错）。

---

## 二 · 为什么现在必须做

| 现状 | 后果 |
| --- | --- |
| skill 只给了**规格**（AUTH-SPEC 212 行 + SKILL.md 里"数据你自己取"） | 每个数字员工都要**自己实现 TC3 签名 + 分页 + 字段映射** |
| TC3 签名（HMAC 链 + 规范化请求）极易写错 | 实测症状：签名不匹配、时间戳时区错、分页死循环、字段路径写错 |
| 每个客户环境各写一遍 | 同一件事有 N 份实现，**对不齐也没人负责** |

> 一句话：**规格 → 规格 + 可执行参考实现。** 规格还在，但不再是唯一交付物。

---

## 三 · 交付物（都在 `Deploy/skills/ontok-query/`）

| 文件 | 内容 |
| --- | --- |
| **`access_fetch.py`**（新） | **零第三方依赖**（Python 3.8+ 标准库）的取数器 |
| `AM-FIELDS.md`（新） | AM 每个键是什么、脚本用到了哪些、哪些脚本**不**用 |
| `SKILL.md`（改） | 新增一章「API 源取数：**用脚本，别手写**」+ 常见错误表 |
| `fixtures/`（可选） | 一份最小 AM + fixture，供 `--selftest` 离线跑 |

---

## 四 · 脚本契约

### 4.1 命令行

```
python access_fetch.py --mapping <am.json> --entity <id|class> [选项]

  --param K=V          可重复；**只允许 capability.pushdown 里的键**
  --limit N            每页条数（上限 paging.max_limit）
  --max-rows N         最多取多少条（默认 1000；**取不全要如实说**）
  --dry-run            用 entity.fixture 取数，⛔ 不发网络
  --check              只校验 AM 结构，不取数
  --selftest           离线自检（无网络、无凭据）
  --format jsonld|rows 默认 jsonld
  --out <file>         默认 stdout
```

### 4.2 取数流程（必须按这个顺序）

1. **校验 AM**：`contract_version` 认识（1）· 必填键 · entity 引用的 `source` 存在 · entity **必须已签**（`reviewed_by` 非空，与 R6 一致：未签不许取数）
2. **凭据**：只从 `auth.credential_env` **命名的环境变量**读（⛔ 绝不从文件/命令行/AM 里读明文）
3. **签名**：`auth.mode == "tc3-hmac-sha256"` 时按 AUTH-SPEC 生成 `Authorization` + `X-TC-Action/Version/Timestamp/Region`
4. **请求**：`operation`（action/version/method/content_type）+ `defaults` + `--param` 合并成 body
5. **分页**：按 `paging.{offset_param,limit_param,max_limit,total_path}` 翻页；受 `--limit` / `--max-rows` 约束
6. **迭代**：按 `iterator`（JSONPath 子集 `$.a.b[*]`）取出记录
7. **映射**：`fields.<属性>.path`（点号路径）→ 属性值；`identity.key_path` → `@id`（填 `iri_template` 的 `{id}`）
8. **输出**：JSON-LD（`--format jsonld`）或平表（`rows`）

### 4.3 错误分类（**固定码 + 固定退出码**）

| 码 | 触发 | 退出码 |
| --- | --- | --- |
| `am.invalid` | AM 结构/版本不对 | 2 |
| `am.unsigned` | entity 未签（`reviewed_by` 空） | 2 |
| `param.not_pushdown` | 给了 `capability.pushdown` 之外的参数 | 2 |
| `credential.missing` | `credential_env` 命名的环境变量没设 | 3 |
| `auth.rejected` | 401/403/签名错 | 4 |
| `api.throttled` | 429 / `RequestLimitExceeded` | 5 |
| `api.invalid` | 400 参数错 | 6 |
| `net.timeout` / `net.error` | 超时/连不上 | 7 |
| `rows.truncated` | 到 `--max-rows` 还没取全 | **8**（数据可用，但**必须明说没取全**） |

⛔ **任何码都不许把凭据、签名字串、`SecretKey` 打出来**（报错信息要脱敏）。

### 4.4 输出形状（jsonld）

```json
{ "@context": {"ontok": "https://ontok.dev/ns#"},
  "@graph": [
    { "@id": "ontok:cvm_instance/ins-abc123",
      "@type": "ontok:cvm_instance",
      "ontok:instance_id": "ins-abc123",
      "ontok:cpu": 4 }
  ],
  "meta": { "entity": "CvmInstance", "class": "cvm_instance",
            "rows": 1, "pages": 1, "truncated": false,
            "source": "tencent-cvm", "mapping_version": 1 } }
```

`meta` 里**必须**有 `rows` / `pages` / `truncated` —— 数字员工靠它判断"够不够"。

---

## 五 · 验收

| # | 判据 |
| --- | --- |
| AC-001 | `--check` 对真实 AM（8 sources / 14 entities）全通过；造坏 AM（缺 `reviewed_by` / 引用不存在的 source / `contract_version=9`）⇒ **点名拒绝**，退出码 2 |
| AC-002 | `--dry-run --entity CvmInstance` 用 fixture 取数 ⇒ 输出 JSON-LD，`@id` 与 `iri_template` 逐条对得上，`meta.rows` 正确 |
| AC-003 | **TC3 签名对已知向量逐字节相等**（固定 secret/时间戳/body ⇒ 断言 `Authorization` 头全串） |
| AC-004 | 分页：fixture 造 2 页（TotalCount=150）⇒ **恰 2 次请求、150 行**；`--max-rows 120` ⇒ 停在第 2 页、`meta.truncated=true`、**退出码 8** |
| AC-005 | 凭据缺失 ⇒ 退出码 3、点名环境变量名、⛔ 不回显值；全部输出/日志 grep 不到 secret |
| AC-006 | 401 / 429 / 400 / 超时 各映射到 §4.3 的码与退出码 |
| AC-007 | 给一个不在 `capability.pushdown` 的参数 ⇒ 拒绝并**列出可用参数** |
| AC-008 | **零第三方依赖**：`ast` 解析 import 清单，只允许标准库；⛔ 出现 `requests`/`urllib3` 等 |
| AC-009 | `reviewed_by` 为空的 entity ⇒ `am.unsigned`、退出码 2（与 R6 一致） |
| AC-010 | `--selftest` **断网也能过**（不访问任何 URL），退出码 0 |
| AC-011 | SKILL.md 新章能让**没读过 AUTH-SPEC 的人**照着跑通（验收方式：把脚本删掉，只看新章能否描述出该跑什么命令） |

---

## 六 · 不做

| 不做 | 为什么 |
| --- | --- |
| 把 `ontok-builder access fetch` 拷进 skill | builder 是 Rust 二进制，客户环境未必有；且它绑定了我们的 CLI 形态 |
| 支持 AM 里没有的源类型（如 `mysql`） | v1 只做 `kind: http-json` |
| 自动重试 / 指数退避 | v1 先"报清楚错"，重试策略交给调用方（写进文档） |
| 把结果推到服务端 | **数字员工自己算、自己用**；服务端不存这些行 |

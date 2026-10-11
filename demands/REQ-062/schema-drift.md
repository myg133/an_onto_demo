# REQ-062 附：源库 schema 漂移实测记录

> 采集时间：2026-10-10（VPN 已通）
> 采集方式：项目内 `.review-workspace/tmpdbprobe/.venv` 里的 psycopg3，
> **每条连接都带 `options=-c default_transaction_read_only=on`**（连接级强制只读，
> 服务端会拒绝任何写语句 —— 已实测 `CREATE TEMP TABLE` 被拒：
> `cannot execute CREATE TABLE in a read-only transaction`）。
> **全程零业务数据行读取**，只查 `pg_database` / `information_schema` / `_sqlx_migrations`。
> 源库地址与口令**未写入本文件**，只以环境变量 `ONTOK_SOURCE_DATABASE_URL` 承载。

---

## 1. 登记基线（门禁输入，不可删）

`.review-workspace/fix-props/schema.json`
sha256 `b71c2afd…`，219688 B。

| 项 | 值 |
| --- | --- |
| 表数 | 91 |
| 列数 | 1051 |
| 外键数 | **0** |
| server_version | **PostgreSQL 17.4**（Debian 17.4，gcc 8.3.0-6） |
| 采集方式 | 现拉（`ontok-introspect`），非入库产物 |

## 2. VPN 实例上的候选源库

VPN 实例：`PostgreSQL 16.14`（Debian 16.14-1.pgdg13+1）。

⚠️ **与基线的 17.4 不是同一个大版本。**

全实例非模板库 14 个，逐库核对后：

| 库 | public 表 | 列 | 是否候选 |
| --- | --- | --- | --- |
| `<业务库名>` | 96 基表 + 1 视图 | 1128 | ✅ **表名 91/91 全中** |
| `aisearch_uat` | 97 | 1126 | 表名同中，列数对得更差 |
| `ontol` | 11 | 85 | ❌ 是**本平台自己**的库（`projects`/`proposals`/…），非客户业务库 |
| 其余 11 个 | — | — | 与本项目 schema 无重合 |

## 3. `<业务库名>` 与基线的差异（负责人已裁定：接受滞后）

### 3.1 基线有、活库**没有**的 5 列（这 5 列就是「滞后」的实体）

| 表 | 缺失列 |
| --- | --- |
| `deductions` | `model_id`、`provider` |
| `package_templates` | `sale_limit` |
| `service_config_peak` | `status`、`weekdays` |

逐表列数：26→24、23→22、14→12，合计 **-5**。

### 3.2 活库多出来的 6 个对象（源库合法新增，**不阻断**）

| 对象 | 类型 | 列数 | 说明 |
| --- | --- | --- | --- |
| `_volc_dts_pg_ddl_18e4b12d3d9d40aa81853a72d73fcd3c` | 基表 | 13 | 火山引擎 DTS 同步簿记 |
| `_volc_dts_pg_ddl_8f7cc0a636ac424f97b7c7b772fbdbc5` | 基表 | 13 | 同上 |
| `_volc_dts_pg_ddl_9a38a0a4c4f442df8445b6af3ef73e49` | 基表 | 13 | 同上 |
| `llm_provider_back` | 基表 | 13 | 备份表 |
| `llm_provider_back_20260630` | 基表 | 13 | 备份表 |
| `v_deductions_with_binding` | **视图** | 17 | 派生视图 |

### 3.3 算术闭合（证明上面的清单是完整的，不是抽样）

```
1046（重叠 91 表的活库列数）
+  65（5 张新基表 × 13 列）
+  17（1 个视图）
= 1128 = 活库 public 总列数   ✅
```

活库 public 对象数：96 基表 + 1 视图 = 97；重叠 91 + 5 新基表 + 1 视图 = 97。✅

### 3.4 ⚠️ 本节曾漏记三类形态（2026-10-10 由 Dev 与 QA 各自独立发现，BA 已复核）

**§3.1/§3.2 只覆盖了「缺表」「缺列」「多出对象」三类。以下三类原清单完全没写。**

#### 形态 c · 列**类型**变化（2 处）—— 最危险的一类

| 表.列 | 登记基线 | 现拉 |
| --- | --- | --- |
| `llm_model_peak_config.peak_periods` | `jsonb` | `json` |
| `service_config_peak.llm_metadata` | `jsonb` | `json` |

全库 `jsonb` 列数：**基线 9 → 现拉 6**（差 3 = 2 处变形 + 1 条整体缺失的 jsonb 列）。

**为什么这类最危险**：§3.1 的「算术闭合」**发现不了它** ——
表名列数全对，差的只是列的 `data_type`。
而抽取器把 `jsonb` 与 `json` 当两种类型处理时，
产出的本体里该属性会**变成另一种东西，且没有任何地方会说「类型变了」**。

这恰好印证 AC-003 的立论（服务端版本不同 ⇒ 类型名/呈现可能不同，
**而这类差异不体现在表名列数上**）—— 两处 `jsonb → json` 用「比列数」永远发现不了。

#### 形态 d · 注释消失（**0 处**）

基线有注释、现拉无注释的列：**0 处**。
（所以 AC-002 把形态 d 降级为 `warn` 在**这份数据上不会触发**，
它的杀伤力只能靠合成夹具证明 —— 这正是必须保留合成夹具的理由。）

#### 形态 g · 现拉多出注释（**255 列**）

基线无注释、现拉有注释的列：**255 列**。

⚠️ 这是**合法演进**（客户后来补了注释），**只 warn 不阻断**。
但它的规模不小（255/1046 ≈ 24%），足以让「注释覆盖率」这类指标看起来剧变，
**任何按注释比例设阈值的判据都会被它误伤** —— 记在这里以备后查。

## 4. 为什么这必须变成 fail-closed，而不是靠人记得

**BA 已核实：全仓没有任何「现拉结果 vs 登记基线」的对账逻辑。**

- `src/bin/ontok-introspect.rs` —— 只负责连库现拉并写文件
- `src/bin/ontok-gate-preflight.rs` —— 只按规则校验**文件本身**（drop rate / FK 命名 / 重名表 / 条数上限）
- 两者之间**没有**「你拉的这份是不是我们登记的那一份」的判断

⇒ 后果：把源库指向 `<业务库名>` 后，门禁会拿一份 96 表/1111 列的 schema 去跑
**规则校验**（这些规则并不在意少 5 列），然后**判「通过」**。
抽取照跑，产出的本体**静默少 5 列**，且**没有任何一处会说「你少抽了」**。

这与本会话反复堵的「判据恒真 / 假安心」是同一个病：
**门禁给了「通过」这个信号，但那个信号不承载它声称的含义。**

---

## 5. 端到端实证（2026-10-10 14:10，BA 亲跑）

摘要中长期挂着一条阻塞项「`ontok-gate-preflight` 现拉路径尚无端到端实证」——
本条一并结清。

命令（在 `code/` worktree，用 `--out` 指向草稿路径以免覆盖基线）：

```
cargo run --bin ontok-gate-preflight -- --out <仓库根>/.review-workspace/req062d/live-pulled.json
```

结果：**退出码 0**。输出自带 `只读留痕 : transaction_read_only = on`。
事后 `sha256sum` 复核登记基线**仍是 `b71c2afd…`，未被触碰**。

| | 登记基线 | 现拉产物 |
| --- | --- | --- |
| 路径 | `.review-workspace/fix-props/schema.json` | `.review-workspace/req062d/live-pulled.json` |
| sha256 | `b71c2afde6c10313…` | `15ce6a4eadf5a7adc1f8…` |
| 字节 | 219688 | 240109 |
| 表 / 列 / 外键 | 91 / 1051 / 0 | 96 / **1111** / 0 |
| server_version | PostgreSQL **17.4** | PostgreSQL **16.14** |

列数 1111 ≠ 之前手算的 1128：`introspect` 只数 **BASE TABLE**，
不含那个 17 列的视图 `v_deductions_with_binding`（1128 − 17 = 1111，自洽）。

### 5.1 工具自己看到的差异（与 §3 的手工核对逐项吻合）

| 形态 | 实测值 |
| --- | --- |
| 基线独有表（形态 a） | **0 张** |
| 现拉独有表 | **5 张**（3 × `_volc_dts_pg_ddl_*` + `llm_provider_back` ×2） |
| 少列的表（形态 b） | **3 张**：`deductions`（`model_id`/`provider`）、`package_templates`（`sale_limit`）、`service_config_peak`（`status`/`weekdays`） |

### 5.2 三条对实现有直接影响的结论

1. **形态 a 在真实数据里是 0，形态 b 是 3。**
   ⇒ 只按真实数据写判据的话，**a 分支没有任何真实样本**，
   它的杀伤力只能靠变异 M1 证明。**不得因「真实数据里没有 a」而省掉 a。**
2. **门禁现有阈值对这份现拉产物是「80 表下限 vs 实测 96 / 900 列下限 vs 实测 1111」——区间正中，必然放行。**
   坐实 AC-000。
3. **`server_version` 差异是实测存在的（17.4 vs 16.14）**，AC-003 有真实样本可跑。

### 5.3 两份文件结构同构

顶层键集合相同，表项键均为
`['column_count', 'columns', 'comment', 'commented_count', 'name']`
⇒ **对账可复用同一个 `SchemaReport` 解析路径，不需要第二套解析器。**

### 5.4 现拉命令自己印的警告（别误当成验证结果）

原话：**「读得到 ≠ 门禁会通过」** ——
`drop_rate` / `test_fk_naming` 两个门禁还要跑抽取器 `extract()` 与投影 `project()`，
**本命令一件都没跑**。原话还点名一条已实测的假信心：
「91 张同名表 + 完整列 ⇒ 本命令通过，drop_rate 门禁仍判红」。
**该警告只适用于那两个门禁，与 REQ-062 无关，但不得被当成本条的验证结论引用。**
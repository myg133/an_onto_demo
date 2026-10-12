# 快速开始 · QUICKSTART

> 本文件里的每条命令都在真实源库（96 表 / 1111 列）上实测跑通过。
> 首次使用请先读 [§2 三个必踩的坑](#2-三个必踩的坑)，否则大概率跑不出想要的结果。

---

## 1. 两条路径

| 角色 | 做什么 | 从哪开始 |
| --- | --- | --- |
| **FDE** | 访问表 → 抽取本体 → 建项目 → 审核落库 → 冻结 | [§3](#3-fde-路径六步) |
| **客户** | 用 client agent 经 MCP 问业务问题 | [§4](#4-客户路径接入-mcp) |

---

## 2. 三个必踩的坑

### 🔴 坑 1 · 仓库自带的翻译表是**脱敏占位版**，直接用会丢掉 71 张表

`src/extract/table_names.json` 是**表名 → 中文显示名**的翻译表。
仓库里那份为了不外泄客户表名，是占位符版本（键是 `t001`、`t002`…）。

实测对照（同一份 schema）：

| 翻译表 | 无中文名的表 | 后果 |
| --- | --- | --- |
| 真版（91 条真实表名） | **5 张** | 正常 |
| 入库的脱敏版 | **71 张**（共 96 张） | 客户视图把 71 张全过滤掉 |

> 翻译表是**部署方资产**，不入库。请用自己 schema 生成一份，覆盖 `src/extract/table_names.json`
> 或用 `--names` 指向你自己的路径。

判定有没有踩到：抽取输出的 `⚠ 无中文名（会被客户视图过滤）` 这一行。
**超过 5 张就说明翻译表没对上。**

### 🔴 坑 2 · 项目名写在**提案 JSON 里**，不在 `import` 命令上

`ontok propose import` **没有 `--project` 参数**。项目名是 `ontok-propose --project X`
在生成提案时写进文件的。所以**抽取时就要把项目名定好**，后面改不了。

### 🔴 坑 3 · 二进制会陈旧

`target/debug/ontok` 是编译产物，不会随 `git pull` 更新。
pull 完代码**先 `cargo build`**，否则你可能在跑几天前的版本
（本次就遇到二进制比代码旧了 18 小时，导致新功能完全看不到）。

---

## 3. FDE 路径（六步）

### 第 0 步 · 环境配置

```bash
cd <仓库>/code

# .env（两行，勿提交）
#   ONTOK_SOURCE_DATABASE_URL=   ← 只读勘查用（源库）
#   ONTOK_TEST_DATABASE_URL=     ← 写入用（本体库）
set -a && . ../.env && set +a

cargo build --bin ontok --bin ontok-introspect --bin ontok-propose
```

源码库**只读**：只访问 `information_schema` / `pg_catalog`，零业务数据行。
勘查输出会打印 `transaction_read_only = on` 作为凭据。

### 第 1 步 · 访问表

```bash
ontok-introspect --out ./schema.json
```

实测输出示例：

```
服务端     : PostgreSQL 16.14 …
只读确认   : transaction_read_only = on
schema    : public
表 / 列    : 96 / 1111
有注释列   : 962 (87%)
外键约束   : 0
```

### 第 2 步 · 抽取本体

```bash
ontok-propose \
  --schema ./schema.json \
  --names  <你的翻译表.json> \
  --out    ./proposal.json \
  --stats  ./stats.json \
  --project myproj          # ← 项目名在这里定（坑 2）
```

关键输出行：

| 指标 | 含义 |
| --- | --- |
| `候选类型 / 候选属性` | 抽出了多少实体、多少属性 |
| `⚠ 无中文名（会被客户视图过滤）` | **超过 5 张就是坑 1** |
| `关系` | 推断出的关系条数（零外键下靠列名后缀推断） |
| `⚠ 低质量注释` | 列注释无汉字且非枚举值域，已回退词表兜底 |

### 第 3 步 · 建项目与基线版本

```bash
ontok project new --name myproj
ontok version new  --project myproj      # → v1（空版本，作为提案的基线）
```

### 第 4 步 · 导入 → 双轨审核 → 应用

```bash
ontok propose import --file ./proposal.json
ontok review decide  --proposal p1 --accept-all
ontok proposal apply --proposal p1        # → 产生 v2
```

- `--proposal p1` 里 `p1` 是**项目内序号**别名（`p2`、`p3`…），也可直接给 UUID。
- 不加 `--accept-all` 时逐条审：`--item <序号> --track biz|model --accept|--reject`
  （`biz` 是业务轨、`model` 是建模轨）
- ⚠️ **顺序不能错**：必须先 `apply` 再 `freeze`。反了会报
  「提案基于的版本已被冻结」。

### 第 5 步 · 冻结

```bash
ontok version freeze --project myproj --version 2
```

未冻结的版本 `mcp-server` 会**拒绝启动**（fail-closed）。

### 第 6 步 · 自己看一眼

```bash
ontok ontology show --project myproj --version 2              # 客户视图
ontok ontology show --project myproj --version 2 --model-view # 建模视图（含 key / 证据），FDE 用
ontok ontology overview --project myproj --version 2          # 业务总览（按概念）
```

---

## 4. 客户路径（接入 MCP）

```bash
ontok mcp-server --project myproj --version 2
```

标准 MCP over **stdio**（JSON-RPC 2.0）。任何支持 stdio 的 MCP client 都能直接挂。

### 握手与工具清单（实测）

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{
  "protocolVersion":"2024-11-05","capabilities":{},
  "clientInfo":{"name":"your-client","version":"1.0"}}}
```

返回：

```json
{"serverInfo":{"name":"ontok","title":"本体问答服务","version":"0.1.0"}}
```

`tools/list` 返回 5 个工具：

| 工具 | 入参 | 用来回答 |
| --- | --- | --- |
| `list_concepts` | 无 | 「系统里有哪些业务对象」 |
| `describe_concept` | `name` | 「客户这个概念包含什么」 |
| `describe_object` | `name` | 「订单这个对象有哪些信息」 |
| `list_relations` | `object`（可选） | 「订单和什么有关系」 |
| `search_objects` | `query` | 「有没有跟限流有关的东西」 |

### 关系的读法（重要）

`list_relations` 每条关系给三个数量字段，**含义不同**：

| 字段 | 含义 |
| --- | --- |
| `how_many` | 关系本身的原始说法（`多对一` / `一对多`…），**不告诉你站在哪一端** |
| `count` | **站在本次查询那个对象这一端**看过去的数量词（`多个` / `1 个`） |
| `count_side` | 它站在 `from` 端还是 `to` 端 |

**传了 `object` 时 `count` 才有值**，且方向随该对象自动翻：

```bash
# 站在「客户订阅」这一端
{"name":"list_relations","arguments":{"object":"客户订阅"}}
→ 客户订阅 --归属于--> 客户   how_many=多对一
   count="1 个"   count_side="from"

# 不传 object：返回全部关系，**没有单一视角**
{"name":"list_relations","arguments":{}}
→ count=null              ← 显式空值，不藏一个未经声明的视角
   count_from="1 个"  count_to="多个"
```

⚠️ **别把无过滤时的 `count` 当成「有 N 个」**——它一定是 `null`。

---

## 5. 命令速查

```bash
ontok --help                      # 总览
ontok project  new|list
ontok version  new|list|freeze
ontok propose  import --file <f>
ontok review   show | decide --proposal p1 [--accept-all | --item N --track biz|model --accept]
ontok proposal apply --proposal p1
ontok ontology show|diff|overview --project P --version V
ontok concept  import|digest
ontok mcp-server --project P --version V
```

独立二进制：

```bash
ontok-introspect       --out <schema.json>        # 源库只读勘查
ontok-propose          --schema --names --out --stats --project
ontok-gate-preflight                            # 抽取质量门禁
```

---

## 6. 故障排查

| 症状 | 原因 | 怎么办 |
| --- | --- | --- |
| `⚠ 无中文名` 超过 5 张 | 翻译表是脱敏占位版（坑 1） | 换真版翻译表重抽 |
| `提案基于的版本已被冻结` | 先 freeze 再 apply | 用新项目重来，顺序 apply → freeze |
| `--proposal 认不出：1` | 只认 UUID 或 `p1/p2` 序号别名 | 用 `p1` |
| `✗ 版本不存在` | 项目名/版本号错，或项目没建 | `ontok project list` / `ontok version list --project P` |
| mcp-server 拒绝启动 | 该版本未冻结 | `ontok version freeze` |
| 提案落到别的项目去了 | `propose import` 没有 `--project` | 项目名来自提案文件；重新用对的 `--project` 抽取 |
| 新功能看不到 | 二进制陈旧（坑 3） | `cargo build` |
| 抽取统计里 `entities_with_cn_name` 正常但名字是 `对象 001` | 脱敏占位版翻译表 | 同坑 1 |
| 测试偶发 `ConnectionReset` | 共享集群连接抖动 | 单跑重跑确认；不要为此加重试掩盖 |

---

## 7. 当前能力边界

- **概念分组是「待确认」状态**：`confirmed_by` 为空时，分组只表示机器的猜测
  （当前规则是「表名首个下划线词干」）。业务分组需要人工确认后才算结论。
- **关系基数是按列名后缀推断的**，不是观测结论。源库零外键，
  判定 `N:1` 与 `1:1` 需要 UNIQUE 约束或值域，当前拿不到。
- **备份表/归档表会被当成独立对象抽出来**（如 `*_back_*`、`*_archive`、
  `dim_*`、`rpt_*`）。它们的处理属于「副本表去重」，不是「概念归并」。

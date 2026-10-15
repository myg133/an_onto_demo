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

## 2. 四个必踩的坑

### 🔴 坑 1 · 仓库自带的翻译表是**脱敏占位版**，直接用会丢掉 71 张表

`src/extract/table_names.json` 是**表名 → 中文显示名**的翻译表。
仓库里那份为了不外泄客户表名，是占位符版本（键是 `t001`、`t002`…）。

实测对照（同一份 schema）：

| 翻译表 | 无中文名的表 | 后果 |
| --- | --- | --- |
| 真版（91 条真实表名） | **5 张** | 正常 |
| 入库的脱敏版 | **71 张**（共 96 张） | 客户视图把 71 张全过滤掉 |

> 翻译表是**部署方资产**，不入库。**从 REQ-090 起有生成器了**，
> 不用手写 —— 见「第 2 步 · 生成翻译表」，一条命令。

判定有没有踩到：抽取输出的 `⚠ 无中文名（会被客户视图过滤）` 这一行。
**超过 5 张就说明翻译表没对上。**

⚠️ **两条红线测试也会替你报警**：翻译表对不上时，
`tests/drop_rate.rs` 与 `tests/test_fk_naming.rs` 必红（丢弃率闸被击穿）。

### 🔴 坑 2 · 项目名写在**提案 JSON 里**，不在 `import` 命令上

`ontok propose import` **没有 `--project` 参数**。项目名是 `ontok-propose --project X`
在生成提案时写进文件的。所以**抽取时就要把项目名定好**，后面改不了。

### 🔴 坑 3 · 二进制会陈旧

`target/debug/ontok` 是编译产物，不会随 `git pull` 更新。
pull 完代码**先 `cargo build`**，否则你可能在跑几天前的版本
（本次就遇到二进制比代码旧了 18 小时，导致新功能完全看不到）。

### 🔴 坑 4 · `table_names.json` 是 **skip-worktree** 的本地资产，**不跨 worktree 传播**

坑 1 说过：真版翻译表**不入库**（它就是客户 schema 本身）。本地是用 git 的
**skip-worktree 位**把它藏起来的 —— 但**这个位是每个 worktree 各自索引上的，
`git worktree add` 不会带过去**。

⇒ 后果：**任何新建的 worktree / feature 分支都会静默拿到仓库里的脱敏占位版**。
表面上 `git status` 干干净净，两条红线测试却红了 —— 而红的原因跟你的改动**毫无关系**。
（本次真被这件事坑过一次：Dev 跑出 3 条失败，判成「pre-existing 缺陷」，
其实只是新 worktree 没带过那个位。）

**新建 worktree 后先验这一条：**

```bash
git ls-files -v -- src/extract/table_names.json   # 期望行首是 S（S = skip-worktree）
# 若是 H（普通跟踪），把它恢复：
cp <你那个有真表的 worktree>/src/extract/table_names.json src/extract/
git update-index --skip-worktree src/extract/table_names.json
git ls-files -v -- src/extract/table_names.json   # 再确认变成 S
git status --short                                # 期望空
```

**推论 · 跑测试前先认基线在哪**：同一个提交，在「有真表的 worktree」和「新 worktree」里
**全量测试数字可以差好几十条**，差的是本地资产不是代码。
所以**基线数字必须标明是在哪个 worktree 测的**，别拿 A 处的基线去卡 B 处的结果。

---

## 3. FDE 路径（六步）

### 第 0 步 · 环境配置

```bash
cd <仓库>/code

# .env（两行，勿提交）
#   ONTOK_SOURCE_DATABASE_URL=   ← 只读勘查用（源库）
#   ONTOK_TEST_DATABASE_URL=     ← 写入用（本体库）
set -a && . ../.env && set +a

cargo build --bin ontok --bin ontok-introspect --bin ontok-propose --bin ontok-translate-names
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

### 第 2 步 · 生成翻译表，再抽取本体

> **2a · 生成翻译表**（REQ-090）。翻译表是**表名 → 中文显示名**的映射，
> 抽取器靠它把英文表名变成客户看得懂的名字。**缺了它，96 张表里 71 张会被静默过滤**（坑 1）。

```bash
export ONTOK_LLM_API_KEY=...          # 只走环境变量，不入库、不进文件、不进日志
export ONTOK_LLM_MODEL=...

ontok-translate-names \
  --schema ./schema.json \
  --out    ./table_names.json \
  --endpoint "$ONTOK_LLM_BASE_URL"    # OpenAI 兼容端点的 base_url
```

**退出码就是给脚本看的，四档互不重载**（脚本只看退出码就能决定处置方向，不必去匹配报错文案）：

| 码 | 含义 | 你该做什么 |
| --- | --- | --- |
| **0** | 成功：**全覆盖、无残留、无退化** | 唯一能拿 0 的情形，放心用 |
| **1** | 命令 / 环境 / 数据形状错 | 去修命令。**没有写出任何文件** |
| **2** | 命令没问题，**但数据不完整**（有表没翻出来） | **没有写出任何文件**。stderr 会列出缺哪几张 |
| **3** | 产物已写出，仍有缺口 / 残留 / 退化 | 读产物里的 `_coverage` 字段 |

> 退出 2 **不写产物**是有意的：一份残缺的翻译表会让抽取器整批丢实体而**没人看见**，
> 不如直接停下来报错。要强行落盘加 `--allow-gaps`（但**一张都没翻出来时加也没用**，
> 空表过不了校验，仍是 2）。

改完 schema 后要**增量**重跑（别从头翻，人工确认过的译名不该被机器覆盖）：

```bash
ontok-translate-names --schema ./schema.json --out ./table_names.json \
  --names ./table_names.json --endpoint "$ONTOK_LLM_BASE_URL"
# 只重翻指定表：          --overwrite <表名>
# 全部重翻（仍不删条目）： --regenerate-all
```

⚠️ **一条翻译是人工确认过的知识资产，不是机器输出。** 不给 `--overwrite` / `--regenerate-all`
时**旧条目逐字节原样保留**，schema 里已消失的表也**只进 `_coverage.absent_from_schema` 清单、
绝不静默删除** —— 删除必须是人的显式动作。

**2b · 抽取本体**

```bash
ontok-propose \
  --schema ./schema.json \
  --names  ./table_names.json \      # ← 2a 刚生成的（坑 1）
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
ontok-translate-names  --schema --out [--endpoint] [--names --overwrite|--regenerate-all]
                                                 # 生成翻译表（REQ-090）
ontok-propose          --schema --names --out --stats --project
ontok-gate-preflight                            # 抽取质量门禁
```

---

## 6. 故障排查

| 症状 | 原因 | 怎么办 |
| --- | --- | --- |
| `⚠ 无中文名` 超过 5 张 | 翻译表是脱敏占位版（坑 1） | 用 `ontok-translate-names` 生成真版重抽 |
| `ontok-translate-names` 退出码 **2** | 命令没问题，是**有表没翻出来** | stderr 会列缺哪几张；补齐后重跑，或 `--allow-gaps` 强行落盘 |
| `ontok-translate-names` 退出码 **3** | 产物已写出，但有缺口 / 残留 / 退化译名 | 读产物的 `_coverage` 字段 |
| 两条红线测试突然变红，`git status` 却干净 | 新 worktree 没带上 skip-worktree 的真翻译表（坑 4） | `git ls-files -v -- src/extract/table_names.json` 看行首是不是 `S` |
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

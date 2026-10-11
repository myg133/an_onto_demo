# 抽取可信度 spike · 使用说明

**对应需求**：[REQ-001](../../REQ-001-spike/demand.md) · **规范依据**：设计规格 §7.1

---

## 这个 spike 回答什么

> **LLM 能否从一张真实客户库里，抽出客户认得的候选本体？**

判据不是「输出得像不像本体」，而是 **客户看一眼能不能指出哪里对、哪里不对**。

## 核心设计：验证权在我们手里

```
阶段 0 · 准备       读 schema + 采样 + 脱敏        纯代码
阶段 1 · 分组抽取   产生候选 + 声明证据           可外置（mock / 模型 API / A2A）
阶段 2 · 证据验证   查表列是否真实存在             ★ 纯代码，不可外包
阶段 3/4 · 一页纸   渲染给客户判读                纯代码
```

**阶段 2 是这个 spike 的心脏。** agent 说「设备.序列号 ← `t_dev.sn`」，
由我们的代码去查 `t_dev` 是否真的有 `sn` 列。**被验掉的比例就是幻觉率。**

---

## 快速开始

### 用 uv（推荐）

`pyproject.toml` 里**基础依赖刻意留空** —— 阶段 0 / 阶段 2 / 一页纸渲染全是纯 stdlib，
所以自测零安装即可跑。真实抽取的依赖按模式拆成 extras，按需装。

```bash
cd spike

# 零依赖自测（不装任何包）
uv run run.py --self-test --out /tmp/spike-out

# 读真实 PostgreSQL + 直连模型
uv run --extra full run.py \
  --dsn "postgresql://ro:pw@host:5432/db" \
  --extractor anthropic \
  --scope "目标决策：判断哪些设备该做预防性维护" \
  --out out

# 按需组合 extras
uv run --extra postgres --extra anthropic run.py --dsn ... --extractor anthropic
uv run --extra mysql run.py --dsn "host=...;dbname=..." --dialect mysql --extractor mock
```

| 命令 | 装什么 |
| --- | --- |
| `uv run run.py --self-test` | **什么都不装** |
| `--extra postgres` | `psycopg[binary]` |
| `--extra mysql` | `pymysql` |
| `--extra anthropic` | `anthropic` SDK（原生 Claude） |
| `--extra openai` | `httpx`（**任意 OpenAI 兼容端点**） |
| `--extra a2a` | `httpx`（外部抽取 agent） |
| `--extra full` | 以上全部 |

`uv.lock` 已提交，团队跑的是同一套依赖版本。

> **不需要 uv？** 直接 `python3 run.py --self-test` 同样能跑 —— 自测零依赖。
> 真实抽取时 `pip install -r` 或 `pip install psycopg[binary] anthropic` 即可。

### 零依赖自测（任何 Python 3.11+）

```bash
python3 run.py --self-test --out /tmp/spike-out
```

用内置的规范例子库（复刻了设计规格 §2.3 里的 `t_dev` / `t_device_info` / `dev_backup`），
**不需要数据库、不需要 API key、不装任何东西**。

预期输出：

```
[阶段 0] schema 已落盘：...（5 张表）
[阶段 2] 证据验证：保留 2 / 4，幻觉率 50.0%
         column_not_found: 1     ← 故意埋的假列
         table_not_found: 1       ← 故意埋的假表
```

> 50% 是**故意的** —— fixture 里埋了两条坏证据，用来验证阶段 2 确实拦得住。

### 读真实客户库

```bash
# PostgreSQL
uv run --extra postgres run.py --dsn "postgresql://user:pw@host:5432/db" --extractor mock

# MySQL（DSN 用 key=value 串）
uv run --extra mysql run.py \
  --dsn "host=db.internal;user=ro;password=***;dbname=crm" \
  --dialect mysql --extractor mock
```

> ⚠ **务必使用只读账号。** 代码里已加 `-c default_transaction_read_only=on`（PG）作为双保险。
> 连不上时会给出逐项排查提示，且**自动遮蔽 DSN 里的密码**（避免打进日志或截图）。

### 97 张表怎么办 —— 用范围闸门

**这是设计规格 §2.6「决策清单限定范围」的直接落地。** 全库抽一轮 = 上百次模型调用，
又慢又贵，而且违背「决策清单限定范围」这条设计。

```bash
# 先看有哪些表
uv run --extra postgres run.py --dsn "postgresql://..." --schema-only

# 再只抽这条决策相关的表
uv run --extra full run.py --dsn "postgresql://..." \
    --tables "t_dev,t_ticket,*workshop*" \
    --scope "目标决策：判断哪些设备该做预防性维护" \
    --extractor openai --base-url http://192.168.31.101:8317/v1 \
    --model MiniMax-M3 --api-key sk-xxx
```

`--tables` 支持逗号分隔 + `*` 通配。命中 0 张时会列出真实表名帮你对。
命中超过一半时会警告「范围闸门的作用是收敛，不是全覆盖」。

**先验连通性再跑抽取**（推荐，省一轮往返）：

```bash
uv run --extra postgres run.py --dsn "postgresql://..." --schema-only
```

只读 schema 并打印列、主键、注释、采样行数，**不调抽取器**。
若这里就报错，多半是连不上或目录权限不足，不必等跑到抽取那一步。
采不到样本会提示「无 SELECT 权限，或表为空」。

### 换成真实抽取 —— 四种抽取器

```bash
# A · Claude（原生 SDK）
export ANTHROPIC_API_KEY=sk-...
uv run --extra anthropic run.py --dsn "postgresql://..." --extractor anthropic \
    --model claude-sonnet-4-5

# B · 任意 OpenAI 兼容端点（一个实现覆盖七家）
uv run --extra openai run.py --dsn "postgresql://..." \
    --extractor openai --provider openai --model gpt-4o-mini

# C · 外部 A2A agent（此时模型由对方选，与 A/B 不冲突）
uv run --extra a2a run.py --dsn "postgresql://..." --extractor a2a \
    --a2a-endpoint https://agent.example.com/rpc --a2a-agent vendor-extractor
```

#### B 支持的端点预设

| `--provider` | 端点 | 默认模型 | 环境变量 |
| --- | --- | --- | --- |
| `openai` | `api.openai.com/v1` | `gpt-4o-mini` | `OPENAI_API_KEY` |
| `deepseek` | `api.deepseek.com/v1` | `deepseek-chat` | `DEEPSEEK_API_KEY` |
| `qwen` | `dashscope.aliyuncs.com/compatible-mode/v1` | `qwen-plus` | `DASHSCOPE_API_KEY` |
| `zhipu` | `open.bigmodel.cn/api/paas/v4` | `glm-4-flash` | `ZHIPU_API_KEY` |
| `kimi` | `api.moonshot.cn/v1` | `moonshot-v1-8k` | `MOONSHOT_API_KEY` |
| `ollama` | `localhost:11434/v1` | `qwen2.5:7b` | *（本地，不强制）* |
| `vllm` | `localhost:8000/v1` | `local-model` | *（本地，不强制）* |

**其他兼容端点**（自建网关、企业内部代理）直接指定：

```bash
uv run --extra openai run.py --dsn ... \
  --extractor openai --base-url https://llm.corp.internal/v1 \
  --model internal-7b --api-key sk-xxx
```

> **抽取场景刻意设 `temperature=0`** —— 同一张表跑两次候选应当一致，
> 否则设计规格 §7.1「基于增量不重新生成」的收敛前提就没了。

---

## 输出物

| 文件 | 给谁看 | 说明 |
| --- | --- | --- |
| `out/report.html` | **客户** | 候选本体一页纸 ← **AC-005 的交付物** |
| `out/verification.json` | 你 | 幻觉率对照表，逐条列出「提了什么 / 为什么被丢」 |
| `out/schema.json` | 你 | 脱敏后的 schema + 样本，确认抽取读到的是正确数据 |
| `out/spike-summary.md` | 你 | 人工判读记录模板（含决策出口） |

**`report.html` 是自包含单文件，零外部依赖，可直接双击打开或投屏。**

---

## 判读标准

| 判据 | 阈值 | 在哪看 |
| --- | --- | --- |
| **AC-003 幻觉率** | **< 30%** 通过；30–50% 勉强；> 50% 停下 | 进程退出码 + `verification.json` |
| **AC-005 客户一眼判读** | 客户能**具体指出**哪里对、哪里不对 | 人工判读 |

> ⚠ **「客户说『还行』但指不出具体哪里错 = 不通过。**
> 那说明抽取没有真正降低客户的认知负担 —— 核心赌注未成立。

---

## 三条硬约束（来自设计规格 §7.1）

1. **产出的是提案，不是直接改本体。** Agent 是提议者，不是决定者。
2. **基于上一版做增量，不重新生成。** `key` 一旦生成就不可变，改名只改 `display_name`。
3. **一次抽取会话内，connector 集合与 agent 版本都固定。** 中途换版 = 换了输入，收敛判定作废。

另有一条**失败必须显式**：某组抽取失败会写进 `verification.json` 的 `failed_groups`，
且**不参与收敛判定** —— 否则「什么都没抽」会被误判成「已收敛」。

---

## 脱敏策略

| 列类型 | 处理 | 理由 |
| --- | --- | --- |
| 证件号 / 手机 / 邮箱 / 密码 / 地址 / 银行 | **完全脱敏**，只留类型 | PII |
| 人员姓名（`manager_name` / `owner_name` 等） | **完全脱敏** | PII |
| `dev_id` / `order_id` | **保留** | 系统标识，不是 PII |
| `workshop_name` / `product_name` | **保留** | 不是 PII（刻意收窄，避免过度脱敏毁掉抽取质量） |
| 枚举 / 低基数（`status` / `type`） | **保留** | 状态值是语义信息量最高的部分 |

> 收窄规则是**假设**，不是定论。拿真库跑完后，按实际数据分级调整
> （设计规格 §4.2：出域方案逐案确认，不预先选）。

### 真实库的非标类型

psycopg 会把 PG 数组 / JSON / 二进制读成 Python 的 list / dict / bytes，
直接进 set 会抛 `unhashable type: 'list'`。已统一规范化：

| 数据库类型 | 渲染成 |
| --- | --- |
| `text[]` / `jsonb` | `[设备, 产线]` / `{厂区: A}` |
| `bytea` | `«二进制 7 字节»` |
| `numeric` | `1234.56`（Decimal → float） |
| `timestamp` / `date` | ISO 字符串 |

另外 prompt 有 24k 字符预算 —— 宽表 + 长文本会自动裁剪样本行，不会顶爆上下文窗口。

---

## UI 约定：够用就好

> **spike 的页面只为「让判断发生」服务，不为好看服务。**
>
> 一次一页纸 HTML 能让人**看懂对象、看清连线、指出哪里不对**，这就达标了。
> 布局微调、交互优化、响应式打磨 —— **一律不做**。
>
> 产品级的双轨审核界面属**切片 1 第 3 份计划**（审核界面与 MCP 接入），届时按真实用户重新设计，不复用这里。

## 代码结构

| 文件 | 职责 |
| --- | --- |
| `core.py` | Schema 建模、采样脱敏、分组、**证据验证**（纯函数，零依赖） |
| `extractors.py` | 抽取器接口 + 四种实现（Mock / Anthropic / OpenAI 兼容 / A2A） |
| `render.py` | 候选本体一页纸 HTML |
| `run.py` | CLI 主入口 + 数据库适配层 |
| `fixture.py` | 内置规范例子库 |

**核心逻辑全是纯函数，数据库访问是薄适配层** —— 所以 `--self-test` 不需要任何环境。

---

## 下一步

跑完按 `out/spike-summary.md` 里的**决策出口**走：

| 结果 | 下一步 |
| --- | --- |
| 客户能一眼判读 | 启动内核重写（Rust），按设计规格 §0.2 |
| 客户判读不了但证据链清晰 | 调 prompt / 加领域提示，再跑一轮 |
| 幻觉率 > 50% | **停下**，重新评估「抽取结构」这个核心假设 |
| 跨表合成判断不可靠 | 该能力不外置，自研阶段 3（需回改规格 §7.1） |

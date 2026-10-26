# REQ-147 · 同模型对照实验（**唯一有效的那份证据**）

> 2026-10-26 · 语料：`.local/tencent-docs` 的 3 篇（cos / monitor / cbs，42 KB → **39 块**）
> 模型：**两边都是 `local-cpa/deepseek-v4.1-flash`**（ACP 侧由会话自报，见下）
> 工具：`ontok-builder doc ingest`（develop `d6f300f`）
> 度量口径：照 `acceptance.md` §R-1（泛动词 = 使用/读取/写入/处理/进行/支持/融合/调用/获取）

---

## 一 · 结论先说

> **skill 确实有效，但旧数字夸大了效果。**
> 旧的那组（泛动词 18.65%→1.31%、两端是概念 26.2%→96.4%）**混了模型变量**（ACP 侧 MiniMax、内置侧 deepseek）；
> 用**同一个模型**重测，改善依然显著，但幅度小得多。

---

## 二 · 同模型实测

| 指标 | 内置 LLM | **ACP + skill** | 变化 |
| --- | ---: | ---: | ---: |
| 断言 | 397 | **593** | **+49%** |
| 概念（去重） | 198 | **207** | +5% |
| 关系 | 139 | **258** | **+86%** |
| 谓词种类 | 27 | **40** | +48% |
| **泛动词占比** | 3.60%（5 条：获取×3、支持×2） | **0.00%** | **−100%** |
| **关系两端都是概念** | 65.5% | **89.9%** | **+24.4pp** |
| 包含类（宽口径） | 35.25% | 41.09% | +5.8pp |
| quote 校验通过 | 100% | 100% | — |
| 耗时 | 7.1 min | **4.7 min** | −34% |
| token | 109.5K | 187.1K | **+71%** |

**ACP 侧确实用了同一个模型**（会话配置自报）：

```
会话 … 的 configOptions：currentValue = "local-cpa/deepseek-v4.1-flash"
agent 身份：pi-acp v0.0.34（握手报的）
契约(skill)：skills/ontok-extract/SKILL.md（sha256 f8971b5dc782…）
```

---

## 三 · 该怎么读这组数字

| # | 判读 |
| --- | --- |
| 1 | **泛动词清零是真的**：内置侧还剩 5 条（获取×3 / 支持×2），ACP 侧 **0 条** |
| 2 | **端点闭合 +24pp**：内置 34.5% 的关系有一端不是概念（悬空），ACP 只有 10.1% |
| 3 | **关系数 +86%** 不是"抽得更全"，而是"更愿意把结构写成边"（包含字段 101 条 vs 内置 24 条） |
| 4 | **代价是 token +71%**（skill 全文随每块下发 + 输出更长），但**墙钟反而快 34%**（无重试/无解析失败） |
| 5 | ⚠️ **单次运行，n=1**，没有重复测方差；跨实验的数字**不可比**（语料/参数不同） |

---

## 四 · 与旧数字的对照（为什么旧数字不能引用）

| 指标 | 旧（混模型） | 本次（同模型） | 说明 |
| --- | --- | --- | --- |
| 泛动词 | 18.65% → 1.31% | **3.60% → 0.00%** | 旧内置侧 18.65% 与本轮 3.60% 差 5 倍 ⇒ **旧语料/参数不同，跨实验不可比** |
| 两端是概念 | 26.2% → 96.4% | **65.5% → 89.9%** | 旧内置侧 26.2% 明显偏低，疑与模型/语料有关 |

**结论**：旧那组只能写「ACP+MiniMax 优于 内置+deepseek」；**要证明"skill 值多少"，只能看本轮**。

---

## 五 · 复现命令

```powershell
$exe = ".local\cargo-develop\release\ontok-builder.exe"

# 内置
$env:ONTOK_LLM_BASE_URL='http://127.0.0.1:8317/v1'
$env:ONTOK_LLM_API_KEY='…'; $env:ONTOK_LLM_MODEL='deepseek-v4.1-flash'
& $exe doc ingest --input .local\exp147\corpus --out .local\exp147\builtin\assertions.jsonl --stats .local\exp147\builtin\stats.json

# ACP（pi-acp 默认 provider 就是 local-cpa/deepseek-v4.1-flash）
$env:CPA_API_KEY='…'
& $exe doc ingest --input .local\exp147\corpus --out .local\exp147\acp\assertions.jsonl --stats .local\exp147\acp\stats.json `
  --agent acp --agent-command "D:\ToolCaches\npm\pi-acp.cmd" `
  --agent-skill skills\ontok-extract\SKILL.md --agent-timeout-secs 300

# 度量
python .local\exp147\metrics_147.py .local\exp147\builtin
python .local\exp147\metrics_147.py .local\exp147\acp
```

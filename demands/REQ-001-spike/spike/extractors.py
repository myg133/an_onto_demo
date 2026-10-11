"""抽取器（阶段 1）—— 可插拔

设计规格 §7.1：这一层**可以外置**。因此这里定义一个极窄的接口，
三种实现：

  - `MockExtractor`   自测用，不调任何模型
  - `AnthropicExtractor`  直连模型 API，官方参考
  - `A2AExtractor`    调外部抽取 agent（协议见 §7.1）

关键约束：无论哪种实现，**产出必须带 `evidence`**。
没有证据的候选会被阶段 2 直接丢弃（见 core.verify_candidates）。
"""

from __future__ import annotations

import json
import os
import re
from typing import Protocol

from core import Candidate, Evidence, Schema, Table


class Extractor(Protocol):
    """抽取器接口 —— 越窄越好，这样换成什么都容易。"""

    name: str
    version: str

    def extract(self, table: Table, context: dict) -> list[Candidate]: ...


# ──────────────────────────────────────────────────────────
# 提示词 —— 刻意「受限」而非「开放」
# ──────────────────────────────────────────────────────────


PROMPT_TEMPLATE = """你是本体抽取助手。给定一张数据表的结构与样本，识别它描述的是哪一类**业务对象**。

## 硬性要求

1. 只输出 JSON，格式为 `{{"candidates": [...]}}`
2. 每个候选必须给出**每条属性对应的来源列名**，来源列必须来自下方给出的列清单。**不得编造列名。**
3. `key` 用稳定的英文蛇形命名，一旦确定后续轮次不要改
4. 拿不准就少提，不要靠想象补列
5. 不做命名美化 —— 用业务上真实存在的叫法，别自己创造术语

## 允许映射到的列（不得超出此清单）

{columns}

## 本轮已存在的类型（只提增量，不要重复提议）

{existing}

## 本轮范围限定（决策清单）

{scope}

## 表结构

{table}

## 样本行

{samples}

## 输出格式

{{
  "candidates": [
    {{
      "key": "device",
      "display_name": "设备",
      "aliases": ["机台", "机器"],
      "parent": null,
      "properties": [
        {{"name": "序列号", "column": "dev_code", "nullable": false}}
      ],
      "evidence": [
        {{"table": "{table_name}", "column": "dev_code", "claim": "该列是设备唯一标识"}}
      ]
    }}
  ]
}}
"""


_MAX_PROMPT_CHARS = 24_000   # 约 8k token；超了按行裁剪


def build_prompt(table: Table, context: dict, desensitized: list[dict]) -> str:
    cols = "\n".join(
        f"- {c.name} ({c.data_type}{', PK' if c.is_primary_key else ''})"
        + (f"  // {c.comment}" if c.comment else "")
        for c in table.columns
    )
    existing = context.get("existing") or "（无，这是第一轮）"
    scope = context.get("scope") or "（未限定）"
    schema_txt = f"表名：{table.name}" + (f"  // {table.comment}" if table.comment else "")

    # 样本行按行裁剪，直到 prompt 落到预算内。
    # 真实库里有宽表 + 长文本，不裁会直接顶爆上下文窗口。
    rows = list(desensitized)
    sample_txt = json.dumps(rows, ensure_ascii=False, indent=2)
    while rows and len(sample_txt) > _MAX_PROMPT_CHARS:
        rows = rows[: max(1, len(rows) // 2)]
        sample_txt = json.dumps(rows, ensure_ascii=False, indent=2) + (
            f"\n（样本已裁剪：仅保留前 {len(rows)} 行）"
        )

    return PROMPT_TEMPLATE.format(
        columns=cols,
        existing=existing,
        scope=scope,
        table=schema_txt,
        samples=sample_txt,
        table_name=table.name,
    )


# ──────────────────────────────────────────────────────────
# 实现 A：Mock（自测 / 无网络 / 无 key）
# ──────────────────────────────────────────────────────────


class MockExtractor:
    """不调模型。用一份预设答案覆盖测试链路本身是否正确。

    刻意**混入几条坏证据**，用来验证阶段 2 确实拦得住。
    """

    name = "mock"
    version = "0.0.1"

    def __init__(self, fixture: dict[str, list[Candidate]] | None = None):
        self.fixture = fixture or {}

    def extract(self, table: Table, context: dict) -> list[Candidate]:
        return list(self.fixture.get(table.name, []))

    def complete(self, prompt: str) -> str:
        """合成阶段用。默认把每组都判为「需要人看」——
        确定性信号只能提出合并，**拍板必须在这一层**。"""
        groups = re.findall(r"【组：(.+?)】", prompt)
        decisions = [
            {"tables": [t.strip() for t in g.split(",")],
             "verdict": "separate", "reason": "mock 不做判断，留给人",
             "relation": "sibling"}
            for g in groups
        ]
        return json.dumps({"decisions": decisions}, ensure_ascii=False)


# ──────────────────────────────────────────────────────────
# 实现 B：直连模型 API（按厂商）
# ──────────────────────────────────────────────────────────


class AnthropicExtractor:
    name = "anthropic-direct"
    version = "1.0"

    def __init__(self, model: str = "claude-sonnet-4-5", max_tokens: int = 4096):
        self.model = model
        self.max_tokens = max_tokens

    def extract(self, table: Table, context: dict) -> list[Candidate]:
        return _parse_candidates(
            self.complete(build_prompt(table, context, context.get("desensitized", []))),
            table.name, f"round{context.get('round', 1)}", self.name,
        )

    def complete(self, prompt: str) -> str:
        api_key = os.environ.get("ANTHROPIC_API_KEY")
        if not api_key:
            raise RuntimeError("缺 ANTHROPIC_API_KEY")
        try:
            import anthropic
        except ImportError as e:
            raise RuntimeError(
                "缺 anthropic SDK。请用：uv run --extra anthropic run.py ..."
            ) from e

        client = anthropic.Anthropic(api_key=api_key)
        resp = client.messages.create(
            model=self.model, max_tokens=self.max_tokens,
            messages=[{"role": "user", "content": prompt}],
        )
        return "".join(b.text for b in resp.content if getattr(b, "type", "") == "text")


# OpenAI 兼容端点预设 —— 覆盖绝大多数可自选模型的场景
OPENAI_COMPATIBLE_PRESETS: dict[str, dict] = {
    "openai":    {"base_url": "https://api.openai.com/v1",              "env": "OPENAI_API_KEY",   "model": "gpt-4o-mini"},
    "deepseek":  {"base_url": "https://api.deepseek.com/v1",           "env": "DEEPSEEK_API_KEY", "model": "deepseek-chat"},
    "qwen":      {"base_url": "https://dashscope.aliyuncs.com/compatible-mode/v1", "env": "DASHSCOPE_API_KEY", "model": "qwen-plus"},
    "zhipu":     {"base_url": "https://open.bigmodel.cn/api/paas/v4",  "env": "ZHIPU_API_KEY",    "model": "glm-4-flash"},
    "kimi":      {"base_url": "https://api.moonshot.cn/v1",            "env": "MOONSHOT_API_KEY",  "model": "moonshot-v1-8k"},
    "ollama":    {"base_url": "http://localhost:11434/v1",            "env": "OLLAMA_API_KEY",   "model": "qwen2.5:7b"},
    "vllm":      {"base_url": "http://localhost:8000/v1",             "env": "VLLM_API_KEY",     "model": "local-model"},
}


class OpenAICompatibleExtractor:
    """**一个实现覆盖所有 OpenAI 兼容端点。**

    已内置：OpenAI / DeepSeek / 通义千问 / 智谱 GLM / Kimi / Ollama / vLLM
    其他兼容端点用 --base-url + --model 直接指定即可。

    两个针对「抽取」场景的刻意选择：
      - `temperature=0`  —— 抽取要可复现。同一张表跑两次，候选应当一致，
        否则 §7.1「基于增量不重新生成」的收敛前提就没了
      - `response_format=json_object`  —— 提高可解析率；但解析层仍做容错兜底
    """

    name = "openai-compatible"
    version = "1.0"

    def __init__(
        self,
        provider: str = "openai",
        base_url: str | None = None,
        model: str | None = None,
        api_key: str | None = None,
        timeout: int = 120,
        max_tokens: int = 4096,
    ):
        preset = OPENAI_COMPATIBLE_PRESETS.get(provider, {})
        self.provider = provider
        self.base_url = (base_url or preset.get("base_url", "")).rstrip("/")
        self.model = model or preset.get("model", "gpt-4o-mini")
        self.api_key = api_key or os.environ.get(preset.get("env", "OPENAI_API_KEY"), "")
        self.timeout = timeout
        self.max_tokens = max_tokens
        if not self.base_url:
            raise ValueError(f"未知 provider {provider!r}，请用 --base-url 指定")

    @property
    def _key_required(self) -> bool:
        """Ollama / vLLM 本地端点通常不需要 key。"""
        return not self.base_url.startswith("http://localhost")

    def extract(self, table: Table, context: dict) -> list[Candidate]:
        try:
            import httpx
        except ImportError as e:
            raise RuntimeError(
                "缺 httpx。请用：uv run --extra openai run.py ..."
            ) from e

        if self._key_required and not self.api_key:
            env = OPENAI_COMPATIBLE_PRESETS.get(self.provider, {}).get(
                "env", "OPENAI_API_KEY"
            )
            raise RuntimeError(f"缺 API key，请设置环境变量 {env}")

        return _parse_candidates(
            self.complete(build_prompt(table, context, context.get("desensitized", []))),
            table.name, f"round{context.get('round', 1)}", f"{self.name}:{self.provider}",
        )

    def complete(self, prompt: str) -> str:
        """任意 prompt → 文本。合成阶段（阶段 3d）也走这里。"""
        try:
            import httpx
        except ImportError as e:
            raise RuntimeError("缺 httpx。请用：uv run --extra openai run.py ...") from e

        if self._key_required and not self.api_key:
            env = OPENAI_COMPATIBLE_PRESETS.get(self.provider, {}).get(
                "env", "OPENAI_API_KEY"
            )
            raise RuntimeError(f"缺 API key，请设置环境变量 {env}")

        payload = {
            "model": self.model,
            "messages": [{"role": "user", "content": prompt}],
            "temperature": 0,          # 抽取/合成都要可复现
            "max_tokens": self.max_tokens,
            "response_format": {"type": "json_object"},
        }
        headers = {"Content-Type": "application/json"}
        if self.api_key:
            headers["Authorization"] = f"Bearer {self.api_key}"

        with httpx.Client(timeout=self.timeout) as c:
            r = c.post(f"{self.base_url}/chat/completions",
                       json=payload, headers=headers)
            r.raise_for_status()
            data = r.json()

        return data["choices"][0]["message"]["content"] or ""


# ──────────────────────────────────────────────────────────
# 实现 C：A2A —— 调外部抽取 agent
# ──────────────────────────────────────────────────────────


class A2AExtractor:
    """通过 A2A 协议调用外部抽取 agent。

    协议：v1.0 稳定版（2026-03，Linux Foundation）。
    这里只做最小可用实现 —— 真实接入时按 Agent Card 协商能力，
    关键是**返回必须能被 parse 成带 evidence 的候选**。
    """

    name = "a2a-external"
    version = "1.0"

    def __init__(self, endpoint: str, agent_id: str, model: str = "external"):
        self.endpoint = endpoint
        self.agent_id = agent_id
        self.model = model

    def extract(self, table: Table, context: dict) -> list[Candidate]:
        try:
            import httpx
        except ImportError as e:
            raise RuntimeError("缺 httpx：pip install httpx") from e

        prompt = build_prompt(table, context, context.get("desensitized", []))
        payload = {
            "jsonrpc": "2.0",
            "id": f"extract-{table.name}",
            "method": "message/send",
            "params": {
                "message": {
                    "role": "user",
                    "parts": [{"kind": "text", "text": prompt}],
                    "contextId": context.get("session_id", "spike"),
                }
            },
        }
        with httpx.Client(timeout=120) as c:
            r = c.post(self.endpoint, json=payload)
            r.raise_for_status()
            data = r.json()

        text = _dig_text(data)
        return _parse_candidates(
            text, table.name, f"round{context.get('round', 1)}", self.name
        )


def _dig_text(data: dict) -> str:
    """从 A2A 响应里挖出文本部分（不同实现位置略有差异）。"""
    result = data.get("result", {})
    for part in result.get("parts", []) or []:
        if part.get("kind") == "text":
            return part.get("text", "")
    msg = result.get("message", {})
    for part in msg.get("parts", []) or []:
        if part.get("kind") == "text":
            return part.get("text", "")
    raise RuntimeError(f"A2A 响应里找不到文本部分：{json.dumps(data, ensure_ascii=False)[:400]}")


# ──────────────────────────────────────────────────────────
# 解析
# ──────────────────────────────────────────────────────────


def _parse_candidates(text: str, table_name: str, rnd: str, agent: str) -> list[Candidate]:
    """把模型/agent 的返回解析成 Candidate。

    刻意**容错**：LLM 经常在 JSON 外面裹 ```json 或加解释文字。
    解析失败返回空列表 —— **不静默伪造**，由调用方把该组标记为抽取失败
    （§7.1 设计点⑤：失败必须显式，否则静默失败会伪造收敛）。
    """
    raw = text.strip()
    if "```" in raw:
        blocks = re.findall(r"```(?:json)?\s*(.+?)```", raw, re.S)
        raw = blocks[0] if blocks else raw
    start, end = raw.find("{"), raw.rfind("}")
    if start < 0 or end <= start:
        return []
    try:
        data = json.loads(raw[start : end + 1])
    except json.JSONDecodeError:
        return []

    out: list[Candidate] = []
    for c in data.get("candidates", []):
        if not c.get("key") or not c.get("display_name"):
            continue
        evidence = [
            Evidence(
                table=e.get("table", table_name),
                column=e.get("column", ""),
                claim=e.get("claim", ""),
            )
            for e in c.get("evidence", [])
            if e.get("column")
        ]
        # 属性里也带 column 的，一并计入证据（agent 常只在这里声明来源）
        for p in c.get("properties", []):
            if p.get("column") and not any(e.column == p["column"] for e in evidence):
                evidence.append(
                    Evidence(table=table_name, column=p["column"], claim=p.get("name", ""))
                )
        out.append(
            Candidate(
                key=c["key"],
                display_name=c["display_name"],
                source_table=table_name,
                aliases=c.get("aliases", []),
                properties=c.get("properties", []),
                evidence=evidence,
                parent=c.get("parent"),
                from_round=int(rnd.replace("round", "") or 1),
                from_agent=agent,
            )
        )
    return out


# ──────────────────────────────────────────────────────────
# 阶段 3d · 组内合成判断（唯一的 LLM 合成环节）
# ──────────────────────────────────────────────────────────

SYNTH_PROMPT = """下面是若干张**结构高度相似**的表，它们很可能是同一个业务对象的不同物理形态（备份表、快照表、报表表）。

但**结构相同不等于语义相同**。请对每一组独立判断。

## 判断要点

- `ip_blacklist` 与 `ip_whitelist` 列几乎一样，但它们是**相反的概念** —— 不能合并
- `llm_provider` 与 `llm_provider_back_20260701` 是**同一张表的历史版本** —— 应该合并
- 表名里的 `_back` / `_archive` / 日期后缀 / `rpt_` 前缀，强烈暗示是同源的另一份
- 若合并，给出**聚合后**的属性清单（跨表去重，并标出哪些列是多表共有的）
- 若不合并，说明它们的**语义差别**是什么，以及彼此是什么关系（兄弟 / 子类）

## 待判断的组

{groups}

## 输出格式

{{
  "decisions": [
    {{
      "tables": ["表1", "表2"],
      "verdict": "merge | separate",
      "reason": "一句话说清为什么",
      "relation": "none | sibling | subtype",
      "parent": "父概念的表名（仅 relation=subtype 时填）",
      "child": "子概念的表名（仅 relation=subtype 时填）",
      "merged_key": "合并后的英文 key（仅 merge 时填）",
      "merged_name": "合并后的中文名（仅 merge 时填）",
      "merged_properties": [
        {{"name": "中文属性名", "column": "列名", "from_tables": ["表1","表2"]}}
      ]
    }}
  ]
}}
"""


def _describe_cluster(cluster) -> str:
    """把一个簇压成紧凑的文本块 —— 上下文有限，说清楚比说全重要。"""
    from core import _clip

    lines = [f"【组：{', '.join(m.source_table for m in cluster.members)}】"]
    for m in cluster.members:
        t = m.source_table
        cols = []
        for p in m.properties:
            col = p.get("column")
            if not col:
                continue
            cols.append(f"{p.get('name', col)}←{col}")
        lines.append(f"  表 {t}")
        lines.append(f"    机器给的 key: {m.key} / 名字: {m.display_name}")
        if cols:
            lines.append(f"    属性: {'、'.join(cols)}")
        if getattr(m, "sample_note", ""):
            lines.append(f"    样本: {m.sample_note}")
    if cluster.signals:
        lines.append(f"  机器给出的同源信号: {'；'.join(cluster.signals[:4])}")
    return "\n".join(lines)


def build_synth_prompt(clusters, scope: str = "") -> str:
    body = SYNTH_PROMPT.format(groups="\n\n".join(_describe_cluster(c) for c in clusters))
    if scope:
        head = f"## 本轮范围限定（决策清单）\n{scope}\n\n---\n\n"
        return head + body
    return body


# ──────────────────────────────────────────────────────────
# 阶段 3e · 跨对象关系（LinkType）—— 回答「这些对象怎么连」
# ──────────────────────────────────────────────────────────

LINK_PROMPT = """下面是本轮抽取出的**业务对象**及其属性。对象本身已经确定，现在需要判断它们之间的**关系**。

## 硬性要求

1. **每条关系必须能用具体的列连起来** —— 在 `from_column` / `to_column` 写出列名。
   这两个列必须**真实存在于**对应对象的属性清单里。**找不到连接列，就不要提这条关系。**
2. 基数 `cardinality`：`1:1` / `1:N` / `N:M`
3. 语义 `semantic`：从这些里选 —— `has`（拥有）/ `belongs_to`（属于）/ `depends_on`（依赖）
   / `references`（引用）/ `records`（记录）/ `derived_from`（由…算出）
4. **只提业务上真实存在的关系**，不要因为两个对象都提到「客户」就连一条
5. `necessary_for_decision`：从本轮决策看，这条关系是不是回答问题**必需**的

## 对象清单

{objects}

## 输出格式

{{
  "links": [
    {{
      "key": "英文蛇形命名",
      "from": "起点对象的 key",
      "to": "终点对象的 key",
      "from_column": "起点对象里做连接的那个属性名",
      "to_column": "终点对象里做连接的那个属性名",
      "cardinality": "1:N",
      "semantic": "has",
      "reason": "一句话说清这条关系在业务上意味着什么",
      "necessary_for_decision": true
    }}
  ]
}}
"""


def build_link_prompt(clusters, scope: str = "", key_to_name: dict | None = None) -> str:
    key_to_name = key_to_name or {}
    blocks: list[str] = []
    for c in clusters:
        props = c.merged_properties
        lines = [f"- key `{c.primary.key}`（{c.primary.display_name}）"
                 f"，来自表 {', '.join(c.tables)}"]
        cols = "、".join(p["name"] for p in props[:24])
        lines.append(f"    属性：{cols or '（无）'}")
        blocks.append("\n".join(lines))
    head = f"## 本轮决策\n{scope}\n\n---\n\n" if scope else ""
    return head + LINK_PROMPT.format(objects="\n".join(blocks))

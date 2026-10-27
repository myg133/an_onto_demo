#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""access_fetch.py —— 按 access mapping（AM）把 API 源的数据取回来。

为什么有这个脚本
----------------
AM 是**本体 ↔ 取数方式的声明式映射**，它由人签（`reviewed_by`）。`AUTH-SPEC.md`
讲签名算法，`RECIPE.md` 讲 `get` 配方的每一键怎么读 —— 两份都是**给客户端实现者
看**的规格：每个数字员工照它实现一遍，就多一份实现，而「签名 / 分页 / 剪枝」正是
最容易写错的三件事（实测 9 次错误里 6 次出在这里）。
本脚本是那份实现的**唯一一份**：你只传 AM 路径 + entity，别自己再写一遍。

本脚本**只按 AM 契约办事**：接口、参数、路径、分页、签名器全都来自 AM，
一处都不猜。加一种鉴权算法 = 往 `SIGNERS` 注册表加一个签名器，**主流程一行不改**
（`--list-signers` 看当前支持哪些）。

用法
----
    # ① 体检：AM 结构对不对、entity 有没有签、fixture 在不在（不取数）
    python access_fetch.py --mapping am.json --check

    # ② 试跑：用 entity.fixture 走完整流程，不发网络、不要凭据
    python access_fetch.py --mapping am.json --entity <entity id> --dry-run

    # ③ 真取数：凭据只从 auth.credential_env 命名的环境变量读
    python access_fetch.py --mapping am.json --entity <entity id> \
        --param <参数名>=<值> --limit 100 --max-rows 500 --out ./rows.json

    # ④ 离线自检：不联网、不要凭据
    python access_fetch.py --selftest

    # ⑤ 看当前注册了哪些签名器
    python access_fetch.py --list-signers

退出码（码 → 退出码的唯一一份表在 `CODE_EXIT`）
----------------------------------------------
    0  成功且取全
    2  am.invalid / am.unsigned / param.not_pushdown
    3  credential.missing
    4  auth.rejected / auth.unsupported（auth.mode 没有对应签名器，⛔ 不 fallback）
    5  api.throttled
    6  api.invalid
    7  net.timeout / net.error
    8  rows.truncated —— **数据可用，但没取全**（看 meta.truncated）

设计约束
--------
* Python 3.8+ **标准库**，零第三方依赖（`requests` / `urllib3` 一律不许）。
* `contract_version` 只认白名单 `SUPPORTED_CONTRACT_VERSIONS`（当前 `1`、`2`）：
  在名单里就按那个版本的形状读，不在就 `am.invalid` + **列出支持的版本** + 退出 2，
  ⛔ 不「尽力解析」、⛔ 不按最新版硬读。
* 凭据**只**从 `auth.credential_env` 命名的环境变量读，⛔ 绝不回显。
* `--param` 只接受 `capability.pushdown` 里的名字。
* 未签 entity（`reviewed_by` 为空）一律拒绝。
* 取不全必须如实说：`meta.truncated=true` + 退出码 8，⛔ 不许静默截断。
* 自建 JSONPath 子集（`$.a.b[*]` / `$.a.b`），⛔ 不引 jsonpath 库。
* 每个签名器**自带默认值**（signed_headers / timestamp / scope / content_type /
  service）；主流程里不出现任何一家厂商的默认值。

参考规格：`AUTH-SPEC.md`（签名）、`RECIPE.md`（模板/剪枝/分页）、`AM-FIELDS.md`（键义）。
"""

import argparse
import ast
import hashlib
import hmac
import io
import json
import os
import re
import socket
import sys
import time
import traceback
import urllib.error
import urllib.parse
import urllib.request

# --------------------------------------------------------------------------- #
# 常量
# --------------------------------------------------------------------------- #

EXIT_OK = 0
EXIT_INPUT = 2       # AM/未签/参数不合法
EXIT_CREDENTIAL = 3  # 环境变量没设
EXIT_AUTH = 4        # 401/403/签名错
EXIT_THROTTLED = 5   # 429 / RequestLimitExceeded
EXIT_API = 6         # 400 参数错
EXIT_NETWORK = 7     # 超时/连不上
EXIT_TRUNCATED = 8   # 没取全

#: 本脚本支持的 AM `contract_version` **白名单**（判定的唯一依据；不在里面就拒）。
#:
#: * **v1** —— 原始契约：`auth` 只声明 `mode` + `credential_env`；签名四件套
#:   （`signed_headers` / `timestamp` / `scope` / `content_type`）由**签名器默认值**兜底。
#: * **v2** —— `auth` 可**显式**声明 `signed_headers` / `timestamp` / `scope`（+ `content_type`）：
#:   签名输入完全由契约决定；缺的仍回落到签名器默认值。
#:   显式值与签名器默认值必须签出**逐字节相同**的 `Authorization`（`--selftest` 对拍）。
#:
#: ⚠️ 版本决定的是「契约**期望**什么」，不是「脚本读什么」：两个版本下脚本读的键相同，
#: 所以加一个版本**不需要**在读取逻辑里分支。3 及以上一律拒绝（⛔ 不「尽力解析」）。
SUPPORTED_CONTRACT_VERSIONS = (1, 2)

#: 每个支持版本的一句话形状（`--check` 打印；`--selftest` 校验它和白名单不漂）。
CONTRACT_VERSION_NOTES = {
    1: "v1 —— 原始契约：`auth` 只声明 mode + credential_env，签名四件套由签名器默认值兜底",
    2: "v2 —— `auth` 可显式声明 signed_headers / timestamp / scope（+ content_type）",
}

#: 兼容别名：= **最新一版**。仅为既有引用保留；**任何判断都必须用白名单**
#: （写成 `version != CONTRACT_VERSION` 就等于又把旧版本拒掉了）。
CONTRACT_VERSION = SUPPORTED_CONTRACT_VERSIONS[-1]

#: 脚本在 `auth` 段**实际会读**的键（两版相同）。`--check` 打印它，让「版本差异」可见。
AUTH_KEYS_REQUIRED_BY_SIGNER = "mode · credential_env（两个变量名） · service"
AUTH_KEYS_DEFAULTABLE = "signed_headers · timestamp.header/.format · scope · content_type"

DEFAULT_MAX_ROWS = 1000       # --max-rows 缺省
DEFAULT_TIMEOUT = 60          # 单次请求超时（秒）
MAX_PAGES_HARD_CAP = 1000     # 分页保险丝：再多就停（并标记 truncated）
JSONLD_CONTEXT = {"ontok": "https://ontok.dev/ns#"}

#: 内置夹具目录：**按脚本自身位置**解析，⛔ 不看 cwd。
#: 数字员工常在别的目录里跑本脚本（`python <skill目录>/ontok-query/access_fetch.py …`），
#: 用 cwd 找 `fixtures/` 会让 `--selftest` 假红。
#: 内置夹具目录。脚本可能被放在 `scripts/` 下（推荐布局）也可能与 SKILL.md 同级，
#: 所以按「脚本旁边 → 上一级」两处找，⛔ 不看 cwd。
FIXTURES_DIR = next(
    (d for d in (
        os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures"),
        os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "fixtures"),
    ) if os.path.isdir(d)),
    os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures"),
)

#: 码 → 退出码（**唯一一份**）。加码只加在这里；既有 9 个码的值一位不动。
CODE_EXIT = {
    "am.invalid": EXIT_INPUT,           # 2  AM 结构/版本/字段不对
    "am.unsigned": EXIT_INPUT,          # 2  entity 未签
    "param.not_pushdown": EXIT_INPUT,   # 2  参数不在 capability.pushdown 白名单
    "credential.missing": EXIT_CREDENTIAL,  # 3  环境变量没设
    "auth.rejected": EXIT_AUTH,         # 4  401/403/签名错
    "auth.unsupported": EXIT_AUTH,      # 4  auth.mode 没有对应签名器（⛔ 不 fallback）
    "api.throttled": EXIT_THROTTLED,    # 5  429 / 限流
    "api.invalid": EXIT_API,            # 6  400 参数错
    "net.timeout": EXIT_NETWORK,        # 7  超时
    "net.error": EXIT_NETWORK,          # 7  连不上 / 5xx
    "rows.truncated": EXIT_TRUNCATED,   # 8  没取全（数据可用）
}

#: 缺失哨兵：`MISSING` 表示「这个位置没有值」，与 JSON 的 `null` 严格区分。
MISSING = object()

#: 已读到的凭据值，用于把任何输出/异常里的明文抹掉（AC-005）。
_SECRET_VALUES = []

#: 占位符：整串只有一个 `{name}` 才算（拼在中间的一律拒绝，见 RECIPE §三）。
_PLACEHOLDER_RE = re.compile(r"^\{([A-Za-z_][A-Za-z0-9_]*)\}$")
_ANY_PLACEHOLDER_RE = re.compile(r"\{[A-Za-z_][A-Za-z0-9_]*\}")


# --------------------------------------------------------------------------- #
# 错误
# --------------------------------------------------------------------------- #

class FetchError(Exception):
    """一个「有码、有退出码、有修法」的失败。

    所有对外失败都必须用它（不要 `raise ValueError`）：码与退出码固定，
    消息面向人（说清发生了什么 + 怎么修），且**不含凭据**。
    """

    def __init__(self, code, exit_code, message, fix=None, extra=None):
        # type: (str, int, str, str, dict) -> None
        Exception.__init__(self, message)
        self.code = code
        self.exit_code = exit_code
        self.message = message
        self.fix = fix
        self.extra = extra or {}

    def rendered(self):
        """人读的完整文本：`[码] 发生了什么` + `修：怎么修`。"""
        text = "[%s] %s" % (self.code, self.message)
        if self.fix:
            text += "\n修：%s" % self.fix
        return redact(text)


def redact(text):
    """把凭据明文从任意文本里抹掉（长度 < 4 的值不处理，避免误伤）。"""
    out = str(text)
    for secret in _SECRET_VALUES:
        if secret and len(secret) >= 4:
            out = out.replace(secret, "***<redacted>***")
    return out


def fail(code, exit_code, message, fix=None, extra=None):
    """抛一个 FetchError（写起来比 `raise FetchError(...)` 短）。"""
    raise FetchError(code, exit_code, message, fix, extra)


def fail_code(code, message, fix=None, extra=None):
    """按 `CODE_EXIT` 查退出码再抛错 —— 码与退出码不会各写一遍、漂开。"""
    raise FetchError(code, CODE_EXIT.get(code, EXIT_INPUT), message, fix, extra)


# --------------------------------------------------------------------------- #
# 小工具
# --------------------------------------------------------------------------- #

def read_json_file(path, what):
    """读一个 UTF-8 JSON 文件；读不了/不是 JSON 都给「怎么修」的话。"""
    try:
        with io.open(path, "r", encoding="utf-8") as fh:
            return json.load(fh)
    except FileNotFoundError:
        fail("am.invalid", EXIT_INPUT, "%s 不存在：%s" % (what, path),
             "确认路径；AM 用绝对路径或少踩 `cd` 的坑（SKILL.md §4.4③）。")
    except UnicodeDecodeError as exc:
        fail("am.invalid", EXIT_INPUT, "%s 不是 UTF-8：%s（%s）" % (what, path, exc),
             "另存为 UTF-8（无 BOM）再试。")
    except ValueError as exc:
        fail("am.invalid", EXIT_INPUT, "%s 不是合法 JSON：%s（%s）" % (what, path, exc),
             "先 `python -m json.tool %s` 看第几行坏，再修。" % path)
    return None


def sha256_hex(data):
    """SHA256 的小写十六进制（签名链全程用它）。"""
    return hashlib.sha256(data).hexdigest()


def hmac_sha256(key, msg):
    """HMAC-SHA256 的裸 digest 字节（派生密钥链用）。"""
    return hmac.new(key, msg.encode("utf-8"), hashlib.sha256).digest()


def dump_body(obj):
    """请求体的**唯一**序列化方式：紧凑、中文不转义。

    ⚠️ 参与签名的必须是**这串字节**（AUTH-SPEC §3.2 的 HashedRequestPayload）。
    换一种 dumps 参数 = 换一份字节 = 签名不匹配。
    """
    return json.dumps(obj, ensure_ascii=False, separators=(",", ":"))


def text_of(value):
    """把 JSON 标量转成 header 里能用的字符串（bool 走小写）。"""
    if isinstance(value, bool):
        return "true" if value else "false"
    return str(value)


# --------------------------------------------------------------------------- #
# JSONPath 子集（自写，约 40 行；⛔ 不引库）
# --------------------------------------------------------------------------- #

class JsonPath(object):
    """`$.a.b[*]`（取数组）或 `$.a.b`（取值）——**只有这两种形状**。

    不做：数组下标（`[0]`）、通配（`[*]` 出现在中间）、递归下降（`..`）、
    过滤器。遇到就报 `am.invalid` 并说清「哪里不支持、该改成什么」——
    宁可拒绝，也不静默取错。
    """

    def __init__(self, expr, segments, many):
        self.expr = expr
        self.segments = segments
        self.many = many

    def resolve(self, doc):
        """沿路径走；中途断了返回 `MISSING`（而不是抛错：缺字段很常见）。"""
        cur = doc
        for seg in self.segments:
            if not isinstance(cur, dict) or seg not in cur:
                return MISSING
            cur = cur[seg]
        if self.many:
            if not isinstance(cur, list):
                return MISSING
            return cur
        return cur

    def __str__(self):
        return self.expr


def parse_jsonpath(expr, where):
    """解析 JSONPath 子集；`where` 用于报错时点名（如 `entity X 的 iterator`）。"""
    if not isinstance(expr, str) or not expr.startswith("$"):
        fail("am.invalid", EXIT_INPUT,
             "%s 的 JSONPath 必须从 `$` 开始，现在是：%r" % (where, expr),
             "形状只有两种：`$.a.b[*]`（数组）或 `$.a.b`（单值）。")
    body = expr[1:]
    many = False
    if body.endswith("[*]"):
        many = True
        body = body[:-3]
    if body.startswith("."):
        body = body[1:]
    if not body:
        fail("am.invalid", EXIT_INPUT,
             "%s 的 JSONPath 只有 `$`：%r" % (where, expr),
             "至少给一段：`$.<字段>.<数组>[*]`。")
    segments = body.split(".")
    for seg in segments:
        if (not seg) or ("[" in seg) or ("]" in seg) or ("*" in seg) or ("$" in seg):
            fail("am.invalid", EXIT_INPUT,
                 "%s 的 JSONPath 用了不支持的写法：%r（问题段：%r）" % (where, expr, seg),
                 "本脚本只支持 `$.a.b[*]` 与 `$.a.b`：不要数组下标 `[0]`、"
                 "不要把 `[*]` 放中间、不要 `..`、不要过滤器。"
                 "真要支持，改 AM 让路径落在这个子集里。")
    return JsonPath(expr, segments, many)


def dig_path(obj, dotted, where="path"):
    """按点号路径取值（`fields.<属性>.path` 用）；取不到返回 `MISSING`。

    RECIPE §六：路径**不支持数组下标**；某一行缺这条路径 ⇒ 该属性当「没有值」，
    ⛔ 不要因此丢掉整行。
    """
    if not isinstance(dotted, str) or not dotted:
        fail("am.invalid", EXIT_INPUT, "%s 为空" % where,
             "每条 `fields.<属性>.path` 都要一个非空字符串，"
             "如 \"<记录里的键名>\" 或 \"<外层>.<内层>\"。")
    cur = obj
    for seg in dotted.split("."):
        if not seg or "[" in seg or "]" in seg:
            fail("am.invalid", EXIT_INPUT,
                 "%s 用了不支持的写法：%r" % (where, dotted),
                 "字段路径只做「点号逐层下钻」，不支持数组下标（RECIPE §六）。")
        if isinstance(cur, dict) and seg in cur:
            cur = cur[seg]
            continue
        return MISSING
    return cur


# --------------------------------------------------------------------------- #
# AM：加载与校验
# --------------------------------------------------------------------------- #

class CheckReport(object):
    """`--check` 的检查结果：errors 阻断，warnings 只提示。"""

    def __init__(self):
        self.errors = []    # [(code, message)]
        self.warnings = []  # [str]
        self.sources = []   # 已校验通过的 source dict
        self.entities = []  # 已校验通过的 entity dict
        self.unsigned = []  # 未签 entity id
        self.auth_defaults = []  # 用了 TC3 默认值的 source id

    def error(self, code, message):
        self.errors.append((code, message))

    def warn(self, message):
        self.warnings.append(message)

    @property
    def ok(self):
        return not self.errors


def load_am(path):
    """读 AM；顺带挡住「读进来不是对象」这种早期错误。"""
    am = read_json_file(path, "AM 文件")
    if not isinstance(am, dict):
        fail("am.invalid", EXIT_INPUT, "AM 顶层必须是 JSON 对象，实际是 %s" % type(am).__name__,
             "照 AM 契约的形状：{contract_version, sources[], entities[]}"
             "（逐键说明见 AM-FIELDS.md）。")
    return am


def am_from_recipe(recipe, source_label="<recipe>"):
    """把服务端 `@pending.get` 给的那一段配方，合成一份**最小 AM**。

    为什么需要：`query serve` 在 v2 契约下，`@pending[].get` 已经含
    `source` / `operation` / `request` / `iterator` / `fields` / `auth` —— 取数所需的一切。
    数字员工如果手上一份 AM 文件都没有，**不必去找：查一次本体就有了**。
    本函数只做「配方 → AM 外壳」这一层，之后走完全相同的取数流程。

    ⚠️ 两处如实标注（⛔ 不假装）：
      · `reviewed_by`：配方由服务端发出，而服务端只对**已签**条目发配方 ⇒ 标注来源，不再要求本地签名字段
      · `identity` / `iri_template`：配方里**没有**（那是本体侧信息）⇒ 用 `--id-property` 指定，
        不给就只能 `--format rows`（⛔ 不瞎猜主键）
    """
    if not isinstance(recipe, dict):
        fail("am.invalid", EXIT_INPUT, "配方必须是 JSON 对象，实际是 %s" % type(recipe).__name__,
             "把 `@pending[0]` 或它的 `get` 子对象整个传进来。")
    entry = recipe.get("get") if isinstance(recipe.get("get"), dict) else recipe
    cls = entry.get("class")
    if not cls:
        fail("am.invalid", EXIT_INPUT, "配方里缺 `class`（该取哪个类）",
             "确认传的是 `@pending[].get` 而不是别的字段。")
    src = entry.get("source") or {}
    base_url = src.get("base_url")
    if not base_url:
        fail("am.invalid", EXIT_INPUT, "配方里缺 `source.base_url`（打哪个接口）",
             "该部署可能用的是 **v1 契约**：v1 的 `@pending.get` 只有 class/filter/pushdown/ref，"
             "不含取数配方 ⇒ 这种部署下必须向调用方要 AM 文件。")
    ref = entry.get("ref") or ""
    entity_id = ref.split("#")[-1].split("@")[0] or cls
    auth = dict(entry.get("auth") or {})
    if src.get("auth_mode"):
        auth.setdefault("mode", src["auth_mode"])
    entity = {
        "id": entity_id,
        "class": cls,
        "source": "pending-recipe",
        "reviewed_by": "server:%s" % (ref or "pending"),
        "operation": entry.get("operation") or {},
        "request": entry.get("request") or {},
        "iterator": entry.get("iterator") or "$[*]",
        # 配方里的 fields 是 {属性: "路径字符串"}；AM 契约里是 {属性: {"path": "..."}}
        # —— 这里归一化成 AM 的形状（两形状都接受，避免因写法不同而拒）
        "fields": {
            k: (v if isinstance(v, dict) else {"path": v})
            for k, v in (entry.get("fields") or {}).items()
        },
        "iri_template": "ontok:%s/{id}" % cls,
        "capability": {"pushdown": entry.get("pushdown") or []},
    }
    return {
        "contract_version": 2,
        "mapping_version": 1,
        "sources": [{
            "id": "pending-recipe",
            "kind": "http-json",
            "base_url": base_url,
            "auth": auth,
            "defaults": (entry.get("request") or {}).get("defaults") or {},
        }],
        "entities": [entity],
    }


def supported_versions_text():
    """支持列表拼成一句话（`--check` 与错误消息共用，不各写一遍）。"""
    return ", ".join(str(v) for v in SUPPORTED_CONTRACT_VERSIONS)


def contract_version_problem(version):
    """`contract_version` 在白名单里就返回 None，否则返回一句「支持哪些」。

    ⛔ 这里**没有**「尽力解析」：不认识就是不认识，绝不按最新版硬读
    （按错的形状读一份 AM，比直接拒绝更危险 —— 它会安静地取错数）。
    """
    if isinstance(version, bool) or not isinstance(version, int):
        # 顺带挡住 `true`：Python 里 `True == 1`，不显式挡掉它会被当成 v1。
        return ("contract_version 必须是整数，实际是 %r（%s）；本脚本支持：%s"
                % (version, type(version).__name__, supported_versions_text()))
    if version not in SUPPORTED_CONTRACT_VERSIONS:
        return ("contract_version=%d，本脚本不支持；支持：%s"
                "（不认识的值一律拒绝，⛔ 不「尽力解析」）"
                % (version, supported_versions_text()))
    return None


def _require(obj, key, where, report, kind=None):
    """取必填键；缺了就记一条 error 并返回 None（好让 --check 一次报完）。"""
    if not isinstance(obj, dict) or key not in obj:
        report.error("am.invalid", "%s 缺必填键 `%s`" % (where, key))
        return None
    value = obj[key]
    if kind is not None and not isinstance(value, kind):
        report.error("am.invalid", "%s 的 `%s` 类型不对（要 %s，实际 %s）"
                     % (where, key, getattr(kind, "__name__", str(kind)), type(value).__name__))
        return None
    return value


def validate_am(am, check_fixtures=True, am_path=None):
    """按 demand §4.2 第 1 步校验 AM 结构，返回 CheckReport。

    校验的是**脚本真正要用到的部分**：契约版本、source 引用、entity 是否已签、
    iterator/fields/paging 的写法是否落在支持集里。结构没问题但只是「不好用」的
    记 warning（例如 fixture 文件不在、auth 缺声明）。
    """
    if am_path is None:
        am_path = getattr(am, "_path", None)
    report = CheckReport()
    if not isinstance(am, dict):
        report.error("am.invalid", "AM 顶层必须是 JSON 对象")
        return report

    version = am.get("contract_version")
    version_problem = contract_version_problem(version)
    if version_problem:
        report.error("am.invalid", version_problem)
    mapping_version = am.get("mapping_version")
    if mapping_version is not None and not isinstance(mapping_version, int):
        report.error("am.invalid", "mapping_version 必须是整数，实际 %r" % (mapping_version,))

    sources = _require(am, "sources", "AM", report, list)
    entities = _require(am, "entities", "AM", report, list)

    # ---- sources ----
    by_source = {}
    for idx, src in enumerate(sources or []):
        where = "sources[%d]" % idx
        if not isinstance(src, dict):
            report.error("am.invalid", "%s 必须是对象" % where)
            continue
        sid = _require(src, "id", where, report, str)
        kind = _require(src, "kind", where, report, str)
        base_url = _require(src, "base_url", where, report, str)
        auth = _require(src, "auth", where, report, dict)
        if not sid:
            continue
        if sid in by_source:
            report.error("am.invalid", "source id 重复：`%s`" % sid)
            continue
        by_source[sid] = src
        where = "source `%s`" % sid
        if kind and kind != "http-json":
            report.error("am.invalid",
                         "%s 的 kind=%r；本脚本只支持 http-json" % (where, kind))
        if base_url and not base_url.startswith(("http://", "https://")):
            report.error("am.invalid",
                         "%s 的 base_url 不是 http(s) 地址：%r" % (where, base_url))
        envs = None
        if auth is not None:
            envs = auth.get("credential_env")
            mode = auth.get("mode")
            signer = None
            if not isinstance(mode, str) or not mode.strip():
                report.error("am.invalid",
                             "%s 的 auth 段缺 `auth.mode`（必填；已支持：%s）"
                             % (where, signers_text()))
            else:
                signer = SIGNERS.get(mode.strip())
                if signer is None:
                    report.error("auth.unsupported",
                                 "%s 的 auth.mode=%r 没有对应的签名器；已支持：%s"
                                 % (where, mode, signers_text()))
            if not isinstance(envs, list) or len(envs) < 2 or \
                    not all(isinstance(x, str) and x for x in envs):
                report.error("am.invalid",
                             "%s 的 auth.credential_env 必须是两个非空变量名"
                             "（第 1 个放凭据 ID、第 2 个放凭据密钥）" % where)
                envs = None
            if signer is not None:
                for key in signer.REQUIRED_AUTH_KEYS:
                    value = auth.get(key)
                    if not isinstance(value, str) or not value.strip():
                        report.error("am.invalid",
                                     "%s 的 `auth.%s` 缺失 —— `%s` 签名器要求显式声明"
                                     "（⛔ 不从 base_url / host 推断）。补：%s"
                                     % (where, key, signer.MODE,
                                        signer.auth_key_hint(key)))
                missing = [k for k in signer.DEFAULTABLE_AUTH_KEYS if not auth.get(k)]
                if missing:
                    report.auth_defaults.append({
                        "id": sid,
                        "mode": signer.MODE,
                        "missing": missing,
                        "defaults": signer.describe_defaults(),
                    })
        report.sources.append(src)

    # ---- entities ----
    seen_ids = set()
    for idx, ent in enumerate(entities or []):
        where = "entities[%d]" % idx
        if not isinstance(ent, dict):
            report.error("am.invalid", "%s 必须是对象" % where)
            continue
        eid = _require(ent, "id", where, report, str)
        if eid:
            where = "entity `%s`" % eid
            if eid in seen_ids:
                report.error("am.invalid", "entity id 重复：`%s`" % eid)
                continue
            seen_ids.add(eid)
        cls = _require(ent, "class", where, report, str)
        src_id = _require(ent, "source", where, report, str)
        if src_id and by_source and src_id not in by_source:
            report.error("am.invalid",
                         "%s 引用了不存在的 source `%s`；已有：%s"
                         % (where, src_id, ", ".join(sorted(by_source)) or "（无）"))
        if "reviewed_by" not in ent:
            report.error("am.unsigned",
                         "%s 缺 reviewed_by —— 未签的 entity 不许取数（R6）" % where)
            report.unsigned.append(eid or where)
        else:
            reviewer = ent.get("reviewed_by")
            if not isinstance(reviewer, str) or not reviewer.strip():
                report.error("am.unsigned",
                             "%s 的 reviewed_by 为空 —— 未签的 entity 不许取数（R6）" % where)
                report.unsigned.append(eid or where)

        op = ent.get("operation")
        if not isinstance(op, dict):
            report.error("am.invalid", "%s 缺 `operation`（含 action/version）" % where)
        else:
            for key in ("action", "version"):
                _require(op, key, "%s.operation" % where, report, str)
            if op.get("method") and str(op["method"]).upper() != "POST":
                report.error("am.invalid",
                             "%s.operation.method=%r；本脚本只实现 POST"
                             % (where, op.get("method")))

        iri = _require(ent, "iri_template", where, report, str)
        if iri and "{id}" not in iri:
            report.error("am.invalid",
                         "%s 的 iri_template 里没有 `{id}`：%r" % (where, iri))

        identity = ent.get("identity")
        if not isinstance(identity, dict) or not identity.get("key_path"):
            report.warn("%s 没有 `identity.key_path`：取数时拿不到 @id（会被拒绝）" % where)

        if "iterator" not in ent:
            report.warn("%s 没有 `iterator`：真取数时会拒绝（拿不到「一行」的路径）" % where)
        else:
            try:
                path = parse_jsonpath(ent["iterator"], "%s.iterator" % where)
                if not path.many:
                    report.warn("%s 的 iterator 不是数组路径（缺 `[*]`）：%s"
                                % (where, ent["iterator"]))
            except FetchError as exc:
                report.error(exc.code, exc.message)

        fields = ent.get("fields")
        if not isinstance(fields, dict) or not fields:
            report.warn("%s 没有 `fields`：取回来的行不会带属性" % where)
        else:
            for prop, spec in fields.items():
                if not isinstance(spec, dict) or "path" not in spec:
                    report.error("am.invalid",
                                 "%s.fields.%s 必须是 {\"path\": \"...\"}" % (where, prop))
                    continue
                try:
                    dig_path({}, spec["path"], "%s.fields.%s.path" % (where, prop))
                except FetchError as exc:
                    report.error(exc.code, exc.message)

        paging = ent.get("paging")
        if paging is not None:
            if not isinstance(paging, dict):
                report.error("am.invalid", "%s 的 paging 必须是对象或 null" % where)
            else:
                for key in ("offset_param", "limit_param"):
                    if paging.get(key) is not None and not isinstance(paging[key], str):
                        report.error("am.invalid",
                                     "%s.paging.%s 必须是字符串" % (where, key))
                mx = paging.get("max_limit")
                if mx is not None and (not isinstance(mx, int) or mx <= 0):
                    report.error("am.invalid",
                                 "%s.paging.max_limit 必须是正整数，实际 %r" % (where, mx))
                total_path = paging.get("total_path")
                if total_path:
                    try:
                        parse_jsonpath(total_path, "%s.paging.total_path" % where)
                    except FetchError as exc:
                        report.error(exc.code, exc.message)

        req = ent.get("request")
        if req is not None and not isinstance(req, dict):
            report.error("am.invalid", "%s 的 request 必须是对象" % where)
        elif isinstance(req, dict):
            if not isinstance(req.get("template", {}), dict):
                report.error("am.invalid", "%s.request.template 必须是对象" % where)
            if not isinstance(req.get("bindings", {}), dict):
                report.error("am.invalid", "%s.request.bindings 必须是对象" % where)
            required = req.get("required", [])
            if not isinstance(required, list) or \
                    not all(isinstance(x, str) for x in required):
                report.error("am.invalid",
                             "%s.request.required 必须是字符串数组" % where)
            bindings = req.get("bindings", {})
            if isinstance(bindings, dict):
                for name, spec in bindings.items():
                    if not isinstance(spec, dict) or "source" not in spec:
                        report.error("am.invalid",
                                     "%s.request.bindings.%s 必须是 {\"source\": ...}"
                                     % (where, name))
                        continue
                    if spec.get("source") not in ("default", "filter", "paging"):
                        report.error("am.invalid",
                                     "%s.request.bindings.%s.source=%r 不认识"
                                     "（只认 default / filter / paging）"
                                     % (where, name, spec.get("source")))

        cap = ent.get("capability")
        if cap is not None and not isinstance(cap, dict):
            report.error("am.invalid", "%s 的 capability 必须是对象" % where)
        elif isinstance(cap, dict):
            pushdown = cap.get("pushdown")
            if pushdown is not None and (
                    not isinstance(pushdown, list)
                    or not all(isinstance(x, str) and x for x in pushdown)):
                report.error("am.invalid",
                             "%s.capability.pushdown 必须是字符串数组" % where)
            bindings = (req or {}).get("bindings") or {}
            for name in (pushdown or []):
                if bindings and name.lower() not in {k.lower() for k in bindings}:
                    report.warn("%s 的 pushdown 里有 `%s`，但 request.bindings 没有它 —— "
                                "这个参数没有安放位置（AM 不一致）" % (where, name))

        fixture = ent.get("fixture")
        if fixture is not None and not isinstance(fixture, str):
            report.error("am.invalid", "%s 的 fixture 必须是路径字符串" % where)
        elif fixture and check_fixtures:
            path = resolve_fixture_path(fixture, am_path)
            if not os.path.isfile(path):
                report.warn("%s 的 fixture 文件不存在：%s（--dry-run 会失败）"
                            % (where, path))
        report.entities.append(ent)

    if not (entities or []):
        report.error("am.invalid", "AM 里没有任何 entity")
    if not (sources or []):
        report.error("am.invalid", "AM 里没有任何 source")
    return report


def first_error(report):
    """把报告的第一条 error 变成 FetchError（取数路径用：先拦住再说）。

    退出码取自 `CODE_EXIT`（与 `--check` 同一张表），所以两边不会漂。
    """
    code, message = report.errors[0]
    exit_code = CODE_EXIT.get(code, EXIT_INPUT)
    hint = None
    if code == "am.unsigned":
        hint = ("让映射作者在 AM 里补 `reviewed_by`（形如 `name@YYYY-MM-DD`），"
                "或换一个已签的 entity（`--check` 会列出哪些已签）。")
    elif code == "auth.unsupported":
        hint = ("换一个已支持的 `auth.mode`（`--list-signers` 可查），"
                "或者给这个 mode 注册一个签名器（`SIGNERS` 里加一行）；"
                "⛔ 本脚本不会退回到别的算法去签。")
    elif code == "am.invalid":
        hint = "按消息点名的那一处改 AM；改完先跑 `--check`。"
    extra = {"errors": report.errors}
    return FetchError(code, exit_code, message, hint, extra)


def find_source(am, source_id):
    """按 id 找 source；找不到就点名已有 id（写错时最能省时间的一句）。"""
    for src in am.get("sources") or []:
        if src.get("id") == source_id:
            return src
    fail("am.invalid", EXIT_INPUT,
         "找不到 source `%s`（entity 的 `source` 指向它）" % source_id,
         "已有 source：%s。改 entity.source，或把 source 补进 AM。"
         % (", ".join(sorted(s.get("id", "?") for s in am.get("sources") or [])) or "（无）"))


def find_entity(am, ref):
    """`--entity` 接受 entity id 或本体 class（先精确、再大小写不敏感）。"""
    entities = [e for e in (am.get("entities") or []) if isinstance(e, dict)]
    for ent in entities:
        if ent.get("id") == ref:
            return ent
    for ent in entities:
        if ent.get("class") == ref:
            return ent
    low = str(ref).lower()
    for ent in entities:
        if str(ent.get("id", "")).lower() == low or str(ent.get("class", "")).lower() == low:
            return ent
    fail("am.invalid", EXIT_INPUT,
         "AM 里没有 entity `%s`（也不存在同名 class）" % ref,
         "可用的 entity：%s" % (", ".join(e.get("id", "?") for e in entities) or "（无）"))


def entity_is_signed(entity):
    """已签 = `reviewed_by` 是非空字符串（与 R6 同一条纪律）。"""
    reviewer = entity.get("reviewed_by")
    return isinstance(reviewer, str) and bool(reviewer.strip())


def resolve_fixture_path(fixture, am_path):
    """fixture 路径按 **AM 所在目录** 解析（AM 常和 fixtures/ 一起分发）。"""
    if os.path.isabs(fixture):
        return fixture
    base = os.path.dirname(os.path.abspath(am_path)) if am_path else os.getcwd()
    return os.path.normpath(os.path.join(base, fixture))


# --------------------------------------------------------------------------- #
# 凭据（只从环境变量读）
# --------------------------------------------------------------------------- #

def load_credentials(source):
    """从 `auth.credential_env` **命名**的环境变量读 SecretId / SecretKey。

    第 1 个变量 = SecretId，第 2 个 = SecretKey（AUTH-SPEC §5：这条对应关系由
    算法规定，契约只给变量名）。缺任何一个都报 `credential.missing` 并**点名**，
    ⛔ 不打印、不记录、不回显值。
    """
    auth = source.get("auth") or {}
    envs = auth.get("credential_env") or []
    if len(envs) < 2:
        fail("am.invalid", EXIT_INPUT,
             "source `%s` 的 auth.credential_env 少于两个变量名" % source.get("id"),
             "写成 [\"<SecretId 的变量名>\", \"<SecretKey 的变量名>\"]。")
    sid_env, key_env = envs[0], envs[1]
    missing = [name for name in (sid_env, key_env) if not os.environ.get(name)]
    if missing:
        fail("credential.missing", EXIT_CREDENTIAL,
             "环境变量没设：%s（source `%s` 的 auth.credential_env 要求它们）"
             % ("、".join(missing), source.get("id")),
             "在当前 shell 里 export 一次即可（只用变量名，⛔ 不要把值写进命令行/"
             "脚本/AM）：`export %s=... %s=...`；之后 ⛔ 不要 `echo` 它们。"
             % (sid_env, key_env))
    sid = os.environ.get(sid_env, "")
    key = os.environ.get(key_env, "")
    # 记下来供 redact() 用：万一某条异常消息里带了值，也会被抹掉。
    for value in (sid, key):
        if value and value not in _SECRET_VALUES:
            _SECRET_VALUES.append(value)
    return sid, key


# --------------------------------------------------------------------------- #
# 请求体：值表 + 剪枝（RECIPE §二 / §三）
# --------------------------------------------------------------------------- #

def _defaults_of(source, entity):
    """合并默认值：`sources[].defaults`（契约 v1）与 `request.defaults`（v2）。"""
    merged = {}
    src_defaults = (source or {}).get("defaults") or {}
    req_defaults = (entity.get("request") or {}).get("defaults") or {}
    if isinstance(src_defaults, dict):
        merged.update(src_defaults)
    if isinstance(req_defaults, dict):
        merged.update(req_defaults)
    return merged


def _lookup_default(defaults, name):
    """先精确、再大小写不敏感（RECIPE §二.2：`Region` 配 `{region}` 是正常的）。"""
    if name in defaults:
        return defaults[name]
    low = name.lower()
    for key, value in defaults.items():
        if str(key).lower() == low:
            return value
    return MISSING


def build_value_table(entity, source, params, timestamp, paging_values):
    """把五类来源合成一张「占位符值表」（RECIPE §二）。

    `operation.vars` + `request.defaults`/`sources[].defaults` + `--param`
    + 分页器 + 时间戳。`--param` 显式给的优先于同名默认值（否则 `region`
    永远只能取默认的地域，pushdown 就没意义了）。
    """
    values = {}
    vars_ = (entity.get("operation") or {}).get("vars") or {}
    for name, value in vars_.items():
        values[name.lower()] = {"value": value, "origin": "operation.vars", "shape": None}

    defaults = _defaults_of(source, entity)
    bindings = (entity.get("request") or {}).get("bindings") or {}
    for name, spec in bindings.items():
        if not isinstance(spec, dict):
            continue
        if spec.get("source") == "default":
            value = _lookup_default(defaults, name)
            if value is not MISSING:
                values[name.lower()] = {"value": value, "origin": "defaults",
                                        "shape": spec.get("shape"),
                                        "value_type": spec.get("value_type")}
    # --param 覆盖同名默认值/绑定值（显式 > 默认）
    for name, value in params.items():
        values[name.lower()] = {"value": value, "origin": "--param",
                                "shape": (bindings.get(name) or {}).get("shape")
                                if isinstance(bindings.get(name), dict) else None,
                                "value_type": (bindings.get(name) or {}).get("value_type")
                                if isinstance(bindings.get(name), dict) else None}
    values["timestamp"] = {"value": timestamp, "origin": "generated", "shape": None}
    for name, value in (paging_values or {}).items():
        values[name] = {"value": value, "origin": "paging", "shape": None}
    return values


def _render_string(node, values, where):
    """渲染一个字符串位置：整串就是一个 `{name}` 才算占位符，否则按字面量。"""
    match = _PLACEHOLDER_RE.match(node)
    if match:
        entry = values.get(match.group(1).lower())
        if entry is None:
            return MISSING
        return entry["value"]
    if _ANY_PLACEHOLDER_RE.search(node):
        fail("am.invalid", EXIT_INPUT,
             "%s 的模板值 %r 把占位符拼在了字符串中间" % (where, node),
             "占位符必须独占整个字符串（`\"{zone}\"`）。拼在中间没有安全的剪法"
             "（剪出来是半截），请改 AM。")
    return node


def render_template(node, values, where, in_array=False):
    """按 RECIPE §三 的剪枝规则渲染模板（返回剪好的值，或 `MISSING`）。

    | 位置 | 没值时的规则 |
    | 字符串里的占位符 | 整个值剪掉 |
    | 数组元素（对象）少一个键 | 整个元素剪掉 |
    | 顶层对象少一个键 | 只丢那个键 |
    | 数组被剪空 | 整个数组剪掉 |
    """
    if isinstance(node, dict):
        out = {}
        for key, sub in node.items():
            rendered = render_template(sub, values, "%s.%s" % (where, key))
            if rendered is MISSING:
                if in_array:
                    return MISSING  # 数组里的对象缺一半 ⇒ 整个元素剪掉
                continue
            out[key] = rendered
        return out

    if isinstance(node, list):
        # 特例：`["{status}"]` + bindings.status.shape=array ⇒ 整段替换成列表
        if len(node) == 1 and isinstance(node[0], str):
            match = _PLACEHOLDER_RE.match(node[0])
            if match:
                entry = values.get(match.group(1).lower())
                if entry is not None and entry.get("shape") == "array":
                    value = entry["value"]
                    return value if isinstance(value, list) else [value]
        out = []
        for item in node:
            rendered = render_template(item, values, where, in_array=True)
            if rendered is MISSING:
                continue
            out.append(rendered)
        if not out:
            return MISSING  # 数组被剪空 ⇒ 整段剪掉
        return out

    if isinstance(node, str):
        return _render_string(node, values, where)

    return node


def coerce_param_value(raw, spec):
    """把 `--param` 的字符串按 binding 声明转成 JSON 类型（`value_type` / `shape`）。

    ⚠️ 逗号只在 `shape == "array"` 时才当分隔符。AM 没声明 `shape` 的多值参数
    （例如 `instance_ids`）会整串当一个值 —— 这是「只透传、不推断」的代价，
    在 `AM-FIELDS.md` 里写明了。
    """
    spec = spec or {}
    value_type = (spec.get("value_type") or "string").lower()
    shape = spec.get("shape")

    def one(text):
        if value_type in ("integer", "int"):
            return int(text)
        if value_type in ("number", "float"):
            return float(text)
        if value_type in ("boolean", "bool"):
            return text.strip().lower() in ("1", "true", "yes", "y")
        return text

    if shape == "array":
        parts = [p.strip() for p in str(raw).split(",") if p.strip() != ""]
        return [one(p) for p in parts]
    if value_type == "string":
        return raw
    try:
        return one(raw)
    except ValueError:
        fail("am.invalid", EXIT_INPUT,
             "参数值 %r 转不成 %s（AM 的 bindings 这么声明的）" % (raw, value_type),
             "按声明给值；或者改 AM 里的 value_type。")


def paging_plan(entity):
    """读出这个 entity 怎么翻页（没有分页时返回 None）。

    同时兼容实体级 `paging{offset_param,limit_param,max_limit,total_path}`
    与 `request.paging{offset,page,limit,max_limit,value_type,offset_base}`
    ——真实 AM 里两份并存，实体级管「循环」，request 级管「类型/基数」。
    """
    paging = entity.get("paging")
    req_paging = (entity.get("request") or {}).get("paging") or {}
    if paging is None and not req_paging:
        return None
    paging = paging or {}
    offset_param = paging.get("offset_param") or req_paging.get("offset") \
        or req_paging.get("page") or "Offset"
    limit_param = paging.get("limit_param") or req_paging.get("limit") or "Limit"
    max_limit = paging.get("max_limit") or req_paging.get("max_limit") or 100
    try:
        max_limit = int(max_limit)
    except (TypeError, ValueError):
        max_limit = 100
    if max_limit <= 0:
        max_limit = 100
    offset_base = req_paging.get("offset_base")
    if offset_base is None:
        offset_base = 1 if req_paging.get("page") else 0
    try:
        offset_base = int(offset_base)
    except (TypeError, ValueError):
        offset_base = 0
    return {
        "offset_param": offset_param,
        "limit_param": limit_param,
        "max_limit": max_limit,
        "total_path": paging.get("total_path"),
        "value_type": (req_paging.get("value_type") or "number").lower(),
        "offset_base": offset_base,
    }


def build_body(entity, source, values, plan, offset, limit):
    """请求体 = 模板渲染（剪枝）+ 分页值注入（RECIPE §三 / §四）。"""
    where = "entity `%s` 的 request.template" % entity.get("id")
    template = (entity.get("request") or {}).get("template") or {}
    body = render_template(template, values, where)
    if body is MISSING:
        body = {}
    if plan is not None and offset is not None:
        if plan["value_type"] == "string":
            body[plan["offset_param"]] = str(offset)
            body[plan["limit_param"]] = str(limit)
        else:
            body[plan["offset_param"]] = int(offset)
            body[plan["limit_param"]] = int(limit)
    if not isinstance(body, dict):
        fail("am.invalid", EXIT_INPUT,
             "%s 渲染出来不是对象（%s）" % (where, type(body).__name__),
             "模板顶层应是对象，如 {\"<参数名>\": \"{<占位名>}\"}。")
    return body


# --------------------------------------------------------------------------- #
# 签名器注册表（SIGNERS）：一个 `auth.mode` = 一个签名器
# --------------------------------------------------------------------------- #
#
# 主流程**只做查表**：`signer = SIGNERS[auth.mode]`。所以加一种鉴权算法 =
# 写一个 `Signer` 子类 + 在 `SIGNERS` 里注册一行，主流程一行不用改。
# 不认识 mode ⇒ `auth.unsupported`（退出码 4）+ 列出已支持的 mode，⛔ 不 fallback。
#
# 每个签名器**自带它的默认值**（signed_headers / timestamp / scope /
# content_type / service），主流程里不出现任何一家厂商的常量。

class Signer(object):
    """签名器接口。

    子类只需要：① 填 `MODE` 与默认值；② 实现 `sign()`（要发算法特有的 header
    就覆盖 `build_headers()`）。默认值与「哪些 auth 键必须显式声明」都在类上，
    所以主流程不必知道具体算法。
    """

    #: 注册用的 mode 字符串（`auth.mode` 的值）
    MODE = ""
    #: AM 没写时用本签名器的默认值（`--check` 会明说正在用默认值）
    DEFAULT_SIGNED_HEADERS = ()
    DEFAULT_TIMESTAMP_HEADER = ""
    DEFAULT_TIMESTAMP_FORMAT = ""
    DEFAULT_SCOPE = ()
    DEFAULT_CONTENT_TYPE = "application/json; charset=utf-8"
    #: 缺了就算默认值的 auth 键（`--check` 会提示）
    DEFAULTABLE_AUTH_KEYS = ("signed_headers", "timestamp", "scope")
    #: AM **必须**显式声明、⛔ 不许推断的 auth 键
    REQUIRED_AUTH_KEYS = ()
    #: 必填键的「怎么补」话术（键 → 一句人话）
    AUTH_KEY_HINTS = {}

    def profile(self, auth, source_id="?"):
        """把 AM 的 `auth` 段 + 本签名器的默认值合成一份 profile。"""
        timestamp_cfg = auth.get("timestamp") or {}
        profile = {
            "signer": self,
            "mode": self.MODE,
            "source": source_id,
            "signed_headers": list(auth.get("signed_headers")
                                   or self.DEFAULT_SIGNED_HEADERS),
            "timestamp_header": timestamp_cfg.get("header")
                                or self.DEFAULT_TIMESTAMP_HEADER,
            "timestamp_format": timestamp_cfg.get("format")
                                or self.DEFAULT_TIMESTAMP_FORMAT,
            "scope": list(auth.get("scope") or self.DEFAULT_SCOPE),
            "content_type": auth.get("content_type") or self.DEFAULT_CONTENT_TYPE,
        }
        for key in self.REQUIRED_AUTH_KEYS:
            value = auth.get(key)
            if not isinstance(value, str) or not value.strip():
                fail("am.invalid", EXIT_INPUT,
                     "source `%s` 的 `auth.%s` 缺失 —— `%s` 签名器要求显式声明"
                     % (source_id, key, self.MODE),
                     self.auth_key_hint(key))
            profile[key] = value.strip()
        return profile

    def auth_key_hint(self, key):
        """必填 auth 键缺失时的一句「怎么补」。"""
        return self.AUTH_KEY_HINTS.get(key) or (
            "在 source 的 `auth` 里加 \"%s\": \"<值>\"（键义见 AM-FIELDS.md）。" % key)

    def describe_defaults(self):
        """一句话说清本签名器的默认值（`--check` 的提示用）。"""
        return ("signed_headers=%s · timestamp=%s/%s · scope=%s"
                % (";".join(self.DEFAULT_SIGNED_HEADERS) or "（无）",
                   self.DEFAULT_TIMESTAMP_HEADER or "（无）",
                   self.DEFAULT_TIMESTAMP_FORMAT or "（无）",
                   "/".join(self.DEFAULT_SCOPE) or "（无）"))

    def build_headers(self, entity, profile, values, timestamp, body, content_type):
        """要发的 header：AM 声明了 `operation.headers` 就照它拼，否则补要用的。

        默认实现只补「所有 HTTP 请求都要有」的 Content-Type / Host 和时间戳头；
        算法特有的头（版本号、动作名…）由子类覆盖。
        """
        operation = entity.get("operation") or {}
        declared = operation.get("headers") or {}
        headers = {}
        if declared:
            for name, template in declared.items():
                rendered = render_template(template, values,
                                           "entity `%s` 的 operation.headers"
                                           % entity.get("id"))
                if rendered is MISSING:
                    continue
                headers[name] = text_of(rendered)
        elif profile.get("timestamp_header"):
            headers["Content-Type"] = content_type
            headers[profile["timestamp_header"]] = str(int(timestamp))

        headers.setdefault("Content-Type", content_type)
        headers["Host"] = profile.get("host") or ""
        return headers

    def sign(self, credentials, *, method, uri, query, headers, body_bytes,
             timestamp, profile):
        """算出 `Authorization` 等值，返回 dict（至少含 `authorization`）。"""
        raise NotImplementedError("签名器 `%s` 没有实现 sign()" % (self.MODE or "?"))


# --------------------------------------------------------------------------- #
# 签名算法细节：TC3-HMAC-SHA256（AUTH-SPEC §2–§6）
# —— 只有 `Tc3Signer` 用，别的签名器自己写自己的
# --------------------------------------------------------------------------- #

def canonical_header_string(headers, signed_headers):
    """AUTH-SPEC §3.1：按 signed_headers 取值、名字小写、压空白、ASCII 升序、每行 `\\n`。"""
    lowered = {}
    for name, value in headers.items():
        lowered[name.lower()] = text_of(value)
    lines = []
    for name in sorted(h.lower() for h in signed_headers):
        if name not in lowered:
            fail("am.invalid", EXIT_INPUT,
                 "auth.signed_headers 声明了 `%s`，但脚本没打算发这个 header" % name,
                 "要么把该 header 写进 operation.headers，要么从 signed_headers 去掉它。"
                 "（AUTH-SPEC §8 坑 6：签了没发的头 ⇒ 服务端 AuthFailure.SignatureFailure）")
        value = " ".join(lowered[name].split())  # 两端去空白 + 内部连续空白压成一个
        if name == "x-tc-action":
            value = value.lower()  # AUTH-SPEC §3.1：接口名按小写进规范头
        lines.append("%s:%s" % (name, value))
    return "\n".join(lines) + "\n"


def derive_signing_key(secret_key, date, service, last_scope_segment):
    """AUTH-SPEC §5 的派生链，返回三段中间值（自检要逐段对拍）。"""
    secret_date = hmac_sha256(("TC3" + secret_key).encode("utf-8"), date)
    secret_service = hmac_sha256(secret_date, service)
    secret_signing = hmac_sha256(secret_service, last_scope_segment)
    return {
        "SecretDate": secret_date,
        "SecretService": secret_service,
        "SecretSigning": secret_signing,
    }


def resolve_scope(scope_segments, date, service):
    """scope 段名 → 值（`date`/`service` 求值，其余段是字面量）。AUTH-SPEC §4。"""
    mapped = []
    for seg in scope_segments:
        if seg == "date":
            mapped.append(date)
        elif seg == "service":
            mapped.append(service)
        else:
            mapped.append(str(seg))
    return "/".join(mapped)


def sign_tc3(secret_id, secret_key, *, method, uri, query, headers, signed_headers,
             body_bytes, timestamp, scope_segments, service):
    """算 TC3 签名，返回全部中间值（自检要逐字节对拍 AUTH-SPEC §7.2）。

    ⚠️ 两个「逐字节」：① 参与签名的 header 值必须与实际发出的一致；
    ② HashedRequestPayload 必须是**实际发送**的 body 字节。
    """
    canon_headers = canonical_header_string(headers, signed_headers)
    payload_hash = sha256_hex(body_bytes)
    signed = ";".join(sorted(h.lower() for h in signed_headers))
    # 规范 header 串自带结尾 `\n`，join 时再加一个 ⇒ query 与第一个 header 之间是空行
    canonical_request = "\n".join([
        method.upper(), uri, query, canon_headers, signed, payload_hash,
    ])

    date = time.strftime("%Y-%m-%d", time.gmtime(int(timestamp)))
    scope = resolve_scope(scope_segments, date, service)
    last_segment = scope.split("/")[-1]
    string_to_sign = "\n".join([
        "TC3-HMAC-SHA256", str(int(timestamp)), scope, sha256_hex(canonical_request.encode("utf-8")),
    ])
    keys = derive_signing_key(secret_key, date, service, last_segment)
    signature = hmac.new(keys["SecretSigning"], string_to_sign.encode("utf-8"),
                         hashlib.sha256).hexdigest()
    authorization = ("TC3-HMAC-SHA256 Credential=%s/%s, SignedHeaders=%s, Signature=%s"
                     % (secret_id, scope, signed, signature))
    return {
        "canonical_headers": canon_headers,
        "canonical_request": canonical_request,
        "hashed_request_payload": payload_hash,
        "hashed_canonical_request": sha256_hex(canonical_request.encode("utf-8")),
        "date": date,
        "scope": scope,
        "string_to_sign": string_to_sign,
        "SecretDate": keys["SecretDate"].hex(),
        "SecretService": keys["SecretService"].hex(),
        "SecretSigning": keys["SecretSigning"].hex(),
        "signature": signature,
        "authorization": authorization,
    }


class Tc3Signer(Signer):
    """TC3-HMAC-SHA256 签名器（算法规格见 `AUTH-SPEC.md`）。

    默认值全部在这个类里 —— 主流程不再持有任何 TC3 常量。
    `service` 是**待签名 scope 与派生密钥的输入**，必须来自 AM（`auth.service`），
    ⛔ 不从 `base_url` 的 host 猜：猜错不会报错，只会签出一个「看起来对」的错签名。
    """

    MODE = "tc3-hmac-sha256"
    DEFAULT_SIGNED_HEADERS = ("content-type", "host", "x-tc-action")
    DEFAULT_TIMESTAMP_HEADER = "X-TC-Timestamp"
    DEFAULT_TIMESTAMP_FORMAT = "unix-seconds"
    DEFAULT_SCOPE = ("date", "service", "tc3_request")
    DEFAULT_CONTENT_TYPE = "application/json; charset=utf-8"
    REQUIRED_AUTH_KEYS = ("service",)
    AUTH_KEY_HINTS = {
        "service": ("在 source 的 `auth` 里加 \"service\": \"<该接口所属服务名>\" —— "
                    "它进待签名 scope 与派生密钥，签名器不替你猜"
                    "（键义见 AM-FIELDS.md『auth.service』）。"),
    }
    ACTION_HEADER = "X-TC-Action"
    VERSION_HEADER = "X-TC-Version"
    REGION_HEADER = "X-TC-Region"

    def build_headers(self, entity, profile, values, timestamp, body, content_type):
        """TC3 的标准头：动作名 / 版本 / 时间戳；Region 只放一处（header 或 body）。"""
        operation = entity.get("operation") or {}
        declared = operation.get("headers") or {}
        headers = {}
        if declared:
            for name, template in declared.items():
                rendered = render_template(
                    template, values,
                    "entity `%s` 的 operation.headers" % entity.get("id"))
                if rendered is MISSING:
                    continue
                headers[name] = text_of(rendered)
        else:
            headers["Content-Type"] = content_type
            headers[self.ACTION_HEADER] = text_of(operation.get("action"))
            headers[self.VERSION_HEADER] = text_of(operation.get("version"))
            headers[profile["timestamp_header"]] = str(int(timestamp))

        headers.setdefault("Content-Type", content_type)
        headers["Host"] = profile["host"]

        region = values.get("region")
        body_has_region = any(str(k).lower() == "region" for k in body)
        if region is not None and not body_has_region:
            headers[self.REGION_HEADER] = text_of(region["value"])
        return headers

    def sign(self, credentials, *, method, uri, query, headers, body_bytes,
             timestamp, profile):
        """按 AUTH-SPEC 算签名；返回全部中间值（自检要逐段对拍）。"""
        secret_id, secret_key = credentials
        return sign_tc3(
            secret_id, secret_key,
            method=method, uri=uri, query=query, headers=headers,
            signed_headers=profile["signed_headers"],
            body_bytes=body_bytes, timestamp=timestamp,
            scope_segments=profile["scope"], service=profile["service"],
        )


#: 已注册的签名器（**唯一一处**）。加一种算法 = 加一行。
SIGNERS = {
    Tc3Signer.MODE: Tc3Signer(),
}


def supported_modes():
    """已注册的 mode（排序后返回；报错与 `--list-signers` 都用它）。"""
    return sorted(SIGNERS)


def signers_text():
    """已注册的 mode 拼成一句话（错误消息里列「支持列表」用）。"""
    return ", ".join(supported_modes()) or "（无）"


def signer_of(mode, source_id=None):
    """按 `auth.mode` 查签名器；未知 ⇒ `auth.unsupported`（退出 4）+ 支持列表。

    ⛔ 这里**没有** fallback：查不到就是查不到，绝不用别的算法顶上。
    """
    where = ("source `%s` 的 " % source_id) if source_id else ""
    if not isinstance(mode, str) or not mode.strip():
        fail("am.invalid", EXIT_INPUT,
             "%s`auth.mode` 缺失 —— AM 必须写明用哪个签名器" % where,
             "补 \"mode\": \"<签名器 mode>\"；已支持：%s（`--list-signers` 可查）。"
             % signers_text())
    signer = SIGNERS.get(mode.strip())
    if signer is None:
        fail_code("auth.unsupported",
                  "%s`auth.mode`=%r 没有对应的签名器；已支持：%s"
                  % (where, mode, signers_text()),
                  "换一个已支持的 mode，或者给这个 mode 注册一个签名器"
                  "（`SIGNERS` 里加一行，主流程不用改）。⛔ 不会退回到别的算法去签。")
    return signer


def auth_profile(entity, source):
    """取这个 entity 的签名参数：签名器按 `auth.mode` **查表**，默认值由签名器自带。

    `auth.service`（tc3 要求必填）这类键**只从 AM 来** —— ⛔ 不从 host 猜。
    """
    auth = source.get("auth") or {}
    source_id = source.get("id")
    signer = signer_of(auth.get("mode"), source_id)
    profile = signer.profile(auth, source_id=source_id)
    host = urllib.parse.urlsplit(source.get("base_url") or "").netloc
    if not host:
        fail("am.invalid", EXIT_INPUT,
             "source `%s` 的 base_url 解析不出 host" % source_id,
             "base_url 要是绝对 http(s) 地址。")
    profile["host"] = host
    return profile


# --------------------------------------------------------------------------- #
# 传输层（真网络 / fixture / 自检列表，三者同接口）
# --------------------------------------------------------------------------- #

class Transport(object):
    """传输接口：`post(url, headers, body_bytes) -> dict`。`calls` 记录发了几个请求。"""

    def __init__(self):
        self.calls = 0

    def post(self, url, headers, body_bytes):  # pragma: no cover - 由子类实现
        raise NotImplementedError


class HttpTransport(Transport):
    """真网络：`urllib.request`（标准库），把 HTTP 状态/异常翻成固定错误码。"""

    def __init__(self, timeout=DEFAULT_TIMEOUT):
        Transport.__init__(self)
        self.timeout = timeout

    def post(self, url, headers, body_bytes):
        """发一次 POST；返回解析后的 JSON 对象（HTTP 层错误在返回前就翻成码）。"""
        request = urllib.request.Request(url, data=body_bytes, headers=headers, method="POST")
        try:
            with urllib.request.urlopen(request, timeout=self.timeout) as resp:
                self.calls += 1
                raw = resp.read()
                status = getattr(resp, "status", 200)
        except urllib.error.HTTPError as exc:
            self.calls += 1
            raw = exc.read()
            status = exc.code
        except socket.timeout as exc:
            self.calls += 1
            fail(classify_network_error(exc), EXIT_NETWORK,
                 "请求超时（%d 秒）：%s" % (self.timeout, url),
                 "网络/代理不通，或接口很慢。先 `--dry-run` 确认映射没问题，"
                 "再查网络；本脚本**不自动重试**（重试策略交给调用方）。")
        except urllib.error.URLError as exc:
            self.calls += 1
            code = classify_network_error(exc)
            fail(code, EXIT_NETWORK, "连不上 %s（%s）" % (url, getattr(exc, "reason", exc)),
                 "查 DNS/代理/出网策略；⛔ 不要改成猜地址（会连到别的环境）。")
        except OSError as exc:
            self.calls += 1
            fail(classify_network_error(exc), EXIT_NETWORK,
                 "网络错误：%s（%s）" % (url, exc),
                 "查网络后重试；本脚本不自动重试。")

        if status != 200:
            fail_for_http_status(status, raw, url)
        try:
            return json.loads(raw.decode("utf-8"))
        except (ValueError, UnicodeDecodeError):
            fail("net.error", EXIT_NETWORK,
                 "服务端返回的不是 JSON（HTTP %d，%d 字节）" % (status, len(raw)),
                 "可能是代理/网关页面。看原始响应再决定；⛔ 别按 JSON 解析。")


def classify_network_error(exc):
    """把连接类异常分成 `net.timeout` 与 `net.error`（纯函数，可离线单测）。

    `URLError.reason` 如果是超时，仍然算超时——`urllib` 会把底层 `socket.timeout`
    包一层，只看异常类型会把它误判成「连不上」。
    """
    reason = getattr(exc, "reason", exc)
    if isinstance(reason, (socket.timeout, TimeoutError)) or isinstance(exc, (socket.timeout, TimeoutError)):
        return "net.timeout"
    return "net.error"


def fail_for_http_status(status, raw, url):
    """HTTP 状态 → 固定错误码（401/403→4，429→5，400→6，5xx→7）。"""
    snippet = ""
    try:
        text = raw.decode("utf-8", "replace").strip()
        snippet = text[:200]
    except Exception:  # 解码失败不该影响报错
        snippet = ""
    detail = "；响应片段：%s" % snippet if snippet else ""
    if status in (401, 403):
        fail_code("auth.rejected",
                  "服务端拒绝鉴权（HTTP %d）：%s%s" % (status, url, detail),
                  "凭据是否有效/是否过期？签名器是否按 `auth.mode` 的实际规格实现"
                  "（本脚本已实现，⛔ 不要自己再写一份）？")
    if status == 429:
        fail_code("api.throttled",
                  "被限流（HTTP 429）：%s%s" % (url, detail),
                  "降并发/减小 --limit 或 --max-rows，隔一会儿再来；本脚本不自动退避。")
    if status in (400, 404):
        fail_code("api.invalid",
                  "请求参数被拒（HTTP %d）：%s%s" % (status, url, detail),
                  "多半是请求体/参数不对：看 `--dry-run` 打出来的 body、以及 "
                  "`capability.pushdown` 允许哪些参数。")
    fail_code("net.error",
              "服务端状态异常（HTTP %d）：%s%s" % (status, url, detail),
              "5xx 多为服务端瞬时故障，稍后重试；持续 5xx 报给运维。")


def classify_vendor_error(code_text):
    """有的接口把业务错误放在 HTTP 200 的响应体里（`<信封>.Error`）；按码翻成固定分类。"""
    low = str(code_text or "").lower()
    if "limitexceeded" in low or "throttl" in low or "toomanyrequest" in low:
        return "api.throttled"
    if low.startswith("authfailure") or "signature" in low or "unauthorized" in low:
        return "auth.rejected"
    if "internalerror" in low:
        return "net.error"
    return "api.invalid"


class FixtureTransport(Transport):
    """`--dry-run`：读 entity.fixture，**不发网络**。

    一个 fixture 只有一页：第 1 次调用返回它，之后返回空响应（分页循环因此
    自然停下，并如实报 `truncated`，⛔ 不会假装取全）。
    """

    def __init__(self, fixture_path):
        Transport.__init__(self)
        self.fixture_path = fixture_path
        self.payload = None
        if not os.path.isfile(fixture_path):
            fail("am.invalid", EXIT_INPUT,
                 "fixture 文件不存在：%s" % fixture_path,
                 "从 AM 里的 `fixture` 字段或运维给的夹具补上；"
                 "或者去掉 `--dry-run` 走真接口。")
        self.payload = read_json_file(fixture_path, "fixture")

    def post(self, url, headers, body_bytes):  # noqa: ARG002 - 形状固定
        """第一次给 fixture，之后给空响应（模拟「没有下一页」）。"""
        self.calls += 1
        if self.calls == 1:
            return self.payload
        return {"Response": {}}


class ListTransport(Transport):
    """自检用：按调用次序依次返回预置响应（可断言「恰 N 次请求」）。"""

    def __init__(self, payloads):
        Transport.__init__(self)
        self.payloads = list(payloads)

    def post(self, url, headers, body_bytes):  # noqa: ARG002
        """第 i 次调用给第 i 个响应；越界给空响应。"""
        index = self.calls
        self.calls += 1
        if index < len(self.payloads):
            return self.payloads[index]
        return {"Response": {}}


# --------------------------------------------------------------------------- #
# 取数
# --------------------------------------------------------------------------- #

class FetchResult(object):
    """一次取数的结果（行、页数、是否取全、为什么停）。"""

    def __init__(self):
        self.rows = []          # 原始行（iterator 切出来的对象）
        self.pages = 0
        self.total = None       # 服务端声明的 TotalCount（没有就是 None）
        self.truncated = False
        self.stop_reason = None
        self.body = None        # 最后一次请求体（dry-run 打印用）


def _stop_reason_text(reason):
    """把终止原因翻成一句人话（放进 `meta.stop_reason`）。"""
    return {
        "complete": "已取全（达到服务端声明的总数）",
        "capped": "到 --max-rows 上限，没取全",
        "empty": "服务端提前返回空页（可能权限/地域受限）",
        "short": "本页条数少于 --limit，且服务端没给总数（按「取到短页」判断结束）",
        "single": "该 entity 没声明分页，只发了 1 次请求",
        "page_cap": "到分页保险丝（%d 页）" % MAX_PAGES_HARD_CAP,
    }.get(reason, str(reason))


def fetch_rows(entity, source, profile, transport, *, limit, max_rows, params,
               timestamp, dry_run=False, credentials=None):
    """按 demand §4.2 的第 3–6 步取数：签名 → 请求 → 翻页 → 切行。

    终止条件（任一命中即停）：空页 / 达到 TotalCount / 达到 --max-rows /
    短页（无 TotalCount 时）/ 分页保险丝。**是否取全**由 `truncated` 如实表达。

    `credentials` 是 `(SecretId, SecretKey)`；`dry_run=True` 时不需要它（不签名）。
    """
    plan = paging_plan(entity)
    operation = entity.get("operation") or {}
    action = operation.get("action")
    content_type = operation.get("content_type") or profile["content_type"]
    url = str(source.get("base_url", "")).rstrip("/") + (operation.get("path") or "/")

    values = build_value_table(entity, source, params, timestamp, None)
    result = FetchResult()
    effective_limit = plan["max_limit"] if plan else 0
    if plan and limit:
        effective_limit = min(limit, plan["max_limit"])
        if limit > plan["max_limit"]:
            sys.stderr.write(redact(
                "[注意] --limit %d 超过 paging.max_limit %d，按 %d 取（服务端单页上限）。\n"
                % (limit, plan["max_limit"], effective_limit)))

    while True:
        offset = None
        if plan is not None:
            offset = plan["offset_base"] + result.pages * effective_limit
        body = build_body(entity, source, values, plan, offset, effective_limit)
        result.body = body
        body_bytes = dump_body(body).encode("utf-8")

        headers = {}
        if not dry_run:
            if not credentials:
                raise RuntimeError("fetch_rows: 非 dry-run 却没有传 credentials")
            signer = profile["signer"]
            headers = signer.build_headers(entity, profile, values, timestamp,
                                           body, content_type)
            signed = signer.sign(
                credentials,
                method=operation.get("method") or "POST",
                uri=operation.get("path") or "/",
                query="",
                headers=headers,
                body_bytes=body_bytes,
                timestamp=timestamp,
                profile=profile,
            )
            headers["Authorization"] = signed["authorization"]

        response = transport.post(url, headers, body_bytes)
        result.pages += 1
        _raise_vendor_error(entity, action, response)

        if plan and plan["total_path"]:
            total = parse_jsonpath(plan["total_path"],
                                   "entity `%s` 的 paging.total_path"
                                   % entity.get("id")).resolve(response)
            if isinstance(total, bool):
                total = None
            if isinstance(total, (int, float)):
                result.total = int(total)

        iterator = entity.get("iterator")
        if not iterator:
            fail("am.invalid", EXIT_INPUT,
                 "entity `%s` 没有 `iterator`，无法知道响应里哪一段是「一行」"
                 % entity.get("id"),
                 "补 `iterator`（形状 `$.<字段>.<数组>[*]`），"
                 "或先 `--check` 看它还缺什么。")
        batch = parse_jsonpath(iterator, "entity `%s` 的 iterator"
                               % entity.get("id")).resolve(response)
        if batch is MISSING:
            batch = []
        if not isinstance(batch, list):
            fail("am.invalid", EXIT_INPUT,
                 "entity `%s` 的 iterator 没取到数组：%s" % (entity.get("id"), iterator),
                 "确认 iterator 末尾有 `[*]`，且响应里那一段确实是数组。")
        result.rows.extend(batch)

        got = len(result.rows)
        want_more_than_cap = result.total is None or result.total > max_rows
        if got >= max_rows and want_more_than_cap:
            result.stop_reason = "capped"
            break
        if result.total is not None and got >= result.total:
            result.stop_reason = "complete"
            break
        if not batch:
            result.stop_reason = "empty"
            break
        if plan is None:
            result.stop_reason = "single"
            break
        if result.total is None and len(batch) < effective_limit:
            result.stop_reason = "short"
            break
        if result.pages >= MAX_PAGES_HARD_CAP:
            result.stop_reason = "page_cap"
            break

    if len(result.rows) > max_rows:
        result.rows = result.rows[:max_rows]
        result.truncated = True
    if result.stop_reason in ("capped", "page_cap"):
        result.truncated = True
    if result.total is not None and len(result.rows) < result.total:
        result.truncated = True
    return result


def _raise_vendor_error(entity, action, response):
    """接口把业务错误放在响应体的 `<信封>.Error` 里时，按码翻成固定错误码。

    ⚠️ 信封形状（`Response.Error{Code,Message}`）本身是**某个厂商的**约定；
    本脚本只认这一种形状，换个信封形状的源需要扩这里（见 AM-FIELDS「已知限制」）。
    """
    if not isinstance(response, dict):
        return
    payload = response.get("Response")
    if not isinstance(payload, dict):
        return
    error = payload.get("Error") or {}
    if not isinstance(error, dict) or not error:
        return
    code = error.get("Code") or ""
    message = error.get("Message") or ""
    mapped_code = classify_vendor_error(code)
    fail_code(mapped_code,
              "接口返回错误：%s · %s（entity `%s`，action `%s`）"
              % (code, message, entity.get("id"), action),
              _fix_for_vendor_code(code))


def _fix_for_vendor_code(code):
    """按厂商错误码给一句「怎么修」。"""
    low = str(code or "").lower()
    if "signature" in low or low.startswith("authfailure"):
        fail_hint = ("签名不匹配：先确认凭据对、时钟准（时间戳是 UTC 秒）、"
                     "再确认 AM 的 auth.signed_headers / timestamp / scope / service "
                     "与实际发出的 header 一致。⛔ 不要自己重写签名，本脚本已按规格实现。")
        return fail_hint
    if "limitexceeded" in low or "throttl" in low:
        return "降 --limit / --max-rows 或收窄 --param 范围；本脚本不自动退避。"
    if "unknownparameter" in low:
        return ("参数不在该接口的接受集里：看 `--dry-run` 打出的 body；"
                "有的参数只能走 header（RECIPE §七①），⛔ 别两处都写。")
    if "missingparameter" in low:
        return "AM 的 request.required 少给了值：补 `--param <名>=<值>`。"
    return "看 code 与 message；必要时先 `--dry-run` 排除映射问题。"


# --------------------------------------------------------------------------- #
# 映射成 JSON-LD / 平表
# --------------------------------------------------------------------------- #

def to_graph(entity, rows):
    """把原始行映射成 JSON-LD `@graph`（demand §4.4 的形状）。

    跳过没有 identity 的行（没有 `@id` 的节点在图上没有身份），并把条数记进
    `meta.skipped_no_identity` —— 不是静默丢。
    """
    cls = entity.get("class")
    iri_template = entity.get("iri_template") or ("ontok:%s/{id}" % cls)
    key_path = (entity.get("identity") or {}).get("key_path")
    fields = entity.get("fields") or {}
    graph = []
    skipped = 0
    for row in rows:
        if not isinstance(row, dict):
            skipped += 1
            continue
        key = dig_path(row, key_path, "entity `%s` 的 identity.key_path" % entity.get("id")) \
            if key_path else MISSING
        if key is MISSING or key is None or str(key) == "":
            skipped += 1
            continue
        node = {
            "@id": iri_template.replace("{id}", text_of(key)),
            "@type": "ontok:%s" % cls,
        }
        for prop, spec in fields.items():
            value = dig_path(row, (spec or {}).get("path"),
                             "entity `%s` 的 fields.%s.path" % (entity.get("id"), prop))
            if value is MISSING:
                continue  # RECIPE §六：缺字段 ⇒ 该属性没有值，不丢整行
            node["ontok:%s" % prop] = value
        graph.append(node)
    return graph, skipped


def to_rows_table(entity, graph):
    """`--format rows`：平表（`columns` + `rows`），嵌套值原样保留。"""
    columns = ["@id", "@type"]
    for prop in (entity.get("fields") or {}):
        columns.append(prop)
    table = []
    for node in graph:
        line = []
        for column in columns:
            if column == "@id":
                line.append(node.get("@id"))
            elif column == "@type":
                line.append(node.get("@type"))
            else:
                line.append(node.get("ontok:%s" % column))
        table.append(line)
    return columns, table


def build_output(entity, source, am, result, graph, skipped, fmt, dry_run):
    """拼最终输出对象：`@context` + `@graph`/`rows` + `meta`。

    `meta` 里 `rows` / `pages` / `truncated` 是**契约字段**：数字员工靠它判断
    「够不够」。`truncated` 为真时退出码一定是 8。
    """
    meta = {
        "entity": entity.get("id"),
        "class": entity.get("class"),
        "rows": len(graph),
        "pages": result.pages,
        "truncated": bool(result.truncated),
        "source": source.get("id"),
        "mapping_version": am.get("mapping_version"),
        "total": result.total,
        "stop_reason": _stop_reason_text(result.stop_reason),
    }
    if skipped:
        meta["skipped_no_identity"] = skipped
    if dry_run:
        meta["dry_run"] = True
        meta["request_body"] = result.body

    if fmt == "rows":
        columns, table = to_rows_table(entity, graph)
        return {"@context": JSONLD_CONTEXT, "columns": columns, "rows": table, "meta": meta}
    return {"@context": JSONLD_CONTEXT, "@graph": graph, "meta": meta}


# --------------------------------------------------------------------------- #
# 输出
# --------------------------------------------------------------------------- #

def emit_json(obj, out_path):
    """写结果：默认 stdout（纯 JSON），`--out` 写文件（UTF-8）。"""
    text = json.dumps(obj, ensure_ascii=False, indent=2)
    if not out_path:
        sys.stdout.write(text + "\n")
        return
    try:
        with io.open(out_path, "w", encoding="utf-8", newline="\n") as fh:
            fh.write(text + "\n")
    except OSError as exc:
        sys.stderr.write(redact("写不进 %s：%s\n" % (out_path, exc)))
        raise SystemExit(1)
    sys.stderr.write(redact("已写入 %s（rows=%d, pages=%d, truncated=%s）\n"
                            % (out_path, obj["meta"]["rows"], obj["meta"]["pages"],
                               str(obj["meta"]["truncated"]).lower())))


def print_err(error):
    """把 FetchError 打到 stderr（脱敏后）。"""
    sys.stderr.write(error.rendered() + "\n")
    for code, message in (error.extra.get("errors") or [])[1:]:
        sys.stderr.write(redact("     · [%s] %s\n" % (code, message)))


# --------------------------------------------------------------------------- #
# 子命令：--check
# --------------------------------------------------------------------------- #

def print_contract_version(version):
    """`--check` 的版本可见性：识别到哪一版 + 该版本下脚本实际读了哪些键。

    版本差异要让用户看见：v1/v2 的差别在「`auth` 四件套写不写进 AM」，
    而脚本的**读取键集两版相同** —— 加一个版本不改读取逻辑，改的是契约期望。
    所以这里同时打「识别到哪版」和「读哪些键」，避免有人以为 v2 有特殊分支。
    """
    note = None if isinstance(version, bool) else CONTRACT_VERSION_NOTES.get(version)
    if note is None:
        print("  ⇒ 识别：不支持（脚本支持 %s）—— 见下方错误" % supported_versions_text())
        return
    print("  ⇒ 识别：%s" % note)
    print("  该版本下脚本实际读的 auth 键（v1/v2 相同，版本不改变读取）：")
    print("    · 必填：%s ← 缺则 am.invalid（签名器要求显式，⛔ 不从 host 猜）"
          % AUTH_KEYS_REQUIRED_BY_SIGNER)
    print("    · 可缺：%s ← 缺则用签名器默认值（末尾会提示）"
          % AUTH_KEYS_DEFAULTABLE)


def cmd_check(am, am_path, entity_ref=None):
    """`--check`：只校验 AM 结构，不取数。有 error 退出 2，只有 warning 退出 0。"""
    report = validate_am(am, am_path=am_path)
    print("AM 检查：%s" % os.path.abspath(am_path))
    print("  contract_version = %r    mapping_version = %r    ontology_version = %r"
          % (am.get("contract_version"), am.get("mapping_version"),
             am.get("ontology_version")))
    print_contract_version(am.get("contract_version"))
    print("  sources  : %d     entities : %d（已签 %d / 未签 %d）    relations : %d（脚本不用）"
          % (len(am.get("sources") or []), len(am.get("entities") or []),
             len(am.get("entities") or []) - len(report.unsigned),
             len(report.unsigned), len(am.get("relations") or [])))

    print("\nsource 检查")
    default_ids = {item["id"] for item in report.auth_defaults}
    for src in report.sources:
        auth = src.get("auth") or {}
        envs = auth.get("credential_env") or []
        extra = ""
        if src.get("id") in default_ids:
            extra = "  [auth 用了签名器默认值]"
        print("  ✓ %-16s %-9s %-38s 凭据变量 %s%s"
              % (src.get("id"), src.get("kind"), src.get("base_url"),
                 "/".join(envs) or "（无）", extra))

    print("\nentity 检查")
    for ent in report.entities:
        fixture = ent.get("fixture")
        fixture_note = "fixture=有" if fixture else "fixture=无"
        if fixture and not os.path.isfile(resolve_fixture_path(fixture, am_path)):
            fixture_note = "fixture=缺"
        mark = "✓" if entity_is_signed(ent) else "✗"
        print("  %s %-18s class=%-22s source=%-15s iterator=%-46s fields=%-3d %s"
              % (mark, ent.get("id"), ent.get("class"), ent.get("source"),
                 ent.get("iterator") or "（无）", len(ent.get("fields") or {}),
                 fixture_note))

    notes = list(report.warnings)
    if report.auth_defaults:
        modes = sorted({item["mode"] for item in report.auth_defaults})
        keys = sorted({key for item in report.auth_defaults
                       for key in item["missing"]})
        defaults = "; ".join(
            "%s: %s" % (mode, SIGNERS[mode].describe_defaults())
            for mode in modes if mode in SIGNERS)
        notes.append("%d 个 source 的 auth 缺 %s ⇒ 按签名器默认值（%s）。"
                     "要精确就在 AM 里补这几项。"
                     % (len(report.auth_defaults), "/".join(keys), defaults))

    exit_code = EXIT_OK
    if report.errors:
        exit_code = CODE_EXIT.get(report.errors[0][0], EXIT_INPUT)
        print("\n错误（阻断，退出码 %d）" % exit_code)
        for code, message in report.errors:
            print("  ✗ [%s] %s" % (code, message))
    if notes:
        print("\n提示（不影响通过）")
        for message in notes:
            print("  · %s" % message)

    if entity_ref:
        try:
            ent = find_entity(am, entity_ref)
            print("\n--entity %s ⇒ id=%s class=%s %s"
                  % (entity_ref, ent.get("id"), ent.get("class"),
                     "已签" if entity_is_signed(ent) else "未签（取不了数）"))
        except FetchError as exc:
            print("\n%s" % exc.rendered())

    if report.errors:
        print("\n结果：拒绝（%d error / %d warning）" % (len(report.errors), len(notes)))
        return exit_code
    print("\n结果：通过（%d sources / %d entities，0 error / %d warning）"
          % (len(report.sources), len(report.entities), len(notes)))
    return EXIT_OK


# --------------------------------------------------------------------------- #
# 子命令：取数
# --------------------------------------------------------------------------- #

def parse_params(raw_params, entity):
    """解析 `--param K=V`，只放行 `capability.pushdown` 里的名字（AC-007）。

    ⛔ 不在这里「顺手放开」：不在 pushdown 里的参数，服务端大概率也不认，
    本地拦住能省一次白跑（还可能计费）。
    """
    capability = entity.get("capability") or {}
    pushdown = capability.get("pushdown")
    if pushdown is None:
        pushdown = []
    allowed = {str(name).lower(): str(name) for name in pushdown}
    bindings = (entity.get("request") or {}).get("bindings") or {}

    params = {}
    for item in raw_params or []:
        if "=" not in item:
            fail("param.not_pushdown", EXIT_INPUT,
                 "--param 要写成 K=V，收到 %r" % item,
                 "例：`--param <参数名>=<值>`。")
        name, value = item.split("=", 1)
        name = name.strip()
        if name.lower() not in allowed:
            available = ", ".join(str(n) for n in pushdown) or "（这个 entity 一个可下推参数都没有）"
            fail("param.not_pushdown", EXIT_INPUT,
                 "参数 `%s` 不在 entity `%s` 的 capability.pushdown 里"
                 % (name, entity.get("id")),
                 "可用参数：%s。其余条件只能拉回来自己筛（或在 AM 里加 pushdown 声明）。"
                 % available)
        canonical = allowed[name.lower()]
        spec = bindings.get(canonical) if isinstance(bindings.get(canonical), dict) else {}
        params[canonical] = coerce_param_value(value, spec)
    return params


def check_required(entity, params, plan, source):
    """`request.required` 里的名字必须有值，否则**发请求之前**就失败（RECIPE §五）。"""
    required = (entity.get("request") or {}).get("required") or []
    defaults = _defaults_of(source, entity)
    bindings = (entity.get("request") or {}).get("bindings") or {}
    missing = []
    for name in required:
        if name in params:
            continue
        spec = bindings.get(name) or {}
        if spec.get("source") == "paging" or (plan is not None and name.lower() in ("offset", "limit", "page")):
            continue
        if spec.get("source") == "default" and _lookup_default(defaults, name) is not MISSING:
            continue
        if spec.get("source") == "filter":
            missing.append(name)
    if missing:
        fail("am.invalid", EXIT_INPUT,
             "entity `%s` 的 request.required 里有 %s，这次没给值 —— 不发请求"
             % (entity.get("id"), "、".join("`%s`" % m for m in missing)),
             "补 `--param <名>=<值>`（该 entity 的 pushdown 允许：%s）。"
             "这类业务标识（业务主键、时间区间…）必须由你从别处拿到，平台不替你猜。"
             % (", ".join(str(x) for x in (entity.get("capability") or {}).get("pushdown") or []) or "（空）"))


def run_fetch(am, am_path, args):
    """取数主流程（demand §4.2 的 8 步，顺序不许换）。"""
    # 1) 校验 AM（结构 + 未签）
    report = validate_am(am, check_fixtures=False, am_path=am_path)
    if not report.ok:
        raise first_error(report)

    entity = find_entity(am, args.entity)
    if not entity_is_signed(entity):
        fail("am.unsigned", EXIT_INPUT,
             "entity `%s` 还没签（reviewed_by 为空）—— 未签的映射不许取数（R6）"
             % entity.get("id"),
             "让映射作者补 `reviewed_by`（形如 `name@YYYY-MM-DD`）并重新签；"
             "或换一个已签的 entity。")
    source = find_source(am, entity.get("source"))
    # `@id` 只有 jsonld 输出才需要；`--format rows` 是平表，⛔ 不该因为缺主键就拒
    # （2026-10-27 实测出来的：`--recipe --format rows` 被这条挡了，与设计矛盾）
    if args.format == "jsonld" and not (entity.get("identity") or {}).get("key_path"):
        fail("am.invalid", EXIT_INPUT,
             "entity `%s` 没有 identity.key_path，无法把一行变成 `@id`" % entity.get("id"),
             "补 `\"identity\": {\"key_path\": \"<记录里的键名>\"}`，或用 `--format rows` "
             "（平表不需要主键）；⛔ 不要拿 fields 里随便一个属性当主键（RECIPE §六）。")
    plan = paging_plan(entity)
    params = parse_params(args.param, entity)
    check_required(entity, params, plan, source)

    # 2) 凭据：只从 auth.credential_env 命名的环境变量读
    credentials = None
    if not args.dry_run:
        sid, key = load_credentials(source)
        credentials = (sid, key)
    profile = auth_profile(entity, source)

    # 3) 时间戳（固定值可注入，供确定性对拍）
    timestamp = args.timestamp or int(time.time())

    # 4–6) 请求 + 翻页 + 切行
    if args.dry_run:
        fixture = entity.get("fixture")
        if not fixture:
            fail("am.invalid", EXIT_INPUT,
                 "entity `%s` 没有 `fixture`，`--dry-run` 没有可用的离线数据"
                 % entity.get("id"),
                 "去掉 `--dry-run` 走真接口，或让运维补一份夹具。")
        transport = FixtureTransport(resolve_fixture_path(fixture, am_path))
    else:
        transport = HttpTransport(timeout=args.timeout)

    max_rows = args.max_rows
    if max_rows <= 0:
        fail("am.invalid", EXIT_INPUT, "--max-rows 必须 >= 1（收到 %r）" % max_rows,
             "想全取就给个大值，例如 `--max-rows 5000`；但注意 `capability.max_scan_rows`。")
    result = fetch_rows(entity, source, profile, transport, limit=args.limit,
                        max_rows=max_rows, params=params, timestamp=timestamp,
                        dry_run=args.dry_run, credentials=credentials)

    # 7) 映射成 JSON-LD
    graph, skipped = to_graph(entity, result.rows)

    # 8) 输出
    output = build_output(entity, source, am, result, graph, skipped,
                          args.format, args.dry_run)
    emit_json(output, args.out)
    if result.truncated:
        sys.stderr.write(redact(
            "[rows.truncated] 没取全：rows=%d, pages=%d, total=%s，原因：%s。\n"
            "修：调大 --max-rows 或收窄 --param 范围；⛔ 别把 truncated=true 当取全了。\n"
            % (len(graph), result.pages, result.total,
               _stop_reason_text(result.stop_reason))))
        return EXIT_TRUNCATED
    return EXIT_OK


# --------------------------------------------------------------------------- #
# 子命令：--selftest（离线；不联网、不要凭据）
# --------------------------------------------------------------------------- #

#: 签名向量的**数据**在 `fixtures/auth-vector.json`（含主机名/服务名这类
#: 「那份规格里的值」）。脚本只放**加载与对拍**的代码：向量里写哪个 mode，就用
#: 注册表里哪个签名器去签 —— 换一种算法做向量，脚本同样不用改。
AUTH_VECTOR_FIXTURE = "auth-vector.json"


def load_auth_vector(selftest):
    """读签名向量；读不到就让自检失败（⛔ 不静默跳过签名验证）。"""
    path = _mini_fixture(selftest, AUTH_VECTOR_FIXTURE)
    if not path:
        return None
    try:
        return read_json_file(path, "签名向量")
    except FetchError as exc:
        selftest.check("签名向量可读", False, exc.rendered())
        return None


class SelfTest(object):
    """极简断言器：逐条跑、逐条打印，最后给总结（不依赖 unittest）。"""

    def __init__(self):
        self.passed = 0
        self.failed = 0

    def check(self, label, condition, detail=""):
        """断言一条；成功打 ✓，失败打 ✗ 并记数。"""
        if condition:
            self.passed += 1
            print("  ✓ %s" % label)
        else:
            self.failed += 1
            print("  ✗ %s%s" % (label, ("  ← " + detail) if detail else ""))
        return bool(condition)

    def equal(self, label, actual, expected):
        """断言相等（失败时打印实际值，方便对拍）。"""
        return self.check(label, actual == expected,
                          "实际 %r ≠ 期望 %r" % (actual, expected))


def _import_whitelist():
    """标准库清单：优先用 `sys.stdlib_module_names`（3.10+），否则用保守白名单。"""
    names = getattr(sys, "stdlib_module_names", None)
    if names:
        return set(names)
    return set("""
        argparse ast base64 codecs collections contextlib copy datetime decimal
        email functools glob gzip hashlib hmac http io itertools json locale
        logging math operator os pathlib pickle pprint queue random re select
        shutil signal socket sqlite3 ssl string struct subprocess sys tempfile
        textwrap threading time token tokenize traceback types typing unicodedata
        unittest urllib uuid warnings zlib
    """.split())


def import_inventory(source_path=None):
    """用 `ast` 列出本文件 import 的顶层模块（AC-008 的机器判据）。"""
    path = source_path or os.path.abspath(__file__)
    with io.open(path, "r", encoding="utf-8") as fh:
        tree = ast.parse(fh.read(), filename=path)
    modules = set()
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            for alias in node.names:
                modules.add(alias.name.split(".")[0])
        elif isinstance(node, ast.ImportFrom):
            if node.level == 0 and node.module:
                modules.add(node.module.split(".")[0])
    return sorted(modules)


def assert_imports_stdlib(selftest):
    """AC-008：import 清单必须全部属于标准库；返回清单供打印。"""
    modules = import_inventory()
    allowed = _import_whitelist()
    offenders = [m for m in modules if m not in allowed]
    selftest.check("import 全部属于标准库（%d 个：%s）" % (len(modules), ", ".join(modules)),
                   not offenders, "越界：%s" % ", ".join(offenders))
    return modules, offenders


def _selftest_auth(selftest):
    """AC-003 / AC-006：签名向量逐字节相等（向量数据在 `fixtures/auth-vector.json`）。

    脚本这边只按向量里的 `mode` 去注册表取签名器 —— 换一种算法做向量，
    这里同样不用改。
    """
    print("\n[1] 签名向量（fixtures/%s；固定凭据 / 时间戳 / body）"
          % AUTH_VECTOR_FIXTURE)
    vector = load_auth_vector(selftest)
    if not vector:
        return None
    mode = vector.get("mode")
    signer = SIGNERS.get(mode) if isinstance(mode, str) else None
    selftest.check("向量里的 mode=%r 已在注册表里" % (mode,), signer is not None,
                   "已注册：%s" % signers_text())
    if signer is None:
        return None

    body = json.dumps(vector.get("body"),
                      ensure_ascii=bool(vector.get("body_ensure_ascii", True))
                      ).encode("utf-8")
    headers = dict(vector.get("headers") or {})
    profile = signer.profile(vector.get("auth") or {}, source_id="<selftest>")
    signed = signer.sign(
        tuple(vector.get("credentials") or ()),
        method=vector.get("method") or "POST",
        uri=vector.get("uri") or "/",
        query=vector.get("query") or "",
        headers=headers,
        body_bytes=body,
        timestamp=vector["timestamp"],
        profile=profile,
    )
    print("  body 字节 = %s" % body.decode("utf-8"))
    print("  规范请求串（\\n 显示为换行）：")
    for line in signed["canonical_request"].split("\n"):
        print("    |%s" % line)
    print("  HashedRequestPayload = %s" % signed["hashed_request_payload"])
    print("  SecretDate / SecretService / SecretSigning = %s / %s / %s"
          % (signed["SecretDate"], signed["SecretService"], signed["SecretSigning"]))
    print("  HashedCanonicalRequest = %s" % signed["hashed_canonical_request"])
    print("  待签名字符串：")
    for line in signed["string_to_sign"].split("\n"):
        print("    |%s" % line)
    print("  Signature = %s" % signed["signature"])
    print("  Authorization = %s" % signed["authorization"])

    expected = vector.get("expected") or {}
    for key in ("hashed_request_payload", "SecretDate", "SecretService",
                "SecretSigning", "hashed_canonical_request", "signature"):
        selftest.equal(key, signed.get(key), expected.get(key))
    selftest.equal("Authorization（全串逐字节）", signed["authorization"],
                   vector.get("expected_authorization"))
    if vector.get("expected_scope"):
        selftest.equal("scope", signed["scope"], vector["expected_scope"])
    return vector


def _selftest_jsonpath(selftest):
    """JSONPath 子集：两种形状能取，越界的写法必须拒绝。"""
    print("\n[2] JSONPath 子集（`$.a.b[*]` / `$.a.b`）")
    doc = {"Response": {"Set": [{"id": 1}, {"id": 2}], "TotalCount": 2,
                        "Nested": {"Deep": "x"}}}
    rows = parse_jsonpath("$.Response.Set[*]", "test").resolve(doc)
    selftest.equal("$.Response.Set[*] 取到 2 行", len(rows or []), 2)
    selftest.equal("$.Response.TotalCount 取到标量",
                   parse_jsonpath("$.Response.TotalCount", "test").resolve(doc), 2)
    selftest.equal("$.Response.Nested.Deep 逐层下钻",
                   parse_jsonpath("$.Response.Nested.Deep", "test").resolve(doc), "x")
    selftest.check("路径断了返回 MISSING（不抛错）",
                   parse_jsonpath("$.Response.Nope[*]", "test").resolve(doc) is MISSING)
    for bad in ("$.Response.Set[0]", "$.Response..Set", "$.Response.Set[*].id", "Response.Set"):
        try:
            parse_jsonpath(bad, "test")
            ok = False
        except FetchError as exc:
            ok = exc.code == "am.invalid"
        selftest.check("拒绝不支持的写法 %r（am.invalid）" % bad, ok)


def _selftest_pruning(selftest):
    """剪枝：缺值的占位符/数组元素怎么剪（RECIPE §三）。"""
    print("\n[3] 模板剪枝（RECIPE §三）")
    values = {
        "zone": {"value": "ap-guangzhou-5", "origin": "--param"},
        "region": {"value": "ap-guangzhou", "origin": "defaults"},
    }
    template = {"Region": "{region}",
                "Filters": [{"Name": "zone", "Values": ["{zone}"]},
                            {"Name": "vpc-id", "Values": ["{vpc_id}"]}]}
    body = render_template(template, values, "test")
    selftest.equal("缺 {vpc_id} ⇒ 整条过滤器剪掉（只剩 1 条）", len(body["Filters"]), 1)
    selftest.equal("有值的过滤器保留", body["Filters"][0]["Values"], ["ap-guangzhou-5"])
    selftest.equal("顶层对象只丢坏的键", body.get("Region"), "ap-guangzhou")
    body2 = render_template({"A": "{missing}"}, values, "test")
    selftest.equal("占位符没值 ⇒ 该键剪掉", body2, {})
    body3 = render_template({"X": ["{missing}"]}, values, "test")
    selftest.equal("数组被剪空 ⇒ 整段剪掉", body3, {})
    try:
        render_template({"Bad": "prefix-{zone}"}, values, "test")
        ok = False
    except FetchError as exc:
        ok = exc.code == "am.invalid"
    selftest.check("占位符拼在字符串中间 ⇒ 拒绝（am.invalid）", ok)


def _mini_fixture(selftest, name):
    """读 skill 自带的 fixtures/ 文件（**按脚本自身位置**解析，⛔ 不看 cwd）。"""
    path = os.path.join(FIXTURES_DIR, name)
    if not os.path.isfile(path):
        selftest.check("fixtures/%s 存在" % name, False, "缺文件：%s" % path)
        return None
    return path


def _selftest_paging(selftest, am, am_path, credentials=()):
    """AC-004：2 页 / 150 行；`--max-rows 120` ⇒ truncated + exit 8。"""
    print("\n[4] 分页（fixtures/paged-*.json：2 页、TotalCount=150）")
    page1 = _mini_fixture(selftest, "paged-page1.json")
    page2 = _mini_fixture(selftest, "paged-page2.json")
    if not page1 or not page2:
        return
    payloads = [read_json_file(page1, "paged-page1"), read_json_file(page2, "paged-page2")]
    selftest.equal("第 1 页 75 行", len(payloads[0]["Response"]["MiniVmSet"]), 75)
    selftest.equal("第 2 页 75 行", len(payloads[1]["Response"]["MiniVmSet"]), 75)
    selftest.equal("TotalCount", payloads[0]["Response"]["TotalCount"], 150)

    entity = find_entity(am, "MiniVm")
    source = find_source(am, entity["source"])
    profile = auth_profile(entity, source)
    params = parse_params([], entity)

    transport = ListTransport(payloads)
    result = fetch_rows(entity, source, profile, transport, limit=None, max_rows=1000,
                        params=params, timestamp=1551113065,
                        credentials=credentials)
    selftest.equal("恰 2 次请求", transport.calls, 2)
    selftest.equal("rows", len(result.rows), 150)
    selftest.check("truncated=false", result.truncated is False)
    graph, skipped = to_graph(entity, result.rows)
    selftest.equal("映射后 150 个节点、0 跳过", (len(graph), skipped), (150, 0))
    selftest.equal("第 1 行 @id 对得上 iri_template",
                   graph[0]["@id"], entity["iri_template"].replace("{id}", "vm-0001"))
    print("    第 1 行：%s" % json.dumps(graph[0], ensure_ascii=False))
    print("    第 150 行 @id：%s" % graph[-1]["@id"])

    transport2 = ListTransport(payloads)
    result2 = fetch_rows(entity, source, profile, transport2, limit=None, max_rows=120,
                         params=params, timestamp=1551113065,
                         credentials=credentials)
    selftest.equal("--max-rows 120 ⇒ rows", len(result2.rows), 120)
    selftest.equal("--max-rows 120 ⇒ 停在第 2 页", result2.pages, 2)
    selftest.check("--max-rows 120 ⇒ truncated=true", result2.truncated is True)
    code = EXIT_TRUNCATED if result2.truncated else EXIT_OK
    selftest.equal("--max-rows 120 ⇒ 退出码", code, 8)
    print("    stop_reason = %s" % _stop_reason_text(result2.stop_reason))

    # dry-run 单页夹具（TotalCount=75，正好取全）⇒ 1 次请求、truncated=false
    single = _mini_fixture(selftest, "mini-vm-page1.json")
    if single:
        dry_transport = FixtureTransport(single)
        result3 = fetch_rows(entity, source, profile, dry_transport, limit=None,
                             max_rows=1000, params=params, timestamp=1551113065,
                             dry_run=True)
        selftest.equal("dry-run 单页夹具（TotalCount=75）⇒ 75 行、1 次请求",
                       (len(result3.rows), dry_transport.calls), (75, 1))
        selftest.check("dry-run 取全 ⇒ truncated=false", result3.truncated is False)

    # 夹具只有一页但声明 TotalCount=150：脚本会再问一页、拿到空页，并如实说没取全
    dry_transport2 = ListTransport([payloads[0]])
    result4 = fetch_rows(entity, source, profile, dry_transport2, limit=None,
                         max_rows=1000, params=params, timestamp=1551113065,
                         dry_run=True)
    selftest.equal("单页夹具 + TotalCount=150 ⇒ 75 行、2 次请求（会去要下一页）",
                   (len(result4.rows), dry_transport2.calls), (75, 2))
    selftest.check("单页夹具 + TotalCount=150 ⇒ truncated=true（不静默）",
                   result4.truncated is True)


def _selftest_guards(selftest, am):
    """AC-005 / AC-007 / AC-009 / AC-006：四类「必须拦住」的情形（全离线）。"""
    print("\n[5] 拒绝与错误码（离线）")

    # AC-009 未签 entity
    unsigned_am = json.loads(json.dumps(am))
    unsigned_am["entities"][0]["reviewed_by"] = ""
    report = validate_am(unsigned_am, check_fixtures=False)
    selftest.check("未签 entity ⇒ am.unsigned",
                   any(code == "am.unsigned" for code, _ in report.errors),
                   str(report.errors[:2]))
    selftest.equal("未签 entity ⇒ 退出码", first_error(report).exit_code, 2)

    # AC-007 pushdown
    entity = find_entity(am, "MiniVm")
    try:
        parse_params(["no_such_param=1"], entity)
        rejected = None
    except FetchError as exc:
        rejected = exc
    selftest.check("pushdown 之外的参数 ⇒ param.not_pushdown",
                   rejected is not None and rejected.code == "param.not_pushdown",
                   str(rejected and rejected.code))
    if rejected is not None:
        selftest.check("拒绝时列出可用参数", "region" in rejected.fix, rejected.fix or "")
        selftest.equal("param.not_pushdown ⇒ 退出码", rejected.exit_code, 2)
    kept = parse_params(["region=ap-beijing"], entity)
    selftest.equal("pushdown 里的参数放行", kept, {"region": "ap-beijing"})

    # AC-005 凭据缺失 + 不回显
    env_name = "ONTOK_SELFTEST_DEFINITELY_UNSET"
    os.environ.pop(env_name, None)
    source = find_source(am, entity["source"])
    broken = json.loads(json.dumps(source))
    broken["auth"]["credential_env"] = [env_name, env_name + "_KEY"]
    try:
        load_credentials(broken)
        cred_error = None
    except FetchError as exc:
        cred_error = exc
    selftest.check("凭据环境变量没设 ⇒ credential.missing",
                   cred_error is not None and cred_error.code == "credential.missing")
    if cred_error is not None:
        selftest.equal("credential.missing ⇒ 退出码", cred_error.exit_code, 3)
        selftest.check("错误消息点名变量名", env_name in cred_error.rendered())

    # 脱敏：把一个假 secret 塞进环境变量，确认它不会出现在任何渲染结果里
    probe = "ZZSELFTESTSECRETVALUE0123456789"
    os.environ[env_name] = probe
    os.environ[env_name + "_KEY"] = probe + "-key"
    try:
        load_credentials(broken)
        leak = [probe in redact(text) for text in
                ("凭据读取失败：%s" % probe, "Authorization: %s" % probe)]
        selftest.check("redact() 抹掉凭据明文", not any(leak))
        selftest.check("_SECRET_VALUES 已登记（供全局脱敏）",
                       probe in _SECRET_VALUES or probe + "-key" in _SECRET_VALUES)
    finally:
        os.environ.pop(env_name, None)
        os.environ.pop(env_name + "_KEY", None)

    # AC-006 HTTP 状态 → 固定码
    cases = [(401, "auth.rejected", 4), (403, "auth.rejected", 4),
             (429, "api.throttled", 5), (400, "api.invalid", 6),
             (503, "net.error", 7)]
    for status, want_code, want_exit in cases:
        try:
            fail_for_http_status(status, b'{"Response":{"Error":{"Code":"x"}}}',
                                 "https://example.invalid/")
            got = (None, None)
        except FetchError as exc:
            got = (exc.code, exc.exit_code)
        selftest.equal("HTTP %d ⇒ %s / exit %d" % (status, want_code, want_exit),
                       got, (want_code, want_exit))

    for vendor, want in (("AuthFailure.SignatureFailure", "auth.rejected"),
                         ("RequestLimitExceeded", "api.throttled"),
                         ("InvalidParameter", "api.invalid"),
                         ("MissingParameter", "api.invalid")):
        selftest.equal("厂商码 %s ⇒ %s" % (vendor, want), classify_vendor_error(vendor), want)

    # 网络异常分类（纯函数；真正的超时在 AC-006 的本地假接口里端到端验过）
    timeout_exc = urllib.error.URLError(socket.timeout("timed out"))
    refused_exc = urllib.error.URLError(ConnectionRefusedError(10061, "refused"))
    selftest.equal("URLError(socket.timeout) ⇒ net.timeout",
                   classify_network_error(timeout_exc), "net.timeout")
    selftest.equal("URLError(ConnectionRefused) ⇒ net.error",
                   classify_network_error(refused_exc), "net.error")
    selftest.equal("net.timeout / net.error ⇒ 退出码 7", EXIT_NETWORK, 7)


def _selftest_check(selftest, am):
    """`--check` 对真实/自带 AM 的判定：坏 AM 必须被点名拒绝（AC-001）。"""
    print("\n[6] AM 校验（坏 AM ⇒ 点名拒绝，退出码 2）")
    good = validate_am(am, check_fixtures=False)
    selftest.check("自带 mini AM 通过校验", good.ok, str(good.errors[:2]))

    bad_cases = []
    missing_reviewer = json.loads(json.dumps(am))
    missing_reviewer["entities"][0].pop("reviewed_by", None)
    bad_cases.append(("缺 reviewed_by", missing_reviewer, "am.unsigned"))
    bad_source = json.loads(json.dumps(am))
    bad_source["entities"][0]["source"] = "no-such-source"
    bad_cases.append(("引用不存在的 source", bad_source, "am.invalid"))
    bad_version = json.loads(json.dumps(am))
    bad_version["contract_version"] = 9
    bad_cases.append(("contract_version=9", bad_version, "am.invalid"))
    for label, broken, want_code in bad_cases:
        report = validate_am(broken, check_fixtures=False)
        codes = [code for code, _ in report.errors]
        selftest.check("%s ⇒ %s" % (label, want_code), want_code in codes, str(codes))
        if report.errors:
            selftest.equal("%s ⇒ 退出码 2" % label, first_error(report).exit_code, 2)

    # REQ-168：契约版本白名单 = (1, 2)。v2 不再当成「坏版本」，3+ 仍拒且列出支持列表。
    v2_am = json.loads(json.dumps(am))
    v2_am["contract_version"] = 2
    v2_report = validate_am(v2_am, check_fixtures=False)
    selftest.check("contract_version=2（v2）通过校验：不再报版本错",
                   v2_report.ok, str(v2_report.errors[:2]))

    v3_am = json.loads(json.dumps(am))
    v3_am["contract_version"] = 3
    v3_report = validate_am(v3_am, check_fixtures=False)
    version_msgs = [msg for code, msg in v3_report.errors if code == "am.invalid"]
    selftest.check("contract_version=3 ⇒ am.invalid 且列出支持版本 1, 2",
                   any("1, 2" in msg for msg in version_msgs),
                   str(v3_report.errors[:2]))


def _selftest_signers(selftest, am):
    """AC-004 / AC-005 / AC-008：签名器注册表、未知 mode、必填 auth 键、码表。"""
    print("\n[7] 签名器注册表与 auth 契约（离线）")

    modes = supported_modes()
    selftest.check("注册表非空（%s）" % signers_text(), bool(modes))
    selftest.check("注册表按 key=MODE 登记",
                   all(SIGNERS[m].MODE == m for m in modes),
                   str({m: SIGNERS[m].MODE for m in modes}))
    selftest.check("注册表里至少有一个签名器能签 tc3-hmac-sha256（AUTH-SPEC 的实现）",
                   "tc3-hmac-sha256" in modes, signers_text())

    # AC-004：未知 mode ⇒ auth.unsupported + 列出支持列表 + 退出码 4，⛔ 不 fallback
    try:
        signer_of("hmac-sha1")
        unsupported = None
    except FetchError as exc:
        unsupported = exc
    selftest.check("未注册的 auth.mode ⇒ auth.unsupported",
                   unsupported is not None and unsupported.code == "auth.unsupported",
                   str(unsupported and unsupported.code))
    if unsupported is not None:
        selftest.equal("auth.unsupported ⇒ 退出码", unsupported.exit_code, 4)
        selftest.check("错误消息列出支持列表",
                       "tc3-hmac-sha256" in unsupported.rendered(),
                       unsupported.rendered())

    # --check 那条路也走同一个判定（不是只有取数路径才拒）
    bad_mode = json.loads(json.dumps(am))
    bad_mode["sources"][0]["auth"]["mode"] = "hmac-sha1"
    report = validate_am(bad_mode, check_fixtures=False)
    selftest.check("--check 对未知 mode ⇒ auth.unsupported",
                   any(code == "auth.unsupported" for code, _ in report.errors),
                   str([code for code, _ in report.errors]))
    selftest.equal("--check 对未知 mode ⇒ 退出码", first_error(report).exit_code, 4)
    selftest.check("--check 的拒绝也列出支持列表",
                   "tc3-hmac-sha256" in report.errors[0][1], report.errors[0][1])

    # AC-005：缺 auth.service ⇒ am.invalid 并点名字段（⛔ 不从 host 猜）
    no_service = json.loads(json.dumps(am))
    no_service["sources"][0]["auth"].pop("service", None)
    report = validate_am(no_service, check_fixtures=False)
    selftest.check("缺 auth.service ⇒ am.invalid 且点名 auth.service",
                   any(code == "am.invalid" and "auth.service" in message
                       for code, message in report.errors),
                   str(report.errors[:3]))
    selftest.equal("缺 auth.service ⇒ 退出码", first_error(report).exit_code, 2)
    selftest.check("缺 auth.service ⇒ 消息里给补法",
                   "service" in report.errors[0][1], report.errors[0][1])
    try:
        auth_profile(None, no_service["sources"][0])
        profile_error = None
    except FetchError as exc:
        profile_error = exc
    selftest.check("取数路径缺 auth.service ⇒ am.invalid 点名 auth.service",
                   profile_error is not None
                   and profile_error.code == "am.invalid"
                   and "auth.service" in profile_error.rendered(),
                   str(profile_error and profile_error.code))

    # AC-008：码 → 退出码表（既有 9 个码的值一个不动，auth.unsupported 是新增）
    expected_codes = {
        "am.invalid": 2, "am.unsigned": 2, "param.not_pushdown": 2,
        "credential.missing": 3, "auth.rejected": 4, "auth.unsupported": 4,
        "api.throttled": 5, "api.invalid": 6,
        "net.timeout": 7, "net.error": 7, "rows.truncated": 8,
    }
    selftest.equal("码表 = 9 个既有码 + auth.unsupported（共 %d 个键）"
                   % len(expected_codes), CODE_EXIT, expected_codes)


def _selftest_recipe(selftest):
    """`--recipe`：把服务端 `@pending.get` 的配方合成最小 AM（REQ-168 之后新增的入口）。"""
    print("\n[8] 配方模式（--recipe，离线）")

    get = {
        "class": "demo_vm", "ref": "access#DemoVm@v1",
        "pushdown": ["region"],
        "source": {"base_url": "https://demo.example.invalid", "auth_mode": "tc3-hmac-sha256"},
        "operation": {"action": "DescribeDemo", "method": "POST", "path": "/"},
        "request": {"template": {"Region": "{region}"}},
        "iterator": "$.Response.Set[*]",
        "fields": {"vm_id": "InstanceId", "cpu": "CPU"},      # 配方里是「属性 → 路径字符串」
        "auth": {"credential_env": ["DEMO_ID", "DEMO_KEY"], "service": "demo"},
    }
    am = am_from_recipe(get)
    ent = am["entities"][0]
    selftest.equal("裸配方 ⇒ 1 个 source / 1 个 entity",
                   (len(am["sources"]), len(am["entities"])), (1, 1))
    selftest.equal("class 原样带过来", ent["class"], "demo_vm")
    selftest.equal("id 从 ref 里取（access#<id>@v<n>）", ent["id"], "DemoVm")
    selftest.check("fields 归一化成 AM 形状 {属性: {path}}（配方里是字符串）",
                   ent["fields"].get("vm_id") == {"path": "InstanceId"},
                   str(ent["fields"].get("vm_id")))
    selftest.check("reviewed_by 标注来源（⛔ 不假装本地签过）",
                   str(ent["reviewed_by"]).startswith("server:"), str(ent["reviewed_by"]))

    # 带 `get` 外壳的整个 @pending 条目也能吃
    wrapped = am_from_recipe({"@type": "ontok:demo_vm", "ontok:sourceKind": "api", "get": get})
    selftest.equal("整条 @pending（带 get 外壳）等价于裸配方",
                   wrapped["entities"][0]["class"], "demo_vm")

    # 缺 class ⇒ am.invalid
    try:
        am_from_recipe({"source": {"base_url": "https://x.example.invalid"}})
        e1 = None
    except FetchError as exc:
        e1 = exc
    selftest.check("配方缺 class ⇒ am.invalid", e1 is not None and e1.code == "am.invalid",
                   str(e1 and e1.code))

    # --id-property：key_path 必须是**字符串**（不是 fields 里的 {path:...} 字典）
    # —— agent 实测抓到的 bug：`--id-property instance_id` 报「identity.key_path 为空」
    _v = (ent.get("fields") or {}).get("vm_id")
    _kp = _v["path"] if isinstance(_v, dict) else _v
    selftest.check("--id-property 取到的是路径字符串（⛔ 不是 dict）",
                   isinstance(_kp, str) and _kp == "InstanceId", repr(_kp))

    # 缺 source.base_url ⇒ am.invalid，且提示「该部署可能是 v1」（v1 的 @pending.get 没有配方）
    try:
        am_from_recipe({"class": "c"})
        e2 = None
    except FetchError as exc:
        e2 = exc
    selftest.check("配方缺 source.base_url ⇒ am.invalid", e2 is not None and e2.code == "am.invalid",
                   str(e2 and e2.code))
    selftest.check("缺 base_url 时提示「该部署可能是 v1」",
                   e2 is not None and "v1" in e2.rendered(), "" if e2 is None else e2.rendered()[:80])


def cmd_selftest():
    """`--selftest`：离线自检。**不联网、不要凭据**（AC-010）。

    实现上先把 `socket.socket` / `socket.getaddrinfo` 换成会抛异常的版本：
    自检期间任何联网尝试都会立刻炸掉 —— 这就是「断网可过」的机器证据。
    """
    selftest = SelfTest()
    print("access_fetch.py · self-test（离线：不联网、不要凭据）")
    print("python: %s   platform: %s" % (sys.version.split()[0], sys.platform))

    # 内置夹具目录不在 ⇒ 一行「怎么办」，⛔ 不逐条刷「缺文件」、⛔ 不崩栈。
    # 场景：agent 只把 access_fetch.py 复制走，没带 fixtures/（最常见的用法错误）。
    if not os.path.isdir(FIXTURES_DIR):
        print("\n✗ 找不到内置 fixtures 目录：%s" % FIXTURES_DIR)
        print("  自检要用 fixtures/ 里的签名向量、AM 与分页夹具（不是可选项，⛔ 不能降级跳过）。")
        print("  修：把**整个 ontok-query/ 目录**一起复制过去 —— ⛔ 只复制 access_fetch.py 不够；")
        print("      或者直接把 `--mapping` 指向你自己的 AM、跳过自检：")
        print("      `python <skill目录>/ontok-query/access_fetch.py --mapping <am.json> --check`。")
        print("  注：脚本按**自身所在目录**找 fixtures（与 cwd 无关），所以 `cd` 到别处不影响；")
        print("      但换一个目录复制**就必须连 fixtures/ 一起**。")
        print("\n结果：无法自检（缺内置 fixtures 目录，退出 1）")
        return 1

    original_socket = socket.socket
    original_getaddrinfo = socket.getaddrinfo

    def _no_network(*_args, **_kwargs):
        raise AssertionError("self-test 试图联网（不允许）")

    class _BlockedSocket(original_socket):
        def __init__(self, *args, **kwargs):
            _no_network()

    socket.socket = _BlockedSocket
    socket.getaddrinfo = _no_network
    try:
        print("\n[0] 依赖与网络闸门")
        modules, offenders = assert_imports_stdlib(selftest)
        selftest.check("网络闸门已装（任何 socket 创建/解析都会失败）", True)
        selftest.check("不依赖 requests / urllib3",
                       not ({"requests", "urllib3"} & set(modules)),
                       "发现第三方 HTTP 库")

        vector = _selftest_auth(selftest)
        _selftest_jsonpath(selftest)
        _selftest_pruning(selftest)

        am_path = _mini_fixture(selftest, "mini-am.json")
        if am_path:
            am = load_am(am_path)
            credentials = tuple((vector or {}).get("credentials") or ())
            _selftest_paging(selftest, am, am_path, credentials)
            _selftest_guards(selftest, am)
            _selftest_check(selftest, am)
            _selftest_signers(selftest, am)
        _selftest_recipe(selftest)
    finally:
        socket.socket = original_socket
        socket.getaddrinfo = original_getaddrinfo

    print("\n结果：%d passed, %d failed" % (selftest.passed, selftest.failed))
    if selftest.failed:
        print("SELFTEST FAILED")
        return 1
    print("SELFTEST OK（本脚本没有访问任何 URL；凭据未被读取）")
    return EXIT_OK


# --------------------------------------------------------------------------- #
# 命令行
# --------------------------------------------------------------------------- #

def build_parser():
    """命令行参数（demand §4.1 的契约；多一个 `--timestamp` 便于确定性对拍）。"""
    parser = argparse.ArgumentParser(
        prog="access_fetch.py",
        description="按 access mapping（AM）取数：读 AM → 签名 → 分页 → 映射成 JSON-LD。",
        epilog="退出码：0 成功 / 2 AM·未签·参数 / 3 凭据 / "
               "4 鉴权（auth.rejected / auth.unsupported）/ 5 限流 / "
               "6 参数错 / 7 网络 / 8 没取全（truncated）。"
               "已支持的签名器：%s（--list-signers）" % signers_text(),
    )
    parser.add_argument("--mapping", metavar="AM.json",
                        help="access mapping 文件（--check / 取数必填）")
    parser.add_argument("--recipe", metavar="RECIPE.json",
                        help="直接吃服务端 `@pending[].get` 的配方（用 `-` 从 stdin 读）："
                             "v2 契约下查一次本体就有，不需要 AM 文件")
    parser.add_argument("--id-property", metavar="PROP",
                        help="配方里没有 identity：用哪个属性当主键（jsonld 输出必填）")
    parser.add_argument("--entity", metavar="ID|CLASS", help="entity id 或本体 class")
    parser.add_argument("--param", action="append", default=[], metavar="K=V",
                        help="可重复；只允许 capability.pushdown 里的键")
    parser.add_argument("--limit", type=int, default=None, metavar="N",
                        help="每页条数（上限 paging.max_limit）")
    parser.add_argument("--max-rows", type=int, default=DEFAULT_MAX_ROWS, metavar="N",
                        help="最多取多少条（默认 %d；取不全要如实说）" % DEFAULT_MAX_ROWS)
    parser.add_argument("--dry-run", action="store_true", help="用 entity.fixture 取数，不发网络")
    parser.add_argument("--check", action="store_true", help="只校验 AM 结构，不取数")
    parser.add_argument("--selftest", action="store_true", help="离线自检（无网络、无凭据）")
    parser.add_argument("--list-signers", action="store_true",
                        help="打印已注册的签名器 mode（auth.mode 的合法值）")
    parser.add_argument("--format", choices=("jsonld", "rows"), default="jsonld",
                        help="输出形状（默认 jsonld）")
    parser.add_argument("--out", metavar="FILE", help="写到文件（默认 stdout）")
    parser.add_argument("--timeout", type=int, default=DEFAULT_TIMEOUT, metavar="SEC",
                        help="单次请求超时（默认 %d 秒）" % DEFAULT_TIMEOUT)
    parser.add_argument("--timestamp", type=int, default=None, metavar="UNIX",
                        help="固定时间戳（只用于对拍/复现，正常别传）")
    return parser


def main(argv=None):
    """入口：选子命令（selftest / check / fetch），统一处理错误与退出码。"""
    # Windows 控制台默认不是 UTF-8，中文会花；能改就改（改不了也不影响 JSON）
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(encoding="utf-8")
        except (AttributeError, ValueError):
            pass

    parser = build_parser()
    args = parser.parse_args(argv)

    if args.selftest:
        return cmd_selftest()

    if args.list_signers:
        # 一行一个 mode，方便 `--list-signers | grep` 这类用法
        for mode in supported_modes():
            print(mode)
        return EXIT_OK

    if args.recipe:
        # ⭐ 从「服务端给的配方」取数：不需要 AM 文件
        try:
            if args.recipe == "-":
                try:
                    recipe = json.loads(sys.stdin.read())
                except ValueError as exc:
                    fail("am.invalid", EXIT_INPUT, "stdin 不是合法 JSON：%s" % exc,
                         "把 `@pending[0]` 或它的 `get` 整段传进来。")
            else:
                recipe = read_json_file(args.recipe, "配方文件")
            am = am_from_recipe(recipe)
            if args.id_property:
                props = am["entities"][0]["fields"] or {}
                if args.id_property not in props:
                    fail("am.invalid", EXIT_INPUT,
                         "--id-property=%s 不在配方的 fields 里（有：%s）"
                         % (args.id_property, ", ".join(sorted(props)[:8])),
                         "用 fields 里的属性名。")
                # fields 归一化后是 {属性: {"path": "<记录内路径>"}} ⇒ 要取里面的 path，
                # ⛔ 不能把整个 dict 塞进 key_path（agent 实测抓到的 bug：key_path 成 dict，
                #    校验报「identity.key_path 为空」⇒ --id-property 这条线不可用）
                _v = props[args.id_property]
                am["entities"][0]["identity"] = {"key_path": _v["path"] if isinstance(_v, dict) else _v}
            elif args.format == "jsonld" and not args.check:
                fail("recipe.no_identity", EXIT_INPUT,
                     "配方里没有 identity，jsonld 输出无法生成 @id",
                     "加 `--id-property <属性名>`，或用 `--format rows`。")
            if args.check:
                return cmd_check(am, args.recipe, args.entity)
            if args.dry_run:
                fail("recipe.no_fixture", EXIT_INPUT,
                     "配方里没有 fixture，`--dry-run` 无从试跑",
                     "去掉 `--dry-run` 走真取数（需要凭据），或改用 `--mapping <带 fixture 的 AM>`。")
            return run_fetch(am, args.recipe, args)
        except FetchError as exc:
            print_err(exc)
            return exc.exit_code

    if not args.mapping:
        parser.error("--mapping 必填（除非用 --selftest / --recipe / --list-signers）")
    am_path = args.mapping
    try:
        am = load_am(am_path)
        if args.check:
            return cmd_check(am, am_path, args.entity)
        if not args.entity:
            parser.error("取数需要 --entity（或用 --check 只体检）")
        return run_fetch(am, am_path, args)
    except FetchError as exc:
        print_err(exc)
        return exc.exit_code
    except KeyboardInterrupt:
        sys.stderr.write("已中断\n")
        return 130
    except SystemExit:
        raise
    except Exception as exc:  # 兜底：脱敏后打栈，退出 1（不算取数错误）
        sys.stderr.write(redact("脚本内部错误：%s\n%s\n"
                                % (exc, traceback.format_exc())))
        return 1


if __name__ == "__main__":
    sys.exit(main())

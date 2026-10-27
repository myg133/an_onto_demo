# 需求：REQ-163 · 构建端「纯本地链」收口（ACP 尾步、出包 CLI、TTL 归属）

## 基本信息

| 项 | 值 |
| --- | --- |
| 编号 | REQ-163 |
| 优先级 | **P1** |
| 状态 | **草稿**（等 REQ-162 合并后细化） |
| 触发 | skills 更新时暴露的三处「口径 vs 代码」不一致（2026-10-27 15:40） |
| 关联 | REQ-160（`review open`）、REQ-162（console v2）、ADR-013 |

---

## 三件事（都来自 skills 收口时的矛盾）

### 1 · `build run` 的尾步该起哪個评审台（**最主要**）

| 现状 | 口径要求 |
| --- | --- |
| `build run` 四段跑完后**无条件调 `review_open::open`**（REQ-160：连库 + 一次性 admin token） | 用户 14:44：「纯本地构建，不需要口令」「console 的核心功能是评审」 ⇒ 尾步应起 **console v2**（`--dir`） |

⇒ 要让 ACP 模式真正做到"一条命令到评审台"，`build run` 必须：
1. 四段产物落到**工作目录**（`<dir>/out/proposal.json`），不再写库
2. 尾步 `console serve --dir <dir>`（或用同一进程内的 v2 路由）
3. ⛔ 不再依赖 `ONTOK_SECRET_KEY` / `ONTOK_DATABASE_URL`

> 这条不改，ACP 模式就永远会掉回"要口令 + 要库"的旧台。

### 2 · 补一个「从工作目录定版出包」的 CLI

| 现状 | 需要的 |
| --- | --- |
| 页面能导出（`POST /api/bundle` → `.ontok/bundles/<dir>-vN.ontokbundle`） | CI / 无头环境需要一条**命令** |

建议形态：

```
ontok-builder bundle pack --dir <工作目录> [--version vN] --out <路径>
```

（`--project/--version` 那套走库的旧形态保留，但明确标旧。）

### 3 · TTL / R2RML 导出的归属

`ontology export --project --version` 与「纯本地/无项目/无库」冲突。
skills 里已降级为「旧 DB 链 · 附加」，但**归属没定**：

| 选项 | 含义 |
| --- | --- |
| A | 构建端彻底不出 TTL/R2RML —— 属服务端/Studio（基于本体 + 源库描述生成） |
| B | 保留但在"定版 → 导出包"之外单列一条"给服务端用的附加导出" |

**BA 倾向 A**（构建端只出包，越简单越不容易出错），等 join 的 P1c-3 落地后再定。

---

## 验收（待细化）

- `build run` 全程 **不设** `ONTOK_DATABASE_URL` / `ONTOK_SECRET_KEY` 也能跑通并起 v2
- `bundle pack --dir` 出的包与页面导出**逐字节相同**（同一定版、同一 `--built-at`）
- skills 里不再出现"尾步落到旧 review open"的兼容提示（那条提示本身就是本单要消灭的）

# 安装 ontok-builder（给 agent 的执行说明）

> **你是谁**：你是帮 FDE 安装 `ontok-builder` 的 agent。
> **目标**：装完之后 `ontok-builder --version` 能跑，且版本号等于 `INSTALL.md` 顶部写的那一个。
> **你不需要 git、不需要 Rust、不需要 clone 任何仓库。**

---

## 〇 · 先确认版本目标

本次要装的版本：**{{VERSION}}**

> 后面每条校验都以它为准。装出来的版本比它低 ⇒ 装的是旧包；比它高 ⇒ 有人先发过新版，**停下来问人**。

---

## 一 · 检查前置（30 秒）

| 检查 | 命令 | 通过条件 |
| --- | --- | --- |
| Node | `node --version` | **≥ 18**（npm 自带） |
| npm | `npm --version` | 有输出即可 |
| 网络 | `npm ping` | 能连上 registry（内网镜像也行） |

**任一条不通过就停**，别继续装 —— 装到一半失败比没装更难查。

---

## 二 · 安装

```bash
npm i -g @anspire-ai/ontok-builder --include=optional
```

**`--include=optional` 不能省**：主包只是一个几十行的转发器，真正的二进制在**平台包**里
（`@anspire-ai/ontok-builder-win32-x64` / `-darwin-arm64` / `-linux-x64`），靠 `optionalDependencies` 按平台装。

支持平台：**Windows x64 · macOS arm64 · Linux x64**。
其它平台（Linux arm64 / macOS x64）**没有发布** —— 遇到就直接告诉人，别自己找替代。

### 没有全局权限时

```bash
npm i @anspire-ai/ontok-builder --include=optional      # 装到当前项目
npx ontok-builder --version                          # 用 npx 调
```

---

## 三 · 校验（必做）

```bash
ontok-builder --version
```

期望输出：

```
ontok-builder {{VERSION}}
```

| 现象 | 含义 | 怎么办 |
| --- | --- | --- |
| `command not found` / `不是内部或外部命令` | npm 的全局 bin 不在 PATH | 用 `npx ontok-builder --version`；或查 `npm bin -g` 把那个目录加进 PATH |
| 版本号不是 `{{VERSION}}` | 装到了旧版本/镜像没同步 | `npm i -g @anspire-ai/ontok-builder@{{VERSION}}` 显式指定 |
| `✗ ontok-builder: 缺平台包 ...` | optional 依赖被跳过了 | `npm i -g @anspire-ai/ontok-builder --include=optional` 重装 |
| `不支持的平台：<os>-<arch>` | 不在支持列表 | 停下来告诉人，⛔ 不要自己编二进制 |

---

## 四 · 把 skill 放到 agent 的 skills 目录

`ontok-builder` 只是**工具**；agent 要会干活，还得有 skill。三个目录（随交付一起给）：

| 目录 | 给谁 | 放哪 |
| --- | --- | --- |
| `ontok-extract/` | **抽取 agent**（把文档变成断言） | agent 的 skills 目录 |
| `ontok-relate/` | **判定 agent**（判断两张表有没有关系） | agent 的 skills 目录 |
| `ontok-query/` | **数字员工 / 查询 agent**（含 `scripts/access_fetch.py`） | agent 的 skills 目录 |

```bash
# 例：Claude Code / 通用 <agent-skills-dir>
cp -r skill/ontok-* <agent-skills-dir>/
```

⚠️ **整目录拷**，别只拷 `SKILL.md`：`ontok-query/scripts/access_fetch.py` 是它的一部分，
少了脚本，agent 会自己去手写 TC3 签名（那就白做了）。

---

## 五 · 装完自检（三条，都过才算装好）

```bash
ontok-builder --version            # 1) 版本对
ontok-builder doc ingest --help    # 2) 子命令能跑（能看到 --agent acp）
ontok-builder relation verbs check --help   # 3) 表间关系那条链在
```

三条都过 ⇒ 告诉人「已装好，版本 {{VERSION}}」，然后按 `fde-guide.md` 开工。

---

## 六 · ⛔ 不要做的事

| 别做 | 为什么 |
| --- | --- |
| `git clone` 本仓库 | 你不需要源码；仓库里的路径在你的机器上不存在 |
| `cargo install` / 自己编译 | 要装 Rust 工具链，且没有发布签名 |
| 从 GitHub Releases 下二进制 | 内网常常拿不到；npm 平台包才是正路 |
| 忽略版本号"先跑起来再说" | 版本不对时，命令参数会悄悄不一样（我们改过好几轮参数） |
| 把 skill 目录拆开只留 `SKILL.md` | `scripts/` / `fixtures/` 也是契约的一部分 |

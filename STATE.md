# 当前状态（给下一个 BA 的交接）

> 最后更新：2026-10-30 11:10 · 交接人 **join**（服务端 / Studio / 契约 / Deploy）· 本会话已**注销**
> 维护约定：每次发版或环境变更后更新本文件；⛔ 不要写「应该」「大概」，只写**已核实**的事实与**待办**。

## 一 · 一句话

三套环境在跑（UAT `0.3.7` / QA `0.3.8` / 生产 `0.3.2`），`develop` 已到 **`0.3.10`**（lion 的 npm 发布链）。
UAT 上 18 套客户本体全部可打开；已知红线 2 条（见 §五）。

## 二 · 环境

| 环境 | 地址 | 镜像 | 模式 | 白名单 | 数据 |
| --- | --- | --- | --- | --- | --- |
| **UAT** | `https://an-notol.mic-scfw.cloudns.org` | **0.3.7** | **多项目** + `--ontology-only` | ✅ 全 14 词 | `ontok` 库 · 18 个项目 |
| **QA** | `https://an-onto-qa.local.anspire.cn` | **0.3.8** | 多项目 + `--ontology-only` | ✅ 全 14 词 | `ontok_qa` 库 |
| **生产** | `https://turing.anchnet.com` | **0.3.2** | 多项目 | ❌ 无 | 用户自管 |

- **凭据不在仓库里**，在同机 `.local/`：`uat-admin-token.txt` / `qa-admin-token.txt` / `*-ontok-secret-key.txt` / `*-query-tokens.json`
- UAT 的 **token 表**（`kubectl -n anspire-ontok-uat get secret ontok-secrets`）里 `partner-ops360` 绑 `project=cloud-resource`，`fde-admin-uat` 是 admin（**不绑项目**，多项目模式下这是硬要求）
- 两套环境的 `--access-mapping` 都**已移除**（REQ-177：多项目 + 文件 AM ⇒ 启动即拒）；AM 现在**随版本入库**（`version_access_mappings`）

## 三 · 分支与 worktree

```
worktree : an_onto_demo | BA | code | Deploy   （4 个，干净）
develop  : 96c1c70（= origin）   ← 含 npm 发布链收尾，版本号 0.3.10
deploy   : 9826b09（= origin）
demand   : 本地可能领先 origin（提交后记得 push）
未合并   : feature/REQ-147（lion 的 ACP 抽取，3 个提交）
```

## 四 · 最近上线了什么（按版本）

| 版本 | 内容 | 影响 |
| --- | --- | --- |
| **0.3.4** | REQ-174 多项目：每套本体带自己的 AM | 多项目落地 |
| **0.3.5** | REQ-175 建项目 key 能鉴权 · REQ-176 业务词白名单 `--term-allowlist` · REQ-177 多项目禁 `--access-mapping` · 基数等价写法 `N:N/M:N/M:M` · **镜像 305 MB → 23 MB**（删 bun/pi） | 客户能自助建项目；闸门可放宽 |
| **0.3.6** | `skos:altLabel` 判据**按块作用域**（`owl:Class` 下的实体别名不算"未翻译语义"） | `yiyao`/`anquan-hegui` 等不再 500 |
| **0.3.7** | REQ-178 **TTL 两视图按需生成**：装配期不渲染，`/api/ontology.ttl` 请求时渲染并按 `version_id` 缓存；失败只挡该端点（新码 **`project.ttl_failed`**） | 一个词写错不再让整个项目 500、也不再挡住重新传包 |
| **0.3.8** | 深色模式对比度修复（`--text-3` 3.99→6.72:1、边框 8.5%→14%、边缘节点 3.62→5.69:1） | QA 上等用户确认 |
| 0.3.9 / 0.3.10 | lion 的 npm 发布链（平台包可执行位、provenance repository、registry 传播轮询） | **只有 npm 侧**，不影响上面的环境 |

## 五 · 待办 / 已知红线

| # | 项 | 状态 | 备注 |
| --- | --- | --- | --- |
| 1 | **`tests/test_req160_review_open.rs` 编不过** | 🔴 未修 | `Options` 已有 `propose`/`relate_skill` 字段，测试文件里 0 处赋值 ⇒ 缺字段报错。挡住 `cargo check --all-targets`，属 lion 面，已报 4 次无人认领 |
| 2 | **`feature/REQ-147` 未合** | 🟡 | lion 的 `doc ingest 接 ACP`，3 个提交 |
| 3 | **REQ-179 本体 × RAG 知识问答** | 📝 草稿 | 口径已定（知识问答 / 向量自建 / evidence 采样+人签 / 原文=抽取语料 / embedding 走外部 API）。核心认知：`description` 只是索引，**断言必须回原文 chunk 并带 `[文件:行]`**；UAT 的 PG **无 pgvector** ⇒ 向量存 `bytea` + Rust 暴力检索 |
| 4 | **UI 深浅色主题** | ❓ 待定 | 用户问"能不能做深浅两种"；已给方案（token 双套 + 图配色做成"主题×语义"表 + 测试从"钉 hex"改成"钉语义与对比度"，约 2.5–3 天）。**需要先定范围**（只 Studio / 含老 console 评审台）与切换方式 |
| 5 | **生产升级** | ⏸ 等用户 | 现 0.3.2；升到 0.3.7+ 要同时挂 `--term-allowlist`（否则 `evidence` 类词仍会 500） |
| 6 | vhdx 压缩 | ⏸ 等用户 | `D:\VM\wsl\podman\ext4.vhdx` 46 GB，VM 内只用 12 GB ⇒ 需**管理员**跑 `Optimize-VHD -Path … -Mode Full`（`podman machine stop` 之后） |

## 六 · 必须知道的红线（踩过坑）

1. **发布闸有两条腿**：① 14 个词（中文字串 / 英文词元）② 形态判据（未翻译的基数记号、关系语义）。
   `--term-allowlist` **只能放宽 ①**。当前 UAT/QA 挂的是**全 14 词**（用户明确"就是要关闸"）⇒ **词表层等于关闸**，只剩形态判据。
2. **配色被测试钉住**：`studio.css` 的色值在 `src/studio/mod.rs`（2 个单测）+ `tests/fixtures/req159e/color_probe.js` 里各有一份镜像，改色必须同步改三处。
3. **多项目模式不许给 `--access-mapping`**（0.3.5+ 启动即拒）；AM 只能随版本入库。老包（REQ-174 之前）**不带 AM**，切多项目前必须先"传带 AM 的包 → 解析 → 激活"。
4. **`import` 对同一 bundle 是幂等早退**：重新上传同一个包不会回填 AM/数据，必须**传新包**。
5. **spawn 自己二进制的集成测试要干净 `CARGO_TARGET_DIR`**：`cargo test` 会把 `debug/ontok.exe` 换成陈旧产物，测试跑的是旧二进制（我用 `.local/t177` 踩过，红假象 20 分钟）。
6. **构建用 `podman build --env CARGO_BUILD_JOBS=4`**：16 核会把 WSL VM 打死（`server probably quit`），单线程 ~20 分钟、4 核 ~8 分钟。
7. **`Deploy/` 与 `code/` 是共用 worktree**：另一个 BA 的 `git add -A` 会扫走你未提交的改动（发生过一次）。约定：只 `git add <自己的文件>`。

## 七 · 常用操作

```powershell
# 发版（示例：QA 升 0.3.8）
# 1) code: bump version + Cargo.lock（两处必须一起改，否则 --locked 失败）
cargo metadata --format-version 1 --locked     # 必须过
# 2) 构建（4 核）
podman build --env CARGO_BUILD_JOBS=4 -t aigccontainer.azurecr.cn/an_onto/ontok:0.3.8 -f Dockerfile .
podman push aigccontainer.azurecr.cn/an_onto/ontok:0.3.8
# 3) 只改目标环境的 overlay newTag，再 apply
kubectl apply -k Deploy\k8s\overlays\qa
kubectl -n anspire-ontok-qa rollout status deploy/ontok-query

# 验环境
curl -s https://an-onto-qa.local.anspire.cn/api/health
# 逐个项目验（admin key 带 ?project=）
# catalog 200 + ontology.ttl 200 才算"项目可打开"

# 发完版清磁盘
podman image prune -f; podman builder prune -f
```

## 八 · 关键文档

| 文档 | 用途 |
| --- | --- |
| `Deploy/NEW-ENV.md` | 新环境 checklist（**必读**：多项目不要 AM、白名单语义、key 语义、四道闸） |
| `Deploy/OPS360-DEPLOY-LOG.md` | 部署日志与四条闸 |
| `Deploy/k8s/overlays/{qa,uat-ops360}/` | 两套 overlay（含 `term-allowlist.txt`） |
| `Deploy/tools/check_am.py` | AM 发布闸（`python Deploy\tools\check_am.py`） |
| `Deploy/skills/ontok-query/` | 产品侧 skill（SKILL.md / AUTH-SPEC.md / RECIPE.md / `scripts/access_fetch.py`） |
| `Deploy/integrations/digital-employee/` | 数字员工对接层（平台约定，**不进产品 skill**） |
| `BA/design/diagrams/build-to-consume.svg` | 构建→消费全链路图（含命令参数） |
| `BA/dispatch/req-registry.md` | 需求台账（当前最大编号 **REQ-179**，下一个 **REQ-180**） |

## 九 · 本次交接做了什么（防重复劳动）

- 清磁盘：`.local` **35.07 GB → 20.9 MB**（删 cargo 缓存 34 GB + 已合并 worktree `feature-REQ-178`）
- 清 podman：镜像 **447 → 11 个 · 35 GB → 3.2 GB**（删 `ontok:0.1.x–0.3.6`、`build033/034` 各 2 GB、悬空层 42 个）
- **保留**：`ontok:0.3.7`（UAT）、`ontok:0.3.8`（QA）、`cloudflared:2026.10.1`、`postgres:16`、rust/alpine 基础镜像
- 未做（需用户）：vhdx 压缩、生产升级、REQ-179 开工、UI 主题开工

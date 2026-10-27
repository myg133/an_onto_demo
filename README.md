# 部署配置（deploy 分支）

**只做部署配置，不做构建。** 镜像由 `code/Dockerfile` 构建（CI 或本机 podman），本分支只描述「怎么部署、部署什么版本」。

| 目录 | 用途 |
| --- | --- |
| `compose/` | 单机 docker-compose（db + query + console） |
| `k8s/` | Kubernetes 清单（kustomize：base + overlays/dev + overlays/prod） |
| `skills/` | ⭐ **agent skill**（`ontok-extract` / `ontok-query` / `mcp.example.json`）—— **只放 agent 加载的东西**（有 `name`/`description` frontmatter） |
| `docs/` | **给人读的文档**（`fde-guide.md` = FDE 构建指南）。⛔ 不是 skill，别装进 agent 目录 |
| `.local/` | 本地临时（不跟踪） |

> ⭐ **2026-10-27：skill 统一回本分支维护**（用户口径，`Deploy` 分支唯一维护处）。
> 历史：2026-10-26 曾从本分支迁到**仓库根 `skills/`**；现在搬回。
> 注意防止漂移的关键**不是放在哪，而是只有一份** —— 仓库根已不再保留副本（workspace 分支的 `.gitignore` 也去掉了 `!/skills/`）。
> 引用当前分支内的 skill 用相对路径：`../skills/...`（从 `digital-employee/` 出发）。

## 镜像

```bash
cd code
podman build -t ontok:dev -f Dockerfile .
```

镜像里只有四个二进制（`ontok` / `ontok-builder introspect` / `ontok-builder propose` / `ontok-builder translate-names`）+ ca-certificates + tzdata，**不含任何声明、口令、证书**。

## compose（单机）

```bash
cd Deploy/compose
cp .env.example .env          # 填源库只读连接串、项目名、版本
# 三份声明 → config/ ；两份口令 → secrets/（都不入库）
docker compose up -d
```

| 服务 | 端口 | 说明 |
| --- | --- | --- |
| `db` | 55432→5432 | **仅元数据库**（本体/提案/快照），源库是外部的 |
| `query` | 8800 | REST `/api/sparql` + MCP `/mcp`（数字员工用） |
| `console` | 8899 | 内部工作台（管理 API + 页面） |

## Kubernetes（kustomize）

```bash
kubectl apply -k Deploy/k8s/overlays/dev
kubectl apply -k Deploy/k8s/overlays/prod   # 先改 overlays/prod 的镜像仓库与 tag
```

| 资源 | 说明 |
| --- | --- |
| `Deployment × 2` | 共用镜像，不同 `args` |
| `Service × 2` | ClusterIP；对外走你们的 Ingress |
| `ConfigMap ontok-config` | 三份声明 + schema.json（**进版本库、可评审**） |
| `Secret ontok-secrets` | 两份口令（示例为空，生产用 sealed-secrets / 外部密钥） |
| `Secret ontok-db` | `metadata-url` 与 `source-url` —— **必须自行创建** |

```bash
kubectl -n ontok create secret generic ontok-db \
  --from-literal=metadata-url='postgresql://user:pass@pg.internal:5432/ontok' \
  --from-literal=source-url='postgresql://readonly:pass@biz.internal:5432/business'
```

## 安全基线（清单里已有，别删）

| 项 | 做法 |
| --- | --- |
| 非 root | `runAsUser: 10001`、`allowPrivilegeEscalation: false` |
| 只读根文件系统 | `readOnlyRootFilesystem: true`（审计日志走 `emptyDir`） |
| 能力收敛 | `capabilities.drop: ["ALL"]` |
| 镜像 tag | 生产**固定版本号**，⛔ 不用 `latest` |
| 口令 | 不进镜像、不进 git |

## 上线顺序（别搞反）

```bash
# 1 建元数据库（compose 的 db，或托管 PG）
# 2 在能连源库的机器上：建项目 → 提议 → 评审 → 应用 → 冻结
ontok-builder project new --name anspire_prod
ontok-builder version new --project anspire_prod
ontok-builder import --file proposal.json
ontok-builder review decide --proposal p1 --project anspire_prod --accept-all
ontok-builder proposal apply  --proposal p1 --project anspire_prod
ontok-builder version freeze  --project anspire_prod --version 3
# 3 导出并归档
ontok-builder ontology export --project anspire_prod --version 3 --model-view --out releases/anspire_prod-v3-20261023/model.ttl
ontok-builder r2rml generate  --project anspire_prod --version 3 --out releases/anspire_prod-v3-20261023/mapping.ttl
# 4 起服务，确认 /api/health 与 /api/catalog
# 5 用两个真实租户做泄漏对拍，再放流量
```

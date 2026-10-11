# 部署配置（deploy 分支）

**只做部署配置，不做构建。**

| 目录 | 用途 |
| --- | --- |
| `apps/` | 各服务的部署清单（helm 等） |
| `environments/` | staging / production 环境配置 |
| `releases/` | 发布快照 |
| `scripts/` | 部署 / 回滚 / 健康检查脚本 |

CI 负责构建镜像；本分支只负责「怎么部署、部署什么版本」。

# ontok 部署 · docker-compose（本机 / 单机）

一个元数据库 + 两个服务（查询 / 控制面），共用同一个镜像、不同的启动命令。

## 目录

```
Deploy/compose/
├── docker-compose.yml
├── .env.example              # 复制成 .env（不入库）
├── config/                   # 声明文件（挂进容器，只读）
│   ├── query-policy.json
│   ├── tenancy.json
│   └── exclusions.json
└── secrets/                  # 口令文件（不入库）
    ├── admin-tokens.json
    └── query-tokens.json
```

## 起

```bash
cd Deploy/compose
cp .env.example .env          # 填数据库连接、项目名、版本
# 把三份声明放进 config/、两份口令放进 secrets/
docker compose up -d
docker compose logs -f query
```

| 服务 | 端口 | 用途 |
| --- | --- | --- |
| `query` | 8800 | REST `/api/sparql` + MCP `/mcp`（数字员工用） |
| `console` | 8899 | 内部工作台（管理 API + 页面） |
| `db` | 5432 | **仅元数据库**（本体/提案/快照）；源库是外部的 |

## 注意

- 源库（业务库）**不在这里**：它是外部只读库，用 `ONTOK_SOURCE_DATABASE_URL` 指向
- 容器内必须绑 `0.0.0.0`，所以两个服务都带 `--allow-public`（启动会打警告，这是预期）
- 口令文件**只挂载、不写进镜像**
- 元数据库首次启动会自动建表（`ontok` 连接时创建）

# 状态：REQ-159 · 构建包导入契约 v1

## 当前状态

**草稿 · 待拍板**（2026-10-27 · join）

## 起点

用户给的流程（第一版）：

> 构建端构建本体、AM、DB 都需要支持，构建完成后打包。
> 服务端创建项目、生成 key，用户手动上传构建包；
> 如果包含 db，弹窗进行 db 配置；完成后服务端检查 db 连通性；
> 没问题后解析构建包，展示 key。

## 契约要点（已写进 demand.md）

| 块 | 内容 |
| --- | --- |
| 包格式 | zip + `manifest.json`（含每文件 sha256）+ `ontology/snapshot.json` + 可选 `am/` `db/` `declarations/` |
| 状态机 | `created → uploaded → (db_pending → db_ok)? → imported → active`，失败不阻塞旧版本 |
| API | 9 个端点，全部 admin token；query key 一律 403 |
| 安全 | key 只显示一次（存 sha256）；DSN 加密存、永不回显 |

## 待拍板（6 个，见 demand.md §七）

1. 包内本体用**快照 JSON** 还是 TTL？→ 建议快照 JSON（TTL 有损）
2. 包要不要**签名**？→ 建议 v1 只 sha256
3. key 一项目一把还是多租户多把？→ 建议 v1 一项目一把
4. key 何时发？→ 建议创建时
5. DSN 存哪？→ 建议元数据库加密列
6. "包内含 db" 怎么判？→ 建议看 `requires_dsn`

## 关联

- 复用 **REQ-124** 控制面（admin/query token、`/api/import|apply|freeze|export|config|audit`）
- 若采纳"服务端解析写库" ⇒ **服务面首次获得写元数据库能力**，要补 ADR

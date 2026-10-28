# 状态：REQ-176 · 发布闸的业务词白名单

**已就绪**（2026-10-28 · 客户本体 `display_name="evidence"` 被误杀触发）

## 一句话
`--term-allowlist <file>`：部署级声明"这些词在这个客户域里是业务词"，
闸门层过滤后仍为空才放行；**默认不放松**，豁免必须留痕，文件坏了必须启动失败。

## 影响面
`src/cli.rs`（新 flag，serve + ontology export）· 闸门层（`domain/pipeline.rs` 的 `gate_*`）
· 启动告警 · 测试

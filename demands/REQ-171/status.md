# 状态：REQ-171 · 配方自足（identity + iri_template）

**已就绪**（2026-10-28 · 平台真实对接触发）

## 一句话

服务端手里有 `identity` / `iri_template`（AM 里），但**没放进 `@pending.get`**
⇒ 配方不自足 ⇒ agent 只能猜 `--id-property`；rows 模式漏传还静默 0 行。

## 改动面

`src/query/source_gate.rs`：`Recipe` 加两字段 + `build_recipe` 原样填；v1 行为不变。

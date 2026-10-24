# 状态：REQ-140 · JSON-LD 上下文子图

## 当前状态

**进行中**（2026-10-24 开单 · **demo 唯一空白**）

## 为什么必须做

Axon 架构 v0.3 的接口方向②写的是「组装为**上下文子图（JSON-LD/RDF）**返回，供 agent 推理」。

我们返回的是：

```json
{ "columns": ["m","name"], "rows": [[iri, "Claude Haiku 4.5"]], … }
```

**表格，不是图。** Agent 拿到还得自己拼实体关系。

## 一半已经做完了

`sql_echo` 实测：

```sql
('https://ontok.local/anspire_r2rml_excl#llm_model/' || t0."id")::text AS "m"
```

**类变量的绑定已是完整实例 IRI**，不用另拼。

## 核心设计

| 决策 | 选择 | 理由 |
| --- | --- | --- |
| 嵌套 vs 扁平 | **扁平 `@graph` + `@id` 引用** | 深嵌套 3 跳爆炸；扁平天然合并 |
| 同实体 | **同 `@id` 合并属性** | 这就是"子图"与"表"的分水岭 |
| 聚合 | `summary` 对象 | 没有实体，造假节点会骗人 |
| `meta` | **必须保留** warnings/coverage/truncated/join_states | 丢了就是"图好看但把限定吞了" |

## 五条裁定

R1 默认 rows 零变化 · R2 确定性逐字节 · R3 meta 保留限定 · R4 聚合进 summary · R5 空结果 `@graph: []`

## 待办

- [ ] Dev（翻译器保留 var→谓词/类 映射；组装器；三个入口；测试）
- [ ] QA（重点：合并、确定性、meta 不丢）
- [ ] demo 前：与 Axon 侧对一次真实契约

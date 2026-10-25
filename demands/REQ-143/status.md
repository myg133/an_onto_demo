# 状态：REQ-143 · 纯文档通路（产品命令）

## 当前状态

**进行中**（2026-10-25 开单 · P0）

## 为什么

`doc merge` 现在是「**用文档补充已有库本体**」：概念必须命中已有实体，关系必须有 `maps_to`。

⇒ **没有库的项目（Ops360）走不通**（实测：空提案 + 3 断言 ⇒ 并入 0 / 未映射 3 / doc-only 0）。

当时我用一个 python 脚本把抽取结果转成 proposal 才跑通。**那是权宜之计**：
skill 里写脚本 ⇒ 路径一变就废、版本会漂、FDE 维护不了。

## 要做

```bash
ontok doc merge --assertions a.jsonl --proposal p.json --out merged.json --allow-doc-only
```

| 情形 | 行为 |
| --- | --- |
| 概念命中 | 照旧（both + 合并证据） |
| 概念没命中 | **新建实体**（key 确定性、band ≤ medium、未签、`origin: doc`） |
| 关系两端都解析得到 | 新建边（无 join 列） |
| 关系有端点缺失 | 仍 `doc_unmapped`（⛔ 不造端点） |

不开开关 ⇒ **与现在逐字节相同**。

## 五条裁定

R1 显式开关 · R2 key 确定性 · R3 band ≤ medium + 未签 · R4 不造端点 · R5 零变化回归

## 配套（BA 自己做）

FDE 侧 skill `ontok-build`：
- 分 A（有库）/ B（纯文档）两条路
- 每步都有「怎么验」
- **「失败长相」一节**（断言 0 条 / 合并后并入 0 / 关系为 0 / catalog 少一半 / 审计里假租户）

## 待办

- [ ] Dev（开关 + 测试 + 端到端）
- [ ] BA（写 skill）
- [ ] QA
- [ ] 用户自己拿 Ops360 文档跑一遍（这才是验收）

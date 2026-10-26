# 状态：REQ-155 · UI 重组

## 当前状态

**进行中**（P1 完成）

## 分期

| 期 | 内容 | 状态 |
| --- | --- | --- |
| P1 | 删 Portal（代码 + Deploy） | ✅ **完成** |
| P2 | Studio 挂 `query serve` | ⬜ 待派（ADR-010-A 已修订） |
| P3 | Console 换皮 | 🟡 视觉稿已确认（含下拉深色修复） |

## P1 收口

| 项 | 结果 |
| --- | --- |
| 代码 | `6695668`（已推 develop）· 21 文件 / **+77 / −5,888** |
| 测试 | passed **1796 → 1718**（−78 = 49 单测 + 18 + 11 e2e）· failed **183 → 183 逐名相同** |
| 守卫测试 | `test_req107_red_lines.rs` 加了 **2 个字符串字面量白名单**（无通配）；其余删除仍红 —— **窄化不是放宽**（BA 已亲自复核） |
| Deploy | `95f31fe`（已推）· 删 5 文件 + 15 文件改引用 · 五份 overlay 渲染 exit 0 且 portal=0 |
| 反向保护 | uat/prod 仍含 console（10 / 7）· uat-ops360 仍不含 console（0） |
| 集群 | ✅ 无孤儿 portal 资源（UAT 用 uat-ops360，本来没部署过）；apply 后 `unchanged`、**未重启** |

## 依据

- `demand.md` · `ADR-010` + **`ADR-010-A`（修订：挂 query serve，不动 ontok serve）**
- 删前恢复点：tag `before-req155-portal-removal`

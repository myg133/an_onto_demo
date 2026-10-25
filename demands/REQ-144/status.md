# 状态：REQ-144 · access mapping 阶段 0

## 当前状态

**进行中**（2026-10-25 开单 · P1 · 等 REQ-143 合入后开工）

## 这是什么

「现场」路径的地基：让数字员工**拿着契约自己去调云 API**，我们只提供"怎么取"。

```
本体（TTL）        ← 有什么、什么关系
access mapping     ← 每个类：哪个 API、哪些参数、响应哪一段、能不能过滤
参考执行器          ← ontok access fetch（客户侧运行）
```

**凭据不出客户边界**——这是我们换取的最大好处。

## 阶段 0 只做三件（⛔ 不碰真 API）

| 产物 | 说明 |
| --- | --- |
| 契约 v1 形状 | sources / entities / relations + capability / freshness / identity |
| `ontok access validate` | 离线自洽检查（4 条有牙的：iri 一致、能力不重叠、端点存在、**字段路径在 fixture 里存在**） |
| `fetch --dry-run-fixture` | 用 fixture 跑完整取数：分页 + 本地过滤 + JSON-LD + meta 三项 |

## 六条裁定里最要紧的两条

| # | 裁定 | 为什么 |
| --- | --- | --- |
| **R5** | 超 `max_scan_rows` ⇒ **拒绝** | 不许"偷偷全量拉"——那是"能跑但慢到爆" |
| **R6** | **未签 entity 拒绝取数** | AI 生成的映射也是提案（ADR-005）；没签就取数 = 用未经人确认的路径读生产数据 |

## 待办

- [ ] Dev（契约 + validate + dry-run）
- [ ] QA
- [ ] 阶段 1（真 API + TC3 签名）——需要 AK/SK 与 fixture

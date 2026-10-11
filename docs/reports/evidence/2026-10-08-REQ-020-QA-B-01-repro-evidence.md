# QA-B · 实测复现证据（`propose import` 可造假溯源）

日期：2026-10-08 · 只读评审，未修改任何项目源码 / worktree

## 0. Dev 报的 PoC 原样跑不通（重要修正）

Dev 给的命令用的是 `"items":[]`：

```
$ ontok propose import --file x.json
✗ 版本不存在：project=qa-req020b-fake-eb seq=1     # 第一次：项目/版本都还没有
$ ontok propose import --file x.json
✗ 提案没有可应用的条目                                # 建完项目+基线版本后
```

`src/db/proposal.rs:114-116` 对空 items 直接返回 `StoreError::NothingApplicable`。
⇒ **Dev 的 PoC 是错的**，但**结论方向没错** —— 只要 items 非空就能造出假溯源（见下）。

## 1. 可跑通的 PoC

`.review-workspace/qa-req020b/fake_real_items.json`：

```json
{
  "project": "qa-req020b-fake-eb",
  "base_version": 1,
  "round": 7,
  "extracted_by": {
    "agent": "totally-not-a-real-agent",
    "version": "9.9.9-fake",
    "round": 7,
    "failed_groups": ["payments", "risk"]
  },
  "items": [
    { "kind": "add_entity_type", "target": "手工塞进来的实体",
      "payload": { "key": "hand_made", "display_name": "手工塞进来的实体",
        "properties": [],
        "evidence": [{"table":"hand_written_by_human","column":"x","claim":"我编的"}] } }
  ]
}
```

## 2. CLI 输出

```
$ ontok propose import --file fake_real_items.json
提案 p1 已导入，共 1 个条目：
  [1] add_entity_type 手工塞进来的实体
  （抽取来源：totally-not-a-real-agent 9.9.9-fake）      ← CLI 自己都打出来了
exit=0

$ ontok review decide --proposal p1 --project qa-req020b-fake-eb --accept-all
提案 p1 的 1 个条目 × 2 轨 → accepted

$ ontok proposal apply --proposal p1 --project qa-req020b-fake-eb
提案已应用 → v2（id=42c3a9b8-57b0-48ab-b3c4-2cfaf7a0165d）
```

## 3. 库里实际内容（直接 SELECT，非 CLI 自述）

```
=== proposals 表（原始列）===
project=qa-req020b-fake-eb  round=7  status=applied
  proposals.extracted_by = {"agent":"totally-not-a-real-agent","round":7,
                            "version":"9.9.9-fake","failed_groups":["payments","risk"]}
  items                  = 手工塞进来的实体

=== ontology_versions 表（原始列）===
  v1  extracted_by = null
  v2  extracted_by = {"agent":"totally-not-a-real-agent","round":7,
                      "version":"9.9.9-fake","failed_groups":["payments","risk"]}
```

另经 `ontok ontology show --version 2 --model-view`（走 `get_snapshot` 读库）复核一致。

## 4. 结论

- **假溯源成立**，且**一路传染到最终版本的 `ontology_versions.extracted_by`**（`src/db/apply.rs:260-263` 直写）。
- 全程**无任何警告、无校验、无留痕**：`src/cli.rs:436-439` 只做 `serde_json::from_value::<ExtractedBy>`
  的**结构**反序列化，语义（这个 agent 真的抽过吗）一概不查。
- `failed_groups: ["payments","risk"]` 也是编的 —— 按 `snapshot.rs:37` 的注释，
  漏记等于假装全成功；反过来伪造失败组同样无阻。

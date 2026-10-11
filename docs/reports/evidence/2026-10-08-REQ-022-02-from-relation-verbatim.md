# `from_relation` 搬移的逐字对照结论

方法：不看 diff 的呈现（git 会重新对齐），直接把
BASE `90d9046:src/extract/propose.rs:391-476` 与 HEAD `504ae9f:...:636-721`
各切成文件后 `diff -u` 逐行比。

## 结论：**是逐字搬移，只差 1 行，且那一行是必要的**

86 行的 match 块（`let (name, note, from_relation) = match (&comment, quality, &fk_name)` 起）
逐字节相同，**唯一**差异在 LowQuality 那一支：

```diff
 (Some(_), Some(CommentQuality::LowQuality), _) => {
-    stats.props_comment_rejected += 1;
+    comment_rejected = true;
     (String::new(), None, false)
 }
```

**必要性论证**：函数搬进 `build_properties` 后签名里**没有** `stats`（返回 `Vec<PropertyDraft>`），
所以那条自增**必须**换成置位，由调用方 `propose.rs:416-418` 的
`if d.comment_rejected { stats.props_comment_rejected += 1 }` 施加。
该 match 臂每个被访问的列只命中一次，置位与自增次数恒等 ⇒ **等价**。

## 尾部计数逻辑：不是逐字搬移，是**忠实改写**，已逐条验证等价

旧（BASE:466-476）：
```
if from_relation { relation+=1; wordlist+=1 } else if name.is_empty() { wordlist+=1 } else { comment+=1 }
```
新拆成两半：
- 函数内（HEAD:714-720）算 `origin`：`from_relation→Relation` / `name.is_empty()→WordList` / 其余→`Comment`
  —— 判据与旧三分支**一一对应**；
- 调用方（HEAD:404-412）按 `origin` 还原同样三个计数，且
  `d.item.name` 就是那个 `name`（`PropertyDraft` 构造时 `name` 原样搬入）⇒ 判据恒等。

## links_for_table 同法核对
BASE:483-509 与 HEAD:510-540 逐行比，差异只有 4 处，**全部是签名变化所必需**：

| 差异 | 为什么必要 |
|---|---|
| `infer_link(c, &known_tables)` → `infer_link(c, known_tables)` | `known_tables` 入参已是 `&HashSet`，解引用成 `&&` 会类型不符 |
| `link_types.push(...)` → `out.push(...)` | 改为返回 `Vec<LinkItem>`，不能借用调用方的累加器 |
| 删掉 `stats.links_inferred += 1` | 同上，函数不再有 `stats`；调用方 `propose.rs:429` 改为 `+= links.len()` |
| `from_cn` 由 `String` 变入参 `&str` | 签名所需 |

被 `continue` 掉的自引用两侧都不计数 ⇒ `links_inferred` **等价**。

## 行为等价的经验证据
`.review-workspace/req022/baseline/` 与 `after/` 的产出**字节相同**：
```
stats.json     6b62a513765636c9440db4a916805bd725e1a51384b0cacbd4257e114ca029d6  (两侧一致)
proposal.json  b269f3436effda7c449afabecb6c48adc95257921c82b4ff9c19e5274b993e95  (两侧一致)
```
真实规模：91 表 / 964 属性 / 579 注释 / 42 关系 / 6 拒用 / 51 关系条目 / 142 items。
（`579 + 385 = 964` 自洽；关系 42 同时计入 wordlist 是既有口径。）

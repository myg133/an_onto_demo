# REQ-017 交付报告：`links_dangling` 地雷 —— 前提不成立（未改代码）

> 分支 `feature/REQ-017` · 基线 `9d6c529`（develop HEAD）
> **本 REQ 未改动任何代码。** 下面是判定依据与实测证据。

---

## 结论（先行）

**REQ-017 所描述的「客户拿不到提案」缺陷在本基线上不存在。**
按 briefing 给定的**逐字命令**跑基线二进制：`exit 0`，`悬空 0`，80 条关系全部产出，产物 sha **与期望值逐字节相同**。
三处代码（`rule_based.rs` 注释 / `ontok-propose.rs` 的 bail / `assemble.rs` 注释）都真实存在，
但它们**不构成一条因果链** —— 前提把「阶段 1 的逐组 stats」与「阶段 3/4 组装 stats」当成了同一个对象。

---

## 一、实测证据（基线 `9d6c529`，零改动）

### 1.1 跨表关系 ⇒ 不被判 dangling、CLI 不 bail（AC-001 前半条）

命令：

```
./target/debug/ontok-propose \
  --schema /home/myg133/Projects/Anchnet/Ontology/.review-workspace/fix-props/schema.json \
  --out   .review-workspace/req017/baseline/proposal.json \
  --stats .review-workspace/req017/baseline/stats.json \
  --project extracted
```

结果（**口径：单次 CLI 运行，退出码与 stdout**）：

| 项 | 值 |
| --- | --- |
| 退出码 | **0** |
| `关系 : 80 条（悬空 0）` | **悬空 = 0** |
| `stats.json` 的 `links_dangling` | **0** |
| `links_inferred` | 80 |
| `items` | 171（实体 91 · 关系 80） |
| 阶段1/2 | 91 组 · 失败 0 |

产物 sha256（**口径：sha256sum，对上述两个文件**）：

| 文件 | 实测 | 期望 | 结论 |
| --- | --- | --- | --- |
| `proposal.json` | `1eef95414b8eec084b12b09b8eb89d6347c64f45476447ec243e5dd66c5c5635` | 同左 | ✅ 一致 |
| `stats.json` | `cda6a9a2a007fcabe2be602a9962b66deda84498e0fb1fb8232d0627632cd7ba` | 同左 | ✅ 一致 |

⇒ 我复现的就是 briefing 指定的场景，而该场景**通过**。91 张表里 80 条跨表关系，一条都没被误判成悬空。

### 1.2 真悬空 ⇒ 仍被计入（AC-001 后半条）

真悬空**只能**由「agent 编造出指向不存在实体的关系」或「某组抽取失败」产生。
仓库里唯一钉这条判据的测试：

```
cargo test --test test_agent_rebuild test_rebuild_path_still_verifies_declarations_in_code
→ 1 passed; 0 failed; 0 ignored; 25 filtered out; 0.02s
```

该测试断言 `run2.outcome.stats.links_dangling == 3`（`tests/test_agent_rebuild.rs:1278`），
用的是把关系指向不存在实体的 stub agent ⇒ **真悬空仍被计入，判据未被削弱**。

---

## 二、因果链断在哪（按内容定位，不信行号）

`links_dangling` 全库只有**两处**被计算：

| 位置（内容定位） | 属于哪一步 | 它的 stats 去哪了 |
| --- | --- | --- |
| `src/extract/propose.rs` 末尾 `stats.links_dangling = before - link_types.len();` | 阶段 1（`extract_group` 调，**受限作用域**）与阶段 3/4 规则路径（**全量作用域**）**共用同一函数** | 见下 |
| `src/agent/assemble.rs` 末尾 `stats.links_dangling = before - links.len();` | 阶段 3/4 重建路径 | 进 `outcome.stats` ⇒ 到 CLI 的 bail |

CLI 读的是 `ontok-propose.rs:243-244` 的 `orchestrate(...)` 返回的 **`outcome.stats`**，也就是**阶段 3/4 组装**那一份。
两条组装路径：

1. **`Assembly::RulePipeline`**（当前默认，stdout 已自证：`阶段3/4 : 规则路径`）
   → `orchestrate.rs:987` 调 `extract_with_context(report, &[], …)`
   → **上下文传空 + report 是全量勘查结果**
   ⇒ `known_tables` = 全部 91 张表，`keys` = 全部 91 个实体 key
   ⇒ `retain()` **一条都删不掉** ⇒ `links_dangling = 0` ⇒ 不 bail。
2. **`Assembly::RebuiltFromCandidates`**
   → `assemble.rs:426`，对**成功组**合并后的结果施加自检
   ⇒ 只有「某组真的失败」或「agent 编造关系」时 `links_dangling > 0`，此时 bail **正确**。

### 再加一道：走重建路径的 agent 今天根本不存在

`Assembly` 由 agent 的**能力声明**决定（`src/agent/mod.rs:448`：声明了
`CAPABILITY_REBUILDS_PROPOSAL` 才走重建路径）。而随二进制发布的 `RuleBasedAgent`
只声明了一个能力（`src/agent/rule_based.rs:158-160`）：

```rust
fn capabilities(&self) -> Vec<String> {
    vec![CAPABILITY_PRODUCES_PROPOSAL.to_string()]
}
```

⇒ 用**出厂二进制 + 规则 agent** 跑，恒为 `RulePipeline`，
⇒ `links_dangling > 0` 在 CLI 的 bail 上是**结构上不可达**的（它是为将来的外部/协议 agent 预留的闸）。

而**受限作用域**下的那份 stats（`rule_based.rs:181` 调 `extract_with_context`）：

- `extract_group` 返回的是 `Ok(GroupExtraction { types, links })`（`rule_based.rs:290`）
- `GroupExtraction` **只有 `types` 与 `links` 两个字段，没有 stats**（`src/agent/group.rs:461-467`）
⇒ **逐组的 `links_dangling` 在类型层面就被丢弃了，物理上到不了 CLI 的 bail。**

这也正是仓库里**已有**的注释所写的（`src/extract/propose.rs:300-306`）：
「作用域只有一组时，指向组外表的关系会被这道自检记成 `links_dangling`。**这不是缺陷**：……`extract_group` 也压根不读 `stats`。」

### 补充：`infer_link` 使「真悬空」在规则管线里不可达

`links_for_table`（`propose.rs:546`）只接收 `infer_link` 返回 `Some` 的候选，而 `infer_link` 只在
目标表 ∈ `known_tables` 时才产出（精确命中 `propose.rs:1219`；词尾命中 `tail_match_target` 在 `propose.rs:1269`
对 `known` 过滤后 `hits.len() == 1`）。函数自己的注释写着「`infer_link` 已在 `known_tables` 内确认过目标存在」。

⇒ 走到自检的关系，**终点一定是一张真实存在的表**。
⇒ 规则管线里被 `retain` 删掉的，按构造只可能是**跨组**，不可能是「真悬空」。
⇒ 真正的「真悬空」只能来自外部/协议 agent 或组失败 —— 正是 1.2 那条测试覆盖的情形。

---

## 三、AC-002：三个方向我都不选

**方向①（语义拆分）不选**，理由是它**会直接违反 AC-004**：
新增一个 `links_cross_group` 计数字段 ⇒ `ExtractionStats` 的序列化形状变了 ⇒ `stats.json` 的 sha 必变，
而 AC-004 明确要求「产物 sha **不变**（本需求不碰抽取逻辑，只碰 dangling 的计数口径）」。
换句话说：**① 在不改任何行为的前提下，一定会打破本 REQ 自己的验收标准。**

**方向②（延后解析）不选**：要动的是阶段 3 的跨组合成 —— 那是 `assemble.rs` 里唯一实现「同 key 不同内容就报错」的
防静默丢数据机制（`assemble.rs:345-379`）。为修一个不存在的缺陷去动它，风险与收益完全不成比例。

**方向③（CLI 侧放宽）不选**：这就是 AC-003 明令禁止的「放宽判据」。而且现状根本不需要放宽。

**代价：零。** 没改代码 ⇒ 没有回归面、产物 sha 不变、`assemble.rs` 与 `propose.rs` 两处自检一字未动。

### AC-002 特别问：同一组被抽两次会不会重复计 / 漏计？

即便将来真要做①，现状答案也是「都不会」，且有两道保险：

- **不重复计**：`ExtractionStats` 每次调用都由 `..Default::default()` **新建**（`propose.rs:335`、`assemble.rs:384`），
  `drive_group` 重试不会把上一轮的计数累加进来。
- **合并层还有一道按 key 去重**：同一条关系在两组里都出现时，`assemble.rs:367-378` 先按 `key` 去重、
  内容不一致直接 `bail!`，**只有去重之后**才对剩下的施加自检 ⇒ 重试产出的同 key 关系不可能被计两次。
- **不漏计**：某组失败时它的表 key 不在 `keys` 里 ⇒ 指向它的关系**必然**被计入 `links_dangling` ⇒ 仍然 bail。
  这正是「部分失败要显式暴露」的设计意图。

⚠️ 反过来说：如果按①实现成「跨组计数由各组各自累加到同一个 stats」，**重复计的风险是真实存在的** ——
现在不做，恰好避开了这个坑。

---

## 四、AC-003：确认未绕过

`src/bin/ontok-propose.rs` 的 bail 一字未动，阈值仍是 `> 0`；
`assemble.rs:425` 与 `propose.rs:439` 两处自检的**判定逻辑一字未动**。
本次没有删除、没有 `#[ignore]`、没有放宽任何断言。

---

## 五、发现但未修（如实登记）

| # | 问题 | 严重度 | 为什么没修 |
| --- | --- | --- | --- |
| 1 | `src/agent/rule_based.rs:263-266` 注释说「**全量路径**末尾的 AC-007 自检会把……删掉」，但它描述的其实是 `extract_group` 内部那次**受限作用域**调用。措辞把两次调用混为一谈，实质结论（所以 links 必须取自 `links_for_table`）是对的。 | 低（仅措辞） | 纯注释，与本 REQ 的缺陷判定无关；改动会让基线产物无关地漂移，且本 REQ 结论是「不改代码」 |
| 2 | 真悬空时 CLI 的报错文案是「提案含 N 条悬空关系，AC-007 会在审核期拦截」，但重建路径下最常见的成因其实是**上游某组抽取失败**（`failed_groups` 非空），文案没指向真因。 | 低（文案） | AC-003 划定了 bail 周边不得改动；且这是既有的文案质量问题，不是本 REQ 的范围 |

---

## 六、全量测试（AC-004）

**口径**：`cargo test --no-fail-fast`，本 worktree 从零编译，无并发；
DB 环境变量按 `.env` 在仓库根 + 库名 `notol`→`ontol` 替换取得（未回显连接串）。
聚合口径：对完整输出里 **41 行 `test result:` 逐行求和**（含 Doc-tests），
用 `awk` 累加，**未用 `paste -sd+`**。

| 指标 | 实测 | AC-004 期望 |
| --- | --- | --- |
| passed | **574** | 574 |
| failed | **0** | 0 |
| ignored | **1** | 1 |
| 结果组数 | 41 | — |
| 退出码 | 0 | — |
| 耗时 | **426s（7m06s）** | 5m~9m 区间内 |
| 编译（首次，从零） | 1m09s（`cargo build --tests`） | — |

⚠️ 这一轮是**在零改动的树上**跑的 —— 它证明基线本身健康、与 AC-004 的期望值逐项吻合，
但它**不构成「本需求的回归验证」**，因为本需求没有产生任何代码改动，
自然也没有 AC-004 里提到的「本需求会新增用例」。

> **我自己引入又清掉的问题（如实登记）**：
> 第一次全量我把输出管给了 `tail -60`，只留下末尾几十行，**聚合数字丢了**，
> 无法满足 AC-004。已重跑一次并把完整输出落盘到
> `.review-workspace/req017/full_run.txt`（本次 426s 的数据来自重跑）。
> 代价是多用了一轮 7 分钟；未改动任何代码或测试。

---

## 七、给 BA 的建议

REQ-017 建议按 **「前提不成立，作废」** 关闭，而不是按「已修」关闭 ——
因为没有任何代码可修。若 BA 认为 REQ-014 引入的「逐组 stats 被丢弃」本身值得留痕，
建议改开一个**文档型**小 REQ：把 `propose.rs:300-306` 已有的正确说明，
补一条指向「`GroupExtraction` 无 stats 字段 ⇒ 逐组 dangling 到不了 CLI」的类型级证据，
避免下一位读到 `rule_based.rs:264` 那条注释时再次误判成地雷。

---

## 八、纪律登记

- 全程未用 `CARGO_TARGET_DIR`、未用 `--release`、未建 probe crate、**未建任何符号链接**；
  target 在本 worktree 内由 `cargo build --tests` 从零编译，耗时 **1m09s**。
- 未与任何其他测试并发；全量单独跑。
- 临时文件只落在 `/home/myg133/Projects/Anchnet/Ontology/.review-workspace/req017/`。
- 口令只走环境变量，**未 echo 连接串**（只校验了 URL 形状与库名是否为 `ontol`）。
- `fix-props/schema.json` 只读，未改动。
- 未 push、未改历史、未动 `BA/`。
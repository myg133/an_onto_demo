# REQ-093 · P1 修复独立复验（QA2）

- 复验对象：worktree `feature-REQ-093`，分支 `feature/REQ-093`
- 提交：`0863fdd`（P1 修复），父 `d216b7a`（初版），基线 `0455523`
- 复验范围：**仅本轮 P1 修复**，不是整个交付
- 复验方式：独立读码 + 真实数据端到端 + 变异验证 + 全量测试；**未改任何代码**（变异全部事后还原，见 §6 末尾校验）

---

## 0. 总判定

**PASS 带非阻断。**

修复本身经独立验证**真的成立**：标识分配在全仓库只有一处，真实数据上 91/91 个节点逐个点开、0 个 404、0 个属性条数不符。
唯一实质问题在**守卫强度**：被称作「P1 的防回归」的那条新测试**不守护生产接线**（把原 bug 原样放回 `serve()`，两条测试照样全绿）。产品今天是对的，缺的是防它退回去的那道网。

---

## 1. 第一节 · 「按构造一致」是否成立

**结论：成立。** 图上的 `node.id` 与 `/api/node` 的寻址键确实由同一份分配产生，且 HTTP 侧不再自行分配。

### 1.1 分配逻辑只有一处

| 位置 | 内容 |
| --- | --- |
| `src/domain/graph.rs:163-185` | `assign_keys()` —— **唯一**的标识分配函数 |
| `src/domain/graph.rs:174` | `format!("{}#{n}", o.name)` —— **全仓库唯一**一处重名消歧的格式化 |
| `src/domain/graph.rs:198` | `node_keys()` → `assign_keys(&v.objects).keys` |
| `src/domain/graph.rs:209` | `project()` → `assign_keys(&v.objects)` |
| `src/server/http.rs:411-414` | HTTP 侧**只消费** `graph::node_keys(&view)`，`.zip(view.objects.iter().cloned()).collect()`，自己不含任何分配判断 |

### 1.2 三条独立佐证

1. **全仓扫描**：`grep -rn '#{n}' --include=*.rs src/ tests/` → **仅 1 处命中**：`src/domain/graph.rs:174`。
2. **HTTP 侧无第二份逻辑**：`grep -n "\.collect()|BTreeMap::|insert(" src/server/http.rs` 的生产代码命中只有 411-414 那条 `collect()`；`grep "o.name|obj.name|.name.clone()"` 在 `http.rs` 里唯一的命中是 `http.rs:468` 的**测试字面量**（`label: "外键约束表".into()`）。
3. **同一份输入**：`http.rs:395` 只 `load_frozen_view` 一次，`http.rs:399` 拿它算图，`http.rs:411` 拿**同一个** `view` 建索引 ⇒ 两端不可能来自不同快照。

### 1.3 但守卫有缺口（见 §6 的 M1）

`assign_keys` 用 `BTreeSet used` 保证键唯一（`graph.rs:164,169,175`），HTTP 侧的 `.collect()` 若遇重复键会静默丢弃 —— 该不变量另有 `http.rs:415-419` 的 `debug_assert_eq!` 兜底。
⚠️ `Cargo.toml` **无任何 `[profile.*]`**（已查）⇒ cargo 默认 release 的 `debug-assertions=false` ⇒ **release 构建下该断言不存在**，重复键会静默丢节点。此项属推断（依据为 cargo 默认值 + Cargo.toml 内容），非实测 —— 实测受「禁 `--release`」约束。

**判定：设计层面「按构造一致」成立，不属于纪律 H 说的「忠实的重复实现」。**

---

## 2. 第二节 · 端到端（真实数据，**逐个，91/91**）

服务：`ontok serve --project demo3 --version 2 --bind 127.0.0.1:18931`（**我自己的端口**；`18812` 上别人的进程全程未碰，PID 856174 仍在）
就绪判定：轮询 `/dev/tcp`，3 秒后 READY（未死等固定秒数）
令牌：自建随机值，仅走 `ONTOK_VIEW_TOKEN`，未出现在任何输出或产物中

**确认跑了全部 91 个，不是抽样。** 遍历脚本对 `graph.json` 的 `nodes` 数组逐个发一次独立 HTTP 请求（`sweep.py` → `sweep.json`，91 条记录齐全）。

| 指标 | 数值 |
| --- | --- |
| 图上节点总数 | **91**（`graph` 节点 id 唯一 91，展示名去重后 90） |
| 逐个发起请求数 | **91** |
| **404 寻址失败** | **0** |
| **`attribute_count ≠ len(properties)`** | **0** |
| **可打开（HTTP 200）** | **88 / 91** |
| **非 404 错误** | **3 × HTTP 500**，明细见下 |
| 状态直方图 | `{200: 88, 500: 3}` |

### 2.1 三条 500 明细

| 节点 id | 图上 `attribute_count` | HTTP | 响应字节 | 守门词（取自我方 serve 日志） |
| --- | --- | --- | --- | --- |
| `鉴权缓存变更事件` | 18 | 500 | 70 | `pk` |
| `鉴权缓存消息事件` | 20 | 500 | 70 | `key` |
| `用量明细` | 15 | 500 | 70 | `key` |

500 正文统一为：`{"error":"这一问不给数据：内容里出现了 1 项只有内部使用的术语（为免泄漏，此处不列出具体词条）。这是发布闸门，请联系维护者。"}` —— **不泄露词条、不顺带发数据**，符合 AC-002。

### 2.2 重名对象（开发方声称 10/10 与 9/9 均 200 —— **实测成立**）

| `node.id` | 图上标 | 点开实测属性条数 | HTTP |
| --- | --- | --- | --- |
| `客户账号绑定` | 10 | **10** | 200 |
| `客户账号绑定#2` | 9 | **9** | 200 |

上一轮的 P1 症状（图上 10、点开 9、另一个 404）**已消失**。

---

## 3. 第三节 · 那 3 个 500 是不是「基线既有」

**结论：是基线既有，不是本轮引入。我独立验证的三条证据：**

### 3.1 `view.rs` 自基线起 0 个 commit 触及

```
git log --oneline 0455523..HEAD -- src/domain/view.rs  →  0 条
```
连带核对另外三条红线：`src/domain/review_view.rs` / `tests/drop_rate.rs` / `tests/test_fk_naming.rs` **均 0 条**。

### 3.2 `ontology show` 今天就 exit 2（我实测）

```
./target/debug/ontok ontology show --project demo3 --version 2   → exit=2
末行：⚠ 客户视图泄露了建模术语：["pk"]
```
即**整份客户视图**今天就被同一道闸门拒发 —— 与「节点打不开」同源，且早于本轮。

### 3.3 属性文案我自己去看过（不采信转述）

用 `ontology show --model-view`（该路径按 `cli.rs:1203-1205` 明确**不过**守门器，只读输出）取真实文案：

| 对象 | 实际属性文案（节选） | 命中的守门词 |
| --- | --- | --- |
| `鉴权缓存变更事件` | `稳定来源事件ID：CDC/table/op/pk/lsn、full_sync/run_id/type、package_lifecycle/...。` | `pk` |
| `鉴权缓存消息事件` | `Kafka key；单 key 事件使用 license_key，defaults 使用 global_rate_limit_defaults。` | `key` |
| `用量明细` | `关联的API Key ID` | `key` |

守门词表见 `src/domain/view.rs:897-901`，`pk` 与 `key` **都在表内**。图之所以还能出图，是因为图**不带属性文案**（只带 `attribute_count`）。

### 3.4 补充：闸门代码本轮未动

本轮 diff 只有 6 个文件（`cli.rs` / `domain/graph.rs` / `domain/mod.rs` / `server/http.rs` / `server/mod.rs` / `tests/test_graph_server.rs`）。
执行闸门的 `src/mcp/tools.rs::guard`（`tools.rs:693-710`，复用 `view::scan_modeling_terms`）**不在其中** ⇒ 拒发行为由未改动的代码 + 未改动的数据路径产生。

**判定：基线既有，非回归。** 且「没有为让节点打开而放松守门器」这一点我核实为真 —— `MODELING_TERMS` 词表与 `scan_modeling_terms` 均未被本轮触碰。

⚠️ 开发方的**实质判断正确，但转述有三处不准**（不影响结论）：
1. 实际命中的守门词是 `pk`（1 个）与 `key`（2 个），不是整串 `CDC/table/op/pk/lsn`；
2. `用量明细` 的文案是「API **Key ID**」，不是「API-Key」；
3. `Kafka key` 那句属于 `鉴权缓存消息事件`，不是变更事件。

---

## 4. 第四节 · 四条小的

### 4.1 `--help` 那句实测为假的话：**已删，新表述与实测一致** ✅

- 旧句确实存在：`git show d216b7a:src/cli.rs | grep 安静` → `81: /// ⚠️ **三个参数缺一不可，少了会安静地什么都拿不到**（不报错，只是空）。`
- 现文（`src/cli.rs:81-82`）：「少了任何一个，clap 会**直接报错退出（exit 2）**，并把缺哪个、怎么写打出来 —— 不是安静地什么都不返回。」
- **实测命令行缺参**：`ontok serve` → **exit 2**，正文 `error: the following required arguments were not provided: --project / --version`
- **实测查询缺参**：`/api/graph` → **400**「要说明看哪个项目：请带上 project 参数。」；`?project=demo3` → **400**「要说明看哪一版：请带上 version 参数。」；`/api/node?...version=2` → **400**「要看哪个对象？请带上 id 参数。」
- ⇒ 新表述与两个实测**都对得上**。

### 4.2 `domain/mod.rs:3` 与守卫**同一把尺** ✅

我独立复算（不跑测试，自写剥注释器 + 先自检扫描器本身，纪律 E/F）：

| 口径 | 我的复算 | mod.rs 的写法 | 一致 |
| --- | --- | --- | --- |
| 剥注释后命中 | **0** | 「剥掉注释后命中 = 0」 | ✅ |
| 连注释按行命中 | **9 行** | 「连注释一起算命中 = 9 行」 | ✅ |
| 基线逐文件 | apply.rs 1 / snapshot.rs 2 / mod.rs 1 / review_view.rs 5 | 同 | ✅ |

- 基线取数用 `git show 0455523:<path>`（纪律 K），逐文件 1/0/0/0/0/0/0/0/0/0/0/0/0/0/5/2/0，**合计 9**。
- 测试侧常量 `BASELINE_COMMENT_HITS = 9`（`tests/test_graph_server.rs:147`），断言用的是 `assert_eq!`（**不增不减**）—— 与 mod.rs 的措辞是同一把尺。
- 扫描器自检：先证我的剥注释器**能**找到代码里的 `PgStore`/`db::`、**不会**把注释里的当命中（第一次自检我自己的探针写错了词，已重做）。
- 附注：`graph.rs` 作为新增文件，命中 0，与它模块头 `graph.rs:19-23` 的自述一致。

### 4.3 405 正文：已注册与未注册路径 × POST/PUT/PATCH/DELETE **16/16 一致** ✅

4 条路径 × 4 动词 = 16 次实测，**全部 405、正文均 33 字节且逐字相同**：
`{"error":"这里只读：只能看，没有任何可以改动的地方。"}`

| 路径 | POST | PUT | PATCH | DELETE |
| --- | --- | --- | --- | --- |
| `/api/graph`（已注册） | 405/33B | 405/33B | 405/33B | 405/33B |
| `/api/node`（已注册） | 405/33B | 405/33B | 405/33B | 405/33B |
| `/`（已注册） | 405/33B | 405/33B | 405/33B | 405/33B |
| `/api/apply`（未注册） | 405/33B | 405/33B | 405/33B | 405/33B |

对照组（纪律 E：证明不是「所有请求都被拒」）：带口令 GET `/api/graph` → **200**；缺查询参数 → **400 人话**；**无口令时 405 变 401**（不泄露路径是否存在）。

### 4.4 两条纪律注释**确实写进了源码** ✅（不是只在交付说明里）

| 纪律 | 源码位置 | 原文要点 |
| --- | --- | --- |
| **Q**（扫描型守卫取工作区 sha256） | `src/server/mod.rs:61-68`；`tests/test_graph_server.rs:21-38` | 「`read_source` 是**运行时读工作区文件**」⇒「要取的是**被扫文件的工作区 sha256**，不是二进制 sha256」 |
| **R**（「只读」是哪个面） | `src/server/mod.rs:16-27` | 「「只读」的**准确**表述：「HTTP 暴露面只读」，**不是**「这个进程只读」」 |

R 的事实依据我也核了：`src/cli.rs:704` 在 `match cli.cmd` **之前**无条件 `crate::db::pg::migrate(&pool).await?` ⇒ serve 进程启动时仍需库写权限，注释所述为真。

---

## 5. 第五节 · 全量测试

### 5.1 绝对数字（口径：逐个 `^test result` 行求和）

```
命令：cargo test --no-fail-fast          （未用 -q）
commit：0863fdd                          （纪律 O：与 commit 绑定）
日志：.review-workspace/req093qa2/full-test.log
```

| 项 | 数值 |
| --- | --- |
| **suite 数** | **61** |
| **passed** | **836** |
| **failed** | **0** |
| **ignored** | **1** |
| 墙钟 | **12 分 07 秒**（21:27:09 → 21:39:16） |

- 无 `FAILED`、无 `failures:` 段、无编译错误。
- 那 1 个 ignored 在 `tests/test_mcp_server.rs:553`，`#[ignore = "缺口清单，不是可运行断言"]`，**先于本轮存在**（该文件在本轮 diff 里 0 行改动）—— 不是本轮新增的 ignore。

### 5.2 与开发方声称一致

`61 / 836 / 0 / 1 @ 0863fdd` —— **与声称逐项吻合**。

基线 `833 @ d216b7a` 我**没有**独立重跑：重跑需要在这个交付 worktree 上移动 HEAD，超出「不改交付」的边界。可佐证的旁证有两条：
1. 算术：836 − 3 = 833；
2. `git diff` 的测试函数名差集恰好 **3 条**（`every_node_including_duplicates_opens_with_a_matching_attribute_count` / `duplicate_names_get_distinct_node_ids_ending_in_a_hash_two` / `every_405_carries_a_human_message`），另 4 条新增是辅助函数非测试 ⇒ +3 成立。

### 5.3 那 11 分钟：**我没遇到；判定为「正常慢」，不是真卡死**

- **我的全量跑没有出现 11 分钟停顿**：39/61 处 30 秒内就推进到 40（21:35:35→21:36:05）；全程最大单 suite 停顿是 `test_mcp_server` 112.27s。
- **按 briefing 指定的判据取证**（「单跑能过 + 无残留进程」），我独立单跑：
  ```
  cargo test --test test_proposal -- --exact test_proposal_item_by_seq
  → exit=0，test result: ok. 1 passed; 0 failed; ... finished in 6.33s（墙钟 8s）
  运行后残留进程：NONE
  ```
  全量跑完后残留测试进程同样为 **NONE**。

⚠️ **我对开发方这一条的数字提出异议**：他说 `test_proposal_item_by_seq` 单跑 **60.61s**，我这里单跑是 **6.33s**，相差近 10 倍；我全量跑里整个 `test_proposal` suite 也只用了 51.17s。
⇒ 「本来就慢 60.61s」这个**具体数字不可复现**；合理的解释是全量并行跑时数据库争用把单测拖慢（我这次最慢的四个 suite 全是连库的：`test_mcp_server` 112s、`test_apply` 88.7s、`test_snapshot` 67.4s、`test_proposal` 51.2s）。
**结论：正常的慢 + 并行争用，不是死锁。** 但那 11 分钟的现场我没看见，无法排除偶发；若再出现，可复现的判别法是「看那个 suite 的测试二进制是否还在推进 DB 查询」。

---

## 6. 第六节 · 变异证据（三元组）

### 6.0 未变异基线

| 项 | 值 |
| --- | --- |
| 工作区 `src/server/http.rs` sha256 | `a418d399677fb079d45b2efec132601bed8a09185f544a6473da2454f4d6542d` |
| 工作区 `src/domain/graph.rs` sha256 | `0e78a2715c55927a8bdb3a2145097f7afdcf4006a4e5eda35a9dca7a03b1042f` |
| lib 测试二进制 sha256 | `92f27ad99e33c169bc1b6aad23c06884aac4f197de0315c0030ed4bd2eb148fc` |
| 运行条数 | 3 条新测试各 1 次 ⇒ **3/3 绿** |

### 6.1 M1 · 把 P1 原 bug 放回**生产 `serve()`**（最关键的一条）

| 项 | 值 |
| --- | --- |
| 变异内容 | `src/server/http.rs:411-414` 由 `node_keys(&view).zip(objects).collect()` 退回 d216b7a 的 `view.objects.iter().map(\|o\| (o.name.clone(), o.clone())).collect()`（**按展示名建表、后者覆盖前者**，即原 P1） |
| patch sha256 | `d905ae71677c80f1c6873a84b709dfe1337dcfeb6f4a8db0566b661b6596fb54` |
| 被改文件工作区 sha256 | `b20740b834c68d39647b5f572d71b37ab94f20f70112f7e25fe4e7319f849aa0` |
| 变异期二进制 sha256 | lib 测试 `a67033708bad8c3a0dd06d00b740e2da96256863b99e9d65947295023a2ff47e`；serve `a2ed820418e670d75a74447cc83de24361d1b4bb18f79fc99a3cfe949ff3fea6`（两者均 ≠ 基线 ⇒ **确实编译进去了**） |
| 编译 | exit 0，`^error` 计数 0 ⇒ 编译失败不成立（纪律 G） |
| 运行条数与逐条结果 | 2 次：`every_node_including_duplicates…` **PASS**、`duplicate_names_get_distinct…` **PASS** |
| 运行时取证 | 变异二进制起服务**当场 panic**：`http.rs:416 assertion left == right failed，left: 90, right: 91` —— 正是重名塌缩的 P1 签名 |

**⇒ 判读：这两条测试**没有**守住生产接线。** 原因是测试自建 `AppState`（`http.rs:801-805`）时**又抄了一遍**接线，而 `serve()` 是唯一会用生产接线的地方却不被任何测试调用 —— 这正是纪律 H 的形态（忠实的重复实现）。
影响边界：**今天产品是对的**（§2 已实测）；缺的是回归网。另注：唯一运行时兜底是 `http.rs:415-419` 的 `debug_assert_eq!`，**release 下不存在**（§1.3）。

### 6.2 M2 · 抽掉唯一性检查（**等价变异**，如实记录）

| 项 | 值 |
| --- | --- |
| 变异内容 | `graph.rs:169-180` 的 `loop` 唯一性搜索换成直接 `format!("{}#2", o.name)` |
| patch sha256 | `daf4fb744d3ef68c700de13a6b8ae73b0aa6706eb03699c07d47af237ec0b783` |
| 被改文件工作区 sha256 | `a4a08d9572b70204e87dc3d027ca9f2bdc37cdf3c1e79b7515b73a74f24351eb` |
| 变异期二进制 sha256 | `a37685d519352d3f896fd459f8f0e1d9a492c627ba0501e0693a23ecf752a85c` |
| 运行条数与逐条结果 | 2 次，**均 PASS** |

**判读：等价变异，对本样本无效** —— 样本只有 2 个同名对象，`#2` 仍唯一；**三个同名**才会撞键。也就是说该测试样本覆盖不到「三重重名塌缩」这一形态。不算缺陷（当前代码有 `used` 集合保证唯一），但记为守卫强度上的一个已知盲区。

### 6.3 M3 · `node_keys` 不再共用 `assign_keys`（「各自分配」）

| 项 | 值 |
| --- | --- |
| 变异内容 | `graph.rs:197-199` 的 `node_keys` 改为自带一份分配 `v.objects.iter().map(\|o\| o.name.clone()).collect()`，不再调 `assign_keys` |
| patch sha256 | `e4eb9adb96dda9fb606a8614053710f4c2a430994f5ca2b45e657b036f4a3d27` |
| 被改文件工作区 sha256 | `406fb305dfe341d3af92e072fc61bbf74bddb1b9cfa40253accde641304d5692` |
| 变异期二进制 sha256 | `2c7ee716d346b8236e4fa9b3c5ea49165a5f7dddc0874066bd8a851a5b276b6a` |
| 编译 | 无 `could not compile` / 无 `error[E`；日志里的 `error: test failed` 是**运行时**测试失败，非编译错误 |
| 运行条数与逐条结果 | 2 次：`every_node_including_duplicates…` **FAILED**、`duplicate_names_get_distinct…` PASS |
| 失败原文 | `panicked at src/server/http.rs:838`：`assertion left == right failed: 点开后的属性条数必须等于图上标的：id="客户账号绑定"` |

**判读：✅ 有效。** 两边一旦各自分配，`every_node_including_duplicates…` **当场转红** —— 它确实钉住了「共用分配」这个不变量。

### 6.4 M4 · 只让图侧漂移（触发 404 分支）

| 项 | 值 |
| --- | --- |
| 变异内容 | `graph.rs:216` `key: key.clone()` → `key: key.replace("#2", "#3")` |
| patch sha256 | `98ca3f0c86ea11597441824f3b21b105cb113659bfcf50b48a3d557208546a9c` |
| 被改文件工作区 sha256 | `9bb49029eb945e5d78568f4ad2165b15d9688df4f077d2d09775ec24e4c28f33` |
| 变异期二进制 sha256 | `39757594b8d41fe2a5bea4e0c3ef7907aed8c9f66f4e3db5fd45e0302a3ccd3e` |
| 运行条数与逐条结果 | 1 次：`every_node_including_duplicates…` **FAILED** |
| 失败原文 | `panicked at src/server/http.rs:829`：`assertion left == right failed: 图上的每个节点都要打得开，404 只读服务就是残废的：id="客户账号绑定#3"` |

**判读：✅ 有效。** 404 分支与属性条数分支**都被真实触发过**（M4 打 404 分支、M3 打条数分支），不是只写了一行没跑过的断言。

### 6.5 第二条新测试钉的是行为还是字符串？

`duplicate_names_get_distinct_node_ids_ending_in_a_hash_two`（`http.rs:848-855`）**两者都钉，但以字符串为主**：

- `assert_eq!(ids, vec!["客户账号绑定", "客户账号绑定#2", "客户"])` —— **字面量**；
- `assert_eq!(attribute_count, 3 / 1)` —— 行为。

判读：**偏过紧**。前端**不依赖**这个格式 —— `src/server/page.rs:230` 只是把 id 原样 `encodeURIComponent` 回传，从不解析 `#N`；所以把消歧改成 `-2` 不会破坏任何真实功能，但这条测试会红。
不过测试**名字本身已声明**它钉的是「ids ending in a hash two」，读代码的人不会被误导，且它作为「现状契约」有一定价值。**判为可接受的取舍，非缺陷**，但不宜被当成「行为等价性」的守卫 —— M3 变异下它**没有转红**就说明了这一点。

### 6.6 变异后还原校验（交付未被改动）

```
git status --porcelain      → 空
git diff --stat             → 空
git rev-parse --short HEAD  → 0863fdd
sha256 http.rs  → a418d399…4f4d6542d   （= 变异前记录）
sha256 graph.rs → 0e78a271…a03b1042f   （= 变异前记录）
```

---

## 7. 第七节 · 我推翻了什么

| # | 开发方/任务书的说法 | 我的实测 | 性质 |
| --- | --- | --- | --- |
| 1 | 「`every_node_including_duplicates…` 是 P1 的防回归」 | **把原 P1 原样放回 `serve()`，它照样 PASS**；变异二进制起服务直接 panic（90 vs 91） | **推翻**：测试不守护生产接线 |
| 2 | 「`table_names.json` 未入库」 | **该路径确实在 commit 里**（`git cat-file -e HEAD:…` 成功）。入库的是 `1.3.0-desensitized` 占位样例（91 条 `t001→对象 001`）；真实的 `1.2.0` 资产是本 worktree 的 skip-worktree 覆盖位。⇒ **「真实内容未入库」为真，「文件未入库」为假** | **订正措辞**（不构成缺陷：6 文件 diff 不含它） |
| 3 | 「`test_proposal_item_by_seq` 本来就慢（60.61s）且通过」 | 单跑 **6.33s** 通过；全量里整个 `test_proposal` suite 也只 51.17s | **推翻该数字**（结论「慢但能过」仍成立，但归因应是并行争用） |
| 4 | 3 个 500 的文案含「`CDC/table/op/pk/lsn`、`Kafka key`、`API-Key`」 | 实际命中词是 `pk`（1 个）与 `key`（2 个）；文案是「API **Key ID**」 | **订正措辞**（实质判断成立） |
| 5 | 「四条路径 405 正文均 33 字节同一句」 | 16/16 实测一致 | 确认 |

---

## 8. 发现清单（按严重度）

### 🟠 P2 · 「P1 防回归」测试不守护生产接线（唯一实质发现）

- **触发条件**：任何人把 `src/server/http.rs:411-414` 改回按展示名建索引（P1 的原样），或在该处引入任何键构造改动。
- **影响**：**今天无客户可见影响**（§2 实测 91/91 正确）。风险是**未来回归不被测试拦下** —— 而这个功能的全部价值就是「点得开、数字对得上」。release 构建下连 `debug_assert_eq!` 兜底都没有。
- **证据**：§6.1（M1）—— 变异编译成功（exit 0、0 编译错误）、二进制 sha256 已变、2 次运行全绿。
- **根因**：`http.rs:801-805` 的测试夹具**抄了一份** `serve()` 的接线（纪律 H 的忠实重复实现）。
- **建议方向**（不在本轮范围）：让测试走生产接线（例如把 `serve()` 的索引构造抽成一个被测函数），或补一条断言「`serve()` 用的键集合 == `graph::node_keys()` 的键集合」。

### 🟡 P3 · `duplicate_names_get_distinct…` 钉的是字面量格式

- **影响**：换一种等价的消歧写法会误红；且它在 M3 下不转红 ⇒ 不能当行为守卫。
- **证据**：§6.5；`page.rs:230` 不解析格式。

### 🟡 P3 · 测试样本只有 2 个同名对象

- **影响**：三重重名导致的键塌缩形态未被覆盖（§6.2 等价变异）。
- **证据**：M2 patch `daf4fb…`。

### 🔵 P4（信息级，不阻断）

1. **守门日志前缀误导**：`mcp/tools.rs:701` 硬编码 `✗ mcp-server 守门器命中…`，HTTP 路径命中时也打「mcp-server」（我的 serve.log 里可见）。运维排障时会被指向错误的组件。
2. **`graph.rs:142` 的 `/// 标识分配的结果。`**：`Keying` 的文档只剩这一行残句式摘要（对比 `assign_keys` 有完整文档块）。可能是有意的一行摘要，也可能是搬移函数时的残留；不影响编译与行为。
3. **`assign_keys` 的签名**用了全限定路径 `crate::domain::view::EntityView`（`graph.rs:163`），而同文件测试模块用 `use …::EntityView` —— 风格不一致。

---

## 9. 纪律执行自查

| 纪律 | 本轮怎么用的 |
| --- | --- |
| E（零命中先怀疑扫描器） | 自写剥注释器先做正/负探针自检；405 一组同时测「带口令 GET=200」「缺参=400」「无口令=401」作对照，排除「全被拒」 |
| F（不存在是异常值） | `table_names.json`「不在 commit 里」被证伪，随即用 `git show HEAD:<path>` 核对入库内容（纪律 K） |
| G（变异三元组） | 4 个变异各给 patch sha256 + 被改文件工作区 sha256 + 变异期二进制 sha256 + 运行条数与逐条结果；并逐个确认**不是编译失败** |
| H（一致性守卫抓不到忠实重复） | 本轮最关键的判断；§1 查「是否只有一份分配」，§6.1 查「测试是否真的守住了它」 |
| I（每条命令自带 cd） | 全部命令均带绝对路径或 `cd` |
| J（禁 `2>/dev/null`） | 采集一律 `> file 2>&1`；`serve.log` 正是靠 stderr 才抓到守门词 |
| K（入库版用 git show） | 基线逐文件计数、table_names 对照、d216b7a 旧文案，全部用 `git show` |
| L（查环境变量来源） | `.env` 在仓库根；连接串全程未 echo |
| N（skip-worktree 逐 worktree） | 只读确认位为 `S`，未提交 |
| O（基线绑 commit） | §5.1 标注 `@ 0863fdd`；未重跑的基线已显式声明并给出旁证 |
| Q（扫描型守卫取工作区 sha256） | 已区分：M1–M4 是运行时行为守卫（二进制 sha256 有效），同时另记工作区 sha256 |
| R（「只读」是哪个面） | 已核 `cli.rs:704` 无条件 `migrate()` |
| T（精确 PID，不误杀） | 只杀自己起的进程；`18812` 的 PID 856174 全程未碰，复核仍在 |
| S（AC 自己也会错） | `--help`、domain 口径、405、91 节点全部先自己跑一遍 |

**产物**：`/home/myg133/Projects/Anchnet/Ontology/feature-REQ-093/.review-workspace/req093qa2/`
（`QA2.md` / `sweep.py`+`sweep.json` / `graph.json` / `full-test.log` / `M1–M4.patch` / `serve.log`）

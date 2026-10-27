# REQ-159 构建端 · QA 发现的缺陷（按级别）

> 只写我**自己跑出来 / 自己逐行核过**的事实。每条都带复现路径。
> 结论速览：**阻断 1 条**（F-01）、**非阻断 4 条**（F-02…F-05）、**契约缺陷归 join 1 条**（F-06）。
>
> 📌 **修复轮（HEAD `2701e14`）复审：F-01…F-05 已全部修好**（我独立复核，见
> `07-fix-round/FIX-REVIEW.md` 与 `REPORT.md` §7）。**F-06 仍 open（归 join）**。
> 下面每条的正文保留 `296d59d` 时的原始证据，作为「退回依据」的存档。


---

## F-01 · 【阻断】`bundle verify` 的 `sha256` 字段路径被**双前缀**；且仓库自带库内单测在 HEAD 上就是红的

**级别**：阻断（自带单测失败 + 用户可见的错误字段路径 + 违反「报具体字段路径」的契约）
**位置**：`src/bundle/verify.rs:707`、`:739`（`check_sha256` 在 `:749` 又补了一次前缀）

### 复现 1：跑仓库自己的库内单测（56 秒，一条命令）

```
cargo test --lib bundle::        # 工作树干净，HEAD 296d59d，共享热 target

test bundle::verify::tests::sha256_must_be_64_lowercase_hex ... FAILED
panicked at src\bundle\verify.rs:935:9:
assertion failed: out.iter().any(|p| p.path == "manifest.json.files[0].sha256")
test result: FAILED. 13 passed; 1 failed; 0 ignored; 832 filtered out
error: test failed, to rerun pass `--lib`
```

⇒ `cargo test`（全量）在 HEAD 上**必然失败**。Dev 自述的「29 passed」只覆盖
`tests/test_req159_bundle.rs`，没覆盖库内单测。证据：`01-baseline-lib.log`

### 复现 2：CLI 侧同一个缺陷（我造的坏包 B5v）

```
$ ontok-builder bundle verify --file B5v.ontokbundle      # files[0].sha256 = "A"*64
bundle verify · cloud-resource 第 5 版 · 包格式 1.0 · 1 个文件 / 1972 字节
✗ 拒绝：2 个问题（退出码 2）
  [bundle.manifest.invalid] manifest.json.manifest.json.files[0].sha256
```

正确应当是 `manifest.json.files[0].sha256`。证据：`05-E-exit/results.json` 的 `B5v`

### 根因

```rust
fn check_sha256(sha: &str, at: &str, out: &mut Vec<Problem>) {
    out.push(Problem::new(code::MANIFEST_INVALID,
        format!("manifest.json.{at}"),      // ← 这里补一次
        …));
}
```

| 调用点 | 传进来的 `at` | 结果 |
| --- | --- | --- |
| `:665`（`components.db.sha256`） | `"components.db.sha256"` | `manifest.json.components.db.sha256` ✅ |
| `:707`（`files[i].sha256`） | `format!("manifest.json.{base}.sha256")` | `manifest.json.manifest.json.files[0].sha256` ❌ |
| `:739`（`component_common` → `components.ontology/am.sha256`） | 同上 | `manifest.json.manifest.json.components.ontology.sha256` ❌ |

3 个调用点错 2 个。

### 修法

把 707 / 739 两处的 `format!("manifest.json.{base}.sha256")` 改成 `format!("{base}.sha256")`
（或把 `check_sha256` 里的前缀去掉，二选一）。改完 `cargo test --lib bundle::` 应 14/14 绿。

### 为什么集成测试没抓到它

三条 `reject_sha256_mismatch_*` 只断言 `code` 与「path 含 `snapshot.json`」/「path == `db/schema.json`」，
**没有一条断言 `files[].sha256` 出错时的字段路径**。唯一断言它的库内单测恰好是红的那条。

---

## F-02 · 【非阻断】快照里 `"evidence": null` 被当成「有证据」放行

**位置**：`src/bundle/verify.rs:424`（`o.contains_key("evidence")`）

| 输入 | 我的期望 | 实际 |
| --- | --- | --- |
| `link_types[0]` 里 **没有** `evidence` 键（视图形状） | exit 2 + `evidence_missing` | ✅ exit 2 + `bundle.snapshot.evidence_missing` |
| `link_types[0].evidence = null` | 应当也拒（`Snapshot.evidence` 是 `Vec<Evidence>`，`null` 反序列化会失败） | ⚠️ **exit 0 放行** |

证据：`05-E-exit/results.json` 的 `F1` / `F2` / `F3`

- 影响：这种包构建端说「通过」，服务端 `serde` 反序列化 `Vec<Evidence>` 时会失败 ⇒ **假绿**。
- 概率：真实 `Snapshot` 序列化出来永远是 `[]`/数组，`null` 只有手写或别家工具才会产生 ⇒ 低。
- 修法：判据改成「键存在 **且** `is_array()`」。
- 判定：非阻断（建议修，一行）。

---

## F-03 · 【非阻断 / 观察】`verify` 不要求 `manifest.json` 是第一个 entry

**位置**：`verify.rs:235`（`entries.iter().find(|e| e.path == MANIFEST_PATH)` —— 按名字找，不看位置）

我把一个**完全合法**的包改成 manifest 放在最后一个 entry：`verify` ⇒ **exit 0**。
（`pack` 侧确实把 manifest 放第一，A 组 M2 变异也证明那条断言有牙。）

- 影响：契约 §三 说 manifest 是「包的目录」，`pack` 的确定性要求 manifest 恒第一；
  但**自检不查位置**。如果将来服务端 P1a 想「先读 manifest 再流式解包」，
  就会依赖一个构建端**没有强制**的性质。
- 判定：非阻断，**建议与 join（服务端）对齐口径**：要么 verify 也查，要么契约明确「位置不承诺」。

---

## F-04 · 【非阻断，但是「pack 与 verify 打架」】`a..b.json` 这种文件名：pack 放行、verify 拒绝

**位置**：`src/bundle/zip.rs:150-178`（`illegal_path_reason` 只看**整段**是否等于 `..`）
vs `src/bundle/verify.rs:485-493`（`path_problem` 按 schema 字面查 **子串** `..`）

我的探针（副本里，纯函数层）：

```
QA_DOTDOT entries=[("manifest.json","（pack 认为合法）"),
                   ("declarations/a..b.json","（pack 认为合法）"),
                   ("ontology/snapshot.json","（pack 认为合法）")]
QA_DOTDOT problem=manifest.json.files[0].path
          声明的路径 `declarations/a..b.json` 非法：含 `..`（schema 的 path pattern 不许，防穿越）
```

⇒ `assemble` 成功、`verify` 失败。**「打得出来的一定过得了自检」这条不变式不成立**
（`verify.rs:484` 的注释正是这么承诺的）。

- **真实可达**：`--declarations <dir>` 里放一个 `a..b.json`（Windows / Linux 都允许这种文件名），
  或 `--am a..b.json`。此时 `bundle pack` 会打完包、自检不过 ⇒ 按 `296d59d` 的新逻辑
  **exit 2** 并打「✗ 打包产物过不了自己的自检 —— 这是打包端的 bug」，把一个**参数问题**
  报成了**打包端 bug**（错误指向错的原因，正是这个项目最在意的 doctype）。
- 本机无元数据库 ⇒ **CLI 端到端没跑**（要连库取快照）；上面是纯函数层的**确定性**证据。
- 修法：把 `illegal_path_reason` 也拒 `..` 子串（与 schema 的 `(?!.*\.\.)` 对齐），
  这样 `pack` 会在**参数校验**阶段就报「包内路径非法」，而不是打完再自责。
- 判定：非阻断（触发条件罕见），但建议在 merge 前修 —— 它是**唯一一条**能让 `pack` 产出
  「自己拒自己」的路径。

---

## F-05 · 【非阻断 / 潜伏】`zip::write` 不检查文件名长度 > 65535，静默截断

**位置**：`src/bundle/zip.rs:233`、`:249`（`push_u16(&mut out, name.len() as u16)`）

探针实测（70020 字节的路径）：

```
QA_LONGNAME write_ok=true
QA_LONGNAME read_back_ok=false name_len_bytes=70020
```

`data` 超过 `u32::MAX` 有检查（`:209`），**名字长度没有**。⇒ 写出一个自己读不回来的容器。

- 可达性：今天从 CLI 到不了（`--declarations` / `--am` 的文件名受文件系统限制，`declarations/<name>` 最长几百字节）。
- 判定：非阻断（潜伏地雷）。修法：`write` 里加一条 `name.len() > u16::MAX` 的 `TooLarge`/`BadFormat`。

---

## F-06 · 【契约缺陷，归 join · 不作为打回理由】`manifest.example.json` 把 `manifest.json` 自己列进 `files[]`

`BA/demands/REQ-159/manifest.example.json` 的 `files[]` 列了 `manifest.json`，而自指不可能满足
（manifest 无法包含自己的 sha256）。**构建端选择「拒绝列出 manifest.json」**：

| 我的用例 | 实际 |
| --- | --- |
| B5n：`files[]` 里插入 `{"path":"manifest.json","sha256":"0"*64,"bytes":10}` | exit 2，`bundle.files.mismatch`（message 里点明「`manifest.json` 不许列进 `files[]`：它无法包含自己的 sha256」）+ 2 条 `sha256.mismatch` |

- 我认同构建端的取舍（除 `manifest.json` 自身外逐个覆盖，是唯一自洽的写法）。
- **要与 join 对齐**：服务端若照 example 实现「files[] 必须含 manifest」，两边会互相拒绝。
- 判定：**契约缺陷归 join**，不计入 Dev 的缺陷（派单 §1 已声明）。

---

## 附：一处**不是缺陷**但我核过的地方（写下来免得别人再查一遍）

- 8 个契约码 + 2 个构建端补充码在 `src/bundle/mod.rs:111-147` **只定义一处**，
  `code::ALL` 去重单测在（`all_codes_are_distinct_and_namespaced` ✅）。
- `PROJECT_MISMATCH` / `DSN_MISSING` 判不出来这件事**是写在代码注释与单测里的**
  （`server_only_codes_are_documented_as_such` ✅），不是静默漏掉 —— 这是诚实的做法。
- `verify` 的检查顺序（体积最先、版本先于文件集合）与契约 1..9 的顺序不同，但**报告指向真原因**，
  我按契约逐条构造的坏包全都能拿到对应码（B 组 34 条）。
- `bundle` 在「连库 + migrate」之前分派（`cli.rs:2045` vs `:2049-2052`）⇒ `verify` 不可能被
  「连不上库」挡住，也不可能触发迁移。这条是我在源码里核对过的，不是听 Dev 说的。

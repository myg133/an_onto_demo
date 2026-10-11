# 验收标准：REQ-072 真实 `review show` 二进制端到端验证（补齐未验证项）

> 依据 REQ-069 的 QA 验收（判 PASS，但明确标出**未验证项**）写成。
> 基线：REQ-069 合并后的 develop（`c3ddcd5`）。
> **纪律依据：「未验证的要写下来，不能并进已验」。本条就是那个未验证项的销账。**

---

## 问题：全部改动的验证都止于 domain 层

REQ-058 / 064 / 065 / 069 一路下来，验证对象一直是
`render_review_text` / `verify_track_separation` / `verify_provenance_scope`
这些 **domain 函数**。

**无任何测试 spawn 真实的 `review show` 二进制。**

⚠️ 本会话反复出现的病是「**测试测的不是你以为的那一层**」——
若真实二进制与 domain 函数之间还有一层转换（读库 → 组装 → 调 domain → 打印），
**那一层里的任何加工都会让上面所有结论作废**。

---

## AC-001 · 跑一次真实的二进制

- 预期结果：用 `CARGO_BIN_EXE_<name>` 或 `env!("CARGO_BIN_EXE_...")` **spawn 真实二进制**，
  喂一条含换行的 evidence payload，断言**stdout 里没有 payload 造出的新行**
- 类型：验证覆盖度
- ⚠️ **照本仓既有先例做**：`tests/test_extracted_by_provenance.rs:100`、
  `tests/test_extract_cli_req020.rs:84` 已有 `CARGO_BIN_EXE` 用法，
  **不要自创一套**

## AC-002 · 端到端断言必须**比 domain 层更严**

- 预期结果：端到端断言至少覆盖
  - payload 的 `\n` **不出现在 stdout 的任何一行里**（不是「判据没报警」）
  - payload 造出的伪结构行**不被打印成独立行**
- ⚠️ **不要**只断言「退出码 0」或「stderr 为空」——
  本项目已经出现过「读得到 ≠ 门禁会通过」这类**信号不承载含义**的问题

## AC-003 · 若发现二进制层与 domain 层行为不一致

- **这本身就是本条最有价值的产出。**
  请**如实报告差异**，不要为了让测试通过去改二进制。
- ⛔ **不要**为了让端到端变绿而放宽 domain 层的行为

---

## ⛔ BA 的范围判断：本条**不做**

1. **不改 domain 层逻辑** —— 本条是**验证覆盖度**需求，不是功能需求。
   若 AC-003 发现差异，另开需求。
2. **不新建二进制** —— 只跑既有的 `review show`
3. 不碰红线；不加 `#[ignore]`；不删测试

## ⚠️ 派单坑

1. 跑全量前 `set -a; . /home/myg133/Projects/Anchnet/Ontology/.env; set +a`，
   只打印变量**长度**不打印值；**不要**把连接串写进任何文件
2. **共享本体库只读**：端到端测试若需要库，**不得写入**；
   ⚠️ **2026-10-11 修订：本条指定的 `pg_temp` 跨进程结构上不可用，已被 Dev 否定并经 QA 实证**
   - `pg_temp` 是**会话私有**、每连接各一份；`src/db/pool.rs:26` 开 **5 连接池**，
     且本条**跨进程**（测试写、子进程读）。
   - QA 实测：两个 backend（3752159 / 3752160）各自 `SET search_path TO pg_temp` 后，
     解析到 **`pg_temp_36` 与 `pg_temp_34`（各一份）**；
     A 建的临时表在 B 侧 `to_regclass` 返回 `None`。
   - **现行做法**：改用**专属 schema**（`CREATE SCHEMA` + `SET search_path`，
     `tests/test_db.rs` 里 REQ-025 的先例），子进程经连接串
     `?options=-c%20search_path%3D…` 传入。
     QA 用**真实二进制双向实测**：不加则连 `public`，
     加指向不存在 schema 则报 `no schema has been selected to create in`
     ⇒ 证明该启动参数**确实到达服务端**并改变了子进程的 `search_path`。

   ### ⚠️「不得写入」措辞需追认（QA 提出，**BA 已裁定**）

   `CREATE SCHEMA` / `DROP SCHEMA` **本身是对共享库 catalog 的写**，
   与原文「**不得写入**」有字面张力。

   **BA 裁定：按「不触碰共享数据与既有命名空间」理解并追认。** 理由：
   ① 依据本会话既有纪律「共享本体库只读：仅 `information_schema`/`pg_catalog`，
   **零业务数据行**」—— catalog 写不在禁止之列；
   ② REQ-025 的先例已在用同一手段；
   ③ QA **在测试期间采样**（不是跑完之后）实证：`public` 里 `%req072%` 项目**全程 0**，
   专属 schema 跑完归零，**零 DELETE / TRUNCATE**。

   ⚠️ 但 **QA 同时指出巡检口径应放宽到仓级** ——
   库里已有 **5 个 `t_migrate_race_*` 残留**（`test_db.rs:141` 所建，**非本需求**），
   只盯 `t_req072%` 会漏。已记入 REQ-078。
3. 临时文件/报告写**仓库根** `.review-workspace/req072d/`
4. **本条的价值是「验到真的那一层」，不是「让测试变绿」。**
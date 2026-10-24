# 在 k8s 上加一个新项目（运维手册）

> **前提认知**：当前一套服务绑定 **一个项目 + 一个版本**。
> `query serve` / `portal serve` 启动时必须给 `--project` 与 `--version`，运行期不切换。
> 所以「加项目」= **加一套负载 + 一份声明**，不是点一个「新建项目」按钮。

---

## 一 · 六个步骤

```
① 源库只读账号（每项目一个）
      ↓  Secret: ontok-db { metadata-url, source-url }
② 建项目 + 抽取 + 人签 + 冻结（一次性）
      ↓  元数据库里的 project / version（已冻结）
③ 四份声明 + schema.json
      ↓  ConfigMap: ontok-<项目>-config
④ 三套口令
      ↓  Secret: ontok-<项目>-tokens
⑤ 三个 Deployment（--project/--version）
      ↓
⑥ Ingress（query / console / portal 三个入口）
```

---

## 二 · 逐步操作

### ① 源库只读账号

每个项目**自己的业务库**，给一个只读账号：

```sql
CREATE USER ro_<项目> WITH PASSWORD '...';
GRANT CONNECT ON DATABASE <业务库> TO ro_<项目>;
GRANT USAGE ON SCHEMA public TO ro_<项目>;
GRANT SELECT ON ALL TABLES IN SCHEMA public TO ro_<项目>;
ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT SELECT ON TABLES TO ro_<项目>;
```

### ② 建项目 + 抽取 + 人签 + 冻结（**一次性 Job**）

```bash
# 在能连「元数据库」和「源库」的环境里跑（CI runner / 跳板机 / 一次性的 k8s Job）
export ONTOK_DATABASE_URL='postgresql://.../ontok'          # 元数据库（共享）
export ONTOK_SOURCE_DATABASE_URL='postgresql://ro_xxx@.../biz'   # 该项目源库

# 2.1 勘查
ontok-introspect --out /work/<项目>/schema.json

# 2.2 提案（AI 提案，不权威）
ontok propose import ...   # 或先 ontok-propose 产出 proposal.json 再 import
ontok-propose --schema /work/<项目>/schema.json \
              --names  /work/<项目>/table_names.json \
              --exclude /work/<项目>/exclusions.json \
              --tenancy /work/<项目>/tenancy.json \
              --out /work/<项目>/proposal.json

# 2.3 人签（⛔ 不能自动化：这是唯一权威来源）
ontok review decide --proposal p1 --project <项目> --accept-all   # 或逐条
ontok proposal apply --proposal p1 --project <项目>
ontok version freeze --project <项目> --version <N>
```

> ⚠️ **`--names` 是必填**（REQ-128）：不传就直接失败，因为默认值是脱敏占位表。
> 用 `ontok-translate-names` 或 `ontok name suggest --as-names` 生成。

### ③ 四份声明 + schema

```yaml
apiVersion: v1
kind: ConfigMap
metadata:
  name: ontok-<项目>-config
  namespace: ontok
data:
  schema.json: |            # 该项目源库的勘查产物（上传前脱敏？它只有结构，无数据）
  query-policy.json: |      # 逐表租户列 + 公共维度（**必须每项目单独写**）
  tenancy.json: |
  exclusions.json: |
  portal-questions.json: |  # 客户门户的预置问题（必须有人签 reviewed_by）
```

### ④ 口令

```bash
kubectl -n ontok create secret generic ontok-<项目>-tokens \
  --from-file=admin-tokens.json \
  --from-file=query-tokens.json \
  --from-file=portal-tokens.json
```

`kind` 必须分别是 `admin` / `query` / `portal`，**同一张 token 不许两种 kind**。

### ⑤ 三个 Deployment

复制 `k8s/base/` 的 deployment，改三处：

| 改什么 | 例 |
| --- | --- |
| `ONTOK_PROJECT` env | `demo` |
| `ONTOK_VERSION` env | `2` |
| configMap/secret 名字 | `ontok-demo-config` / `ontok-demo-tokens` |
| `--policy` 等同参数 | 指向挂载后的路径（不变） |

推荐用 **kustomize overlay**，一个项目一个目录：

```
k8s/overlays/prod-demo/kustomization.yaml
  resources: [../../base]
  patches: 改 ONTOK_PROJECT / ONTOK_VERSION / configMap / secret 名
  images:  registry/ontok:0.1.0
```

### ⑥ Ingress

| 入口 | 建议域名 | 谁能访问 |
| --- | --- | --- |
| console | `console-<项目>.internal` | **只内网**（admin token） |
| query | `api-<项目>.internal` | 数字员工 |
| portal | `<项目>.example.com` | 客户 |

---

## 三 · 「给项目加文档」怎么做

### 现在能做的（三步）

```
① 文档放进 PVC（或对象存储挂载）
      ↓
② 一次性 Job 跑抽取
   kubectl apply -f job-doc-ingest.yaml
      ↓
③ 产物：assertions.jsonl + stats.json（带 evidence.quote）
```

Job 骨架：

```yaml
apiVersion: batch/v1
kind: Job
metadata:
  name: ontok-doc-<项目>
spec:
  template:
    spec:
      containers:
        - name: ingest
          image: registry/ontok:0.1.0
          args:
            - doc
            - ingest
            - --input=/data/docs
            - --out=/data/out/assertions.jsonl
            - --stats=/data/out/stats.json
          env:
            - name: ONTOK_LLM_BASE_URL
              valueFrom: { secretKeyRef: { name: ontok-llm, key: base-url } }
            - name: ONTOK_LLM_API_KEY
              valueFrom: { secretKeyRef: { name: ontok-llm, key: api-key } }
          volumeMounts:
            - { name: docs, mountPath: /data/docs, readOnly: true }
            - { name: out,  mountPath: /data/out }
      volumes:
        - name: docs
          persistentVolumeClaim: { claimName: docs-<项目> }
        - name: out
          persistentVolumeClaim: { claimName: work-<项目> }
      restartPolicy: Never
```

### ⚠️ 但**文档抽完进不了本体**

`ontok doc ingest` 的帮助原话：

> 「**第一步**：非结构化文档 → 带证据的断言」「本命令**只做第一步**」

⇒ 产物是 `assertions.jsonl`，**没有**「断言 → 本体提案」的命令。
现在文档只能到「断言 + 证据」，**要人工看 JSONL 再手工整理**。

**要闭环，需要新需求**：断言 → 提案的合并器（含「文档里的『套餐』对应库里哪张表」这个判定，那本身也要证据 + 人签）。

---

## 四 · 两个必须先定的设计问题

### ① 要不要「多项目单服务」？

| 现在 | 如果改成多项目 |
| --- | --- |
| 一套负载 = 一个项目 | 一套负载服务多个项目，请求里带项目标识 |
| 加项目 = 加一套 Deployment（资源翻倍） | 加项目 = 加一条记录（但要在每个请求里路由 + 隔离） |
| 隔离靠**部署边界**（最硬） | 隔离靠**代码正确性**（要测试兜底） |

**目前的取舍是刻意选「部署边界」**——因为这套系统里最贵的错误是跨租户泄漏，部署隔离是最可靠的那道墙。

### ② 项目数量的现实上限

| 项 | 现值 |
| --- | --- |
| 每套：3 个 Deployment × 1 副本 | 3 Pod |
| 每 Pod 内存 | request 256Mi / limit 1Gi |
| 10 个项目 | 30 Pod / 内存 request 2.5Gi |

**10 个以内用「一套一项目」完全可行；到几十个就该开「多项目单服务」的需求。**

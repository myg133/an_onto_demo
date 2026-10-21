# 状态：REQ-109 · AI 语义项接入

## 当前状态

**进行中**（2026-10-21 · REQ-108 已合并，Dev 开发中）

## 前置已满足

REQ-108 已于 2026-10-21 合入 `develop` `a8a51ed`（`confidence` 结构 / `semantic_fit` 权重位 / 自证链校验均已在主干）。
本需求直接基于 `feature-REQ-109`（base `a8a51ed`）开发。

## LLM 端点（实测通）

| 项 | 值 |
| --- | --- |
| Base URL | `https://ollama.apps.anspire.cn/v1`（OpenAI 兼容） |
| Model | `qwen36-27B` |
| 连通性 | `/v1/models` 正常；一次 chat 请求 usage **17 / 2** tokens |
| 凭据 | ⛔ **只进环境变量**，不落任何被跟踪文件（含测试/文档/示例/日志） |

## 已完成

- [x] `demand.md` —— 依据（离线对照 11 条修正、0 条被推翻）、范围、R1–R7 裁定
- [x] `acceptance.md` —— 12 条 AC，全部可用 scripted transport 验证（无 key 也能跑）
- [x] `design-summary.md` —— 指纹构成、缓存表、v2 权重（AI 占 15%）、协议、开关
- [ ] 等 REQ-108 合并
- [ ] Dev 实现

## 关键设计判断

| 项 | 决定 | 理由 |
| --- | --- | --- |
| AI 权重 | **0.15**（其余等比压缩） | 等比压缩保证「AI 缺失时」与 REQ-108 逐位一致（AC-001） |
| 幂等 | 指纹缓存，prompt/model 进指纹 | `temperature=0` 靠不住；只有地址能保证 |
| 无 key | **报错退出** | 静默跳过会让产物看着正常其实没跑 |
| veto | AI 不能翻转 | 语义分再高，0 命中仍是 ≤0.20 |
| 范围 | 只判「候选集合内」 | G1 已登记：候选里没有正确目标时 AI 无能为力 |

## 依赖与阻塞

- **阻塞**：REQ-108 未合并（当前「已退回·第 3 轮修复中」）
- 无外部阻塞（不需要额外凭据即可开发；真实运行才需要 key）

## 真实运行需要（交付后由用户提供）

```powershell
$env:ONTOK_LLM_BASE_URL = "https://…/v1"
$env:ONTOK_LLM_API_KEY  = "sk-…"
$env:ONTOK_LLM_MODEL    = "…"
```


# 发布到 npm · 操作手册

> 给**维护者**看（不是给 FDE）。FDE 只需要 [`../fde/INSTALL.md`](../fde/INSTALL.md)。

## 一 · 认证方式：二选一

| 方式 | 什么时候用 | 优点 | 缺点 |
| --- | --- | --- | --- |
| **A · Granular token**（写进 `NPM_TOKEN`） | 想少配置 | 一把 token 搞定 | **账号开了 2FA 时可能要求 OTP**（`EOTP`）——那种情况只能用 B |
| **B · Trusted publishing（OIDC）** | CI 里完全不要 token | 最安全、无过期、自动 provenance | 需要**包已存在**（首次发布得先手动） |

工作流的 preflight 会自动判：**有 `NPM_TOKEN` 就用它，没有但有 `id-token` 权限就走 OIDC**。

---

## 二 · 第一次发布（包还不存在 ⇒ 必须手动一次）

> 为什么必须手动：npm 的 trusted publishing 配在**包设置页**上，包不存在就没有那个页面。

### 1 · 空跑一次，拿到 4 个包

GitHub → Actions → `release` → Run workflow（`publish` **不勾**）→ 跑完下载 artifact **`npm-release-bundle`**（一个 zip，里面就是 4 个包）。

### 2 · 本机发布（会问你 OTP，输入即可）

```bash
unzip npm-<版本>.zip -d out
cd out/@anspire-ai

# 先发三个平台包，最后发主包（主包靠 optionalDependencies 找它们）
npm publish ontok-builder-win32-x64   --access public
npm publish ontok-builder-darwin-arm64 --access public
npm publish ontok-builder-linux-x64   --access public
npm publish ontok-builder             --access public
```

⚠️ 发布前先 `npm login`（用有 `@anspire-ai` 权限的账号），并用 `npm whoami` 确认。

### 3 · 验证

```bash
npm i -g @anspire-ai/ontok-builder --include=optional
ontok-builder --version     # 必须等于刚发的版本
```

---

## 三 · 之后：接上 CI 自动发布

### 方式 B（推荐）：trusted publishing

在 npm 上给**每个包**（4 个）配：

```
npmjs.com → 包页面 → Settings → Trusted publishing → Add
  Provider:      GitHub Actions
  Organization:  myg133
  Repository:    an_onto_demo
  Workflow:      release.yml
  Environment:   （留空）
```

配好之后：**不用 `NPM_TOKEN`**，工作流会走 OIDC。

### 方式 A：granular token

```
npmjs.com → Access Tokens → Generate New Token → Granular
  Packages:    @anspire-ai/*（或全选）
  Permissions: Read and write
  Expiration:  按公司策略
```

把 token 存成仓库 secret `NPM_TOKEN`。

⚠️ 如果发版时仍然 `EOTP`：说明你的账号强制 2FA 且 granular token 不能绕 ⇒ **改走方式 B**。

---

## 四 · 正式发版（每次）

```bash
# 1) 先升版本（唯一源是 Cargo.toml，Cargo.lock 要一起）
# 2) 跑守卫
python Deploy/tools/check_versions.py --tag v<版本>
# 3) 打 tag（⛔ 只能推一次，发布成功后不可复用）
git tag v<版本> && git push origin v<版本>
```

工作流会：三平台构建 → 生成 4 个包 → **平台包先发、主包后发** → 从 registry 装回来冒烟。

---

## 五 · 报错对照表

| 报错 | 含义 | 处理 |
| --- | --- | --- |
| `EOTP` | token 需要一次性密码 | 用 granular（read-write）**或**改 OIDC |
| `E403` | 对该 scope 无发布权 | 确认是 `@anspire-ai` org 成员 |
| `E404` | scope 不存在 | 先在 npm 建 org |
| `EPUBLISHCONFLICT` / `cannot publish over` | 该版本已发布 | **升版本号**，⛔ 不要删 tag 重发同版本 |
| `缺平台包 @anspire-ai/ontok-builder-<plat>` | 用户装的是不支持的平台 | 见 INSTALL.md 的平台表 |
| preflight `npm whoami` 失败 | token 无效/过期 | 重建 token 或换 OIDC |

---

## 六 · 一条纪律

> **版本号不复用。** 一旦某个版本发布成功（registry 上能查到），就只能升版本号；
> 只有在"一个包都没发出去"的失败场景下，才允许删 tag 重推。

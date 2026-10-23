# 镜像构建命令

> 现状（2026-10-23 实测）：**podman 的 WSL 机器里已配好代理环境变量**，
> 所以构建**不需要**再传 `--build-arg HTTP_PROXY=…`。

---

## 一 · 一条命令

```bash
cd code
podman build -t ontok:0.1.0 -f Dockerfile .
```

镜像里包含：

| 内容 | 说明 |
| --- | --- |
| `ontok` / `ontok-introspect` / `ontok-propose` / `ontok-translate-names` | 四个二进制（musl 静态链接，alpine 运行） |
| **Rust 1.96.0** | 构建期工具链；`rust-version` 声明是 **1.88**（依赖的硬下限） |
| **bun** | 装包 + 运行 pi |
| **pi 1.4.0 / pi-acp 0.0.34** | 抽取 agent 与 ACP 适配器（ACP 需要本地子进程，故与服务同镜像） |

Dockerfile 内置的源（VM 内实测可达）：

| 用途 | 源 |
| --- | --- |
| apk | `mirrors.aliyun.com`（`--build-arg APK_MIRROR=` 可换） |
| rustup | `rsproxy.cn` |
| crates | `mirrors.aliyun.com/crates.io-index` |

---

## 二 · 构建完先验证

```bash
# 1) 主命令
podman run --rm ontok:0.1.0 --help | head -20

# 2) 四个二进制
podman run --rm --entrypoint sh ontok:0.1.0 -c \
  "for b in ontok ontok-introspect ontok-propose ontok-translate-names; do printf '%s: ' \$b; \$b --help >/dev/null 2>&1 && echo ok || echo FAIL; done"

# 3) pi / pi-acp / bun（关键：pi 是否能在 bun 下跑）
podman run --rm --entrypoint sh ontok:0.1.0 -c "bun --version; pi --version; pi-acp --help 2>&1 | head -3"

# 4) 服务参数解析
podman run --rm --entrypoint ontok ontok:0.1.0 query serve --help | head -12
```

⚠️ 第 3 条如果 `pi --version` 报 node 版本/模块错误，说明 bun 跑不动这个 bundle。
补一行即可：

```dockerfile
RUN apk add --no-cache nodejs
```
（或把 `bun add -g` 换成 `npm i -g`，但需要 node 22+）

---

## 三 · 打 tag 并推仓

```bash
podman tag ontok:0.1.0 registry.example.com/ontok:0.1.0
podman push registry.example.com/ontok:0.1.0
```

多架构：

```bash
podman build --platform linux/amd64,linux/arm64 \
  --manifest registry.example.com/ontok:0.1.0 -f Dockerfile .
```

---

## 四 · 接到 compose / k8s

**compose**（`Deploy/compose/.env`）：

```dotenv
ONTOK_IMAGE=registry.example.com/ontok:0.1.0
```

**k8s**（`Deploy/k8s/overlays/prod/kustomization.yaml`）：

```yaml
images:
  - name: ontok
    newName: registry.example.com/ontok
    newTag: 0.1.0
```

---

## 五 · 备选：在**没有代理**的机器上构建

```bash
podman build -t ontok:0.1.0 -f Dockerfile . \
  --build-arg HTTP_PROXY=http://<proxy-host>:<port> \
  --build-arg HTTPS_PROXY=http://<proxy-host>:<port>
```

> podman VM 看到的宿主机网关一般是 `172.17.0.1`（`podman machine ssh "ip route"` 可查）。
> 仅当 `--build-arg` 给了值时，Dockerfile 才会为 build/runtime 阶段设置代理环境变量。

---

## 六 · 已知的坑（本地实测记录）

| 现象 | 原因 | 处理 |
| --- | --- | --- |
| `rustc 1.86.0 is not supported ... home@0.5.12 requires rustc 1.88` | **`rust-version` 与实际锁文件不符**（已修：1.86→1.88，工具链→1.96） | 用当前 Dockerfile |
| `podman run` 报 `controller 'pids' is not available` | VM cgroup 控制器异常 | `--cgroups=disabled`，或 `podman machine stop && start` |
| `cargo` 卡在 `Updating crates.io index` | `index.crates.io` 不可达 | Dockerfile 已配阿里云镜像 |
| rustup 下载极慢 | 官方源限速 | Dockerfile 已改用 `rsproxy.cn` |
| 大镜像拉取 `TLS handshake timeout` | 部分镜像源 R2 后端不稳 | 只拉小镜像 / 已本地化的基础镜像 |

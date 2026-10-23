# 镜像构建命令（你来跑）

> 这台机器出网受限（daocloud 大镜像 TLS 超时、docker.io 层不可达、crates.io 索引不可达），
> 所以**构建请在你自己那边跑**。下面是完整命令，按需选。

---

## 一 · 前置

| 项 | 要求 |
| --- | --- |
| 构建工具 | `podman`（推荐）或 `docker` |
| 出网 | 能访问 Docker Hub（或国内镜像源）+ crates.io（或镜像）+ `bun.sh` |
| 磁盘 | ≥ 4 GB 可用（Rust 工具链 + 依赖） |

---

## 二 · 最简：一条命令

```bash
cd code
podman build -t ontok:0.1.0 -f Dockerfile .
```

Dockerfile 已经内置：
- 基础镜像 `alpine:3.20`
- apk 源 → `mirrors.aliyun.com`（可用 `--build-arg APK_MIRROR=` 换）
- crates → `mirrors.aliyun.com/crates.io-index`（写好 `/usr/local/cargo/config.toml`）
- rust 1.86 通过 **rustup 官方脚本**装
- 运行时：`bun` + `pi@1.4.0` + `pi-acp@0.0.34` + 四个 ontok 二进制

---

## 三 · 有代理（你的场景）

本机 podman VM 看到的宿主机网关是 `172.17.0.1`，你的代理在 `10808`：

```bash
cd code
podman build -t ontok:0.1.0 -f Dockerfile . \
  --build-arg HTTP_PROXY=http://172.17.0.1:10808 \
  --build-arg HTTPS_PROXY=http://172.17.0.1:10808
```

> 若代理是 SOCKS5：`--build-arg HTTPS_PROXY=socks5h://172.17.0.1:10808`
> （`curl`/`cargo` 支持 socks5h；`apk` 不一定支持，所以 apk 仍走阿里云源）

---

## 四 · 多架构（amd64 + arm64）

```bash
podman build --platform linux/amd64,linux/arm64 \
  -t registry.example.com/ontok:0.1.0 -f Dockerfile . \
  --manifest registry.example.com/ontok:0.1.0
```

---

## 五 · 构建完先验证

```bash
# 1) 主命令
podman run --rm ontok:0.1.0 --help | head -20

# 2) 四个二进制都在
podman run --rm --entrypoint sh ontok:0.1.0 -c \
  "for b in ontok ontok-introspect ontok-propose ontok-translate-names; do printf '%s: ' \$b; \$b --help >/dev/null 2>&1 && echo ok || echo FAIL; done"

# 3) pi / pi-acp / bun
podman run --rm --entrypoint sh ontok:0.1.0 -c "bun --version; pi --version; pi-acp --help 2>&1 | head -3"

# 4) 服务能起来（需要外部元数据库；这里只验启动参数解析）
podman run --rm --entrypoint ontok ontok:0.1.0 query serve --help | head -15
```

⚠️ **`pi` 若报 `cannot find module` 或 node 版本错误**：说明 bun 跑不动这个 bundle。
补救（Dockerfile 里加一行即可）：
```dockerfile
RUN apk add --no-cache nodejs \
 && ls /usr/local/bin/pi
```
或把 `bun add -g` 换成 `npm i -g`（需要 node 22+ 的镜像源）。

---

## 六 · 打 tag 并推仓

```bash
podman tag ontok:0.1.0 registry.example.com/ontok:0.1.0
podman push registry.example.com/ontok:0.1.0
```

---

## 七 · 接到 compose / k8s

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

## 八 · 这台机器上已知的坑（供你参考，不用踩）

| 现象 | 原因 | 处理 |
| --- | --- | --- |
| `podman pull` 卡在 Copying blob | daocloud R2 后端 TLS 超时 | 用本地已有镜像 / 换网络 / 走代理 |
| `podman run` 报 `controller 'pids' is not available` | VM 的 cgroup 控制器异常 | 加 `--cgroups=disabled`，或 `podman machine stop && start` |
| `cargo` 卡在 `Updating crates.io index` | `index.crates.io` 不可达 | 用 Dockerfile 里已配好的阿里云镜像 |
| rustup 下 200 MB 很慢 | 官方源限速 | 走代理，或换 `RUSTUP_DIST_SERVER` |

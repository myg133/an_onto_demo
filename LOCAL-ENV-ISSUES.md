# 本机 podman 环境缺陷与修法（2026-10-24）

> 这台机器（Windows + podman machine WSL + **rootful**）有两个会挡住 `podman run` 的环境问题。
> 都不是产品代码的问题，但每次重建/重启后会复发，所以记在这里。

---

## 问题 1 · `controller 'pids' is not available`

### 症状

```
Error: preparing container ... for attach: crun: controller `pids` is not available under
/sys/fs/cgroup/non-systemd/machine.slice/libpod-<id>.scope/container/cgroup.controllers:
OCI runtime error
```

rootless 时路径是 `.../user.slice/user-1000.slice/...`，同样报错。

### 根因

WSL 把 **podman 服务进程**放在 `/sys/fs/cgroup/non-systemd` 子树里，而这个子树的
`cgroup.subtree_control` **没有开启 `pids`**：

```
/sys/fs/cgroup/cgroup.controllers            → cpuset cpu io memory hugetlb pids rdma
/sys/fs/cgroup/non-systemd/cgroup.controllers → cpuset cpu io memory hugetlb pids
/sys/fs/cgroup/non-systemd/cgroup.subtree_control → （空）
```

**尝试手动开启会失败**：

```
echo +pids > /sys/fs/cgroup/non-systemd/cgroup.subtree_control
→ write error: Device or resource busy
```

原因：cgroup v2 不允许在**含进程的子树**上开新控制器。

### 修法（当前采用）

在 **rootful** 配置里关闭容器 cgroup：

```bash
podman machine ssh "printf '[containers]\ncgroups = \"disabled\"\n' | sudo tee /etc/containers/containers.conf"
podman machine stop && podman machine start
```

验证：

```bash
podman run --rm docker.m.daocloud.io/library/alpine:3.20 echo ok   # 不加任何 flag
```

### 代价（必须知道）

| 影响 | 说明 |
| --- | --- |
| 容器**没有内存/CPU 限额** | `--memory` / `--cpus` 不生效 |
| 容器**不能暂停/恢复** | pause/unpause 依赖 cgroup freezer |
| 对本机开发**可以接受** | 只有 postgres 一个容器；但**生产不要这么干** |

### 备选（未采用）

| 方案 | 为什么没选 |
| --- | --- |
| 每条命令加 `--cgroups=disabled` | 能work，但每个 `podman run` 都要记得加，容易漏 |
| 把 podman 服务改成 systemd 管理（使其进入 `system.slice`） | 要改 podman machine 的 VM 内部单元，重启后可能被覆盖；改动面太大 |
| 给 `/sys/fs/cgroup/non-systemd` 开 `+pids` | 报 `Device or resource busy`（子树的子树有进程） |

---

## 问题 2 · `libstdc++.so.6: No such file or directory`（`bun` 跑不起来）

### 症状

```
bun was installed successfully to /usr/local/bin/bun
Error loading shared library libstdc++.so.6: No such file or directory (needed by /usr/local/bin/bun)
Error relocating /usr/local/bin/bun: _ZSt20__throw_length_errorPKc: symbol not found
```

### 根因

alpine 基础镜像**只带 musl，不带 C++ 运行库**；`bun` 是 C++ 写的，动态依赖 `libstdc++`。

### 修法（已进 Dockerfile）

```dockerfile
apk add --no-cache ca-certificates tzdata bash curl unzip libstdc++ libgcc
```

> 我们自己的 `ontok` 二进制是 **musl 静态链接**，不受影响；出问题的只有后装的 `bun`。

---

## 问题 3 · 容器显示 `Exited (0) 292 years ago`

**原因**：磁盘修复/时钟偏移导致时间戳异常。
**处理**：`podman machine stop && start` 后恢复正常；**数据不受影响**（在卷里）。

# ontok 运行时镜像（官方 alpine + 构建期走宿主机代理；含 pi / pi-acp）
#
# 构建（代理走 podman VM 看到的宿主机网关 172.17.0.1:10808）：
#   podman build -t ontok:dev \
#     --build-arg HTTP_PROXY=http://172.17.0.1:10808 \
#     --build-arg HTTPS_PROXY=http://172.17.0.1:10808 \
#     -f Dockerfile .
#
# 为什么这样做：
#   · 基础镜像用**官方 alpine:3.20**（本机已有，不再拉大镜像）
#   · 3.20 自带 rust 1.78 < MSRV 1.86 ⇒ 用 rustup 装官方 1.86 工具链
#   · 所有下载（apk / rustup / crates / bun）都经代理；apk 与 crates 另有国内镜像兜底
ARG BASE=alpine:3.20
ARG APK_MIRROR=mirrors.aliyun.com

# ── 构建阶段 ─────────────────────────────────────────────
FROM ${BASE} AS build
ARG APK_MIRROR
ARG HTTP_PROXY=""
ARG HTTPS_PROXY=""
ARG NO_PROXY="localhost,127.0.0.1"
ENV HTTP_PROXY=${HTTP_PROXY} \
    HTTPS_PROXY=${HTTPS_PROXY} \
    http_proxy=${HTTP_PROXY} \
    https_proxy=${HTTPS_PROXY} \
    NO_PROXY=${NO_PROXY} \
    no_proxy=${NO_PROXY}

RUN sed -i "s|dl-cdn.alpinelinux.org|${APK_MIRROR}|g" /etc/apk/repositories \
 && apk add --no-cache build-base curl

ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    RUSTUP_UPDATE_ROOT=https://rsproxy.cn/rustup \
    RUSTUP_DIST_SERVER=https://rsproxy.cn \
    PATH=/usr/local/cargo/bin:$PATH

# 官方 rustup（走代理）
RUN curl -sSf https://rsproxy.cn/rustup-init.sh -o /tmp/rustup.sh \
 && sh /tmp/rustup.sh -y --no-modify-path --profile minimal --default-toolchain 1.96.0 \
 && rm -f /tmp/rustup.sh \
 && rustc --version && cargo --version

# crates 走阿里云镜像（index + dl）
RUN printf '[source.crates-io]\nreplace-with = "aliyun"\n\n[source.aliyun]\nregistry = "sparse+https://mirrors.aliyun.com/crates.io-index/"\n' > $CARGO_HOME/config.toml

WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
# ⚠️ 编译期 include_str! 引用的**仓库根**文件（不在 src/ 下）必须显式 COPY，
#    否则报 `couldn't read src/.../../../xxx.json`。
#    当前两处：src/sparql/policy.rs → query-policy.example.json
#              src/domain/relation_verbs.rs → relation-verbs.json（REQ-157）
#    ⚠️ 这条注释**曾经预言过一次真实故障**（REQ-157）：漏 COPY ⇒ 镜像构建
#    `couldn't read ../../relation-verbs.json`，而**本地 cargo build 是绿的**。
#    守卫：tests/test_dockerfile_covers_include_str.rs（自动对拍，别再靠记性）。
COPY query-policy.example.json ./
COPY relation-verbs.json ./
RUN cargo build --release --bins

# ── 运行阶段 ─────────────────────────────────────────────
FROM ${BASE}
ARG APK_MIRROR
ARG HTTP_PROXY=""
ARG HTTPS_PROXY=""

RUN sed -i "s|dl-cdn.alpinelinux.org|${APK_MIRROR}|g" /etc/apk/repositories \
 && apk add --no-cache ca-certificates tzdata bash curl unzip libstdc++ libgcc \
 && adduser -D -u 10001 -h /data ontok

# ⚠️ `libstdc++` / `libgcc` 是 **bun 的硬依赖**（alpine 默认不带 C++ 运行库）：
#    缺了会报 `Error loading shared library libstdc++.so.6` +
#    `_ZSt20__throw_length_errorPKc: symbol not found` 一串。
#    这是 alpine 上跑 bun 的经典坑，别删这两行。

# bun（装包用；在 build 期用代理，运行期不需要代理）
ENV BUN_INSTALL=/usr/local
RUN HTTPS_PROXY=${HTTPS_PROXY} HTTP_PROXY=${HTTP_PROXY} sh -c 'curl -fsSL https://bun.sh/install | bash' \
 && ln -sf /usr/local/bin/bun /usr/local/bin/bunx \
 && /usr/local/bin/bun --version

RUN HTTPS_PROXY=${HTTPS_PROXY} HTTP_PROXY=${HTTP_PROXY} bun add -g @earendil-works/pi-coding-agent@1.4.0 pi-acp@0.0.34 \
 && ls -l /usr/local/bin | grep -E 'pi$|pi-acp' || true

# ⚠️ pi / pi-acp 的 bin 是 `#!/usr/bin/env node`，而本镜像**只装 bun 不装 node**。
#    直接用会报 `env: can't execute 'node': No such file or directory`。
#    实测 `bun <cli.js> --version` 能正常跑（pi 1.4.0）⇒ 把 shebang 指向 bun，省掉 node。
#    若将来某个子命令确实依赖 node（例如 pi 去 spawn `node` 子进程），再 `apk add nodejs`。
RUN sed -i '1s|^#!/usr/bin/env node\r\?$|#!/usr/local/bin/bun|' \
        /usr/local/install/global/node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js \
        /usr/local/install/global/node_modules/pi-acp/dist/index.js \
 && head -1 /usr/local/install/global/node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js \
 && pi --version

# ⭐ REQ-152 · **镜像里只有一个 ontok**（服务面）。
#
# ⛔ 不再 COPY `ontok-introspect` / `ontok-propose` / `ontok-translate-names`：
#    它们是 FDE 本机的活，已经并进**构建面** `ontok-builder`（**不进镜像**）。
#    留着它们的下一步就是「客户镜像里带着勘查/提案工具 + 执行器」，那正是
#    REQ-152 要消灭的形状。判据：`podman run --rm <img> ls /usr/local/bin` ⇒ 只有 ontok。
COPY --from=build /src/target/release/ontok /usr/local/bin/ontok

USER ontok
WORKDIR /data
ENV TZ=Asia/Shanghai

# 运行时给 pi 注入配置与凭据（⛔ 不进镜像）：
#   -v ~/.pi:/data/.pi  -e CPA_API_KEY=...

ENTRYPOINT ["ontok"]
CMD ["--help"]

EXPOSE 8800 8899 8900

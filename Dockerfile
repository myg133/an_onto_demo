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
RUN cargo build --release --bins

# ── 运行阶段 ─────────────────────────────────────────────
FROM ${BASE}
ARG APK_MIRROR
ARG HTTP_PROXY=""
ARG HTTPS_PROXY=""

RUN sed -i "s|dl-cdn.alpinelinux.org|${APK_MIRROR}|g" /etc/apk/repositories \
 && apk add --no-cache ca-certificates tzdata bash curl unzip \
 && adduser -D -u 10001 -h /data ontok

# bun（装包用；在 build 期用代理，运行期不需要代理）
ENV BUN_INSTALL=/usr/local
RUN HTTPS_PROXY=${HTTPS_PROXY} HTTP_PROXY=${HTTP_PROXY} sh -c 'curl -fsSL https://bun.sh/install | bash' \
 && ln -sf /usr/local/bin/bun /usr/local/bin/bunx \
 && /usr/local/bin/bun --version

RUN HTTPS_PROXY=${HTTPS_PROXY} HTTP_PROXY=${HTTP_PROXY} bun add -g @earendil-works/pi-coding-agent@1.4.0 pi-acp@0.0.34 \
 && ls -l /usr/local/bin | grep -E 'pi$|pi-acp' || true

COPY --from=build /src/target/release/ontok                 /usr/local/bin/ontok
COPY --from=build /src/target/release/ontok-introspect      /usr/local/bin/ontok-introspect
COPY --from=build /src/target/release/ontok-propose         /usr/local/bin/ontok-propose
COPY --from=build /src/target/release/ontok-translate-names /usr/local/bin/ontok-translate-names

USER ontok
WORKDIR /data
ENV TZ=Asia/Shanghai

# 运行时给 pi 注入配置与凭据（⛔ 不进镜像）：
#   -v ~/.pi:/data/.pi  -e CPA_API_KEY=...

ENTRYPOINT ["ontok"]
CMD ["--help"]

EXPOSE 8800 8899 8900

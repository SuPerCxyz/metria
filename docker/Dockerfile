# syntax=docker/dockerfile:1
#
# Metria 多阶段构建
#   1. web    : Node 构建前端（仅构建期使用 Node，运行时无 Node.js）
#   2. build  : Rust 构建 Hub 与三种原生 Agent 二进制（web/dist 经 rust-embed 嵌入）。
#               build 阶段固定运行在 BUILDPLATFORM（amd64 原生，无 QEMU），
#               使用 gcc-aarch64-linux-gnu / MinGW 交叉编译非当前目标资产。
#   3. hub    : 运行时镜像（TARGETPLATFORM，非 root，无 Node.js）
#
# 构建：
#   docker build -f docker/Dockerfile --target hub -t metria:dev .
#   docker buildx build --platform linux/amd64,linux/arm64 --target hub -t ghcr.io/supercxyz/metria:0.1.0 --push .

# ---- Web build（平台无关，原生构建） ----
FROM --platform=$BUILDPLATFORM node:24-alpine AS web
WORKDIR /build
COPY web/package.json web/package-lock.json ./
RUN --mount=type=cache,target=/root/.npm npm ci
COPY web/ ./
RUN npm run build

# ---- Rust build（原生运行，交叉编译目标架构） ----
FROM --platform=$BUILDPLATFORM rust:1.96-slim AS build
WORKDIR /src
ARG TARGETARCH
# 构建 profile：本地开发默认 release 也缓存复用；需要快速迭代时
#   docker build --build-arg PROFILE=dev ...
# 即用 dev profile（无 LTO/低优化），显著加快编译；正式发布保持 release 全量 LTO。
ARG PROFILE=release
RUN apt-get update \
    && apt-get install -y --no-install-recommends build-essential pkg-config ca-certificates \
       gcc-aarch64-linux-gnu libc6-dev-arm64-cross gcc-mingw-w64-x86-64 \
    && rustup target add aarch64-unknown-linux-gnu x86_64-pc-windows-gnu \
    && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY migrations ./migrations
COPY web/chart-theme.json ./web/chart-theme.json
COPY --from=web /build/dist ./web/dist
# cargo 缓存挂载：registry（依赖只下/编一次）+ target（增量编译，源码变更不触发全量）。
# cache id 按 TARGETARCH 区分：buildx 多平台并行时两个平台的 cargo 会同时
# 解包 registry 源码，共享同一 cache 产生 File exists 竞态导致随机失败。
# target 通过 CARGO_TARGET_DIR 指向缓存挂载，避免其内容进入层快照导致 COPY 拿不到二进制；
# 最终产物显式拷贝到层内 /src/target/release/metria 与 /src/agent-binaries，供 hub 阶段 COPY。
RUN --mount=type=cache,id=cargo-registry-${TARGETARCH},target=/usr/local/cargo/registry \
    --mount=type=cache,id=cargo-target-${TARGETARCH},target=/cargo-target \
    profile="$PROFILE"; target_dir=release; [ "$profile" = "dev" ] && target_dir=debug; \
    mkdir -p target/release agent-binaries; \
    if [ "$TARGETARCH" = "arm64" ]; then \
      CARGO_TARGET_DIR=/cargo-target \
      CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
      CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc \
      cargo build --profile "$profile" --workspace --bin metria --target aarch64-unknown-linux-gnu \
      && cp "/cargo-target/aarch64-unknown-linux-gnu/$target_dir/metria" target/release/metria \
      && cp "/cargo-target/aarch64-unknown-linux-gnu/$target_dir/metria" agent-binaries/metria-linux-arm64; \
    else \
      CARGO_TARGET_DIR=/cargo-target \
      cargo build --profile "$profile" --workspace --bin metria \
      && cp "/cargo-target/$target_dir/metria" target/release/metria \
      && cp "/cargo-target/$target_dir/metria" agent-binaries/metria-linux-amd64; \
    fi; \
    if [ "$TARGETARCH" = "arm64" ]; then \
      CARGO_TARGET_DIR=/cargo-target \
      cargo build --profile "$profile" --workspace --bin metria \
      && cp "/cargo-target/$target_dir/metria" agent-binaries/metria-linux-amd64; \
    else \
      CARGO_TARGET_DIR=/cargo-target \
      CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
      CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc \
      cargo build --profile "$profile" --workspace --bin metria --target aarch64-unknown-linux-gnu \
      && cp "/cargo-target/aarch64-unknown-linux-gnu/$target_dir/metria" agent-binaries/metria-linux-arm64; \
    fi; \
    CARGO_TARGET_DIR=/cargo-target \
    CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc \
    CC_x86_64_pc_windows_gnu=x86_64-w64-mingw32-gcc \
    cargo build --profile "$profile" --workspace --bin metria --target x86_64-pc-windows-gnu; \
    cp "/cargo-target/x86_64-pc-windows-gnu/$target_dir/metria.exe" agent-binaries/metria-windows-amd64.exe; \
    test -s target/release/metria \
      && test -s agent-binaries/metria-linux-amd64 \
      && test -s agent-binaries/metria-linux-arm64 \
      && test -s agent-binaries/metria-windows-amd64.exe

# ---- Runtime（目标平台） ----
FROM debian:bookworm-slim AS hub
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates tzdata \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 65532 --home-dir /home/metria --create-home metria \
    && mkdir -p /data \
    && chown -R metria:metria /home/metria \
    && chmod 0777 /data
COPY --from=build /src/target/release/metria /app/metria
COPY --from=build /src/agent-binaries /app/agent-binaries
ENV METRIA_AGENT_BINARIES_DIR=/app/agent-binaries
VOLUME /data
WORKDIR /app
USER metria
ENTRYPOINT ["/app/metria"]
CMD ["healthcheck"]

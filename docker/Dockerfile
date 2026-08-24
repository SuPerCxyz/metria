# syntax=docker/dockerfile:1
#
# Metria 多阶段构建
#   1. web    : Node 构建前端（仅构建期使用 Node，运行时无 Node.js）
#   2. build  : Rust 构建 metria 单二进制（web/dist 经 rust-embed 嵌入）。
#               build 阶段固定运行在 BUILDPLATFORM（amd64 原生，无 QEMU），
#               对 arm64 使用 gcc-aarch64-linux-gnu 交叉编译，显著加速多架构构建。
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
    && if [ "$TARGETARCH" = "arm64" ]; then \
         apt-get install -y --no-install-recommends gcc-aarch64-linux-gnu libc6-dev-arm64-cross \
         && rustup target add aarch64-unknown-linux-gnu; \
       fi \
    && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY migrations ./migrations
COPY --from=web /build/dist ./web/dist
# cargo 缓存挂载：registry（依赖只下/编一次）+ target（增量编译，源码变更不触发全量）。
# target 通过 CARGO_TARGET_DIR 指向缓存挂载，避免其内容进入层快照导致 COPY 拿不到二进制；
# 最终产物显式拷贝到层内 /src/target/release/metria，供 hub 阶段 COPY。
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/cargo-target \
    profile="$PROFILE"; target_dir=release; [ "$profile" = "dev" ] && target_dir=debug; \
    if [ "$TARGETARCH" = "arm64" ]; then \
      CARGO_TARGET_DIR=/cargo-target \
      CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
      CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc \
      cargo build --profile "$profile" --workspace --bin metria --target aarch64-unknown-linux-gnu \
      && mkdir -p target/release \
      && cp "/cargo-target/aarch64-unknown-linux-gnu/$target_dir/metria" target/release/metria; \
    else \
      CARGO_TARGET_DIR=/cargo-target \
      cargo build --profile "$profile" --workspace --bin metria \
      && mkdir -p target/release \
      && cp "/cargo-target/$target_dir/metria" target/release/metria; \
    fi

# ---- Runtime（目标平台） ----
FROM debian:bookworm-slim AS hub
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates tzdata \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 65532 --home-dir /home/metria --create-home metria \
    && mkdir -p /data \
    && chown -R metria:metria /data /home/metria
COPY --from=build /src/target/release/metria /app/metria
VOLUME /data
WORKDIR /app
USER metria
ENTRYPOINT ["/app/metria"]
CMD ["healthcheck"]

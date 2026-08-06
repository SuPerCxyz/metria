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
RUN npm ci
COPY web/ ./
RUN npm run build

# ---- Rust build（原生运行，交叉编译目标架构） ----
FROM --platform=$BUILDPLATFORM rust:1.96-slim AS build
WORKDIR /src
ARG TARGETARCH
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
RUN if [ "$TARGETARCH" = "arm64" ]; then \
      CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
      CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc \
      cargo build --release --workspace --bin metria --target aarch64-unknown-linux-gnu \
      && mkdir -p target/release \
      && cp target/aarch64-unknown-linux-gnu/release/metria target/release/metria; \
    else \
      cargo build --release --workspace --bin metria; \
    fi

# ---- Runtime（目标平台） ----
FROM --platform=$TARGETPLATFORM debian:bookworm-slim AS hub
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

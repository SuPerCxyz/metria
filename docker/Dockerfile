# syntax=docker/dockerfile:1
#
# Metria 多阶段构建
#   1. web    : Node 构建前端（仅构建期使用 Node，运行时无 Node.js）
#   2. build  : Rust 构建 metria 单二进制（web/dist 经 rust-embed 嵌入）
#   3. hub    : 运行时镜像（非 root，无 Node.js）
#
# 构建：
#   docker build -f docker/Dockerfile --target hub -t metria:dev .
#   docker buildx build --platform linux/amd64,linux/arm64 --target hub -t ghcr.io/supercxyz/metria:0.1.0 .

# ---- Web build ----
FROM node:24-alpine AS web
WORKDIR /build
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

# ---- Rust build ----
FROM rust:1.96-slim AS build
WORKDIR /src
RUN apt-get update \
    && apt-get install -y --no-install-recommends build-essential pkg-config ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY migrations ./migrations
COPY --from=web /build/dist ./web/dist
ARG TARGETARCH
RUN cargo build --release --workspace --bin metria

# ---- Runtime ----
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

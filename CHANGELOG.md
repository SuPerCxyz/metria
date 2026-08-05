# Changelog

本项目遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/) 语义版本规范（[SemVer](https://semver.org/lang/zh-CN/)）。

## [Unreleased]

## [0.1.0] - 2026-08-05

### Added

- Rust workspace：12 个 crate（core/protocol/storage/pricing/traffic/adapter-api/三个 adapter/agent/hub/cli）。
- `metria` CLI 骨架：hub/agent/import/doctor/config/export/backup/restore/mcp/healthcheck/version 子命令。
- metria-core：配置（ContentMode/timezone/env 解析）、分层错误类型、tracing 日志初始化。
- metria-storage：SQLite 打开与 PRAGMA（WAL/foreign_keys/busy_timeout）、版本化迁移框架（rust-embed 嵌入 `migrations/`）、Repository 抽象。
- metria-hub：axum 服务骨架（healthz + 前端静态资源 rust-embed + SPA fallback + 优雅退出）、迁移应用、容器 healthcheck。
- Web：Preact + TypeScript + Vite 骨架，light/dark 主题 CSS Variables，PWA manifest。
- Docker：多阶段构建（Node 构建期 / Rust 构建期 / 非 root 运行时，运行时无 Node.js）。
- Docker Compose：hub 单服务、agent 单服务、hub+agent+demo 完整示例。
- 质量门禁脚本 `scripts/check.sh`。

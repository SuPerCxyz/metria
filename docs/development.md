# 开发指南

## 1. 环境要求

- Rust stable（workspace 固定 `rust-toolchain.toml`）
- Node.js + npm（仅 Web 构建期需要；运行时镜像不含 Node）
- Docker（构建镜像 / compose）

## 2. Workspace 结构

```
crates/
├── metria-core         领域模型·ID·归一化·脱敏·时间·金额·内容分类
├── metria-protocol     Agent↔Hub 线协议·序列化·校验·限长
├── metria-storage      SQLite 连接/迁移/Repository 抽象
├── metria-pricing      价格引擎（内置目录+用户规则+快照+match+reprice）
├── metria-traffic      流量估算（字节统计·重建·token profile·置信区间）
├── metria-adapter-api  SourceAdapter trait·类型·错误·testutil
├── metria-adapter-claude / codex / opencode
├── metria-agent         采集器（blocking 栈）
├── metria-hub           Hub（api/db/rollup/catalog/demo/share/export）
└── metria-cli           CLI（hub/agent/import/doctor/config/export/backup/restore/mcp/healthcheck/version）

web/                      Preact+TS+Vite+uPlot（dist 由 hub embed）
migrations/               SQLite 版本化 migration
fixtures/                 claude/ codex/ opencode/ malformed/ traffic/
docker/                   Dockerfile + compose.*.yaml + .env.example
docs/                     architecture/data-model/adapters/api/deployment/privacy/operations
```

依赖方向：`core ← protocol ← {traffic,pricing} ← storage ← adapter-api ← adapters ← agent/hub ← cli`，禁止反向/循环。

## 3. 本地开发

```bash
# Rust
cargo build --workspace
cargo test --workspace
cargo run -p metria-cli -- hub --demo        # 演示 Hub（合成数据）

# Web（需 Hub 运行在 8080）
cd web && npm install
npm run dev                                    # Vite dev server
npm run typecheck && npm run build

# 前端单测
npm test                                       # vitest run
```

## 4. 质量门禁（提交前必须全绿）

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace
cd web && npm run typecheck && npm run build && npm test
docker build -f docker/Dockerfile --target hub -t metria:dev .
docker compose -f docker/compose.full.yaml config
```

`scripts/check.sh` 串联上述门禁。

## 5. 数据库迁移

1. 新建 `migrations/N_name.sql`（N 递增）。
2. 迁移事务执行并记录 `schema_migrations`；禁止直接改表。
3. 涉及行为变更的迁移同时更新 `docs/data-model.md` 与 ops 升级/回滚说明。

## 6. 测试策略

- 单元测试：core 全模型、traffic 估算、pricing 引擎、spool、rollup。
- Adapter：golden + malformed fixture 全覆盖（详见 `docs/adapters.md`）。
- 集成（e2e）：真实 HTTP 全链路——注册→上传（含 zstd bomb/深度/单事件/协议版本校验）→
  幂等→部分成功→rollup→查询；token 过期与续期；时钟偏移。
- Web：Vitest（format / i18n / range / api 序列化）。
- 基准：`crates/metria-hub/tests/bench.rs`（10 万/100 万事件）。

## 7. 提交规范

- subject ≤50 字符、祈使句、无 `feat:` 前缀；正文每行 ≤72 字符。
- 每次提交前 `git status` / `git diff`，只暂存本任务相关文件。
- 数据库/API 行为变化在提交信息中说明影响。
- 不手写 Change-Id（由 commit-msg hook 生成）。

## 8. 已知限制

- Codex 会话级 model 聚合以 message/agent 为粒度。
- M1 的 `rebuild_range`（rollup 全量重建 API）为占位，对账修复走 `rebuild_drift`（最近 N 天）。
- 单 Admin、无 i18n 切换持久化之外的复杂国际化需求（Web 已有 zh/en 基础抽象）。

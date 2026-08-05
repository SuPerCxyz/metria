#!/usr/bin/env bash
# Metria 质量门禁：所有阶段必须全绿。
# 用法：scripts/check.sh [--skip-docker] [--skip-web]
set -euo pipefail

cd "$(dirname "$0")/.."

SKIP_DOCKER=0
SKIP_WEB=0
for arg in "$@"; do
  case "$arg" in
    --skip-docker) SKIP_DOCKER=1 ;;
    --skip-web) SKIP_WEB=1 ;;
    *) echo "未知参数: $arg" >&2; exit 2 ;;
  esac
done

step() { printf '\n\033[1;36m== %s ==\033[0m\n' "$1"; }

step "cargo fmt"
cargo fmt --all -- --check

step "cargo clippy"
cargo clippy --all-targets --all-features -- -D warnings

step "cargo test"
cargo test --workspace

if [ "$SKIP_WEB" -eq 0 ]; then
  step "web typecheck + build"
  (cd web && npm run typecheck && npm run build)
fi

if [ "$SKIP_DOCKER" -eq 0 ]; then
  step "docker build (hub target)"
  docker build -f docker/Dockerfile --target hub -t metria:dev .

  step "docker compose config"
  docker compose -f docker/compose.yaml config --quiet
  docker compose -f docker/compose.agent.yaml config --quiet
  docker compose -f docker/compose.full.yaml config --quiet
fi

echo
echo "✔ 全部质量门禁通过"

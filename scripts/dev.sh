#!/usr/bin/env bash
# Runs every service (and the web app) on the host with live reload-free
# cargo builds. Ctrl-C stops all of them.
set -euo pipefail
cd "$(dirname "$0")/.."

export DATABASE_URL="${DATABASE_URL:-postgres://sdi:sdi@127.0.0.1:5440/sdi}"
export RUST_LOG="${RUST_LOG:-info,sqlx=warn,tower_http=info}"

if [[ -z "${GITHUB_TOKEN:-}" ]] && command -v gh >/dev/null && gh auth status >/dev/null 2>&1; then
  export GITHUB_TOKEN="$(gh auth token)"
  echo "GITHUB_TOKEN taken from gh auth"
fi

if [[ -f .env ]]; then
  set -a; source .env; set +a
fi

cargo build --workspace

pids=()
cleanup() { kill "${pids[@]}" 2>/dev/null || true; wait 2>/dev/null || true; }
trap cleanup EXIT INT TERM

for svc in knowledge intelligence execution gateway; do
  ./target/debug/sdi-$svc &
  pids+=($!)
  sleep 0.3
done

if [[ "${1:-}" != "--no-web" ]]; then
  (cd web && pnpm dev) &
  pids+=($!)
fi

echo
echo "  gateway   http://127.0.0.1:8080/api/docs"
echo "  web       http://127.0.0.1:5173"
echo
wait

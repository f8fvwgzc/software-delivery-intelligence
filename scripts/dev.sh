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

# Workdirs of the docker adapter. Absolute, because the adapter hands paths
# under it to the Docker daemon, which resolves them on the host, not
# relative to this shell. DOCKER_HOST is left alone: the CLI default applies.
export SDI_RUN_DIR="${SDI_RUN_DIR:-$PWD/.run}"
mkdir -p "$SDI_RUN_DIR"

# Refuse to start on top of a previous stack: a half-bound set of services is
# worse than a clear message.
for port in 8080 5173 50051 50052 50053; do
  if lsof -nP -iTCP:"$port" -sTCP:LISTEN >/dev/null 2>&1; then
    echo "port $port is already in use (an earlier stack is still running)." >&2
    echo "stop it with: pkill -f scripts/dev.sh; pkill -f target/debug/sdi-; pkill -f vite" >&2
    exit 1
  fi
done

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

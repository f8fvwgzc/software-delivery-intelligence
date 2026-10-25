#!/usr/bin/env bash
# Runs every service (and the web app) on the host with live reload-free
# cargo builds. Ctrl-C stops all of them.
#
#   ./scripts/dev.sh             build, then run the services and the web app
#   ./scripts/dev.sh --no-web    services only
#   ./scripts/dev.sh --no-build  run the binaries already in target/debug
set -euo pipefail
cd "$(dirname "$0")/.."

with_web=1
with_build=1
for arg in "$@"; do
  case "$arg" in
    --no-web) with_web=0 ;;
    --no-build) with_build=0 ;;
    *) echo "unknown option: $arg (known: --no-web, --no-build)" >&2; exit 2 ;;
  esac
done

# Missing tools, a signed-out gh or a half-configured model are reported
# here, with the command that fixes each, instead of as a build error later.
./scripts/doctor.sh --quiet

export DATABASE_URL="${DATABASE_URL:-postgres://sdi:sdi@127.0.0.1:5440/sdi}"
export RUST_LOG="${RUST_LOG:-info,sqlx=warn,tower_http=info}"

if [[ -f .env ]]; then
  set -a; source .env; set +a
fi

# No token in the environment or in .env: use the gh login. For GitHub
# Enterprise Server (GITHUB_API_URL=https://<host>/api/v3) that is the login
# for that host. The token is exported to the services and never printed.
if [[ -z "${GITHUB_TOKEN:-}" ]] && command -v gh >/dev/null; then
  gh_host=""
  if [[ -n "${GITHUB_API_URL:-}" ]]; then
    gh_host="$(printf '%s' "$GITHUB_API_URL" | sed -E 's#^[a-z]+://([^/:]+).*#\1#')"
    [[ "$gh_host" == "api.github.com" ]] && gh_host=""
  fi
  if token="$(gh auth token ${gh_host:+--hostname "$gh_host"} 2>/dev/null)" && [[ -n "$token" ]]; then
    export GITHUB_TOKEN="$token"
    echo "GITHUB_TOKEN taken from gh auth${gh_host:+ ($gh_host)}"
  fi
  unset token gh_host
fi

# Identity (CONTRACT.md §18). Without a registered GitHub OAuth app,
# SDI_DEV_LOGIN=1 makes GET /auth/login create a local session for the
# GITHUB_TOKEN account with a personal workspace. The session secret signs
# cookies and encrypts stored tokens; it is generated once and kept in
# .run/session-secret (git-ignored) so sessions survive restarts.
export SDI_DEV_LOGIN="${SDI_DEV_LOGIN:-1}"
export SDI_PUBLIC_URL="${SDI_PUBLIC_URL:-http://localhost:5173}"
if [[ -z "${SDI_SESSION_SECRET:-}" ]]; then
  secret_file=.run/session-secret
  if [[ ! -s "$secret_file" ]]; then
    mkdir -p .run
    umask_prev=$(umask); umask 077
    if command -v openssl >/dev/null; then
      openssl rand -hex 32 > "$secret_file"
    else
      head -c 32 /dev/urandom | od -An -tx1 | tr -d ' \n' > "$secret_file"
      echo >> "$secret_file"
    fi
    umask "$umask_prev"
    echo "SDI_SESSION_SECRET generated into $secret_file"
  fi
  export SDI_SESSION_SECRET="$(tr -d '\n' < "$secret_file")"
fi

# Workdirs of the docker adapter. Absolute, because the adapter hands paths
# under it to the Docker daemon, which resolves them on the host, not
# relative to this shell. DOCKER_HOST is left alone: the CLI default applies.
export SDI_RUN_DIR="${SDI_RUN_DIR:-$PWD/.run}"
mkdir -p "$SDI_RUN_DIR"

# Refuse to start on top of a previous stack: a half-bound set of services is
# worse than a clear message.
port_in_use() {
  if command -v lsof >/dev/null 2>&1; then
    lsof -nP -iTCP:"$1" -sTCP:LISTEN >/dev/null 2>&1
  elif command -v ss >/dev/null 2>&1; then
    ss -ltn 2>/dev/null | awk '{print $4}' | grep -q ":$1\$"
  else
    return 1
  fi
}
for port in 8080 5173 50051 50052 50053 50054 50055; do
  if port_in_use "$port"; then
    echo "port $port is already in use (an earlier stack is still running)." >&2
    echo "stop it with: pkill -f scripts/dev.sh; pkill -f target/debug/sdi-; pkill -f vite" >&2
    exit 1
  fi
done

# A fresh clone has no web dependencies yet; install them once, from the lockfile.
if [[ $with_web -eq 1 && ! -d web/node_modules ]]; then
  (cd web && pnpm install --frozen-lockfile)
fi

# The GitOps engine (CONTRACT.md §24) is Go; GOTOOLCHAIN=auto fetches the
# toolchain its go.mod asks for. A local cluster's API server is on a
# private address, which deploy targets may only use when this is set.
export SDI_ALLOW_PRIVATE_DEPLOY_TARGETS="${SDI_ALLOW_PRIVATE_DEPLOY_TARGETS:-1}"
export GITOPS_GRPC_URL="${GITOPS_GRPC_URL:-http://127.0.0.1:50055}"

if [[ $with_build -eq 1 ]]; then
  cargo build --workspace
  (cd services/gitops && GOTOOLCHAIN=auto go build -o ../../target/debug/sdi-gitops ./cmd/sdi-gitops)
else
  for svc in identity knowledge intelligence execution gitops gateway; do
    if [[ ! -x target/debug/sdi-$svc ]]; then
      echo "target/debug/sdi-$svc is missing: run without --no-build once." >&2
      exit 1
    fi
  done
fi

pids=()
cleanup() { kill "${pids[@]}" 2>/dev/null || true; wait 2>/dev/null || true; }
trap cleanup EXIT INT TERM

for svc in identity knowledge intelligence execution gitops gateway; do
  ./target/debug/sdi-$svc &
  pids+=($!)
  sleep 0.3
done

if [[ $with_web -eq 1 ]]; then
  (cd web && pnpm dev) &
  pids+=($!)
fi

echo
echo "  gateway   http://127.0.0.1:8080/api/docs"
echo "  web       http://localhost:5173"
echo "  sign-in   dev login (SDI_DEV_LOGIN=$SDI_DEV_LOGIN) as the GITHUB_TOKEN account"
echo
wait

#!/usr/bin/env bash
# Checks that this machine can run SDI (`make dev`) and says exactly what to
# do about anything that is missing. Read-only: it installs and changes
# nothing, and it never prints a token or a key.
#
#   ./scripts/doctor.sh          every check
#   ./scripts/doctor.sh --quiet  only what needs attention (used by dev.sh)
set -uo pipefail
cd "$(dirname "$0")/.."

quiet=0
[[ "${1:-}" == "--quiet" ]] && quiet=1

failures=0
warnings=0

ok()   { [[ $quiet -eq 1 ]] || printf '  ok    %s\n' "$1"; }
warn() { warnings=$((warnings + 1)); printf '  warn  %s\n' "$1"; [[ -n "${2:-}" ]] && printf '        %s\n' "$2"; }
fail() { failures=$((failures + 1)); printf '  FAIL  %s\n' "$1"; [[ -n "${2:-}" ]] && printf '        %s\n' "$2"; }

# 1.88.0 -> 1088000, so versions compare as numbers.
version_number() {
  local IFS=.
  # shellcheck disable=SC2206
  local parts=($1)
  printf '%d%03d%03d' "${parts[0]:-0}" "${parts[1]:-0}" "${parts[2]:-0}"
}
at_least() { [[ "$(version_number "$1")" -ge "$(version_number "$2")" ]]; }

# A value from .env without sourcing the file (so nothing in it runs here).
env_value() {
  [[ -f .env ]] || return 0
  sed -n "s/^[[:space:]]*$1[[:space:]]*=[[:space:]]*//p" .env | tail -n 1 | sed -e 's/^"\(.*\)"$/\1/' -e "s/^'\(.*\)'$/\1/"
}
setting() {
  local from_env="${!1:-}"
  if [[ -n "$from_env" ]]; then printf '%s' "$from_env"; else env_value "$1"; fi
}

[[ $quiet -eq 1 ]] || echo "Tools"

case "$(uname -s)" in
  Darwin|Linux) ok "$(uname -s) $(uname -m)" ;;
  *) fail "unsupported shell environment: $(uname -s)" "Use macOS, Linux, or Windows through WSL2." ;;
esac

if command -v docker >/dev/null 2>&1; then
  if docker info >/dev/null 2>&1; then
    ok "docker is running"
    if docker compose version >/dev/null 2>&1; then ok "docker compose"; else
      fail "docker compose is missing" "Install the Compose plugin (it ships with Docker Desktop)."
    fi
  else
    fail "docker is installed but the daemon is not running" "Start Docker Desktop (or 'sudo systemctl start docker'), then run this again."
  fi
else
  fail "docker is not installed" "Postgres runs in Docker. Install Docker Desktop: https://docs.docker.com/get-docker/"
fi

if command -v cargo >/dev/null 2>&1 && command -v rustc >/dev/null 2>&1; then
  rust="$(rustc --version | awk '{print $2}' | sed 's/-.*//')"
  if at_least "$rust" "1.88.0"; then ok "rust $rust"; else
    fail "rust $rust is too old (1.88 or newer is needed)" "rustup update stable"
  fi
else
  fail "rust is not installed" "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
fi

if command -v protoc >/dev/null 2>&1; then
  ok "protoc $(protoc --version | awk '{print $2}')"
else
  case "$(uname -s)" in
    Darwin) hint="brew install protobuf" ;;
    *) hint="sudo apt-get install -y protobuf-compiler libprotobuf-dev" ;;
  esac
  fail "protoc is not installed (the gRPC contracts are compiled at build time)" "$hint"
fi

if command -v go >/dev/null 2>&1; then
  go_version="$(go env GOVERSION 2>/dev/null | sed 's/^go//')"
  if at_least "${go_version:-0}" "1.21.0"; then
    ok "go $go_version (the GitOps engine's newer toolchain is fetched on first build)"
  else
    fail "go ${go_version:-?} is too old (1.21 or newer can fetch the toolchain the GitOps engine needs)" "Install Go: https://go.dev/dl/   (or: brew install go)"
  fi
else
  fail "go is not installed (the GitOps engine is a Go service)" "Install Go: https://go.dev/dl/   (or: brew install go)"
fi

if command -v node >/dev/null 2>&1; then
  node_version="$(node --version | sed 's/^v//')"
  if at_least "$node_version" "22.12.0" || { at_least "$node_version" "20.19.0" && ! at_least "$node_version" "21.0.0"; }; then
    ok "node $node_version"
  else
    fail "node $node_version is too old for the web app (20.19+ or 22.12+)" "Install Node 22 LTS: https://nodejs.org/ (or 'nvm install 22')"
  fi
else
  fail "node is not installed" "Install Node 22 LTS: https://nodejs.org/ (or 'nvm install 22')"
fi

if command -v pnpm >/dev/null 2>&1; then
  ok "pnpm $(pnpm --version)"
else
  fail "pnpm is not installed" "corepack enable && corepack prepare pnpm@latest --activate   (or: npm install -g pnpm)"
fi

[[ $quiet -eq 1 ]] || { echo; echo "GitHub"; }

# GitHub Enterprise Server: GITHUB_API_URL=https://<host>/api/v3 selects the gh login for that host.
gh_host=""
api_url="$(setting GITHUB_API_URL)"
if [[ -n "$api_url" ]]; then
  gh_host="$(printf '%s' "$api_url" | sed -E 's#^[a-z]+://([^/:]+).*#\1#')"
  [[ "$gh_host" == "api.github.com" ]] && gh_host=""
fi
gh_args=()
[[ -n "$gh_host" ]] && gh_args=(--hostname "$gh_host")

if [[ -n "$(setting GITHUB_TOKEN)" ]]; then
  ok "GITHUB_TOKEN is set (its scopes cannot be checked from here)"
elif command -v gh >/dev/null 2>&1; then
  if gh auth token ${gh_args[@]+"${gh_args[@]}"} >/dev/null 2>&1; then
    login="$(gh api ${gh_args[@]+"${gh_args[@]}"} user --jq .login 2>/dev/null || true)"
    ok "gh is signed in${login:+ as $login}${gh_host:+ on $gh_host}"
    # Scopes come from the response headers; the token itself is never read here.
    scopes="$(gh api ${gh_args[@]+"${gh_args[@]}"} -i user 2>/dev/null | tr -d '\r' | sed -n 's/^[Xx]-[Oo][Aa]uth-[Ss]copes:[[:space:]]*//p' | tail -n 1)"
    if [[ -z "$scopes" ]]; then
      ok "fine-grained or app token (scopes are set per repository on GitHub)"
    else
      case ", $scopes," in
        *", repo,"*) ok "scope repo (read and write repositories, pull requests, Dependabot alerts)" ;;
        *) fail "the gh token lacks the 'repo' scope" "gh auth refresh${gh_host:+ --hostname $gh_host} -s repo" ;;
      esac
      case ", $scopes," in
        *", workflow,"*) ok "scope workflow (pull requests that add or change GitHub Actions workflows)" ;;
        *) warn "the gh token lacks the 'workflow' scope: pull requests that touch .github/workflows will be refused" "gh auth refresh${gh_host:+ --hostname $gh_host} -s workflow" ;;
      esac
    fi
  else
    fail "gh is installed but not signed in${gh_host:+ to $gh_host}" "gh auth login${gh_host:+ --hostname $gh_host}"
  fi
else
  fail "no GitHub access: gh is not installed and GITHUB_TOKEN is not set" "Install the GitHub CLI (https://cli.github.com/) and run 'gh auth login', or put GITHUB_TOKEN in .env"
fi

[[ $quiet -eq 1 ]] || { echo; echo "Model (optional)"; }

provider="$(setting LLM_PROVIDER)"
model="$(setting LLM_MODEL)"
[[ -z "$model" ]] && model="$(setting ANTHROPIC_MODEL)"
base_url="$(setting LLM_BASE_URL)"
has_key=0
[[ -n "$(setting LLM_API_KEY)" || -n "$(setting ANTHROPIC_API_KEY)" ]] && has_key=1

if [[ $has_key -eq 0 && -z "$base_url" ]]; then
  ok "no model key set: reviews and proposals use the built-in analyzers only (add LLM_* to .env to change that)"
else
  shown_provider="${provider:-anthropic}"
  case "$shown_provider" in
    anthropic|claude)
      ok "provider anthropic, model ${model:-default}"
      ;;
    *)
      if [[ -z "$(setting LLM_MODEL)" ]]; then
        fail "LLM_PROVIDER=$shown_provider needs LLM_MODEL (there is no default model for this provider)" "Add a line like LLM_MODEL=<a model your key can use> to .env"
      else
        ok "provider $shown_provider, model $model${base_url:+, endpoint set}"
      fi
      [[ $has_key -eq 0 ]] && warn "no LLM_API_KEY set for $shown_provider" "Fine for a local endpoint without auth (Ollama); otherwise add LLM_API_KEY to .env"
      ;;
  esac
fi

[[ $quiet -eq 1 ]] || { echo; echo "Machine"; }

free_kb="$(df -Pk . | awk 'NR==2 {print $4}')"
free_gb=$((free_kb / 1024 / 1024))
if [[ -x target/debug/sdi-gateway ]]; then
  # Built before: a rebuild only rewrites what changed.
  if [[ $free_gb -ge 5 ]]; then ok "${free_gb} GB free"; elif [[ $free_gb -ge 2 ]]; then
    warn "${free_gb} GB free: builds keep adding to target/" "Reclaim space with: rm -rf target/debug/incremental   (it is rebuilt as needed)"
  else
    fail "only ${free_gb} GB free: a build would run out of space" "Reclaim space with: rm -rf target/debug/incremental   (it is rebuilt as needed)"
  fi
elif [[ $free_gb -ge 15 ]]; then ok "${free_gb} GB free"; elif [[ $free_gb -ge 8 ]]; then
  warn "${free_gb} GB free: the first build writes about 12 GB into target/" "Free some space if the build stops with 'No space left on device'."
else
  fail "only ${free_gb} GB free: the first build writes about 12 GB into target/" "Free disk space first."
fi

busy=()
for port in 5440 8080 5173 50051 50052 50053 50054 50055; do
  in_use=1
  if command -v lsof >/dev/null 2>&1; then
    lsof -nP -iTCP:"$port" -sTCP:LISTEN >/dev/null 2>&1 && in_use=0
  elif command -v ss >/dev/null 2>&1; then
    ss -ltn 2>/dev/null | awk '{print $4}' | grep -q ":$port\$" && in_use=0
  fi
  [[ $in_use -eq 0 ]] && busy+=("$port")
done
if [[ ${#busy[@]} -eq 0 ]]; then
  ok "ports 5440, 8080, 5173 and 50051-50055 are free"
else
  # 5440 alone is the dev Postgres already up, which is what `make dev` wants.
  if [[ "${busy[*]}" == "5440" ]]; then ok "the dev database is already running on 5440"; else
    warn "ports in use: ${busy[*]}" "If that is an earlier SDI stack, stop it: pkill -f scripts/dev.sh; pkill -f target/debug/sdi-; pkill -f vite"
  fi
fi

echo
if [[ $failures -gt 0 ]]; then
  if [[ $warnings -gt 0 ]]; then
    echo "$failures thing(s) to fix before 'make dev' can work, and $warnings warning(s)."
  else
    echo "$failures thing(s) to fix before 'make dev' can work."
  fi
  exit 1
fi
if [[ $warnings -gt 0 ]]; then
  echo "Ready, with $warnings warning(s). Start with: make dev"
else
  [[ $quiet -eq 1 ]] || echo "Ready. Start with: make dev"
fi
exit 0

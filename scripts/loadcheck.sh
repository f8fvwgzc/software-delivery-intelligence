#!/usr/bin/env bash
# Load check of the API through whatever answers at the base URL (the nginx
# edge of `make up`, a gateway replica, or a stack started on alternate
# ports). Two modes:
#
#   suite (default)  signs in once (dev login, CONTRACT.md §18), then holds
#                    N SSE connections on /events/stream and hits
#                    /projects, /executions?limit=50, /proposals and
#                    /projects/{id}/tree with C concurrent clients for D
#                    seconds; reports p50/p95/p99, the non-2xx share and
#                    the RSS of every local service before and after.
#   single (-u URL)  the original one-URL check (p50/p95, 429 share).
#
# Uses `oha` or `hey` when installed, otherwise a curl loop fanned out with
# `xargs -P`. Only bash, curl, awk, sort, ps and lsof are needed for the
# fallback, so it runs on a bare macOS or a CI runner.
#
#   scripts/loadcheck.sh [-b BASE] [-c CONCURRENCY] [-d SECONDS] [-s SSE]
#                        [-m MAX_P95_MS] [-e MAX_ERROR_PCT] [-p PROJECT_ID]
#                        [-P "PORT PORT ..."] [-u URL]
#
#   -b  base URL              (default http://localhost)
#   -c  concurrent clients    (default 200 in suite mode, 16 for -u)
#   -d  duration in seconds   (default 60 in suite mode, 30 for -u)
#   -s  SSE connections held  (default 50; 0 skips)
#   -m  fail when any p95 exceeds this many milliseconds (default: report)
#   -e  fail when the non-2xx share exceeds this percent (default 1)
#   -p  project id for /projects/{id}/tree (default: first listed project)
#   -P  listen ports of the local services whose RSS is sampled with ps
#       (default "50051 50052 50053 50054 8080"; empty skips)
#   -u  one URL to hit instead of the suite (no sign-in)
#
# Environment: LOADCHECK_BASE, LOADCHECK_CONCURRENCY, LOADCHECK_DURATION,
# LOADCHECK_SSE, LOADCHECK_MAX_P95_MS, LOADCHECK_MAX_ERROR_PCT,
# LOADCHECK_PROJECT, LOADCHECK_PORTS, LOADCHECK_COOKIE (an sdi_session
# value to use instead of signing in), LOADCHECK_URL (single mode).
set -euo pipefail

base="${LOADCHECK_BASE:-http://localhost}"
url="${LOADCHECK_URL:-}"
concurrency="${LOADCHECK_CONCURRENCY:-}"
duration="${LOADCHECK_DURATION:-}"
sse="${LOADCHECK_SSE:-50}"
max_p95_ms="${LOADCHECK_MAX_P95_MS:-}"
max_error_pct="${LOADCHECK_MAX_ERROR_PCT:-1}"
project="${LOADCHECK_PROJECT:-}"
ports="${LOADCHECK_PORTS-50051 50052 50053 50054 8080}"
cookie="${LOADCHECK_COOKIE:-}"

usage() {
  sed -n '2,38p' "$0" | sed 's/^# \{0,1\}//'
}

while getopts "b:u:c:d:s:m:e:p:P:h" opt; do
  case "$opt" in
    b) base="$OPTARG" ;;
    u) url="$OPTARG" ;;
    c) concurrency="$OPTARG" ;;
    d) duration="$OPTARG" ;;
    s) sse="$OPTARG" ;;
    m) max_p95_ms="$OPTARG" ;;
    e) max_error_pct="$OPTARG" ;;
    p) project="$OPTARG" ;;
    P) ports="$OPTARG" ;;
    h) usage; exit 0 ;;
    *) usage >&2; exit 2 ;;
  esac
done
base="${base%/}"
if [[ -n "$url" ]]; then
  concurrency="${concurrency:-16}"
  duration="${duration:-30}"
else
  concurrency="${concurrency:-200}"
  duration="${duration:-60}"
fi

for n in "$concurrency" "$duration" "$sse"; do
  case "$n" in
    ''|*[!0-9]*) echo "loadcheck: -c, -d and -s take non-negative integers (got '$n')" >&2; exit 2 ;;
  esac
done

if command -v oha >/dev/null 2>&1; then
  tool=oha
elif command -v hey >/dev/null 2>&1; then
  tool=hey
else
  tool=curl
fi

tmp="$(mktemp -d "${TMPDIR:-/tmp}/loadcheck.XXXXXX")"
sse_pids=()
cleanup() {
  if ((${#sse_pids[@]})); then
    kill "${sse_pids[@]}" 2>/dev/null || true
    wait "${sse_pids[@]}" 2>/dev/null || true
  fi
  rm -rf "$tmp"
}
trap cleanup EXIT

# --- helpers -----------------------------------------------------------------

share() {
  # Percentage of $1 over $2, "0.0" when nothing was counted.
  awk -v a="$1" -v b="$2" 'BEGIN { if (b > 0) printf "%.1f", a * 100 / b; else print "0.0" }'
}

# RSS in MB of whatever listens on each port (empty when nothing local does).
rss_snapshot() {
  local port pid rss
  for port in $ports; do
    pid="$(lsof -nP -iTCP:"$port" -sTCP:LISTEN -t 2>/dev/null | head -n 1 || true)"
    if [[ -n "$pid" ]]; then
      rss="$(ps -o rss= -p "$pid" 2>/dev/null | tr -d ' ' || echo 0)"
      printf '%s %s %s %s\n' "$port" "$pid" "$(ps -o comm= -p "$pid" 2>/dev/null | xargs basename 2>/dev/null || echo '?')" "$(awk -v k="${rss:-0}" 'BEGIN { printf "%.1f", k / 1024 }')"
    fi
  done
}

# Runs one endpoint for the full duration with `clients` concurrent clients
# and writes "<label> <requests> <p50> <p95> <p99> <ok> <non2xx>" to $1.
# Every tool ends in the same line shape so the summary is tool-independent.
run_endpoint() {
  local out="$1" label="$2" target="$3" clients="$4"
  local p50 p95 p99 total ok bad
  case "$tool" in
    oha)
      local json
      # oha ≥ 1.5 spells it --output-format json; older releases --json.
      if oha --help 2>&1 | grep -q -- '--output-format'; then
        json="$(oha -z "${duration}s" -c "$clients" --no-tui --output-format json ${cookie:+-H "Cookie: sdi_session=$cookie"} "$target")"
      else
        json="$(oha -z "${duration}s" -c "$clients" --no-tui --json ${cookie:+-H "Cookie: sdi_session=$cookie"} "$target")"
      fi
      if command -v jq >/dev/null 2>&1; then
        p50="$(printf '%s' "$json" | jq -r '(.latencyPercentiles.p50 * 1000 * 10 | round) / 10')"
        p95="$(printf '%s' "$json" | jq -r '(.latencyPercentiles.p95 * 1000 * 10 | round) / 10')"
        p99="$(printf '%s' "$json" | jq -r '(.latencyPercentiles.p99 * 1000 * 10 | round) / 10')"
        # Requests oha itself cut off when the duration ended are neither
        # answers nor failures; every other transport error counts.
        total="$(printf '%s' "$json" | jq -r '([.statusCodeDistribution[]] | add // 0) + ([.errorDistribution // {} | to_entries[] | select(.key | test("deadline") | not) | .value] | add // 0)')"
        ok="$(printf '%s' "$json" | jq -r '[.statusCodeDistribution | to_entries[] | select(.key | test("^2")) | .value] | add // 0')"
      else
        echo "loadcheck: install jq for the oha summary" >&2
        exit 1
      fi
      ;;
    hey)
      local report
      report="$(hey -z "${duration}s" -c "$clients" ${cookie:+-H "Cookie: sdi_session=$cookie"} "$target")"
      p50="$(printf '%s\n' "$report" | awk '/ 50% in /{printf "%.1f", $3 * 1000}')"
      p95="$(printf '%s\n' "$report" | awk '/ 95% in /{printf "%.1f", $3 * 1000}')"
      p99="$(printf '%s\n' "$report" | awk '/ 99% in /{printf "%.1f", $3 * 1000}')"
      total="$(printf '%s\n' "$report" | awk '/^ *\[[0-9]+\] [0-9]+ responses/{n += $2} END {print n + 0}')"
      ok="$(printf '%s\n' "$report" | awk '/^ *\[2[0-9][0-9]\] /{n += $2} END {print n + 0}')"
      ;;
    curl)
      local dir="$tmp/$label"
      mkdir -p "$dir"
      export LOADCHECK_WORKER_URL="$target" LOADCHECK_WORKER_SECONDS="$duration" LOADCHECK_WORKER_DIR="$dir" LOADCHECK_WORKER_COOKIE="$cookie"
      seq "$clients" | xargs -P "$clients" -n 1 bash -c '
        while (( SECONDS < LOADCHECK_WORKER_SECONDS )); do
          curl -s -o /dev/null --max-time 10 ${LOADCHECK_WORKER_COOKIE:+-H "Cookie: sdi_session=$LOADCHECK_WORKER_COOKIE"} -w "%{http_code} %{time_total}\n" "$LOADCHECK_WORKER_URL" || echo "000 0"
        done > "$LOADCHECK_WORKER_DIR/worker-$1.log"
      ' _
      cat "$dir"/worker-*.log > "$dir/all.log"
      total="$(wc -l < "$dir/all.log" | tr -d ' ')"
      ok="$(awk '$1 ~ /^2/' "$dir/all.log" | wc -l | tr -d ' ')"
      read -r p50 p95 p99 < <(sort -n -k2,2 "$dir/all.log" | awk '
        { t[NR] = $2 }
        function rank(p, n,   i) { i = int(p * n); if (i < p * n) i++; if (i < 1) i = 1; return i }
        END {
          if (NR == 0) { print "0 0 0"; exit }
          printf "%.1f %.1f %.1f\n", t[rank(0.5, NR)] * 1000, t[rank(0.95, NR)] * 1000, t[rank(0.99, NR)] * 1000
        }')
      ;;
  esac
  bad=$(( total - ok ))
  printf '%s %s %s %s %s %s %s\n' "$label" "$total" "$p50" "$p95" "$p99" "$ok" "$bad" >> "$out"
}

# --- single-URL mode ---------------------------------------------------------

if [[ -n "$url" ]]; then
  if ! curl -fsS -o /dev/null --max-time 5 "$url"; then
    echo "loadcheck: $url does not answer; is the stack up (make up)?" >&2
    exit 1
  fi
  echo "loadcheck: $tool -> $url, ${concurrency} clients, ${duration}s"
  echo
  run_endpoint "$tmp/single" single "$url" "$concurrency"
  read -r _ total p50 p95 p99 ok bad < "$tmp/single"
  printf '%-14s %s (%s r/s)\n' "requests" "$total" "$(awk -v t="$total" -v d="$duration" 'BEGIN { printf "%.1f", t / d }')"
  printf '%-14s %s ms\n' "p50" "$p50"
  printf '%-14s %s ms\n' "p95" "$p95"
  printf '%-14s %s ms\n' "p99" "$p99"
  printf '%-14s %s (%s%%)\n' "status 2xx" "$ok" "$(share "$ok" "$total")"
  printf '%-14s %s (%s%%)\n' "other" "$bad" "$(share "$bad" "$total")"
  if [[ "$ok" -eq 0 ]]; then
    echo; echo "loadcheck: not one 2xx in ${duration}s; the edge or the gateway pool is broken" >&2; exit 1
  fi
  if [[ -n "$max_p95_ms" ]] && awk -v p="$p95" -v m="$max_p95_ms" 'BEGIN { exit !(p > m) }'; then
    echo; echo "loadcheck: p95 ${p95} ms exceeds the ${max_p95_ms} ms budget" >&2; exit 1
  fi
  exit 0
fi

# --- suite mode --------------------------------------------------------------

if ! curl -fsS -o /dev/null --max-time 5 "$base/api/v1/health"; then
  echo "loadcheck: $base/api/v1/health does not answer; is the stack up?" >&2
  exit 1
fi

# Sign in once. Dev login (SDI_DEV_LOGIN=1) redirects straight to the
# callback, which sets the cookie and redirects to the public origin; the
# jar keeps the cookie whatever the final page answers.
if [[ -z "$cookie" ]]; then
  jar="$tmp/jar"
  curl -sS -c "$jar" -L --max-redirs 3 -o /dev/null "$base/api/v1/auth/login?returnTo=/" || true
  cookie="$(awk '$6 == "sdi_session" {print $7}' "$jar" | tail -n 1)"
  if [[ -z "$cookie" ]]; then
    echo "loadcheck: sign-in did not set sdi_session (is SDI_DEV_LOGIN=1 with a GITHUB_TOKEN, or pass LOADCHECK_COOKIE)" >&2
    exit 1
  fi
fi
me="$(curl -sS --max-time 10 -H "Cookie: sdi_session=$cookie" "$base/api/v1/auth/me")"
if ! printf '%s' "$me" | grep -q '"currentWorkspaceId"'; then
  echo "loadcheck: /auth/me rejected the session: $me" >&2
  exit 1
fi
login="$(printf '%s' "$me" | sed -n 's/.*"login":"\([^"]*\)".*/\1/p' | head -n 1)"
if [[ -z "$project" ]]; then
  project="$(curl -sS --max-time 10 -H "Cookie: sdi_session=$cookie" "$base/api/v1/projects" | sed -n 's/^\[{"id":"\([^"]*\)".*/\1/p')"
fi

echo "loadcheck: $tool -> $base as ${login:-?}, ${concurrency} clients for ${duration}s, ${sse} SSE connections held"
echo

echo "RSS before (MB):"
rss_snapshot | tee "$tmp/rss-before" | awk '{ printf "  %-6s %-14s %8s\n", $1, $3, $4 }'
[[ -s "$tmp/rss-before" ]] || echo "  (no local service listens on: $ports)"
echo

# Hold the SSE connections for the whole run. Each curl records the HTTP
# status it got; the stream itself only carries keep-alives unless
# something changes.
sse_ok=0
if (( sse > 0 )); then
  mkdir -p "$tmp/sse"
  set +m
  for i in $(seq "$sse"); do
    curl -sN --max-time $(( duration + 5 )) -o /dev/null -H "Cookie: sdi_session=$cookie" \
      -w '%{http_code}\n' "$base/api/v1/events/stream" > "$tmp/sse/$i" 2>/dev/null &
    sse_pids+=($!)
  done
  sleep 2
fi

# The four endpoints share the client budget and run at the same time.
endpoints=(
  "projects|$base/api/v1/projects"
  "executions|$base/api/v1/executions?limit=50"
  "proposals|$base/api/v1/proposals"
)
if [[ -n "$project" ]]; then
  endpoints+=("tree|$base/api/v1/projects/$project/tree")
else
  echo "loadcheck: no project connected; /projects/{id}/tree is skipped (connect one or pass -p)"
fi
per=$(( concurrency / ${#endpoints[@]} ))
(( per < 1 )) && per=1
: > "$tmp/results"
pids=()
for entry in "${endpoints[@]}"; do
  run_endpoint "$tmp/results" "${entry%%|*}" "${entry#*|}" "$per" &
  pids+=($!)
done
wait "${pids[@]}"

if (( sse > 0 )); then
  wait "${sse_pids[@]}" 2>/dev/null || true
  sse_ok="$(cat "$tmp/sse"/* 2>/dev/null | grep -c '^200$' || true)"
  sse_pids=()
fi

echo "RSS after (MB):"
rss_snapshot | tee "$tmp/rss-after" | awk '{ printf "  %-6s %-14s %8s\n", $1, $3, $4 }'
echo

printf '%-12s %9s %8s %9s %9s %9s %8s\n' endpoint requests "r/s" "p50 ms" "p95 ms" "p99 ms" "non-2xx"
total_all=0; bad_all=0; worst_p95=0
while read -r label total p50 p95 p99 ok bad; do
  printf '%-12s %9s %8s %9s %9s %9s %7s%%\n' "$label" "$total" "$(awk -v t="$total" -v d="$duration" 'BEGIN { printf "%.1f", t / d }')" "$p50" "$p95" "$p99" "$(share "$bad" "$total")"
  total_all=$(( total_all + total )); bad_all=$(( bad_all + bad ))
  worst_p95="$(awk -v a="$p95" -v b="$worst_p95" 'BEGIN { print (a > b) ? a : b }')"
done < <(sort "$tmp/results")
if (( sse > 0 )); then
  printf '%-12s %9s held for %ss, %s answered 200\n' "events/stream" "$sse" "$duration" "$sse_ok"
fi
echo
if [[ -s "$tmp/rss-before" && -s "$tmp/rss-after" ]]; then
  echo "RSS delta (MB):"
  join -j 1 <(sort "$tmp/rss-before") <(sort "$tmp/rss-after") | awk '{ printf "  %-6s %-14s %8s -> %8s (%+.1f)\n", $1, $3, $4, $7, $7 - $4 }'
  echo
fi

error_pct="$(share "$bad_all" "$total_all")"
echo "total: $total_all requests, non-2xx ${error_pct}%, worst p95 ${worst_p95} ms"

status=0
if [[ "$total_all" -eq 0 || "$bad_all" -eq "$total_all" ]]; then
  echo "loadcheck: nothing answered 2xx" >&2; status=1
fi
if awk -v e="$error_pct" -v m="$max_error_pct" 'BEGIN { exit !(e > m) }'; then
  echo "loadcheck: non-2xx share ${error_pct}% exceeds ${max_error_pct}%" >&2; status=1
fi
if [[ -n "$max_p95_ms" ]] && awk -v p="$worst_p95" -v m="$max_p95_ms" 'BEGIN { exit !(p > m) }'; then
  echo "loadcheck: worst p95 ${worst_p95} ms exceeds the ${max_p95_ms} ms budget" >&2; status=1
fi
if (( sse > 0 )) && (( sse_ok < sse )); then
  echo "loadcheck: only ${sse_ok}/${sse} SSE connections were accepted" >&2; status=1
fi
exit "$status"

#!/usr/bin/env bash
# 30-second load check through the nginx edge. Reports p50/p95 latency and
# the share of 429s. The 429 share is the number that says whether the rate
# limits in CONTRACT.md §10 behave: one address hammering /api/ at more than
# 20 r/s (burst 40) *should* be told off, and an SSE route never should.
#
# Uses `oha` or `hey` when one is installed, otherwise a curl loop fanned out
# with `xargs -P`. Only bash, curl, awk and sort are required for the
# fallback, so it runs on a bare macOS or a CI runner.
#
#   scripts/loadcheck.sh [-u URL] [-c CONNECTIONS] [-d SECONDS] [-m MAX_P95_MS]
#
#   -u  URL to hit            (default http://localhost/api/v1/health)
#   -c  concurrent clients    (default 16)
#   -d  duration in seconds   (default 30)
#   -m  fail when p95 exceeds this many milliseconds (default: report only)
#
# Environment variables LOADCHECK_URL, LOADCHECK_CONCURRENCY,
# LOADCHECK_DURATION and LOADCHECK_MAX_P95_MS set the same defaults.
set -euo pipefail

url="${LOADCHECK_URL:-http://localhost/api/v1/health}"
concurrency="${LOADCHECK_CONCURRENCY:-16}"
duration="${LOADCHECK_DURATION:-30}"
max_p95_ms="${LOADCHECK_MAX_P95_MS:-}"

usage() {
  sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'
}

while getopts "u:c:d:m:h" opt; do
  case "$opt" in
    u) url="$OPTARG" ;;
    c) concurrency="$OPTARG" ;;
    d) duration="$OPTARG" ;;
    m) max_p95_ms="$OPTARG" ;;
    h) usage; exit 0 ;;
    *) usage >&2; exit 2 ;;
  esac
done

for n in "$concurrency" "$duration"; do
  case "$n" in
    ''|*[!0-9]*) echo "loadcheck: -c and -d take positive integers (got '$n')" >&2; exit 2 ;;
  esac
done

# The edge must answer before it is measured; a dead stack would otherwise
# report a beautiful p50 of connection refusals.
if ! curl -fsS -o /dev/null --max-time 5 "$url"; then
  echo "loadcheck: $url does not answer; is the stack up (make up)?" >&2
  exit 1
fi

if command -v oha >/dev/null 2>&1; then
  tool=oha
elif command -v hey >/dev/null 2>&1; then
  tool=hey
else
  tool=curl
fi
echo "loadcheck: $tool -> $url, ${concurrency} clients, ${duration}s"
echo

# Every branch ends by setting these, in milliseconds and counts, so the
# summary and the threshold check below are tool-independent.
p50_ms=""
p95_ms=""
total=0
ok=0
limited=0

case "$tool" in
  oha)
    # --no-tui for a non-interactive run; the JSON carries the percentiles
    # and the status histogram, so no parsing of the human report is needed.
    json="$(oha -z "${duration}s" -c "$concurrency" --no-tui --json "$url")"
    if ! command -v jq >/dev/null 2>&1; then
      echo "$json"
      echo "loadcheck: install jq for the summary" >&2
      exit 0
    fi
    p50_ms="$(printf '%s' "$json" | jq -r '(.latencyPercentiles.p50 * 1000 * 10 | round) / 10')"
    p95_ms="$(printf '%s' "$json" | jq -r '(.latencyPercentiles.p95 * 1000 * 10 | round) / 10')"
    total="$(printf '%s' "$json" | jq -r '[.statusCodeDistribution[]] | add // 0')"
    ok="$(printf '%s' "$json" | jq -r '.statusCodeDistribution["200"] // 0')"
    limited="$(printf '%s' "$json" | jq -r '.statusCodeDistribution["429"] // 0')"
    ;;
  hey)
    # hey only prints a human report: pull the two percentiles out of its
    # latency distribution and the counts out of its status code section.
    report="$(hey -z "${duration}s" -c "$concurrency" "$url")"
    p50_ms="$(printf '%s\n' "$report" | awk '/ 50% in /{printf "%.1f", $3 * 1000}')"
    p95_ms="$(printf '%s\n' "$report" | awk '/ 95% in /{printf "%.1f", $3 * 1000}')"
    total="$(printf '%s\n' "$report" | awk '/^ *\[[0-9]+\] [0-9]+ responses/{n += $2} END {print n + 0}')"
    ok="$(printf '%s\n' "$report" | awk '/^ *\[200\] /{print $2 + 0; f = 1} END {if (!f) print 0}')"
    limited="$(printf '%s\n' "$report" | awk '/^ *\[429\] /{print $2 + 0; f = 1} END {if (!f) print 0}')"
    ;;
  curl)
    tmp="$(mktemp -d "${TMPDIR:-/tmp}/loadcheck.XXXXXX")"
    trap 'rm -rf "$tmp"' EXIT

    # One worker per client, each looping until its own SECONDS counter
    # (fresh in every `bash -c`) passes the deadline; xargs fans them out and
    # waits for all of them. Each line is "<status> <seconds>"; a transport
    # failure records status 000 so it still counts against the total.
    export LOADCHECK_WORKER_URL="$url" LOADCHECK_WORKER_SECONDS="$duration" LOADCHECK_WORKER_DIR="$tmp"
    seq "$concurrency" | xargs -P "$concurrency" -n 1 bash -c '
      while (( SECONDS < LOADCHECK_WORKER_SECONDS )); do
        curl -s -o /dev/null --max-time 10 -w "%{http_code} %{time_total}\n" "$LOADCHECK_WORKER_URL" || echo "000 0"
      done > "$LOADCHECK_WORKER_DIR/worker-$1.log"
    ' _

    cat "$tmp"/worker-*.log > "$tmp/all.log"
    total="$(wc -l < "$tmp/all.log" | tr -d ' ')"
    ok="$(awk '$1 == 200' "$tmp/all.log" | wc -l | tr -d ' ')"
    limited="$(awk '$1 == 429' "$tmp/all.log" | wc -l | tr -d ' ')"

    # Nearest-rank percentiles over the sorted latencies, in milliseconds.
    percentiles="$(sort -n -k2,2 "$tmp/all.log" | awk '
      { t[NR] = $2 }
      function rank(p, n,   i) { i = int(p * n); if (i < p * n) i++; if (i < 1) i = 1; return i }
      END {
        if (NR == 0) { print "0 0"; exit }
        printf "%.1f %.1f\n", t[rank(0.5, NR)] * 1000, t[rank(0.95, NR)] * 1000
      }')"
    p50_ms="${percentiles%% *}"
    p95_ms="${percentiles##* }"
    ;;
esac

share() {
  # Percentage of $1 over $2, "0.0" when nothing was counted.
  awk -v a="$1" -v b="$2" 'BEGIN { if (b > 0) printf "%.1f", a * 100 / b; else print "0.0" }'
}

other=$(( total - ok - limited ))
printf '%-14s %s (%s r/s)\n' "requests" "$total" "$(awk -v t="$total" -v d="$duration" 'BEGIN { printf "%.1f", t / d }')"
printf '%-14s %s ms\n' "p50" "$p50_ms"
printf '%-14s %s ms\n' "p95" "$p95_ms"
printf '%-14s %s (%s%%)\n' "status 200" "$ok" "$(share "$ok" "$total")"
printf '%-14s %s (%s%%)\n' "status 429" "$limited" "$(share "$limited" "$total")"
printf '%-14s %s\n' "other" "$other"

if [[ "$ok" -eq 0 ]]; then
  echo
  echo "loadcheck: not one 200 in ${duration}s; the edge or the gateway pool is broken" >&2
  exit 1
fi

if [[ -n "$max_p95_ms" ]]; then
  if awk -v p="$p95_ms" -v m="$max_p95_ms" 'BEGIN { exit !(p > m) }'; then
    echo
    echo "loadcheck: p95 ${p95_ms} ms exceeds the ${max_p95_ms} ms budget" >&2
    exit 1
  fi
fi

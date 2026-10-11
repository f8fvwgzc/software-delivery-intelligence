#!/usr/bin/env bash
# Container healthcheck for the slim Rust images, which carry no curl or
# grpc tooling: open a TCP connection to the service's own listen port with
# bash's /dev/tcp. The gateway additionally gets a real HTTP liveness probe.
set -eu

case "${SDI_SERVICE:-}" in
  gateway)      listen="${GATEWAY_LISTEN:-0.0.0.0:8080}" ;;
  knowledge)    listen="${KNOWLEDGE_LISTEN:-0.0.0.0:50051}" ;;
  intelligence) listen="${INTELLIGENCE_LISTEN:-0.0.0.0:50052}" ;;
  execution)    listen="${EXECUTION_LISTEN:-0.0.0.0:50053}" ;;
  identity)     listen="${IDENTITY_LISTEN:-0.0.0.0:50054}" ;;
  *) echo "unknown SDI_SERVICE: ${SDI_SERVICE:-<unset>}" >&2; exit 1 ;;
esac
port="${listen##*:}"

exec 3<>"/dev/tcp/127.0.0.1/${port}"

if [[ "${SDI_SERVICE}" == gateway ]]; then
  printf 'GET /api/v1/health HTTP/1.0\r\nHost: localhost\r\n\r\n' >&3
  IFS= read -r -t 3 -u 3 status_line
  [[ "${status_line}" == *" 200 "* ]]
fi

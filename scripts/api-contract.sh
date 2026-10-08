#!/usr/bin/env bash
# Builds the gateway, dumps its OpenAPI document to contracts/openapi.json and
# regenerates the web client's types from it.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo run -q -p sdi-gateway -- --print-openapi > contracts/openapi.json
echo "wrote contracts/openapi.json"
(cd web && pnpm api:generate)

# End-to-end on the host

The exact phase-2 sequence that was run against the real services, the real
`f8fvwgzc` GitHub account and the host's Docker daemon. Every command below
worked as written; ids are examples and must be replaced with the ones your
calls return. `jq` only pulls ids out of responses. The v1 sequence (facts,
graph, impact, search, negotiation details, the simulated dry run with a
fault) still works unchanged; it is kept short here and the phase-2 surface
gets the detail.

## 0. Start everything

```sh
docker compose -f docker-compose.dev.yml up -d --wait          # Postgres + pgvector on 127.0.0.1:5440
export DATABASE_URL=postgres://sdi:sdi@127.0.0.1:5440/sdi
export GITHUB_TOKEN="$(gh auth token)"                         # scopes: repo (+ workflow for PRs that add workflows)
export RUST_LOG=info,sqlx=warn
export SDI_RUN_DIR="$PWD/.run"; mkdir -p "$SDI_RUN_DIR"        # docker adapter workdirs, absolute
cargo build --workspace
for s in knowledge intelligence execution gateway; do
  nohup ./target/debug/sdi-$s > /tmp/claude-501/sdi2-$s.log 2>&1 &
  sleep 0.5
done
G=http://127.0.0.1:8080/api/v1
curl -s $G/system/status | jq '.status, [.services[] | {name, status}]'
# "ok"; knowledge, intelligence, execution all "serving"
```

The knowledge service applies `migrations/0002_phase2.sql` and seeds the 26
catalog patterns on start; execution applies `0002_adapters.sql` and seeds the
`github-token` identity while `GITHUB_TOKEN` is set.

## 1. The live feed, in a second terminal

```sh
curl -sN $G/events/stream > /tmp/claude-501/sdi2-events.log &
# every step below shows up here; at the end of this sequence the log held
# 140 events: project 4, scan created/progress/updated 2/28/2, pattern 12,
# proposal created/updated 7/10, decision 3, execution created/updated 4/60,
# approval created/updated 4/4
```

## 2. Connect and scan (tarball path, live scan stream, maturity, config)

```sh
curl -s $G/projects | jq '.[] | {id, repoFullName, status, maturityScore, hasConfig}'
PID=$(curl -s -X POST $G/projects -H 'content-type: application/json' \
  -d '{"repoFullName":"f8fvwgzc/view-engine"}' | jq -r .id)      # 201; skip when already connected
SID=$(curl -s -X POST $G/projects/$PID/scans -H 'content-type: application/json' -d '{}' | jq -r .id)   # 201 queued

# The scan stream: one `data: <Scan>` per stage change, then `event: end`.
curl -sN --max-time 90 $G/scans/$SID/stream
# 15 data lines (metadata, tree, files, analyze, graph, embeddings, delivery
# each running -> done), the last one status "succeeded", then `event: end`.

curl -s $G/scans/$SID | jq '{status, filesTotal, filesAnalyzed, stages: [.stages[] | {key, detail}]}'
# files: "28 of 28 wanted files (tarball)"  -- one GET /repos/{o}/{r}/tarball/{sha}, no per-file downloads
# analyze: "34 facts, 13 nodes, 14 edges, 2 questions, maturity 47 (transitional)"

curl -s $G/projects/$PID/maturity | jq '{overall, stage, axes: [.axes[] | {key, score, summary, findings, recommendedPatterns}]}'
# overall 47 transitional; eight axes:
#   containerization 85 (Dockerfile at backend/Dockerfile; compose; healthcheck; finding "Runs as root: frontend/Dockerfile has no USER")
#   ci 0 (No CI workflows -> add-ci-baseline, gha-ci-build-test)
#   tests 60, iac 0, dependencies 60 (Dependabot not configured), docs 50 (No architecture document),
#   secrets 40 (No .env.example; no secret scanning), runtime 100 (Rust 1.91/edition 2021, Python 3.12, Node 22)

curl -s $G/projects/$PID/config
# {"present":false,"path":null,"deployTarget":null,...,"errors":[]}  -- view-engine has no .sdi.yml
```

## 3. Runtime facts and release classification (texc-symphony)

```sh
PID2=$(curl -s $G/projects | jq -r '.[] | select(.repoFullName=="f8fvwgzc/texc-symphony") | .id')
curl -s -X POST $G/projects/$PID2/scans -H 'content-type: application/json' -d '{}' | jq -r .id
curl -s "$G/projects/$PID2/facts" | jq -r '.[] | select(.key | startswith("runtime:") or startswith("deploy:pipeline") or startswith("ci:build")) | "\(.key)\t\(.value)\t\(.evidence)"'
# runtime:rust:1.99          rust-toolchain.toml: 1.99.0
# runtime:rust:edition-2024  Cargo.toml: edition = "2024"
# runtime:node:22            web/package.json: engines.node >=22.12.0
# deploy:pipeline:.github/workflows/release.yml = release   (pushes an image to GHCR: "Releases from CI")
# deploy:pipeline:.github/workflows/ci.yml      = release   (conditional push on main)
# ci:build:.github/workflows/{ci,release}.yml   (docker/build-push-action)
```

## 4. Proposals: modernization, budget, negotiation, artifacts

```sh
PRM=$(curl -s -X POST $G/proposals -H 'content-type: application/json' \
  -d "{\"projectId\":\"$PID\",\"kind\":\"modernization\"}" | jq -r .id)
curl -s $G/proposals/$PRM | jq '.options[] | {rank, patternId, fit: .score.architectureFit, overall: .score.overall, artifactCount, unmetRequirements}'
# 1 add-ci-baseline (ci 0 -> fit 1.0, 2 files)   2 managed-database (iac 0, `database:*` matched by database:postgres)
# 3 config-to-environment (secrets 40)           4 upgrade-runtime (dependencies 60)   5 containerize-legacy-app (containerization 85 -> 0.15)

curl -s $G/proposals/$PRM/options/opt-add-ci-baseline/artifacts | jq -r '.[] | "--- \(.path) replaces=\(.replacesExisting)\n\(.content)"'
# .github/workflows/ci.yml: jobs test-backend (Rust: fmt, clippy, cargo test in backend/),
#   test-backend-rlcd and test-quant (Python/uv), test-frontend (npm), image (context ./backend, file backend/Dockerfile)
# .github/dependabot.yml: cargo /backend, npm /frontend, pip /backend/rlcd and /quant, docker /backend, github-actions /
curl -s $G/proposals/$PRM/options/opt-containerize-legacy-app/artifacts | jq -r '.[] | select(.path=="backend/Dockerfile") | .content'
# rust:1.91-slim cargo-chef build stage, debian runtime as uid 10001, binary view-engine-api (the crate, not the repo),
# ENV PORT=3001 / EXPOSE 3001 (the port the scanned Dockerfile exposes), HEALTHCHECK on /health

# Budget: an over-budget option is marked, the cheapest viable one stays in the top 5.
PRD=$(curl -s -X POST $G/proposals -H 'content-type: application/json' \
  -d "{\"projectId\":\"$PID\",\"kind\":\"deployment\",\"budgetMonthlyUsd\":15}" | jq -r .id)
curl -s $G/proposals/$PRD | jq '{constraints: [.constraints[].key], options: [.options[] | {rank, patternId, estimatedMonthlyCostUsd, unmetRequirements, costBreakdown: [.costBreakdown[] | {item, monthlyUsd}]}]}'
# constraint budget_monthly_usd:15; hetzner-vm-compose $5 recommended; fly-io $20 (compute 10 + postgres 7 + redis 3) marked budget:15; render $17 marked

curl -s -X POST $G/proposals/$PRD/messages -H 'content-type: application/json' -d '{"content":"under 10 dollars a month"}' \
  | jq '{constraints: [.proposal.constraints[] | {key, description}], revision: .proposal.revision, reply: .reply.content}'
# constraint budget_monthly_usd:10 "budget under $10/month", revision 2, options above $10 now carry budget:10
```

## 5. Verify locally with Docker (`adapter: docker`)

```sh
EID=$(curl -s -X POST $G/proposals/$PRD/accept -H 'content-type: application/json' \
  -d '{"createExecution":true,"adapter":"docker"}' | jq -r .executionId)
curl -s $G/executions/$EID | jq '{status, adapter, steps: [.steps[] | {key, actionType, policyDecision, params}]}'
# planning, adapter docker, identity local-docker (repository:read, container:build, container:run, environment:write):
#   checkout  repo.checkout        auto
#   build     container.build      auto      dockerfile backend/Dockerfile, context ./backend, image view-engine:sdi-<sha7>
#   run       container.run        approval  port 3001, health_path /health
#   test      tests.run_local      auto      test_command "cargo test --locked", test_image rust:1.91-slim, test_dir backend
#   cleanup   env.destroy_temporary auto

curl -sN $G/executions/$EID/events/stream > /tmp/claude-501/sdi2-exec-events.log &
curl -s -X POST $G/executions/$EID/start -H 'content-type: application/json' -d '{}' | jq .status   # authorized
# LOG events stream the tarball download, "wrote N generated files on top of the checkout" and every
# `docker build` line (coalesced, <= 20 events/s); the build of the generated cargo-chef Dockerfile succeeded.
AID=$(curl -s "$G/approvals?status=pending&executionId=$EID" | jq -r '.[0].id')
curl -s -X POST $G/approvals/$AID/decide -H 'content-type: application/json' -d '{"approve":true}' | jq .status
curl -s $G/executions/$EID | jq '{status, outputs, failure: {code: .failure.code, rawOutput: .failure.rawOutput}}'
```

For `view-engine` the honest result is `diagnosing` with
`HEALTH_CHECK_FAILED`: `view-engine-api` binds `127.0.0.1` and needs
PostgreSQL at start, so the container exits before the health poll; the
failure names the container state (`exited with code N`) and carries the
`docker logs` tail (the adapter runs containers without `--rm` for exactly
this reason).
Cancelling it (`POST /executions/$EID/cancel`) removes the container and the
workdir. A repository that runs on its own goes green; the inline fixture
below is the run that was verified end to end:

```sh
# A tiny static site, written on top of the view-engine checkout as `artifacts` (no repository needed).
cat > /tmp/claude-501/fixture.json <<'JSON'
{"projectId":"<PID>","title":"Inline fixture: static site on nginx-unprivileged","adapter":"docker",
 "artifacts":{"sdi-fixture/Dockerfile":"FROM nginxinc/nginx-unprivileged:1.28-alpine AS runtime\nCOPY index.html /usr/share/nginx/html/index.html\nEXPOSE 8080\nHEALTHCHECK --interval=10s --timeout=3s --retries=3 CMD wget -qO- http://127.0.0.1:8080/ || exit 1\n",
              "sdi-fixture/index.html":"<!doctype html><title>sdi fixture</title><h1>ok</h1>\n"},
 "steps":[
  {"key":"checkout","title":"Download view-engine","actionType":"repo.checkout","adapter":"docker","dependsOn":[],"requiredPermissions":["repository:read"],"params":{"repository":"f8fvwgzc/view-engine","commit":"<scan commit>"},"verification":"workdir holds the tree"},
  {"key":"build","title":"Build the fixture image","actionType":"container.build","adapter":"docker","dependsOn":["checkout"],"requiredPermissions":["container:build"],"params":{"repository":"f8fvwgzc/view-engine","image":"view-engine:sdi-fixture","dockerfile":"sdi-fixture/Dockerfile","context":"./sdi-fixture"},"verification":"docker build exits 0"},
  {"key":"run","title":"Start the fixture","actionType":"container.run","adapter":"docker","dependsOn":["build"],"requiredPermissions":["container:run"],"params":{"repository":"f8fvwgzc/view-engine","image":"view-engine:sdi-fixture","port":"8080","health_path":"/"},"verification":"GET / answers 200"},
  {"key":"test","title":"nginx -t","actionType":"tests.run_local","adapter":"docker","dependsOn":["run"],"requiredPermissions":["container:run"],"params":{"repository":"f8fvwgzc/view-engine","image":"view-engine:sdi-fixture","test_command":"nginx -t"},"verification":"exits 0"},
  {"key":"cleanup","title":"Remove everything","actionType":"env.destroy_temporary","adapter":"docker","dependsOn":["test"],"requiredPermissions":["environment:write"],"params":{"repository":"f8fvwgzc/view-engine","image":"view-engine:sdi-fixture"},"verification":"nothing left"}]}
JSON
EID=$(curl -s -X POST $G/executions -H 'content-type: application/json' --data @/tmp/claude-501/fixture.json | jq -r .id)
curl -s -X POST $G/executions/$EID/start -H 'content-type: application/json' -d '{}' > /dev/null
sleep 12; AID=$(curl -s "$G/approvals?status=pending&executionId=$EID" | jq -r '.[0].id')
curl -s -X POST $G/approvals/$AID/decide -H 'content-type: application/json' -d '{"approve":true}' > /dev/null
sleep 18; curl -s $G/executions/$EID | jq '{status, steps: [.steps[] | {key, status}], outputs}'
# succeeded; outputs.image view-engine:sdi-fixture, outputs.container_url http://127.0.0.1:<mapped port>,
# outputs.workdir; `ls .run` is empty and `docker ps -a --filter name=sdi-` shows nothing
```

## 6. Open a pull request (`adapter: github`, private repository only)

```sh
PRT=$(curl -s -X POST $G/proposals -H 'content-type: application/json' \
  -d "{\"projectId\":\"$PID2\",\"kind\":\"modernization\"}" | jq -r .id)
# Pick an option whose files stay outside .github/workflows/ unless the token has the `workflow` scope.
EID=$(curl -s -X POST $G/proposals/$PRT/pull-request -H 'content-type: application/json' \
  -d '{"optionId":"opt-config-to-environment"}' | jq -r .executionId)                       # 201, the proposal
curl -s -X POST $G/executions/$EID/start -H 'content-type: application/json' -d '{}' > /dev/null
sleep 8; AID=$(curl -s "$G/approvals?status=pending&executionId=$EID" | jq -r '.[0].id')   # repo.open_pull_request
curl -s -X POST $G/approvals/$AID/decide -H 'content-type: application/json' -d '{"approve":true}' > /dev/null
sleep 15; curl -s $G/executions/$EID | jq '{status, outputs}'
# succeeded; outputs.branch sdi/<proposal-id-8>, outputs.commit, outputs.pull_request_url
#   https://github.com/f8fvwgzc/texc-symphony/pull/1   (.env.example, docker-compose.yml, docs/runbook.md)

gh pr view 1 --repo f8fvwgzc/texc-symphony --json state,headRefName,files
gh pr close 1 --repo f8fvwgzc/texc-symphony --delete-branch                 # cleanup; branch sdi/01a128cc removed
```

With a token that has only `repo`, an option carrying workflow files (the
first attempt used `fly-io`) fails the step with `PERMISSION_DENIED`:
`POST /repos/.../git/trees -> 403: Not Found (404 on a tree with workflow
files); GitHub refuses to write files under .github/workflows/ with this
token: a classic token needs the workflow scope (gh auth refresh -s workflow)`.
The branch it created is deleted by the cleanup above or with
`gh api -X DELETE repos/<owner>/<repo>/git/refs/heads/sdi/<id>`.

## 7. Settings round trip

```sh
curl -s $G/settings | jq .
# knowledge {scanMaxFiles 150, scanMaxFileBytes 200000, scanConcurrency 6, pipelineRunsLimit 50, cacheGithub true}
# intelligence {defaultWeights {...}, reasoner "claude", model "claude-sonnet-5-5", apiKeyConfigured false, costCeilingUsd 500}
curl -s -X PATCH $G/settings -H 'content-type: application/json' \
  -d '{"knowledge":{"scanConcurrency":4},"intelligence":{"costCeilingUsd":400}}' | jq '{c: .knowledge.scanConcurrency, ceiling: .intelligence.costCeilingUsd, files: .knowledge.scanMaxFiles}'
# 4, 400, 150 (untouched fields keep their value)
curl -s -X PATCH $G/settings -H 'content-type: application/json' -d '{"knowledge":{"scanConcurrency":6},"intelligence":{"costCeilingUsd":500}}' > /dev/null
```

## 8. Stop, then the Docker stack

```sh
pkill -f "target/debug/sdi-(knowledge|intelligence|execution|gateway)"

docker compose config --quiet && docker compose --profile local-exec config --quiet
docker compose up --build -d --wait                       # 7 containers healthy
curl -s http://localhost/api/v1/health                    # {"status":"ok"}
curl -s http://localhost/api/v1/system/status | jq '[.services[].status]'
curl -sN --max-time 5 -D - http://localhost/api/v1/events/stream   # 200, text/event-stream, Cache-Control: no-cache, never 429
./scripts/loadcheck.sh -u http://localhost/api/v1/health -c 16 -d 10
# 7394 requests (739 r/s), p50 3.2 ms, p95 5.9 ms, 3.2% 200 / 96.8% 429: one address above 20 r/s is told off
docker compose down
```

Building the five images needs about 10 GB of free space in the Docker VM
(`docker system df`; `docker builder prune --keep-storage 12GB` reclaims the
build cache); the dev Postgres shares that VM and refuses writes when it
fills up.

## 9. Playwright

```sh
./scripts/dev.sh &                                        # services on the host + Vite on :5173
cd web && SDI_E2E_GITHUB=1 pnpm e2e                        # 27 passed: smoke, policies, a11y, critical path
```

The critical path connects or reuses `f8fvwgzc/view-engine`, rescans it
while watching the live stage rail, reads the maturity score from the UI and
`GET /projects/{id}/maturity`, generates a deployment proposal, negotiates,
accepts into a simulated dry run with `SKU_UNAVAILABLE` injected, takes the
recommended remediation for that fault and for the `PERMISSION_DENIED` pause
that follows (`request_access`, the simulated identity lacks
`container:run`), approves what asks, and ends on `succeeded` with a
`checkpoint` in the event log.

# End-to-end on the host

The exact sequence that was run against the real services and the real
`f8fvwgzc` GitHub account. Every command below worked as written; the ids are
examples and must be replaced with the ones your calls return. `jq` is only
used to pull ids out of responses.

## 0. Start everything

```sh
docker compose -f docker-compose.dev.yml up -d --wait          # Postgres + pgvector on 127.0.0.1:5440
export DATABASE_URL=postgres://sdi:sdi@127.0.0.1:5440/sdi
export GITHUB_TOKEN="$(gh auth token)"
export RUST_LOG=info,sqlx=warn
cargo build --workspace
mkdir -p /tmp/claude-501
for s in knowledge intelligence execution gateway; do
  nohup ./target/debug/sdi-$s > /tmp/claude-501/sdi-$s.log 2>&1 &
  sleep 0.5
done
G=http://127.0.0.1:8080/api/v1
curl -s $G/system/status | jq '.status, [.services[] | {name, status}]'
# "ok"  and every service "serving"
```

## 1. GitHub account and connecting a repository

```sh
curl -s $G/github/account | jq '{tokenConfigured, login}'       # login "f8fvwgzc"
curl -s "$G/github/repos?query=view" | jq '.[] | {fullName, connected}'

PID=$(curl -s -X POST $G/projects -H 'content-type: application/json' \
  -d '{"repoFullName":"f8fvwgzc/view-engine"}' | jq -r .id)       # 201, status "pending"

# The scan starts immediately; poll the project until it is ready (~5 s).
until [ "$(curl -s $G/projects/$PID | jq -r .status)" = ready ]; do sleep 2; done
SID=$(curl -s $G/projects/$PID | jq -r .lastScanId)
curl -s $G/scans/$SID | jq '{status, filesTotal, filesAnalyzed, stages: [.stages[] | {key, status, detail}]}'
```

Observed for `view-engine` (a monorepo with a React/Vite frontend, an axum
backend and two FastAPI services): 145 files in the tree, 25 downloaded,
28 facts, 13 nodes, 14 edges, 2 open questions, 14 chunks, 0 runs (the
repository has no workflows).

## 2. What the knowledge service knows

```sh
curl -s $G/projects/$PID/understanding | jq '{tldr, architecture, stack, requestFlow}'
# tldr: "view-engine is a React web application with an axum backend backed by
#        PostgreSQL and Redis, containerised with Docker."
curl -s "$G/projects/$PID/facts" | jq -r '.[] | "\(.kind)\t\(.category)\t\(.key)\t\(.value)"'
curl -s "$G/projects/$PID/questions" | jq '.[] | {key, prompt, options}'
curl -s "$G/projects/$PID/graph" | jq '{nodes: (.nodes | length), edges: (.edges | length)}'
curl -s "$G/projects/$PID/graph?includeHistory=true" | jq '[.edges[] | select(.validTo != null)] | length'
curl -s -X POST $G/projects/$PID/impact -H 'content-type: application/json' \
  -d "{\"nodeId\":\"$(curl -s $G/projects/$PID/graph | jq -r '.nodes[] | select(.kind=="datastore") | .id' | head -1)\"}" | jq '{summary, impacted: (.impacted | length)}'
curl -s $G/projects/$PID/dependencies | jq '{byEcosystem, dependabot, count: (.dependencies | length)}'
curl -s $G/projects/$PID/pipelines | jq '.[] | {name, path, provider, triggers}'
curl -s "$G/projects/$PID/pipeline-runs?refresh=true&limit=5" | jq '{stats, runs: [.runs[] | {runNumber, pipelineName, status, failure}]}'
curl -s "$G/projects/$PID/deployments?refresh=true" | jq '.stats'
curl -s "$G/knowledge/search?q=postgres+migrations&limit=5" | jq '.[] | {kind, title, score}'
curl -s $G/knowledge/stats | jq .
```

Answering an open question keeps the answer across rescans:

```sh
QID=$(curl -s "$G/projects/$PID/questions?onlyOpen=true" | jq -r '.[0].id')
curl -s -X POST $G/questions/$QID/answer -H 'content-type: application/json' -d '{"answer":"I don'"'"'t deploy yet"}' | jq '{key, answer}'
curl -s -X POST $G/projects/$PID/scans | jq '{id, status}'           # queued
```

## 3. Proposal, negotiation, acceptance

```sh
PRID=$(curl -s -X POST $G/proposals -H 'content-type: application/json' \
  -d "{\"projectId\":\"$PID\",\"kind\":\"deployment\"}" | jq -r .id)
curl -s $G/proposals/$PRID | jq '{recommendedOptionId, weights, options: [.options[] | {rank, patternId, overall: .score.overall, unmetRequirements, evidence: [.evidence[] | {kind, refId, label}]}]}'
# 5 ranked catalog options; fly-io recommended at 0.66; every option cites the
# pattern, the Dockerfile fact (with its id) and the policy gates of its plan.

curl -s -X POST $G/proposals/$PRID/messages -H 'content-type: application/json' \
  -d '{"content":"Can we use something cheaper than Kubernetes? We standardise on GitHub Actions because it is already set up."}' \
  | jq '{constraints: .proposal.constraints, weights: .proposal.weights, revision: .proposal.revision,
         decision: {title: .recordedDecision.title, tags: .recordedDecision.tags, scope: .recordedDecision.scope},
         changedRecommendation: .reply.changedRecommendation, reply: .reply.content}'
```

Observed, as CONTRACT.md §6 requires:

- constraint `exclude:kubernetes` (kubernetes-helm drops out of the candidates);
- the cost weight rises 0.10 → 0.22 and the others renormalise;
- an organization-scope decision "Standardise on github actions" with tag
  `ci:github_actions`; the policy signal is recomputed (1.0 for every
  pattern that delivers through GitHub Actions, previously 0.6);
- `revision` 1 → 2, `changedRecommendation` false (fly-io stays first), and
  the reply lists before/after scores plus the evidence.

```sh
curl -s "$G/decisions" | jq '.[] | {scope, status, title, tags, source}'
curl -s "$G/proposals/$PRID/messages" | jq '.[] | {role, changedRecommendation}'

EID=$(curl -s -X POST $G/proposals/$PRID/accept -H 'content-type: application/json' \
  -d '{"createExecution":true}' | jq -r .executionId)                 # proposal "accepted"
curl -s $G/executions/$EID | jq '{number, status, adapter, identityId, steps: [.steps[] | {key, actionType, dependsOn}]}'
# planning; steps analyze -> build-image -> app -> deploy-staging -> deploy-production
```

## 4. Dry run with a fault, approvals, diagnosis, replan

The `simulated` identity only holds `repository:read`, `container:build`
and `environment:write` (§5.3). Grant what the plan needs (default TTL 30
minutes) or the `app` step fails with `PERMISSION_DENIED` first, which is
also a valid path (resolve with `request_access`).

```sh
IID=$(curl -s $G/identities | jq -r '.[] | select(.name=="simulated") | .id')
curl -s -X POST $G/identities/$IID/grants -H 'content-type: application/json' \
  -d '{"permissions":["cloud:write","deploy:staging","deploy:production"],"ttlMinutes":60}' | jq '[.grants[].permission]'

# Watch the event stream in a second terminal (ends with `event: end`).
curl -N $G/executions/$EID/events/stream

curl -s -X POST $G/executions/$EID/start -H 'content-type: application/json' \
  -d '{"fault":{"stepKey":"app","code":"SKU_UNAVAILABLE"}}' | jq .status   # "authorized"
sleep 4
curl -s $G/executions/$EID | jq '{status, steps: [.steps[] | {key, status, policyDecision}]}'
# waiting_for_approval: analyze and build-image succeeded (auto), app waits (approval)

AID=$(curl -s "$G/approvals?status=pending" | jq -r '.[0].id')
curl -s -X POST $G/approvals/$AID/decide -H 'content-type: application/json' -d '{"approve":true,"note":"ok for dev"}' | jq .status
sleep 4
curl -s $G/executions/$EID | jq '{status, lastCheckpoint, failure: {stepKey: .failure.stepKey, code: .failure.code, blockedSteps: .failure.blockedSteps, options: [.failure.options[] | {id, recommended}]}}'
# diagnosing, lastCheckpoint build-image, blockedSteps [deploy-staging, deploy-production],
# options change_region (recommended) / use_supported_sku / investigate_alternative / cancel

curl -s -X POST $G/executions/$EID/resolve -H 'content-type: application/json' -d '{"optionId":"change_region"}' | jq '{status, revision}'
# authorized, revision 1; the injected fault clears after firing once

# Approve the remaining gated steps as they come up (app again, deploy-staging, deploy-production).
until [ "$(curl -s $G/executions/$EID | jq -r .status)" = succeeded ]; do
  for a in $(curl -s "$G/approvals?status=pending" | jq -r '.[].id'); do
    curl -s -X POST $G/approvals/$a/decide -H 'content-type: application/json' -d '{"approve":true}' > /dev/null
  done
  sleep 3
done
curl -s $G/executions/$EID | jq '{status, revision, lastCheckpoint, steps: [.steps[] | {key, status, attempts}]}'
# succeeded; app has 2 attempts, every other step 1

curl -s "$G/executions/$EID/events?limit=200" | jq 'length'          # 51 events, seq 1..51
curl -s $G/patterns/fly-io | jq '{deployments, successRate, lastUsedAt, usedBy}'
# deployments 1, successRate 1.0, usedBy ["view-engine"]
curl -s $G/delivery/stats | jq .                                      # failuresRecovered 1
curl -s -X POST $G/projects/$PID/documents -H 'content-type: application/json' -d '{"kind":"architecture"}' | jq '{title, factsUsed}'
```

## 5. A private Rust repository

```sh
PID2=$(curl -s -X POST $G/projects -H 'content-type: application/json' \
  -d '{"repoFullName":"f8fvwgzc/texc-symphony"}' | jq -r .id)
until [ "$(curl -s $G/projects/$PID2 | jq -r .status)" = ready ]; do sleep 2; done
curl -s $G/projects/$PID2/understanding | jq '{tldr, stack}'
curl -s "$G/projects/$PID2/facts" | jq -r '.[] | select(.category=="language" or .category=="framework" or .category=="ci_cd" or .category=="testing") | "\(.key)\t\(.value)"'
curl -s $G/projects/$PID2/pipelines | jq '.[] | {name, jobs: [.jobs[] | {key, needs}]}'
curl -s "$G/projects/$PID2/pipeline-runs?limit=5" | jq '{stats, failed: [.runs[] | select(.status=="failed") | {runNumber, failure}]}'
curl -s "$G/projects/$PID2/similar" | jq '.[] | {name: .project.name, overall}'
```

Observed: `lang:rust` primary, `framework:axum`, `package:cargo` (152
dependencies across the workspace), `datastore:sqlite` via rusqlite,
`testing:cargo_test`, `ci:github_actions` with the `ci` and `release`
workflows and their job DAGs, `deploy:pipeline:…/release.yml`,
`environment:production`, 5 synced runs (all failed on the docker job with the
failing step named) and `view-engine` as the most similar project.

## 6. Stop

```sh
pkill -f target/debug/sdi-gateway; pkill -f target/debug/sdi-intelligence
pkill -f target/debug/sdi-execution; pkill -f target/debug/sdi-knowledge
```

The Docker stack (`make up`) exposes the same API through nginx on
`http://localhost/api/v1`; the sequence above works unchanged with
`G=http://localhost/api/v1`, subject to the edge rate limits
(20 r/s burst 40, mutations 5 r/s).

# CONTRACT.md — the source of truth

Everything that has to fit together across services, the gateway and the web
app is defined here. When code and this file disagree, this file wins and the
code is wrong. Keep it updated in the same change.

## 1. What the platform does

```
GitHub repo ──connect──▶ knowledge ──context──▶ intelligence ──accepted plan──▶ execution
                        facts, graph,          ranked proposals,              policy-gated DAG,
                        pattern memory,        negotiation,                   approvals, checkpoints,
                        semantic retrieval     decisions, documents           stop → diagnose → ask → replan
```

The loop is **observe → understand → store → retrieve → compare → rank →
propose → negotiate → approve → execute → observe → learn outcome**.

Four principles every component honours:

1. **Detected ≠ inferred ≠ proposed ≠ decided.** They are different types and
   the UI never mixes them.
2. **Discover first, ask only what cannot be known.** Open questions are
   explicit objects, asked once and remembered.
3. **Observe mode is the default.** Execute mode is opt-in per project and
   every action still passes permission → policy → approval.
4. **Failure stops the run.** Dependents are blocked, the failure is
   diagnosed, the human picks a remediation, the plan is rebuilt from the last
   checkpoint. Never "continue and hope".

## 2. Topology

| Component | Kind | Port | Talks to |
|---|---|---|---|
| `web` | React 19 + Vite + shadcn/ui + TanStack Query | 5173 (dev) | gateway over HTTP |
| `nginx` | web server, reverse proxy, rate limiter, load balancer | 80 (host) / 8080 (container, unprivileged) | web static files, gateway replicas |
| `services/gateway` | axum REST + OpenAPI (public API) | 8080 | knowledge, intelligence, execution over gRPC |
| `services/knowledge` | tonic gRPC | 50051 | Postgres (`knowledge` schema), GitHub API |
| `services/intelligence` | tonic gRPC | 50052 | Postgres (`intelligence` schema), knowledge, execution |
| `services/execution` | tonic gRPC | 50053 | Postgres (`execution` schema), knowledge |
| `postgres` | pgvector/pgvector:pg16 | 5440 (dev host), 5432 (docker) | — |

Service-to-service calls are **gRPC only** (`/proto/sdi/*/v1/*.proto`, compiled
by `crates/proto`). The gateway is the only HTTP surface. Services never share
tables: each owns one Postgres schema and reaches the others' data through
gRPC. Building needs `protoc` with its include directory
(`google/protobuf/timestamp.proto`): Homebrew `protobuf` on the host,
`protobuf-compiler` + `libprotobuf-dev` in the Docker builder and CI.

Shared crates:

- `crates/proto` — generated clients/servers. Never hand-edited.
- `crates/common` — config, telemetry, db pool per schema, lazy gRPC channels,
  shutdown, `embed::embed()` (256-dim local hashing embeddings), time helpers.
- `crates/analyzer` — pure functions `wanted_files(tree)` and
  `analyze_at(&RepoSnapshot, today) -> Analysis`. No I/O, no clock: the
  caller passes the date (there is no default one in the crate).

## 3. Environment

| Variable | Default | Used by |
|---|---|---|
| `DATABASE_URL` | `postgres://sdi:sdi@127.0.0.1:5440/sdi` | all services |
| `GITHUB_TOKEN` | — | knowledge (dev fallback only: used for every workspace while `IDENTITY_GRPC_URL` is unreachable, logged once; `scripts/dev.sh` exports `gh auth token`) |
| `LLM_PROVIDER` | `anthropic` | intelligence (instance-wide reasoner fallback, §19: `anthropic` or `openai_compatible`) |
| `LLM_BASE_URL` | — | intelligence (OpenAI-compatible endpoint: OpenAI, Azure OpenAI with `?api-version=`, Ollama, vLLM, OpenRouter, Gemini) |
| `LLM_API_KEY` / `ANTHROPIC_API_KEY` | — | intelligence (optional dev fallback key; `ANTHROPIC_API_KEY` is the alias) |
| `LLM_MODEL` / `ANTHROPIC_MODEL` | `claude-sonnet-5-5` | intelligence |
| `OSV_API_URL` | `https://api.osv.dev` | intelligence (§20 dependency vulnerabilities; `off` disables) |
| `KNOWLEDGE_GRPC_URL` | `http://127.0.0.1:50051` | gateway, intelligence, execution |
| `IDENTITY_GRPC_URL` | `http://127.0.0.1:50054` | gateway, knowledge (tokens, data controls, audit), intelligence, execution |
| `INTELLIGENCE_GRPC_URL` | `http://127.0.0.1:50052` | gateway |
| `EXECUTION_GRPC_URL` | `http://127.0.0.1:50053` | gateway, intelligence |
| `GATEWAY_LISTEN` / `KNOWLEDGE_LISTEN` / `INTELLIGENCE_LISTEN` / `EXECUTION_LISTEN` | `0.0.0.0:<port>` | each service |
| `LOG_FORMAT` | text (`json` in containers) | all |
| `RUST_LOG` | `info,sqlx=warn` | all |
| `SDI_RUN_DIR` | `.run` | execution (`docker` adapter workdirs) |
| `GITHUB_API_URL` | `https://api.github.com` | execution (`docker` tarball downloads and the `github` adapter; GHES or a test server) |
| `DOCKER_HOST` | Docker default | execution (`docker` adapter; the compose `local-exec` profile mounts the socket) |
| `GITHUB_TOKEN` | — | execution (`github` adapter; same token as knowledge, needs `repo` write) |

## 4. Database

One server, three schemas: `knowledge`, `intelligence`, `execution`. Each
service runs `sqlx::migrate!()` from its own `migrations/` on start, against a
pool whose `search_path` is `<schema>, public` (`sdi_common::db::connect`).
`CREATE EXTENSION IF NOT EXISTS vector` lives in the knowledge service's first
migration (extension objects land in `public`).

Rules: runtime `sqlx::query`/`query_as` with `FromRow` structs (no compile-time
`query!` macros, so builds never need a live database). Vectors are bound as
text and cast: `$1::vector`. Dimension is `sdi_common::embed::EMBEDDING_DIM`
(256). Ids are `uuid` v7 generated in Rust; timestamps `timestamptz`.

## 5. Vocabularies

### 5.1 Capabilities (`Analysis.capabilities`, `Pattern.requires`)

Stable lowercase keys. A project *has* capabilities; a pattern *requires* them.

```
lang:<rust|typescript|javascript|python|go|java|kotlin|csharp|ruby|php|elixir|swift|...>
framework:<react|nextjs|vue|nuxt|svelte|angular|astro|express|nestjs|fastapi|django|flask|axum|actix|rocket|gin|spring|rails|laravel|phoenix|...>
database:<postgres|mysql|sqlite|mongodb|redis|...>
package:<npm|pnpm|yarn|cargo|pip|poetry|go|maven|gradle|composer|bundler|mix>
dockerfile          docker_compose      lockfile            env_example
ci:github_actions   ci:gitlab_ci        dependabot
iac:terraform       iac:bicep           iac:pulumi          iac:cloudformation
k8s:manifests       k8s:helm
cloud:azure         cloud:aws           cloud:gcp
tests:unit          tests:e2e           migrations
static_site         monorepo            docs:readme         docs:architecture
```

Capabilities are derived only from facts with `confidence ≥ 0.8`, so a cloud
inferred from an SDK (0.6) never satisfies a pattern's `requires`. Fact keys
carry their category and are unique per analysis (the knowledge service
upserts by `(project_id, key)`); the prefixes other services may rely on are:

```
lang:<l>  framework:<f>  package:<p>  cloud:<c>  ci:github_actions | ci:gitlab_ci  ci:workflow:<path>
datastore:<engine>  infra:datastore:<engine>  container:dockerfile:<path>  container:compose:<path>
iac:<tool>  k8s:manifests | k8s:helm  migrations:<tool>  tests:unit | tests:e2e
security:lockfile | security:env_example | security:dependabot
repository:static_site | repository:monorepo  docs:readme | docs:architecture
deploy:pipeline:<path>  environment:<name>
```

Graph node keys follow `<kind>:<name>` (`service:<name>`,
`container_image:<name>`, `pipeline:<path>`, `iac_module:<dir>`,
`datastore:<engine>`, `environment:<name>`); every service, library,
pipeline and IaC module hangs under the `repository:<owner/name>` node.
Relations (upper snake case verbs on `Edge.relation`): `CONTAINS`,
`DEPENDS_ON`, `USES` (external API), `DEPLOYED_TO`, `BUILDS`, `DEPLOYS`,
`RELEASES`, `TESTED_BY`, `PROVISIONS` (iac_module → infrastructure),
`PROVIDES` (infrastructure → datastore), `PACKAGED_AS` (service →
container_image), `APPLIES_TO` (migration → datastore), `DOCUMENTED_BY`,
`OWNED_BY`. Consumers key on the verb, not on `RelationCategory`.

### 5.2 Policy action types (`Policy.action_type`, `PlanStep.action_type`)

Seeded by the execution service; the defaults follow the risk table from the
design conversation. `editable=false` rows can never be relaxed.

| action_type | label | risk | default decision | editable |
|---|---|---|---|---|
| `repo.read` | Read repository | safe | auto | no |
| `repo.analyze` | Analyze repository | safe | auto | no |
| `tests.run_local` | Run tests locally | safe | auto | yes |
| `docs.generate` | Generate documentation | safe | auto | no |
| `deps.install` | Install dependencies | medium | auto | yes |
| `container.build` | Build container image | medium | auto | yes |
| `container.run` | Run container | medium | approval | yes |
| `env.create_temporary` | Create temporary environment | medium | auto | yes |
| `db.create_temporary` | Create temporary database | medium | approval | yes |
| `cloud.create_dev_resource` | Create development cloud resource | medium | approval | yes |
| `cloud.modify_resource` | Modify existing cloud resource | high | approval | yes |
| `deploy.staging` | Deploy to staging | high | approval | yes |
| `deploy.production` | Deploy to production | high | required | no |
| `db.migrate_production` | Run production database migration | high | required | no |
| `backup.restore_production` | Restore production backup | high | required | no |
| `resource.delete` | Delete resource | high | required | no |
| `firewall.modify` | Modify firewall | high | required | no |
| `secrets.rotate` | Rotate secrets | high | required | no |
| `db.destroy_production` | Destroy production database | blocked | blocked | no |
| `repo.checkout` | Download the repository at a commit | safe | auto | no |
| `env.destroy_temporary` | Remove a temporary environment | safe | auto | yes |
| `repo.open_pull_request` | Open a pull request with generated files | medium | approval | yes |

### 5.3 Permissions (`PlanStep.required_permissions`, identity grants)

`<resource>:<verb>` — e.g. `repository:read`, `repository:write`,
`container:build`, `container:run`, `environment:write`, `database:write`,
`cloud:write`, `deploy:staging`, `deploy:production`, `secrets:write`. The
seeded `simulated` identity holds `repository:read`, `container:build`,
`environment:write` only; the seeded `local-docker` identity adds
`container:run`, and the seeded `github-token` identity holds
`repository:read` + `repository:write` (it is the connected token, so the
grant only exists while `GITHUB_TOKEN` is configured); everything else must be granted (TTL, default 30
minutes) and a missing grant produces a `PERMISSION_DENIED` failure **before**
the step runs.

### 5.4 Adapters

`CreateExecutionRequest.adapter` picks the runtime for the whole run; each
step's `PlanStep.adapter` names the tool family it needs. The execution
service refuses a run (`INVALID_ARGUMENT`) whose steps need a tool the chosen
runtime cannot provide.

| adapter | what it does | steps it can run |
|---|---|---|
| `simulated` (default) | timed dry run, fault injection, never touches anything | every action type |
| `docker` | real local execution on the host's Docker daemon: `repo.checkout` downloads the GitHub tarball at the scan commit into `.run/<execution>/` and writes `CreateExecutionRequest.artifacts` on top of it (so an option's generated Dockerfile is what gets built), `container.build` runs `docker build -f params.dockerfile params.context` (the application directory's Dockerfile in a monorepo), `container.run` starts the image detached (no `--rm`, so a crashed container keeps its logs for the failure) with the configured port and polls `health_path`, `tests.run_local` runs `test_command` inside the image, or, when the plan names `params.test_image` (a compiled stack whose runtime image has no toolchain), in that image with `params.test_dir` of the checkout mounted at `/src`; `env.destroy_temporary` stops and removes everything, and a cancelled or finished run removes its container and workdir either way; stdout/stderr lines become `LOG` events | `repo.checkout`, `container.build`, `container.run`, `tests.run_local`, `env.create_temporary`, `env.destroy_temporary`, `deps.install` |
| `github` | writes to the connected repository with the configured token: `repo.open_pull_request` creates branch `sdi/<first 8 chars of the proposal id>` from the default branch, commits `CreateExecutionRequest.artifacts` via the Git Data API, opens the PR and records `outputs.pull_request_url`. Files under `.github/workflows/` need the token's `workflow` scope; without it GitHub answers 404 on the tree and the step fails `PERMISSION_DENIED` naming that scope | `repo.checkout`, `repo.open_pull_request` |
| `cloud`, `kubernetes` | reserved; `UNIMPLEMENTED` | — |

A plan may mix families (build locally, then open a PR): the service routes
each step to the family named on the step as long as the chosen runtime is
not `simulated`. Every adapter honours the same gates: permission → policy →
approval → run → verify → checkpoint. The `docker` adapter requires the
`container:build` / `container:run` permissions; `github` requires
`repository:write`.

### 5.5 Failure codes

`PERMISSION_DENIED`, `POLICY_BLOCKED`, `SKU_UNAVAILABLE`, `QUOTA_EXCEEDED`,
`TIMEOUT`, `HEALTH_CHECK_FAILED`, `BUILD_FAILED`, `TESTS_FAILED`,
`DEPENDENCY_CONFLICT`, `UNKNOWN`. Each has at least two remediation options
plus the free-text instruction path.

### 5.6 Pattern catalog (seeded by knowledge, `source=catalog`, scope `public_template`)

| id slug | name | category | requires |
|---|---|---|---|
| `vm-docker-compose` | Single VM with Docker Compose | deployment | `dockerfile` |
| `github-pages-static` | Static site on GitHub Pages | deployment | `static_site` |
| `azure-container-apps` | Azure Container Apps + GitHub OIDC | deployment | `dockerfile`, `ci:github_actions` |
| `aws-ecs-fargate` | AWS ECS Fargate + Terraform | deployment | `dockerfile` |
| `gcp-cloud-run` | Google Cloud Run + Artifact Registry | deployment | `dockerfile` |
| `kubernetes-helm` | Kubernetes with Helm | deployment | `dockerfile`, `k8s:manifests` |
| `fly-io` | Fly.io machines | deployment | `dockerfile` |
| `gha-ci-build-test` | GitHub Actions: lint, test, build | ci_cd | — |
| `gha-docker-publish` | GitHub Actions: build and publish image | ci_cd | `dockerfile` |
| `dependabot-weekly` | Dependabot weekly grouped updates | dependency | `lockfile` |
| `docker-compose-test-env` | Disposable Docker Compose test environment | testing | `docker_compose` |
| `uat-checklist` | Generated UAT checklist per change | testing | — |
| `terraform-remote-state` | Terraform remote state + plan on PR | infrastructure | `iac:terraform` |
| `secret-scanning-baseline` | Secret scanning + env example | security | — |
| `readme-architecture-docs` | README + architecture document | documentation | — |
| `containerize-legacy-app` | Containerize an application that has no Dockerfile | modernization | — |
| `add-ci-baseline` | Add a CI baseline where none exists | modernization | — |
| `upgrade-runtime` | Move an end-of-life runtime to a supported version | modernization | — |
| `config-to-environment` | Externalise configuration into environment variables | modernization | — |
| `add-healthcheck` | Add a health endpoint and container healthcheck | modernization | `dockerfile` |
| `managed-database` | Move a self-hosted database to a managed service | modernization | `database:*` |
| `monorepo-ci-matrix` | Per-package CI matrix for a monorepo | ci_cd | `monorepo` |
| `azure-app-service` | Azure App Service (Linux) + GitHub OIDC | deployment | — |
| `aws-lambda-container` | AWS Lambda container image + API Gateway | deployment | `dockerfile` |
| `hetzner-vm-compose` | Hetzner VM + Docker Compose (lowest cost) | deployment | `dockerfile` |
| `render-web-service` | Render web service | deployment | — |

Every pattern (catalog and modernization) ships a realistic `plan` (4–8
`PlanStep`s with `action_type`, `adapter`, `depends_on`,
`required_permissions`, `verification`), `pros`, `cons`, cost, security score,
complexity, and a **cost model** (`Pattern.cost_lines`, `CostLine`s: compute,
database, registry, networking, CI minutes) that intelligence fills per
project (replicas, private-repository CI minutes, the project's own
datastores as managed services). `modernization` patterns carry
`Pattern.targets_axes` so proposals and the maturity report point at the
same axes. Both are seeded by the knowledge catalog and read back from the
pattern, never recomputed elsewhere.

Every catalog pattern ships a realistic `plan` (4–8 `PlanStep`s with
`action_type`, `adapter`, `depends_on`, `required_permissions`,
`verification`), `pros`, `cons`, cost, security score and complexity.

## 6. Ranking (intelligence)

```
overall = Σ weight_i · signal_i   (weights sum to 1, default below)
semantic_similarity 0.20 · policy 0.20 · recency 0.10 · success 0.15
architecture_fit 0.15 · cost 0.10 · security 0.10
```

- `semantic_similarity`: cosine(project profile embedding, pattern embedding).
- `policy`: 1.0 if the pattern agrees with every active decision tag
  (`cloud:*`, `runtime:*`, `iac:*`, `ci:*`), 0.0 if it contradicts one, 0.6 if
  no decision applies. A pattern whose cloud/runtime/iac is `any` or empty is
  neutral; a `ci:*` tag is matched against the pattern's `requires` and named
  technologies (a pattern naming no CI system is neutral).
- `recency`: decays with `last_used_at` (1.0 ≤ 30 days … 0.3 ≥ 2 years);
  catalog patterns 0.8.
- `success`: pattern `success_rate`, shrunk toward 0.7 for < 5 deployments.
- `architecture_fit`: share of `requires` the project satisfies, minus 0.1 per
  unmet requirement, floor 0.
- `cost`: 1 − min(cost, 500)/500.
- `security`: pattern `security_score`.

Negotiation changes **constraints** and **weights**, never the signals: "we
need private networking" adds a constraint and re-ranks; "cheaper" raises the
cost weight; "we standardise on X" records a `Decision` with tags and reruns
policy. Parsing is deterministic (`intelligence/src/domain/negotiation.rs`):
a weight word shifts that weight by +0.15 and renormalises; "not X",
"avoid X", "cheaper than X", "instead of X" add `exclude:x`; "use X" /
"prefer X" add `prefer:x`; "need X" / "keep X" add `require:x`;
"standardise on X because Y" records an organization-scope decision whose
tags come from X (`cloud:*`, `runtime:*`, `iac:*`, `ci:*`) and whose reasons
come from Y. Constraints act on `overall` only: `exclude` removes candidates,
`prefer` adds 0.10, `require` subtracts 0.10 and is listed under
`unmet_requirements`. Option ids are `opt-<pattern_id>` so they survive
re-ranking; every revision bumps `revision` and sets
`changed_recommendation` on the reply when rank 1 changed. Every assistant
reply cites `Evidence`. When the workspace's reasoner is on (§19) the
reply text is written by the configured LLM from the structured context,
within the token budget; the ranking is always deterministic.

**Budget.** `GenerateProposalRequest.budget_monthly_usd`, the project's
`.sdi.yml` budget, or a negotiation turn ("under $15 a month", "max 20
dollars") sets the `budget_monthly_usd` constraint. Options whose estimated
cost exceeds it are kept but marked (`unmet_requirements` gets
`budget:<n>`) and lose 0.25 on `overall`; the cheapest viable option is
always present in the top 5 even when its signals would rank it lower. Every
option carries a `cost_breakdown` (`CostLine`s) that sums to
`estimated_monthly_cost_usd`.

**Modernization.** `PROPOSAL_KIND_MODERNIZATION` ranks patterns of category
`modernization` by the maturity axes they raise: `architecture_fit` for this
kind is `1 − axis_score/100` of the weakest axis the pattern targets, so the
lowest axis is addressed first. The maturity report (§13) links each axis to
its `recommended_patterns`.

**Artifacts.** Each option of kind `deployment`, `ci_cd`, `infrastructure`,
`security`, `dependency`, `testing` and `modernization` carries generated
files (§14). Artifacts are generated once per revision from the project's
facts and config; negotiation that changes a parameter (region, port,
runtime) regenerates them.

**Realtime.** `GenerateProposal` answers as soon as the options are ranked
(within 2 s for a project with 50 candidate patterns; measured by
`generate_proposal_answers_fast_and_fills_artifacts_in_the_background`)
with `status=proposed` and empty `artifacts`. While it works it streams
`PlatformEvent{entity="proposal", action="progress", status="generating",
data.step}` with `step` = `retrieving_context` → `ranking` →
`generating_artifacts`; the files are generated in a background step
(`spawn_blocking`) that updates the proposal's options and emits
`action="updated", status="proposed", data.step="done"` (plus `revision`).
The update is skipped when a negotiation re-ranked the proposal meanwhile
(it regenerated the files itself). The web app shows the options at once
and fills the files in on `done`. Negotiation turns stay synchronous.

**Memory without a rescan.** Decisions (`CreateDecision`, negotiation,
accepted proposals) and open-question answers live in intelligence and
are read on every ranking: a decision `cloud:aws` changes the `policy`
signal of the next proposal without the project being scanned again, and
the constraints a negotiation adds stay on the proposal through every
later revision (`decisions_rerank_without_a_rescan_and_constraints_persist`).

## 7. Execution state machine

```
PLANNING ─start─▶ WAITING_FOR_APPROVAL ─all approved─▶ AUTHORIZED ─▶ EXECUTING ─▶ VERIFYING
   │                      │ rejected                                       │            │ ok → checkpoint → next step
   │                      ▼                                                │            ▼
   │                   CANCELLED                                           │ fail   SUCCEEDED (no steps left)
   └──────────────────────────────────────────────────────────────────────┘ ▼
                                                                         PAUSED ─▶ DIAGNOSING ─▶ (waits for ResolveFailure)
                                                                                                   │ option / instruction
                                                                                                   ▼
                                                                                              REPLANNING ─▶ WAITING_FOR_APPROVAL ...
                                                                                              (cancel → CANCELLED, give up → FAILED)
```

- Steps form a DAG (`depends_on`). A step is `READY` when all dependencies
  are checkpointed. One step runs at a time (v1). When a step fails, every
  transitive dependent becomes `BLOCKED`.
- Gating is per step: `StartExecution` moves `PLANNING → AUTHORIZED` and each
  gated step then goes `AUTHORIZED → WAITING_FOR_APPROVAL → AUTHORIZED`, so
  the execution returned by start may already be waiting, executing or even
  paused. `REPLANNING` continues the same way. `POLICY_BLOCKED` on a step with
  dependents offers `skip_branch` (step and its blocked dependents become
  `SKIPPED`). A fault injected at start fires once, then clears, so the
  retry succeeds.
- Before each step: permission check (identity grants) → policy check →
  approval if `approval`/`required` → run → verify → `CHECKPOINT` event.
- Every transition writes an `ExecutionEvent` (monotonic `seq`) and
  `WatchExecution` streams them live; the gateway exposes it as SSE.
- `ResolveFailure` rebuilds only the failed branch from the last checkpoint
  (`revision += 1`, old pending approvals expire).

## 8. Public REST API (gateway)

Base `/api/v1`. JSON, `camelCase` fields, enums as lowercase strings without
their proto prefix (`PROJECT_STATUS_SCANNING` → `"scanning"`,
`EXECUTION_STATUS_WAITING_FOR_APPROVAL` → `"waiting_for_approval"`,
`RISK_LEVEL_HIGH` → `"high"`). Timestamps RFC 3339. Ids are strings.
Errors: `{ "code": string, "message": string }` with 400/404/409/412/429/502/504.
Every DTO carries `#[derive(ToSchema)]` and every route `#[utoipa::path]`;
`GET /api/openapi.json` is the contract and `GET /api/docs` renders it
(Scalar). `web/src/lib/api/schema.d.ts` is generated from it
(`pnpm api:generate`) and `contracts/openapi.json` is committed.

| Method and path | gRPC |
|---|---|
| `GET /github/account` | Knowledge.GetGithubAccount |
| `GET /github/repos?query=` | Knowledge.ListGithubRepos |
| `GET /projects`, `POST /projects {repoFullName}`, `GET /projects/{id}`, `DELETE /projects/{id}` | Knowledge.*Project* |
| `PATCH /projects/{id}/mode {mode}` | Knowledge.SetProjectMode |
| `POST /projects/{id}/scans`, `GET /projects/{id}/scans`, `GET /scans/{id}` | Knowledge.*Scan* |
| `GET /projects/{id}/facts?kind=&category=` | Knowledge.ListFacts |
| `GET /projects/{id}/questions?onlyOpen=`, `POST /questions/{id}/answer {answer}` | Knowledge.*Question* |
| `GET /projects/{id}/understanding` | Knowledge.GetUnderstanding |
| `GET /projects/{id}/graph?includeHistory=` | Knowledge.GetGraph |
| `POST /projects/{id}/impact {nodeId?, path?, maxDepth?}` | Knowledge.AnalyzeImpact |
| `GET /projects/{id}/dependencies` | Knowledge.ListDependencies |
| `GET /projects/{id}/pipelines` | Knowledge.ListPipelines |
| `GET /projects/{id}/pipeline-runs?refresh=&limit=` | Knowledge.ListPipelineRuns |
| `GET /projects/{id}/deployments?refresh=&limit=` | Knowledge.ListDeployments |
| `GET /projects/{id}/similar?limit=` | Knowledge.ListSimilarProjects |
| `GET /patterns?category=&includeDeprecated=`, `GET /patterns/{id}` | Knowledge.*Pattern* |
| `GET /knowledge/search?q=&projectId=&kinds=&limit=` | Knowledge.SearchKnowledge |
| `GET /knowledge/stats` | Knowledge.GetKnowledgeStats |
| `GET /proposals?projectId=`, `POST /proposals {projectId, kind, prompt?}`, `GET /proposals/{id}` | Intelligence.*Proposal* |
| `POST /proposals/{id}/accept {optionId?, createExecution?}`, `POST /proposals/{id}/reject {reason}` | Intelligence.Accept/Reject |
| `GET /proposals/{id}/messages`, `POST /proposals/{id}/messages {content}` | Intelligence.ListMessages / Negotiate |
| `GET /decisions?projectId=&includeSuperseded=`, `POST /decisions`, `POST /decisions/{id}/supersede` | Intelligence.*Decision* |
| `GET /projects/{id}/documents`, `POST /projects/{id}/documents {kind}`, `GET /documents/{id}` | Intelligence.*Document* |
| `GET /intelligence/stats` | Intelligence.GetIntelligenceStats |
| `GET /executions?projectId=&limit=`, `POST /executions`, `GET /executions/{id}` | Execution.*Execution* |
| `POST /executions/{id}/start {fault?}`, `POST /executions/{id}/resolve {optionId?, instruction?}`, `POST /executions/{id}/cancel {reason}` | Execution.Start/Resolve/Cancel |
| `GET /executions/{id}/events?afterSeq=&limit=` | Execution.ListEvents |
| `GET /executions/{id}/events/stream?afterSeq=` (SSE, `data: <ExecutionEvent>`, final `event: end`) | Execution.WatchExecution |
| `GET /approvals?status=&executionId=`, `POST /approvals/{id}/decide {approve, note?, decidedBy?}` | Execution.*Approval* |
| `GET /policies`, `PATCH /policies/{actionType} {decision}` | Execution.*Policy* |
| `GET /identities`, `POST /identities/{id}/grants {permissions, ttlMinutes?}` | Execution.*Identity* |
| `GET /delivery/stats?projectId=` | Execution.GetDeliveryStats |
| `GET /system/status` | gateway: gRPC health of each service + versions |
| `GET /health` | gateway liveness |
| `GET /scans/{id}/stream` (SSE, `data: <Scan>`, final `event: end`) | Knowledge.WatchScan |
| `GET /projects/{id}/maturity` | Knowledge.GetMaturity |
| `GET /projects/{id}/config` | gateway: `ProjectContext.config` via Knowledge.GetProjectContext |
| `GET /events/stream` (SSE, `data: <PlatformEvent>`, never ends, keep-alive 15s) | gateway merges Knowledge/Intelligence/Execution.WatchEvents |
| `GET /settings`, `PATCH /settings {knowledge?, intelligence?}` | Knowledge.Get/UpdateSettings + Intelligence.Get/UpdateSettings, composed as `{knowledge, intelligence}` |
| `GET /proposals/{id}/options/{optionId}/artifacts` | gateway: the option's artifacts (omitted from `GET /proposals/{id}` and list responses to keep them small; `GET /proposals/{id}` carries `artifactCount` per option) |
| `POST /proposals/{id}/pull-request {optionId?}` | Intelligence.AcceptProposal(create_execution) with a `github`-adapter plan of `repo.checkout → repo.open_pull_request`; returns the proposal (201) whose `executionId` is that execution, where `outputs.pull_request_url` appears once the approved step ran |
| `POST /executions {…, adapter?, artifacts?}` | Execution.CreateExecution (adapter per §5.4) |

**Live events.** The web app opens `GET /events/stream` once (in the shell)
and invalidates TanStack Query families by `entity`: `project`/`scan` →
`['projects']`, `['scans']`; `proposal`/`decision`/`document` →
`['proposals']`, `['decisions']`, `['documents']`; `execution`/`approval` →
`['executions']`, `['approvals']`; `pattern` → `['patterns']`. Polling stays
only as a fallback while the stream is disconnected.

The gateway converts gRPC status codes: `NOT_FOUND`→404,
`INVALID_ARGUMENT`→400, `ALREADY_EXISTS`→409, `FAILED_PRECONDITION`→412,
`PERMISSION_DENIED`→403, `UNAVAILABLE`→502, `DEADLINE_EXCEEDED`→504,
`RESOURCE_EXHAUSTED`→429, `UNAUTHENTICATED`→401, other→500 with an opaque
`internal` body (the real message is only logged). Mutating routes answer
201 for creates (`POST /projects`, `/proposals`, `/proposals/{id}/messages`,
`/decisions`, `/projects/{id}/documents`, `/executions`) and 200 otherwise.
Required proto timestamps a service leaves unset serialise as
`1970-01-01T00:00:00Z`; optional ones are `null`, as are empty id strings.

## 9. Web app conventions

```
web/src
├── app/            providers, router (lazy routes + Suspense in the shell), layout/app-shell
├── components/ui   shadcn primitives (Button has a `loading` prop that renders the Spinner inline)
├── components/shared  page frame, feedback (EmptyState, ErrorState, QueryBoundary, skeletons), data (Stat, StatusBadge, ScoreBar, …)
├── features/<feature>/
│   ├── api/        keys.ts (query key factory), queries.ts (queryOptions), mutations.ts (useMutation + invalidation)
│   ├── components/<component>/index.tsx (+ sibling parts)   nested, one responsibility each
│   ├── pages/<page>.tsx   default export, lazy-loaded by the router
│   └── lib/        pure helpers (labels, tone maps, formatting)
└── lib/            api client (openapi-fetch typed by schema.d.ts), query client, utils
```

- Page data: `useSuspenseQuery(queryOptions)` inside the shell's
  `QueryBoundary`; live data: `useQuery` with `refetchInterval` or SSE.
- Mutations: `useMutation` → invalidate the feature key family; buttons use
  `loading={mutation.isPending}`.
- Status → `StatusBadge` tone maps live in `features/<feature>/lib/labels.ts`.
- Design memory: `.ui-craft/brief.md` and `.ui-craft/tokens.md` apply to every
  surface. Dashboard recipe: Command composition, operator persona.

## 10. nginx (deploy/nginx)

- Serves `web/dist` with long-cache hashed assets and SPA fallback.
- Reverse proxies `/api/` to `upstream gateway { gateway-1:8080; gateway-2:8080; }`
  (least_conn, health-aware via `max_fails`), forwarding `X-Request-Id`.
- Rate limits: `limit_req_zone … zone=api rate=20r/s` with `burst=40 nodelay`
  on `/api/`, a stricter `zone=mutations rate=5r/s` for POST/PATCH/DELETE,
  `limit_conn` 32 per address; 429 returns the JSON error body
  (`{"code":"rate_limited","message":"Too many requests"}`). When no replica
  answers, the edge itself answers JSON `502 {"code":"upstream_unavailable"}`
  or `504 {"code":"upstream_timeout"}`; gateway-generated bodies pass through
  unchanged.
- SSE routes `/api/v1/executions/*/events/stream`, `/api/v1/scans/*/stream`
  and `/api/v1/events/stream`: `proxy_buffering off`, `proxy_read_timeout 1h`,
  `Cache-Control: no-cache`, excluded from the request rate limit (one
  long-lived connection per tab).
- Security headers, gzip, `server_tokens off`.

## 11. Definition of done (every slice)

- `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`,
  `cargo test --workspace` pass.
- `pnpm typecheck`, `pnpm lint`, `pnpm build` pass in `web/`.
- `docker compose up --build` serves the app on http://localhost and
  `curl localhost/api/v1/health` returns `{"status":"ok"}`.
- A real private GitHub repository can be connected, scanned, proposed
  against, negotiated with, and executed as a dry run from the browser.
- `pnpm e2e` (Playwright, against `make up`) passes the critical path:
  connect → scan (live) → proposal → negotiate → accept → dry run with an
  injected fault → resolve → succeeded.

## 12. Repository configuration — `.sdi.yml`

Optional file at the repository root. Values are **configured facts**
(`FACT_KIND_CONFIGURED`, confidence 1.0, `source_path=.sdi.yml`) and answer
the matching open questions automatically (`answered_by=config`). Unknown
keys are reported in `ProjectConfig.errors` and ignored; a file that fails to
parse is ignored entirely and reported.

```yaml
version: 1                      # required
deploy:
  target: vm | azure | aws | gcp | kubernetes | fly | render | none   # answers deployment_target
  environments: [staging, production]                                 # answers environments
  region: westeurope
budget:
  monthly_usd: 20                 # becomes the budget constraint on every proposal
test:
  command: "cargo test"           # used by the docker adapter's tests.run_local
  health_path: "/health"          # polled by container.run
  port: 8080
scan:
  exclude: ["docs/legacy/**", "vendor/**"]   # glob, applied to wanted_files
review:                           # tunes pull request reviews (§20)
  ignore_paths: ["gen/**", "vendor/**"]      # no rule reads these files
  require_tests_for: ["src/billing/**"]      # a change here without a test change is `tests.required` (high), test framework or not
  severity:                                  # overrides by rule id
    correctness.todo: high
    docker.latest_tag: info
  rules:                                     # repository rules over added lines → `custom.<id>`
    - id: no-moment                          # [a-z0-9_-], unique
      pattern: "from ['\"]moment['\"]"       # regex; an invalid one makes the file invalid
      severity: medium                       # info | low | medium | high | critical
      message: Use date-fns instead of moment
      paths: ["web/**"]                      # globs; all files when omitted
```

Globs are the `scan.exclude` grammar (`*` within a segment, `**` across,
a bare directory name means everything below it). The `review:` section
is read by the review pipeline from the repository's `.sdi.yml` at the
scanned commit (through `GetRepoFile`) and parsed by the same analyzer
parser, so a file that is invalid for the scan is invalid for the review
too; the scan records one configured fact `config:review.rules`
summarising it.

Everything that is *not* a configuration value stays a generated document
(README, architecture, deployment guide, runbook, ADR, UAT plan, reports) in
Markdown, downloadable from the Documents page and committable through a
`repo.open_pull_request` step.

## 13. Delivery maturity (knowledge)

Eight axes, each 0–100, from facts only (never from guesses):

| axis | 100 means | typical findings |
|---|---|---|
| `containerization` | Dockerfile + compose or k8s, non-root user, healthcheck | "No Dockerfile", "Runs as root" |
| `ci` | CI with test + build + a deploy or release job | "No workflows", "CI has no test step" |
| `tests` | unit + e2e suites, test command detectable | "No test framework detected" |
| `iac` | IaC present with remote state / modules | "Infrastructure is not declared" |
| `dependencies` | lockfile + Dependabot + no EOL major pins | "No lockfile", "Dependabot not configured" |
| `docs` | README + architecture/ADR docs | "README missing", "No architecture document" |
| `secrets` | `.env.example`, no committed secret-looking files, secret scanning | "`.env` committed" |
| `runtime` | every detected runtime within support | "Node 16 reached end of life 2023-09-11" |

`overall` is the weighted mean (containerization 0.15, ci 0.2, tests 0.15,
iac 0.1, dependencies 0.15, docs 0.05, secrets 0.1, runtime 0.1); `stage`
is `legacy` < 35, `transitional` 35–69, `modern` ≥ 70. Runtime currency uses a
curated EOL table (`crates/analyzer/src/eol.rs`: Node, Python, Java, .NET,
PHP, Ruby, Go, Rust edition, Ubuntu/Debian/Alpine base images) judged
against the date the knowledge service passes to `analyze_at` (the
analyzer has no default date), read from
`.nvmrc`, `.node-version`, `.tool-versions`, `engines`, `runtime.txt`,
`pyproject requires-python`, `pom maven.compiler`, `<TargetFramework>`,
`go.mod go`, `Cargo.toml edition`, Dockerfile base images.

## 14. Generators (intelligence)

`intelligence/src/generators/` turns a pattern + `ProjectContext` into
`Artifact`s. One module per family; each is a pure function unit-tested with
fixtures, and every file is complete (no `TODO` placeholders):

| family | files | inputs it needs |
|---|---|---|
| `dockerfile` | `Dockerfile`, `.dockerignore` | language/framework/package manager, entry point, port, runtime version |
| `compose` | `docker-compose.yml` | services, datastores, ports, env example |
| `github_actions` | `.github/workflows/ci.yml`, `deploy.yml` | package manager, test command, image name, deploy target; in a monorepo one `ci.yml` job per workspace package with that package's toolchain (a Cargo or pnpm workspace with a root manifest counts as one package tested from the root) |
| `terraform` | `infra/<cloud>/{main,variables,outputs}.tf` | cloud, region, service name, database engine |
| `bicep` | `infra/azure/main.bicep` | same as terraform for Azure |
| `kubernetes` | `deploy/k8s/{deployment,service,ingress}.yaml` or `charts/<name>/` | image, port, health path, replicas |
| `nginx` | `deploy/nginx/default.conf` | upstream port, static dir |
| `dependabot` | `.github/dependabot.yml` | ecosystems found, one entry per package directory of each ecosystem |
| `env` | `.env.example` | env vars referenced in code/compose |
| `docs` | `docs/deployment.md`, `docs/runbook.md` | the plan and its policy gates |

Generated content uses the project's own names (repo name, service names,
detected port) and the negotiated parameters (region, runtime). Files that
already exist in the repository are generated with `replaces_existing=true`
and the PR step writes them only when the option says so.

## 15. Code browser (knowledge)

`GetRepoTree` returns every entry of the repository at the scanned commit
(from the GitHub recursive tree, cached per `(project, commit)` in the
`repo_trees` table; `truncated` when GitHub truncated it). `GetRepoFile`
returns one file's text (contents API, raw media type, cached in
`repo_files` by `(project, commit, path)`; 1 MiB limit → `truncated`; binary
detection by NUL bytes / known extensions → `binary=true`, empty content;
`language` from the extension table in `crates/analyzer`). Paths are
validated (no `..`, no leading `/`). Secret-looking files (`.env`,
`*.pem`, `*.key`, `id_rsa*`) are refused with `PERMISSION_DENIED` — the
platform never displays secrets even when the repository contains them.

What a cache miss leaves behind follows the workspace's data controls
(§19, read from `Identity.GetDataControls`, cached 60 s): `code_cache=none`
fetches and answers without writing `repo_files`; `session` writes the row
with `expires_at = now + code_cache_ttl_hours` (default 24 h) — reads treat
an expired row as a miss and a background sweeper deletes expired rows
every 10 minutes; `persistent` writes it without expiry. Trees are always
cached (the browser needs them). Every `GetRepoFile` is audited through
`Identity.RecordAudit` (`action=file.read`, `entity=file`, `entity_id=<path>`,
fire-and-forget). `html_url` derives from the project's `repo_url`, so GHES
links work.

Routes: `GET /projects/{id}/tree`, `GET /projects/{id}/file?path=<path>`.
Web: `/projects/:id/code` and `/projects/:id/code/*path` — a GitHub-like
tree (directories first, lazy-expanded, keyboard navigable) and a file view
(line numbers, mono, wrap off, language caption, "Open on GitHub", facts
detected from this file listed above the code). Diagram nodes and facts link
here by path.

## 16. Diagrams (intelligence)

A `Diagram` is a `DiagramSpec` (groups, nodes, edges — the single source of
truth for rendering) plus the Mermaid rendered from it. Generation is
**evidence-based like gitdiagram, deterministic by default**: the
knowledge graph (nodes, valid edges, facts), the repo tree (`GetRepoTree`)
and a bounded sample of key files via `GetRepoFile` (up to 60 files of
24 KB: entry points, routers, module hubs, manifests and large sources
ranked by structure, topped up with the files those import most; tests,
fixtures, generated, vendored and CI files excluded; `db_schema` reads every
migration and model file, up to 200) produce the spec; nodes carry the
repository `path` that implements them and edges carry `evidence_path`
(the file that shows the call) or are `dashed` when inferred. Imports are
parsed per language (Rust `mod`/`use`, TS/JS `import`/`require`, Python
`from`/`import`, Go packages, Java/C# packages) into a file-level import
graph; files collapse into components (a directory with ≥ 3 source files,
or a single entry point / file imported by ≥ 3 others / file over 8 KB,
captioned with its file name) and edges follow the importer → imported
direction with verbs read from the importing file (`tokio::spawn` /
`Command::new` → "starts"/"runs", HTTP clients → "calls", store writes →
"records", reads → "reads", channels/SSE → "streams", config parsing →
"loads", validation → "validates", else "uses"). When the workspace's reasoner is
on (§19), the LLM writes the `explanation` (reply capped at 250 tokens,
JSON mode where the provider has one) and may rename labels / regroup
nodes but can only *remove* edges, never add unsupported ones. Limits: 34 nodes, 48 edges, 3–6 groups (split into overview + detail
beyond that). `architecture` targets 18–24 nodes and ≤ 32 edges for
readability: single-file components fold into their directory (or package
root) unless they are entry points, hubs imported by ≥ 4 sampled files or
the only component of their role; parallel edges between two nodes merge
into the one with the strongest verb; generic "uses" edges inside a group
that already shows ≥ 6 internal edges are dropped; actors, entry points,
every datastore and every external system always stay. 34/48 remain the
hard cap.

Kinds and what they read:

| kind | built from |
|---|---|
| `architecture` | packages, module components and significant files from the import graph, datastores, external APIs and the hosts the code calls; actors ("User"/"Operator", "CI"); groups = the roles bootstrap / execution / integrations / monitoring / state when the code names ≥ 3 of them, else top-level directories |
| `high_level` | actors (users, CI, GitHub) → product → external systems; ≤ 9 nodes |
| `data_flow` | the architecture map reduced to its data and async edges: datastores, queues and the components (packages, store modules) that read, write or publish |
| `deployment` | environments, container images, infrastructure nodes, pipelines (`DEPLOYED_TO`, `PROVISIONS`, `RELEASES`) |
| `dependency` | packages by ecosystem with `DEPENDS_ON` fan-in |
| `sequence` | the principal workflow as ordered messages: participants in call order (actor, interface, entry point, the components the import graph reaches from it to depth 6, the store, its database); `edge.id` is `m01`, `m02`, … in message order, style `call` for requests and `returns` (dashed) for the replies, labels 1–3 words naming the message ("POST /runs", "load workflow", "spawn runner", "insert rows", "200 OK") |
| `module_map` | directories/packages as a tree from the repo tree |
| `db_schema` | every table with its columns after applying all migrations in path order (`CREATE TABLE`, `ALTER TABLE ADD/DROP/RENAME COLUMN`, `RENAME TO`, `DROP TABLE`) plus ORM models (sqlx/diesel/sea-orm/prisma/SQLAlchemy/Django/TypeORM/ActiveRecord); one `datastore` node per table (label = table name, caption "N columns", description one column per line as `name type [PK] [FK → table.column] [NOT NULL] [UNIQUE]`), one edge per foreign key child → parent labelled "references" with `evidence_path` = the migration that declared it; grouped by schema when there is more than one |
| `custom` | the prompt (reasoner) or imported Mermaid |

Mermaid output: `flowchart LR|TB` with `subgraph` per group, node shapes
by kind (`[(db)]` for datastores, `([actor])`, `[[infra]]`, `>external]`),
`-.->` for dashed edges, `click <id> "/projects/<id>/code/<path>"` for nodes
with a path, and `%% evidence: <path>` comments per edge. `db_schema` is
rendered as `erDiagram` instead: one entity per table with typed
attributes and `PK`/`FK`/`UK` markers and one `child }o--|| parent : fk_column`
relationship per foreign key. `sequence` is rendered as `sequenceDiagram`:
`participant` lines in call order, `->>` for calls, `-->>` for returns and
a `Note over` naming the workflow. `UpdateDiagram`
with `mermaid` makes the Mermaid source authoritative (`source="mermaid"`);
the spec is kept for the editorial renderer but marked stale in the UI.

Routes: `GET /projects/{id}/diagrams`, `POST /projects/{id}/diagrams {kind, prompt?}`,
`GET /diagrams/{id}`, `PATCH /diagrams/{id} {title?, spec?, mermaid?}`,
`DELETE /diagrams/{id}`.

## 17. Editorial diagram renderer (web)

`web/src/features/diagrams/components/editorial-canvas` renders a
`DiagramSpec` as inline SVG on the plain app background, colourful and
shape-coded like gitdiagram's Mermaid output but with the project's own
tokens: hairline strokes, no shadows or gradients, Geist for labels and
Geist Mono for file names and captions. The diagram kind picks the
renderer:

- **Shaped graph** (every kind but `db_schema` and `sequence`): the shape
  says the kind — actor a 64 px circle, service/module/job a 6 px-radius
  rectangle, ui a 12 px-radius rectangle, datastore a cylinder, external a
  stadium, queue a hexagon, infra a rectangle with doubled sides — with the
  label (≤ 2 lines, 13 px) and a mono line (`[main.rs]` for code nodes,
  the caption for data/external ones); nodes are ≥ 150 px wide and grow
  with their text. The tint says the group: six hues (`--diagram-1..6`
  and `--diagram-N-fg` in `web/src/index.css`, fills at
  `--diagram-fill-alpha`, borders at `--diagram-stroke-alpha`, `.dark`
  overrides) assigned to groups in order; actors are brand-tinted,
  datastores always rose, externals always green. Groups are light
  rectangles labelled top-centre (12 px medium, 16 px padding). Edges are
  foreground/60 with open arrowheads, dashed when inferred or `returns`,
  labels plain 12 px text on a background-coloured halo, nudged in 14 px
  steps off other labels, nodes and group titles. Layout is top-down
  (`TB`) for every graph kind except `dependency` (`LR`) and `custom`
  (the spec's `direction`), because the builders currently write the
  model default `LR` rather than a choice. `TB` is a stacked band layout
  (`graph/bands.ts`): actors in a top band, then the groups in role order
  (bootstrap/configuration → execution → integrations → monitoring/UI →
  state/storage, matched on id and label; otherwise the topological order
  of the edges between groups; unmatched groups sit between execution and
  integrations), each group's nodes ordered by a dagre pass over its own
  edges and wrapped into rows of at most four (≤ 900 px), datastores on
  the last row; consecutive single-row groups share a band while they fit
  in 960 px, so the drawing is at most ~1100 px wide and grows down.
  Edges are routed orthogonally (`graph/ortho.ts`): out of the node's top
  or bottom, along the channel between rows, through a corridor between
  nodes or outside the drawing when rows must be crossed, into the
  target's top or bottom, never through a node; runs sharing a channel
  or corridor take 10 px lanes. `LR` keeps a compound `@dagrejs/dagre`
  layout (ranksep 70, nodesep 40, edgesep 20) with dagre's edge points as
  rounded polylines. The pane fits the drawing to its width (never past
  1:1) and scrolls vertically; the pane is the viewport minus the chrome,
  at least 640 px.
- **ER** (`db_schema`): a card per table — mono name in a tinted header,
  a row per column parsed from `node.description` (one per line,
  `name type [PK] [FK → table.col] [NOT NULL] [UNIQUE]`; a comma list
  of names and `(+N more)` are read too; "N columns" from the caption
  when empty) with PK/FK badges and the type at the right; FK edges child
  → parent with a crow's foot at the child and a bar at the parent,
  labelled with the FK column; dagre `LR`, ranksep 90.
- **Sequence** (`sequence`): participants in spec order across the top
  (actor as a head over its box, 180 px columns), dashed lifelines,
  messages in `edge.id` order (`m01`, `m02`…) top-down at 36 px steps —
  `call` solid with a filled head, `returns` dashed with an open head,
  label above, self-messages as a loop — and an activation bar on the
  callee from a call to the return that answers it.

Shared by all three: a legend strip under the drawing (shapes and line
styles in use, cardinality ends and key badges for ER, call/return/
activation for sequence), hover highlights a node's edges and neighbours,
click (or Enter on a focused node) opens the code browser at `node.path`,
⌘-scroll zoom and drag pan, fit-to-view, light/dark via tokens. Export:
SVG (serialised with the tokens resolved to the current theme) and PNG
(2× canvas). The Mermaid tab renders the diagram's `mermaid` with
`mermaid` (lazy chunk; its `themeVariables` take the same palette —
primary/secondary/tertiary are the first three tints over the
background) and offers an editor with live preview, syntax errors shown
inline, and Save (`PATCH … {mermaid}`).

## 18. Identity, workspaces and tenancy

Identity is GitHub. There are no SDI passwords, no separate employee
directory and no way to grant someone more than GitHub already grants them.

- **Sign-in**: GitHub OAuth (`read:user user:email`; for the personal
  workspace also `repo` read via the OAuth grant). A session is an opaque
  token in an `httpOnly; Secure; SameSite=Lax` cookie `sdi_session`, 30
  days, revocable.
- **Organisations**: install the **SDI GitHub App** (permissions:
  `contents:read`, `metadata:read`, `actions:read`, `deployments:read`,
  `pull_requests:write` (only used by the `github` adapter), `members:read`;
  events `installation`, `installation_repositories`, `push`,
  `pull_request`, `member`, `team`). One installation = one workspace of
  kind `organization`. GitHub Enterprise Server works through a
  per-installation `api_base_url`.
- **Projects** come only from the installation's repository selection
  (`ListGithubRepos` lists that set); personal workspaces list the user's
  own repositories.
- **Membership** is synced from GitHub org membership and teams
  (`SyncMembers`, webhooks `member`/`team`, and lazily when a user opens a
  workspace they are not yet a member of): org `owner` → `OWNER`; a member
  of a team named `sdi-admins` → `ADMIN`; `sdi-approvers` → `APPROVER`;
  other org members → `MEMBER`; outside collaborators → `VIEWER`. An admin
  may set `override_role` **lower** than the derived role, never higher.
  Losing org membership removes access at the next request.
- **Authorisation**: the gateway resolves the cookie + the `X-Sdi-Workspace`
  header (or `?workspace=`) through `Identity.Authorize` (cached 60 s) and
  forwards `x-sdi-user`, `x-sdi-workspace`, `x-sdi-role` as gRPC metadata.
  Every service reads them with a tonic interceptor into a `Principal`
  extension; **every repository function takes `workspace_id`** and every
  tenant table has a `workspace_id uuid not null` column with an index;
  cross-workspace reads are impossible by construction (ids alone never
  address a row). Role checks: viewer = GET only; member = connect/scan/
  propose/negotiate/documents/diagrams/dry-run executions; approver =
  decide approvals, docker/github executions, grants; admin = policies,
  settings, members, data controls; owner = delete workspace.
- **Service-to-service** calls (intelligence → knowledge/execution,
  execution → knowledge) forward the same metadata unchanged. Internal
  jobs (webhook-triggered rescans) run with `x-sdi-user: system`.
- **Knowledge tenancy** (migration `0004_tenancy`): `x-sdi-workspace` must
  be a uuid (anything else is `UNAUTHENTICATED`). Rows that existed before
  tenancy belong to the **bootstrap workspace
  `00000000-0000-7000-8000-00000000dead`**; dev mode's personal workspace
  maps to it. `projects` are unique per `(workspace_id, repo_full_name)`,
  so one repository may be connected in a personal and in an organisation
  workspace. Catalog patterns are global (`workspace_id IS NULL`,
  `scope=public_template`); learned patterns carry the workspace and are
  ids `learned:<workspace>:<key>` (the bootstrap workspace keeps its
  pre-tenancy `learned:<key>` rows). `ListPatterns`/`GetPattern` return the
  catalog plus the caller's learned patterns, and usage roll-ups
  (`used_by`, deployments, success rate) only count the caller's projects.
  Settings are per workspace, seeded lazily with the defaults on first
  read. `SearchKnowledge`, `ListSimilarProjects`, `GetKnowledgeStats` and
  `WatchEvents` are workspace-scoped; every `PlatformEvent` carries
  `data["workspace_id"]`. Knowledge role checks: reads VIEWER;
  `ConnectProject`, `StartScan`, `AnswerQuestion`, `SetProjectMode`,
  `DeleteProject`, `RecordPatternOutcome`, `IndexKnowledge` MEMBER;
  `UpdateSettings`, `PurgeCodeCache`, `ExportWorkspaceData` ADMIN;
  `DeleteWorkspaceData` OWNER. A project of another workspace is
  `NOT_FOUND`, never `PERMISSION_DENIED`.
- **Intelligence tenancy** (migration `0004_workspaces`): `proposals`,
  `messages`, `decisions`, `documents`, `diagrams` and `settings` carry
  `workspace_id` (pre-tenancy rows → bootstrap workspace); settings are one
  row per workspace (the old single row is the bootstrap workspace's), read
  with defaults when missing. Role checks: reads VIEWER; `GenerateProposal`,
  `Negotiate`, `AcceptProposal`, `RejectProposal`, `GenerateDocument`,
  `GenerateDiagram`, `UpdateDiagram`, `DeleteDiagram`, `CreateDecision`,
  `SupersedeDecision` MEMBER; `UpdateSettings`, `ExportWorkspaceData` ADMIN;
  `DeleteWorkspaceData` OWNER. Outgoing knowledge/execution calls forward
  the caller's principal; a decision's `approved_by` defaults to the caller.
  **BYOK reasoner**: per call, intelligence resolves the reasoner from
  `Identity.GetDataControls` (`reasoner` must not be `off`) and the
  workspace's own key from the internal `Identity.GetReasonerKey`
  (`workspace_id → api_key, model`; `NOT_FOUND` = no key; never exposed
  through the gateway, never logged). `LLM_API_KEY` (alias
  `ANTHROPIC_API_KEY`) is the dev fallback only when the workspace allows
  the reasoner and holds no key; with the controls `off` (default) even an
  environment key does nothing. An OpenAI-compatible endpoint (§19) needs
  no key. The model is the settings row's, else the key's, else
  `LLM_MODEL`/`ANTHROPIC_MODEL`. `IntelligenceSettings.api_key_configured`
  means "this workspace can reach its provider". When identity is
  unreachable the reasoner stays off.
- **Execution tenancy** (migration `0003_workspaces`): every table carries
  `workspace_id` (pre-tenancy rows → bootstrap workspace); `executions`
  also store `created_by` (`x-sdi-user`). **Policies and identities are
  per workspace**: on a workspace's first RPC the §5.2 table and the §5.3
  identities (`simulated`, `local-docker`, and `github-token` while
  `GITHUB_TOKEN` is set) are seeded for it (`PRIMARY KEY (workspace_id,
  action_type)`, `UNIQUE (workspace_id, name)`; identity ids are fresh
  uuids outside bootstrap). Role checks: reads VIEWER; `CreateExecution`,
  `StartExecution`, `ResolveFailure`, `CancelExecution` MEMBER for the
  `simulated` adapter and APPROVER for `docker`/`github`; `DecideApproval`
  and `GrantPermission` APPROVER (`decided_by` defaults to the caller);
  `UpdatePolicy`, `ExportWorkspaceData` ADMIN; `DeleteWorkspaceData` OWNER.
  An execution of another workspace is `NOT_FOUND`. Runner tasks are
  background work: they continue as `Principal::system(workspace)` (the
  `RecordPatternOutcome` call carries `x-sdi-user: system`). Every
  `PlatformEvent` carries `data["workspace_id"]` and `WatchEvents` is
  filtered to the caller's workspace. Startup recovery resumes executions
  across workspaces; `GITHUB_TOKEN` (re)seeds or withdraws `github-token`
  in every workspace that has identities.
- **GitHub access in knowledge**: there is no process-wide token. Per
  workspace, knowledge calls `Identity.GetInstallationToken` (installation
  token for an organisation; the user's OAuth token for a personal
  workspace, both with their host's `api_base_url`) and
  `Identity.GetWorkspace` (to know whether the token is an installation),
  caches the result in memory until 5 minutes before `expires_at`, and
  keys its GitHub ETag cache by workspace. `ListGithubRepos` lists
  `/installation/repositories` for an installation and `/user/repos` for a
  user token; `GetGithubAccount` describes the installation (slug, grants)
  or the user. When identity is unreachable and `GITHUB_TOKEN` is set, that
  token is used for every workspace (dev mode, warned once); otherwise
  GitHub-backed RPCs answer `FAILED_PRECONDITION`.
- Routes: `GET /auth/login?returnTo=` (302 to GitHub), `GET /auth/callback`
  (sets the cookie, 302 to `returnTo`), `POST /auth/logout`, `GET /auth/me`
  (`{user, workspaces}` or 401), `GET /workspaces`, `GET /workspaces/{id}`,
  `GET /workspaces/{id}/members`, `PATCH /workspaces/{id}/members/{userId}
  {overrideRole}`, `POST /workspaces/{id}/members/sync`,
  `GET /github/app/install-url`, `GET /github/app/setup?installation_id=`
  (post-install redirect), `POST /webhooks/github` (HMAC `X-Hub-Signature-256`
  verified, no session), `GET /workspaces/{id}/audit?actionPrefix=&before=&limit=`,
  `GET/PATCH /workspaces/{id}/data-controls`, `PUT /workspaces/{id}/data-controls/reasoner-key {apiKey}`,
  `POST /workspaces/{id}/data/export` (JSON archive of everything stored
  for the workspace), `POST /workspaces/{id}/data/purge-code-cache`,
  `DELETE /workspaces/{id}` (owner; everything).
  All existing `/api/v1/*` routes require a session and a workspace; the
  SSE streams are filtered to the workspace.
- Environment: `GITHUB_OAUTH_CLIENT_ID`, `GITHUB_OAUTH_CLIENT_SECRET`,
  `GITHUB_APP_ID`, `GITHUB_APP_PRIVATE_KEY` (PEM, base64 allowed),
  `GITHUB_APP_SLUG`, `GITHUB_WEBHOOK_SECRET`, `SDI_SESSION_SECRET` (32+ bytes,
  signs session tokens and encrypts stored keys), `SDI_PUBLIC_URL`,
  `IDENTITY_GRPC_URL` (`http://127.0.0.1:50054`). **Dev mode**: when the
  OAuth variables are absent and `SDI_DEV_LOGIN=1`, `GET /auth/login`
  creates a local session for the `GITHUB_TOKEN` account with a personal
  workspace, so `make dev` keeps working without registering an app.

## 19. Trust: what SDI stores and never does

The product's claim is "we read your repository to help you deliver it;
we do not keep your code, train on it or move it anywhere you did not
choose". The architecture has to make that checkable:

| What | Where | Retention | Can be turned off |
|---|---|---|---|
| Facts, graph, understanding, maturity, dependencies, pipeline/deployment metadata | `knowledge` schema, per workspace | until the project is disconnected | no (this is the product) |
| Semantic chunks + embeddings | `knowledge.chunks` | until disconnect | `minimal_chunks` keeps ≤ 400-char excerpts |
| Repository tree | `knowledge.repo_trees` | latest commit only | no (needed for the code browser) |
| File contents (code browser, diagram inputs) | `knowledge.repo_files` | `code_cache`: `none` (never stored) / `session` (default, TTL 24 h, purged on disconnect) / `persistent` | yes |
| Generated artifacts, proposals, decisions, documents, diagrams | `intelligence` schema | until deleted | n/a |
| Execution logs and outputs | `execution` schema | until deleted; workdirs removed after each run | n/a |
| LLM requests | none stored; sent only when the data controls allow the reasoner, to the workspace's configured provider with its own key; payload = structured context (findings, options, facts, diagram spec), never raw files, trimmed to the token budget below | `reasoner_usage` keeps token counts per workspace and day (no content) | yes (default off) |
| OSV lookups (§20) | package coordinates (`ecosystem`, `name`, `version`) of dependencies a pull request adds, sent to `OSV_API_URL`; answers cached in `osv_cache` (shared, not per tenant) | 24 h TTL | `OSV_API_URL=off` |
| Audit entries | `identity.audit` | `audit_retention_days` (default 365) | no |
| GitHub tokens | installation tokens are minted per request and cached ≤ 55 min in memory; OAuth tokens encrypted at rest with `SDI_SESSION_SECRET` | session lifetime | — |

Knowledge RPCs behind the data controls (all take the workspace from the
principal; a `workspace_id` in the body is optional and must match):
`PurgeCodeCache(workspace_id?, project_id?)` → files and trees deleted
(ADMIN, audited `data.purge`); `DeleteWorkspaceData(workspace_id?)` →
rows deleted per table, in one transaction, including learned patterns and
settings (OWNER, audited `data.delete`); `ExportWorkspaceData(workspace_id?)`
→ a stream of `ExportRecord{entity=<table>, record=<row as JSON>}` over
every tenant table, embeddings omitted as derived data (ADMIN, audited
`data.export`). `DeleteProject` purges the project's code caches.
`minimal_chunks` cuts chunk content to 400 characters before it is embedded
and stored (scans and `IndexKnowledge` alike). When identity cannot answer,
the defaults apply (`session`, 24 h, whole chunks) and are cached a minute.

Intelligence and execution expose the same pair (workspace from the
principal; the body's `workspace_id` is required and must match, otherwise
`PERMISSION_DENIED`): `DeleteWorkspaceData(workspace_id)` → `{deleted:
map<entity, rows>}` over every tenant table in one transaction (intelligence:
`settings`, `proposal`, `message`, `decision`, `document`, `diagram`;
execution: `policy`, `identity`, `grant`, `execution`, `step`, `event`,
`approval`, plus `cancelled`: execution first cancels the workspace's live
runs through their runners and removes every real run's workdir and
containers, and forgets the seed so the next use re-seeds);
`ExportWorkspaceData(workspace_id)` → a stream of
`WorkspaceDataRecord{entity, json}` with every row encoded by Postgres
(`row_to_json`, read through a cursor, parents before children). Neither
RPC writes an audit entry of its own: the `data.delete`/`data.export`
entry belongs to the workspace-level request that fans out to the services.
Audit entries written by the services themselves (`Identity.RecordAudit`,
fire-and-forget with the caller's principal, never blocking the action):
intelligence `proposal.accept` (entity `proposal`, detail: option and
execution) and `decision.create` (every decision: manual, negotiated,
accepted); execution `execution.start`, `execution.resolve` (entity
`execution`), `approval.decide` (entity `approval`, detail starts with
`approved`/`rejected`), `policy.update` (entity `policy`, entity_id =
action type, detail `OLD -> NEW`), `identity.grant` (entity `identity`).

**Any LLM, token-efficient.** The reasoner is provider-agnostic
(`intelligence/src/reasoner/provider.rs`): `Provider::Anthropic` (Messages
API) or `Provider::OpenAiCompatible { base_url }` (chat completions:
OpenAI, Azure OpenAI with the deployment URL and `?api-version=`, Ollama,
vLLM, OpenRouter, Gemini's OpenAI endpoint), chosen per workspace by
`IntelligenceSettings.reasoner_provider` / `reasoner_base_url` /
`reasoner_model` (the same value as `model`), with `LLM_PROVIDER`,
`LLM_BASE_URL`, `LLM_API_KEY`, `LLM_MODEL` as environment fallbacks.
Every call is one `ReasonerRequest { system, context_json, instruction,
max_tokens, json }`: the context is trimmed deterministically to ≤ 6 000
estimated tokens (4 characters per token) by key priority — findings and
options first, then effects/decisions, facts, areas, and the diagram or
graph last; a cut key is shortened (arrays lose their tail) and listed
under `_truncated` — and replies are capped (review summary 600,
negotiation reply 400, diagram explanation 250). OpenAI-compatible calls
ask for JSON mode (`response_format: json_object`) when the reply must be
JSON. 429 and 5xx are retried three times with exponential backoff; every
attempt has a 20 s timeout. Each workspace has a daily token ceiling
(`reasoner_daily_token_cap`, default 200 000, UTC days): a call whose
estimated input plus reply cap would cross it is not made, and actual
usage (the provider's counts, else the estimate) is recorded in
`reasoner_usage` and shown in `IntelligenceStats.reasoner_tokens_today`,
`reasoner_daily_token_cap`, `reasoner_calls_today`. Any failure, the cap
included, falls back to the templated text. The stored reasoner name is
`claude` for Anthropic, `llm` for an OpenAI-compatible endpoint,
`heuristic` otherwise. Both protocols are unit-tested against a fake HTTP
server (request shape, auth header, budget trimming, cap enforcement,
retries).

Guarantees enforced in code: secrets-looking paths are never read (§15);
no third-party telemetry; exports are complete (`data/export`) and deletes
are real (`DELETE /workspaces/{id}` cascades every schema and is audited);
self-hosting is a first-class path (`make up`) so an organisation can run
SDI inside its own network. `TRUST.md` and `PRIVACY.md` at the repository
root state this in plain language and point at the tables above; the
product shows the same table on the Data & privacy page.

## 20. Code review (pull requests)

SDI reviews a pull request the same way it proposes: evidence first, a
human decides, then it acts. A review is a four-step pipeline that the UI
shows as a rail, like executions:

| step | what happens |
|---|---|
| `context` | knowledge fetches the PR (`GetPullRequest`: files, bounded unified diff ≤ 300 KB / 200 files, touched facts, impacted graph nodes via `AnalyzeImpact` on each changed path) |
| `analyze` | deterministic analyzers over the diff, each a pure function `fn(&ReviewInput) -> Vec<Finding>` in `intelligence/src/domain/review/rules/`: `secrets` (keys/tokens/private keys in added lines; `config_credential`: password/secret/token/api_key keys with literal values in yaml/json/toml/env/properties files, references like `${VAR}`/`secretKeyRef` and placeholders excluded), `security` (shell injection, `eval`, SQL built by format, disabled TLS verification, `chmod 777`, hard-coded hosts; insecure defaults `debug_enabled`, `cors_wildcard`, `insecure_cookie` outside local/dev/example files), `dependencies` (new deps without lockfile change, EOL runtimes introduced — judged by the analyzer's EOL table against the review date —, version pins loosened, `vulnerable`: see OSV below), `docker` (root user, `latest` tags, secrets in `ENV`, missing healthcheck), `ci` (workflows without permissions block, `pull_request_target` with checkout, unpinned actions), `iac` (public buckets/0.0.0.0/0 ingress, missing tags/state), `tests` (`missing`: source changed without tests when the project has a test framework; `required`: a path matching `.sdi.yml review.require_tests_for` changed without a test change, high), `docs` (README/CONTRACT claims touched vs code), `correctness` (TODO/FIXME added, unwrap/expect/`!` in non-test Rust/TS, swallowed errors, `console.log`; `unhandled_promise`: `.then(` chains without `.catch`/`.finally` and floating `fetch`/`axios`/`save()`-style calls in TS/JS; `unhandled_result`: a dropped `.await;` in Rust, an ignored error return in Go; `large_function`: more than 120 added lines in one function; `duplicate_block`: 6+ meaningful added lines repeated elsewhere in the pull request, reported once per copy pointing at the first), `migrations` (`drop`: DROP TABLE/COLUMN and the ORM equivalents; `alter_type`: a column's type changed in place; `not_null_without_default`: NOT NULL added or set without a default on a table the migration does not itself create), `maturity` (the change moves a maturity axis up or down), `custom` (`.sdi.yml review.rules`, §12), `review.truncated` (info: the pull request was over the limits, naming what was not read). Every finding carries `path`, `line`, `rule`, evidence in `detail` and, when mechanical, a `suggestion` block; findings are deduplicated on `(rule, path, line)`. Diff edge cases every rule family handles through `ReviewInput`: CRLF (`\r` stripped before matching and quoting), binary files and pure renames (no hunks: not reviewable, never asked for tests), whole-file rewrites (counted as a real change however short), truncated patches and file lists over 200 files (skipped and named in `review.truncated`) |
| `findings` | status `proposed`; the human accepts or dismisses each (`DecideFinding`) with a note. **Learnings**: every verdict updates `review_learnings(workspace, project, rule, accepted, dismissed, last_note)` (a changed verdict moves the count, never double-counts); before each analysis the project's rows are loaded into `ReviewInput.learnings`: a rule dismissed ≥ 3 times with no accept is emitted with `severity=info` and `detail` prefixed "Suppressed by project learnings" (the stored signal; the domain `Finding` also carries a `suppressed` flag), unless the finding is `critical`; a rule the project accepted at least once orders one severity step higher (its stored severity is unchanged). Learnings are exported and deleted with the workspace (entity `review_learning`) |
| `summary` | walkthrough Markdown: intent (from title/body/commits), what changed per file group, impacted components (graph), risk, and what to verify — templated from the facts; when the workspace reasoner is on (§19), rewritten in prose by the configured LLM from the structured context (never raw files), reply capped at 600 tokens |
| `publish` | `PublishReview` posts a GitHub PR review (`github` token with `pull_requests:write`) with the summary as the body and one review comment per **accepted** finding (`suggestion` blocks become GitHub suggestions); event `comment` / `request_changes` / `approve` |

**OSV.** Nothing about vulnerabilities is bundled. For every dependency
the pull request adds with an exact version (`package.json`,
`Cargo.toml`, `pyproject.toml`/`Pipfile`/`requirements*.txt`, `go.mod`,
`Gemfile`, `composer.json`; ranges are not looked up) the runner asks
`OSV_API_URL` (`POST /v1/querybatch`, then `GET /v1/vulns/{id}` for at
most 25 advisories) and emits `dependencies.vulnerable` at the line that
adds the package, with the advisory ids, summaries, severities (GHSA word,
else from the CVSS vector) and the first fixed version. Answers are cached
in `osv_cache` per `(ecosystem, name, version)` for 24 h, shared across
workspaces. The check has a 20 s budget and fails soft: no network, a
broken endpoint or a slow one means no finding, never a failed review.

**Configuration.** The repository's `.sdi.yml` `review:` section (§12)
is applied by the rule engine: `ignore_paths` removes files before any
rule runs, `rules` adds `custom.<id>` findings, `severity` overrides a
rule's severity after all rules ran, `require_tests_for` drives
`tests.required`.

Rules: never publish automatically; webhooks `pull_request` (opened,
synchronize) only *start* a review when the workspace setting
`auto_review` is on (default off), and it stops at `proposed`. Reviews are
per `(project, pr_number, head_sha)`; a new push creates a new review and
links the previous one. `ListReviewActivity` is the audit of the review
loop (started, proposed, accepted/dismissed, published, failed) and feeds the
Activity page. Reasoner off → `reasoner=heuristic`.

Routes: `GET /projects/{id}/pull-requests?state=&refresh=&limit=`,
`GET /projects/{id}/pull-requests/{number}`, `POST /projects/{id}/pull-requests/{number}/reviews`,
`GET /reviews?projectId=&limit=`, `GET /reviews/{id}`, `POST /findings/{id}/decide {decision, note?}`,
`POST /reviews/{id}/publish {event?}`, `GET /activity?projectId=&before=&limit=`.
Roles: start/decide MEMBER, publish APPROVER. PlatformEvent entity `review`
(and `finding`) with `data["workspace_id"]`.

## 21. Web performance contract

- Every list endpoint returns summaries only (no bodies, specs, diffs,
  artifacts); details are fetched per route. Lists accept `limit` (default
  50) and cursor `before` where ordered by time.
- The shell loads `/auth/me` and the workspace's `/system/status` only;
  every page fetches its own data with `useSuspenseQuery` and renders
  panels progressively behind their own `QueryBoundary`; nothing is fetched
  for panels below the fold until they mount.
- Live updates come from the single SSE stream; polling is the fallback
  only. `staleTime` ≥ 30 s on everything scoped to a commit (tree, files,
  tokens, diagrams).
- Vite `build.rollupOptions.output.manualChunks`: `react` (react, react-dom,
  react-router), `query` (TanStack), `ui` (radix-ui, cmdk, lucide), `charts`
  (dagre), with `shiki`, `mermaid`, `@xyflow/react`, the editorial canvas
  and the markdown engine as route-level lazy chunks. Budget: initial JS
  ≤ 220 kB gzip; no route chunk > 120 kB gzip except the lazily loaded
  engines. `pnpm build` prints the chunk table and CI fails above budget.
- Motion: `motion` (framer-motion successor) only for layout/presence
  transitions (page enter 150 ms fade+4 px rise, list rows 80 ms stagger,
  rail step changes), honouring `prefers-reduced-motion`; no scroll-linked
  animation; all transitions ≤ 250 ms.

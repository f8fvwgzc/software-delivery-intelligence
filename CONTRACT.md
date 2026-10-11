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
  `analyze(&RepoSnapshot) -> Analysis`. No I/O.

## 3. Environment

| Variable | Default | Used by |
|---|---|---|
| `DATABASE_URL` | `postgres://sdi:sdi@127.0.0.1:5440/sdi` | all services |
| `GITHUB_TOKEN` | — | knowledge (read-only; `scripts/dev.sh` exports `gh auth token`) |
| `ANTHROPIC_API_KEY` | — | intelligence (optional; enables the `claude` reasoner) |
| `ANTHROPIC_MODEL` | `claude-sonnet-5-5` | intelligence |
| `KNOWLEDGE_GRPC_URL` | `http://127.0.0.1:50051` | gateway, intelligence, execution |
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
reply cites `Evidence`. If `ANTHROPIC_API_KEY` is set the reply text is
written by Claude from the structured context (model `ANTHROPIC_MODEL`); the
ranking is always deterministic.

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
```

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
PHP, Ruby, Go, Rust edition, Ubuntu/Debian/Alpine base images) read from
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
"loads", validation → "validates", else "uses"). When `ANTHROPIC_API_KEY` is set and the
reasoner is `claude`, the reasoner writes the `explanation` and may rename
labels / regroup nodes but can only *remove* edges, never add unsupported
ones. Limits: 34 nodes, 48 edges, 3–6 groups (split into overview + detail
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
| LLM requests | none stored; sent only when `reasoner=claude` with the workspace's own key; payload = structured context (facts, options, spec), never raw files | — | yes (default off) |
| Audit entries | `identity.audit` | `audit_retention_days` (default 365) | no |
| GitHub tokens | installation tokens are minted per request and cached ≤ 55 min in memory; OAuth tokens encrypted at rest with `SDI_SESSION_SECRET` | session lifetime | — |

Guarantees enforced in code: secrets-looking paths are never read (§15);
no third-party telemetry; exports are complete (`data/export`) and deletes
are real (`DELETE /workspaces/{id}` cascades every schema and is audited);
self-hosting is a first-class path (`make up`) so an organisation can run
SDI inside its own network. `TRUST.md` and `PRIVACY.md` at the repository
root state this in plain language and point at the tables above; the
product shows the same table on the Data & privacy page.

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
| `nginx` | web server, reverse proxy, rate limiter, load balancer | 80 | web static files, gateway replicas |
| `services/gateway` | axum REST + OpenAPI (public API) | 8080 | knowledge, intelligence, execution over gRPC |
| `services/knowledge` | tonic gRPC | 50051 | Postgres (`knowledge` schema), GitHub API |
| `services/intelligence` | tonic gRPC | 50052 | Postgres (`intelligence` schema), knowledge, execution |
| `services/execution` | tonic gRPC | 50053 | Postgres (`execution` schema), knowledge |
| `postgres` | pgvector/pgvector:pg16 | 5440 (dev host), 5432 (docker) | — |

Service-to-service calls are **gRPC only** (`/proto/sdi/*/v1/*.proto`, compiled
by `crates/proto`). The gateway is the only HTTP surface. Services never share
tables: each owns one Postgres schema and reaches the others' data through
gRPC.

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

### 5.3 Permissions (`PlanStep.required_permissions`, identity grants)

`<resource>:<verb>` — e.g. `repository:read`, `container:build`,
`container:run`, `environment:write`, `database:write`, `cloud:write`,
`deploy:staging`, `deploy:production`, `secrets:write`. The seeded
`simulated` identity holds `repository:read`, `container:build`,
`environment:write` only; everything else must be granted (TTL, default 30
minutes) and a missing grant produces a `PERMISSION_DENIED` failure **before**
the step runs.

### 5.4 Adapters

`simulated` (v1, every execution) runs steps as timed dry runs and supports
fault injection. `github`, `docker`, `cloud`, `kubernetes` are reserved names;
the `Adapter` trait in the execution service is the extension point.

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
  (`cloud:*`, `runtime:*`, `iac:*`), 0.0 if it contradicts one, 0.6 if no
  decision applies.
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
policy. Every assistant reply cites `Evidence`. If `ANTHROPIC_API_KEY` is set
the reply text is written by Claude from the structured context (model
`ANTHROPIC_MODEL`); the ranking is always deterministic.

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

The gateway converts gRPC status codes: `NOT_FOUND`→404,
`INVALID_ARGUMENT`→400, `ALREADY_EXISTS`→409, `FAILED_PRECONDITION`→412,
`PERMISSION_DENIED`→403, `UNAVAILABLE`→502, `DEADLINE_EXCEEDED`→504,
other→500. Mutating routes answer 201 for creates and 200 otherwise.

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
  `limit_conn` 32 per address; 429 returns the JSON error body.
- SSE route `/api/v1/executions/*/events/stream`: `proxy_buffering off`,
  `proxy_read_timeout 1h`, `Cache-Control: no-cache`.
- Security headers, gzip, `server_tokens off`.

## 11. Definition of done (every slice)

- `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`,
  `cargo test --workspace` pass.
- `pnpm typecheck`, `pnpm lint`, `pnpm build` pass in `web/`.
- `docker compose up --build` serves the app on http://localhost and
  `curl localhost/api/v1/health` returns `{"status":"ok"}`.
- A real private GitHub repository can be connected, scanned, proposed
  against, negotiated with, and executed as a dry run from the browser.

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
| `services/gateway` | axum REST + OpenAPI (public API) | 8080 | knowledge, intelligence, execution, identity, gitops over gRPC |
| `services/knowledge` | tonic gRPC | 50051 | Postgres (`knowledge` schema), GitHub API |
| `services/intelligence` | tonic gRPC | 50052 | Postgres (`intelligence` schema), knowledge, execution |
| `services/execution` | tonic gRPC | 50053 | Postgres (`execution` schema), knowledge |
| `services/gitops` | Go gRPC, Argo CD's gitops-engine embedded (§24) | 50055 | Postgres (`gitops` schema), identity, knowledge, Kubernetes clusters, GitHub API |
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
- `crates/inspect` — pure image inspection (§22.1): `ImageScan` over
  decompressed layer streams, `rules::apply` with the release and the
  date, `osv::queries` / `osv::add_findings` around the caller's HTTP,
  `Gate`. No network, no clock, no disk.

## 3. Environment

| Variable | Default | Used by |
|---|---|---|
| `DATABASE_URL` | `postgres://sdi:sdi@127.0.0.1:5440/sdi` | all services |
| `GITHUB_TOKEN` | — | knowledge (dev fallback only: used for every workspace while `IDENTITY_GRPC_URL` is unreachable, logged once; `scripts/dev.sh` exports `gh auth token`) |
| `LLM_PROVIDER` | `anthropic` | intelligence (instance-wide reasoner fallback, §19: `anthropic` or `openai_compatible`) |
| `LLM_BASE_URL` | — | intelligence (OpenAI-compatible endpoint: OpenAI, Azure OpenAI with `?api-version=`, Ollama, vLLM, OpenRouter, Gemini) |
| `LLM_API_KEY` / `ANTHROPIC_API_KEY` | — | intelligence (optional dev fallback key; `ANTHROPIC_API_KEY` is the alias) |
| `LLM_MODEL` / `ANTHROPIC_MODEL` | `claude-sonnet-5-5` for `anthropic`, none otherwise | intelligence (`LLM_MODEL` names the model for whichever provider is configured; `ANTHROPIC_MODEL` is the older name and applies to the Anthropic provider only. An OpenAI-compatible provider has no default: until a model is named nothing is called, §19) |
| `OSV_API_URL` | `https://api.osv.dev` | intelligence (§20 dependency vulnerabilities), execution (§22.1 what an inspected image ships); `off` disables both, empty means the default |
| `SDI_INSPECT_MAX_MB` | `2048` | execution: compressed layer bytes one image inspection may download (§22.1); layers past the budget are a limitation, never a silent pass |
| `SDI_REA_BIN` | unset | execution: path of the REA CLI (`rea`, from `rea-agents`); when set, its `inspect-binary-hardening` / `inspect-go-binary` / `inspect-rust-binary` run over at most five application binaries of an inspected image as a second opinion (§22.1) |
| `KNOWLEDGE_GRPC_URL` | `http://127.0.0.1:50051` | gateway, intelligence, execution |
| `GITOPS_GRPC_URL` | `http://127.0.0.1:50055` | gateway, execution (§24) |
| `GITOPS_LISTEN` | `0.0.0.0:50055` | gitops |
| `GITOPS_RECONCILE_SECONDS` | `180` | gitops: seconds between two comparisons of every application; minimum 30, `0` turns the loop and automated sync off |
| `GITOPS_MAX_CLUSTERS` | `64` | gitops: cluster caches held at once |
| `IDENTITY_GRPC_URL` | `http://127.0.0.1:50054` | gateway, knowledge (tokens, data controls, audit), intelligence, execution |
| `INTELLIGENCE_GRPC_URL` | `http://127.0.0.1:50052` | gateway |
| `EXECUTION_GRPC_URL` | `http://127.0.0.1:50053` | gateway, intelligence |
| `GATEWAY_LISTEN` / `KNOWLEDGE_LISTEN` / `INTELLIGENCE_LISTEN` / `EXECUTION_LISTEN` | `0.0.0.0:<port>` | each service |
| `LOG_FORMAT` | text (`json` in containers) | all |
| `RUST_LOG` | `info,sqlx=warn` | all |
| `SDI_RUN_DIR` | `.run` | execution (`docker` adapter workdirs) |
| `SDI_ALLOW_PRIVATE_DEPLOY_TARGETS` | unset | execution: set to `1` to let deployers and health checks reach loopback and private addresses (self-hosted networks, local development); by default those are refused as an SSRF guard |
| `SDI_PR_WATCH_SECONDS` | `60` | intelligence: seconds between two looks at the open pull requests of workspaces with `auto_review` on (§23). Minimum 15; `0` turns the watcher off |
| `AZURE_DEVOPS_URL` | `https://dev.azure.com` | execution (`azure_devops` deployer; Azure DevOps Server) |
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

Delivery evidence adds the capabilities `cd:flux`, `cd:argocd` and
`registry:<ghcr|acr|ecr|gar|dockerhub>`, from the fact prefixes `cd:flux`,
`cd:argocd`, `cd:jenkins`, `cd:azure_pipelines`,
`cd:github_environment:<name>`, `registry:<kind>` and
`ci:build_image:<path>` (value `push` when the workflow publishes the image,
`build` when it only builds it).

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
| `registry.verify_image` | Verify the image exists in the registry | safe | auto | no |
| `artifact.inspect` | Inspect the shipped image (§22.1) | safe | auto | no |
| `deploy.development` | Deploy to development | medium | auto | yes |
| `deploy.verify_health` | Verify the deployment is healthy | safe | auto | no |
| `deploy.record` | Record the deployment on GitHub | safe | auto | yes |
| `deploy.rollback` | Roll back an environment | high | approval | yes |
| `repo.commit_fixes` | Commit review fixes to a pull request branch | medium | approval | yes |
| `repo.verify_mergeable` | Check that a pull request can be merged | safe | auto | no |
| `repo.merge_pull_request` | Merge a pull request | high | approval | yes |
| `repo.delete_branch` | Delete a merged branch | medium | auto | yes |
| `repo.enable_security_updates` | Turn on Dependabot alerts and security updates | medium | approval | yes |
| `gitops.preview` | Preview what a sync would change | safe | auto | no |
| `gitops.sync` | Sync an application to its cluster | high | approval | yes |
| `gitops.rollback` | Roll an application back to an earlier sync | high | approval | yes |
| `gitops.verify_health` | Wait for an application to be healthy | safe | auto | no |

### 5.3 Permissions (`PlanStep.required_permissions`, identity grants)

`<resource>:<verb>` — e.g. `repository:read`, `repository:write`,
`container:build`, `container:run`, `environment:write`, `database:write`,
`cloud:write`, `deploy:staging`, `deploy:production`, `secrets:write`. The
seeded `simulated` identity holds `repository:read`, `container:build`,
`environment:write` only; the seeded `local-docker` identity adds
`container:run`, and the seeded `github-token` identity holds
`repository:read` + `repository:write` (it stands for the workspace's
GitHub access, so it only exists while the workspace has one: the
installed GitHub App, the signed-in account's OAuth grant, or the
instance-wide `GITHUB_TOKEN`; a workspace without any is looked at again
every 5 minutes, and creating a `github` execution there answers
`FAILED_PRECONDITION` naming the three ways to get one); everything else must be granted (TTL, default 30
minutes) and a missing grant produces a `PERMISSION_DENIED` failure **before**
the step runs. `repository:admin` (changing repository settings,
§23) is held by nobody: it is always a timed grant. `cluster:read` (reading an application's
state on a cluster, §24) is held by the seeded `gitops` and `delivery`
identities; `cluster:write` (syncing or rolling back an application) is
held by nobody: always a timed grant. The `gitops` runtime executes as the
seeded `gitops` identity and forwards the approver (else the requester,
else the workspace's system principal) to the gitops service.

### 5.4 Adapters

`CreateExecutionRequest.adapter` picks the runtime for the whole run; each
step's `PlanStep.adapter` names the tool family it needs. The execution
service refuses a run (`INVALID_ARGUMENT`) whose steps need a tool the chosen
runtime cannot provide.

| adapter | what it does | steps it can run |
|---|---|---|
| `simulated` (default) | timed dry run, fault injection, never touches anything | every action type |
| `docker` | real local execution on the host's Docker daemon: `repo.checkout` downloads the GitHub tarball at the scan commit into `.run/<execution>/` and writes `CreateExecutionRequest.artifacts` on top of it (so an option's generated Dockerfile is what gets built), `container.build` runs `docker build -f params.dockerfile params.context` (the application directory's Dockerfile in a monorepo), `container.run` starts the image detached (no `--rm`, so a crashed container keeps its logs for the failure) with the configured port and polls `health_path`, `tests.run_local` runs `test_command` inside the image, or, when the plan names `params.test_image` (a compiled stack whose runtime image has no toolchain), in that image with `params.test_dir` of the checkout mounted at `/src`; `env.destroy_temporary` stops and removes everything, and a cancelled or finished run removes its container and workdir either way; stdout/stderr lines become `LOG` events | `repo.checkout`, `container.build`, `container.run`, `tests.run_local`, `env.create_temporary`, `env.destroy_temporary`, `deps.install` |
| `github` | writes to the connected repository with the configured token: `repo.open_pull_request` creates branch `sdi/<proposal id>` from the default branch, commits `CreateExecutionRequest.artifacts` via the Git Data API, opens the PR and records `outputs.pull_request_url`. Files under `.github/workflows/` need the token's `workflow` scope; without it GitHub answers 404 on the tree and the step fails `PERMISSION_DENIED` naming that scope. `params.branch` names the branch instead when there is no proposal. The pull request operations of §23 run here too | `repo.checkout`, `repo.open_pull_request`, `repo.commit_fixes`, `repo.verify_mergeable`, `repo.merge_pull_request`, `repo.delete_branch`, `repo.enable_security_updates` |
| `gitops` | calls the gitops service (§24): dry-run preview, sync, rollback, health wait of an application on a registered cluster | `gitops.preview`, `gitops.sync`, `gitops.rollback`, `gitops.verify_health` |
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
`DEPENDENCY_CONFLICT`, `IMAGE_NOT_FOUND`, `DEPLOYER_FAILED`,
`DEPLOYER_TIMEOUT`, `GITOPS_CONFLICT`, `HEAD_MOVED`, `MERGE_CONFLICT`,
`CHECKS_FAILING`, `MERGE_BLOCKED`, `RENDER_FAILED`, `CLUSTER_UNREACHABLE`,
`SYNC_FAILED`, `ARTIFACT_FINDINGS` (§22.1), `UNKNOWN`. Each has at least
two remediation options plus the free-text instruction path.

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
Errors: `{ "code": string, "message": string }` with 400/404/409/412/429/501/502/504.
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
| `GET /events/stream` (SSE, `data: <PlatformEvent>`, never ends, keep-alive 15s) | gateway merges Knowledge/Intelligence/Execution/Gitops.WatchEvents |
| `GET /settings`, `PATCH /settings {knowledge?, intelligence?}` | Knowledge.Get/UpdateSettings + Intelligence.Get/UpdateSettings, composed as `{knowledge, intelligence}` |
| `GET /proposals/{id}/options/{optionId}/artifacts` | gateway: the option's artifacts (omitted from `GET /proposals/{id}` and list responses to keep them small; `GET /proposals/{id}` carries `artifactCount` per option) |
| `POST /proposals/{id}/pull-request {optionId?}` | Intelligence.AcceptProposal(create_execution) with a `github`-adapter plan of `repo.checkout → repo.open_pull_request`; returns the proposal (201) whose `executionId` is that execution, where `outputs.pull_request_url` appears once the approved step ran |
| `POST /executions {…, adapter?, artifacts?}` | Execution.CreateExecution (adapter per §5.4) |
| `POST /settings/reasoner/test` | Intelligence.TestReasoner (§19); a misconfiguration is a 200 with a status |
| `POST /findings/{id}/draft-fix` | Intelligence.DraftFix (§23.2) |
| `POST /reviews/{id}/apply-fixes` | Intelligence.ApplyFixes (§23.2); 201 |
| `GET /projects/{id}/pull-requests/{number}/merge-readiness` | Intelligence.GetMergeReadiness (§23.3) |
| `POST /projects/{id}/pull-requests/{number}/merge {method?, deleteBranch?}` | Intelligence.MergePullRequest (§23.3); 201 `{executionId, readiness}` |
| `GET /projects/{id}/security?refresh=` | Intelligence.GetSecurityOverview (§23.4) |
| `POST /projects/{id}/security/dependabot {alerts?, securityUpdates?, configFile?}` | Intelligence.ConfigureDependabot (§23.4); 201 `{executionId}` |

Every SSE stream starts with the comment line `: open` the moment it is
accepted, before any event exists, and repeats `: keep-alive` every 15s.
A proxy that holds a response until its first body byte therefore lets it
through at once, and `EventSource` leaves `CONNECTING` without waiting for
an event. Comments never reach listeners.

**Live events.** The web app opens `GET /events/stream` once (in the shell)
and invalidates TanStack Query families by `entity`: `project`/`scan` →
`['projects']`, `['scans']`; `proposal`/`decision`/`document` →
`['proposals']`, `['decisions']`, `['documents']`; `execution`/`approval` →
`['executions']`, `['approvals']`; `pattern` → `['patterns']`. Polling stays
only as a fallback while the stream is disconnected.

The gateway converts gRPC status codes: `NOT_FOUND`→404,
`INVALID_ARGUMENT`→400, `ALREADY_EXISTS`→409, `FAILED_PRECONDITION`→412,
`PERMISSION_DENIED`→403, `UNAVAILABLE`→502, `DEADLINE_EXCEEDED`→504,
`UNIMPLEMENTED`→501 (the service is an older build than the gateway; the
message says to bring them to the same version),
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
  for continuous delivery (§22) also `actions:write` (workflow dispatch),
  `deployments:write` (record), `packages:read` (GHCR verify) and
  `contents:write` (GitOps commits and setup-file pull requests); for the
  pull request autopilot (§23) also `checks:read` and `statuses:read`
  (merge readiness), `vulnerability_alerts:read` (Dependabot alerts) and,
  only to turn Dependabot on, `administration:write` — a step
  whose permission is missing pauses with `PERMISSION_DENIED`, it never
  falls back to anything broader;
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
  `GET /people?ids=a,b,c` (names for stored actor ids, see below),
  `GET /github/app/install-url`, `GET /github/app/setup?installation_id=`
  (post-install redirect), `POST /webhooks/github` (HMAC `X-Hub-Signature-256`
  verified, no session), `GET /workspaces/{id}/audit?actionPrefix=&before=&limit=`,
  `GET/PATCH /workspaces/{id}/data-controls`, `PUT /workspaces/{id}/data-controls/reasoner-key {apiKey}`,
  `POST /workspaces/{id}/data/export` (JSON archive of everything stored
  for the workspace), `POST /workspaces/{id}/data/purge-code-cache`,
  `DELETE /workspaces/{id}` (owner; everything).
  All existing `/api/v1/*` routes require a session and a workspace; the
  SSE streams are filtered to the workspace.
- **Actors are stored as user ids, shown as people.** `startedBy`,
  `createdBy`, `decidedBy` and `requestedBy` carry the caller's user id (a
  UUID) or, for automation, a plain word such as `system`. Clients never
  print the id: they resolve a page of them with one `GET /people?ids=`
  (comma-separated, at most 100) → `[{id, login, name, avatarUrl, member}]`
  via `Identity.ResolveUsers`. The answer is scoped to the current
  workspace: its members, its owner, and anyone in its own audit trail
  (`member: false`, so history keeps its names after someone leaves). An id
  the workspace may not name is absent, not an error; values that are not
  UUIDs are ignored; the email address is never part of the answer.
- Environment: `GITHUB_OAUTH_CLIENT_ID`, `GITHUB_OAUTH_CLIENT_SECRET`,
  `GITHUB_APP_ID`, `GITHUB_APP_PRIVATE_KEY` (PEM, base64 allowed),
  `GITHUB_APP_SLUG`, `GITHUB_WEBHOOK_SECRET`, `SDI_SESSION_SECRET` (32+ bytes,
  signs session tokens and encrypts stored keys), `SDI_PUBLIC_URL`,
  `IDENTITY_GRPC_URL` (`http://127.0.0.1:50054`). **Dev mode**: when the
  OAuth variables are absent and `SDI_DEV_LOGIN=1`, `GET /auth/login`
  creates a local session for the `GITHUB_TOKEN` account with a personal
  workspace, so `make dev` keeps working without registering an app. With
  no `GITHUB_TOKEN` it signs in a local developer (`local-dev`, GitHub id
  0) whose workspace has no GitHub access: its token is refused
  (`FAILED_PRECONDITION`, with the sentence), everything else works, which
  is how CI runs the e2e suite without a GitHub account.

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
| Source lines sent to the model (§23) | nothing by default. With `code_to_model = changed_lines` and the reasoner on: the lines one finding points at plus at most 20 lines either side, from the pull request under review, capped at 120 lines per request, to the workspace's configured provider. Never a whole file, never a `secrets.*` finding, and any line a secrets rule matches is replaced by `<redacted>` before sending. The reply is stored as that finding's suggestion | the suggestion lives with the review; the request is not stored | yes (default off; `code_to_model = off`) |
| OSV lookups (§20) | package coordinates (`ecosystem`, `name`, `version`) of dependencies a pull request adds, sent to `OSV_API_URL`; answers cached in `osv_cache` (shared, not per tenant) | 24 h TTL | `OSV_API_URL=off` |
| Image inspection (§22.1) | layer blobs of a release image, pulled from the workspace's registry into `<SDI_RUN_DIR>/inspect` and deleted as soon as each is read; the report on the release names files, packages, versions, variable names and binary metadata, never contents, key material or variable values; the shipped packages' coordinates go to `OSV_API_URL` | the report: until the release (or the workspace) is deleted; blobs: the step | yes (`inspect: off`, the default; `OSV_API_URL=off`) |
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

**What is effective, what is stored.** `GetSettings` returns the
*effective* provider, endpoint and model: the workspace's own choice,
else what it inherits (the model stored with its key, `LLM_PROVIDER` /
`LLM_BASE_URL` / `LLM_MODEL`, and for Anthropic only `ANTHROPIC_MODEL` or
the built-in default). An inherited value is never stored as the
workspace's choice: the gateway sends an unpatched provider, endpoint or
model as empty ("keep what is stored"), and the service drops a value
that only repeats what the workspace would inherit anyway. So a workspace
that chose nothing keeps following the environment. An OpenAI-compatible
provider with no model named anywhere has no effective model, and then
the built-in reasoner answers.

**Testing the connection.** `TestReasoner` (ADMIN;
`POST /settings/reasoner/test`) makes one call of at most 16 reply tokens
and answers `{status, message, provider, model, endpoint, latencyMs,
inputTokens, outputTokens}`. `status` is `ok`, or the first thing in the
way, in the order a person fixes them: `off` (the data controls),
`heuristic` (the settings choose the built-in reasoner), `no_key`,
`no_model`, `cap_reached`, `provider_error` (the provider's HTTP status,
never its body), `identity_unavailable`. A misconfiguration is an answer
(200), not an error. `endpoint` is scheme and host only. The call counts
against the daily cap; audit action `settings.reasoner_test`.

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
| `publish` | `PublishReview` posts a GitHub PR review (`github` token with `pull_requests:write`) with the summary as the body and one review comment per **accepted** finding (an exact replacement becomes a GitHub suggestion block, an example plain code under "Example to adapt", §23.2); event `comment` / `request_changes` / `approve` |

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
synchronize) and the pull request watcher (§23, for installs that receive
no webhooks) only *start* a review when the workspace setting
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

## 22. Continuous delivery

CI builds an image; CD moves that exact image through environments. SDI
does not replace the tools that deploy (GitHub Actions, Flux, Argo CD,
Jenkins, Azure Pipelines); it **orchestrates them through the same gates
as everything else**: permission → policy → approval → act → verify →
checkpoint, and stops on failure.

**Objects** (execution service, schema `execution`, all per workspace):

- `Registry` — where images live. `ghcr` is seeded as the workspace default
  (`ghcr.io/<workspace slug>`, auth `github_token`: nothing to configure).
  Others: `dockerhub`, `acr`, `ecr`, `gar`, `generic`, with auth `oidc`
  (cloud registries, from CI) or `secret` (names of workspace secrets).
- `Environment` — per project, ordered by `position` (promotion order),
  with a deployer `kind` and its `config`:

  | kind | what SDI does | config keys |
  |---|---|---|
  | `github_actions` | dispatches the deploy workflow (`workflow_dispatch` with inputs `environment`, `image`, `digest`, `version`) and follows that run to completion | `workflow` (default `deploy.yml`), `ref` (default branch) |
  | `gitops_flux`, `gitops_argocd` | edits the image tag in the GitOps repository and opens a pull request (or commits), then waits for the merge; the controller reconciles | `repo` (`owner/name`, default the project repo), `branch`, `path` (file), `strategy` (`kustomize_image` \| `helm_values` \| `yaml_path`), `yaml_path`, `mode` (`pull_request` default \| `commit`) |
  | `jenkins` | `POST <url>/job/<job>/buildWithParameters` and polls the queued build | `url`, `job`, `user_secret`, `token_secret` |
  | `azure_devops` | `POST dev.azure.com/<organization>/<project>/_apis/pipelines/<pipeline_id>/runs` and polls the run | `organization`, `project`, `pipeline_id`, `pat_secret` |
  | `webhook` | signed `POST` (HMAC-SHA256 in `X-SDI-Signature-256`) with the release JSON; 2xx is success | `url`, `secret`, `timeout_seconds` |
  | `docker_local` | runs the image on the host's Docker (development only; needs the `local-exec` profile) | `port`, `env` (JSON) |
  | `sdi_gitops` | syncs an application of SDI's own GitOps engine with the release image, then waits for it to be healthy (§24.7) | `application` (required, a UUID) |

  `protected=true` gives production semantics. `auto_promote=true` promotes
  into the stage as soon as the previous one succeeds (still gated).
  `source=config` environments come from `.sdi.yml` and are read-only in
  the UI.
- `Release` — one immutable image at one commit (`image`, `tag`, `digest`,
  `version`, `notes` = commit subjects since the previous release).
  Created (a) by the `workflow_run` webhook when a build workflow of the
  project succeeds on the default branch, (b) by `registry_package` /
  `package` webhooks, (c) manually (`POST …/releases`, default: default
  branch head). The tag defaults to the short commit SHA; the image
  defaults to `<registry host>/<namespace>/<repo name>`.
- `DeploymentRun` — a release going to an environment (`promote` or
  `rollback`), backed by an `Execution` so approvals, logs, diagnosis and
  replanning are the ones in §7. Plan:

  | step | action_type | adapter | default policy |
  |---|---|---|---|
  | verify image | `registry.verify_image` | `registry` (Registry v2 `HEAD /v2/<name>/manifests/<tag>`, token auth; records the digest) | safe · auto |
  | inspect image (only when the environment's `config.inspect` is `report` or `gate`, promotions only; §22.1) | `artifact.inspect` | `registry` (pulls the layers of the digest just verified) | safe · auto |
  | deploy | `deploy.development` (position 0, not protected) · `deploy.staging` · `deploy.production` (protected) | the environment's deployer | medium · auto / high · approval / high · required |
  | verify health | `deploy.verify_health` | `http` (polls `url + health_path` for 2xx, up to `health_timeout_seconds`, default 120; skipped without a URL) | safe · auto |
  | record | `deploy.record` | `github` (creates a GitHub Deployment + status so the repository shows the environment) | safe · auto |

  A failed deploy or health check pauses the run with remediations
  `retry`, `rollback_to_previous` (starts a rollback run) and `cancel`.
  `Rollback` is the same plan with action type `deploy.rollback`
  (high · approval). New permissions: `registry:read`,
  `deploy:development`, `pipeline:trigger`; the `github-token` identity
  holds `pipeline:trigger`, deploy permissions are granted per §5.3.
- `DeliveryPipeline` (`GetPipeline`) — stages in order with the current
  release, last run, health and the promotable `candidate`, plus 30-day
  success rate, deployment count and median lead time.

Operational notes: a deployment run's execution uses the adapter name
`delivery` (refused on `POST /executions`; only Promote/Rollback create
it) and runs as the seeded `delivery` identity (`repository:read`,
`registry:read`, `pipeline:trigger`). Every `deploy:*` permission needs a
timed grant (§5.3), so the first promote of a workspace pauses with
`PERMISSION_DENIED` → `request_access`; that is intended. Every
environment kind also accepts `timeout_seconds` and
`health_timeout_seconds` in `config`. `github_token` registry auth is
only allowed for `ghcr.io` (or a GHES `containers.*` host) and the token
is only sent to a token realm on the registry's own host, so a registry
definition can never be used to exfiltrate the workspace token. Outbound
deployer and health URLs must be `https` and must not resolve to
loopback, link-local, private or metadata addresses unless
`SDI_ALLOW_PRIVATE_DEPLOY_TARGETS=1`. The rule is applied twice: saving
an environment or registry answers `invalid_argument` (naming the field)
for a plain `http` URL and for a host that is an IP literal outside the
public ranges or `localhost`; every call is then checked again after the
host is resolved, which also covers a name that later moves. `docker_local`
is exempt from the save-time check. A workflow dispatch is matched to
its run by creation time, so two simultaneous dispatches of the same
workflow cannot be told apart: runs to one environment are serialised. A
paused run reads `failed` with `finishedAt: null` and stays open until
resolved; the REST status enum is `DeploymentRunStatus`.

**Workspace secrets** (identity): `SetSecret`/`ListSecrets`/`DeleteSecret`
store values encrypted with `SDI_SESSION_SECRET`; only names and a 4-char
hint are ever returned; `ResolveSecret` is service-to-service and audited
(`secret.resolve`). Deployers reference secrets by name, never by value.

**Config as code** — `.sdi.yml`:

```yaml
delivery:
  registry: { kind: ghcr }                # or acr/ecr/gar/dockerhub/generic with host + namespace
  build_workflow: ci.yml                  # which workflow's success creates a release
  inspect:                                # §22.1, every environment's default (or just `inspect: report`)
    mode: report                          # off | report | gate
    fail_on: high                         # what a gate stops on
    ignore: [binary.no_pie]               # rule ids or families (binary.*)
  environments:
    - name: staging
      kind: github_actions
      url: https://staging.example.com
      health_path: /health
      auto_promote: true
    - name: production
      kind: gitops_flux
      protected: true
      config: { repo: acme/fleet, path: apps/api/kustomization.yaml, strategy: kustomize_image, inspect: gate }
```

The analyzer parses it into `ProjectConfig.delivery_json`; after each scan
the knowledge service calls `Execution.SyncEnvironments`.

Config validation is strict and identical in the analyzer (`.sdi.yml`)
and the execution service (API), so a file that parses cleanly cannot
fail the sync: a registry of kind `acr`/`ecr`/`gar`/`generic` needs a
`host`; `gitops_flux`/`gitops_argocd` need `path` (`strategy` defaults to
`kustomize_image`; `yaml_path` is required for strategy `yaml_path` and
defaults to `image.tag` for `helm_values`; `gitops_flux` also accepts
`image_automation: true|false`); `jenkins` needs `url`, `job`,
`user_secret`, `token_secret`; `azure_devops` needs `organization`,
`project`, `pipeline_id`, `pat_secret`; `webhook` needs `url` and
`secret`. Secret names match `^[A-Z][A-Z0-9_]{1,63}$`; timeouts are
1–21600 s (webhook 1–600, health 1–3600); at most 50 environments per
project, names up to 63 characters. Errors name the key as
`config.<key>`. Unknown `config` keys for a kind are dropped with a
warning. A config with errors changes nothing; removing
`delivery.environments` from a usable config clears the config-managed
environments at the next scan. `delivery.registry` is matched to an
existing workspace registry by name, id or kind and never creates one.
Generated files never overwrite by guesswork: the build lands in
`build.yml` unless `build_workflow` names a file that does not exist yet,
an existing `Jenkinsfile` / `azure-pipelines.yml` gets a `.deploy`
sibling, and files for an external GitOps repository are placed under
`gitops/<repo>/`. Deployers pass the parameters `IMAGE`, `DIGEST`,
`VERSION`, `ENVIRONMENT` (Jenkins) and `image`, `digest`, `version`,
`environment` (Azure Pipelines, GitHub Actions inputs).

**Setup files** (`Intelligence.GenerateDeliveryArtifacts`, generators in
§14): `.github/workflows/build.yml` (login per registry kind — GHCR with
`GITHUB_TOKEN` and `packages: write`, ACR/ECR/GAR through OIDC actions,
Docker Hub through secrets — `docker/metadata-action`, `build-push-action`
with provenance + SBOM, digest output), `.github/workflows/deploy.yml`
(`workflow_dispatch` inputs, `environment:` so GitHub Environment rules
also apply), Flux (`GitRepository`, `Kustomization`, optional image
automation) or Argo CD (`Application`) manifests, `Jenkinsfile`,
`azure-pipelines.yml` — only for the deployer kinds the project uses. The
web app offers them as a pull request through the `github` adapter.

**Routes**: `GET/POST /registries`, `PUT/DELETE /registries/{id}`,
`GET/POST /projects/{id}/environments`, `PUT/DELETE /environments/{id}`,
`GET /projects/{id}/pipeline`, `GET/POST /projects/{id}/releases`,
`GET /releases/{id}`, `POST /environments/{id}/promote {releaseId?}`,
`POST /environments/{id}/rollback {releaseId?, reason?}`,
`GET /projects/{id}/deployment-runs?environmentId=&limit=&before=`,
`GET /deployment-runs/{id}`, `GET /projects/{id}/delivery/artifacts`,
`GET /workspaces/{id}/secrets`, `PUT/DELETE /workspaces/{id}/secrets/{name}`.
Roles: registries, secrets and environment edits ADMIN; create release
MEMBER; promote/rollback APPROVER. PlatformEvent entities `release`,
`deployment` and `environment` (with `data["workspace_id"]`).

**Pipeline numbers** (`GET /projects/{id}/pipeline`): `deployments30d`
counts runs *started* in the last 30 days whatever became of them;
`finishedRuns30d` counts the ones that reached SUCCEEDED or FAILED in that
window and is the denominator of `successRate`. A paused run is not
finished, so `finishedRuns30d: 0` means "no rate yet" and clients show a
dash, never 0%.

### 22.1 Image inspection (reverse-engineering what ships)

A pull request review sees what the team meant to ship; the release image
is what actually ships. Between "the registry has the image" and "deploy
it", a deployment run can reverse-engineer that exact image and find what
only the built artifact shows: the dependency versions compiled into its
binaries (a repository can say `golang.org/x/net v0.23.0` while the binary
still links v0.17.0), the Go toolchain that built them, the packages of
the base image, binary hardening, provenance, and things that must never
ship. The readers are Rust ports of REA, "Reverse Engineer Anything"
(`github.com/morluto/rea`, MIT, see NOTICE), in the pure crate
`crates/inspect` (`sdi-inspect`: bytes in, report out; no network, no
clock, no disk), and keep REA's evidence model: every property has a
`state`, a `confidence` (`observed` from the bytes, `derived` from what
was read) and a `basis`; whatever could not be read is a limitation,
never a clean result.

**Where it runs.** Step `inspect_image`, action `artifact.inspect` (safe ·
auto, not editable), tool `registry`, depending on `verify_image`; the
deploy step then depends on it. Planned only for promotions (a rollback
goes back to a release already inspected, and a recovery must not wait on
a new advisory) and only when the environment's `config.inspect` is
`report` or `gate`. Off (the default) leaves §22's four-step plan as it is.

**What it reads.** Through the same Registry v2 auth as `verify_image`:
the manifest of the digest `verify_image` just recorded (an index resolves
to `linux/amd64`, else `linux/arm64`, else the first Linux image;
attestations are skipped), the image config, then every layer blob, lowest
first. A blob follows the registry's redirect to its storage without the
registry's `Authorization` once the host changes, is streamed to a file
under `<SDI_RUN_DIR>/inspect` (deleted when read), must hash to its
digest, and is read through gzip (zstd layers are a limitation). Files
are kept only if the final filesystem still holds them (OCI whiteouts and
opaque directories apply). Budgets: `SDI_INSPECT_MAX_MB` of compressed
layers, 128 MiB per binary, 8 MiB per package database, 2 000 000 files,
15 minutes per step (`params.timeout_seconds`).

| family | rule ids | what it reads |
|---|---|---|
| hardening (application binaries: outside the distribution's directories and outside `node_modules`/`site-packages`, or a Go binary whose main module is the release's repository) | `binary.exec_stack` (high), `binary.no_pie` (medium; low for Go), `binary.no_relro` (low; not Go), `binary.partial_relro` (info), `binary.no_stack_protector` (low; C/C++ only), `binary.unsafe_rpath` (medium: relative or `/tmp`-like RPATH/RUNPATH), `binary.debug_info` (info), `rust.no_dependency_list` (info) | ELF header, program headers (`PT_GNU_STACK`, `PT_GNU_RELRO`, `PT_INTERP`), dynamic table (`DF_1_PIE`, `BIND_NOW`, `DT_RPATH`/`DT_RUNPATH`), symbols (`__stack_chk_fail`, `__*_chk`), sections |
| toolchain and provenance | `go.toolchain_eol` (medium, the analyzer's EOL table on the run's date), `go.dirty_build` (medium: `vcs.modified=true`), `provenance.commit_mismatch` (high: the repository's own Go binary was built from another `vcs.revision` than the release's commit) | Go build info (`.go.buildinfo`, inline and pointer encodings) |
| what ships | `dependency.vulnerable` (the advisories' highest severity; unknown is medium) | Go modules and the Go standard library from build info, Rust crates from cargo-auditable's `.dep-v0`, `node_modules/**/package.json`, Python `*.dist-info/METADATA`, dpkg (`status`, `status.d/`) and apk databases with the ecosystem from `/etc/os-release` (`Debian:<major>`, `Alpine:v<major.minor>`), asked of `OSV_API_URL` (`querybatch`, at most 1 000 coordinates, details of at most 60 advisories, 30 s budget, fails soft). A distribution package is reported only for advisories with a fixed version; the rest are counted in the limitations |
| base image | `image.os_eol` (high) | `/etc/os-release` against the analyzer's EOL table |
| never ship | `image.private_key` (critical; test, fixture, doc and third-party trees excluded), `image.env_file` (high when a variable name says credential, else medium), `image.secret_in_env` (high: a credential-named `ENV` with a value in the image config), `image.source_map` (low), `image.runs_as_root` (medium: no or root `User` in the image config) | file names, PEM markers, `.env` variable names, the image config |

Values are never copied: a report names files, packages, versions,
variables and binary metadata, never file contents, key material or the
value of a variable.

**What the environment decides** (config keys every deployer kind accepts,
validated identically by the analyzer and the execution service):
`inspect` = `off` | `report` | `gate`; `inspect_fail_on` = `info` | `low`
| `medium` | `high` (default) | `critical`; `inspect_ignore` = rule ids or
families (`binary.*`), comma-separated, at most 64 characters each.
`report` never stops the run: an image that could not be read is noted
(`inspection_status = skipped`) and the run goes on. `gate` fails the step
with `ARTIFACT_FINDINGS` when findings at or above `inspect_fail_on` remain
after `inspect_ignore` (its raw output lists them with their fixes), and
with the registry's own code (`IMAGE_NOT_FOUND`, `PERMISSION_DENIED`,
`QUOTA_EXCEEDED`, `TIMEOUT`, `UNKNOWN`, the last also for a blob whose
bytes do not match its digest) when the image could not be read.
`ARTIFACT_FINDINGS` offers `accept_findings` ("Deploy anyway": retries the
step with `params.accept_findings = true`; it inspects again, records
`inspection_status = accepted` and lets the run continue; the RESOLVE
event says a person accepted), `retry` and `cancel` (recommended); any
other failure of the step offers `accept_findings` as "Deploy without the
inspection" next to its usual options. `skip_step` is never offered: the
deploy depends on the step.

**Where the report goes.** The step emits the whole report as the
progress output `inspection_report`; the delivery projection (in the same
transaction) stores it on the release (`releases.inspection`, with
`inspection_status` `passed` | `findings` | `incomplete`,
`inspection_max_severity`, `inspection_findings`, `inspected_at`) and
removes it from the execution's outputs, which keep `inspection_status`,
`inspection_findings`, `inspection_max_severity`, `inspection_blocking`
and `inspection_digest`. The release's report is the latest one: a
release promoted to production a week after staging is inspected again,
so an advisory published in between is found. `Release.inspection` (REST
`inspection`) carries the summary everywhere and the typed report
(`InspectionReport`) only on `GET /releases/{id}`; the platform event is
`release updated` with status `inspected`.

**Second opinion.** With `SDI_REA_BIN` set, the REA CLI's
`inspect-binary-hardening` (plus `inspect-go-binary` or
`inspect-rust-binary` by language) runs over at most five application
binaries, 90 s each, written to a scratch directory under the same work
directory; each run is a bounded `tools[]` entry of the report and never
fails the step.

`.sdi.yml`: `delivery.inspect` is every environment's default, either a
mode (`inspect: gate`) or `{mode, fail_on, ignore: [rule ids]}`; an
environment's own `config.inspect*` wins.

## 23. Pull request autopilot

SDI can carry a pull request from "opened" to "merged" with a person only
deciding: it notices the pull request, reviews it, writes the fixes it can,
commits them, and merges. Each of those is a separate, switchable step, and
the three that change the repository (commit, merge, repository settings)
are executions, so permission → policy → approval → act → verify applies
exactly as in §7. With every default left alone, nothing is published,
committed or merged without a person deciding it.

It works with a plain token (`gh auth`, a personal access token) as well as
with the GitHub App: no step depends on a webhook.

Each of the three executions is **created and started** by intelligence as
the principal that asked (`CreateExecution`, then `StartExecution`), so it
runs straight to its first approval instead of waiting in planning for a
second click. A start that fails does not undo the request: the execution
is recorded and returned, and can be started from its page. Requests for
the same thing take turns (`pg_try_advisory_xact_lock` per review, per
pull request, per project): while one is being handled a second is refused
with `FAILED_PRECONDITION`, so "is one already open?" can never be passed
twice.

### 23.1 Watcher (intelligence)

A background task looks at the open pull requests of every workspace whose
`auto_review` setting is on, every `SDI_PR_WATCH_SECONDS` (default 60,
minimum 15, `0` = off):

1. `Knowledge.ListProjects` as the system principal of that workspace, then
   `ListPullRequests(state=open, refresh=true, limit=50)` per project.
2. A pull request that is not a draft and has no review for its current
   `head_sha` gets one (`requested_by = system`, activity actor `watcher`).
   It stops at `proposed`, like every review.
3. With `auto_merge = dependabot`: a Dependabot pull request whose merge
   readiness (§23.3) has no blocker and that has no merge execution yet gets
   one, exactly as if a person had asked for the merge.

Bounds: at most 5 reviews and 2 merge requests started per workspace per
look; one look at a time across replicas (`pg_try_advisory_lock`); up to 8
workspaces looked at side by side, the one looked at longest ago first,
each with a 120-second budget; a workspace whose look fails is skipped
until the next one and the reason is kept in `pr_watch_last_error` (a
problem with one project or pull request is noted there and the look goes
on). The first look comes one interval after start. A Dependabot pull
request found blocked is not asked about again for 5 minutes. A look that finds nothing new costs GitHub
only conditional requests (ETag), which do not count against the rate
limit. Reviews are unique per `(project, pr_number, head_sha)`, so the
watcher and a webhook can never produce two.

### 23.2 Fixes

A finding's `suggestion` is code that goes with it, and `suggestion_source`
says what it is. Only an exact replacement of lines `line..=end_line` of
`path` at the review's `head_sha` may ever be committed:

- `rule`: the analyzer's exact replacement (§20). A rule may only claim
  this (`Finding::with_replacement`) when replacing those lines with the
  text is the whole fix and nothing in it needs filling in. Today that is
  `dependencies.pin_loosened`, which puts the removed line back.
- `example`: the analyzer's illustration (`Finding::with_suggestion`): a
  line to add rather than replace (`HEALTHCHECK …`, a `permissions:`
  block), or text with a placeholder (`FROM node:<version>`). It is shown
  with the finding, **never committed**, never counted in `fixes_ready`,
  and published to GitHub as plain code under "Example to adapt", not as a
  suggestion block (GitHub's "commit suggestion" would replace the
  commented lines with it). The model may still be asked for the real
  replacement, which then takes the example's place.
- `model`: `DraftFix(finding_id)` asked the workspace's model. Allowed only
  when the data controls say `reasoner != off` **and** `code_to_model =
  changed_lines` (§19); otherwise `FAILED_PRECONDITION` with a sentence
  naming the two switches. `can_draft_fix` is false, and the call refused,
  for a finding that already has a replacement (`rule` or `model`), has no `line`, is in a
  binary, removed or truncated file, belongs to the `secrets` category, or
  points into `.github/workflows/`. The request carries the rule id, title
  and detail, the path and language, and the flagged lines with at most 20
  lines either side read from the head commit (`GetRepoFile(ref)`), 120
  lines at most, secret-matching lines redacted. The model is told to
  return only the replacement for the flagged lines. The reply is capped at
  800 tokens, stripped of code fences, and refused (`FAILED_PRECONDITION`,
  nothing stored) when it is empty, identical to the original, longer than
  200 lines, or when the daily token cap is spent. A drafted fix never
  changes the finding's decision: a person still accepts or dismisses it.
  With the setting `auto_draft_fixes` the review drafts up to 10 such fixes
  itself after `analyze`, most severe first, and fails soft. Unasked, it
  leaves alone the findings that carry an example and the `tests` and
  `maturity` categories (no line replacement fixes "there are no tests"):
  a person can still ask for those.

`ApplyFixes(review_id)` (APPROVER, like every request that writes to the
repository through the `github` adapter, §18) commits every **accepted**
finding that has a replacement (`rule` or `model`) and no
`fix_execution_id` yet:

- Refused with `FAILED_PRECONDITION` when there is none; when the pull
  request is not open; when its head is no longer the review's `head_sha`
  ("review the new head first"); when it comes from a fork ("publish the
  review so the author can apply the suggestions"); or when a fix execution
  for this review is still open.
- Each file is read at `head_sha` and its replacements applied bottom-up.
  Two suggestions that overlap in one file: the more severe one wins, the
  other is left out and stays available. A replacement whose target lines
  are beyond the file's end is left out the same way.
- The result is one execution, adapter `github`, title `Apply N review
  fixes to #<number>`, `artifacts` = the changed files in full:
  `repo.checkout` → `repo.commit_fixes` with `params.branch` (the head
  branch), `params.expected_head_sha`, `params.message`,
  `params.pull_request`. The adapter creates one commit on top of
  `expected_head_sha` and moves the branch without force; a branch that
  moved fails `HEAD_MOVED`. Outputs `commit_sha`, `commit_url`.
- The review keeps `fix_execution_id`, each included finding too. The new
  head is reviewed again by the watcher (or by hand), linked to this review.
  A fix execution that ends failed or cancelled frees its findings, so
  their suggestions can be applied again.

### 23.3 Merge

`GetMergeReadiness(project, pr_number)` reads GitHub now
(`Knowledge.GetPullRequestStatus`) and adds SDI's own gate. `blockers`:

| code | when |
|---|---|
| `not_open` | closed or already merged |
| `draft` | still a draft |
| `conflicts` | GitHub says the branch conflicts with its base |
| `mergeability_unknown` | GitHub has not computed mergeability yet (ask again) |
| `checks_failing` | at least one check run or commit status failed |
| `checks_pending` | none failed, at least one is still running |
| `changes_requested` | a reviewer on GitHub requested changes |
| `blocked_by_github` | `mergeable_state = blocked` (branch protection) with none of the above explaining it |
| `no_review` | `merge_requires_review` is on and no SDI review exists for this pull request |
| `review_outdated` | the newest SDI review is for an older head |
| `review_running` | the review of this head has not reached `proposed` |
| `findings_undecided` | the review of this head has an undecided finding of severity high or critical |
| `merge_in_progress` | a merge execution for this pull request is still open |

The last five are SDI's gate; the first three review ones exist only while
`merge_requires_review` is on (default). `can_merge` is `blockers.is_empty()`.

**Checks** are the head commit's check runs (`GET …/check-runs`, GitHub's
default `filter=latest`) and its commit statuses (`GET …/status`), counted
together, one entry per name. GitHub already returns only the newest
attempt of a run and the newest status of a context, so what still shares
a name is two different things (two apps, two workflows, a run and a
status), and the **worst** outcome under a name counts: failed over
pending over passed. Knowledge (readiness) and execution
(`repo.verify_mergeable`) fold identically, so the frame a person reads and
the step that enforces it cannot disagree. A conclusion nobody here knows
counts as failed; an unknown status state as pending.

`MergePullRequest(project, pr_number, method?, delete_branch?)` (APPROVER)
refuses with `FAILED_PRECONDITION` and the first blocker's sentence unless
`can_merge`; `method` must be one the repository allows (empty picks
squash, else merge, else rebase). It creates one execution, adapter
`github`, title `Merge #<number>: <title>`:

1. `repo.verify_mergeable` — reads the pull request again; fails
   `HEAD_MOVED` when the head is not `params.expected_head_sha`,
   `MERGE_CONFLICT`, `CHECKS_FAILING` (failed or still running) or
   `MERGE_BLOCKED` with GitHub's words.
2. `repo.merge_pull_request` — `PUT /pulls/{n}/merge` with `sha =
   expected_head_sha` and `merge_method`; `409` → `HEAD_MOVED`, `405` →
   `MERGE_BLOCKED` (GitHub's message, e.g. a required review is missing).
   Outputs `merge_commit_sha`, `pull_request_url`.
3. `repo.delete_branch` — only with `delete_branch` and a same-repository
   head; a branch that is already gone is success.

`repo.merge_pull_request` is `high · approval` by default and editable: a
workspace that wants hands-free merges sets that policy to `auto`, and that
is the only switch that removes the human from a merge. Remediations:
`HEAD_MOVED` → review the new head and ask again (ends the run) / cancel;
`MERGE_CONFLICT` → retry after the branch is updated / give up / cancel
(resolving a conflict normally adds a commit, and then the retry stops as
`HEAD_MOVED`, because the plan is pinned to the head that was approved: the
option says so); `CHECKS_FAILING` → retry when the checks are green / give
up / cancel; `MERGE_BLOCKED` → retry after the rule is satisfied on GitHub
/ give up / cancel.

### 23.4 Vulnerabilities and Dependabot

`Knowledge.GetRepositorySecurity(project, refresh?)` reads, with the
workspace's GitHub access and a 5-minute cache:

- whether a Dependabot configuration exists on the default branch
  (`.github/dependabot.yml` / `.yaml`) and which ecosystems it names;
- `GET /repos/{o}/{r}/vulnerability-alerts` → `alerts` (`204` enabled,
  `404` disabled, anything else `unknown`) and
  `GET /repos/{o}/{r}/automated-security-fixes` → `security_updates`;
- `GET /repos/{o}/{r}/dependabot/alerts?state=open` (up to 300, most severe
  first) → `open_alerts`. `alerts_status` is `ok`, `disabled` (GitHub says
  alerts are off), `forbidden` (the token may not read them; the message
  names the scope or App permission) or `unavailable`. It never fails the
  call: the status and a sentence say what happened.
- `fix_pull_request`: an open Dependabot pull request whose branch names
  the alert's package.
- Alerts are read worst first, in two passes (`severity=critical,high`,
  then `medium,low` while there is room), because GitHub can filter by
  severity but not sort by it: a repository with more than 300 open alerts
  loses the least severe ones, never the critical ones, and `truncated`
  says that it did. Ecosystems use `Dependency.ecosystem`'s words (`rust`
  → `cargo`, `actions` → `github_actions`), severities `moderate` →
  `medium`.
- Reading the two switches needs admin rights on the repository; without
  them `alerts` and `security_updates` are `unknown`, which is not an
  error. `alerts` is `enabled` whenever the alert list itself answered.

`Intelligence.GetSecurityOverview(project, refresh?)` is what clients read.
`source = dependabot` when `alerts_status = ok`. Otherwise `source = osv`:
the project's dependencies with an exact version (`ListDependencies`) are
looked up through the OSV client and cache of §20 (same budget, same soft
failure), and `source_note` says why it is not Dependabot's list. `none`
when neither answered. Nothing is stored: the overview is computed per
request and cached for 5 minutes per project.

`ConfigureDependabot(project, alerts?, security_updates?, config_file?)`
(ADMIN; at least one must be true) creates one execution, adapter `github`,
title `Turn on Dependabot for <repository>`:

1. `repo.checkout`
2. `repo.enable_security_updates` (when `alerts` or `security_updates`) —
   `PUT /repos/{o}/{r}/vulnerability-alerts`, then, when asked,
   `PUT /repos/{o}/{r}/automated-security-fixes`. Needs
   `repository:admin`, which is always a timed grant (§5.3), so the first
   run pauses for access; `403`/`404` from GitHub → `PERMISSION_DENIED`
   naming what the token needs (admin on the repository; `administration:
   write` for the App).
3. `repo.open_pull_request` (when `config_file`) — the §14 Dependabot
   generator's file on branch `sdi/dependabot`. Skipped, not failed, when
   the file already exists.

Dependabot's own pull requests are ordinary pull requests here: listed
with `is_dependabot`, reviewed by the watcher, merged through §23.3.

### 23.5 Plan steps (intelligence writes them, execution runs them)

Every step has `adapter = github`. Step keys and params are fixed so the
two services agree without sharing code:

| key | action_type | required_permissions | params |
|---|---|---|---|
| `checkout` | `repo.checkout` | `repository:read` | — |
| `commit_fixes` | `repo.commit_fixes` | `repository:write` | `branch`, `expected_head_sha`, `message`, `pull_request` |
| `verify_mergeable` | `repo.verify_mergeable` | `repository:read` | `pull_request`, `expected_head_sha` |
| `merge` | `repo.merge_pull_request` | `repository:write` | `pull_request`, `expected_head_sha`, `merge_method` (`merge` \| `squash` \| `rebase`), `commit_title` (optional) |
| `delete_branch` | `repo.delete_branch` | `repository:write` | `branch`, `pull_request` |
| `enable_security_updates` | `repo.enable_security_updates` | `repository:admin` | `alerts`, `security_updates` (`true` \| `false`) |
| `open_pull_request` | `repo.open_pull_request` | `repository:write` | as §5.4, plus `branch` (used instead of `sdi/<proposal>`), `title`, `body`, and `skip_if_exists` (a repository path: when it already exists on the default branch the step succeeds without opening anything and says so in its output) |

`pull_request` is the number as decimal text. Each step depends on the one
before it. The repository is the execution's project: a step may name it
in `params.repo` (`owner/name`), and when it does not the runner asks
`Knowledge.GetProject`. `repo.commit_fixes` commits
`CreateExecutionRequest.artifacts` (full file contents keyed by path),
keeps each file's mode, and refuses the default branch, an empty commit and
a commit that changes nothing. A plan the adapter cannot make sense of (a
missing or malformed param) fails `UNKNOWN` with a sentence. "Skip this
step" is never offered for `repo.commit_fixes` or `repo.merge_pull_request`:
skipping would end the run as succeeded without a commit or a merge.

Every one of these steps is safe to run twice, because the runner resumes
after a restart: a commit that is already the branch head, a pull request
that is already merged and a branch that is already gone are successes.

### 23.6 Settings, routes, roles, events

`IntelligenceSettings`: `auto_review` (existing), `auto_draft_fixes`
(default false), `auto_merge` (`off` | `dependabot`, default `off`),
`merge_requires_review` (default true); read-only
`pr_watch_interval_seconds`, `pr_watch_last_run_at`, `pr_watch_last_error`.
`DataControls.code_to_model`: `off` (default) | `changed_lines` (ADMIN,
audited like every data control).

Routes: `POST /findings/{id}/draft-fix` → Finding (MEMBER);
`POST /reviews/{id}/apply-fixes` → Review, 201 (APPROVER);
`GET /projects/{id}/pull-requests/{number}/merge-readiness` → MergeReadiness;
`POST /projects/{id}/pull-requests/{number}/merge {method?, deleteBranch?}`
→ `{executionId, readiness}`, 201 (APPROVER);
`GET /projects/{id}/security?refresh=` → SecurityOverview;
`POST /projects/{id}/security/dependabot {alerts?, securityUpdates?, configFile?}`
→ `{executionId}`, 201 (ADMIN). The pull request list and detail carry
`headRepo`, `isFork`, `isBot`, `isDependabot`.

Audit actions (entity / entity id): `review.fix_drafted` (`finding` / its
id), `review.fixes_requested` (`review` / its id),
`pull_request.merge_requested` (`pull_request` / the number),
`security.dependabot_requested` (`project` / its id). As for every
mutating route, two entries tell the story: the gateway records the
request (who, from where, and the HTTP status when it was refused) and
intelligence records the outcome with its detail. A merge the watcher
asks for has only the second, with user `system`, so none is missing from
the trail. Review activity gains
`fix.drafted`, `fixes.requested`, `merge.requested`.
PlatformEvents: `finding` (`updated`) for a drafted fix, `review`
(`updated`) when a fix or merge execution is attached; the executions
themselves emit as always.

What SDI does not do: it does not write features. The code it writes is
the files of §14 and §22 (generated from facts) and the replacements of
§23.2 (a finding's lines). Lockfiles are never regenerated by SDI;
dependency updates are Dependabot's pull requests.

## 24. GitOps engine (from Argo CD)

Argo CD's core inside SDI, so a team can deploy to Kubernetes without
running Argo CD: declare an application (a path in a repository, a cluster,
a namespace), and SDI renders it, compares it with the cluster, syncs it,
keeps it healthy and remembers every sync. Contract:
`proto/sdi/gitops/v1/gitops.proto`.

### 24.1 The service

`services/gitops`, **Go** (the only Go service), gRPC on **50055**
(`GITOPS_LISTEN`, peers reach it at `GITOPS_GRPC_URL`), Postgres schema
`gitops`. It embeds Argo CD's own **gitops-engine** library
(`github.com/argoproj/argo-cd/gitops-engine`, Apache-2.0), pinned to the
commit of the latest stable Argo CD release (v3.5.6 when this was
written), for what makes Argo CD reliable: the cluster cache (informers on
every watched kind), diffing with Kubernetes normalisation, sync planning
with phases, waves and hooks, pruning, and health assessment. Around it,
the service does what Argo CD's repo-server and application controller
do. `NOTICE` names Argo CD; the engine is a module dependency, not a copy.
Go stubs are generated from `/proto` into `services/gitops/gen` (committed,
`make gen-go` refreshes them); every proto carries a `go_package`.

Principal and roles: the same `x-sdi-user` / `x-sdi-workspace` /
`x-sdi-role` metadata as every service (`internal/principal` mirrors
`crates/common/src/principal.rs`, including `system`). Reads VIEWER,
`RefreshApplication` MEMBER, clusters and applications ADMIN to change,
`SyncApplication` / `RollbackApplication` APPROVER (in practice the
execution service's `gitops` adapter, §24.7, forwarding the approver).

### 24.2 Clusters

A cluster is a name plus the **name of a workspace secret** holding a
kubeconfig (§22 secrets), an optional context, a default namespace and an
optional list of namespaces (namespaced mode: the engine watches and writes
only those). The kubeconfig is read through `Identity.ResolveSecret` as
the workspace's system principal, only in memory, never logged, never
returned. The kubeconfig's user must carry inline credentials (a token, a
client certificate or basic auth): exec and auth-provider plugins are
refused, and so is a user with no credentials, which client libraries
would otherwise fill in with the pod's own service account. For the
engine's apply path the credentials and manifests are written for the
duration of one call to a private (0700) directory, under `/dev/shm`
where it exists, and removed after. `UpsertCluster` connects once (server
version) and answers `status`/`message`, so a bad kubeconfig is reported
when it is saved; an unreachable cluster (including one refused by the
address check below) is still saved, with the reason in `message`. The API server address is checked like
every deploy target (§22): loopback, private and metadata addresses are
refused unless `SDI_ALLOW_PRIVATE_DEPLOY_TARGETS=1` (`make dev` sets it,
since a local cluster is one). One engine cluster cache per cluster, built
on first use, dropped when the cluster changes or after 30 minutes unused,
at most `GITOPS_MAX_CLUSTERS` (64) at once. A workspace holds at most 200
clusters and 2 000 applications. A kubeconfig is a workspace secret, so
its size limit (32 KiB) applies; inline certificates fit in it.

### 24.3 Rendering the desired state

The source (`ApplicationSource`) is a repository on the workspace's GitHub
(default: the project's, from `Knowledge.GetProject`), a revision (branch,
tag or SHA; default the default branch) and a path. The revision is
resolved to a commit SHA, and the repository is read as GitHub's tarball at
that SHA with the workspace's token (`Identity.GetInstallationToken`, the
same access every service uses), extracted into a temporary directory and
removed afterwards. Rendering happens **in process, with no network**:

| type | how |
|---|---|
| `directory` | every `.yaml`, `.yml`, `.json` under the path (`recurse` for subdirectories), split into documents |
| `kustomize` | the kustomize Go API (krusty), loading restricted to the checkout, no remote bases; `kustomize_images` applied as `images:` overrides |
| `helm` | the Helm Go SDK, client-only template of the chart at the path with `helm_value_files` then `helm_parameters`, release name and destination namespace; dependencies must be vendored under `charts/` |
| `""` | `kustomization.yaml`/`.yml` → kustomize, `Chart.yaml` → helm, else directory |

Limits: tarball 200 MB, 1 GiB and 100 000 files unpacked, 5 000
resources, 10 MB per file; a source over a limit is a condition, not a
crash. Rendered manifests are cached by
`(source, commit)` for 24 h (bounded); a hard refresh drops the entry.

Every rendered resource gets the tracking annotation
`sdi.dev/tracking-id: <application id>:<group>/<kind>:<namespace>/<name>`
(Argo CD's annotation tracking). A live resource is the application's only
when that annotation names this application and this exact resource, so
SDI never adopts, overwrites or prunes what it did not create.

### 24.4 Comparing and health

A comparison renders the source, takes the managed live objects from the
cluster cache and diffs each pair with the engine (`diff`, with the same
normalisation Argo CD applies). Per resource `sync_status` is `synced`,
`out_of_sync`, `missing` (desired, not live) or `extraneous` (live, no
longer desired); the application is `synced` when every non-hook resource
is. A live hook resource is listed with `hook: true` and `sync_status`
`synced`. Health comes from the engine's health checks per resource
(`healthy`, `progressing`, `degraded`, `suspended`, `missing`, `unknown`);
the application's health is the worst of its resources, so one desired
resource that is not live makes the application `missing`. A comparison that
cannot finish keeps the last known statuses and adds a sentence to
`conditions`. Diffs and manifests mask every value of a `Secret`'s `data`
and `stringData` (`++++++++`, as Argo CD does), live and desired alike.

### 24.5 Syncing

`SyncApplication` stores any `kustomize_images` / `helm_parameters` into
the source first (the desired state stays declared; a dry run stores
nothing), renders, and calls
the engine's sync: phases PreSync → Sync → PostSync with `SyncFail` hooks
(`argocd.argoproj.io/hook`, `hook-delete-policy`), waves by
`argocd.argoproj.io/sync-wave`, kinds in dependency order, `prune` and
`dry_run` as asked, the destination namespace created when the policy
says so, a partial sync when `resources` is set. It waits for the
operation to end (`timeout_seconds`, default 600, then terminate) and
answers each resource's result and the application's health right after.
Every sync that is not a dry run appends a history entry (kind `sync` or
`rollback`, revision, the source as synced with its overrides, phase, who,
the execution); the last 50 are kept. The service audits what the gateway
cannot see (the outcome of a sync or rollback, automated syncs); a refresh
is not audited by the service, since health checks poll it.
`RollbackApplication` syncs a history entry's source at its revision;
as in Argo CD it is refused (`FAILED_PRECONDITION`; the gateway answers
412 before creating an execution) while `sync_policy.automated` is on,
since the next reconcile would sync the current source again. One operation per application at a time
(`FAILED_PRECONDITION` while one runs; the gateway answers 412 before
creating an execution). `DeleteApplication` with `cascade` deletes the
managed live resources first, as an operation of kind `delete`.

### 24.6 Reconciling and automation

A loop compares every application every `GITOPS_RECONCILE_SECONDS`
(default 180; minimum 30; `0` turns it off), up to 8 at once, one replica
at a time (`pg_try_advisory_lock`), and emits a PlatformEvent (entity
`application`, `data["workspace_id"]`) when sync or health status changes.
An application whose `sync_policy.automated` is set is synced by the loop
as the system principal when it is out of sync: on a new rendered state
always, on drift of the same state only with `self_heal`; `prune` as the
policy says; a self-heal sync of drift is a partial sync of the drifted
resources, without hooks. A failed automated sync is retried with backoff
(10 s, 20 s, 40 s, …, capped at 5 min): the first attempt and then up to
`retry_limit` retries (default 3, at most 10), after which it waits for a
new rendered state. Automated syncs are history entries with `automated: true`
and audit entries (`application.auto_sync`, user `system`). Events are
emitted by the replica that observed the change, and `WatchEvents` streams
that replica's events: run one replica (the Kubernetes manifest does). Turning
automation on is an ADMIN's explicit choice per application: it is the
only way anything reaches a cluster without an execution's approval.

### 24.7 Executions and continuous delivery

The execution service gains the adapter **`gitops`** (it calls this
service over gRPC) and the action types:

| action_type | label | risk | default decision | editable |
|---|---|---|---|---|
| `gitops.preview` | Preview what a sync would change | safe | auto | no |
| `gitops.sync` | Sync an application to its cluster | high | approval | yes |
| `gitops.rollback` | Roll an application back to an earlier sync | high | approval | yes |
| `gitops.verify_health` | Wait for an application to be healthy | safe | auto | no |

`gitops.sync` and `gitops.rollback` need `cluster:write`, held by nobody:
always a timed grant (§5.3). `gitops.preview` is a dry-run sync whose
output lists what would be created, changed or pruned;
`gitops.verify_health` polls the application until it is healthy
(`params.timeout_seconds`, default 300), degraded or missing failing it.
Params: `application` (id), `prune`, `revision`, `history_id` (rollback),
`kustomize_images` / `helm_parameters` (JSON). New failure codes:
`RENDER_FAILED`, `CLUSTER_UNREACHABLE`, `SYNC_FAILED`, each with retry /
cancel remediations.

Continuous delivery (§22) gains the environment kind **`sdi_gitops`**,
config `application` (its id, a UUID; `.sdi.yml` validates the same):
promoting a release runs `registry.verify_image` → the stage's own deploy
step (`deploy.development` / `deploy.staging` / `deploy.production`, or
`deploy.rollback`, with `params.operation = gitops.sync`, so a protected
stage keeps its required, non-editable approval whatever the `gitops.sync`
policy says) syncing the application with the release image as the
kustomize image override (or `helm_parameters` `image.repository` /
`image.tag` for a helm source) → `gitops.verify_health` → `deploy.record`.
`gitops.verify_health` refreshes the application on every poll (a live
comparison, not the loop's last snapshot) and fails on degraded or missing
seen twice in a row.

### 24.8 Gateway, web, events

Routes (each also in §8's table by reference): `GET/POST /clusters`, `PUT/DELETE /clusters/{id}` (ADMIN to
change); `GET /applications?projectId=`, `POST /applications`,
`GET/PUT/DELETE /applications/{id}` (`DELETE ?cascade=`);
`POST /applications/{id}/refresh {hard?}` (MEMBER);
`GET /applications/{id}/manifests`, `GET /applications/{id}/diff`,
`GET /applications/{id}/history`; `POST /applications/{id}/sync {prune?,
dryRun?, revision?}` and `POST /applications/{id}/rollback {historyId,
prune?}` (APPROVER) create and start an execution with the plan
`gitops.preview → gitops.sync → gitops.verify_health` (or `gitops.rollback`
in place of the sync) and answer `201 {executionId}`. A sync with
`dryRun` is a preview-only execution (`gitops.preview` alone). A rollback
is previewed as a dry-run sync of the application's current source at the
history entry's revision, which differs from the rollback itself when the
path, images or parameters changed since. History is read 50 entries at a
time (`limit = 0` means 50). Deleting or exporting a workspace (§19)
includes `Gitops.DeleteWorkspaceData` / `ExportWorkspaceData`. The platform event
stream merges `Gitops.WatchEvents`. Audit: `cluster.upsert`,
`cluster.delete`, `application.upsert`, `application.delete`,
`application.refresh`, `application.sync`, `application.rollback`.

Web: a project tab **Applications** (list with sync and health tags, the
cluster and namespace, the revision) and an application page (the resource
tree grouped by kind with each resource's sync and health, the diff per
resource, the rendered manifests, history with rollback, Sync and Refresh
with the execution link); **Workspace, Clusters** for the cluster list
and editor.

### 24.9 Not included

ApplicationSets, multi-source applications, config management plugins,
remote Helm dependencies and remote Kustomize bases, Argo CD's own UI,
SSO and RBAC (SDI's roles apply), notifications (SDI's events and audit
do that), and non-GitHub repositories. Each is a separate decision.

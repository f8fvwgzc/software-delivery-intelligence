# Software Delivery Intelligence

[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[Trust](TRUST.md) · [Privacy](PRIVACY.md) · [Security](SECURITY.md) · [Self-hosting](docs/self-hosting.md) · [GitHub App](docs/github-app.md)

An engineering control plane for your GitHub repositories. It connects a repository,
understands the system (facts first, inference second), stores what it learned
as a temporal graph and pattern memory, scores the repository's delivery
maturity, proposes the next best delivery action with evidence and generated
files, negotiates that proposal in plain language, and executes the agreed
plan one policy-gated step at a time: as a dry run, for real against your
local Docker daemon, or as a pull request. When a step fails the run stops,
the failure is diagnosed, a human picks the remediation and the plan is
rebuilt from the last checkpoint. The rules every component follows live in
[`CONTRACT.md`](CONTRACT.md); this file is the tour.

## Who is it for

Identity is GitHub: there are no SDI passwords, and nobody gets more than
GitHub already grants them (`CONTRACT.md` §18).

- **Individuals** sign in with GitHub. Your account is a personal
  workspace; connect the repositories you own and run the loop on them.
- **Organisations** install the [SDI GitHub App](docs/github-app.md) on
  the organisation and pick the repositories it may see. One installation
  is one workspace; roles come from org ownership and the `sdi-admins` /
  `sdi-approvers` teams and can only be narrowed, never widened.
- **Self-host for full control.** The whole stack is one compose file
  ([`docs/self-hosting.md`](docs/self-hosting.md)); it talks to GitHub and,
  only if you turn the reasoner on with your own key, to a model API, and
  to nothing else. No telemetry, no training, no code kept beyond the
  cache mode you choose: [`TRUST.md`](TRUST.md) says exactly what is stored
  and points at the code that enforces it; [`PRIVACY.md`](PRIVACY.md) is
  the notice an operator publishes; [`SECURITY.md`](SECURITY.md) is how to
  report a problem. Apache-2.0 ([`LICENSE`](LICENSE)).

## Architecture

```
                         browser
                            │ http://localhost
                            ▼
                ┌─────────────────────────┐
                │  web  (nginx, :80→8080) │  SPA + static assets
                │  rate limits · JSON log │  /api/* → least_conn pool, SSE passthrough
                └───────┬─────────┬───────┘
                        │         │
              ┌─────────▼──┐  ┌───▼────────┐
              │ gateway-1  │  │ gateway-2  │   axum REST + OpenAPI  :8080
              └─────┬──────┘  └─────┬──────┘   /api/v1, /api/docs, 3 SSE streams
                    └───────┬───────┘
                            │ Identity.Authorize (cookie → principal)
                   ┌────────▼───────┐
                   │   identity     │  GitHub OAuth · GitHub App · workspaces · roles · audit
                   │    :50054      │  x-sdi-user / x-sdi-workspace / x-sdi-role → gRPC metadata
                   └────────┬───────┘
             gRPC           │ gRPC            gRPC
     ┌──────────────────────┼──────────────────────────┐
     ▼                      ▼                          ▼
┌───────────┐  context ┌──────────────┐ accepted plan ┌───────────┐ sync ┌──────────┐
│ knowledge │◀────────▶│ intelligence │──────────────▶│ execution │─────▶│  gitops  │
│  :50051   │          │   :50052     │               │  :50053   │      │  :50055  │
└─────┬─────┘          └──────┬───────┘               └─────┬─────┘      └────┬─────┘
      │ GitHub API            │ Anthropic API (optional)    │ Docker · GitHub │ Kubernetes API
      ▼                       ▼                             ▼                 ▼
┌─────────────────────────────────────────────────────────────────────────────────┐
│ postgres (pgvector/pg16)  schemas: identity · knowledge · intelligence ·         │
│                                    execution · gitops                            │
└─────────────────────────────────────────────────────────────────────────────────┘
```

- **web** React 19 + Vite + shadcn/ui + TanStack Query, served by an
  unprivileged nginx that also fronts the API. Opens one live event stream
  per tab and refreshes what changed.
- **gateway** the only HTTP surface. Resolves the session cookie and the
  workspace header through identity, translates JSON into gRPC, publishes
  `GET /api/openapi.json` (rendered at `/api/docs`) and streams execution
  events, scan progress and platform events as SSE.
- **identity** GitHub OAuth sessions, GitHub App installations,
  workspaces, membership synced from org roles and teams, audit trail,
  per-workspace data controls (cache mode, reasoner, retention). Every
  tenant row in every schema carries the workspace it belongs to.
- **knowledge** GitHub connector, scanner, facts (detected, inferred,
  configured from `.sdi.yml`), open questions, temporal graph, pattern
  memory, semantic retrieval (256-dim local embeddings), delivery maturity.
- **intelligence** proposals ranked with evidence and a cost breakdown,
  budget and modernization proposals, generated files per option,
  negotiation, decision memory, generated documents.
- **execution** policy and permission checks, approvals, one-step-at-a-time
  DAG execution, checkpoints, diagnosis and replanning, through the
  `simulated`, `docker`, `github` and `gitops` adapters.
- **gitops** (Go) Argo CD's gitops-engine embedded: renders plain YAML,
  Kustomize and Helm from the repository, diffs with the cluster, syncs
  with waves, hooks and prune, health, auto-sync and self-heal, history
  and rollback ([`docs/gitops.md`](docs/gitops.md)).

Services talk gRPC only (`proto/sdi/*/v1`), never share tables, and each owns
one Postgres schema. The deeper design is in
[`docs/architecture.md`](docs/architecture.md).

## Quick start

Prerequisites: Docker with Compose v2, Rust 1.88 or newer, Go 1.21 or
newer (it fetches the GitOps engine's toolchain), Node 22 with pnpm,
`protoc`, and the `gh` CLI logged in (`gh auth login`) so SDI can
read and write your repositories. `make doctor` checks all of it and
prints the fix for anything missing. The step-by-step version, including
the model key and the automation switches, is
[`docs/getting-started.md`](docs/getting-started.md).

```sh
make doctor                     # is this machine ready?
cp .env.example .env            # optional: a model key (LLM_PROVIDER, LLM_API_KEY, LLM_MODEL)

make dev                        # Postgres in Docker, services + web on the host
#   gateway   http://127.0.0.1:8080/api/docs
#   web       http://localhost:5173

make up                         # the whole stack in Docker behind nginx
#   open http://localhost
curl localhost/api/v1/health    # {"status":"ok"}
make down

make up-local-exec              # same, plus the docker adapter (read "Adapters" first)
```

`make dev` runs the doctor first, installs the web dependencies on a fresh
clone, exports `GITHUB_TOKEN` from `gh auth token` when `.env` does not
set it, turns on the dev login (`SDI_DEV_LOGIN=1`: `/auth/login` signs you
in as that account without registering an OAuth app) and generates a
session secret into `.run/session-secret`. `make up` builds five Rust
images from one Dockerfile (`deploy/docker/rust.Dockerfile`, cargo-chef
cached) and the web image, then waits for every healthcheck; it needs
`SDI_SESSION_SECRET` in `.env` and either the GitHub variables from
[`docs/github-app.md`](docs/github-app.md) or `SDI_DEV_LOGIN=1`. `protoc` must ship its include directory
(`google/protobuf/timestamp.proto`): Homebrew's `protobuf` does.

The whole loop (connect → scan → propose → negotiate → accept → dry run with
an injected fault → diagnose → resolve → succeed) as a verified `curl`
sequence lives in [`docs/e2e.md`](docs/e2e.md). `make check` runs the
gates from `CONTRACT.md` §11; `pnpm lint` is pinned to four oxlint threads
because the default thread count exhausts memory on a 16 GB machine.

## Environment

| Variable | Default | Used by |
|---|---|---|
| `DATABASE_URL` | `postgres://sdi:sdi@127.0.0.1:5440/sdi` (host) · `…@postgres:5432/sdi` (compose) | all services |
| `GITHUB_TOKEN` | — | knowledge (read); execution (`github` adapter, needs `repo` write) |
| `LLM_PROVIDER` / `LLM_BASE_URL` | `anthropic` / — | intelligence (instance-wide reasoner fallback: `anthropic`, or `openai_compatible` with the endpoint of OpenAI, Azure OpenAI, Ollama, vLLM, OpenRouter, Gemini) |
| `LLM_API_KEY` (alias `ANTHROPIC_API_KEY`) | — | intelligence (optional; enables the reasoner for workspaces that allow it and hold no key) |
| `LLM_MODEL` (alias `ANTHROPIC_MODEL`) | `claude-sonnet-5-5` for `anthropic`; required for every other provider | intelligence |
| `SDI_PR_WATCH_SECONDS` | `60` | intelligence: how often the pull request watcher looks at open pull requests of workspaces with automatic review on (§23); `0` turns it off |
| `OSV_API_URL` | `https://api.osv.dev` | intelligence (review vulnerability check; `off` disables) |
| `KNOWLEDGE_GRPC_URL` / `INTELLIGENCE_GRPC_URL` / `EXECUTION_GRPC_URL` / `IDENTITY_GRPC_URL` / `GITOPS_GRPC_URL` | `http://127.0.0.1:5005x` (host) · `http://<service>:5005x` (compose) | gateway, intelligence, execution, knowledge, gitops |
| `GATEWAY_LISTEN` / `KNOWLEDGE_LISTEN` / `INTELLIGENCE_LISTEN` / `EXECUTION_LISTEN` / `IDENTITY_LISTEN` / `GITOPS_LISTEN` | `0.0.0.0:<port>` | each service |
| `GITOPS_RECONCILE_SECONDS` | `180` | gitops: seconds between two comparisons of an application (§24); `0` turns the loop off |
| `SDI_ALLOW_PRIVATE_DEPLOY_TARGETS` | `1` under `make dev`, unset otherwise | execution, gitops: allow deploy targets and cluster API servers on private addresses |
| `SDI_SESSION_SECRET` | — (required; `scripts/dev.sh` generates one into `.run/session-secret`) | identity: signs sessions, encrypts stored tokens and keys |
| `SDI_PUBLIC_URL` | `http://localhost:5173` (dev) · `http://localhost` (compose) | identity, gateway: OAuth callback origin, cookie host |
| `SDI_DEV_LOGIN` | `1` under `make dev`, unset otherwise | identity: local session for the `GITHUB_TOKEN` account when no OAuth app is configured |
| `GITHUB_OAUTH_CLIENT_ID` / `GITHUB_OAUTH_CLIENT_SECRET` | — | identity: sign-in for individuals ([`docs/github-app.md`](docs/github-app.md)) |
| `GITHUB_APP_ID` / `GITHUB_APP_SLUG` / `GITHUB_APP_PRIVATE_KEY` / `GITHUB_WEBHOOK_SECRET` | — | identity: organisation installations and webhook verification |
| `LOG_FORMAT` | text · `json` in containers | all |
| `RUST_LOG` | `info,sqlx=warn` | all |
| `SDI_RUN_DIR` | `.run` (host, made absolute by `scripts/dev.sh`) · `/var/lib/sdi/run` named volume (compose) | execution (`docker` adapter workdirs) |
| `DOCKER_HOST` | Docker default, unset | execution (`docker` adapter); compose passes it through only when set, the `local-exec` profile provides the socket instead |
| `GITHUB_API_URL` | `https://api.github.com` | execution (tarball downloads and the `github` adapter; GHES or a test server) |
| `SDI_HTTP_PORT` | `80` | compose: host port for the web container |
| `SDI_TAG` | `local` | compose: image tag |
| `COMPOSE_PROFILES` | — | compose: `local-exec` is `make up-local-exec` |

Compose reads `.env` for interpolation only; secrets are never baked into
images. Full table: [`CONTRACT.md` §3](CONTRACT.md#3-environment).

## Folder map

```
CONTRACT.md            source of truth: topology, env, vocabularies, API, nginx, .sdi.yml, maturity
proto/sdi/*/v1         gRPC contracts (compiled by crates/proto)
crates/proto           generated clients and servers, never hand-edited
crates/common          config, telemetry, db pool per schema, gRPC channels, embeddings
crates/analyzer        pure repository analysis: wanted_files(tree), analyze(snapshot), EOL table
services/gateway       axum REST + OpenAPI, SSE for executions, scans and platform events
services/identity      GitHub OAuth + App, sessions, workspaces, roles, audit, data controls
services/knowledge     GitHub, scans, facts, graph, patterns, retrieval, maturity, .sdi.yml
services/intelligence  proposals, ranking, budget, negotiation, decisions, generators, documents
services/execution     policies, permissions, approvals, DAG runner, diagnosis, adapters
services/gitops        Go: Argo CD's gitops-engine behind gRPC (render, diff, sync, health, history)
web/                   React app (features/<feature>/{api,components,pages,lib}), e2e/ (Playwright)
contracts/openapi.json committed API contract (scripts/api-contract.sh)
deploy/docker          rust.Dockerfile (ARG SERVICE, Docker CLI in execution only), go.Dockerfile, web.Dockerfile, healthcheck.sh
deploy/nginx           edge config: SPA, /api proxy, rate limits, SSE (snippets/sse.conf), JSON logs
docker-compose.yml     production-shaped stack (+ profile local-exec); docker-compose.dev.yml is Postgres only
scripts/               dev.sh (everything on the host), api-contract.sh, loadcheck.sh
docs/                  getting-started.md, architecture.md, configuration.md (.sdi.yml),
                       adapters.md, continuous-delivery.md, gitops.md, e2e.md,
                       github-app.md, self-hosting.md, performance.md
examples/gitops/hello  a small Kustomize application to try the GitOps engine on
TRUST.md · PRIVACY.md · SECURITY.md · LICENSE (Apache-2.0)
.github/               CI (rust, web, edge, images, Playwright), nightly, Dependabot
```

## The loop

```
observe → understand → store → retrieve → compare → rank
      → propose → negotiate → approve → execute → observe → learn outcome
```

1. **Observe / understand** a scan reads the repository tree, manifests,
   CI, IaC, deployments and `.sdi.yml` and turns them into `Fact`s. Each fact
   is *detected* (seen in a file), *inferred* (derived with a confidence),
   *configured* (declared in `.sdi.yml`, confidence 1.0) or an open
   `Question` the platform asks once and remembers. The same facts score the
   repository's delivery maturity on eight axes.
2. **Store / retrieve** facts become nodes and edges of a temporal graph;
   patterns and project profiles get embeddings for semantic retrieval.
3. **Compare / rank / propose** intelligence retrieves candidate patterns
   and ranks them with a weighted, deterministic score
   ([`CONTRACT.md` §6](CONTRACT.md#6-ranking-intelligence)). Every option
   carries `Evidence`, a `cost_breakdown`, and the files it would add to the
   repository.
4. **Negotiate** a reply like "we need private networking" adds a
   constraint and re-ranks; "under $15 a month" sets a budget; "we
   standardise on Azure" records a `Decision`. The signals never change,
   only constraints and weights.
5. **Approve / execute** an accepted proposal becomes an execution: a DAG
   of `PlanStep`s run one at a time, each passing permission → policy →
   approval → run → verify → checkpoint, through the adapter the run was
   created with.
6. **Learn outcome** pattern success rates and last-used times feed the
   next ranking.

**Pull requests.** The same loop runs on pull requests
([`CONTRACT.md` §20, §23](CONTRACT.md)): a watcher reviews every open pull
request of a connected repository (it polls, so it needs no webhook),
findings come with the evidence and, where possible, the replacement code,
accepted replacements are committed to the branch, and a merge shows what
stands in the way before it asks for approval. The Security tab lists
vulnerable dependencies and can turn Dependabot on. Every one of those
writes is an execution behind an approval.

Four principles hold everywhere: detected ≠ inferred ≠ proposed ≠ decided;
discover first, ask only what cannot be known; observe mode is the default;
failure stops the run.

## Screenshots

_Placeholder: overview with the execution rail, proposal negotiation,
approvals queue, paused execution with remediation options, maturity report,
settings. Capture from `make up` once the UI slice lands._

## Repository configuration: `.sdi.yml`

An optional file at the repository root answers what a scan cannot discover.
Its values become **configured facts** (confidence 1.0, cited as
`.sdi.yml`) and answer the matching open questions automatically.

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

Unknown keys are reported (`GET /projects/{id}/config` → `errors`) and
ignored; a file that fails to parse is ignored entirely and reported. A
worked example, what each key changes, and how the file relates to
negotiation and decisions: [`docs/configuration.md`](docs/configuration.md).

## Adapters

`POST /executions` takes `adapter`; it picks the runtime for the whole run.

| adapter | what it does | steps it can run |
|---|---|---|
| `simulated` (default) | timed dry run, fault injection, never touches anything | every action type |
| `docker` | real local execution on the host's Docker daemon: checkout of the scanned commit into `SDI_RUN_DIR` with the option's generated files written on top, `docker build` of the application directory's Dockerfile, run detached with the configured port and poll `health_path` (a container that dies keeps its logs for the failure), run the test command inside the image or, for a compiled stack, in its toolchain image with the source mounted, tear everything down; a cancelled or finished run removes its container and workdir; stdout/stderr become `LOG` events | `repo.checkout`, `container.build`, `container.run`, `tests.run_local`, `env.create_temporary`, `env.destroy_temporary`, `deps.install` |
| `gitops` | Kubernetes through the GitOps service: a dry-run preview, a sync or rollback of an application (after approval, with a timed `cluster:write` grant), then waiting for it to be healthy ([`docs/gitops.md`](docs/gitops.md)) | `gitops.preview`, `gitops.sync`, `gitops.rollback`, `gitops.verify_health`, and the `deploy.*` steps of an `sdi_gitops` environment |
| `github` | writes to the connected repository: branch `sdi/<first 8 chars of the proposal id>` from the default branch, the option's generated files committed through the Git Data API, a pull request, `outputs.pull_request_url` | `repo.checkout`, `repo.open_pull_request` |
| `cloud`, `kubernetes` | reserved; `UNIMPLEMENTED` (Kubernetes deploys go through `gitops`) | — |

Every adapter passes the same gates. Two actions in the UI use the real ones:

- **Verify locally with Docker** runs `repo.checkout → container.build →
  container.run → tests.run_local → env.destroy_temporary` with the `docker`
  adapter. Needs the `container:build` and `container:run` permissions (the
  seeded `local-docker` identity has them), an approval for `container.run`
  (default policy), the `test` block of `.sdi.yml`, and a Docker daemon the
  execution service can reach: on the host that is the default socket; in
  the Docker stack it is `make up-local-exec`, the `local-exec` compose
  profile that hands the host's Docker socket to the execution container.
  **That socket is root on the host**: with the profile on, the execution
  service can start any container on your machine. Developer machines only;
  the trade-off and the hardening options are spelled out in
  [`docs/adapters.md`](docs/adapters.md).
- **Open a pull request** (`POST /proposals/{id}/pull-request`, answers 201
  with the proposal whose `executionId` is the new run) runs
  `repo.checkout → repo.open_pull_request` with the `github` adapter. Needs
  `repository:write` (the seeded `github-token` identity, which exists only
  while `GITHUB_TOKEN` is configured), an approval for
  `repo.open_pull_request` (default policy), and a token with write access
  to the repository: classic `repo`, or fine-grained Contents + Pull
  requests read/write. Options that generate `.github/workflows/*` also need
  the `workflow` scope (`gh auth refresh -s workflow`); without it GitHub
  answers 404 on the commit tree and the step fails `PERMISSION_DENIED`
  naming that scope. The same token is passed to the execution service by
  compose and `scripts/dev.sh`.

Generated files follow the repository's shape: in a monorepo the Dockerfile,
compose build context and image job point at the package that serves
traffic (`backend/` in view-engine), `ci.yml` gets one job per workspace
package with that package's toolchain (a Cargo or pnpm workspace with a root
manifest is one job from the root), and `dependabot.yml` lists every package
directory per ecosystem. The container port comes from the Dockerfile the
scan found before the framework's conventional one.

## Maturity report

`GET /projects/{id}/maturity` scores the repository on eight axes, 0–100
each, from facts only, never from guesses:

| axis | weight | 100 means |
|---|---|---|
| `containerization` | 0.15 | Dockerfile + compose or k8s, non-root user, healthcheck |
| `ci` | 0.20 | CI with test + build + a deploy or release job |
| `tests` | 0.15 | unit + e2e suites, test command detectable |
| `iac` | 0.10 | IaC present with remote state / modules |
| `dependencies` | 0.15 | lockfile + Dependabot + no EOL major pins |
| `docs` | 0.05 | README + architecture/ADR docs |
| `secrets` | 0.10 | `.env.example`, no committed secret-looking files, secret scanning |
| `runtime` | 0.10 | every detected runtime within support (curated EOL table) |

`overall` is the weighted mean and `stage` is `legacy` (< 35),
`transitional` (35–69) or `modern` (≥ 70). Each axis lists its findings in
plain sentences ("Node 16 reached end of life 2023-09-11") and the
`recommended_patterns` that would raise it. A proposal of kind
`modernization` ranks exactly those patterns, weakest axis first, so the
report is the entry point of the modernization loop: score → propose →
generated files → pull request → rescan → score.

## Settings

`GET /settings` and `PATCH /settings {knowledge?, intelligence?}` back the
settings page; the gateway composes the two services' settings into one
object.

| section | fields |
|---|---|
| `knowledge` | `scanMaxFiles`, `scanMaxFileBytes`, `scanConcurrency`, `pipelineRunsLimit`, `cacheGithub` (ETag caching of GitHub responses) |
| `intelligence` | `defaultWeights` (the seven ranking weights), `reasoner` (`heuristic` or `claude` — the latter meaning "the configured provider"; it needs the workspace's data controls on and a key, and falls back otherwise), `reasonerProvider` (`anthropic` or `openai_compatible`), `reasonerBaseUrl`, `model`/`reasonerModel`, `reasonerDailyTokenCap` (default 200 000 tokens per UTC day), `apiKeyConfigured` (read-only), `costCeilingUsd`, `autoReview` |

Weights edited here are the defaults every new proposal starts from;
negotiation still adjusts them per proposal.

## Live events

The gateway exposes three server-sent event streams, and nginx passes them
through unbuffered, without the request rate limit, for up to an hour:

| stream | payload | ends |
|---|---|---|
| `GET /api/v1/events/stream` | `PlatformEvent {entity, entityId, projectId, action, status, at, data}` merged from all three services | never; keep-alive every 15 s |
| `GET /api/v1/scans/{id}/stream` | the `Scan` after every stage change | `event: end` when the scan settles |
| `GET /api/v1/executions/{id}/events/stream` | each `ExecutionEvent` | `event: end` when the run settles |

The web app opens the platform stream once, in the shell, and invalidates
the matching query families by `entity` (`project`/`scan`, `proposal`/
`decision`/`document`, `execution`/`approval`, `pattern`), so a scan that
finishes, an approval that lands or a pull request that opens shows up in
every tab without polling. Polling remains only as a fallback while the
stream is disconnected.

## How execution safety works

Every step is checked in order, and the first failing check stops the run
before anything happens:

1. **Permission** the executing identity must hold every
   `required_permission` of the step (`<resource>:<verb>`, e.g.
   `container:run`). Seeded identities: `simulated` holds `repository:read`,
   `container:build`, `environment:write`; `local-docker` adds
   `container:run`; `github-token` holds `repository:read` and
   `repository:write` and exists only while `GITHUB_TOKEN` is configured.
   Anything else is a time-boxed grant (default 30 minutes). A missing grant
   fails the step with `PERMISSION_DENIED` before it runs.
2. **Policy** the step's `action_type` maps to a default decision. Rows
   marked not editable can never be relaxed.
3. **Approval** `approval` and `required` decisions create an `Approval`
   the human decides in the UI; the execution waits in
   `WAITING_FOR_APPROVAL`.
4. **Run and verify** the adapter the run was created with performs the
   step (`simulated` is a timed dry run with fault injection). A
   `CHECKPOINT` event is written after verification.

| action_type | risk | default | editable |
|---|---|---|---|
| `repo.read`, `repo.analyze`, `repo.checkout`, `docs.generate` | safe | auto | no |
| `tests.run_local`, `env.destroy_temporary` | safe | auto | yes |
| `deps.install`, `container.build`, `env.create_temporary` | medium | auto | yes |
| `container.run`, `db.create_temporary`, `cloud.create_dev_resource`, `repo.open_pull_request` | medium | approval | yes |
| `cloud.modify_resource`, `deploy.staging` | high | approval | yes |
| `deploy.production`, `db.migrate_production`, `backup.restore_production`, `resource.delete`, `firewall.modify`, `secrets.rotate` | high | required | no |
| `db.destroy_production` | blocked | blocked | no |

Observe mode is the default per project; execute mode is opt-in and still
passes every check above. On failure every transitive dependent is
`BLOCKED`, the failure is diagnosed into a code
(`PERMISSION_DENIED`, `QUOTA_EXCEEDED`, `TESTS_FAILED`, …) with at least two
remediation options plus a free-text instruction, and `ResolveFailure`
rebuilds only the failed branch from the last checkpoint. Full tables:
[`CONTRACT.md` §5.2–5.5 and §7](CONTRACT.md#52-policy-action-types-policyaction_type-planstepaction_type).

## Deployment shape

`docker compose up` (project `sdi`) starts Postgres, the four gRPC
services, two gateway replicas and the nginx edge on private networks; only
the edge publishes a port. The edge (`deploy/nginx`):

- serves `web/dist` with immutable hashed assets and a no-cache `index.html`,
  falling back to the SPA for client routes;
- proxies `/api/` to `upstream gateway` (least_conn, `max_fails=3`,
  keepalive, names re-resolved through Docker DNS), forwarding
  `X-Request-Id` (minted when absent) and `X-Forwarded-*`;
- rate limits `/api/` at 20 r/s (burst 40) per address, mutations at 5 r/s,
  32 connections per address, answering `429 {"code":"rate_limited",…}`;
  when no replica answers it returns `502`/`504` in the same JSON shape;
- gives `/api/v1/webhooks/github` its own 10 r/s budget instead of the
  mutation one, caps its body at 1 MB and forwards `X-Hub-Signature-256`
  unchanged for the gateway's HMAC check; `/api/v1/auth/*` is ordinary
  traffic;
- keeps the three SSE routes (`/api/v1/events/stream`,
  `/api/v1/scans/*/stream`, `/api/v1/executions/*/events/stream`)
  unbuffered for an hour and outside the request rate limit, so one
  long-lived connection per tab is never answered with a 429
  (`snippets/sse.conf`);
- logs JSON lines, sends security headers, hides server tokens.

Rust images are built from `deploy/docker/rust.Dockerfile` with
`--build-arg SERVICE=<gateway|identity|knowledge|intelligence|execution>`
(cargo-chef dependency cache, `debian:bookworm-slim`, non-root, read-only
root filesystem, bash `/dev/tcp` healthcheck). Only the execution image adds
the Docker CLI and buildx, for the `docker` adapter. The web image builds
with pnpm on `node:22-alpine` and serves from `nginxinc/nginx-unprivileged`.

The `local-exec` profile (`make up-local-exec`) adds a `docker-socket`
relay that puts the host's Docker socket into the execution container at the
default path, and a named volume holds the adapter's workdirs
(`SDI_RUN_DIR`). `DOCKER_HOST` is never set by the stack; set it on the host
to use a remote daemon instead. `make down` removes the relay too.

## Verification

```sh
make check          # cargo fmt --check, clippy -D warnings, pnpm typecheck + lint
make test           # cargo test --workspace
make deploy-check   # compose config (both profiles), nginx -t in the image, bash -n
make up && make e2e # Playwright critical path against the stack (SDI_E2E_GITHUB=0 skips real-GitHub specs)
make loadcheck      # 30 s through nginx: p50/p95 and the share of 429s
make api-contract   # regenerate contracts/openapi.json and web/src/lib/api/schema.d.ts
```

`scripts/loadcheck.sh` uses `oha` or `hey` when one is installed and a
`curl` loop fanned out with `xargs -P 16` otherwise; `-u`, `-c`, `-d` and
`-m` (fail above a p95 budget) adjust it. From one address the 429 share is
expected to be high: that is the edge doing its job.

## Contributing

Every change has to pass the definition of done in
[`CONTRACT.md` §11](CONTRACT.md#11-definition-of-done-every-slice).
CI (`.github/workflows/ci.yml`) runs the Rust, web and edge checks, builds
every image without pushing, then brings the stack up with `make up` on the
runner from the same layer cache and runs `pnpm e2e` against it; Playwright
browsers are cached by lockfile hash. The real-GitHub specs run only when
the `SDI_GITHUB_TOKEN` repository secret exists (GitHub forbids secrets
named `GITHUB_*`); otherwise the job passes `SDI_E2E_GITHUB=0`.
`.github/workflows/nightly.yml` runs the whole file every night and on
demand. Dependabot groups weekly updates for Cargo, npm, GitHub Actions and
base images. When code and `CONTRACT.md` disagree, the contract wins and the
code is wrong; change them together.

## License

Apache License 2.0, copyright 2026 f8fvwgzc. See [`LICENSE`](LICENSE).
Contributions are accepted under the same terms (Apache-2.0 §5).

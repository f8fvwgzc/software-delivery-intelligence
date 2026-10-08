# Software Delivery Intelligence

An engineering control plane for one GitHub account. It connects a repository,
understands the system (facts first, inference second), stores what it learned
as a temporal graph and pattern memory, proposes the next best delivery action
with evidence, negotiates that proposal in plain language, and executes the
agreed plan one policy-gated step at a time. When a step fails the run stops,
the failure is diagnosed, a human picks the remediation and the plan is
rebuilt from the last checkpoint. The rules every component follows live in
[`CONTRACT.md`](CONTRACT.md); this file is the tour.

## Architecture

```
                         browser
                            │ http://localhost
                            ▼
                ┌─────────────────────────┐
                │  web  (nginx, :80→8080) │  SPA + static assets
                │  rate limits · JSON log │  /api/* → least_conn pool
                └───────┬─────────┬───────┘
                        │         │
              ┌─────────▼──┐  ┌───▼────────┐
              │ gateway-1  │  │ gateway-2  │   axum REST + OpenAPI  :8080
              └─────┬──────┘  └─────┬──────┘   /api/v1, /api/docs, SSE
                    └───────┬───────┘
             gRPC           │ gRPC            gRPC
     ┌──────────────────────┼──────────────────────────┐
     ▼                      ▼                          ▼
┌───────────┐  context ┌──────────────┐ accepted plan ┌───────────┐
│ knowledge │◀────────▶│ intelligence │──────────────▶│ execution │
│  :50051   │          │   :50052     │               │  :50053   │
└─────┬─────┘          └──────┬───────┘               └─────┬─────┘
      │ GitHub API            │ Anthropic API (optional)    │
      ▼                       ▼                             ▼
┌─────────────────────────────────────────────────────────────────┐
│ postgres (pgvector/pg16)  schemas: knowledge · intelligence ·    │
│                                    execution                     │
└─────────────────────────────────────────────────────────────────┘
```

- **web** React 19 + Vite + shadcn/ui + TanStack Query, served by an
  unprivileged nginx that also fronts the API.
- **gateway** the only HTTP surface. Translates JSON into gRPC, publishes
  `GET /api/openapi.json` (rendered at `/api/docs`) and streams execution
  events as SSE.
- **knowledge** GitHub connector, scanner, facts, open questions, temporal
  graph, pattern memory, semantic retrieval (256-dim local embeddings).
- **intelligence** proposals ranked with evidence, negotiation, decision
  memory, generated documents.
- **execution** policy and permission checks, approvals, one-step-at-a-time
  DAG execution, checkpoints, diagnosis and replanning.

Services talk gRPC only (`proto/sdi/*/v1`), never share tables, and each owns
one Postgres schema. The deeper design is in
[`docs/architecture.md`](docs/architecture.md).

## Quick start

Prerequisites: Docker with Compose v2, Rust stable (edition 2024), Node 22
with pnpm 10, `protoc`, and the `gh` CLI logged in (`gh auth login`) so the
knowledge service can read your repositories.

```sh
cp .env.example .env            # optional: GITHUB_TOKEN, ANTHROPIC_API_KEY

make dev                        # Postgres in Docker, services + web on the host
#   gateway   http://127.0.0.1:8080/api/docs
#   web       http://127.0.0.1:5173

make up                         # the whole stack in Docker behind nginx
#   open http://localhost
curl localhost/api/v1/health    # {"status":"ok"}
make down
```

`make dev` exports `GITHUB_TOKEN` from `gh auth token` when `.env` does not
set it. `make up` builds four Rust images from one Dockerfile
(`deploy/docker/rust.Dockerfile`, cargo-chef cached) and the web image, then
waits for every healthcheck. `protoc` must ship its include directory
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
| `GITHUB_TOKEN` | — | knowledge (read-only) |
| `ANTHROPIC_API_KEY` | — | intelligence (optional; enables the `claude` reasoner) |
| `ANTHROPIC_MODEL` | `claude-sonnet-5-5` | intelligence |
| `KNOWLEDGE_GRPC_URL` / `INTELLIGENCE_GRPC_URL` / `EXECUTION_GRPC_URL` | `http://127.0.0.1:5005x` (host) · `http://<service>:5005x` (compose) | gateway, intelligence, execution |
| `GATEWAY_LISTEN` / `KNOWLEDGE_LISTEN` / `INTELLIGENCE_LISTEN` / `EXECUTION_LISTEN` | `0.0.0.0:<port>` | each service |
| `LOG_FORMAT` | text · `json` in containers | all |
| `RUST_LOG` | `info,sqlx=warn` | all |
| `SDI_HTTP_PORT` | `80` | compose: host port for the web container |
| `SDI_TAG` | `local` | compose: image tag |

Compose reads `.env` for interpolation only; secrets are never baked into
images. Full table: [`CONTRACT.md` §3](CONTRACT.md#3-environment).

## Folder map

```
CONTRACT.md            source of truth: topology, env, vocabularies, API, nginx
proto/sdi/*/v1         gRPC contracts (compiled by crates/proto)
crates/proto           generated clients and servers, never hand-edited
crates/common          config, telemetry, db pool per schema, gRPC channels, embeddings
crates/analyzer        pure repository analysis: wanted_files(tree), analyze(snapshot)
services/gateway       axum REST + OpenAPI, SSE for execution events
services/knowledge     GitHub, scans, facts, graph, patterns, retrieval
services/intelligence  proposals, ranking, negotiation, decisions, documents
services/execution     policies, permissions, approvals, DAG runner, diagnosis
web/                   React app (features/<feature>/{api,components,pages,lib})
contracts/openapi.json committed API contract (scripts/api-contract.sh)
deploy/docker          rust.Dockerfile (ARG SERVICE), web.Dockerfile, healthcheck.sh
deploy/nginx           edge config: SPA, /api proxy, rate limits, SSE, JSON logs
docker-compose.yml     production-shaped stack; docker-compose.dev.yml is Postgres only
scripts/               dev.sh (run everything on the host), api-contract.sh
docs/architecture.md   the design, referencing CONTRACT.md
.github/               CI (rust, web, nginx, images) and Dependabot
```

## The loop

```
observe → understand → store → retrieve → compare → rank
      → propose → negotiate → approve → execute → observe → learn outcome
```

1. **Observe / understand** a scan reads the repository tree, manifests,
   CI, IaC and deployments and turns them into `Fact`s. Each fact is
   *detected* (seen in a file), *inferred* (derived with a confidence) or an
   open `Question` the platform asks once and remembers.
2. **Store / retrieve** facts become nodes and edges of a temporal graph;
   patterns and project profiles get embeddings for semantic retrieval.
3. **Compare / rank / propose** intelligence retrieves candidate patterns
   and ranks them with a weighted, deterministic score
   ([`CONTRACT.md` §6](CONTRACT.md#6-ranking-intelligence)). Every option
   carries `Evidence`.
4. **Negotiate** a reply like "we need private networking" adds a
   constraint and re-ranks; "we standardise on Azure" records a `Decision`.
   The signals never change, only constraints and weights.
5. **Approve / execute** an accepted proposal becomes an execution: a DAG
   of `PlanStep`s run one at a time, each passing permission → policy →
   approval → run → verify → checkpoint.
6. **Learn outcome** pattern success rates and last-used times feed the
   next ranking.

Four principles hold everywhere: detected ≠ inferred ≠ proposed ≠ decided;
discover first, ask only what cannot be known; observe mode is the default;
failure stops the run.

## Screenshots

_Placeholder: overview with the execution rail, proposal negotiation,
approvals queue, paused execution with remediation options. Capture from
`make up` once the UI slice lands._

## How execution safety works

Every step is checked in order, and the first failing check stops the run
before anything happens:

1. **Permission** the executing identity must hold every
   `required_permission` of the step (`<resource>:<verb>`, e.g.
   `container:run`). The seeded `simulated` identity holds only
   `repository:read`, `container:build`, `environment:write`; anything else
   is a time-boxed grant (default 30 minutes). A missing grant fails the step
   with `PERMISSION_DENIED` before it runs.
2. **Policy** the step's `action_type` maps to a default decision. Rows
   marked not editable can never be relaxed.
3. **Approval** `approval` and `required` decisions create an `Approval`
   the human decides in the UI; the execution waits in
   `WAITING_FOR_APPROVAL`.
4. **Run and verify** v1 runs everything through the `simulated` adapter
   (timed dry runs with fault injection). A `CHECKPOINT` event is written
   after verification.

| action_type | risk | default | editable |
|---|---|---|---|
| `repo.read`, `repo.analyze`, `docs.generate` | safe | auto | no |
| `tests.run_local` | safe | auto | yes |
| `deps.install`, `container.build`, `env.create_temporary` | medium | auto | yes |
| `container.run`, `db.create_temporary`, `cloud.create_dev_resource` | medium | approval | yes |
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

`docker compose up` (project `sdi`) starts Postgres, the three gRPC
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
- keeps `/api/v1/executions/*/events/stream` unbuffered for an hour;
- logs JSON lines, sends security headers, hides server tokens.

Rust images are built from `deploy/docker/rust.Dockerfile` with
`--build-arg SERVICE=<gateway|knowledge|intelligence|execution>`
(cargo-chef dependency cache, `debian:bookworm-slim`, non-root, read-only
root filesystem, bash `/dev/tcp` healthcheck). The web image builds with pnpm
on `node:22-alpine` and serves from `nginxinc/nginx-unprivileged`.

## Contributing

Every change has to pass the definition of done in
[`CONTRACT.md` §11](CONTRACT.md#11-definition-of-done-every-slice). Locally:

```sh
make check      # cargo fmt --check, clippy -D warnings, pnpm typecheck + lint
make test       # cargo test --workspace
make api-contract   # regenerate contracts/openapi.json and web/src/lib/api/schema.d.ts
docker run --rm -v "$PWD/deploy/nginx:/etc/nginx:ro" \
  nginxinc/nginx-unprivileged:1.28-alpine nginx -t
docker compose config --quiet
```

CI (`.github/workflows/ci.yml`) runs the same checks, then builds every image
without pushing. Dependabot groups weekly updates for Cargo, npm, GitHub
Actions and base images. When code and `CONTRACT.md` disagree, the contract
wins and the code is wrong; change them together.

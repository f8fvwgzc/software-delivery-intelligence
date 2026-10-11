# Architecture

This document explains *why* the platform is shaped the way it is. The
normative tables (ports, environment, vocabularies, ranking weights, the
state machine, the REST routes) live in [`CONTRACT.md`](../CONTRACT.md) and
are referenced, not repeated.

## 1. Three services, one loop

The loop in `CONTRACT.md` §1 (observe → … → learn outcome) is split along
its two trust boundaries:

| Service | Owns | Why it is separate |
|---|---|---|
| `knowledge` | what is *true* about a project: facts, graph, patterns, embeddings | It is the only component allowed to talk to GitHub. Everything downstream reasons over its output, never over raw files. |
| `intelligence` | what the platform *thinks* should happen: proposals, ranking, negotiation, decisions, documents | Opinions must be reproducible and explainable. Keeping them out of the knowledge store means a bad ranking never contaminates a fact. |
| `execution` | what *actually happens*: policies, permissions, approvals, the DAG runner, checkpoints, diagnosis | The blast radius lives here, so this is where every gate is enforced, regardless of who asked. |

The `gateway` is the one HTTP surface and holds no state; the `web` app is a
client of the gateway only. Services call each other over gRPC
(`proto/sdi/*/v1`) and never share tables: each owns one Postgres schema
(`CONTRACT.md` §4) and reads the others' data through RPCs such as
`Knowledge.GetProjectContext`. That constraint is what lets a service be
rewritten, scaled or replaced without a migration in another one.

Shared crates keep the services thin: `crates/proto` (generated code),
`crates/common` (config, telemetry, per-schema pools, lazy gRPC channels,
shutdown, local embeddings) and `crates/analyzer` (pure functions over a
repository snapshot, so analysis is unit-testable without I/O).

## 2. Knowledge model

### Detected, inferred, proposed, decided

The first principle in `CONTRACT.md` is a type system. A `Fact` carries a
`FactKind`: *detected* facts come from a file the scanner read (a
`Dockerfile`, a workflow, a lockfile) and are cited by path; *inferred* facts
are derived with a confidence (a `package.json` with `next` → `framework:nextjs`).
Proposals are `ProposalOption`s produced by intelligence; decisions are
`Decision` records the human made or the system recorded during negotiation.
The UI renders each with its own badge and never collapses them, so a user can
always tell what the platform saw from what it guessed and what they chose.

### Discover first, ask once

The scanner runs `wanted_files(tree)` to pick the small set of files worth
fetching, then `analyze(&RepoSnapshot)` to produce an `Analysis`: capabilities
(`CONTRACT.md` §5.1 vocabulary), facts, open questions. Questions are objects
with a lifecycle (`Question`, `AnswerQuestion`): asked once, answered once,
remembered. Nothing the platform could have discovered is ever asked.

### Temporal graph

Facts become `GraphNode`s (`NodeKind`: service, database, pipeline, cloud
resource, …) and `GraphEdge`s (`RelationCategory`: depends on, deploys to,
builds, …) stamped with validity intervals, so `GetGraph(includeHistory)`
can show what the system looked like at a previous scan and `AnalyzeImpact`
can walk dependents of a node or a path. The graph is the answer to "what
breaks if this changes".

### Pattern memory and retrieval

A `Pattern` is a reusable delivery recipe: a `plan` of `PlanStep`s with
`action_type`, `adapter`, `depends_on`, `required_permissions` and
`verification`, plus `requires` (capabilities), cost, security score,
complexity and an outcome history (`success_rate`, `last_used_at`). The
catalog in `CONTRACT.md` §5.6 is seeded with `scope=public_template`;
patterns learned from the user's own projects are `private`. Every pattern and
every project profile has a 256-dimension embedding
(`sdi_common::embed`, a local hashing embedder, so no network call is needed
to search) stored as pgvector columns; `SearchKnowledge` and
`ListSimilarProjects` are cosine searches over them.

## 3. Ranking and negotiation

Ranking is deterministic on purpose. `overall` is a weighted sum of seven
signals (`CONTRACT.md` §6): semantic similarity, policy agreement with active
decisions, recency, success, architecture fit, cost and security. Each signal
is computed from stored data, and each option ships a `ScoreBreakdown` and a
list of `Evidence` (facts, decisions, prior outcomes) so the UI can show
*why* an option is first.

Negotiation never touches the signals. A message is interpreted into one of
three edits: a `Constraint` (re-rank with a filter), a change of
`ScoreWeights` ("cheaper" raises the cost weight) or a `Decision` with tags
(`cloud:*`, `runtime:*`, `iac:*`) that feeds the policy signal. When
`ANTHROPIC_API_KEY` is set the assistant's prose is written by Claude from
that structured context; without it a template writes the same facts. Either
way the ordering of options is the same, which is what makes the proposal
auditable.

## 4. Execution state machine

`CONTRACT.md` §7 draws the states; the design intent behind them:

- **Everything is an event.** Each transition writes an `ExecutionEvent`
  with a monotonic `seq`. `WatchExecution` streams them and the gateway
  re-exposes the stream as SSE, so the web app renders the execution rail
  from the same log an auditor would read.
- **One step at a time.** Steps form a DAG (`depends_on`); a step is
  `READY` only when all its dependencies are checkpointed. Serial execution
  in v1 keeps the blast radius and the diagnosis simple.
- **Gates before side effects.** Permission (identity grants with TTL) →
  policy (`CONTRACT.md` §5.2, with non-editable rows) → approval (an
  `Approval` object the human decides) → run through an `Adapter` →
  verify → `CHECKPOINT`.
- **Failure stops the run.** Dependents become `BLOCKED`, the run goes
  `PAUSED → DIAGNOSING` and a `Failure` with a code (`CONTRACT.md` §5.5)
  and at least two `RemediationOption`s is attached. `ResolveFailure`
  (option or free-text instruction) rebuilds only the failed branch from the
  last checkpoint, bumps `revision`, expires stale approvals and returns to
  `WAITING_FOR_APPROVAL`. "Continue and hope" is not a transition.
- **Adapters are the extension point.** `simulated` runs every step as a
  timed dry run and supports fault injection (`StartExecution.fault`), which
  is how the failure path is exercised end to end from the browser.
  `docker` and `github` perform the same steps for real (§10.4); `cloud`
  and `kubernetes` are reserved names. The run's adapter is chosen at
  creation, each step names the tool family it needs, and the service
  refuses a run whose steps the runtime cannot serve, so a plan never
  discovers half-way through that it cannot continue.

## 5. Public API

The gateway maps every REST route to one RPC (`CONTRACT.md` §8), converts
gRPC status codes to HTTP, and renders enums and timestamps uniformly. Every
DTO derives `ToSchema` and every route is a `utoipa::path`, so
`GET /api/openapi.json` is generated from the code, committed as
`contracts/openapi.json`, and turned into `web/src/lib/api/schema.d.ts` for
a typed client. A drift between service and web app is therefore a type
error, not a runtime surprise.

## 6. Data

One Postgres server (pgvector) with three schemas. Each service runs its own
`sqlx::migrate!()` on start against a pool whose `search_path` is
`<schema>, public`; the `vector` extension is created by knowledge's first
migration and lands in `public`. Queries are runtime `sqlx::query`/
`query_as` with `FromRow` structs so builds never need a database; vectors
are bound as text and cast (`$1::vector`). Ids are UUID v7 (time-ordered,
generated in Rust), timestamps `timestamptz`.

## 7. Deployment topology

```
host :80 ──▶ web (nginx-unprivileged :8080) ──▶ gateway-1 :8080 ─┐
             frontend network                    gateway-2 :8080 ─┤ backend network
                                                                  ▼
                                   knowledge :50051 · intelligence :50052 · execution :50053
                                                                  ▼
                                                        postgres :5432 (volume pgdata)
```

`docker-compose.yml` is production-shaped rather than a dev convenience:

- **Two networks.** Only `web` and the gateways sit on `frontend`; Postgres
  and the gRPC services are reachable solely through a gateway. Neither
  network is `internal` because knowledge needs GitHub and intelligence may
  need the Anthropic API.
- **Health-gated start.** Every service has a healthcheck (Postgres
  `pg_isready`, Rust services a bash `/dev/tcp` probe since the images carry
  no curl, the gateway an HTTP `GET /api/v1/health`, nginx `/healthz`) and
  `depends_on … service_healthy`, so `docker compose up --wait` returns when
  the stack actually serves.
- **Hardened containers.** Non-root users, `read_only` root filesystems with
  a `tmpfs` for `/tmp`, `cap_drop: ALL`, `no-new-privileges`, modest CPU and
  memory limits, rotated JSON logs.
- **One Rust image recipe.** `deploy/docker/rust.Dockerfile` takes
  `ARG SERVICE`; cargo-chef cooks the whole workspace's dependencies in a
  layer that is byte-identical for all four services, so the expensive part
  compiles once. An `extras-<service>` stage, selected by name, adds what a
  service needs beyond its binary: only execution gets the Docker CLI and
  buildx. Two gateway replicas share the `sdi/gateway` tag; the second one
  never pulls.
- **Opt-in host access.** The `local-exec` profile (§10.4) is the only way
  the stack touches the host's Docker daemon, and it is a separate,
  visible container rather than a flag on the execution service.

### The edge

nginx is the only public listener and implements `CONTRACT.md` §10:

- **Static.** Vite's hashed `/assets/*` are `immutable` for a year;
  `index.html` is `no-cache`; unknown paths fall back to the SPA.
- **Proxy.** `upstream gateway` uses `least_conn`, `max_fails=3
  fail_timeout=10s`, a keepalive pool, and `resolve` through Docker's DNS
  (`127.0.0.11`) so a restarted replica with a new address is picked up
  without a reload. `X-Request-Id` is honoured when the client sends one and
  minted otherwise; it is forwarded to the gateway, returned to the client
  and is the correlation key in the JSON access log.
- **Rate limits.** `zone=api` 20 r/s per address with `burst=40 nodelay`;
  `zone=mutations` 5 r/s keyed by address only for `POST`/`PUT`/`PATCH`/
  `DELETE` (a `map` on `$request_method`); `limit_conn` 32 per address. A
  rejection is rewritten through a named location into the gateway's error
  shape, `429 {"code":"rate_limited","message":"Too many requests"}`.
- **SSE.** The three stream routes (`/api/v1/events/stream`,
  `/api/v1/scans/*/stream`, `/api/v1/executions/*/events/stream`) share one
  snippet that disables proxy buffering, caching and gzip, sets
  `Cache-Control: no-cache` and allows a one-hour read timeout. They carry
  no `limit_req`: a tab holds one connection per stream and reconnects
  after any drop, and a 429 on that reconnect would blind the UI while the
  REST calls kept succeeding. `limit_conn` still bounds how many streams one
  address may hold. Exact and regex locations win over the `/api/` prefix,
  which is what keeps the prefix's rate limit off them.
- **Hygiene.** `server_tokens off`, gzip for text types, security headers
  (`X-Content-Type-Options`, `X-Frame-Options SAMEORIGIN`,
  `Referrer-Policy`, `Permissions-Policy`). TLS is expected to terminate in
  front of this container when exposed publicly; that layer owns HSTS.

## 8. Observability

Every service initialises `sdi_common::telemetry` with `LOG_FORMAT=json` in
containers, so logs are one JSON object per line from nginx through the
gateway to the services, all carrying the request id the edge assigned.
`GET /api/v1/system/status` reports gRPC health and versions of each
service; `GET /api/v1/health` is the liveness probe used by the compose
healthcheck and the definition of done.

## 9. Local development

`make dev` runs Postgres in Docker (`docker-compose.dev.yml`, host port
5440) and every service plus the Vite dev server on the host, which keeps
compile-run cycles fast and lets a debugger attach. `make up` is the same
code in the production shape. Both paths read the same environment table
(`CONTRACT.md` §3); only the hostnames differ.

## 10. Phase 2: from observing to changing the repository

v1 closed the loop as a dry run. Phase 2 lets the platform read what the
repository declares, measure where it stands, generate the files that would
move it, and apply them for real, while keeping every gate from §4. The
flows below are the ones a reader needs to hold in their head; the tables
behind them are in `CONTRACT.md` §5.2–5.6, §6, §8, §10, §12–§14.

### 10.1 Configured facts (`.sdi.yml`)

```
.sdi.yml ──scan──▶ configured facts (confidence 1.0, source .sdi.yml)
                     ├─▶ answers open questions (answered_by = config)
                     ├─▶ budget constraint on every proposal of the project
                     ├─▶ test command / health path / port for the docker adapter
                     └─▶ scan.exclude applied to wanted_files(tree)
```

A third `FactKind`, *configured*, sits next to detected and inferred. It is
the only kind a human writes, it is cited by path like a detected fact, and
it never claims more than the file says: unknown keys and parse errors are
reported in `ProjectContext.config.errors` and ignored, so a typo can only
remove an answer, never invent one. Keeping the file to *values* (target,
environments, budget, test parameters, scan scope) is deliberate; prose
stays a generated document. [`configuration.md`](configuration.md) is the
user's guide.

### 10.2 Maturity and modernization

```
facts ──▶ 8 axes (0–100) ──weighted mean──▶ overall, stage
            │ findings ("No Dockerfile", "Node 16 EOL 2023-09-11")
            └ recommended_patterns per axis
                    │
                    ▼
       proposal kind=modernization: architecture_fit = 1 − weakest targeted axis / 100
```

The report (`Knowledge.GetMaturity`) is computed from facts only; an
inferred fact with confidence 0.6 does not raise an axis, for the same
reason it cannot satisfy a pattern's `requires`. Runtime currency comes
from a curated end-of-life table in `crates/analyzer` read from version
files, manifests and Dockerfile base images, so "Node 16 reached end of
life" is a finding with a date, not an opinion. Modernization patterns
carry the axes they target, and the modernization proposal's architecture
fit rewards the pattern that lifts the *lowest* axis, which is how the
weakest point of a legacy repository is addressed first rather than the
easiest.

### 10.3 Budget, cost breakdown and artifacts

Ranking stays the seven-signal sum of §3. A budget (from the request, the
project's `.sdi.yml`, or a negotiation turn such as "under $15 a month")
is a constraint on `overall`, never a signal: options above it are kept,
marked with `budget:<n>` under `unmet_requirements` and penalised 0.25, and
the cheapest viable option is pinned into the top five so the human always
sees the floor. Every option carries `CostLine`s that sum to its estimate,
because a number without its parts cannot be negotiated.

Each option of a deployment, CI, infrastructure, security, dependency,
testing or modernization proposal also carries `Artifact`s: the files the
pattern would add, generated by one pure function per family
(`intelligence/src/generators/`, `CONTRACT.md` §14) from the pattern and
the `ProjectContext`, with the project's own names, detected port and
negotiated parameters. They are generated once per revision; a negotiation
turn that changes a parameter regenerates them, so what the human reads is
always what the pull request would contain. The gateway keeps them off the
proposal list and detail responses (`artifactCount` only) and serves them
from `GET /proposals/{id}/options/{optionId}/artifacts`, because a proposal
with five options and a Terraform module each is not a list-page payload.

### 10.4 Real adapters

```
accept option ──▶ CreateExecution(adapter)
   adapter=docker:  repo.checkout → container.build → container.run → tests.run_local → env.destroy_temporary
   adapter=github:  repo.checkout → repo.open_pull_request   (POST /proposals/{id}/pull-request)
every step:          permission → policy → approval → run → verify → checkpoint
```

The adapter is chosen for the whole run and each step names the tool
family it needs, so the service can refuse an impossible run at creation
(`INVALID_ARGUMENT`) instead of failing half-way. Both real adapters go
through exactly the gates of the dry run: `container.run` and
`repo.open_pull_request` default to *approval*, the `docker` family needs
`container:build`/`container:run`, the `github` family `repository:write`.
Seeded identities make the capability legible: `local-docker` holds the
container permissions, `github-token` holds the repository ones and exists
only while a token is configured, so `GET /identities` answers "what can
this installation actually do".

The `docker` adapter needs a daemon. On the host that is the default
socket. In the Docker stack it is the `local-exec` compose profile: a
`socat` relay container republishes the host's socket into a volume the
execution container mounts at `/run`, so the daemon appears at the default
path and `DOCKER_HOST` stays unset. A profile can enable a service but not
add a mount to an existing one, which is why the relay is a container of
its own, and that is also what makes the host access visible
(`docker compose ps`) rather than a line buried in a service definition.
The socket is root on the host; the trade-off and the hardening path (a
filtering proxy or rootless Docker behind `DOCKER_HOST`) are in
[`adapters.md`](adapters.md). The execution image alone ships the Docker CLI
and buildx, and its workdirs (`SDI_RUN_DIR`) live on a named volume because
its root filesystem is read-only.

The `github` adapter writes with the same token the knowledge service
reads with, so there is one credential to reason about; opening a pull
request is the only write it performs, on a branch named after the
proposal, never on the default branch.

### 10.5 Live events

```
knowledge.WatchEvents ─┐
intelligence.WatchEvents ─┼─ gateway merge ─▶ GET /events/stream (SSE, keep-alive 15 s, never ends)
execution.WatchEvents ─┘
                                            web shell: one EventSource per tab
                                            entity → invalidate query families
```

Every service already wrote an event for each transition; phase 2 exposes
them as one `PlatformEvent` stream. The gateway fans the three gRPC
streams into one SSE response so the browser holds a single connection,
and the web app translates `entity` into TanStack Query invalidations
(`project`/`scan` → projects and scans, `execution`/`approval` → executions
and approvals, and so on) instead of re-rendering from the payload. That
keeps the event small, keeps the query cache the single source of truth,
and lets polling remain only as the fallback while the stream is down.
`GET /scans/{id}/stream` does the same for one scan's stages, which is how
the connect-and-scan flow is live from the first second. nginx treats all
three as what they are (§7): unbuffered, long-lived, and never rate-limited
per request.

### 10.6 Settings

`GET`/`PATCH /settings` composes `KnowledgeSettings` (scanner limits, GitHub
ETag caching) and `IntelligenceSettings` (default ranking weights, the
reasoner and its model, the cost ceiling) into one object. Each service
owns and validates its half, so the gateway stays stateless and a bad
weight set is rejected by the service that will use it. Settings change
defaults, not history: a proposal keeps the weights it was ranked with,
and its revisions change them only through negotiation.

### 10.7 Verifying the whole thing

CI brings the stack up on the runner with `make up` and runs the Playwright
suite against it; a nightly run repeats the full set and enables the
real-GitHub specs when a token secret exists. `scripts/loadcheck.sh` hits
the edge for thirty seconds and reports p50, p95 and the share of 429s, so
a change to the rate limits or the SSE exemption shows up as a number.

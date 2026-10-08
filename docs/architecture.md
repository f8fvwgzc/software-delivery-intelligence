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
  `github`, `docker`, `cloud`, `kubernetes` are reserved names.

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
  compiles once. Two gateway replicas share the `sdi/gateway` tag; the second
  one never pulls.

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
- **SSE.** `/api/v1/executions/*/events/stream` disables proxy buffering,
  caching and gzip, sets `Cache-Control: no-cache` and allows a one-hour
  read timeout so the execution rail stays live.
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

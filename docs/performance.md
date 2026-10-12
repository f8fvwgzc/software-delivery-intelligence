# Performance

What the API does under load, how it was measured, and what to run before
calling a release production-ready. The budget: every list and detail
route answers in **p95 ≤ 300 ms** at 200 concurrent clients with 50 live
event streams held open, with **no error** and **no service whose memory
keeps growing**. The script that measures it is `scripts/loadcheck.sh`
(`make loadcheck` against `make up`); the bounds that keep memory flat are
listed in [`self-hosting.md` §9](self-hosting.md#9-capacity-bounds-and-kubernetes).

## Method

`scripts/loadcheck.sh` signs in once (dev login), holds 50 connections on
`GET /events/stream`, then drives `GET /projects`, `GET /executions?limit=50`,
`GET /proposals` and `GET /projects/{id}/tree` with the client budget
split evenly (200 clients → 50 per route) for 60 s using `oha` (or `hey`,
or a `curl` loop). It prints p50/p95/p99 per route, the non-2xx share,
how many of the SSE connections were accepted, and the RSS (`ps`) of every
local service before and after. Requests `oha` itself cuts off when the
duration ends are not counted either way.

The run below is the host stack (release binaries, one replica of each
service, no nginx) on a laptop (Apple Silicon), against a private
`sdi_load` database on the dev Postgres, with a seeded workspace holding
one scanned project whose tree has 1 100 entries (117 KB of JSON per
`/tree` answer). Ports were the alternate ones (`*_LISTEN` 6005x and
18080), so a developer's own `make dev` stack keeps running untouched.

```sh
LOADCHECK_COOKIE=<sdi_session> scripts/loadcheck.sh \
  -b http://127.0.0.1:18080 -P "60051 60052 60053 60054 18080" \
  -c 200 -d 60 -s 50 -m 300
```

## Results (2026-10-12, commit after the hardening pass)

| endpoint | requests | r/s | p50 ms | p95 ms | p99 ms | non-2xx |
|---|---:|---:|---:|---:|---:|---:|
| `/executions?limit=50` | 96 250 | 1 604 | 30.6 | 39.6 | 45.6 | 0.0 % |
| `/projects` | 89 659 | 1 494 | 33.0 | 42.0 | 48.5 | 0.0 % |
| `/proposals` | 191 650 | 3 194 | 14.7 | 22.1 | 26.8 | 0.0 % |
| `/projects/{id}/tree` (117 KB) | 29 865 | 498 | 99.3 | 112.3 | 122.5 | 0.0 % |
| `/events/stream` | 50 held for 60 s | | | | | 50 × 200 |

407 424 requests, 0 non-2xx, worst p95 112 ms.

| service | RSS before | RSS after | note |
|---|---:|---:|---|
| gateway | 13.3 MB | 91.3 MB | working set of 200 in-flight responses plus 150 upstream gRPC watch streams; back to ~11 MB once the clients leave (allocator release is lazy on macOS) |
| knowledge | 11.1 MB | 22.0 MB | 50 concurrent `/tree` reads |
| identity | 11.5 MB | 17.5 MB | authorize cache misses only every 60 s per session |
| intelligence | 8.4 MB | 13.2 MB | |
| execution | 8.4 MB | 13.8 MB | |

A second run on the same processes lands within a few MB of the "after"
column: the growth is the working set of the load, not accumulation.

## What the first run found (and what fixed it)

The same script, before the fixes, showed the gateway at **283 MB after
fifty event streams opened and 1.45 GB after 60 s of load** while every
other service stayed flat. `malloc_history` against a symbolized build
pointed at `axum::Router::into_inner → Endpoint::clone →
utoipa::openapi::OpenApi::clone`: the `/api/openapi.json` handler and the
Scalar page captured the whole OpenAPI document by value, and axum clones
every route's service per connection, so each open connection (every SSE
tab) carried two copies of a ~4.5 MB tree of maps. The document and the
page are now rendered once at startup and served as shared bytes
(`services/gateway/src/app.rs`). The regression test is the number above;
`scripts/loadcheck.sh -s 50` reports the RSS delta, and anything over a
few MB per held stream is this class of bug again.

Other changes from the same pass that show up in these numbers:

- the gateway's workspace switcher used one `ListProjects` call per
  workspace (N+1) to count projects; `Knowledge.CountProjects` answers
  every workspace in one query;
- list queries have indexes matching their `ORDER BY`
  (`executions (workspace_id, updated_at DESC, number DESC)`,
  `approvals (workspace_id, requested_at DESC)`, `facts (project_id,
  source_path)`, the partial `questions … WHERE answered_at IS NULL` and
  `scans … WHERE status IN ('queued','running')`);
- scans write facts, graph nodes and edges, dependencies and chunks in
  batches of 500 rows (`UNNEST`) instead of one statement per row;
- repository archives stream to a temporary file (50 MB cap) instead of
  the heap, in knowledge and in the execution `docker` adapter;
- every cache is bounded and expires (authorize answers, GitHub tokens,
  data controls, ETag bodies, installation tokens), every stream is capped
  per workspace (`SSE_MAX_PER_WORKSPACE`, 429 past it), the webhook route
  answers 202 and works off a bounded queue, and gRPC messages are capped
  at 16 MiB.

## Running it yourself

```sh
make up                       # the compose stack behind nginx on :80
make loadcheck                # 200 clients, 60 s, 50 SSE streams, through the edge
scripts/loadcheck.sh -h       # every knob
```

Through nginx the edge's rate limit (20 r/s per address, burst 40) turns a
single-address load test into mostly 429s by design; `-u` keeps the
original single-URL check that measures exactly that. For the API numbers
above, point `-b` at a gateway replica or run the services on the host.
For containers, `docker stats --no-stream` gives the RSS that `ps` gives
on the host.

Budgets worth failing a release over: `-m 300` (p95) and the default
`-e 1` (one percent non-2xx), plus "RSS after" within 100 MB of "RSS
before" for the gateway and within 50 MB for the others.

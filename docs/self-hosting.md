# Self-hosting

Self-hosting is the first-class path for organisations: the whole stack
runs inside your network from one compose file, and the only outbound calls
are to GitHub and, if you turn the reasoner on, the model provider. This
guide takes a Linux host with Docker from nothing to a TLS-fronted
instance with backups and an upgrade routine. What the stack is and why it
is shaped that way is in [`architecture.md` §7](architecture.md#7-deployment-topology);
what it stores is in [`TRUST.md`](../TRUST.md).

## 1. Requirements

- A host with Docker Engine 24+ and Compose v2 (`docker compose version`),
  2 CPUs and 4 GB of RAM for a small organisation (the compose file caps
  each container; Postgres and knowledge get the most). Disk: the Postgres
  volume grows with the number of connected repositories; 10 GB is a
  comfortable start.
- A DNS name pointing at the host and a way to terminate TLS in front of
  port 80 (§4). SDI's edge speaks plain HTTP on purpose.
- A GitHub OAuth app and, for organisations, the SDI GitHub App
  ([`github-app.md`](github-app.md)). Both can be created after the first
  start; the dev login works until then.
- `openssl` on the host for generating secrets.

## 2. First start

```sh
git clone https://github.com/f8fvwgzc/software-delivery-intelligence.git sdi
cd sdi
git checkout <latest tag>
cp .env.example .env
```

Edit `.env`. The minimum for a shared instance:

```sh
SDI_PUBLIC_URL=https://sdi.example.com        # what users type; TLS terminates in front
SDI_SESSION_SECRET=$(openssl rand -hex 32)    # paste the value, not the command
SDI_HTTP_PORT=8080                            # the edge binds here; the TLS proxy forwards to it

GITHUB_OAUTH_CLIENT_ID=…                      # github-app.md §1
GITHUB_OAUTH_CLIENT_SECRET=…
GITHUB_APP_ID=…                               # github-app.md §2 (organisations)
GITHUB_APP_SLUG=…
GITHUB_APP_PRIVATE_KEY=…
GITHUB_WEBHOOK_SECRET=…

#LLM_PROVIDER=anthropic                        # or openai_compatible (OpenAI, Azure OpenAI, Ollama, vLLM, OpenRouter, Gemini)
#LLM_BASE_URL=                                 # OpenAI-compatible endpoint, e.g. http://ollama:11434/v1
#LLM_API_KEY=                                  # optional instance-wide default (alias ANTHROPIC_API_KEY); workspaces can bring their own
#LLM_MODEL=                                    # alias ANTHROPIC_MODEL
#OSV_API_URL=https://api.osv.dev               # `off` disables the review's vulnerability lookups
#SDI_DEV_LOGIN=1                               # never on a shared instance
```

`SDI_SESSION_SECRET` is the one value you cannot lose: it encrypts every
stored credential. Keep a copy wherever you keep secrets. `SDI_DEV_LOGIN`
must stay unset: with it, anyone who reaches the instance signs in as the
`GITHUB_TOKEN` account.

Then:

```sh
make up                       # builds the images, starts the stack, waits for every healthcheck
curl -s localhost:8080/api/v1/health      # {"status":"ok"}
docker compose ps             # every service "healthy"
```

`make up` builds on the host. For hosts that should not compile Rust,
build the images elsewhere (CI does: `.github/workflows/ci.yml`, job
`docker`), push them to a registry, set `SDI_TAG` to that tag and run
`make up UP_FLAGS=` after `docker compose pull`.

### Environment reference

Everything the stack reads is in [`.env.example`](../.env.example) with a
comment per variable and in `CONTRACT.md` §3 and §18. Compose interpolates
`.env` into container environments; nothing is baked into an image, and
`docker compose config` shows the result with secrets in the clear, so do
not paste its output anywhere.

| Variable | Set it when |
|---|---|
| `SDI_PUBLIC_URL`, `SDI_SESSION_SECRET` | always |
| `GITHUB_OAUTH_*` | individuals sign in |
| `GITHUB_APP_*`, `GITHUB_WEBHOOK_SECRET` | organisations install the App |
| `GITHUB_TOKEN` | a personal instance without an OAuth app (`SDI_DEV_LOGIN=1`), or the `github` adapter on a personal workspace |
| `LLM_PROVIDER`, `LLM_BASE_URL`, `LLM_API_KEY`, `LLM_MODEL` (aliases `ANTHROPIC_API_KEY`, `ANTHROPIC_MODEL`) | an instance-wide reasoner provider and key; workspaces may configure their own provider in the intelligence settings and store their own key instead |
| `OSV_API_URL` | where reviews look up added dependencies (`off` for an air-gapped instance) |
| `GITHUB_API_URL` | GitHub Enterprise Server (`https://<host>/api/v3`); organisations on other hosts get their base from their installation |
| `SDI_HTTP_PORT`, `SDI_TAG`, `RUST_LOG` | the port the edge publishes, the image tag, log verbosity |
| `DB_MAX_CONNECTIONS` (20), `DB_STATEMENT_TIMEOUT_MS` (30000) | Postgres pool per service and the session statement timeout; five services × the pool must fit the server's `max_connections` (§9) |
| `GATEWAY_AUTH_CACHE_MAX` (10000), `SSE_MAX_PER_WORKSPACE` (50), `WEBHOOK_QUEUE_MAX` (256) | per-replica bounds of the gateway (§9) |
| `SCAN_STAGE_TIMEOUT_SECS` (600), `SCAN_TIMEOUT_SECS` (1800) | a scan stage, or a whole scan, that overruns fails with the stage named instead of sitting `running` |
| `SHUTDOWN_GRACE_SECS` (20) | how long a stopping service waits for scans, runners and audit entries in flight |
| `SDI_DOCKER_PUBLISH_HOST` (127.0.0.1) | where the `docker` adapter publishes a test container's port |
| `COMPOSE_PROFILES=local-exec` | never on a shared host; see `adapters.md` |

## 3. What is running

```
internet ──TLS──▶ your reverse proxy ──http──▶ web (nginx :8080→SDI_HTTP_PORT)
                                                  │ /api/ least_conn
                                            gateway-1 · gateway-2
                                                  │ gRPC, backend network
                        identity :50054 · knowledge :50051 · intelligence :50052 · execution :50053
                                                  │
                                           postgres :5432 (volume pgdata)
```

Only the `web` container publishes a port. Postgres, the gRPC services and
the gateways are on compose-private networks; nothing but the edge is
reachable from the host, and the gRPC ports must stay that way (anyone who
can reach them can impersonate any user; `SECURITY.md`). Containers run as
non-root with read-only root filesystems, dropped capabilities and memory
limits; logs are JSON lines rotated at 3 × 10 MB per service
(`docker compose logs -f <service>`).

## 4. TLS: a reverse proxy in front of nginx

SDI's own nginx is the application edge (static files, API proxy, rate
limits, SSE). It does not hold certificates, because certificate handling
is better done once, by whatever already fronts your other services. Put
a TLS-terminating proxy in front and forward to `SDI_HTTP_PORT`. Three
things matter in that proxy:

1. **Forward `X-Forwarded-Proto: https`** so the gateway marks the
   `sdi_session` cookie `Secure` and builds `https://` redirect URLs.
   `SDI_PUBLIC_URL` must be the `https://` origin.
2. **Do not buffer the SSE routes** (`/api/v1/events/stream`,
   `/api/v1/scans/*/stream`, `/api/v1/executions/*/events/stream`) and
   allow long read timeouts on them; the UI holds one connection per tab
   for as long as it is open.
3. **Pass bodies through unchanged** on `/api/v1/webhooks/github`; the
   signature covers the raw bytes.

Caddy does all three by default:

```
# Caddyfile
sdi.example.com {
    reverse_proxy 127.0.0.1:8080 {
        flush_interval -1
    }
}
```

nginx in front of nginx:

```nginx
server {
    listen 443 ssl http2;
    server_name sdi.example.com;
    ssl_certificate     /etc/letsencrypt/live/sdi.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/sdi.example.com/privkey.pem;
    add_header Strict-Transport-Security "max-age=31536000; includeSubDomains" always;

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-Proto https;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Request-Id $request_id;
    }

    location ~ ^/api/v1/(events/stream|scans/[^/]+/stream|executions/[^/]+/events/stream)$ {
        proxy_pass http://127.0.0.1:8080;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-Proto https;
        proxy_buffering off;
        proxy_read_timeout 1h;
    }
}
server {
    listen 80;
    server_name sdi.example.com;
    return 301 https://$host$request_uri;
}
```

Traefik, an ingress controller or a cloud load balancer work the same way:
HTTPS in, HTTP to the edge, `X-Forwarded-Proto` set, streaming responses
not buffered. HSTS belongs on the TLS layer, which is why SDI's own edge
does not send it.

Bind the edge to loopback when the proxy is on the same host so port 8080
is not reachable from outside: in a `docker-compose.override.yml` next to
`docker-compose.yml` (compose merges it automatically; `!override` needs
Compose 2.24+; keep the file out of version control if it carries
host-specific values):

```yaml
services:
  web:
    ports: !override
      - "127.0.0.1:${SDI_HTTP_PORT:-8080}:8080"
```

## 5. Backups of Postgres

Everything SDI stores is in the one Postgres volume (`pgdata`): four
schemas, `knowledge`, `intelligence`, `execution`, `identity`. A backup of
that database plus your `.env` (for `SDI_SESSION_SECRET`, without which the
stored credentials in a restore are unreadable) is a complete backup.

Logical dump, safe while the stack runs:

```sh
# nightly, from cron; keeps 14 days
docker compose exec -T postgres pg_dump -U sdi -d sdi -Fc \
  > /backup/sdi-$(date +%F).dump
find /backup -name 'sdi-*.dump' -mtime +14 -delete
```

Restore into an empty stack:

```sh
docker compose down
docker volume rm sdi_pgdata
docker compose up -d --wait postgres
docker compose exec -T postgres pg_restore -U sdi -d sdi --no-owner < /backup/sdi-2026-03-01.dump
make up UP_FLAGS=
```

Test a restore before you need one: restore into a second host (or the
same host with `-p sdi-restore` and a different `SDI_HTTP_PORT`) and sign
in. A backup that has never been restored is a hope, not a backup.

Three things to know:

- **Backups contain repository knowledge and encrypted tokens.** Store
  them with the same care as the database; the tokens are useless without
  `SDI_SESSION_SECRET`, the facts are not.
- **A deleted workspace lives on in backups** until those expire. State
  the retention in your copy of `PRIVACY.md` §3.
- The `rundata` volume (execution checkouts) and the images are
  reproducible and need no backup.

## 6. Upgrading

Releases are tags on `main`. Schema migrations run automatically at
service start (`sqlx::migrate!()` per schema), forward only.

```sh
cd sdi
git fetch --tags
git checkout <new tag>
git diff <old tag> <new tag> -- .env.example CONTRACT.md   # new variables? changed defaults?
docker compose exec -T postgres pg_dump -U sdi -d sdi -Fc > /backup/pre-upgrade.dump
make up                       # rebuilds changed images, restarts, waits for healthy
curl -s localhost:8080/api/v1/system/status | jq .   # every service "serving", versions match the tag
```

`make up` restarts containers whose image or configuration changed and
leaves the rest alone; gateway replicas restart one after another, so the
edge keeps answering. Read the release notes for anything marked
**security** or **migration**; a migration that cannot be undone says so.

Rolling back: `git checkout <old tag>` and `make up` works as long as the
new version added no migration. If it did, restore `pre-upgrade.dump` into
an empty volume (§5) and then start the old tag. This is why the dump
comes before `make up`.

## 7. Operating it

- **Health**: `GET /api/v1/health` (liveness, used by the compose
  healthcheck) and `GET /api/v1/system/status` (per-service gRPC health
  and versions). `docker compose ps` shows the container view.
- **Logs**: `docker compose logs -f --since 1h gateway-1 identity`. Every
  line is JSON with the edge's `request_id`, so one id follows a request
  from nginx to a service. Ship them with any agent that tails the Docker
  json-file driver.
- **Rate limits**: 20 r/s per address on `/api/`, 5 r/s on mutations,
  10 r/s on the webhook route, 32 connections per address; the edge
  answers `429` in the API's error shape. Behind a proxy, make sure the
  proxy sends `X-Forwarded-For` or every user shares the proxy's address
  and one busy tab throttles everyone (the edge limits on the socket peer
  address; put the per-user limit in the TLS proxy if that matters).
- **Rotating `SDI_SESSION_SECRET`** signs everyone out and discards stored
  OAuth tokens and reasoner keys; users sign in again, admins re-enter
  keys. Do it after a suspected leak, not routinely.
- **Rotating the App private key or webhook secret**: `github-app.md`.
- **Resource limits** are in `docker-compose.yml` (`x-small` and per
  service). Raise `knowledge` first for large monorepos, then Postgres.
- **The `local-exec` profile** hands the host's Docker socket to the
  execution service. It exists for developer machines; on a shared host
  point `DOCKER_HOST` at a filtered proxy or a rootless daemon instead
  (`adapters.md`).

## 8. Removing an instance

```sh
make down                     # containers and networks; volumes stay
docker volume rm sdi_pgdata sdi_rundata sdi_docker-sock   # everything SDI stored
```

Uninstall the GitHub App from each organisation and delete the OAuth app
on GitHub; the credentials they issued are then useless regardless of any
backup.

## 9. Capacity, bounds and Kubernetes

Every in-memory structure a service keeps is bounded and expires, so a
public instance's memory is flat however many tenants pass through it
([`performance.md`](performance.md) has the measurements):

| Where | What | Bound |
|---|---|---|
| gateway | `Identity.Authorize` answers (per replica) | `GATEWAY_AUTH_CACHE_MAX` entries, 60 s each |
| gateway | open SSE streams per workspace (per replica) | `SSE_MAX_PER_WORKSPACE`; the next one is `429 resource_exhausted` until a tab closes |
| gateway | webhook deliveries waiting for the worker | `WEBHOOK_QUEUE_MAX`; the handler answers `202` at once and `503 Retry-After: 30` when the queue is full, so GitHub redelivers |
| knowledge | GitHub ETag bodies | 256 entries, 1 h |
| knowledge | workspace tokens, data controls | 4096 workspaces each; tokens until 5 min before expiry, controls 60 s |
| knowledge | repository archives | streamed to `/tmp`, never the heap; 50 MB cap, then per-file downloads; one file ≤ 1 MiB, one pull-request diff ≤ 300 KB |
| identity | installation tokens | 4096 installations, dropped before expiry |
| execution | per-execution event channels | dropped when the run ends; the seed note resets past 10 000 workspaces |
| all | gRPC messages | 16 MiB in either direction |
| all | Postgres | `DB_MAX_CONNECTIONS` per service, `DB_STATEMENT_TIMEOUT_MS` per statement |

**Gateway replicas are stateless.** The caches above are per replica, so
two replicas behind `least_conn` (or ten behind a cloud balancer) need no
sticky sessions: a request that lands on a replica that has not seen its
session costs one `Identity.Authorize`. Redirects are absolute on
`SDI_PUBLIC_URL` and the session cookie is `Secure` when that origin is
`https` or the edge forwards `X-Forwarded-Proto: https`.

**Health endpoints.** Every gRPC service registers `grpc.health.v1.Health`
(what the compose healthcheck's TCP probe stands in for and what
Kubernetes gRPC probes read); the gateway answers `GET /api/v1/health`,
the web container `GET /healthz`, and `GET /api/v1/system/status` reports
the gRPC health of each service behind a gateway.

**Kubernetes.** `deploy/k8s/` is a kustomization of the same stack:
Deployments and Services for the five services and the web pods, a
StatefulSet for Postgres (swap it for a managed instance by setting
`DATABASE_URL` and dropping `postgres.yaml` from `resources`), an
ingress-nginx Ingress with buffering off and an hour of read timeout for
the SSE routes, a HorizontalPodAutoscaler on the gateway (2–10 replicas at
70 % CPU) with a PodDisruptionBudget, a ConfigMap with the bounds above and
a Secret generated from `deploy/k8s/secrets.env`:

```sh
cp deploy/k8s/secrets.example.env deploy/k8s/secrets.env   # fill in
kubectl kustomize deploy/k8s | less                         # review
kubectl apply -k deploy/k8s
```

Probes: gRPC liveness/readiness on each service's port, `httpGet` on the
gateway's `/api/v1/health` and the web's `/healthz`. The archives
knowledge and execution stream to `/tmp` land on an `emptyDir`; the
execution workdir is an `emptyDir` too (a paused run survives a pod
restart only with a persistent volume there).

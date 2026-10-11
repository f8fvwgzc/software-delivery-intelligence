# Security policy

## Reporting a vulnerability

Please do not open a public issue for anything that could be exploited.

- Preferred: a **private vulnerability report** on GitHub
  (`Security` tab → `Report a vulnerability` on the repository), which
  reaches the maintainers without a public trace.
- If that is unavailable, email the maintainer listed in the repository's
  profile with `SDI security` in the subject.

Include what you found, where (file path, route, or component), how to
reproduce it, and what you believe the impact is. You will get an
acknowledgement within 3 working days and a fix or a reasoned decision
within 30 days for anything that breaks a promise in [`TRUST.md`](TRUST.md)
or [`CONTRACT.md` §18–§19](CONTRACT.md#18-identity-workspaces-and-tenancy).
Credit is given in the release notes unless you ask otherwise.

The following count as vulnerabilities here, beyond the usual:

- any way to read data of a workspace you are not a member of, including
  through ids, SSE streams, exports or the audit route;
- any way to obtain a role GitHub does not grant you, or to keep access
  after losing organisation membership;
- any path by which file contents are stored longer than the `code_cache`
  setting allows, or are displayed for a secret-looking path;
- any outbound call that is not GitHub or the configured model provider;
- a webhook accepted without a valid `X-Hub-Signature-256`.

## Supported versions

SDI is pre-1.0 and ships from `main`. Security fixes land on `main` and are
tagged; there are no maintenance branches. Run the latest tag and read the
changelog entry for anything marked **security** before upgrading
([`docs/self-hosting.md`](docs/self-hosting.md#6-upgrading) covers the
procedure).

| Version | Supported |
|---|---|
| latest tag on `main` | yes |
| anything older | upgrade |

## Secrets in repositories: the secret-path rule

SDI reads repositories, and repositories sometimes contain secrets. The
rule, from `CONTRACT.md` §15 and enforced in
`services/knowledge/src/grpc/code.rs` (`looks_secret`):

- Files whose name or extension marks them as credentials (`.env` and
  `.env.*`, `*.pem`, `*.key`, `*.p12`, `*.pfx`, `*.jks`, `*.keystore`,
  `*.ppk`, `id_rsa*`, `secrets.yml`/`secrets.yaml`, and similar) are
  **refused with `PERMISSION_DENIED`** by the code browser and never
  cached, never embedded, never shown, and never sent to a model.
- The check runs on the requested path *and* on the resolved path, so a
  different spelling of the same file does not get around it.
- The analyzer's `wanted_files` list never includes these names; a scan
  does not fetch them even when the tree lists them.

The rule is a safety net, not a scanner. SDI does not try to detect secrets
*inside* ordinary files; the delivery-maturity `secrets` axis reports
whether the repository has secret scanning enabled, which is the right
place for that job.

## Token and secret handling in the platform

- **`SDI_SESSION_SECRET`** (32+ bytes) signs session tokens and encrypts
  every stored credential (OAuth tokens, workspace LLM keys). It lives
  only in the environment of the `identity` service. Rotating it signs
  everyone out and invalidates stored keys; that is the intended
  emergency response to a suspected leak.
- **GitHub OAuth tokens** are encrypted at rest and decrypted only inside
  the identity service for the duration of a request. They are never
  returned by any API, never logged, never forwarded to the browser.
- **GitHub App installation tokens** are minted per request from the App's
  private key, cached in memory for at most 55 minutes, and never written
  to disk or to the database.
- **The App private key** (`GITHUB_APP_PRIVATE_KEY`) and the **webhook
  secret** are read from the environment of the identity service only.
  Compose interpolates them from a git-ignored `.env`; nothing bakes them
  into an image. `.gitignore` excludes `.env` and `.env.*`.
- **Workspace LLM keys** are encrypted at rest; the API returns only a
  "configured" flag and the last four characters.
- **Sessions** are opaque tokens in an `httpOnly; Secure; SameSite=Lax`
  cookie named `sdi_session`, valid for 30 days, revocable with
  `POST /api/v1/auth/logout`.
- **Service-to-service** calls carry `x-sdi-user`, `x-sdi-workspace` and
  `x-sdi-role` as gRPC metadata on a private network; the gRPC ports are
  not published by the compose file and must not be exposed by any other
  deployment either. Anyone who can reach port 50051–50054 can impersonate
  any principal.
- **Logs** carry request ids, routes, status codes and timings. Tokens,
  cookies, request bodies and file contents are never logged; a log line
  that contains one is a bug under this policy.
- **The edge** verifies nothing about identity; it forwards the cookie and
  the webhook signature untouched and rate-limits. Terminate TLS in front
  of it (`docs/self-hosting.md`) so the `Secure` cookie attribute holds.

## Dependencies and images

Dependabot opens grouped weekly updates for Cargo, npm, GitHub Actions and
base images. Runtime images are `debian:bookworm-slim` (Rust services) and
`nginxinc/nginx-unprivileged` (web), run as non-root with read-only root
filesystems, all capabilities dropped and `no-new-privileges`. Rebuild and
redeploy after a base-image advisory; `make up` rebuilds from the pinned
tags in `deploy/docker/*.Dockerfile`.

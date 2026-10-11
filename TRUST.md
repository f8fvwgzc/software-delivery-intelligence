# Trust: what SDI reads, what it keeps, what it never does

Software Delivery Intelligence reads your repository to help you deliver
it. The claim is short: **we do not keep your code, train on it, or move it
anywhere you did not choose.** This page says what that means in plain
language, and for every claim points at the code that makes it true, so you
can check rather than trust. The normative version is
[`CONTRACT.md` §18–§19](CONTRACT.md#19-trust-what-sdi-stores-and-never-does);
when this page and the contract disagree, the contract wins and this page
is wrong.

## In one paragraph

SDI connects to GitHub with the permissions you grant (your own account for
a personal workspace, a GitHub App installation for an organisation), reads
the files it needs to understand how a repository is built, tested and
deployed, and stores *what it learned*: facts, a dependency graph, a
maturity score, proposals and decisions. File contents are read into memory
for analysis and, by default, cached for 24 hours per commit so the code
browser and diagrams do not refetch them; you can turn that cache off. No
model is trained on anything. No telemetry leaves your deployment. LLM
calls happen only if you turn them on, with your own key, and carry
structured context, never raw files. You can export everything and delete
everything, and the deletion is real.

## What SDI reads

- From GitHub, with the token or installation you provided: the repository
  tree, the files the analyzer wants (manifests, lockfiles, Dockerfiles,
  CI workflows, infrastructure, documentation), workflow runs and
  deployments, organisation membership and teams (organisations only).
- It never reads files that look like secrets, even when they are
  committed: `.env`, `*.pem`, `*.key`, `id_rsa*`, keystores,
  `secrets.yml` and friends are refused before any byte is fetched.
- It never reads anything outside the repositories you connected or the
  installation's repository selection.

## What SDI stores, and for how long

This is the table from `CONTRACT.md` §19, rendered for humans. "Workspace"
is your personal account or one GitHub App installation; every row is
scoped to a workspace and nothing can be read across that boundary.

| What | Where it lives | Kept until | Your control |
|---|---|---|---|
| **What SDI understood**: facts, the dependency graph, delivery maturity, pipeline and deployment metadata | Postgres, `knowledge` schema | you disconnect the project | This is the product; disconnecting removes it. |
| **Semantic index**: short text chunks and their embeddings, used for retrieval | `knowledge.chunks` | you disconnect the project | `minimal_chunks` keeps excerpts of at most 400 characters instead of whole sections. |
| **Repository tree**: file names and sizes at the last scanned commit | `knowledge.repo_trees` | the next scan replaces it | Needed for the code browser; the latest commit only. |
| **File contents** for the code browser and diagram inputs | `knowledge.repo_files` | depends on `code_cache` | `none`: never stored, fetched on demand. `session` (default): per commit, 24 h, purged on disconnect. `persistent`: until disconnect. |
| **What SDI produced**: proposals, generated files, decisions, documents, diagrams | Postgres, `intelligence` schema | you delete them | Delete per item or with the workspace. |
| **Execution records**: step logs and outputs of runs | Postgres, `execution` schema | you delete them | Checkout directories are removed after every run. |
| **LLM requests** | nowhere | — | Off by default. Only `reasoner=claude` with *your* key sends anything, and the payload is structured context (facts, options, a spec), never file contents. |
| **Audit trail**: who did what, when | `identity.audit` | `audit_retention_days`, default 365 | Retention is a workspace setting; 0 keeps forever. |
| **GitHub credentials** | memory (installation tokens, at most 55 minutes); Postgres `identity` schema, encrypted (OAuth tokens) | the session ends or is revoked | Encrypted with `SDI_SESSION_SECRET`; rotating the secret invalidates them. |

The product shows this same table on the **Data & privacy** page of every
workspace, with the current values of the settings.

## What SDI never does

- **No training.** Nothing you connect is used to train, fine-tune or
  evaluate any model, by us or by anyone we send data to. There is no such
  pipeline in the code base.
- **No third-party telemetry.** The services emit structured logs to their
  own stdout and nothing else. No analytics SDK, no crash reporter, no
  usage ping, no CDN-loaded scripts in the web app.
- **No code retention beyond the cache mode you chose.** File contents
  exist in memory during analysis and in `knowledge.repo_files` only as
  the `code_cache` setting allows.
- **No LLM calls without your key.** Bring your own key (BYOK): the
  reasoner is off by default, and when on it uses the key stored for your
  workspace, encrypted at rest and never returned by the API.
- **No access beyond GitHub's.** Identity *is* GitHub. Roles are derived
  from organisation ownership and team membership and can only be
  narrowed by an admin, never widened. Losing organisation membership
  removes access at the next request.
- **No secrets on screen.** Secret-looking paths are refused by the code
  browser and never cached, even when the repository contains them.
- **No phoning home.** A self-hosted SDI talks to GitHub, to the model API
  if you enabled it, and to nothing else. Both compose networks exist only
  so those two calls can leave.

## How to check each claim

Every statement above is backed by code you can read. Paths are relative
to the repository root.

| Claim | Where to look |
|---|---|
| Only the wanted files are fetched | `crates/analyzer/src/files.rs` (`wanted_files`), called from the knowledge scanner |
| Secret-looking paths are refused | `services/knowledge/src/grpc/code.rs`, `looks_secret()` and the `PERMISSION_DENIED` branch in `GetRepoFile` |
| File contents follow `code_cache` | `knowledge.repo_files` writes in `services/knowledge`, gated by the workspace's data controls; the setting itself is `DataControls.code_cache` in `proto/sdi/identity/v1/identity.proto` |
| Chunks can be limited to excerpts | `DataControls.minimal_chunks` in the same proto; the chunker in `services/knowledge` |
| No telemetry | `crates/common/src/telemetry.rs` is a `tracing` subscriber writing to stdout; `grep -ri 'segment\|sentry\|posthog\|analytics' --include='*.rs' --include='*.ts' --include='*.tsx' .` finds nothing |
| LLM is opt-in, BYOK, structured context only | `services/intelligence/src/config.rs` (reasoner selection), `DataControls.reasoner` and `SetReasonerKey` in `proto/sdi/identity/v1/identity.proto`, the request builder in `services/intelligence` |
| Stored keys and tokens are encrypted | `services/identity` (encryption with `SDI_SESSION_SECRET`; `CONTRACT.md` §18) |
| Tenancy is enforced in every query | every repository function in `services/*/src` takes a `workspace_id`; every tenant table's migration has `workspace_id uuid not null` plus an index (`services/*/migrations`) |
| Roles are derived and only narrowed | `CONTRACT.md` §18 and `SetMemberRole` in `services/identity` (`override_role` lower than the derived role only) |
| Webhooks are authenticated | HMAC check of `X-Hub-Signature-256` in the gateway's `/api/v1/webhooks/github` handler; `deploy/nginx/conf.d/default.conf` forwards the header unchanged |
| Exports are complete | `POST /api/v1/workspaces/{id}/data/export` in the gateway, which walks every schema for the workspace |
| Deletes are real | `DELETE /api/v1/workspaces/{id}` cascades through the `knowledge`, `intelligence`, `execution` and `identity` schemas and writes an audit entry |
| Self-hosting is first class | `docker-compose.yml`, `make up`, [`docs/self-hosting.md`](docs/self-hosting.md) |
| The edge does not leak | `deploy/nginx` sends security headers, hides server tokens, logs locally |

If you find a place where the code does less than this page promises, that
is a bug: see [`SECURITY.md`](SECURITY.md) for how to report it privately.

## Export and delete

Both are API routes, so they work from the UI, from `curl`, or from a
script, and both are audited.

```sh
# Everything stored for a workspace, as one JSON archive.
curl -b sdi_session=... -H 'X-Sdi-Workspace: <id>' \
  -X POST https://sdi.example.com/api/v1/workspaces/<id>/data/export -o export.json

# Drop cached file contents now, regardless of the cache mode.
curl -b sdi_session=... -H 'X-Sdi-Workspace: <id>' \
  -X POST https://sdi.example.com/api/v1/workspaces/<id>/data/purge-code-cache

# Delete the workspace and everything in every schema. Owner only.
curl -b sdi_session=... -H 'X-Sdi-Workspace: <id>' \
  -X DELETE https://sdi.example.com/api/v1/workspaces/<id>
```

For an organisation, uninstalling the SDI GitHub App from GitHub sends an
`installation` webhook that removes the workspace the same way. For a
personal workspace, revoking the OAuth grant in your GitHub settings ends
every session; the workspace stays until you delete it so you can export
first.

Self-hosters hold the database: `docs/self-hosting.md` covers backups, and
dropping the volume is the ultimate delete.

## Who you are trusting

- **Self-hosted** (the recommended path for organisations): nobody but
  yourselves. The stack runs inside your network; GitHub and, if enabled,
  the model API are the only outbound calls.
- **A hosted instance** run by someone else: that operator holds the
  database and the `SDI_SESSION_SECRET`. Everything on this page still
  applies to the software; ask the operator for their own statement about
  the machines. [`PRIVACY.md`](PRIVACY.md) is the template for that.

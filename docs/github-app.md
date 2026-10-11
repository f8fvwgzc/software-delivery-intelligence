# Create the GitHub App

Identity in SDI is GitHub (`CONTRACT.md` §18). Individuals sign in through
a **GitHub OAuth app**; organisations install the **SDI GitHub App**. Both
are registered by you, on your GitHub account or organisation, so the
client secrets and the private key never leave your hands. This guide
creates both and wires them into an SDI instance. For a developer machine
none of this is needed: `make dev` uses the dev login (`SDI_DEV_LOGIN=1`).

Throughout, `PUBLIC_URL` is the origin users open in the browser, for
example `https://sdi.example.com`. It must match `SDI_PUBLIC_URL` exactly,
scheme included; GitHub compares callback URLs literally.

## 1. OAuth app (individuals)

GitHub → **Settings → Developer settings → OAuth Apps → New OAuth App**.

| Field | Value |
|---|---|
| Application name | `SDI` (or your instance's name) |
| Homepage URL | `PUBLIC_URL` |
| Authorization callback URL | `PUBLIC_URL/api/v1/auth/callback` |
| Enable Device Flow | off |

Generate a client secret. Put both values in `.env`:

```sh
GITHUB_OAUTH_CLIENT_ID=Iv1.…
GITHUB_OAUTH_CLIENT_SECRET=…
```

Scopes are requested by SDI at login time (`read:user user:email`, plus
`repo` for the personal workspace, because GitHub has no read-only scope
for private repositories); nothing to configure here.

A GitHub App can also act as the OAuth provider (its own client id and
secret under **Optional features → User authorization**), which saves
registering two things. SDI accepts either; the variable names stay the
same.

## 2. The SDI GitHub App (organisations)

GitHub → **Settings → Developer settings → GitHub Apps → New GitHub App**
(for an organisation-owned app: the organisation's **Settings → Developer
settings**).

### Basics

| Field | Value |
|---|---|
| GitHub App name | `SDI` or `sdi-<org>` (globally unique; its slug becomes `GITHUB_APP_SLUG`) |
| Homepage URL | `PUBLIC_URL` |
| Callback URL | `PUBLIC_URL/api/v1/auth/callback` |
| Request user authorization (OAuth) during installation | on, if this App is also your OAuth provider |
| Setup URL | `PUBLIC_URL/api/v1/github/app/setup` |
| Redirect on update | on |
| Webhook: Active | on |
| Webhook URL | `PUBLIC_URL/api/v1/webhooks/github` |
| Webhook secret | `openssl rand -hex 32`; the same value goes into `GITHUB_WEBHOOK_SECRET` |
| SSL verification | enabled |

The setup URL is where GitHub sends the installer after installation with
`?installation_id=…`; SDI records the installation as a workspace there.
The webhook URL carries every event below; the edge forwards it to the
gateway outside the normal mutation rate limit, with a 1 MB body cap
(`deploy/nginx/conf.d/default.conf`), and the gateway verifies
`X-Hub-Signature-256` against the secret before reading the body.

### Permissions

Exactly these, from `CONTRACT.md` §18. Fewer breaks a feature; more is
never needed and is a bug if SDI asks for it.

| Section | Permission | Access | Why |
|---|---|---|---|
| Repository | Contents | Read-only | Tree, analyzer inputs, tarballs for Docker dry runs |
| Repository | Metadata | Read-only | Repository list and attributes (mandatory) |
| Repository | Actions | Read-only | Workflows and runs |
| Repository | Deployments | Read-only | Deployment history |
| Repository | Pull requests | Read and write | The `github` execution adapter opens pull requests; nothing else writes |
| Organization | Members | Read-only | Membership and teams → workspace roles |

### Subscribe to events

| Event | SDI uses it for |
|---|---|
| Installation | create the workspace; remove it (and everything in it) on uninstall |
| Installation repositories | keep the list of connectable projects equal to the App's repository selection |
| Push | mark the project stale and schedule a rescan on the default branch |
| Pull request | keep delivery facts and pattern memory fresh |
| Member | re-derive a member's role when org membership changes |
| Team | re-derive roles when `sdi-admins` / `sdi-approvers` membership changes |

### Where can this GitHub App be installed?

**Any account** if you run one instance for several organisations; **Only
on this account** for a single organisation. Either way one installation
is one workspace.

### Private key

After creating the App, **Generate a private key**. GitHub downloads a
`.pem`; this is the only copy. Store it where you keep secrets (a secrets
manager, or a mode-600 file on the host outside the repository) and hand
it to the identity service through `GITHUB_APP_PRIVATE_KEY`:

```sh
# .env, one line, base64 of the whole PEM (identity decodes it)
GITHUB_APP_PRIVATE_KEY=$(base64 < sdi.2026-01-01.private-key.pem | tr -d '\n')

# or in a shell that starts compose, the PEM itself
export GITHUB_APP_PRIVATE_KEY="$(cat sdi.2026-01-01.private-key.pem)"
```

Never commit the file. `.gitignore` already excludes `.env*`; do not put
the PEM under the repository at all. Rotating: generate a new key on
GitHub, update the variable, restart identity (`docker compose up -d
identity`), then delete the old key on GitHub.

### Values for `.env`

From the App's **General** page:

```sh
GITHUB_APP_ID=123456            # "App ID"
GITHUB_APP_SLUG=sdi-example     # from the public link, github.com/apps/<slug>
GITHUB_APP_PRIVATE_KEY=…        # see above
GITHUB_WEBHOOK_SECRET=…         # what you typed in the Webhook secret field
```

Restart the identity service and the gateways:

```sh
docker compose up -d identity gateway-1 gateway-2
```

## 3. Install it

Open `PUBLIC_URL`, sign in, and choose **Add organisation**; SDI redirects
to `GET /api/v1/github/app/install-url`, which sends the installer to
GitHub's install page. Pick the organisation and **Only select
repositories** (recommended; SDI never sees anything outside the
selection). GitHub sends the installer back to the setup URL, SDI records
the workspace, and the installer, as an organisation owner, becomes its
`OWNER`.

Roles for everyone else are derived at their first visit and kept fresh by
the `member` and `team` webhooks:

| GitHub | SDI role |
|---|---|
| organisation owner | `OWNER` |
| member of team `sdi-admins` | `ADMIN` |
| member of team `sdi-approvers` | `APPROVER` |
| other organisation member | `MEMBER` |
| outside collaborator | `VIEWER` |

Create the two teams on GitHub if you want admins and approvers who are
not organisation owners. An SDI admin can lower a member's role in
**Members**, never raise it.

## 4. GitHub Enterprise Server

Register the App on the GHES instance instead of github.com; the fields
are the same. Set `GITHUB_API_URL=https://ghes.example.com/api/v3` for
the knowledge and execution services, and the installation's
`api_base_url` is recorded from the webhook so a single SDI instance can
serve github.com and GHES installations at once.

## 5. Checking it works

```sh
# Deliveries: GitHub App → Advanced → Recent Deliveries. Each should be 2xx.
# A 401 means the secret differs; a 429 means the edge throttled (10 r/s burst 20).
# A 413 means a payload over 1 MB; raise client_max_body_size in default.conf.

# From the instance:
docker compose logs -f identity gateway-1 | grep -i 'webhook\|installation'
curl -s -b sdi_session=… PUBLIC_URL/api/v1/auth/me | jq .workspaces
```

| Symptom | Cause |
|---|---|
| `redirect_uri_mismatch` from GitHub | Callback URL on GitHub differs from `SDI_PUBLIC_URL` + `/api/v1/auth/callback` (scheme, host, port, trailing slash) |
| Login works, no organisation appears | The App is installed but the setup URL never received the redirect; open `PUBLIC_URL/api/v1/github/app/setup?installation_id=<id>` once, or reinstall |
| Members see `VIEWER` | They are outside collaborators, or membership is private and the App lacks `Members: read` |
| Webhooks 401 | `GITHUB_WEBHOOK_SECRET` does not match the App's secret |
| Identity refuses to start | `SDI_SESSION_SECRET` unset or under 32 bytes, or the private key fails to parse (check base64 vs PEM) |

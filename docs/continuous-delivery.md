# Continuous delivery

CI builds an image. Continuous delivery moves that exact image through your
environments. SDI does not replace the tool that deploys; it drives it, and
puts every deploy behind the same gates as the rest of the platform:
permission, policy, approval, act, verify, checkpoint, and stop on failure.

The contract is `CONTRACT.md` §22. This page is the how-to.

## The five things you configure

| Thing | Where | What it is |
|---|---|---|
| Registry | Workspace → Registries | Where images live. GHCR is the default and needs nothing. |
| Secrets | Workspace → Secrets | Tokens the deployers need, stored encrypted, referenced by name. |
| Environments | Project → Pipeline | The stages a release moves through, in order, each with a deployer. |
| Releases | Project → Pipeline | One image at one commit. Created automatically or by hand. |
| Policies | Governance → Policies | Which deploys run on their own and which wait for a person. |

## 1. Registry

GHCR works out of the box: images go to `ghcr.io/<workspace>/<repository>`
and the build workflow logs in with the workflow's own `GITHUB_TOKEN`.

For anything else, add a registry and pick how CI authenticates:

| Registry | Recommended auth | You provide |
|---|---|---|
| Azure Container Registry | OIDC | host `myorg.azurecr.io`, a federated credential for the repository |
| Amazon ECR | OIDC | host `<account>.dkr.ecr.<region>.amazonaws.com`, an IAM role that trusts GitHub |
| Google Artifact Registry | OIDC | host `<region>-docker.pkg.dev`, a workload identity provider |
| Docker Hub, generic | Secret | two workspace secrets: a username and a token |

With OIDC, SDI cannot read the registry itself, so the "verify image" step
reports that it was skipped. With a secret or GHCR it confirms the manifest
exists and records its digest before anything is deployed.

## 2. Environments and deployers

Add environments in promotion order. Each one chooses how a release gets
there:

| Deployer | SDI does | You need |
|---|---|---|
| GitHub Actions | Dispatches `deploy.yml` with `environment`, `image`, `digest`, `version` and follows the run | A deploy workflow (SDI generates one) |
| Flux, Argo CD | Updates the image tag in the GitOps repository by pull request and waits for the merge | The repository, file path and edit strategy |
| SDI GitOps | Syncs an SDI application with the release image as its image override and waits for it to be healthy; no Argo CD needed | An application on a cluster ([`gitops.md`](gitops.md)); config `application` |
| Jenkins | Triggers the job with `IMAGE`, `DIGEST`, `VERSION`, `ENVIRONMENT` and polls the build | Job URL and name, a user and API token as secrets |
| Azure DevOps | Runs the pipeline with the same four parameters and polls it | Organization, project, pipeline id, a PAT as a secret |
| Webhook | Sends a signed `POST` with the release | A URL and a signing secret |
| Local Docker | Runs the image on the machine SDI runs on | The `local-exec` compose profile; development only |

Two switches per environment:

- **Protected** gives production semantics: approval is always required and
  cannot be relaxed.
- **Auto promote** deploys into the stage as soon as the previous one
  succeeds. It is still subject to the stage's policy.

Give an environment a URL and health path and SDI polls it after each
deploy; a failing check pauses the run and offers a rollback.

## 3. Config as code

Everything above can live in the repository instead:

```yaml
# .sdi.yml
version: 1
delivery:
  registry: { kind: ghcr }
  build_workflow: build.yml
  environments:
    - name: staging
      kind: github_actions
      url: https://staging.example.com
      health_path: /health
      auto_promote: true
    - name: production
      kind: gitops_flux
      protected: true
      config:
        repo: acme/fleet
        path: apps/api/kustomization.yaml
        strategy: kustomize_image
```

Environments declared this way are read-only in the app and are replaced
at every scan. A file with errors changes nothing; the Understanding page
shows what is wrong.

## 4. Setup files

**Pipeline → Setup files** generates what your repository needs for the
registry and deployers you chose: the build-and-push workflow, the deploy
workflow, Flux or Argo CD manifests, a `Jenkinsfile`, an
`azure-pipelines.yml`. It lists the secrets those files reference and
whether each one exists. **Open pull request** commits them to a branch;
nothing is written to the default branch.

Existing files are not overwritten: a deploy variant is written beside
them.

## 5. Releasing and promoting

A release appears when the build workflow succeeds on the default branch,
when a package is published, or when you create one by hand.

The pipeline board shows, per stage, what runs there now and what could go
next. **Promote** starts a deployment run:

1. Verify the image exists in the registry.
2. Inspect the image, when the stage asks for it (next section).
3. Deploy through the stage's deployer.
4. Verify health.
5. Record a GitHub Deployment so the repository shows the environment.

Each run is an execution, so approvals, live logs, failure diagnosis and
remediation work as they do everywhere else. **Rollback** redeploys the
previous successful release of that stage.

The first promote in a workspace pauses and asks for access. Deploy
permissions are granted per run and expire; that is by design.

## 6. Inspecting the image before it deploys

"We upgraded the vulnerable library, the build passed, it deployed" is
what the repository says. The image is what actually runs. A stage with
**Image inspection** on reads the release image's layers from the registry,
exactly the digest the registry just confirmed, and reverse-engineers what
ships before anything is deployed:

- the dependency versions compiled into Go binaries (build info) and Rust
  binaries built with `cargo auditable`, the Go toolchain that built them,
  and the packages of the base image, `node_modules` and `site-packages`,
  all looked up at OSV;
- binary hardening of your own binaries: PIE, non-executable stack,
  RELRO, stack protector, FORTIFY, RPATH, debug symbols;
- provenance: your Go binary built from another commit than the release
  says, or from uncommitted changes;
- what must never ship: private keys, `.env` files, credentials in `ENV`,
  source maps, a root user, an end-of-life distribution.

Pick per stage, in the environment editor or in `.sdi.yml`:

| mode | what happens |
|---|---|
| Off (default) | the plan is the four steps above |
| Report | the release gets a report (the tag on the release opens it); the run always goes on |
| Gate | findings at or above **Stop at** (default high), minus the stage's **Accepted rules**, pause the run before the deploy. **Deploy anyway** continues and records that a person accepted them; **Cancel** is the recommended way out: fix, rebuild, release again |

```yaml
delivery:
  inspect: { mode: report, fail_on: high, ignore: [binary.no_pie] }   # every stage's default
  environments:
    - name: staging
      kind: github_actions
    - name: production
      kind: gitops_argocd
      protected: true
      config: { path: apps/api/values.yaml, strategy: helm_values, inspect: gate }
```

A rollback is never inspected (the release was, when it was promoted),
and each promotion inspects again: an advisory published between staging
and production is caught at production. The report names files,
packages, versions and variable names, never contents or values. The
readers are Rust ports of [REA](https://github.com/morluto/rea)'s static
analyzers; set `SDI_REA_BIN` to an installed `rea` CLI to have it run over
your binaries too. `OSV_API_URL=off` keeps the lookups inside an
air-gapped network; `SDI_INSPECT_MAX_MB` (default 2048) bounds what one
inspection downloads.

## GitHub App permissions

Delivery needs more than analysis does. Without a permission, the step
that needs it pauses with a clear message; nothing falls back to a broader
credential.

| Permission | Used for |
|---|---|
| `actions: write` | dispatching the deploy workflow |
| `deployments: write` | recording the deployment |
| `packages: read` | verifying an image in GHCR |
| `contents: write` | GitOps commits and setup-file pull requests |

## Self-hosted networks

Deployers, registries and health checks refuse loopback, private and
metadata addresses by default. The refusal happens twice: when an
environment or registry is saved with an address that is plainly private
(an IP literal or `localhost`), and again when the call is made, after the
host name has been resolved, so a name that later moves to a private
address is caught too. If your Jenkins or cluster endpoints are on a
private network that SDI should reach, set
`SDI_ALLOW_PRIVATE_DEPLOY_TARGETS=1` on the execution service.

## Who did what

Releases, deployment runs and approvals record the person who acted, and
the app shows them by GitHub login and avatar. Someone who has since left
the workspace keeps their name in its history, marked as no longer a
member.

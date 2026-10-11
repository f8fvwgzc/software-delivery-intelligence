# Execution adapters

An adapter is the runtime an execution uses to perform its steps. v1 only had
`simulated`; phase 2 adds two that touch the real world, `docker` and
`github`. The normative table is
[`CONTRACT.md` §5.4](../CONTRACT.md#54-adapters); this page explains what
each one does, what it needs, and how to run it safely.

## The gates do not change

Whatever the adapter, every step passes the same sequence before and after
it runs:

```
permission ─▶ policy ─▶ approval ─▶ run ─▶ verify ─▶ checkpoint
```

- **Permission.** The executing identity must hold every
  `required_permission` of the step (`<resource>:<verb>`). A missing grant
  fails the step with `PERMISSION_DENIED` *before* anything runs.
- **Policy.** The step's `action_type` has a default decision
  (`CONTRACT.md` §5.2); `approval` and `required` create an `Approval` the
  human decides in the UI.
- **Verify, checkpoint.** The adapter reports success; the service verifies
  (`PlanStep.verification`) and writes a `CHECKPOINT` event. On failure the
  run pauses, is diagnosed, and the human picks a remediation.

A real adapter therefore cannot do anything a dry run would not have asked
permission for. What changes is that the step's side effect is real.

## Adapters

| adapter | what it does | steps it can run |
|---|---|---|
| `simulated` (default) | timed dry run, fault injection, never touches anything | every action type |
| `docker` | real local execution on the host's Docker daemon: `repo.checkout` downloads the GitHub tarball at the scan commit into `.run/<execution>/`, `container.build` runs `docker build`, `container.run` starts the image detached with the configured port and polls `health_path`, `tests.run_local` runs `test_command` inside the image, `env.destroy_temporary` stops and removes everything; stdout/stderr lines become `LOG` events | `repo.checkout`, `container.build`, `container.run`, `tests.run_local`, `env.create_temporary`, `env.destroy_temporary`, `deps.install` |
| `github` | writes to the connected repository with the configured token: `repo.open_pull_request` creates branch `sdi/<proposal-id>` from the default branch, commits `CreateExecutionRequest.artifacts` via the Git Data API, opens the PR and records `outputs.pull_request_url` | `repo.checkout`, `repo.open_pull_request` |
| `cloud`, `kubernetes` | reserved; `UNIMPLEMENTED` | — |

`CreateExecutionRequest.adapter` picks the runtime for the whole run; each
step's `PlanStep.adapter` names the tool family it needs. The execution
service refuses a run (`INVALID_ARGUMENT`) whose steps need a tool the chosen
runtime cannot provide, and a plan may mix families (build locally, then
open a PR) as long as the chosen runtime is not `simulated`.

### Identities and what they hold

| identity | permissions | exists |
|---|---|---|
| `simulated` | `repository:read`, `container:build`, `environment:write` | always |
| `local-docker` | the above plus `container:run` | always |
| `github-token` | `repository:read`, `repository:write` | only while `GITHUB_TOKEN` is configured (it *is* the connected token) |

Anything else is a time-boxed grant (default 30 minutes):

```sh
G=http://localhost/api/v1
IID=$(curl -s $G/identities | jq -r '.[] | select(.name=="local-docker") | .id')
curl -s -X POST $G/identities/$IID/grants -H 'content-type: application/json' \
  -d '{"permissions":["database:write"],"ttlMinutes":60}'
```

## "Verify locally with Docker"

The action on an accepted deployment or modernization option that builds
and tests the repository on your machine before anything is deployed. It
creates an execution with `adapter: docker` and a plan of

```
repo.checkout ─▶ container.build ─▶ container.run ─▶ tests.run_local ─▶ env.destroy_temporary
```

| step | what happens | policy default |
|---|---|---|
| `repo.checkout` | downloads the repository tarball at the scanned commit into `SDI_RUN_DIR/<execution>/` | safe, auto |
| `container.build` | `docker build` of the repository's Dockerfile (or the generated one from the option's artifacts) | medium, auto |
| `container.run` | starts the image detached, publishes `test.port`, polls `test.health_path` until it answers | medium, **approval** |
| `tests.run_local` | runs `test.command` inside the image; stdout and stderr stream into the execution rail as `LOG` events | safe, auto |
| `env.destroy_temporary` | stops and removes the container and the image's temporary tags | safe, auto |

**Requires**

- *Permissions* `container:build` and `container:run`. The seeded
  `local-docker` identity holds both; `simulated` lacks `container:run`, so
  a run under it stops with `PERMISSION_DENIED` at `container.run` (resolve
  with `request_access`, or pick the right identity).
- *Approval* for `container.run` (default policy); the execution waits in
  `WAITING_FOR_APPROVAL` until it is decided on the Approvals page. The
  policy is editable if you want local runs to go through unattended.
- *Test parameters* from `.sdi.yml` (`test.command`, `test.health_path`,
  `test.port`, see [`configuration.md`](configuration.md)): without them
  `tests.run_local` has nothing to run and `container.run` nothing to poll.
- *A Docker daemon the execution service can reach*:
  - on the host (`make dev`), the CLI default `/var/run/docker.sock`;
    `scripts/dev.sh` sets `SDI_RUN_DIR` to an absolute `.run/` under the
    repository because the daemon resolves paths on the host;
  - in the Docker stack, the **`local-exec` profile** (next section) or a
    `DOCKER_HOST` pointing at a remote daemon.
- *A read-capable `GITHUB_TOKEN`* for the tarball of a private repository.

### The `local-exec` compose profile

```sh
make up-local-exec          # docker compose --profile local-exec up --build -d --wait
make down                   # removes the relay too
```

The profile adds one container, `docker-socket`: a `socat` relay that mounts
the host's `/var/run/docker.sock` and republishes it into the `docker-sock`
volume, which the execution service mounts at `/run`. Inside execution the
daemon then sits at the default `/var/run/docker.sock`, so `DOCKER_HOST`
stays unset; without the profile the volume is empty and `docker` steps
cannot reach a daemon. The execution image is the only one
that ships the Docker CLI and buildx (`deploy/docker/rust.Dockerfile`,
`extras` stage), and its `SDI_RUN_DIR` is the `rundata` named volume because
the container's root filesystem is read-only.

The relay exists because a compose profile can enable a *service*, not a
*volume line* on an existing service, and the alternative (a second
execution service) would leave two containers answering to the name
`execution`.

**The trade-off, in plain words.** The Docker socket is root on the host.
Whoever can talk to it can run `docker run --privileged -v /:/host` and own
the machine. With the profile on, the execution service, and anything that
can make it run a plan, can start containers on this host. `cap_drop`,
`read_only` and `no-new-privileges` on the execution container still apply,
but they limit what its own process may do, not what it may ask the daemon
for. So:

- use the profile on a developer machine, for your own repositories;
- never on a shared or internet-facing host: there, put a filtering proxy
  (`tecnativa/docker-socket-proxy`, which can deny `exec`, `privileged`,
  volume mounts) or rootless Docker behind `DOCKER_HOST` and leave the
  profile off;
- the policy gate is your second line: `container.run` requires approval
  by default, and `resource.delete` is non-editable `required`.

Container-security guidance is blunt about this ("never mount the Docker
socket in a container; use the remote API with TLS"), and the default stack
honours it: the profile is opt-in, documented, and visible in
`docker compose ps` as its own container.

## "Open a pull request"

The action on an accepted option whose artifacts (generated files,
`CONTRACT.md` §14) should land in the repository.
`POST /proposals/{id}/pull-request {optionId?}` accepts the proposal and
creates an execution with `adapter: github` and the plan

```
repo.checkout ─▶ repo.open_pull_request
```

| step | what happens | policy default |
|---|---|---|
| `repo.checkout` | the checkout of the scanned commit, so the artifacts are applied against the tree they were generated from | safe, auto |
| `repo.open_pull_request` | creates branch `sdi/<proposal-id>` from the default branch, commits every artifact through the Git Data API (blobs → tree → commit → ref), opens the pull request and records `outputs.pull_request_url` on the execution | medium, **approval** |

Artifacts that would overwrite a file the repository already has carry
`replaces_existing=true` and are written only when the option says so.
Nothing is pushed to the default branch, ever.

**Requires**

- *Permission* `repository:write`. The seeded `github-token` identity holds
  it, and only exists while `GITHUB_TOKEN` is configured, which is what
  makes "can the platform write to GitHub" a one-line answer from
  `GET /identities`.
- *Approval* for `repo.open_pull_request` (default policy, editable).
- *Token scope.* The same `GITHUB_TOKEN` the knowledge service reads with,
  now with write access to the one repository:
  - classic PAT: `repo` (already needed to read private repositories);
  - fine-grained token: **Contents: read and write** and **Pull requests:
    read and write** on the repository, plus Metadata: read. Actions and
    Deployments: read remain for the pipeline and deployment views.
  Pass it to the execution service too: compose does
  (`GITHUB_TOKEN: ${GITHUB_TOKEN:-}` on `execution`), and `scripts/dev.sh`
  exports it for every host process.
- *An option with artifacts.* `GET /proposals/{id}` reports
  `artifactCount` per option; the files themselves are at
  `GET /proposals/{id}/options/{optionId}/artifacts`.

## Troubleshooting

| Symptom | Cause | Fix |
|---|---|---|
| `INVALID_ARGUMENT: step … needs adapter docker` on create | the chosen runtime cannot provide a step's tool family | pick `adapter: docker` (or `github`) for the run; `simulated` runs everything but mixes with nothing |
| `PERMISSION_DENIED` at `container.run` | identity lacks `container:run` | use the `local-docker` identity or grant the permission with a TTL |
| `PERMISSION_DENIED` at `repo.open_pull_request` | no `github-token` identity: `GITHUB_TOKEN` unset where execution runs | export it (`.env`, or `make up` with the variable set) and restart execution |
| `BUILD_FAILED` with "cannot connect to the Docker daemon" | no socket in the execution container | `make up-local-exec`, or set `DOCKER_HOST` |
| `HEALTH_CHECK_FAILED` right after `container.run` | wrong `test.port`/`test.health_path`, or the app binds `127.0.0.1` inside its container | fix `.sdi.yml`; make the app listen on `0.0.0.0` |
| GitHub answers 403 on the PR step | token lacks Contents/Pull requests write, or SSO is not authorised for the organisation | widen the token's scope; authorise it for the org |
| PR opened against the wrong base | the scan is older than the default branch head | rescan, regenerate the proposal, open again |

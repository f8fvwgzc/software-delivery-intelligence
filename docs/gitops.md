# GitOps: deploy to Kubernetes without running Argo CD

SDI carries Argo CD's core. You declare an application (a folder in a
repository, a cluster, a namespace) and SDI renders it, compares it with
the cluster, syncs it, keeps it healthy and remembers every sync. Every
sync a person starts is an execution with an approval, like any other
change SDI makes.

The contract is `CONTRACT.md` §24. This page is the how-to.

## What it is made of

The `gitops` service (Go, gRPC on 50055) embeds Argo CD's own
**gitops-engine** library, pinned to a stable Argo CD release, for the
parts that make Argo CD reliable:

| Argo CD part | In SDI |
|---|---|
| Cluster cache (watches on every kind) | the engine's cache, one per cluster, dropped after 30 minutes unused |
| Diff with Kubernetes normalisation | the engine's diff; `Secret` values are masked as `++++++++` |
| Sync: phases, waves, hooks, prune | the engine's sync: `PreSync → Sync → PostSync`, `SyncFail` hooks, `argocd.argoproj.io/sync-wave`, `argocd.argoproj.io/hook` and `hook-delete-policy` work as in Argo CD |
| Health checks | the engine's checks per resource; the application takes the worst |
| repo-server | renders plain YAML, Kustomize and Helm in process, with no network |
| application controller | reconcile loop, auto-sync, self-heal, retry with backoff, history, rollback |

SDI keeps its own roles, approvals, audit and events around it. Argo CD's
UI, SSO, RBAC, ApplicationSets, plugins and notifications are not
included (§24.9).

Resources are tracked with the annotation
`sdi.dev/tracking-id: <application>:<group>/<kind>:<namespace>/<name>`.
SDI never adopts, overwrites or prunes a resource it did not create.

## 1. Add a cluster

1. **Workspace, Secrets**: add a secret holding the cluster's kubeconfig.
   Paste it into the value field; a value with several lines switches the
   field to a text area. The kubeconfig must carry its credentials inline
   (a token or a client certificate). Exec plugins (`aws eks get-token`,
   `gke-gcloud-auth-plugin`) are refused: create a service account token
   for SDI instead.
2. **Workspace, Clusters**: add a cluster that names that secret. Saving
   connects once and shows the server version, or what went wrong.

A cluster whose API server is on a private address (a local cluster, a
VPN) is refused unless the instance sets
`SDI_ALLOW_PRIVATE_DEPLOY_TARGETS=1`. `make dev` sets it, because a
cluster on your machine is one.

To limit what SDI can touch, give the cluster a namespace list: the engine
then watches and writes only those namespaces, and the kubeconfig's user
only needs rights there.

## 2. Add an application

Project, **Applications**, New application:

| Field | Meaning |
|---|---|
| Repository | default: the project's repository |
| Revision | a branch, tag or commit; default: the default branch |
| Path | the folder with the manifests |
| Type | `directory`, `kustomize`, `helm`, or automatic (`kustomization.yaml` → kustomize, `Chart.yaml` → helm, else plain YAML) |
| Cluster, namespace | where it goes; the namespace can be created on the first sync |
| Sync policy | manual (the default), or automated with prune and self-heal |

Helm charts render client-side (`helm template`), so dependencies must be
vendored under `charts/`. Kustomize cannot load remote bases.

## 3. Sync

**Sync** on the application page starts an execution:

```
gitops.preview → gitops.sync → gitops.verify_health
```

The preview is a dry run that lists what would be created, changed or
pruned. The sync waits for approval and needs the `cluster:write`
permission, which nobody holds: the run pauses and asks for access, and
what you grant expires. Verify health waits until the application is
healthy and fails the run on degraded or missing.

**Refresh** compares again now; a hard refresh renders the source again
too. The diff tab shows the change per resource, the manifests tab the
rendered state.

## 4. History and rollback

Every sync that is not a dry run is a history entry: the revision, the
source as synced, the result, who started it and the execution. **Roll
back** on an entry syncs that source at that revision, through the same
approval. A rollback is refused while automated sync is on, as in Argo CD,
because the next reconcile would sync forward again.

## 5. Automated sync and self-heal

With automated sync on, the reconcile loop (every
`GITOPS_RECONCILE_SECONDS`, default 180) syncs the application when a new
commit changes what it renders. With self-heal it also puts back drift: a
resource someone changed or deleted by hand. A failed automated sync is
retried with backoff (10 s, 20 s, 40 s, up to 5 minutes) `retry_limit`
times, then waits for the next change.

This is the one path that reaches a cluster without an approval, so only
an admin can turn it on, per application. Every automated sync is in the
history and the audit log as `system`.

## 6. Continuous delivery

An environment of kind **SDI GitOps** (`sdi_gitops`) deploys a release by
syncing an application with the release image as the image override:

```yaml
# .sdi.yml
delivery:
  environments:
    - name: staging
      kind: sdi_gitops
      config:
        application: 7c0e0a52-0d43-4a8e-9a8f-1d0f5f0b4c11
```

Promotion runs `registry.verify_image → deploy.staging (gitops.sync) →
gitops.verify_health → deploy.record`. A protected stage keeps its
required approval whatever the `gitops.sync` policy says. For a Kustomize
source the override is a `kustomize` image; for Helm it is the
`image.repository` and `image.tag` parameters.

## Running it

| Where | How |
|---|---|
| `make dev` | builds the Go binary into `target/debug/sdi-gitops` and runs it with the others; needs Go 1.21 or newer, which fetches the exact toolchain on first build |
| `make up` | the `gitops` container (`deploy/docker/go.Dockerfile`, distroless, non-root) |
| Kubernetes | `deploy/k8s/gitops.yaml`, one replica: the reconcile loop and the cluster caches live in the process |

| Variable | Default | |
|---|---|---|
| `GITOPS_LISTEN` / `GITOPS_GRPC_URL` | `0.0.0.0:50055` / `http://127.0.0.1:50055` | |
| `GITOPS_RECONCILE_SECONDS` | `180` | minimum 30, `0` turns the loop off |
| `GITOPS_MAX_CLUSTERS` | `64` | cluster caches held at once |
| `SDI_ALLOW_PRIVATE_DEPLOY_TARGETS` | unset (`1` under `make dev`) | allow API servers on private addresses |

## Trying it on a throwaway cluster

```sh
export KUBECONFIG="$PWD/.run/minikube.kubeconfig"   # leaves ~/.kube/config alone
minikube start -p sdi-gitops-test
kubectl config view --flatten --minify > .run/sdi-gitops-test.kubeconfig
unset KUBECONFIG
```

`.run/` is git-ignored. `--flatten` puts the certificates inline, which
SDI needs. Store that file as a workspace secret, add the
cluster, and point an application at `examples/gitops/hello` in this
repository. The Go integration tests use the same cluster:

```sh
cd services/gitops
SDI_GITOPS_TEST_KUBECONFIG=../../.run/sdi-gitops-test.kubeconfig \
SDI_ALLOW_PRIVATE_DEPLOY_TARGETS=1 \
DATABASE_URL=postgres://sdi:sdi@127.0.0.1:5440/sdi \
go test ./... -count=1
```

They create and delete their own `sdi-gitops-test-*` namespaces.

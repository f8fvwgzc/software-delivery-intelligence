//! The GitOps engine (CONTRACT.md §24): clusters, applications, what they
//! render to, how they differ from the cluster, and their sync history.
//!
//! Create and update bodies are flat objects with the fields the service
//! reads; lists are bare arrays. Strings the service leaves empty are `null`
//! when they stand for "not set". A cluster names the workspace secret that
//! holds its kubeconfig: no shape here has a field for the kubeconfig itself,
//! so it can be neither sent nor answered. Manifests and diffs arrive with
//! every `Secret` value already masked by the service.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use sdi_proto::gitops::v1 as proto;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use super::{opt_string, opt_timestamp, timestamp};

/// A closed vocabulary the service sends as a lowercase string. Values this
/// gateway does not know (a newer service) become the fallback variant
/// instead of failing the whole response.
macro_rules! vocabulary {
    (
        $(#[$meta:meta])*
        $name:ident fallback $fallback:ident {
            $($(#[$vmeta:meta])* $variant:ident = $text:literal),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
        #[serde(rename_all = "snake_case")]
        pub enum $name {
            $($(#[$vmeta])* $variant),+
        }

        impl $name {
            /// The service's word for it.
            #[allow(dead_code)]
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text),+
                }
            }

            /// The service's word, or the fallback for one this build does not know.
            pub fn parse(raw: &str) -> Self {
                match raw {
                    $($text => Self::$variant,)+
                    _ => Self::$fallback,
                }
            }
        }
    };
}

vocabulary! {
    /// Whether the cluster answered the last time SDI reached it.
    ClusterStatus fallback Unknown {
        /// It answered (`serverVersion` says with what).
        Connected = "connected",
        /// It did not; `message` says why.
        Unreachable = "unreachable",
        /// Not checked yet.
        Unknown = "unknown",
    }
}

vocabulary! {
    /// How the live state compares with the desired state. An application is
    /// `synced`, `out_of_sync` or `unknown`; a resource is also `missing`
    /// (desired, not in the cluster) or `extraneous` (in the cluster, no
    /// longer desired: a sync with prune deletes it).
    SyncStatus fallback Unknown {
        /// Live matches desired.
        Synced = "synced",
        /// Live differs from desired.
        OutOfSync = "out_of_sync",
        /// Desired, not in the cluster.
        Missing = "missing",
        /// In the cluster, no longer desired.
        Extraneous = "extraneous",
        /// Not compared yet, or the comparison could not finish.
        Unknown = "unknown",
    }
}

vocabulary! {
    /// Health from Argo CD's checks; an application's is the worst of its resources'.
    HealthStatus fallback Unknown {
        /// Running as intended.
        Healthy = "healthy",
        /// Not there yet (a rollout in progress).
        Progressing = "progressing",
        /// Failing.
        Degraded = "degraded",
        /// Paused on purpose (a suspended CronJob, a paused rollout).
        Suspended = "suspended",
        /// Not in the cluster.
        Missing = "missing",
        /// No verdict.
        Unknown = "unknown",
    }
}

vocabulary! {
    /// What an operation does.
    OperationKind fallback Unknown {
        /// Applies the desired state.
        Sync = "sync",
        /// Syncs the source of an earlier history entry.
        Rollback = "rollback",
        /// A kind this gateway build does not know.
        Unknown = "unknown",
    }
}

vocabulary! {
    /// Where an operation is, or how it ended. History entries are
    /// `succeeded`, `failed` or `error`.
    OperationPhase fallback Unknown {
        /// Still applying.
        Running = "running",
        /// Every resource applied.
        Succeeded = "succeeded",
        /// A resource or a hook failed.
        Failed = "failed",
        /// Could not start (render failed, cluster unreachable).
        Error = "error",
        /// Being stopped (its timeout passed).
        Terminating = "terminating",
        /// A phase this gateway build does not know.
        Unknown = "unknown",
    }
}

/// How the source is rendered. Null (not said) detects it:
/// `kustomization.yaml` means kustomize, `Chart.yaml` helm, else a directory.
/// A request body naming any other type is a 400 here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SourceType {
    /// Every `.yaml`, `.yml` and `.json` under the path.
    Directory,
    /// The kustomization at the path, no remote bases.
    Kustomize,
    /// The chart at the path, dependencies vendored under `charts/`.
    Helm,
}

impl SourceType {
    /// The service's word for it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Directory => "directory",
            Self::Kustomize => "kustomize",
            Self::Helm => "helm",
        }
    }
}

/// `""` (detect) is `null`, and so is a type this gateway build does not
/// know: the service detects in both cases.
fn source_type(raw: &str) -> Option<SourceType> {
    [
        SourceType::Directory,
        SourceType::Kustomize,
        SourceType::Helm,
    ]
    .into_iter()
    .find(|t| t.as_str() == raw)
}

// ---------------------------------------------------------------------------
// Clusters
// ---------------------------------------------------------------------------

/// A Kubernetes cluster applications deploy to. Its kubeconfig lives in a
/// workspace secret and is never part of this shape.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Cluster {
    /// Cluster id.
    pub id: String,
    /// Name shown in the workspace.
    pub name: String,
    /// Name of the workspace secret that holds the kubeconfig (never its value).
    pub kubeconfig_secret: String,
    /// Context inside the kubeconfig; null uses its current context.
    pub context: Option<String>,
    /// API server URL read from the kubeconfig, for display; null until it was read.
    pub server: Option<String>,
    /// Namespace for resources that name none; null when not set.
    pub default_namespace: Option<String>,
    /// When not empty, SDI only watches and writes these namespaces; empty watches the whole cluster.
    pub namespaces: Vec<String>,
    /// Whether the cluster answered the last check.
    pub status: ClusterStatus,
    /// Why it is unreachable, as a sentence; null when it is not.
    pub message: Option<String>,
    /// Kubernetes version the API server reported; null until it answered.
    pub server_version: Option<String>,
    /// Applications that deploy to it.
    pub application_count: u32,
    /// When it was last reached; null before the first check.
    pub checked_at: Option<DateTime<Utc>>,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

impl From<proto::Cluster> for Cluster {
    fn from(cluster: proto::Cluster) -> Self {
        Self {
            id: cluster.id,
            name: cluster.name,
            kubeconfig_secret: cluster.kubeconfig_secret,
            context: opt_string(cluster.context),
            server: opt_string(cluster.server),
            default_namespace: opt_string(cluster.default_namespace),
            namespaces: cluster.namespaces,
            status: ClusterStatus::parse(&cluster.status),
            message: opt_string(cluster.message),
            server_version: opt_string(cluster.server_version),
            application_count: cluster.application_count,
            checked_at: opt_timestamp(cluster.checked_at.as_ref()),
            created_at: timestamp(cluster.created_at.as_ref()),
            updated_at: timestamp(cluster.updated_at.as_ref()),
        }
    }
}

/// Body of `POST /clusters` and `PUT /clusters/{id}` (admin). Store the
/// kubeconfig as a workspace secret first and name it here; the cluster is
/// reached once on save and the answer says whether it connected.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpsertClusterRequest {
    /// Name shown in the workspace.
    pub name: String,
    /// Name of the workspace secret that holds the kubeconfig (YAML).
    pub kubeconfig_secret: String,
    /// Context inside the kubeconfig; null uses its current context.
    pub context: Option<String>,
    /// Namespace for resources that name none.
    pub default_namespace: Option<String>,
    /// Only watch and write these namespaces; null or empty watches the whole cluster.
    pub namespaces: Option<Vec<String>>,
}

impl UpsertClusterRequest {
    /// The proto message; `id` is empty to create. Everything else the
    /// service derives (status, server, version) stays unset.
    pub fn into_proto(self, id: String) -> proto::Cluster {
        proto::Cluster {
            id,
            name: self.name,
            kubeconfig_secret: self.kubeconfig_secret,
            context: self.context.unwrap_or_default(),
            default_namespace: self.default_namespace.unwrap_or_default(),
            namespaces: self.namespaces.unwrap_or_default(),
            ..Default::default()
        }
    }
}

// ---------------------------------------------------------------------------
// Applications
// ---------------------------------------------------------------------------

/// Where the desired state comes from (Argo CD's `spec.source`).
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationSource {
    /// `owner/name` on the workspace's GitHub; null is the project's repository.
    pub repo: Option<String>,
    /// Branch, tag or commit SHA; null is the repository's default branch.
    pub revision: Option<String>,
    /// Directory inside the repository; null is its root.
    pub path: Option<String>,
    /// How it is rendered; null detects it.
    #[serde(rename = "type")]
    pub source_type: Option<SourceType>,
    /// Directory sources: include subdirectories.
    pub recurse: bool,
    /// Helm: values files relative to `path`, applied in order.
    pub helm_value_files: Vec<String>,
    /// Helm: `--set` parameters, applied after the values files.
    pub helm_parameters: BTreeMap<String, String>,
    /// Helm: release name; null uses the application name.
    pub helm_release_name: Option<String>,
    /// Kustomize: image overrides (`name=newName:tag`, `name:tag` or `name@sha256:…`).
    pub kustomize_images: Vec<String>,
}

impl From<proto::ApplicationSource> for ApplicationSource {
    fn from(source: proto::ApplicationSource) -> Self {
        Self {
            repo: opt_string(source.repo),
            revision: opt_string(source.revision),
            path: opt_string(source.path),
            source_type: source_type(&source.r#type),
            recurse: source.recurse,
            helm_value_files: source.helm_value_files,
            helm_parameters: source.helm_parameters.into_iter().collect(),
            helm_release_name: opt_string(source.helm_release_name),
            kustomize_images: source.kustomize_images,
        }
    }
}

/// The source in an application's create or update body; every field may
/// be left out.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationSourceInput {
    /// `owner/name` on the workspace's GitHub; null is the project's repository.
    pub repo: Option<String>,
    /// Branch, tag or commit SHA; null is the repository's default branch.
    pub revision: Option<String>,
    /// Directory inside the repository; null is its root.
    pub path: Option<String>,
    /// `directory`, `kustomize` or `helm`; null detects it.
    #[serde(rename = "type")]
    pub source_type: Option<SourceType>,
    /// Directory sources: include subdirectories. Default false.
    pub recurse: Option<bool>,
    /// Helm: values files relative to `path`, applied in order.
    pub helm_value_files: Option<Vec<String>>,
    /// Helm: `--set` parameters, applied after the values files.
    pub helm_parameters: Option<BTreeMap<String, String>>,
    /// Helm: release name; null uses the application name.
    pub helm_release_name: Option<String>,
    /// Kustomize: image overrides (`name=newName:tag`, `name:tag` or `name@sha256:…`).
    pub kustomize_images: Option<Vec<String>>,
}

impl From<ApplicationSourceInput> for proto::ApplicationSource {
    fn from(source: ApplicationSourceInput) -> Self {
        Self {
            repo: source.repo.unwrap_or_default(),
            revision: source.revision.unwrap_or_default(),
            path: source.path.unwrap_or_default(),
            r#type: source
                .source_type
                .map(|t| t.as_str().to_owned())
                .unwrap_or_default(),
            recurse: source.recurse.unwrap_or(false),
            helm_value_files: source.helm_value_files.unwrap_or_default(),
            helm_parameters: source
                .helm_parameters
                .unwrap_or_default()
                .into_iter()
                .collect(),
            helm_release_name: source.helm_release_name.unwrap_or_default(),
            kustomize_images: source.kustomize_images.unwrap_or_default(),
        }
    }
}

/// When the application syncs without an execution (Argo CD's `syncPolicy`).
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SyncPolicy {
    /// Sync on its own when out of sync, as the system user: the only way anything reaches the cluster without an approval.
    pub automated: bool,
    /// Automated syncs also delete resources that left the desired state.
    pub prune: bool,
    /// Automated syncs also undo changes made to the cluster by hand, not only new commits.
    pub self_heal: bool,
    /// Create the destination namespace when it is missing.
    pub create_namespace: bool,
    /// Attempts of a failed automated sync before it waits for a new revision; 0 means 3.
    pub retry_limit: u32,
}

impl From<proto::SyncPolicy> for SyncPolicy {
    fn from(policy: proto::SyncPolicy) -> Self {
        Self {
            automated: policy.automated,
            prune: policy.prune,
            self_heal: policy.self_heal,
            create_namespace: policy.create_namespace,
            retry_limit: policy.retry_limit,
        }
    }
}

/// The sync policy in an application's create or update body; every field
/// may be left out (false, and 3 retries).
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SyncPolicyInput {
    /// Sync on its own when out of sync (an admin's explicit choice).
    pub automated: Option<bool>,
    /// Automated syncs also delete resources that left the desired state.
    pub prune: Option<bool>,
    /// Automated syncs also undo changes made to the cluster by hand.
    pub self_heal: Option<bool>,
    /// Create the destination namespace when it is missing.
    pub create_namespace: Option<bool>,
    /// Attempts of a failed automated sync before it waits for a new revision; 0 or null means 3.
    pub retry_limit: Option<u32>,
}

impl From<SyncPolicyInput> for proto::SyncPolicy {
    fn from(policy: SyncPolicyInput) -> Self {
        Self {
            automated: policy.automated.unwrap_or(false),
            prune: policy.prune.unwrap_or(false),
            self_heal: policy.self_heal.unwrap_or(false),
            create_namespace: policy.create_namespace.unwrap_or(false),
            retry_limit: policy.retry_limit.unwrap_or(0),
        }
    }
}

/// One resource the application manages, live or desired.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResourceStatus {
    /// API group; `""` is the core group, as Kubernetes writes it.
    pub group: String,
    /// API version.
    pub version: String,
    /// Kind, e.g. `Deployment`.
    pub kind: String,
    /// Namespace; null for cluster-scoped resources.
    pub namespace: Option<String>,
    /// Name.
    pub name: String,
    /// `synced`, `out_of_sync`, `missing` or `extraneous`.
    pub sync_status: SyncStatus,
    /// Health; null for kinds without a health check.
    pub health_status: Option<HealthStatus>,
    /// Why it has that health, as the check said it; null when it said nothing.
    pub health_message: Option<String>,
    /// A sync hook (`argocd.argoproj.io/hook`), not part of the steady state.
    pub hook: bool,
    /// `argocd.argoproj.io/sync-wave`.
    pub sync_wave: i32,
}

impl From<proto::ResourceStatus> for ResourceStatus {
    fn from(resource: proto::ResourceStatus) -> Self {
        Self {
            group: resource.group,
            version: resource.version,
            kind: resource.kind,
            namespace: opt_string(resource.namespace),
            name: resource.name,
            sync_status: SyncStatus::parse(&resource.sync_status),
            health_status: (!resource.health_status.is_empty())
                .then(|| HealthStatus::parse(&resource.health_status)),
            health_message: opt_string(resource.health_message),
            hook: resource.hook,
            sync_wave: resource.sync_wave,
        }
    }
}

/// The operation in flight, or the last one that ran.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationOperation {
    /// `sync` or `rollback`.
    pub kind: OperationKind,
    /// Where it is, or how it ended.
    pub phase: OperationPhase,
    /// What happened, as a sentence; null when there is nothing to say.
    pub message: Option<String>,
    /// The commit it applies; null before it was resolved.
    pub revision: Option<String>,
    /// The user id that asked, `system` for an automated sync.
    pub initiated_by: String,
    /// The execution that runs it; null for an automated sync.
    pub execution_id: Option<String>,
    /// When it started.
    pub started_at: DateTime<Utc>,
    /// When it ended; null while it runs.
    pub finished_at: Option<DateTime<Utc>>,
}

impl From<proto::Operation> for ApplicationOperation {
    fn from(operation: proto::Operation) -> Self {
        Self {
            kind: OperationKind::parse(&operation.kind),
            phase: OperationPhase::parse(&operation.phase),
            message: opt_string(operation.message),
            revision: opt_string(operation.revision),
            initiated_by: operation.initiated_by,
            execution_id: opt_string(operation.execution_id),
            started_at: timestamp(operation.started_at.as_ref()),
            finished_at: opt_timestamp(operation.finished_at.as_ref()),
        }
    }
}

/// A path in a repository deployed to a namespace of a cluster.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Application {
    /// Application id.
    pub id: String,
    /// Project it belongs to (its executions run there).
    pub project_id: String,
    /// Name, unique within the project.
    pub name: String,
    /// Cluster it deploys to.
    pub cluster_id: String,
    /// That cluster's name.
    pub cluster_name: String,
    /// Destination namespace; null uses the cluster's default namespace.
    pub namespace: Option<String>,
    /// Where the desired state comes from.
    pub source: ApplicationSource,
    /// When it syncs without an execution.
    pub sync_policy: SyncPolicy,
    /// `synced`, `out_of_sync` or `unknown`.
    pub sync_status: SyncStatus,
    /// The worst health of its resources.
    pub health_status: HealthStatus,
    /// Why it has that health; null when nothing was said.
    pub health_message: Option<String>,
    /// The commit the last comparison rendered; null before the first.
    pub revision: Option<String>,
    /// The commit the last successful sync applied; null before the first.
    pub synced_revision: Option<String>,
    /// The operation in flight or the last one that ran; null before the first.
    pub operation: Option<ApplicationOperation>,
    /// What it manages, live or desired. Only `GET /applications/{id}` and the refresh answer carry them; lists leave this empty.
    pub resources: Vec<ResourceStatus>,
    /// Why the last comparison could not complete, as sentences (render failed, cluster unreachable, …); empty when it did. Lists carry the first only.
    pub conditions: Vec<String>,
    /// Resources it manages.
    pub resource_count: u32,
    /// Of those, the ones that are not synced.
    pub out_of_sync_count: u32,
    /// When it was last compared with the cluster; null before the first comparison.
    pub reconciled_at: Option<DateTime<Utc>>,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

impl From<proto::Application> for Application {
    fn from(application: proto::Application) -> Self {
        Self {
            id: application.id,
            project_id: application.project_id,
            name: application.name,
            cluster_id: application.cluster_id,
            cluster_name: application.cluster_name,
            namespace: opt_string(application.namespace),
            source: application.source.unwrap_or_default().into(),
            sync_policy: application.sync_policy.unwrap_or_default().into(),
            sync_status: SyncStatus::parse(&application.sync_status),
            health_status: HealthStatus::parse(&application.health_status),
            health_message: opt_string(application.health_message),
            revision: opt_string(application.revision),
            synced_revision: opt_string(application.synced_revision),
            operation: application.operation.map(Into::into),
            resources: application.resources.into_iter().map(Into::into).collect(),
            conditions: application.conditions,
            resource_count: application.resource_count,
            out_of_sync_count: application.out_of_sync_count,
            reconciled_at: opt_timestamp(application.reconciled_at.as_ref()),
            created_at: timestamp(application.created_at.as_ref()),
            updated_at: timestamp(application.updated_at.as_ref()),
        }
    }
}

/// Body of `POST /applications` and `PUT /applications/{id}` (admin). An
/// update replaces every field.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpsertApplicationRequest {
    /// Project it belongs to.
    pub project_id: String,
    /// Name, unique within the project.
    pub name: String,
    /// Cluster it deploys to.
    pub cluster_id: String,
    /// Destination namespace; null uses the cluster's default namespace.
    pub namespace: Option<String>,
    /// Where the desired state comes from; null is the root of the project's repository at its default branch, detected.
    pub source: Option<ApplicationSourceInput>,
    /// When it syncs without an execution; null is never.
    pub sync_policy: Option<SyncPolicyInput>,
}

impl UpsertApplicationRequest {
    /// The proto message; `id` is empty to create. What the service derives
    /// (statuses, revisions, resources) stays unset.
    pub fn into_proto(self, id: String) -> proto::Application {
        proto::Application {
            id,
            project_id: self.project_id,
            name: self.name,
            cluster_id: self.cluster_id,
            namespace: self.namespace.unwrap_or_default(),
            source: Some(self.source.unwrap_or_default().into()),
            sync_policy: Some(self.sync_policy.unwrap_or_default().into()),
            ..Default::default()
        }
    }
}

/// Query of `GET /applications`.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct ListApplicationsQuery {
    /// Only this project's; omitted lists every project's.
    pub project_id: Option<String>,
}

/// Query of `DELETE /applications/{id}`.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct DeleteApplicationQuery {
    /// Also delete the resources it manages from the cluster. Default false: they stay, unmanaged.
    pub cascade: Option<bool>,
}

/// Response of `DELETE /applications/{id}`.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeletedApplication {
    /// Resources deleted from the cluster; 0 unless `cascade` was asked.
    pub deleted_resources: u32,
}

impl From<proto::DeleteApplicationResponse> for DeletedApplication {
    fn from(response: proto::DeleteApplicationResponse) -> Self {
        Self {
            deleted_resources: response.deleted_resources,
        }
    }
}

/// Body of `POST /applications/{id}/refresh` (member). It may be left out.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RefreshApplicationRequest {
    /// Render again instead of using the cached manifests of the same commit (Argo CD's hard refresh). Default false.
    pub hard: Option<bool>,
}

// ---------------------------------------------------------------------------
// Manifests and diff
// ---------------------------------------------------------------------------

/// Query of `GET /applications/{id}/manifests`.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct ManifestsQuery {
    /// Branch, tag or SHA to render; omitted renders the source's revision.
    pub revision: Option<String>,
}

/// The desired state as rendered, `Secret` values masked.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Manifests {
    /// The commit that was rendered; null when the service did not say.
    pub revision: Option<String>,
    /// One YAML document per resource, sorted by sync wave then kind order.
    pub manifests: Vec<String>,
}

impl From<proto::Manifests> for Manifests {
    fn from(manifests: proto::Manifests) -> Self {
        Self {
            revision: opt_string(manifests.revision),
            manifests: manifests.manifests,
        }
    }
}

/// How one resource differs between the cluster and the desired state.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResourceDiff {
    /// API group; `""` is the core group.
    pub group: String,
    /// Kind.
    pub kind: String,
    /// Namespace; null for cluster-scoped resources.
    pub namespace: Option<String>,
    /// Name.
    pub name: String,
    /// `synced`, `out_of_sync`, `missing` or `extraneous`.
    pub sync_status: SyncStatus,
    /// Unified diff from live to desired; null when they match.
    pub diff: Option<String>,
    /// The live object, normalised YAML with `Secret` values masked; null when it is not in the cluster.
    pub live: Option<String>,
    /// The desired object, normalised YAML with `Secret` values masked; null when it is no longer desired.
    pub target: Option<String>,
}

impl From<proto::ResourceDiff> for ResourceDiff {
    fn from(diff: proto::ResourceDiff) -> Self {
        Self {
            group: diff.group,
            kind: diff.kind,
            namespace: opt_string(diff.namespace),
            name: diff.name,
            sync_status: SyncStatus::parse(&diff.sync_status),
            diff: opt_string(diff.diff),
            live: opt_string(diff.live),
            target: opt_string(diff.target),
        }
    }
}

/// Per-resource differences between desired and live.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationDiff {
    /// The commit the desired state was rendered from; null when the service did not say.
    pub revision: Option<String>,
    /// Every managed resource, matching or not.
    pub resources: Vec<ResourceDiff>,
}

impl From<proto::ApplicationDiff> for ApplicationDiff {
    fn from(diff: proto::ApplicationDiff) -> Self {
        Self {
            revision: opt_string(diff.revision),
            resources: diff.resources.into_iter().map(Into::into).collect(),
        }
    }
}

// ---------------------------------------------------------------------------
// History and operations
// ---------------------------------------------------------------------------

/// Query of `GET /applications/{id}/history`.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct HistoryQuery {
    /// Entries, newest first; omitted answers all that are kept (the last 50).
    pub limit: Option<u32>,
}

/// One sync that was not a dry run.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    /// History entry id (what a rollback names).
    pub id: String,
    /// Application it belongs to.
    pub application_id: String,
    /// `sync`, or `rollback` when it synced an earlier entry again.
    pub kind: String,
    /// The commit that was synced.
    pub revision: String,
    /// The source as it was synced; a rollback syncs it again.
    pub source: ApplicationSource,
    /// `succeeded`, `failed` or `error`.
    pub phase: OperationPhase,
    /// What happened, as a sentence; null when there is nothing to say.
    pub message: Option<String>,
    /// The user id that asked, `system` for an automated sync.
    pub initiated_by: String,
    /// The execution that ran it; null for an automated sync.
    pub execution_id: Option<String>,
    /// True when the sync policy did it, not a person.
    pub automated: bool,
    /// When it started.
    pub started_at: DateTime<Utc>,
    /// When it ended.
    pub finished_at: DateTime<Utc>,
}

impl From<proto::HistoryEntry> for HistoryEntry {
    fn from(entry: proto::HistoryEntry) -> Self {
        Self {
            id: entry.id,
            application_id: entry.application_id,
            kind: if entry.kind.is_empty() {
                "sync".to_owned()
            } else {
                entry.kind
            },
            revision: entry.revision,
            source: entry.source.unwrap_or_default().into(),
            phase: OperationPhase::parse(&entry.phase),
            message: opt_string(entry.message),
            initiated_by: entry.initiated_by,
            execution_id: opt_string(entry.execution_id),
            automated: entry.automated,
            started_at: timestamp(entry.started_at.as_ref()),
            finished_at: timestamp(entry.finished_at.as_ref()),
        }
    }
}

/// Body of `POST /applications/{id}/sync` (approver). It may be left out.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SyncApplicationRequest {
    /// Also delete resources that left the desired state. Default false.
    pub prune: Option<bool>,
    /// Only preview what would change: the execution has the preview step alone and applies nothing. Default false.
    pub dry_run: Option<bool>,
    /// Branch, tag or SHA to sync; omitted syncs the source's revision.
    pub revision: Option<String>,
}

/// Body of `POST /applications/{id}/rollback` (approver).
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RollbackApplicationRequest {
    /// The history entry whose source is synced again (`GET /applications/{id}/history`).
    pub history_id: String,
    /// Also delete resources the earlier state did not have. Default false.
    pub prune: Option<bool>,
}

/// Response of `POST /applications/{id}/sync` and `/rollback`.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OperationStarted {
    /// The started execution that previews, applies and verifies the change; its approval (the `gitops.sync` or `gitops.rollback` policy and a timed `cluster:write` grant) and its outcome are there.
    pub execution_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vocabularies_parse_the_service_words_and_fall_back() {
        assert_eq!(SyncStatus::parse("out_of_sync"), SyncStatus::OutOfSync);
        assert_eq!(SyncStatus::parse("sideways"), SyncStatus::Unknown);
        assert_eq!(HealthStatus::parse(""), HealthStatus::Unknown);
        assert_eq!(ClusterStatus::parse("connected"), ClusterStatus::Connected);
        assert_eq!(OperationPhase::parse("terminating").as_str(), "terminating");
        assert_eq!(
            serde_json::to_value(SyncStatus::OutOfSync).unwrap(),
            "out_of_sync"
        );
        assert_eq!(source_type(""), None);
        assert_eq!(source_type("helm"), Some(SourceType::Helm));
        assert!(serde_json::from_str::<SourceType>("\"jsonnet\"").is_err());
    }

    #[test]
    fn empty_strings_are_null_and_a_resource_without_health_has_none() {
        let resource = ResourceStatus::from(proto::ResourceStatus {
            kind: "ClusterRole".into(),
            name: "reader".into(),
            sync_status: "missing".into(),
            ..Default::default()
        });
        let json = serde_json::to_value(&resource).unwrap();
        assert_eq!(json["namespace"], serde_json::Value::Null);
        assert_eq!(json["healthStatus"], serde_json::Value::Null);
        assert_eq!(json["group"], "");
        assert_eq!(json["syncStatus"], "missing");
    }

    #[test]
    fn an_omitted_source_and_policy_travel_as_defaults() {
        let body: UpsertApplicationRequest = serde_json::from_value(serde_json::json!({
            "projectId": "p1", "name": "web", "clusterId": "c1"
        }))
        .unwrap();
        let application = body.into_proto(String::new());
        let source = application.source.unwrap();
        assert_eq!(source.r#type, "");
        assert!(!source.recurse);
        assert!(!application.sync_policy.unwrap().automated);
    }
}

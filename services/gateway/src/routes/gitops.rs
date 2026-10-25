//! The GitOps engine (CONTRACT.md §24.8): clusters, applications, their
//! rendered manifests, diff and history, and the two requests that change
//! a cluster.
//!
//! Sync and rollback never reach the cluster from here: they create an
//! execution with the `gitops` adapter (preview → sync or rollback → verify
//! health) and start it, so permission (`cluster:write`, always a timed
//! grant), policy and approval are the execution's (§7, §24.7).
//!
//! Roles (also checked by the auth middleware): reads VIEWER; refresh
//! MEMBER; clusters and applications ADMIN to change; sync and rollback
//! APPROVER. The gitops service enforces them again from the forwarded
//! principal.

use axum::{Json, http::StatusCode};
use sdi_proto::{common::v1 as common, execution::v1 as execution, gitops::v1 as proto};
use utoipa_axum::{router::OpenApiRouter, routes};

use super::{map_all, require_id};
use crate::{
    dto::gitops::{
        Application, ApplicationDiff, Cluster, DeleteApplicationQuery, DeletedApplication,
        HistoryEntry, HistoryQuery, ListApplicationsQuery, Manifests, ManifestsQuery,
        OperationStarted, RefreshApplicationRequest, RollbackApplicationRequest,
        SyncApplicationRequest, UpsertApplicationRequest, UpsertClusterRequest,
    },
    error::{ApiError, ApiJson, ApiPath, ApiQuery, ApiResult, ErrorResponses},
    state::{AppState, Scoped},
};

/// The adapter of every execution these routes create, and the tool family
/// of each of its steps (CONTRACT.md §24.7).
pub const ADAPTER: &str = "gitops";
/// History entries the service keeps per application (§24.5): a rollback
/// target is always among them.
const HISTORY_KEPT: u32 = 50;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_clusters, create_cluster))
        .routes(routes!(update_cluster, delete_cluster))
        .routes(routes!(list_applications, create_application))
        .routes(routes!(
            get_application,
            update_application,
            delete_application
        ))
        .routes(routes!(refresh_application))
        .routes(routes!(get_manifests))
        .routes(routes!(diff_application))
        .routes(routes!(list_history))
        .routes(routes!(sync_application))
        .routes(routes!(rollback_application))
}

// --- clusters ---------------------------------------------------------------

/// The workspace's clusters with the status of their last check.
#[utoipa::path(
    get,
    path = "/clusters",
    tag = "gitops",
    responses((status = 200, body = Vec<Cluster>), ErrorResponses),
)]
async fn list_clusters(state: Scoped) -> ApiResult<Json<Vec<Cluster>>> {
    let response = state
        .gitops()
        .list_clusters(proto::ListClustersRequest {})
        .await?
        .into_inner();
    Ok(Json(map_all(response.clusters)))
}

/// Adds a cluster (admin). The kubeconfig is a workspace secret named by
/// `kubeconfigSecret`. The cluster is reached once: a bad kubeconfig or an
/// unreachable API server is saved anyway and said in `status` / `message`.
/// A loopback or private API server address is refused unless the
/// deployment allows private targets.
#[utoipa::path(
    post,
    path = "/clusters",
    tag = "gitops",
    request_body = UpsertClusterRequest,
    responses((status = 201, body = Cluster), ErrorResponses),
)]
async fn create_cluster(
    state: Scoped,
    ApiJson(body): ApiJson<UpsertClusterRequest>,
) -> ApiResult<(StatusCode, Json<Cluster>)> {
    let cluster = state
        .gitops()
        .upsert_cluster(proto::UpsertClusterRequest {
            cluster: Some(body.into_proto(String::new())),
        })
        .await?
        .into_inner();
    Ok((StatusCode::CREATED, Json(cluster.into())))
}

/// Replaces a cluster's settings (admin) and reaches it again.
#[utoipa::path(
    put,
    path = "/clusters/{id}",
    tag = "gitops",
    params(("id" = String, Path, description = "Cluster id")),
    request_body = UpsertClusterRequest,
    responses((status = 200, body = Cluster), ErrorResponses),
)]
async fn update_cluster(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
    ApiJson(body): ApiJson<UpsertClusterRequest>,
) -> ApiResult<Json<Cluster>> {
    let cluster = state
        .gitops()
        .upsert_cluster(proto::UpsertClusterRequest {
            cluster: Some(body.into_proto(require_id(id)?)),
        })
        .await?
        .into_inner();
    Ok(Json(cluster.into()))
}

/// Deletes a cluster (admin). 412 while an application deploys to it.
#[utoipa::path(
    delete,
    path = "/clusters/{id}",
    tag = "gitops",
    params(("id" = String, Path, description = "Cluster id")),
    responses((status = 200, description = "Deleted"), ErrorResponses),
)]
async fn delete_cluster(state: Scoped, ApiPath(id): ApiPath<String>) -> ApiResult<StatusCode> {
    state
        .gitops()
        .delete_cluster(proto::DeleteClusterRequest { id })
        .await?;
    Ok(StatusCode::OK)
}

// --- applications -----------------------------------------------------------

/// Applications, optionally of one project: summaries with their sync and
/// health status, revision and first condition, without resources.
#[utoipa::path(
    get,
    path = "/applications",
    tag = "gitops",
    params(ListApplicationsQuery),
    responses((status = 200, body = Vec<Application>), ErrorResponses),
)]
async fn list_applications(
    state: Scoped,
    ApiQuery(query): ApiQuery<ListApplicationsQuery>,
) -> ApiResult<Json<Vec<Application>>> {
    let response = state
        .gitops()
        .list_applications(proto::ListApplicationsRequest {
            project_id: query.project_id.unwrap_or_default(),
        })
        .await?
        .into_inner();
    Ok(Json(map_all(response.applications)))
}

/// Declares an application (admin): a path in a repository, a cluster and
/// a namespace. Turning `syncPolicy.automated` on lets the service sync it
/// without an execution.
#[utoipa::path(
    post,
    path = "/applications",
    tag = "gitops",
    request_body = UpsertApplicationRequest,
    responses((status = 201, body = Application), ErrorResponses),
)]
async fn create_application(
    state: Scoped,
    ApiJson(body): ApiJson<UpsertApplicationRequest>,
) -> ApiResult<(StatusCode, Json<Application>)> {
    let application = state
        .gitops()
        .upsert_application(proto::UpsertApplicationRequest {
            application: Some(body.into_proto(String::new())),
        })
        .await?
        .into_inner();
    Ok((StatusCode::CREATED, Json(application.into())))
}

/// One application with the resources it manages, each with its sync and
/// health status, and its conditions.
#[utoipa::path(
    get,
    path = "/applications/{id}",
    tag = "gitops",
    params(("id" = String, Path, description = "Application id")),
    responses((status = 200, body = Application), ErrorResponses),
)]
async fn get_application(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
) -> ApiResult<Json<Application>> {
    Ok(Json(read_application(&state, id).await?.into()))
}

/// Replaces an application's settings (admin).
#[utoipa::path(
    put,
    path = "/applications/{id}",
    tag = "gitops",
    params(("id" = String, Path, description = "Application id")),
    request_body = UpsertApplicationRequest,
    responses((status = 200, body = Application), ErrorResponses),
)]
async fn update_application(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
    ApiJson(body): ApiJson<UpsertApplicationRequest>,
) -> ApiResult<Json<Application>> {
    let application = state
        .gitops()
        .upsert_application(proto::UpsertApplicationRequest {
            application: Some(body.into_proto(require_id(id)?)),
        })
        .await?
        .into_inner();
    Ok(Json(application.into()))
}

/// Deletes an application (admin). With `cascade=true` the resources it
/// manages are deleted from the cluster too; without, they stay unmanaged.
#[utoipa::path(
    delete,
    path = "/applications/{id}",
    tag = "gitops",
    params(("id" = String, Path, description = "Application id"), DeleteApplicationQuery),
    responses((status = 200, body = DeletedApplication), ErrorResponses),
)]
async fn delete_application(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
    ApiQuery(query): ApiQuery<DeleteApplicationQuery>,
) -> ApiResult<Json<DeletedApplication>> {
    let response = state
        .gitops()
        .delete_application(proto::DeleteApplicationRequest {
            id,
            cascade: query.cascade.unwrap_or(false),
        })
        .await?
        .into_inner();
    Ok(Json(response.into()))
}

/// Renders and compares now instead of waiting for the next reconcile
/// (member). `hard` renders again even when the commit did not change. The
/// body may be left out.
#[utoipa::path(
    post,
    path = "/applications/{id}/refresh",
    tag = "gitops",
    params(("id" = String, Path, description = "Application id")),
    request_body = RefreshApplicationRequest,
    responses((status = 200, body = Application), ErrorResponses),
)]
async fn refresh_application(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
    body: Option<ApiJson<RefreshApplicationRequest>>,
) -> ApiResult<Json<Application>> {
    let body = body.map(|ApiJson(body)| body).unwrap_or_default();
    let application = state
        .gitops()
        .refresh_application(proto::RefreshApplicationRequest {
            id,
            hard: body.hard.unwrap_or(false),
        })
        .await?
        .into_inner();
    Ok(Json(application.into()))
}

/// The desired state as rendered, one YAML document per resource, with
/// every `Secret` value masked.
#[utoipa::path(
    get,
    path = "/applications/{id}/manifests",
    tag = "gitops",
    params(("id" = String, Path, description = "Application id"), ManifestsQuery),
    responses((status = 200, body = Manifests), ErrorResponses),
)]
async fn get_manifests(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
    ApiQuery(query): ApiQuery<ManifestsQuery>,
) -> ApiResult<Json<Manifests>> {
    let manifests = state
        .gitops()
        .get_manifests(proto::GetManifestsRequest {
            id,
            revision: query.revision.unwrap_or_default(),
        })
        .await?
        .into_inner();
    Ok(Json(manifests.into()))
}

/// Per resource, how the cluster differs from the desired state: a unified
/// diff and both sides as normalised YAML, `Secret` values masked.
#[utoipa::path(
    get,
    path = "/applications/{id}/diff",
    tag = "gitops",
    params(("id" = String, Path, description = "Application id")),
    responses((status = 200, body = ApplicationDiff), ErrorResponses),
)]
async fn diff_application(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
) -> ApiResult<Json<ApplicationDiff>> {
    let diff = state
        .gitops()
        .diff_application(proto::DiffApplicationRequest { id })
        .await?
        .into_inner();
    Ok(Json(diff.into()))
}

/// The application's syncs, newest first (dry runs are not kept).
#[utoipa::path(
    get,
    path = "/applications/{id}/history",
    tag = "gitops",
    params(("id" = String, Path, description = "Application id"), HistoryQuery),
    responses((status = 200, body = Vec<HistoryEntry>), ErrorResponses),
)]
async fn list_history(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
    ApiQuery(query): ApiQuery<HistoryQuery>,
) -> ApiResult<Json<Vec<HistoryEntry>>> {
    let response = state
        .gitops()
        .list_history(proto::ListHistoryRequest {
            id,
            limit: query.limit.unwrap_or(0),
        })
        .await?
        .into_inner();
    Ok(Json(map_all(response.entries)))
}

// --- operations -------------------------------------------------------------

/// Asks for the application to be synced (approver): creates and starts an
/// execution that previews the change, syncs (waiting for its approval and
/// a timed `cluster:write` grant) and waits for the application to be
/// healthy. With `dryRun` the execution only previews. The body may be left
/// out. 404 when the application does not exist.
#[utoipa::path(
    post,
    path = "/applications/{id}/sync",
    tag = "gitops",
    params(("id" = String, Path, description = "Application id")),
    request_body = SyncApplicationRequest,
    responses((status = 201, body = OperationStarted), ErrorResponses),
)]
async fn sync_application(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
    body: Option<ApiJson<SyncApplicationRequest>>,
) -> ApiResult<(StatusCode, Json<OperationStarted>)> {
    let body = body.map(|ApiJson(body)| body).unwrap_or_default();
    let application = read_application(&state, id).await?;
    if !body.dry_run.unwrap_or(false) {
        refuse_while_running(&application)?;
    }
    let revision = body.revision.unwrap_or_default();
    let (title, steps) = plan::sync(
        &application,
        body.prune.unwrap_or(false),
        body.dry_run.unwrap_or(false),
        revision.trim(),
    );
    let execution_id = create_and_start(&state, application.project_id, title, steps).await?;
    Ok((StatusCode::CREATED, Json(OperationStarted { execution_id })))
}

/// Asks for the application to be rolled back to a history entry
/// (approver): creates and starts an execution that previews, syncs that
/// entry's source again (waiting for its approval and a timed
/// `cluster:write` grant) and waits for the application to be healthy. 404
/// when the application or the entry does not exist.
#[utoipa::path(
    post,
    path = "/applications/{id}/rollback",
    tag = "gitops",
    params(("id" = String, Path, description = "Application id")),
    request_body = RollbackApplicationRequest,
    responses((status = 201, body = OperationStarted), ErrorResponses),
)]
async fn rollback_application(
    state: Scoped,
    ApiPath(id): ApiPath<String>,
    ApiJson(body): ApiJson<RollbackApplicationRequest>,
) -> ApiResult<(StatusCode, Json<OperationStarted>)> {
    let history_id = body.history_id.trim();
    if history_id.is_empty() {
        return Err(ApiError::bad_request("historyId is required"));
    }
    let application = read_application(&state, id).await?;
    refuse_while_running(&application)?;
    // The service refuses it too (§24.5), but only at the rollback step,
    // after an approval and a grant: refuse before the execution exists.
    if application
        .sync_policy
        .as_ref()
        .is_some_and(|policy| policy.automated)
    {
        return Err(ApiError::precondition(format!(
            "{} syncs automatically: turn off automated sync before rolling back, \
             or the next reconcile syncs the current source again",
            application.name
        )));
    }
    let entry = state
        .gitops()
        .list_history(proto::ListHistoryRequest {
            id: application.id.clone(),
            limit: HISTORY_KEPT,
        })
        .await?
        .into_inner()
        .entries
        .into_iter()
        .find(|entry| entry.id == history_id)
        .ok_or_else(|| {
            ApiError::not_found(format!(
                "{} has no history entry {history_id}",
                application.name
            ))
        })?;
    let (title, steps) = plan::rollback(&application, &entry, body.prune.unwrap_or(false));
    let execution_id = create_and_start(&state, application.project_id, title, steps).await?;
    Ok((StatusCode::CREATED, Json(OperationStarted { execution_id })))
}

/// One operation per application at a time (CONTRACT.md §24.5): a second
/// request while one runs would create an execution that can only fail at
/// its sync step, so it is refused here, with the way to the running one.
fn refuse_while_running(application: &proto::Application) -> ApiResult<()> {
    match &application.operation {
        Some(operation) if matches!(operation.phase.as_str(), "running" | "terminating") => {
            let execution = if operation.execution_id.is_empty() {
                String::new()
            } else {
                format!(" (execution {})", operation.execution_id)
            };
            Err(ApiError::precondition(format!(
                "a {} of {} is running{execution}; wait for it to end before asking again",
                if operation.kind.is_empty() {
                    "sync"
                } else {
                    operation.kind.as_str()
                },
                application.name
            )))
        }
        _ => Ok(()),
    }
}

async fn read_application(state: &Scoped, id: String) -> ApiResult<proto::Application> {
    Ok(state
        .gitops()
        .get_application(proto::GetApplicationRequest { id })
        .await?
        .into_inner())
}

/// Creates the execution in the application's project and starts it (the
/// policy check: it waits for approval or runs). A refused start answers
/// that refusal; the execution it leaves in `planning` is listed with the
/// project's executions.
async fn create_and_start(
    state: &Scoped,
    project_id: String,
    title: String,
    steps: Vec<common::PlanStep>,
) -> ApiResult<String> {
    let mut client = state.execution();
    let created = client
        .create_execution(execution::CreateExecutionRequest {
            project_id,
            proposal_id: String::new(),
            pattern_id: String::new(),
            title,
            steps,
            adapter: ADAPTER.to_owned(),
            artifacts: Default::default(),
        })
        .await?
        .into_inner();
    client
        .start_execution(execution::StartExecutionRequest {
            id: created.id.clone(),
            fault: None,
        })
        .await?;
    Ok(created.id)
}

/// The plans of CONTRACT.md §24.7, built from the application as it is now.
pub mod plan {
    use std::collections::HashMap;

    use sdi_proto::{common::v1::PlanStep, gitops::v1 as proto};

    use super::ADAPTER;

    pub const PREVIEW: &str = "gitops.preview";
    pub const SYNC: &str = "gitops.sync";
    pub const ROLLBACK: &str = "gitops.rollback";
    pub const VERIFY_HEALTH: &str = "gitops.verify_health";
    /// Held by nobody: every sync or rollback waits for a timed grant (§24.7).
    pub const CLUSTER_WRITE: &str = "cluster:write";

    pub const STEP_PREVIEW: &str = "preview";
    pub const STEP_SYNC: &str = "sync";
    pub const STEP_ROLLBACK: &str = "rollback";
    pub const STEP_VERIFY_HEALTH: &str = "verify_health";

    /// `<cluster>/<namespace>`, or the cluster alone when the application
    /// uses the cluster's default namespace.
    fn destination(application: &proto::Application) -> String {
        let cluster = if application.cluster_name.is_empty() {
            &application.cluster_id
        } else {
            &application.cluster_name
        };
        if application.namespace.is_empty() {
            cluster.clone()
        } else {
            format!("{cluster}/{}", application.namespace)
        }
    }

    /// A full commit SHA is shown as its first seven characters.
    fn short(revision: &str) -> &str {
        if revision.len() == 40 && revision.bytes().all(|b| b.is_ascii_hexdigit()) {
            &revision[..7]
        } else {
            revision
        }
    }

    /// `application` and `prune` always; the others only when they say something.
    fn params(
        application: &proto::Application,
        prune: bool,
        extra: &[(&str, &str)],
    ) -> HashMap<String, String> {
        let mut params = HashMap::from([
            ("application".to_owned(), application.id.clone()),
            ("prune".to_owned(), prune.to_string()),
        ]);
        for (key, value) in extra {
            if !value.is_empty() {
                params.insert((*key).to_owned(), (*value).to_owned());
            }
        }
        params
    }

    /// A step of the `gitops` tool family with no dependency, permission
    /// or parameter yet; callers fill those in.
    fn step(key: &str, action_type: &str, title: String, description: String) -> PlanStep {
        PlanStep {
            key: key.to_owned(),
            title,
            description,
            action_type: action_type.to_owned(),
            adapter: ADAPTER.to_owned(),
            ..Default::default()
        }
    }

    fn preview(
        application: &proto::Application,
        what: &str,
        params: HashMap<String, String>,
    ) -> PlanStep {
        PlanStep {
            params,
            verification: "the dry run lists what would change; nothing is applied".to_owned(),
            ..step(
                STEP_PREVIEW,
                PREVIEW,
                format!("Preview the {what}"),
                format!(
                    "Dry-run the {what} of {} to {} and list what would be created, changed or pruned.",
                    application.name,
                    destination(application)
                ),
            )
        }
    }

    /// The step that changes the cluster: after the preview, behind a
    /// timed `cluster:write` grant.
    fn apply(step: PlanStep, params: HashMap<String, String>) -> PlanStep {
        PlanStep {
            depends_on: vec![STEP_PREVIEW.to_owned()],
            required_permissions: vec![CLUSTER_WRITE.to_owned()],
            params,
            verification: "every resource applied and the operation succeeded".to_owned(),
            ..step
        }
    }

    fn verify_health(application: &proto::Application, after: &str) -> PlanStep {
        PlanStep {
            depends_on: vec![after.to_owned()],
            params: HashMap::from([("application".to_owned(), application.id.clone())]),
            verification: "the application reports healthy".to_owned(),
            ..step(
                STEP_VERIFY_HEALTH,
                VERIFY_HEALTH,
                "Verify health".to_owned(),
                format!(
                    "Poll {} until it is healthy; degraded or missing fails the step.",
                    application.name
                ),
            )
        }
    }

    /// `gitops.preview → gitops.sync → gitops.verify_health`, or the
    /// preview alone for a dry run. `revision` empty syncs the source's.
    pub fn sync(
        application: &proto::Application,
        prune: bool,
        dry_run: bool,
        revision: &str,
    ) -> (String, Vec<PlanStep>) {
        let destination = destination(application);
        let params = params(application, prune, &[("revision", revision)]);
        let preview = preview(application, "sync", params.clone());
        if dry_run {
            let title = format!("Preview a sync of {} to {destination}", application.name);
            return (title, vec![preview]);
        }
        let at = if revision.is_empty() {
            "its source revision".to_owned()
        } else {
            short(revision).to_owned()
        };
        let sync = apply(
            step(
                STEP_SYNC,
                SYNC,
                format!("Sync {}", application.name),
                format!(
                    "Apply the desired state of {} at {at} to {destination}{}.",
                    application.name,
                    if prune { ", deleting what left it" } else { "" }
                ),
            ),
            params,
        );
        (
            format!("Sync {} to {destination}", application.name),
            vec![preview, sync, verify_health(application, STEP_SYNC)],
        )
    }

    /// `gitops.preview → gitops.rollback → gitops.verify_health` to the
    /// source of `entry`.
    pub fn rollback(
        application: &proto::Application,
        entry: &proto::HistoryEntry,
        prune: bool,
    ) -> (String, Vec<PlanStep>) {
        let destination = destination(application);
        let params = params(
            application,
            prune,
            &[
                ("history_id", entry.id.as_str()),
                ("revision", entry.revision.as_str()),
            ],
        );
        let revision = short(&entry.revision);
        let rollback = apply(
            step(
                STEP_ROLLBACK,
                ROLLBACK,
                format!("Roll back {}", application.name),
                format!(
                    "Sync the source of {} as it was at {revision} (history entry {}) to {destination} again{}.",
                    application.name,
                    entry.id,
                    if prune {
                        ", deleting what it did not have"
                    } else {
                        ""
                    }
                ),
            ),
            params.clone(),
        );
        (
            format!(
                "Roll back {} on {destination} to {revision}",
                application.name
            ),
            vec![
                preview(application, "rollback", params),
                rollback,
                verify_health(application, STEP_ROLLBACK),
            ],
        )
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn application() -> proto::Application {
            proto::Application {
                id: "a1".into(),
                project_id: "p1".into(),
                name: "web".into(),
                cluster_id: "c1".into(),
                cluster_name: "prod".into(),
                namespace: "shop".into(),
                ..Default::default()
            }
        }

        #[test]
        fn a_sync_previews_syncs_with_a_grant_and_verifies() {
            let (title, steps) = sync(&application(), true, false, "");
            assert_eq!(title, "Sync web to prod/shop");
            let keys: Vec<_> = steps.iter().map(|s| s.key.as_str()).collect();
            assert_eq!(keys, [STEP_PREVIEW, STEP_SYNC, STEP_VERIFY_HEALTH]);
            assert!(steps.iter().all(|s| s.adapter == ADAPTER));
            assert_eq!(steps[1].required_permissions, [CLUSTER_WRITE]);
            assert!(steps[0].required_permissions.is_empty());
            assert_eq!(steps[1].depends_on, [STEP_PREVIEW]);
            assert_eq!(steps[2].depends_on, [STEP_SYNC]);
            assert_eq!(steps[1].params["prune"], "true");
            assert!(!steps[1].params.contains_key("revision"));
            assert_eq!(steps[2].params.len(), 1);
        }

        #[test]
        fn a_dry_run_only_previews_and_the_default_namespace_is_left_out() {
            let mut app = application();
            app.namespace.clear();
            app.cluster_name.clear();
            let (title, steps) = sync(&app, false, true, "v1.2");
            assert_eq!(title, "Preview a sync of web to c1");
            assert_eq!(steps.len(), 1);
            assert_eq!(steps[0].action_type, PREVIEW);
            assert_eq!(steps[0].params["revision"], "v1.2");
        }

        #[test]
        fn a_rollback_names_the_entry_and_its_short_revision() {
            let entry = proto::HistoryEntry {
                id: "h1".into(),
                revision: "0123456789abcdef0123456789abcdef01234567".into(),
                ..Default::default()
            };
            let (title, steps) = rollback(&application(), &entry, false);
            assert_eq!(title, "Roll back web on prod/shop to 0123456");
            assert_eq!(steps[1].key, STEP_ROLLBACK);
            assert_eq!(steps[1].action_type, ROLLBACK);
            assert_eq!(steps[1].params["history_id"], "h1");
            assert_eq!(steps[0].params["history_id"], "h1");
            assert_eq!(steps[2].depends_on, [STEP_ROLLBACK]);
        }
    }
}

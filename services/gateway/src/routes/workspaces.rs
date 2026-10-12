//! Workspaces: membership, audit, data controls, secrets, export, purge and
//! deletion (CONTRACT.md §18, §19, §22). The workspace in the path is the
//! one the request was authorised for (the auth middleware resolves
//! `/workspaces/{id}`), so `{id}` and the principal's workspace always agree.

use std::collections::{BTreeMap, HashMap};

use axum::Json;
use sdi_proto::{
    execution::v1 as execution, identity::v1 as identity, intelligence::v1 as intelligence,
    knowledge::v1 as knowledge,
};
use utoipa_axum::{router::OpenApiRouter, routes};

use super::map_all;
use crate::{
    dto::{
        delivery::{SecretInfo, SetSecretRequest},
        identity::{
            AuditEntry, AuditQuery, DataControls, DataExport, Member, MembersResponse, PurgeResult,
            SetReasonerKeyRequest, UpdateDataControlsRequest, UpdateMemberRoleRequest, Workspace,
            WorkspaceDeleted,
        },
    },
    error::{ApiError, ApiJson, ApiPath, ApiQuery, ApiResult, ErrorResponses},
    state::{AppState, Auth, Scoped},
};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_workspaces))
        .routes(routes!(get_workspace, delete_workspace))
        .routes(routes!(list_members))
        .routes(routes!(update_member_role))
        .routes(routes!(sync_members))
        .routes(routes!(list_audit))
        .routes(routes!(get_data_controls, update_data_controls))
        .routes(routes!(set_reasoner_key))
        .routes(routes!(list_secrets))
        .routes(routes!(set_secret, delete_secret))
        .routes(routes!(export_data))
        .routes(routes!(purge_code_cache))
}

/// Every workspace of a user, with the project count the knowledge service
/// reports for each (0 when it cannot be asked). One `CountProjects` call
/// covers every workspace, so the switcher costs one round trip however
/// many organisations the user belongs to.
pub(crate) async fn list_for(scoped: &Scoped, user_id: &str) -> ApiResult<Vec<Workspace>> {
    let response = scoped
        .identity()
        .list_workspaces(identity::ListWorkspacesRequest {
            user_id: user_id.to_owned(),
        })
        .await?
        .into_inner();
    let ids: Vec<String> = response.workspaces.iter().map(|ws| ws.id.clone()).collect();
    let counts = project_counts(scoped, ids).await;
    Ok(response
        .workspaces
        .into_iter()
        .map(|ws| Workspace {
            project_count: counts.get(&ws.id).copied().unwrap_or(0),
            ..ws.into()
        })
        .collect())
}

/// Connected projects per workspace, as `system` in the caller's own
/// workspace (CONTRACT.md §18: internal jobs run as `system`). Knowledge
/// caps one call at 100 ids; a user in more organisations than that gets
/// the counts in batches. Unreachable knowledge means zeros, not an error.
async fn project_counts(scoped: &Scoped, ids: Vec<String>) -> HashMap<String, u32> {
    let mut counts = HashMap::new();
    let system = scoped.state.system(scoped.workspace_id());
    for batch in ids.chunks(100) {
        match system
            .knowledge()
            .count_projects(knowledge::CountProjectsRequest {
                workspace_ids: batch.to_vec(),
            })
            .await
        {
            Ok(response) => counts.extend(response.into_inner().counts),
            Err(error) => {
                tracing::warn!(%error, "project counts unavailable");
            }
        }
    }
    counts
}

fn ensure_current(auth: &Auth, id: &str) -> ApiResult<()> {
    if auth.principal.workspace_id == id {
        Ok(())
    } else {
        Err(ApiError::forbidden(
            "this workspace is not the one the session was resolved for",
        ))
    }
}

/// Every workspace the signed-in user can open, personal first.
#[utoipa::path(
    get,
    path = "/workspaces",
    tag = "workspaces",
    responses((status = 200, body = Vec<Workspace>), ErrorResponses),
)]
async fn list_workspaces(scoped: Scoped, auth: Auth) -> ApiResult<Json<Vec<Workspace>>> {
    Ok(Json(list_for(&scoped, &auth.user.id).await?))
}

/// One workspace the user can open (403 otherwise).
#[utoipa::path(
    get,
    path = "/workspaces/{id}",
    tag = "workspaces",
    params(("id" = String, Path, description = "Workspace id")),
    responses((status = 200, body = Workspace), ErrorResponses),
)]
async fn get_workspace(
    scoped: Scoped,
    auth: Auth,
    ApiPath(id): ApiPath<String>,
) -> ApiResult<Json<Workspace>> {
    ensure_current(&auth, &id)?;
    let project_count = project_counts(&scoped, vec![id.clone()])
        .await
        .get(&id)
        .copied()
        .unwrap_or(0);
    Ok(Json(Workspace {
        project_count,
        ..auth.workspace
    }))
}

/// Members synced from GitHub with their derived and effective roles.
#[utoipa::path(
    get,
    path = "/workspaces/{id}/members",
    tag = "workspaces",
    params(("id" = String, Path, description = "Workspace id")),
    responses((status = 200, body = MembersResponse), ErrorResponses),
)]
async fn list_members(
    scoped: Scoped,
    auth: Auth,
    ApiPath(id): ApiPath<String>,
) -> ApiResult<Json<MembersResponse>> {
    ensure_current(&auth, &id)?;
    let response = scoped
        .identity()
        .list_members(identity::ListMembersRequest { workspace_id: id })
        .await?
        .into_inner();
    Ok(Json(MembersResponse {
        members: map_all(response.members),
        synced_at: crate::dto::opt_timestamp(response.synced_at.as_ref()),
    }))
}

/// Narrows a member's role below what GitHub grants (admin). Null clears the override.
#[utoipa::path(
    patch,
    path = "/workspaces/{id}/members/{userId}",
    tag = "workspaces",
    params(
        ("id" = String, Path, description = "Workspace id"),
        ("userId" = String, Path, description = "User id"),
    ),
    request_body = UpdateMemberRoleRequest,
    responses((status = 200, body = Member), ErrorResponses),
)]
async fn update_member_role(
    scoped: Scoped,
    auth: Auth,
    ApiPath((id, user_id)): ApiPath<(String, String)>,
    ApiJson(body): ApiJson<UpdateMemberRoleRequest>,
) -> ApiResult<Json<Member>> {
    ensure_current(&auth, &id)?;
    let member = scoped
        .identity()
        .set_member_role(identity::SetMemberRoleRequest {
            workspace_id: id.clone(),
            user_id,
            override_role: body.override_role.map(Into::into).unwrap_or(0),
        })
        .await?
        .into_inner();
    scoped.state.forget_auth(None, Some(&id));
    Ok(Json(member.into()))
}

/// Re-reads organisation members and teams from GitHub (admin).
#[utoipa::path(
    post,
    path = "/workspaces/{id}/members/sync",
    tag = "workspaces",
    params(("id" = String, Path, description = "Workspace id")),
    responses((status = 200, body = MembersResponse), ErrorResponses),
)]
async fn sync_members(
    scoped: Scoped,
    auth: Auth,
    ApiPath(id): ApiPath<String>,
) -> ApiResult<Json<MembersResponse>> {
    ensure_current(&auth, &id)?;
    let response = scoped
        .identity()
        .sync_members(identity::SyncMembersRequest {
            workspace_id: id.clone(),
        })
        .await?
        .into_inner();
    scoped.state.forget_auth(None, Some(&id));
    Ok(Json(MembersResponse {
        members: map_all(response.members),
        synced_at: crate::dto::opt_timestamp(response.synced_at.as_ref()),
    }))
}

/// Audit entries, newest first (admin). Page with `before`.
#[utoipa::path(
    get,
    path = "/workspaces/{id}/audit",
    tag = "workspaces",
    params(("id" = String, Path, description = "Workspace id"), AuditQuery),
    responses((status = 200, body = Vec<AuditEntry>), ErrorResponses),
)]
async fn list_audit(
    scoped: Scoped,
    auth: Auth,
    ApiPath(id): ApiPath<String>,
    ApiQuery(query): ApiQuery<AuditQuery>,
) -> ApiResult<Json<Vec<AuditEntry>>> {
    ensure_current(&auth, &id)?;
    let response = scoped
        .identity()
        .list_audit(identity::ListAuditRequest {
            workspace_id: id,
            action_prefix: query.action_prefix.unwrap_or_default(),
            limit: query.limit.unwrap_or(0),
            before: query.before.map(sdi_common::time::to_proto),
        })
        .await?
        .into_inner();
    Ok(Json(map_all(response.entries)))
}

/// What the workspace lets SDI store (CONTRACT.md §19).
#[utoipa::path(
    get,
    path = "/workspaces/{id}/data-controls",
    tag = "workspaces",
    params(("id" = String, Path, description = "Workspace id")),
    responses((status = 200, body = DataControls), ErrorResponses),
)]
async fn get_data_controls(
    scoped: Scoped,
    auth: Auth,
    ApiPath(id): ApiPath<String>,
) -> ApiResult<Json<DataControls>> {
    ensure_current(&auth, &id)?;
    let controls = scoped
        .identity()
        .get_data_controls(identity::GetDataControlsRequest { workspace_id: id })
        .await?
        .into_inner();
    Ok(Json(controls.into()))
}

/// Changes the data controls (admin). Omitted fields keep their value.
#[utoipa::path(
    patch,
    path = "/workspaces/{id}/data-controls",
    tag = "workspaces",
    params(("id" = String, Path, description = "Workspace id")),
    request_body = UpdateDataControlsRequest,
    responses((status = 200, body = DataControls), ErrorResponses),
)]
async fn update_data_controls(
    scoped: Scoped,
    auth: Auth,
    ApiPath(id): ApiPath<String>,
    ApiJson(body): ApiJson<UpdateDataControlsRequest>,
) -> ApiResult<Json<DataControls>> {
    ensure_current(&auth, &id)?;
    let mut client = scoped.identity();
    let current = client
        .get_data_controls(identity::GetDataControlsRequest { workspace_id: id })
        .await?
        .into_inner();
    let updated = client
        .update_data_controls(identity::UpdateDataControlsRequest {
            controls: Some(body.apply(current)),
        })
        .await?
        .into_inner();
    Ok(Json(updated.into()))
}

/// Stores the workspace's own model API key, encrypted at rest (admin).
/// An empty key removes it. The key is never returned.
#[utoipa::path(
    put,
    path = "/workspaces/{id}/data-controls/reasoner-key",
    tag = "workspaces",
    params(("id" = String, Path, description = "Workspace id")),
    request_body = SetReasonerKeyRequest,
    responses((status = 200, body = DataControls), ErrorResponses),
)]
async fn set_reasoner_key(
    scoped: Scoped,
    auth: Auth,
    ApiPath(id): ApiPath<String>,
    ApiJson(body): ApiJson<SetReasonerKeyRequest>,
) -> ApiResult<Json<DataControls>> {
    ensure_current(&auth, &id)?;
    let controls = scoped
        .identity()
        .set_reasoner_key(identity::SetReasonerKeyRequest {
            workspace_id: id,
            api_key: body.api_key,
        })
        .await?
        .into_inner();
    Ok(Json(controls.into()))
}

/// The workspace's secrets (admin): names, a 4-character hint and a
/// description. Values are write-only; no route returns one.
#[utoipa::path(
    get,
    path = "/workspaces/{id}/secrets",
    tag = "workspaces",
    params(("id" = String, Path, description = "Workspace id")),
    responses((status = 200, body = Vec<SecretInfo>), ErrorResponses),
)]
async fn list_secrets(
    scoped: Scoped,
    auth: Auth,
    ApiPath(id): ApiPath<String>,
) -> ApiResult<Json<Vec<SecretInfo>>> {
    ensure_current(&auth, &id)?;
    let response = scoped
        .identity_as_caller()
        .list_secrets(identity::ListSecretsRequest { workspace_id: id })
        .await?
        .into_inner();
    Ok(Json(map_all(response.secrets)))
}

/// Stores or replaces a workspace secret (admin). Deployers and registries
/// reference it by `name` (upper snake case, e.g. `JENKINS_TOKEN`); the
/// value is encrypted at rest and never returned.
#[utoipa::path(
    put,
    path = "/workspaces/{id}/secrets/{name}",
    tag = "workspaces",
    params(
        ("id" = String, Path, description = "Workspace id"),
        ("name" = String, Path, description = "Secret name, upper snake case"),
    ),
    request_body = SetSecretRequest,
    responses((status = 200, body = SecretInfo), ErrorResponses),
)]
async fn set_secret(
    scoped: Scoped,
    auth: Auth,
    ApiPath((id, name)): ApiPath<(String, String)>,
    ApiJson(body): ApiJson<SetSecretRequest>,
) -> ApiResult<Json<SecretInfo>> {
    ensure_current(&auth, &id)?;
    let secret = scoped
        .identity_as_caller()
        .set_secret(identity::SetSecretRequest {
            workspace_id: id,
            name,
            value: body.value,
            description: body.description.unwrap_or_default(),
        })
        .await?
        .into_inner();
    Ok(Json(secret.into()))
}

/// Deletes a workspace secret (admin). Deployers that name it fail with a
/// message saying so until it is stored again.
#[utoipa::path(
    delete,
    path = "/workspaces/{id}/secrets/{name}",
    tag = "workspaces",
    params(
        ("id" = String, Path, description = "Workspace id"),
        ("name" = String, Path, description = "Secret name"),
    ),
    responses((status = 200, description = "Deleted"), ErrorResponses),
)]
async fn delete_secret(
    scoped: Scoped,
    auth: Auth,
    ApiPath((id, name)): ApiPath<(String, String)>,
) -> ApiResult<axum::http::StatusCode> {
    ensure_current(&auth, &id)?;
    scoped
        .identity_as_caller()
        .delete_secret(identity::DeleteSecretRequest {
            workspace_id: id,
            name,
        })
        .await?;
    Ok(axum::http::StatusCode::OK)
}

/// Removes every secret of the workspace, one by one through the same RPC
/// an admin uses. Returns how many went.
async fn delete_all_secrets(scoped: &Scoped, workspace_id: &str) -> Result<u64, tonic::Status> {
    let mut client = scoped.identity_as_caller();
    let secrets = client
        .list_secrets(identity::ListSecretsRequest {
            workspace_id: workspace_id.to_owned(),
        })
        .await?
        .into_inner()
        .secrets;
    let mut removed = 0;
    for secret in secrets {
        match client
            .delete_secret(identity::DeleteSecretRequest {
                workspace_id: workspace_id.to_owned(),
                name: secret.name,
            })
            .await
        {
            Ok(_) => removed += 1,
            // Deleted by someone else in the meantime: gone is gone.
            Err(status) if status.code() == tonic::Code::NotFound => {}
            Err(status) => return Err(status),
        }
    }
    Ok(removed)
}

/// Everything stored for the workspace as one JSON document (admin),
/// streamed: workspace, members, data controls, every knowledge,
/// intelligence and execution row by entity, and the audit trail.
#[utoipa::path(
    post,
    path = "/workspaces/{id}/data/export",
    tag = "workspaces",
    params(("id" = String, Path, description = "Workspace id")),
    responses(
        (status = 200, description = "The archive, streamed", content_type = "application/json", body = DataExport),
        ErrorResponses,
    ),
)]
async fn export_data(
    scoped: Scoped,
    auth: Auth,
    ApiPath(id): ApiPath<String>,
) -> ApiResult<axum::response::Response> {
    ensure_current(&auth, &id)?;
    Ok(super::export::stream(scoped, auth))
}

/// Drops every cached file content and tree of the workspace (admin).
#[utoipa::path(
    post,
    path = "/workspaces/{id}/data/purge-code-cache",
    tag = "workspaces",
    params(("id" = String, Path, description = "Workspace id")),
    responses((status = 200, body = PurgeResult), ErrorResponses),
)]
async fn purge_code_cache(
    scoped: Scoped,
    auth: Auth,
    ApiPath(id): ApiPath<String>,
) -> ApiResult<Json<PurgeResult>> {
    ensure_current(&auth, &id)?;
    let response = scoped
        .knowledge()
        .purge_code_cache(knowledge::PurgeCodeCacheRequest {
            workspace_id: id,
            project_id: String::new(),
        })
        .await?
        .into_inner();
    Ok(Json(PurgeResult {
        files_deleted: response.files_deleted,
        trees_deleted: response.trees_deleted,
    }))
}

/// Deletes everything the platform stores for the workspace (owner):
/// every knowledge, intelligence and execution row, the workspace's
/// secrets, then the GitHub App installation record. The deletion itself
/// is audited.
#[utoipa::path(
    delete,
    path = "/workspaces/{id}",
    tag = "workspaces",
    params(("id" = String, Path, description = "Workspace id")),
    responses((status = 200, body = WorkspaceDeleted), ErrorResponses),
)]
async fn delete_workspace(
    scoped: Scoped,
    auth: Auth,
    ApiPath(id): ApiPath<String>,
) -> ApiResult<Json<WorkspaceDeleted>> {
    ensure_current(&auth, &id)?;
    let (knowledge, intelligence, execution) = tokio::try_join!(
        async {
            scoped
                .knowledge()
                .delete_workspace_data(knowledge::DeleteWorkspaceDataRequest {
                    workspace_id: id.clone(),
                })
                .await
                .map(|r| r.into_inner().deleted)
        },
        async {
            scoped
                .intelligence()
                .delete_workspace_data(intelligence::DeleteWorkspaceDataRequest {
                    workspace_id: id.clone(),
                })
                .await
                .map(|r| r.into_inner().deleted)
        },
        async {
            scoped
                .execution()
                .delete_workspace_data(execution::DeleteWorkspaceDataRequest {
                    workspace_id: id.clone(),
                })
                .await
                .map(|r| {
                    let inner = r.into_inner();
                    let mut deleted = inner.deleted;
                    deleted.insert("cancelled".into(), u64::from(inner.cancelled));
                    deleted
                })
        },
    )?;
    // The secrets go with the rows that named them (CONTRACT.md §22).
    let secrets = delete_all_secrets(&scoped, &id).await?;
    if let Some(installation_id) = auth.workspace.installation_id {
        scoped
            .identity()
            .remove_installation(identity::RemoveInstallationRequest { installation_id })
            .await?;
    }
    scoped.state.forget_auth(None, Some(&id));
    let mut deleted = BTreeMap::new();
    for (name, rows) in [
        ("knowledge", knowledge),
        ("intelligence", intelligence),
        ("execution", execution),
    ] {
        deleted.insert(name.to_owned(), rows.into_iter().collect());
    }
    deleted.insert(
        "identity".to_owned(),
        BTreeMap::from([("secret".to_owned(), secrets)]),
    );
    Ok(Json(WorkspaceDeleted {
        workspace_id: id,
        deleted,
    }))
}

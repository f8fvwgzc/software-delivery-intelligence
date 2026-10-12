use sdi_common::{
    grpc::db_status,
    principal::{Role, SYSTEM_USER},
};
use sdi_proto::knowledge::v1::*;
use tonic::Status;
use uuid::Uuid;

/// Workspaces one `CountProjects` call may ask about.
pub const MAX_COUNTED_WORKSPACES: usize = 100;

use super::{KnowledgeState, github_status, parse_id};
use crate::{
    auth::Caller,
    convert::{self, project_mode_to_db},
    db::{self, projects::NewProject},
    github::split_full_name,
    scanner,
};

pub(super) async fn connect_project(
    state: &KnowledgeState,
    caller: &Caller,
    req: ConnectProjectRequest,
) -> Result<Project, Status> {
    caller.require(Role::Member)?;
    let ws = caller.workspace_id;
    let (owner, name) = split_full_name(&req.repo_full_name)
        .ok_or_else(|| Status::invalid_argument("repo_full_name must look like owner/name"))?;
    if db::projects::get_by_repo(&state.pool, ws, &req.repo_full_name)
        .await
        .map_err(db_status)?
        .is_some()
    {
        return Err(Status::already_exists("repository is already connected"));
    }

    // The workspace's own token decides what it may connect.
    let inputs = state.scan_inputs(caller).await?;
    let repo = inputs
        .github
        .repo(owner, name)
        .await
        .map_err(github_status)?;
    let new = NewProject {
        github_repo_id: Some(repo.id),
        name: repo.name,
        repo_full_name: repo.full_name,
        repo_url: repo.html_url,
        default_branch: repo.default_branch,
        is_private: repo.private,
        description: repo.description.unwrap_or_default(),
        primary_language: repo.language.unwrap_or_default(),
    };
    let project = db::projects::insert(&state.pool, ws, &new)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db) if db.is_unique_violation() => {
                Status::already_exists("repository is already connected")
            }
            other => db_status(other),
        })?;
    state
        .events
        .project(ws, project.id, "created", &project.status);
    state.identity.audit(
        caller,
        "project.connect",
        "project",
        &project.id.to_string(),
        &project.repo_full_name,
    );
    // The scan flips the project to `scanning`; answer with that state so the
    // caller never shows a stale `pending`.
    scanner::start(&state.pool, &state.events, inputs, project.clone())
        .await
        .map_err(db_status)?;
    state
        .project(caller, &project.id.to_string())
        .await
        .map(convert::project)
}

pub(super) async fn list_projects(
    state: &KnowledgeState,
    caller: &Caller,
    _req: ListProjectsRequest,
) -> Result<ListProjectsResponse, Status> {
    let projects = db::projects::list(&state.pool, caller.workspace_id)
        .await
        .map_err(db_status)?;
    Ok(ListProjectsResponse {
        projects: projects.into_iter().map(convert::project).collect(),
    })
}

pub(super) async fn get_project(
    state: &KnowledgeState,
    caller: &Caller,
    req: GetProjectRequest,
) -> Result<Project, Status> {
    state.project(caller, &req.id).await.map(convert::project)
}

/// Connected projects per workspace in one query. A user may only count
/// their own workspace; the gateway's workspace switcher asks as `system`
/// (CONTRACT.md §18) for every workspace the user belongs to, which is the
/// one read that legitimately spans tenants. Ids that are not uuids or
/// hold no projects are simply absent from the answer.
pub(super) async fn count_projects(
    state: &KnowledgeState,
    caller: &Caller,
    req: CountProjectsRequest,
) -> Result<CountProjectsResponse, Status> {
    let mut ids: Vec<Uuid> = req
        .workspace_ids
        .iter()
        .filter_map(|raw| Uuid::parse_str(raw.trim()).ok())
        .collect();
    ids.sort_unstable();
    ids.dedup();
    if ids.is_empty() {
        ids.push(caller.workspace_id);
    }
    if ids.len() > MAX_COUNTED_WORKSPACES {
        return Err(Status::invalid_argument(format!(
            "at most {MAX_COUNTED_WORKSPACES} workspace ids per call"
        )));
    }
    if caller.user_id != SYSTEM_USER && ids.iter().any(|id| *id != caller.workspace_id) {
        return Err(Status::permission_denied(
            "only the system principal may count projects across workspaces",
        ));
    }
    let counts = db::projects::count_by_workspaces(&state.pool, &ids)
        .await
        .map_err(db_status)?;
    Ok(CountProjectsResponse {
        counts: counts
            .into_iter()
            .map(|(id, count)| (id.to_string(), u32::try_from(count).unwrap_or(u32::MAX)))
            .collect(),
    })
}

/// Disconnecting a project purges its code caches explicitly (the cascade
/// would too, but the purge is the contract) and then removes everything.
pub(super) async fn delete_project(
    state: &KnowledgeState,
    caller: &Caller,
    req: DeleteProjectRequest,
) -> Result<DeleteProjectResponse, Status> {
    caller.require(Role::Member)?;
    let ws = caller.workspace_id;
    let id = parse_id(&req.id, "project id")?;
    let mut tx = state.pool.begin().await.map_err(db_status)?;
    let purged = db::code::purge(&mut tx, ws, Some(id))
        .await
        .map_err(db_status)?;
    let deleted = db::projects::delete(&mut *tx, ws, id)
        .await
        .map_err(db_status)?;
    if !deleted {
        return Err(Status::not_found("project not found"));
    }
    tx.commit().await.map_err(db_status)?;
    tracing::info!(project = %id, files = purged.files, trees = purged.trees, "project deleted, caches purged");
    state.events.project(ws, id, "deleted", "");
    state
        .identity
        .audit(caller, "project.delete", "project", &id.to_string(), "");
    Ok(DeleteProjectResponse {})
}

pub(super) async fn set_project_mode(
    state: &KnowledgeState,
    caller: &Caller,
    req: SetProjectModeRequest,
) -> Result<Project, Status> {
    caller.require(Role::Member)?;
    let id = parse_id(&req.id, "project id")?;
    let mode = match ProjectMode::try_from(req.mode) {
        Ok(ProjectMode::Observe) => ProjectMode::Observe,
        Ok(ProjectMode::Execute) => ProjectMode::Execute,
        _ => return Err(Status::invalid_argument("mode must be observe or execute")),
    };
    let project = db::projects::set_mode(
        &state.pool,
        caller.workspace_id,
        id,
        &project_mode_to_db(mode),
    )
    .await
    .map_err(|e| match e {
        sqlx::Error::RowNotFound => Status::not_found("project not found"),
        other => db_status(other),
    })?;
    state
        .events
        .project(caller.workspace_id, id, "updated", &project.status);
    Ok(convert::project(project))
}

use sdi_common::grpc::db_status;
use sdi_proto::knowledge::v1::*;
use tonic::Status;

use super::{KnowledgeState, github_status, parse_id};
use crate::{
    convert::{self, project_mode_to_db},
    db::{self, projects::NewProject},
    github::split_full_name,
    scanner,
};

pub(super) async fn connect_project(
    state: &KnowledgeState,
    req: ConnectProjectRequest,
) -> Result<Project, Status> {
    let (owner, name) = split_full_name(&req.repo_full_name)
        .ok_or_else(|| Status::invalid_argument("repo_full_name must look like owner/name"))?;
    if db::projects::get_by_repo(&state.pool, &req.repo_full_name)
        .await
        .map_err(db_status)?
        .is_some()
    {
        return Err(Status::already_exists("repository is already connected"));
    }

    let repo = state
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
    let project = db::projects::insert(&state.pool, &new)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db) if db.is_unique_violation() => {
                Status::already_exists("repository is already connected")
            }
            other => db_status(other),
        })?;
    state.events.project(project.id, "created", &project.status);
    // The scan flips the project to `scanning`; answer with that state so the
    // caller never shows a stale `pending`.
    scanner::start(&state.pool, &state.github, &state.events, project.clone())
        .await
        .map_err(db_status)?;
    state
        .project(&project.id.to_string())
        .await
        .map(convert::project)
}

pub(super) async fn list_projects(
    state: &KnowledgeState,
    _req: ListProjectsRequest,
) -> Result<ListProjectsResponse, Status> {
    let projects = db::projects::list(&state.pool).await.map_err(db_status)?;
    Ok(ListProjectsResponse {
        projects: projects.into_iter().map(convert::project).collect(),
    })
}

pub(super) async fn get_project(
    state: &KnowledgeState,
    req: GetProjectRequest,
) -> Result<Project, Status> {
    state.project(&req.id).await.map(convert::project)
}

pub(super) async fn delete_project(
    state: &KnowledgeState,
    req: DeleteProjectRequest,
) -> Result<DeleteProjectResponse, Status> {
    let id = parse_id(&req.id, "project id")?;
    if !db::projects::delete(&state.pool, id)
        .await
        .map_err(db_status)?
    {
        return Err(Status::not_found("project not found"));
    }
    state.events.project(id, "deleted", "");
    Ok(DeleteProjectResponse {})
}

pub(super) async fn set_project_mode(
    state: &KnowledgeState,
    req: SetProjectModeRequest,
) -> Result<Project, Status> {
    let id = parse_id(&req.id, "project id")?;
    let mode = match ProjectMode::try_from(req.mode) {
        Ok(ProjectMode::Observe) => ProjectMode::Observe,
        Ok(ProjectMode::Execute) => ProjectMode::Execute,
        _ => return Err(Status::invalid_argument("mode must be observe or execute")),
    };
    let project = db::projects::set_mode(&state.pool, id, &project_mode_to_db(mode))
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => Status::not_found("project not found"),
            other => db_status(other),
        })?;
    state.events.project(id, "updated", &project.status);
    Ok(convert::project(project))
}

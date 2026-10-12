//! Starting a deployment run (CONTRACT.md §22) and what follows one.
//!
//! `Promote` and `Rollback` both end in [`start`]: pick the release, build
//! the four-step plan, store the execution and the run in one transaction,
//! then start the execution. From there the run is the runner's: its status
//! is a projection of the execution (`projection`), and when a promotion
//! succeeds the next stage is promoted too if it says `auto_promote`
//! ([`after_success`]). The gates stay the same either way: an automatic
//! promotion still waits for its permission, its policy and its approval.

use std::{collections::BTreeMap, sync::Arc};

use sdi_common::{grpc::db_status, principal::Principal};
use sdi_proto::execution::v1::DeploymentStatus;
use tonic::Status;
use uuid::Uuid;

use super::{
    db::{
        environments::{self, EnvironmentRow},
        releases::{self, ReleaseRow},
        runs::{self, NewRun, RunRow},
    },
    model::{self, RUN_PROMOTE, RUN_ROLLBACK},
    pipeline, plan, registry_for,
};
use crate::{
    adapters,
    grpc::executions::{NewPlan, insert_plan, publish_created},
    identity,
    runner::{Command, Runtime},
};

/// What `start` is asked to do.
pub struct StartRun {
    pub environment_id: Uuid,
    /// `None` picks the stage's candidate (promote) or the previous
    /// successful release (rollback).
    pub release_id: Option<Uuid>,
    /// `promote` or `rollback`.
    pub kind: &'static str,
    pub reason: String,
}

/// The release a promotion without a `release_id` takes: the candidate.
async fn candidate(
    rt: &Runtime,
    workspace: Uuid,
    environment: &EnvironmentRow,
) -> Result<ReleaseRow, Status> {
    pipeline::candidate_for(&rt.pool, workspace, environment)
        .await
        .map_err(db_status)?
        .ok_or_else(|| {
            Status::failed_precondition(if environment.position <= 0 {
                format!(
                    "nothing to promote: {} already runs the newest release (or the project has none)",
                    environment.name
                )
            } else {
                format!(
                    "nothing to promote: no release newer than what {} runs has succeeded in the previous stage",
                    environment.name
                )
            })
        })
}

/// The release a rollback goes back to: the most recent one that ran
/// successfully in the environment and is not the release of its latest
/// deployment. After a healthy deploy of R2 that is R1; after a failed
/// deploy of R2 it is R1 as well, the release still (supposed to be) there.
pub async fn rollback_target(
    rt: &Runtime,
    workspace: Uuid,
    environment_id: Uuid,
) -> Result<Option<Uuid>, Status> {
    let latest = runs::latest_for_environment(&rt.pool, workspace, environment_id)
        .await
        .map_err(db_status)?;
    let Some(latest) = latest else {
        return Ok(None);
    };
    runs::last_successful_release(&rt.pool, workspace, environment_id, Some(latest.release_id))
        .await
        .map_err(db_status)
}

async fn pick_release(
    rt: &Runtime,
    workspace: Uuid,
    environment: &EnvironmentRow,
    req: &StartRun,
) -> Result<ReleaseRow, Status> {
    let release =
        match (req.release_id, req.kind) {
            (Some(id), _) => releases::get(&rt.pool, workspace, id)
                .await
                .map_err(|error| match error {
                    sqlx::Error::RowNotFound => Status::not_found("no such release"),
                    other => db_status(other),
                })?,
            (None, RUN_ROLLBACK) => {
                let target = rollback_target(rt, workspace, environment.id)
                    .await?
                    .ok_or_else(|| {
                        Status::failed_precondition(format!(
                            "{} has no earlier successful release to roll back to",
                            environment.name
                        ))
                    })?;
                releases::get(&rt.pool, workspace, target)
                    .await
                    .map_err(db_status)?
            }
            (None, _) => candidate(rt, workspace, environment).await?,
        };
    if release.project_id != environment.project_id {
        return Err(Status::invalid_argument(
            "the release belongs to another project than the environment",
        ));
    }
    if req.kind == RUN_ROLLBACK
        && req.release_id.is_some()
        && !runs::has_succeeded(&rt.pool, workspace, environment.id, release.id)
            .await
            .map_err(db_status)?
    {
        return Err(Status::failed_precondition(format!(
            "{} never ran successfully in {}; a rollback goes back to a release that did (promote it instead)",
            release.version, environment.name
        )));
    }
    Ok(release)
}

/// Creates the run and its execution and starts it. `principal` is who
/// asked: an approver, or the workspace's system principal for automatic
/// promotions and remediation rollbacks.
pub async fn start(
    rt: &Arc<Runtime>,
    principal: &Principal,
    workspace: Uuid,
    req: StartRun,
) -> Result<RunRow, Status> {
    let environment = environments::get(&rt.pool, workspace, req.environment_id)
        .await
        .map_err(|error| match error {
            sqlx::Error::RowNotFound => Status::not_found("no such environment"),
            other => db_status(other),
        })?;
    let release = pick_release(rt, workspace, &environment, &req).await?;
    let registry = registry_for(rt, workspace, environment.registry_id).await?;
    let input = plan::PlanInput {
        environment: &environment,
        release: &release,
        registry: &registry,
        run_kind: req.kind,
    };
    let steps = plan::build(&input);
    let title = plan::title(&input);

    let mut tx = rt.pool.begin().await.map_err(db_status)?;
    // One deployment to an environment at a time: the row lock makes two
    // concurrent promotions see each other.
    let locked = environments::lock(&mut tx, workspace, environment.id)
        .await
        .map_err(db_status)?;
    if let Some(open) = runs::open_for_environment(&mut *tx, workspace, environment.id)
        .await
        .map_err(db_status)?
    {
        return Err(Status::failed_precondition(format!(
            "a deployment of {} to {} is still open ({}); finish, resolve or cancel it first",
            open.release_version,
            environment.name,
            model::status_text(model::status_from_db(&open.status))
        )));
    }
    let (execution_id, event) = insert_plan(
        &mut tx,
        rt,
        workspace,
        NewPlan {
            project_id: &environment.project_id,
            project_name: &release.project_name,
            proposal_id: "",
            pattern_id: "",
            title: &title,
            steps: &steps,
            runtime: adapters::DELIVERY,
            artifacts: &BTreeMap::new(),
            created_by: &principal.user_id,
        },
    )
    .await?;
    let run = runs::insert(
        &mut tx,
        NewRun {
            id: Uuid::now_v7(),
            workspace_id: workspace,
            project_id: &environment.project_id,
            environment_id: environment.id,
            environment_name: &environment.name,
            release_id: release.id,
            release_version: &release.version,
            kind: req.kind,
            status: model::status_to_db(DeploymentStatus::Pending),
            execution_id,
            previous_release_id: locked.current_release_id,
            started_by: &principal.user_id,
            reason: req.reason.trim(),
        },
    )
    .await
    .map_err(db_status)?;
    tx.commit().await.map_err(db_status)?;

    publish_created(
        rt,
        workspace,
        execution_id,
        &environment.project_id,
        adapters::DELIVERY,
        event,
    );
    rt.platform.delivery(
        workspace,
        "deployment",
        &run.id.to_string(),
        &run.project_id,
        "created",
        &model::status_text(DeploymentStatus::Pending),
        run_data(&run),
    );
    rt.audit.record(
        principal,
        identity::entry(
            principal,
            if req.kind == RUN_ROLLBACK {
                "deployment.rollback"
            } else {
                "deployment.promote"
            },
            "deployment",
            &run.id.to_string(),
            format!(
                "{} {} ({}) to {}{}",
                req.kind,
                release.version,
                release.image,
                environment.name,
                if req.reason.trim().is_empty() {
                    String::new()
                } else {
                    format!(": {}", req.reason.trim())
                }
            ),
        ),
    );

    rt.command(workspace, execution_id, |reply| Command::Start {
        fault: None,
        reply,
    })
    .await?;
    runs::get(&rt.pool, workspace, run.id)
        .await
        .map_err(db_status)
}

/// What a deployment event tells its listeners.
pub fn run_data(run: &RunRow) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("environment_id".to_owned(), run.environment_id.to_string()),
        ("environment".to_owned(), run.environment_name.clone()),
        ("release_id".to_owned(), run.release_id.to_string()),
        ("execution_id".to_owned(), run.execution_id.to_string()),
        ("kind".to_owned(), run.kind.clone()),
    ])
}

/// A promotion succeeded: promote the same release into the next stage
/// when that stage says `auto_promote`. Runs in the background as the
/// workspace's system principal; a refusal (the next stage is busy, or
/// already runs something newer) is logged, never an error of the run that
/// just succeeded.
pub fn after_success(rt: &Arc<Runtime>, run: RunRow) {
    if run.kind != RUN_PROMOTE {
        return;
    }
    let rt = Arc::clone(rt);
    sdi_common::tasks::spawn("auto-promote", async move {
        match promote_next(&rt, &run).await {
            Ok(Some(next)) => tracing::info!(
                release = %next.release_version,
                environment = %next.environment_name,
                run = %next.id,
                "auto-promoted into the next stage"
            ),
            Ok(None) => {}
            Err(status) => tracing::warn!(
                release = %run.release_version,
                after = %run.environment_name,
                %status,
                "auto-promotion did not start"
            ),
        }
    });
}

async fn promote_next(rt: &Arc<Runtime>, run: &RunRow) -> Result<Option<RunRow>, Status> {
    let stages = environments::list_for_project(&rt.pool, run.workspace_id, &run.project_id)
        .await
        .map_err(db_status)?;
    let Some(index) = stages.iter().position(|e| e.id == run.environment_id) else {
        return Ok(None);
    };
    let Some(next) = stages.get(index + 1) else {
        return Ok(None);
    };
    if !next.auto_promote || next.current_release_id == Some(run.release_id) {
        return Ok(None);
    }
    auto_promote_into(
        rt,
        run.workspace_id,
        next,
        run.release_id,
        &run.environment_name,
    )
    .await
}

/// Promotes `release_id` into `stage` unless it already runs something as
/// new or newer (an automatic promotion never moves a stage backwards).
async fn auto_promote_into(
    rt: &Arc<Runtime>,
    workspace: Uuid,
    stage: &EnvironmentRow,
    release_id: Uuid,
    after: &str,
) -> Result<Option<RunRow>, Status> {
    let release = releases::get(&rt.pool, workspace, release_id)
        .await
        .map_err(db_status)?;
    if let Some(current) = stage.current_release_id {
        let current = releases::get(&rt.pool, workspace, current)
            .await
            .map_err(db_status)?;
        if (current.created_at, current.id) >= (release.created_at, release.id) {
            return Ok(None);
        }
    }
    let principal = Principal::system(workspace.to_string());
    start(
        rt,
        &principal,
        workspace,
        StartRun {
            environment_id: stage.id,
            release_id: Some(release_id),
            kind: RUN_PROMOTE,
            reason: format!("auto-promote after {after}"),
        },
    )
    .await
    .map(Some)
}

/// A release was just created: promote it into the first stage when that
/// stage says `auto_promote` (the build is the stage before it).
pub fn after_release(rt: &Arc<Runtime>, release: ReleaseRow) {
    let rt = Arc::clone(rt);
    sdi_common::tasks::spawn("auto-promote", async move {
        let stages = match environments::list_for_project(
            &rt.pool,
            release.workspace_id,
            &release.project_id,
        )
        .await
        {
            Ok(stages) => stages,
            Err(error) => {
                tracing::warn!(%error, "auto-promotion could not read the stages");
                return;
            }
        };
        let Some(first) = stages.first().filter(|stage| stage.auto_promote) else {
            return;
        };
        match auto_promote_into(&rt, release.workspace_id, first, release.id, "the build").await {
            Ok(Some(run)) => tracing::info!(
                release = %run.release_version,
                environment = %run.environment_name,
                run = %run.id,
                "auto-promoted a new release into the first stage"
            ),
            Ok(None) => {}
            Err(status) => tracing::warn!(
                release = %release.version,
                %status,
                "auto-promotion of a new release did not start"
            ),
        }
    });
}

/// The `rollback_to_previous` remediation: the failed run was cancelled,
/// now its environment goes back to its last good release.
pub async fn rollback_after_failure(
    rt: &Arc<Runtime>,
    workspace: Uuid,
    execution_id: Uuid,
    by: &str,
) -> Result<RunRow, Status> {
    let failed = runs::by_execution(&rt.pool, workspace, execution_id)
        .await
        .map_err(db_status)?
        .ok_or_else(|| Status::failed_precondition("this execution is not a deployment run"))?;
    let target = rollback_target(rt, workspace, failed.environment_id)
        .await?
        .ok_or_else(|| {
            Status::failed_precondition(format!(
                "{} has no earlier successful release to roll back to",
                failed.environment_name
            ))
        })?;
    let principal = Principal {
        user_id: if by.trim().is_empty() {
            sdi_common::principal::SYSTEM_USER.to_owned()
        } else {
            by.trim().to_owned()
        },
        ..Principal::system(workspace.to_string())
    };
    start(
        rt,
        &principal,
        workspace,
        StartRun {
            environment_id: failed.environment_id,
            release_id: Some(target),
            kind: RUN_ROLLBACK,
            reason: format!(
                "deployment of {} to {} failed",
                failed.release_version, failed.environment_name
            ),
        },
    )
    .await
}

/// Whether the run behind `execution_id` could be rolled back: it is a
/// deployment run and its environment has an earlier good release.
pub async fn can_rollback(rt: &Runtime, workspace: Uuid, execution_id: Uuid) -> bool {
    let Ok(Some(run)) = runs::by_execution(&rt.pool, workspace, execution_id).await else {
        return false;
    };
    matches!(
        rollback_target(rt, workspace, run.environment_id).await,
        Ok(Some(_))
    )
}

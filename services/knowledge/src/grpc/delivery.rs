use sdi_common::{grpc::db_status, time::opt_to_proto};
use sdi_proto::knowledge::v1::*;
use sqlx::PgPool;
use tonic::Status;
use uuid::Uuid;

use super::{KnowledgeState, anyhow_status, limit, parse_id};
use crate::{convert, db, domain::stats, scanner::delivery as sync};

/// Rows considered for statistics; the response itself is cut to `limit`.
const STATS_WINDOW: i64 = 500;

pub(super) async fn list_pipelines(
    state: &KnowledgeState,
    req: ListPipelinesRequest,
) -> Result<ListPipelinesResponse, Status> {
    let project_id = parse_id(&req.project_id, "project id")?;
    let pipelines = db::pipelines::list(&state.pool, project_id)
        .await
        .map_err(db_status)?;
    Ok(ListPipelinesResponse {
        pipelines: pipelines.into_iter().map(convert::pipeline).collect(),
    })
}

pub(super) async fn list_pipeline_runs(
    state: &KnowledgeState,
    req: ListPipelineRunsRequest,
) -> Result<ListPipelineRunsResponse, Status> {
    let mut project = state.project(&req.project_id).await?;
    if req.refresh {
        sync::sync_runs(&state.pool, &state.github, &project)
            .await
            .map_err(anyhow_status)?;
        project = state.project(&req.project_id).await?;
    }
    let rows = db::delivery::list_runs(&state.pool, project.id, STATS_WINDOW)
        .await
        .map_err(db_status)?;
    let stats = pipeline_stats(&state.pool, project.id).await?;
    let runs = rows
        .into_iter()
        .take(limit(req.limit, 50, STATS_WINDOW as u32) as usize)
        .map(convert::pipeline_run)
        .collect();
    Ok(ListPipelineRunsResponse {
        runs,
        stats: Some(stats),
        synced_at: opt_to_proto(project.runs_synced_at),
    })
}

pub(super) async fn list_deployments(
    state: &KnowledgeState,
    req: ListDeploymentsRequest,
) -> Result<ListDeploymentsResponse, Status> {
    let project = state.project(&req.project_id).await?;
    if req.refresh {
        sync::sync_deployments(&state.pool, &state.github, &project)
            .await
            .map_err(anyhow_status)?;
    }
    let rows = db::delivery::list_deployments(&state.pool, project.id, STATS_WINDOW)
        .await
        .map_err(db_status)?;
    let stats = deployment_stats(&state.pool, project.id).await?;
    let deployments = rows
        .into_iter()
        .take(limit(req.limit, 50, STATS_WINDOW as u32) as usize)
        .map(convert::deployment)
        .collect();
    Ok(ListDeploymentsResponse {
        deployments,
        stats: Some(stats),
    })
}

pub(super) async fn pipeline_stats(
    pool: &PgPool,
    project_id: Uuid,
) -> Result<PipelineStats, Status> {
    let rows = db::delivery::list_runs(pool, project_id, STATS_WINDOW)
        .await
        .map_err(db_status)?;
    let statuses: Vec<&str> = rows.iter().map(|r| r.status.as_str()).collect();
    let durations: Vec<u32> = rows
        .iter()
        .map(|r| r.duration_seconds.max(0) as u32)
        .collect();
    let s = stats::pipeline_stats(&statuses, &durations);
    Ok(PipelineStats {
        total: s.total,
        succeeded: s.succeeded,
        failed: s.failed,
        success_rate: s.success_rate,
        avg_duration_seconds: s.avg_duration_seconds,
    })
}

pub(super) async fn deployment_stats(
    pool: &PgPool,
    project_id: Uuid,
) -> Result<DeploymentStats, Status> {
    let rows = db::delivery::list_deployments(pool, project_id, STATS_WINDOW)
        .await
        .map_err(db_status)?;
    let statuses: Vec<&str> = rows.iter().map(|r| r.status.as_str()).collect();
    let s = stats::deployment_stats(&statuses);
    Ok(DeploymentStats {
        total: s.total,
        succeeded: s.succeeded,
        failed: s.failed,
        success_rate: s.success_rate,
    })
}

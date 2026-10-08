//! Pipeline runs and deployments observed from the provider. Rows are keyed
//! by the provider's id so a refresh upserts instead of duplicating.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgConnection, PgExecutor, types::Json};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Failure {
    pub job: String,
    pub step: String,
    pub summary: String,
    pub suggested_fix: String,
}

#[derive(Debug, Clone, FromRow)]
pub struct RunRow {
    pub id: Uuid,
    pub project_id: Uuid,
    pub provider_run_id: i64,
    pub pipeline_name: String,
    pub pipeline_path: String,
    pub run_number: i64,
    pub status: String,
    pub branch: String,
    pub commit_sha: String,
    pub commit_message: String,
    pub actor: String,
    pub event: String,
    pub started_at: Option<DateTime<Utc>>,
    pub duration_seconds: i32,
    pub html_url: String,
    pub failure: Option<Json<Failure>>,
    pub synced_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewRun {
    pub provider_run_id: i64,
    pub pipeline_name: String,
    pub pipeline_path: String,
    pub run_number: i64,
    pub status: String,
    pub branch: String,
    pub commit_sha: String,
    pub commit_message: String,
    pub actor: String,
    pub event: String,
    pub started_at: Option<DateTime<Utc>>,
    pub duration_seconds: i32,
    pub html_url: String,
    pub failure: Option<Failure>,
}

#[derive(Debug, Clone, FromRow)]
pub struct DeploymentRow {
    pub id: Uuid,
    pub project_id: Uuid,
    pub provider_id: i64,
    pub source: String,
    pub environment: String,
    pub r#ref: String,
    pub commit_sha: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub creator: String,
    pub url: String,
    pub description: String,
    pub synced_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewDeployment {
    pub provider_id: i64,
    pub source: String,
    pub environment: String,
    pub r#ref: String,
    pub commit_sha: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub creator: String,
    pub url: String,
    pub description: String,
}

pub async fn upsert_runs(
    conn: &mut PgConnection,
    project_id: Uuid,
    runs: &[NewRun],
) -> sqlx::Result<()> {
    for run in runs {
        sqlx::query(
            "INSERT INTO pipeline_runs (id, project_id, provider_run_id, pipeline_name, pipeline_path, run_number, status, branch,
                commit_sha, commit_message, actor, event, started_at, duration_seconds, html_url, failure, synced_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, now())
             ON CONFLICT (project_id, provider_run_id) DO UPDATE SET
               pipeline_name = EXCLUDED.pipeline_name, pipeline_path = EXCLUDED.pipeline_path, status = EXCLUDED.status,
               commit_message = EXCLUDED.commit_message, actor = EXCLUDED.actor, started_at = EXCLUDED.started_at,
               duration_seconds = EXCLUDED.duration_seconds, html_url = EXCLUDED.html_url,
               failure = EXCLUDED.failure, synced_at = now()",
        )
        .bind(Uuid::now_v7())
        .bind(project_id)
        .bind(run.provider_run_id)
        .bind(&run.pipeline_name)
        .bind(&run.pipeline_path)
        .bind(run.run_number)
        .bind(&run.status)
        .bind(&run.branch)
        .bind(&run.commit_sha)
        .bind(&run.commit_message)
        .bind(&run.actor)
        .bind(&run.event)
        .bind(run.started_at)
        .bind(run.duration_seconds)
        .bind(&run.html_url)
        .bind(run.failure.as_ref().map(Json))
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

pub async fn list_runs(
    exec: impl PgExecutor<'_>,
    project_id: Uuid,
    limit: i64,
) -> sqlx::Result<Vec<RunRow>> {
    sqlx::query_as::<_, RunRow>(
        "SELECT * FROM pipeline_runs WHERE project_id = $1 ORDER BY started_at DESC NULLS LAST, run_number DESC LIMIT $2",
    )
    .bind(project_id)
    .bind(limit)
    .fetch_all(exec)
    .await
}

pub async fn upsert_deployments(
    conn: &mut PgConnection,
    project_id: Uuid,
    deployments: &[NewDeployment],
) -> sqlx::Result<()> {
    for d in deployments {
        sqlx::query(
            "INSERT INTO deployments (id, project_id, provider_id, source, environment, ref, commit_sha, status, created_at, creator, url, description, synced_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, now())
             ON CONFLICT (project_id, source, provider_id) DO UPDATE SET
               environment = EXCLUDED.environment, status = EXCLUDED.status, url = EXCLUDED.url,
               description = EXCLUDED.description, synced_at = now()",
        )
        .bind(Uuid::now_v7())
        .bind(project_id)
        .bind(d.provider_id)
        .bind(&d.source)
        .bind(&d.environment)
        .bind(&d.r#ref)
        .bind(&d.commit_sha)
        .bind(&d.status)
        .bind(d.created_at)
        .bind(&d.creator)
        .bind(&d.url)
        .bind(&d.description)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

pub async fn list_deployments(
    exec: impl PgExecutor<'_>,
    project_id: Uuid,
    limit: i64,
) -> sqlx::Result<Vec<DeploymentRow>> {
    sqlx::query_as::<_, DeploymentRow>(
        "SELECT * FROM deployments WHERE project_id = $1 ORDER BY created_at DESC LIMIT $2",
    )
    .bind(project_id)
    .bind(limit)
    .fetch_all(exec)
    .await
}

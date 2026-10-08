use sdi_analyzer::PipelineDraft;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgConnection, PgExecutor, types::Json};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub key: String,
    pub name: String,
    pub runs_on: String,
    pub needs: Vec<String>,
    pub steps: Vec<String>,
}

#[derive(Debug, Clone, FromRow)]
pub struct PipelineRow {
    pub id: Uuid,
    pub project_id: Uuid,
    pub provider: String,
    pub name: String,
    pub path: String,
    pub triggers: Vec<String>,
    pub jobs: Json<Vec<Job>>,
}

pub async fn replace(
    conn: &mut PgConnection,
    project_id: Uuid,
    pipelines: &[PipelineDraft],
) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM pipelines WHERE project_id = $1")
        .bind(project_id)
        .execute(&mut *conn)
        .await?;
    for pipeline in pipelines {
        let jobs: Vec<Job> = pipeline
            .jobs
            .iter()
            .map(|j| Job {
                key: j.key.clone(),
                name: j.name.clone(),
                runs_on: j.runs_on.clone(),
                needs: j.needs.clone(),
                steps: j.steps.clone(),
            })
            .collect();
        sqlx::query(
            "INSERT INTO pipelines (id, project_id, provider, name, path, triggers, jobs) VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(Uuid::now_v7())
        .bind(project_id)
        .bind(&pipeline.provider)
        .bind(&pipeline.name)
        .bind(&pipeline.path)
        .bind(&pipeline.triggers)
        .bind(Json(jobs))
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

pub async fn list(exec: impl PgExecutor<'_>, project_id: Uuid) -> sqlx::Result<Vec<PipelineRow>> {
    sqlx::query_as::<_, PipelineRow>("SELECT * FROM pipelines WHERE project_id = $1 ORDER BY path")
        .bind(project_id)
        .fetch_all(exec)
        .await
}

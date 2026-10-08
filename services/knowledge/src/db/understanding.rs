use chrono::{DateTime, Utc};
use sdi_analyzer::UnderstandingDraft;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgExecutor, types::Json};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Section {
    pub key: String,
    pub title: String,
    pub summary: String,
    pub items: Vec<String>,
}

#[derive(Debug, Clone, FromRow)]
pub struct UnderstandingRow {
    pub project_id: Uuid,
    pub tldr: String,
    pub architecture: String,
    pub stack: Vec<String>,
    pub sections: Json<Vec<Section>>,
    pub request_flow: Vec<String>,
    pub commit_sha: String,
    pub generated_at: DateTime<Utc>,
}

pub async fn upsert(
    exec: impl PgExecutor<'_>,
    project_id: Uuid,
    draft: &UnderstandingDraft,
    commit_sha: &str,
) -> sqlx::Result<()> {
    let sections: Vec<Section> = draft
        .sections
        .iter()
        .map(|s| Section {
            key: s.key.clone(),
            title: s.title.clone(),
            summary: s.summary.clone(),
            items: s.items.clone(),
        })
        .collect();
    sqlx::query(
        "INSERT INTO understandings (project_id, tldr, architecture, stack, sections, request_flow, commit_sha, generated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, now())
         ON CONFLICT (project_id) DO UPDATE SET
           tldr = EXCLUDED.tldr, architecture = EXCLUDED.architecture, stack = EXCLUDED.stack,
           sections = EXCLUDED.sections, request_flow = EXCLUDED.request_flow,
           commit_sha = EXCLUDED.commit_sha, generated_at = now()",
    )
    .bind(project_id)
    .bind(&draft.tldr)
    .bind(&draft.architecture)
    .bind(&draft.stack)
    .bind(Json(sections))
    .bind(&draft.request_flow)
    .bind(commit_sha)
    .execute(exec)
    .await?;
    Ok(())
}

pub async fn get(
    exec: impl PgExecutor<'_>,
    project_id: Uuid,
) -> sqlx::Result<Option<UnderstandingRow>> {
    sqlx::query_as::<_, UnderstandingRow>("SELECT * FROM understandings WHERE project_id = $1")
        .bind(project_id)
        .fetch_optional(exec)
        .await
}

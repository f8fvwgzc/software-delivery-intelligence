use sdi_common::embed::to_pgvector;
use sqlx::{FromRow, PgExecutor};
use uuid::Uuid;

use super::parse_pgvector;
use crate::domain::similarity::Profile;

#[derive(Debug, Clone, FromRow)]
pub struct ProfileRow {
    pub project_id: Uuid,
    /// pgvector literal; see [`parse_pgvector`].
    pub embedding: String,
    pub profile_text: String,
    pub technologies: Vec<String>,
    pub capabilities: Vec<String>,
}

impl ProfileRow {
    pub fn to_profile(&self) -> Profile {
        Profile {
            technologies: self.technologies.clone(),
            capabilities: self.capabilities.clone(),
            embedding: parse_pgvector(&self.embedding),
        }
    }
}

const SELECT: &str = "SELECT project_id, embedding::text AS embedding, profile_text, technologies, capabilities FROM project_profiles";

pub async fn upsert(
    exec: impl PgExecutor<'_>,
    project_id: Uuid,
    profile_text: &str,
    technologies: &[String],
    capabilities: &[String],
    embedding: &[f32],
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO project_profiles (project_id, embedding, profile_text, technologies, capabilities, updated_at)
         VALUES ($1, $2::vector, $3, $4, $5, now())
         ON CONFLICT (project_id) DO UPDATE SET
           embedding = EXCLUDED.embedding, profile_text = EXCLUDED.profile_text,
           technologies = EXCLUDED.technologies, capabilities = EXCLUDED.capabilities, updated_at = now()",
    )
    .bind(project_id)
    .bind(to_pgvector(embedding))
    .bind(profile_text)
    .bind(technologies)
    .bind(capabilities)
    .execute(exec)
    .await?;
    Ok(())
}

pub async fn get(exec: impl PgExecutor<'_>, project_id: Uuid) -> sqlx::Result<Option<ProfileRow>> {
    sqlx::query_as::<_, ProfileRow>(sqlx::AssertSqlSafe(format!(
        "{SELECT} WHERE project_id = $1"
    )))
    .bind(project_id)
    .fetch_optional(exec)
    .await
}

pub async fn list(exec: impl PgExecutor<'_>) -> sqlx::Result<Vec<ProfileRow>> {
    sqlx::query_as::<_, ProfileRow>(SELECT)
        .fetch_all(exec)
        .await
}

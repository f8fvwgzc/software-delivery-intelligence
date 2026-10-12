//! Repositories over the `intelligence` schema. Runtime queries only, so the
//! crate builds without a database. Every function takes the workspace it
//! operates in (CONTRACT.md §18): an id alone never addresses a row.

pub mod decisions;
pub mod diagrams;
pub mod documents;
pub mod messages;
pub mod proposals;
pub mod reviews;
pub mod settings;
pub mod workspace;

use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(Debug, FromRow)]
pub struct StatsRow {
    pub proposals: i64,
    pub proposals_open: i64,
    pub proposals_accepted: i64,
    pub decisions_active: i64,
    pub documents: i64,
    pub messages: i64,
}

pub async fn stats(pool: &PgPool, workspace_id: Uuid) -> sqlx::Result<StatsRow> {
    sqlx::query_as(
        "SELECT
            (SELECT count(*) FROM proposals WHERE workspace_id = $1) AS proposals,
            (SELECT count(*) FROM proposals WHERE workspace_id = $1 AND status IN ('proposed', 'negotiating')) AS proposals_open,
            (SELECT count(*) FROM proposals WHERE workspace_id = $1 AND status = 'accepted') AS proposals_accepted,
            (SELECT count(*) FROM decisions WHERE workspace_id = $1 AND status = 'active') AS decisions_active,
            (SELECT count(*) FROM documents WHERE workspace_id = $1) AS documents,
            (SELECT count(*) FROM messages WHERE workspace_id = $1) AS messages",
    )
    .bind(workspace_id)
    .fetch_one(pool)
    .await
}

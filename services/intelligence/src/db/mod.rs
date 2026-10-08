//! Repositories over the `intelligence` schema. Runtime queries only, so the
//! crate builds without a database.

pub mod decisions;
pub mod documents;
pub mod messages;
pub mod proposals;

use sqlx::{FromRow, PgPool};

#[derive(Debug, FromRow)]
pub struct StatsRow {
    pub proposals: i64,
    pub proposals_open: i64,
    pub proposals_accepted: i64,
    pub decisions_active: i64,
    pub documents: i64,
    pub messages: i64,
}

pub async fn stats(pool: &PgPool) -> sqlx::Result<StatsRow> {
    sqlx::query_as(
        "SELECT
            (SELECT count(*) FROM proposals) AS proposals,
            (SELECT count(*) FROM proposals WHERE status IN ('proposed', 'negotiating')) AS proposals_open,
            (SELECT count(*) FROM proposals WHERE status = 'accepted') AS proposals_accepted,
            (SELECT count(*) FROM decisions WHERE status = 'active') AS decisions_active,
            (SELECT count(*) FROM documents) AS documents,
            (SELECT count(*) FROM messages) AS messages",
    )
    .fetch_one(pool)
    .await
}

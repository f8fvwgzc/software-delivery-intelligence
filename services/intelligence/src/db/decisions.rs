use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
pub struct DecisionRow {
    pub id: Uuid,
    pub project_id: String,
    pub scope: String,
    pub status: String,
    pub source: String,
    pub title: String,
    pub statement: String,
    pub reasons: Vec<String>,
    pub approved_by: String,
    pub proposal_id: String,
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub superseded_at: Option<DateTime<Utc>>,
    pub superseded_by: String,
}

const COLUMNS: &str = "id, project_id, scope, status, source, title, statement, reasons, approved_by, proposal_id, \
    tags, created_at, superseded_at, superseded_by";

pub async fn insert(pool: &PgPool, row: &DecisionRow) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO decisions (id, project_id, scope, status, source, title, statement, reasons, approved_by, \
         proposal_id, tags, created_at, superseded_at, superseded_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)",
    )
    .bind(row.id)
    .bind(&row.project_id)
    .bind(&row.scope)
    .bind(&row.status)
    .bind(&row.source)
    .bind(&row.title)
    .bind(&row.statement)
    .bind(&row.reasons)
    .bind(&row.approved_by)
    .bind(&row.proposal_id)
    .bind(&row.tags)
    .bind(row.created_at)
    .bind(row.superseded_at)
    .bind(&row.superseded_by)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get(pool: &PgPool, id: Uuid) -> sqlx::Result<DecisionRow> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM decisions WHERE id = $1"
    )))
    .bind(id)
    .fetch_one(pool)
    .await
}

/// Empty `project_id` lists every decision.
pub async fn list(
    pool: &PgPool,
    project_id: &str,
    include_superseded: bool,
) -> sqlx::Result<Vec<DecisionRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM decisions \
         WHERE ($1 = '' OR project_id = $1 OR scope <> 'project') AND ($2 OR status = 'active') \
         ORDER BY created_at DESC"
    )))
    .bind(project_id)
    .bind(include_superseded)
    .fetch_all(pool)
    .await
}

/// Decisions the ranker must honour for one project: its own plus every wider scope.
pub async fn active_for_project(pool: &PgPool, project_id: &str) -> sqlx::Result<Vec<DecisionRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM decisions \
         WHERE status = 'active' AND (project_id = $1 OR project_id = '' OR scope <> 'project') \
         ORDER BY created_at"
    )))
    .bind(project_id)
    .fetch_all(pool)
    .await
}

pub async fn supersede(pool: &PgPool, id: Uuid, superseded_by: &str) -> sqlx::Result<u64> {
    let result = sqlx::query(
        "UPDATE decisions SET status = 'superseded', superseded_at = now(), superseded_by = $2 \
         WHERE id = $1 AND status = 'active'",
    )
    .bind(id)
    .bind(superseded_by)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

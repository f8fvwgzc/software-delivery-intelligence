use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool, types::Json};
use uuid::Uuid;

use crate::domain::model::{ConstraintData, RankedOption, Weights};

#[derive(Debug, Clone, FromRow)]
pub struct ProposalRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub project_id: String,
    pub kind: String,
    pub status: String,
    pub title: String,
    pub summary: String,
    pub prompt: String,
    pub recommended_option_id: String,
    pub accepted_option_id: String,
    pub execution_id: String,
    pub options: Json<Vec<RankedOption>>,
    pub constraints: Json<Vec<ConstraintData>>,
    pub weights: Json<Weights>,
    pub revision: i32,
    pub reasoner: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ProposalRow {
    pub fn option(&self, id: &str) -> Option<&RankedOption> {
        self.options.0.iter().find(|o| o.id == id)
    }

    pub fn is_open(&self) -> bool {
        matches!(self.status.as_str(), "proposed" | "negotiating")
    }
}

const COLUMNS: &str = "id, workspace_id, project_id, kind, status, title, summary, prompt, recommended_option_id, \
    accepted_option_id, execution_id, options, constraints, weights, revision, reasoner, created_at, updated_at";

pub async fn insert(pool: &PgPool, row: &ProposalRow) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO proposals (id, workspace_id, project_id, kind, status, title, summary, prompt, recommended_option_id, \
         accepted_option_id, execution_id, options, constraints, weights, revision, reasoner, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)",
    )
    .bind(row.id)
    .bind(row.workspace_id)
    .bind(&row.project_id)
    .bind(&row.kind)
    .bind(&row.status)
    .bind(&row.title)
    .bind(&row.summary)
    .bind(&row.prompt)
    .bind(&row.recommended_option_id)
    .bind(&row.accepted_option_id)
    .bind(&row.execution_id)
    .bind(&row.options)
    .bind(&row.constraints)
    .bind(&row.weights)
    .bind(row.revision)
    .bind(&row.reasoner)
    .bind(row.created_at)
    .bind(row.updated_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get(pool: &PgPool, workspace_id: Uuid, id: Uuid) -> sqlx::Result<ProposalRow> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM proposals WHERE workspace_id = $1 AND id = $2"
    )))
    .bind(workspace_id)
    .bind(id)
    .fetch_one(pool)
    .await
}

/// Columns for list responses: the options come back without their
/// `artifacts` (generated file bodies can be megabytes per proposal), which
/// `RankedOption` tolerates through `#[serde(default)]`.
const LIST_COLUMNS: &str = "id, workspace_id, project_id, kind, status, title, summary, prompt, recommended_option_id, \
    accepted_option_id, execution_id, \
    COALESCE((SELECT jsonb_agg(o - 'artifacts' ORDER BY ord) FROM jsonb_array_elements(options) WITH ORDINALITY AS t(o, ord)), '[]'::jsonb) AS options, \
    constraints, weights, revision, reasoner, created_at, updated_at";

/// Empty `project_id` lists every project of the workspace. Artifacts are left out.
pub async fn list(
    pool: &PgPool,
    workspace_id: Uuid,
    project_id: &str,
) -> sqlx::Result<Vec<ProposalRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {LIST_COLUMNS} FROM proposals WHERE workspace_id = $1 AND ($2 = '' OR project_id = $2) \
         ORDER BY created_at DESC"
    )))
    .bind(workspace_id)
    .bind(project_id)
    .fetch_all(pool)
    .await
}

/// Writes the result of a (re-)ranking: options, constraints, weights and a
/// bumped revision.
pub async fn update_ranking(pool: &PgPool, row: &ProposalRow) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE proposals SET status = $3, summary = $4, recommended_option_id = $5, options = $6, \
         constraints = $7, weights = $8, revision = $9, reasoner = $10, updated_at = now() \
         WHERE workspace_id = $1 AND id = $2",
    )
    .bind(row.workspace_id)
    .bind(row.id)
    .bind(&row.status)
    .bind(&row.summary)
    .bind(&row.recommended_option_id)
    .bind(&row.options)
    .bind(&row.constraints)
    .bind(&row.weights)
    .bind(row.revision)
    .bind(&row.reasoner)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn accept(
    pool: &PgPool,
    workspace_id: Uuid,
    id: Uuid,
    option_id: &str,
    execution_id: &str,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE proposals SET status = 'accepted', accepted_option_id = $3, execution_id = $4, updated_at = now() \
         WHERE workspace_id = $1 AND id = $2",
    )
    .bind(workspace_id)
    .bind(id)
    .bind(option_id)
    .bind(execution_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn set_status(
    pool: &PgPool,
    workspace_id: Uuid,
    id: Uuid,
    status: &str,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE proposals SET status = $3, updated_at = now() WHERE workspace_id = $1 AND id = $2",
    )
    .bind(workspace_id)
    .bind(id)
    .bind(status)
    .execute(pool)
    .await?;
    Ok(())
}

/// Older open proposals of the same project and kind give way to a new one.
/// Returns their ids so the change can be broadcast.
pub async fn supersede_open(
    pool: &PgPool,
    workspace_id: Uuid,
    project_id: &str,
    kind: &str,
    keep: Uuid,
) -> sqlx::Result<Vec<Uuid>> {
    let rows: Vec<(Uuid,)> = sqlx::query_as(
        "UPDATE proposals SET status = 'superseded', updated_at = now() \
         WHERE workspace_id = $1 AND project_id = $2 AND kind = $3 AND id <> $4 \
           AND status IN ('proposed', 'negotiating') \
         RETURNING id",
    )
    .bind(workspace_id)
    .bind(project_id)
    .bind(kind)
    .bind(keep)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

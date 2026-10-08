use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool, types::Json};
use uuid::Uuid;

use crate::domain::model::EvidenceItem;

#[derive(Debug, Clone, FromRow)]
pub struct MessageRow {
    pub id: Uuid,
    pub proposal_id: Uuid,
    pub role: String,
    pub content: String,
    pub changed_recommendation: bool,
    pub references: Json<Vec<EvidenceItem>>,
    pub created_at: DateTime<Utc>,
}

impl MessageRow {
    pub fn new(
        proposal_id: Uuid,
        role: &str,
        content: String,
        changed: bool,
        references: Vec<EvidenceItem>,
    ) -> Self {
        Self {
            id: Uuid::now_v7(),
            proposal_id,
            role: role.to_owned(),
            content,
            changed_recommendation: changed,
            references: Json(references),
            created_at: Utc::now(),
        }
    }
}

const COLUMNS: &str =
    "id, proposal_id, role, content, changed_recommendation, \"references\", created_at";

pub async fn insert(pool: &PgPool, row: &MessageRow) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO messages (id, proposal_id, role, content, changed_recommendation, \"references\", created_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(row.id)
    .bind(row.proposal_id)
    .bind(&row.role)
    .bind(&row.content)
    .bind(row.changed_recommendation)
    .bind(&row.references)
    .bind(row.created_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list(pool: &PgPool, proposal_id: Uuid) -> sqlx::Result<Vec<MessageRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM messages WHERE proposal_id = $1 ORDER BY created_at, id"
    )))
    .bind(proposal_id)
    .fetch_all(pool)
    .await
}

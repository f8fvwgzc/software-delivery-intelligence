use chrono::{DateTime, Utc};
use sdi_analyzer::FactDraft;
use sqlx::{FromRow, PgConnection, PgExecutor};
use uuid::Uuid;

use crate::convert::{fact_category, fact_category_to_db, fact_kind, fact_kind_to_db};

#[derive(Debug, Clone, FromRow)]
pub struct FactRow {
    pub id: Uuid,
    pub project_id: Uuid,
    pub kind: String,
    pub category: String,
    pub key: String,
    pub label: String,
    pub value: String,
    pub confidence: f64,
    pub source_path: String,
    pub evidence: String,
    pub detected_at: DateTime<Utc>,
}

/// Facts are a snapshot: the new analysis replaces the old one wholesale.
pub async fn replace(
    conn: &mut PgConnection,
    project_id: Uuid,
    facts: &[FactDraft],
) -> sqlx::Result<usize> {
    sqlx::query("DELETE FROM facts WHERE project_id = $1")
        .bind(project_id)
        .execute(&mut *conn)
        .await?;
    let mut inserted = 0;
    for fact in facts {
        let result = sqlx::query(
            "INSERT INTO facts (id, project_id, kind, category, key, label, value, confidence, source_path, evidence)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
             ON CONFLICT (project_id, key) DO NOTHING",
        )
        .bind(Uuid::now_v7())
        .bind(project_id)
        .bind(fact_kind_to_db(fact_kind(fact.kind)))
        .bind(fact_category_to_db(fact_category(fact.category)))
        .bind(&fact.key)
        .bind(&fact.label)
        .bind(&fact.value)
        .bind(fact.confidence.clamp(0.0, 1.0))
        .bind(&fact.source_path)
        .bind(&fact.evidence)
        .execute(&mut *conn)
        .await?;
        inserted += result.rows_affected() as usize;
    }
    Ok(inserted)
}

pub async fn list(
    exec: impl PgExecutor<'_>,
    project_id: Uuid,
    kind: Option<&str>,
    category: Option<&str>,
) -> sqlx::Result<Vec<FactRow>> {
    sqlx::query_as::<_, FactRow>(
        "SELECT * FROM facts WHERE project_id = $1
           AND ($2::text IS NULL OR kind = $2)
           AND ($3::text IS NULL OR category = $3)
         ORDER BY category, kind, key",
    )
    .bind(project_id)
    .bind(kind)
    .bind(category)
    .fetch_all(exec)
    .await
}

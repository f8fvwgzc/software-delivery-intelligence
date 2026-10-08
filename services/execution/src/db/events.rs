use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgExecutor, types::Json};
use uuid::Uuid;

use super::Tx;

#[derive(Debug, Clone, FromRow)]
pub struct EventRow {
    pub execution_id: Uuid,
    pub seq: i64,
    pub r#type: String,
    pub step_key: String,
    pub message: String,
    pub status: String,
    pub data: Json<BTreeMap<String, String>>,
    pub at: DateTime<Utc>,
}

pub struct NewEvent<'a> {
    pub execution_id: Uuid,
    pub r#type: &'a str,
    pub step_key: &'a str,
    pub message: String,
    pub status: &'a str,
    pub data: BTreeMap<String, String>,
}

/// Appends with the next seq. Callers hold the execution row lock
/// (`executions::lock`) so the max(seq)+1 read cannot race.
pub async fn append(tx: &mut Tx, event: NewEvent<'_>) -> sqlx::Result<EventRow> {
    sqlx::query_as(
        "INSERT INTO events (execution_id, seq, type, step_key, message, status, data) \
         SELECT $1, COALESCE(MAX(seq), 0) + 1, $2, $3, $4, $5, $6 FROM events WHERE execution_id = $1 \
         RETURNING execution_id, seq, type, step_key, message, status, data, at",
    )
    .bind(event.execution_id)
    .bind(event.r#type)
    .bind(event.step_key)
    .bind(event.message)
    .bind(event.status)
    .bind(Json(event.data))
    .fetch_one(&mut **tx)
    .await
}

pub async fn list<'e>(
    exec: impl PgExecutor<'e>,
    execution_id: Uuid,
    after_seq: i64,
    limit: i64,
) -> sqlx::Result<Vec<EventRow>> {
    sqlx::query_as(
        "SELECT execution_id, seq, type, step_key, message, status, data, at FROM events \
         WHERE execution_id = $1 AND seq > $2 ORDER BY seq LIMIT $3",
    )
    .bind(execution_id)
    .bind(after_seq)
    .bind(limit)
    .fetch_all(exec)
    .await
}

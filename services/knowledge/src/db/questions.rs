use chrono::{DateTime, Utc};
use sdi_analyzer::QuestionDraft;
use sqlx::{FromRow, PgConnection, PgExecutor};
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
pub struct QuestionRow {
    pub id: Uuid,
    pub project_id: Uuid,
    pub key: String,
    pub prompt: String,
    pub reason: String,
    pub options: Vec<String>,
    pub allow_free_text: bool,
    pub answer: String,
    pub answered_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Questions are asked once and remembered: an existing key keeps its answer,
/// and only unanswered questions the scan no longer asks are dropped.
pub async fn sync(
    conn: &mut PgConnection,
    project_id: Uuid,
    questions: &[QuestionDraft],
) -> sqlx::Result<()> {
    let keys: Vec<&str> = questions.iter().map(|q| q.key.as_str()).collect();
    sqlx::query("DELETE FROM questions WHERE project_id = $1 AND answered_at IS NULL AND NOT (key = ANY($2))")
        .bind(project_id)
        .bind(&keys)
        .execute(&mut *conn)
        .await?;
    for question in questions {
        sqlx::query(
            "INSERT INTO questions (id, project_id, key, prompt, reason, options, allow_free_text)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (project_id, key) DO UPDATE
               SET prompt = EXCLUDED.prompt, reason = EXCLUDED.reason,
                   options = EXCLUDED.options, allow_free_text = EXCLUDED.allow_free_text",
        )
        .bind(Uuid::now_v7())
        .bind(project_id)
        .bind(&question.key)
        .bind(&question.prompt)
        .bind(&question.reason)
        .bind(&question.options)
        .bind(question.allow_free_text)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

pub async fn list(
    exec: impl PgExecutor<'_>,
    project_id: Uuid,
    only_open: bool,
) -> sqlx::Result<Vec<QuestionRow>> {
    sqlx::query_as::<_, QuestionRow>(
        "SELECT * FROM questions WHERE project_id = $1 AND (NOT $2 OR answered_at IS NULL) ORDER BY created_at, key",
    )
    .bind(project_id)
    .bind(only_open)
    .fetch_all(exec)
    .await
}

pub async fn get(exec: impl PgExecutor<'_>, id: Uuid) -> sqlx::Result<QuestionRow> {
    sqlx::query_as::<_, QuestionRow>("SELECT * FROM questions WHERE id = $1")
        .bind(id)
        .fetch_one(exec)
        .await
}

pub async fn answer(
    exec: impl PgExecutor<'_>,
    id: Uuid,
    answer: &str,
) -> sqlx::Result<QuestionRow> {
    sqlx::query_as::<_, QuestionRow>(
        "UPDATE questions SET answer = $2, answered_at = now() WHERE id = $1 RETURNING *",
    )
    .bind(id)
    .bind(answer)
    .fetch_one(exec)
    .await
}

use chrono::{DateTime, Utc};
use sdi_analyzer::QuestionDraft;
use sqlx::{FromRow, PgConnection, PgExecutor};
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
pub struct QuestionRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub project_id: Uuid,
    pub key: String,
    pub prompt: String,
    pub reason: String,
    pub options: Vec<String>,
    pub allow_free_text: bool,
    pub answer: String,
    pub answered_at: Option<DateTime<Utc>>,
    /// `user` | `config`; empty while open.
    pub answered_by: String,
    pub created_at: DateTime<Utc>,
}

/// Questions are asked once and remembered: an existing key keeps its answer,
/// and only unanswered questions the scan no longer asks are dropped.
///
/// A question `.sdi.yml` answers arrives pre-filled (`answered_by=config`)
/// and overrides whatever was stored, because the repository is the source
/// of truth for its own configuration. A question the config stopped
/// answering keeps a user's answer and reopens a config one.
pub async fn sync(
    conn: &mut PgConnection,
    workspace_id: Uuid,
    project_id: Uuid,
    questions: &[QuestionDraft],
) -> sqlx::Result<()> {
    let keys: Vec<&str> = questions.iter().map(|q| q.key.as_str()).collect();
    sqlx::query(
        "DELETE FROM questions WHERE workspace_id = $1 AND project_id = $2 AND answered_at IS NULL AND NOT (key = ANY($3))",
    )
    .bind(workspace_id)
    .bind(project_id)
    .bind(&keys)
    .execute(&mut *conn)
    .await?;
    for question in questions {
        let configured = question.answered_by == "config" && !question.answer.trim().is_empty();
        sqlx::query(
            "INSERT INTO questions (id, workspace_id, project_id, key, prompt, reason, options, allow_free_text, answer, answered_at, answered_by)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, CASE WHEN $10 THEN now() END, CASE WHEN $10 THEN 'config' ELSE '' END)
             ON CONFLICT (project_id, key) DO UPDATE
               SET prompt = EXCLUDED.prompt, reason = EXCLUDED.reason,
                   options = EXCLUDED.options, allow_free_text = EXCLUDED.allow_free_text,
                   answer = CASE WHEN $10 THEN EXCLUDED.answer
                                 WHEN questions.answered_by = 'config' THEN ''
                                 ELSE questions.answer END,
                   answered_at = CASE WHEN $10 THEN now()
                                      WHEN questions.answered_by = 'config' THEN NULL
                                      ELSE questions.answered_at END,
                   answered_by = CASE WHEN $10 THEN 'config'
                                      WHEN questions.answered_by = 'config' THEN ''
                                      ELSE questions.answered_by END",
        )
        .bind(Uuid::now_v7())
        .bind(workspace_id)
        .bind(project_id)
        .bind(&question.key)
        .bind(&question.prompt)
        .bind(&question.reason)
        .bind(&question.options)
        .bind(question.allow_free_text)
        .bind(if configured { question.answer.trim() } else { "" })
        .bind(configured)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

pub async fn list(
    exec: impl PgExecutor<'_>,
    workspace_id: Uuid,
    project_id: Uuid,
    only_open: bool,
) -> sqlx::Result<Vec<QuestionRow>> {
    sqlx::query_as::<_, QuestionRow>(
        "SELECT * FROM questions WHERE workspace_id = $1 AND project_id = $2 AND (NOT $3 OR answered_at IS NULL) ORDER BY created_at, key",
    )
    .bind(workspace_id)
    .bind(project_id)
    .bind(only_open)
    .fetch_all(exec)
    .await
}

pub async fn get(
    exec: impl PgExecutor<'_>,
    workspace_id: Uuid,
    id: Uuid,
) -> sqlx::Result<QuestionRow> {
    sqlx::query_as::<_, QuestionRow>("SELECT * FROM questions WHERE workspace_id = $1 AND id = $2")
        .bind(workspace_id)
        .bind(id)
        .fetch_one(exec)
        .await
}

pub async fn answer(
    exec: impl PgExecutor<'_>,
    workspace_id: Uuid,
    id: Uuid,
    answer: &str,
) -> sqlx::Result<QuestionRow> {
    sqlx::query_as::<_, QuestionRow>(
        "UPDATE questions SET answer = $3, answered_at = now(), answered_by = 'user' WHERE workspace_id = $1 AND id = $2 RETURNING *",
    )
    .bind(workspace_id)
    .bind(id)
    .bind(answer)
    .fetch_one(exec)
    .await
}

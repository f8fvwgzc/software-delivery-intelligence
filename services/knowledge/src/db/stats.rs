use sqlx::{FromRow, PgExecutor};

#[derive(Debug, Clone, FromRow)]
pub struct CountsRow {
    pub projects: i64,
    pub facts: i64,
    pub nodes: i64,
    pub edges: i64,
    pub patterns: i64,
    pub chunks: i64,
    pub open_questions: i64,
}

pub async fn counts(exec: impl PgExecutor<'_>) -> sqlx::Result<CountsRow> {
    sqlx::query_as::<_, CountsRow>(
        "SELECT (SELECT count(*) FROM projects) AS projects,
                (SELECT count(*) FROM facts) AS facts,
                (SELECT count(*) FROM graph_nodes) AS nodes,
                (SELECT count(*) FROM graph_edges WHERE valid_to IS NULL) AS edges,
                (SELECT count(*) FROM patterns WHERE status = 'active') AS patterns,
                (SELECT count(*) FROM chunks) AS chunks,
                (SELECT count(*) FROM questions WHERE answered_at IS NULL) AS open_questions",
    )
    .fetch_one(exec)
    .await
}

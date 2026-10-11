use sqlx::{FromRow, PgExecutor};
use uuid::Uuid;

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

/// Counts for one workspace. `patterns` is the catalog plus the
/// workspace's learned patterns, which is what `ListPatterns` returns.
pub async fn counts(exec: impl PgExecutor<'_>, workspace_id: Uuid) -> sqlx::Result<CountsRow> {
    sqlx::query_as::<_, CountsRow>(
        "SELECT (SELECT count(*) FROM projects WHERE workspace_id = $1) AS projects,
                (SELECT count(*) FROM facts WHERE workspace_id = $1) AS facts,
                (SELECT count(*) FROM graph_nodes WHERE workspace_id = $1) AS nodes,
                (SELECT count(*) FROM graph_edges WHERE workspace_id = $1 AND valid_to IS NULL) AS edges,
                (SELECT count(*) FROM patterns WHERE status = 'active' AND (workspace_id IS NULL OR workspace_id = $1)) AS patterns,
                (SELECT count(*) FROM chunks WHERE workspace_id = $1) AS chunks,
                (SELECT count(*) FROM questions WHERE workspace_id = $1 AND answered_at IS NULL) AS open_questions",
    )
    .bind(workspace_id)
    .fetch_one(exec)
    .await
}

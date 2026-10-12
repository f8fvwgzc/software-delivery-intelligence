//! Whole-workspace operations (CONTRACT.md §19): a real delete of everything
//! the schema holds for a tenant, and a complete export of it.

use std::collections::BTreeMap;

use futures::stream::BoxStream;
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

/// Every tenant table, children before parents so a delete can count each
/// one instead of relying on the cascade. `patterns` holds only the
/// workspace's learned rows (the catalog has no workspace).
pub const TENANT_TABLES: [&str; 23] = [
    "pull_request_files",
    "pull_requests",
    "repo_files",
    "repo_trees",
    "chunks",
    "project_profiles",
    "maturity",
    "project_config",
    "pattern_usage",
    "deployments",
    "pipeline_runs",
    "pipelines",
    "dependabot",
    "dependencies",
    "graph_edges",
    "graph_nodes",
    "understandings",
    "questions",
    "facts",
    "scans",
    "projects",
    "patterns",
    "workspace_settings",
];

/// Deletes every row of the workspace in every table and returns the count
/// per table. Runs on the caller's connection so it can be one transaction.
pub async fn delete_all(
    conn: &mut PgConnection,
    workspace_id: Uuid,
) -> sqlx::Result<BTreeMap<String, u64>> {
    let mut deleted = BTreeMap::new();
    for table in TENANT_TABLES {
        let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            "DELETE FROM {table} WHERE workspace_id = $1"
        )))
        .bind(workspace_id)
        .execute(&mut *conn)
        .await?
        .rows_affected();
        deleted.insert(table.to_owned(), rows);
    }
    Ok(deleted)
}

/// Streams the workspace's rows of `table` as JSON objects. Embeddings are
/// left out: they are derived from the content next to them, and a 256-float
/// literal per row would triple the export for nothing a reader can use.
pub fn export_rows<'p>(
    pool: &'p PgPool,
    workspace_id: Uuid,
    table: &str,
) -> BoxStream<'p, sqlx::Result<String>> {
    sqlx::query_scalar::<_, String>(sqlx::AssertSqlSafe(format!(
        "SELECT (to_jsonb(t) - 'embedding')::text FROM {table} t WHERE t.workspace_id = $1"
    )))
    .bind(workspace_id)
    .fetch(pool)
}

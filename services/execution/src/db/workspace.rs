//! Whole-workspace operations (CONTRACT.md §19): a complete export and a
//! real delete of the rows. Both walk the same table list so neither can
//! forget one. Cancelling live runs and removing their workdirs happens in
//! the runtime before `delete` is called.

use futures::StreamExt;
use sqlx::PgPool;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

/// `(entity name, table, order)`: parents before children so an import
/// could replay the stream in order.
pub const TABLES: [(&str, &str, &str); 7] = [
    ("policy", "policies", "action_type"),
    ("identity", "identities", "id"),
    ("grant", "grants", "identity_id, permission"),
    ("execution", "executions", "number"),
    ("step", "steps", "execution_id, position"),
    ("event", "events", "execution_id, seq"),
    ("approval", "approvals", "id"),
];

const EXPORT_BUFFER: usize = 64;

/// Every row of the workspace as `(entity, json)`, one table after another,
/// each row encoded by Postgres (`row_to_json`) and read through a cursor so
/// a large workspace never sits in memory at once. The stream ends after the
/// first error.
pub fn export(
    pool: PgPool,
    workspace_id: Uuid,
) -> ReceiverStream<sqlx::Result<(&'static str, String)>> {
    let (tx, rx) = mpsc::channel(EXPORT_BUFFER);
    sdi_common::tasks::spawn("workspace-export-rows", async move {
        for (entity, table, order) in TABLES {
            // Table and order come from `TABLES`, never from a caller.
            let sql = format!(
                "SELECT row_to_json(t)::text FROM (SELECT * FROM {table} WHERE workspace_id = $1 ORDER BY {order}) t"
            );
            let mut rows = sqlx::query_scalar::<_, String>(sqlx::AssertSqlSafe(sql))
                .bind(workspace_id)
                .fetch(&pool);
            while let Some(row) = rows.next().await {
                let failed = row.is_err();
                if tx.send(row.map(|json| (entity, json))).await.is_err() || failed {
                    return;
                }
            }
        }
    });
    ReceiverStream::new(rx)
}

/// Deletes every row of the workspace in one transaction, children first
/// so the counts are honest and no foreign key gets in the way. Returns
/// `(entity, rows removed)` in `TABLES` order.
pub async fn delete(pool: &PgPool, workspace_id: Uuid) -> sqlx::Result<Vec<(&'static str, u64)>> {
    let mut tx = pool.begin().await?;
    let mut removed = Vec::with_capacity(TABLES.len());
    for (entity, table, _) in TABLES.iter().rev() {
        let result = sqlx::query(sqlx::AssertSqlSafe(format!(
            "DELETE FROM {table} WHERE workspace_id = $1"
        )))
        .bind(workspace_id)
        .execute(&mut *tx)
        .await?;
        removed.push((*entity, result.rows_affected()));
    }
    tx.commit().await?;
    removed.reverse();
    Ok(removed)
}

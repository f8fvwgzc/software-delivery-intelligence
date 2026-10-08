//! Semantic memory over pgvector. Vectors travel as text literals and are
//! cast in SQL, so no custom sqlx type is needed.

use sdi_common::embed::to_pgvector;
use sqlx::{FromRow, PgConnection, PgExecutor};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct NewChunk {
    pub kind: String,
    pub project_id: Option<Uuid>,
    pub ref_id: String,
    pub title: String,
    pub path: String,
    pub content: String,
    pub embedding: Vec<f32>,
}

#[derive(Debug, Clone, FromRow)]
pub struct HitRow {
    pub id: Uuid,
    pub kind: String,
    pub project_id: Option<Uuid>,
    pub project_name: String,
    pub ref_id: String,
    pub title: String,
    pub path: String,
    pub snippet: String,
    pub score: f64,
}

pub async fn insert(exec: impl PgExecutor<'_>, chunk: &NewChunk) -> sqlx::Result<Uuid> {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO chunks (id, kind, project_id, ref_id, title, path, content, embedding)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8::vector)",
    )
    .bind(id)
    .bind(&chunk.kind)
    .bind(chunk.project_id)
    .bind(&chunk.ref_id)
    .bind(&chunk.title)
    .bind(&chunk.path)
    .bind(&chunk.content)
    .bind(to_pgvector(&chunk.embedding))
    .execute(exec)
    .await?;
    Ok(id)
}

/// Replaces the chunks a scan owns (`kinds`) and leaves the rest alone.
pub async fn replace_kinds(
    conn: &mut PgConnection,
    project_id: Uuid,
    kinds: &[String],
    chunks: &[NewChunk],
) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM chunks WHERE project_id = $1 AND kind = ANY($2)")
        .bind(project_id)
        .bind(kinds)
        .execute(&mut *conn)
        .await?;
    for chunk in chunks {
        insert(&mut *conn, chunk).await?;
    }
    Ok(())
}

pub async fn search(
    exec: impl PgExecutor<'_>,
    embedding: &[f32],
    project_id: Option<Uuid>,
    kinds: &[String],
    limit: i64,
) -> sqlx::Result<Vec<HitRow>> {
    sqlx::query_as::<_, HitRow>(
        "SELECT c.id, c.kind, c.project_id, COALESCE(p.name, '') AS project_name, c.ref_id, c.title, c.path,
                left(c.content, 400) AS snippet,
                (1 - (c.embedding <=> $1::vector))::float8 AS score
         FROM chunks c LEFT JOIN projects p ON p.id = c.project_id
         WHERE ($2::uuid IS NULL OR c.project_id = $2)
           AND (cardinality($3::text[]) = 0 OR c.kind = ANY($3))
         ORDER BY c.embedding <=> $1::vector
         LIMIT $4",
    )
    .bind(to_pgvector(embedding))
    .bind(project_id)
    .bind(kinds)
    .bind(limit)
    .fetch_all(exec)
    .await
}

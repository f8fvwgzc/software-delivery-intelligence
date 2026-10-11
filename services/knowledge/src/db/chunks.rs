//! Semantic memory over pgvector. Vectors travel as text literals and are
//! cast in SQL, so no custom sqlx type is needed. Every chunk belongs to a
//! workspace; `project_id` is NULL for cross-project knowledge within it.

use sdi_common::embed::to_pgvector;
use sqlx::{FromRow, PgConnection, PgExecutor};
use uuid::Uuid;

/// Longest chunk content stored when the workspace's data controls say
/// `minimal_chunks` (CONTRACT.md §19): an excerpt, never a whole file.
pub const MINIMAL_CHUNK_CHARS: usize = 400;

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

/// Cuts `content` to at most [`MINIMAL_CHUNK_CHARS`] characters on a char
/// boundary. Applied before embedding and storing so neither the vector nor
/// the row ever sees more than the excerpt.
pub fn minimal_excerpt(content: &str) -> &str {
    match content.char_indices().nth(MINIMAL_CHUNK_CHARS) {
        Some((cut, _)) => &content[..cut],
        None => content,
    }
}

pub async fn insert(
    exec: impl PgExecutor<'_>,
    workspace_id: Uuid,
    chunk: &NewChunk,
) -> sqlx::Result<Uuid> {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO chunks (id, workspace_id, kind, project_id, ref_id, title, path, content, embedding)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9::vector)",
    )
    .bind(id)
    .bind(workspace_id)
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
    workspace_id: Uuid,
    project_id: Uuid,
    kinds: &[String],
    chunks: &[NewChunk],
) -> sqlx::Result<()> {
    sqlx::query(
        "DELETE FROM chunks WHERE workspace_id = $1 AND project_id = $2 AND kind = ANY($3)",
    )
    .bind(workspace_id)
    .bind(project_id)
    .bind(kinds)
    .execute(&mut *conn)
    .await?;
    for chunk in chunks {
        insert(&mut *conn, workspace_id, chunk).await?;
    }
    Ok(())
}

pub async fn search(
    exec: impl PgExecutor<'_>,
    workspace_id: Uuid,
    embedding: &[f32],
    project_id: Option<Uuid>,
    kinds: &[String],
    limit: i64,
) -> sqlx::Result<Vec<HitRow>> {
    sqlx::query_as::<_, HitRow>(
        "SELECT c.id, c.kind, c.project_id, COALESCE(p.name, '') AS project_name, c.ref_id, c.title, c.path,
                left(c.content, 400) AS snippet,
                (1 - (c.embedding <=> $2::vector))::float8 AS score
         FROM chunks c LEFT JOIN projects p ON p.id = c.project_id AND p.workspace_id = c.workspace_id
         WHERE c.workspace_id = $1
           AND ($3::uuid IS NULL OR c.project_id = $3)
           AND (cardinality($4::text[]) = 0 OR c.kind = ANY($4))
         ORDER BY c.embedding <=> $2::vector
         LIMIT $5",
    )
    .bind(workspace_id)
    .bind(to_pgvector(embedding))
    .bind(project_id)
    .bind(kinds)
    .bind(limit)
    .fetch_all(exec)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_excerpt_cuts_on_char_boundaries() {
        assert_eq!(minimal_excerpt("short"), "short");
        let long: String = "é".repeat(MINIMAL_CHUNK_CHARS + 50);
        let cut = minimal_excerpt(&long);
        assert_eq!(cut.chars().count(), MINIMAL_CHUNK_CHARS);
        let exact: String = "a".repeat(MINIMAL_CHUNK_CHARS);
        assert_eq!(minimal_excerpt(&exact), exact);
    }
}

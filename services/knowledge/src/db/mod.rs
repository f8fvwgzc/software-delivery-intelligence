//! Repositories: `FromRow` structs plus plain async functions. Single-query
//! functions take any executor; multi-query ones take a connection so they
//! can run inside the caller's transaction.
//!
//! Tenancy (CONTRACT.md §18): every function takes the caller's
//! `workspace_id` and every query filters on it, so an id alone never
//! addresses a row. Project lookups are `(workspace_id, id)`; child rows
//! carry the workspace too and are filtered on it as well, which makes a
//! cross-workspace read impossible even if a project id leaks.

pub mod chunks;
pub mod code;
pub mod config;
pub mod delivery;
pub mod dependencies;
pub mod facts;
pub mod graph;
pub mod maturity;
pub mod patterns;
pub mod pipelines;
pub mod profiles;
pub mod projects;
pub mod pull_requests;
pub mod questions;
pub mod scans;
pub mod settings;
pub mod stats;
pub mod understanding;
pub mod workspace;

use uuid::Uuid;

/// Rows per multi-row `INSERT`. A scan of a 50k-file monorepo writes
/// thousands of facts, nodes, edges, dependencies and chunks; one statement
/// per row was one round trip per row. Columns travel as arrays through
/// `UNNEST`, so the statement text never grows with the batch.
pub const INSERT_BATCH: usize = 500;

/// The workspace every row created before tenancy belongs to (migration
/// `0004_tenancy`): the single-tenant installation that preceded
/// workspaces. Dev mode's personal workspace maps to it.
pub const BOOTSTRAP_WORKSPACE: Uuid = uuid::uuid!("00000000-0000-7000-8000-00000000dead");

/// Parses a pgvector text literal (`[0.1,0.2]`) back into floats.
pub fn parse_pgvector(literal: &str) -> Vec<f32> {
    literal
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .filter_map(|v| v.trim().parse().ok())
        .collect()
}

/// Applies the schema: makes sure pgvector lives where every schema can
/// see it, then runs the migrations. The service and its tests both come
/// through here.
///
/// `CREATE EXTENSION` puts an extension's types into the first schema of
/// the search path, and an extension exists once per database. On a fresh
/// database the first schema to run the migrations would therefore own
/// `vector`, and every other schema (another install sharing the database,
/// each test's own schema) would fail with `type "vector" does not exist`.
/// So the extension is created in `public`, which is on every pool's search
/// path, before the first migration asks for it, and one that an earlier
/// version left in another schema is moved there. The work is serialised by
/// an advisory lock because `IF NOT EXISTS` does not protect two sessions
/// that create the same extension at the same moment.
pub async fn migrate(pool: &sqlx::PgPool) -> anyhow::Result<()> {
    use anyhow::Context as _;

    let mut tx = pool.begin().await.context("could not open a transaction")?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('sdi.knowledge.pgvector', 0))")
        .execute(&mut *tx)
        .await
        .context("could not take the extension lock")?;
    let home: Option<String> = sqlx::query_scalar(
        "SELECT n.nspname FROM pg_extension e JOIN pg_namespace n ON n.oid = e.extnamespace \
         WHERE e.extname = 'vector'",
    )
    .fetch_optional(&mut *tx)
    .await
    .context("could not look for the pgvector extension")?;
    match home.as_deref() {
        Some("public") => {}
        Some(other) => {
            sqlx::query("ALTER EXTENSION vector SET SCHEMA public")
                .execute(&mut *tx)
                .await
                .with_context(|| {
                    format!("could not move pgvector from schema {other} to public")
                })?;
            tracing::info!(from = other, "pgvector moved to the public schema");
        }
        None => {
            sqlx::query("CREATE EXTENSION IF NOT EXISTS vector WITH SCHEMA public")
                .execute(&mut *tx)
                .await
                .context(
                    "could not create the pgvector extension: use a Postgres that ships it \
                     (the pgvector/pgvector image does) and a role that may create extensions",
                )?;
        }
    }
    tx.commit()
        .await
        .context("could not commit the extension setup")?;

    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .context("knowledge migrations failed")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pgvector_literal_round_trips() {
        let vector = vec![0.5_f32, -0.25, 1.0];
        let literal = sdi_common::embed::to_pgvector(&vector);
        assert_eq!(parse_pgvector(&literal), vector);
        assert!(parse_pgvector("").is_empty());
    }

    #[test]
    fn bootstrap_workspace_is_the_documented_id() {
        assert_eq!(
            BOOTSTRAP_WORKSPACE.to_string(),
            "00000000-0000-7000-8000-00000000dead"
        );
    }
}

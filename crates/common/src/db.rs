//! Postgres access. Services share one database server but each owns its
//! own schema, so no service can read another service's tables directly.

use std::{str::FromStr, time::Duration};

use anyhow::{Context, Result, ensure};
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};

/// Opens a pool whose `search_path` is `<schema>, public`.
///
/// The schema is created when missing. `public` stays on the path so
/// extension types such as pgvector's `vector` resolve.
pub async fn connect(database_url: &str, schema: &str) -> Result<PgPool> {
    ensure!(
        !schema.is_empty() && schema.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
        "schema name must be lowercase ascii: {schema}"
    );

    let base = PgConnectOptions::from_str(database_url).context("DATABASE_URL is not valid")?;

    let bootstrap = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(10))
        .connect_with(base.clone())
        .await
        .context("could not reach Postgres")?;
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA IF NOT EXISTS {schema}")))
        .execute(&bootstrap)
        .await
        .with_context(|| format!("could not create schema {schema}"))?;
    bootstrap.close().await;

    let options = base.options([("search_path", format!("{schema},public"))]);
    PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(10))
        .connect_with(options)
        .await
        .with_context(|| format!("could not open pool for schema {schema}"))
}

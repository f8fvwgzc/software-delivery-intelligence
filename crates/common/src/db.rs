//! Postgres access. Services share one database server but each owns its
//! own schema, so no service can read another service's tables directly.
//!
//! Pool limits come from the environment so a deployment can size them
//! against `max_connections` on the server (five services × the pool size
//! must fit): `DB_MAX_CONNECTIONS` (default 20), `DB_MIN_CONNECTIONS`
//! (default 0), `DB_ACQUIRE_TIMEOUT_SECS` (default 10), `DB_IDLE_TIMEOUT_SECS`
//! (default 600) and `DB_STATEMENT_TIMEOUT_MS` (default 30000, applied as
//! the session's `statement_timeout` so a runaway query can never pin a
//! connection for good).

use std::{str::FromStr, time::Duration};

use anyhow::{Context, Result, ensure};
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};

use crate::config;

pub const DEFAULT_MAX_CONNECTIONS: u32 = 20;
pub const DEFAULT_ACQUIRE_TIMEOUT_SECS: u64 = 10;
pub const DEFAULT_IDLE_TIMEOUT_SECS: u64 = 600;
pub const DEFAULT_STATEMENT_TIMEOUT_MS: u64 = 30_000;

/// Pool sizing, read from the environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolSettings {
    pub max_connections: u32,
    pub min_connections: u32,
    pub acquire_timeout: Duration,
    pub idle_timeout: Duration,
    /// `0` disables the session statement timeout.
    pub statement_timeout: Duration,
}

impl Default for PoolSettings {
    fn default() -> Self {
        Self {
            max_connections: DEFAULT_MAX_CONNECTIONS,
            min_connections: 0,
            acquire_timeout: Duration::from_secs(DEFAULT_ACQUIRE_TIMEOUT_SECS),
            idle_timeout: Duration::from_secs(DEFAULT_IDLE_TIMEOUT_SECS),
            statement_timeout: Duration::from_millis(DEFAULT_STATEMENT_TIMEOUT_MS),
        }
    }
}

impl PoolSettings {
    pub fn from_env() -> Result<Self> {
        let defaults = Self::default();
        let max_connections: u32 =
            config::env_parse("DB_MAX_CONNECTIONS", defaults.max_connections)?;
        ensure!(
            max_connections >= 1,
            "DB_MAX_CONNECTIONS must be at least 1"
        );
        let min_connections: u32 =
            config::env_parse("DB_MIN_CONNECTIONS", defaults.min_connections)?;
        Ok(Self {
            max_connections,
            min_connections: min_connections.min(max_connections),
            acquire_timeout: Duration::from_secs(config::env_parse(
                "DB_ACQUIRE_TIMEOUT_SECS",
                DEFAULT_ACQUIRE_TIMEOUT_SECS,
            )?),
            idle_timeout: Duration::from_secs(config::env_parse(
                "DB_IDLE_TIMEOUT_SECS",
                DEFAULT_IDLE_TIMEOUT_SECS,
            )?),
            statement_timeout: Duration::from_millis(config::env_parse(
                "DB_STATEMENT_TIMEOUT_MS",
                DEFAULT_STATEMENT_TIMEOUT_MS,
            )?),
        })
    }
}

/// Opens a pool whose `search_path` is `<schema>, public`, sized from the
/// environment ([`PoolSettings::from_env`]).
///
/// The schema is created when missing. `public` stays on the path so
/// extension types such as pgvector's `vector` resolve.
pub async fn connect(database_url: &str, schema: &str) -> Result<PgPool> {
    connect_with(database_url, schema, PoolSettings::from_env()?).await
}

pub async fn connect_with(
    database_url: &str,
    schema: &str,
    settings: PoolSettings,
) -> Result<PgPool> {
    ensure!(
        !schema.is_empty() && schema.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
        "schema name must be lowercase ascii: {schema}"
    );

    let base = PgConnectOptions::from_str(database_url).context("DATABASE_URL is not valid")?;

    let bootstrap = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(settings.acquire_timeout)
        .connect_with(base.clone())
        .await
        .context("could not reach Postgres")?;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "CREATE SCHEMA IF NOT EXISTS {schema}"
    )))
    .execute(&bootstrap)
    .await
    .with_context(|| format!("could not create schema {schema}"))?;
    bootstrap.close().await;

    let mut session = vec![("search_path", format!("{schema},public"))];
    if !settings.statement_timeout.is_zero() {
        session.push((
            "statement_timeout",
            settings.statement_timeout.as_millis().to_string(),
        ));
    }
    let options = base.options(session);
    PgPoolOptions::new()
        .max_connections(settings.max_connections)
        .min_connections(settings.min_connections)
        .acquire_timeout(settings.acquire_timeout)
        .idle_timeout(settings.idle_timeout)
        .connect_with(options)
        .await
        .with_context(|| format!("could not open pool for schema {schema}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_documented_values() {
        let settings = PoolSettings::default();
        assert_eq!(settings.max_connections, 20);
        assert_eq!(settings.statement_timeout, Duration::from_millis(30_000));
        assert_eq!(settings.acquire_timeout, Duration::from_secs(10));
    }
}

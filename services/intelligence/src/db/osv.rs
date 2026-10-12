//! Cache of OSV answers (CONTRACT.md §20), keyed by package coordinates and
//! shared across workspaces. Entries expire after `TTL`.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool, types::Json};

pub const TTL_HOURS: i64 = 24;

/// One known vulnerability, as the review cites it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Vulnerability {
    pub id: String,
    #[serde(default)]
    pub summary: String,
    /// "CRITICAL" | "HIGH" | "MODERATE" | "LOW" | "" as OSV/GHSA reports it.
    #[serde(default)]
    pub severity: String,
    /// First fixed version, when known.
    #[serde(default)]
    pub fixed: String,
}

#[derive(Debug, Clone, FromRow)]
pub struct CacheRow {
    pub vulns: Json<Vec<Vulnerability>>,
    pub fetched_at: DateTime<Utc>,
}

impl CacheRow {
    pub fn fresh(&self, now: DateTime<Utc>) -> bool {
        now - self.fetched_at < Duration::hours(TTL_HOURS)
    }
}

pub async fn get(
    pool: &PgPool,
    ecosystem: &str,
    name: &str,
    version: &str,
) -> sqlx::Result<Option<CacheRow>> {
    sqlx::query_as(
        "SELECT vulns, fetched_at FROM osv_cache WHERE ecosystem = $1 AND name = $2 AND version = $3",
    )
    .bind(ecosystem)
    .bind(name)
    .bind(version)
    .fetch_optional(pool)
    .await
}

pub async fn put(
    pool: &PgPool,
    ecosystem: &str,
    name: &str,
    version: &str,
    vulns: &[Vulnerability],
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO osv_cache (ecosystem, name, version, vulns, fetched_at) VALUES ($1, $2, $3, $4, now()) \
         ON CONFLICT (ecosystem, name, version) DO UPDATE SET vulns = EXCLUDED.vulns, fetched_at = now()",
    )
    .bind(ecosystem)
    .bind(name)
    .bind(version)
    .bind(Json(vulns))
    .execute(pool)
    .await?;
    Ok(())
}

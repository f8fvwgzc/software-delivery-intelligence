use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgExecutor};
use uuid::Uuid;

use super::Tx;
use crate::domain::policy::Grant;

#[derive(Debug, Clone, FromRow)]
pub struct IdentityRow {
    pub id: Uuid,
    pub name: String,
    pub kind: String,
    pub description: String,
}

#[derive(Debug, Clone, FromRow)]
pub struct GrantRow {
    pub identity_id: Uuid,
    pub permission: String,
    pub expires_at: Option<DateTime<Utc>>,
}

impl From<GrantRow> for Grant {
    fn from(row: GrantRow) -> Self {
        Grant {
            permission: row.permission,
            expires_at: row.expires_at,
        }
    }
}

pub async fn list<'e>(exec: impl PgExecutor<'e>) -> sqlx::Result<Vec<IdentityRow>> {
    sqlx::query_as("SELECT id, name, kind, description FROM identities ORDER BY name")
        .fetch_all(exec)
        .await
}

pub async fn get<'e>(exec: impl PgExecutor<'e>, id: Uuid) -> sqlx::Result<IdentityRow> {
    sqlx::query_as("SELECT id, name, kind, description FROM identities WHERE id = $1")
        .bind(id)
        .fetch_one(exec)
        .await
}

pub async fn by_name<'e>(exec: impl PgExecutor<'e>, name: &str) -> sqlx::Result<IdentityRow> {
    sqlx::query_as("SELECT id, name, kind, description FROM identities WHERE name = $1")
        .bind(name)
        .fetch_one(exec)
        .await
}

/// Live grants only; expired rows are dropped here so no caller has to
/// remember to filter them.
pub async fn grants_for<'e>(
    exec: impl PgExecutor<'e>,
    identity_id: Uuid,
) -> sqlx::Result<Vec<GrantRow>> {
    sqlx::query_as(
        "SELECT identity_id, permission, expires_at FROM grants \
         WHERE identity_id = $1 AND (expires_at IS NULL OR expires_at > now()) ORDER BY permission",
    )
    .bind(identity_id)
    .fetch_all(exec)
    .await
}

pub async fn all_grants<'e>(exec: impl PgExecutor<'e>) -> sqlx::Result<Vec<GrantRow>> {
    sqlx::query_as(
        "SELECT identity_id, permission, expires_at FROM grants \
         WHERE expires_at IS NULL OR expires_at > now() ORDER BY identity_id, permission",
    )
    .fetch_all(exec)
    .await
}

/// Timed grant. A base-scope grant is never shortened; an existing timed
/// grant is extended, never cut.
pub async fn grant(
    tx: &mut Tx,
    identity_id: Uuid,
    permissions: &[String],
    ttl_minutes: i64,
) -> sqlx::Result<()> {
    let expires_at = Utc::now() + chrono::Duration::minutes(ttl_minutes);
    for permission in permissions {
        sqlx::query(
            "INSERT INTO grants (identity_id, permission, expires_at) VALUES ($1, $2, $3) \
             ON CONFLICT (identity_id, permission) DO UPDATE SET expires_at = \
               CASE WHEN grants.expires_at IS NULL THEN NULL ELSE GREATEST(grants.expires_at, EXCLUDED.expires_at) END",
        )
        .bind(identity_id)
        .bind(permission)
        .bind(expires_at)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

/// Name and fixed id of the identity that stands for the connected token.
pub const GITHUB_TOKEN_IDENTITY: &str = "github-token";
const GITHUB_TOKEN_ID: Uuid = Uuid::from_u128(0x0000_0000_0000_7000_8000_0000_0000_0003);
const GITHUB_TOKEN_SCOPES: [&str; 2] = ["repository:read", "repository:write"];

/// Seeds (or withdraws) the `github-token` identity. It only exists while a
/// token is configured (CONTRACT.md §5.3), so this runs on every start:
/// with a token the identity and its base grants are upserted; without one
/// the base grants go and the identity itself is dropped unless an old
/// execution still references it.
pub async fn seed_github_token(pool: &sqlx::PgPool, configured: bool) -> sqlx::Result<()> {
    let mut tx = pool.begin().await?;
    if configured {
        sqlx::query(
            "INSERT INTO identities (id, name, kind, description) VALUES ($1, $2, 'github', $3) \
             ON CONFLICT (name) DO UPDATE SET description = EXCLUDED.description",
        )
        .bind(GITHUB_TOKEN_ID)
        .bind(GITHUB_TOKEN_IDENTITY)
        .bind("The connected GITHUB_TOKEN. Reads and writes the repositories the token can reach; present only while the token is configured.")
        .execute(&mut *tx)
        .await?;
        for permission in GITHUB_TOKEN_SCOPES {
            sqlx::query(
                "INSERT INTO grants (identity_id, permission, expires_at) \
                 SELECT id, $2, NULL FROM identities WHERE name = $1 \
                 ON CONFLICT (identity_id, permission) DO UPDATE SET expires_at = NULL",
            )
            .bind(GITHUB_TOKEN_IDENTITY)
            .bind(permission)
            .execute(&mut *tx)
            .await?;
        }
    } else {
        sqlx::query(
            "DELETE FROM grants WHERE expires_at IS NULL \
             AND identity_id IN (SELECT id FROM identities WHERE name = $1)",
        )
        .bind(GITHUB_TOKEN_IDENTITY)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "DELETE FROM identities i WHERE i.name = $1 \
             AND NOT EXISTS (SELECT 1 FROM executions e WHERE e.identity_id = i.id)",
        )
        .bind(GITHUB_TOKEN_IDENTITY)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await
}

/// An identity other than `except` that currently holds every permission.
pub async fn holding_all<'e>(
    exec: impl PgExecutor<'e>,
    permissions: &[String],
    except: Uuid,
) -> sqlx::Result<Option<IdentityRow>> {
    sqlx::query_as(
        "SELECT i.id, i.name, i.kind, i.description FROM identities i \
         WHERE i.id <> $2 AND NOT EXISTS ( \
           SELECT 1 FROM unnest($1::text[]) AS need(permission) \
           WHERE NOT EXISTS ( \
             SELECT 1 FROM grants g WHERE g.identity_id = i.id AND g.permission = need.permission \
               AND (g.expires_at IS NULL OR g.expires_at > now()))) \
         ORDER BY i.name LIMIT 1",
    )
    .bind(permissions)
    .bind(except)
    .fetch_optional(exec)
    .await
}

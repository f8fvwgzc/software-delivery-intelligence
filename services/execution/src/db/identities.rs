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

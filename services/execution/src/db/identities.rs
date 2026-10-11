//! Per-workspace identities and grants (CONTRACT.md §5.3). The three seeded
//! identities are created for a workspace on its first use (`seed`); the
//! `github-token` one only exists while `GITHUB_TOKEN` is configured.

use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgExecutor};
use uuid::Uuid;

use super::Tx;
use crate::domain::policy::Grant;

#[derive(Debug, Clone, FromRow)]
pub struct IdentityRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
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

const COLUMNS: &str = "id, workspace_id, name, kind, description";

pub async fn list<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
) -> sqlx::Result<Vec<IdentityRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM identities WHERE workspace_id = $1 ORDER BY name"
    )))
    .bind(workspace_id)
    .fetch_all(exec)
    .await
}

pub async fn get<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
    id: Uuid,
) -> sqlx::Result<IdentityRow> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM identities WHERE workspace_id = $1 AND id = $2"
    )))
    .bind(workspace_id)
    .bind(id)
    .fetch_one(exec)
    .await
}

pub async fn by_name<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
    name: &str,
) -> sqlx::Result<IdentityRow> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM identities WHERE workspace_id = $1 AND name = $2"
    )))
    .bind(workspace_id)
    .bind(name)
    .fetch_one(exec)
    .await
}

/// Live grants only; expired rows are dropped here so no caller has to
/// remember to filter them.
pub async fn grants_for<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
    identity_id: Uuid,
) -> sqlx::Result<Vec<GrantRow>> {
    sqlx::query_as(
        "SELECT identity_id, permission, expires_at FROM grants \
         WHERE workspace_id = $1 AND identity_id = $2 AND (expires_at IS NULL OR expires_at > now()) \
         ORDER BY permission",
    )
    .bind(workspace_id)
    .bind(identity_id)
    .fetch_all(exec)
    .await
}

pub async fn all_grants<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
) -> sqlx::Result<Vec<GrantRow>> {
    sqlx::query_as(
        "SELECT identity_id, permission, expires_at FROM grants \
         WHERE workspace_id = $1 AND (expires_at IS NULL OR expires_at > now()) \
         ORDER BY identity_id, permission",
    )
    .bind(workspace_id)
    .fetch_all(exec)
    .await
}

/// Timed grant. A base-scope grant is never shortened; an existing timed
/// grant is extended, never cut.
pub async fn grant(
    tx: &mut Tx,
    workspace_id: Uuid,
    identity_id: Uuid,
    permissions: &[String],
    ttl_minutes: i64,
) -> sqlx::Result<()> {
    let expires_at = Utc::now() + chrono::Duration::minutes(ttl_minutes);
    for permission in permissions {
        sqlx::query(
            "INSERT INTO grants (workspace_id, identity_id, permission, expires_at) VALUES ($1, $2, $3, $4) \
             ON CONFLICT (identity_id, permission) DO UPDATE SET expires_at = \
               CASE WHEN grants.expires_at IS NULL THEN NULL ELSE GREATEST(grants.expires_at, EXCLUDED.expires_at) END",
        )
        .bind(workspace_id)
        .bind(identity_id)
        .bind(permission)
        .bind(expires_at)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

/// The seeded identities and their base scopes (CONTRACT.md §5.3).
const SIMULATED: (&str, &str, &str, [&str; 3]) = (
    "simulated",
    "simulated",
    "Dry-run identity. Holds read and build scopes only; everything else needs a timed grant.",
    ["repository:read", "container:build", "environment:write"],
);
const LOCAL_DOCKER: (&str, &str, &str, [&str; 4]) = (
    "local-docker",
    "docker",
    "Runs steps on the host Docker daemon. Holds read, build, run and temporary-environment scopes; nothing that reaches a cloud or a repository write.",
    [
        "repository:read",
        "container:build",
        "container:run",
        "environment:write",
    ],
);

/// Name of the identity that stands for the connected token.
pub const GITHUB_TOKEN_IDENTITY: &str = "github-token";
const GITHUB_TOKEN_DESCRIPTION: &str = "The connected GITHUB_TOKEN. Reads and writes the repositories the token can reach; present only while the token is configured.";
const GITHUB_TOKEN_SCOPES: [&str; 2] = ["repository:read", "repository:write"];

/// Upserts one seeded identity with its base grants. Existing rows keep
/// their id and description; a base grant that exists stays untouched.
async fn upsert(
    tx: &mut Tx,
    workspace_id: Uuid,
    name: &str,
    kind: &str,
    description: &str,
    scopes: &[&str],
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO identities (id, workspace_id, name, kind, description) VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (workspace_id, name) DO NOTHING",
    )
    .bind(Uuid::now_v7())
    .bind(workspace_id)
    .bind(name)
    .bind(kind)
    .bind(description)
    .execute(&mut **tx)
    .await?;
    for permission in scopes {
        sqlx::query(
            "INSERT INTO grants (workspace_id, identity_id, permission, expires_at) \
             SELECT $1, id, $3, NULL FROM identities WHERE workspace_id = $1 AND name = $2 \
             ON CONFLICT (identity_id, permission) DO UPDATE SET expires_at = NULL",
        )
        .bind(workspace_id)
        .bind(name)
        .bind(permission)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

/// Creates the workspace's seeded identities when they are missing:
/// `simulated` and `local-docker` always, `github-token` only while a
/// token is configured. Idempotent.
pub async fn seed(tx: &mut Tx, workspace_id: Uuid, github_configured: bool) -> sqlx::Result<()> {
    let (name, kind, description, scopes) = SIMULATED;
    upsert(tx, workspace_id, name, kind, description, &scopes).await?;
    let (name, kind, description, scopes) = LOCAL_DOCKER;
    upsert(tx, workspace_id, name, kind, description, &scopes).await?;
    if github_configured {
        upsert(
            tx,
            workspace_id,
            GITHUB_TOKEN_IDENTITY,
            "github",
            GITHUB_TOKEN_DESCRIPTION,
            &GITHUB_TOKEN_SCOPES,
        )
        .await?;
    }
    Ok(())
}

/// Seeds (or withdraws) the `github-token` identity in every workspace that
/// has identities. It only exists while a token is configured (CONTRACT.md
/// §5.3), so this runs on every start: with a token the identity and its
/// base grants are upserted; without one the base grants go and the
/// identity itself is dropped unless an old execution still references it.
pub async fn seed_github_token(pool: &sqlx::PgPool, configured: bool) -> sqlx::Result<()> {
    let mut tx = pool.begin().await?;
    if configured {
        let workspaces: Vec<(Uuid,)> =
            sqlx::query_as("SELECT DISTINCT workspace_id FROM identities")
                .fetch_all(&mut *tx)
                .await?;
        for (workspace_id,) in workspaces {
            upsert(
                &mut tx,
                workspace_id,
                GITHUB_TOKEN_IDENTITY,
                "github",
                GITHUB_TOKEN_DESCRIPTION,
                &GITHUB_TOKEN_SCOPES,
            )
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

/// An identity of the workspace other than `except` that currently holds
/// every permission.
pub async fn holding_all<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
    permissions: &[String],
    except: Uuid,
) -> sqlx::Result<Option<IdentityRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM identities i \
         WHERE i.workspace_id = $1 AND i.id <> $3 AND NOT EXISTS ( \
           SELECT 1 FROM unnest($2::text[]) AS need(permission) \
           WHERE NOT EXISTS ( \
             SELECT 1 FROM grants g WHERE g.identity_id = i.id AND g.permission = need.permission \
               AND (g.expires_at IS NULL OR g.expires_at > now()))) \
         ORDER BY i.name LIMIT 1"
    )))
    .bind(workspace_id)
    .bind(permissions)
    .bind(except)
    .fetch_optional(exec)
    .await
}

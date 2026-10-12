//! Per-workspace policy table (CONTRACT.md §5.2). A workspace gets its own
//! copy of the seed the first time it is used (`seed`), so an admin's edits
//! never leak into another workspace and `editable = false` rows keep their
//! default everywhere.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgExecutor};
use uuid::Uuid;

use super::Tx;
use crate::{convert, domain::policy::PolicyRule};

#[derive(Debug, Clone, FromRow)]
pub struct PolicyRow {
    pub workspace_id: Uuid,
    pub action_type: String,
    pub label: String,
    pub description: String,
    pub risk: String,
    pub decision: String,
    pub editable: bool,
    pub updated_at: DateTime<Utc>,
}

/// `(action_type, label, risk, decision, editable)`: the §5.2 table. The
/// bootstrap workspace got the same rows from the migrations.
pub const SEED: [(&str, &str, &str, &str, bool); 27] = [
    ("repo.read", "Read repository", "SAFE", "AUTO", false),
    ("repo.analyze", "Analyze repository", "SAFE", "AUTO", false),
    ("tests.run_local", "Run tests locally", "SAFE", "AUTO", true),
    (
        "docs.generate",
        "Generate documentation",
        "SAFE",
        "AUTO",
        false,
    ),
    (
        "deps.install",
        "Install dependencies",
        "MEDIUM",
        "AUTO",
        true,
    ),
    (
        "container.build",
        "Build container image",
        "MEDIUM",
        "AUTO",
        true,
    ),
    ("container.run", "Run container", "MEDIUM", "APPROVAL", true),
    (
        "env.create_temporary",
        "Create temporary environment",
        "MEDIUM",
        "AUTO",
        true,
    ),
    (
        "db.create_temporary",
        "Create temporary database",
        "MEDIUM",
        "APPROVAL",
        true,
    ),
    (
        "cloud.create_dev_resource",
        "Create development cloud resource",
        "MEDIUM",
        "APPROVAL",
        true,
    ),
    (
        "cloud.modify_resource",
        "Modify existing cloud resource",
        "HIGH",
        "APPROVAL",
        true,
    ),
    (
        "deploy.staging",
        "Deploy to staging",
        "HIGH",
        "APPROVAL",
        true,
    ),
    (
        "deploy.production",
        "Deploy to production",
        "HIGH",
        "REQUIRED",
        false,
    ),
    (
        "db.migrate_production",
        "Run production database migration",
        "HIGH",
        "REQUIRED",
        false,
    ),
    (
        "backup.restore_production",
        "Restore production backup",
        "HIGH",
        "REQUIRED",
        false,
    ),
    (
        "resource.delete",
        "Delete resource",
        "HIGH",
        "REQUIRED",
        false,
    ),
    (
        "firewall.modify",
        "Modify firewall",
        "HIGH",
        "REQUIRED",
        false,
    ),
    (
        "secrets.rotate",
        "Rotate secrets",
        "HIGH",
        "REQUIRED",
        false,
    ),
    (
        "db.destroy_production",
        "Destroy production database",
        "BLOCKED",
        "BLOCKED",
        false,
    ),
    (
        "repo.checkout",
        "Download the repository at a commit",
        "SAFE",
        "AUTO",
        false,
    ),
    (
        "env.destroy_temporary",
        "Remove a temporary environment",
        "SAFE",
        "AUTO",
        true,
    ),
    (
        "repo.open_pull_request",
        "Open a pull request with generated files",
        "MEDIUM",
        "APPROVAL",
        true,
    ),
    // CONTRACT.md §22: the plan of a deployment run. `deploy.staging` and
    // `deploy.production` are the §5.2 rows above.
    (
        "registry.verify_image",
        "Verify the image exists in the registry",
        "SAFE",
        "AUTO",
        false,
    ),
    (
        "deploy.development",
        "Deploy to development",
        "MEDIUM",
        "AUTO",
        true,
    ),
    (
        "deploy.verify_health",
        "Verify the deployment is healthy",
        "SAFE",
        "AUTO",
        false,
    ),
    (
        "deploy.record",
        "Record the deployment on GitHub",
        "SAFE",
        "AUTO",
        true,
    ),
    (
        "deploy.rollback",
        "Roll back an environment",
        "HIGH",
        "APPROVAL",
        true,
    ),
];

const COLUMNS: &str =
    "workspace_id, action_type, label, description, risk, decision, editable, updated_at";

/// Copies the §5.2 defaults into the workspace; rows it already has stay as
/// they are, so this is safe to run on every first use.
pub async fn seed(tx: &mut Tx, workspace_id: Uuid) -> sqlx::Result<()> {
    for (action_type, label, risk, decision, editable) in SEED {
        sqlx::query(
            "INSERT INTO policies (workspace_id, action_type, label, risk, decision, editable) \
             VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (workspace_id, action_type) DO NOTHING",
        )
        .bind(workspace_id)
        .bind(action_type)
        .bind(label)
        .bind(risk)
        .bind(decision)
        .bind(editable)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

pub async fn list<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
) -> sqlx::Result<Vec<PolicyRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM policies WHERE workspace_id = $1 ORDER BY risk, action_type"
    )))
    .bind(workspace_id)
    .fetch_all(exec)
    .await
}

pub async fn get<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
    action_type: &str,
) -> sqlx::Result<PolicyRow> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM policies WHERE workspace_id = $1 AND action_type = $2"
    )))
    .bind(workspace_id)
    .bind(action_type)
    .fetch_one(exec)
    .await
}

/// The lookup table the domain uses to decorate steps.
pub async fn rules<'e>(
    exec: impl PgExecutor<'e>,
    workspace_id: Uuid,
) -> sqlx::Result<HashMap<String, PolicyRule>> {
    Ok(list(exec, workspace_id)
        .await?
        .into_iter()
        .map(|row| {
            (
                row.action_type,
                PolicyRule {
                    risk: convert::risk_from_db(&row.risk),
                    decision: convert::decision_from_db(&row.decision),
                },
            )
        })
        .collect())
}

pub async fn update(
    tx: &mut Tx,
    workspace_id: Uuid,
    action_type: &str,
    decision: &str,
) -> sqlx::Result<PolicyRow> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "UPDATE policies SET decision = $3, updated_at = now() WHERE workspace_id = $1 AND action_type = $2 \
         RETURNING {COLUMNS}"
    )))
    .bind(workspace_id)
    .bind(action_type)
    .bind(decision)
    .fetch_one(&mut **tx)
    .await
}

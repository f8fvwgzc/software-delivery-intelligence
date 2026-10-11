//! Pattern memory. Usage statistics live per project in `pattern_usage` and
//! are rolled up on read, so `success_rate` always reflects every project.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use sdi_common::embed::to_pgvector;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgExecutor, types::Json};
use uuid::Uuid;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlanStepRow {
    pub key: String,
    pub title: String,
    pub description: String,
    pub action_type: String,
    pub adapter: String,
    pub depends_on: Vec<String>,
    pub required_permissions: Vec<String>,
    #[serde(default)]
    pub params: BTreeMap<String, String>,
    pub verification: String,
}

#[derive(Debug, Clone, FromRow)]
pub struct PatternRow {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub category: String,
    pub technologies: Vec<String>,
    pub cloud: String,
    pub runtime: String,
    pub iac: String,
    pub delivery: String,
    pub requires: Vec<String>,
    pub scope: String,
    pub source: String,
    pub status: String,
    pub estimated_monthly_cost_usd: f64,
    pub security_score: f64,
    pub complexity: i32,
    pub pros: Vec<String>,
    pub cons: Vec<String>,
    pub plan: Json<Vec<PlanStepRow>>,
    /// Free-form, e.g. `{cost_lines: [{item, monthly_usd, note}], targets_axes: [..]}`.
    pub attributes: Json<serde_json::Value>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub used_by: Vec<String>,
    pub deployments: i32,
    pub successes: i32,
    pub incidents: i32,
}

/// Everything that defines a pattern; usage is tracked separately.
#[derive(Debug, Clone)]
pub struct PatternUpsert {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub category: String,
    pub technologies: Vec<String>,
    pub cloud: String,
    pub runtime: String,
    pub iac: String,
    pub delivery: String,
    pub requires: Vec<String>,
    pub scope: String,
    pub source: String,
    pub estimated_monthly_cost_usd: f64,
    pub security_score: f64,
    pub complexity: i32,
    pub pros: Vec<String>,
    pub cons: Vec<String>,
    pub plan: Vec<PlanStepRow>,
    /// See [`PatternRow::attributes`].
    pub attributes: serde_json::Value,
    pub embedding: Vec<f32>,
}

const SELECT: &str = "SELECT p.id, p.name, p.summary, p.category, p.technologies, p.cloud, p.runtime, p.iac, p.delivery,
       p.requires, p.scope, p.source, p.status, p.estimated_monthly_cost_usd, p.security_score, p.complexity,
       p.pros, p.cons, p.plan, p.attributes, p.last_used_at, p.created_at,
       COALESCE(u.used_by, '{}'::text[]) AS used_by,
       COALESCE(u.deployments, 0) AS deployments,
       COALESCE(u.successes, 0) AS successes,
       COALESCE(u.incidents, 0) AS incidents
FROM patterns p
LEFT JOIN LATERAL (
    SELECT array_agg(pr.name ORDER BY pr.name) AS used_by,
           sum(pu.deployments)::int AS deployments,
           sum(pu.successes)::int AS successes,
           sum(pu.incidents)::int AS incidents
    FROM pattern_usage pu JOIN projects pr ON pr.id = pu.project_id
    WHERE pu.pattern_id = p.id
) u ON true";

pub async fn upsert(exec: impl PgExecutor<'_>, pattern: &PatternUpsert) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO patterns (id, name, summary, category, technologies, cloud, runtime, iac, delivery, requires, scope, source,
            status, estimated_monthly_cost_usd, security_score, complexity, pros, cons, plan, embedding, attributes, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, 'active', $13, $14, $15, $16, $17, $18, $19::vector, $20, now(), now())
         ON CONFLICT (id) DO UPDATE SET
           name = EXCLUDED.name, summary = EXCLUDED.summary, category = EXCLUDED.category,
           technologies = EXCLUDED.technologies, cloud = EXCLUDED.cloud, runtime = EXCLUDED.runtime,
           iac = EXCLUDED.iac, delivery = EXCLUDED.delivery, requires = EXCLUDED.requires,
           scope = EXCLUDED.scope, source = EXCLUDED.source,
           estimated_monthly_cost_usd = EXCLUDED.estimated_monthly_cost_usd,
           security_score = EXCLUDED.security_score, complexity = EXCLUDED.complexity,
           pros = EXCLUDED.pros, cons = EXCLUDED.cons, plan = EXCLUDED.plan,
           embedding = EXCLUDED.embedding, attributes = EXCLUDED.attributes, updated_at = now()",
    )
    .bind(&pattern.id)
    .bind(&pattern.name)
    .bind(&pattern.summary)
    .bind(&pattern.category)
    .bind(&pattern.technologies)
    .bind(&pattern.cloud)
    .bind(&pattern.runtime)
    .bind(&pattern.iac)
    .bind(&pattern.delivery)
    .bind(&pattern.requires)
    .bind(&pattern.scope)
    .bind(&pattern.source)
    .bind(pattern.estimated_monthly_cost_usd)
    .bind(pattern.security_score)
    .bind(pattern.complexity)
    .bind(&pattern.pros)
    .bind(&pattern.cons)
    .bind(Json(&pattern.plan))
    .bind(to_pgvector(&pattern.embedding))
    .bind(Json(&pattern.attributes))
    .execute(exec)
    .await?;
    Ok(())
}

pub async fn list(
    exec: impl PgExecutor<'_>,
    category: Option<&str>,
    include_deprecated: bool,
) -> sqlx::Result<Vec<PatternRow>> {
    sqlx::query_as::<_, PatternRow>(sqlx::AssertSqlSafe(format!(
        "{SELECT} WHERE ($1::text IS NULL OR p.category = $1) AND ($2 OR p.status = 'active') ORDER BY p.source, p.category, p.name"
    )))
    .bind(category)
    .bind(include_deprecated)
    .fetch_all(exec)
    .await
}

pub async fn get(exec: impl PgExecutor<'_>, id: &str) -> sqlx::Result<PatternRow> {
    sqlx::query_as::<_, PatternRow>(sqlx::AssertSqlSafe(format!("{SELECT} WHERE p.id = $1")))
        .bind(id)
        .fetch_one(exec)
        .await
}

/// Records that `project_id` uses the pattern (counts untouched).
pub async fn add_used_by(
    exec: impl PgExecutor<'_>,
    pattern_id: &str,
    project_id: Uuid,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO pattern_usage (pattern_id, project_id) VALUES ($1, $2)
         ON CONFLICT (pattern_id, project_id) DO UPDATE SET last_used_at = now()",
    )
    .bind(pattern_id)
    .bind(project_id)
    .execute(exec)
    .await?;
    Ok(())
}

pub async fn record_outcome(
    exec: impl PgExecutor<'_>,
    pattern_id: &str,
    project_id: Uuid,
    succeeded: bool,
    incident: bool,
) -> sqlx::Result<()> {
    sqlx::query(
        "WITH usage AS (
            INSERT INTO pattern_usage (pattern_id, project_id, deployments, successes, incidents, last_used_at)
            VALUES ($1, $2, 1, $3, $4, now())
            ON CONFLICT (pattern_id, project_id) DO UPDATE SET
              deployments = pattern_usage.deployments + 1,
              successes = pattern_usage.successes + EXCLUDED.successes,
              incidents = pattern_usage.incidents + EXCLUDED.incidents,
              last_used_at = now()
         )
         UPDATE patterns SET last_used_at = now(), updated_at = now() WHERE id = $1",
    )
    .bind(pattern_id)
    .bind(project_id)
    .bind(i32::from(succeeded))
    .bind(i32::from(incident))
    .execute(exec)
    .await?;
    Ok(())
}

pub async fn exists(exec: impl PgExecutor<'_>, id: &str) -> sqlx::Result<bool> {
    sqlx::query_scalar::<_, bool>("SELECT EXISTS (SELECT 1 FROM patterns WHERE id = $1)")
        .bind(id)
        .fetch_one(exec)
        .await
}

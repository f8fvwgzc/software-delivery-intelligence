//! Delivery maturity report (CONTRACT.md §13), one row per project,
//! replaced by every successful scan.

use chrono::{DateTime, Utc};
use sdi_analyzer::{MaturityAxisDraft, MaturityDraft};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgExecutor, types::Json};
use uuid::Uuid;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Axis {
    pub key: String,
    pub label: String,
    pub score: u32,
    pub summary: String,
    pub findings: Vec<String>,
    pub recommended_patterns: Vec<String>,
}

#[derive(Debug, Clone, FromRow)]
pub struct MaturityRow {
    pub workspace_id: Uuid,
    pub project_id: Uuid,
    pub overall: i32,
    pub stage: String,
    pub axes: Json<Vec<Axis>>,
    pub computed_at: DateTime<Utc>,
}

fn axis(draft: &MaturityAxisDraft) -> Axis {
    Axis {
        key: draft.key.clone(),
        label: draft.label.clone(),
        score: draft.score.min(100),
        summary: draft.summary.clone(),
        findings: draft.findings.clone(),
        recommended_patterns: draft.recommended_patterns.clone(),
    }
}

/// The stage is derived here rather than trusted from the draft so the
/// stored row always satisfies the table's check constraint.
pub fn stage_for(overall: u32) -> &'static str {
    match overall {
        0..=34 => "legacy",
        35..=69 => "transitional",
        _ => "modern",
    }
}

pub async fn upsert(
    exec: impl PgExecutor<'_>,
    workspace_id: Uuid,
    project_id: Uuid,
    draft: &MaturityDraft,
) -> sqlx::Result<()> {
    let overall = draft.overall.min(100);
    let axes: Vec<Axis> = draft.axes.iter().map(axis).collect();
    sqlx::query(
        "INSERT INTO maturity (workspace_id, project_id, overall, stage, axes, computed_at)
         VALUES ($1, $2, $3, $4, $5, now())
         ON CONFLICT (project_id) DO UPDATE SET
           overall = EXCLUDED.overall, stage = EXCLUDED.stage, axes = EXCLUDED.axes, computed_at = now()",
    )
    .bind(workspace_id)
    .bind(project_id)
    .bind(overall as i32)
    .bind(stage_for(overall))
    .bind(Json(axes))
    .execute(exec)
    .await?;
    Ok(())
}

pub async fn get(
    exec: impl PgExecutor<'_>,
    workspace_id: Uuid,
    project_id: Uuid,
) -> sqlx::Result<Option<MaturityRow>> {
    sqlx::query_as::<_, MaturityRow>(
        "SELECT * FROM maturity WHERE workspace_id = $1 AND project_id = $2",
    )
    .bind(workspace_id)
    .bind(project_id)
    .fetch_optional(exec)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_thresholds_follow_the_contract() {
        assert_eq!(stage_for(0), "legacy");
        assert_eq!(stage_for(34), "legacy");
        assert_eq!(stage_for(35), "transitional");
        assert_eq!(stage_for(69), "transitional");
        assert_eq!(stage_for(70), "modern");
        assert_eq!(stage_for(100), "modern");
    }
}

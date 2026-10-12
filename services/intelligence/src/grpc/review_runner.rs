//! The review pipeline (CONTRACT.md §20) as a background task: context →
//! analyze → findings → summary. Every step is persisted on the review row
//! as soon as it changes and announced as a `review` PlatformEvent, so the
//! rail in the web app follows along; a failure marks the step and the
//! review `failed` and is recorded in the activity log.

use std::{sync::Arc, time::Instant};

use chrono::Utc;
use sdi_common::principal::Principal;
use sdi_proto::knowledge::v1::PullRequest;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    db::reviews::{self as repo, ActivityRow, FindingRow, ReviewRow, StepRecord},
    domain::review::{self, ReviewInput, rules::Finding},
    events::EventBus,
    gateway::KnowledgeGateway,
    reasoner::Reasoner,
};

/// Everything one run needs, owned, so the task outlives the RPC.
pub(super) struct ReviewRun {
    pub pool: PgPool,
    pub knowledge: Arc<dyn KnowledgeGateway>,
    pub events: EventBus,
    pub reasoner: Arc<dyn Reasoner>,
    pub principal: Principal,
    pub workspace: Uuid,
    pub review: ReviewRow,
    pub pull_request: PullRequest,
}

/// The rail at the start: every step pending.
pub(super) fn initial_steps() -> Vec<StepRecord> {
    review::STEPS
        .iter()
        .map(|(key, label)| StepRecord {
            key: (*key).to_owned(),
            label: (*label).to_owned(),
            status: "pending".to_owned(),
            detail: String::new(),
            duration_ms: 0,
        })
        .collect()
}

pub(super) fn set_step(
    steps: &mut [StepRecord],
    key: &str,
    status: &str,
    detail: &str,
    started: Option<Instant>,
) {
    if let Some(step) = steps.iter_mut().find(|s| s.key == key) {
        step.status = status.to_owned();
        step.detail = detail.to_owned();
        if let Some(started) = started {
            step.duration_ms = u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX);
        }
    }
}

pub(super) fn activity(
    review: &ReviewRow,
    action: &str,
    actor: &str,
    detail: String,
) -> ActivityRow {
    ActivityRow {
        id: Uuid::now_v7(),
        workspace_id: review.workspace_id,
        review_id: review.id,
        project_id: review.project_id.clone(),
        project_name: review.project_name.clone(),
        pr_number: review.pr_number,
        action: action.to_owned(),
        actor: actor.to_owned(),
        detail,
        at: Utc::now(),
    }
}

pub(super) fn emit(events: &EventBus, review: &ReviewRow, action: &str, status: &str, step: &str) {
    events.emit(
        review.workspace_id,
        "review",
        &review.id.to_string(),
        &review.project_id,
        action,
        status,
        &[
            ("step", step.to_owned()),
            ("pr_number", review.pr_number.to_string()),
            ("head_sha", review.head_sha.clone()),
        ],
    );
}

fn finding_row(
    workspace_id: Uuid,
    review_id: Uuid,
    position: usize,
    finding: &Finding,
) -> FindingRow {
    FindingRow {
        id: Uuid::now_v7(),
        workspace_id,
        review_id,
        position: position as i32,
        category: finding.category.to_owned(),
        severity: finding.severity.as_str().to_owned(),
        title: finding.title.clone(),
        detail: finding.detail.clone(),
        path: finding.path.clone(),
        line: finding.line as i32,
        end_line: finding.end_line.max(finding.line) as i32,
        suggestion: finding.suggestion.clone().unwrap_or_default(),
        rule: finding.rule.clone(),
        decision: "pending".to_owned(),
        decision_note: String::new(),
        decided_by: String::new(),
        decided_at: None,
    }
}

pub(super) fn spawn(run: ReviewRun) {
    tokio::spawn(async move {
        let review_id = run.review.id;
        if let Err(error) = execute(run).await {
            tracing::warn!(%error, review = %review_id, "review pipeline failed");
        }
    });
}

/// Runs the four steps; `Err` means the failure was recorded already.
async fn execute(run: ReviewRun) -> Result<(), String> {
    let ReviewRun {
        pool,
        knowledge,
        events,
        reasoner,
        principal,
        workspace,
        mut review,
        pull_request,
    } = run;
    let mut steps = review.steps.0.clone();

    // --- context -------------------------------------------------------
    let started = Instant::now();
    set_step(&mut steps, "context", "running", "", None);
    persist(&pool, &review, "context", &steps).await?;
    emit(&events, &review, "progress", "context", "context");
    let context = match knowledge
        .project_context(&principal, &review.project_id)
        .await
    {
        Ok(context) => context,
        Err(error) => {
            let message = format!("project context unavailable: {}", error.status.message());
            return fail(
                &pool,
                &events,
                &mut review,
                &mut steps,
                "context",
                &message,
                started,
            )
            .await;
        }
    };
    let input = ReviewInput::from_proto(&pull_request, &context);
    if !input.project_name.is_empty() {
        review.project_name = input.project_name.clone();
    }
    if !input.repo_full_name.is_empty() {
        review.repo_full_name = input.repo_full_name.clone();
    }
    repo::set_project(
        &pool,
        workspace,
        review.id,
        &review.project_name,
        &review.repo_full_name,
    )
    .await
    .map_err(|e| e.to_string())?;
    set_step(
        &mut steps,
        "context",
        "done",
        &format!(
            "{} files, {} touched facts, {} impacted nodes{}",
            input.files.len(),
            input.touched_facts.len(),
            input.impacted_nodes.len(),
            if input.files_truncated {
                " (file list cut at the limit)"
            } else {
                ""
            }
        ),
        Some(started),
    );

    // --- analyze -------------------------------------------------------
    let started = Instant::now();
    set_step(&mut steps, "analyze", "running", "", None);
    persist(&pool, &review, "analyzing", &steps).await?;
    emit(&events, &review, "progress", "analyzing", "analyze");
    let findings = review::analyze_all(&input);
    let rows: Vec<FindingRow> = findings
        .iter()
        .enumerate()
        .map(|(i, f)| finding_row(workspace, review.id, i, f))
        .collect();
    if let Err(error) = repo::insert_findings(&pool, &rows).await {
        return fail(
            &pool,
            &events,
            &mut review,
            &mut steps,
            "analyze",
            &format!("could not store findings: {error}"),
            started,
        )
        .await;
    }
    review.files_reviewed = input.files.len() as i32;
    review.findings_total = findings.len() as i32;
    repo::set_analysis(
        &pool,
        workspace,
        review.id,
        review.files_reviewed,
        review.findings_total,
    )
    .await
    .map_err(|e| e.to_string())?;
    let by_severity = {
        let mut counts = std::collections::BTreeMap::new();
        for f in &findings {
            *counts.entry(f.severity).or_insert(0usize) += 1;
        }
        counts
            .iter()
            .rev()
            .map(|(s, n)| format!("{n} {}", s.as_str()))
            .collect::<Vec<_>>()
            .join(", ")
    };
    set_step(
        &mut steps,
        "analyze",
        "done",
        &if findings.is_empty() {
            "no findings".to_owned()
        } else {
            format!("{} findings: {by_severity}", findings.len())
        },
        Some(started),
    );

    // --- findings ------------------------------------------------------
    set_step(
        &mut steps,
        "findings",
        "done",
        if findings.is_empty() {
            "nothing to decide"
        } else {
            "awaiting decisions"
        },
        None,
    );
    persist(&pool, &review, "analyzing", &steps).await?;
    emit(&events, &review, "progress", "analyzing", "findings");

    // --- summary -------------------------------------------------------
    let started = Instant::now();
    set_step(&mut steps, "summary", "running", "", None);
    persist(&pool, &review, "analyzing", &steps).await?;
    emit(&events, &review, "progress", "analyzing", "summary");
    let draft = review::summary::walkthrough(&input, &findings);
    let (summary, reasoner_name) = if reasoner.name() == "claude" {
        let context = review::summary::context_json(&input, &findings, &draft);
        let text = reasoner.write_review_summary(&draft, &context).await;
        let rewritten = text != draft;
        (text, if rewritten { "claude" } else { "heuristic" })
    } else {
        (draft, "heuristic")
    };
    repo::set_summary(&pool, workspace, review.id, &summary, reasoner_name)
        .await
        .map_err(|e| e.to_string())?;
    review.summary = summary;
    review.reasoner = reasoner_name.to_owned();
    set_step(
        &mut steps,
        "summary",
        "done",
        &format!(
            "{} words, {reasoner_name}",
            review::summary::word_count(&review.summary)
        ),
        Some(started),
    );
    persist(&pool, &review, "proposed", &steps).await?;
    review.status = "proposed".to_owned();
    repo::insert_activity(
        &pool,
        &activity(
            &review,
            "review.proposed",
            sdi_common::principal::SYSTEM_USER,
            format!(
                "{} findings over {} files ({reasoner_name})",
                review.findings_total, review.files_reviewed
            ),
        ),
    )
    .await
    .map_err(|e| e.to_string())?;
    emit(&events, &review, "updated", "proposed", "summary");
    Ok(())
}

async fn persist(
    pool: &PgPool,
    review: &ReviewRow,
    status: &str,
    steps: &[StepRecord],
) -> Result<(), String> {
    repo::set_steps(pool, review.workspace_id, review.id, status, steps)
        .await
        .map_err(|e| e.to_string())
}

async fn fail(
    pool: &PgPool,
    events: &EventBus,
    review: &mut ReviewRow,
    steps: &mut [StepRecord],
    step: &str,
    message: &str,
    started: Instant,
) -> Result<(), String> {
    set_step(steps, step, "failed", message, Some(started));
    repo::set_failed(pool, review.workspace_id, review.id, message, steps)
        .await
        .map_err(|e| e.to_string())?;
    review.status = "failed".to_owned();
    review.error = message.to_owned();
    let _ = repo::insert_activity(
        pool,
        &activity(
            review,
            "review.failed",
            sdi_common::principal::SYSTEM_USER,
            format!("{step}: {message}"),
        ),
    )
    .await;
    emit(events, review, "updated", "failed", step);
    Err(message.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_start_pending_and_update_in_place() {
        let mut steps = initial_steps();
        assert_eq!(steps.len(), 5);
        assert!(steps.iter().all(|s| s.status == "pending"));
        let started = Instant::now();
        set_step(&mut steps, "analyze", "done", "3 findings", Some(started));
        assert_eq!(steps[1].status, "done");
        assert_eq!(steps[1].detail, "3 findings");
        set_step(&mut steps, "nope", "done", "", None);
        assert_eq!(steps.iter().filter(|s| s.status == "done").count(), 1);
    }

    #[test]
    fn finding_rows_keep_the_evidence() {
        let finding = Finding::new(
            "secrets",
            "aws_key",
            review::rules::Severity::Critical,
            "a.rs",
            3,
            "t",
            "d",
        )
        .with_suggestion("x");
        let row = finding_row(Uuid::nil(), Uuid::nil(), 2, &finding);
        assert_eq!(row.position, 2);
        assert_eq!(row.severity, "critical");
        assert_eq!(row.rule, "secrets.aws_key");
        assert_eq!(row.end_line, 3);
        assert_eq!(row.suggestion, "x");
        assert_eq!(row.decision, "pending");
    }
}

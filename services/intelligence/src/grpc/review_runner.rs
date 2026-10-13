//! The review pipeline (CONTRACT.md §20) as a background task: context →
//! analyze → findings → summary. Every step is persisted on the review row
//! as soon as it changes and announced as a `review` PlatformEvent, so the
//! rail in the web app follows along; a failure marks the step and the
//! review `failed` and is recorded in the activity log.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use chrono::Utc;
use sdi_common::principal::Principal;
use sdi_proto::knowledge::v1::PullRequest;
use sqlx::PgPool;
use uuid::Uuid;

use super::fixes::Drafter;
use crate::{
    db::reviews::{
        self as repo, ActivityRow, FindingRow, ReviewRow, SOURCE_EXAMPLE, SOURCE_RULE, StepRecord,
    },
    domain::review::{
        self, ReviewInput,
        input::{ReviewConfig, RuleLearning},
        rules::{self, Finding},
    },
    events::EventBus,
    gateway::KnowledgeGateway,
    osv::OsvClient,
    reasoner::Reasoner,
};

/// The vulnerability check must never hold a review up for long.
const OSV_BUDGET: Duration = Duration::from_secs(20);

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
    pub osv: Option<Arc<OsvClient>>,
    /// Present when the review drafts fixes on its own (`auto_draft_fixes`
    /// with both data controls on and a model configured).
    pub drafter: Option<Drafter>,
}

/// The repository's `.sdi.yml` `review:` section at the scanned commit,
/// read through knowledge and parsed by the analyzer. Missing, unreadable
/// or invalid means "no review configuration", never a failed review.
async fn review_config(
    knowledge: &dyn KnowledgeGateway,
    principal: &Principal,
    project_id: &str,
) -> ReviewConfig {
    match knowledge
        .repo_file(principal, project_id, sdi_analyzer::CONFIG_PATH)
        .await
    {
        Ok(file) => {
            let draft = sdi_analyzer::parse_config(&file.content);
            if sdi_analyzer::config_is_usable(&draft) {
                ReviewConfig::from_draft(&draft.review)
            } else {
                tracing::debug!(project = project_id, errors = ?draft.errors, ".sdi.yml ignored by the review");
                ReviewConfig::default()
            }
        }
        Err(error) if error.status.code() == tonic::Code::NotFound => ReviewConfig::default(),
        Err(error) => {
            tracing::debug!(%error, project = project_id, ".sdi.yml unavailable to the review");
            ReviewConfig::default()
        }
    }
}

/// What the project taught so far (CONTRACT.md §20 learnings).
async fn learnings(pool: &PgPool, workspace: Uuid, project_id: &str) -> Vec<RuleLearning> {
    match crate::db::learnings::for_project(pool, workspace, project_id).await {
        Ok(rows) => rows
            .into_iter()
            .map(|r| RuleLearning {
                rule: r.rule,
                accepted: u32::try_from(r.accepted).unwrap_or(0),
                dismissed: u32::try_from(r.dismissed).unwrap_or(0),
            })
            .collect(),
        Err(error) => {
            tracing::warn!(%error, "review learnings unavailable; analysing without them");
            Vec::new()
        }
    }
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

/// Whether the lines of `path` can be replaced at the head commit, as far
/// as the pull request tells: not for a binary, removed or truncated file.
fn patchable(input: &ReviewInput, path: &str) -> bool {
    input
        .files
        .iter()
        .find(|f| f.path == path)
        .is_none_or(|f| !(f.is_removed() || f.is_binary() || f.truncated))
}

fn finding_row(
    workspace_id: Uuid,
    review_id: Uuid,
    position: usize,
    finding: &Finding,
    patchable: bool,
) -> FindingRow {
    let suggestion = finding.suggestion.clone().unwrap_or_default();
    FindingRow {
        suggestion_source: if suggestion.trim().is_empty() {
            String::new()
        } else if finding.suggestion_exact {
            SOURCE_RULE.to_owned()
        } else {
            SOURCE_EXAMPLE.to_owned()
        },
        fix_execution_id: String::new(),
        patchable,
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
        suggestion,
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
        osv,
        drafter,
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
    let today = Utc::now().date_naive();
    let input = ReviewInput::from_proto(&pull_request, &context)
        .with_config(review_config(knowledge.as_ref(), &principal, &review.project_id).await)
        .with_learnings(learnings(&pool, workspace, &review.project_id).await)
        .with_today((
            chrono::Datelike::year(&today),
            chrono::Datelike::month(&today),
            chrono::Datelike::day(&today),
        ));
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
            "{} files, {} touched facts, {} impacted nodes{}{}{}",
            input.files.len(),
            input.touched_facts.len(),
            input.impacted_nodes.len(),
            if input.files_truncated {
                " (file list cut at the limit)"
            } else {
                ""
            },
            if input.config.rules.is_empty() && input.config.ignore_paths.is_empty() {
                String::new()
            } else {
                format!(
                    ", .sdi.yml: {} custom rules, {} ignored globs",
                    input.config.rules.len(),
                    input.config.ignore_paths.len()
                )
            },
            if input.learnings.is_empty() {
                String::new()
            } else {
                format!(", {} learned rules", input.learnings.len())
            }
        ),
        Some(started),
    );

    // --- analyze -------------------------------------------------------
    let started = Instant::now();
    set_step(&mut steps, "analyze", "running", "", None);
    persist(&pool, &review, "analyzing", &steps).await?;
    emit(&events, &review, "progress", "analyzing", "analyze");
    let mut findings = rules::collect(&input);
    let deps = rules::dependencies::added_dependencies(&input);
    if let Some(osv) = osv.as_ref().filter(|_| !deps.is_empty()) {
        match tokio::time::timeout(OSV_BUDGET, osv.check(&deps)).await {
            Ok(answers) => findings.extend(rules::osv::findings(&deps, &answers)),
            Err(_) => tracing::warn!(review = %review.id, "osv check timed out; skipped"),
        }
    }
    let findings = rules::finalize(findings, &input);
    let rows: Vec<FindingRow> = findings
        .iter()
        .enumerate()
        .map(|(i, f)| finding_row(workspace, review.id, i, f, patchable(&input, &f.path)))
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
    // With `auto_draft_fixes`: the model drafts what the rules could not
    // fix mechanically, most severe first, a bounded number of times
    // (CONTRACT.md §23.2). It fails soft: the review goes on either way.
    let drafted = match &drafter {
        Some(drafter) if !rows.is_empty() => drafter.draft_for_review(&review, &rows).await,
        _ => 0,
    };
    set_step(
        &mut steps,
        "analyze",
        "done",
        &if findings.is_empty() {
            "no findings".to_owned()
        } else {
            let suppressed = findings.iter().filter(|f| f.suppressed).count();
            format!(
                "{} findings: {by_severity}{}{}",
                findings.len(),
                if suppressed > 0 {
                    format!(" ({suppressed} suppressed by project learnings)")
                } else {
                    String::new()
                },
                if drafted > 0 {
                    format!(
                        ", {drafted} fix{} drafted by the model",
                        if drafted == 1 { "" } else { "es" }
                    )
                } else {
                    String::new()
                }
            )
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
    let (summary, reasoner_name) = if reasoner.name() != "heuristic" {
        let context = review::summary::context_json(&input, &findings, &draft);
        let text = reasoner.write_review_summary(&draft, &context).await;
        let rewritten = text != draft;
        (
            text,
            if rewritten {
                reasoner.name()
            } else {
                "heuristic"
            },
        )
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
        .with_replacement("x");
        let row = finding_row(Uuid::nil(), Uuid::nil(), 2, &finding, true);
        assert_eq!(row.position, 2);
        assert_eq!(row.suggestion_source, "rule", "an exact replacement");
        // An illustration keeps its text and says what it is.
        let example = Finding::new(
            "docker",
            "latest_tag",
            review::rules::Severity::Medium,
            "Dockerfile",
            1,
            "t",
            "d",
        )
        .with_suggestion("FROM node:<version>");
        let row_example = finding_row(Uuid::nil(), Uuid::nil(), 1, &example, true);
        assert_eq!(row_example.suggestion_source, "example");
        assert_eq!(row_example.suggestion, "FROM node:<version>");
        assert!(row.patchable && row.fix_execution_id.is_empty());
        let plain = Finding::new(
            "correctness",
            "todo",
            review::rules::Severity::Low,
            "a.rs",
            3,
            "t",
            "d",
        );
        let row_plain = finding_row(Uuid::nil(), Uuid::nil(), 0, &plain, false);
        assert_eq!(row_plain.suggestion_source, "", "no suggestion, no source");
        assert!(!row_plain.patchable);
        assert_eq!(row.severity, "critical");
        assert_eq!(row.rule, "secrets.aws_key");
        assert_eq!(row.end_line, 3);
        assert_eq!(row.suggestion, "x");
        assert_eq!(row.decision, "pending");
    }
}

//! Code review RPCs (CONTRACT.md §20). `StartReview` reads the pull request
//! from knowledge (which fixes the head commit), records the review and
//! hands the pipeline to a background task; `DecideFinding` is the human's
//! verdict; `PublishReview` posts the summary and the accepted findings to
//! GitHub. Reviews are per `(project, pr_number, head_sha)`: starting one
//! that exists returns it, unless it failed (then it is retried).

use std::{sync::Arc, time::Instant};

use chrono::Utc;
use sdi_common::grpc::db_status;
use sdi_proto::intelligence::v1::{
    DecideFindingRequest, Finding, FindingDecision, GetReviewRequest, ListReviewActivityRequest,
    ListReviewActivityResponse, ListReviewsRequest, ListReviewsResponse, PublishReviewRequest,
    Review, StartReviewRequest,
};
use sqlx::types::Json;
use tonic::{Code, Status};
use uuid::Uuid;

use super::{
    Caller, IntelligenceGrpc, parse_id, require,
    review_runner::{self, ReviewRun, activity, emit, initial_steps, set_step},
};
use crate::{
    convert,
    db::reviews::{self as repo, FindingRow, ReviewRow},
    github::{ReviewComment, ReviewPost, review_event},
};

/// The footer every published review body ends with.
pub const FOOTER: &str = "Reviewed by SDI";

pub(super) async fn start(
    svc: &IntelligenceGrpc,
    caller: &Caller,
    request: StartReviewRequest,
) -> Result<Review, Status> {
    let project_id = require("project_id", &request.project_id)?;
    if request.pr_number == 0 {
        return Err(Status::invalid_argument("pr_number is required"));
    }
    let principal = &caller.principal;

    // Knowledge re-reads the PR: its head commit is the review's identity.
    let pull_request = svc
        .knowledge
        .pull_request(principal, &project_id, request.pr_number)
        .await
        .map_err(|e| e.into_status())?;
    let summary = pull_request.summary.clone().unwrap_or_default();
    if summary.head_sha.trim().is_empty() {
        return Err(Status::failed_precondition(
            "the pull request has no head commit",
        ));
    }
    let pr_number = request.pr_number as i64;

    if let Some(existing) = repo::find_by_head(
        &svc.pool,
        caller.workspace,
        &project_id,
        pr_number,
        &summary.head_sha,
    )
    .await
    .map_err(db_status)?
    {
        if existing.status != "failed" {
            let findings = repo::findings_of(&svc.pool, caller.workspace, existing.id)
                .await
                .map_err(db_status)?;
            return Ok(convert::review(&existing, &findings));
        }
        // A failed review is retried under a fresh id; its activity stays.
        repo::delete(&svc.pool, caller.workspace, existing.id)
            .await
            .map_err(db_status)?;
    }
    let previous = repo::latest_for_pr(&svc.pool, caller.workspace, &project_id, pr_number)
        .await
        .map_err(db_status)?
        .map(|r| r.id.to_string())
        .unwrap_or_default();

    let now = Utc::now();
    let row = ReviewRow {
        id: Uuid::now_v7(),
        workspace_id: caller.workspace,
        project_id: project_id.clone(),
        project_name: String::new(),
        repo_full_name: String::new(),
        pr_number,
        pr_title: summary.title.clone(),
        pr_author: summary.author.clone(),
        head_sha: summary.head_sha.clone(),
        base_ref: summary.base_ref.clone(),
        status: "context".to_owned(),
        steps: Json(initial_steps()),
        summary: String::new(),
        files_reviewed: pull_request.files.len() as i32,
        findings_total: 0,
        findings_accepted: 0,
        reasoner: "heuristic".to_owned(),
        published_url: String::new(),
        error: String::new(),
        requested_by: principal.user_id.clone(),
        previous_review_id: previous.clone(),
        created_at: now,
        updated_at: now,
    };
    repo::insert(&svc.pool, &row).await.map_err(db_status)?;
    repo::insert_activity(
        &svc.pool,
        &activity(
            &row,
            "review.started",
            &principal.user_id,
            format!(
                "{} @ {}{}",
                row.pr_title,
                &row.head_sha[..row.head_sha.len().min(12)],
                if previous.is_empty() {
                    String::new()
                } else {
                    format!(" (follows {previous})")
                }
            ),
        ),
    )
    .await
    .map_err(db_status)?;
    emit(&svc.events, &row, "created", "context", "context");

    // The reasoner is resolved now, with the caller's access, so the task
    // never has to ask identity on its own.
    let settings = svc.settings(caller).await?;
    let access = svc.reasoner_access(caller, &settings).await;
    let reasoner = svc.reasoner_for(caller, &settings, &access);
    review_runner::spawn(ReviewRun {
        pool: svc.pool.clone(),
        knowledge: Arc::clone(&svc.knowledge),
        events: svc.events.clone(),
        reasoner,
        principal: principal.clone(),
        workspace: caller.workspace,
        review: row.clone(),
        pull_request,
        osv: svc.osv.clone(),
    });
    Ok(convert::review(&row, &[]))
}

pub(super) async fn list(
    svc: &IntelligenceGrpc,
    caller: &Caller,
    request: ListReviewsRequest,
) -> Result<ListReviewsResponse, Status> {
    let rows = repo::list(
        &svc.pool,
        caller.workspace,
        request.project_id.trim(),
        super::page_limit(request.limit),
        None,
    )
    .await
    .map_err(db_status)?;
    Ok(ListReviewsResponse {
        reviews: rows.iter().map(|r| convert::review(r, &[])).collect(),
    })
}

pub(super) async fn get(
    svc: &IntelligenceGrpc,
    caller: &Caller,
    request: GetReviewRequest,
) -> Result<Review, Status> {
    let id = parse_id("id", &request.id)?;
    let row = repo::get(&svc.pool, caller.workspace, id)
        .await
        .map_err(db_status)?;
    let findings = repo::findings_of(&svc.pool, caller.workspace, id)
        .await
        .map_err(db_status)?;
    Ok(convert::review(&row, &findings))
}

pub(super) async fn decide_finding(
    svc: &IntelligenceGrpc,
    caller: &Caller,
    request: DecideFindingRequest,
) -> Result<Finding, Status> {
    let id = parse_id("finding_id", &request.finding_id)?;
    let decision = match request.decision() {
        FindingDecision::Accepted => "accepted",
        FindingDecision::Dismissed => "dismissed",
        _ => {
            return Err(Status::invalid_argument(
                "decision must be ACCEPTED or DISMISSED",
            ));
        }
    };
    let finding = repo::get_finding(&svc.pool, caller.workspace, id)
        .await
        .map_err(db_status)?;
    let review = repo::get(&svc.pool, caller.workspace, finding.review_id)
        .await
        .map_err(db_status)?;
    if review.status == "published" {
        return Err(Status::failed_precondition(
            "the review is already published; its findings are final",
        ));
    }
    let note = request.note.trim();
    let previous = finding.decision.clone();
    let row = repo::decide_finding(
        &svc.pool,
        caller.workspace,
        id,
        decision,
        note,
        &caller.principal.user_id,
    )
    .await
    .map_err(db_status)?;
    repo::refresh_accepted(&svc.pool, caller.workspace, review.id)
        .await
        .map_err(db_status)?;
    // The project remembers the verdict (CONTRACT.md §20 learnings); a
    // ledger that cannot be written must not block the decision.
    if !row.rule.is_empty()
        && let Err(error) = crate::db::learnings::record(
            &svc.pool,
            caller.workspace,
            &review.project_id,
            &row.rule,
            &previous,
            decision,
            note,
        )
        .await
    {
        tracing::warn!(%error, rule = %row.rule, "could not record the review learning");
    }
    repo::insert_activity(
        &svc.pool,
        &activity(
            &review,
            &format!("finding.{decision}"),
            &caller.principal.user_id,
            format!(
                "{} ({}{}){}",
                row.title,
                row.path,
                if row.line > 0 {
                    format!(":{}", row.line)
                } else {
                    String::new()
                },
                if note.is_empty() {
                    String::new()
                } else {
                    format!(" — {note}")
                }
            ),
        ),
    )
    .await
    .map_err(db_status)?;
    svc.events.emit(
        caller.workspace,
        "finding",
        &row.id.to_string(),
        &review.project_id,
        "decided",
        decision,
        &[
            ("review_id", review.id.to_string()),
            ("rule", row.rule.clone()),
        ],
    );
    emit(&svc.events, &review, "updated", &review.status, "findings");
    Ok(convert::finding(&row))
}

pub(super) async fn publish(
    svc: &IntelligenceGrpc,
    caller: &Caller,
    request: PublishReviewRequest,
) -> Result<Review, Status> {
    let id = parse_id("review_id", &request.review_id)?;
    let event = review_event(&request.event)?;
    let mut review = repo::get(&svc.pool, caller.workspace, id)
        .await
        .map_err(db_status)?;
    match review.status.as_str() {
        "proposed" => {}
        "published" => {
            return Err(Status::failed_precondition(format!(
                "the review is already published: {}",
                review.published_url
            )));
        }
        other => {
            return Err(Status::failed_precondition(format!(
                "the review is {other}; only a proposed review can be published"
            )));
        }
    }
    if review.repo_full_name.trim().is_empty() {
        return Err(Status::failed_precondition(
            "the review does not know its repository",
        ));
    }
    let findings = repo::findings_of(&svc.pool, caller.workspace, id)
        .await
        .map_err(db_status)?;
    let accepted: Vec<&FindingRow> = findings
        .iter()
        .filter(|f| f.decision == "accepted")
        .collect();
    let post = build_post(&review, &accepted, event);

    let access = svc
        .github_tokens
        .access(svc.identity.as_ref(), &caller.principal, caller.workspace)
        .await?;

    let started = Instant::now();
    let mut steps = review.steps.0.clone();
    set_step(&mut steps, "publish", "running", "", None);
    repo::set_steps(&svc.pool, caller.workspace, id, &review.status, &steps)
        .await
        .map_err(db_status)?;
    emit(&svc.events, &review, "progress", "proposed", "publish");

    match svc
        .publisher
        .post_review(
            &access,
            &review.repo_full_name,
            review.pr_number as u64,
            &post,
        )
        .await
    {
        Ok(posted) => {
            tracing::info!(review = %id, github_review = posted.id, url = %posted.html_url, "review published");
            set_step(
                &mut steps,
                "publish",
                "done",
                &format!(
                    "{} as {}, {} comment{}",
                    posted.html_url,
                    event.to_ascii_lowercase(),
                    accepted.len(),
                    if accepted.len() == 1 { "" } else { "s" }
                ),
                Some(started),
            );
            repo::set_published(&svc.pool, caller.workspace, id, &posted.html_url, &steps)
                .await
                .map_err(db_status)?;
            review.status = "published".to_owned();
            review.published_url = posted.html_url.clone();
            review.steps = Json(steps);
            repo::insert_activity(
                &svc.pool,
                &activity(
                    &review,
                    "review.published",
                    &caller.principal.user_id,
                    format!(
                        "{} ({}, {} accepted finding{})",
                        posted.html_url,
                        event.to_ascii_lowercase(),
                        accepted.len(),
                        if accepted.len() == 1 { "" } else { "s" }
                    ),
                ),
            )
            .await
            .map_err(db_status)?;
            emit(&svc.events, &review, "updated", "published", "publish");
            Ok(convert::review(&review, &findings))
        }
        Err(error) => {
            if error.code() == Code::Unauthenticated {
                svc.github_tokens.forget(caller.workspace);
            }
            // The review stays proposed so the publish can be retried.
            set_step(
                &mut steps,
                "publish",
                "failed",
                error.message(),
                Some(started),
            );
            repo::set_steps(&svc.pool, caller.workspace, id, &review.status, &steps)
                .await
                .map_err(db_status)?;
            let _ = repo::insert_activity(
                &svc.pool,
                &activity(
                    &review,
                    "review.failed",
                    &caller.principal.user_id,
                    format!("publish: {}", error.message()),
                ),
            )
            .await;
            emit(&svc.events, &review, "updated", "proposed", "publish");
            Err(error)
        }
    }
}

/// The GitHub review: the summary with the footer as the body, one inline
/// comment per accepted finding that points at a line (with a suggestion
/// block when the finding carries one), and file-level findings listed in
/// the body since GitHub cannot anchor them.
pub(crate) fn build_post(review: &ReviewRow, accepted: &[&FindingRow], event: &str) -> ReviewPost {
    let mut body = review.summary.trim_end().to_owned();
    let unanchored: Vec<&&FindingRow> = accepted.iter().filter(|f| f.line <= 0).collect();
    if !unanchored.is_empty() {
        body.push_str("\n\n## Also noted\n\n");
        for finding in unanchored {
            body.push_str(&format!(
                "- **{}** — `{}` ({}, {})\n",
                finding.title, finding.path, finding.rule, finding.severity
            ));
        }
    }
    body.push_str(&format!("\n\n---\n{FOOTER}"));
    let comments = accepted
        .iter()
        .filter(|f| f.line > 0)
        .map(|f| {
            let mut text = format!(
                "**{}** — `{}`, {}\n\n{}",
                f.title,
                f.rule,
                f.severity,
                f.detail.trim()
            );
            if !f.decision_note.trim().is_empty() {
                text.push_str(&format!("\n\n_Reviewer note: {}_", f.decision_note.trim()));
            }
            if !f.suggestion.trim().is_empty() {
                text.push_str(&format!(
                    "\n\n```suggestion\n{}\n```",
                    f.suggestion.trim_end()
                ));
            }
            let multi = f.end_line > f.line;
            ReviewComment {
                path: f.path.clone(),
                line: f.end_line.max(f.line) as u32,
                start_line: multi.then_some(f.line as u32),
                side: "RIGHT".to_owned(),
                start_side: multi.then(|| "RIGHT".to_owned()),
                body: text,
            }
        })
        .collect();
    ReviewPost {
        commit_id: review.head_sha.clone(),
        body,
        event: event.to_owned(),
        comments,
    }
}

pub(super) async fn list_activity(
    svc: &IntelligenceGrpc,
    caller: &Caller,
    request: ListReviewActivityRequest,
) -> Result<ListReviewActivityResponse, Status> {
    let rows = repo::list_activity(
        &svc.pool,
        caller.workspace,
        request.project_id.trim(),
        super::page_limit(request.limit),
        super::before(request.before.as_ref()),
    )
    .await
    .map_err(db_status)?;
    Ok(ListReviewActivityResponse {
        activity: rows.iter().map(convert::review_activity).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn review() -> ReviewRow {
        ReviewRow {
            id: Uuid::nil(),
            workspace_id: Uuid::nil(),
            project_id: "p".into(),
            project_name: "repo".into(),
            repo_full_name: "o/repo".into(),
            pr_number: 7,
            pr_title: "t".into(),
            pr_author: "a".into(),
            head_sha: "abc".into(),
            base_ref: "main".into(),
            status: "proposed".into(),
            steps: Json(initial_steps()),
            summary: "## Intent\n\nx\n".into(),
            files_reviewed: 1,
            findings_total: 3,
            findings_accepted: 2,
            reasoner: "heuristic".into(),
            published_url: String::new(),
            error: String::new(),
            requested_by: "u".into(),
            previous_review_id: String::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn finding(line: i32, end_line: i32, suggestion: &str) -> FindingRow {
        FindingRow {
            id: Uuid::nil(),
            workspace_id: Uuid::nil(),
            review_id: Uuid::nil(),
            position: 0,
            category: "docker".into(),
            severity: "medium".into(),
            title: "Base image is not pinned".into(),
            detail: "Line 1 uses `latest`.".into(),
            path: "Dockerfile".into(),
            line,
            end_line,
            suggestion: suggestion.into(),
            rule: "docker.latest_tag".into(),
            decision: "accepted".into(),
            decision_note: "please".into(),
            decided_by: "u".into(),
            decided_at: None,
        }
    }

    #[test]
    fn post_has_the_summary_footer_and_anchored_comments() {
        let anchored = finding(1, 1, "FROM node:22");
        let ranged = finding(4, 6, "");
        let file_level = finding(0, 0, "");
        let post = build_post(
            &review(),
            &[&anchored, &ranged, &file_level],
            "REQUEST_CHANGES",
        );
        assert_eq!(post.commit_id, "abc");
        assert_eq!(post.event, "REQUEST_CHANGES");
        assert!(post.body.starts_with("## Intent"));
        assert!(post.body.ends_with(&format!("---\n{FOOTER}")));
        assert!(post.body.contains("## Also noted"));
        assert_eq!(post.comments.len(), 2);
        let first = &post.comments[0];
        assert_eq!(
            (first.path.as_str(), first.line, first.side.as_str()),
            ("Dockerfile", 1, "RIGHT")
        );
        assert!(first.start_line.is_none());
        assert!(first.body.contains("docker.latest_tag"));
        assert!(first.body.contains("Reviewer note: please"));
        assert!(first.body.ends_with("```suggestion\nFROM node:22\n```"));
        let second = &post.comments[1];
        assert_eq!(second.line, 6);
        assert_eq!(second.start_line, Some(4));
        assert_eq!(second.start_side.as_deref(), Some("RIGHT"));
        assert!(!second.body.contains("```suggestion"));
    }

    #[test]
    fn post_without_accepted_findings_is_just_the_body() {
        let post = build_post(&review(), &[], "APPROVE");
        assert!(post.comments.is_empty());
        assert!(!post.body.contains("Also noted"));
        assert!(post.body.contains(FOOTER));
    }
}

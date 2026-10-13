//! `GetPullRequestStatus` (CONTRACT.md §23.3): whether one pull request can
//! be merged right now. Everything is read from GitHub when asked: the pull
//! request (state, mergeability, head), the checks and statuses of its head
//! commit, the reviews people left, what the repository allows, and how the
//! head relates to its base. Nothing is stored and nothing is cached beyond
//! ETag revalidation: a second look at an unchanged pull request costs
//! GitHub `304`s, and a changed one is never answered from memory.
//!
//! SDI's own gate (reviews, findings) is the intelligence service's to add;
//! this answer is GitHub's half.

use chrono::Utc;
use sdi_common::time::to_proto;
use sdi_proto::knowledge::v1::*;
use tonic::Status;

use super::{
    KnowledgeState, github_status,
    pull_requests::{new_pull_request, repo_parts},
};
use crate::{
    auth::Caller,
    convert,
    domain::merge::{
        self, Check, ReviewEvent, check_run_outcome, commit_status_outcome, fold_reviews,
        summarize_checks,
    },
    github::{CheckRun, CommitStatus, GithubError, MAX_STATUS_PAGES, Review},
};

pub(super) async fn get_pull_request_status(
    state: &KnowledgeState,
    caller: &Caller,
    req: GetPullRequestStatusRequest,
) -> Result<PullRequestStatus, Status> {
    let number = req.number;
    if number == 0 {
        return Err(Status::invalid_argument("number is required"));
    }
    let project = state.project(caller, &req.project_id).await?;
    let (owner, name) = repo_parts(&project)?;
    let repository = project.repo_full_name.as_str();
    let github = state.github(caller).await?;

    let pull = github
        .pull(owner, name, number)
        .await
        .map_err(|error| match error {
            GithubError::NotFound(_) => Status::not_found(format!(
                "pull request #{number} does not exist in {repository}"
            )),
            other => github_status(other),
        })?;
    let head_sha = pull
        .head
        .as_ref()
        .map(|head| head.sha.trim().to_owned())
        .unwrap_or_default();
    if head_sha.is_empty() {
        return Err(Status::unavailable(format!(
            "GitHub returned pull request #{number} of {repository} without a head commit; ask again"
        )));
    }
    let base_ref = pull
        .base
        .as_ref()
        .map(|base| base.reference.trim().to_owned())
        .unwrap_or_default();

    let (settings, runs, statuses, reviews, protected, behind) = tokio::join!(
        github.merge_settings(owner, name),
        github.check_runs(owner, name, &head_sha),
        github.commit_statuses(owner, name, &head_sha),
        github.pull_reviews(owner, name, number),
        async {
            match base_ref.as_str() {
                "" => Ok(false),
                base => github.branch_protected(owner, name, base).await,
            }
        },
        async {
            match base_ref.as_str() {
                "" => Ok(0),
                base => github.behind_by(owner, name, base, &head_sha).await,
            }
        },
    );
    // These four decide whether a merge is safe; a guess is not an answer.
    let part = |what: &str, error: GithubError| part_status(what, number, repository, error);
    let settings = settings.map_err(|e| part("the repository's merge settings", e))?;
    let runs = runs.map_err(|e| part("the check runs", e))?;
    let statuses = statuses.map_err(|e| part("the commit statuses", e))?;
    let reviews = reviews.map_err(|e| part("the reviews", e))?;
    // Nor is part of a list: the check that was not read may be the one
    // that failed, the review not read the one that asked for changes.
    for (what, more) in [
        ("check runs", runs.more),
        ("commit statuses", statuses.more),
        ("reviews", reviews.more),
    ] {
        if more {
            return Err(Status::failed_precondition(format!(
                "pull request #{number} of {repository} has more than {} {what}; merge readiness is not judged on a partial list",
                MAX_STATUS_PAGES * 100
            )));
        }
    }
    let (runs, statuses, reviews) = (runs.items, statuses.items, reviews.items);
    // These two only describe; without them the defaults stand (the
    // contract's choice), and GitHub still enforces both at merge time.
    let base_protected = protected.unwrap_or_else(|error| {
        tracing::debug!(%error, repository, base = %base_ref, "base branch protection not read");
        false
    });
    let behind_by = behind.unwrap_or_else(|error| {
        tracing::debug!(%error, repository, number, "distance behind the base not read");
        0
    });

    let totals = summarize_checks(checks(runs, statuses));
    let review_totals = fold_reviews(&review_events(&reviews));
    let mergeable_state = match pull.mergeable_state.trim() {
        "" => "unknown".to_owned(),
        word => word.to_owned(),
    };
    let summary = convert::pull_request_summary(
        new_pull_request(repository, &pull).unstored(caller.workspace_id, project.id),
    );
    Ok(PullRequestStatus {
        summary: Some(summary),
        mergeable: merge::mergeable_word(pull.mergeable).to_owned(),
        mergeable_state,
        checks: Some(CheckSummary {
            total: totals.total,
            passed: totals.passed,
            failed: totals.failed,
            pending: totals.pending,
            failed_names: totals.failed_names,
            pending_names: totals.pending_names,
        }),
        approvals: review_totals.approvals,
        changes_requested: review_totals.changes_requested,
        allowed_methods: merge::allowed_methods(
            settings.allow_squash_merge,
            settings.allow_merge_commit,
            settings.allow_rebase_merge,
        ),
        base_protected,
        behind_by,
        checked_at: Some(to_proto(Utc::now())),
    })
}

/// The status for a part of the answer GitHub would not give. Permission
/// problems map as they do for every pull request call. A `404` here is
/// not "no such pull request" (that one was just read): the head moved or
/// was collected between two calls, so the caller should simply ask again.
fn part_status(what: &str, number: u64, repository: &str, error: GithubError) -> Status {
    match error {
        GithubError::NotFound(_) => Status::unavailable(format!(
            "GitHub no longer has {what} of pull request #{number} in {repository} (its head moved); ask again"
        )),
        other => {
            let status = github_status(other);
            Status::new(
                status.code(),
                format!(
                    "could not read {what} of pull request #{number} in {repository}: {}",
                    status.message()
                ),
            )
        }
    }
}

/// Check runs and commit statuses of the head as one list. GitHub returns
/// the newest attempt of each run and the newest status of each context;
/// what still shares a name is folded by `summarize_checks`.
fn checks(runs: Vec<CheckRun>, statuses: Vec<CommitStatus>) -> impl Iterator<Item = Check> {
    let runs = runs.into_iter().map(|run| Check {
        outcome: check_run_outcome(&run.status, run.conclusion.as_deref()),
        name: run.name,
    });
    let statuses = statuses.into_iter().map(|status| Check {
        outcome: commit_status_outcome(&status.state),
        name: status.context,
    });
    runs.chain(statuses)
}

/// Reviews with a reviewer. One whose account was deleted cannot be told
/// apart from another such, so it has no verdict to fold.
fn review_events(reviews: &[Review]) -> Vec<ReviewEvent<'_>> {
    reviews
        .iter()
        .filter_map(|review| {
            let reviewer = review.user.as_ref()?.login.as_str();
            (!reviewer.is_empty()).then_some(ReviewEvent {
                reviewer,
                state: review.state.as_str(),
                at: review.submitted_at,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::merge::Outcome;

    #[test]
    fn runs_and_statuses_become_one_list_of_checks() {
        let runs: Vec<CheckRun> = serde_json::from_value(serde_json::json!([
            {"id": 10, "name": "build", "status": "completed", "conclusion": "success",
             "started_at": "2026-10-13T09:00:00Z", "completed_at": "2026-10-13T09:05:00Z"},
            {"id": 11, "name": "test", "status": "in_progress", "conclusion": null,
             "started_at": "2026-10-13T09:01:00Z", "completed_at": null},
        ]))
        .unwrap();
        let statuses: Vec<CommitStatus> = serde_json::from_value(serde_json::json!([
            {"context": "ci/external", "state": "failure", "updated_at": "2026-10-13T09:02:00Z"},
            {"context": "license/cla", "state": "success", "created_at": "2026-10-13T09:00:30Z"},
        ]))
        .unwrap();
        let list: Vec<Check> = checks(runs, statuses).collect();
        assert_eq!(list.len(), 4);
        assert_eq!(
            (list[0].name.as_str(), list[0].outcome),
            ("build", Outcome::Passed)
        );
        assert_eq!(
            (list[1].name.as_str(), list[1].outcome),
            ("test", Outcome::Pending)
        );
        assert_eq!(
            (list[2].name.as_str(), list[2].outcome),
            ("ci/external", Outcome::Failed)
        );
        assert_eq!(
            (list[3].name.as_str(), list[3].outcome),
            ("license/cla", Outcome::Passed)
        );
    }

    #[test]
    fn reviews_without_a_reviewer_have_no_verdict() {
        let reviews: Vec<Review> = serde_json::from_value(serde_json::json!([
            {"user": {"login": "ana"}, "state": "APPROVED", "submitted_at": "2026-10-13T09:00:00Z"},
            {"user": null, "state": "CHANGES_REQUESTED", "submitted_at": "2026-10-13T09:01:00Z"},
        ]))
        .unwrap();
        let events = review_events(&reviews);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].reviewer, "ana");
        let totals = fold_reviews(&events);
        assert_eq!((totals.approvals, totals.changes_requested), (1, 0));
    }

    #[test]
    fn a_missing_part_is_not_a_missing_pull_request() {
        let gone = part_status(
            "the check runs",
            7,
            "acme/api",
            GithubError::NotFound("/x".into()),
        );
        assert_eq!(gone.code(), tonic::Code::Unavailable);
        assert!(gone.message().contains("ask again"), "{}", gone.message());

        let forbidden = part_status(
            "the reviews",
            7,
            "acme/api",
            GithubError::Forbidden("Resource not accessible by integration".into()),
        );
        assert_eq!(forbidden.code(), tonic::Code::PermissionDenied);
        assert!(
            forbidden
                .message()
                .contains("the reviews of pull request #7"),
            "{}",
            forbidden.message()
        );
        assert!(
            forbidden
                .message()
                .contains("Resource not accessible by integration")
        );
    }
}

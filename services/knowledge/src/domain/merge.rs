//! Merge readiness, the part that is arithmetic (CONTRACT.md §23.3): how
//! GitHub's check runs, commit statuses and reviews fold into the counts
//! `PullRequestStatus` reports. The handler reads GitHub; nothing here does.

use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, Utc};

/// Names listed per outcome in a [`CheckTotals`].
pub const MAX_NAMES: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Passed,
    Failed,
    Pending,
}

/// What a check run counts as. A run that has not finished is pending
/// whatever else it says; a finished one passes only on `success`,
/// `neutral` or `skipped`. A conclusion this code has never heard of is a
/// failure: an unknown word must not read as "safe to merge".
pub fn check_run_outcome(status: &str, conclusion: Option<&str>) -> Outcome {
    if matches!(
        status,
        "queued" | "in_progress" | "waiting" | "requested" | "pending"
    ) {
        return Outcome::Pending;
    }
    match conclusion {
        Some("success" | "neutral" | "skipped") => Outcome::Passed,
        // failure, timed_out, cancelled, action_required, startup_failure,
        // stale, and whatever GitHub adds next.
        Some(_) => Outcome::Failed,
        // Finished without a verdict is not a pass; anything else without
        // one has not finished.
        None if status == "completed" => Outcome::Failed,
        None => Outcome::Pending,
    }
}

/// What a commit status counts as. An unknown state is pending: it blocks
/// a merge and says "ask again", which is the honest reading of a word
/// nobody here understands.
pub fn commit_status_outcome(state: &str) -> Outcome {
    match state {
        "success" => Outcome::Passed,
        "failure" | "error" => Outcome::Failed,
        _ => Outcome::Pending,
    }
}

impl Outcome {
    /// How much an outcome stands in the way of a merge.
    fn weight(self) -> u8 {
        match self {
            Outcome::Passed => 0,
            Outcome::Pending => 1,
            Outcome::Failed => 2,
        }
    }
}

/// One check run or commit status of the head commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    /// The check run's name or the status's context.
    pub name: String,
    pub outcome: Outcome,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CheckTotals {
    pub total: u32,
    pub passed: u32,
    pub failed: u32,
    pub pending: u32,
    /// Sorted by name, at most [`MAX_NAMES`].
    pub failed_names: Vec<String>,
    /// Sorted by name, at most [`MAX_NAMES`].
    pub pending_names: Vec<String>,
}

/// Counts check runs and commit statuses together, one entry per name,
/// and the worst outcome reported under a name is the one that counts:
/// failed over pending over passed. GitHub already hands over only the
/// newest attempt of a check run and the newest status of a context, so
/// what shares a name here is two different things (two apps, two
/// workflows, a run and a status), and one of them failing must not hide
/// behind the other. The execution service's `repo.verify_mergeable` folds
/// the same way, so readiness and the merge step never disagree.
pub fn summarize_checks(checks: impl IntoIterator<Item = Check>) -> CheckTotals {
    let mut worst: BTreeMap<String, Check> = BTreeMap::new();
    for check in checks {
        match worst.get(&check.name) {
            Some(held) if held.outcome.weight() >= check.outcome.weight() => {}
            _ => {
                worst.insert(check.name.clone(), check);
            }
        }
    }
    let mut totals = CheckTotals::default();
    for (name, check) in worst {
        totals.total += 1;
        let name = if name.is_empty() {
            "(unnamed check)".to_owned()
        } else {
            name
        };
        match check.outcome {
            Outcome::Passed => totals.passed += 1,
            Outcome::Failed => {
                totals.failed += 1;
                if totals.failed_names.len() < MAX_NAMES {
                    totals.failed_names.push(name);
                }
            }
            Outcome::Pending => {
                totals.pending += 1;
                if totals.pending_names.len() < MAX_NAMES {
                    totals.pending_names.push(name);
                }
            }
        }
    }
    totals
}

/// One review as GitHub lists it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReviewEvent<'a> {
    pub reviewer: &'a str,
    /// "APPROVED" | "CHANGES_REQUESTED" | "COMMENTED" | "DISMISSED" | "PENDING"
    pub state: &'a str,
    pub at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReviewTotals {
    pub approvals: u32,
    pub changes_requested: u32,
}

/// The standing verdict of every reviewer: their latest approval or change
/// request. A comment (or a review still being written) changes nothing;
/// a dismissed review takes that reviewer's verdict away. Logins compare
/// without case, as GitHub's do.
pub fn fold_reviews(reviews: &[ReviewEvent<'_>]) -> ReviewTotals {
    // GitHub lists reviews oldest first; the sort only matters if it ever
    // does not, and is stable so reviews without a time keep their place.
    let mut ordered: Vec<&ReviewEvent<'_>> = reviews.iter().collect();
    ordered.sort_by_key(|review| review.at);
    let mut verdicts: HashMap<String, bool> = HashMap::new();
    for review in ordered {
        let reviewer = review.reviewer.to_ascii_lowercase();
        match review.state.to_ascii_uppercase().as_str() {
            "APPROVED" => {
                verdicts.insert(reviewer, true);
            }
            "CHANGES_REQUESTED" => {
                verdicts.insert(reviewer, false);
            }
            "DISMISSED" => {
                verdicts.remove(&reviewer);
            }
            _ => {}
        }
    }
    let approvals = verdicts.values().filter(|approved| **approved).count() as u32;
    ReviewTotals {
        approvals,
        changes_requested: verdicts.len() as u32 - approvals,
    }
}

/// GitHub's tri-state `mergeable` in the contract's words.
pub fn mergeable_word(mergeable: Option<bool>) -> &'static str {
    match mergeable {
        Some(true) => "mergeable",
        Some(false) => "conflicting",
        None => "unknown",
    }
}

/// The merge methods a repository allows, in the order a default is picked
/// from (squash, merge, rebase). GitHub only tells a token with push access;
/// when it told nothing at all, every method is offered and GitHub refuses
/// the wrong one at merge time.
pub fn allowed_methods(
    squash: Option<bool>,
    merge_commit: Option<bool>,
    rebase: Option<bool>,
) -> Vec<String> {
    let untold = squash.is_none() && merge_commit.is_none() && rebase.is_none();
    [
        ("squash", squash),
        ("merge", merge_commit),
        ("rebase", rebase),
    ]
    .into_iter()
    .filter(|(_, allowed)| allowed.unwrap_or(untold))
    .map(|(method, _)| method.to_owned())
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(minute: u32) -> Option<DateTime<Utc>> {
        Some(Utc.with_ymd_and_hms(2026, 10, 13, 9, minute, 0).unwrap())
    }

    fn check(name: &str, outcome: Outcome) -> Check {
        Check {
            name: name.to_owned(),
            outcome,
        }
    }

    #[test]
    fn check_runs_map_by_status_then_conclusion() {
        for status in ["queued", "in_progress", "waiting", "requested", "pending"] {
            // A leftover conclusion never outranks "not finished".
            assert_eq!(check_run_outcome(status, None), Outcome::Pending);
            assert_eq!(
                check_run_outcome(status, Some("success")),
                Outcome::Pending,
                "{status}"
            );
        }
        for conclusion in ["success", "neutral", "skipped"] {
            assert_eq!(
                check_run_outcome("completed", Some(conclusion)),
                Outcome::Passed,
                "{conclusion}"
            );
        }
        for conclusion in [
            "failure",
            "timed_out",
            "cancelled",
            "action_required",
            "startup_failure",
            "stale",
            "something_new",
        ] {
            assert_eq!(
                check_run_outcome("completed", Some(conclusion)),
                Outcome::Failed,
                "{conclusion}"
            );
        }
        assert_eq!(check_run_outcome("completed", None), Outcome::Failed);
        assert_eq!(check_run_outcome("", None), Outcome::Pending);
    }

    #[test]
    fn commit_statuses_map_by_state() {
        assert_eq!(commit_status_outcome("success"), Outcome::Passed);
        assert_eq!(commit_status_outcome("failure"), Outcome::Failed);
        assert_eq!(commit_status_outcome("error"), Outcome::Failed);
        assert_eq!(commit_status_outcome("pending"), Outcome::Pending);
        assert_eq!(commit_status_outcome("expected"), Outcome::Pending);
    }

    #[test]
    fn checks_count_once_per_name_by_their_worst_outcome() {
        let totals = summarize_checks([
            // Two workflows with a job called `build`: one failing is enough.
            check("build", Outcome::Passed),
            check("build", Outcome::Failed),
            check("lint", Outcome::Passed),
            // The order they are listed in changes nothing.
            check("test", Outcome::Failed),
            check("test", Outcome::Passed),
            // Still running under one name while done under the same name.
            check("deploy/preview", Outcome::Pending),
            check("deploy/preview", Outcome::Passed),
            // Failed outranks pending.
            check("scan", Outcome::Pending),
            check("scan", Outcome::Failed),
            check("docs", Outcome::Passed),
            check("docs", Outcome::Passed),
        ]);
        assert_eq!(
            (totals.total, totals.passed, totals.failed, totals.pending),
            (6, 2, 3, 1)
        );
        assert_eq!(totals.failed_names, ["build", "scan", "test"]);
        assert_eq!(totals.pending_names, ["deploy/preview"]);
        assert_eq!(summarize_checks([]), CheckTotals::default());
    }

    #[test]
    fn names_are_sorted_and_capped_while_counts_are_not() {
        let many = (0..30).map(|i| check(&format!("job-{i:02}"), Outcome::Failed));
        let pending = (0..25).map(|i| check(&format!("wait-{i:02}"), Outcome::Pending));
        let totals = summarize_checks(many.chain(pending));
        assert_eq!((totals.total, totals.failed, totals.pending), (55, 30, 25));
        assert_eq!(totals.failed_names.len(), MAX_NAMES);
        assert_eq!(totals.pending_names.len(), MAX_NAMES);
        assert_eq!(totals.failed_names[0], "job-00");
        assert_eq!(totals.failed_names[19], "job-19");
        // A check GitHub listed without a name still counts and is shown.
        let unnamed = summarize_checks([check("", Outcome::Failed)]);
        assert_eq!(unnamed.failed_names, ["(unnamed check)"]);
    }

    #[test]
    fn reviews_fold_to_the_latest_verdict_per_reviewer() {
        let review = |reviewer, state, minute| ReviewEvent {
            reviewer,
            state,
            at: at(minute),
        };
        let totals = fold_reviews(&[
            review("ana", "CHANGES_REQUESTED", 1),
            review("ben", "APPROVED", 2),
            // A comment after an approval leaves the approval standing.
            review("ben", "COMMENTED", 3),
            // Ana came round; logins compare without case.
            review("Ana", "APPROVED", 4),
            review("cy", "APPROVED", 5),
            // Cy's approval was dismissed (a new push with stale-review dismissal).
            review("cy", "DISMISSED", 6),
            review("dee", "CHANGES_REQUESTED", 7),
            // A review still being written has no say.
            ReviewEvent {
                reviewer: "dee",
                state: "PENDING",
                at: None,
            },
            review("eve", "COMMENTED", 8),
        ]);
        assert_eq!(
            totals,
            ReviewTotals {
                approvals: 2,
                changes_requested: 1
            }
        );
        assert_eq!(fold_reviews(&[]), ReviewTotals::default());

        // Out of order on the wire: the time decides, not the position.
        let shuffled = fold_reviews(&[
            review("ana", "APPROVED", 9),
            review("ana", "CHANGES_REQUESTED", 2),
        ]);
        assert_eq!(shuffled.approvals, 1);
        assert_eq!(shuffled.changes_requested, 0);
    }

    #[test]
    fn mergeability_and_methods_follow_what_github_told() {
        assert_eq!(mergeable_word(Some(true)), "mergeable");
        assert_eq!(mergeable_word(Some(false)), "conflicting");
        assert_eq!(mergeable_word(None), "unknown");

        assert_eq!(
            allowed_methods(Some(true), Some(false), Some(true)),
            ["squash", "rebase"]
        );
        assert_eq!(
            allowed_methods(Some(false), Some(true), Some(false)),
            ["merge"]
        );
        // Not told (no push access): all three, in the default order.
        assert_eq!(
            allowed_methods(None, None, None),
            ["squash", "merge", "rebase"]
        );
        // Told about some: the silent ones are not assumed.
        assert_eq!(allowed_methods(None, Some(true), None), ["merge"]);
    }
}

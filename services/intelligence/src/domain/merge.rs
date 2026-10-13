//! Merge readiness (CONTRACT.md §23.3), the pure half: what stands between
//! a pull request and its merge, from what GitHub says (through knowledge)
//! and SDI's own gate, and which merge method to use. Every blocker comes
//! with a sentence that says what to do about it.

use sdi_proto::knowledge::v1::PullRequestStatus;

pub const METHODS: [&str; 3] = ["squash", "merge", "rebase"];

/// One reason a pull request cannot be merged yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blocker {
    pub code: &'static str,
    pub message: String,
}

impl Blocker {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// The SDI review of the pull request's current head.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HeadReview {
    /// "context" | "analyzing" | "proposed" | "published" | "failed"
    pub status: String,
    pub error: String,
    /// Findings of severity high or critical nobody decided yet.
    pub undecided_blocking: u32,
}

impl HeadReview {
    /// The review reached `proposed` (and maybe was published since).
    pub fn finished(&self) -> bool {
        matches!(self.status.as_str(), "proposed" | "published")
    }
}

/// SDI's own side of the gate.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Gate {
    /// The workspace setting `merge_requires_review`.
    pub requires_review: bool,
    /// Head commit of the newest SDI review of this pull request, if any.
    pub newest_review_head: Option<String>,
    pub head_review: Option<HeadReview>,
    /// A merge execution for this pull request that has not ended.
    pub open_merge_execution: Option<String>,
    /// The execution service could not say whether it ended.
    pub merge_execution_unconfirmed: bool,
}

fn plural(count: u32, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

fn named(names: &[String]) -> String {
    if names.is_empty() {
        return String::new();
    }
    let shown: Vec<&str> = names.iter().take(5).map(String::as_str).collect();
    let more = names.len().saturating_sub(shown.len());
    format!(
        " ({}{})",
        shown.join(", "),
        if more > 0 {
            format!(", and {more} more")
        } else {
            String::new()
        }
    )
}

fn short(sha: &str) -> String {
    sha.chars().take(12).collect()
}

/// What GitHub itself says stands in the way, in the order of the
/// contract's table. A pull request that is not open reports only that.
pub fn github_blockers(status: &PullRequestStatus) -> Vec<Blocker> {
    let summary = status.summary.clone().unwrap_or_default();
    let checks = status.checks.clone().unwrap_or_default();
    let base = if summary.base_ref.is_empty() {
        "the base branch".to_owned()
    } else {
        format!("`{}`", summary.base_ref)
    };
    match summary.state.as_str() {
        "merged" => {
            return vec![Blocker::new(
                "not_open",
                "This pull request is already merged; there is nothing left to merge.",
            )];
        }
        "open" | "draft" => {}
        _ => {
            return vec![Blocker::new(
                "not_open",
                "This pull request is closed; reopen it on GitHub before merging.",
            )];
        }
    }
    let mut out = Vec::new();
    if summary.state == "draft" || status.mergeable_state == "draft" {
        out.push(Blocker::new(
            "draft",
            "This pull request is still a draft; mark it ready for review on GitHub first.",
        ));
    }
    if status.mergeable == "conflicting" || status.mergeable_state == "dirty" {
        out.push(Blocker::new(
            "conflicts",
            format!(
                "The branch conflicts with {base}; update it from {base} and resolve the conflicts, then ask again."
            ),
        ));
    } else if status.mergeable != "mergeable" {
        out.push(Blocker::new(
            "mergeability_unknown",
            "GitHub has not worked out yet whether this branch can be merged; ask again in a few seconds.",
        ));
    }
    if checks.failed > 0 {
        out.push(Blocker::new(
            "checks_failing",
            format!(
                "{} failed{}; fix or re-run {} on GitHub before merging.",
                plural(checks.failed, "check", "checks"),
                named(&checks.failed_names),
                if checks.failed == 1 { "it" } else { "them" }
            ),
        ));
    } else if checks.pending > 0 {
        out.push(Blocker::new(
            "checks_pending",
            format!(
                "{} still running{}; wait for {} to finish and ask again.",
                if checks.pending == 1 {
                    "1 check is".to_owned()
                } else {
                    format!("{} checks are", checks.pending)
                },
                named(&checks.pending_names),
                if checks.pending == 1 { "it" } else { "them" }
            ),
        ));
    }
    if status.changes_requested > 0 {
        out.push(Blocker::new(
            "changes_requested",
            format!(
                "{} requested changes on GitHub; address them and ask for a new review there.",
                plural(status.changes_requested, "reviewer", "reviewers")
            ),
        ));
    }
    // Branch protection, only when nothing above already explains the block.
    if status.mergeable_state == "blocked" && out.is_empty() {
        out.push(Blocker::new(
            "blocked_by_github",
            format!(
                "GitHub blocks this merge: {base} is protected and one of its rules is not met (a required \
                 approval or check, for example). Satisfy the rule on GitHub and ask again."
            ),
        ));
    }
    out
}

/// SDI's review gate for the pull request at `head_sha`: `no_review`,
/// `review_outdated` and `review_running` only while the workspace asks for
/// a review, `findings_undecided` whenever the head's review has one.
pub fn review_blockers(head_sha: &str, gate: &Gate) -> Vec<Blocker> {
    let mut out = Vec::new();
    if gate.requires_review {
        match (&gate.newest_review_head, &gate.head_review) {
            (None, _) => out.push(Blocker::new(
                "no_review",
                "SDI has not reviewed this pull request; start a review and decide its findings first.",
            )),
            (Some(reviewed), None) => out.push(Blocker::new(
                "review_outdated",
                format!(
                    "The newest SDI review is for {}, and the pull request is now at {}; review the new head first.",
                    short(reviewed),
                    short(head_sha)
                ),
            )),
            (Some(_), Some(review)) if !review.finished() => out.push(Blocker::new(
                "review_running",
                if review.status == "failed" {
                    format!(
                        "The SDI review of this head failed{}; start it again.",
                        if review.error.trim().is_empty() {
                            String::new()
                        } else {
                            format!(" ({})", review.error.trim())
                        }
                    )
                } else {
                    "The SDI review of this head is still running; wait until it is proposed.".to_owned()
                },
            )),
            _ => {}
        }
    }
    if let Some(review) = gate.head_review.as_ref().filter(|r| r.finished())
        && review.undecided_blocking > 0
    {
        out.push(Blocker::new(
            "findings_undecided",
            format!(
                "{} of severity high or critical {} undecided; accept or dismiss {} in the review first.",
                plural(review.undecided_blocking, "finding", "findings"),
                if review.undecided_blocking == 1 {
                    "is"
                } else {
                    "are"
                },
                if review.undecided_blocking == 1 {
                    "it"
                } else {
                    "them"
                }
            ),
        ));
    }
    out
}

/// `merge_in_progress`, when a merge execution of this pull request is open.
pub fn merge_blocker(gate: &Gate) -> Option<Blocker> {
    let execution = gate.open_merge_execution.as_ref()?;
    Some(Blocker::new(
        "merge_in_progress",
        if gate.merge_execution_unconfirmed {
            format!(
                "Merge execution {execution} was requested for this pull request and the execution service \
                 cannot be reached to confirm it ended; try again shortly."
            )
        } else {
            format!(
                "Merge execution {execution} for this pull request is still open; approve, finish or cancel it \
                 first."
            )
        },
    ))
}

/// SDI's whole gate without asking GitHub anything: what the watcher
/// checks before it spends a request on a pull request's status.
pub fn gate_blockers(head_sha: &str, gate: &Gate) -> Vec<Blocker> {
    let mut out = review_blockers(head_sha, gate);
    out.extend(merge_blocker(gate));
    out
}

/// Every blocker, in the order of the contract's table. Empty means the
/// pull request can be merged now.
pub fn blockers(status: &PullRequestStatus, gate: &Gate) -> Vec<Blocker> {
    let mut out = github_blockers(status);
    if !out.iter().any(|b| b.code == "not_open") {
        let head_sha = status
            .summary
            .as_ref()
            .map(|s| s.head_sha.as_str())
            .unwrap_or_default();
        out.extend(review_blockers(head_sha, gate));
    }
    out.extend(merge_blocker(gate));
    out
}

/// Why a merge method cannot be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MethodError {
    /// Not one of merge, squash, rebase.
    Unknown(String),
    /// The repository does not allow it.
    NotAllowed {
        method: String,
        allowed: Vec<String>,
    },
    /// Nothing to pick from: GitHub named no method the repository allows.
    NoneAllowed,
}

impl MethodError {
    pub fn sentence(&self) -> String {
        match self {
            Self::Unknown(method) => {
                format!("method {method:?} is not one of merge, squash, rebase")
            }
            Self::NotAllowed { method, allowed } => format!(
                "this repository does not allow {method} merges; it allows: {}",
                allowed.join(", ")
            ),
            Self::NoneAllowed => {
                "GitHub did not name a merge method this repository allows; name one (merge, \
                                  squash or rebase) explicitly"
                    .to_owned()
            }
        }
    }
}

/// The method the merge uses: the requested one when the repository allows
/// it; for an empty request the first allowed of squash, merge, rebase.
pub fn pick_method(requested: &str, allowed: &[String]) -> Result<String, MethodError> {
    let requested = requested.trim().to_ascii_lowercase();
    let allows = |method: &str| allowed.iter().any(|a| a.eq_ignore_ascii_case(method));
    if requested.is_empty() {
        return METHODS
            .iter()
            .find(|m| allows(m))
            .map(|m| (*m).to_owned())
            .ok_or(MethodError::NoneAllowed);
    }
    if !METHODS.contains(&requested.as_str()) {
        return Err(MethodError::Unknown(requested));
    }
    // An empty list means GitHub did not say; GitHub still enforces its
    // own setting when the merge is attempted.
    if !allowed.is_empty() && !allows(&requested) {
        return Err(MethodError::NotAllowed {
            method: requested,
            allowed: allowed.to_vec(),
        });
    }
    Ok(requested)
}

/// "Merge #7: Containerise the API" (CONTRACT.md §23.3).
pub fn merge_title(pr_number: u64, title: &str) -> String {
    let title: String = title.trim().chars().take(120).collect();
    if title.is_empty() {
        format!("Merge #{pr_number}")
    } else {
        format!("Merge #{pr_number}: {title}")
    }
}

#[cfg(test)]
pub(crate) mod fixtures {
    use sdi_proto::knowledge::v1::{CheckSummary, PullRequestStatus, PullRequestSummary};

    pub const HEAD: &str = "1111111111111111111111111111111111111111";

    /// An open pull request GitHub would merge right now.
    pub fn clean(number: u64) -> PullRequestStatus {
        PullRequestStatus {
            summary: Some(PullRequestSummary {
                project_id: "proj-1".into(),
                number,
                title: "Bump left-pad from 1.3.0 to 1.3.1".into(),
                author: "dependabot[bot]".into(),
                state: "open".into(),
                base_ref: "main".into(),
                head_ref: "dependabot/npm_and_yarn/left-pad-1.3.1".into(),
                head_sha: HEAD.into(),
                head_repo: "f8fvwgzc/view-engine".into(),
                is_bot: true,
                is_dependabot: true,
                ..Default::default()
            }),
            mergeable: "mergeable".into(),
            mergeable_state: "clean".into(),
            checks: Some(CheckSummary {
                total: 3,
                passed: 3,
                ..Default::default()
            }),
            approvals: 1,
            allowed_methods: vec!["merge".into(), "squash".into(), "rebase".into()],
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{fixtures::*, *};

    fn reviewed() -> Gate {
        Gate {
            requires_review: true,
            newest_review_head: Some(HEAD.into()),
            head_review: Some(HeadReview {
                status: "proposed".into(),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn codes(blockers: &[Blocker]) -> Vec<&'static str> {
        blockers.iter().map(|b| b.code).collect()
    }

    #[test]
    fn a_clean_reviewed_pull_request_has_no_blocker() {
        assert!(blockers(&clean(7), &reviewed()).is_empty());
    }

    #[test]
    fn every_github_blocker_has_its_code_and_a_sentence() {
        let with = |edit: fn(&mut PullRequestStatus)| {
            let mut status = clean(7);
            edit(&mut status);
            blockers(&status, &reviewed())
        };
        let state = |s: &'static str| {
            let mut status = clean(7);
            status.summary.as_mut().unwrap().state = s.into();
            blockers(&status, &reviewed())
        };
        assert_eq!(codes(&state("closed")), ["not_open"]);
        let merged = state("merged");
        assert_eq!(codes(&merged), ["not_open"]);
        assert!(merged[0].message.contains("already merged"));
        assert_eq!(codes(&state("draft")), ["draft"]);

        let conflicts = with(|s| s.mergeable = "conflicting".into());
        assert_eq!(codes(&conflicts), ["conflicts"]);
        assert!(
            conflicts[0].message.contains("`main`"),
            "{}",
            conflicts[0].message
        );
        assert_eq!(
            codes(&with(|s| s.mergeable_state = "dirty".into())),
            ["conflicts"]
        );
        assert_eq!(
            codes(&with(|s| s.mergeable = "unknown".into())),
            ["mergeability_unknown"]
        );
        assert_eq!(
            codes(&with(|s| s.mergeable = String::new())),
            ["mergeability_unknown"],
            "silence is not a yes"
        );

        let failing = with(|s| {
            let checks = s.checks.as_mut().unwrap();
            checks.failed = 2;
            checks.pending = 1;
            checks.failed_names = vec!["test".into(), "lint".into()];
        });
        assert_eq!(
            codes(&failing),
            ["checks_failing"],
            "pending hides behind failed"
        );
        assert!(failing[0].message.contains("2 checks failed (test, lint)"));
        let pending = with(|s| {
            let checks = s.checks.as_mut().unwrap();
            checks.pending = 1;
            checks.pending_names = vec!["build".into()];
        });
        assert_eq!(codes(&pending), ["checks_pending"]);
        assert!(
            pending[0]
                .message
                .contains("1 check is still running (build)")
        );

        let requested = with(|s| s.changes_requested = 1);
        assert_eq!(codes(&requested), ["changes_requested"]);
        assert!(
            requested[0]
                .message
                .contains("1 reviewer requested changes")
        );

        // Branch protection only when nothing above explains the block.
        let blocked = with(|s| s.mergeable_state = "blocked".into());
        assert_eq!(codes(&blocked), ["blocked_by_github"]);
        assert!(blocked[0].message.contains("protected"));
        let explained = with(|s| {
            s.mergeable_state = "blocked".into();
            s.changes_requested = 2;
        });
        assert_eq!(codes(&explained), ["changes_requested"]);

        // Several at once come in the table's order.
        let many = with(|s| {
            s.summary.as_mut().unwrap().state = "draft".into();
            s.mergeable = "conflicting".into();
            s.checks.as_mut().unwrap().failed = 1;
            s.changes_requested = 1;
        });
        assert_eq!(
            codes(&many),
            ["draft", "conflicts", "checks_failing", "changes_requested"]
        );
        for blocker in many.iter().chain(&blocked).chain(&pending) {
            assert!(blocker.message.ends_with('.'), "{}", blocker.message);
            assert!(blocker.message.len() > 30, "{}", blocker.message);
        }
    }

    #[test]
    fn the_review_gate_has_its_five_codes() {
        let status = clean(7);
        let gate = |edit: fn(&mut Gate)| {
            let mut gate = reviewed();
            edit(&mut gate);
            blockers(&clean(7), &gate)
        };
        let none = gate(|g| {
            g.newest_review_head = None;
            g.head_review = None;
        });
        assert_eq!(codes(&none), ["no_review"]);
        let outdated = gate(|g| {
            g.newest_review_head = Some("0000000000000000000000000000000000000000".into());
            g.head_review = None;
        });
        assert_eq!(codes(&outdated), ["review_outdated"]);
        assert!(outdated[0].message.contains("000000000000"));
        assert!(outdated[0].message.contains("111111111111"));
        let running = gate(|g| g.head_review.as_mut().unwrap().status = "analyzing".into());
        assert_eq!(codes(&running), ["review_running"]);
        let failed = gate(|g| {
            let review = g.head_review.as_mut().unwrap();
            review.status = "failed".into();
            review.error = "project context unavailable".into();
        });
        assert_eq!(codes(&failed), ["review_running"]);
        assert!(
            failed[0]
                .message
                .contains("failed (project context unavailable)")
        );
        let undecided = gate(|g| g.head_review.as_mut().unwrap().undecided_blocking = 2);
        assert_eq!(codes(&undecided), ["findings_undecided"]);
        assert!(
            undecided[0]
                .message
                .contains("2 findings of severity high or critical are")
        );
        // Published reviews count as finished.
        assert!(gate(|g| g.head_review.as_mut().unwrap().status = "published".into()).is_empty());

        let merging = gate(|g| g.open_merge_execution = Some("exec-9".into()));
        assert_eq!(codes(&merging), ["merge_in_progress"]);
        assert!(merging[0].message.contains("exec-9"));
        let unsure = gate(|g| {
            g.open_merge_execution = Some("exec-9".into());
            g.merge_execution_unconfirmed = true;
        });
        assert!(unsure[0].message.contains("cannot be reached"));
        // It also shows on a pull request that is no longer open.
        let mut closed = status.clone();
        closed.summary.as_mut().unwrap().state = "merged".into();
        let mut gate_open = reviewed();
        gate_open.open_merge_execution = Some("exec-9".into());
        assert_eq!(
            codes(&blockers(&closed, &gate_open)),
            ["not_open", "merge_in_progress"]
        );
    }

    #[test]
    fn with_the_gate_off_only_undecided_findings_and_an_open_merge_remain() {
        let off = |edit: fn(&mut Gate)| {
            let mut gate = Gate::default();
            edit(&mut gate);
            blockers(&clean(7), &gate)
        };
        assert!(off(|_| {}).is_empty(), "no review needed");
        assert!(
            off(|g| g.newest_review_head = Some("0000".into())).is_empty(),
            "an outdated review does not block"
        );
        assert!(
            off(|g| {
                g.newest_review_head = Some(HEAD.into());
                g.head_review = Some(HeadReview {
                    status: "analyzing".into(),
                    ..Default::default()
                });
            })
            .is_empty(),
            "a running review does not block"
        );
        let undecided = off(|g| {
            g.newest_review_head = Some(HEAD.into());
            g.head_review = Some(HeadReview {
                status: "proposed".into(),
                undecided_blocking: 1,
                ..Default::default()
            });
        });
        assert_eq!(codes(&undecided), ["findings_undecided"]);
        assert_eq!(
            codes(&off(|g| g.open_merge_execution = Some("exec-1".into()))),
            ["merge_in_progress"]
        );
        // GitHub's own blockers do not depend on the setting.
        let mut status = clean(7);
        status.checks.as_mut().unwrap().failed = 1;
        assert_eq!(
            codes(&blockers(&status, &Gate::default())),
            ["checks_failing"]
        );
    }

    #[test]
    fn method_is_validated_or_picked_in_order() {
        let all: Vec<String> = vec!["merge".into(), "squash".into(), "rebase".into()];
        assert_eq!(pick_method("", &all).unwrap(), "squash");
        assert_eq!(pick_method("", &all[..1]).unwrap(), "merge");
        assert_eq!(
            pick_method("", &["rebase".to_owned(), "merge".to_owned()]).unwrap(),
            "merge"
        );
        assert_eq!(pick_method("", &["rebase".to_owned()]).unwrap(), "rebase");
        assert_eq!(pick_method("", &[]), Err(MethodError::NoneAllowed));
        assert_eq!(pick_method(" Rebase ", &all).unwrap(), "rebase");
        assert_eq!(
            pick_method("fast-forward", &all),
            Err(MethodError::Unknown("fast-forward".into()))
        );
        let refused = pick_method("squash", &all[..1]).unwrap_err();
        assert_eq!(
            refused,
            MethodError::NotAllowed {
                method: "squash".into(),
                allowed: vec!["merge".into()]
            }
        );
        assert!(refused.sentence().contains("it allows: merge"));
        // GitHub did not say: an explicit choice is passed on.
        assert_eq!(pick_method("squash", &[]).unwrap(), "squash");
        assert_eq!(
            merge_title(7, " Containerise the API "),
            "Merge #7: Containerise the API"
        );
        assert_eq!(merge_title(7, ""), "Merge #7");
    }
}

//! GitHub Actions workflows: a new workflow without a `permissions` block,
//! `pull_request_target` combined with a checkout of the PR head, and
//! actions pinned to a branch or tag instead of a commit.

use std::sync::LazyLock;

use regex::Regex;

use super::{Finding, Severity, evidence};
use crate::domain::review::input::ReviewInput;

static USES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^\s*-?\s*uses:\s*["']?([\w.-]+/[\w./-]+)@([^\s"'#]+)"#).expect("uses")
});
static CHECKOUT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"uses:\s*['\x22]?actions/checkout@").expect("checkout"));

fn is_sha(reference: &str) -> bool {
    reference.len() == 40 && reference.chars().all(|c| c.is_ascii_hexdigit())
}

pub fn analyze(input: &ReviewInput) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in input
        .files
        .iter()
        .filter(|f| f.is_workflow() && !f.is_removed())
    {
        let path = &file.path;
        let added = file.added_lines();
        let text = file.added_text();

        if file.is_added()
            && !text
                .lines()
                .any(|l| l.trim_start().starts_with("permissions:"))
        {
            findings.push(
                Finding::new(
                    "ci",
                    "no_permissions",
                    Severity::Medium,
                    path,
                    file.first_line(),
                    "Workflow without a permissions block",
                    format!(
                        "`{path}` is a new workflow with no top-level `permissions:`, so every job gets the repository's \
                         default `GITHUB_TOKEN` rights. Declare the least the jobs need (usually `contents: read`)."
                    ),
                )
                .with_suggestion("permissions:\n  contents: read"),
            );
        }

        let targets_pr = text.contains("pull_request_target");
        if targets_pr {
            for (line_no, line) in &added {
                if CHECKOUT.is_match(line) {
                    findings.push(Finding::new(
                        "ci",
                        "pull_request_target_checkout",
                        Severity::Critical,
                        path,
                        *line_no,
                        "pull_request_target with a checkout",
                        format!(
                            "The workflow runs on `pull_request_target` (with the base repository's secrets) and line {line_no} \
                             checks out code: {}. A fork's PR can then run its own code with those secrets. Use \
                             `pull_request`, or never check out the PR head here.",
                            evidence(line)
                        ),
                    ));
                }
            }
        }

        for (line_no, line) in &added {
            if let Some(caps) = USES.captures(line) {
                let (action, reference) = (&caps[1], &caps[2]);
                if is_sha(reference) {
                    continue;
                }
                let branch = matches!(reference, "main" | "master" | "develop" | "trunk");
                let (severity, what) = if branch {
                    (Severity::Medium, "a branch")
                } else {
                    (Severity::Low, "a tag")
                };
                findings.push(Finding::new(
                    "ci",
                    "unpinned_action",
                    severity,
                    path,
                    *line_no,
                    format!("Action pinned to {what}: {action}"),
                    format!(
                        "Line {line_no} uses `{action}@{reference}`, {what} that can move. Pin the full commit SHA \
                         (keep the tag in a trailing comment) so a compromised action cannot change what your workflow runs."
                    ),
                ));
            }
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::*;
    use super::*;

    #[test]
    fn new_workflow_without_permissions_and_tag_pins() {
        let findings = analyze(&input(vec![added(
            ".github/workflows/ci.yml",
            &[
                "name: ci",
                "on: [push]",
                "jobs:",
                "  build:",
                "    steps:",
                "      - uses: actions/checkout@v4",
                "      - uses: actions/setup-node@v4",
            ],
        )]));
        assert_eq!(
            rules(&findings),
            vec![
                "ci.no_permissions",
                "ci.unpinned_action",
                "ci.unpinned_action"
            ]
        );
        assert_eq!(findings[0].line, 1);
        assert_eq!(findings[1].severity, Severity::Low);
    }

    #[test]
    fn pull_request_target_with_checkout_is_critical() {
        let findings = analyze(&input(vec![modified(
            ".github/workflows/pr.yml",
            &[
                "on: pull_request_target",
                "permissions: write-all",
                "      - uses: actions/checkout@8e5e7e5ab8b370d6c329ec480221332ada57f0ab",
                "        with:",
                "          ref: ${{ github.event.pull_request.head.sha }}",
            ],
        )]));
        assert_eq!(rules(&findings), vec!["ci.pull_request_target_checkout"]);
        assert_eq!(findings[0].severity, Severity::Critical);
        assert_eq!(findings[0].line, 3);
    }

    #[test]
    fn branch_pins_are_medium() {
        let findings = analyze(&input(vec![modified(
            ".github/workflows/release.yaml",
            &["      - uses: some-org/release-action@main"],
        )]));
        assert_eq!(rules(&findings), vec!["ci.unpinned_action"]);
        assert_eq!(findings[0].severity, Severity::Medium);
    }

    #[test]
    fn sha_pinned_scoped_workflow_is_clean() {
        let findings = analyze(&input(vec![
            added(
                ".github/workflows/ci.yml",
                &[
                    "on: pull_request",
                    "permissions:",
                    "  contents: read",
                    "      - uses: actions/checkout@8e5e7e5ab8b370d6c329ec480221332ada57f0ab # v4",
                    "      - uses: ./.github/actions/local",
                ],
            ),
            added(".github/dependabot.yml", &["- uses: nope@v1"]),
        ]));
        assert!(findings.is_empty(), "{findings:?}");
    }
}

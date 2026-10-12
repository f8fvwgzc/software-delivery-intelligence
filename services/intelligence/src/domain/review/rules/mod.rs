//! The deterministic analyzers (CONTRACT.md §20), one file per category.
//! Each is `fn analyze(&ReviewInput) -> Vec<Finding>`: pure, evidence in
//! `detail`, a `suggestion` when the fix is mechanical, and a rule id of
//! the form `<category>.<check>` so a human can tell the checks apart.

pub mod ci;
pub mod correctness;
pub mod custom;
pub mod dependencies;
pub mod docker;
pub mod docs;
pub mod duplication;
pub mod iac;
pub mod maturity;
pub mod migrations;
pub mod osv;
pub mod secrets;
pub mod security;
pub mod tests;

use std::{borrow::Cow, collections::HashSet};

use super::input::{ReviewInput, SUPPRESS_AFTER_DISMISSALS};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

impl Severity {
    /// One step up, for the ordering of rules a project keeps accepting.
    pub fn bumped(self) -> Self {
        match self {
            Self::Info => Self::Low,
            Self::Low => Self::Medium,
            Self::Medium => Self::High,
            Self::High | Self::Critical => Self::Critical,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// "secrets" | "security" | "dependencies" | "docker" | "ci" | "iac" | "tests" | "docs" | "correctness" | "maturity"
    pub category: &'static str,
    pub severity: Severity,
    pub title: String,
    /// Markdown: the evidence and the fix.
    pub detail: String,
    pub path: String,
    /// Line in the new file; 0 for a file-level finding.
    pub line: u32,
    pub end_line: u32,
    /// Replacement for the referenced lines, when the fix is mechanical.
    pub suggestion: Option<String>,
    /// `<category>.<check>`.
    pub rule: String,
    /// Set by the project's learnings (CONTRACT.md §20): the rule was
    /// dismissed often enough that this is shown as information only.
    pub suppressed: bool,
}

/// What a suppressed finding's detail starts with.
pub const SUPPRESSED_NOTE: &str = "Suppressed by project learnings";

impl Finding {
    pub fn new(
        category: &'static str,
        check: &str,
        severity: Severity,
        path: &str,
        line: u32,
        title: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            category,
            severity,
            title: title.into(),
            detail: detail.into(),
            path: path.to_owned(),
            line,
            end_line: line,
            suggestion: None,
            rule: format!("{category}.{check}"),
            suppressed: false,
        }
    }

    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestion = Some(suggestion.into());
        self
    }
}

/// Shows a line in a finding's evidence without pasting a secret or a wall
/// of text: trimmed, cut at 120 characters, fenced.
pub(crate) fn evidence(line: &str) -> String {
    let trimmed = line.trim();
    let cut: String = trimmed.chars().take(120).collect();
    let suffix = if cut.len() < trimmed.len() { "…" } else { "" };
    format!("`{}{}`", cut.replace('`', "'"), suffix)
}

/// Every rule, in category order, then [`finalize`]: the full pipeline
/// without the asynchronous OSV check, which the runner adds in between.
#[cfg(test)]
pub fn analyze_all(input: &ReviewInput) -> Vec<Finding> {
    finalize(collect(input), input)
}

/// The input minus the files `.sdi.yml review.ignore_paths` excludes.
pub fn without_ignored(input: &ReviewInput) -> Cow<'_, ReviewInput> {
    if input.config.ignore_paths.is_empty() {
        return Cow::Borrowed(input);
    }
    let mut filtered = input.clone();
    filtered.files.retain(|f| !input.config.ignores(&f.path));
    Cow::Owned(filtered)
}

/// Every rule's raw findings over the files that are not ignored, plus one
/// note when the pull request was too large to read in full.
pub fn collect(input: &ReviewInput) -> Vec<Finding> {
    let input = without_ignored(input);
    let input = input.as_ref();
    let mut findings = Vec::new();
    findings.extend(secrets::analyze(input));
    findings.extend(security::analyze(input));
    findings.extend(dependencies::analyze(input));
    findings.extend(docker::analyze(input));
    findings.extend(ci::analyze(input));
    findings.extend(iac::analyze(input));
    findings.extend(tests::analyze(input));
    findings.extend(docs::analyze(input));
    findings.extend(correctness::analyze(input));
    findings.extend(duplication::analyze(input));
    findings.extend(migrations::analyze(input));
    findings.extend(maturity::analyze(input));
    findings.extend(custom::analyze(input));
    let skipped: Vec<&_> = input.files.iter().filter(|f| f.truncated).collect();
    if input.files_truncated || !skipped.is_empty() {
        let mut parts = Vec::new();
        if input.files_truncated {
            parts.push(format!(
                "only {} of {} changed files were fetched",
                input.files.len(),
                input.pr.changed_files
            ));
        }
        if !skipped.is_empty() {
            parts.push(format!(
                "{} file{} had a patch too large to read ({})",
                skipped.len(),
                if skipped.len() == 1 { "" } else { "s" },
                skipped
                    .iter()
                    .take(5)
                    .map(|f| format!("`{}`", f.path))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        findings.push(Finding::new(
            "review",
            "truncated",
            Severity::Info,
            "",
            0,
            "Review did not read the whole change",
            format!(
                "The pull request is over the review limits: {}. Those files were not analysed; review them by \
                 hand or split the pull request.",
                parts.join("; ")
            ),
        ));
    }
    findings
}

/// Applies `.sdi.yml` severity overrides and the project's learnings,
/// drops exact duplicates and sorts: worst first (an accepted rule's
/// findings rank one step higher), then path, line and rule.
pub fn finalize(mut findings: Vec<Finding>, input: &ReviewInput) -> Vec<Finding> {
    for finding in &mut findings {
        if let Some(severity) = input
            .config
            .severity
            .get(&finding.rule)
            .and_then(|s| custom::severity_from_str(s))
        {
            finding.severity = severity;
        }
        if let Some(learning) = input.learning(&finding.rule)
            && learning.accepted == 0
            && learning.dismissed >= SUPPRESS_AFTER_DISMISSALS
            && finding.severity != Severity::Critical
        {
            finding.suppressed = true;
            finding.severity = Severity::Info;
            finding.detail = format!(
                "_{SUPPRESSED_NOTE}: this project dismissed `{}` {} times and never accepted it._\n\n{}",
                finding.rule, learning.dismissed, finding.detail
            );
        }
    }
    let mut seen: HashSet<(String, String, u32)> = HashSet::new();
    findings.retain(|f| seen.insert((f.rule.clone(), f.path.clone(), f.line)));
    let order = |f: &Finding| {
        let accepted = input.learning(&f.rule).is_some_and(|l| l.accepted > 0) && !f.suppressed;
        if accepted {
            f.severity.bumped()
        } else {
            f.severity
        }
    };
    findings.sort_by(|a, b| {
        order(b)
            .cmp(&order(a))
            .then_with(|| b.severity.cmp(&a.severity))
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.line.cmp(&b.line))
            .then_with(|| a.rule.cmp(&b.rule))
    });
    findings
}

/// Diff fixtures for the rule tests: a file is described by its path,
/// status and the lines it adds (and optionally removes), which is all
/// the analyzers look at.
#[cfg(test)]
pub(crate) mod fixtures {
    use super::super::input::{ChangedFile, Hunk, ReviewInput};

    pub fn added(path: &str, lines: &[&str]) -> ChangedFile {
        file(path, "added", lines, &[])
    }

    pub fn modified(path: &str, lines: &[&str]) -> ChangedFile {
        file(path, "modified", lines, &[])
    }

    pub fn removed(path: &str) -> ChangedFile {
        ChangedFile {
            path: path.into(),
            status: "removed".into(),
            deletions: 1,
            ..Default::default()
        }
    }

    /// One hunk at line 1: `removed` lines first, then `added` lines.
    pub fn file(path: &str, status: &str, added: &[&str], removed: &[&str]) -> ChangedFile {
        let mut lines: Vec<String> = removed.iter().map(|l| format!("-{l}")).collect();
        lines.extend(added.iter().map(|l| format!("+{l}")));
        ChangedFile {
            path: path.into(),
            status: status.into(),
            additions: added.len() as u32,
            deletions: removed.len() as u32,
            language: sdi_analyzer_language(path),
            hunks: vec![Hunk {
                old_start: 1,
                old_lines: removed.len() as u32,
                new_start: 1,
                new_lines: added.len() as u32,
                lines,
            }],
            truncated: false,
            ..Default::default()
        }
    }

    fn sdi_analyzer_language(path: &str) -> String {
        match path.rsplit_once('.').map(|(_, e)| e) {
            Some("rs") => "rust",
            Some("ts" | "tsx") => "typescript",
            Some("js" | "jsx") => "javascript",
            Some("py") => "python",
            Some("go") => "go",
            _ => "",
        }
        .to_owned()
    }

    /// The day the rule fixtures are judged on (end-of-life checks).
    pub const TODAY: sdi_analyzer::Day = (2026, 10, 9);

    pub fn input(files: Vec<ChangedFile>) -> ReviewInput {
        ReviewInput {
            project_name: "repo".into(),
            repo_full_name: "o/repo".into(),
            primary_language: "Rust".into(),
            files,
            today: TODAY,
            ..Default::default()
        }
    }

    pub fn rules(findings: &[super::Finding]) -> Vec<&str> {
        findings.iter().map(|f| f.rule.as_str()).collect()
    }
}

#[cfg(test)]
mod tests_all {
    use super::{fixtures::*, *};

    #[test]
    fn findings_are_sorted_and_deduped() {
        let input = input(vec![
            added("src/main.rs", &["let x = foo().unwrap();", "// TODO later"]),
            added(
                "config.py",
                &["AWS_KEY = \"AKIAIOSFODNN7EXAMPLE\"", "verify=False"],
            ),
        ]);
        let findings = analyze_all(&input);
        assert!(!findings.is_empty());
        for pair in findings.windows(2) {
            assert!(pair[0].severity >= pair[1].severity);
        }
        let keys: HashSet<_> = findings
            .iter()
            .map(|f| (f.rule.clone(), f.path.clone(), f.line))
            .collect();
        assert_eq!(keys.len(), findings.len());
        assert_eq!(findings[0].category, "secrets");
        assert!(evidence("  `x` ").starts_with("`'x'`"));
        assert!(evidence(&"y".repeat(200)).ends_with("…`"));
    }

    #[test]
    fn ignored_paths_overrides_and_learnings_shape_the_result() {
        use crate::domain::review::input::{ReviewConfig, RuleLearning};
        let mut input = input(vec![
            added("src/main.rs", &["let x = foo().unwrap();", "// TODO later"]),
            added("gen/out.rs", &["let y = bar().unwrap();"]),
            added("config.py", &["verify=False"]),
        ]);
        input.config = ReviewConfig {
            ignore_paths: vec!["gen/**".into()],
            severity: [("correctness.todo".to_owned(), "high".to_owned())]
                .into_iter()
                .collect(),
            ..Default::default()
        };
        input.learnings = vec![
            RuleLearning {
                rule: "correctness.unwrap".into(),
                accepted: 0,
                dismissed: 3,
            },
            RuleLearning {
                rule: "security.tls_disabled".into(),
                accepted: 2,
                dismissed: 1,
            },
        ];
        let findings = analyze_all(&input);
        assert!(findings.iter().all(|f| !f.path.starts_with("gen/")));
        let todo = findings
            .iter()
            .find(|f| f.rule == "correctness.todo")
            .unwrap();
        assert_eq!(todo.severity, Severity::High);
        let unwrap = findings
            .iter()
            .find(|f| f.rule == "correctness.unwrap")
            .unwrap();
        assert!(unwrap.suppressed);
        assert_eq!(unwrap.severity, Severity::Info);
        assert!(unwrap.detail.starts_with(&format!("_{SUPPRESSED_NOTE}")));
        // An accepted High rule orders above a plain High one (todo).
        assert_eq!(findings[0].rule, "security.tls_disabled");
        assert_eq!(findings[0].severity, Severity::High);
        // Dismissed but also accepted once: never suppressed.
        input.learnings[0].accepted = 1;
        let again = analyze_all(&input);
        assert!(
            !again
                .iter()
                .find(|f| f.rule == "correctness.unwrap")
                .unwrap()
                .suppressed
        );
        // Critical findings are never suppressed.
        input.learnings.push(RuleLearning {
            rule: "secrets.aws_key".into(),
            accepted: 0,
            dismissed: 9,
        });
        input
            .files
            .push(added("k.py", &["AWS_KEY = \"AKIAIOSFODNN7EXAMPLE\""]));
        let with_secret = analyze_all(&input);
        let key = with_secret
            .iter()
            .find(|f| f.rule == "secrets.aws_key")
            .unwrap();
        assert!(!key.suppressed && key.severity == Severity::Critical);
    }

    #[test]
    fn truncated_pull_requests_say_so_once() {
        let mut input = input(vec![added("a.rs", &["fn a() {}"])]);
        input.files_truncated = true;
        input.pr.changed_files = 250;
        input.files.push(crate::domain::review::input::ChangedFile {
            path: "huge.json".into(),
            status: "modified".into(),
            additions: 50_000,
            truncated: true,
            ..Default::default()
        });
        let findings = analyze_all(&input);
        let notes: Vec<&Finding> = findings
            .iter()
            .filter(|f| f.rule == "review.truncated")
            .collect();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].severity, Severity::Info);
        assert!(notes[0].detail.contains("2 of 250"));
        assert!(notes[0].detail.contains("`huge.json`"));
    }
}

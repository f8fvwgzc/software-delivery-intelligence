//! The deterministic analyzers (CONTRACT.md §20), one file per category.
//! Each is `fn analyze(&ReviewInput) -> Vec<Finding>`: pure, evidence in
//! `detail`, a `suggestion` when the fix is mechanical, and a rule id of
//! the form `<category>.<check>` so a human can tell the checks apart.

pub mod ci;
pub mod correctness;
pub mod dependencies;
pub mod docker;
pub mod docs;
pub mod iac;
pub mod maturity;
pub mod secrets;
pub mod security;
pub mod tests;

use std::collections::HashSet;

use super::input::ReviewInput;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

impl Severity {
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
}

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

/// Every rule, in category order; findings sorted by severity (worst
/// first), then path and line, with exact duplicates dropped.
pub fn analyze_all(input: &ReviewInput) -> Vec<Finding> {
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
    findings.extend(maturity::analyze(input));
    let mut seen: HashSet<(String, String, u32)> = HashSet::new();
    findings.retain(|f| seen.insert((f.rule.clone(), f.path.clone(), f.line)));
    findings.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
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

    pub fn input(files: Vec<ChangedFile>) -> ReviewInput {
        ReviewInput {
            project_name: "repo".into(),
            repo_full_name: "o/repo".into(),
            primary_language: "Rust".into(),
            files,
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
}

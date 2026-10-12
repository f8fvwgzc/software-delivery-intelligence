//! Source changed without tests, when the project has a test framework
//! to write them in.

use super::{Finding, Severity};
use crate::domain::review::input::ReviewInput;

/// Below this many changed source lines a missing test is noise, not a finding.
const MIN_SOURCE_LINES: u32 = 5;

pub fn analyze(input: &ReviewInput) -> Vec<Finding> {
    let mut findings = Vec::new();
    let has_tests = input.has_file(|f| f.is_test() && !f.is_removed());
    // `.sdi.yml review.require_tests_for`: these paths always need a test
    // change, test framework detected or not, however small the edit.
    if !has_tests && !input.config.require_tests_for.is_empty() {
        let required: Vec<&_> = input
            .files
            .iter()
            .filter(|f| f.is_reviewable() && !f.is_test() && input.config.requires_tests(&f.path))
            .collect();
        if let Some(first) = required.first() {
            let listed: Vec<String> = required
                .iter()
                .take(5)
                .map(|f| format!("`{}`", f.path))
                .collect();
            findings.push(Finding::new(
                "tests",
                "required",
                Severity::High,
                &first.path,
                first.first_line(),
                "Change to a path that requires tests",
                format!(
                    "{} match{} `review.require_tests_for` in `.sdi.yml` ({}{}) and no test file is part of this pull \
                     request. Add or extend a test that fails without this change.",
                    if required.len() == 1 { "This file".to_owned() } else { format!("{} files", required.len()) },
                    if required.len() == 1 { "es" } else { "" },
                    listed.join(", "),
                    if required.len() > 5 { ", …" } else { "" }
                ),
            ));
        }
    }
    if !input.has_test_framework || has_tests {
        return findings;
    }
    let sources: Vec<_> = input
        .files
        .iter()
        .filter(|f| f.is_source() && f.is_reviewable())
        .collect();
    let touched_lines: u32 = sources.iter().map(|f| f.additions + f.deletions).sum();
    // A whole-file rewrite is a real change however short the file is.
    let rewrite = sources.iter().any(|f| f.is_whole_rewrite());
    if sources.is_empty() || (touched_lines < MIN_SOURCE_LINES && !rewrite) {
        return findings;
    }
    if findings.iter().any(|f| f.rule == "tests.required") {
        // The stricter finding already asks for the same thing.
        return findings;
    }
    let first = sources[0];
    let listed: Vec<String> = sources
        .iter()
        .take(5)
        .map(|f| format!("`{}`", f.path))
        .collect();
    findings.push(Finding::new(
        "tests",
        "missing",
        Severity::Medium,
        &first.path,
        first.first_line(),
        "Source changed without tests",
        format!(
            "{} source file{} change{} ({touched_lines} lines: {}{}) and no test file is part of this pull request, \
             although the project has a test suite. Add or extend a test that fails without this change.",
            sources.len(),
            if sources.len() == 1 { "" } else { "s" },
            if sources.len() == 1 { "s" } else { "" },
            listed.join(", "),
            if sources.len() > 5 { ", …" } else { "" }
        ),
    ));
    findings
}

#[cfg(test)]
mod cases {
    use super::super::fixtures::*;
    use super::*;

    fn with_framework(files: Vec<crate::domain::review::input::ChangedFile>) -> ReviewInput {
        let mut input = input(files);
        input.has_test_framework = true;
        input
    }

    #[test]
    fn source_without_tests() {
        let findings = analyze(&with_framework(vec![added(
            "src/pricing.rs",
            &[
                "fn a() {}",
                "fn b() {}",
                "fn c() {}",
                "fn d() {}",
                "fn e() {}",
            ],
        )]));
        assert_eq!(rules(&findings), vec!["tests.missing"]);
        assert_eq!(findings[0].path, "src/pricing.rs");
        assert!(findings[0].detail.contains("1 source file changes"));
    }

    #[test]
    fn several_sources_count_together() {
        let findings = analyze(&with_framework(vec![
            modified("web/src/App.tsx", &["a", "b", "c"]),
            modified("web/src/api.ts", &["d", "e"]),
            modified("README.md", &["docs"]),
        ]));
        assert_eq!(rules(&findings), vec!["tests.missing"]);
        assert!(findings[0].detail.contains("2 source files change"));
    }

    #[test]
    fn python_module_without_test() {
        let findings = analyze(&with_framework(vec![added(
            "app/models.py",
            &[
                "class A: pass",
                "class B: pass",
                "class C: pass",
                "x = 1",
                "y = 2",
                "z = 3",
            ],
        )]));
        assert_eq!(rules(&findings), vec!["tests.missing"]);
    }

    #[test]
    fn tests_present_tiny_changes_or_no_framework_are_clean() {
        let lines = ["a", "b", "c", "d", "e", "f"];
        assert!(
            analyze(&with_framework(vec![
                modified("src/lib.rs", &lines),
                added("tests/lib_test.rs", &["#[test] fn t() {}"]),
            ]))
            .is_empty()
        );
        assert!(analyze(&with_framework(vec![modified("src/lib.rs", &["a", "b"])])).is_empty());
        assert!(
            analyze(&with_framework(vec![
                modified("docs/x.md", &lines),
                modified("Cargo.toml", &lines)
            ]))
            .is_empty()
        );
        assert!(analyze(&input(vec![modified("src/lib.rs", &lines)])).is_empty());
    }

    #[test]
    fn required_paths_need_tests_even_without_a_framework_and_renames_do_not() {
        let mut input = input(vec![modified("src/billing/invoice.rs", &["a"])]);
        input.config.require_tests_for = vec!["src/billing/**".into()];
        let findings = analyze(&input);
        assert_eq!(rules(&findings), vec!["tests.required"]);
        assert_eq!(findings[0].severity, Severity::High);
        let mut with_test = input.clone();
        with_test
            .files
            .push(added("tests/invoice.rs", &["#[test] fn t() {}"]));
        assert!(analyze(&with_test).is_empty());
        // A pure rename inside the required tree is not a change to test.
        let mut renamed = input.clone();
        renamed.files = vec![crate::domain::review::input::ChangedFile {
            path: "src/billing/new.rs".into(),
            previous_path: "src/billing/old.rs".into(),
            status: "renamed".into(),
            ..Default::default()
        }];
        renamed.has_test_framework = true;
        assert!(analyze(&renamed).is_empty());
    }
}

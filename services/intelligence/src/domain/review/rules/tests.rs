//! Source changed without tests, when the project has a test framework
//! to write them in.

use super::{Finding, Severity};
use crate::domain::review::input::ReviewInput;

/// Below this many changed source lines a missing test is noise, not a finding.
const MIN_SOURCE_LINES: u32 = 5;

pub fn analyze(input: &ReviewInput) -> Vec<Finding> {
    if !input.has_test_framework {
        return Vec::new();
    }
    let sources: Vec<_> = input
        .files
        .iter()
        .filter(|f| f.is_source() && !f.is_removed())
        .collect();
    let touched_lines: u32 = sources.iter().map(|f| f.additions + f.deletions).sum();
    if sources.is_empty() || touched_lines < MIN_SOURCE_LINES {
        return Vec::new();
    }
    if input.has_file(|f| f.is_test() && !f.is_removed()) {
        return Vec::new();
    }
    let first = sources[0];
    let listed: Vec<String> = sources
        .iter()
        .take(5)
        .map(|f| format!("`{}`", f.path))
        .collect();
    vec![Finding::new(
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
    )]
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
}

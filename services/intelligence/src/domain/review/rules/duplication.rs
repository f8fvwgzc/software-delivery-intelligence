//! Code pasted twice within one pull request: the same run of added lines
//! (whitespace-normalised, comments and braces ignored) appearing in two
//! places. Deterministic: the first occurrence is the original, every later
//! one is reported once, pointing back at it.

use std::collections::HashMap;

use super::{Finding, Severity};
use crate::domain::review::input::{ChangedFile, ReviewInput};

/// Meaningful lines that must repeat before it is a duplicate.
pub const WINDOW: usize = 6;

fn trivial(line: &str) -> bool {
    let t = line.trim();
    t.len() < 4
        || t.chars().all(|c| !c.is_alphanumeric())
        || t.starts_with("//")
        || t.starts_with('#')
        || t.starts_with("/*")
        || t.starts_with('*')
        || matches!(
            t,
            "end" | "else" | "else:" | "return" | "break" | "continue" | "pass"
        )
        || t.starts_with("import ")
        || t.starts_with("use ")
        || t.starts_with("from ")
}

fn normalise(line: &str) -> String {
    line.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `(line number, normalised text)` of every meaningful added line.
fn meaningful(file: &ChangedFile) -> Vec<(u32, String)> {
    file.added_lines()
        .into_iter()
        .filter(|(_, l)| !trivial(l))
        .map(|(n, l)| (n, normalise(l)))
        .collect()
}

/// A pasted block: where it is and where it was first added.
struct Copy {
    path: String,
    start: u32,
    end: u32,
    first_path: String,
    first_start: u32,
    first_end: u32,
}

pub fn analyze(input: &ReviewInput) -> Vec<Finding> {
    // First place each window was seen: (path, start line, end line).
    let mut seen: HashMap<String, (String, u32, u32)> = HashMap::new();
    let mut copies: Vec<Copy> = Vec::new();
    for file in input
        .files
        .iter()
        .filter(|f| f.is_reviewable() && f.is_source() && !f.is_test())
    {
        let lines = meaningful(file);
        if lines.len() < WINDOW {
            continue;
        }
        for window in lines.windows(WINDOW) {
            let key = window
                .iter()
                .map(|(_, l)| l.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            let (start, end) = (window[0].0, window[WINDOW - 1].0);
            match seen.get(&key) {
                Some((path, first_start, first_end)) => {
                    let same_block =
                        path == &file.path && start <= *first_end && end >= *first_start;
                    if same_block {
                        continue;
                    }
                    // Overlapping windows of one pasted block extend the
                    // same copy instead of reporting it again.
                    if let Some(last) = copies.last_mut().filter(|c| {
                        c.path == file.path && start <= c.end + 1 && c.first_path == *path
                    }) {
                        last.end = end;
                        last.first_end = last.first_end.max(*first_end);
                        continue;
                    }
                    copies.push(Copy {
                        path: file.path.clone(),
                        start,
                        end,
                        first_path: path.clone(),
                        first_start: *first_start,
                        first_end: *first_end,
                    });
                }
                None => {
                    seen.insert(key, (file.path.clone(), start, end));
                }
            }
        }
    }
    copies
        .into_iter()
        .map(|c| {
            let mut finding = Finding::new(
                "correctness",
                "duplicate_block",
                Severity::Low,
                &c.path,
                c.start,
                "Code duplicated within the change",
                format!(
                    "Lines {}–{} of `{}` repeat {WINDOW}+ lines first added at `{}` lines {}–{}. Extract the \
                     shared part into one function so a fix lands in one place.",
                    c.start, c.end, c.path, c.first_path, c.first_start, c.first_end
                ),
            );
            finding.end_line = c.end;
            finding
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::*;
    use super::*;

    fn block(prefix: &str) -> Vec<String> {
        (0..8)
            .map(|i| format!("{prefix}let value_{i} = compute(input, {i}) + offset;"))
            .collect()
    }

    #[test]
    fn repeated_blocks_are_reported_once_per_copy() {
        let a: Vec<String> = block("");
        let b: Vec<String> = block("    ");
        let a_ref: Vec<&str> = a.iter().map(String::as_str).collect();
        let b_ref: Vec<&str> = b.iter().map(String::as_str).collect();
        let mut second = vec!["fn other() {", "// a comment"];
        second.extend(b_ref.iter().copied());
        second.push("}");
        let findings = analyze(&input(vec![
            added("src/one.rs", &a_ref),
            added("src/two.rs", &second),
            added("tests/dup.rs", &a_ref),
        ]));
        assert_eq!(findings.len(), 1, "{findings:?}");
        let f = &findings[0];
        assert_eq!(f.rule, "correctness.duplicate_block");
        assert_eq!((f.path.as_str(), f.line, f.end_line), ("src/two.rs", 3, 10));
        assert!(f.detail.contains("`src/one.rs` lines 1–8"));
    }

    #[test]
    fn short_or_distinct_code_is_not_duplication() {
        let short: Vec<String> = (0..4).map(|i| format!("let a{i} = {i};")).collect();
        let short: Vec<&str> = short.iter().map(String::as_str).collect();
        let distinct: Vec<String> = (0..10).map(|i| format!("let b{i} = f({i});")).collect();
        let distinct: Vec<&str> = distinct.iter().map(String::as_str).collect();
        let findings = analyze(&input(vec![
            added("src/a.rs", &short),
            added("src/b.rs", &short),
            added("src/c.rs", &distinct),
        ]));
        assert!(findings.is_empty(), "{findings:?}");
    }
}

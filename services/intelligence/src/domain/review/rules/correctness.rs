//! Markers of unfinished or fragile code in added lines: TODO/FIXME,
//! `unwrap`/`expect`/`!` outside tests, swallowed errors, and debug output
//! left behind.

use std::sync::LazyLock;

use regex::Regex;

use super::{Finding, Severity, evidence};
use crate::domain::review::input::{ChangedFile, ReviewInput};

static TODO: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|[^\w])(?:TODO|FIXME|XXX|HACK)\b").expect("todo"));
static UNWRAP_RS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\.(?:unwrap|expect)\(").expect("unwrap"));
/// `foo!.bar`, `foo()!.bar`, `x!;` — never `!=`, `!==`, `!x`, macros `name!(`.
static NON_NULL_TS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\w)\]]!(?:\.|\)|;|,|\s*\]|\s*\})").expect("non-null"));
static SWALLOWED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"catch\s*(?:\([^)]*\))?\s*\{\s*\}|\.catch\(\s*\(\s*\)\s*=>\s*\{\s*\}\s*\)|\.catch\(\s*(?:_|\w+)\s*=>\s*\{\s*\}\s*\)|except(?:\s+\w+(?:\s+as\s+\w+)?)?\s*:\s*pass\b|except\s*:\s*$|if let Err\(_\)\s*=\s*[^{]+\{\s*\}|let _ = .*\?;|rescue\s*(?:=>\s*\w+)?\s*$|\.ok\(\);\s*$|\.unwrap_or_default\(\);\s*$",
    )
    .expect("swallowed")
});
static DEBUG_OUTPUT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|[^\w.])console\.log\(|\bdbg!\(|^\s*print\(|\bbinding\.pry\b|\bdebugger;?\s*$")
        .expect("debug")
});

fn is_comment(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("//") || t.starts_with('#') || t.starts_with('*') || t.starts_with("/*")
}

fn is_script(file: &ChangedFile) -> bool {
    file.path.starts_with("scripts/")
        || file.path.contains("/scripts/")
        || file.path.starts_with("bin/")
}

pub fn analyze(input: &ReviewInput) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in input
        .files
        .iter()
        .filter(|f| !f.is_removed() && !f.is_docs() && !f.is_lockfile())
    {
        let path = &file.path;
        let ext = file.extension();
        let rust = ext == "rs";
        let ts = matches!(ext, "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs");
        let py = ext == "py";
        let code = rust || ts || py || matches!(ext, "go" | "rb" | "java" | "kt" | "cs" | "php");
        let test = file.is_test();
        for (line_no, line) in file.added_lines() {
            if TODO.is_match(line)
                && (is_comment(line)
                    || line.contains("//")
                    || line.contains('#')
                    || line.contains("/*"))
            {
                findings.push(Finding::new(
                    "correctness",
                    "todo",
                    Severity::Info,
                    path,
                    line_no,
                    "Work left for later",
                    format!(
                        "Line {line_no} adds a TODO/FIXME marker: {}. Say what is missing and link an issue, or finish it \
                         before merging.",
                        evidence(line)
                    ),
                ));
            }
            if is_comment(line) || !code {
                continue;
            }
            if !test && rust && UNWRAP_RS.is_match(line) && !line.contains("#[test]") {
                findings.push(Finding::new(
                    "correctness",
                    "unwrap",
                    Severity::Low,
                    path,
                    line_no,
                    "unwrap/expect outside tests",
                    format!(
                        "Line {line_no} panics on an error or a missing value: {}. Propagate it with `?` or handle the \
                         `None`/`Err` case; a panic takes the whole task down.",
                        evidence(line)
                    ),
                ));
            }
            if !test && ts && NON_NULL_TS.is_match(line) && !line.contains("!=") {
                findings.push(Finding::new(
                    "correctness",
                    "non_null_assertion",
                    Severity::Low,
                    path,
                    line_no,
                    "Non-null assertion outside tests",
                    format!(
                        "Line {line_no} tells the type checker a value is never null: {}. If it ever is, this throws at \
                         run time; narrow the type with a check or `?.` instead.",
                        evidence(line)
                    ),
                ));
            }
            if SWALLOWED.is_match(line) {
                findings.push(Finding::new(
                    "correctness",
                    "swallowed_error",
                    Severity::Medium,
                    path,
                    line_no,
                    "Error swallowed silently",
                    format!(
                        "Line {line_no} catches or discards an error without logging or handling it: {}. At least log it \
                         with context; silent failures are the hardest to diagnose.",
                        evidence(line)
                    ),
                ));
            }
            if !test
                && !is_script(file)
                && DEBUG_OUTPUT.is_match(line)
                && !(py && !line.trim_start().starts_with("print("))
            {
                findings.push(Finding::new(
                    "correctness",
                    "debug_output",
                    Severity::Low,
                    path,
                    line_no,
                    "Debug output left in",
                    format!(
                        "Line {line_no} prints to the console: {}. Use the project's logger (with a level) or remove it \
                         before merging.",
                        evidence(line)
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
    fn todo_unwrap_and_non_null() {
        let findings = analyze(&input(vec![
            added(
                "src/main.rs",
                &[
                    "// TODO: handle the error",
                    "let n = parse(input).unwrap();",
                ],
            ),
            added("web/src/user.ts", &["const name = user!.name;"]),
        ]));
        assert_eq!(
            rules(&findings),
            vec![
                "correctness.todo",
                "correctness.unwrap",
                "correctness.non_null_assertion"
            ]
        );
        assert_eq!(findings[0].severity, Severity::Info);
    }

    #[test]
    fn swallowed_errors_in_three_languages() {
        let findings = analyze(&input(vec![
            added("web/src/save.ts", &["try { save() } catch (e) {}"]),
            added("app/job.py", &["except Exception: pass"]),
            added("src/job.rs", &["    let _ = cleanup().await?;"]),
        ]));
        assert_eq!(
            rules(&findings),
            vec![
                "correctness.swallowed_error",
                "correctness.swallowed_error",
                "correctness.swallowed_error"
            ]
        );
        assert!(findings.iter().all(|f| f.severity == Severity::Medium));
    }

    #[test]
    fn debug_output() {
        let findings = analyze(&input(vec![
            added("web/src/api.ts", &["console.log(response);"]),
            added("src/lib.rs", &["dbg!(&value);"]),
            added("app/views.py", &["print(request.body)"]),
        ]));
        assert_eq!(
            rules(&findings),
            vec![
                "correctness.debug_output",
                "correctness.debug_output",
                "correctness.debug_output"
            ]
        );
    }

    #[test]
    fn tests_scripts_and_handled_errors_are_clean() {
        let findings = analyze(&input(vec![
            added(
                "src/tests/parse.rs",
                &["let n = parse(\"1\").unwrap();", "console.log('x')"],
            ),
            added(
                "scripts/seed.ts",
                &["console.log('seeding');", "if (a != b) {}"],
            ),
            added(
                "src/main.rs",
                &[
                    "let n = parse(input)?;",
                    "if let Err(error) = cleanup().await { tracing::warn!(%error, \"cleanup\"); }",
                    "assert!(ok != done);",
                ],
            ),
            added(
                "web/src/user.ts",
                &["const name = user?.name ?? '';", "if (a !== b) {}"],
            ),
            added("README.md", &["TODO: write docs"]),
        ]));
        assert!(findings.is_empty(), "{findings:?}");
    }
}

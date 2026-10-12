//! Markers of unfinished or fragile code in added lines: TODO/FIXME,
//! `unwrap`/`expect`/`!` outside tests, swallowed errors, debug output
//! left behind, promises and results whose failure nobody handles, and
//! functions too large to review in one sitting.

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

/// A `.then(` chain or a bare `fetch`/`axios` call whose rejection nothing
/// catches on the same or the following lines.
static THEN_CHAIN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\.then\s*\(").expect("then"));
static HANDLED_PROMISE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\.catch\s*\(|\.finally\s*\(|\bawait\b|\breturn\b|\bvoid\b").expect("handled")
});
static FLOATING_CALL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(?:fetch|axios(?:\.\w+)?|\w+\.(?:save|remove|delete|update|insert|send|write|publish|commit|flush|close|disconnect|connect)(?:Async)?)\s*\([^;]*\)\s*;?\s*$")
        .expect("floating")
});
/// Rust: an awaited call whose value is dropped on the floor.
static DROPPED_AWAIT_RS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*[\w:]+(?:\.[\w]+(?:\([^;]*\))?|\([^;]*\))+\.await\s*;\s*$")
        .expect("dropped await")
});
/// Go: an error return thrown away.
static DROPPED_ERR_GO: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(?:_\s*(?:,\s*_\s*)*[:=]+\s*)?\w[\w.]*\.(?:Close|Write|WriteString|Flush|Sync|Exec|ExecContext|Rollback|Commit|Send|Shutdown|Remove|RemoveAll|Unmarshal|Decode|Encode|ListenAndServe|Scan|Set|Del)\([^;]*\)\s*$")
        .expect("dropped err")
});

/// Function headers per language, for the size check.
static FN_HEADER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?P<indent>\s*)(?:(?:pub(?:\([^)]*\))?\s+)?(?:async\s+|unsafe\s+|const\s+)*fn\s+\w+|(?:export\s+)?(?:default\s+)?(?:async\s+)?function\s*\*?\s*\w*\s*\(|(?:export\s+)?(?:const|let|var)\s+\w+\s*(?::[^=]+)?=\s*(?:async\s*)?(?:\([^)]*\)|\w+)\s*(?::\s*[^=]+)?=>|(?:async\s+)?def\s+\w+\s*\(|func\s+(?:\([^)]*\)\s*)?\w+\s*\(|(?:public|private|protected|internal|static|override|final|async|virtual|synchronized|abstract)\s+(?:[\w<>\[\],?.]+\s+)+\w+\s*\([^;{]*\)\s*(?:throws\s+[\w,\s]+)?\{?\s*$|(?:public|private|protected|static|async|\s)*\w+\s*\([^;{)]*\)\s*(?::\s*[\w<>\[\]|,\s.]+)?\s*\{\s*$)",
    )
    .expect("fn header")
});

/// Added lines in one function before it is too large to review.
pub const LARGE_FUNCTION_LINES: usize = 120;

fn is_comment(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("//") || t.starts_with('#') || t.starts_with('*') || t.starts_with("/*")
}

/// Functions whose body gains more than [`LARGE_FUNCTION_LINES`] lines in
/// one go: a header starts a block, which runs over the consecutive added
/// lines until the next header at the same or a shallower indentation.
fn large_functions(file: &ChangedFile) -> Vec<(u32, u32, usize, String)> {
    let mut out = Vec::new();
    let mut current: Option<(u32, usize, usize, String)> = None; // (start, indent, count, name)
    let mut last_line: u32 = 0;
    let mut last_end: u32 = 0;
    let close = |current: &mut Option<(u32, usize, usize, String)>,
                 end: u32,
                 out: &mut Vec<(u32, u32, usize, String)>| {
        if let Some((start, _, count, name)) = current.take()
            && count > LARGE_FUNCTION_LINES
        {
            out.push((start, end, count, name));
        }
    };
    for (line_no, line) in file.added_lines() {
        let contiguous = last_line > 0 && line_no == last_line + 1;
        if !contiguous {
            close(&mut current, last_end, &mut out);
        }
        let indent = line.len() - line.trim_start().len();
        if let Some(caps) = FN_HEADER.captures(line)
            && !is_comment(line)
            && current.as_ref().is_none_or(|(_, i, _, _)| indent <= *i)
        {
            close(&mut current, last_end, &mut out);
            let name = caps
                .get(0)
                .map(|m| m.as_str().trim().chars().take(60).collect::<String>())
                .unwrap_or_default();
            current = Some((line_no, indent, 0, name));
        } else if let Some(block) = current.as_mut()
            && !line.trim().is_empty()
        {
            block.2 += 1;
        }
        last_line = line_no;
        last_end = line_no;
    }
    close(&mut current, last_end, &mut out);
    out
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
        let go = ext == "go";
        if !test && code {
            for (start, end, count, name) in large_functions(file) {
                let mut finding = Finding::new(
                    "correctness",
                    "large_function",
                    Severity::Medium,
                    path,
                    start,
                    "Function too large to review",
                    format!(
                        "`{name}` at line {start} gains {count} lines in this change (lines {start}–{end}); above \
                         {LARGE_FUNCTION_LINES} a reviewer cannot hold it in their head. Split it into named steps \
                         with their own tests.",
                    ),
                );
                finding.end_line = end;
                findings.push(finding);
            }
        }
        let added = file.added_lines();
        for (index, (line_no, line)) in added.iter().copied().enumerate() {
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
            if !test && ts {
                // The chain may continue on the next lines (`  .catch(...)`).
                let next_lines = added
                    .iter()
                    .skip(index + 1)
                    .take(4)
                    .take_while(|(n, l)| *n <= line_no + 4 && l.trim_start().starts_with('.'))
                    .any(|(_, l)| l.contains(".catch(") || l.contains(".finally("));
                let unhandled_chain =
                    THEN_CHAIN.is_match(line) && !HANDLED_PROMISE.is_match(line) && !next_lines;
                let floating = FLOATING_CALL.is_match(line)
                    && !HANDLED_PROMISE.is_match(line)
                    && !THEN_CHAIN.is_match(line);
                if unhandled_chain || floating {
                    findings.push(Finding::new(
                        "correctness",
                        "unhandled_promise",
                        Severity::Medium,
                        path,
                        line_no,
                        "Promise rejection not handled",
                        format!(
                            "Line {line_no} starts an asynchronous call whose failure nothing catches: {}. `await` it \
                             inside a `try`, add `.catch(...)`, or return it so the caller decides.",
                            evidence(line)
                        ),
                    ));
                }
            }
            if !test && rust && DROPPED_AWAIT_RS.is_match(line) {
                findings.push(Finding::new(
                    "correctness",
                    "unhandled_result",
                    Severity::Low,
                    path,
                    line_no,
                    "Awaited value dropped",
                    format!(
                        "Line {line_no} awaits a call and discards what it returns: {}. If that is a `Result`, the \
                         error is lost; propagate it with `?` or handle the `Err` case.",
                        evidence(line)
                    ),
                ));
            }
            if !test && go && DROPPED_ERR_GO.is_match(line) {
                findings.push(Finding::new(
                    "correctness",
                    "unhandled_result",
                    Severity::Medium,
                    path,
                    line_no,
                    "Error return ignored",
                    format!(
                        "Line {line_no} calls something that returns an error and does not check it: {}. Assign and \
                         handle `err`, or at least log it.",
                        evidence(line)
                    ),
                ));
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

    #[test]
    fn unhandled_promises_and_results() {
        let findings = analyze(&input(vec![
            added(
                "src/api.ts",
                &[
                    "fetch(url).then(r => r.json());",
                    "fetch(url).then(r => r.json())",
                    "  .catch(err => log(err));",
                    "fetch(url);",
                    "const data = await fetch(url);",
                    "return client.get(url).then(x => x);",
                    "repo.save(entity);",
                ],
            ),
            added(
                "src/worker.rs",
                &[
                    "client.send(msg).await;",
                    "let reply = client.send(msg).await?;",
                    "tokio::time::sleep(d).await;",
                ],
            ),
            added(
                "pkg/store.go",
                &[
                    "f.Close()",
                    "defer f.Close()",
                    "_ = db.Exec(q)",
                    "if err := db.Exec(q); err != nil {",
                ],
            ),
            added("src/api.test.ts", &["fetch(url);"]),
        ]));
        let hits: Vec<(&str, &str, u32)> = findings
            .iter()
            .filter(|f| {
                matches!(
                    f.rule.as_str(),
                    "correctness.unhandled_promise" | "correctness.unhandled_result"
                )
            })
            .map(|f| (f.rule.as_str(), f.path.as_str(), f.line))
            .collect();
        assert_eq!(
            hits,
            vec![
                ("correctness.unhandled_promise", "src/api.ts", 1),
                ("correctness.unhandled_promise", "src/api.ts", 4),
                ("correctness.unhandled_promise", "src/api.ts", 7),
                ("correctness.unhandled_result", "src/worker.rs", 1),
                ("correctness.unhandled_result", "src/worker.rs", 3),
                ("correctness.unhandled_result", "pkg/store.go", 1),
                ("correctness.unhandled_result", "pkg/store.go", 3),
            ],
            "{findings:?}"
        );
    }

    #[test]
    fn large_functions_are_measured_in_added_lines() {
        let mut big = vec!["pub fn load_everything(cfg: &Config) -> Result<State> {".to_owned()];
        big.extend((0..130).map(|i| format!("    let step_{i} = run(cfg, {i})?;")));
        big.push("    Ok(State::default())".into());
        big.push("}".into());
        big.push("fn small() {}".into());
        let big_ref: Vec<&str> = big.iter().map(String::as_str).collect();
        let mut ts = vec!["export const handler = async (req: Request) => {".to_owned()];
        ts.extend((0..50).map(|i| format!("  const v{i} = await step({i});")));
        ts.push("};".into());
        let ts_ref: Vec<&str> = ts.iter().map(String::as_str).collect();
        let findings = analyze(&input(vec![
            added("src/state.rs", &big_ref),
            added("src/handler.ts", &ts_ref),
            added("tests/big_test.rs", &big_ref),
        ]));
        let large: Vec<&Finding> = findings
            .iter()
            .filter(|f| f.rule == "correctness.large_function")
            .collect();
        assert_eq!(large.len(), 1, "{findings:?}");
        assert_eq!((large[0].path.as_str(), large[0].line), ("src/state.rs", 1));
        assert_eq!(large[0].end_line, 133);
        assert!(large[0].detail.contains("gains 132 lines"));
        assert!(large[0].detail.contains("load_everything"));
    }
}

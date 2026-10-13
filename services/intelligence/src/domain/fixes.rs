//! Fixes (CONTRACT.md §23.2), the pure half: which findings the model may
//! be asked about, the excerpt one request carries (bounded and redacted),
//! and how accepted suggestions are written into a file. Nothing here
//! talks to a model, GitHub or a database.

use std::cmp::Reverse;

use uuid::Uuid;

use crate::{
    db::reviews::FindingRow,
    domain::review::rules::secrets::line_may_hold_secret,
    reasoner::fixes::{FixRequest, MAX_DETAIL_CHARS, REDACTED},
};

/// Context lines either side of the flagged ones, at most.
pub const CONTEXT_LINES: usize = 20;
/// Source lines one request carries, flagged ones included, at most.
pub const MAX_REQUEST_LINES: usize = 120;
/// A context line longer than this is cut: it is there for orientation.
pub const MAX_CONTEXT_LINE_CHARS: usize = 240;
/// The flagged lines are sent whole or not at all; past this many
/// characters they are a blob (minified code, data), not something to patch.
pub const MAX_FLAGGED_CHARS: usize = 4_000;
/// Lines of a commit message that list what was fixed, at most.
const MAX_MESSAGE_ENTRIES: usize = 50;

/// What a draft is refused with when the finding has a replacement.
pub const ALREADY_SUGGESTED: &str = "this finding already has a suggestion";

/// Why the model cannot be asked to fix this finding; `None` when it can.
/// The order is the contract's: a suggestion that exists, no line, a file
/// whose lines cannot be replaced, a secret, a workflow file.
pub fn draft_blocker(finding: &FindingRow) -> Option<&'static str> {
    // An example from the analyzer is not a fix: the model may still be
    // asked for the real replacement, which then takes the example's place.
    if !finding.suggestion.trim().is_empty()
        && crate::db::reviews::source_applies(&finding.suggestion_source)
    {
        return Some(ALREADY_SUGGESTED);
    }
    if finding.line <= 0 || finding.path.trim().is_empty() {
        return Some("this finding does not point at a line, so there is nothing to replace");
    }
    if !finding.patchable {
        return Some(
            "the file is binary, removed or too large to read in this pull request, so its lines cannot be replaced",
        );
    }
    if finding.category == "secrets" || finding.rule.starts_with("secrets.") {
        return Some(
            "findings about secrets are never sent to a model: remove the secret and rotate it by hand",
        );
    }
    if is_workflow_path(&finding.path) {
        return Some(
            "files under .github/workflows/ are not rewritten by a model: they run with the repository's credentials",
        );
    }
    None
}

/// `Finding.can_draft_fix` (CONTRACT.md §23.2).
pub fn can_draft_fix(finding: &FindingRow) -> bool {
    draft_blocker(finding).is_none()
}

/// Whether a review drafts this finding's fix on its own
/// (`auto_draft_fixes`): what a person may ask for, minus the findings that
/// are about the pull request as a whole. `tests.*` and `maturity.*` point
/// at the first changed line of a file only to have somewhere to stand;
/// no replacement of that line fixes "there are no tests", so asking would
/// spend tokens (and one of the review's ten drafts) on nothing. A finding
/// that already carries the analyzer's example is left alone too: the
/// example usually means the fix is something to add or to fill in, which
/// a person is better placed to ask for than a review is to guess.
pub fn drafted_unasked(finding: &FindingRow) -> bool {
    can_draft_fix(finding)
        && finding.suggestion.trim().is_empty()
        && !matches!(finding.category.as_str(), "tests" | "maturity")
}

pub fn is_workflow_path(path: &str) -> bool {
    path.trim_start_matches("./")
        .starts_with(".github/workflows/")
}

/// The lines of a file as people count them: split on `\n`, `\r` dropped,
/// and no phantom last line after a trailing newline.
pub fn file_lines(content: &str) -> Vec<&str> {
    if content.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<&str> = content
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    if content.ends_with('\n') {
        lines.pop();
    }
    lines
}

/// Why no excerpt can be sent for a finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExcerptError {
    /// The finding points past the end of the file at the head commit.
    BeyondEnd { line: u32, lines: usize },
    /// More flagged lines than one request may carry.
    TooManyLines(usize),
    /// The flagged lines are too long to send whole.
    TooLong(usize),
    /// A flagged line looks like it holds a secret: it would have to be
    /// hidden, and a hidden line cannot be rewritten.
    Secret,
}

impl ExcerptError {
    pub fn sentence(&self) -> String {
        match self {
            Self::BeyondEnd { line, lines } => format!(
                "the finding points at line {line} but the file has {lines} lines at the reviewed commit"
            ),
            Self::TooManyLines(count) => format!(
                "the finding spans {count} lines; at most {MAX_REQUEST_LINES} lines are sent to a model"
            ),
            Self::TooLong(chars) => format!(
                "the flagged lines are {chars} characters long; at most {MAX_FLAGGED_CHARS} are sent to a model"
            ),
            Self::Secret => {
                "a flagged line looks like it holds a secret, so nothing was sent to the model"
                    .to_owned()
            }
        }
    }
}

fn cut(line: &str, max: usize) -> String {
    if line.chars().count() <= max {
        return line.to_owned();
    }
    let mut out: String = line.chars().take(max).collect();
    out.push('…');
    out
}

fn context_line(path: &str, line: &str) -> String {
    if line_may_hold_secret(path, line) {
        REDACTED.to_owned()
    } else {
        cut(line, MAX_CONTEXT_LINE_CHARS)
    }
}

/// The finding's explanation as the request carries it: bounded, and with
/// any line that looks like a secret hidden like the excerpt's.
fn request_detail(path: &str, detail: &str) -> String {
    let redacted = detail
        .lines()
        .map(|l| {
            if line_may_hold_secret(path, l) {
                REDACTED
            } else {
                l
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    cut(redacted.trim(), MAX_DETAIL_CHARS)
}

/// The request for one finding over the file's content at the head commit:
/// the flagged lines verbatim, at most [`CONTEXT_LINES`] either side, at
/// most [`MAX_REQUEST_LINES`] in total, every context line a secrets rule
/// could match replaced by `<redacted>`.
pub fn excerpt(
    finding: &FindingRow,
    language: &str,
    content: &str,
) -> Result<FixRequest, ExcerptError> {
    let lines = file_lines(content);
    let start = usize::try_from(finding.line.max(1)).unwrap_or(1);
    let end = usize::try_from(finding.end_line.max(finding.line).max(1)).unwrap_or(start);
    if end > lines.len() {
        return Err(ExcerptError::BeyondEnd {
            line: end as u32,
            lines: lines.len(),
        });
    }
    let flagged = &lines[start - 1..end];
    if flagged.len() > MAX_REQUEST_LINES {
        return Err(ExcerptError::TooManyLines(flagged.len()));
    }
    let chars: usize = flagged.iter().map(|l| l.chars().count()).sum();
    if chars > MAX_FLAGGED_CHARS {
        return Err(ExcerptError::TooLong(chars));
    }
    if flagged
        .iter()
        .any(|l| line_may_hold_secret(&finding.path, l))
    {
        return Err(ExcerptError::Secret);
    }
    let room = MAX_REQUEST_LINES - flagged.len();
    let before = CONTEXT_LINES.min(room / 2).min(start - 1);
    let after = CONTEXT_LINES.min(room - before).min(lines.len() - end);
    Ok(FixRequest {
        rule: finding.rule.clone(),
        title: cut(finding.title.trim(), 160),
        detail: request_detail(&finding.path, &finding.detail),
        path: finding.path.clone(),
        language: language.to_owned(),
        start_line: start as u32,
        end_line: end as u32,
        before: lines[start - 1 - before..start - 1]
            .iter()
            .map(|l| context_line(&finding.path, l))
            .collect(),
        flagged: flagged.iter().map(|l| (*l).to_owned()).collect(),
        after: lines[end..end + after]
            .iter()
            .map(|l| context_line(&finding.path, l))
            .collect(),
    })
}

/// A language id for the request when knowledge did not name one.
pub fn language_of(path: &str) -> &'static str {
    let name = path.rsplit('/').next().unwrap_or(path);
    if name == "Dockerfile" || name.starts_with("Dockerfile.") {
        return "dockerfile";
    }
    match name.rsplit_once('.').map(|(_, e)| e).unwrap_or("") {
        "rs" => "rust",
        "ts" | "tsx" => "typescript",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "py" => "python",
        "go" => "go",
        "java" => "java",
        "kt" => "kotlin",
        "rb" => "ruby",
        "php" => "php",
        "cs" => "csharp",
        "swift" => "swift",
        "c" | "h" => "c",
        "cc" | "cpp" | "hpp" => "cpp",
        "sh" | "bash" => "shell",
        "yml" | "yaml" => "yaml",
        "json" => "json",
        "toml" => "toml",
        "tf" => "hcl",
        "sql" => "sql",
        "md" => "markdown",
        _ => "",
    }
}

// --- Applying accepted suggestions ---------------------------------------------

fn severity_rank(severity: &str) -> u8 {
    match severity {
        "critical" => 4,
        "high" => 3,
        "medium" => 2,
        "low" => 1,
        _ => 0,
    }
}

/// Why an accepted suggestion is not part of the commit. It keeps no
/// `fix_execution_id`, so it stays available for a later one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeftOut {
    /// Its lines overlap a more severe (or, at equal severity, earlier)
    /// finding's in the same file.
    Overlaps { winner: Uuid },
    /// Its target lines are beyond the file's end at the head commit.
    BeyondEnd { lines: usize },
    /// The file already reads as the suggestion says.
    AlreadyApplied,
}

impl LeftOut {
    pub fn sentence(&self) -> String {
        match self {
            Self::Overlaps { .. } => "overlaps a more severe fix in the same file".to_owned(),
            Self::BeyondEnd { lines } => {
                format!("points past the end of the file ({lines} lines at the head commit)")
            }
            Self::AlreadyApplied => "the file already reads as suggested".to_owned(),
        }
    }
}

/// One file after its suggestions were written into it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePatch {
    /// The whole new content; equal to the input when nothing applied.
    pub content: String,
    /// Findings whose suggestion is in `content`, top of the file first.
    pub applied: Vec<Uuid>,
    pub left_out: Vec<(Uuid, LeftOut)>,
}

fn suggestion_lines(suggestion: &str) -> Vec<&str> {
    suggestion
        .trim_end_matches(['\n', '\r'])
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect()
}

/// Writes the suggestions of `findings` (all of one file) into `content`.
/// Each replaces lines `line..=end_line`. Overlaps go to the more severe
/// finding (ties: the one the review listed first); what does not fit is
/// left out with its reason. The replacements are applied from the bottom
/// of the file up, so line numbers above them never shift. The file's
/// line ending and its trailing newline (or lack of one) are kept.
pub fn patch_file(content: &str, findings: &[&FindingRow]) -> FilePatch {
    let crlf = content.contains("\r\n");
    let trailing_newline = content.ends_with('\n');
    let mut lines: Vec<String> = file_lines(content).into_iter().map(str::to_owned).collect();
    let total = lines.len();

    let mut ranked: Vec<&FindingRow> = findings.to_vec();
    ranked.sort_by_key(|f| (Reverse(severity_rank(&f.severity)), f.position, f.id));

    let mut chosen: Vec<&FindingRow> = Vec::new();
    let mut left_out: Vec<(Uuid, LeftOut)> = Vec::new();
    for finding in ranked {
        let start = usize::try_from(finding.line).unwrap_or(0);
        let end = usize::try_from(finding.end_line.max(finding.line)).unwrap_or(0);
        if start == 0 || end > total {
            left_out.push((finding.id, LeftOut::BeyondEnd { lines: total }));
            continue;
        }
        let replacement = suggestion_lines(&finding.suggestion);
        if lines[start - 1..end]
            .iter()
            .map(String::as_str)
            .eq(replacement.iter().copied())
        {
            left_out.push((finding.id, LeftOut::AlreadyApplied));
            continue;
        }
        let clash = chosen.iter().find(|other| {
            let other_start = other.line as usize;
            let other_end = other.end_line.max(other.line) as usize;
            start <= other_end && other_start <= end
        });
        match clash {
            Some(winner) => left_out.push((finding.id, LeftOut::Overlaps { winner: winner.id })),
            None => chosen.push(finding),
        }
    }

    // Bottom-up: a replacement never moves the lines of one still to come.
    chosen.sort_by_key(|f| Reverse(f.line));
    for finding in &chosen {
        let start = finding.line as usize;
        let end = finding.end_line.max(finding.line) as usize;
        lines.splice(
            start - 1..end,
            suggestion_lines(&finding.suggestion)
                .into_iter()
                .map(str::to_owned),
        );
    }
    chosen.reverse();

    let newline = if crlf { "\r\n" } else { "\n" };
    let mut patched = lines.join(newline);
    if trailing_newline && !lines.is_empty() {
        patched.push_str(newline);
    }
    FilePatch {
        content: if chosen.is_empty() {
            content.to_owned()
        } else {
            patched
        },
        applied: chosen.iter().map(|f| f.id).collect(),
        left_out,
    }
}

/// `path:12` or `path:12-14`.
pub fn location(finding: &FindingRow) -> String {
    if finding.end_line > finding.line {
        format!("{}:{}-{}", finding.path, finding.line, finding.end_line)
    } else {
        format!("{}:{}", finding.path, finding.line)
    }
}

/// "Apply 3 review fixes to #7" (CONTRACT.md §23.2).
pub fn fix_title(count: usize, pr_number: i64) -> String {
    format!(
        "Apply {count} review {} to #{pr_number}",
        if count == 1 { "fix" } else { "fixes" }
    )
}

/// The commit message: the title, then the rule and `path:line` of every
/// fix in the commit, then where they came from.
pub fn commit_message(
    applied: &[&FindingRow],
    pr_number: i64,
    review_id: Uuid,
    head_sha: &str,
) -> String {
    let mut message = format!("{}\n\n", fix_title(applied.len(), pr_number));
    for finding in applied.iter().take(MAX_MESSAGE_ENTRIES) {
        message.push_str(&format!("- {} {}\n", finding.rule, location(finding)));
    }
    if applied.len() > MAX_MESSAGE_ENTRIES {
        message.push_str(&format!(
            "- and {} more\n",
            applied.len() - MAX_MESSAGE_ENTRIES
        ));
    }
    let short: String = head_sha.chars().take(12).collect();
    message.push_str(&format!(
        "\nAccepted in SDI review {review_id} of {short}.\n"
    ));
    message
}

#[cfg(test)]
pub(crate) mod fixtures {
    use super::*;

    /// A pending finding with no suggestion on one line of a source file.
    pub fn finding(path: &str, line: i32, end_line: i32) -> FindingRow {
        FindingRow {
            id: Uuid::now_v7(),
            workspace_id: Uuid::nil(),
            review_id: Uuid::nil(),
            position: 0,
            category: "correctness".into(),
            severity: "medium".into(),
            title: "unwrap() in non-test code".into(),
            detail: "Calls `unwrap()` on a `Result`.".into(),
            path: path.into(),
            line,
            end_line,
            suggestion: String::new(),
            rule: "correctness.unwrap".into(),
            decision: "pending".into(),
            decision_note: String::new(),
            decided_by: String::new(),
            decided_at: None,
            suggestion_source: String::new(),
            fix_execution_id: String::new(),
            patchable: true,
        }
    }

    /// An accepted finding that carries `suggestion`.
    pub fn accepted(
        path: &str,
        line: i32,
        end_line: i32,
        severity: &str,
        suggestion: &str,
    ) -> FindingRow {
        FindingRow {
            severity: severity.into(),
            suggestion: suggestion.into(),
            suggestion_source: "rule".into(),
            decision: "accepted".into(),
            ..finding(path, line, end_line)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{fixtures::*, *};

    #[test]
    fn can_draft_fix_follows_the_contract() {
        let base = finding("src/a.rs", 3, 3);
        assert!(can_draft_fix(&base));
        let with = |edit: fn(&mut FindingRow)| {
            let mut f = finding("src/a.rs", 3, 3);
            edit(&mut f);
            f
        };
        let cases: [(FindingRow, &str); 8] = [
            (
                with(|f| {
                    f.suggestion = "x".into();
                    f.suggestion_source = "rule".into();
                }),
                "already has a suggestion",
            ),
            (
                with(|f| {
                    f.suggestion = "x".into();
                    f.suggestion_source = "model".into();
                }),
                "already has a suggestion",
            ),
            (with(|f| f.line = 0), "does not point at a line"),
            (with(|f| f.path = String::new()), "does not point at a line"),
            (
                with(|f| f.patchable = false),
                "binary, removed or too large",
            ),
            (with(|f| f.category = "secrets".into()), "secrets"),
            (with(|f| f.rule = "secrets.aws_key".into()), "secrets"),
            (
                with(|f| f.path = ".github/workflows/ci.yml".into()),
                ".github/workflows/",
            ),
        ];
        for (finding, needle) in cases {
            assert!(!can_draft_fix(&finding));
            let reason = draft_blocker(&finding).unwrap();
            assert!(reason.contains(needle), "{reason}");
        }
        // An example from the analyzer is not a fix: the model may still
        // be asked for the replacement.
        assert!(can_draft_fix(&with(|f| {
            f.suggestion = "FROM node:<version>".into();
            f.suggestion_source = "example".into();
        })));
        // A decision does not change it: an accepted finding without a
        // suggestion can still get one.
        assert!(can_draft_fix(&with(|f| f.decision = "accepted".into())));
        // Other files under .github are fair game.
        assert!(can_draft_fix(&with(
            |f| f.path = ".github/CODEOWNERS".into()
        )));
        // A review does not draft, unasked, what no line replacement can
        // fix; a person may still ask.
        for category in ["tests", "maturity"] {
            let mut whole = finding("src/a.rs", 1, 1);
            whole.category = category.into();
            whole.rule = format!("{category}.missing");
            assert!(can_draft_fix(&whole));
            assert!(!drafted_unasked(&whole), "{category}");
        }
        assert!(drafted_unasked(&base));
        assert!(!drafted_unasked(&with(|f| f.suggestion = "x".into())));
    }

    fn numbered(count: usize) -> String {
        (1..=count).map(|i| format!("line {i}\n")).collect()
    }

    #[test]
    fn excerpt_takes_twenty_lines_either_side_and_never_more_than_120() {
        let content = numbered(200);
        let request = excerpt(&finding("src/a.rs", 100, 101), "rust", &content).unwrap();
        assert_eq!(request.flagged, vec!["line 100", "line 101"]);
        assert_eq!(request.before.len(), 20);
        assert_eq!(request.before.first().unwrap(), "line 80");
        assert_eq!(request.before.last().unwrap(), "line 99");
        assert_eq!(request.after.len(), 20);
        assert_eq!(request.after.first().unwrap(), "line 102");
        assert_eq!(request.after.last().unwrap(), "line 121");
        assert_eq!((request.start_line, request.end_line), (100, 101));
        assert_eq!(request.lines_sent(), 42);

        // The edges of the file give what there is.
        let top = excerpt(&finding("src/a.rs", 2, 2), "rust", &content).unwrap();
        assert_eq!(top.before, vec!["line 1"]);
        assert_eq!(top.after.len(), 20);
        let bottom = excerpt(&finding("src/a.rs", 200, 200), "rust", &content).unwrap();
        assert_eq!(bottom.before.len(), 20);
        assert!(bottom.after.is_empty());
        // No trailing newline, CRLF: same lines, no stray \r.
        let crlf = excerpt(&finding("src/a.rs", 2, 2), "rust", "a\r\nb\r\nc").unwrap();
        assert_eq!(
            (crlf.before, crlf.flagged, crlf.after),
            (
                vec!["a".to_owned()],
                vec!["b".to_owned()],
                vec!["c".to_owned()]
            )
        );

        // A long finding squeezes the context: 100 flagged lines leave 20.
        let wide = excerpt(&finding("src/a.rs", 50, 149), "rust", &content).unwrap();
        assert_eq!(wide.flagged.len(), 100);
        assert_eq!((wide.before.len(), wide.after.len()), (10, 10));
        assert_eq!(wide.lines_sent(), MAX_REQUEST_LINES);
        let full = excerpt(&finding("src/a.rs", 1, 120), "rust", &content).unwrap();
        assert_eq!(full.lines_sent(), MAX_REQUEST_LINES);
        assert!(full.before.is_empty() && full.after.is_empty());
        assert_eq!(
            excerpt(&finding("src/a.rs", 1, 121), "rust", &content),
            Err(ExcerptError::TooManyLines(121))
        );
        assert_eq!(
            excerpt(&finding("src/a.rs", 200, 201), "rust", &content),
            Err(ExcerptError::BeyondEnd {
                line: 201,
                lines: 200
            })
        );
        assert_eq!(
            excerpt(&finding("src/a.rs", 1, 1), "rust", ""),
            Err(ExcerptError::BeyondEnd { line: 1, lines: 0 })
        );
    }

    #[test]
    fn excerpt_redacts_secret_looking_context_and_refuses_secret_flagged_lines() {
        let content = "const region = \"eu-west-1\";\n\
                       const key = \"AKIAIOSFODNN7EXAMPLE\";\n\
                       const password = \"correct-horse-battery\";\n\
                       const client = connect(region).unwrap();\n\
                       export default client;\n";
        let mut flagged = finding("src/aws.ts", 4, 4);
        flagged.detail = "Line 4 unwraps.\nconst key = \"AKIAIOSFODNN7EXAMPLE\";".into();
        let request = excerpt(&flagged, "typescript", content).unwrap();
        assert_eq!(
            request.before,
            vec!["const region = \"eu-west-1\";", REDACTED, REDACTED]
        );
        assert_eq!(
            request.flagged,
            vec!["const client = connect(region).unwrap();"]
        );
        let sent = format!("{}{}", request.context_json(), request.instruction());
        assert!(!sent.contains("AKIA"), "{sent}");
        assert!(!sent.contains("correct-horse"), "{sent}");
        assert_eq!(request.detail, format!("Line 4 unwraps.\n{REDACTED}"));

        // The flagged line itself looks like a secret: nothing is built.
        assert_eq!(
            excerpt(&finding("src/aws.ts", 2, 2), "typescript", content),
            Err(ExcerptError::Secret)
        );
        assert_eq!(
            excerpt(&finding("src/aws.ts", 1, 4), "typescript", content),
            Err(ExcerptError::Secret)
        );
    }

    #[test]
    fn excerpt_bounds_long_lines() {
        let long = "x".repeat(1_000);
        let content = format!("{long}\nflagged();\n{long}\n");
        let request = excerpt(&finding("src/a.js", 2, 2), "javascript", &content).unwrap();
        assert_eq!(
            request.before[0].chars().count(),
            MAX_CONTEXT_LINE_CHARS + 1
        );
        assert!(request.before[0].ends_with('…'));
        assert_eq!(request.flagged, vec!["flagged();"]);
        // The flagged lines are never cut: too long means not sent.
        let blob = format!("{}\n", "y".repeat(MAX_FLAGGED_CHARS + 1));
        assert_eq!(
            excerpt(&finding("dist/app.min.js", 1, 1), "javascript", &blob),
            Err(ExcerptError::TooLong(MAX_FLAGGED_CHARS + 1))
        );
        let mut wordy = finding("src/a.js", 2, 2);
        wordy.detail = "d".repeat(5_000);
        let request = excerpt(&wordy, "javascript", &content).unwrap();
        assert_eq!(request.detail.chars().count(), MAX_DETAIL_CHARS + 1);
        // Worst case of one request, in characters: the bound that keeps
        // the call small.
        let worst = MAX_FLAGGED_CHARS + 2 * CONTEXT_LINES * (MAX_CONTEXT_LINE_CHARS + 1);
        assert!(worst <= 14_000, "{worst}");
    }

    /// The size of an ordinary request, measured on this crate's own
    /// source: one flagged line in the middle of a file, twenty lines
    /// either side. Token efficiency is a product requirement, so the
    /// numbers are pinned from above.
    #[test]
    fn a_typical_request_stays_small() {
        use crate::reasoner::{budget::estimate_tokens, fixes::SYSTEM_PROMPT};
        let content = include_str!("../osv.rs");
        let line = content
            .lines()
            .position(|l| l.contains(".expect(\"reqwest client\")"))
            .expect("the line this test measures") as i32
            + 1;
        let mut flagged = finding("services/intelligence/src/osv.rs", line, line);
        flagged.title = "unwrap/expect outside tests".into();
        flagged.detail = format!(
            "Line {line} panics on an error or a missing value: `.expect(\"reqwest client\"),`. Propagate it with \
             `?` or handle the `None`/`Err` case; a panic takes the whole task down."
        );
        let request = excerpt(&flagged, "rust", content).unwrap();
        assert_eq!(request.lines_sent(), 41);
        // What the provider receives as the user turn (see
        // `LlmReasoner::user_content`).
        let user = format!(
            "Structured context (JSON):\n{}\n\n{}",
            request.context_json(),
            request.instruction()
        );
        let system = estimate_tokens(SYSTEM_PROMPT);
        let input = system + estimate_tokens(&user);
        println!(
            "draft-fix request: system {} chars (~{system} tokens), user {} chars (~{} tokens), ~{input} input \
             tokens, reply capped at {} tokens",
            SYSTEM_PROMPT.len(),
            user.len(),
            estimate_tokens(&user),
            crate::reasoner::budget::FIX_MAX_TOKENS
        );
        assert!(system < 300, "{system}");
        assert!(input < 1_000, "{input}");
        // The hard ceiling of any request: 120 lines, of which the flagged
        // ones hold at most 4 000 characters and each context line 240.
        let ceiling = estimate_tokens(SYSTEM_PROMPT)
            + (MAX_FLAGGED_CHARS
                + 2 * CONTEXT_LINES * (MAX_CONTEXT_LINE_CHARS + 2)
                + MAX_DETAIL_CHARS
                + 600)
                .div_ceil(4);
        assert!(ceiling < 4_000, "{ceiling}");
    }

    #[test]
    fn patches_apply_bottom_up_over_several_findings() {
        let content = "one\ntwo\nthree\nfour\nfive\nsix\n";
        // Three fixes in one file, given top-down; the middle one grows the
        // file and the top one shrinks it.
        let top = accepted("a.txt", 1, 2, "low", "ONE-TWO");
        let middle = accepted("a.txt", 4, 4, "medium", "four-a\nfour-b\nfour-c\n");
        let bottom = accepted("a.txt", 6, 6, "high", "SIX");
        let patch = patch_file(content, &[&top, &middle, &bottom]);
        assert_eq!(
            patch.content,
            "ONE-TWO\nthree\nfour-a\nfour-b\nfour-c\nfive\nSIX\n"
        );
        assert_eq!(patch.applied, vec![top.id, middle.id, bottom.id]);
        assert!(patch.left_out.is_empty());
        // The order they are given in does not matter.
        let again = patch_file(content, &[&bottom, &top, &middle]);
        assert_eq!(again.content, patch.content);
    }

    #[test]
    fn overlaps_go_to_the_more_severe_finding() {
        let content = "a\nb\nc\nd\ne\n";
        let low = accepted("a.txt", 2, 3, "low", "LOW");
        let high = accepted("a.txt", 3, 4, "high", "HIGH");
        let apart = accepted("a.txt", 5, 5, "info", "E");
        let patch = patch_file(content, &[&low, &high, &apart]);
        assert_eq!(patch.content, "a\nb\nHIGH\nE\n");
        assert_eq!(patch.applied, vec![high.id, apart.id]);
        assert_eq!(
            patch.left_out,
            vec![(low.id, LeftOut::Overlaps { winner: high.id })]
        );
        // Equal severity: the one the review listed first wins.
        let mut first = accepted("a.txt", 1, 2, "medium", "FIRST");
        first.position = 1;
        let mut second = accepted("a.txt", 2, 2, "medium", "SECOND");
        second.position = 2;
        let patch = patch_file(content, &[&second, &first]);
        assert_eq!(patch.content, "FIRST\nc\nd\ne\n");
        assert_eq!(
            patch.left_out,
            vec![(second.id, LeftOut::Overlaps { winner: first.id })]
        );
        // Touching ranges do not overlap.
        let upper = accepted("a.txt", 1, 2, "medium", "AB");
        let lower = accepted("a.txt", 3, 3, "medium", "C");
        let patch = patch_file(content, &[&upper, &lower]);
        assert_eq!(patch.content, "AB\nC\nd\ne\n");
    }

    #[test]
    fn what_cannot_be_applied_is_left_out_and_the_file_keeps_its_shape() {
        let content = "a\nb\n";
        let mut beyond = accepted("a.txt", 2, 3, "high", "X");
        beyond.position = 1;
        let mut same = accepted("a.txt", 1, 1, "high", "a\n");
        same.position = 2;
        let patch = patch_file(content, &[&beyond, &same]);
        assert_eq!(patch.content, content);
        assert!(patch.applied.is_empty());
        assert_eq!(
            patch.left_out,
            vec![
                (beyond.id, LeftOut::BeyondEnd { lines: 2 }),
                (same.id, LeftOut::AlreadyApplied)
            ]
        );
        // CRLF files stay CRLF, including the lines a suggestion adds.
        let fix = accepted("a.txt", 2, 2, "medium", "B1\nB2");
        assert_eq!(
            patch_file("a\r\nb\r\nc\r\n", &[&fix]).content,
            "a\r\nB1\r\nB2\r\nc\r\n"
        );
        // No newline at the end of the file: none is added.
        assert_eq!(patch_file("a\nb", &[&fix]).content, "a\nB1\nB2");
        for reason in [
            LeftOut::Overlaps {
                winner: Uuid::nil(),
            },
            LeftOut::BeyondEnd { lines: 2 },
            LeftOut::AlreadyApplied,
        ] {
            assert!(!reason.sentence().is_empty());
        }
    }

    #[test]
    fn commit_message_lists_rules_and_locations() {
        let mut one = accepted("Dockerfile", 1, 1, "medium", "FROM node:22");
        one.rule = "docker.latest_tag".into();
        let two = accepted("src/main.rs", 12, 14, "high", "x");
        let review = Uuid::nil();
        let message = commit_message(&[&one, &two], 7, review, "0123456789abcdef0123");
        assert_eq!(
            message,
            format!(
                "Apply 2 review fixes to #7\n\n- docker.latest_tag Dockerfile:1\n\
                 - correctness.unwrap src/main.rs:12-14\n\n\
                 Accepted in SDI review {review} of 0123456789ab.\n"
            )
        );
        assert_eq!(fix_title(1, 7), "Apply 1 review fix to #7");
        let many: Vec<FindingRow> = (0..60)
            .map(|i| accepted("a.rs", i + 1, i + 1, "low", "x"))
            .collect();
        let refs: Vec<&FindingRow> = many.iter().collect();
        let message = commit_message(&refs, 7, review, "abc");
        assert_eq!(message.matches("- correctness.unwrap").count(), 50);
        assert!(message.contains("- and 10 more\n"));
        assert_eq!(language_of("src/a.tsx"), "typescript");
        assert_eq!(language_of("deploy/Dockerfile"), "dockerfile");
        assert_eq!(language_of("LICENSE"), "");
    }
}

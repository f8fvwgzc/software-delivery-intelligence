//! Unified-diff hunks as GitHub's `/pulls/{n}/files` reports them in the
//! `patch` field: no `diff --git` or `---`/`+++` headers, one or more
//! `@@ -a,b +c,d @@` sections. Parsed once, stored as JSON, and handed to the
//! review analyzers, which only need line numbers and the leading marker.

use serde::{Deserialize, Serialize};

/// One `@@` section. `lines` keep their leading ` `, `+` or `-`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub lines: Vec<String>,
}

/// Parses a GitHub patch. Lines outside a hunk (there should be none) and
/// the `\ No newline at end of file` markers are dropped; a malformed
/// header ends the previous hunk and starts none.
pub fn parse_patch(patch: &str) -> Vec<Hunk> {
    let mut hunks: Vec<Hunk> = Vec::new();
    let mut current: Option<Hunk> = None;
    for line in patch.lines() {
        if let Some(rest) = line.strip_prefix("@@ ") {
            if let Some(hunk) = current.take() {
                hunks.push(hunk);
            }
            current = parse_header(rest);
            continue;
        }
        let Some(hunk) = current.as_mut() else {
            continue;
        };
        match line.as_bytes().first() {
            Some(b' ' | b'+' | b'-') => hunk.lines.push(line.to_owned()),
            // An empty context line arrives without its leading space.
            None => hunk.lines.push(" ".to_owned()),
            Some(b'\\') => {}
            Some(_) => {}
        }
    }
    if let Some(hunk) = current.take() {
        hunks.push(hunk);
    }
    hunks
}

/// `-a,b +c,d @@ heading` → the four numbers; `b`/`d` default to 1 as in
/// the format.
fn parse_header(rest: &str) -> Option<Hunk> {
    let (ranges, _) = rest.split_once(" @@")?;
    let (old, new) = ranges.split_once(' ')?;
    let (old_start, old_lines) = parse_range(old.strip_prefix('-')?)?;
    let (new_start, new_lines) = parse_range(new.strip_prefix('+')?)?;
    Some(Hunk {
        old_start,
        old_lines,
        new_start,
        new_lines,
        lines: Vec::new(),
    })
}

fn parse_range(range: &str) -> Option<(u32, u32)> {
    match range.split_once(',') {
        Some((start, count)) => Some((start.parse().ok()?, count.parse().ok()?)),
        None => Some((range.parse().ok()?, 1)),
    }
}

/// Added lines with their line number in the new file.
pub fn added_lines(hunks: &[Hunk]) -> Vec<(u32, &str)> {
    let mut out = Vec::new();
    for hunk in hunks {
        let mut line_no = hunk.new_start;
        for line in &hunk.lines {
            match line.as_bytes().first() {
                Some(b'+') => {
                    out.push((line_no, &line[1..]));
                    line_no += 1;
                }
                Some(b'-') => {}
                _ => line_no += 1,
            }
        }
    }
    out
}

/// Removed lines with their line number in the old file.
pub fn removed_lines(hunks: &[Hunk]) -> Vec<(u32, &str)> {
    let mut out = Vec::new();
    for hunk in hunks {
        let mut line_no = hunk.old_start;
        for line in &hunk.lines {
            match line.as_bytes().first() {
                Some(b'-') => {
                    out.push((line_no, &line[1..]));
                    line_no += 1;
                }
                Some(b'+') => {}
                _ => line_no += 1,
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATCH: &str = "@@ -1,3 +1,4 @@\n use std::io;\n-fn main() {}\n+fn main() {\n+    println!(\"hi\");\n+}\n@@ -10 +11,2 @@ fn other()\n-old\n+new\n+newer\n\\ No newline at end of file";

    #[test]
    fn parses_headers_and_lines() {
        let hunks = parse_patch(PATCH);
        assert_eq!(hunks.len(), 2);
        assert_eq!((hunks[0].old_start, hunks[0].old_lines), (1, 3));
        assert_eq!((hunks[0].new_start, hunks[0].new_lines), (1, 4));
        assert_eq!(hunks[0].lines.len(), 5);
        assert_eq!(hunks[0].lines[1], "-fn main() {}");
        assert_eq!((hunks[1].old_start, hunks[1].old_lines), (10, 1));
        assert_eq!((hunks[1].new_start, hunks[1].new_lines), (11, 2));
        // The no-newline marker is not a diff line.
        assert_eq!(hunks[1].lines, vec!["-old", "+new", "+newer"]);
    }

    #[test]
    fn numbers_added_and_removed_lines() {
        let hunks = parse_patch(PATCH);
        let added = added_lines(&hunks);
        assert_eq!(
            added,
            vec![
                (2, "fn main() {"),
                (3, "    println!(\"hi\");"),
                (4, "}"),
                (11, "new"),
                (12, "newer")
            ]
        );
        let removed = removed_lines(&hunks);
        assert_eq!(removed, vec![(2, "fn main() {}"), (10, "old")]);
    }

    #[test]
    fn tolerates_garbage_and_empty_context() {
        assert!(parse_patch("").is_empty());
        assert!(parse_patch("not a diff\n+nope").is_empty());
        assert!(parse_patch("@@ broken @@\n+x").is_empty());
        let hunks = parse_patch("@@ -1,2 +1,2 @@\n\n+x");
        assert_eq!(hunks[0].lines, vec![" ", "+x"]);
        assert_eq!(added_lines(&hunks), vec![(2, "x")]);
    }
}

//! Text edits of YAML files that leave every other byte alone.
//!
//! A GitOps repository is read by people: its files carry comments, blank
//! lines, quoting habits and key order that a parse-and-reserialise round
//! trip would destroy, turning a one-line image bump into a diff nobody can
//! review. So nothing here builds a document tree. The file is scanned line
//! by line, the one scalar to change is located by indentation, and only
//! its characters are replaced (or, for a Kustomize `images:` entry, one
//! line is added or removed). Line endings, trailing comments and the
//! quoting style of the old value are kept.
//!
//! The scanner understands block mappings and block sequences, which is how
//! Kustomize, Helm values, Argo CD and Kubernetes manifests are written. A
//! path that runs into a flow collection (`{a: b}`, `[a, b]`), a block
//! scalar (`|`, `>`), an anchor, an alias or a tag is refused with a message
//! that says so; nothing is ever guessed.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    /// The path does not exist in the file.
    NotFound(String),
    /// The path exists but is written in a form this editor does not touch.
    Unsupported(String),
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(message) | Self::Unsupported(message) => f.write_str(message),
        }
    }
}

/// The result of an edit: the whole file again, and what changed in words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub content: String,
    pub changed: bool,
    pub summary: String,
}

// ---------------------------------------------------------------------------
// Lines
// ---------------------------------------------------------------------------

/// One physical line: its text and the line ending it had.
#[derive(Debug, Clone)]
struct Line {
    body: String,
    eol: String,
}

fn split_lines(text: &str) -> Vec<Line> {
    text.split_inclusive('\n')
        .map(|raw| {
            let (body, eol) = if let Some(body) = raw.strip_suffix("\r\n") {
                (body, "\r\n")
            } else if let Some(body) = raw.strip_suffix('\n') {
                (body, "\n")
            } else {
                (raw, "")
            };
            Line {
                body: body.to_owned(),
                eol: eol.to_owned(),
            }
        })
        .collect()
}

fn join_lines(lines: &[Line]) -> String {
    let mut out = String::with_capacity(lines.iter().map(|l| l.body.len() + 2).sum());
    for line in lines {
        out.push_str(&line.body);
        out.push_str(&line.eol);
    }
    out
}

/// The line ending the file uses (for lines this editor adds).
fn dominant_eol(lines: &[Line]) -> &'static str {
    if lines.iter().any(|l| l.eol == "\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

/// What a content line holds, by byte offsets into its body.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Parsed {
    /// Column of the first non-space character.
    indent: usize,
    /// The line starts a sequence item (`- `).
    dash: bool,
    /// Column where the key (or the item's scalar) starts.
    key_col: usize,
    key: Option<String>,
    /// Byte offset right after the key's colon.
    after_colon: Option<usize>,
    /// Span of the inline value, when there is one.
    value: Option<(usize, usize)>,
}

fn is_separator(body: &str) -> bool {
    body == "---" || body.starts_with("--- ") || body == "..." || body.starts_with("---\t")
}

/// `None` for blank lines, comments and document separators.
fn parse_line(body: &str) -> Option<Parsed> {
    let indent = body.len() - body.trim_start_matches(' ').len();
    let rest = &body[indent..];
    if rest.trim().is_empty() || rest.starts_with('#') || is_separator(body) {
        return None;
    }
    let (dash, key_col) = if rest == "-" {
        (true, indent + 1)
    } else if rest.starts_with("- ") {
        let after = &rest[1..];
        let spaces = after.len() - after.trim_start_matches(' ').len();
        (true, indent + 1 + spaces)
    } else {
        (false, indent)
    };
    let rest = &body[key_col.min(body.len())..];
    if rest.is_empty() || rest.starts_with('#') {
        // A bare dash: the item's content is on the following lines.
        return Some(Parsed {
            indent,
            dash,
            key_col,
            key: None,
            after_colon: None,
            value: None,
        });
    }

    // A quoted key or a quoted scalar.
    if let Some(quote) = rest.chars().next().filter(|c| *c == '"' || *c == '\'') {
        let end = quoted_end(rest, quote)?;
        let after = &rest[end..];
        let trimmed = after.trim_start_matches(' ');
        if let Some(tail) = trimmed.strip_prefix(':')
            && (tail.is_empty() || tail.starts_with(' '))
        {
            let after_colon = key_col + end + (after.len() - trimmed.len()) + 1;
            return Some(Parsed {
                indent,
                dash,
                key_col,
                key: Some(unquote(&rest[..end])),
                after_colon: Some(after_colon),
                value: value_span(body, after_colon),
            });
        }
        return Some(Parsed {
            indent,
            dash,
            key_col,
            key: None,
            after_colon: None,
            value: Some((key_col, key_col + end)),
        });
    }

    // A plain key ends at the first colon followed by a space or the end of
    // the line; a comment before that means there is no key.
    let mut key_end = None;
    let bytes = rest.as_bytes();
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'#' && i > 0 && bytes[i - 1] == b' ' {
            break;
        }
        if *b == b':' && (i + 1 == bytes.len() || bytes[i + 1] == b' ') {
            key_end = Some(i);
            break;
        }
    }
    match key_end {
        Some(end) if !rest.starts_with(['{', '[']) => {
            let after_colon = key_col + end + 1;
            Some(Parsed {
                indent,
                dash,
                key_col,
                key: Some(rest[..end].trim_end().to_owned()),
                after_colon: Some(after_colon),
                value: value_span(body, after_colon),
            })
        }
        _ => Some(Parsed {
            indent,
            dash,
            key_col,
            key: None,
            after_colon: None,
            value: value_span(body, key_col),
        }),
    }
}

/// Byte length of the quoted string `text` starts with, quotes included.
fn quoted_end(text: &str, quote: char) -> Option<usize> {
    let mut chars = text.char_indices().skip(1).peekable();
    while let Some((i, c)) = chars.next() {
        if quote == '"' && c == '\\' {
            chars.next();
            continue;
        }
        if c == quote {
            if quote == '\'' && chars.peek().is_some_and(|(_, next)| *next == '\'') {
                chars.next();
                continue;
            }
            return Some(i + c.len_utf8());
        }
    }
    None
}

/// The span of the scalar that starts at or after `from`, without its
/// trailing comment. `None` when the rest of the line is empty or a comment.
fn value_span(body: &str, from: usize) -> Option<(usize, usize)> {
    let rest = body.get(from..)?;
    let start = from + (rest.len() - rest.trim_start_matches(' ').len());
    let rest = &body[start..];
    if rest.is_empty() || rest.starts_with('#') {
        return None;
    }
    if let Some(quote) = rest.chars().next().filter(|c| *c == '"' || *c == '\'') {
        // An unterminated quote spans lines; take the rest of the line so
        // the caller refuses it.
        let end = quoted_end(rest, quote).unwrap_or(rest.len());
        return Some((start, start + end));
    }
    let bytes = rest.as_bytes();
    let mut end = bytes.len();
    for i in 1..bytes.len() {
        if bytes[i] == b'#' && bytes[i - 1] == b' ' {
            end = i;
            break;
        }
    }
    let end = rest[..end].trim_end().len();
    Some((start, start + end))
}

fn unquote(raw: &str) -> String {
    if raw.len() >= 2 && raw.starts_with('"') && raw.ends_with('"') {
        let inner = &raw[1..raw.len() - 1];
        let mut out = String::with_capacity(inner.len());
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some(other) => out.push(other),
                    None => {}
                }
            } else {
                out.push(c);
            }
        }
        out
    } else if raw.len() >= 2 && raw.starts_with('\'') && raw.ends_with('\'') {
        raw[1..raw.len() - 1].replace("''", "'")
    } else {
        raw.to_owned()
    }
}

/// Whether `value` written without quotes would be read as something other
/// than the string it is: a number (a short commit SHA made of digits is
/// the classic), a boolean, null, or YAML syntax.
pub fn needs_quotes(value: &str) -> bool {
    if value.is_empty() || value != value.trim() {
        return true;
    }
    let lower = value.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "true" | "false" | "yes" | "no" | "on" | "off" | "null" | "~" | "y" | "n"
    ) {
        return true;
    }
    let first = value.chars().next().unwrap_or(' ');
    if "!&*-?|>'\"%@`{}[],#:".contains(first) {
        return true;
    }
    if value.contains(": ") || value.contains(" #") || value.ends_with(':') {
        return true;
    }
    // Anything that starts like a number and holds only what numbers hold
    // (digits, hex digits, separators, exponents): quote rather than find
    // out which YAML version the reader implements.
    let numeric_start = first.is_ascii_digit() || first == '+' || first == '.';
    let numeric_body = value
        .chars()
        .all(|c| c.is_ascii_hexdigit() || "._+-xXoO:".contains(c));
    if numeric_start && numeric_body {
        return true;
    }
    value.parse::<f64>().is_ok()
}

/// `value` in the quoting style `old_raw` used.
fn render(old_raw: &str, value: &str) -> String {
    if old_raw.starts_with('"') {
        format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
    } else if old_raw.starts_with('\'') {
        format!("'{}'", value.replace('\'', "''"))
    } else if needs_quotes(value) {
        format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        value.to_owned()
    }
}

/// Refuses values this editor does not rewrite.
fn check_editable(raw: &str, path: &str) -> Result<(), EditError> {
    let reason = match raw.chars().next() {
        Some('|') | Some('>') => "a block scalar",
        Some('{') | Some('[') => "a flow collection",
        Some('&') => "an anchored value",
        Some('*') => "an alias",
        Some('!') => "a tagged value",
        Some('"') if quoted_end(raw, '"') != Some(raw.len()) => "a multi-line string",
        Some('\'') if quoted_end(raw, '\'') != Some(raw.len()) => "a multi-line string",
        _ => return Ok(()),
    };
    Err(EditError::Unsupported(format!(
        "{path} is {reason}; only plain and quoted single-line scalars are edited"
    )))
}

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum Segment {
    Key(String),
    Index(usize),
}

/// `a.b[0].c`, with `["dotted.key"]` for keys that contain dots.
fn parse_path(path: &str) -> Result<Vec<Segment>, EditError> {
    let bad = |why: &str| EditError::NotFound(format!("yaml path {path:?} is not valid: {why}"));
    let mut segments = Vec::new();
    let mut key = String::new();
    let mut chars = path.trim().chars().peekable();
    let mut expect_separator = false;
    while let Some(c) = chars.next() {
        match c {
            '.' => {
                if !key.is_empty() {
                    segments.push(Segment::Key(std::mem::take(&mut key)));
                } else if !expect_separator {
                    return Err(bad("empty segment"));
                }
                expect_separator = false;
            }
            '[' => {
                if !key.is_empty() {
                    segments.push(Segment::Key(std::mem::take(&mut key)));
                }
                let mut inner = String::new();
                let mut closed = false;
                for c in chars.by_ref() {
                    if c == ']' {
                        closed = true;
                        break;
                    }
                    inner.push(c);
                }
                if !closed {
                    return Err(bad("unclosed ["));
                }
                let inner = inner.trim();
                if let Ok(index) = inner.parse::<usize>() {
                    segments.push(Segment::Index(index));
                } else {
                    let unquoted = unquote(inner);
                    if unquoted.is_empty() || unquoted == inner {
                        return Err(bad("[...] holds an index or a quoted key"));
                    }
                    segments.push(Segment::Key(unquoted));
                }
                expect_separator = true;
            }
            other => {
                if expect_separator {
                    return Err(bad("expected . after ]"));
                }
                key.push(other);
            }
        }
    }
    if !key.is_empty() {
        segments.push(Segment::Key(key));
    }
    if segments.is_empty() {
        return Err(bad("it is empty"));
    }
    Ok(segments)
}

/// Where a scalar lives: its line and, when it has a value, its span.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Found {
    line: usize,
    value: Option<(usize, usize)>,
    /// Where a value goes when the key has none (`key:` followed by nothing).
    insert_at: usize,
}

struct Doc<'a> {
    lines: &'a [Line],
    parsed: Vec<Option<Parsed>>,
}

impl<'a> Doc<'a> {
    fn new(lines: &'a [Line]) -> Self {
        Self {
            parsed: lines.iter().map(|l| parse_line(&l.body)).collect(),
            lines,
        }
    }

    /// `[lo, hi)` ranges of the documents in the file.
    fn documents(&self) -> Vec<(usize, usize)> {
        let mut ranges = Vec::new();
        let mut start = 0;
        for (i, line) in self.lines.iter().enumerate() {
            if is_separator(&line.body) {
                if (start..i).any(|j| self.parsed[j].is_some()) {
                    ranges.push((start, i));
                }
                start = i + 1;
            }
        }
        if (start..self.lines.len()).any(|j| self.parsed[j].is_some()) {
            ranges.push((start, self.lines.len()));
        }
        ranges
    }

    fn first_content(&self, lo: usize, hi: usize) -> Option<usize> {
        (lo..hi).find(|i| self.parsed[*i].is_some())
    }

    /// The lines that belong to the key (or bare dash) on line `at`.
    fn block_of(&self, at: usize, hi: usize) -> (usize, usize) {
        let owner = self.parsed[at].as_ref().expect("content line");
        let col = owner.key_col;
        let mut end = at + 1;
        for i in at + 1..hi {
            match &self.parsed[i] {
                None => {
                    if is_separator(&self.lines[i].body) {
                        break;
                    }
                }
                Some(p) => {
                    // Deeper lines belong to the key; so does a sequence
                    // written at the key's own column (`key:` / `- item`).
                    let child =
                        p.indent > col || (p.indent == col && p.dash && owner.key.is_some());
                    if !child {
                        break;
                    }
                    end = i + 1;
                }
            }
        }
        (at + 1, end)
    }

    /// The item ranges of the sequence in `[lo, hi)`.
    fn items(&self, lo: usize, hi: usize, path: &str) -> Result<Vec<(usize, usize)>, EditError> {
        let Some(first) = self.first_content(lo, hi) else {
            return Err(EditError::NotFound(format!("{path} is empty")));
        };
        let head = self.parsed[first].as_ref().expect("content line");
        if !head.dash {
            let body = self.lines[first].body.trim_start();
            return Err(if body.starts_with(['[', '{']) {
                EditError::Unsupported(format!(
                    "{path} is a flow collection; only block sequences are edited"
                ))
            } else {
                EditError::NotFound(format!("{path} is not a sequence"))
            });
        }
        let col = head.indent;
        let starts: Vec<usize> = (first..hi)
            .filter(|i| {
                self.parsed[*i]
                    .as_ref()
                    .is_some_and(|p| p.dash && p.indent == col)
            })
            .collect();
        Ok(starts
            .iter()
            .enumerate()
            .map(|(n, start)| (*start, starts.get(n + 1).copied().unwrap_or(hi)))
            .collect())
    }

    /// The line holding `key` in the mapping that occupies `[lo, hi)`.
    fn key_line(&self, lo: usize, hi: usize, key: &str) -> Option<usize> {
        let first = self.first_content(lo, hi)?;
        let col = self.parsed[first].as_ref()?.key_col;
        (first..hi).find(|i| {
            self.parsed[*i]
                .as_ref()
                .is_some_and(|p| p.key_col == col && p.key.as_deref() == Some(key))
        })
    }

    /// Follows `segments` from the mapping or sequence in `[lo, hi)`.
    fn locate(
        &self,
        mut lo: usize,
        mut hi: usize,
        segments: &[Segment],
        path: &str,
    ) -> Result<Found, EditError> {
        let mut walked = String::new();
        for (n, segment) in segments.iter().enumerate() {
            let last = n + 1 == segments.len();
            match segment {
                Segment::Key(key) => {
                    if !walked.is_empty() {
                        walked.push('.');
                    }
                    walked.push_str(key);
                    let Some(at) = self.key_line(lo, hi, key) else {
                        if let Some(first) = self.first_content(lo, hi)
                            && self.lines[first].body.trim_start().starts_with(['{', '['])
                        {
                            return Err(EditError::Unsupported(format!(
                                "{walked} sits inside a flow collection; only block mappings are edited"
                            )));
                        }
                        return Err(EditError::NotFound(format!(
                            "{path}: no key {walked} in the file"
                        )));
                    };
                    let parsed = self.parsed[at].as_ref().expect("content line");
                    let (block_lo, block_hi) = self.block_of(at, hi);
                    let has_block = self.first_content(block_lo, block_hi).is_some();
                    if last {
                        if parsed.value.is_none() && has_block {
                            return Err(EditError::NotFound(format!(
                                "{path} is a mapping or a sequence, not a scalar"
                            )));
                        }
                        return Ok(Found {
                            line: at,
                            value: parsed.value,
                            insert_at: parsed.after_colon.unwrap_or(self.lines[at].body.len()),
                        });
                    }
                    if let Some((start, end)) = parsed.value {
                        check_editable(&self.lines[at].body[start..end], &walked)?;
                        return Err(EditError::NotFound(format!(
                            "{path}: {walked} is a scalar, it has no children"
                        )));
                    }
                    lo = block_lo;
                    hi = block_hi;
                }
                Segment::Index(index) => {
                    walked.push_str(&format!("[{index}]"));
                    let items = self.items(lo, hi, &walked)?;
                    let Some((item_lo, item_hi)) = items.get(*index).copied() else {
                        return Err(EditError::NotFound(format!(
                            "{path}: {walked} is out of range ({} items)",
                            items.len()
                        )));
                    };
                    if last {
                        let parsed = self.parsed[item_lo].as_ref().expect("content line");
                        if parsed.key.is_some() {
                            return Err(EditError::NotFound(format!(
                                "{path} is a mapping, not a scalar"
                            )));
                        }
                        return Ok(Found {
                            line: item_lo,
                            value: parsed.value,
                            insert_at: self.lines[item_lo].body.len(),
                        });
                    }
                    lo = item_lo;
                    hi = item_hi;
                }
            }
        }
        Err(EditError::NotFound(format!("{path} is empty")))
    }

    /// The first document in which `segments` resolve.
    fn find(&self, segments: &[Segment], path: &str) -> Result<Found, EditError> {
        let mut first_error = None;
        for (lo, hi) in self.documents() {
            match self.locate(lo, hi, segments, path) {
                Ok(found) => return Ok(found),
                Err(error @ EditError::Unsupported(_)) => return Err(error),
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
        Err(first_error
            .unwrap_or_else(|| EditError::NotFound(format!("{path}: the file has no content"))))
    }
}

/// Writes `value` at `found`, keeping the old value's quoting.
fn write_scalar(
    lines: &mut [Line],
    found: &Found,
    value: &str,
    path: &str,
) -> Result<(), EditError> {
    let body = &mut lines[found.line].body;
    match found.value {
        Some((start, end)) => {
            let old = body[start..end].to_owned();
            check_editable(&old, path)?;
            body.replace_range(start..end, &render(&old, value));
        }
        None => {
            // `key:` with nothing after it: the value goes right behind the
            // colon, before any trailing comment.
            let at = found.insert_at.min(body.len());
            body.insert_str(at, &format!(" {}", render("", value)));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Public edits
// ---------------------------------------------------------------------------

/// The scalar at `path`, unquoted; empty when the key has no value.
pub fn get_scalar(text: &str, path: &str) -> Result<String, EditError> {
    let lines = split_lines(text);
    let doc = Doc::new(&lines);
    let found = doc.find(&parse_path(path)?, path)?;
    Ok(match found.value {
        Some((start, end)) => {
            let raw = &lines[found.line].body[start..end];
            check_editable(raw, path)?;
            unquote(raw)
        }
        None => String::new(),
    })
}

/// Sets the scalar at `path` to `value` (the `helm_values` and `yaml_path`
/// strategies). Everything else in the file stays as it was.
pub fn set_scalar(text: &str, path: &str, value: &str) -> Result<Edit, EditError> {
    let mut lines = split_lines(text);
    let found = Doc::new(&lines).find(&parse_path(path)?, path)?;
    let old = match found.value {
        Some((start, end)) => unquote(&lines[found.line].body[start..end]),
        None => String::new(),
    };
    write_scalar(&mut lines, &found, value, path)?;
    let content = join_lines(&lines);
    let changed = content != text;
    Ok(Edit {
        summary: if changed {
            format!("{path}: {old} -> {value}")
        } else {
            format!("{path} is already {value}")
        },
        content,
        changed,
    })
}

/// The image reference without its tag and digest.
pub fn image_repository(reference: &str) -> &str {
    let without_digest = reference.split('@').next().unwrap_or(reference);
    match without_digest.rfind(':') {
        // A colon before the last slash is a registry port, not a tag.
        Some(colon) if !without_digest[colon..].contains('/') => &without_digest[..colon],
        _ => without_digest,
    }
}

/// What the scalar at a `yaml_path` should become. A value that is a full
/// reference to the release's image (`ghcr.io/acme/api:1.2`, as in a
/// Deployment manifest) is replaced by the new full reference, keeping a
/// digest pin if it had one; a bare digest is replaced by the digest; any
/// other value is taken to be the tag.
pub fn target_value(current: &str, image: &str, tag: &str, digest: &str) -> String {
    let current = current.trim();
    if !image.is_empty() && image_repository(current) == image {
        return match (current.contains('@'), digest.is_empty()) {
            (true, false) if current.split('@').next().is_some_and(|c| c != image) => {
                format!("{image}:{tag}@{digest}")
            }
            (true, false) => format!("{image}@{digest}"),
            _ => format!("{image}:{tag}"),
        };
    }
    if current.starts_with("sha256:") && !digest.is_empty() {
        return digest.to_owned();
    }
    tag.to_owned()
}

/// Points the `images:` entry for `image` in a `kustomization.yaml` at
/// `tag` (and `digest`, when the entry pins one). The entry is found by its
/// `name`, by its `newName`, or, when exactly one entry is named like the
/// image's last path segment, by that. A `newTag` line is added when the
/// entry has none; a `digest` pin is updated when the new digest is known
/// and removed when it is not, because Kustomize prefers a digest over the
/// tag and a stale one would keep the old image running.
pub fn set_kustomize_image(
    text: &str,
    image: &str,
    tag: &str,
    digest: &str,
) -> Result<Edit, EditError> {
    let mut lines = split_lines(text);
    let (item_lo, item_hi, key_col, shown_name) = {
        let doc = Doc::new(&lines);
        let mut located = None;
        let mut first_error = None;
        for (lo, hi) in doc.documents() {
            match locate_image(&doc, lo, hi, image) {
                Ok(found) => {
                    located = Some(found);
                    break;
                }
                Err(error @ EditError::Unsupported(_)) => return Err(error),
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
        match located {
            Some(found) => found,
            None => {
                return Err(first_error.unwrap_or_else(|| {
                    EditError::NotFound("the kustomization has no images: section".to_owned())
                }));
            }
        }
    };

    let mut notes = Vec::new();
    let item_key = |lines: &[Line], key: &str| -> Option<(usize, Parsed)> {
        (item_lo..item_hi.min(lines.len())).find_map(|i| {
            parse_line(&lines[i].body)
                .filter(|p| p.key_col == key_col && p.key.as_deref() == Some(key))
                .map(|p| (i, p))
        })
    };

    // newTag: replace, or add right after newName / name.
    let mut item_hi = item_hi;
    match item_key(&lines, "newTag") {
        Some((at, parsed)) => {
            let found = Found {
                line: at,
                value: parsed.value,
                insert_at: parsed.after_colon.unwrap_or(lines[at].body.len()),
            };
            let old = parsed
                .value
                .map(|(s, e)| unquote(&lines[at].body[s..e]))
                .unwrap_or_default();
            write_scalar(&mut lines, &found, tag, "images[].newTag")?;
            if old != tag {
                notes.push(format!("newTag {old} -> {tag}"));
            }
        }
        None => {
            let (anchor, _) = item_key(&lines, "newName")
                .or_else(|| item_key(&lines, "name"))
                .ok_or_else(|| {
                    EditError::NotFound(format!("the images entry for {image} has no name"))
                })?;
            let eol = dominant_eol(&lines);
            if lines[anchor].eol.is_empty() {
                lines[anchor].eol = eol.to_owned();
            }
            lines.insert(
                anchor + 1,
                Line {
                    body: format!("{}newTag: {}", " ".repeat(key_col), render("", tag)),
                    eol: eol.to_owned(),
                },
            );
            item_hi += 1;
            notes.push(format!("newTag {tag} added"));
        }
    }

    // digest: follow the release when the entry pins one.
    let digest_line = (item_lo..item_hi.min(lines.len())).find_map(|i| {
        parse_line(&lines[i].body)
            .filter(|p| p.key_col == key_col && p.key.as_deref() == Some("digest"))
            .map(|p| (i, p))
    });
    if let Some((at, parsed)) = digest_line {
        let old = parsed
            .value
            .map(|(s, e)| unquote(&lines[at].body[s..e]))
            .unwrap_or_default();
        if digest.is_empty() {
            if parsed.dash {
                // Removing the line that carries the item's dash would
                // merge the entry into the previous one.
                return Err(EditError::Unsupported(format!(
                    "the images entry for {image} pins a digest on its first line and no digest \
                     is known for {tag}; put name first so the pin can be removed"
                )));
            }
            lines.remove(at);
            notes.push(format!(
                "digest pin {old} removed (no digest known for {tag})"
            ));
        } else {
            let found = Found {
                line: at,
                value: parsed.value,
                insert_at: parsed.after_colon.unwrap_or(lines[at].body.len()),
            };
            write_scalar(&mut lines, &found, digest, "images[].digest")?;
            if old != digest {
                notes.push(format!("digest {old} -> {digest}"));
            }
        }
    }

    let content = join_lines(&lines);
    let changed = content != text;
    Ok(Edit {
        summary: if changed {
            format!("images[{shown_name}]: {}", notes.join(", "))
        } else {
            format!("images[{shown_name}] is already at {tag}")
        },
        content,
        changed,
    })
}

/// The `images:` entry for `image` in the document `[lo, hi)`: its line
/// range, the column its keys sit at, and the name it goes by.
fn locate_image(
    doc: &Doc<'_>,
    lo: usize,
    hi: usize,
    image: &str,
) -> Result<(usize, usize, usize, String), EditError> {
    let Some(at) = doc.key_line(lo, hi, "images") else {
        return Err(EditError::NotFound(
            "the kustomization has no images: section".to_owned(),
        ));
    };
    let parsed = doc.parsed[at].as_ref().expect("content line");
    if let Some((start, end)) = parsed.value {
        let raw = &doc.lines[at].body[start..end];
        return Err(if raw.starts_with('[') {
            EditError::Unsupported(
                "images: is a flow sequence; write it as a block list to let SDI edit it"
                    .to_owned(),
            )
        } else {
            EditError::NotFound("images: is not a list".to_owned())
        });
    }
    let (block_lo, block_hi) = doc.block_of(at, hi);
    let items = doc.items(block_lo, block_hi, "images")?;
    let last_segment = |s: &str| s.rsplit('/').next().unwrap_or(s).to_owned();

    let mut entries = Vec::new();
    for (item_lo, item_hi) in items {
        // The item's keys sit at the column of the key on its dash line, or
        // of its first line when the dash stands alone.
        let head = doc.parsed[item_lo].as_ref().expect("content line");
        let key_col = if head.key.is_some() {
            head.key_col
        } else {
            match doc.first_content(item_lo + 1, item_hi) {
                Some(next) => doc.parsed[next].as_ref().expect("content line").key_col,
                None => continue,
            }
        };
        let scalar = |key: &str| -> Option<String> {
            (item_lo..item_hi).find_map(|i| {
                let p = doc.parsed[i].as_ref()?;
                if p.key_col != key_col || p.key.as_deref() != Some(key) {
                    return None;
                }
                let (start, end) = p.value?;
                Some(unquote(&doc.lines[i].body[start..end]))
            })
        };
        if head.key.is_none() && head.value.is_some() {
            return Err(EditError::Unsupported(
                "images: holds plain strings; write entries as name/newTag mappings".to_owned(),
            ));
        }
        entries.push((
            item_lo,
            item_hi,
            key_col,
            scalar("name").unwrap_or_default(),
            scalar("newName").unwrap_or_default(),
        ));
    }

    let by_name = entries.iter().find(|e| e.3 == image);
    let by_new_name = entries.iter().find(|e| e.4 == image);
    let loose: Vec<_> = entries
        .iter()
        .filter(|e| !e.3.is_empty() && last_segment(&e.3) == last_segment(image))
        .collect();
    let chosen = by_name.or(by_new_name).or(if loose.len() == 1 {
        Some(loose[0])
    } else {
        None
    });
    match chosen {
        Some((item_lo, item_hi, key_col, name, _)) => {
            Ok((*item_lo, *item_hi, *key_col, name.clone()))
        }
        None => {
            let names: Vec<&str> = entries.iter().map(|e| e.3.as_str()).collect();
            Err(EditError::NotFound(format!(
                "the kustomization has no images entry for {image} (entries: {})",
                if names.is_empty() {
                    "none".to_owned()
                } else {
                    names.join(", ")
                }
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KUSTOMIZATION: &str = "\
# Fleet: production overlay for the API.
apiVersion: kustomize.config.k8s.io/v1beta1
kind: Kustomization

namespace: api-prod   # owned by team-platform

resources:
  - ../../base
  - ingress.yaml

images:
  # The sidecar is pinned by hand; do not touch.
  - name: ghcr.io/acme/envoy
    newTag: v1.31.2
  - name: ghcr.io/acme/api   # bumped by SDI
    newTag: 9f2c1ab          # last deploy: 2026-10-01
  - name: worker
    newName: ghcr.io/acme/worker
    newTag: \"1.4.0\"

patches:
  - path: replicas.yaml
";

    #[test]
    fn kustomize_bumps_only_the_matching_entry_and_keeps_every_comment() {
        let edit = set_kustomize_image(KUSTOMIZATION, "ghcr.io/acme/api", "a1b2c3d", "").unwrap();
        assert!(edit.changed);
        assert_eq!(
            edit.summary,
            "images[ghcr.io/acme/api]: newTag 9f2c1ab -> a1b2c3d"
        );
        // Exactly one line differs, and only in the tag.
        let before: Vec<&str> = KUSTOMIZATION.lines().collect();
        let after: Vec<&str> = edit.content.lines().collect();
        assert_eq!(before.len(), after.len());
        let differing: Vec<(&&str, &&str)> =
            before.iter().zip(&after).filter(|(a, b)| a != b).collect();
        assert_eq!(differing.len(), 1, "{differing:?}");
        assert_eq!(
            *differing[0].1,
            "    newTag: a1b2c3d          # last deploy: 2026-10-01"
        );
        // The result is still the YAML it was, with the one value changed.
        let parsed: serde_yaml_ng::Value = serde_yaml_ng::from_str(&edit.content).unwrap();
        assert_eq!(parsed["images"][1]["newTag"], "a1b2c3d");
        assert_eq!(parsed["images"][0]["newTag"], "v1.31.2");
        assert_eq!(parsed["images"][2]["newTag"], "1.4.0");
        assert_eq!(parsed["namespace"], "api-prod");

        // Applying the same release again changes nothing.
        let again = set_kustomize_image(&edit.content, "ghcr.io/acme/api", "a1b2c3d", "").unwrap();
        assert!(!again.changed);
        assert_eq!(again.content, edit.content);
    }

    #[test]
    fn kustomize_matches_new_name_and_keeps_the_quoting_style() {
        let edit = set_kustomize_image(KUSTOMIZATION, "ghcr.io/acme/worker", "1.5.0", "").unwrap();
        assert!(edit.content.contains("    newTag: \"1.5.0\"\n"));
        assert_eq!(edit.summary, "images[worker]: newTag 1.4.0 -> 1.5.0");
        let parsed: serde_yaml_ng::Value = serde_yaml_ng::from_str(&edit.content).unwrap();
        assert_eq!(parsed["images"][2]["newTag"], "1.5.0");
        assert_eq!(parsed["images"][1]["newTag"], "9f2c1ab");
    }

    #[test]
    fn a_tag_that_looks_like_a_number_is_quoted_so_it_stays_a_string() {
        // A short SHA made of digits would be read as an integer.
        let edit = set_kustomize_image(KUSTOMIZATION, "ghcr.io/acme/api", "1234567", "").unwrap();
        assert!(
            edit.content
                .contains("    newTag: \"1234567\"          # last deploy: 2026-10-01\n"),
            "{}",
            edit.content
        );
        let parsed: serde_yaml_ng::Value = serde_yaml_ng::from_str(&edit.content).unwrap();
        assert_eq!(parsed["images"][1]["newTag"], "1234567");
        for (value, quoted) in [
            ("1234567", true),
            ("1e5", true),
            ("0x1f", true),
            ("12e4567", true),
            ("2024.10.1", true),
            ("true", true),
            ("null", true),
            ("", true),
            ("-rc1", true),
            ("a: b", true),
            ("v1.2.3", false),
            ("a1b2c3d", false),
            ("sha-9f2c1ab", false),
            ("main", false),
            ("ghcr.io/acme/api:1.2", false),
        ] {
            assert_eq!(needs_quotes(value), quoted, "{value:?}");
        }
    }

    #[test]
    fn kustomize_adds_a_missing_new_tag_and_follows_or_drops_a_digest_pin() {
        let file = "\
images:
- name: ghcr.io/acme/api
- digest: sha256:1111111111111111111111111111111111111111111111111111111111111111   # pinned
  name: ghcr.io/acme/db
  newTag: v1
";
        // No newTag yet: one line is added under the name, same indentation.
        let added = set_kustomize_image(file, "ghcr.io/acme/api", "abc1234", "").unwrap();
        assert!(added.content.starts_with(
            "images:\n- name: ghcr.io/acme/api\n  newTag: abc1234\n- digest: sha256:1111"
        ));
        assert_eq!(
            added.summary,
            "images[ghcr.io/acme/api]: newTag abc1234 added"
        );

        // A digest pin written first on its item cannot be moved by a text edit.
        let error = set_kustomize_image(file, "ghcr.io/acme/db", "v2", "").unwrap_err();
        assert!(matches!(error, EditError::Unsupported(_)), "{error}");

        let pinned = "\
images:
  - name: ghcr.io/acme/db
    newTag: v1
    digest: sha256:1111111111111111111111111111111111111111111111111111111111111111   # pinned
";
        let new_digest = format!("sha256:{}", "2".repeat(64));
        let followed = set_kustomize_image(pinned, "ghcr.io/acme/db", "v2", &new_digest).unwrap();
        assert!(
            followed
                .content
                .contains(&format!("    digest: {new_digest}   # pinned\n"))
        );
        assert!(followed.content.contains("    newTag: v2\n"));
        // Without a known digest the stale pin goes: Kustomize would prefer it.
        let dropped = set_kustomize_image(pinned, "ghcr.io/acme/db", "v2", "").unwrap();
        assert_eq!(
            dropped.content,
            "images:\n  - name: ghcr.io/acme/db\n    newTag: v2\n"
        );
        assert!(
            dropped.summary.contains("digest pin"),
            "{}",
            dropped.summary
        );
    }

    #[test]
    fn kustomize_refuses_what_it_cannot_edit_safely() {
        let missing =
            set_kustomize_image(KUSTOMIZATION, "ghcr.io/acme/other", "v1", "").unwrap_err();
        assert!(
            missing
                .to_string()
                .contains("no images entry for ghcr.io/acme/other (entries: ghcr.io/acme/envoy, ghcr.io/acme/api, worker)"),
            "{missing}"
        );
        let none = set_kustomize_image("resources:\n  - a.yaml\n", "x/y", "v1", "").unwrap_err();
        assert_eq!(none.to_string(), "the kustomization has no images: section");
        let flow = set_kustomize_image("images: [{name: x/y, newTag: v0}]\n", "x/y", "v1", "")
            .unwrap_err();
        assert!(matches!(flow, EditError::Unsupported(_)), "{flow}");
        // Two entries share a last segment: neither is picked by it.
        let ambiguous =
            "images:\n  - name: a/api\n    newTag: v1\n  - name: b/api\n    newTag: v1\n";
        assert!(set_kustomize_image(ambiguous, "ghcr.io/acme/api", "v2", "").is_err());
        // One entry named like the image's last segment is the image.
        let placeholder = "images:\n  - name: api\n    newTag: v1\n";
        let edit = set_kustomize_image(placeholder, "ghcr.io/acme/api", "v2", "").unwrap();
        assert_eq!(edit.content, "images:\n  - name: api\n    newTag: v2\n");
    }

    const VALUES: &str = "\
# Helm values for the API chart.
replicaCount: 3

image:
  repository: ghcr.io/acme/api
  # Bumped on every release; never edit by hand.
  tag: \"9f2c1ab\"   # current
  pullPolicy: IfNotPresent

service:
  type: ClusterIP
  port: 80

resources: {}
ingress:
  hosts:
    - host: api.example.com
      paths: [\"/\"]
";

    #[test]
    fn helm_values_tag_is_set_in_place() {
        let edit = set_scalar(VALUES, "image.tag", "a1b2c3d").unwrap();
        assert!(edit.changed);
        assert_eq!(edit.summary, "image.tag: 9f2c1ab -> a1b2c3d");
        assert_eq!(edit.content, VALUES.replace("\"9f2c1ab\"", "\"a1b2c3d\""));
        let parsed: serde_yaml_ng::Value = serde_yaml_ng::from_str(&edit.content).unwrap();
        assert_eq!(parsed["image"]["tag"], "a1b2c3d");
        assert_eq!(parsed["replicaCount"], 3);

        assert_eq!(get_scalar(VALUES, "image.tag").unwrap(), "9f2c1ab");
        assert_eq!(get_scalar(VALUES, "service.port").unwrap(), "80");
        assert_eq!(
            get_scalar(VALUES, "ingress.hosts[0].host").unwrap(),
            "api.example.com"
        );
        let unchanged = set_scalar(&edit.content, "image.tag", "a1b2c3d").unwrap();
        assert!(!unchanged.changed);
        assert_eq!(unchanged.summary, "image.tag is already a1b2c3d");
    }

    #[test]
    fn missing_and_unsupported_paths_say_which() {
        for (path, kind) in [
            ("image.digest", "no key image.digest"),
            ("image", "is a mapping or a sequence, not a scalar"),
            ("image.tag.more", "image.tag is a scalar"),
            ("ingress.hosts[3].host", "out of range (1 items)"),
            ("nope.tag", "no key nope"),
        ] {
            let error = set_scalar(VALUES, path, "x").unwrap_err();
            assert!(matches!(error, EditError::NotFound(_)), "{path}: {error}");
            assert!(error.to_string().contains(kind), "{path}: {error}");
        }
        // Flow collections and block scalars are refused, never rewritten.
        let error = set_scalar(VALUES, "ingress.hosts[0].paths", "x").unwrap_err();
        assert!(matches!(error, EditError::Unsupported(_)), "{error}");
        let error = set_scalar(VALUES, "resources.limits", "x").unwrap_err();
        assert!(matches!(error, EditError::Unsupported(_)), "{error}");
        let block = "script: |\n  echo hi\ntag: v1\n";
        let error = set_scalar(block, "script", "x").unwrap_err();
        assert!(error.to_string().contains("block scalar"), "{error}");
        assert_eq!(
            set_scalar(block, "tag", "v2").unwrap().content,
            "script: |\n  echo hi\ntag: v2\n"
        );
        let anchored = "tag: &t v1\nother: *t\n";
        assert!(set_scalar(anchored, "tag", "v2").is_err());
        assert!(set_scalar(anchored, "other", "v2").is_err());
        assert!(set_scalar(VALUES, "", "x").is_err());
        assert!(set_scalar(VALUES, "a[", "x").is_err());
    }

    const MANIFESTS: &str = "\
apiVersion: v1
kind: Service
metadata:
  name: api
spec:
  ports:
  - port: 80
---
# The workload.
apiVersion: apps/v1
kind: Deployment
metadata:
  name: api
  annotations:
    app.kubernetes.io/version: 9f2c1ab
spec:
  replicas: 2
  template:
    spec:
      initContainers:
      - name: migrate
        image: ghcr.io/acme/api:9f2c1ab   # same image, runs migrations
        args: [\"migrate\"]
      containers:
      - name: api
        image: ghcr.io/acme/api:9f2c1ab
        ports:
        - containerPort: 8080
      - name: proxy
        image: envoyproxy/envoy:v1.31.2
";

    #[test]
    fn a_path_through_sequences_reaches_one_container_in_a_multi_document_file() {
        let path = "spec.template.spec.containers[0].image";
        let current = get_scalar(MANIFESTS, path).unwrap();
        assert_eq!(current, "ghcr.io/acme/api:9f2c1ab");
        let value = target_value(&current, "ghcr.io/acme/api", "a1b2c3d", "");
        assert_eq!(value, "ghcr.io/acme/api:a1b2c3d");
        let edit = set_scalar(MANIFESTS, path, &value).unwrap();
        // The init container (same image, other path) and the proxy stay.
        assert_eq!(
            edit.content,
            MANIFESTS.replacen(
                "        image: ghcr.io/acme/api:9f2c1ab\n        ports:",
                "        image: ghcr.io/acme/api:a1b2c3d\n        ports:",
                1
            )
        );
        use serde::Deserialize;
        let documents: Vec<serde_yaml_ng::Value> =
            serde_yaml_ng::Deserializer::from_str(&edit.content)
                .map(|document| serde_yaml_ng::Value::deserialize(document).unwrap())
                .collect();
        assert_eq!(documents.len(), 2);
        let containers = &documents[1]["spec"]["template"]["spec"];
        assert_eq!(
            containers["containers"][0]["image"],
            "ghcr.io/acme/api:a1b2c3d"
        );
        assert_eq!(
            containers["containers"][1]["image"],
            "envoyproxy/envoy:v1.31.2"
        );
        assert_eq!(
            containers["initContainers"][0]["image"],
            "ghcr.io/acme/api:9f2c1ab"
        );

        // The second container, and a key with dots through the bracket form.
        let proxy = set_scalar(MANIFESTS, "spec.template.spec.containers[1].image", "x:y").unwrap();
        assert!(proxy.content.contains("        image: x:y\n"));
        let annotated = set_scalar(
            MANIFESTS,
            "metadata.annotations[\"app.kubernetes.io/version\"]",
            "a1b2c3d",
        )
        .unwrap();
        assert!(
            annotated
                .content
                .contains("    app.kubernetes.io/version: a1b2c3d\n")
        );
        // The first document is searched first: `spec.ports[0].port` is there.
        assert_eq!(get_scalar(MANIFESTS, "spec.ports[0].port").unwrap(), "80");
    }

    #[test]
    fn argo_cd_applications_and_windows_line_endings_survive() {
        let application = "apiVersion: argoproj.io/v1alpha1\r\nkind: Application\r\nspec:\r\n  source:\r\n    repoURL: https://github.com/acme/fleet\r\n    targetRevision: 'v1.4.0'  # release\r\n    helm:\r\n      parameters:\r\n        - name: image.tag\r\n          value: v1.4.0\r\n";
        let edit = set_scalar(application, "spec.source.targetRevision", "v1.5.0").unwrap();
        assert!(
            edit.content
                .contains("    targetRevision: 'v1.5.0'  # release\r\n")
        );
        let edit = set_scalar(
            &edit.content,
            "spec.source.helm.parameters[0].value",
            "v1.5.0",
        )
        .unwrap();
        assert!(edit.content.contains("          value: v1.5.0\r\n"));
        assert_eq!(
            edit.content.matches("\r\n").count(),
            application.matches("\r\n").count()
        );
        assert!(!edit.content.replace("\r\n", "").contains('\n'));

        // A key with no value yet gets one, before its comment.
        let empty = "image:\n  tag:   # set by SDI\n  name: api\n";
        let edit = set_scalar(empty, "image.tag", "v1").unwrap();
        assert_eq!(
            edit.content,
            "image:\n  tag: v1   # set by SDI\n  name: api\n"
        );
        // A file without a trailing newline keeps not having one.
        let edit = set_scalar("image:\n  tag: v1", "image.tag", "v2").unwrap();
        assert_eq!(edit.content, "image:\n  tag: v2");
        // A single quote inside a single-quoted value is doubled.
        let edit = set_scalar("note: 'old'\n", "note", "it's").unwrap();
        assert_eq!(edit.content, "note: 'it''s'\n");
        assert_eq!(get_scalar(&edit.content, "note").unwrap(), "it's");
    }

    #[test]
    fn target_values_follow_what_the_file_holds() {
        let digest = format!("sha256:{}", "a".repeat(64));
        let image = "ghcr.io/acme/api";
        // A bare tag stays a tag.
        assert_eq!(
            target_value("9f2c1ab", image, "a1b2c3d", &digest),
            "a1b2c3d"
        );
        assert_eq!(target_value("", image, "a1b2c3d", ""), "a1b2c3d");
        // A full reference stays a full reference.
        assert_eq!(
            target_value("ghcr.io/acme/api:9f2c1ab", image, "a1b2c3d", ""),
            "ghcr.io/acme/api:a1b2c3d"
        );
        // A digest pin stays a digest pin when the digest is known ...
        assert_eq!(
            target_value(
                &format!("{image}@sha256:{}", "b".repeat(64)),
                image,
                "a1b2c3d",
                &digest
            ),
            format!("{image}@{digest}")
        );
        assert_eq!(
            target_value(
                &format!("{image}:old@sha256:{}", "b".repeat(64)),
                image,
                "a1b2c3d",
                &digest
            ),
            format!("{image}:a1b2c3d@{digest}")
        );
        // ... and falls back to the tag when it is not.
        assert_eq!(
            target_value(
                &format!("{image}@sha256:{}", "b".repeat(64)),
                image,
                "a1b2c3d",
                ""
            ),
            "ghcr.io/acme/api:a1b2c3d"
        );
        // A bare digest is replaced by the digest.
        assert_eq!(
            target_value(
                &format!("sha256:{}", "b".repeat(64)),
                image,
                "a1b2c3d",
                &digest
            ),
            digest
        );
        // The image without a tag gets one.
        assert_eq!(
            target_value(image, image, "a1b2c3d", ""),
            "ghcr.io/acme/api:a1b2c3d"
        );
        // Another image's reference is not rewritten into ours.
        assert_eq!(
            target_value("envoyproxy/envoy:v1", image, "a1b2c3d", ""),
            "a1b2c3d"
        );
        // A registry port is not a tag.
        assert_eq!(
            image_repository("localhost:5000/acme/api:v1"),
            "localhost:5000/acme/api"
        );
        assert_eq!(
            image_repository("localhost:5000/acme/api"),
            "localhost:5000/acme/api"
        );
        assert_eq!(
            image_repository("ghcr.io/acme/api@sha256:abc"),
            "ghcr.io/acme/api"
        );
    }
}

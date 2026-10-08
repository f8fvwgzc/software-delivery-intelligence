//! Documentation: README, architecture docs, ADRs and docs/. Sections are
//! chunked individually so retrieval lands on the paragraph, not the file.

use crate::detectors::{Ctx, attrs};
use crate::model::*;
use crate::util::{basename, short_path};

fn is_readme(path: &str) -> bool {
    let lower = basename(path).to_ascii_lowercase();
    lower.starts_with("readme") && !path.contains('/')
}

fn is_architecture(path: &str) -> bool {
    let lower = basename(path).to_ascii_lowercase();
    lower.starts_with("architecture")
        || lower.starts_with("adr")
        || path.to_ascii_lowercase().contains("/adr/")
        || path.to_ascii_lowercase().contains("docs/architecture")
}

fn is_doc(path: &str) -> bool {
    (path.starts_with("docs/") || path.starts_with("doc/")) && path.ends_with(".md")
}

pub(crate) fn detect(ctx: &mut Ctx<'_>) {
    let repo = ctx.repo_key.clone();
    for (path, content) in ctx.files_where(is_readme) {
        let title = heading(content).unwrap_or_else(|| basename(path).to_string());
        ctx.detected(
            FactCategory::Documentation,
            "docs:readme",
            "README",
            &format!("{} lines", content.lines().count()),
            path,
            &title,
        );
        document_node(ctx, path, &title, "readme");
        ctx.edge(
            &repo,
            &format!("document:{path}"),
            "DOCUMENTED_BY",
            RelationCategory::Source,
        );
        chunk_sections(ctx, path, &title, content, 24);
    }
    if !ctx.has_fact("docs:readme") && ctx.tree_has(is_readme) {
        let p = ctx
            .tree_paths()
            .find(|p| is_readme(p))
            .unwrap_or("")
            .to_string();
        ctx.detected(
            FactCategory::Documentation,
            "docs:readme",
            "README",
            "present",
            &p,
            "README in tree",
        );
    }

    let arch: Vec<(&str, &str)> = ctx.files_where(is_architecture);
    for (path, content) in &arch {
        let title = heading(content).unwrap_or_else(|| short_path(path, 2));
        if !ctx.has_fact("docs:architecture") {
            ctx.detected(
                FactCategory::Documentation,
                "docs:architecture",
                "Architecture documentation",
                &short_path(path, 2),
                path,
                &title,
            );
        }
        document_node(ctx, path, &title, "architecture");
        ctx.edge(
            &repo,
            &format!("document:{path}"),
            "DOCUMENTED_BY",
            RelationCategory::Source,
        );
        chunk_sections(ctx, path, &title, content, 16);
    }
    if !ctx.has_fact("docs:architecture") && ctx.tree_has(is_architecture) {
        let p = ctx
            .tree_paths()
            .find(|p| is_architecture(p))
            .unwrap_or("")
            .to_string();
        ctx.detected(
            FactCategory::Documentation,
            "docs:architecture",
            "Architecture documentation",
            &short_path(&p, 2),
            &p,
            "architecture document in tree",
        );
    }

    let docs: Vec<(&str, &str)> = ctx
        .files_where(|p| is_doc(p) && !is_architecture(p))
        .into_iter()
        .take(12)
        .collect();
    if !docs.is_empty() {
        ctx.detected(
            FactCategory::Documentation,
            "docs:folder",
            "Documentation folder",
            &format!(
                "{} markdown files",
                ctx.tree_paths().filter(|p| is_doc(p)).count()
            ),
            docs[0].0,
            heading(docs[0].1).as_deref().unwrap_or("docs/"),
        );
    }
    for (path, content) in docs {
        let title = heading(content).unwrap_or_else(|| short_path(path, 2));
        document_node(ctx, path, &title, "docs");
        ctx.edge(
            &repo,
            &format!("document:{path}"),
            "DOCUMENTED_BY",
            RelationCategory::Source,
        );
        chunk_sections(ctx, path, &title, content, 8);
    }
}

fn document_node(ctx: &mut Ctx<'_>, path: &str, title: &str, kind: &str) {
    ctx.node(
        NodeKind::Document,
        &format!("document:{path}"),
        title,
        &format!("{kind} document"),
        path,
        attrs(&[("kind", kind)]),
    );
}

fn heading(content: &str) -> Option<String> {
    content
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with('#'))
        .map(|l| l.trim_start_matches('#').trim().to_string())
        .filter(|l| !l.is_empty())
}

/// Splits Markdown on `#`/`##` headings; the preamble before the first
/// heading is its own chunk. Code fences are respected so a `# comment`
/// inside a shell block does not start a section.
pub(crate) fn chunk_sections(
    ctx: &mut Ctx<'_>,
    path: &str,
    doc_title: &str,
    content: &str,
    max: usize,
) {
    let mut sections: Vec<(String, String)> = Vec::new();
    let mut current_title = doc_title.to_string();
    let mut current = String::new();
    let mut in_fence = false;
    for line in content.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
        }
        let is_heading = !in_fence && (line.starts_with("# ") || line.starts_with("## "));
        if is_heading {
            if !current.trim().is_empty() {
                sections.push((current_title.clone(), current.clone()));
            }
            current_title = line.trim_start_matches('#').trim().to_string();
            current = String::new();
        }
        current.push_str(line);
        current.push('\n');
    }
    if !current.trim().is_empty() {
        sections.push((current_title, current));
    }
    for (title, body) in sections.into_iter().take(max) {
        let body = scrub(&body);
        // A heading with nothing under it is not worth an embedding.
        if body.trim().len() < 40 {
            continue;
        }
        ctx.chunk(
            ChunkKind::Document,
            path,
            &format!("{} › {}", short_path(path, 1), title),
            &body,
        );
    }
}

/// Drops lines that look like credentials pasted into docs.
fn scrub(text: &str) -> String {
    text.lines()
        .filter(|l| {
            let lower = l.to_ascii_lowercase();
            !(lower.contains("password=")
                || lower.contains("secret=")
                || lower.contains("token=")
                || lower.contains("api_key="))
                || lower.contains("example")
                || lower.contains("<")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

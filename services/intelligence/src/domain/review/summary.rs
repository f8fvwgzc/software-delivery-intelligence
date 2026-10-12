//! The walkthrough (CONTRACT.md §20): intent, what changed per file group,
//! impacted components, risk, and what to verify. Templated from the facts
//! only; when the workspace reasoner is Claude the same structured context
//! (never file contents) is handed over to rewrite it in prose.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::{
    input::{ChangedFile, ReviewInput},
    rules::{Finding, Severity},
};

/// Most file groups and files listed per group before "…".
const MAX_GROUPS: usize = 12;
const MAX_FILES_PER_GROUP: usize = 6;
const MAX_INTENT_CHARS: usize = 400;

/// Top-level directory (two levels for `services/x`, `crates/x`, `apps/x`,
/// `packages/x`), or the file itself at the root.
pub fn group_of(path: &str) -> String {
    let parts: Vec<&str> = path.split('/').collect();
    match parts.as_slice() {
        [file] => (*file).to_owned(),
        [first, second, _, ..]
            if matches!(
                *first,
                "services" | "crates" | "apps" | "packages" | "libs" | "modules"
            ) =>
        {
            format!("{first}/{second}")
        }
        [first, ..] => (*first).to_owned(),
        [] => String::new(),
    }
}

struct Group {
    files: Vec<String>,
    additions: u32,
    deletions: u32,
    added: usize,
    removed: usize,
}

fn groups(files: &[ChangedFile]) -> BTreeMap<String, Group> {
    let mut groups: BTreeMap<String, Group> = BTreeMap::new();
    for file in files {
        let group = groups.entry(group_of(&file.path)).or_insert(Group {
            files: Vec::new(),
            additions: 0,
            deletions: 0,
            added: 0,
            removed: 0,
        });
        group.files.push(file.path.clone());
        group.additions += file.additions;
        group.deletions += file.deletions;
        if file.is_added() {
            group.added += 1;
        }
        if file.is_removed() {
            group.removed += 1;
        }
    }
    groups
}

/// The first paragraph of the PR body, cut to a sentence or two.
fn intent_of(input: &ReviewInput) -> String {
    let body = input.pr.body.trim();
    let paragraph = body
        .split("\n\n")
        .map(str::trim)
        .find(|p| {
            !p.is_empty()
                && !p.starts_with('#')
                && !p.starts_with("<!--")
                && !p.starts_with("- [ ]")
        })
        .unwrap_or_default();
    let mut text: String = paragraph
        .lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(" ");
    if text.chars().count() > MAX_INTENT_CHARS {
        text = text
            .chars()
            .take(MAX_INTENT_CHARS)
            .collect::<String>()
            .trim_end()
            .to_owned()
            + "…";
    }
    text
}

fn counts(findings: &[Finding]) -> BTreeMap<Severity, usize> {
    let mut counts = BTreeMap::new();
    for f in findings {
        *counts.entry(f.severity).or_insert(0) += 1;
    }
    counts
}

pub fn word_count(text: &str) -> usize {
    text.split_whitespace().count()
}

fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

/// The templated Markdown walkthrough.
pub fn walkthrough(input: &ReviewInput, findings: &[Finding]) -> String {
    let pr = &input.pr;
    let mut out = String::new();

    // Intent
    out.push_str("## Intent\n\n");
    out.push_str(&format!("**{}**", pr.title.trim()));
    if !pr.author.is_empty() {
        out.push_str(&format!(" by @{}", pr.author));
    }
    if !pr.head_ref.is_empty() && !pr.base_ref.is_empty() {
        out.push_str(&format!(" (`{}` → `{}`)", pr.head_ref, pr.base_ref));
    }
    out.push_str(".\n\n");
    let intent = intent_of(input);
    if intent.is_empty() {
        out.push_str("The pull request carries no description; the intent below is read from the files it touches.\n\n");
    } else {
        out.push_str(&format!("{intent}\n\n"));
    }

    // What changed
    out.push_str("## What changed\n\n");
    let files = &input.files;
    out.push_str(&format!(
        "{} across {} (+{} / −{})",
        plural(pr.changed_files.max(files.len() as u32) as usize, "file"),
        plural(groups(files).len(), "area"),
        pr.additions,
        pr.deletions
    ));
    if input.files_truncated {
        out.push_str(&format!("; only the first {} files were read", files.len()));
    }
    out.push_str(".\n\n");
    for (name, group) in groups(files).iter().take(MAX_GROUPS) {
        let mut shape = Vec::new();
        if group.added > 0 {
            shape.push(format!("{} new", group.added));
        }
        if group.removed > 0 {
            shape.push(format!("{} removed", group.removed));
        }
        let shape = if shape.is_empty() {
            String::new()
        } else {
            format!(", {}", shape.join(", "))
        };
        let listed: Vec<String> = group
            .files
            .iter()
            .take(MAX_FILES_PER_GROUP)
            .map(|p| format!("`{}`", p.strip_prefix(&format!("{name}/")).unwrap_or(p)))
            .collect();
        let more = if group.files.len() > MAX_FILES_PER_GROUP {
            format!(", +{} more", group.files.len() - MAX_FILES_PER_GROUP)
        } else {
            String::new()
        };
        out.push_str(&format!(
            "- **{name}** — {} (+{} / −{}{shape}): {}{more}\n",
            plural(group.files.len(), "file"),
            group.additions,
            group.deletions,
            listed.join(", ")
        ));
    }
    if groups(files).len() > MAX_GROUPS {
        out.push_str(&format!(
            "- … and {} more areas\n",
            groups(files).len() - MAX_GROUPS
        ));
    }
    let truncated_files = files.iter().filter(|f| f.truncated).count();
    if truncated_files > 0 {
        out.push_str(&format!(
            "\n{} {} too large to read in full; the counters are included, the lines were not analyzed.\n",
            plural(truncated_files, "file"),
            if truncated_files == 1 { "was" } else { "were" }
        ));
    }
    out.push('\n');

    // Impacted components
    out.push_str("## Impacted components\n\n");
    if input.impacted_nodes.is_empty() && input.touched_facts.is_empty() {
        out.push_str("The knowledge graph links none of the changed files to a known component; treat the blast radius as unknown.\n\n");
    } else {
        if !input.impacted_nodes.is_empty() {
            let mut by_kind: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
            for node in &input.impacted_nodes {
                by_kind
                    .entry(node.kind.as_str())
                    .or_default()
                    .push(node.label.as_str());
            }
            let parts: Vec<String> = by_kind
                .iter()
                .map(|(kind, labels)| {
                    let shown: Vec<String> =
                        labels.iter().take(6).map(|l| format!("`{l}`")).collect();
                    let more = if labels.len() > 6 {
                        format!(" +{}", labels.len() - 6)
                    } else {
                        String::new()
                    };
                    format!(
                        "{} {}: {}{more}",
                        labels.len(),
                        kind.replace('_', " "),
                        shown.join(", ")
                    )
                })
                .collect();
            out.push_str(&format!(
                "The change reaches {} in the graph — {}.\n\n",
                plural(input.impacted_nodes.len(), "node"),
                parts.join("; ")
            ));
        }
        if !input.touched_facts.is_empty() {
            let labels: Vec<String> = input
                .touched_facts
                .iter()
                .take(8)
                .map(|f| {
                    if f.value.is_empty() || f.value == f.label {
                        format!("`{}`", f.label)
                    } else {
                        format!("`{}` ({})", f.label, f.value)
                    }
                })
                .collect();
            out.push_str(&format!(
                "It edits the evidence behind {}: {}{}.\n\n",
                plural(input.touched_facts.len(), "recorded fact"),
                labels.join(", "),
                if input.touched_facts.len() > 8 {
                    ", …"
                } else {
                    ""
                }
            ));
        }
    }

    // Risk
    out.push_str("## Risk\n\n");
    let counts = counts(findings);
    if findings.is_empty() {
        out.push_str("No analyzer finding. The rules cover secrets, security, dependencies, Docker, CI, IaC, tests, docs, correctness and maturity; they do not replace reading the code.\n\n");
    } else {
        let summary: Vec<String> = [
            Severity::Critical,
            Severity::High,
            Severity::Medium,
            Severity::Low,
            Severity::Info,
        ]
        .iter()
        .filter_map(|s| counts.get(s).map(|n| format!("{n} {}", s.as_str())))
        .collect();
        let level = match findings.iter().map(|f| f.severity).max() {
            Some(Severity::Critical) => "**Critical**",
            Some(Severity::High) => "**High**",
            Some(Severity::Medium) => "Medium",
            Some(Severity::Low) => "Low",
            _ => "Informational",
        };
        out.push_str(&format!(
            "{level} — {} ({}).\n\n",
            plural(findings.len(), "finding"),
            summary.join(", ")
        ));
        for finding in findings
            .iter()
            .filter(|f| f.severity >= Severity::Medium)
            .take(8)
        {
            let at = if finding.line > 0 {
                format!("`{}:{}`", finding.path, finding.line)
            } else {
                format!("`{}`", finding.path)
            };
            out.push_str(&format!(
                "- {} — {} ({})\n",
                at, finding.title, finding.rule
            ));
        }
        if findings
            .iter()
            .filter(|f| f.severity >= Severity::Medium)
            .count()
            > 8
        {
            out.push_str("- … see the findings list for the rest\n");
        }
        out.push('\n');
    }

    // What to verify
    out.push_str("## What to verify\n\n");
    for item in verify_items(input, findings) {
        out.push_str(&format!("- {item}\n"));
    }
    out.trim_end().to_owned()
}

fn verify_items(input: &ReviewInput, findings: &[Finding]) -> Vec<String> {
    let mut items = Vec::new();
    let has = |rule: &str| findings.iter().any(|f| f.rule.starts_with(rule));
    let touches = |pred: fn(&ChangedFile) -> bool| input.files.iter().any(pred);

    if has("secrets.") {
        items.push("Rotate any credential the secrets findings point at; removing it from the diff does not remove it from history.".to_owned());
    }
    if touches(|f| f.is_test()) {
        items.push(
            "Run the test suite and check that the new or changed tests fail without the change."
                .to_owned(),
        );
    } else if touches(|f| f.is_source()) {
        items.push(if input.has_test_framework {
            "No test changed: run the existing suite and decide whether the change needs one."
                .to_owned()
        } else {
            "The project has no detected test suite; exercise the change by hand before merging."
                .to_owned()
        });
    }
    if touches(|f| f.is_workflow()) {
        items.push("Open the Actions run for this head commit and check every job the edited workflow defines actually ran.".to_owned());
    }
    if touches(|f| f.is_dockerfile()) {
        items.push("Build the image locally and start it once; confirm the exposed port and the health endpoint answer.".to_owned());
    }
    if touches(|f| f.is_iac()) {
        items.push("Run a plan (`terraform plan` / `what-if`) against the target environment and read every destroy or replace.".to_owned());
    }
    if touches(|f| f.is_lockfile()) || has("dependencies.") {
        items.push("Reinstall from the lockfile on a clean checkout; check the dependency diff for unexpected transitive upgrades.".to_owned());
    }
    if touches(|f| {
        f.path.contains("migrations/") || f.path.contains("/migrate/") || f.extension() == "sql"
    }) {
        items.push("Apply the migration to a copy of production data and check it is reversible or at least safe to roll forward.".to_owned());
    }
    if !input.impacted_nodes.is_empty() {
        let labels: Vec<String> = input
            .impacted_nodes
            .iter()
            .take(4)
            .map(|n| format!("`{}`", n.label))
            .collect();
        items.push(format!(
            "Smoke-test the impacted components ({}) end to end.",
            labels.join(", ")
        ));
    }
    if touches(|f| f.is_docs()) || has("docs.") {
        items.push("Read the documentation changes against the code: every command, path and variable named must exist after the merge.".to_owned());
    }
    if input.files_truncated {
        items.push(format!(
            "Only the first {} files were analyzed; review the rest on GitHub.",
            input.files.len()
        ));
    }
    if items.is_empty() {
        items.push("Read the diff once more with the intent above in mind; nothing in it triggered a specific check.".to_owned());
    }
    items
}

/// The structured context a reasoner may rewrite the walkthrough from:
/// metadata, file paths with counters, grouped areas, findings, impacted
/// components. No file contents, no diff lines.
pub fn context_json(input: &ReviewInput, findings: &[Finding], draft: &str) -> Value {
    let groups = groups(&input.files);
    json!({
        "project": {
            "name": input.project_name,
            "repository": input.repo_full_name,
            "primary_language": input.primary_language,
            "technologies": input.technologies,
            "has_test_framework": input.has_test_framework,
            "maturity_axes": input.maturity_axes.iter().map(|a| json!({"key": a.key, "label": a.label, "score": a.score})).collect::<Vec<_>>(),
        },
        "pull_request": {
            "number": input.pr.number,
            "title": input.pr.title,
            "description": intent_of(input),
            "author": input.pr.author,
            "base": input.pr.base_ref,
            "head": input.pr.head_ref,
            "changed_files": input.pr.changed_files,
            "additions": input.pr.additions,
            "deletions": input.pr.deletions,
        },
        "areas": groups.iter().map(|(name, g)| json!({
            "area": name,
            "files": g.files,
            "additions": g.additions,
            "deletions": g.deletions,
            "new_files": g.added,
            "removed_files": g.removed,
        })).collect::<Vec<_>>(),
        "files_truncated": input.files_truncated,
        "impacted_components": input.impacted_nodes.iter().map(|n| json!({"label": n.label, "kind": n.kind, "path": n.source_path})).collect::<Vec<_>>(),
        "touched_facts": input.touched_facts.iter().map(|f| json!({"label": f.label, "value": f.value, "path": f.source_path})).collect::<Vec<_>>(),
        "findings": findings.iter().map(|f| json!({
            "rule": f.rule,
            "severity": f.severity.as_str(),
            "title": f.title,
            "path": f.path,
            "line": f.line,
        })).collect::<Vec<_>>(),
        "verify": verify_items(input, findings),
        "draft": draft,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::review::{input::NodeRef, rules::fixtures::*};

    fn sample() -> (ReviewInput, Vec<Finding>) {
        let mut input = input(vec![
            added("Dockerfile", &["FROM node:latest", "CMD [\"x\"]"]),
            modified("services/api/src/main.rs", &["a", "b", "c", "d", "e", "f"]),
            modified("services/api/src/lib.rs", &["x"]),
            modified("README.md", &["docs"]),
        ]);
        input.pr.title = "Containerise the API".into();
        input.pr.body = "<!-- template -->\n\nAdds a Dockerfile so the API can run in the cluster.\n\nMore details here.".into();
        input.pr.author = "ana".into();
        input.pr.base_ref = "main".into();
        input.pr.head_ref = "feat/docker".into();
        input.pr.changed_files = 4;
        input.pr.additions = 10;
        input.has_test_framework = true;
        input.impacted_nodes = vec![NodeRef {
            id: "n1".into(),
            kind: "service".into(),
            label: "API".into(),
            source_path: "services/api".into(),
        }];
        let findings = crate::domain::review::rules::analyze_all(&input);
        (input, findings)
    }

    #[test]
    fn groups_fold_monorepo_paths() {
        assert_eq!(group_of("services/api/src/main.rs"), "services/api");
        assert_eq!(group_of("web/src/App.tsx"), "web");
        assert_eq!(group_of("README.md"), "README.md");
        assert_eq!(group_of("services/api"), "services");
    }

    #[test]
    fn walkthrough_has_the_five_sections_and_reads_the_facts() {
        let (input, findings) = sample();
        let text = walkthrough(&input, &findings);
        for heading in [
            "## Intent",
            "## What changed",
            "## Impacted components",
            "## Risk",
            "## What to verify",
        ] {
            assert!(text.contains(heading), "{heading} missing in:\n{text}");
        }
        assert!(text.contains("**Containerise the API** by @ana (`feat/docker` → `main`)"));
        assert!(text.contains("Adds a Dockerfile so the API can run in the cluster."));
        assert!(!text.contains("template"));
        assert!(text.contains("**services/api** — 2 files"));
        assert!(text.contains("`src/main.rs`"));
        assert!(text.contains("1 service: `API`"));
        assert!(text.contains("docker.latest_tag"));
        assert!(text.contains("Build the image locally"));
        assert!(text.contains("No test changed"));
        assert!(text.contains("Smoke-test the impacted components (`API`)"));
    }

    #[test]
    fn empty_pr_still_reads_sensibly() {
        let input = input(vec![]);
        let text = walkthrough(&input, &[]);
        assert!(text.contains("carries no description"));
        assert!(text.contains("No analyzer finding"));
        assert!(text.contains("treat the blast radius as unknown"));
        assert!(text.contains("nothing in it triggered a specific check"));
    }

    #[test]
    fn context_carries_no_diff_lines() {
        let (input, findings) = sample();
        let draft = walkthrough(&input, &findings);
        let context = context_json(&input, &findings, &draft);
        let text = context.to_string();
        assert!(text.contains("\"rule\":\"docker.latest_tag\""));
        assert!(text.contains("services/api/src/main.rs"));
        // Added lines of the diff never travel.
        assert!(!text.contains("FROM node:latest"));
        assert_eq!(context["pull_request"]["number"], 0);
        assert_eq!(context["areas"].as_array().unwrap().len(), 3);
    }
}

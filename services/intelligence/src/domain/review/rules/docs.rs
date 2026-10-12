//! README and CONTRACT claims against the code: references to files this
//! change removes or renames, and new configuration (environment variables)
//! that no document or example file mentions.

use std::{collections::BTreeSet, sync::LazyLock};

use regex::Regex;

use super::{Finding, Severity, evidence};
use crate::domain::review::input::{ChangedFile, ReviewInput};

/// `std::env::var("X")`, `process.env.X`, `os.environ["X"]`, `os.Getenv("X")`,
/// `ENV["X"]`, `getenv("X")`, `env::var_os("X")`, `Deno.env.get("X")`.
static ENV_VAR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?:env::var(?:_os)?\(|process\.env\.|process\.env\[["']|os\.environ(?:\.get)?\(?\[?["']|os\.Getenv\(|getenv\(|ENV(?:\.fetch)?\(?\[?["']|Deno\.env\.get\(|env_opt\(|env_or\()["']?([A-Z][A-Z0-9_]{2,})"#,
    )
    .expect("env var")
});

fn is_claim_doc(file: &ChangedFile) -> bool {
    let name = file.file_name().to_ascii_uppercase();
    name.starts_with("README") || name.starts_with("CONTRACT") || name.starts_with("ARCHITECTURE")
}

fn documents_config(file: &ChangedFile) -> bool {
    is_claim_doc(file)
        || file.is_docs()
        || file.file_name().starts_with(".env")
        || matches!(
            file.file_name(),
            "docker-compose.yml" | "docker-compose.yaml" | "compose.yml" | "compose.yaml"
        )
}

pub fn analyze(input: &ReviewInput) -> Vec<Finding> {
    let mut findings = Vec::new();

    // Paths that stop existing under their old name.
    let gone: Vec<&str> = input
        .files
        .iter()
        .filter_map(|f| match f.status.as_str() {
            "removed" => Some(f.path.as_str()),
            "renamed" if !f.previous_path.is_empty() => Some(f.previous_path.as_str()),
            _ => None,
        })
        .collect();
    if !gone.is_empty() {
        for doc in input
            .files
            .iter()
            .filter(|f| is_claim_doc(f) && !f.is_removed())
        {
            // Only lines the PR keeps or adds can go stale; removed lines are fine.
            let kept: Vec<(u32, &str)> = doc.added_lines();
            for (line_no, line) in kept {
                for path in &gone {
                    if line.contains(path) {
                        findings.push(Finding::new(
                            "docs",
                            "stale_reference",
                            Severity::Medium,
                            &doc.path,
                            line_no,
                            format!("Documentation points at a removed file: {path}"),
                            format!(
                                "Line {line_no} of `{}` still refers to `{path}`, which this pull request removes or renames: {}. \
                                 Update the reference or drop the claim.",
                                doc.path,
                                evidence(line)
                            ),
                        ));
                    }
                }
            }
        }
        // Claims in documents the PR does not touch cannot be checked here
        // without the tree; a removed README reference elsewhere is the
        // summary's "what to verify" item, not a finding.
    }

    // New environment variables read by code but documented nowhere in the change.
    let documented: BTreeSet<String> = input
        .files
        .iter()
        .filter(|f| documents_config(f) && !f.is_removed())
        .flat_map(|f| f.added_lines().into_iter().map(|(_, l)| l.to_owned()))
        .collect();
    let mentions = |name: &str| documented.iter().any(|l| l.contains(name));
    let mut reported: BTreeSet<String> = BTreeSet::new();
    for file in input
        .files
        .iter()
        .filter(|f| f.is_source() && !f.is_removed())
    {
        for (line_no, line) in file.added_lines() {
            for caps in ENV_VAR.captures_iter(line) {
                let name = caps[1].to_owned();
                if reported.contains(&name)
                    || mentions(&name)
                    || name == "NODE_ENV"
                    || name == "RUST_LOG"
                    || name == "HOME"
                    || name == "PATH"
                {
                    continue;
                }
                reported.insert(name.clone());
                findings.push(Finding::new(
                    "docs",
                    "undocumented_env",
                    Severity::Low,
                    &file.path,
                    line_no,
                    format!("New environment variable not documented: {name}"),
                    format!(
                        "Line {line_no} of `{}` reads `{name}`, and neither the README, CONTRACT, `.env.example` nor a \
                         compose file in this change mentions it. Document what it does and its default, or add it to \
                         `.env.example`.",
                        file.path
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
    fn readme_keeps_pointing_at_a_removed_file() {
        let findings = analyze(&input(vec![
            removed("scripts/deploy.sh"),
            modified(
                "README.md",
                &["Run `scripts/deploy.sh` to ship.", "Then open the app."],
            ),
        ]));
        assert_eq!(rules(&findings), vec!["docs.stale_reference"]);
        assert_eq!(findings[0].line, 1);
    }

    #[test]
    fn contract_mentions_the_old_name_of_a_renamed_file() {
        let mut renamed = modified("services/api/src/routes.rs", &["x"]);
        renamed.status = "renamed".into();
        renamed.previous_path = "services/api/src/handlers.rs".into();
        let findings = analyze(&input(vec![
            renamed,
            modified(
                "CONTRACT.md",
                &["Handlers live in `services/api/src/handlers.rs`."],
            ),
        ]));
        assert_eq!(rules(&findings), vec!["docs.stale_reference"]);
        assert!(findings[0].title.contains("handlers.rs"));
    }

    #[test]
    fn new_env_vars_without_documentation() {
        let findings = analyze(&input(vec![
            added(
                "src/config.rs",
                &[
                    "let url = std::env::var(\"PAYMENTS_URL\")?;",
                    "let again = std::env::var(\"PAYMENTS_URL\")?;",
                ],
            ),
            added(
                "web/src/env.ts",
                &["const key = process.env.PUBLIC_MAPS_KEY;"],
            ),
        ]));
        assert_eq!(
            rules(&findings),
            vec!["docs.undocumented_env", "docs.undocumented_env"]
        );
        assert!(findings[0].title.contains("PAYMENTS_URL"));
        assert!(findings[1].title.contains("PUBLIC_MAPS_KEY"));
    }

    #[test]
    fn documented_vars_and_updated_references_are_clean() {
        let findings = analyze(&input(vec![
            removed("scripts/deploy.sh"),
            file(
                "README.md",
                "modified",
                &["Run `make deploy` to ship."],
                &["Run `scripts/deploy.sh` to ship."],
            ),
            added(
                "src/config.rs",
                &["let url = std::env::var(\"PAYMENTS_URL\")?;"],
            ),
            added(".env.example", &["PAYMENTS_URL=http://localhost:9000"]),
            added("tests/config.rs", &["std::env::var(\"ONLY_IN_TESTS\")"]),
        ]));
        assert!(findings.is_empty(), "{findings:?}");
    }
}

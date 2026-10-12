//! Whether the change moves a delivery-maturity axis (CONTRACT.md §13):
//! adding a Dockerfile, a workflow, tests or IaC raises one; removing them
//! lowers it. Informational, so the walkthrough can say "this PR adds CI".

use super::{Finding, Severity};
use crate::domain::review::input::{ChangedFile, ReviewInput};

/// `(axis key, label, what counts as a signal file)`.
type Signal = fn(&ChangedFile) -> bool;

const AXES: &[(&str, &str, Signal)] = &[
    ("containerization", "Containerization", |f| {
        f.is_dockerfile()
            || matches!(
                f.file_name(),
                "docker-compose.yml" | "docker-compose.yaml" | "compose.yml" | "compose.yaml"
            )
    }),
    ("ci", "CI", |f| {
        f.is_workflow()
            || matches!(
                f.file_name(),
                ".gitlab-ci.yml" | "Jenkinsfile" | "azure-pipelines.yml" | ".circleci/config.yml"
            )
    }),
    ("tests", "Tests", |f| f.is_test()),
    ("iac", "Infrastructure as code", |f| {
        matches!(f.extension(), "tf" | "bicep")
            || f.path.starts_with("k8s/")
            || f.path.starts_with("helm/")
            || f.path.starts_with("kubernetes/")
    }),
    ("dependencies", "Dependency hygiene", |f| {
        matches!(
            f.file_name(),
            "dependabot.yml" | "renovate.json" | "renovate.json5" | ".renovaterc"
        ) || f.is_lockfile()
    }),
    ("docs", "Documentation", |f| {
        f.file_name().to_ascii_uppercase().starts_with("README")
            || f.file_name()
                .to_ascii_uppercase()
                .starts_with("CONTRIBUTING")
            || f.file_name()
                .to_ascii_uppercase()
                .starts_with("ARCHITECTURE")
    }),
    ("secrets", "Secret handling", |f| {
        f.file_name() == ".env.example" || f.file_name() == ".env.sample"
    }),
];

pub fn analyze(input: &ReviewInput) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (key, label, signal) in AXES {
        let added: Vec<&ChangedFile> = input
            .files
            .iter()
            .filter(|f| signal(f) && f.is_added())
            .collect();
        let removed: Vec<&ChangedFile> = input
            .files
            .iter()
            .filter(|f| signal(f) && f.is_removed())
            .collect();
        let score = input.axis(key).map(|a| a.score);
        let score_text = score.map_or(String::new(), |s| {
            format!(" (scored {s}/100 at the last scan)")
        });
        if !added.is_empty() && removed.len() < added.len() {
            let first = added[0];
            findings.push(Finding::new(
                "maturity",
                "up",
                Severity::Info,
                &first.path,
                first.first_line(),
                format!("Raises the {label} axis"),
                format!(
                    "This change adds {} ({}), which the {label} axis{score_text} rewards. The next scan will rescore it.",
                    describe(added.len(), key),
                    added.iter().take(3).map(|f| format!("`{}`", f.path)).collect::<Vec<_>>().join(", ")
                ),
            ));
        } else if !removed.is_empty() && added.len() < removed.len() {
            let first = removed[0];
            findings.push(Finding::new(
                "maturity",
                "down",
                Severity::Low,
                &first.path,
                0,
                format!("Lowers the {label} axis"),
                format!(
                    "This change removes {} ({}), which the {label} axis{score_text} counts. If that is intended, say so in \
                     the description; otherwise restore or replace it.",
                    describe(removed.len(), key),
                    removed.iter().take(3).map(|f| format!("`{}`", f.path)).collect::<Vec<_>>().join(", ")
                ),
            ));
        }
    }
    // A committed .env (not the example) lowers secret handling whatever else happens.
    for file in input.files.iter().filter(|f| {
        f.is_added()
            && f.file_name().starts_with(".env")
            && !f.file_name().contains("example")
            && !f.file_name().contains("sample")
    }) {
        findings.push(Finding::new(
            "maturity",
            "down",
            Severity::Low,
            &file.path,
            0,
            "Lowers the Secret handling axis",
            format!(
                "`{}` is committed; environment files hold real configuration and often secrets. Keep it out of the \
                 repository (`.gitignore`) and commit a `.env.example` instead.",
                file.path
            ),
        ));
    }
    findings
}

fn describe(count: usize, key: &str) -> String {
    let what = match key {
        "containerization" => "container file",
        "ci" => "pipeline definition",
        "tests" => "test file",
        "iac" => "infrastructure definition",
        "dependencies" => "dependency-management file",
        "docs" => "documentation file",
        _ => "secret-handling file",
    };
    format!("{count} {what}{}", if count == 1 { "" } else { "s" })
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::*;
    use super::*;
    use crate::domain::review::input::AxisScore;

    #[test]
    fn adding_a_dockerfile_and_a_workflow_raises_two_axes() {
        let mut input = input(vec![
            added("Dockerfile", &["FROM node:22"]),
            added(".github/workflows/ci.yml", &["name: ci"]),
        ]);
        input.maturity_axes = vec![AxisScore {
            key: "ci".into(),
            label: "CI".into(),
            score: 10,
        }];
        let findings = analyze(&input);
        assert_eq!(rules(&findings), vec!["maturity.up", "maturity.up"]);
        assert!(findings[0].title.contains("Containerization"));
        assert!(findings[1].detail.contains("scored 10/100"));
        assert!(findings.iter().all(|f| f.severity == Severity::Info));
    }

    #[test]
    fn removing_tests_lowers_the_axis() {
        let findings = analyze(&input(vec![
            removed("tests/api.rs"),
            removed("tests/web.rs"),
            added("tests/new.rs", &["#[test] fn t() {}"]),
        ]));
        assert_eq!(rules(&findings), vec!["maturity.down"]);
        assert!(findings[0].detail.contains("2 test files"));
        assert_eq!(findings[0].line, 0);
    }

    #[test]
    fn committed_env_file_lowers_secret_handling() {
        let findings = analyze(&input(vec![
            added(".env", &["DB_PASSWORD=x"]),
            added(".env.example", &["DB_PASSWORD="]),
        ]));
        assert_eq!(rules(&findings), vec!["maturity.up", "maturity.down"]);
        assert!(findings[1].title.contains("Secret handling"));
    }

    #[test]
    fn plain_source_changes_move_nothing() {
        let findings = analyze(&input(vec![
            modified("src/main.rs", &["x"]),
            modified("Dockerfile", &["RUN apt-get update"]),
            modified("tests/api.rs", &["y"]),
        ]));
        assert!(findings.is_empty(), "{findings:?}");
    }
}

//! Maps facts onto the CONTRACT.md §5.1 capability vocabulary. Only facts
//! strong enough to act on (confidence ≥ 0.8) count: a cloud guessed from an
//! SDK must not make a pattern "fit".

use std::collections::BTreeSet;

use crate::model::*;

const MIN_CONFIDENCE: f64 = 0.8;

/// Fact key → capability key. Keys that map to themselves are listed so the
/// vocabulary stays explicit and nothing leaks through by accident.
fn capability_for(key: &str) -> Option<String> {
    for prefix in [
        "lang:",
        "framework:",
        "package:",
        "cloud:",
        "ci:github_actions",
        "ci:gitlab_ci",
    ] {
        if key.starts_with(prefix) && !key.starts_with("ci:workflow:") {
            return Some(key.to_string());
        }
    }
    if let Some(engine) = key.strip_prefix("datastore:") {
        if engine.starts_with("orm:") || engine.starts_with("configured:") {
            return None;
        }
        return Some(format!("database:{engine}"));
    }
    if let Some(engine) = key.strip_prefix("infra:datastore:") {
        return Some(format!("database:{engine}"));
    }
    if key.starts_with("container:dockerfile:") {
        return Some("dockerfile".into());
    }
    if key.starts_with("container:compose:") {
        return Some("docker_compose".into());
    }
    if key.starts_with("iac:terraform:") {
        return None;
    }
    // Delivery (CONTRACT.md §22): the GitOps controller that reconciles the
    // project and the registries its pipelines push to. Jenkins, Azure
    // Pipelines and GitHub environments stay facts: no pattern requires them.
    if key.starts_with("registry:") {
        return Some(key.to_string());
    }
    Some(
        match key {
            "security:lockfile" => "lockfile",
            "security:env_example" => "env_example",
            "security:dependabot" => "dependabot",
            "iac:terraform" | "iac:bicep" | "iac:pulumi" | "iac:cloudformation" => key,
            "k8s:manifests" | "k8s:helm" => key,
            "cd:flux" | "cd:argocd" => key,
            "tests:unit" | "tests:e2e" => key,
            "repository:static_site" => "static_site",
            "repository:monorepo" => "monorepo",
            "docs:readme" | "docs:architecture" => key,
            k if k.starts_with("migrations:") => "migrations",
            _ => return None,
        }
        .to_string(),
    )
}

pub(crate) fn derive(facts: &[FactDraft]) -> Vec<String> {
    let set: BTreeSet<String> = facts
        .iter()
        .filter(|f| f.confidence >= MIN_CONFIDENCE)
        .filter_map(|f| capability_for(&f.key))
        .collect();
    set.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_vocabulary() {
        assert_eq!(capability_for("lang:rust").as_deref(), Some("lang:rust"));
        assert_eq!(
            capability_for("datastore:postgres").as_deref(),
            Some("database:postgres")
        );
        assert_eq!(capability_for("datastore:orm:knex"), None);
        assert_eq!(
            capability_for("container:dockerfile:Dockerfile").as_deref(),
            Some("dockerfile")
        );
        assert_eq!(capability_for("ci:workflow:x.yml"), None);
        assert_eq!(
            capability_for("migrations:sqlx").as_deref(),
            Some("migrations")
        );
        assert_eq!(capability_for("iac:terraform:provider:azurerm"), None);
        assert_eq!(capability_for("cd:flux").as_deref(), Some("cd:flux"));
        assert_eq!(capability_for("cd:argocd").as_deref(), Some("cd:argocd"));
        assert_eq!(
            capability_for("registry:ghcr").as_deref(),
            Some("registry:ghcr")
        );
        assert_eq!(capability_for("cd:jenkins"), None);
        assert_eq!(capability_for("cd:github_environment:production"), None);
        assert_eq!(
            capability_for("ci:build_image:.github/workflows/ci.yml"),
            None
        );
    }

    #[test]
    fn a_guessed_registry_is_not_a_capability() {
        let fact = |kind, confidence| FactDraft {
            kind,
            category: FactCategory::Container,
            key: "registry:ghcr".into(),
            label: String::new(),
            value: String::new(),
            confidence,
            source_path: String::new(),
            evidence: String::new(),
        };
        assert!(derive(&[fact(FactKind::Inferred, 0.6)]).is_empty());
        assert_eq!(derive(&[fact(FactKind::Detected, 1.0)]), ["registry:ghcr"]);
    }
}

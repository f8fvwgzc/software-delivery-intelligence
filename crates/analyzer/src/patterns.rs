//! Ways of working the repository already follows. These feed pattern
//! memory so a second project with the same shape gets the same proposal.

use crate::detectors::Ctx;
use crate::model::*;
use crate::util::display;

fn pattern(key: &str, name: &str, summary: &str, category: &str) -> LearnedPatternDraft {
    LearnedPatternDraft {
        key: key.to_string(),
        name: name.to_string(),
        summary: summary.to_string(),
        category: category.to_string(),
        ..Default::default()
    }
}

pub(crate) fn derive(ctx: &Ctx<'_>, capabilities: &[String]) -> Vec<LearnedPatternDraft> {
    let has = |c: &str| capabilities.iter().any(|x| x == c);
    let cloud = ["azure", "aws", "gcp"]
        .into_iter()
        .find(|c| has(&format!("cloud:{c}")))
        .map(String::from)
        .unwrap_or_default();
    let iac = ["terraform", "bicep", "pulumi", "cloudformation"]
        .into_iter()
        .find(|i| has(&format!("iac:{i}")))
        .map(String::from)
        .unwrap_or_default();
    let ci = if has("ci:github_actions") {
        "github_actions"
    } else if has("ci:gitlab_ci") {
        "gitlab_ci"
    } else {
        ""
    };
    let runtime = if has("dockerfile") {
        "container"
    } else if has("static_site") {
        "static"
    } else {
        ""
    };
    let mut out = Vec::new();

    if has("dockerfile") && !ci.is_empty() {
        let mut p = pattern(
            &format!("deployment:docker+{ci}"),
            &format!("Container built and shipped by {}", display(ci)),
            "The repository builds a container image and drives delivery from its CI pipeline.",
            "deployment",
        );
        p.technologies = vec!["docker".into(), ci.into()];
        p.cloud = cloud.clone();
        p.runtime = "container".into();
        p.iac = iac.clone();
        p.delivery = ci.into();
        p.requires = vec!["dockerfile".into(), format!("ci:{ci}")];
        out.push(p);
    }
    if !iac.is_empty() {
        let key = if cloud.is_empty() {
            format!("infrastructure:{iac}")
        } else {
            format!("infrastructure:{iac}+{cloud}")
        };
        let mut p = pattern(
            &key,
            &format!(
                "{} managed with {}",
                if cloud.is_empty() {
                    "Infrastructure".to_string()
                } else {
                    display(&cloud)
                },
                display(&iac)
            ),
            "Infrastructure is declared as code and versioned next to the application.",
            "infrastructure",
        );
        p.technologies = vec![iac.clone()];
        if !cloud.is_empty() {
            p.technologies.push(cloud.clone());
        }
        p.cloud = cloud.clone();
        p.iac = iac.clone();
        p.runtime = runtime.into();
        p.delivery = ci.into();
        p.requires = vec![format!("iac:{iac}")];
        out.push(p);
    }
    if has("k8s:manifests") || has("k8s:helm") {
        let helm = has("k8s:helm");
        let mut p = pattern(
            if helm {
                "deployment:kubernetes+helm"
            } else {
                "deployment:kubernetes"
            },
            if helm {
                "Kubernetes with Helm"
            } else {
                "Kubernetes manifests"
            },
            "Workloads are deployed to Kubernetes from manifests kept in the repository.",
            "deployment",
        );
        p.technologies = vec!["kubernetes".into()];
        if helm {
            p.technologies.push("helm".into());
        }
        p.cloud = cloud.clone();
        p.runtime = "kubernetes".into();
        p.iac = iac.clone();
        p.delivery = ci.into();
        p.requires = vec!["dockerfile".into(), "k8s:manifests".into()];
        out.push(p);
    }
    if ctx.deploy_targets.contains("github_pages") {
        let mut p = pattern(
            "deployment:github_pages",
            "Static site on GitHub Pages",
            "A static build is published to GitHub Pages from the workflow.",
            "deployment",
        );
        p.technologies = vec!["github_pages".into(), "github_actions".into()];
        p.runtime = "static".into();
        p.delivery = "github_actions".into();
        p.requires = vec!["ci:github_actions".into()];
        out.push(p);
    }
    if !ci.is_empty() {
        let tests = has("tests:unit");
        let mut p = pattern(
            &format!("ci:{ci}"),
            &format!("{} pipeline", display(ci)),
            if tests {
                "Every push is linted, tested and built by the CI pipeline."
            } else {
                "A CI pipeline builds the project on every push."
            },
            "ci_cd",
        );
        p.technologies = vec![ci.into()];
        p.delivery = ci.into();
        p.runtime = runtime.into();
        p.requires = vec![format!("ci:{ci}")];
        out.push(p);
    }
    for suite in ctx.fact_suffixes("testing:") {
        if matches!(suite.as_str(), "ci" | "files" | "script") {
            continue;
        }
        let e2e = ctx
            .fact(&format!("testing:{suite}"))
            .map(|f| f.value == "end-to-end")
            .unwrap_or(false);
        let mut p = pattern(
            &format!("testing:{suite}"),
            &format!(
                "{} {}",
                display(&suite),
                if e2e { "end-to-end tests" } else { "tests" }
            ),
            &format!("Tests are written with {}.", display(&suite)),
            "testing",
        );
        p.technologies = vec![suite.clone()];
        p.requires = vec![if e2e {
            "tests:e2e".into()
        } else {
            "tests:unit".into()
        }];
        out.push(p);
    }
    if has("docker_compose") {
        let mut p = pattern(
            "testing:docker_compose",
            "Docker Compose local environment",
            "Dependencies run locally from a compose file, giving a disposable test environment.",
            "testing",
        );
        p.technologies = vec!["docker_compose".into()];
        p.requires = vec!["docker_compose".into()];
        out.push(p);
    }
    if has("dependabot") {
        let mut p = pattern(
            "dependency:dependabot",
            "Dependabot updates",
            "Dependency updates arrive as automated pull requests.",
            "dependency",
        );
        p.technologies = vec!["dependabot".into()];
        p.requires = vec!["lockfile".into()];
        out.push(p);
    }
    if has("env_example") {
        let mut p = pattern(
            "security:env_example",
            "Environment template committed",
            "Configuration is documented in an .env example while real values stay out of git.",
            "security",
        );
        p.requires = vec!["env_example".into()];
        out.push(p);
    }
    if ctx.has_fact("security:secret_scanning") || ctx.has_fact("security:codeql") {
        let mut p = pattern(
            "security:scanning_in_ci",
            "Security scanning in CI",
            "Code and secrets are scanned on every pipeline run.",
            "security",
        );
        p.delivery = ci.into();
        p.requires = vec![format!("ci:{ci}")];
        out.push(p);
    }
    if has("docs:readme") && has("docs:architecture") {
        let mut p = pattern(
            "documentation:readme+architecture",
            "README and architecture document",
            "The repository explains both how to run it and how it is built.",
            "documentation",
        );
        p.requires = vec!["docs:readme".into(), "docs:architecture".into()];
        out.push(p);
    }
    for tool in ctx.fact_suffixes("migrations:") {
        let mut p = pattern(
            &format!("migrations:{tool}"),
            &format!("Schema migrations with {tool}"),
            "Database schema changes are versioned as migrations.",
            "infrastructure",
        );
        p.technologies = vec![tool.clone()];
        p.requires = vec!["migrations".into()];
        out.push(p);
    }
    out
}

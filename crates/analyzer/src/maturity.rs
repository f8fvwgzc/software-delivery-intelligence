//! Delivery maturity (CONTRACT.md §13): eight axes scored from facts only.
//!
//! Every point is tied to a fact key, so the findings can cite evidence and
//! two scans of the same commit always agree. Guesses (inferred facts with
//! low confidence) only ever *lower* a score: a Dockerfile that is inferred
//! to run as root costs points, but an inferred cloud never earns any.

use crate::model::*;

/// `(key, label, weight)` in contract order. Weights sum to 1.
pub const AXES: &[(&str, &str, f64)] = &[
    ("containerization", "Containerization", 0.15),
    ("ci", "CI/CD", 0.20),
    ("tests", "Tests", 0.15),
    ("iac", "Infrastructure as code", 0.10),
    ("dependencies", "Dependencies", 0.15),
    ("docs", "Documentation", 0.05),
    ("secrets", "Secrets", 0.10),
    ("runtime", "Runtime currency", 0.10),
];

pub const STAGE_LEGACY: &str = "legacy";
pub const STAGE_TRANSITIONAL: &str = "transitional";
pub const STAGE_MODERN: &str = "modern";

/// `legacy` < 35, `transitional` 35–69, `modern` ≥ 70.
pub fn stage_for(overall: u32) -> &'static str {
    if overall < 35 {
        STAGE_LEGACY
    } else if overall < 70 {
        STAGE_TRANSITIONAL
    } else {
        STAGE_MODERN
    }
}

struct Facts<'a>(&'a [FactDraft]);

impl Facts<'_> {
    fn has(&self, key: &str) -> bool {
        self.0.iter().any(|f| f.key == key)
    }
    fn prefix(&self, prefix: &str) -> Vec<&FactDraft> {
        self.0
            .iter()
            .filter(|f| f.key.starts_with(prefix))
            .collect()
    }
    fn has_prefix(&self, prefix: &str) -> bool {
        self.0.iter().any(|f| f.key.starts_with(prefix))
    }
}

/// One axis under construction: points, the sentences that justify them,
/// and the patterns that would earn the missing ones.
struct Axis {
    score: u32,
    positives: Vec<String>,
    findings: Vec<String>,
    patterns: Vec<&'static str>,
}

impl Axis {
    fn new() -> Self {
        Self {
            score: 0,
            positives: Vec::new(),
            findings: Vec::new(),
            patterns: Vec::new(),
        }
    }
    fn earn(&mut self, points: u32, because: String) {
        self.score += points;
        self.positives.push(because);
    }
    fn miss(&mut self, finding: String, pattern: Option<&'static str>) {
        self.findings.push(finding);
        if let Some(p) = pattern
            && !self.patterns.contains(&p)
        {
            self.patterns.push(p);
        }
    }
    fn finish(self, key: &str, label: &str) -> MaturityAxisDraft {
        let summary = if self.positives.is_empty() {
            match self.findings.first() {
                Some(f) => f.clone(),
                None => "Nothing detected.".into(),
            }
        } else {
            format!("{}.", self.positives.join("; "))
        };
        MaturityAxisDraft {
            key: key.into(),
            label: label.into(),
            score: self.score.min(100),
            summary,
            findings: self.findings,
            recommended_patterns: self.patterns.into_iter().map(String::from).collect(),
        }
    }
}

fn containerization(f: &Facts<'_>) -> Axis {
    let mut a = Axis::new();
    let dockerfiles = f.prefix("container:dockerfile:");
    let has_dockerfile = !dockerfiles.is_empty();
    if has_dockerfile {
        a.earn(50, format!("Dockerfile at {}", dockerfiles[0].source_path));
    } else {
        a.miss("No Dockerfile".into(), Some("containerize-legacy-app"));
    }
    if f.has_prefix("container:compose:") {
        a.earn(20, "Docker Compose present".into());
    } else if f.has("k8s:manifests") || f.has("k8s:helm") {
        a.earn(20, "Kubernetes manifests present".into());
    } else {
        a.miss(
            "No Docker Compose file or Kubernetes manifests".into(),
            if has_dockerfile {
                Some("vm-docker-compose")
            } else {
                Some("containerize-legacy-app")
            },
        );
    }
    let root = f.prefix("security:dockerfile_root:");
    if has_dockerfile {
        if root.is_empty() {
            a.earn(15, "Container runs as a non-root user".into());
        } else {
            a.miss(
                format!("Runs as root: {}", root[0].evidence),
                Some("containerize-legacy-app"),
            );
        }
    }
    let health = f.prefix("container:healthcheck:");
    if !health.is_empty() {
        a.earn(15, format!("Healthcheck in {}", health[0].source_path));
    } else {
        a.miss(
            "No container healthcheck".into(),
            if has_dockerfile {
                Some("add-healthcheck")
            } else {
                Some("containerize-legacy-app")
            },
        );
    }
    a
}

fn ci(f: &Facts<'_>) -> Axis {
    let mut a = Axis::new();
    let provider = if f.has("ci:github_actions") {
        Some("GitHub Actions")
    } else if f.has("ci:gitlab_ci") {
        Some("GitLab CI")
    } else {
        None
    };
    match provider {
        Some(p) => a.earn(40, format!("{p} workflows present")),
        None => {
            a.miss("No CI workflows".into(), Some("add-ci-baseline"));
            a.miss(
                "Nothing tests, builds or releases on push".into(),
                Some("gha-ci-build-test"),
            );
            return a;
        }
    }
    let tests = f.prefix("testing:ci");
    if !tests.is_empty() {
        a.earn(25, format!("CI runs tests ({})", tests[0].evidence));
    } else {
        a.miss("CI has no test step".into(), Some("gha-ci-build-test"));
    }
    let builds = f.prefix("ci:build:");
    if !builds.is_empty() {
        a.earn(
            15,
            format!("CI builds the project ({})", builds[0].evidence),
        );
    } else {
        a.miss("CI has no build step".into(), Some("gha-ci-build-test"));
    }
    let deploys = f.prefix("deploy:pipeline:");
    if !deploys.is_empty() {
        let what = if deploys.iter().all(|d| d.value == "release") {
            "releases"
        } else {
            "deploys"
        };
        a.earn(20, format!("CI {what} ({})", deploys[0].source_path));
    } else {
        a.miss(
            "CI has no deploy or release job".into(),
            Some("gha-docker-publish"),
        );
    }
    a
}

fn tests(f: &Facts<'_>) -> Axis {
    let mut a = Axis::new();
    let unit = f.prefix("tests:unit");
    if !unit.is_empty() {
        a.earn(50, format!("Unit tests ({})", unit[0].evidence));
    } else {
        a.miss("No test framework detected".into(), Some("uat-checklist"));
    }
    if f.has("tests:e2e") {
        a.earn(30, "End-to-end tests present".into());
    } else {
        a.miss(
            "No end-to-end tests".into(),
            if f.has_prefix("container:compose:") {
                Some("uat-checklist")
            } else {
                Some("docker-compose-test-env")
            },
        );
    }
    // A package script, a CI test step or `.sdi.yml test.command` all tell the
    // docker adapter what to run.
    let command = f.0.iter().find(|t| {
        t.key == "testing:script" || t.key == "testing:ci" || t.key == "config:test.command"
    });
    if let Some(c) = command {
        a.earn(20, format!("Test command detectable ({})", c.value));
    } else if !unit.is_empty() {
        // A known runner without a script is still runnable by convention.
        a.earn(10, "Test runner known by convention".into());
        a.miss("No explicit test command".into(), Some("uat-checklist"));
    } else {
        a.miss("No test command detectable".into(), Some("uat-checklist"));
    }
    a
}

fn iac(f: &Facts<'_>) -> Axis {
    let mut a = declared_infrastructure(f);
    // A datastore the project runs itself (a Compose or Kubernetes image,
    // as opposed to one an IaC resource provisions) is infrastructure that
    // the managed-database pattern would declare and back up.
    for datastore in f
        .prefix("infra:datastore:")
        .into_iter()
        .filter(|d| !d.label.to_lowercase().contains("managed"))
    {
        a.miss(
            format!(
                "{} is self-hosted ({})",
                datastore.label, datastore.source_path
            ),
            Some("managed-database"),
        );
    }
    a
}

fn declared_infrastructure(f: &Facts<'_>) -> Axis {
    let mut a = Axis::new();
    let tools: Vec<&str> = ["terraform", "bicep", "pulumi", "cloudformation"]
        .into_iter()
        .filter(|t| f.has(&format!("iac:{t}")))
        .collect();
    let k8s = f.has("k8s:manifests") || f.has("k8s:helm");
    if tools.is_empty() && !k8s {
        a.miss(
            "Infrastructure is not declared".into(),
            Some("terraform-remote-state"),
        );
        return a;
    }
    let mut named: Vec<&str> = tools.clone();
    if k8s {
        named.push("kubernetes");
    }
    a.earn(
        60,
        format!("Infrastructure declared with {}", named.join(", ")),
    );
    if tools.contains(&"terraform") {
        let backend = f.prefix("iac:terraform:backend:");
        if !backend.is_empty() {
            a.earn(20, format!("Terraform remote state ({})", backend[0].value));
        } else {
            a.miss(
                "Terraform state is local".into(),
                Some("terraform-remote-state"),
            );
        }
        if f.has_prefix("iac:terraform:module:") {
            a.earn(20, "Terraform modules used".into());
        } else {
            a.miss(
                "No Terraform modules".into(),
                Some("terraform-remote-state"),
            );
        }
    } else {
        // Bicep, Pulumi, CloudFormation and Helm keep state in the platform.
        a.earn(40, "State managed by the platform".into());
    }
    a
}

fn dependencies(f: &Facts<'_>) -> Axis {
    let mut a = Axis::new();
    let lock = f.prefix("security:lockfile");
    if !lock.is_empty() {
        a.earn(40, format!("Lockfile {}", lock[0].value));
    } else {
        a.miss("No lockfile".into(), Some("dependabot-weekly"));
    }
    if f.has("security:dependabot") {
        a.earn(40, "Dependabot configured".into());
    } else {
        a.miss(
            "Dependabot not configured".into(),
            Some("dependabot-weekly"),
        );
    }
    let eol = f.prefix("dependency:eol:");
    if eol.is_empty() {
        a.earn(20, "No end-of-life major versions pinned".into());
    } else {
        for d in eol {
            a.miss(
                format!(
                    "Pinned to an end-of-life major: {} ({})",
                    d.value, d.source_path
                ),
                Some("upgrade-runtime"),
            );
        }
    }
    a
}

fn docs(f: &Facts<'_>) -> Axis {
    let mut a = Axis::new();
    if f.has("docs:readme") {
        a.earn(50, "README present".into());
    } else {
        a.miss("README missing".into(), Some("readme-architecture-docs"));
    }
    if f.has("docs:architecture") {
        a.earn(50, "Architecture document present".into());
    } else {
        if f.has("docs:folder") {
            a.earn(20, "Documentation folder present".into());
        }
        a.miss(
            "No architecture document".into(),
            Some("readme-architecture-docs"),
        );
    }
    a
}

fn secrets(f: &Facts<'_>) -> Axis {
    let mut a = Axis::new();
    if f.has("security:env_example") {
        a.earn(40, "Environment template committed".into());
    } else {
        a.miss(
            "No .env.example documents the configuration".into(),
            Some("config-to-environment"),
        );
    }
    let suspicious = f.prefix("security:suspicious_file:");
    if suspicious.is_empty() {
        a.earn(40, "No secret-looking files committed".into());
    } else {
        for s in suspicious.iter().take(5) {
            a.miss(
                format!("`{}` committed", s.source_path),
                Some("secret-scanning-baseline"),
            );
        }
    }
    if f.has("security:secret_scanning") || f.has("security:codeql") {
        a.earn(20, "Secret scanning in CI".into());
    } else {
        a.miss(
            "No secret scanning in CI".into(),
            Some("secret-scanning-baseline"),
        );
    }
    for (key, name) in [
        ("config:web_xml", "web.xml"),
        ("config:web_config", "web.config"),
    ] {
        if let Some(c) = f.prefix(key).first() {
            a.miss(
                format!(
                    "Configuration lives in {name} ({}) rather than the environment",
                    c.source_path
                ),
                Some("config-to-environment"),
            );
        }
    }
    a
}

fn runtime(f: &Facts<'_>) -> Axis {
    let mut a = Axis::new();
    let runtimes: Vec<&FactDraft> = f
        .prefix("runtime:")
        .into_iter()
        .filter(|r| !r.key.starts_with("runtime:eol:"))
        .collect();
    let eol = f.prefix("runtime:eol:");
    let python2 = f.prefix("python:2");
    if runtimes.is_empty() && python2.is_empty() {
        a.score = 60;
        a.miss(
            "No runtime version is declared, so support status cannot be verified".into(),
            None,
        );
        return a;
    }
    // Count by runtime name so Node read from both .nvmrc and the Dockerfile is one runtime.
    let mut names: Vec<&str> = runtimes
        .iter()
        .filter_map(|r| r.key.split(':').nth(1))
        .collect();
    names.sort_unstable();
    names.dedup();
    let total = names.len().max(1) as u32;
    let bad = eol.len().min(names.len()) as u32;
    a.score = 100 * (total - bad) / total;
    if bad == 0 && python2.is_empty() {
        let labels: Vec<&str> = runtimes.iter().map(|r| r.label.as_str()).collect();
        a.positives.push(format!(
            "Every detected runtime is supported ({})",
            labels.join(", ")
        ));
    }
    for e in eol {
        a.miss(e.evidence.clone(), Some("upgrade-runtime"));
    }
    for p in python2 {
        a.score = a.score.min(20);
        a.miss(
            format!("Python 2 signals: {}", p.evidence),
            Some("upgrade-runtime"),
        );
    }
    a
}

/// Scores the eight axes and the weighted overall. Pure and deterministic.
pub fn score(facts: &[FactDraft]) -> MaturityDraft {
    let f = Facts(facts);
    let mut axes = Vec::with_capacity(AXES.len());
    let mut weighted = 0.0;
    for (key, label, weight) in AXES {
        let axis = match *key {
            "containerization" => containerization(&f),
            "ci" => ci(&f),
            "tests" => tests(&f),
            "iac" => iac(&f),
            "dependencies" => dependencies(&f),
            "docs" => docs(&f),
            "secrets" => secrets(&f),
            _ => runtime(&f),
        }
        .finish(key, label);
        weighted += f64::from(axis.score) * weight;
        axes.push(axis);
    }
    let overall = weighted.round().clamp(0.0, 100.0) as u32;
    MaturityDraft {
        overall,
        stage: stage_for(overall).into(),
        axes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fact(key: &str, value: &str, source: &str) -> FactDraft {
        FactDraft {
            kind: FactKind::Detected,
            category: FactCategory::Repository,
            key: key.into(),
            label: key.into(),
            value: value.into(),
            confidence: 1.0,
            source_path: source.into(),
            evidence: format!("{value} ({source})"),
        }
    }

    fn facts(keys: &[&str]) -> Vec<FactDraft> {
        keys.iter().map(|k| fact(k, "present", "x")).collect()
    }

    fn axis<'a>(m: &'a MaturityDraft, key: &str) -> &'a MaturityAxisDraft {
        m.axes.iter().find(|a| a.key == key).unwrap()
    }

    #[test]
    fn weights_sum_to_one_and_axes_are_in_contract_order() {
        let total: f64 = AXES.iter().map(|(_, _, w)| w).sum();
        assert!((total - 1.0).abs() < 1e-9);
        let keys: Vec<&str> = AXES.iter().map(|(k, _, _)| *k).collect();
        assert_eq!(
            keys,
            [
                "containerization",
                "ci",
                "tests",
                "iac",
                "dependencies",
                "docs",
                "secrets",
                "runtime"
            ]
        );
        assert_eq!(stage_for(34), "legacy");
        assert_eq!(stage_for(35), "transitional");
        assert_eq!(stage_for(69), "transitional");
        assert_eq!(stage_for(70), "modern");
    }

    #[test]
    fn modern_repository_scores_at_least_70() {
        let mut f = facts(&[
            "container:dockerfile:Dockerfile",
            "container:compose:docker-compose.yml",
            "container:healthcheck:Dockerfile",
            "ci:github_actions",
            "testing:ci",
            "ci:build:.github/workflows/ci.yml",
            "tests:unit",
            "tests:e2e",
            "testing:script",
            "iac:terraform",
            "iac:terraform:backend:azurerm",
            "iac:terraform:module:network",
            "security:lockfile",
            "security:dependabot",
            "docs:readme",
            "docs:architecture",
            "security:env_example",
            "security:secret_scanning",
        ]);
        f.push(fact(
            "deploy:pipeline:.github/workflows/deploy.yml",
            "azure",
            "x",
        ));
        f.push(fact("runtime:node:22", "22", ".nvmrc"));
        let m = score(&f);
        assert_eq!(m.overall, 100, "{m:#?}");
        assert_eq!(m.stage, "modern");
        assert!(m.axes.iter().all(|a| a.findings.is_empty()), "{m:#?}");
        assert_eq!(m.axes.len(), 8);
    }

    #[test]
    fn legacy_java_ee_war_scores_below_35() {
        let mut f = facts(&[
            "lang:java",
            "framework:java_ee",
            "packaging:war",
            "config:web_xml",
            "package:maven",
            "docs:readme",
        ]);
        f.push(fact("runtime:java:8", "8", "pom.xml"));
        let mut eol = fact("runtime:eol:java", "2022-03-31", "pom.xml");
        eol.evidence = "Java 8 reached end of life 2022-03-31 (pom.xml)".into();
        f.push(eol);
        let m = score(&f);
        assert!(m.overall < 35, "{m:#?}");
        assert_eq!(m.stage, "legacy");
        let c = axis(&m, "containerization");
        assert_eq!(c.score, 0);
        assert!(c.findings.iter().any(|x| x == "No Dockerfile"));
        assert!(
            c.recommended_patterns
                .contains(&"containerize-legacy-app".to_string())
        );
        let ci = axis(&m, "ci");
        assert_eq!(ci.score, 0);
        assert!(ci.findings.iter().any(|x| x == "No CI workflows"));
        assert_eq!(ci.recommended_patterns[0], "add-ci-baseline");
        let r = axis(&m, "runtime");
        assert_eq!(r.score, 0);
        assert!(r.findings[0].contains("Java 8 reached end of life 2022-03-31"));
        assert_eq!(r.recommended_patterns, vec!["upgrade-runtime"]);
        let s = axis(&m, "secrets");
        assert!(
            s.recommended_patterns
                .contains(&"config-to-environment".to_string())
        );
        assert!(
            axis(&m, "iac")
                .findings
                .iter()
                .any(|x| x == "Infrastructure is not declared")
        );
        assert!(
            axis(&m, "dependencies")
                .findings
                .iter()
                .any(|x| x == "No lockfile")
        );
        assert!(
            axis(&m, "docs")
                .findings
                .iter()
                .any(|x| x == "No architecture document")
        );
    }

    #[test]
    fn transitional_repository_lands_in_the_middle() {
        let mut f = facts(&[
            "container:dockerfile:Dockerfile",
            "security:dockerfile_root:Dockerfile",
            "ci:github_actions",
            "testing:ci",
            "tests:unit",
            "testing:script",
            "security:lockfile",
            "docs:readme",
        ]);
        f.push(fact("runtime:node:16", "16", ".nvmrc"));
        let mut eol = fact("runtime:eol:node", "2023-09-11", ".nvmrc");
        eol.evidence = "Node 16 reached end of life 2023-09-11 (.nvmrc)".into();
        f.push(eol);
        let m = score(&f);
        assert!((35..70).contains(&m.overall), "{m:#?}");
        assert_eq!(m.stage, "transitional");
        let c = axis(&m, "containerization");
        assert!(c.findings.iter().any(|x| x.starts_with("Runs as root")));
        assert!(
            c.recommended_patterns
                .contains(&"add-healthcheck".to_string())
        );
        assert!(
            axis(&m, "ci")
                .findings
                .iter()
                .any(|x| x == "CI has no build step")
        );
        assert_eq!(
            axis(&m, "runtime").findings[0],
            "Node 16 reached end of life 2023-09-11 (.nvmrc)"
        );
    }

    #[test]
    fn a_self_hosted_database_recommends_the_managed_database_pattern() {
        let mut f = facts(&["iac:terraform", "iac:terraform:backend:azurerm"]);
        let mut pg = fact("infra:datastore:postgres", "postgres:16", "compose.yml");
        pg.label = "PostgreSQL (Docker Compose)".into();
        f.push(pg);
        let mut managed = fact(
            "infra:datastore:redis",
            "provisioned by azurerm_redis_cache",
            "main.tf",
        );
        managed.label = "Redis (managed)".into();
        f.push(managed);
        let m = score(&f);
        let iac = axis(&m, "iac");
        assert_eq!(iac.recommended_patterns.len(), 2, "{iac:?}");
        assert!(
            iac.recommended_patterns
                .contains(&"managed-database".to_string())
        );
        assert!(
            iac.findings
                .iter()
                .any(|x| x == "PostgreSQL (Docker Compose) is self-hosted (compose.yml)"),
            "{:?}",
            iac.findings
        );
        assert!(!iac.findings.iter().any(|x| x.contains("Redis")));
        // The finding is advice, not a penalty: the score is unchanged.
        assert_eq!(iac.score, 80);
    }

    #[test]
    fn unknown_runtime_is_neutral_not_failing() {
        let m = score(&[]);
        assert_eq!(axis(&m, "runtime").score, 60);
        assert!(axis(&m, "runtime").recommended_patterns.is_empty());
        assert_eq!(m.stage, "legacy");
    }
}

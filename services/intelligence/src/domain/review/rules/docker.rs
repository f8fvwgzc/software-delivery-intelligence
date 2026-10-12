//! Dockerfiles: running as root, floating base image tags, secrets baked
//! into `ENV`/`ARG`, and new images without a `HEALTHCHECK`.

use std::sync::LazyLock;

use regex::Regex;

use super::{Finding, Severity, evidence};
use crate::domain::review::input::ReviewInput;

static FROM: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\s*FROM\s+(?:--platform=\S+\s+)?(\S+)(?:\s+AS\s+(\S+))?").expect("from")
});
static ENV_SECRET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)^\s*(?:ENV|ARG)\s+(?:[A-Z0-9_]*(?:PASSWORD|PASSWD|SECRET|TOKEN|API_KEY|APIKEY|ACCESS_KEY|PRIVATE_KEY|CREDENTIALS)[A-Z0-9_]*)\s*=?\s*["']?([^\s"']+)"#,
    )
    .expect("env secret")
});

fn placeholder(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.is_empty()
        || lower.starts_with('$')
        || lower.contains("example")
        || lower.contains("changeme")
        || lower.contains("<")
        || lower.contains("xxx")
        || lower.contains("dummy")
        || lower.contains("placeholder")
        || lower == "true"
        || lower == "false"
        || lower.len() < 6
}

pub fn analyze(input: &ReviewInput) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in input
        .files
        .iter()
        .filter(|f| f.is_dockerfile() && !f.is_removed())
    {
        let path = &file.path;
        let added = file.added_lines();
        let text = file.added_text();
        let stages: Vec<String> = added
            .iter()
            .filter_map(|(_, l)| FROM.captures(l))
            .filter_map(|c| c.get(2).map(|m| m.as_str().to_ascii_lowercase()))
            .collect();

        for (line_no, line) in &added {
            let trimmed = line.trim();
            if trimmed.eq_ignore_ascii_case("USER root") || trimmed.eq_ignore_ascii_case("USER 0") {
                findings.push(Finding::new(
                    "docker",
                    "root_user",
                    Severity::Medium,
                    path,
                    *line_no,
                    "Container runs as root",
                    format!(
                        "Line {line_no} switches to root and nothing after it switches back. Create an unprivileged \
                         user and end the image with `USER <name>` so a compromised process cannot own the container."
                    ),
                ));
            }
            if let Some(caps) = FROM.captures(line) {
                let image = &caps[1];
                let (name, tag) = match image.rsplit_once(':') {
                    Some((n, t)) if !t.contains('/') => (n, Some(t)),
                    _ => (image, None),
                };
                let is_stage = stages.contains(&name.to_ascii_lowercase());
                let digest = image.contains('@');
                if !is_stage && !digest && name != "scratch" {
                    let floating = match tag {
                        None => Some("no tag"),
                        Some("latest") => Some("the `latest` tag"),
                        Some(_) => None,
                    };
                    if let Some(what) = floating {
                        let suggestion = format!("FROM {name}:<version>");
                        findings.push(
                            Finding::new(
                                "docker",
                                "latest_tag",
                                Severity::Medium,
                                path,
                                *line_no,
                                "Base image is not pinned",
                                format!(
                                    "Line {line_no} uses {what} for `{name}`: {}. The build changes whenever upstream pushes; \
                                     pin a version tag (and ideally a digest) so rebuilds are reproducible.",
                                    evidence(line)
                                ),
                            )
                            .with_suggestion(suggestion),
                        );
                    }
                }
            }
            if let Some(caps) = ENV_SECRET.captures(line)
                && !placeholder(&caps[1])
            {
                findings.push(Finding::new(
                    "docker",
                    "env_secret",
                    Severity::High,
                    path,
                    *line_no,
                    "Secret baked into the image",
                    format!(
                        "Line {line_no} sets a credential with `ENV`/`ARG`; it ends up in the image layers and `docker history`. \
                         Inject it at run time (`--env-file`, an orchestrator secret) or use a BuildKit `--mount=type=secret`."
                    ),
                ));
            }
        }

        let has_healthcheck = text.lines().any(|l| {
            l.trim_start()
                .to_ascii_uppercase()
                .starts_with("HEALTHCHECK")
        });
        let final_stage_runs = added.iter().any(|(_, l)| {
            let upper = l.trim_start().to_ascii_uppercase();
            upper.starts_with("CMD") || upper.starts_with("ENTRYPOINT")
        });
        if file.is_added() && !has_healthcheck && final_stage_runs {
            findings.push(
                Finding::new(
                    "docker",
                    "no_healthcheck",
                    Severity::Low,
                    path,
                    file.first_line(),
                    "No HEALTHCHECK",
                    format!(
                        "`{path}` is a new image with a `CMD`/`ENTRYPOINT` but no `HEALTHCHECK`. Orchestrators then \
                         consider the container healthy as soon as the process starts; add a cheap probe of the service."
                    ),
                )
                .with_suggestion("HEALTHCHECK --interval=30s --timeout=3s CMD curl -fsS http://127.0.0.1:8080/health || exit 1"),
            );
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::*;
    use super::*;

    #[test]
    fn root_user_and_floating_tags() {
        let findings = analyze(&input(vec![added(
            "Dockerfile",
            &[
                "FROM node:latest AS build",
                "FROM ubuntu",
                "USER root",
                "CMD [\"node\", \"server.js\"]",
            ],
        )]));
        assert_eq!(
            rules(&findings),
            vec![
                "docker.latest_tag",
                "docker.latest_tag",
                "docker.root_user",
                "docker.no_healthcheck"
            ]
        );
        assert_eq!(
            findings[0].suggestion.as_deref(),
            Some("FROM node:<version>")
        );
        assert_eq!(findings[1].line, 2);
    }

    #[test]
    fn secrets_in_env_and_arg() {
        let findings = analyze(&input(vec![modified(
            "deploy/Dockerfile.api",
            &[
                "ENV DATABASE_PASSWORD=supersecret123",
                "ARG NPM_TOKEN=npm_abcdefghijklmnop",
                "ENV API_KEY=\"${API_KEY}\"",
            ],
        )]));
        assert_eq!(
            rules(&findings),
            vec!["docker.env_secret", "docker.env_secret"]
        );
        assert_eq!(findings[0].severity, Severity::High);
    }

    #[test]
    fn new_image_without_healthcheck() {
        let findings = analyze(&input(vec![added(
            "services/api/Dockerfile",
            &[
                "FROM rust:1.82-slim AS build",
                "FROM debian:bookworm-slim",
                "COPY --from=build /app /app",
                "USER app",
                "ENTRYPOINT [\"/app\"]",
            ],
        )]));
        assert_eq!(rules(&findings), vec!["docker.no_healthcheck"]);
        assert!(findings[0].suggestion.is_some());
    }

    #[test]
    fn pinned_rootless_probed_image_is_clean() {
        let findings = analyze(&input(vec![
            added(
                "Dockerfile",
                &[
                    "FROM node:22-alpine AS deps",
                    "FROM deps AS build",
                    "FROM gcr.io/distroless/nodejs22-debian12@sha256:abc",
                    "ARG BUILD_SECRET_NAME=example",
                    "USER nonroot",
                    "HEALTHCHECK CMD wget -qO- http://127.0.0.1:3000/health",
                    "CMD [\"server.js\"]",
                ],
            ),
            modified("README.md", &["FROM ubuntu:latest"]),
        ]));
        assert!(findings.is_empty(), "{findings:?}");
    }
}

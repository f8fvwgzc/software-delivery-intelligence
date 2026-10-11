//! Plans an accepted option hands to the execution service per adapter
//! (CONTRACT.md §5.4). `simulated` runs the option's own plan as a dry run;
//! `docker` runs the local verification loop; `github` opens a pull request
//! with the option's generated files.

use std::collections::BTreeMap;

use sdi_proto::knowledge::v1::ProjectContext;

use super::model::{ArtifactData, EvidenceItem, PlanStepData, RankedOption};
use crate::generators::{GenInput, Stack};

pub const ADAPTERS: [&str; 3] = ["simulated", "docker", "github"];

/// Image tag the local verification builds: `<repo>:sdi-<short-sha>`.
pub fn image_tag(input: &GenInput, commit_sha: &str) -> String {
    let short: String = commit_sha.chars().take(7).collect();
    let tag = if short.is_empty() {
        "latest".to_owned()
    } else {
        format!("sdi-{short}")
    };
    format!("{}:{tag}", input.image)
}

#[allow(clippy::too_many_arguments)]
fn step(
    key: &str,
    title: String,
    action_type: &str,
    adapter: &str,
    depends_on: &[&str],
    permissions: &[&str],
    params: BTreeMap<String, String>,
    verification: impl Into<String>,
) -> PlanStepData {
    PlanStepData {
        key: key.to_owned(),
        title,
        description: String::new(),
        action_type: action_type.to_owned(),
        adapter: adapter.to_owned(),
        depends_on: depends_on.iter().map(|s| (*s).to_owned()).collect(),
        required_permissions: permissions.iter().map(|s| (*s).to_owned()).collect(),
        params,
        verification: verification.into(),
    }
}

fn repo_params(ctx: &ProjectContext) -> BTreeMap<String, String> {
    let mut params = BTreeMap::new();
    if let Some(project) = &ctx.project {
        params.insert("repository".into(), project.repo_full_name.clone());
        params.insert(
            "branch".into(),
            if project.default_branch.is_empty() {
                "main".to_owned()
            } else {
                project.default_branch.clone()
            },
        );
    }
    if let Some(sha) = ctx
        .understanding
        .as_ref()
        .map(|u| u.commit_sha.as_str())
        .filter(|s| !s.is_empty())
    {
        params.insert("commit".into(), sha.to_owned());
    }
    params
}

/// checkout → build → run → tests → destroy, with the image, port, health
/// path and test command the docker adapter needs.
pub fn verification_plan(ctx: &ProjectContext, input: &GenInput) -> Vec<PlanStepData> {
    let sha = ctx
        .understanding
        .as_ref()
        .map(|u| u.commit_sha.clone())
        .unwrap_or_default();
    let image = image_tag(input, &sha);
    let base = repo_params(ctx);
    let with = |extra: &[(&str, String)]| {
        let mut p = base.clone();
        for (k, v) in extra {
            p.insert((*k).to_owned(), v.clone());
        }
        p
    };
    // Build the Dockerfile of the application directory: the one the scan
    // found there, or the generated one written on top of the checkout.
    let dockerfile = input.in_app("Dockerfile");
    let context = input.context_dir();
    let mut test_params = vec![
        ("image", image.clone()),
        ("test_command", input.test_command.clone()),
    ];
    if let Some(toolchain) = toolchain_image(input) {
        test_params.push(("test_image", toolchain));
        test_params.push(("test_dir", context.trim_start_matches("./").to_owned()));
    }
    vec![
        step(
            "checkout",
            format!("Download {} at the scanned commit", input.repo_name),
            "repo.checkout",
            "docker",
            &[],
            &["repository:read"],
            base.clone(),
            "the working directory holds the repository tree",
        ),
        step(
            "build",
            format!("Build image {image}"),
            "container.build",
            "docker",
            &["checkout"],
            &["container:build"],
            with(&[
                ("image", image.clone()),
                ("dockerfile", dockerfile),
                ("context", context.clone()),
            ]),
            "docker build exits 0 and the image is listed",
        ),
        step(
            "run",
            format!("Start {image} and poll {}", input.health_path),
            "container.run",
            "docker",
            &["build"],
            &["container:run"],
            with(&[
                ("image", image.clone()),
                ("port", input.port.to_string()),
                ("health_path", input.health_path.clone()),
            ]),
            format!("GET {} on the mapped port answers 2xx", input.health_path),
        ),
        step(
            "test",
            format!("Run `{}` inside the image", input.test_command),
            "tests.run_local",
            "docker",
            &["run"],
            &["container:run"],
            with(&test_params),
            "the test command exits 0",
        ),
        step(
            "cleanup",
            "Stop and remove the temporary container".to_owned(),
            "env.destroy_temporary",
            "docker",
            &["test"],
            &["environment:write"],
            with(&[("image", image)]),
            "no container or network from this run remains",
        ),
    ]
}

/// The toolchain image a compiled stack's tests run in, with the source
/// mounted: the runtime image only carries the binary. Interpreted stacks
/// test inside the built image, which still holds the source.
fn toolchain_image(input: &GenInput) -> Option<String> {
    let v = &input.runtime_version;
    Some(match input.stack {
        Stack::Rust => format!("rust:{v}-slim"),
        Stack::Go => format!("golang:{v}-alpine"),
        Stack::Java => {
            if input.package_manager == "gradle" {
                format!("gradle:8-jdk{v}")
            } else {
                format!("maven:3-eclipse-temurin-{v}")
            }
        }
        Stack::DotNet => format!("mcr.microsoft.com/dotnet/sdk:{v}"),
        _ => return None,
    })
}

/// checkout → open_pull_request with the option's files.
pub fn pull_request_plan(
    ctx: &ProjectContext,
    option: &RankedOption,
    proposal_id: &str,
) -> Vec<PlanStepData> {
    let short: String = proposal_id.chars().take(8).collect();
    let base = repo_params(ctx);
    let mut pr = base.clone();
    pr.insert("branch".into(), format!("sdi/{short}"));
    pr.insert(
        "base_branch".into(),
        base.get("branch")
            .cloned()
            .unwrap_or_else(|| "main".to_owned()),
    );
    pr.insert("title".into(), option.title.clone());
    pr.insert(
        "body".into(),
        pull_request_body(option, &option.artifacts, &option.evidence),
    );
    vec![
        step(
            "checkout",
            "Download the repository at the scanned commit".to_owned(),
            "repo.checkout",
            "github",
            &[],
            &["repository:read"],
            base,
            "the default branch head is known",
        ),
        step(
            "pull_request",
            format!(
                "Open a pull request with {} generated file(s)",
                option.artifacts.len()
            ),
            "repo.open_pull_request",
            "github",
            &["checkout"],
            &["repository:write"],
            pr,
            "the pull request exists and lists every generated file",
        ),
    ]
}

fn pull_request_body(
    option: &RankedOption,
    artifacts: &[ArtifactData],
    evidence: &[EvidenceItem],
) -> String {
    let mut body = format!("## {}\n\n{}\n\n### Files\n\n", option.title, option.summary);
    for file in artifacts {
        body.push_str(&format!(
            "- `{}`{} - {}\n",
            file.path,
            if file.replaces_existing {
                " (replaces the existing file)"
            } else {
                ""
            },
            file.purpose
        ));
    }
    body.push_str("\n### Evidence\n\n");
    for item in evidence {
        body.push_str(&format!("- {}: {}\n", item.label, item.detail));
    }
    body.push_str(
        "\nGenerated by Software Delivery Intelligence from the repository's own facts.\n",
    );
    body
}

/// `{path: content}` for `CreateExecutionRequest.artifacts`.
pub fn artifact_map(artifacts: &[ArtifactData]) -> std::collections::HashMap<String, String> {
    artifacts
        .iter()
        .map(|a| (a.path.clone(), a.content.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{domain::ranking::fixtures, generators::input as gen_input};
    use sdi_proto::knowledge::v1::Understanding;

    fn context() -> ProjectContext {
        let mut ctx = fixtures::context();
        ctx.understanding = Some(Understanding {
            commit_sha: "0123456789abcdef".into(),
            ..Default::default()
        });
        ctx
    }

    #[test]
    fn docker_verification_plan_has_the_contract_shape() {
        let ctx = context();
        let input = gen_input::build(&ctx, &BTreeMap::new(), "");
        let plan = verification_plan(&ctx, &input);
        let keys: Vec<&str> = plan.iter().map(|s| s.action_type.as_str()).collect();
        assert_eq!(
            keys,
            vec![
                "repo.checkout",
                "container.build",
                "container.run",
                "tests.run_local",
                "env.destroy_temporary"
            ]
        );
        assert_eq!(
            plan[1].params.get("image").map(String::as_str),
            Some("view-engine:sdi-0123456")
        );
        assert_eq!(plan[2].params.get("port").map(String::as_str), Some("3000"));
        assert_eq!(
            plan[2].params.get("health_path").map(String::as_str),
            Some("/health")
        );
        assert_eq!(
            plan[3].params.get("test_command").map(String::as_str),
            Some("npm test")
        );
        assert_eq!(plan[3].params.get("test_image"), None);
        assert_eq!(
            plan[1].params.get("dockerfile").map(String::as_str),
            Some("Dockerfile")
        );
        assert_eq!(plan[1].params.get("context").map(String::as_str), Some("."));
        assert!(plan.iter().all(|s| s.adapter == "docker"));
        assert_eq!(plan[4].depends_on, vec!["test"]);
    }

    #[test]
    fn compiled_stacks_test_in_a_toolchain_image_from_the_app_dir() {
        let ctx = context();
        let mut input = gen_input::build(&ctx, &BTreeMap::new(), "");
        input.stack = Stack::Rust;
        input.runtime_version = "1.91".into();
        input.app_dir = "backend".into();
        input.test_command = "cargo test --locked".into();
        let plan = verification_plan(&ctx, &input);
        assert_eq!(
            plan[1].params.get("dockerfile").map(String::as_str),
            Some("backend/Dockerfile")
        );
        assert_eq!(
            plan[1].params.get("context").map(String::as_str),
            Some("./backend")
        );
        assert_eq!(
            plan[3].params.get("test_image").map(String::as_str),
            Some("rust:1.91-slim")
        );
        assert_eq!(
            plan[3].params.get("test_dir").map(String::as_str),
            Some("backend")
        );
    }

    #[test]
    fn github_plan_carries_branch_title_body_and_files() {
        let ctx = context();
        let option = RankedOption {
            id: "opt-x".into(),
            title: "Azure Container Apps".into(),
            summary: "serverless".into(),
            pattern_id: "x".into(),
            rank: 1,
            score: Default::default(),
            pros: vec![],
            cons: vec![],
            evidence: vec![EvidenceItem::new(
                super::super::model::EvidenceKind::Fact,
                "f",
                "Dockerfile",
                "detected",
            )],
            estimated_monthly_cost_usd: 0.0,
            unmet_requirements: vec![],
            plan: vec![],
            tags: vec![],
            artifacts: vec![ArtifactData {
                path: "Dockerfile".into(),
                content: "FROM scratch\n".into(),
                language: "dockerfile".into(),
                purpose: "image".into(),
                replaces_existing: true,
            }],
            cost_breakdown: vec![],
        };
        let plan = pull_request_plan(&ctx, &option, "0192a1b2-aaaa-bbbb-cccc-ddddeeeeffff");
        assert_eq!(plan.len(), 2);
        assert_eq!(plan[1].action_type, "repo.open_pull_request");
        assert_eq!(
            plan[1].params.get("branch").map(String::as_str),
            Some("sdi/0192a1b2")
        );
        assert_eq!(
            plan[1].params.get("base_branch").map(String::as_str),
            Some("main")
        );
        assert_eq!(
            plan[1].params.get("title").map(String::as_str),
            Some("Azure Container Apps")
        );
        let body = plan[1].params.get("body").unwrap();
        assert!(body.contains("`Dockerfile` (replaces the existing file) - image"));
        assert!(body.contains("- Dockerfile: detected"));
        assert_eq!(
            artifact_map(&option.artifacts)
                .get("Dockerfile")
                .map(String::as_str),
            Some("FROM scratch\n")
        );
    }
}

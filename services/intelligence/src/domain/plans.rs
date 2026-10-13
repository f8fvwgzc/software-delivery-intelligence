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

// --- Pull request autopilot (CONTRACT.md §23.5) ---------------------------------
//
// Step keys, action types, permissions and params are fixed by the
// contract's table so intelligence and execution agree without sharing
// code. Every step runs on the `github` adapter and depends on the one
// before it. Each also names the repository (`owner/name`): that is how the
// adapter learns which repository a step is about.

pub const GITHUB: &str = "github";
/// The branch the Dependabot configuration pull request is opened from.
pub const DEPENDABOT_BRANCH: &str = "sdi/dependabot";
pub const DEPENDABOT_CONFIG_PATH: &str = ".github/dependabot.yml";

fn params(repository: &str, pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    if !repository.trim().is_empty() {
        out.insert("repository".to_owned(), repository.trim().to_owned());
    }
    for (key, value) in pairs {
        out.insert((*key).to_owned(), (*value).to_owned());
    }
    out
}

fn checkout_step(repository: &str) -> PlanStepData {
    step(
        "checkout",
        if repository.trim().is_empty() {
            "Read the repository".to_owned()
        } else {
            format!("Read {repository}")
        },
        "repo.checkout",
        GITHUB,
        &[],
        &["repository:read"],
        params(repository, &[]),
        "the repository answers and its branches are known",
    )
}

/// What a fix execution commits and where (CONTRACT.md §23.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixPlan<'a> {
    pub repository: &'a str,
    /// The pull request's head branch.
    pub branch: &'a str,
    /// The commit the fixes were computed against.
    pub expected_head_sha: &'a str,
    pub message: &'a str,
    pub pr_number: u64,
    pub fixes: usize,
    pub files: usize,
}

/// `checkout` → `commit_fixes`: one commit on top of `expected_head_sha`.
pub fn fix_plan(plan: &FixPlan<'_>) -> Vec<PlanStepData> {
    vec![
        checkout_step(plan.repository),
        step(
            "commit_fixes",
            format!(
                "Commit {} review fix{} in {} file{} to {}",
                plan.fixes,
                if plan.fixes == 1 { "" } else { "es" },
                plan.files,
                if plan.files == 1 { "" } else { "s" },
                plan.branch
            ),
            "repo.commit_fixes",
            GITHUB,
            &["checkout"],
            &["repository:write"],
            params(
                plan.repository,
                &[
                    ("branch", plan.branch),
                    ("expected_head_sha", plan.expected_head_sha),
                    ("message", plan.message),
                    ("pull_request", &plan.pr_number.to_string()),
                ],
            ),
            "the branch points at a new commit whose parent is the expected head",
        ),
    ]
}

/// What a merge execution does (CONTRACT.md §23.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergePlan<'a> {
    pub repository: &'a str,
    pub pr_number: u64,
    pub expected_head_sha: &'a str,
    /// "merge" | "squash" | "rebase"
    pub method: &'a str,
    /// The head branch to delete after the merge. `None` when it was not
    /// asked for or the branch lives in another repository (a fork).
    pub delete_branch: Option<&'a str>,
}

/// `verify_mergeable` → `merge` → `delete_branch` (only with a branch).
pub fn merge_plan(plan: &MergePlan<'_>) -> Vec<PlanStepData> {
    let number = plan.pr_number.to_string();
    let mut steps = vec![
        step(
            "verify_mergeable",
            format!("Check that #{number} can still be merged"),
            "repo.verify_mergeable",
            GITHUB,
            &[],
            &["repository:read"],
            params(
                plan.repository,
                &[
                    ("pull_request", &number),
                    ("expected_head_sha", plan.expected_head_sha),
                ],
            ),
            "the head is the expected commit, there is no conflict and no check failed or is still running",
        ),
        step(
            "merge",
            format!("Merge #{number} ({})", plan.method),
            "repo.merge_pull_request",
            GITHUB,
            &["verify_mergeable"],
            &["repository:write"],
            params(
                plan.repository,
                &[
                    ("pull_request", &number),
                    ("expected_head_sha", plan.expected_head_sha),
                    ("merge_method", plan.method),
                ],
            ),
            "GitHub reports the pull request as merged",
        ),
    ];
    if let Some(branch) = plan.delete_branch.filter(|b| !b.trim().is_empty()) {
        steps.push(step(
            "delete_branch",
            format!("Delete the merged branch {branch}"),
            "repo.delete_branch",
            GITHUB,
            &["merge"],
            &["repository:write"],
            params(
                plan.repository,
                &[("branch", branch), ("pull_request", &number)],
            ),
            "the branch no longer exists",
        ));
    }
    steps
}

/// What a Dependabot setup execution does (CONTRACT.md §23.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependabotPlan<'a> {
    pub repository: &'a str,
    pub default_branch: &'a str,
    pub alerts: bool,
    pub security_updates: bool,
    /// Title and body of the pull request that adds the configuration
    /// file; `None` when the file was not asked for.
    pub config_pull_request: Option<(&'a str, &'a str)>,
}

/// `checkout` → `enable_security_updates` (alerts or security updates) →
/// `open_pull_request` (the configuration file).
pub fn dependabot_plan(plan: &DependabotPlan<'_>) -> Vec<PlanStepData> {
    let mut steps = vec![checkout_step(plan.repository)];
    let mut previous = "checkout";
    if plan.alerts || plan.security_updates {
        steps.push(step(
            "enable_security_updates",
            match (plan.alerts, plan.security_updates) {
                (true, true) => "Turn on Dependabot alerts and security updates",
                (true, false) => "Turn on Dependabot alerts",
                _ => "Turn on Dependabot security updates",
            }
            .to_owned(),
            "repo.enable_security_updates",
            GITHUB,
            &[previous],
            &["repository:admin"],
            params(
                plan.repository,
                &[
                    ("alerts", if plan.alerts { "true" } else { "false" }),
                    (
                        "security_updates",
                        if plan.security_updates {
                            "true"
                        } else {
                            "false"
                        },
                    ),
                ],
            ),
            "GitHub reports the settings as enabled",
        ));
        previous = "enable_security_updates";
    }
    if let Some((title, body)) = plan.config_pull_request {
        let base = if plan.default_branch.trim().is_empty() {
            "main"
        } else {
            plan.default_branch.trim()
        };
        steps.push(step(
            "open_pull_request",
            format!("Open a pull request that adds {DEPENDABOT_CONFIG_PATH}"),
            "repo.open_pull_request",
            GITHUB,
            &[previous],
            &["repository:write"],
            params(
                plan.repository,
                &[
                    ("branch", DEPENDABOT_BRANCH),
                    ("base_branch", base),
                    ("title", title),
                    ("body", body),
                    ("skip_if_exists", DEPENDABOT_CONFIG_PATH),
                ],
            ),
            "the pull request exists, or the file was already there and nothing was opened",
        ));
    }
    steps
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

    fn shape(plan: &[PlanStepData]) -> Vec<(&str, &str, Vec<&str>, Vec<&str>)> {
        plan.iter()
            .map(|s| {
                (
                    s.key.as_str(),
                    s.action_type.as_str(),
                    s.depends_on.iter().map(String::as_str).collect(),
                    s.required_permissions.iter().map(String::as_str).collect(),
                )
            })
            .collect()
    }

    fn param<'a>(step: &'a PlanStepData, key: &str) -> Option<&'a str> {
        step.params.get(key).map(String::as_str)
    }

    fn keys(step: &PlanStepData) -> Vec<&str> {
        step.params.keys().map(String::as_str).collect()
    }

    #[test]
    fn fix_plan_is_checkout_then_commit_fixes() {
        let plan = fix_plan(&FixPlan {
            repository: "o/repo",
            branch: "feat/docker",
            expected_head_sha: "abc123",
            message: "Apply 2 review fixes to #7\n",
            pr_number: 7,
            fixes: 2,
            files: 1,
        });
        assert_eq!(
            shape(&plan),
            vec![
                ("checkout", "repo.checkout", vec![], vec!["repository:read"]),
                (
                    "commit_fixes",
                    "repo.commit_fixes",
                    vec!["checkout"],
                    vec!["repository:write"]
                ),
            ]
        );
        assert!(plan.iter().all(|s| s.adapter == "github"));
        assert_eq!(keys(&plan[0]), ["repository"]);
        assert_eq!(
            keys(&plan[1]),
            [
                "branch",
                "expected_head_sha",
                "message",
                "pull_request",
                "repository"
            ]
        );
        assert_eq!(param(&plan[1], "branch"), Some("feat/docker"));
        assert_eq!(param(&plan[1], "expected_head_sha"), Some("abc123"));
        assert_eq!(param(&plan[1], "pull_request"), Some("7"));
        assert_eq!(
            param(&plan[1], "message"),
            Some("Apply 2 review fixes to #7\n")
        );
        assert_eq!(param(&plan[1], "repository"), Some("o/repo"));
        assert!(plan[1].title.contains("2 review fixes in 1 file"));
    }

    #[test]
    fn merge_plan_deletes_the_branch_only_when_given_one() {
        let mut request = MergePlan {
            repository: "o/repo",
            pr_number: 12,
            expected_head_sha: "abc123",
            method: "squash",
            delete_branch: None,
        };
        let plan = merge_plan(&request);
        assert_eq!(
            shape(&plan),
            vec![
                (
                    "verify_mergeable",
                    "repo.verify_mergeable",
                    vec![],
                    vec!["repository:read"]
                ),
                (
                    "merge",
                    "repo.merge_pull_request",
                    vec!["verify_mergeable"],
                    vec!["repository:write"]
                ),
            ]
        );
        assert_eq!(
            keys(&plan[0]),
            ["expected_head_sha", "pull_request", "repository"]
        );
        assert_eq!(
            keys(&plan[1]),
            [
                "expected_head_sha",
                "merge_method",
                "pull_request",
                "repository"
            ]
        );
        assert_eq!(param(&plan[1], "merge_method"), Some("squash"));
        assert_eq!(param(&plan[1], "pull_request"), Some("12"));
        assert_eq!(param(&plan[0], "expected_head_sha"), Some("abc123"));

        request.delete_branch = Some("feat/x");
        let plan = merge_plan(&request);
        assert_eq!(plan.len(), 3);
        assert_eq!(
            shape(&plan)[2],
            (
                "delete_branch",
                "repo.delete_branch",
                vec!["merge"],
                vec!["repository:write"]
            )
        );
        assert_eq!(keys(&plan[2]), ["branch", "pull_request", "repository"]);
        assert_eq!(param(&plan[2], "branch"), Some("feat/x"));
        request.delete_branch = Some("  ");
        assert_eq!(merge_plan(&request).len(), 2);
    }

    #[test]
    fn dependabot_plan_follows_the_flags() {
        let plan_for = |alerts, security_updates, config: bool| {
            dependabot_plan(&DependabotPlan {
                repository: "o/repo",
                default_branch: "trunk",
                alerts,
                security_updates,
                config_pull_request: config.then_some(("Add Dependabot", "Body")),
            })
        };
        let key_list =
            |plan: &[PlanStepData]| -> Vec<String> { plan.iter().map(|s| s.key.clone()).collect() };
        assert_eq!(
            key_list(&plan_for(true, false, false)),
            ["checkout", "enable_security_updates"]
        );
        assert_eq!(
            key_list(&plan_for(false, true, false)),
            ["checkout", "enable_security_updates"]
        );
        assert_eq!(
            key_list(&plan_for(false, false, true)),
            ["checkout", "open_pull_request"]
        );
        let full = plan_for(true, true, true);
        assert_eq!(
            shape(&full),
            vec![
                ("checkout", "repo.checkout", vec![], vec!["repository:read"]),
                (
                    "enable_security_updates",
                    "repo.enable_security_updates",
                    vec!["checkout"],
                    vec!["repository:admin"]
                ),
                (
                    "open_pull_request",
                    "repo.open_pull_request",
                    vec!["enable_security_updates"],
                    vec!["repository:write"]
                ),
            ]
        );
        assert_eq!(keys(&full[1]), ["alerts", "repository", "security_updates"]);
        assert_eq!(param(&full[1], "alerts"), Some("true"));
        assert_eq!(param(&full[1], "security_updates"), Some("true"));
        let only_updates = plan_for(false, true, false);
        assert_eq!(param(&only_updates[1], "alerts"), Some("false"));
        assert_eq!(param(&only_updates[1], "security_updates"), Some("true"));
        assert_eq!(param(&full[2], "branch"), Some("sdi/dependabot"));
        assert_eq!(
            param(&full[2], "skip_if_exists"),
            Some(".github/dependabot.yml")
        );
        assert_eq!(param(&full[2], "base_branch"), Some("trunk"));
        assert_eq!(param(&full[2], "title"), Some("Add Dependabot"));
        assert_eq!(param(&full[2], "body"), Some("Body"));
        // Without the settings step the pull request follows the checkout.
        assert_eq!(plan_for(false, false, true)[1].depends_on, ["checkout"]);
        assert!(full.iter().all(|s| s.adapter == "github"));
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

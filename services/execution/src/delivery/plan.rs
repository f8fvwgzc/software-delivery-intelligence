//! The plan of a deployment run (CONTRACT.md §22): four steps, the same for
//! a promotion and a rollback.
//!
//! | step | action_type | tool |
//! |---|---|---|
//! | verify image | `registry.verify_image` | `registry` |
//! | deploy | `deploy.development` · `deploy.staging` · `deploy.production` · `deploy.rollback` | the environment's deployer |
//! | verify health | `deploy.verify_health` | `http` |
//! | record | `deploy.record` | `github` |
//!
//! Each step carries everything its adapter needs as params, and only
//! that: names of workspace secrets, never their values. A plan is a
//! snapshot: editing the environment later does not change a run that is
//! already planned.

use std::collections::HashMap;

use sdi_proto::common::v1::PlanStep;

use super::{
    db::{environments::EnvironmentRow, registries::RegistryRow, releases::ReleaseRow},
    model::{
        self, ENV_DOCKER_LOCAL, ENV_GITHUB_ACTIONS, ENV_GITOPS_ARGOCD, ENV_GITOPS_FLUX,
        ENV_WEBHOOK, RUN_ROLLBACK, STEP_DEPLOY, STEP_RECORD, STEP_VERIFY_HEALTH, STEP_VERIFY_IMAGE,
    },
};
use crate::adapters::{
    self, azure_devops, docker,
    github::{self, OP_DEPLOYMENT_RECORD, OP_WORKFLOW_DISPATCH},
    gitops, http_health, jenkins, webhook,
};

pub struct PlanInput<'a> {
    pub environment: &'a EnvironmentRow,
    pub release: &'a ReleaseRow,
    pub registry: &'a RegistryRow,
    /// `promote` or `rollback`.
    pub run_kind: &'a str,
}

fn params<const N: usize>(pairs: [(&str, &str); N]) -> HashMap<String, String> {
    pairs
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect()
}

/// How long the deployer of a kind gets when the environment says nothing.
pub fn default_timeout_seconds(kind: &str) -> u64 {
    match kind {
        ENV_GITHUB_ACTIONS => github::DISPATCH_TIMEOUT_SECONDS,
        ENV_GITOPS_FLUX | ENV_GITOPS_ARGOCD => gitops::MERGE_TIMEOUT_SECONDS,
        model::ENV_JENKINS => jenkins::DEFAULT_TIMEOUT_SECONDS,
        model::ENV_AZURE_DEVOPS => azure_devops::DEFAULT_TIMEOUT_SECONDS,
        ENV_WEBHOOK => webhook::DEFAULT_TIMEOUT_SECONDS,
        _ => docker::LOCAL_DEPLOY_TIMEOUT_SECONDS,
    }
}

/// What the run is called in the execution list.
pub fn title(input: &PlanInput<'_>) -> String {
    let what = if input.release.project_name.is_empty() {
        model_name(&input.release.repo_full_name)
    } else {
        input.release.project_name.clone()
    };
    if input.run_kind == RUN_ROLLBACK {
        format!(
            "Roll back {what} in {} to {}",
            input.environment.name, input.release.version
        )
    } else {
        format!(
            "Deploy {what} {} to {}",
            input.release.version, input.environment.name
        )
    }
}

fn model_name(repo_full_name: &str) -> String {
    match super::image::repo_name(repo_full_name) {
        "" => "release".to_owned(),
        name => name.to_owned(),
    }
}

/// The permissions the deploy step needs beyond `deploy:<tier>`: what the
/// deployer does on the workspace's behalf.
fn deployer_permissions(kind: &str) -> &'static [&'static str] {
    match kind {
        ENV_GITHUB_ACTIONS | model::ENV_JENKINS | model::ENV_AZURE_DEVOPS => &["pipeline:trigger"],
        ENV_GITOPS_FLUX | ENV_GITOPS_ARGOCD => &["repository:write"],
        ENV_DOCKER_LOCAL => &["container:run"],
        _ => &[],
    }
}

pub fn build(input: &PlanInput<'_>) -> Vec<PlanStep> {
    let PlanInput {
        environment,
        release,
        registry,
        run_kind,
    } = input;
    let rollback = *run_kind == RUN_ROLLBACK;
    let reference = format!("{}:{}", release.image, release.tag);
    let position = environment.position.to_string();
    let config = &environment.config.0;
    let setting = |key: &str| config.get(key).map(String::as_str).unwrap_or_default();

    let verify_image = PlanStep {
        key: STEP_VERIFY_IMAGE.to_owned(),
        title: "Verify image in registry".to_owned(),
        description: format!(
            "Ask {} whether {reference} exists and record its digest.",
            registry.host
        ),
        action_type: adapters::VERIFY_IMAGE.to_owned(),
        adapter: adapters::TOOL_REGISTRY.to_owned(),
        depends_on: Vec::new(),
        required_permissions: vec!["registry:read".to_owned()],
        params: params([
            ("registry_kind", registry.kind.as_str()),
            ("registry_host", registry.host.as_str()),
            ("registry_auth", registry.auth.as_str()),
            ("username_secret", registry.username_secret.as_str()),
            ("password_secret", registry.password_secret.as_str()),
            ("image", release.image.as_str()),
            ("tag", release.tag.as_str()),
            ("digest", release.digest.as_str()),
        ]),
        verification: "the registry answers with the manifest's digest".to_owned(),
    };

    // The deployer: the release, the environment's settings, and how.
    let kind = environment.kind.as_str();
    let timeout = match setting("timeout_seconds") {
        "" => default_timeout_seconds(kind).to_string(),
        seconds => seconds.to_owned(),
    };
    let mut deploy_params = params([
        ("deployer", kind),
        ("run_kind", run_kind),
        ("environment", environment.name.as_str()),
        ("stage", position.as_str()),
        ("project_id", release.project_id.as_str()),
        ("repo", release.repo_full_name.as_str()),
        ("release_id", &release.id.to_string()),
        ("version", release.version.as_str()),
        ("image", release.image.as_str()),
        ("tag", release.tag.as_str()),
        ("digest", release.digest.as_str()),
        ("commit_sha", release.commit_sha.as_str()),
        ("timeout_seconds", timeout.as_str()),
    ]);
    for key in model::config_keys(kind) {
        if let Some(value) = config.get(*key) {
            deploy_params.insert((*key).to_owned(), value.clone());
        }
    }
    let how = match kind {
        ENV_GITHUB_ACTIONS => {
            deploy_params.insert("operation".to_owned(), OP_WORKFLOW_DISPATCH.to_owned());
            let workflow = match setting("workflow") {
                "" => github::DEFAULT_WORKFLOW,
                workflow => workflow,
            };
            deploy_params.insert("workflow".to_owned(), workflow.to_owned());
            format!(
                "Dispatch {workflow} on {} with the release as inputs and follow the run.",
                release.repo_full_name
            )
        }
        ENV_GITOPS_FLUX | ENV_GITOPS_ARGOCD => {
            // The GitOps repository defaults to the project's own.
            let repo = match setting("repo") {
                "" => release.repo_full_name.as_str(),
                repo => repo,
            };
            deploy_params.insert("repo".to_owned(), repo.to_owned());
            let strategy = match setting("strategy") {
                "" => gitops::STRATEGY_KUSTOMIZE,
                strategy => strategy,
            };
            let mode = match setting("mode") {
                "" => gitops::MODE_PULL_REQUEST,
                mode => mode,
            };
            deploy_params.insert("strategy".to_owned(), strategy.to_owned());
            deploy_params.insert("mode".to_owned(), mode.to_owned());
            format!(
                "Point {} in {repo} at {reference} ({strategy}) through a {} and let {} reconcile.",
                setting("path"),
                if mode == gitops::MODE_COMMIT {
                    "direct commit"
                } else {
                    "pull request that is waited for"
                },
                if kind == ENV_GITOPS_FLUX {
                    "Flux"
                } else {
                    "Argo CD"
                }
            )
        }
        model::ENV_JENKINS => format!(
            "Build the Jenkins job {} with the release as parameters and follow the build.",
            setting("job")
        ),
        model::ENV_AZURE_DEVOPS => format!(
            "Run Azure pipeline {} in {}/{} with the release as parameters and follow the run.",
            setting("pipeline_id"),
            setting("organization"),
            setting("project")
        ),
        ENV_WEBHOOK => {
            "Send the release to the environment's webhook, signed; 2xx is success.".to_owned()
        }
        _ => format!("Pull {reference} and run it on the local Docker daemon."),
    };
    let mut permissions =
        vec![model::deploy_permission(environment.position, environment.protected).to_owned()];
    permissions.extend(deployer_permissions(kind).iter().map(|p| (*p).to_owned()));
    let deploy = PlanStep {
        key: STEP_DEPLOY.to_owned(),
        title: if rollback {
            format!("Roll back {} to {}", environment.name, release.version)
        } else {
            format!("Deploy {} to {}", release.version, environment.name)
        },
        description: how,
        action_type: model::deploy_action(environment.position, environment.protected, run_kind)
            .to_owned(),
        adapter: model::deploy_tool(kind).to_owned(),
        depends_on: vec![STEP_VERIFY_IMAGE.to_owned()],
        required_permissions: permissions,
        params: deploy_params,
        verification: "the deployer reports success".to_owned(),
    };

    let health_timeout = match setting("health_timeout_seconds") {
        "" => http_health::DEFAULT_TIMEOUT_SECONDS.to_string(),
        seconds => seconds.to_owned(),
    };
    let health_target = if environment.url.is_empty() {
        "the address the deployer reports".to_owned()
    } else {
        http_health::health_url(&environment.url, &environment.health_path)
    };
    let verify_health = PlanStep {
        key: STEP_VERIFY_HEALTH.to_owned(),
        title: "Verify health".to_owned(),
        description: format!(
            "Poll {health_target} for a 2xx answer for up to {health_timeout}s; skipped when the environment has no URL."
        ),
        action_type: adapters::VERIFY_HEALTH.to_owned(),
        adapter: adapters::TOOL_HTTP.to_owned(),
        depends_on: vec![STEP_DEPLOY.to_owned()],
        required_permissions: Vec::new(),
        params: params([
            ("environment", environment.name.as_str()),
            ("url", environment.url.as_str()),
            ("health_path", environment.health_path.as_str()),
            ("health_timeout_seconds", health_timeout.as_str()),
        ]),
        verification: "the health URL answers 2xx".to_owned(),
    };

    let record = PlanStep {
        key: STEP_RECORD.to_owned(),
        title: "Record deployment".to_owned(),
        description: format!(
            "Create a GitHub Deployment for {} in {} so the repository shows what runs where.",
            release.version, environment.name
        ),
        action_type: adapters::DEPLOY_RECORD.to_owned(),
        adapter: adapters::TOOL_GITHUB.to_owned(),
        depends_on: vec![STEP_VERIFY_HEALTH.to_owned()],
        required_permissions: vec!["repository:read".to_owned()],
        params: params([
            ("operation", OP_DEPLOYMENT_RECORD),
            ("run_kind", run_kind),
            ("repo", release.repo_full_name.as_str()),
            ("commit_sha", release.commit_sha.as_str()),
            ("environment", environment.name.as_str()),
            ("url", environment.url.as_str()),
            (
                "protected",
                if environment.protected {
                    "true"
                } else {
                    "false"
                },
            ),
            ("version", release.version.as_str()),
            ("image", release.image.as_str()),
            ("tag", release.tag.as_str()),
            ("digest", release.digest.as_str()),
        ]),
        verification: "GitHub lists the deployment with a success status".to_owned(),
    };

    vec![verify_image, deploy, verify_health, record]
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use chrono::Utc;
    use sqlx::types::Json;
    use uuid::Uuid;

    use super::*;
    use crate::{
        adapters::{DELIVERY, runnable},
        delivery::model::{ENVIRONMENT_KINDS, RUN_PROMOTE},
    };

    fn registry() -> RegistryRow {
        RegistryRow {
            id: Uuid::now_v7(),
            name: "ghcr".into(),
            kind: "ghcr".into(),
            host: "ghcr.io".into(),
            namespace: "acme".into(),
            auth: "github_token".into(),
            username_secret: String::new(),
            password_secret: String::new(),
            is_default: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn release() -> ReleaseRow {
        ReleaseRow {
            id: Uuid::now_v7(),
            workspace_id: Uuid::nil(),
            project_id: "p-1".into(),
            version: "a1b2c3d".into(),
            commit_sha: "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678".into(),
            image: "ghcr.io/acme/api".into(),
            tag: "a1b2c3d".into(),
            digest: String::new(),
            source: "manual".into(),
            pipeline_run_id: String::new(),
            notes: String::new(),
            verified: false,
            created_by: "u-1".into(),
            repo_full_name: "acme/api".into(),
            project_name: "api".into(),
            created_at: Utc::now(),
        }
    }

    fn environment(
        kind: &str,
        position: i32,
        protected: bool,
        config: &[(&str, &str)],
    ) -> EnvironmentRow {
        EnvironmentRow {
            id: Uuid::now_v7(),
            workspace_id: Uuid::nil(),
            project_id: "p-1".into(),
            name: "staging".into(),
            position,
            kind: kind.into(),
            config: Json(
                config
                    .iter()
                    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                    .collect::<BTreeMap<_, _>>(),
            ),
            url: "https://staging.example.com".into(),
            health_path: "/health".into(),
            registry_id: None,
            protected,
            auto_promote: false,
            source: "ui".into(),
            current_release_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn plan(environment: &EnvironmentRow, run_kind: &str) -> Vec<PlanStep> {
        build(&PlanInput {
            environment,
            release: &release(),
            registry: &registry(),
            run_kind,
        })
    }

    #[test]
    fn the_plan_is_the_four_steps_of_the_contract_in_a_chain() {
        let environment = environment("github_actions", 1, false, &[]);
        let steps = plan(&environment, RUN_PROMOTE);
        let shape: Vec<(&str, &str, &str, Vec<&str>)> = steps
            .iter()
            .map(|s| {
                (
                    s.key.as_str(),
                    s.action_type.as_str(),
                    s.adapter.as_str(),
                    s.depends_on.iter().map(String::as_str).collect(),
                )
            })
            .collect();
        assert_eq!(
            shape,
            [
                ("verify_image", "registry.verify_image", "registry", vec![]),
                ("deploy", "deploy.staging", "github", vec!["verify_image"]),
                (
                    "verify_health",
                    "deploy.verify_health",
                    "http",
                    vec!["deploy"]
                ),
                ("record", "deploy.record", "github", vec!["verify_health"]),
            ]
        );
        // Every step is one the delivery runtime can run.
        for step in &steps {
            assert!(
                runnable(DELIVERY, &step.action_type),
                "{}",
                step.action_type
            );
        }
        assert_eq!(steps[0].required_permissions, ["registry:read"]);
        assert_eq!(
            steps[1].required_permissions,
            ["deploy:staging", "pipeline:trigger"]
        );
        assert!(steps[2].required_permissions.is_empty());
        assert_eq!(steps[3].required_permissions, ["repository:read"]);

        // The registry step knows where to look and how to authenticate.
        assert_eq!(steps[0].params["registry_host"], "ghcr.io");
        assert_eq!(steps[0].params["registry_auth"], "github_token");
        assert_eq!(steps[0].params["image"], "ghcr.io/acme/api");
        assert_eq!(steps[0].params["tag"], "a1b2c3d");
        // The dispatch gets its defaults.
        assert_eq!(steps[1].params["operation"], "workflow.dispatch");
        assert_eq!(steps[1].params["workflow"], "deploy.yml");
        assert_eq!(steps[1].params["repo"], "acme/api");
        assert_eq!(steps[1].params["timeout_seconds"], "1800");
        assert_eq!(steps[1].params["environment"], "staging");
        assert!(
            !steps[1].params.contains_key("ref"),
            "GitHub's default branch is asked for at run time"
        );
        assert_eq!(steps[2].params["url"], "https://staging.example.com");
        assert_eq!(steps[2].params["health_path"], "/health");
        assert_eq!(steps[2].params["health_timeout_seconds"], "120");
        assert_eq!(steps[3].params["operation"], "deployment.record");
        assert_eq!(
            steps[3].params["commit_sha"],
            "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678"
        );
        assert_eq!(steps[3].params["protected"], "false");
    }

    #[test]
    fn the_deploy_action_follows_stage_protection_and_run_kind() {
        let action = |position, protected, kind: &str| {
            plan(&environment("webhook", position, protected, &[]), kind)[1]
                .action_type
                .clone()
        };
        assert_eq!(action(0, false, RUN_PROMOTE), "deploy.development");
        assert_eq!(action(1, false, RUN_PROMOTE), "deploy.staging");
        assert_eq!(action(2, true, RUN_PROMOTE), "deploy.production");
        assert_eq!(action(0, true, RUN_PROMOTE), "deploy.production");
        assert_eq!(action(2, true, RUN_ROLLBACK), "deploy.rollback");
        // A rollback still needs the permission of the stage it touches.
        let steps = plan(&environment("webhook", 2, true, &[]), RUN_ROLLBACK);
        assert_eq!(steps[1].required_permissions, ["deploy:production"]);
        assert_eq!(steps[1].params["run_kind"], "rollback");
        assert_eq!(steps[3].params["run_kind"], "rollback");
        assert_eq!(steps[3].params["protected"], "true");
        assert!(steps[1].title.starts_with("Roll back staging to"));
        let development = plan(&environment("webhook", 0, false, &[]), RUN_PROMOTE);
        assert_eq!(development[1].required_permissions, ["deploy:development"]);
    }

    #[test]
    fn every_deployer_gets_its_tool_its_settings_and_its_permissions() {
        type Settings = Vec<(&'static str, &'static str)>;
        type Case = (&'static str, Settings, &'static str, Vec<&'static str>);
        let cases: [Case; 7] = [
            (
                "github_actions",
                vec![("workflow", "release.yml"), ("ref", "release")],
                "github",
                vec!["pipeline:trigger"],
            ),
            (
                "gitops_flux",
                vec![
                    ("repo", "acme/fleet"),
                    ("path", "apps/api/kustomization.yaml"),
                ],
                "gitops",
                vec!["repository:write"],
            ),
            (
                "gitops_argocd",
                vec![
                    ("path", "apps/api/values.yaml"),
                    ("strategy", "helm_values"),
                    ("mode", "commit"),
                ],
                "gitops",
                vec!["repository:write"],
            ),
            (
                "jenkins",
                vec![
                    ("url", "https://ci.example.com"),
                    ("job", "team/deploy"),
                    ("user_secret", "JENKINS_USER"),
                    ("token_secret", "JENKINS_TOKEN"),
                ],
                "jenkins",
                vec!["pipeline:trigger"],
            ),
            (
                "azure_devops",
                vec![
                    ("organization", "acme"),
                    ("project", "shop"),
                    ("pipeline_id", "12"),
                    ("pat_secret", "AZDO_PAT"),
                ],
                "azure_devops",
                vec!["pipeline:trigger"],
            ),
            (
                "webhook",
                vec![
                    ("url", "https://deploy.example.com/hook"),
                    ("secret", "HOOK_KEY"),
                ],
                "webhook",
                vec![],
            ),
            (
                "docker_local",
                vec![("port", "3000")],
                "docker",
                vec!["container:run"],
            ),
        ];
        assert_eq!(cases.len(), ENVIRONMENT_KINDS.len());
        for (kind, config, tool, extra) in cases {
            let environment = environment(kind, 1, false, &config);
            let steps = plan(&environment, RUN_PROMOTE);
            let deploy = &steps[1];
            assert_eq!(deploy.adapter, tool, "{kind}");
            assert_eq!(deploy.params["deployer"], kind);
            let mut permissions = vec!["deploy:staging"];
            permissions.extend(extra);
            assert_eq!(deploy.required_permissions, permissions, "{kind}");
            for (key, value) in &config {
                assert_eq!(deploy.params[*key], *value, "{kind}.{key}");
            }
            assert!(!deploy.description.is_empty());
            assert!(deploy.params["timeout_seconds"].parse::<u64>().unwrap() > 0);
        }
        // GitOps defaults: the project's own repository, a pull request, Kustomize.
        let own = plan(
            &environment(
                "gitops_flux",
                1,
                false,
                &[("path", "k8s/kustomization.yaml")],
            ),
            RUN_PROMOTE,
        );
        assert_eq!(own[1].params["repo"], "acme/api");
        assert_eq!(own[1].params["mode"], "pull_request");
        assert_eq!(own[1].params["strategy"], "kustomize_image");
        // Environment timeouts override the defaults.
        let tuned = plan(
            &environment(
                "webhook",
                1,
                false,
                &[("timeout_seconds", "90"), ("health_timeout_seconds", "600")],
            ),
            RUN_PROMOTE,
        );
        assert_eq!(tuned[1].params["timeout_seconds"], "90");
        assert_eq!(tuned[2].params["health_timeout_seconds"], "600");
    }

    #[test]
    fn plans_carry_secret_names_and_never_anything_else_about_them() {
        let mut registry = registry();
        registry.kind = "acr".into();
        registry.host = "acme.azurecr.io".into();
        registry.auth = "secret".into();
        registry.username_secret = "ACR_USER".into();
        registry.password_secret = "ACR_PASSWORD".into();
        let environment = environment(
            "webhook",
            2,
            true,
            &[
                ("url", "https://deploy.example.com/hook"),
                ("secret", "HOOK_KEY"),
            ],
        );
        let steps = build(&PlanInput {
            environment: &environment,
            release: &release(),
            registry: &registry,
            run_kind: RUN_PROMOTE,
        });
        assert_eq!(steps[0].params["username_secret"], "ACR_USER");
        assert_eq!(steps[0].params["password_secret"], "ACR_PASSWORD");
        assert_eq!(steps[1].params["secret"], "HOOK_KEY");
        assert_eq!(
            title(&PlanInput {
                environment: &environment,
                release: &release(),
                registry: &registry,
                run_kind: RUN_PROMOTE,
            }),
            "Deploy api a1b2c3d to staging"
        );
        assert_eq!(
            title(&PlanInput {
                environment: &environment,
                release: &release(),
                registry: &registry,
                run_kind: RUN_ROLLBACK,
            }),
            "Roll back api in staging to a1b2c3d"
        );
    }
}

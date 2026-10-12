//! CONTRACT.md §14: a pattern plus a `GenInput` becomes complete, committable
//! files. One module per family, every generator a pure
//! `fn(&GenInput) -> Vec<ArtifactData>`. Which families a pattern needs is
//! decided here from the catalog slug (CONTRACT.md §5.6) with a fallback on
//! the pattern's category, so a learned pattern still gets sensible files.
//!
//! `delivery` is the one family that is not driven by a pattern: it wires
//! the project to the registries and environments the execution service
//! holds for it (CONTRACT.md §22).

pub mod bicep;
pub mod compose;
pub mod delivery;
pub mod dependabot;
pub mod dockerfile;
pub mod docs;
pub mod env;
pub mod github_actions;
pub mod input;
pub mod kubernetes;
pub mod nginx;
pub mod terraform;

pub use input::{GenInput, Package, Stack};

use crate::domain::model::ArtifactData;

/// pnpm major the generated files pin, in the CI workflow (`pnpm/action-setup`)
/// and in the runtime image, so both run the same package manager.
pub const PNPM_MAJOR: &str = "9";

/// Deploy targets `github_actions::deploy` knows how to write a workflow for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeployTarget {
    AzureContainerApps,
    AzureAppService,
    AwsEcs,
    AwsLambda,
    GcpCloudRun,
    Fly,
    Render,
    Vm,
    GithubPages,
    Kubernetes,
    /// Publish the image to GHCR only.
    Registry,
}

/// Builds one artifact; `replaces_existing` comes from the facts' known paths.
pub fn artifact(
    input: &GenInput,
    path: &str,
    language: &str,
    purpose: impl Into<String>,
    content: String,
) -> ArtifactData {
    ArtifactData {
        path: path.to_owned(),
        content,
        language: language.to_owned(),
        purpose: purpose.into(),
        replaces_existing: input.exists(path),
    }
}

/// Deploy target of a catalog slug, when the pattern deploys somewhere.
pub fn deploy_target(pattern_id: &str, cloud: &str, runtime: &str) -> Option<DeployTarget> {
    let by_slug = match pattern_id {
        "azure-container-apps" => Some(DeployTarget::AzureContainerApps),
        "azure-app-service" => Some(DeployTarget::AzureAppService),
        "aws-ecs-fargate" => Some(DeployTarget::AwsEcs),
        "aws-lambda-container" => Some(DeployTarget::AwsLambda),
        "gcp-cloud-run" => Some(DeployTarget::GcpCloudRun),
        "fly-io" => Some(DeployTarget::Fly),
        "render-web-service" => Some(DeployTarget::Render),
        "vm-docker-compose" | "hetzner-vm-compose" => Some(DeployTarget::Vm),
        "github-pages-static" => Some(DeployTarget::GithubPages),
        "kubernetes-helm" => Some(DeployTarget::Kubernetes),
        "gha-docker-publish" => Some(DeployTarget::Registry),
        _ => None,
    };
    by_slug.or(match (cloud, runtime) {
        (_, "kubernetes") => Some(DeployTarget::Kubernetes),
        (_, "static_hosting") => Some(DeployTarget::GithubPages),
        (_, "vm_docker_compose") => Some(DeployTarget::Vm),
        ("azure", "serverless_container") => Some(DeployTarget::AzureContainerApps),
        ("azure", _) => Some(DeployTarget::AzureAppService),
        ("aws", "serverless") => Some(DeployTarget::AwsLambda),
        ("aws", _) => Some(DeployTarget::AwsEcs),
        ("gcp", _) => Some(DeployTarget::GcpCloudRun),
        (_, "paas") => Some(DeployTarget::Fly),
        _ => None,
    })
}

/// Every file an option of this pattern would add. Documentation patterns
/// produce documents, not repository files, so they yield nothing.
pub fn for_pattern(
    pattern_id: &str,
    category: &str,
    cloud: &str,
    runtime: &str,
    input: &GenInput,
) -> Vec<ArtifactData> {
    let mut files: Vec<ArtifactData> = Vec::new();
    let mut add = |more: Vec<ArtifactData>| {
        for file in more {
            if !files.iter().any(|f| f.path == file.path) {
                files.push(file);
            }
        }
    };
    let target = deploy_target(pattern_id, cloud, runtime);

    match pattern_id {
        "github-pages-static" => {
            add(github_actions::deploy(input, DeployTarget::GithubPages));
            add(docs::deployment(input));
        }
        "gha-ci-build-test" => add(github_actions::ci(input)),
        "gha-docker-publish" => {
            add(dockerfile::generate(input));
            add(github_actions::ci(input));
            add(github_actions::deploy(input, DeployTarget::Registry));
        }
        "monorepo-ci-matrix" => add(github_actions::ci(input)),
        "dependabot-weekly" => add(dependabot::generate(input)),
        "docker-compose-test-env" => {
            add(compose::generate(input));
            add(github_actions::ci(input));
        }
        "uat-checklist" => add(docs::uat(input)),
        "terraform-remote-state" => {
            add(terraform::remote_state(input));
            add(github_actions::terraform_plan(input));
        }
        "secret-scanning-baseline" => {
            add(env::generate(input));
            add(github_actions::secret_scan(input));
        }
        "readme-architecture-docs" => {}
        "containerize-legacy-app" => {
            add(dockerfile::generate(input));
            add(compose::generate(input));
            add(env::generate(input));
            add(docs::deployment(input));
        }
        "add-ci-baseline" => {
            add(github_actions::ci(input));
            add(dependabot::generate(input));
        }
        "upgrade-runtime" => {
            add(dockerfile::generate(input));
            add(github_actions::ci(input));
            add(docs::runbook(input));
        }
        "config-to-environment" => {
            add(env::generate(input));
            add(compose::generate(input));
            add(docs::runbook(input));
        }
        "add-healthcheck" => {
            add(dockerfile::generate(input));
            add(compose::generate(input));
            add(docs::runbook(input));
        }
        "managed-database" => {
            add(terraform::generate(input));
            add(env::generate(input));
            add(docs::runbook(input));
        }
        _ => match target {
            Some(t) => add(deployment_bundle(input, t)),
            None => match category {
                "ci_cd" => add(github_actions::ci(input)),
                "dependency" => add(dependabot::generate(input)),
                "testing" => {
                    add(compose::generate(input));
                    add(github_actions::ci(input));
                }
                "security" => {
                    add(env::generate(input));
                    add(github_actions::secret_scan(input));
                }
                "infrastructure" => add(terraform::generate(input)),
                "modernization" => {
                    add(dockerfile::generate(input));
                    add(github_actions::ci(input));
                    add(env::generate(input));
                    add(docs::deployment(input));
                }
                _ => {}
            },
        },
    }
    files
}

/// Everything a deployment to `target` needs: container, pipeline, infra, docs.
fn deployment_bundle(input: &GenInput, target: DeployTarget) -> Vec<ArtifactData> {
    let mut files = Vec::new();
    match target {
        DeployTarget::GithubPages => {
            files.extend(github_actions::deploy(input, target));
        }
        DeployTarget::Vm => {
            files.extend(dockerfile::generate(input));
            files.extend(compose::generate(input));
            files.extend(nginx::generate(input));
            files.extend(env::generate(input));
            files.extend(github_actions::ci(input));
            files.extend(github_actions::deploy(input, target));
        }
        DeployTarget::Kubernetes => {
            files.extend(dockerfile::generate(input));
            files.extend(kubernetes::generate(input));
            files.extend(github_actions::ci(input));
            files.extend(github_actions::deploy(input, target));
        }
        DeployTarget::AzureContainerApps | DeployTarget::AzureAppService => {
            files.extend(dockerfile::generate(input));
            files.extend(github_actions::ci(input));
            files.extend(github_actions::deploy(input, target));
            files.extend(bicep::generate(input));
            files.extend(terraform::generate(input));
            files.extend(env::generate(input));
        }
        DeployTarget::AwsEcs | DeployTarget::AwsLambda | DeployTarget::GcpCloudRun => {
            files.extend(dockerfile::generate(input));
            files.extend(github_actions::ci(input));
            files.extend(github_actions::deploy(input, target));
            files.extend(terraform::generate(input));
            files.extend(env::generate(input));
        }
        DeployTarget::Fly | DeployTarget::Render | DeployTarget::Registry => {
            files.extend(dockerfile::generate(input));
            files.extend(github_actions::ci(input));
            files.extend(github_actions::deploy(input, target));
            files.extend(env::generate(input));
        }
    }
    files.extend(docs::deployment(input));
    files.extend(docs::runbook(input));
    files
}

#[cfg(test)]
pub(crate) mod snapshot {
    //! Snapshot files live next to the generators. Run with
    //! `SDI_UPDATE_SNAPSHOTS=1` to rewrite them after an intended change.

    use std::path::PathBuf;

    use crate::domain::model::ArtifactData;

    fn dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("generators")
            .join("snapshots")
    }

    pub fn assert_snapshot(name: &str, artifact: &ArtifactData) {
        let path = dir().join(format!("{name}.snap"));
        if std::env::var_os("SDI_UPDATE_SNAPSHOTS").is_some() {
            std::fs::write(&path, &artifact.content).expect("write snapshot");
        }
        let expected = std::fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("missing snapshot {}", path.display()));
        assert_eq!(
            artifact.content, expected,
            "snapshot {name} ({}) differs",
            artifact.path
        );
        assert!(
            !artifact.content.contains("TODO") && !artifact.content.contains("FIXME"),
            "{name} has a placeholder"
        );
        assert!(!artifact.purpose.is_empty(), "{name} has no purpose");
    }
}

#[cfg(test)]
mod tests {
    use super::{input::fixtures, *};

    #[test]
    fn slug_decides_the_families_and_category_is_the_fallback() {
        let input = fixtures::input(&fixtures::node_context(), "azure");
        let paths =
            |files: &[ArtifactData]| files.iter().map(|f| f.path.clone()).collect::<Vec<_>>();
        let aca = for_pattern(
            "azure-container-apps",
            "deployment",
            "azure",
            "serverless_container",
            &input,
        );
        let p = paths(&aca);
        assert!(p.contains(&"Dockerfile".to_owned()));
        assert!(p.contains(&".github/workflows/deploy.yml".to_owned()));
        assert!(p.contains(&"infra/azure/main.bicep".to_owned()));
        assert!(p.contains(&"infra/azure/main.tf".to_owned()));
        assert!(p.contains(&"docs/deployment.md".to_owned()));
        assert!(
            aca.iter()
                .find(|a| a.path == "Dockerfile")
                .unwrap()
                .replaces_existing
        );
        assert!(
            !aca.iter()
                .find(|a| a.path == ".github/workflows/deploy.yml")
                .unwrap()
                .replaces_existing
        );
        assert!(
            aca.iter()
                .find(|a| a.path == ".github/workflows/ci.yml")
                .unwrap()
                .replaces_existing
        );

        assert!(
            for_pattern(
                "readme-architecture-docs",
                "documentation",
                "any",
                "",
                &input
            )
            .is_empty()
        );
        let learned = for_pattern("team-ci", "ci_cd", "any", "", &input);
        assert_eq!(paths(&learned), vec![".github/workflows/ci.yml"]);
        let gcp_input = fixtures::input(&fixtures::node_context(), "gcp");
        let unknown_deploy = for_pattern(
            "team-gcp",
            "deployment",
            "gcp",
            "serverless_container",
            &gcp_input,
        );
        assert!(paths(&unknown_deploy).contains(&"infra/gcp/main.tf".to_owned()));
        for file in aca.iter().chain(&learned).chain(&unknown_deploy) {
            assert!(!file.content.contains("TODO"), "{} has a TODO", file.path);
            assert!(
                file.content.ends_with('\n'),
                "{} lacks a final newline",
                file.path
            );
        }
    }

    #[test]
    fn deploy_targets_fall_back_on_cloud_and_runtime() {
        assert_eq!(
            deploy_target("fly-io", "any", "paas"),
            Some(DeployTarget::Fly)
        );
        assert_eq!(
            deploy_target("x", "aws", "serverless_container"),
            Some(DeployTarget::AwsEcs)
        );
        assert_eq!(
            deploy_target("x", "any", "kubernetes"),
            Some(DeployTarget::Kubernetes)
        );
        assert_eq!(deploy_target("x", "any", ""), None);
    }
}

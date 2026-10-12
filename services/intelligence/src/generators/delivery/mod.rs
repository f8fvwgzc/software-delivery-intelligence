//! CONTRACT.md §22 "Setup files": everything that wires a project's CI to
//! its registry and its environments' deployers.
//!
//! * `.github/workflows/build.yml` builds and pushes the image a release is
//!   cut from, with a login per registry kind ([`build_workflow`]);
//! * `.github/workflows/deploy.yml` deploys one release to one GitHub
//!   environment, dispatched by the execution service ([`deploy_workflow`]);
//! * Flux ([`flux`]) or Argo CD ([`argocd`]) manifests for the GitOps
//!   environments, with the directory a promotion edits;
//! * a `Jenkinsfile` ([`jenkins`]) and an `azure-pipelines.yml`
//!   ([`azure_pipelines`]) for the environments those tools deploy.
//!
//! Only the deployer kinds the project uses get files. Like every other
//! generator this is a pure function of its input ([`DeliveryInput`]), and
//! every file is complete: nothing is left for a person to fill in inside
//! a file. What does need a person (a secret to create, an OIDC trust to
//! set up) is said in `notes` and listed in `required_secrets`.

pub mod argocd;
pub mod azure_pipelines;
pub mod build_workflow;
pub mod deploy_workflow;
pub mod flux;
mod gitops;
pub mod input;
pub mod jenkins;
pub mod target;

pub use input::DeliveryInput;

use sdi_proto::{
    execution::v1::{Environment, Registry},
    knowledge::v1::ProjectContext,
};

use self::{
    input::DeployerKind,
    target::{Target, TargetChoice},
};
use super::{DeployTarget, Stack, artifact, dockerfile};
use crate::domain::model::ArtifactData;

/// What the execution service holds for a project (CONTRACT.md §22): the
/// workspace's registries and the project's environments.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Setup {
    pub registries: Vec<Registry>,
    pub environments: Vec<Environment>,
}

/// The generated files with what a person has to do before they work.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeliveryFiles {
    pub artifacts: Vec<ArtifactData>,
    /// Plain sentences: what each file does and what must be configured.
    pub notes: Vec<String>,
    /// Secret names the generated files reference, in first-use order.
    pub required_secrets: Vec<String>,
}

/// What the generators write into. A path, a note and a secret are each
/// recorded once, in the order they first came up.
pub type Output = DeliveryFiles;

impl DeliveryFiles {
    pub(crate) fn file(
        &mut self,
        input: &DeliveryInput,
        path: &str,
        language: &str,
        purpose: impl Into<String>,
        content: String,
    ) {
        if self.artifacts.iter().any(|a| a.path == path) {
            return;
        }
        self.artifacts
            .push(artifact(&input.base, path, language, purpose, content));
    }

    pub(crate) fn note(&mut self, note: impl Into<String>) {
        let note = note.into();
        if !self.notes.contains(&note) {
            self.notes.push(note);
        }
    }

    pub(crate) fn secret(&mut self, name: &str) {
        if !self.required_secrets.iter().any(|s| s == name) {
            self.required_secrets.push(name.to_owned());
        }
    }

    /// Appends what a generator collected on the side while rendering, so
    /// the note that introduces a file can come before the notes about what
    /// the file needs.
    pub(crate) fn absorb(&mut self, other: DeliveryFiles) {
        for note in other.notes {
            self.note(note);
        }
        for secret in other.required_secrets {
            self.secret(&secret);
        }
    }

    /// Adds these files to an option's own (CONTRACT.md §6 Artifacts): a
    /// file at the same path is replaced, and a pattern's deploy workflow
    /// that publishes an image on every push goes, because the build
    /// workflow now does that and a deploy belongs behind the gates.
    pub fn merge_into(&self, artifacts: &mut Vec<ArtifactData>) {
        artifacts.retain(|existing| {
            let replaced = self.artifacts.iter().any(|d| d.path == existing.path);
            let publishes_on_push = existing.path == ".github/workflows/deploy.yml"
                && existing.content.contains("docker/build-push-action")
                && existing.content.contains("push: true");
            !replaced && !publishes_on_push
        });
        artifacts.extend(self.artifacts.iter().cloned());
    }
}

/// Every setup file of a project, from its context and what the execution
/// service holds for it.
pub fn for_project(
    ctx: &ProjectContext,
    registries: &[Registry],
    environments: &[Environment],
) -> DeliveryFiles {
    generate(&input::build(ctx, registries, environments))
}

/// The setup files of one proposal option. An option proposes where the
/// project runs, so when its pattern names a deploy target the deploy step
/// follows the pattern instead of what the repository shows today: the
/// option "Azure Container Apps" must not ship a workflow that restarts
/// Docker Compose over SSH.
pub fn for_option(
    ctx: &ProjectContext,
    setup: &Setup,
    pattern: Option<(DeployTarget, &str)>,
) -> DeliveryFiles {
    let mut input = input::build(ctx, &setup.registries, &setup.environments);
    if let Some((deploy_target, name)) = pattern
        && let Some(target) = Target::of_pattern(deploy_target)
    {
        input.target = TargetChoice {
            target,
            evidence: format!("the option sets up {name}"),
            detected: true,
        };
    }
    generate(&input)
}

pub fn generate(input: &DeliveryInput) -> DeliveryFiles {
    let mut out = DeliveryFiles::default();
    for note in &input.notes {
        out.note(note.clone());
    }
    build_workflow::generate(input, &mut out);
    // A build needs something to build: the Dockerfile the scan did not find.
    if !input.primary_image().dockerfile_exists {
        if input.base.stack == Stack::Unknown {
            out.note(format!(
                "No Dockerfile was found and the stack could not be detected, so none is generated: add `{}` before the first build.",
                input.primary_image().dockerfile
            ));
        } else {
            for file in dockerfile::generate(&input.base) {
                out.file(
                    input,
                    &file.path,
                    &file.language,
                    file.purpose,
                    file.content,
                );
            }
        }
    }
    deploy_workflow::generate(input, &mut out);
    flux::generate(input, &mut out);
    argocd::generate(input, &mut out);
    jenkins::generate(input, &mut out);
    azure_pipelines::generate(input, &mut out);

    if input.environments.is_empty() {
        out.note(
            "No environments are configured for this project yet, so only the build is set up. Add environments (or `delivery.environments` in `.sdi.yml`) and generate again for the deploy files.",
        );
    }
    for env in &input.environments {
        match env.kind {
            DeployerKind::Webhook => out.note(format!(
                "`{}` is deployed by a signed webhook to {}: verify the `X-SDI-Signature-256` header (HMAC-SHA256 of the body){}. No file is generated for it.",
                env.name,
                match env.setting("url") {
                    "" => "its configured URL".to_owned(),
                    url => format!("`{url}`"),
                },
                match env.setting("secret") {
                    "" => String::new(),
                    secret => format!(" with the workspace secret `{secret}`"),
                }
            )),
            DeployerKind::DockerLocal => out.note(format!(
                "`{}` runs on the Docker daemon of the SDI host (development only). No file is generated for it.",
                env.name
            )),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests;

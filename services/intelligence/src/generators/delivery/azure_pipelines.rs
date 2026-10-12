//! `azure-pipelines.yml` for `azure_devops` environments: a pipeline the
//! execution service queues through the Pipelines API with the parameters
//! `environment`, `image`, `digest` and `version` and polls to its end
//! (CONTRACT.md §22). The work is a deployment job bound to the Azure
//! DevOps environment of the same name, so the approvals and checks
//! configured there hold the run before anything is deployed.
//!
//! It deploys the same way the GitHub workflow would
//! ([`Target`](super::target::Target)). Secrets come from one variable
//! group per environment, mapped into the script explicitly: Azure
//! Pipelines never exposes a secret variable to a script on its own.

use super::{
    Output,
    input::{DeliveryInput, DeployerKind, EnvironmentSpec},
    target::{Plan, SettingDefault},
};
use crate::generators::compose::yaml_string;

/// The repository's own pipeline definition is left alone: the deploy
/// pipeline goes next to it.
pub fn file_name(input: &DeliveryInput) -> &'static str {
    if input.base.exists("azure-pipelines.yml") {
        "azure-pipelines.deploy.yml"
    } else {
        "azure-pipelines.yml"
    }
}

/// A double-quoted shell string.
fn shell_string(value: &str) -> String {
    let mut out = String::from("\"");
    for c in value.chars() {
        if matches!(c, '\\' | '"' | '$' | '`') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

pub(super) fn generate(input: &DeliveryInput, out: &mut Output) {
    let environments = input.environments_of(DeployerKind::AzureDevops);
    if environments.is_empty() {
        return;
    }
    let target = input.target.target;
    let plan = target.plan(&input.base, &input.workload);
    let file = file_name(input);
    let names = environments
        .iter()
        .map(|e| format!("`{}`", e.name))
        .collect::<Vec<_>>()
        .join(", ");
    out.file(
        input,
        file,
        "yaml",
        format!(
            "Azure Pipelines definition that deploys one released image to one environment ({names}) on {}, behind the environment's approvals and checks.",
            target.label()
        ),
        pipeline(input, &environments, &plan),
    );

    out.note(format!(
        "`{file}` is queued by SDI with the parameters `environment`, `image`, `digest` and `version`; it deploys to {} because {}.",
        target.label(),
        input.target.evidence
    ));
    for env in &environments {
        out.note(format!(
            "`{}`: create a pipeline from `{file}` in the project `{}` of the organization `{}` and make sure the environment's `pipeline_id` ({}) is its id.{}",
            env.name,
            env.setting("project"),
            env.setting("organization"),
            env.setting("pipeline_id"),
            match env.setting("pat_secret") {
                "" => " Set `pat_secret` on the environment to the workspace secret holding a personal access token with the Build (read and execute) scope.".to_owned(),
                secret => format!(
                    " SDI queues it with the workspace secret `{secret}` (a personal access token with the Build read and execute scope)."
                ),
            }
        ));
    }
    out.note(format!(
        "Create the Azure DevOps environments {names} (Pipelines, Environments) and add the approvals and checks each stage needs: the deployment job waits for them."
    ));
    if !plan.credentials.is_empty() {
        for credential in &plan.credentials {
            out.secret(credential.name);
        }
        out.note(format!(
            "Create one variable group per environment named `{}-<environment>` (Pipelines, Library), allow this pipeline to use it, and add the secret variables {}.",
            input.base.image,
            plan.credentials
                .iter()
                .map(|c| format!("`{}` ({})", c.name, c.what))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !plan.settings.is_empty() {
        out.note(format!(
            "A variable group may also override {}.",
            plan.settings
                .iter()
                .map(|s| format!("`{}`", s.name))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
}

fn pipeline(input: &DeliveryInput, environments: &[&EnvironmentSpec], plan: &Plan) -> String {
    let target = input.target.target;
    let values: String = environments
        .iter()
        .map(|e| format!("      - {}\n", yaml_string(&e.name)))
        .collect();
    // Defaults first, the group last: a group may override a default.
    let variables: String = plan
        .settings
        .iter()
        .map(|s| {
            let value = match &s.default {
                SettingDefault::Text(text) => yaml_string(text),
                SettingDefault::EnvironmentName => "${{ parameters.environment }}".to_owned(),
            };
            format!("  - name: {}\n    value: {value}\n", s.name)
        })
        .collect();
    let step = "                    ";
    let mut resolve: String = target
        .resolve_reference()
        .iter()
        .map(|line| format!("{step}{line}\n"))
        .collect();
    resolve.push_str(&format!(
        "{step}echo \"##vso[task.setvariable variable=IMAGE_REF]$ref\"\n"
    ));
    if target.needs_image_tag() {
        resolve.push_str(&format!(
            "{step}echo \"##vso[task.setvariable variable=IMAGE_TAG]$tag\"\n"
        ));
    }
    let script: String = plan
        .login
        .iter()
        .chain(&plan.deploy)
        .map(|line| format!("{step}{line}\n"))
        .collect();
    let secrets: String = plan
        .credentials
        .iter()
        .map(|c| format!("{step}{name}: $({name})\n", name = c.name))
        .collect();
    let urls: Vec<String> = environments
        .iter()
        .filter(|e| !e.health_url().is_empty())
        .map(|e| {
            format!(
                "{step}  {}) url={} ;;\n",
                shell_string(&e.name),
                shell_string(&e.health_url())
            )
        })
        .collect();
    let health = if urls.is_empty() {
        String::new()
    } else {
        format!(
            "                - bash: |
{step}set -eu
{step}case \"$ENVIRONMENT\" in
{cases}{step}  *) echo \"No URL is configured for $ENVIRONMENT; skipping the health check\"; exit 0 ;;
{step}esac
{step}curl --retry 10 --retry-delay 6 --retry-all-errors -fsS \"$url\"
                  displayName: Verify health
                  env:
{step}ENVIRONMENT: ${{{{ parameters.environment }}}}
",
            cases = urls.concat(),
        )
    };
    format!(
        "# Deploys one released image of {app} to one environment.
# SDI queues this pipeline once a promotion has passed its policy gates and
# follows the run to its end. The approvals and checks of the Azure DevOps
# environment apply before the deployment job starts.

trigger: none
pr: none

parameters:
  - name: environment
    displayName: Environment to deploy to
    type: string
    values:
{values}  - name: image
    displayName: Image repository without a tag
    type: string
    default: {image}
  - name: digest
    displayName: Digest of the released image (sha256:...)
    type: string
    default: \"\"
  - name: version
    displayName: Release version, which is also the image tag
    type: string
    default: \"\"

variables:
{variables}  - group: {group}-${{{{ parameters.environment }}}}

pool:
  vmImage: ubuntu-latest

stages:
  - stage: deploy
    displayName: Deploy to ${{{{ parameters.environment }}}}
    jobs:
      - deployment: deploy
        displayName: Deploy ${{{{ parameters.version }}}}
        environment: ${{{{ parameters.environment }}}}
        timeoutInMinutes: 30
        strategy:
          runOnce:
            deploy:
              steps:
                - download: none
                - checkout: {checkout}
                - bash: |
{step}set -eu
{resolve}                  displayName: Verify the release
                  env:
{step}IMAGE: ${{{{ parameters.image }}}}
{step}DIGEST: ${{{{ parameters.digest }}}}
{step}VERSION: ${{{{ parameters.version }}}}
                - bash: |
{step}set -eu
{script}                  displayName: Deploy to {label}
                  env:
{step}IMAGE: ${{{{ parameters.image }}}}
{secrets}{health}",
        app = input.base.image,
        image = yaml_string(&input.release_image()),
        group = input.base.image,
        checkout = if plan.needs_checkout { "self" } else { "none" },
        label = target.label(),
    )
}

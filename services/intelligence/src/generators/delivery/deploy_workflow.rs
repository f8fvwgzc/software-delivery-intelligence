//! `.github/workflows/deploy.yml`: deploys one release to one environment.
//!
//! The execution service dispatches it (`workflow_dispatch` with
//! `environment`, `image`, `digest` and `version`) once a promotion has
//! passed its policy gates, and follows the run to its end (CONTRACT.md
//! §22). The job runs in `environment: ${{ inputs.environment }}`, so
//! GitHub's own protection rules, secrets and variables of that environment
//! apply too, and one deployment per environment runs at a time.
//!
//! The deploy step is the one branch that fits the project
//! ([`Target`]), never a menu. Cloud targets log in through OIDC; nothing
//! here builds an image.

use super::{
    Output,
    input::{DeliveryInput, DeployerKind, EnvironmentSpec},
    target::{Credential, Plan, SettingDefault, Target},
};

/// The workflow file an environment is deployed by (`config.workflow`,
/// default `deploy.yml`).
pub fn file_name(env: &EnvironmentSpec) -> String {
    let name = env
        .setting("workflow")
        .trim_start_matches("./")
        .trim_start_matches(".github/workflows/");
    let is_file = !name.is_empty()
        && !name.contains('/')
        && !name.contains(char::is_whitespace)
        && (name.ends_with(".yml") || name.ends_with(".yaml"));
    if is_file {
        name.to_owned()
    } else {
        "deploy.yml".to_owned()
    }
}

pub(super) fn generate(input: &DeliveryInput, out: &mut Output) {
    let environments = input.environments_of(DeployerKind::GithubActions);
    let mut files: Vec<String> = Vec::new();
    for env in &environments {
        let file = file_name(env);
        if !files.contains(&file) {
            files.push(file);
        }
    }
    for file in files {
        let served: Vec<&EnvironmentSpec> = environments
            .iter()
            .copied()
            .filter(|e| file_name(e) == file)
            .collect();
        let names = served
            .iter()
            .map(|e| format!("`{}`", e.name))
            .collect::<Vec<_>>()
            .join(", ");
        let path = format!(".github/workflows/{file}");
        let target = input.target.target;
        let mut needs = Output::default();
        let content = workflow(input, &mut needs);
        out.file(
            input,
            &path,
            "yaml",
            format!(
                "Deploys one released image to one environment ({names}) on {}; dispatched by SDI after the promotion's gates, never on push.",
                target.label()
            ),
            content,
        );
        out.note(format!(
            "`{path}` takes the inputs `environment`, `image`, `digest` and `version` and runs in the GitHub environment of that name, so its protection rules, secrets and variables apply. It deploys to {} because {}.",
            target.label(),
            input.target.evidence
        ));
        if !input.target.detected {
            out.note(
                "No deployment target was detected, so the workflow copies the release to a host over SSH and restarts it with Docker Compose. Set `deploy.target` in `.sdi.yml` (`kubernetes`, `azure`, `aws`, `gcp`, `fly`, `render` or `vm`) and generate again to change that.",
            );
        }
        out.note(if served.len() == 1 {
            format!(
                "Create the GitHub environment {names} (Settings, Environments) and give it its secrets; a protected stage can also carry required reviewers there."
            )
        } else {
            format!(
                "Create the GitHub environments {names} (Settings, Environments) and give each its own secrets; a protected stage can also carry required reviewers there."
            )
        });
        out.absorb(needs);
    }
}

/// `'` doubled, as GitHub expressions quote strings.
fn expression_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// The steps that authenticate before the script, and whether the script
/// still logs in with the plan's own credentials.
struct Auth {
    steps: String,
    oidc: bool,
    script_login: bool,
}

fn auth(target: Target, input: &DeliveryInput, out: &mut Output) -> Auth {
    let repo = &input.base.repo_full_name;
    match target {
        Target::Kubectl => Auth {
            steps: "      - uses: azure/setup-kubectl@v4\n".to_owned(),
            oidc: false,
            script_login: true,
        },
        Target::Helm => Auth {
            steps: "      - uses: azure/setup-kubectl@v4\n      - uses: azure/setup-helm@v4\n"
                .to_owned(),
            oidc: false,
            script_login: true,
        },
        Target::AzureContainerApps | Target::AzureAppService => {
            for secret in [
                "AZURE_CLIENT_ID",
                "AZURE_TENANT_ID",
                "AZURE_SUBSCRIPTION_ID",
            ] {
                out.secret(secret);
            }
            out.note(format!(
                "Azure: add a federated credential with the subject `repo:{repo}:environment:<name>` for each environment to an app registration that may update the application, and store its ids as the environment secrets `AZURE_CLIENT_ID`, `AZURE_TENANT_ID` and `AZURE_SUBSCRIPTION_ID`."
            ));
            Auth {
                steps: "      - name: Log in to Azure
        uses: azure/login@v2
        with:
          client-id: ${{ secrets.AZURE_CLIENT_ID }}
          tenant-id: ${{ secrets.AZURE_TENANT_ID }}
          subscription-id: ${{ secrets.AZURE_SUBSCRIPTION_ID }}
"
                .to_owned(),
                oidc: true,
                script_login: false,
            }
        }
        Target::CloudRun => {
            out.secret("GCP_WORKLOAD_IDENTITY_PROVIDER");
            out.secret("GCP_DEPLOY_SERVICE_ACCOUNT");
            out.note(format!(
                "Google Cloud: let `{repo}` impersonate a service account with the `Cloud Run Developer` role through Workload Identity Federation, and store the provider resource name and the service account e-mail as the environment secrets `GCP_WORKLOAD_IDENTITY_PROVIDER` and `GCP_DEPLOY_SERVICE_ACCOUNT`."
            ));
            Auth {
                steps: "      - name: Authenticate to Google Cloud
        uses: google-github-actions/auth@v2
        with:
          workload_identity_provider: ${{ secrets.GCP_WORKLOAD_IDENTITY_PROVIDER }}
          service_account: ${{ secrets.GCP_DEPLOY_SERVICE_ACCOUNT }}
      - uses: google-github-actions/setup-gcloud@v2
"
                .to_owned(),
                oidc: true,
                script_login: false,
            }
        }
        Target::AwsEcs | Target::AwsLambda => {
            out.secret("AWS_DEPLOY_ROLE_ARN");
            out.note(format!(
                "AWS: create an IAM role that trusts GitHub's OIDC provider for `repo:{repo}:environment:<name>` and may {}, and store its ARN as the environment secret `AWS_DEPLOY_ROLE_ARN`.",
                if target == Target::AwsLambda {
                    "update the function's code"
                } else {
                    "register task definitions and update the service"
                }
            ));
            Auth {
                steps: "      - name: Configure AWS credentials
        uses: aws-actions/configure-aws-credentials@v4
        with:
          role-to-assume: ${{ secrets.AWS_DEPLOY_ROLE_ARN }}
          aws-region: ${{ env.AWS_REGION }}
"
                .to_owned(),
                oidc: true,
                script_login: false,
            }
        }
        Target::Fly | Target::Render | Target::SshCompose => Auth {
            steps: String::new(),
            oidc: false,
            script_login: true,
        },
    }
}

fn credential_notes(credentials: &[Credential], out: &mut Output) {
    if credentials.is_empty() {
        return;
    }
    for credential in credentials {
        out.secret(credential.name);
    }
    out.note(format!(
        "Each GitHub environment needs the secret{} {}.",
        if credentials.len() == 1 { "" } else { "s" },
        credentials
            .iter()
            .map(|c| format!("`{}` ({})", c.name, c.what))
            .collect::<Vec<_>>()
            .join(", ")
    ));
}

fn setting_notes(plan: &Plan, out: &mut Output) {
    if plan.settings.is_empty() {
        return;
    }
    out.note(format!(
        "Override per environment with variables when the defaults do not fit: {}.",
        plan.settings
            .iter()
            .map(|s| match &s.default {
                SettingDefault::Text(text) => format!("`{}` (default `{text}`)", s.name),
                SettingDefault::EnvironmentName =>
                    format!("`{}` (default: the environment's name)", s.name),
            })
            .collect::<Vec<_>>()
            .join(", ")
    ));
}

fn workflow(input: &DeliveryInput, out: &mut Output) -> String {
    let target = input.target.target;
    let plan = target.plan(&input.base, &input.workload);
    let auth = auth(target, input, out);
    let credentials: &[Credential] = if auth.script_login {
        &plan.credentials
    } else {
        &[]
    };
    credential_notes(credentials, out);
    setting_notes(&plan, out);
    match target {
        Target::CloudRun
            if !input.registries.iter().any(|r| {
                matches!(
                    r.kind,
                    super::input::RegistryKind::Gar | super::input::RegistryKind::DockerHub
                )
            }) =>
        {
            out.note(format!(
                "Cloud Run pulls from Artifact Registry or Docker Hub; `{}` is neither, so add an Artifact Registry remote repository in front of it or push there as well.",
                input.release_image()
            ));
        }
        Target::AwsLambda
            if !input
                .registries
                .iter()
                .any(|r| r.kind == super::input::RegistryKind::Ecr) =>
        {
            out.note(format!(
                "Lambda only runs images from Amazon ECR in the function's own account and region; `{}` is elsewhere, so push to an ECR repository as well and promote that image.",
                input.release_image()
            ));
        }
        Target::SshCompose => out.note(format!(
            "The host needs Docker with the Compose plugin, a `docker-compose.yml` in the deploy directory whose application service is named by `COMPOSE_SERVICE`, and a one-time `docker login {}` so it can pull the image.",
            input.primary_registry().host
        )),
        _ => {}
    }

    let settings: String = plan
        .settings
        .iter()
        .map(|s| {
            let fallback = match &s.default {
                SettingDefault::Text(text) => expression_string(text),
                SettingDefault::EnvironmentName => "inputs.environment".to_owned(),
            };
            format!(
                "      {name}: ${{{{ vars.{name} || {fallback} }}}}\n",
                name = s.name
            )
        })
        .collect();
    let mut resolve: String = target
        .resolve_reference()
        .iter()
        .map(|line| format!("          {line}\n"))
        .collect();
    resolve.push_str("          echo \"IMAGE_REF=$ref\" >> \"$GITHUB_ENV\"\n");
    if target.needs_image_tag() {
        resolve.push_str("          echo \"IMAGE_TAG=$tag\" >> \"$GITHUB_ENV\"\n");
    }
    let secret_env: String = if credentials.is_empty() {
        String::new()
    } else {
        format!(
            "        env:\n{}",
            credentials
                .iter()
                .map(|c| format!(
                    "          {name}: ${{{{ secrets.{name} }}}}\n",
                    name = c.name
                ))
                .collect::<String>()
        )
    };
    let script: String = plan
        .login
        .iter()
        .filter(|_| auth.script_login)
        .chain(&plan.deploy)
        .map(|line| format!("          {line}\n"))
        .collect();
    format!(
        "name: Deploy

on:
  workflow_dispatch:
    inputs:
      environment:
        description: Environment to deploy to
        required: true
        type: environment
      image:
        description: Image repository without a tag, such as {image}
        required: true
        type: string
      digest:
        description: Digest of the released image (sha256:...)
        required: false
        type: string
        default: \"\"
      version:
        description: Release version, which is also the image tag
        required: true
        type: string

run-name: Deploy ${{{{ inputs.version }}}} to ${{{{ inputs.environment }}}}

concurrency:
  group: deploy-${{{{ inputs.environment }}}}
  cancel-in-progress: false

permissions:
  contents: read
{id_token}
jobs:
  deploy:
    name: Deploy to ${{{{ inputs.environment }}}}
    runs-on: ubuntu-latest
    timeout-minutes: 30
    environment: ${{{{ inputs.environment }}}}
    env:
      IMAGE: ${{{{ inputs.image }}}}
      DIGEST: ${{{{ inputs.digest }}}}
      VERSION: ${{{{ inputs.version }}}}
{settings}    steps:
{checkout}      - name: Resolve the image reference
        run: |
{resolve}{auth_steps}      - name: Deploy to {label}
{secret_env}        run: |
          set -eu
{script}",
        image = input.release_image(),
        id_token = if auth.oidc { "  id-token: write\n" } else { "" },
        checkout = if plan.needs_checkout {
            "      - uses: actions/checkout@v4\n"
        } else {
            ""
        },
        auth_steps = auth.steps,
        label = target.label(),
    )
}

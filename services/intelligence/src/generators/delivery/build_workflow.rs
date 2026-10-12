//! `.github/workflows/build.yml`: builds each image of the repository,
//! pushes it to the project's registries and reports its digest. A
//! successful run on the default branch is what a release is cut from
//! (CONTRACT.md §22), so the tags here are the ones a release names: the
//! short commit SHA always, the version on a tag, `latest` on the default
//! branch.
//!
//! Logins never use a long-lived cloud key: GHCR takes the workflow's own
//! token, the cloud registries are reached through OIDC federation, and
//! only Docker Hub or a private registry reads a username and a password
//! from repository secrets.

use super::{
    Output,
    input::{DeliveryInput, ImageBuild, RegistryAuth, RegistryKind, RegistrySpec},
};
use crate::generators::compose::yaml_string;

/// The workflow file the build goes to: the name `.sdi.yml` gives it, as
/// long as that does not overwrite a workflow the repository already has.
pub fn file_name(input: &DeliveryInput) -> String {
    let configured = input.build_workflow.trim();
    if !configured.is_empty()
        && !input
            .base
            .exists(&format!(".github/workflows/{configured}"))
    {
        configured.to_owned()
    } else {
        "build.yml".to_owned()
    }
}

pub(super) fn generate(input: &DeliveryInput, out: &mut Output) {
    let file = file_name(input);
    let path = format!(".github/workflows/{file}");
    let registries = input
        .registries
        .iter()
        .map(|r| r.kind.label())
        .collect::<Vec<_>>();
    // Rendering collects what the logins need; that is said after the file
    // itself has been introduced.
    let mut needs = Output::default();
    let content = workflow(input, &mut needs);
    out.file(
        input,
        &path,
        "yaml",
        format!(
            "Builds {} and pushes {} to {} with provenance and an SBOM; its success on `{}` is what a release is cut from.",
            match input.images.as_slice() {
                [only] => format!("the image from `{}`", only.dockerfile),
                many => format!("{} images", many.len()),
            },
            if input.images.len() == 1 { "it" } else { "them" },
            join_words(&registries),
            input.base.default_branch
        ),
        content,
    );
    out.note(format!(
        "`{path}` builds on every push to `{}`, on `v*` tags and on demand, and pushes `{}` tagged with the short commit SHA (plus the version on a tag and `latest` on `{}`). The job outputs `image`, `digest` and `version`.",
        input.base.default_branch,
        input.release_image(),
        input.base.default_branch
    ));

    out.absorb(needs);

    // What `.sdi.yml` calls the build workflow must be the one that builds.
    let configured = input.build_workflow.trim();
    if configured.is_empty() {
        out.note(format!(
            "Add `delivery.build_workflow: {file}` to `.sdi.yml` so a successful run of this workflow creates the release."
        ));
    } else if configured != file {
        let existing = format!(".github/workflows/{configured}");
        if input.pushing_workflows.contains(&existing) {
            out.note(format!(
                "`.sdi.yml` names `{configured}` as the build workflow and it already pushes an image, so it was left untouched. Keep one of the two: drop `{path}` from this change, or remove the image job from `{configured}` and set `delivery.build_workflow: {file}`."
            ));
        } else {
            out.note(format!(
                "`.sdi.yml` names `{configured}` as the build workflow, but that workflow pushes no image and was left untouched. Set `delivery.build_workflow: {file}` so a successful build creates the release."
            ));
        }
    }
    for other in input
        .pushing_workflows
        .iter()
        .filter(|w| **w != path && **w != format!(".github/workflows/{configured}"))
    {
        out.note(format!(
            "`{other}` already pushes an image; with `{path}` in place the same commit would be published twice, so remove the image job from `{other}`."
        ));
    }
    out.note(
        "The image carries BuildKit provenance and an SBOM. The signed GitHub attestation step runs for public repositories only; a private repository needs GitHub Enterprise Cloud for it, and the `if:` on that step can go once yours has it.",
    );
    for image in input.images.iter().filter(|i| !i.dockerfile_exists) {
        out.note(format!(
            "The build expects `{}`, which the scan did not find; the Dockerfile in this change is generated for the detected stack.",
            image.dockerfile
        ));
    }
}

fn join_words(words: &[&str]) -> String {
    match words {
        [] => String::new(),
        [one] => (*one).to_owned(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

/// Step id suffix that keeps the ids of several registries of one kind apart.
fn suffix(index: usize, registries: &[RegistrySpec]) -> String {
    let kind = registries[index].kind;
    let same: Vec<usize> = registries
        .iter()
        .enumerate()
        .filter(|(_, r)| r.kind == kind)
        .map(|(i, _)| i)
        .collect();
    match same.iter().position(|i| *i == index) {
        Some(0) | None => String::new(),
        Some(n) => format!("-{}", n + 1),
    }
}

/// The login steps of one registry, and what they need configured.
fn login(index: usize, input: &DeliveryInput, out: &mut Output) -> String {
    let registry = &input.registries[index];
    let host = &registry.host;
    let n = suffix(index, &input.registries);
    match (registry.kind, registry.auth) {
        (_, RegistryAuth::Secret) => {
            out.secret(&registry.username_secret);
            out.secret(&registry.password_secret);
            out.note(format!(
                "Create the repository secrets `{}` and `{}` (Settings, Secrets and variables, Actions) with an account that may push to {}{}.",
                registry.username_secret,
                registry.password_secret,
                registry.image(""),
                if registry.kind == RegistryKind::DockerHub {
                    "; use an access token, not the account password"
                } else {
                    ""
                }
            ));
            let target = if registry.kind == RegistryKind::DockerHub {
                String::new()
            } else {
                format!("          registry: {host}\n")
            };
            format!(
                "      - name: Log in to {label}
        uses: docker/login-action@v3
        with:
{target}          username: ${{{{ secrets.{user} }}}}
          password: ${{{{ secrets.{password} }}}}
",
                label = if registry.kind == RegistryKind::Generic {
                    host.as_str()
                } else {
                    registry.kind.label()
                },
                user = registry.username_secret,
                password = registry.password_secret,
            )
        }
        (RegistryKind::Acr, _) => {
            for secret in [
                "AZURE_CLIENT_ID",
                "AZURE_TENANT_ID",
                "AZURE_SUBSCRIPTION_ID",
            ] {
                out.secret(secret);
            }
            out.note(format!(
                "Azure: add a federated credential for `repo:{}:ref:refs/heads/{}` (and one for tags) to an app registration that holds the `AcrPush` role on `{}`, then store its ids as the secrets `AZURE_CLIENT_ID`, `AZURE_TENANT_ID` and `AZURE_SUBSCRIPTION_ID`. No client secret is needed.",
                input.base.repo_full_name,
                input.base.default_branch,
                registry.acr_name()
            ));
            format!(
                "      - name: Log in to Azure
        uses: azure/login@v2
        with:
          client-id: ${{{{ secrets.AZURE_CLIENT_ID }}}}
          tenant-id: ${{{{ secrets.AZURE_TENANT_ID }}}}
          subscription-id: ${{{{ secrets.AZURE_SUBSCRIPTION_ID }}}}
      - name: Log in to Azure Container Registry
        run: az acr login --name {name}
",
                name = registry.acr_name()
            )
        }
        (RegistryKind::Ecr, _) => {
            out.secret("AWS_BUILD_ROLE_ARN");
            out.note(format!(
                "AWS: create an IAM role that trusts GitHub's OIDC provider for `repo:{}:*` and may push to ECR, and store its ARN as the secret `AWS_BUILD_ROLE_ARN`. No access key is needed.",
                input.base.repo_full_name
            ));
            let (region, public) = if registry.is_public_ecr() {
                // The public registry only authenticates in us-east-1.
                (
                    "us-east-1".to_owned(),
                    "        with:\n          registry-type: public\n",
                )
            } else {
                match registry.ecr_region() {
                    Some(region) => (region.to_owned(), ""),
                    None => {
                        out.note(format!(
                            "The region of `{host}` could not be read from its name; set the repository variable `AWS_REGION`."
                        ));
                        ("${{ vars.AWS_REGION }}".to_owned(), "")
                    }
                }
            };
            format!(
                "      - name: Configure AWS credentials
        uses: aws-actions/configure-aws-credentials@v4
        with:
          role-to-assume: ${{{{ secrets.AWS_BUILD_ROLE_ARN }}}}
          aws-region: {region}
      - name: Log in to Amazon ECR
        uses: aws-actions/amazon-ecr-login@v2
{public}"
            )
        }
        (RegistryKind::Gar, _) => {
            out.secret("GCP_WORKLOAD_IDENTITY_PROVIDER");
            out.secret("GCP_BUILD_SERVICE_ACCOUNT");
            out.note(format!(
                "Google Cloud: allow the repository `{}` to impersonate a service account with the `Artifact Registry Writer` role through Workload Identity Federation, and store the provider resource name and the service account e-mail as the secrets `GCP_WORKLOAD_IDENTITY_PROVIDER` and `GCP_BUILD_SERVICE_ACCOUNT`. No key file is needed.",
                input.base.repo_full_name
            ));
            format!(
                "      - id: gcp-auth{n}
        name: Authenticate to Google Cloud
        uses: google-github-actions/auth@v2
        with:
          token_format: access_token
          workload_identity_provider: ${{{{ secrets.GCP_WORKLOAD_IDENTITY_PROVIDER }}}}
          service_account: ${{{{ secrets.GCP_BUILD_SERVICE_ACCOUNT }}}}
      - name: Log in to Artifact Registry
        uses: docker/login-action@v3
        with:
          registry: {host}
          username: oauth2accesstoken
          password: ${{{{ steps.gcp-auth{n}.outputs.access_token }}}}
"
            )
        }
        // GHCR with the workflow's own token; nothing to configure.
        _ => format!(
            "      - name: Log in to GitHub Container Registry
        uses: docker/login-action@v3
        with:
          registry: {host}
          username: ${{{{ github.actor }}}}
          password: ${{{{ secrets.GITHUB_TOKEN }}}}
"
        ),
    }
}

fn job(image: &ImageBuild, logins: &str, input: &DeliveryInput) -> String {
    let images: String = input
        .registries
        .iter()
        .map(|r| format!("            {}\n", r.image(&image.name)))
        .collect();
    let subject = input.primary_registry().image(&image.name);
    // Several images share one cache backend; a scope each keeps them apart.
    let scope = if input.images.len() > 1 {
        format!(",scope={}", image.job)
    } else {
        String::new()
    };
    format!(
        "  {job}:
    name: Build and push {name}
    runs-on: ubuntu-latest
    timeout-minutes: 30
    outputs:
      image: {subject}
      digest: ${{{{ steps.build.outputs.digest }}}}
      version: ${{{{ steps.meta.outputs.version }}}}
    steps:
      - uses: actions/checkout@v4
      - uses: docker/setup-buildx-action@v3
{logins}      - id: meta
        uses: docker/metadata-action@v5
        with:
          images: |
{images}          tags: |
            type=sha,format=short,prefix=
            type=semver,pattern={{{{version}}}}
            type=raw,value=latest,enable={{{{is_default_branch}}}}
      - id: build
        uses: docker/build-push-action@v6
        with:
          context: {context}
          file: {dockerfile}
          push: true
          tags: ${{{{ steps.meta.outputs.tags }}}}
          labels: ${{{{ steps.meta.outputs.labels }}}}
          cache-from: type=gha{scope}
          cache-to: type=gha,mode=max{scope}
          provenance: mode=max
          sbom: true
      - name: Attest build provenance
        if: ${{{{ !github.event.repository.private }}}}
        uses: actions/attest-build-provenance@v2
        with:
          subject-name: {subject}
          subject-digest: ${{{{ steps.build.outputs.digest }}}}
          push-to-registry: true
      - name: Report the image
        env:
          DIGEST: ${{{{ steps.build.outputs.digest }}}}
          VERSION: ${{{{ steps.meta.outputs.version }}}}
        run: |
          echo \"Pushed \\`{subject}:$VERSION\\` as \\`$DIGEST\\`\" >> \"$GITHUB_STEP_SUMMARY\"
",
        job = image.job,
        name = image.name,
        context = yaml_string(&image.context),
        dockerfile = yaml_string(&image.dockerfile),
    )
}

fn workflow(input: &DeliveryInput, out: &mut Output) -> String {
    let logins: String = (0..input.registries.len())
        .map(|index| login(index, input, out))
        .collect();
    let jobs: Vec<String> = input
        .images
        .iter()
        .map(|image| job(image, &logins, input))
        .collect();
    format!(
        "name: Build

on:
  push:
    branches: [{branch}]
    tags: [\"v*\"]
  workflow_dispatch:

concurrency:
  group: build-${{{{ github.ref }}}}
  cancel-in-progress: ${{{{ !startsWith(github.ref, 'refs/tags/') }}}}

permissions:
  contents: read
  packages: write
  id-token: write
  attestations: write

jobs:
{jobs}",
        branch = yaml_string(&input.base.default_branch),
        jobs = jobs.join("\n"),
    )
}

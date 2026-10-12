//! What the delivery generators read, resolved once: the registries images
//! are pushed to, the environments a release moves through, the images the
//! repository builds and the workload a deploy step updates. Everything
//! comes from the project's facts, its `.sdi.yml` and what the execution
//! service holds for the workspace, so the same inputs always yield the
//! same files.

use std::collections::{BTreeMap, BTreeSet};

use sdi_analyzer::DeliveryConfigDraft;
use sdi_proto::{
    execution::v1::{Environment, Registry},
    knowledge::v1::{NodeKind, ProjectContext},
};

use super::target::{self, TargetChoice};
use crate::generators::{GenInput, input as gen_input};

// ---------------------------------------------------------------------------
// Registries
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RegistryKind {
    Ghcr,
    DockerHub,
    Acr,
    Ecr,
    Gar,
    Generic,
}

impl RegistryKind {
    /// CONTRACT.md §22 kinds; anything unknown is treated as a plain
    /// username/password registry.
    pub fn parse(kind: &str) -> Self {
        match kind.trim().to_ascii_lowercase().as_str() {
            "ghcr" => Self::Ghcr,
            "dockerhub" => Self::DockerHub,
            "acr" => Self::Acr,
            "ecr" => Self::Ecr,
            "gar" => Self::Gar,
            _ => Self::Generic,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Ghcr => "GitHub Container Registry",
            Self::DockerHub => "Docker Hub",
            Self::Acr => "Azure Container Registry",
            Self::Ecr => "Amazon ECR",
            Self::Gar => "Google Artifact Registry",
            Self::Generic => "the container registry",
        }
    }

    fn secret_prefix(self) -> &'static str {
        match self {
            Self::Ghcr => "GHCR",
            Self::DockerHub => "DOCKERHUB",
            Self::Acr => "ACR",
            Self::Ecr => "ECR",
            Self::Gar => "GAR",
            Self::Generic => "REGISTRY",
        }
    }
}

/// How the build workflow logs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryAuth {
    /// GHCR with the workflow's own `GITHUB_TOKEN`.
    GithubToken,
    /// A cloud registry through the cloud's OIDC federation.
    Oidc,
    /// Username and password from repository secrets.
    Secret,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistrySpec {
    pub kind: RegistryKind,
    /// Host without scheme: `ghcr.io`, `acme.azurecr.io`.
    pub host: String,
    /// Path images are pushed under; never has leading or trailing slashes.
    pub namespace: String,
    pub auth: RegistryAuth,
    /// GitHub Actions secret names (`Secret` auth only).
    pub username_secret: String,
    pub password_secret: String,
}

impl RegistrySpec {
    /// `ghcr.io/acme` + `api` → `ghcr.io/acme/api`.
    pub fn image(&self, name: &str) -> String {
        if self.namespace.is_empty() {
            format!("{}/{name}", self.host)
        } else {
            format!("{}/{}/{name}", self.host, self.namespace)
        }
    }

    /// `acme` of `acme.azurecr.io`: what `az acr login --name` takes.
    pub fn acr_name(&self) -> &str {
        self.host.split('.').next().unwrap_or(&self.host)
    }

    /// Region of a private ECR host (`<account>.dkr.ecr.<region>.amazonaws.com`).
    pub fn ecr_region(&self) -> Option<&str> {
        let mut parts = self.host.split('.');
        match (
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
        ) {
            (Some(_), Some("dkr"), Some("ecr"), Some(region), Some("amazonaws")) => Some(region),
            _ => None,
        }
    }

    pub fn is_public_ecr(&self) -> bool {
        self.host == "public.ecr.aws"
    }
}

/// A GitHub Actions secret name for a workspace secret name: upper case,
/// letters, digits and underscores only, never starting with a digit or
/// with the reserved `GITHUB_` prefix.
pub fn actions_secret_name(name: &str) -> String {
    let mut out: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect();
    if out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    if out.starts_with("GITHUB_") {
        out.insert_str(0, "REGISTRY_");
    }
    out
}

fn host_of(kind: RegistryKind, host: &str) -> String {
    let host = host
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_ascii_lowercase();
    if !host.is_empty() {
        return host;
    }
    match kind {
        RegistryKind::Ghcr => "ghcr.io".to_owned(),
        RegistryKind::DockerHub => "docker.io".to_owned(),
        _ => host,
    }
}

/// A registry as the execution service or `.sdi.yml` describes it.
#[derive(Default)]
struct RawRegistry<'a> {
    kind: &'a str,
    host: &'a str,
    namespace: &'a str,
    auth: &'a str,
    username_secret: &'a str,
    password_secret: &'a str,
}

/// One registry as the build workflow needs it. `notes` collects what a
/// person has to know about a default that was filled in.
fn registry_spec(
    raw: &RawRegistry<'_>,
    gen_input: &GenInput,
    notes: &mut Vec<String>,
) -> RegistrySpec {
    let RawRegistry {
        auth,
        username_secret,
        password_secret,
        ..
    } = *raw;
    let kind = RegistryKind::parse(raw.kind);
    let host = host_of(kind, raw.host);
    let owner = gen_input.owner.to_lowercase();
    let mut namespace = raw.namespace.trim().trim_matches('/').to_owned();
    match kind {
        RegistryKind::Ghcr => {
            // GHCR paths are lower case, and `GITHUB_TOKEN` may only push
            // under the repository's own owner.
            namespace = namespace.to_lowercase();
            if namespace.is_empty() {
                namespace = owner.clone();
            } else if !owner.is_empty() && namespace != owner {
                notes.push(format!(
                    "The registry namespace is `{namespace}` but the repository belongs to `{owner}`: `GITHUB_TOKEN` can only push under `ghcr.io/{owner}`, so either change the registry's namespace or log in with a personal access token that may write packages of `{namespace}`."
                ));
            }
        }
        RegistryKind::DockerHub => {
            if namespace.is_empty() {
                namespace = owner.clone();
            }
        }
        RegistryKind::Gar => {
            if namespace.is_empty() {
                namespace = format!("{owner}/{}", gen_input.image);
                notes.push(format!(
                    "The Artifact Registry has no namespace, so images are pushed under `{host}/{namespace}`; set the registry's namespace to `<project id>/<repository>` to match your Google Cloud project."
                ));
            }
        }
        RegistryKind::Acr | RegistryKind::Ecr | RegistryKind::Generic => {}
    }
    let named_secrets = !username_secret.trim().is_empty() || !password_secret.trim().is_empty();
    let auth = match (kind, auth.trim().to_ascii_lowercase().as_str()) {
        (_, "secret") => RegistryAuth::Secret,
        (RegistryKind::Ghcr, _) if named_secrets => RegistryAuth::Secret,
        (RegistryKind::Ghcr, _) => RegistryAuth::GithubToken,
        (RegistryKind::Acr | RegistryKind::Ecr | RegistryKind::Gar, _) => RegistryAuth::Oidc,
        (RegistryKind::DockerHub | RegistryKind::Generic, _) => RegistryAuth::Secret,
    };
    let secret = |given: &str, suffix: &str| {
        if given.trim().is_empty() {
            format!("{}_{suffix}", kind.secret_prefix())
        } else {
            actions_secret_name(given)
        }
    };
    RegistrySpec {
        kind,
        host,
        namespace,
        auth,
        username_secret: secret(username_secret, "USERNAME"),
        password_secret: secret(
            password_secret,
            if kind == RegistryKind::DockerHub {
                "TOKEN"
            } else {
                "PASSWORD"
            },
        ),
    }
}

// ---------------------------------------------------------------------------
// Environments
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DeployerKind {
    GithubActions,
    GitopsFlux,
    GitopsArgocd,
    Jenkins,
    AzureDevops,
    Webhook,
    DockerLocal,
}

impl DeployerKind {
    pub fn parse(kind: &str) -> Option<Self> {
        Some(match kind.trim().to_ascii_lowercase().as_str() {
            "github_actions" => Self::GithubActions,
            "gitops_flux" => Self::GitopsFlux,
            "gitops_argocd" => Self::GitopsArgocd,
            "jenkins" => Self::Jenkins,
            "azure_devops" => Self::AzureDevops,
            "webhook" => Self::Webhook,
            "docker_local" => Self::DockerLocal,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentSpec {
    pub name: String,
    pub kind: DeployerKind,
    pub url: String,
    pub health_path: String,
    pub protected: bool,
    pub auto_promote: bool,
    pub config: BTreeMap<String, String>,
    /// Index into [`DeliveryInput::registries`].
    pub registry: usize,
}

impl EnvironmentSpec {
    pub fn setting(&self, key: &str) -> &str {
        self.config.get(key).map(String::as_str).unwrap_or("")
    }

    /// `url + health_path`, or empty when the environment has no URL.
    pub fn health_url(&self) -> String {
        if self.url.is_empty() {
            return String::new();
        }
        format!("{}{}", self.url.trim_end_matches('/'), self.health_path)
    }

    /// The environment name as a Kubernetes name.
    pub fn k8s_name(&self) -> String {
        k8s_name(&self.name)
    }
}

/// A DNS-1123 label: lower case letters, digits and `-`, at most 63.
pub fn k8s_name(raw: &str) -> String {
    let mut out = String::new();
    for c in raw.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out: String = out.trim_matches('-').chars().take(63).collect();
    let out = out.trim_matches('-').to_owned();
    if out.is_empty() {
        "app".to_owned()
    } else {
        out
    }
}

// ---------------------------------------------------------------------------
// Images and the workload
// ---------------------------------------------------------------------------

/// One image the build workflow produces: one job each.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageBuild {
    /// GitHub Actions job id: `image`, or `image-<name>` when there are several.
    pub job: String,
    /// Image name without registry: `api`, `api-worker`.
    pub name: String,
    /// What it is built from, for people: `the repository`, `backend`.
    pub source: String,
    /// Build context relative to the repository root (`.`, `./backend`).
    pub context: String,
    pub dockerfile: String,
    /// The image a release is cut from (`<registry>/<namespace>/<repo name>`).
    pub primary: bool,
    pub dockerfile_exists: bool,
}

/// What a deploy step updates, read from the repository's own manifests.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Workload {
    /// Kubernetes Deployment (and container) name.
    pub deployment: String,
    /// Directory of the Helm chart, when the repository has one.
    pub chart_dir: String,
    /// Directory holding the Deployment manifest, when there is one.
    pub manifests_dir: String,
    /// File names in `manifests_dir` that hold Kubernetes objects, sorted.
    pub manifest_files: Vec<String>,
    /// `manifests_dir` already has a `kustomization.yaml`.
    pub has_kustomization: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeliveryInput {
    pub base: GenInput,
    /// Registries the build pushes to; never empty, the first is the one
    /// releases are cut from.
    pub registries: Vec<RegistrySpec>,
    pub environments: Vec<EnvironmentSpec>,
    /// Never empty; exactly one is `primary`.
    pub images: Vec<ImageBuild>,
    pub target: TargetChoice,
    pub workload: Workload,
    /// `delivery.build_workflow` of `.sdi.yml` (a file name), or empty.
    pub build_workflow: String,
    /// Existing workflows that already push an image.
    pub pushing_workflows: Vec<String>,
    /// `https://github.com` or the GitHub Enterprise host of the repository.
    pub git_host: String,
    /// What a person has to know about the choices made while resolving.
    pub notes: Vec<String>,
}

impl DeliveryInput {
    pub fn primary_registry(&self) -> &RegistrySpec {
        &self.registries[0]
    }

    pub fn primary_image(&self) -> &ImageBuild {
        self.images
            .iter()
            .find(|i| i.primary)
            .unwrap_or(&self.images[0])
    }

    /// The image reference a release names: registry, namespace and image,
    /// without a tag.
    pub fn release_image(&self) -> String {
        self.primary_registry().image(&self.primary_image().name)
    }

    /// The image an environment pulls: the release image in its registry.
    pub fn image_for(&self, env: &EnvironmentSpec) -> String {
        self.registries
            .get(env.registry)
            .unwrap_or(self.primary_registry())
            .image(&self.primary_image().name)
    }

    pub fn environments_of(&self, kind: DeployerKind) -> Vec<&EnvironmentSpec> {
        self.environments
            .iter()
            .filter(|e| e.kind == kind)
            .collect()
    }

    /// `https://github.com/acme/api.git` for `acme/api`.
    pub fn git_url(&self, repo: &str) -> String {
        format!("{}/{repo}.git", self.git_host)
    }
}

const TOOLING_DIRS: &[&str] = &[
    "docker",
    ".docker",
    "build",
    "deploy",
    "ci",
    "ops",
    "docker/app",
];
const NON_APP_DIRS: &[&str] = &[
    ".devcontainer",
    "test",
    "tests",
    "e2e",
    "examples",
    "example",
    "docs",
    "benchmarks",
    "fixtures",
];
/// More image jobs than this in one workflow is a fleet, not a project.
const MAX_IMAGES: usize = 12;

fn dirname(path: &str) -> &str {
    path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("")
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// A Dockerfile that builds something deployable: `Dockerfile` itself (or
/// `Dockerfile.prod`), outside test and example trees.
fn is_app_dockerfile(path: &str) -> bool {
    let name = basename(path);
    let variant_ok = match name.strip_prefix("Dockerfile.") {
        Some(variant) => matches!(variant, "prod" | "production" | "release"),
        None => name == "Dockerfile",
    };
    variant_ok
        && !dirname(path)
            .split('/')
            .any(|segment| NON_APP_DIRS.contains(&segment))
}

fn images_of(ctx: &ProjectContext, base: &GenInput, notes: &mut Vec<String>) -> Vec<ImageBuild> {
    let mut dockerfiles: Vec<String> = ctx
        .facts
        .iter()
        .filter_map(|f| f.key.strip_prefix("container:dockerfile:"))
        .map(|p| p.trim_start_matches("./").to_owned())
        .filter(|p| is_app_dockerfile(p))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if dockerfiles.len() > MAX_IMAGES {
        notes.push(format!(
            "The repository has {} Dockerfiles; the build workflow covers the first {MAX_IMAGES}.",
            dockerfiles.len()
        ));
        dockerfiles.truncate(MAX_IMAGES);
    }
    if dockerfiles.is_empty() {
        return vec![ImageBuild {
            job: "image".to_owned(),
            name: base.image.clone(),
            source: if base.app_dir.is_empty() {
                "the repository".to_owned()
            } else {
                base.app_dir.clone()
            },
            context: base.context_dir(),
            dockerfile: base.in_app("Dockerfile"),
            primary: true,
            // No keyed fact, but another fact may still point at the file.
            dockerfile_exists: base.exists(&base.in_app("Dockerfile")),
        }];
    }
    // The image a release is cut from is the application's own: the
    // Dockerfile of the server package, else the one at the root, else the first.
    let primary = dockerfiles
        .iter()
        .position(|p| dirname(p) == base.app_dir)
        .or_else(|| dockerfiles.iter().position(|p| dirname(p).is_empty()))
        .unwrap_or(0);
    let single = dockerfiles.len() == 1;
    let mut names: BTreeSet<String> = BTreeSet::new();
    dockerfiles
        .iter()
        .enumerate()
        .map(|(index, path)| {
            let dir = dirname(path);
            // `docker/Dockerfile` builds the repository, not the `docker` directory.
            let tooling = TOOLING_DIRS.contains(&dir);
            let package = base.packages.iter().find(|p| p.dir == dir);
            let label = match package {
                Some(package) if !package.name.is_empty() => package.name.clone(),
                _ => basename(dir).to_owned(),
            };
            let is_primary = index == primary;
            let mut name = if is_primary {
                base.image.clone()
            } else {
                format!("{}-{}", base.image, k8s_name(&label))
            };
            while !names.insert(name.clone()) {
                name.push_str("-image");
            }
            ImageBuild {
                job: if single {
                    "image".to_owned()
                } else {
                    format!(
                        "image-{}",
                        name.trim_start_matches(&format!("{}-", base.image))
                    )
                },
                name,
                source: if dir.is_empty() || tooling {
                    "the repository".to_owned()
                } else {
                    dir.to_owned()
                },
                context: if dir.is_empty() || tooling {
                    ".".to_owned()
                } else {
                    format!("./{dir}")
                },
                dockerfile: path.clone(),
                primary: is_primary,
                dockerfile_exists: true,
            }
        })
        .collect()
}

fn workload_of(ctx: &ProjectContext, base: &GenInput) -> Workload {
    let deployment = ctx.graph.as_ref().and_then(|g| {
        let mut nodes: Vec<_> = g
            .nodes
            .iter()
            .filter(|n| n.kind() == NodeKind::Infrastructure)
            .filter_map(|n| {
                n.key
                    .strip_prefix("infrastructure:k8s/Deployment.")
                    .map(|name| (name.to_owned(), dirname(&n.source_path).to_owned()))
            })
            .collect();
        nodes.sort();
        // The Deployment named after the image is the application's; one of
        // several others would be a guess, so it only counts when alone.
        nodes
            .iter()
            .find(|(name, _)| name == &base.image)
            .or(if nodes.len() == 1 {
                nodes.first()
            } else {
                None
            })
            .cloned()
    });
    let chart_dir = ctx
        .facts
        .iter()
        .find(|f| f.key == "k8s:helm" && f.source_path.ends_with("Chart.yaml"))
        .map(|f| dirname(&f.source_path).to_owned())
        .unwrap_or_default();
    let (deployment, manifests_dir) = match deployment {
        Some((name, dir)) => (name, dir),
        None => (base.image.clone(), String::new()),
    };
    // Every file of that directory the scan found Kubernetes objects in.
    let files: BTreeSet<String> = ctx
        .graph
        .iter()
        .flat_map(|g| g.nodes.iter())
        .filter(|n| n.key.starts_with("infrastructure:k8s/"))
        .filter(|n| !manifests_dir.is_empty() && dirname(&n.source_path) == manifests_dir)
        .map(|n| basename(&n.source_path).to_owned())
        .collect();
    let has_kustomization = files
        .iter()
        .any(|f| matches!(f.as_str(), "kustomization.yaml" | "kustomization.yml"));
    Workload {
        deployment,
        chart_dir,
        manifests_dir,
        manifest_files: files
            .into_iter()
            .filter(|f| !f.starts_with("kustomization."))
            .collect(),
        has_kustomization,
    }
}

/// The registries the build pushes to: the ones the environments name,
/// then the workspace default, then what `.sdi.yml` declares, and GHCR
/// under the repository's owner when nothing else is known.
fn registries_of(
    registries: &[Registry],
    environments: &[Environment],
    declared: Option<&DeliveryConfigDraft>,
    base: &GenInput,
    notes: &mut Vec<String>,
) -> (Vec<RegistrySpec>, BTreeMap<String, usize>) {
    let mut specs: Vec<RegistrySpec> = Vec::new();
    let mut index_by_id: BTreeMap<String, usize> = BTreeMap::new();
    let mut push = |registry: &Registry, specs: &mut Vec<RegistrySpec>, notes: &mut Vec<String>| {
        let spec = registry_spec(
            &RawRegistry {
                kind: &registry.kind,
                host: &registry.host,
                namespace: &registry.namespace,
                auth: &registry.auth,
                username_secret: &registry.username_secret,
                password_secret: &registry.password_secret,
            },
            base,
            notes,
        );
        let index = match specs
            .iter()
            .position(|s| s.host == spec.host && s.namespace == spec.namespace)
        {
            Some(index) => index,
            None => {
                specs.push(spec);
                specs.len() - 1
            }
        };
        index_by_id.insert(registry.id.clone(), index);
    };
    let default = registries.iter().find(|r| r.is_default);
    // The default first when any environment relies on it, so it is the
    // registry releases are cut from.
    let uses_default = environments.is_empty()
        || environments
            .iter()
            .any(|e| e.registry_id.is_empty() || !registries.iter().any(|r| r.id == e.registry_id));
    if uses_default && let Some(default) = default {
        push(default, &mut specs, notes);
    }
    for env in environments {
        if let Some(registry) = registries.iter().find(|r| r.id == env.registry_id) {
            push(registry, &mut specs, notes);
        }
    }
    if specs.is_empty() {
        let declared = declared.map(|d| &d.registry).filter(|r| !r.kind.is_empty());
        let raw = match declared {
            Some(r) => RawRegistry {
                kind: &r.kind,
                host: &r.host,
                namespace: &r.namespace,
                ..Default::default()
            },
            None => RawRegistry {
                kind: "ghcr",
                ..Default::default()
            },
        };
        specs.push(registry_spec(&raw, base, notes));
    }
    (specs, index_by_id)
}

/// Resolves everything the delivery generators read.
pub fn build(
    ctx: &ProjectContext,
    registries: &[Registry],
    environments: &[Environment],
) -> DeliveryInput {
    let base = gen_input::build(ctx, &BTreeMap::new(), "");
    let mut notes: Vec<String> = Vec::new();
    let declared = gen_input::usable_config(ctx)
        .and_then(|c| DeliveryConfigDraft::from_json(&c.delivery_json));

    let (registry_specs, index_by_id) = registries_of(
        registries,
        environments,
        declared.as_ref(),
        &base,
        &mut notes,
    );

    let mut ordered: Vec<&Environment> = environments.iter().collect();
    ordered.sort_by(|a, b| a.position.cmp(&b.position).then(a.name.cmp(&b.name)));
    let environment_specs: Vec<EnvironmentSpec> = ordered
        .into_iter()
        .filter_map(|env| {
            let Some(kind) = DeployerKind::parse(&env.kind) else {
                notes.push(format!(
                    "Environment `{}` uses the deployer `{}`, which nothing is generated for.",
                    env.name, env.kind
                ));
                return None;
            };
            Some(EnvironmentSpec {
                name: env.name.trim().to_owned(),
                kind,
                url: env.url.trim().trim_end_matches('/').to_owned(),
                health_path: env.health_path.trim().to_owned(),
                protected: env.protected,
                auto_promote: env.auto_promote,
                config: env
                    .config
                    .iter()
                    .map(|(k, v)| (k.clone(), v.trim().to_owned()))
                    .filter(|(_, v)| !v.is_empty())
                    .collect(),
                registry: index_by_id.get(&env.registry_id).copied().unwrap_or(0),
            })
        })
        .collect();

    let images = images_of(ctx, &base, &mut notes);
    let workload = workload_of(ctx, &base);
    let target = target::detect(ctx, &base, &workload);
    let pushing_workflows: Vec<String> = ctx
        .facts
        .iter()
        .filter(|f| f.value == "push")
        .filter_map(|f| f.key.strip_prefix("ci:build_image:"))
        .map(str::to_owned)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let git_host = ctx
        .project
        .as_ref()
        .map(|p| p.repo_url.trim())
        .and_then(|url| {
            let rest = url.strip_prefix("https://")?;
            let host = rest.split('/').next().filter(|h| !h.is_empty())?;
            Some(format!("https://{host}"))
        })
        .unwrap_or_else(|| "https://github.com".to_owned());

    DeliveryInput {
        base,
        registries: registry_specs,
        environments: environment_specs,
        images,
        target,
        workload,
        build_workflow: declared.map(|d| d.build_workflow).unwrap_or_default(),
        pushing_workflows,
        git_host,
        notes,
    }
}

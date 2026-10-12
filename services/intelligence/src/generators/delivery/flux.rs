//! Flux manifests for `gitops_flux` environments: a `GitRepository` that
//! watches the GitOps repository and the object that reconciles the
//! environment from it. With the default `kustomize_image` strategy that is
//! a `Kustomization` with health checks over a kustomize directory whose
//! `images:` entry a promotion edits; with `helm_values` it is a
//! `HelmRelease` over the chart and the values file a promotion edits.
//!
//! `config.image_automation: true` adds the image reflector and automation
//! objects, so Flux itself follows new version tags; the setter marker on
//! `newTag` is what it rewrites.

use super::{
    Output,
    gitops::{self, Layout, Strategy},
    input::{DeliveryInput, DeployerKind, EnvironmentSpec, RegistryKind},
};
use crate::generators::compose::yaml_string;

const NAMESPACE: &str = "flux-system";

pub(super) fn generate(input: &DeliveryInput, out: &mut Output) {
    for env in input.environments_of(DeployerKind::GitopsFlux) {
        environment(input, env, out);
    }
}

fn environment(input: &DeliveryInput, env: &EnvironmentSpec, out: &mut Output) {
    let layout = gitops::layout(input, env);
    let automation =
        env.setting("image_automation") == "true" && layout.strategy == Strategy::KustomizeImage;
    let name = &layout.name;
    let source = git_repository(input, &layout, automation, out);
    let reconciler = match layout.strategy {
        Strategy::KustomizeImage => {
            let marker = if automation {
                format!(" # {{\"$imagepolicy\": \"{NAMESPACE}:{name}:tag\"}}")
            } else {
                String::new()
            };
            let deployment = gitops::kustomize_overlay(input, env, &layout, &marker, out);
            kustomization(
                &layout,
                &format!(
                    "  healthChecks:\n    - apiVersion: apps/v1\n      kind: Deployment\n      name: {deployment}\n      namespace: {name}\n"
                ),
            )
        }
        // The manifests are the repository's own: wait for whatever they hold.
        Strategy::YamlPath => kustomization(&layout, "  wait: true\n"),
        Strategy::HelmValues => {
            let chart = gitops::helm_values(input, env, &layout, out);
            helm_release(&layout, &chart)
        }
    };
    let objects = format!("clusters/{}/{name}.yaml", env.k8s_name());
    out.file(
        input,
        &layout.file(&objects),
        "yaml",
        format!(
            "Flux source and {} that reconcile the `{}` environment from `{}`.",
            if layout.strategy == Strategy::HelmValues {
                "HelmRelease"
            } else {
                "Kustomization"
            },
            env.name,
            layout.repo
        ),
        format!(
            "# Flux objects of the {} environment of {}.\n# Commit this file under the path Flux was bootstrapped with, or apply it once.\n{source}---\n{reconciler}",
            env.name, input.base.image
        ),
    );
    gitops::notes(env, &layout, "Flux", out);
    out.note(format!(
        "Flux must already run in the cluster (`flux bootstrap github`); `{}` then goes under the bootstrap path or is applied with `kubectl apply -f`.",
        layout.file(&objects)
    ));
    if automation {
        image_automation(input, env, &layout, out);
    } else if env.setting("image_automation") == "true" {
        out.note(format!(
            "`image_automation` of `{}` only applies to the `kustomize_image` strategy and was not generated.",
            env.name
        ));
    }
}

/// A private repository needs a credential; with image automation the same
/// credential must be able to push.
fn git_repository(
    input: &DeliveryInput,
    layout: &Layout,
    writes: bool,
    out: &mut Output,
) -> String {
    let name = &layout.name;
    let url = input.git_url(&layout.repo);
    let secret = if input.base.is_private || writes {
        out.note(format!(
            "Give Flux access to `{}`: `flux create secret git {name}-git --namespace {NAMESPACE} --url={url} --username=git --password=<token>` with a token that may {} the repository.",
            layout.repo,
            if writes { "read and write" } else { "read" }
        ));
        format!("  secretRef:\n    name: {name}-git\n")
    } else {
        String::new()
    };
    format!(
        "apiVersion: source.toolkit.fluxcd.io/v1
kind: GitRepository
metadata:
  name: {name}
  namespace: {NAMESPACE}
spec:
  interval: 1m
  url: {url}
  ref:
    branch: {branch}
{secret}",
        branch = yaml_string(&layout.branch),
    )
}

fn kustomization(layout: &Layout, health: &str) -> String {
    format!(
        "apiVersion: kustomize.toolkit.fluxcd.io/v1
kind: Kustomization
metadata:
  name: {name}
  namespace: {NAMESPACE}
spec:
  interval: 10m
  retryInterval: 2m
  timeout: 5m
  path: {path}
  prune: true
  sourceRef:
    kind: GitRepository
    name: {name}
{health}",
        name = layout.name,
        path = yaml_string(&layout.dir_path()),
    )
}

fn helm_release(layout: &Layout, chart: &str) -> String {
    format!(
        "apiVersion: helm.toolkit.fluxcd.io/v2
kind: HelmRelease
metadata:
  name: {name}
  namespace: {NAMESPACE}
spec:
  interval: 10m
  timeout: 5m
  releaseName: {name}
  targetNamespace: {name}
  install:
    createNamespace: true
    remediation:
      retries: 3
  upgrade:
    remediation:
      retries: 3
      remediateLastFailure: true
  chart:
    spec:
      chart: {chart}
      reconcileStrategy: Revision
      sourceRef:
        kind: GitRepository
        name: {name}
      valuesFiles:
        - {own_values}
        - {values}
",
        name = layout.name,
        chart = yaml_string(&format!("./{chart}")),
        own_values = yaml_string(&format!("./{chart}/values.yaml")),
        values = yaml_string(&format!("./{}", layout.path)),
    )
}

/// `ImageRepository`, `ImagePolicy` and `ImageUpdateAutomation`: Flux scans
/// the registry for version tags and writes the newest into the
/// kustomization, on its own branch unless the environment commits directly.
fn image_automation(
    input: &DeliveryInput,
    env: &EnvironmentSpec,
    layout: &Layout,
    out: &mut Output,
) {
    let name = &layout.name;
    let image = input.image_for(env);
    let registry = input
        .registries
        .get(env.registry)
        .unwrap_or(input.primary_registry());
    // Cloud registries authenticate through the controller's workload
    // identity; anything else that is private needs a pull secret.
    let access = match registry.kind {
        RegistryKind::Acr => "  provider: azure\n".to_owned(),
        RegistryKind::Ecr => "  provider: aws\n".to_owned(),
        RegistryKind::Gar => "  provider: gcp\n".to_owned(),
        _ if input.base.is_private => {
            out.note(format!(
                "Flux scans `{image}` for new tags: `flux create secret docker-registry {name}-registry --namespace {NAMESPACE} --docker-server={} --docker-username=<user> --docker-password=<token>`.",
                registry.host
            ));
            format!("  secretRef:\n    name: {name}-registry\n")
        }
        _ => String::new(),
    };
    let push_branch = if env.setting("mode") == "commit" {
        layout.branch.clone()
    } else {
        format!("flux/{name}")
    };
    let path = format!("clusters/{}/{name}-images.yaml", env.k8s_name());
    out.file(
        input,
        &layout.file(&path),
        "yaml",
        format!(
            "Flux image automation for `{}`: follows the newest version tag of the image and writes it into `{}`.",
            env.name, layout.path
        ),
        format!(
            "# Flux image automation of the {environment} environment of {app}.
# Needs the image-reflector and image-automation controllers
# (`flux bootstrap --components-extra=image-reflector-controller,image-automation-controller`).
apiVersion: image.toolkit.fluxcd.io/v1beta2
kind: ImageRepository
metadata:
  name: {name}
  namespace: {NAMESPACE}
spec:
  image: {image}
  interval: 5m
{access}---
apiVersion: image.toolkit.fluxcd.io/v1beta2
kind: ImagePolicy
metadata:
  name: {name}
  namespace: {NAMESPACE}
spec:
  imageRepositoryRef:
    name: {name}
  policy:
    semver:
      range: \">=0.0.0\"
---
apiVersion: image.toolkit.fluxcd.io/v1beta2
kind: ImageUpdateAutomation
metadata:
  name: {name}
  namespace: {NAMESPACE}
spec:
  interval: 5m
  sourceRef:
    kind: GitRepository
    name: {name}
  git:
    checkout:
      ref:
        branch: {branch}
    commit:
      author:
        name: fluxcdbot
        email: fluxcdbot@users.noreply.github.com
      messageTemplate: {message}
    push:
      branch: {push_branch}
  update:
    path: {dir}
    strategy: Setters
",
            environment = env.name,
            app = input.base.image,
            branch = yaml_string(&layout.branch),
            message = yaml_string(&format!(
                "chore({}): update the {} image",
                env.name, input.base.image
            )),
            push_branch = yaml_string(&push_branch),
            dir = yaml_string(&layout.dir_path()),
        ),
    );
    out.note(format!(
        "Image automation for `{}` follows version tags (`v1.2.3` pushes `1.2.3`), not commit SHAs, and writes to the branch `{push_branch}`{}. It uses the `image.toolkit.fluxcd.io/v1beta2` API; `flux migrate` moves it to `v1` on Flux 2.7 or newer.",
        env.name,
        if push_branch == layout.branch {
            String::new()
        } else {
            format!(", from which a pull request into `{}` is opened", layout.branch)
        }
    ));
}

//! What Flux and Argo CD have in common: where an environment lives in the
//! GitOps repository and the files a controller reconciles there.
//!
//! The execution service promotes a release by editing one file of that
//! repository (`config.path`) and opening a pull request (CONTRACT.md §22):
//! the `images:` entry of a kustomization, a Helm values file, or a YAML
//! path of a manifest. The files written here are the ones that edit lands
//! in, so the layout follows the environment's own `repo`, `branch`, `path`
//! and `strategy`.

use super::{
    Output,
    input::{DeliveryInput, EnvironmentSpec, k8s_name},
};
use crate::generators::{compose::yaml_string, kubernetes};

/// How a promotion is written into the GitOps repository.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    /// `newTag` of the `images:` entry in a `kustomization.yaml`.
    KustomizeImage,
    /// `image.tag` of a Helm values file.
    HelmValues,
    /// One YAML path of a manifest the repository already has.
    YamlPath,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// `owner/name` of the GitOps repository.
    pub repo: String,
    /// It is not the project's own repository.
    pub external: bool,
    pub branch: String,
    pub strategy: Strategy,
    /// The file a promotion edits, relative to the GitOps repository.
    pub path: String,
    /// The directory the controller reconciles, relative to the GitOps
    /// repository (`.` never: a root file has the empty string).
    pub dir: String,
    /// Name of the controller objects, and the namespace the workload runs
    /// in: `<image>-<environment>`.
    pub name: String,
    /// Prefix of every generated path: empty when the GitOps repository is
    /// the project's, `gitops/<repository>/` when the files have to be
    /// carried over to another repository.
    pub root: String,
}

impl Layout {
    /// A path of the GitOps repository as a path of this change.
    pub fn file(&self, path: &str) -> String {
        format!("{}{}", self.root, path.trim_start_matches("./"))
    }

    /// A path inside the reconciled directory.
    pub fn in_dir(&self, name: &str) -> String {
        if self.dir.is_empty() {
            name.to_owned()
        } else {
            format!("{}/{name}", self.dir)
        }
    }

    /// The reconciled directory as controllers write it: `./apps/api`.
    pub fn dir_path(&self) -> String {
        if self.dir.is_empty() {
            ".".to_owned()
        } else {
            format!("./{}", self.dir)
        }
    }
}

fn dirname(path: &str) -> &str {
    path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("")
}

/// `to` as seen from the directory `from`: `deploy/production` → `deploy/k8s`
/// is `../k8s`.
pub fn relative(from: &str, to: &str) -> String {
    let split = |p: &str| -> Vec<String> {
        p.split('/')
            .filter(|s| !s.is_empty() && *s != ".")
            .map(str::to_owned)
            .collect()
    };
    let (from, to) = (split(from), split(to));
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = vec!["..".to_owned(); from.len() - common];
    parts.extend(to[common..].iter().cloned());
    if parts.is_empty() {
        ".".to_owned()
    } else {
        parts.join("/")
    }
}

pub fn layout(input: &DeliveryInput, env: &EnvironmentSpec) -> Layout {
    let own = &input.base.repo_full_name;
    let repo = match env.setting("repo") {
        "" => own.clone(),
        repo => repo.to_owned(),
    };
    let external = !repo.eq_ignore_ascii_case(own);
    let strategy = match env.setting("strategy") {
        "helm_values" => Strategy::HelmValues,
        "yaml_path" => Strategy::YamlPath,
        _ => Strategy::KustomizeImage,
    };
    let env_dir = format!("deploy/{}", env.k8s_name());
    let path = match (env.setting("path").trim_start_matches("./"), strategy) {
        ("", Strategy::KustomizeImage) => format!("{env_dir}/kustomization.yaml"),
        ("", Strategy::HelmValues) => format!("{env_dir}/values.yaml"),
        ("", Strategy::YamlPath) => {
            if input.workload.manifests_dir.is_empty() {
                "deploy/k8s/deployment.yaml".to_owned()
            } else {
                format!("{}/deployment.yaml", input.workload.manifests_dir)
            }
        }
        (path, _) => path.trim_matches('/').to_owned(),
    };
    Layout {
        external,
        branch: match env.setting("branch") {
            "" if external => "main".to_owned(),
            "" => input.base.default_branch.clone(),
            branch => branch.to_owned(),
        },
        strategy,
        dir: dirname(&path).to_owned(),
        path,
        name: k8s_name(&format!("{}-{}", input.base.image, env.name)),
        root: if external {
            format!("gitops/{}/", repo.rsplit('/').next().unwrap_or(&repo))
        } else {
            String::new()
        },
        repo,
    }
}

/// Notes every GitOps environment shares: what a promotion edits and, for
/// another repository, where the files have to go.
pub fn notes(env: &EnvironmentSpec, layout: &Layout, tool: &str, out: &mut Output) {
    let edit = match layout.strategy {
        Strategy::KustomizeImage => "the `newTag` of its `images:` entry".to_owned(),
        Strategy::HelmValues => format!("its `{}`", helm_value_path(env)),
        Strategy::YamlPath => format!("the value at `{}`", env.setting("yaml_path")),
    };
    let how = if env.setting("mode") == "commit" {
        "commits it"
    } else {
        "opens a pull request"
    };
    out.note(format!(
        "A promotion to `{}` edits `{}` in `{}` on `{}` ({edit}) and {how}; {tool} rolls it out once the change is on that branch.",
        env.name, layout.path, layout.repo, layout.branch
    ));
    if layout.external {
        out.note(format!(
            "`{}` is deployed from `{}`, which this change cannot reach: copy the contents of `{}` to the root of that repository.",
            env.name,
            layout.repo,
            layout.root.trim_end_matches('/')
        ));
    }
}

/// The image name the workload manifests use, which `images:` matches on.
fn manifest_image(input: &DeliveryInput, env: &EnvironmentSpec, generated: bool) -> String {
    if generated {
        // What `kubernetes::generate` writes into the Deployment.
        input.base.image_ref()
    } else {
        input.image_for(env)
    }
}

/// The workload of an environment as a kustomize directory: a namespace,
/// the Deployment and Service (the repository's own when it has them, a
/// generated pair otherwise) and the `images:` entry a promotion edits.
/// `tag_marker` is appended to the `newTag` line (Flux's image policy
/// setter). Returns the Deployment's name for health checks.
pub fn kustomize_overlay(
    input: &DeliveryInput,
    env: &EnvironmentSpec,
    layout: &Layout,
    tag_marker: &str,
    out: &mut Output,
) -> String {
    let workload = &input.workload;
    // The project's own manifests can only be referenced where they live.
    let own_manifests = !layout.external && !workload.manifests_dir.is_empty();
    let in_place = own_manifests && workload.manifests_dir == layout.dir;
    let mut resources: Vec<String> = vec!["namespace.yaml".to_owned()];
    let deployment = if own_manifests {
        if in_place {
            resources.extend(workload.manifest_files.iter().cloned());
        } else {
            resources.push(relative(&layout.dir, &workload.manifests_dir));
            if !workload.has_kustomization {
                // Kustomize only loads another directory as a base.
                let list: String = workload
                    .manifest_files
                    .iter()
                    .map(|f| format!("  - {f}\n"))
                    .collect();
                out.file(
                    input,
                    &format!("{}/kustomization.yaml", workload.manifests_dir),
                    "yaml",
                    "Makes the existing manifests a kustomize base the environment overlays build on.",
                    format!(
                        "apiVersion: kustomize.config.k8s.io/v1beta1\nkind: Kustomization\nresources:\n{list}"
                    ),
                );
            }
        }
        out.note(format!(
            "The `images:` entry of `{}` matches containers that use `{}`; if the manifests in `{}` name the image differently, change `name` to that value.",
            layout.path,
            manifest_image(input, env, false),
            workload.manifests_dir
        ));
        workload.deployment.clone()
    } else {
        for file in kubernetes::generate(&input.base) {
            let name = match file.path.as_str() {
                "deploy/k8s/deployment.yaml" => "deployment.yaml",
                "deploy/k8s/service.yaml" => "service.yaml",
                _ => continue,
            };
            resources.push(name.to_owned());
            out.file(
                input,
                &layout.file(&layout.in_dir(name)),
                "yaml",
                format!(
                    "{} of the `{}` environment.",
                    file.purpose.trim_end_matches('.'),
                    env.name
                ),
                file.content,
            );
        }
        input.base.image.clone()
    };
    out.file(
        input,
        &layout.file(&layout.in_dir("namespace.yaml")),
        "yaml",
        format!("The namespace the `{}` environment runs in.", env.name),
        format!(
            "apiVersion: v1\nkind: Namespace\nmetadata:\n  name: {name}\n  labels:\n    app.kubernetes.io/part-of: {app}\n    environment: {environment}\n",
            name = layout.name,
            app = input.base.image,
            environment = yaml_string(&env.name),
        ),
    );
    let resources: String = resources.iter().map(|r| format!("  - {r}\n")).collect();
    out.file(
        input,
        &layout.file(&layout.path),
        "yaml",
        format!(
            "Kustomize entry point of the `{}` environment; a promotion rewrites the `newTag` of its `images:` entry.",
            env.name
        ),
        format!(
            "# The {environment} environment of {app}.
# `newTag` is rewritten on every promotion: the release that runs here is
# whatever this file says on `{branch}`.
apiVersion: kustomize.config.k8s.io/v1beta1
kind: Kustomization
namespace: {namespace}
resources:
{resources}images:
  - name: {name}
    newName: {new_name}
    newTag: latest{tag_marker}
",
            environment = env.name,
            app = input.base.image,
            branch = layout.branch,
            namespace = layout.name,
            name = manifest_image(input, env, !own_manifests),
            new_name = input.image_for(env),
        ),
    );
    deployment
}

/// A Helm values file a promotion edits, plus the chart it configures when
/// the repository has none. Returns the chart directory relative to the
/// GitOps repository.
pub fn helm_values(
    input: &DeliveryInput,
    env: &EnvironmentSpec,
    layout: &Layout,
    out: &mut Output,
) -> String {
    let own_chart = !layout.external && !input.workload.chart_dir.is_empty();
    let chart_dir = if own_chart {
        input.workload.chart_dir.clone()
    } else {
        let generated = format!("charts/{}", input.base.image);
        for file in kubernetes::generate(&input.base) {
            if !file.path.starts_with(&format!("{generated}/")) {
                continue;
            }
            // One chart serves every environment, so the host is not the
            // chart's to know: its ingress is off until an environment's
            // values turn it on with that environment's own host.
            let content = if file.path.ends_with("/values.yaml") {
                chart_defaults(&file.content)
            } else {
                file.content
            };
            out.file(
                input,
                &layout.file(&file.path),
                &file.language,
                file.purpose,
                content,
            );
        }
        generated
    };
    // Only the generated chart is known to read `ingress.*`.
    let ingress = match (own_chart, host_of(&env.url)) {
        (true, _) => String::new(),
        (false, Some(host)) => format!("ingress:\n  enabled: true\n  host: {host}\n"),
        (false, None) => String::new(),
    };
    out.file(
        input,
        &layout.file(&layout.path),
        "yaml",
        format!(
            "Helm values of the `{}` environment; a promotion rewrites `{}`.",
            env.name,
            helm_value_path(env)
        ),
        format!(
            "# Values of {app} in the {environment} environment, on top of the chart's own.
# `{path}` is rewritten on every promotion.
{values}{ingress}",
            app = input.base.image,
            environment = env.name,
            path = helm_value_path(env),
            values = helm_values_block(&input.image_for(env), helm_value_path(env)),
        ),
    );
    chart_dir
}

/// Host name of an environment's URL, when it has one.
fn host_of(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let host = rest.split(['/', ':']).next().unwrap_or_default();
    (!host.is_empty()).then(|| host.to_owned())
}

/// The generated chart's default values with the ingress switched off and
/// no host of its own.
fn chart_defaults(values: &str) -> String {
    let mut in_ingress = false;
    let mut out = String::new();
    for line in values.lines() {
        if !line.starts_with(' ') && !line.is_empty() {
            in_ingress = line.starts_with("ingress:");
        }
        let line = match line.trim_start() {
            l if in_ingress && l.starts_with("enabled:") => "  enabled: false".to_owned(),
            l if in_ingress && l.starts_with("host:") => "  host: \"\"".to_owned(),
            _ => line.to_owned(),
        };
        out.push_str(&line);
        out.push('\n');
    }
    out
}

/// The value a promotion rewrites in a Helm values file: `config.yaml_path`
/// when the environment sets one, `image.tag` otherwise (the execution
/// service applies the same default).
fn helm_value_path(env: &EnvironmentSpec) -> &str {
    match env.setting("yaml_path") {
        "" => "image.tag",
        path => path,
    }
}

/// The starting values: the image repository, and `latest` at the dotted
/// `path` so the first promotion has a scalar to rewrite.
fn helm_values_block(image: &str, path: &str) -> String {
    let segments: Vec<&str> = path.split('.').filter(|s| !s.is_empty()).collect();
    let mut out = format!("image:\n  repository: {image}\n");
    // A path under `image` continues that mapping; anything else starts its own.
    let (rest, base) = match segments.split_first() {
        Some((&"image", rest)) => (rest, 1),
        _ => (segments.as_slice(), 0),
    };
    for (depth, segment) in rest.iter().enumerate() {
        let indent = "  ".repeat(base + depth);
        if depth + 1 == rest.len() {
            out.push_str(&format!("{indent}{segment}: latest\n"));
        } else {
            out.push_str(&format!("{indent}{segment}:\n"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::relative;

    #[test]
    fn relative_paths_between_directories() {
        assert_eq!(relative("deploy/production", "deploy/k8s"), "../k8s");
        assert_eq!(relative("apps/api", "apps/api"), ".");
        assert_eq!(relative("", "deploy/k8s"), "deploy/k8s");
        assert_eq!(relative("deploy/production", ""), "../..");
        assert_eq!(relative("a/b/c", "a/x"), "../../x");
        assert_eq!(
            relative("charts/api", "deploy/production/values.yaml"),
            "../../deploy/production/values.yaml"
        );
    }
}

//! Argo CD for `gitops_argocd` environments: one `Application` per
//! environment that syncs the directory a promotion edits, plus that
//! directory itself (a kustomize overlay by default, a Helm values file
//! with `helm_values`). Sync is automated with pruning and self-healing:
//! the gate is the pull request the execution service opens, not a manual
//! sync afterwards.

use super::{
    Output,
    gitops::{self, Strategy},
    input::{DeliveryInput, DeployerKind, EnvironmentSpec},
};
use crate::generators::compose::yaml_string;

pub(super) fn generate(input: &DeliveryInput, out: &mut Output) {
    for env in input.environments_of(DeployerKind::GitopsArgocd) {
        environment(input, env, out);
    }
}

fn environment(input: &DeliveryInput, env: &EnvironmentSpec, out: &mut Output) {
    let layout = gitops::layout(input, env);
    let name = &layout.name;
    let url = input.git_url(&layout.repo);
    // Argo CD wants `.` for the repository root.
    let dir = if layout.dir.is_empty() {
        "."
    } else {
        layout.dir.as_str()
    };
    let source = match layout.strategy {
        Strategy::KustomizeImage => {
            gitops::kustomize_overlay(input, env, &layout, "", out);
            format!("    path: {}\n", yaml_string(dir))
        }
        Strategy::YamlPath => format!("    path: {}\n", yaml_string(dir)),
        Strategy::HelmValues => {
            let chart = gitops::helm_values(input, env, &layout, out);
            // Value files are resolved from the chart's directory.
            format!(
                "    path: {}\n    helm:\n      releaseName: {name}\n      valueFiles:\n        - {}\n",
                yaml_string(&chart),
                yaml_string(&gitops::relative(&chart, &layout.path)),
            )
        }
    };
    let path = format!("argocd/{name}.yaml");
    out.file(
        input,
        &layout.file(&path),
        "yaml",
        format!(
            "Argo CD Application that keeps the `{}` environment in sync with `{}` in `{}`.",
            env.name, dir, layout.repo
        ),
        format!(
            "# Argo CD application of the {environment} environment of {app}.
# Apply it to the cluster Argo CD runs in, or let an app-of-apps pick it up.
apiVersion: argoproj.io/v1alpha1
kind: Application
metadata:
  name: {name}
  namespace: argocd
  finalizers:
    - resources-finalizer.argocd.argoproj.io
spec:
  project: default
  source:
    repoURL: {url}
    targetRevision: {branch}
{source}  destination:
    server: https://kubernetes.default.svc
    namespace: {name}
  syncPolicy:
    automated:
      prune: true
      selfHeal: true
    syncOptions:
      - CreateNamespace=true
    retry:
      limit: 5
      backoff:
        duration: 10s
        factor: 2
        maxDuration: 3m
",
            environment = env.name,
            app = input.base.image,
            branch = yaml_string(&layout.branch),
        ),
    );
    gitops::notes(env, &layout, "Argo CD", out);
    out.note(format!(
        "Apply `{}` with `kubectl apply -n argocd -f` (or add it to your app-of-apps){}.",
        layout.file(&path),
        if input.base.is_private {
            format!(
                "; Argo CD needs read access to the repository first: `argocd repo add {url} --username git --password <token>`"
            )
        } else {
            String::new()
        }
    ));
}

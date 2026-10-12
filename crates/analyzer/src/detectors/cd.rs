//! Continuous delivery evidence (CONTRACT.md §22): the tools that move a
//! built image into an environment. Flux and Argo CD are recognised by the
//! API group of the objects they reconcile, never by a `kind` alone
//! (kustomize has its own `Kustomization`, Argo Workflows shares
//! `argoproj.io`). Jenkins and Azure Pipelines are recognised by their
//! pipeline file. Each is one detected fact naming the file that proves it;
//! the registry, image build and GitHub environment facts come from the
//! workflows themselves (`ci.rs`).

use std::collections::BTreeSet;

use crate::detectors::Ctx;
use crate::files::{is_azure_pipelines, is_jenkinsfile};
use crate::model::*;
use crate::util::{basename, display, has_dir, is_yaml, line_with, short_path};
use crate::yaml;

/// API group suffix of every Flux controller (`kustomize.toolkit.fluxcd.io`,
/// `source.toolkit.fluxcd.io`, `helm.toolkit.fluxcd.io`, `image.…`).
const FLUX_GROUP: &str = "toolkit.fluxcd.io";
const ARGO_GROUP: &str = "argoproj.io";
/// The Argo CD kinds; `Workflow`, `Rollout` and friends are other products.
const ARGO_CD_KINDS: &[&str] = &["Application", "ApplicationSet", "AppProject"];

pub(crate) fn detect(ctx: &mut Ctx<'_>) {
    gitops(ctx);
    jenkins(ctx);
    azure_pipelines(ctx);
}

/// What one GitOps controller reconciles in this repository.
#[derive(Default)]
struct Found {
    kinds: BTreeSet<String>,
    first_path: String,
    evidence: String,
    files: usize,
}

impl Found {
    fn record(&mut self, kind: &str, path: &str, content: &str, group: &str) {
        if self.first_path.is_empty() {
            self.first_path = path.to_string();
            self.evidence = line_with(content, group)
                .unwrap_or("apiVersion")
                .to_string();
        }
        self.kinds.insert(kind.to_string());
    }
}

fn gitops(ctx: &mut Ctx<'_>) {
    let mut flux = Found::default();
    let mut argo = Found::default();
    for (path, content) in ctx.files_where(|p| is_yaml(p) && !p.starts_with(".github/")) {
        if !content.contains(FLUX_GROUP) && !content.contains(ARGO_GROUP) {
            continue;
        }
        let (mut in_flux, mut in_argo) = (false, false);
        for doc in yaml::docs(content) {
            let (Some(api), Some(kind)) = (
                yaml::get_str(&doc, "apiVersion"),
                yaml::get_str(&doc, "kind"),
            ) else {
                continue;
            };
            let group = api.split('/').next().unwrap_or(api);
            if group.ends_with(FLUX_GROUP) {
                flux.record(kind, path, content, FLUX_GROUP);
                in_flux = true;
            } else if group == ARGO_GROUP && ARGO_CD_KINDS.contains(&kind) {
                argo.record(kind, path, content, ARGO_GROUP);
                in_argo = true;
            }
        }
        flux.files += usize::from(in_flux);
        argo.files += usize::from(in_argo);
    }
    // `flux bootstrap` writes `flux-system/gotk-*.yaml`; the components file
    // is too large to download, so the directory itself is the evidence.
    if flux.kinds.is_empty()
        && let Some(path) = ctx
            .tree_paths()
            .find(|p| has_dir(p, "flux-system") && basename(p).starts_with("gotk-"))
    {
        flux.first_path = path.to_string();
        flux.evidence = format!("flux bootstrap: {path}");
        flux.files = 1;
    }
    for (found, key) in [(flux, "cd:flux"), (argo, "cd:argocd")] {
        if found.first_path.is_empty() {
            continue;
        }
        let tool = key.trim_start_matches("cd:");
        let kinds: Vec<String> = found.kinds.into_iter().collect();
        let value = if kinds.is_empty() {
            short_path(&found.first_path, 2)
        } else {
            format!(
                "{} in {} file{}",
                kinds.join(", "),
                found.files,
                if found.files == 1 { "" } else { "s" }
            )
        };
        ctx.detected(
            FactCategory::CiCd,
            key,
            &display(tool),
            &value,
            &found.first_path,
            &found.evidence,
        );
        ctx.deploy_targets.insert("kubernetes".into());
    }
}

/// The shortest path wins: the root `Jenkinsfile` describes the project,
/// one deeper down describes a part of it.
fn first_in_tree(ctx: &Ctx<'_>, pred: impl Fn(&str) -> bool) -> Option<String> {
    ctx.tree_paths()
        .filter(|p| pred(p))
        .min_by_key(|p| (p.matches('/').count(), p.len()))
        .map(String::from)
}

fn jenkins(ctx: &mut Ctx<'_>) {
    let Some(path) = first_in_tree(ctx, is_jenkinsfile) else {
        return;
    };
    let content = ctx.file(&path).unwrap_or("");
    let evidence = line_with(content, "pipeline {")
        .or_else(|| line_with(content, "node"))
        .unwrap_or("Jenkinsfile in the repository");
    ctx.detected(
        FactCategory::CiCd,
        "cd:jenkins",
        &display("jenkins"),
        &path,
        &path,
        evidence,
    );
    ctx.chunk(ChunkKind::CiCd, &path, "Jenkins pipeline", content);
}

fn azure_pipelines(ctx: &mut Ctx<'_>) {
    let Some(path) = first_in_tree(ctx, is_azure_pipelines) else {
        return;
    };
    let content = ctx.file(&path).unwrap_or("");
    let evidence = ["stages:", "trigger:", "pool:", "steps:"]
        .into_iter()
        .find_map(|needle| line_with(content, needle))
        .unwrap_or("Azure Pipelines definition in the repository");
    ctx.detected(
        FactCategory::CiCd,
        "cd:azure_pipelines",
        &display("azure_pipelines"),
        &path,
        &path,
        evidence,
    );
    ctx.chunk(
        ChunkKind::CiCd,
        &path,
        "Azure Pipelines definition",
        content,
    );
}

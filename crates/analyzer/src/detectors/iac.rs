//! Infrastructure as code: Terraform, Bicep, Pulumi, CloudFormation,
//! Kubernetes manifests and Helm charts. IaC is the strongest evidence of
//! where a project deploys, so clouds found here are Detected, not Inferred.

use std::collections::BTreeSet;

use regex::Regex;

use crate::detectors::{Ctx, ImageRef, attrs};
use crate::files::is_k8s_dir;
use crate::model::*;
use crate::util::{basename, dirname, display, extension, is_yaml, line_with, short_path};
use crate::yaml;

pub(crate) fn detect(ctx: &mut Ctx<'_>) {
    terraform(ctx);
    bicep(ctx);
    pulumi(ctx);
    for (path, content) in ctx.files_where(|p| is_yaml(p) && !p.starts_with(".github/")) {
        let name = basename(path);
        if name == "Chart.yaml" {
            helm(ctx, path, content);
        } else if content.contains("AWSTemplateFormatVersion")
            || (content.contains("Resources:") && content.contains("Type: AWS::"))
        {
            cloudformation(ctx, path, content);
        } else if is_k8s_dir(path) || content.contains("apiVersion:") {
            kubernetes(ctx, path, content);
        }
    }
}

fn cloud_for_provider(provider: &str) -> Option<&'static str> {
    Some(match provider {
        "azurerm" | "azuread" | "azapi" | "azurestack" => "azure",
        "aws" => "aws",
        "google" | "google-beta" => "gcp",
        _ => return None,
    })
}

/// Terraform resource types that are managed datastores.
fn datastore_for_resource(resource: &str, block: &str) -> Option<&'static str> {
    let r = resource;
    // Generic RDS / Cloud SQL resources only say which engine inside the block.
    let generic = matches!(
        r,
        "aws_db_instance" | "aws_rds_cluster" | "google_sql_database_instance"
    );
    let engine = block.to_ascii_lowercase();
    Some(
        if r.contains("postgres") || (generic && engine.contains("postgres")) {
            "postgres"
        } else if r.contains("mysql") || (generic && engine.contains("mysql")) {
            "mysql"
        } else if r.contains("redis") || r.contains("elasticache") || r.contains("memorystore") {
            "redis"
        } else if r.contains("cosmosdb") && block.contains("MongoDB") {
            "mongodb"
        } else if r.contains("cosmosdb") {
            "cosmosdb"
        } else if r.contains("dynamodb") {
            "dynamodb"
        } else {
            return None;
        },
    )
}

fn terraform(ctx: &mut Ctx<'_>) {
    let files = ctx.files_where(|p| extension(p) == "tf");
    if files.is_empty() {
        return;
    }
    let provider_re = Regex::new(r#"provider\s+"([\w-]+)""#).expect("static regex");
    let required_re = Regex::new(r#"(?m)^\s*([\w-]+)\s*=\s*\{\s*\n?\s*source\s*=\s*"[^"]+""#)
        .expect("static regex");
    let resource_re = Regex::new(r#"resource\s+"([\w-]+)"\s+"([\w-]+)""#).expect("static regex");
    let module_re = Regex::new(r#"module\s+"([\w-]+)""#).expect("static regex");
    let backend_re = Regex::new(r#"backend\s+"([\w-]+)""#).expect("static regex");

    let first_path = files[0].0.to_string();
    ctx.detected(
        FactCategory::Iac,
        "iac:terraform",
        "Terraform",
        &format!("{} .tf files", files.len()),
        &first_path,
        line_with(files[0].1, "terraform").unwrap_or("*.tf present"),
    );

    let mut modules: BTreeSet<String> = BTreeSet::new();
    for (path, content) in files {
        let dir = dirname(path).to_string();
        let module_key = format!("iac_module:{}", if dir.is_empty() { "root" } else { &dir });
        if modules.insert(module_key.clone()) {
            ctx.node(
                NodeKind::IacModule,
                &module_key,
                &if dir.is_empty() {
                    "terraform".to_string()
                } else {
                    short_path(&dir, 2)
                },
                &format!(
                    "Terraform module in {}",
                    if dir.is_empty() { "." } else { &dir }
                ),
                path,
                attrs(&[("tool", "terraform")]),
            );
            let repo = ctx.repo_key.clone();
            ctx.edge(&repo, &module_key, "CONTAINS", RelationCategory::Structure);
        }
        for cap in provider_re
            .captures_iter(content)
            .chain(required_re.captures_iter(content))
        {
            let provider = cap[1].to_string();
            if let Some(cloud) = cloud_for_provider(&provider) {
                record_cloud(ctx, cloud, path, &cap[0], "terraform provider");
            } else if provider == "kubernetes" || provider == "helm" {
                ctx.deploy_targets.insert("kubernetes".into());
            }
            ctx.detected(
                FactCategory::Iac,
                &format!("iac:terraform:provider:{provider}"),
                "Terraform provider",
                &provider,
                path,
                &cap[0],
            );
        }
        for cap in backend_re.captures_iter(content) {
            ctx.detected(
                FactCategory::Iac,
                &format!("iac:terraform:backend:{}", &cap[1]),
                "Terraform remote state",
                &cap[1],
                path,
                &cap[0],
            );
        }
        for cap in module_re.captures_iter(content) {
            ctx.detected(
                FactCategory::Iac,
                &format!("iac:terraform:module:{}", &cap[1]),
                "Terraform module",
                &cap[1],
                path,
                &cap[0],
            );
        }
        for cap in resource_re.captures_iter(content) {
            let (rtype, rname) = (cap[1].to_string(), cap[2].to_string());
            let start = cap.get(0).map(|m| m.start()).unwrap_or(0);
            let block = &content[start..content.len().min(start + 1500)];
            let key = format!("infrastructure:{rtype}.{rname}");
            ctx.node(
                NodeKind::Infrastructure,
                &key,
                &format!("{rtype}.{rname}"),
                &format!("Terraform resource {rtype}"),
                path,
                attrs(&[("resource_type", &rtype), ("tool", "terraform")]),
            );
            ctx.edge(
                &module_key,
                &key,
                "PROVISIONS",
                RelationCategory::Infrastructure,
            );
            if let Some(db) = datastore_for_resource(&rtype, block) {
                ctx.detected(
                    FactCategory::Datastore,
                    &format!("infra:datastore:{db}"),
                    &format!("{} (managed)", display(db)),
                    &format!("provisioned by {rtype}"),
                    path,
                    &cap[0],
                );
                let ds_key = format!("datastore:{db}");
                ensure_datastore_node(ctx, db, path, "terraform");
                ctx.edge(&key, &ds_key, "PROVIDES", RelationCategory::Infrastructure);
            }
            if rtype.contains("kubernetes_cluster")
                || rtype == "aws_eks_cluster"
                || rtype == "google_container_cluster"
            {
                ctx.deploy_targets.insert("kubernetes".into());
            }
        }
        environments_from_path(ctx, path);
        ctx.chunk(
            ChunkKind::Iac,
            path,
            &format!("Terraform ({})", short_path(path, 2)),
            content,
        );
    }
    // tfvars files name environments even when not downloaded.
    for p in ctx.tree_paths().collect::<Vec<_>>() {
        if p.ends_with(".tfvars") {
            environments_from_path(ctx, p);
        }
    }
}

pub(crate) fn ensure_datastore_node(ctx: &mut Ctx<'_>, db: &str, path: &str, defined_by: &str) {
    let key = format!("datastore:{db}");
    if !ctx.has_node(&key) {
        ctx.node(
            NodeKind::Datastore,
            &key,
            &display(db),
            &format!("{} defined by {defined_by}", display(db)),
            path,
            attrs(&[("engine", db), ("defined_by", defined_by)]),
        );
    }
}

pub(crate) fn record_cloud(ctx: &mut Ctx<'_>, cloud: &str, path: &str, evidence: &str, via: &str) {
    ctx.deploy_targets.insert(cloud.to_string());
    let key = format!("cloud:{cloud}");
    if !ctx.has_fact(&key) {
        ctx.detected(
            FactCategory::Cloud,
            &key,
            &display(cloud),
            via,
            path,
            evidence,
        );
    }
}

/// `envs/prod/main.tf`, `staging.tfvars`, `overlays/production/` all name an environment.
pub(crate) fn environments_from_path(ctx: &mut Ctx<'_>, path: &str) {
    for seg in path.split('/') {
        let seg = seg
            .trim_end_matches(".tfvars")
            .trim_end_matches(".yaml")
            .trim_end_matches(".yml");
        let env = match seg.to_ascii_lowercase().as_str() {
            "prod" | "production" => "production",
            "staging" | "stage" | "stg" => "staging",
            "dev" | "development" => "development",
            "test" | "testing" | "qa" => "test",
            _ => continue,
        };
        add_environment(ctx, env, path, "path");
    }
}

pub(crate) fn add_environment(ctx: &mut Ctx<'_>, env: &str, path: &str, defined_by: &str) {
    let key = format!("environment:{env}");
    ctx.environments.insert(env.to_string());
    if ctx.has_node(&key) {
        return;
    }
    ctx.node(
        NodeKind::Environment,
        &key,
        env,
        &format!("{env} environment ({defined_by})"),
        path,
        attrs(&[("defined_by", defined_by)]),
    );
    ctx.detected(
        FactCategory::Deployment,
        &key,
        "Environment",
        env,
        path,
        &format!("{defined_by}: {path}"),
    );
}

fn bicep(ctx: &mut Ctx<'_>) {
    let files = ctx.files_where(|p| extension(p) == "bicep");
    if files.is_empty() {
        return;
    }
    let re = Regex::new(r#"resource\s+(\w+)\s+'([\w./]+)@"#).expect("static regex");
    ctx.detected(
        FactCategory::Iac,
        "iac:bicep",
        "Bicep",
        &format!("{} .bicep files", files.len()),
        files[0].0,
        line_with(files[0].1, "resource").unwrap_or("*.bicep present"),
    );
    for (path, content) in files {
        record_cloud(ctx, "azure", path, "Bicep template", "bicep");
        let module_key = format!("iac_module:{}", path);
        ctx.node(
            NodeKind::IacModule,
            &module_key,
            &short_path(path, 2),
            "Bicep template",
            path,
            attrs(&[("tool", "bicep")]),
        );
        let repo = ctx.repo_key.clone();
        ctx.edge(&repo, &module_key, "CONTAINS", RelationCategory::Structure);
        for cap in re.captures_iter(content) {
            let key = format!("infrastructure:{}.{}", &cap[2], &cap[1]);
            ctx.node(
                NodeKind::Infrastructure,
                &key,
                &format!("{} ({})", &cap[2], &cap[1]),
                "Azure resource",
                path,
                attrs(&[("resource_type", &cap[2]), ("tool", "bicep")]),
            );
            ctx.edge(
                &module_key,
                &key,
                "PROVISIONS",
                RelationCategory::Infrastructure,
            );
            let lower = cap[2].to_ascii_lowercase();
            let db = if lower.contains("postgresql") {
                Some("postgres")
            } else if lower.contains("mysql") {
                Some("mysql")
            } else if lower.contains("cache") {
                Some("redis")
            } else {
                None
            };
            if let Some(db) = db {
                ctx.detected(
                    FactCategory::Datastore,
                    &format!("infra:datastore:{db}"),
                    &format!("{} (managed)", display(db)),
                    &format!("provisioned by {}", &cap[2]),
                    path,
                    &cap[0],
                );
                ensure_datastore_node(ctx, db, path, "bicep");
                ctx.edge(
                    &key,
                    &format!("datastore:{db}"),
                    "PROVIDES",
                    RelationCategory::Infrastructure,
                );
            }
        }
        environments_from_path(ctx, path);
        ctx.chunk(
            ChunkKind::Iac,
            path,
            &format!("Bicep ({})", short_path(path, 2)),
            content,
        );
    }
}

fn pulumi(ctx: &mut Ctx<'_>) {
    let files = ctx.files_where(|p| basename(p).starts_with("Pulumi.") && is_yaml(p));
    if files.is_empty() {
        return;
    }
    ctx.detected(
        FactCategory::Iac,
        "iac:pulumi",
        "Pulumi",
        &format!("{} Pulumi files", files.len()),
        files[0].0,
        line_with(files[0].1, "runtime").unwrap_or("Pulumi.yaml present"),
    );
    for (path, content) in files {
        let key = format!("iac_module:{path}");
        ctx.node(
            NodeKind::IacModule,
            &key,
            &short_path(path, 2),
            "Pulumi program",
            path,
            attrs(&[("tool", "pulumi")]),
        );
        let repo = ctx.repo_key.clone();
        ctx.edge(&repo, &key, "CONTAINS", RelationCategory::Structure);
        for (needle, cloud) in [("azure", "azure"), ("aws:", "aws"), ("gcp:", "gcp")] {
            if let Some(line) = line_with(content, needle) {
                record_cloud(ctx, cloud, path, line, "pulumi config");
            }
        }
        // Pulumi.<stack>.yaml names the stack, which usually is the environment.
        if let Some(stack) = basename(path)
            .strip_prefix("Pulumi.")
            .and_then(|s| s.strip_suffix(".yaml").or_else(|| s.strip_suffix(".yml")))
        {
            environments_from_path(ctx, &format!("{stack}/"));
        }
        ctx.chunk(
            ChunkKind::Iac,
            path,
            &format!("Pulumi ({})", short_path(path, 2)),
            content,
        );
    }
}

fn cloudformation(ctx: &mut Ctx<'_>, path: &str, content: &str) {
    ctx.detected(
        FactCategory::Iac,
        "iac:cloudformation",
        "CloudFormation",
        &short_path(path, 2),
        path,
        line_with(content, "AWSTemplateFormatVersion").unwrap_or("Type: AWS::"),
    );
    record_cloud(
        ctx,
        "aws",
        path,
        "CloudFormation template",
        "cloudformation",
    );
    let key = format!("iac_module:{path}");
    ctx.node(
        NodeKind::IacModule,
        &key,
        &short_path(path, 2),
        "CloudFormation template",
        path,
        attrs(&[("tool", "cloudformation")]),
    );
    let repo = ctx.repo_key.clone();
    ctx.edge(&repo, &key, "CONTAINS", RelationCategory::Structure);
    if let Some(doc) = yaml::first(content) {
        for (name, res) in yaml::entries(yaml::get(&doc, "Resources")) {
            let rtype = yaml::get_str(res, "Type").unwrap_or("AWS::Resource");
            let rkey = format!("infrastructure:{rtype}.{name}");
            ctx.node(
                NodeKind::Infrastructure,
                &rkey,
                &format!("{rtype} ({name})"),
                "AWS resource",
                path,
                attrs(&[("resource_type", rtype), ("tool", "cloudformation")]),
            );
            ctx.edge(&key, &rkey, "PROVISIONS", RelationCategory::Infrastructure);
        }
    }
    ctx.chunk(
        ChunkKind::Iac,
        path,
        &format!("CloudFormation ({})", short_path(path, 2)),
        content,
    );
}

fn kubernetes(ctx: &mut Ctx<'_>, path: &str, content: &str) {
    let docs = yaml::docs(content);
    let objects: Vec<(String, String)> = docs
        .iter()
        .filter(|d| yaml::get(d, "apiVersion").is_some())
        .filter_map(|d| {
            let kind = yaml::get_str(d, "kind")?;
            let name = yaml::get(d, "metadata")
                .and_then(|m| yaml::get_str(m, "name"))
                .unwrap_or("unnamed");
            Some((kind.to_string(), name.to_string()))
        })
        .collect();
    if objects.is_empty() {
        return;
    }
    if !ctx.has_fact("k8s:manifests") {
        ctx.detected(
            FactCategory::Iac,
            "k8s:manifests",
            "Kubernetes manifests",
            &short_path(path, 2),
            path,
            line_with(content, "kind:").unwrap_or("kind:"),
        );
    }
    ctx.deploy_targets.insert("kubernetes".into());
    let module_key = format!("iac_module:{path}");
    ctx.node(
        NodeKind::IacModule,
        &module_key,
        &short_path(path, 2),
        "Kubernetes manifest",
        path,
        attrs(&[("tool", "kubernetes")]),
    );
    let repo = ctx.repo_key.clone();
    ctx.edge(&repo, &module_key, "CONTAINS", RelationCategory::Structure);
    for (kind, name) in &objects {
        let key = format!("infrastructure:k8s/{kind}.{name}");
        ctx.node(
            NodeKind::Infrastructure,
            &key,
            &format!("{kind} {name}"),
            &format!("Kubernetes {kind}"),
            path,
            attrs(&[
                ("resource_type", &format!("k8s/{kind}")),
                ("tool", "kubernetes"),
            ]),
        );
        ctx.edge(
            &module_key,
            &key,
            "PROVISIONS",
            RelationCategory::Infrastructure,
        );
    }
    for line in content.lines() {
        let t = line.trim();
        if let Some(image) = t.strip_prefix("image:") {
            let image = image.trim().trim_matches('"').trim_matches('\'');
            ctx.images.push(ImageRef {
                path: path.to_string(),
                image: image.to_string(),
                line: t.to_string(),
            });
        }
    }
    if let Some(ns) = docs
        .iter()
        .find_map(|d| yaml::get(d, "metadata").and_then(|m| yaml::get_str(m, "namespace")))
    {
        environments_from_path(ctx, &format!("{ns}/"));
    }
    environments_from_path(ctx, path);
    ctx.chunk(
        ChunkKind::Iac,
        path,
        &format!("Kubernetes ({})", short_path(path, 2)),
        content,
    );
}

fn helm(ctx: &mut Ctx<'_>, path: &str, content: &str) {
    let doc = yaml::first(content);
    let name = doc
        .as_ref()
        .and_then(|d| yaml::get_str(d, "name"))
        .unwrap_or("chart")
        .to_string();
    if !ctx.has_fact("k8s:helm") {
        ctx.detected(
            FactCategory::Iac,
            "k8s:helm",
            "Helm chart",
            &name,
            path,
            line_with(content, "name:").unwrap_or("Chart.yaml"),
        );
    }
    ctx.deploy_targets.insert("kubernetes".into());
    let key = format!("iac_module:helm/{name}");
    ctx.node(
        NodeKind::IacModule,
        &key,
        &format!("helm/{name}"),
        "Helm chart",
        path,
        attrs(&[("tool", "helm")]),
    );
    let repo = ctx.repo_key.clone();
    ctx.edge(&repo, &key, "CONTAINS", RelationCategory::Structure);
    if let Some(dep) = doc.as_ref().and_then(|d| yaml::get(d, "dependencies"))
        && let serde_yaml_ng::Value::Sequence(seq) = dep
    {
        for d in seq {
            let dep_name = yaml::get_str(d, "name").unwrap_or("");
            let db = match dep_name {
                "postgresql" | "postgresql-ha" => Some("postgres"),
                "redis" => Some("redis"),
                "mongodb" => Some("mongodb"),
                "mysql" | "mariadb" => Some("mysql"),
                _ => None,
            };
            if let Some(db) = db {
                ctx.detected(
                    FactCategory::Datastore,
                    &format!("infra:datastore:{db}"),
                    &format!("{} (Helm dependency)", display(db)),
                    dep_name,
                    path,
                    &format!("dependencies: {dep_name}"),
                );
                ensure_datastore_node(ctx, db, path, "helm");
                ctx.edge(
                    &key,
                    &format!("datastore:{db}"),
                    "PROVISIONS",
                    RelationCategory::Infrastructure,
                );
            }
        }
    }
    ctx.chunk(ChunkKind::Iac, path, &format!("Helm chart {name}"), content);
}

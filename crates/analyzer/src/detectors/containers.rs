//! Dockerfiles become container images; compose files become a local
//! environment plus the images (and therefore datastores) it runs.

use crate::detectors::{Ctx, ImageRef, attrs};
use crate::files::{is_compose, is_dockerfile};
use crate::model::*;
use crate::util::{basename, dirname, line_with, short_path};
use crate::yaml;

pub(crate) fn detect(ctx: &mut Ctx<'_>) {
    for (path, content) in ctx.files_where(|p| is_dockerfile(basename(p))) {
        dockerfile(ctx, path, content);
    }
    // Dockerfiles that were in the tree but not downloaded still prove containerisation.
    let undownloaded: Vec<String> = ctx
        .tree_paths()
        .filter(|p| is_dockerfile(basename(p)) && ctx.file(p).is_none())
        .map(String::from)
        .collect();
    for path in undownloaded {
        ctx.detected(
            FactCategory::Container,
            &format!("container:dockerfile:{path}"),
            "Dockerfile",
            &short_path(&path, 2),
            &path,
            "Dockerfile present in tree",
        );
    }
    for (path, content) in ctx.files_where(|p| is_compose(basename(p))) {
        compose(ctx, path, content);
    }
}

fn dockerfile(ctx: &mut Ctx<'_>, path: &str, content: &str) {
    let mut bases = Vec::new();
    let mut stages = 0usize;
    let mut ports = Vec::new();
    let mut has_user = false;
    for line in content.lines().map(str::trim) {
        let upper = line.to_ascii_uppercase();
        if let Some(rest) = upper.strip_prefix("FROM ") {
            let image = line[5..]
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_string();
            if rest.contains(" AS ") {
                stages += 1;
            }
            bases.push(image);
        } else if let Some(rest) = line.strip_prefix("EXPOSE ") {
            ports.extend(rest.split_whitespace().map(String::from));
        } else if upper.starts_with("USER ") {
            has_user = true;
        }
    }
    let base = bases.last().cloned().unwrap_or_default();
    let dir = dirname(path);
    let app = ctx.app_for_path(path).cloned();
    let image_name = app
        .as_ref()
        .map(|a| a.name.clone())
        .unwrap_or_else(|| ctx.repo_name().to_string());
    let image_key = format!("container_image:{}", image_name.to_ascii_lowercase());
    let multi_stage = stages > 0 || bases.len() > 1;

    ctx.detected(
        FactCategory::Container,
        &format!("container:dockerfile:{path}"),
        "Dockerfile",
        &if base.is_empty() {
            short_path(path, 2)
        } else {
            format!("FROM {base}")
        },
        path,
        line_with(content, "FROM").unwrap_or("Dockerfile"),
    );
    if multi_stage {
        ctx.detected(
            FactCategory::Container,
            &format!("container:multistage:{path}"),
            "Multi-stage build",
            &format!("{} stages", bases.len()),
            path,
            &bases.join(" → "),
        );
    }
    if !has_user {
        ctx.inferred(
            FactCategory::Security,
            &format!("security:dockerfile_root:{path}"),
            "Container runs as root",
            "no USER instruction",
            0.8,
            &format!("{path} has no USER instruction"),
        );
    }
    ctx.node(
        NodeKind::ContainerImage,
        &image_key,
        &image_name,
        &format!("Built from {path}"),
        path,
        attrs(&[
            ("base_image", &base),
            ("stages", &bases.len().to_string()),
            ("ports", &ports.join(",")),
            ("directory", if dir.is_empty() { "." } else { dir }),
        ]),
    );
    if let Some(app) = app {
        ctx.edge(
            &app.key,
            &image_key,
            "PACKAGED_AS",
            RelationCategory::Structure,
        );
    } else {
        let repo = ctx.repo_key.clone();
        ctx.edge(&repo, &image_key, "CONTAINS", RelationCategory::Structure);
    }
}

fn compose(ctx: &mut Ctx<'_>, path: &str, content: &str) {
    let Some(doc) = yaml::first(content) else {
        return;
    };
    let services = yaml::entries(yaml::get(&doc, "services"));
    if services.is_empty() {
        return;
    }
    let names: Vec<String> = services.iter().map(|(n, _)| n.clone()).collect();
    ctx.detected(
        FactCategory::Container,
        &format!("container:compose:{path}"),
        "Docker Compose",
        &format!("{} services: {}", names.len(), names.join(", ")),
        path,
        line_with(content, "services:").unwrap_or("services:"),
    );
    let env_key = "environment:local".to_string();
    if !ctx.has_node(&env_key) {
        ctx.node(
            NodeKind::Environment,
            &env_key,
            "local",
            "Docker Compose development environment",
            path,
            attrs(&[("defined_by", "docker_compose")]),
        );
    }
    ctx.environments.insert("local".into());
    for (name, svc) in services {
        if let Some(image) = yaml::get_str(svc, "image") {
            ctx.images.push(ImageRef {
                path: path.to_string(),
                image: image.to_string(),
                line: line_with(content, &format!("image: {image}"))
                    .unwrap_or(image)
                    .to_string(),
            });
        }
        if let Some(build) = yaml::get(svc, "build") {
            let context = yaml::str_of(Some(build))
                .or_else(|| yaml::get_str(build, "context"))
                .unwrap_or(".");
            let built = normalize(dirname(path), context);
            let app_key = ctx
                .app_for_path(&format!("{built}/x"))
                .map(|a| a.key.clone());
            if let Some(app_key) = app_key {
                ctx.edge_with(
                    &app_key,
                    &env_key,
                    "DEPLOYED_TO",
                    RelationCategory::Infrastructure,
                    attrs(&[("compose_service", &name), ("path", path)]),
                );
            }
        }
    }
    ctx.chunk(
        ChunkKind::Iac,
        path,
        &format!("Docker Compose ({})", short_path(path, 2)),
        content,
    );
}

/// Joins `dir` and a compose build context such as `./api` or `../web`.
fn normalize(dir: &str, context: &str) -> String {
    let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    for seg in context.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

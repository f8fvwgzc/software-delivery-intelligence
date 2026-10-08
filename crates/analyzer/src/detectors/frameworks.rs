//! Frameworks from dependency names, confirmed or supplemented by config
//! files. Each hit is attached to the app whose manifest declared it.

use crate::detectors::Ctx;
use crate::model::*;
use crate::util::{basename, display, line_with};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    Frontend,
    Backend,
    Fullstack,
    Tooling,
}

/// (ecosystem, dependency name, framework key, role)
const FRAMEWORKS: &[(&str, &str, &str, Role)] = &[
    ("npm", "react", "react", Role::Frontend),
    ("npm", "next", "nextjs", Role::Fullstack),
    ("npm", "vue", "vue", Role::Frontend),
    ("npm", "nuxt", "nuxt", Role::Fullstack),
    ("npm", "svelte", "svelte", Role::Frontend),
    ("npm", "@sveltejs/kit", "sveltekit", Role::Fullstack),
    ("npm", "@angular/core", "angular", Role::Frontend),
    ("npm", "astro", "astro", Role::Fullstack),
    ("npm", "@remix-run/react", "remix", Role::Fullstack),
    ("npm", "gatsby", "gatsby", Role::Frontend),
    ("npm", "vite", "vite", Role::Tooling),
    ("npm", "express", "express", Role::Backend),
    ("npm", "@nestjs/core", "nestjs", Role::Backend),
    ("npm", "fastify", "fastify", Role::Backend),
    ("npm", "koa", "koa", Role::Backend),
    ("npm", "hono", "hono", Role::Backend),
    ("cargo", "axum", "axum", Role::Backend),
    ("cargo", "actix-web", "actix", Role::Backend),
    ("cargo", "rocket", "rocket", Role::Backend),
    ("cargo", "warp", "warp", Role::Backend),
    ("cargo", "tonic", "tonic", Role::Backend),
    ("pip", "fastapi", "fastapi", Role::Backend),
    ("pip", "django", "django", Role::Backend),
    ("pip", "flask", "flask", Role::Backend),
    ("pip", "starlette", "starlette", Role::Backend),
    ("go", "github.com/gin-gonic/gin", "gin", Role::Backend),
    ("go", "github.com/labstack/echo/v4", "echo", Role::Backend),
    ("go", "github.com/gofiber/fiber/v2", "fiber", Role::Backend),
    ("go", "github.com/go-chi/chi/v5", "chi", Role::Backend),
    (
        "maven",
        "org.springframework.boot:spring-boot-starter-web",
        "spring",
        Role::Backend,
    ),
    (
        "maven",
        "org.springframework.boot:spring-boot-starter",
        "spring",
        Role::Backend,
    ),
    (
        "maven",
        "plugin:org.springframework.boot",
        "spring",
        Role::Backend,
    ),
    (
        "maven",
        "io.quarkus:quarkus-resteasy",
        "quarkus",
        Role::Backend,
    ),
    (
        "maven",
        "io.micronaut:micronaut-http-server-netty",
        "micronaut",
        Role::Backend,
    ),
    ("bundler", "rails", "rails", Role::Fullstack),
    ("bundler", "sinatra", "sinatra", Role::Backend),
    ("composer", "laravel/framework", "laravel", Role::Fullstack),
    (
        "composer",
        "symfony/framework-bundle",
        "symfony",
        Role::Fullstack,
    ),
    ("mix", "phoenix", "phoenix", Role::Fullstack),
];

/// Config files that prove a framework even when the manifest is missing.
const FILE_SIGNALS: &[(&str, &str, Role)] = &[
    ("next.config.", "nextjs", Role::Fullstack),
    ("nuxt.config.", "nuxt", Role::Fullstack),
    ("svelte.config.", "svelte", Role::Frontend),
    ("astro.config.", "astro", Role::Fullstack),
    ("vite.config.", "vite", Role::Tooling),
    ("angular.json", "angular", Role::Frontend),
    ("manage.py", "django", Role::Backend),
];

pub(crate) fn detect(ctx: &mut Ctx<'_>) {
    for m in ctx.manifests.clone() {
        let content = ctx.file(&m.path).unwrap_or("");
        for (eco, dep, key, role) in FRAMEWORKS {
            if m.ecosystem != *eco || !m.has_dep(dep) {
                continue;
            }
            let evidence = line_with(content, dep)
                .map(String::from)
                .unwrap_or_else(|| format!("{dep} in {}", m.path));
            record(ctx, key, *role, &m.path, &evidence);
        }
    }
    for path in ctx.tree_paths().collect::<Vec<_>>() {
        let name = basename(path);
        for (signal, key, role) in FILE_SIGNALS {
            if name.starts_with(signal) {
                let manifest = ctx
                    .app_for_path(path)
                    .map(|a| a.manifest_path.clone())
                    .unwrap_or_default();
                record(ctx, key, *role, &manifest, &format!("{path} present"));
            }
        }
    }
    for app in ctx.apps.clone() {
        if app.frontend.is_empty() && app.backend.is_empty() {
            continue;
        }
        let mut frameworks = app.frontend.clone();
        frameworks.extend(app.backend.iter().cloned());
        let names: Vec<String> = frameworks.iter().map(|f| display(f)).collect();
        if let Some(n) = ctx.out.nodes.iter_mut().find(|n| n.key == app.key) {
            n.attributes.insert("frameworks".into(), names.join(", "));
            if n.description.is_empty() {
                n.description = format!("{} application", names.join(" + "));
            }
        }
    }
}

fn record(ctx: &mut Ctx<'_>, key: &str, role: Role, manifest_path: &str, evidence: &str) {
    let fact_key = format!("framework:{key}");
    if !ctx.has_fact(&fact_key) {
        let value = match role {
            Role::Frontend => "frontend",
            Role::Backend => "backend",
            Role::Fullstack => "fullstack",
            Role::Tooling => "build tool",
        };
        ctx.detected(
            FactCategory::Framework,
            &fact_key,
            &display(key),
            value,
            manifest_path,
            evidence,
        );
    }
    let app_key = ctx
        .app_for_manifest(manifest_path)
        .map(|a| a.key.clone())
        .or_else(|| ctx.apps.first().map(|a| a.key.clone()));
    let Some(app_key) = app_key else {
        return;
    };
    if let Some(app) = ctx.apps.iter_mut().find(|a| a.key == app_key) {
        let bucket = match role {
            Role::Frontend => &mut app.frontend,
            Role::Backend | Role::Fullstack => &mut app.backend,
            Role::Tooling => return,
        };
        if !bucket.contains(&key.to_string()) {
            bucket.push(key.to_string());
        }
        if role == Role::Fullstack && !app.frontend.contains(&key.to_string()) {
            app.frontend.push(key.to_string());
        }
    }
}

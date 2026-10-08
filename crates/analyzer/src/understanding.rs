//! TL;DR first, deep dive underneath. Every sentence is built from facts
//! already in the analysis so the text never claims more than the evidence.

use crate::detectors::Ctx;
use crate::model::*;
use crate::util::display;

struct View {
    name: String,
    languages: Vec<String>,
    frontend: Vec<String>,
    backend: Vec<String>,
    tooling: Vec<String>,
    datastores: Vec<String>,
    external: Vec<String>,
    dockerfiles: usize,
    compose: bool,
    nginx: bool,
    ci: Vec<String>,
    deploys_from_ci: bool,
    iac: Vec<String>,
    clouds: Vec<String>,
    tests: Vec<String>,
    envs: Vec<String>,
    static_site: bool,
    library_only: bool,
    monorepo: bool,
}

fn view(ctx: &Ctx<'_>) -> View {
    let facts = &ctx.out.facts;
    let strong = |prefix: &str| -> Vec<String> {
        let mut out = Vec::new();
        for f in facts.iter().filter(|f| f.confidence >= 0.8) {
            if let Some(s) = f.key.strip_prefix(prefix)
                && !out.contains(&s.to_string())
            {
                out.push(s.to_string());
            }
        }
        out
    };
    let frameworks = strong("framework:");
    let role = |k: &str| {
        ctx.fact(&format!("framework:{k}"))
            .map(|f| f.value.clone())
            .unwrap_or_default()
    };
    let mut datastores: Vec<String> = strong("datastore:")
        .into_iter()
        .filter(|d| !d.starts_with("orm:") && !d.starts_with("configured:"))
        .collect();
    for d in strong("infra:datastore:") {
        if !datastores.contains(&d) {
            datastores.push(d);
        }
    }
    View {
        name: ctx.repo_name().to_string(),
        languages: strong("lang:")
            .into_iter()
            .filter(|l| !matches!(l.as_str(), "shell"))
            .collect(),
        frontend: frameworks
            .iter()
            .filter(|k| matches!(role(k).as_str(), "frontend" | "fullstack"))
            .cloned()
            .collect(),
        backend: frameworks
            .iter()
            .filter(|k| matches!(role(k).as_str(), "backend" | "fullstack"))
            .cloned()
            .collect(),
        tooling: frameworks
            .iter()
            .filter(|k| role(k) == "build tool")
            .cloned()
            .collect(),
        datastores,
        external: ctx
            .out
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::ExternalApi)
            .map(|n| n.label.clone())
            .collect(),
        dockerfiles: strong("container:dockerfile:").len(),
        compose: ctx.has_fact_prefix("container:compose:"),
        nginx: ctx.has_fact("container:nginx"),
        ci: ["github_actions", "gitlab_ci"]
            .into_iter()
            .filter(|c| ctx.has_fact(&format!("ci:{c}")))
            .map(String::from)
            .collect(),
        deploys_from_ci: ctx.has_fact_prefix("deploy:pipeline:"),
        iac: ["terraform", "bicep", "pulumi", "cloudformation"]
            .into_iter()
            .filter(|i| ctx.has_fact(&format!("iac:{i}")))
            .map(String::from)
            .chain(
                ["manifests", "helm"]
                    .into_iter()
                    .filter(|k| ctx.has_fact(&format!("k8s:{k}")))
                    .map(|k| {
                        if k == "helm" {
                            "helm".to_string()
                        } else {
                            "kubernetes".to_string()
                        }
                    }),
            )
            .collect(),
        clouds: strong("cloud:"),
        tests: strong("testing:")
            .into_iter()
            .filter(|t| !matches!(t.as_str(), "ci" | "files" | "script"))
            .collect(),
        envs: ctx.environments.iter().cloned().collect(),
        static_site: ctx.has_fact("repository:static_site"),
        library_only: !ctx.apps.is_empty() && ctx.apps.iter().all(|a| a.library),
        monorepo: ctx.has_fact("repository:monorepo"),
    }
}

fn names(keys: &[String]) -> Vec<String> {
    keys.iter().map(|k| display(k)).collect()
}

fn join(items: &[String]) -> String {
    match items.len() {
        0 => String::new(),
        1 => items[0].clone(),
        n => format!("{} and {}", items[..n - 1].join(", "), items[n - 1]),
    }
}

/// "an axum service", "a React frontend": the article follows the sound of
/// the first letter, which is good enough for technology names.
fn with_article(phrase: &str) -> String {
    let vowel = phrase
        .chars()
        .next()
        .is_some_and(|c| "aeiouAEIOU".contains(c));
    format!("{} {phrase}", if vowel { "an" } else { "a" })
}

/// Language a backend framework is written in; a JS framework reports the
/// flavour the repository actually uses.
fn framework_language(framework: &str, languages: &[String]) -> Option<String> {
    let lang = match framework {
        "axum" | "actix" | "rocket" | "warp" | "tonic" => "rust",
        "fastapi" | "django" | "flask" | "starlette" => "python",
        "gin" | "echo" | "fiber" | "chi" => "go",
        "spring" | "quarkus" | "micronaut" => {
            if languages.iter().any(|l| l == "kotlin") {
                "kotlin"
            } else {
                "java"
            }
        }
        "rails" | "sinatra" => "ruby",
        "laravel" | "symfony" => "php",
        "phoenix" => "elixir",
        "express" | "nestjs" | "fastify" | "koa" | "hono" | "nextjs" | "nuxt" | "sveltekit"
        | "astro" | "remix" => {
            if languages.iter().any(|l| l == "javascript")
                && !languages.iter().any(|l| l == "typescript")
            {
                "javascript"
            } else {
                "typescript"
            }
        }
        _ => return None,
    };
    Some(display(lang))
}

fn kind_phrase(v: &View) -> String {
    if v.static_site {
        return "static website".into();
    }
    if v.library_only {
        return format!(
            "{} library",
            join(&names(&v.languages[..v.languages.len().min(1)]))
        );
    }
    let primary = v.languages.first().map(|l| display(l)).unwrap_or_default();
    match (!v.frontend.is_empty(), !v.backend.is_empty()) {
        (true, true) if v.frontend == v.backend => {
            format!("{} full-stack application", display(&v.frontend[0]))
        }
        (true, true) => format!(
            "{} web application with {} backend",
            display(&v.frontend[0]),
            with_article(&display(&v.backend[0]))
        ),
        (true, false) => format!("{} frontend application", display(&v.frontend[0])),
        (false, true) => format!(
            "{} {} service",
            display(&v.backend[0]),
            if primary.is_empty() {
                "API".into()
            } else {
                primary
            }
        ),
        (false, false) if !primary.is_empty() => format!("{primary} project"),
        _ => "project".into(),
    }
}

fn tldr(v: &View) -> String {
    let mut s = format!("{} is {}", v.name, with_article(&kind_phrase(v)));
    if !v.datastores.is_empty() {
        s.push_str(&format!(" backed by {}", join(&names(&v.datastores))));
    }
    let mut delivery = Vec::new();
    if v.dockerfiles > 0 {
        delivery.push("containerised with Docker".to_string());
    }
    if !v.ci.is_empty() {
        delivery.push(format!(
            "{} by {}",
            if v.deploys_from_ci {
                "built and deployed"
            } else {
                "built and tested"
            },
            join(&names(&v.ci))
        ));
    }
    if !v.iac.is_empty() || !v.clouds.is_empty() {
        let mut infra = names(&v.iac);
        if !v.clouds.is_empty() {
            infra.push(format!("on {}", join(&names(&v.clouds))));
        }
        delivery.push(format!("with infrastructure in {}", infra.join(" ")));
    }
    if !delivery.is_empty() {
        s.push_str(", ");
        s.push_str(&delivery.join(", "));
    }
    s.push('.');
    s
}

fn architecture(v: &View) -> String {
    let mut parts: Vec<String> = Vec::new();
    if v.static_site {
        parts.push("Static HTML, CSS and JavaScript are served directly to the browser with no build step and no server-side code.".into());
    } else {
        if !v.frontend.is_empty() {
            let tooling = if v.tooling.is_empty() {
                String::new()
            } else {
                format!(" built with {}", join(&names(&v.tooling)))
            };
            parts.push(format!(
                "The user interface is {} frontend{tooling}.",
                with_article(&join(&names(&v.frontend)))
            ));
        }
        if !v.backend.is_empty() {
            let services: Vec<String> = v
                .backend
                .iter()
                .map(|b| {
                    let lang = framework_language(b, &v.languages)
                        .or_else(|| v.languages.first().map(|l| display(l)))
                        .map(|l| format!(" written in {l}"))
                        .unwrap_or_default();
                    format!("{} service{lang}", with_article(&display(b)))
                })
                .collect();
            parts.push(format!("Requests are handled by {}.", join(&services)));
        } else if !v.languages.is_empty() && v.frontend.is_empty() {
            parts.push(format!(
                "The code is written in {}.",
                join(&names(&v.languages))
            ));
        }
    }
    if !v.datastores.is_empty() {
        parts.push(format!("State lives in {}.", join(&names(&v.datastores))));
    }
    if !v.external.is_empty() {
        parts.push(format!("It integrates with {}.", join(&v.external)));
    }
    if v.dockerfiles > 0 {
        let compose = if v.compose {
            " and Docker Compose runs the stack locally"
        } else {
            ""
        };
        parts.push(format!("Docker packages the application{compose}."));
    } else if v.compose {
        parts.push("Docker Compose runs the supporting services locally.".into());
    }
    if !v.ci.is_empty() {
        parts.push(format!(
            "{} {}.",
            join(&names(&v.ci)),
            if v.deploys_from_ci {
                "builds, tests and deploys every change"
            } else {
                "builds and tests every change"
            }
        ));
    }
    if !v.iac.is_empty() {
        let cloud = if v.clouds.is_empty() {
            String::new()
        } else {
            format!(" on {}", join(&names(&v.clouds)))
        };
        parts.push(format!(
            "Infrastructure is declared with {}{cloud}.",
            join(&names(&v.iac))
        ));
    } else if !v.clouds.is_empty() {
        parts.push(format!("Deployments target {}.", join(&names(&v.clouds))));
    }
    if v.monorepo {
        parts.push("Several packages share this monorepo.".into());
    }
    if parts.is_empty() {
        parts.push("Too little was detected to describe the architecture; the open questions ask for the missing pieces.".into());
    }
    parts.join(" ")
}

fn request_flow(v: &View) -> Vec<String> {
    let mut hops = vec!["Browser".to_string()];
    if v.static_site {
        hops.push("Static files".into());
        return hops;
    }
    if v.library_only {
        return vec!["Consumer".into(), v.name.clone()];
    }
    if v.nginx {
        hops.push("nginx".into());
    }
    if let Some(f) = v.frontend.first() {
        hops.push(display(f));
    }
    if let Some(b) = v.backend.first() {
        if Some(b) != v.frontend.first() {
            hops.push(display(b));
        }
    } else if v.frontend.is_empty() {
        hops.push("API".into());
    }
    for d in &v.datastores {
        hops.push(display(d));
    }
    hops
}

fn section(key: &str, title: &str, summary: String, items: Vec<String>) -> SectionDraft {
    SectionDraft {
        key: key.into(),
        title: title.into(),
        summary,
        items,
    }
}

fn or_none(items: &[String], what: &str) -> String {
    if items.is_empty() {
        format!("No {what} detected.")
    } else {
        join(items)
    }
}

pub(crate) fn derive(ctx: &Ctx<'_>) -> UnderstandingDraft {
    let v = view(ctx);
    let deps = &ctx.out.dependencies;
    let mut by_eco: Vec<(String, usize)> = Vec::new();
    for d in deps {
        match by_eco.iter_mut().find(|(e, _)| *e == d.ecosystem) {
            Some(e) => e.1 += 1,
            None => by_eco.push((d.ecosystem.clone(), 1)),
        }
    }
    let notable: Vec<String> = deps
        .iter()
        .filter(|d| d.scope == "runtime")
        .map(|d| d.name.clone())
        .take(15)
        .collect();
    let pipelines: Vec<String> = ctx
        .out
        .pipelines
        .iter()
        .map(|p| {
            format!(
                "{} ({}; {} jobs; on {})",
                p.name,
                p.path,
                p.jobs.len(),
                p.triggers.join(", ")
            )
        })
        .collect();
    let infra: Vec<String> = ctx
        .out
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Infrastructure)
        .map(|n| n.label.clone())
        .take(20)
        .collect();
    let security: Vec<String> = ctx
        .out
        .facts
        .iter()
        .filter(|f| f.category == FactCategory::Security)
        .map(|f| format!("{}: {}", f.label, f.value))
        .collect();
    let deploy_items: Vec<String> = ctx
        .out
        .facts
        .iter()
        .filter(|f| f.category == FactCategory::Deployment || f.key.starts_with("cloud:"))
        .map(|f| format!("{}: {}", f.label, f.value))
        .collect();
    let docs_items: Vec<String> = ctx
        .out
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Document)
        .map(|n| format!("{} ({})", n.label, n.source_path))
        .collect();
    let stack: Vec<String> = ctx.out.technologies.clone();

    let mut arch_items = Vec::new();
    for a in &ctx.apps {
        let fw: Vec<String> = a
            .frontend
            .iter()
            .chain(a.backend.iter())
            .map(|f| display(f))
            .collect();
        arch_items.push(format!(
            "{} {} ({}{})",
            if a.library { "library" } else { "service" },
            a.name,
            a.manifest_path,
            if fw.is_empty() {
                String::new()
            } else {
                format!("; {}", fw.join(", "))
            }
        ));
    }

    let sections = vec![
        section(
            "stack",
            "Stack",
            or_none(&stack, "technologies"),
            stack.clone(),
        ),
        section("architecture", "Architecture", architecture(&v), arch_items),
        section(
            "dependencies",
            "Dependencies",
            if deps.is_empty() {
                "No manifest dependencies detected.".into()
            } else {
                by_eco
                    .iter()
                    .map(|(e, n)| format!("{n} {e}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            },
            notable,
        ),
        section(
            "database",
            "Database",
            or_none(&names(&v.datastores), "datastore"),
            ctx.out
                .facts
                .iter()
                .filter(|f| {
                    f.category == FactCategory::Datastore || f.category == FactCategory::Migration
                })
                .map(|f| format!("{}: {}", f.label, f.value))
                .collect(),
        ),
        section(
            "external_apis",
            "External APIs",
            or_none(&v.external, "third-party API"),
            v.external.clone(),
        ),
        section(
            "tests",
            "Tests",
            or_none(&names(&v.tests), "test framework"),
            ctx.out
                .facts
                .iter()
                .filter(|f| f.category == FactCategory::Testing)
                .map(|f| format!("{}: {}", f.label, f.value))
                .collect(),
        ),
        section(
            "ci_cd",
            "CI/CD",
            or_none(&names(&v.ci), "CI pipeline"),
            pipelines,
        ),
        section(
            "infrastructure",
            "Infrastructure",
            or_none(&names(&v.iac), "infrastructure as code"),
            infra,
        ),
        section(
            "security",
            "Security",
            or_none(&security, "security configuration"),
            security.clone(),
        ),
        section(
            "deployment",
            "Deployment",
            if v.clouds.is_empty() && !v.deploys_from_ci && ctx.deploy_targets.is_empty() {
                "No deployment target detected.".into()
            } else {
                let mut t: Vec<String> = ctx.deploy_targets.iter().map(|t| display(t)).collect();
                if !v.envs.is_empty() {
                    t.push(format!("environments: {}", v.envs.join(", ")));
                }
                join(&t)
            },
            deploy_items,
        ),
        section(
            "documentation",
            "Documentation",
            or_none(&docs_items, "documentation"),
            docs_items.clone(),
        ),
    ];

    UnderstandingDraft {
        tldr: tldr(&v),
        architecture: architecture(&v),
        stack,
        sections,
        request_flow: request_flow(&v),
    }
}

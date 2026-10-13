//! Security posture read from names and configuration only: env templates,
//! Dependabot, committed secret-looking files, scanning in CI, CODEOWNERS.
//! File contents of anything secret-like are never opened.

use crate::detectors::{Ctx, attrs};
use crate::files::{is_env_template, looks_secret};
use crate::model::*;
use crate::util::{basename, line_with};
use crate::yaml;

pub(crate) fn detect(ctx: &mut Ctx<'_>) {
    env_example(ctx);
    dependabot(ctx);
    committed_secrets(ctx);
    scanning(ctx);
    codeowners(ctx);
    if let Some((path, content)) = ctx
        .files_where(|p| basename(p).eq_ignore_ascii_case("security.md"))
        .into_iter()
        .next()
    {
        ctx.detected(
            FactCategory::Security,
            "security:policy",
            "Security policy",
            "SECURITY.md",
            path,
            line_with(content, "#").unwrap_or("SECURITY.md"),
        );
    }
}

fn env_example(ctx: &mut Ctx<'_>) {
    let Some(path) = ctx
        .tree_paths()
        .find(|p| is_env_template(&basename(p).to_ascii_lowercase()))
        .map(String::from)
    else {
        return;
    };
    // Variable names are safe to surface; values never are.
    let vars: Vec<String> = ctx
        .file(&path)
        .map(|c| {
            c.lines()
                .filter(|l| !l.trim_start().starts_with('#'))
                .filter_map(|l| l.split('=').next())
                .map(|k| k.trim().to_string())
                .filter(|k| !k.is_empty())
                .collect()
        })
        .unwrap_or_default();
    ctx.detected(
        FactCategory::Security,
        "security:env_example",
        "Environment template",
        &format!("{} variables", vars.len()),
        &path,
        &if vars.is_empty() {
            basename(&path).to_string()
        } else {
            vars.iter().take(6).cloned().collect::<Vec<_>>().join(", ")
        },
    );
    for v in &vars {
        let upper = v.to_ascii_uppercase();
        let engine = if upper.contains("DATABASE_URL")
            || upper.contains("POSTGRES")
            || upper.starts_with("PG")
        {
            Some("postgres")
        } else if upper.contains("REDIS") {
            Some("redis")
        } else if upper.contains("MONGO") {
            Some("mongodb")
        } else if upper.contains("MYSQL") {
            Some("mysql")
        } else {
            None
        };
        if let Some(engine) = engine {
            let key = format!("datastore:configured:{engine}");
            if !ctx.has_fact(&key) {
                ctx.inferred(
                    FactCategory::Datastore,
                    &key,
                    "Datastore configured by environment",
                    engine,
                    0.6,
                    &format!("{v} in {}", basename(&path)),
                );
            }
        }
    }
}

/// GitHub reads either spelling of the file name.
const DEPENDABOT_PATHS: [&str; 2] = [".github/dependabot.yml", ".github/dependabot.yaml"];

fn dependabot(ctx: &mut Ctx<'_>) {
    let Some(path) = DEPENDABOT_PATHS
        .into_iter()
        .find(|path| ctx.file(path).is_some() || ctx.tree_has(|p| p == *path))
    else {
        return;
    };
    let Some(content) = ctx.file(path) else {
        ctx.out.dependabot = DependabotDraft {
            configured: true,
            config_path: path.into(),
            ecosystems: Vec::new(),
        };
        ctx.detected(
            FactCategory::Security,
            "security:dependabot",
            "Dependabot",
            "configured",
            path,
            "Dependabot configuration file in tree",
        );
        return;
    };
    let ecosystems: Vec<String> = yaml::first(content)
        .map(|doc| match yaml::get(&doc, "updates") {
            Some(serde_yaml_ng::Value::Sequence(seq)) => seq
                .iter()
                .filter_map(|u| yaml::get_str(u, "package-ecosystem").map(String::from))
                .collect(),
            _ => Vec::new(),
        })
        .unwrap_or_default();
    ctx.out.dependabot = DependabotDraft {
        configured: true,
        config_path: path.into(),
        ecosystems: ecosystems.clone(),
    };
    ctx.detected(
        FactCategory::Security,
        "security:dependabot",
        "Dependabot",
        &ecosystems.join(", "),
        path,
        line_with(content, "package-ecosystem").unwrap_or("updates:"),
    );
}

fn committed_secrets(ctx: &mut Ctx<'_>) {
    let suspicious: Vec<String> = ctx
        .tree_paths()
        .filter(|p| looks_secret(p))
        .map(String::from)
        .take(10)
        .collect();
    for path in suspicious {
        ctx.detected(
            FactCategory::Security,
            &format!("security:suspicious_file:{path}"),
            "Secret-looking file committed",
            basename(&path),
            &path,
            "file name suggests credentials; content not inspected",
        );
    }
}

fn scanning(ctx: &mut Ctx<'_>) {
    let mut hits: Vec<(&'static str, &'static str, String, String)> = Vec::new();
    for (path, content) in ctx.files_where(|p| p.starts_with(".github/workflows/")) {
        let checks: &[(&str, &str, &str)] = &[
            ("github/codeql-action", "security:codeql", "CodeQL"),
            (
                "gitleaks/gitleaks-action",
                "security:secret_scanning",
                "Secret scanning",
            ),
            (
                "trufflesecurity/trufflehog",
                "security:secret_scanning",
                "Secret scanning",
            ),
            (
                "aquasecurity/trivy-action",
                "security:image_scanning",
                "Trivy",
            ),
            ("snyk/actions", "security:dependency_scanning", "Snyk"),
            ("cargo audit", "security:dependency_scanning", "cargo audit"),
            ("npm audit", "security:dependency_scanning", "npm audit"),
            ("pip-audit", "security:dependency_scanning", "pip-audit"),
        ];
        for (needle, key, label) in checks {
            if let Some(line) = line_with(content, needle)
                && !hits.iter().any(|(k, _, _, _)| k == key)
            {
                hits.push((key, label, path.to_string(), line.to_string()));
            }
        }
        if yaml::first(content)
            .map(|d| yaml::get(&d, "permissions").is_some())
            .unwrap_or(false)
            && !hits
                .iter()
                .any(|(k, _, _, _)| *k == "security:workflow_permissions")
        {
            hits.push((
                "security:workflow_permissions",
                "Scoped workflow permissions",
                path.to_string(),
                line_with(content, "permissions:")
                    .unwrap_or("permissions:")
                    .to_string(),
            ));
        }
    }
    for (key, label, path, line) in hits {
        ctx.detected(FactCategory::Security, key, label, "enabled", &path, &line);
    }
}

fn codeowners(ctx: &mut Ctx<'_>) {
    let Some((path, content)) = ctx
        .files_where(|p| p == ".github/CODEOWNERS" || p == "CODEOWNERS" || p == "docs/CODEOWNERS")
        .into_iter()
        .next()
    else {
        return;
    };
    let repo = ctx.repo_key.clone();
    let mut owners: Vec<String> = Vec::new();
    for line in content.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        for owner in t.split_whitespace().skip(1).filter(|o| o.starts_with('@')) {
            if !owners.contains(&owner.to_string()) {
                owners.push(owner.to_string());
            }
        }
    }
    if owners.is_empty() {
        return;
    }
    ctx.detected(
        FactCategory::Repository,
        "repository:codeowners",
        "Code owners",
        &owners.join(", "),
        path,
        line_with(content, "@").unwrap_or("CODEOWNERS"),
    );
    for owner in owners {
        let key = format!("owner:{}", owner.trim_start_matches('@'));
        ctx.node(
            NodeKind::Owner,
            &key,
            &owner,
            "Code owner",
            path,
            attrs(&[("handle", &owner)]),
        );
        ctx.edge(&repo, &key, "OWNED_BY", RelationCategory::Ownership);
    }
}

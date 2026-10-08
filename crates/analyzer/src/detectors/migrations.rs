//! Schema migrations: which tool, how many, and which datastore they target.
//! File names are enough; content is only read for small SQL files.

use regex::Regex;

use crate::detectors::{Ctx, attrs};
use crate::files::is_migration_path;
use crate::model::*;
use crate::util::{basename, dirname, has_dir};

struct Found {
    tool: &'static str,
    dir: String,
    count: usize,
    sample: String,
}

fn tool_for(ctx: &Ctx<'_>, path: &str) -> Option<&'static str> {
    let name = basename(path);
    let has_cargo_dep = |d: &str| {
        ctx.manifests
            .iter()
            .any(|m| m.ecosystem == "cargo" && m.has_dep(d))
    };
    let has_npm_dep = |d: &str| {
        ctx.manifests
            .iter()
            .any(|m| m.ecosystem == "npm" && m.has_dep(d))
    };
    let has_pip_dep = |d: &str| {
        ctx.manifests
            .iter()
            .any(|m| m.ecosystem == "pip" && m.has_dep(d))
    };
    Some(if path.contains("prisma/migrations/") {
        "prisma"
    } else if has_dir(path, "alembic") || (has_dir(path, "versions") && name.ends_with(".py")) {
        "alembic"
    } else if path.contains("db/migrate/") && name.ends_with(".rb") {
        "rails"
    } else if path.contains("priv/repo/migrations/") {
        "ecto"
    } else if path.contains("db/migration/")
        || (name.starts_with('V') && name.contains("__") && name.ends_with(".sql"))
    {
        "flyway"
    } else if has_dir(path, "migrations")
        && name.ends_with(".py")
        && name.starts_with(char::is_numeric)
    {
        "django"
    } else if has_dir(path, "drizzle") && name.ends_with(".sql") {
        "drizzle"
    } else if has_dir(path, "migrations") && name.ends_with(".sql") {
        if has_cargo_dep("sqlx") {
            "sqlx"
        } else if has_cargo_dep("diesel") {
            "diesel"
        } else if has_npm_dep("knex") {
            "knex"
        } else if name.contains(".up.") || name.contains(".down.") {
            "golang-migrate"
        } else if has_pip_dep("yoyo-migrations") {
            "yoyo"
        } else {
            "sql"
        }
    } else if has_dir(path, "migrations") && (name.ends_with(".js") || name.ends_with(".ts")) {
        if has_npm_dep("knex") {
            "knex"
        } else if has_npm_dep("typeorm") {
            "typeorm"
        } else {
            "javascript"
        }
    } else if has_dir(path, "migrations") && name.ends_with(".rs") {
        "diesel"
    } else {
        return None;
    })
}

pub(crate) fn detect(ctx: &mut Ctx<'_>) {
    let mut found: Vec<Found> = Vec::new();
    for path in ctx.tree_paths().collect::<Vec<_>>() {
        if !is_migration_path(path) && !path.contains("prisma/migrations/") {
            continue;
        }
        let Some(tool) = tool_for(ctx, path) else {
            continue;
        };
        let dir = migration_root(path);
        match found.iter_mut().find(|f| f.dir == dir) {
            Some(f) => f.count += 1,
            None => found.push(Found {
                tool,
                dir,
                count: 1,
                sample: path.to_string(),
            }),
        }
    }
    if found.is_empty() {
        return;
    }
    let table_re = Regex::new(r"(?i)create\s+table\s+(?:if\s+not\s+exists\s+)?[`\x22]?([\w.]+)")
        .expect("static regex");
    for f in found {
        ctx.detected(
            FactCategory::Migration,
            &format!("migrations:{}", f.tool),
            "Schema migrations",
            &format!("{} ({} files)", f.tool, f.count),
            &f.sample,
            &format!("{}/ holds {} migration files", f.dir, f.count),
        );
        let mut tables: Vec<String> = Vec::new();
        for (p, c) in ctx.files_where(|p| p.starts_with(&f.dir)) {
            if !p.ends_with(".sql") {
                continue;
            }
            for cap in table_re.captures_iter(c) {
                let t = cap[1].to_string();
                if !tables.contains(&t) && tables.len() < 12 {
                    tables.push(t);
                }
            }
        }
        let key = format!("migration:{}", f.dir);
        ctx.node(
            NodeKind::Migration,
            &key,
            &format!("{} migrations", f.tool),
            &format!("{} migration files in {}", f.count, f.dir),
            &f.sample,
            attrs(&[
                ("tool", f.tool),
                ("count", &f.count.to_string()),
                ("tables", &tables.join(",")),
            ]),
        );
        let owner = ctx
            .app_key_for_path(&f.sample)
            .unwrap_or_else(|| ctx.repo_key.clone());
        ctx.edge(&owner, &key, "CONTAINS", RelationCategory::Structure);
        let datastores: Vec<String> = ctx
            .out
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Datastore)
            .filter(|n| {
                !matches!(
                    n.attributes.get("engine").map(String::as_str),
                    Some("redis" | "kafka" | "rabbitmq" | "nats" | "minio" | "elasticsearch")
                )
            })
            .map(|n| n.key.clone())
            .collect();
        for ds in datastores {
            ctx.edge(&key, &ds, "APPLIES_TO", RelationCategory::Infrastructure);
        }
    }
}

/// `api/migrations/0001_init.sql` → `api/migrations`; keeps nested tool dirs intact.
fn migration_root(path: &str) -> String {
    let dir = dirname(path);
    let parts: Vec<&str> = dir.split('/').collect();
    if let Some(i) = parts
        .iter()
        .rposition(|p| matches!(*p, "migrations" | "migrate" | "versions" | "migration"))
    {
        return parts[..=i].join("/");
    }
    dir.to_string()
}

//! Datastores from two directions: drivers a service links against (what it
//! *needs*) and images or managed resources that infrastructure defines
//! (what it *has*). The gap between the two is a question for the human.

use crate::detectors::iac::ensure_datastore_node;
use crate::detectors::{Ctx, attrs};
use crate::model::*;
use crate::util::{display, line_with};

/// (ecosystem, dependency, engine). `*` engine means "look at features".
const DRIVERS: &[(&str, &str, &str)] = &[
    ("npm", "pg", "postgres"),
    ("npm", "postgres", "postgres"),
    ("npm", "pg-promise", "postgres"),
    ("npm", "@vercel/postgres", "postgres"),
    ("npm", "@neondatabase/serverless", "postgres"),
    ("npm", "@supabase/supabase-js", "postgres"),
    ("npm", "mysql", "mysql"),
    ("npm", "mysql2", "mysql"),
    ("npm", "sqlite3", "sqlite"),
    ("npm", "better-sqlite3", "sqlite"),
    ("npm", "mongoose", "mongodb"),
    ("npm", "mongodb", "mongodb"),
    ("npm", "redis", "redis"),
    ("npm", "ioredis", "redis"),
    ("npm", "@prisma/client", "*"),
    ("npm", "drizzle-orm", "*"),
    ("npm", "typeorm", "*"),
    ("npm", "knex", "*"),
    ("npm", "sequelize", "*"),
    ("cargo", "sqlx", "*"),
    ("cargo", "diesel", "*"),
    ("cargo", "sea-orm", "*"),
    ("cargo", "tokio-postgres", "postgres"),
    ("cargo", "postgres", "postgres"),
    ("cargo", "deadpool-postgres", "postgres"),
    ("cargo", "mysql_async", "mysql"),
    ("cargo", "rusqlite", "sqlite"),
    ("cargo", "mongodb", "mongodb"),
    ("cargo", "redis", "redis"),
    ("pip", "psycopg", "postgres"),
    ("pip", "psycopg2", "postgres"),
    ("pip", "psycopg2-binary", "postgres"),
    ("pip", "asyncpg", "postgres"),
    ("pip", "pymysql", "mysql"),
    ("pip", "mysqlclient", "mysql"),
    ("pip", "pymongo", "mongodb"),
    ("pip", "motor", "mongodb"),
    ("pip", "redis", "redis"),
    ("pip", "sqlalchemy", "*"),
    ("pip", "sqlmodel", "*"),
    ("pip", "tortoise-orm", "*"),
    ("go", "github.com/lib/pq", "postgres"),
    ("go", "github.com/jackc/pgx/v5", "postgres"),
    ("go", "github.com/jackc/pgx/v4", "postgres"),
    ("go", "github.com/go-sql-driver/mysql", "mysql"),
    ("go", "github.com/mattn/go-sqlite3", "sqlite"),
    ("go", "go.mongodb.org/mongo-driver", "mongodb"),
    ("go", "github.com/redis/go-redis/v9", "redis"),
    ("go", "github.com/go-redis/redis/v8", "redis"),
    ("maven", "org.postgresql:postgresql", "postgres"),
    ("maven", "com.mysql:mysql-connector-j", "mysql"),
    ("maven", "mysql:mysql-connector-java", "mysql"),
    ("maven", "org.xerial:sqlite-jdbc", "sqlite"),
    ("maven", "org.mongodb:mongodb-driver-sync", "mongodb"),
    (
        "maven",
        "org.springframework.boot:spring-boot-starter-data-mongodb",
        "mongodb",
    ),
    (
        "maven",
        "org.springframework.boot:spring-boot-starter-data-redis",
        "redis",
    ),
    ("maven", "redis.clients:jedis", "redis"),
    ("maven", "io.lettuce:lettuce-core", "redis"),
    ("bundler", "pg", "postgres"),
    ("bundler", "mysql2", "mysql"),
    ("bundler", "sqlite3", "sqlite"),
    ("bundler", "mongoid", "mongodb"),
    ("bundler", "redis", "redis"),
    ("composer", "doctrine/dbal", "*"),
    ("composer", "predis/predis", "redis"),
    ("mix", "postgrex", "postgres"),
    ("mix", "myxql", "mysql"),
    ("mix", "redix", "redis"),
    ("mix", "mongodb_driver", "mongodb"),
];

/// Engines named in an image reference such as `postgres:16-alpine` or
/// `bitnami/redis`.
pub(crate) fn engine_for_image(image: &str) -> Option<&'static str> {
    let repo = image
        .split(':')
        .next()
        .unwrap_or(image)
        .rsplit('/')
        .next()
        .unwrap_or(image)
        .to_ascii_lowercase();
    Some(match repo.as_str() {
        "postgres" | "postgresql" | "pgvector" | "timescaledb" | "postgis" => "postgres",
        "mysql" => "mysql",
        "mariadb" => "mariadb",
        "mongo" | "mongodb" => "mongodb",
        "redis" | "valkey" | "keydb" => "redis",
        "elasticsearch" | "opensearch" => "elasticsearch",
        "kafka" | "cp-kafka" => "kafka",
        "rabbitmq" => "rabbitmq",
        "nats" => "nats",
        "minio" => "minio",
        _ => return None,
    })
}

/// Engine implied by ORM features or a Prisma schema.
fn engine_from_features(ctx: &Ctx<'_>, dep: &str, features: &[String]) -> Vec<&'static str> {
    let mut out = Vec::new();
    for f in features {
        match f.as_str() {
            "postgres" | "postgresql" | "sqlx-postgres" => out.push("postgres"),
            "mysql" | "sqlx-mysql" => out.push("mysql"),
            "sqlite" | "sqlx-sqlite" => out.push("sqlite"),
            _ => {}
        }
    }
    if dep == "@prisma/client"
        && let Some((_, schema)) = ctx
            .files_where(|p| p.ends_with("schema.prisma"))
            .into_iter()
            .next()
        && let Some(line) = line_with(schema, "provider")
    {
        let l = line.to_ascii_lowercase();
        if l.contains("postgres") {
            out.push("postgres");
        } else if l.contains("mysql") {
            out.push("mysql");
        } else if l.contains("sqlite") {
            out.push("sqlite");
        } else if l.contains("mongodb") {
            out.push("mongodb");
        }
    }
    out
}

pub(crate) fn detect(ctx: &mut Ctx<'_>) {
    // 1. Drivers → datastore the service needs.
    for m in ctx.manifests.clone() {
        let content = ctx.file(&m.path).unwrap_or("");
        let app_key = ctx
            .app_for_manifest(&m.path)
            .map(|a| a.key.clone())
            .or_else(|| ctx.app_key_for_path(&m.path));
        for (eco, dep, engine) in DRIVERS {
            if m.ecosystem != *eco {
                continue;
            }
            let Some(d) = m.dep(dep) else {
                continue;
            };
            let engines: Vec<&str> = if *engine == "*" {
                engine_from_features(ctx, dep, &d.features)
            } else {
                vec![engine]
            };
            let evidence = line_with(content, dep)
                .map(String::from)
                .unwrap_or_else(|| format!("{dep} in {}", m.path));
            if engines.is_empty() {
                ctx.detected(
                    FactCategory::Datastore,
                    &format!("datastore:orm:{dep}"),
                    "Database access layer",
                    dep,
                    &m.path,
                    &evidence,
                );
                continue;
            }
            for engine in engines {
                ctx.detected(
                    FactCategory::Datastore,
                    &format!("datastore:{engine}"),
                    &display(engine),
                    &format!("driver {dep}"),
                    &m.path,
                    &evidence,
                );
                ensure_datastore_node(ctx, engine, &m.path, "dependency");
                if let Some(app) = &app_key {
                    ctx.edge_with(
                        app,
                        &format!("datastore:{engine}"),
                        "DEPENDS_ON",
                        RelationCategory::Infrastructure,
                        attrs(&[("driver", dep)]),
                    );
                }
            }
        }
    }

    // 2. Images in compose / Kubernetes → datastore the infrastructure provides.
    for img in ctx.images.clone() {
        let Some(engine) = engine_for_image(&img.image) else {
            if !img.image.is_empty() && img.image.contains("nginx") {
                ctx.detected(
                    FactCategory::Container,
                    "container:nginx",
                    "nginx",
                    "reverse proxy",
                    &img.path,
                    &img.line,
                );
            }
            continue;
        };
        let defined_by = if img.path.contains("compose") {
            "docker_compose"
        } else {
            "kubernetes"
        };
        ctx.detected(
            FactCategory::Datastore,
            &format!("infra:datastore:{engine}"),
            &format!("{} ({})", display(engine), display(defined_by)),
            &img.image,
            &img.path,
            &img.line,
        );
        ensure_datastore_node(ctx, engine, &img.path, defined_by);
        if let Some(n) = ctx
            .out
            .nodes
            .iter_mut()
            .find(|n| n.key == format!("datastore:{engine}"))
        {
            n.attributes.insert("image".into(), img.image.clone());
        }
        // A compose-only datastore with no driver still belongs to the app that runs beside it.
        let ds_key = format!("datastore:{engine}");
        let linked = ctx
            .out
            .edges
            .iter()
            .any(|e| e.target_key == ds_key && e.relation == "DEPENDS_ON");
        if !linked && let Some(app) = ctx.primary_service_key() {
            ctx.edge_with(
                &app,
                &ds_key,
                "DEPENDS_ON",
                RelationCategory::Infrastructure,
                attrs(&[("via", defined_by)]),
            );
        }
        if matches!(engine, "kafka" | "rabbitmq" | "nats") {
            ctx.inferred(
                FactCategory::Datastore,
                "architecture:messaging",
                "Message broker in use",
                engine,
                0.8,
                &img.line,
            );
        }
    }

    // 3. One architecture inference once everything is known.
    let engines = ctx.fact_suffixes("datastore:");
    let engines: Vec<String> = engines
        .into_iter()
        .filter(|e| !e.starts_with("orm:"))
        .collect();
    if engines.len() > 1 {
        ctx.inferred(
            FactCategory::Datastore,
            "architecture:polyglot_persistence",
            "Several datastores",
            &engines.join(", "),
            0.7,
            "more than one datastore engine is referenced",
        );
    }
}

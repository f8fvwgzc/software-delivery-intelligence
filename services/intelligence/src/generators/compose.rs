//! `docker-compose.yml`: the application plus every detected datastore, with
//! healthchecks so `depends_on` waits for readiness and a `test` profile that
//! runs the project's own test command against the same services.

use super::{GenInput, Stack, artifact};
use crate::domain::model::ArtifactData;

pub fn generate(input: &GenInput) -> Vec<ArtifactData> {
    vec![artifact(
        input,
        "docker-compose.yml",
        "yaml",
        format!(
            "Runs {} with {} locally and in CI; the `test` profile executes `{}`.",
            input.repo_name,
            if input.datastores.is_empty() {
                "no datastores".to_owned()
            } else {
                input.datastores.join(", ")
            },
            input.test_command
        ),
        compose(input),
    )]
}

struct DatastoreService {
    name: &'static str,
    image: &'static str,
    port: u16,
    environment: Vec<String>,
    healthcheck: &'static str,
    volume: Option<&'static str>,
    url_var: Option<(&'static str, String)>,
}

fn datastore(engine: &str, app: &str) -> Option<DatastoreService> {
    let db = app.replace('-', "_");
    Some(match engine {
        "postgres" => DatastoreService {
            name: "postgres",
            image: "postgres:16-alpine",
            port: 5432,
            environment: vec![
                format!("POSTGRES_DB={db}"),
                "POSTGRES_USER=app".into(),
                "POSTGRES_PASSWORD=${POSTGRES_PASSWORD:-app}".into(),
            ],
            healthcheck: "pg_isready -U app",
            volume: Some("/var/lib/postgresql/data"),
            url_var: Some((
                "DATABASE_URL",
                format!("postgres://app:${{POSTGRES_PASSWORD:-app}}@postgres:5432/{db}"),
            )),
        },
        "mysql" => DatastoreService {
            name: "mysql",
            image: "mysql:8",
            port: 3306,
            environment: vec![
                format!("MYSQL_DATABASE={db}"),
                "MYSQL_USER=app".into(),
                "MYSQL_PASSWORD=${MYSQL_PASSWORD:-app}".into(),
                "MYSQL_ROOT_PASSWORD=${MYSQL_ROOT_PASSWORD:-root}".into(),
            ],
            healthcheck: "mysqladmin ping -h 127.0.0.1 -u app -p${MYSQL_PASSWORD:-app}",
            volume: Some("/var/lib/mysql"),
            url_var: Some((
                "DATABASE_URL",
                format!("mysql://app:${{MYSQL_PASSWORD:-app}}@mysql:3306/{db}"),
            )),
        },
        "redis" => DatastoreService {
            name: "redis",
            image: "redis:7-alpine",
            port: 6379,
            environment: vec![],
            healthcheck: "redis-cli ping",
            volume: None,
            url_var: Some(("REDIS_URL", "redis://redis:6379".into())),
        },
        "mongodb" => DatastoreService {
            name: "mongodb",
            image: "mongo:7",
            port: 27017,
            environment: vec![],
            healthcheck: "mongosh --quiet --eval 'db.runCommand({ ping: 1 }).ok'",
            volume: Some("/data/db"),
            url_var: Some(("DATABASE_URL", format!("mongodb://mongodb:27017/{db}"))),
        },
        _ => return None,
    })
}

pub fn compose(input: &GenInput) -> String {
    let services: Vec<DatastoreService> = input
        .datastores
        .iter()
        .filter_map(|e| datastore(e, &input.image))
        .collect();
    let mut out = String::from("services:\n");
    out.push_str(&format!(
        "  app:\n    build:\n      context: {context}\n      dockerfile: Dockerfile\n    image: {}:local\n    restart: unless-stopped\n    ports:\n      - \"{port}:{port}\"\n",
        input.image,
        context = input.context_dir(),
        port = input.port
    ));
    out.push_str("    env_file:\n      - .env\n    environment:\n");
    out.push_str(&format!("      PORT: \"{}\"\n", input.port));
    for service in &services {
        if let Some((var, value)) = &service.url_var {
            out.push_str(&format!("      {var}: {value}\n"));
        }
    }
    if !services.is_empty() {
        out.push_str("    depends_on:\n");
        for service in &services {
            out.push_str(&format!(
                "      {}:\n        condition: service_healthy\n",
                service.name
            ));
        }
    }
    let probe = if input.stack == Stack::Php {
        "php-fpm -t".to_owned()
    } else {
        format!(
            "wget -qO- http://127.0.0.1:{}{} || exit 1",
            input.port, input.health_path
        )
    };
    out.push_str(&format!(
        "    healthcheck:\n      test: [\"CMD-SHELL\", \"{probe}\"]\n      interval: 30s\n      timeout: 5s\n      retries: 3\n      start_period: 15s\n"
    ));

    // The test runner reuses the app image's build stage so dependencies
    // match production; every generated Dockerfile names that stage `build`,
    // except the PHP one, which has no build stage (composer runs in `vendor`).
    let target = if input.stack == Stack::Php {
        ""
    } else {
        "      target: build\n"
    };
    out.push_str(&format!(
        "\n  test:\n    profiles: [\"test\"]\n    build:\n      context: {context}\n      dockerfile: Dockerfile\n{target}    command: {}\n    env_file:\n      - .env\n",
        yaml_string(&input.test_command),
        context = input.context_dir(),
    ));
    if !services.is_empty() {
        out.push_str("    environment:\n");
        for service in &services {
            if let Some((var, value)) = &service.url_var {
                out.push_str(&format!("      {var}: {value}\n"));
            }
        }
        out.push_str("    depends_on:\n");
        for service in &services {
            out.push_str(&format!(
                "      {}:\n        condition: service_healthy\n",
                service.name
            ));
        }
    }

    for service in &services {
        out.push_str(&format!(
            "\n  {}:\n    image: {}\n    restart: unless-stopped\n",
            service.name, service.image
        ));
        if !service.environment.is_empty() {
            out.push_str("    environment:\n");
            for line in &service.environment {
                let (k, v) = line.split_once('=').unwrap_or((line, ""));
                out.push_str(&format!("      {k}: {}\n", yaml_string(v)));
            }
        }
        out.push_str(&format!(
            "    ports:\n      - \"127.0.0.1:{port}:{port}\"\n",
            port = service.port
        ));
        if let Some(path) = service.volume {
            out.push_str(&format!(
                "    volumes:\n      - {}-data:{path}\n",
                service.name
            ));
        }
        out.push_str(&format!(
            "    healthcheck:\n      test: [\"CMD-SHELL\", \"{}\"]\n      interval: 10s\n      timeout: 5s\n      retries: 5\n",
            service.healthcheck
        ));
    }

    let volumes: Vec<&str> = services
        .iter()
        .filter(|s| s.volume.is_some())
        .map(|s| s.name)
        .collect();
    if !volumes.is_empty() {
        out.push_str("\nvolumes:\n");
        for name in volumes {
            out.push_str(&format!("  {name}-data:\n"));
        }
    }
    out
}

/// Double-quotes a scalar so `${VAR}` and colons survive YAML parsing.
pub fn yaml_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::super::{input::fixtures, snapshot::assert_snapshot};
    use super::*;

    #[test]
    fn compose_with_postgres_snapshot() {
        let files = generate(&fixtures::input(&fixtures::node_context(), ""));
        assert_snapshot("compose_node_postgres", &files[0]);
        let text = &files[0].content;
        assert!(text.contains("postgres:16-alpine"));
        assert!(text.contains("condition: service_healthy"));
        assert!(text.contains(
            "DATABASE_URL: postgres://app:${POSTGRES_PASSWORD:-app}@postgres:5432/view_engine"
        ));
        assert!(text.contains("command: \"pnpm test\""));
        assert!(text.contains("postgres-data:"));
        assert!(
            files[0].replaces_existing,
            "the postgres fact came from docker-compose.yml"
        );
    }

    #[test]
    fn compose_without_datastores_has_no_volumes() {
        let files = generate(&fixtures::input(&fixtures::python_context(), ""));
        assert!(!files[0].content.contains("volumes:"));
        assert!(!files[0].content.contains("depends_on"));
        assert!(files[0].content.contains("\"8000:8000\""));
    }
}

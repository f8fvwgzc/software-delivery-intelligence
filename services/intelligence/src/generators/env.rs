//! `.env.example`: every variable the compose file and the facts reference,
//! with safe placeholder values. Real secrets never appear in the repository.

use super::{GenInput, artifact};
use crate::domain::model::ArtifactData;

pub fn generate(input: &GenInput) -> Vec<ArtifactData> {
    vec![artifact(
        input,
        ".env.example",
        "text",
        "Documents every environment variable the service reads; copy to `.env` and fill in real values.",
        env_example(input),
    )]
}

fn placeholder(input: &GenInput, name: &str) -> String {
    let db = input.image.replace('-', "_");
    match name {
        "PORT" => input.port.to_string(),
        "DATABASE_URL" if input.has_datastore("mysql") => {
            format!("mysql://app:change-me@localhost:3306/{db}")
        }
        "DATABASE_URL" if input.has_datastore("mongodb") => {
            format!("mongodb://localhost:27017/{db}")
        }
        "DATABASE_URL" if input.has_datastore("sqlite") => format!("sqlite://{db}.db"),
        "DATABASE_URL" => format!("postgres://app:change-me@localhost:5432/{db}"),
        "REDIS_URL" => "redis://localhost:6379".to_owned(),
        "NODE_ENV" | "RACK_ENV" | "RAILS_ENV" | "APP_ENV" => "production".to_owned(),
        "LOG_LEVEL" | "RUST_LOG" => "info".to_owned(),
        n if n.ends_with("_SECRET")
            || n.ends_with("_KEY")
            || n.ends_with("_TOKEN")
            || n.ends_with("_PASSWORD") =>
        {
            "change-me".to_owned()
        }
        n if n.ends_with("_URL") => "https://example.invalid".to_owned(),
        _ => String::new(),
    }
}

pub fn env_example(input: &GenInput) -> String {
    let mut lines = vec![
        format!(
            "# Environment for {}. Copy to .env; .env is git-ignored.",
            input.repo_name
        ),
        String::new(),
        "# Runtime".to_owned(),
    ];
    let mut names: Vec<String> = input.env_vars.clone();
    if !names.iter().any(|n| n == "LOG_LEVEL") {
        names.push("LOG_LEVEL".to_owned());
    }
    let (datastore_vars, runtime_vars): (Vec<&String>, Vec<&String>) = names
        .iter()
        .partition(|n| n.as_str() == "DATABASE_URL" || n.as_str() == "REDIS_URL");
    for name in runtime_vars {
        lines.push(format!("{name}={}", placeholder(input, name)));
    }
    if !datastore_vars.is_empty() {
        lines.push(String::new());
        lines.push("# Datastores (docker-compose.yml provides local instances)".to_owned());
        for name in datastore_vars {
            lines.push(format!("{name}={}", placeholder(input, name)));
        }
    }
    if input.has_datastore("postgres") {
        lines.push("POSTGRES_PASSWORD=change-me".to_owned());
    }
    if input.has_datastore("mysql") {
        lines.push("MYSQL_PASSWORD=change-me".to_owned());
        lines.push("MYSQL_ROOT_PASSWORD=change-me".to_owned());
    }
    lines.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::super::{input::fixtures, snapshot::assert_snapshot};
    use super::*;

    #[test]
    fn env_example_snapshot() {
        let files = generate(&fixtures::input(&fixtures::node_context(), ""));
        assert_snapshot("env_node_postgres", &files[0]);
        assert!(files[0].content.contains("PORT=3000"));
        assert!(
            files[0]
                .content
                .contains("DATABASE_URL=postgres://app:change-me@localhost:5432/view_engine")
        );
        assert!(files[0].content.contains("POSTGRES_PASSWORD=change-me"));
    }
}

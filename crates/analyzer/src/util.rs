//! Path and text helpers shared by every detector.

/// Last path component.
pub(crate) fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Directory part without trailing slash; empty for root files.
pub(crate) fn dirname(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => &path[..i],
        None => "",
    }
}

/// Lowercase extension without the dot; empty when there is none.
pub(crate) fn extension(path: &str) -> String {
    let name = basename(path);
    match name.rfind('.') {
        Some(i) if i > 0 => name[i + 1..].to_ascii_lowercase(),
        _ => String::new(),
    }
}

/// Number of directory levels above the file.
pub(crate) fn depth(path: &str) -> usize {
    path.matches('/').count()
}

/// True when any directory component equals `dir`.
pub(crate) fn has_dir(path: &str, dir: &str) -> bool {
    let d = dirname(path);
    !d.is_empty() && d.split('/').any(|c| c == dir)
}

pub(crate) fn is_yaml(path: &str) -> bool {
    matches!(extension(path).as_str(), "yml" | "yaml")
}

/// Short single-line excerpt used as fact evidence. Collapses whitespace so
/// evidence never carries indentation noise.
pub(crate) fn excerpt(text: &str, max: usize) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    truncate(&line.split_whitespace().collect::<Vec<_>>().join(" "), max)
}

/// First line containing `needle` (case-insensitive), trimmed.
pub(crate) fn line_with<'a>(text: &'a str, needle: &str) -> Option<&'a str> {
    let needle = needle.to_ascii_lowercase();
    text.lines()
        .map(str::trim)
        .find(|l| l.to_ascii_lowercase().contains(&needle))
}

/// Blanks the value of any `password: …` / `TOKEN=…` style line so a chunk
/// can quote a compose file or workflow without ever quoting a credential.
pub(crate) fn redact(text: &str) -> String {
    let re = regex::Regex::new(
        r#"(?im)^(\s*[-"']?[\w."']*(?:password|passwd|secret|token|api[_-]?key|apikey|private[_-]?key|credential)[\w."']*\s*[:=]\s*)(\S.*)$"#,
    )
    .expect("static regex");
    re.replace_all(text, "${1}<redacted>").into_owned()
}

pub(crate) fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Last `n` components as a short, readable label (`api/Dockerfile`).
pub(crate) fn short_path(path: &str, n: usize) -> String {
    let parts: Vec<&str> = path.split('/').collect();
    let start = parts.len().saturating_sub(n);
    parts[start..].join("/")
}

/// Marketing names for the keys used everywhere else. Unknown keys fall back
/// to the key itself so nothing is ever silently dropped.
pub(crate) fn display(key: &str) -> String {
    let name = match key {
        "rust" => "Rust",
        "typescript" => "TypeScript",
        "javascript" => "JavaScript",
        "python" => "Python",
        "go" => "Go",
        "java" => "Java",
        "kotlin" => "Kotlin",
        "csharp" => "C#",
        "ruby" => "Ruby",
        "php" => "PHP",
        "elixir" => "Elixir",
        "swift" => "Swift",
        "html" => "HTML",
        "css" => "CSS",
        "shell" => "Shell",
        "scala" => "Scala",
        "dart" => "Dart",
        "cpp" => "C++",
        "c" => "C",
        "react" => "React",
        "nextjs" => "Next.js",
        "vue" => "Vue",
        "nuxt" => "Nuxt",
        "svelte" => "Svelte",
        "sveltekit" => "SvelteKit",
        "angular" => "Angular",
        "astro" => "Astro",
        "remix" => "Remix",
        "gatsby" => "Gatsby",
        "vite" => "Vite",
        "express" => "Express",
        "nestjs" => "NestJS",
        "fastify" => "Fastify",
        "koa" => "Koa",
        "hono" => "Hono",
        "fastapi" => "FastAPI",
        "django" => "Django",
        "flask" => "Flask",
        "starlette" => "Starlette",
        "axum" => "axum",
        "actix" => "Actix Web",
        "rocket" => "Rocket",
        "warp" => "warp",
        "tonic" => "tonic",
        "gin" => "Gin",
        "echo" => "Echo",
        "fiber" => "Fiber",
        "chi" => "chi",
        "spring" => "Spring Boot",
        "quarkus" => "Quarkus",
        "micronaut" => "Micronaut",
        "rails" => "Ruby on Rails",
        "sinatra" => "Sinatra",
        "laravel" => "Laravel",
        "symfony" => "Symfony",
        "phoenix" => "Phoenix",
        "aspnet" => "ASP.NET Core",
        "postgres" => "PostgreSQL",
        "mysql" => "MySQL",
        "mariadb" => "MariaDB",
        "sqlite" => "SQLite",
        "mongodb" => "MongoDB",
        "redis" => "Redis",
        "elasticsearch" => "Elasticsearch",
        "kafka" => "Kafka",
        "rabbitmq" => "RabbitMQ",
        "nats" => "NATS",
        "minio" => "MinIO",
        "cosmosdb" => "Cosmos DB",
        "dynamodb" => "DynamoDB",
        "docker" => "Docker",
        "docker_compose" => "Docker Compose",
        "github_actions" => "GitHub Actions",
        "gitlab_ci" => "GitLab CI",
        "terraform" => "Terraform",
        "bicep" => "Bicep",
        "pulumi" => "Pulumi",
        "cloudformation" => "CloudFormation",
        "kubernetes" => "Kubernetes",
        "helm" => "Helm",
        "azure" => "Azure",
        "aws" => "AWS",
        "gcp" => "Google Cloud",
        "github_pages" => "GitHub Pages",
        "fly" => "Fly.io",
        "vercel" => "Vercel",
        "netlify" => "Netlify",
        "heroku" => "Heroku",
        "jest" => "Jest",
        "vitest" => "Vitest",
        "mocha" => "Mocha",
        "cypress" => "Cypress",
        "playwright" => "Playwright",
        "pytest" => "pytest",
        "cargo_test" => "cargo test",
        "go_test" => "go test",
        "junit" => "JUnit",
        "rspec" => "RSpec",
        "phpunit" => "PHPUnit",
        "exunit" => "ExUnit",
        "npm" => "npm",
        "pnpm" => "pnpm",
        "yarn" => "Yarn",
        "bun" => "Bun",
        "cargo" => "Cargo",
        "pip" => "pip",
        "poetry" => "Poetry",
        "uv" => "uv",
        "maven" => "Maven",
        "gradle" => "Gradle",
        "composer" => "Composer",
        "bundler" => "Bundler",
        "mix" => "Mix",
        "nginx" => "nginx",
        "prisma" => "Prisma",
        other => other,
    };
    name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_helpers() {
        assert_eq!(basename("a/b/c.rs"), "c.rs");
        assert_eq!(dirname("a/b/c.rs"), "a/b");
        assert_eq!(dirname("c.rs"), "");
        assert_eq!(extension("a/Dockerfile"), "");
        assert_eq!(extension("x/y.TF"), "tf");
        assert!(has_dir("node_modules/x/package.json", "node_modules"));
        assert!(!has_dir("package.json", "node_modules"));
        assert_eq!(depth("a/b/c"), 2);
    }

    #[test]
    fn redacts_credential_lines_only() {
        let out =
            redact("image: postgres\n  POSTGRES_PASSWORD: hunter2\nAPI_KEY=abc\nports: [5432]");
        assert!(out.contains("POSTGRES_PASSWORD: <redacted>"));
        assert!(out.contains("API_KEY=<redacted>"));
        assert!(!out.contains("hunter2") && !out.contains("abc"));
        assert!(out.contains("ports: [5432]"));
    }

    #[test]
    fn excerpt_collapses_whitespace() {
        assert_eq!(excerpt("\n\n   a   b\nc", 10), "a b");
        assert_eq!(truncate("abcdef", 4), "abc…");
    }
}

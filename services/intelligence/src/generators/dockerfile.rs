//! `Dockerfile` + `.dockerignore` per stack. Every image is multi-stage,
//! runs as a non-root user, exposes the detected port and carries a
//! HEALTHCHECK on the configured health path, so the maturity axes the
//! containerization score looks at are all satisfied by what we emit.

use super::{GenInput, Stack, artifact};
use crate::domain::model::ArtifactData;

pub fn generate(input: &GenInput) -> Vec<ArtifactData> {
    let Some(dockerfile) = dockerfile(input) else {
        return Vec::new();
    };
    vec![
        artifact(
            input,
            &input.in_app("Dockerfile"),
            "dockerfile",
            format!(
                "Multi-stage {} image for {} that runs as a non-root user on port {} with a healthcheck on {}.",
                input.stack.label(),
                input.repo_name,
                input.port,
                input.health_path
            ),
            dockerfile,
        ),
        artifact(
            input,
            &input.in_app(".dockerignore"),
            "text",
            "Keeps build output, dependencies and secrets out of the image build context.",
            dockerignore(input),
        ),
    ]
}

/// `None` when the stack is unknown: a guessed Dockerfile would not build.
pub fn dockerfile(input: &GenInput) -> Option<String> {
    let body = match input.stack {
        Stack::Node => node(input),
        Stack::Rust => rust(input),
        Stack::Python => python(input),
        Stack::Go => go(input),
        Stack::Java => java(input),
        Stack::DotNet => dotnet(input),
        Stack::Php => php(input),
        Stack::Ruby => ruby(input),
        Stack::Static => static_site(input),
        Stack::Unknown => return None,
    };
    Some(format!("# syntax=docker/dockerfile:1\n{body}"))
}

fn healthcheck(input: &GenInput, tool: &str) -> String {
    let probe = match tool {
        "curl" => format!(
            "curl -fsS http://127.0.0.1:{}{} || exit 1",
            input.port, input.health_path
        ),
        _ => format!(
            "wget -qO- http://127.0.0.1:{}{} || exit 1",
            input.port, input.health_path
        ),
    };
    format!("HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 CMD {probe}")
}

fn node(input: &GenInput) -> String {
    let v = &input.runtime_version;
    // One install, then prune: the runtime copies the build stage's whole
    // tree (plain-JS projects need their source), so devDependencies have to
    // leave node_modules there rather than in a stage nothing copies from.
    let (lock, install, prune, run) = match input.package_manager.as_str() {
        "pnpm" => (
            "pnpm-lock.yaml",
            "corepack enable && pnpm install --frozen-lockfile",
            "pnpm prune --prod",
            "pnpm",
        ),
        "yarn" => (
            "yarn.lock",
            "corepack enable && yarn install --frozen-lockfile",
            "yarn install --frozen-lockfile --production --ignore-scripts",
            "yarn",
        ),
        "bun" => (
            "bun.lockb",
            "bun install --frozen-lockfile",
            "rm -rf node_modules && bun install --frozen-lockfile --production",
            "bun",
        ),
        _ => ("package-lock.json", "npm ci", "npm prune --omit=dev", "npm"),
    };
    // Bun ships its own runtime image; the Node version does not apply to it.
    let base = if input.package_manager == "bun" {
        "oven/bun:1-alpine".to_owned()
    } else {
        format!("node:{v}-alpine")
    };
    // The official node images ship corepack disabled and bundle only yarn
    // 1.x, so a runtime stage that starts with `pnpm` has to install it;
    // the CI workflow pins the same major.
    let package_manager = if input.package_manager == "pnpm" {
        format!(
            "RUN npm install -g pnpm@{} && npm cache clean --force\n",
            super::PNPM_MAJOR
        )
    } else {
        String::new()
    };
    let start = match (input.package_manager.as_str(), input.framework.as_str()) {
        ("bun", _) => "CMD [\"bun\", \"run\", \"start\"]".to_owned(),
        (_, "nextjs") => format!("CMD [\"{run}\", \"run\", \"start\"]"),
        _ => format!("CMD [\"{run}\", \"start\"]"),
    };
    format!(
        "FROM {base} AS build
WORKDIR /app
COPY package.json {lock} ./
RUN {install}
COPY . .
RUN {run} run build --if-present
RUN {prune}

FROM {base} AS runtime
ENV NODE_ENV=production
ENV PORT={port}
WORKDIR /app
{package_manager}RUN addgroup -S app && adduser -S app -G app
COPY --from=build --chown=app:app /app ./
USER app
EXPOSE {port}
{health}
{start}
",
        port = input.port,
        health = healthcheck(input, "wget"),
    )
}

fn rust(input: &GenInput) -> String {
    // The binary is named after the crate, which in a monorepo is not the
    // repository: view-engine builds `view-engine-api`.
    let bin = &input.app_name;
    format!(
        "FROM rust:{v}-slim AS chef
RUN cargo install cargo-chef --locked
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS build
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release --locked

FROM debian:bookworm-slim AS runtime
RUN apt-get update \\
    && apt-get install -y --no-install-recommends ca-certificates curl \\
    && rm -rf /var/lib/apt/lists/* \\
    && useradd --system --uid 10001 --create-home app
WORKDIR /app
COPY --from=build /app/target/release/{bin} /usr/local/bin/{bin}
USER app
ENV PORT={port}
EXPOSE {port}
{health}
CMD [\"{bin}\"]
",
        v = input.runtime_version,
        port = input.port,
        health = healthcheck(input, "curl"),
    )
}

fn python_entrypoint(input: &GenInput) -> String {
    let module = input.image.replace('-', "_");
    match input.framework.as_str() {
        "fastapi" => format!(
            "CMD [\"uvicorn\", \"main:app\", \"--host\", \"0.0.0.0\", \"--port\", \"{}\"]",
            input.port
        ),
        "django" => format!(
            "CMD [\"gunicorn\", \"{module}.wsgi:application\", \"--bind\", \"0.0.0.0:{}\", \"--workers\", \"2\"]",
            input.port
        ),
        "flask" => format!(
            "CMD [\"gunicorn\", \"app:app\", \"--bind\", \"0.0.0.0:{}\", \"--workers\", \"2\"]",
            input.port
        ),
        _ => "CMD [\"python\", \"main.py\"]".to_owned(),
    }
}

fn python(input: &GenInput) -> String {
    let v = &input.runtime_version;
    let (manifest, install) = match input.package_manager.as_str() {
        "uv" => (
            "pyproject.toml uv.lock",
            "COPY --from=ghcr.io/astral-sh/uv:0.5 /uv /usr/local/bin/uv
ENV UV_COMPILE_BYTECODE=1 UV_LINK_MODE=copy UV_PROJECT_ENVIRONMENT=/opt/venv
RUN uv sync --frozen --no-dev --no-install-project
COPY . .
RUN uv sync --frozen --no-dev",
        ),
        "poetry" => (
            "pyproject.toml poetry.lock",
            "RUN pip install --no-cache-dir poetry==1.8.3 \\
    && python -m venv /opt/venv \\
    && . /opt/venv/bin/activate \\
    && poetry install --only main --no-root --no-interaction
COPY . .",
        ),
        _ => (
            "requirements.txt",
            "RUN python -m venv /opt/venv \\
    && /opt/venv/bin/pip install --no-cache-dir --upgrade pip \\
    && /opt/venv/bin/pip install --no-cache-dir -r requirements.txt
COPY . .",
        ),
    };
    format!(
        "FROM python:{v}-slim AS build
ENV PYTHONDONTWRITEBYTECODE=1 PYTHONUNBUFFERED=1
WORKDIR /app
COPY {manifest} ./
{install}

FROM python:{v}-slim AS runtime
ENV PYTHONDONTWRITEBYTECODE=1 PYTHONUNBUFFERED=1 PATH=\"/opt/venv/bin:$PATH\"
ENV PORT={port}
RUN apt-get update \\
    && apt-get install -y --no-install-recommends curl \\
    && rm -rf /var/lib/apt/lists/* \\
    && useradd --system --uid 10001 --create-home app
WORKDIR /app
COPY --from=build /opt/venv /opt/venv
COPY --from=build --chown=app:app /app /app
USER app
EXPOSE {port}
{health}
{entry}
",
        port = input.port,
        health = healthcheck(input, "curl"),
        entry = python_entrypoint(input),
    )
}

fn go(input: &GenInput) -> String {
    let bin = &input.image;
    format!(
        "FROM golang:{v}-alpine AS build
WORKDIR /src
COPY go.mod go.sum ./
RUN go mod download
COPY . .
RUN CGO_ENABLED=0 GOOS=linux go build -trimpath -ldflags=\"-s -w\" -o /out/{bin} .

FROM alpine:3.20 AS runtime
RUN apk add --no-cache ca-certificates wget \\
    && addgroup -S app && adduser -S app -G app
COPY --from=build /out/{bin} /usr/local/bin/{bin}
USER app
ENV PORT={port}
EXPOSE {port}
{health}
ENTRYPOINT [\"/usr/local/bin/{bin}\"]
",
        v = input.runtime_version,
        port = input.port,
        health = healthcheck(input, "wget"),
    )
}

fn java(input: &GenInput) -> String {
    let v = &input.runtime_version;
    let (build_image, prepare, build, jar) = if input.package_manager == "gradle" {
        (
            format!("gradle:8-jdk{v}"),
            "COPY build.gradle* settings.gradle* gradle.properties* ./\nRUN gradle dependencies --no-daemon -q || true",
            "RUN gradle build -x test --no-daemon -q",
            "build/libs/*.jar",
        )
    } else {
        (
            format!("maven:3.9-eclipse-temurin-{v}"),
            "COPY pom.xml ./\nRUN mvn -B -q dependency:go-offline",
            "RUN mvn -B -q package -DskipTests",
            "target/*.jar",
        )
    };
    format!(
        "FROM {build_image} AS build
WORKDIR /app
{prepare}
COPY src ./src
{build}

FROM eclipse-temurin:{v}-jre-alpine AS runtime
RUN apk add --no-cache wget && addgroup -S app && adduser -S app -G app
WORKDIR /app
COPY --from=build /app/{jar} app.jar
USER app
ENV PORT={port}
ENV JAVA_TOOL_OPTIONS=\"-XX:MaxRAMPercentage=75 -Dserver.port={port}\"
EXPOSE {port}
{health}
ENTRYPOINT [\"java\", \"-jar\", \"app.jar\"]
",
        port = input.port,
        health = healthcheck(input, "wget"),
    )
}

fn dotnet(input: &GenInput) -> String {
    let dll = input
        .repo_name
        .split(['-', '_'])
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<String>();
    format!(
        "FROM mcr.microsoft.com/dotnet/sdk:{v} AS build
WORKDIR /src
COPY . .
RUN dotnet restore \\
    && dotnet publish -c Release -o /app/publish --no-restore

FROM mcr.microsoft.com/dotnet/aspnet:{v} AS runtime
WORKDIR /app
ENV ASPNETCORE_URLS=http://+:{port}
ENV PORT={port}
RUN apt-get update \\
    && apt-get install -y --no-install-recommends curl \\
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /app/publish .
USER app
EXPOSE {port}
{health}
ENTRYPOINT [\"dotnet\", \"{dll}.dll\"]
",
        v = input.runtime_version,
        port = input.port,
        health = healthcheck(input, "curl"),
    )
}

fn php(input: &GenInput) -> String {
    let (packages, extensions) = if input.has_datastore("postgres") {
        ("postgresql-dev", "pdo pdo_pgsql opcache")
    } else {
        ("", "pdo pdo_mysql opcache")
    };
    let apk = if packages.is_empty() {
        String::new()
    } else {
        format!("RUN apk add --no-cache {packages}\n")
    };
    format!(
        "FROM composer:2 AS vendor
WORKDIR /app
COPY composer.json composer.lock ./
RUN composer install --no-dev --no-scripts --prefer-dist --optimize-autoloader --no-interaction

FROM php:{v}-fpm-alpine AS runtime
{apk}RUN docker-php-ext-install {extensions} \\
    && mv \"$PHP_INI_DIR/php.ini-production\" \"$PHP_INI_DIR/php.ini\"
WORKDIR /var/www/html
COPY --from=vendor /app/vendor ./vendor
COPY --chown=www-data:www-data . .
USER www-data
EXPOSE {port}
HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 CMD php-fpm -t || exit 1
CMD [\"php-fpm\"]
",
        v = input.runtime_version,
        port = input.port,
    )
}

fn ruby(input: &GenInput) -> String {
    let (build_libs, run_libs) = if input.has_datastore("mysql") {
        ("default-libmysqlclient-dev", "libmariadb3")
    } else {
        ("libpq-dev", "libpq5")
    };
    let start = if input.framework == "rails" {
        "CMD [\"bundle\", \"exec\", \"puma\", \"-C\", \"config/puma.rb\"]"
    } else {
        "CMD [\"bundle\", \"exec\", \"rackup\", \"--host\", \"0.0.0.0\", \"--port\", \"3000\"]"
    };
    format!(
        "FROM ruby:{v}-slim AS build
RUN apt-get update \\
    && apt-get install -y --no-install-recommends build-essential git {build_libs} \\
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY Gemfile Gemfile.lock ./
RUN bundle config set --local deployment true \\
    && bundle config set --local without 'development test' \\
    && bundle install --jobs 4
COPY . .

FROM ruby:{v}-slim AS runtime
RUN apt-get update \\
    && apt-get install -y --no-install-recommends curl {run_libs} \\
    && rm -rf /var/lib/apt/lists/* \\
    && useradd --system --uid 10001 --create-home app
WORKDIR /app
ENV RACK_ENV=production RAILS_ENV=production PORT={port}
COPY --from=build /usr/local/bundle /usr/local/bundle
COPY --from=build --chown=app:app /app /app
USER app
EXPOSE {port}
{health}
{start}
",
        v = input.runtime_version,
        port = input.port,
        health = healthcheck(input, "curl"),
    )
}

fn static_site(input: &GenInput) -> String {
    let build_stage = if input.package_manager.is_empty() {
        String::new()
    } else {
        let (lock, install, run) = match input.package_manager.as_str() {
            "pnpm" => (
                "pnpm-lock.yaml",
                "corepack enable && pnpm install --frozen-lockfile",
                "pnpm",
            ),
            "yarn" => (
                "yarn.lock",
                "corepack enable && yarn install --frozen-lockfile",
                "yarn",
            ),
            "bun" => ("bun.lockb", "bun install --frozen-lockfile", "bun"),
            _ => ("package-lock.json", "npm ci", "npm"),
        };
        format!(
            "FROM node:{v}-alpine AS build
WORKDIR /app
COPY package.json {lock} ./
RUN {install}
COPY . .
RUN {run} run build

",
            v = input.runtime_version
        )
    };
    let copy = if input.package_manager.is_empty() {
        "COPY . /usr/share/nginx/html".to_owned()
    } else {
        format!(
            "COPY --from=build /app/{} /usr/share/nginx/html",
            input.static_dir
        )
    };
    format!(
        "{build_stage}FROM nginxinc/nginx-unprivileged:1.27-alpine AS runtime
COPY deploy/nginx/default.conf /etc/nginx/conf.d/default.conf
{copy}
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 CMD wget -qO- http://127.0.0.1:8080/ || exit 1
",
    )
}

fn dockerignore(input: &GenInput) -> String {
    let mut lines: Vec<&str> = vec![
        ".git",
        ".github",
        ".env",
        ".env.*",
        "!.env.example",
        "*.md",
        "docs/",
        "Dockerfile",
        ".dockerignore",
    ];
    lines.extend(match input.stack {
        Stack::Node | Stack::Static => vec![
            "node_modules",
            "dist",
            ".next",
            "coverage",
            "npm-debug.log*",
        ],
        Stack::Rust => vec!["target"],
        Stack::Python => vec![
            ".venv",
            "__pycache__",
            "*.pyc",
            ".pytest_cache",
            ".mypy_cache",
        ],
        Stack::Go => vec!["bin/", "vendor/"],
        Stack::Java => vec!["target", "build", ".gradle", "*.iml"],
        Stack::DotNet => vec!["bin/", "obj/"],
        Stack::Php => vec!["vendor", "storage/logs"],
        Stack::Ruby => vec!["tmp/", "log/", "vendor/bundle"],
        Stack::Unknown => vec![],
    });
    lines.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::super::{input::fixtures, snapshot::assert_snapshot};
    use super::*;

    #[test]
    fn node_pnpm_dockerfile_snapshot() {
        let files = generate(&fixtures::input(&fixtures::node_context(), ""));
        assert_eq!(files.len(), 2);
        assert_snapshot("dockerfile_node_pnpm", &files[0]);
        assert!(
            files[0].replaces_existing,
            "the fixture already has a Dockerfile"
        );
        assert!(files[0].content.contains("node:18-alpine"));
        assert!(files[0].content.contains("USER app"));
        assert!(files[0].content.contains("/healthz"));
        assert!(files[1].content.contains("node_modules"));
        // The runtime stage must be able to run `pnpm start`, and must not
        // ship devDependencies: the build stage prunes before the copy.
        let runtime = files[0].content.split("AS runtime").nth(1).unwrap();
        assert!(
            runtime.contains(&format!("npm install -g pnpm@{}", super::super::PNPM_MAJOR)),
            "{runtime}"
        );
        assert!(runtime.contains("CMD [\"pnpm\", \"start\"]"));
        assert!(files[0].content.contains("RUN pnpm prune --prod"));
        assert!(!files[0].content.contains("AS deps"));
    }

    #[test]
    fn rust_uses_cargo_chef_and_python_uses_uv() {
        let rust = generate(&fixtures::input(&fixtures::rust_context(), ""));
        assert_snapshot("dockerfile_rust", &rust[0]);
        assert!(rust[0].content.contains("cargo chef cook --release"));
        assert!(rust[0].content.contains("/usr/local/bin/texc-symphony"));
        let py = generate(&fixtures::input(&fixtures::python_context(), ""));
        assert!(py[0].content.contains("uv sync --frozen --no-dev"));
        assert!(py[0].content.contains("uvicorn"));
        assert!(py[0].content.contains("\"8000\""));
    }

    #[test]
    fn every_stack_builds_a_complete_non_root_image() {
        let base = fixtures::input(&fixtures::rust_context(), "");
        for (stack, pm) in [
            (Stack::Node, "npm"),
            (Stack::Node, "yarn"),
            (Stack::Node, "bun"),
            (Stack::Python, "poetry"),
            (Stack::Python, "pip"),
            (Stack::Go, "go"),
            (Stack::Java, "maven"),
            (Stack::Java, "gradle"),
            (Stack::DotNet, "dotnet"),
            (Stack::Php, "composer"),
            (Stack::Ruby, "bundler"),
            (Stack::Static, ""),
            (Stack::Static, "npm"),
        ] {
            let mut input = base.clone();
            input.stack = stack;
            input.package_manager = pm.to_owned();
            input.runtime_version = stack.default_runtime_version().to_owned();
            let text = dockerfile(&input).unwrap();
            assert!(
                text.contains("HEALTHCHECK"),
                "{stack:?}/{pm} lacks a healthcheck"
            );
            assert!(text.contains("EXPOSE"), "{stack:?}/{pm} lacks EXPOSE");
            assert!(
                text.contains("USER ") || text.contains("nginx-unprivileged"),
                "{stack:?}/{pm} runs as root"
            );
            assert!(
                text.matches("FROM ").count() >= 2 || stack == Stack::Static,
                "{stack:?} is single-stage"
            );
            assert!(!text.contains("TODO"));
        }
        let mut unknown = base;
        unknown.stack = Stack::Unknown;
        assert!(generate(&unknown).is_empty());
    }
}

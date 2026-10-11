//! Call and data evidence found in the sampled key files with plain
//! regexes: imports between workspace packages, HTTP client base URLs that
//! name a sibling service, and SQL/ORM/driver usage that names a datastore.
//! Every hit cites the file it was found in (the caller's file).

use std::sync::LazyLock;

use regex::Regex;
use sdi_proto::knowledge::v1::{GraphNode, NodeKind};

use super::{
    catalog::{Catalog, capitalise, node_id},
    imports::{is_generated, is_source},
    model::{DiagramEdge, style},
};

/// One relationship read from a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// Diagram node ids (`node_id(key)`).
    pub source: String,
    pub target: String,
    pub label: String,
    pub style: String,
    pub evidence_path: String,
}

impl Found {
    pub fn edge(&self) -> DiagramEdge {
        DiagramEdge {
            id: String::new(),
            source: self.source.clone(),
            target: self.target.clone(),
            label: self.label.clone(),
            style: self.style.clone(),
            dashed: false,
            evidence_path: self.evidence_path.clone(),
        }
    }
}

static URL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"https?://([A-Za-z0-9_.-]+)(?::(\d{2,5}))?").expect("url regex"));
static ENV_URL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b([A-Z][A-Z0-9]*(?:_[A-Z0-9]+)*)_(?:URL|BASE_URL|ENDPOINT|HOST|ADDR)\b")
        .expect("env url regex")
});
static RELATIVE_API: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"fetch\(\s*[`'"]/api|axios|baseURL|VITE_API|REACT_APP_API|NEXT_PUBLIC_API|createTRPC|useQuery\("#)
        .expect("relative api regex")
});
pub static SQL_WRITE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(INSERT\s+INTO|UPDATE\s+\w+\s+SET|DELETE\s+FROM|\.create\(|\.save\(|\.insert\(|\.upsert\(|\.execute\()")
        .expect("sql write regex")
});
pub static SQL_READ: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(SELECT\s|\.find\w*\(|\.query\(|\.fetch_(one|all|optional)\(|\.get\(|\.filter\(|\.first\()")
        .expect("sql read regex")
});
static ROUTE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?x)
        \.route\(\s*"(/[^"\s]*)"(?:\s*,\s*(get|post|put|patch|delete)\()?   # axum/warp
        | @(?:app|router|api|bp)\.(get|post|put|patch|delete)\(\s*['"](/[^'"\s]*)['"]   # fastapi/flask
        | \b(?:app|router|server|api)\.(get|post|put|patch|delete)\(\s*['"`](/[^'"`\s]*)['"`]  # express/fastify/hono
        | path\(\s*['"]([^'"\s]*)['"]                               # django urls
        | \.(GET|POST|PUT|PATCH|DELETE)\(\s*"(/[^"\s]*)"            # gin/echo
        "#,
    )
    .expect("route regex")
});

/// Driver, client and ORM spellings and the engine they talk to. `sql` is
/// an engine-agnostic ORM: it resolves to the project's only SQL datastore.
const DATASTORE_PATTERNS: [(&str, &str); 16] = [
    (
        r"\b(asyncpg|psycopg2?|pg\.Pool|new Pool\(|sqlx::postgres|PgPool|postgres://|postgresql://|tokio_postgres|node-postgres|require\(['\x22]pg['\x22]\)|from ['\x22]pg['\x22])",
        "postgres",
    ),
    (r"\b(mysql2?|pymysql|aiomysql|MySqlPool|mysql://)", "mysql"),
    (
        r"\b(ioredis|aioredis|redis\.(Redis|createClient|from_url|asyncio)|redis::Client|redis://|from redis|require\(['\x22]redis['\x22]\)|from ['\x22]redis['\x22])",
        "redis",
    ),
    (
        r"\b(mongoose|pymongo|motor\.|MongoClient|mongodb://|mongodb::Client)",
        "mongodb",
    ),
    (
        r"\b(sqlite3|rusqlite|SqlitePool|better-sqlite3|sqlite://)",
        "sqlite",
    ),
    (
        r"\b(kafkajs|rdkafka|aiokafka|confluent_kafka|KafkaProducer|KafkaConsumer)",
        "kafka",
    ),
    (r"\b(amqplib|pika\.|aio_pika|lapin|amqp://)", "rabbitmq"),
    (r"\b(nats\.connect|async_nats|nats://|from nats)", "nats"),
    (
        r"\b(elasticsearch|@elastic/elasticsearch|opensearch)",
        "elasticsearch",
    ),
    (
        r"\b(boto3\.client\(['\x22]dynamodb|DynamoDBClient|aws_sdk_dynamodb)",
        "dynamodb",
    ),
    (r"\b(boto3\.client\(['\x22]s3|S3Client|aws_sdk_s3)", "s3"),
    (
        r"\b(boto3\.client\(['\x22]sqs|SQSClient|aws_sdk_sqs)",
        "sqs",
    ),
    (
        r"\b(PrismaClient|from ['\x22]@prisma/client['\x22]|knex\(|new Sequelize|typeorm|drizzle\(|drizzle-orm)",
        "sql",
    ),
    (
        r"\b(sqlalchemy|SQLAlchemy|create_engine\(|declarative_base|from django\.db|models\.Model)",
        "sql",
    ),
    (
        r"\b(diesel::|sea_orm|sqlx::query|sqlx::Pool|sqlx::query_as|sqlx::migrate)",
        "sql",
    ),
    (r"\b(gorm\.Open|database/sql|sqlx\.Connect)", "sql"),
];

static DATASTORES: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    DATASTORE_PATTERNS
        .iter()
        .map(|(pattern, engine)| (Regex::new(pattern).expect("datastore regex"), *engine))
        .collect()
});

/// Everything the excerpts show: `(path, text, owner)` triples come from the catalog.
pub fn detect(catalog: &Catalog<'_>) -> Vec<Found> {
    let mut out = Vec::new();
    for (path, text, owner) in catalog.excerpts() {
        out.extend(detect_in(catalog, path, text, owner));
    }
    out.dedup_by(|a, b| a.source == b.source && a.target == b.target && a.label == b.label);
    out
}

pub fn detect_in(catalog: &Catalog<'_>, path: &str, text: &str, owner: &GraphNode) -> Vec<Found> {
    let mut out = Vec::new();
    let source = node_id(&owner.key);
    let push = |out: &mut Vec<Found>, target: &GraphNode, label: &str, style: &str| {
        if target.key == owner.key {
            return;
        }
        let found = Found {
            source: source.clone(),
            target: node_id(&target.key),
            label: label.to_owned(),
            style: style.to_owned(),
            evidence_path: path.to_owned(),
        };
        if !out.contains(&found) {
            out.push(found);
        }
    };

    // 1. Imports of sibling packages.
    for sibling in catalog.nodes().filter(|n| Catalog::is_repository_node(n)) {
        if sibling.key == owner.key {
            continue;
        }
        if imports(text, &sibling.label) {
            push(&mut out, sibling, "imports", style::CALL);
        }
    }

    // 2. HTTP clients pointed at a sibling service, by host name or env var.
    let services: Vec<&GraphNode> = catalog
        .nodes()
        .filter(|n| n.kind() == NodeKind::Service && n.key != owner.key)
        .collect();
    for cap in URL.captures_iter(text) {
        let host = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        let port = cap.get(2).map(|m| m.as_str()).unwrap_or("");
        if let Some(target) = services.iter().find(|s| {
            same_name(&s.label, host) || (is_local_host(host) && exposes_port(catalog, s, port))
        }) {
            push(&mut out, target, "calls", style::CALL);
        }
    }
    for cap in ENV_URL.captures_iter(text) {
        let prefix = cap
            .get(1)
            .map(|m| m.as_str().to_lowercase())
            .unwrap_or_default();
        let stem = prefix
            .trim_end_matches("_service")
            .trim_end_matches("_api")
            .trim_end_matches("_server");
        if let Some(target) = services
            .iter()
            .find(|s| same_name(&s.label, &prefix) || same_name(&s.label, stem))
        {
            push(&mut out, target, "calls", style::CALL);
        }
    }
    // A UI talking to "/api" names the only backend there is.
    if catalog.is_ui(owner) && RELATIVE_API.is_match(text) {
        let backends: Vec<&GraphNode> = services
            .iter()
            .copied()
            .filter(|s| catalog.is_backend_service(s))
            .collect();
        if let [only] = backends.as_slice() {
            push(&mut out, only, "calls", style::CALL);
        }
    }

    // 3. Datastore drivers and ORMs.
    let datastores: Vec<&GraphNode> = catalog.of_kind(NodeKind::Datastore).collect();
    let sql_stores: Vec<&GraphNode> = datastores
        .iter()
        .copied()
        .filter(|d| {
            matches!(
                Catalog::engine_of(d).as_str(),
                "postgres" | "postgresql" | "mysql" | "mariadb" | "sqlite"
            )
        })
        .collect();
    for (regex, engine) in DATASTORES.iter() {
        if !regex.is_match(text) {
            continue;
        }
        let target = if *engine == "sql" {
            match sql_stores.as_slice() {
                [only] => Some(*only),
                _ => None,
            }
        } else {
            datastores
                .iter()
                .copied()
                .find(|d| engine_matches(&Catalog::engine_of(d), engine))
        };
        if let Some(target) = target {
            if Catalog::is_queue(target) {
                push(&mut out, target, "publishes", style::ASYNC);
            } else {
                push(&mut out, target, data_verb(text), style::DATA);
            }
        }
    }
    out
}

/// "reads", "writes" or "reads/writes" from the statements in the file.
pub fn data_verb(text: &str) -> &'static str {
    match (SQL_READ.is_match(text), SQL_WRITE.is_match(text)) {
        (true, false) => "reads",
        (false, true) => "writes",
        _ => "reads/writes",
    }
}

/// Route paths declared in a file, in order of appearance (at most `limit`).
pub fn routes_in(text: &str, limit: usize) -> Vec<String> {
    let mut out = Vec::new();
    for cap in ROUTE.captures_iter(text) {
        let route = (1..=9)
            .filter_map(|i| cap.get(i))
            .map(|m| m.as_str())
            .find(|s| s.starts_with('/'))
            .unwrap_or("");
        let verb = (1..=9)
            .filter_map(|i| cap.get(i))
            .map(|m| m.as_str())
            .find(|s| !s.starts_with('/'))
            .map(|v| v.to_uppercase())
            .unwrap_or_default();
        if route.is_empty() {
            continue;
        }
        let label = if verb.is_empty() {
            route.to_owned()
        } else {
            format!("{verb} {route}")
        };
        if !out.contains(&label) {
            out.push(label);
        }
        if out.len() >= limit {
            break;
        }
    }
    out
}

/// An external system the sampled code reaches over HTTP.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalHost {
    /// Diagram node id (`external_linear`).
    pub id: String,
    pub label: String,
    pub host: String,
    /// `(file, host as written)` for every sampled file that names it.
    pub callers: Vec<(String, String)>,
}

/// Well-known API hosts and the systems they stand for.
const KNOWN_HOSTS: [(&str, &str); 22] = [
    ("api.linear.app", "Linear"),
    ("api.github.com", "GitHub"),
    ("gitlab.com", "GitLab"),
    ("atlassian.net", "Jira"),
    ("app.asana.com", "Asana"),
    ("api.openai.com", "OpenAI"),
    ("api.anthropic.com", "Anthropic"),
    ("api.stripe.com", "Stripe"),
    ("hooks.slack.com", "Slack"),
    ("slack.com", "Slack"),
    ("api.twilio.com", "Twilio"),
    ("api.sendgrid.com", "SendGrid"),
    ("sentry.io", "Sentry"),
    ("amazonaws.com", "AWS"),
    ("googleapis.com", "Google APIs"),
    ("api.notion.com", "Notion"),
    ("api.pagerduty.com", "PagerDuty"),
    ("api.datadoghq.com", "Datadog"),
    ("discord.com", "Discord"),
    ("api.telegram.org", "Telegram"),
    ("huggingface.co", "Hugging Face"),
    ("api.mapbox.com", "Mapbox"),
];

/// Hosts that are documentation, schema or package registries, not systems
/// the product calls.
const NOISE_HOSTS: [&str; 14] = [
    "github.com",
    "www.github.com",
    "docs.rs",
    "crates.io",
    "npmjs.com",
    "www.npmjs.com",
    "pypi.org",
    "www.w3.org",
    "json-schema.org",
    "schemas.microsoft.com",
    "example.com",
    "www.example.com",
    "localhost",
    "developer.mozilla.org",
];

/// External systems named by URLs in the sampled source files: well-known
/// API hosts, or `api.<name>.<tld>`-style hosts, that are not a sibling
/// service of the product. Deterministic order by label.
pub fn external_hosts(catalog: &Catalog<'_>) -> Vec<ExternalHost> {
    let services: Vec<&GraphNode> = catalog.of_kind(NodeKind::Service).collect();
    let known_externals: Vec<String> = catalog
        .of_kind(NodeKind::ExternalApi)
        .map(|n| n.label.to_lowercase())
        .collect();
    let mut hosts: std::collections::BTreeMap<String, ExternalHost> =
        std::collections::BTreeMap::new();
    let mut paths: Vec<&String> = catalog.input.files.keys().collect();
    paths.sort();
    for path in paths {
        if !is_source(path) || Catalog::is_excluded_path(path) || is_generated(path) {
            continue;
        }
        let text = &catalog.input.files[path];
        for cap in URL.captures_iter(text) {
            let host = cap.get(1).map(|m| m.as_str()).unwrap_or("").to_lowercase();
            if host.is_empty()
                || is_local_host(&host)
                || NOISE_HOSTS.contains(&host.as_str())
                || host.parse::<std::net::IpAddr>().is_ok()
                || services.iter().any(|s| same_name(&s.label, &host))
            {
                continue;
            }
            let Some(label) = external_label(&host) else {
                continue;
            };
            if known_externals.iter().any(|k| k == &label.to_lowercase()) {
                // The graph already has it (from a dependency); let that node stand.
                continue;
            }
            let id = format!("external_{}", node_id(&label));
            let entry = hosts.entry(id.clone()).or_insert_with(|| ExternalHost {
                id,
                label: label.clone(),
                host: host.clone(),
                callers: Vec::new(),
            });
            if !entry.callers.iter().any(|(p, _)| p == path) {
                entry.callers.push((path.clone(), host.clone()));
            }
        }
    }
    hosts.into_values().collect()
}

/// "Linear" for `api.linear.app`, "Acme API" for `api.acme.io`; `None`
/// for hosts that look like nothing in particular.
pub fn external_label(host: &str) -> Option<String> {
    for (known, label) in KNOWN_HOSTS {
        if host == known || host.ends_with(&format!(".{known}")) {
            return Some((*label).to_owned());
        }
    }
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() < 2 {
        return None;
    }
    let api_like = matches!(parts[0], "api" | "apis" | "gateway" | "hooks" | "events")
        || host.contains("api.");
    if !api_like {
        return None;
    }
    let name = parts
        .iter()
        .rev()
        .skip(1)
        .find(|p| {
            !matches!(
                **p,
                "api"
                    | "apis"
                    | "co"
                    | "com"
                    | "org"
                    | "net"
                    | "io"
                    | "dev"
                    | "app"
                    | "gateway"
                    | "hooks"
                    | "events"
            )
        })
        .copied()
        .unwrap_or(parts[parts.len() - 2]);
    Some(format!("{} API", capitalise(name)))
}

fn imports(text: &str, name: &str) -> bool {
    let name = name.trim().to_lowercase();
    if name.len() < 2 {
        return false;
    }
    let underscored = regex::escape(&name.replace('-', "_"));
    let dashed = regex::escape(&name.replace('_', "-"));
    let pattern = format!(
        r#"(?m)^\s*(?:pub\s+)?(?:use|extern crate)\s+{underscored}\b|from\s+['"](?:@[\w.-]+/)?(?:{dashed}|{underscored})['"]|require\(\s*['"](?:@[\w.-]+/)?(?:{dashed}|{underscored})['"]|^\s*(?:from|import)\s+{underscored}\b"#
    );
    Regex::new(&pattern)
        .map(|r| r.is_match(text))
        .unwrap_or(false)
}

fn same_name(label: &str, host: &str) -> bool {
    let a = label.to_lowercase().replace('-', "_");
    let b = host.to_lowercase().replace('-', "_");
    !b.is_empty() && (a == b || a == format!("{b}_service") || b == format!("{a}_service"))
}

fn is_local_host(host: &str) -> bool {
    matches!(
        host,
        "localhost" | "127.0.0.1" | "0.0.0.0" | "host.docker.internal"
    )
}

fn exposes_port(catalog: &Catalog<'_>, service: &GraphNode, port: &str) -> bool {
    if port.is_empty() {
        return false;
    }
    catalog.edges().any(|e| {
        e.relation == "PACKAGED_AS"
            && e.source_id == service.id
            && catalog
                .node_by_id(&e.target_id)
                .and_then(|image| image.attributes.get("ports"))
                .is_some_and(|ports| ports.split(',').any(|p| p.trim() == port))
    })
}

fn engine_matches(engine: &str, wanted: &str) -> bool {
    match wanted {
        "postgres" => matches!(engine, "postgres" | "postgresql"),
        "mongodb" => matches!(engine, "mongodb" | "mongo"),
        "elasticsearch" => matches!(engine, "elasticsearch" | "opensearch"),
        other => engine == other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_match_crate_and_package_spellings() {
        assert!(imports("use sdi_common::db;\n", "sdi-common"));
        assert!(imports("import { x } from '@view/shared';", "shared"));
        assert!(imports("const s = require(\"shared-lib\")", "shared_lib"));
        assert!(imports("from quant_core import price", "quant-core"));
        assert!(!imports("use sdi_commonly::x", "sdi-common"));
        assert!(!imports("", "a"));
    }

    #[test]
    fn external_labels_come_from_known_hosts_or_api_subdomains() {
        assert_eq!(external_label("api.linear.app").as_deref(), Some("Linear"));
        assert_eq!(
            external_label("acme.atlassian.net").as_deref(),
            Some("Jira")
        );
        assert_eq!(external_label("api.acme.io").as_deref(), Some("Acme API"));
        assert_eq!(external_label("acme.io"), None);
        assert_eq!(external_label("localhost"), None);
    }

    #[test]
    fn data_verbs_follow_the_statements() {
        assert_eq!(data_verb("SELECT * FROM users"), "reads");
        assert_eq!(data_verb("INSERT INTO users VALUES (1)"), "writes");
        assert_eq!(data_verb("SELECT 1; INSERT INTO t"), "reads/writes");
        assert_eq!(data_verb("nothing"), "reads/writes");
    }

    #[test]
    fn routes_are_listed_with_their_verb() {
        let text = r#"
            app.get('/api/items', handler);
            router.post("/api/items", create);
            Router::new().route("/health", get(health))
            @app.get("/price")
        "#;
        assert_eq!(
            routes_in(text, 10),
            vec![
                "GET /api/items",
                "POST /api/items",
                "GET /health",
                "GET /price"
            ]
        );
        assert_eq!(routes_in(text, 1).len(), 1);
    }
}

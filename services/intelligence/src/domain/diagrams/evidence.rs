//! Call and data evidence found in the sampled key files with plain
//! regexes: imports between workspace packages, HTTP client base URLs that
//! name a sibling service, and SQL/ORM/driver usage that names a datastore.
//! Every hit cites the file it was found in (the caller's file).

use std::sync::LazyLock;

use regex::Regex;
use sdi_proto::knowledge::v1::{GraphNode, NodeKind};

use super::{
    catalog::{Catalog, node_id},
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
static SQL_WRITE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(INSERT\s+INTO|UPDATE\s+\w+\s+SET|DELETE\s+FROM|\.create\(|\.save\(|\.insert\(|\.upsert\(|\.execute\()")
        .expect("sql write regex")
});
static SQL_READ: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(SELECT\s|\.find\w*\(|\.query\(|\.fetch_(one|all|optional)\(|\.get\(|\.filter\(|\.first\()")
        .expect("sql read regex")
});
static ROUTE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?x)
        \.route\(\s*"(/[^"\s]*)"                                   # axum/warp
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
            vec!["GET /api/items", "POST /api/items", "/health", "GET /price"]
        );
        assert_eq!(routes_in(text, 1).len(), 1);
    }
}

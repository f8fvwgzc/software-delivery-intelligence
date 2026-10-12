//! `.sdi.yml` — the repository's own declarations (CONTRACT.md §12).
//!
//! Values here are *configured* facts: trusted like detected ones but shown
//! apart, and they answer the open questions they cover. The parser is
//! lenient where it is safe (an unknown key is reported and skipped) and
//! strict where a wrong guess would steer a deployment (a value of the
//! wrong type or outside the vocabulary makes the whole file invalid, so
//! nothing half-read is ever acted on).

use serde_yaml_ng::Value;

use crate::detectors::Ctx;
use crate::model::*;
use crate::util::line_with;
use crate::yaml;

/// The only file name the contract recognises.
pub const CONFIG_PATH: &str = ".sdi.yml";

const DEPLOY_TARGETS: &[&str] = &[
    "vm",
    "azure",
    "aws",
    "gcp",
    "kubernetes",
    "fly",
    "render",
    "none",
];

/// Parses `.sdi.yml`. `present` is always true for a file that exists; a
/// non-empty `errors` with cleared values means the file was ignored.
pub fn parse_config(content: &str) -> ProjectConfigDraft {
    let mut cfg = ProjectConfigDraft {
        present: true,
        path: CONFIG_PATH.into(),
        ..Default::default()
    };
    let mut warnings: Vec<String> = Vec::new();
    let mut fatal: Vec<String> = Vec::new();

    let doc = match serde_yaml_ng::from_str::<Value>(content) {
        Ok(v) => v,
        Err(e) => {
            cfg.errors
                .push(format!("error: {CONFIG_PATH} does not parse as YAML: {e}"));
            return cfg;
        }
    };
    if !doc.is_mapping() {
        cfg.errors.push(format!(
            "error: {CONFIG_PATH} must be a mapping with a `version` key"
        ));
        return cfg;
    }
    match yaml::get(&doc, "version") {
        Some(Value::Number(n)) if n.as_u64() == Some(1) => {}
        Some(Value::String(s)) if s.trim() == "1" => {}
        Some(other) => fatal.push(format!(
            "error: `version` must be 1, got {}",
            describe(other)
        )),
        None => fatal.push("error: `version: 1` is required".into()),
    }
    check_keys(
        &doc,
        "",
        &[
            "version", "deploy", "budget", "test", "scan", "review", "delivery",
        ],
        &mut warnings,
    );

    if let Some(deploy) = yaml::get(&doc, "deploy")
        && let Some(m) = require_mapping(deploy, "deploy", &mut fatal)
    {
        check_keys(
            m,
            "deploy.",
            &["target", "environments", "region"],
            &mut warnings,
        );
        if let Some(v) = yaml::get(m, "target") {
            match v {
                Value::String(s) => {
                    let t = s.trim().to_ascii_lowercase();
                    if DEPLOY_TARGETS.contains(&t.as_str()) {
                        cfg.deploy_target = t;
                    } else {
                        fatal.push(format!(
                            "error: `deploy.target` must be one of {}, got `{s}`",
                            DEPLOY_TARGETS.join(" | ")
                        ));
                    }
                }
                other => fatal.push(format!(
                    "error: `deploy.target` must be a string, got {}",
                    describe(other)
                )),
            }
        }
        if let Some(v) = yaml::get(m, "environments") {
            match string_list(v) {
                Some(list) => cfg.environments = list,
                None => fatal.push(format!(
                    "error: `deploy.environments` must be a list of names, got {}",
                    describe(v)
                )),
            }
        }
        if let Some(v) = yaml::get(m, "region") {
            match v {
                Value::String(s) => cfg.region = s.trim().to_string(),
                other => fatal.push(format!(
                    "error: `deploy.region` must be a string, got {}",
                    describe(other)
                )),
            }
        }
    }
    if let Some(budget) = yaml::get(&doc, "budget")
        && let Some(m) = require_mapping(budget, "budget", &mut fatal)
    {
        check_keys(m, "budget.", &["monthly_usd"], &mut warnings);
        if let Some(v) = yaml::get(m, "monthly_usd") {
            match v.as_f64() {
                Some(n) if n >= 0.0 && n.is_finite() => cfg.budget_monthly_usd = n,
                _ => fatal.push(format!(
                    "error: `budget.monthly_usd` must be a non-negative number, got {}",
                    describe(v)
                )),
            }
        }
    }
    if let Some(test) = yaml::get(&doc, "test")
        && let Some(m) = require_mapping(test, "test", &mut fatal)
    {
        check_keys(
            m,
            "test.",
            &["command", "health_path", "port"],
            &mut warnings,
        );
        for (key, slot) in [
            ("command", &mut cfg.test_command),
            ("health_path", &mut cfg.health_path),
        ] {
            if let Some(v) = yaml::get(m, key) {
                match v {
                    Value::String(s) => *slot = s.trim().to_string(),
                    other => fatal.push(format!(
                        "error: `test.{key}` must be a string, got {}",
                        describe(other)
                    )),
                }
            }
        }
        if let Some(v) = yaml::get(m, "port") {
            match v.as_u64() {
                Some(p) if (1..=65535).contains(&p) => cfg.port = p as u32,
                _ => fatal.push(format!(
                    "error: `test.port` must be a port number, got {}",
                    describe(v)
                )),
            }
        }
    }
    if let Some(scan) = yaml::get(&doc, "scan")
        && let Some(m) = require_mapping(scan, "scan", &mut fatal)
    {
        check_keys(m, "scan.", &["exclude"], &mut warnings);
        if let Some(v) = yaml::get(m, "exclude") {
            match string_list(v) {
                Some(list) => cfg.scan_exclude = list,
                None => fatal.push(format!(
                    "error: `scan.exclude` must be a list of globs, got {}",
                    describe(v)
                )),
            }
        }
    }

    if let Some(review) = yaml::get(&doc, "review")
        && let Some(m) = require_mapping(review, "review", &mut fatal)
    {
        cfg.review = parse_review(m, &mut warnings, &mut fatal);
    }

    if let Some(delivery) = yaml::get(&doc, "delivery")
        && let Some(m) = require_mapping(delivery, "delivery", &mut fatal)
    {
        let delivery = parse_delivery(m, &mut warnings, &mut fatal);
        if !delivery.is_empty() {
            cfg.delivery_json = delivery.to_json();
        }
    }

    if !fatal.is_empty() {
        // A half-understood file must not steer anything: keep only the diagnosis.
        cfg = ProjectConfigDraft {
            present: true,
            path: CONFIG_PATH.into(),
            errors: fatal,
            ..Default::default()
        };
        cfg.errors.extend(warnings);
        cfg.errors.push(format!("error: {CONFIG_PATH} was ignored"));
        return cfg;
    }
    cfg.errors = warnings;
    cfg
}

/// `review:` (CONTRACT.md §12). Globs are strings, severities come from
/// [`REVIEW_SEVERITIES`], rule ids are `[a-z0-9_-]` and unique, and every
/// `pattern` must compile as a regex: a rule that cannot run would
/// otherwise look like a rule that found nothing.
fn parse_review(
    m: &Value,
    warnings: &mut Vec<String>,
    fatal: &mut Vec<String>,
) -> ReviewConfigDraft {
    let mut out = ReviewConfigDraft::default();
    check_keys(
        m,
        "review.",
        &["ignore_paths", "rules", "severity", "require_tests_for"],
        warnings,
    );
    for (key, slot) in [
        ("ignore_paths", &mut out.ignore_paths),
        ("require_tests_for", &mut out.require_tests_for),
    ] {
        if let Some(v) = yaml::get(m, key) {
            match string_list(v) {
                Some(list) => *slot = list,
                None => fatal.push(format!(
                    "error: `review.{key}` must be a list of globs, got {}",
                    describe(v)
                )),
            }
        }
    }
    if let Some(v) = yaml::get(m, "severity") {
        if v.is_mapping() {
            for (rule, sev) in yaml::entries(Some(v)) {
                match sev {
                    Value::String(s)
                        if REVIEW_SEVERITIES.contains(&s.trim().to_ascii_lowercase().as_str()) =>
                    {
                        out.severity
                            .insert(rule.trim().to_string(), s.trim().to_ascii_lowercase());
                    }
                    other => fatal.push(format!(
                        "error: `review.severity.{rule}` must be one of {}, got {}",
                        REVIEW_SEVERITIES.join(" | "),
                        describe(other)
                    )),
                }
            }
        } else {
            fatal.push(format!(
                "error: `review.severity` must be a mapping of rule id to severity, got {}",
                describe(v)
            ));
        }
    }
    if let Some(v) = yaml::get(m, "rules") {
        let Value::Sequence(items) = v else {
            fatal.push(format!(
                "error: `review.rules` must be a list, got {}",
                describe(v)
            ));
            return out;
        };
        for (index, item) in items.iter().enumerate() {
            let label = format!("review.rules[{index}]");
            if !item.is_mapping() {
                fatal.push(format!(
                    "error: `{label}` must be a mapping, got {}",
                    describe(item)
                ));
                continue;
            }
            check_keys(
                item,
                &format!("{label}."),
                &["id", "pattern", "severity", "message", "paths"],
                warnings,
            );
            let mut rule = CustomRuleDraft::default();
            for (key, slot) in [
                ("id", &mut rule.id),
                ("pattern", &mut rule.pattern),
                ("severity", &mut rule.severity),
                ("message", &mut rule.message),
            ] {
                match yaml::get(item, key) {
                    Some(Value::String(s)) => *slot = s.trim().to_string(),
                    Some(other) => fatal.push(format!(
                        "error: `{label}.{key}` must be a string, got {}",
                        describe(other)
                    )),
                    None if key == "message" => {}
                    None => fatal.push(format!("error: `{label}.{key}` is required")),
                }
            }
            if let Some(v) = yaml::get(item, "paths") {
                match string_list(v) {
                    Some(list) => rule.paths = list,
                    None => fatal.push(format!(
                        "error: `{label}.paths` must be a list of globs, got {}",
                        describe(v)
                    )),
                }
            }
            rule.id = rule.id.to_ascii_lowercase();
            if !rule.id.is_empty()
                && !rule
                    .id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                fatal.push(format!(
                    "error: `{label}.id` may only use letters, digits, `_` and `-`, got `{}`",
                    rule.id
                ));
            }
            if out.rules.iter().any(|r| r.id == rule.id) {
                fatal.push(format!("error: `{label}.id` `{}` is used twice", rule.id));
            }
            rule.severity = rule.severity.to_ascii_lowercase();
            if !rule.severity.is_empty() && !REVIEW_SEVERITIES.contains(&rule.severity.as_str()) {
                fatal.push(format!(
                    "error: `{label}.severity` must be one of {}, got `{}`",
                    REVIEW_SEVERITIES.join(" | "),
                    rule.severity
                ));
            }
            if !rule.pattern.is_empty()
                && let Err(e) = regex::Regex::new(&rule.pattern)
            {
                fatal.push(format!(
                    "error: `{label}.pattern` is not a valid regex: {}",
                    e.to_string().lines().last().unwrap_or("").trim()
                ));
            }
            if rule.message.is_empty() {
                rule.message = format!("Matches review rule `{}`", rule.id);
            }
            out.rules.push(rule);
        }
    }
    out
}

/// `delivery:` (CONTRACT.md §22): the registry images are pushed to, the
/// workflow whose success creates a release, and the environments a release
/// is promoted through, in order. Everything here ends up steering a real
/// deployment, so the vocabulary is closed: an unknown registry or deployer
/// kind, a value of the wrong type, a duplicate environment name or a
/// deployer that lacks what it needs to run makes the file invalid. Only a
/// key nobody reads is a warning.
fn parse_delivery(
    m: &Value,
    warnings: &mut Vec<String>,
    fatal: &mut Vec<String>,
) -> DeliveryConfigDraft {
    let mut out = DeliveryConfigDraft::default();
    check_keys(
        m,
        "delivery.",
        &["registry", "build_workflow", "environments"],
        warnings,
    );
    if let Some(v) = yaml::get(m, "registry")
        && let Some(r) = require_mapping(v, "delivery.registry", fatal)
    {
        out.registry = parse_registry(r, warnings, fatal);
    }
    if let Some(v) = yaml::get(m, "build_workflow") {
        match v {
            Value::String(s) => {
                // `ci.yml` and `.github/workflows/ci.yml` name the same file.
                let name = s
                    .trim()
                    .trim_start_matches("./")
                    .trim_start_matches(".github/workflows/");
                let is_file = !name.is_empty()
                    && !name.contains('/')
                    && !name.contains(char::is_whitespace)
                    && (name.ends_with(".yml") || name.ends_with(".yaml"));
                if is_file {
                    out.build_workflow = name.to_string();
                } else {
                    fatal.push(format!(
                        "error: `delivery.build_workflow` must be a workflow file name such as `ci.yml`, got `{}`",
                        crate::util::truncate(s, 60)
                    ));
                }
            }
            other => fatal.push(format!(
                "error: `delivery.build_workflow` must be a string, got {}",
                describe(other)
            )),
        }
    }
    if let Some(v) = yaml::get(m, "environments") {
        let Value::Sequence(items) = v else {
            fatal.push(format!(
                "error: `delivery.environments` must be a list, got {}",
                describe(v)
            ));
            return out;
        };
        if items.len() > MAX_ENVIRONMENTS {
            fatal.push(format!(
                "error: `delivery.environments` holds at most {MAX_ENVIRONMENTS} environments, got {}",
                items.len()
            ));
        }
        for (index, item) in items.iter().enumerate() {
            let label = format!("delivery.environments[{index}]");
            if !item.is_mapping() {
                fatal.push(format!(
                    "error: `{label}` must be a mapping, got {}",
                    describe(item)
                ));
                continue;
            }
            let env = parse_environment(item, &label, warnings, fatal);
            if !env.name.is_empty()
                && out
                    .environments
                    .iter()
                    .any(|e| e.name.eq_ignore_ascii_case(&env.name))
            {
                fatal.push(format!(
                    "error: `{label}.name` `{}` is used twice",
                    env.name
                ));
            }
            out.environments.push(env);
        }
    }
    out
}

fn parse_registry(
    r: &Value,
    warnings: &mut Vec<String>,
    fatal: &mut Vec<String>,
) -> DeliveryRegistryDraft {
    let mut out = DeliveryRegistryDraft::default();
    check_keys(
        r,
        "delivery.registry.",
        &["kind", "host", "namespace"],
        warnings,
    );
    match yaml::get(r, "kind") {
        Some(Value::String(s)) => {
            let kind = s.trim().to_ascii_lowercase();
            if REGISTRY_KINDS.contains(&kind.as_str()) {
                out.kind = kind;
            } else {
                fatal.push(format!(
                    "error: `delivery.registry.kind` must be one of {}, got `{s}`",
                    REGISTRY_KINDS.join(" | ")
                ));
            }
        }
        Some(other) => fatal.push(format!(
            "error: `delivery.registry.kind` must be a string, got {}",
            describe(other)
        )),
        None => fatal.push("error: `delivery.registry.kind` is required".into()),
    }
    for (key, slot) in [("host", &mut out.host), ("namespace", &mut out.namespace)] {
        match yaml::get(r, key) {
            Some(Value::String(s)) => *slot = s.trim().to_string(),
            Some(other) => fatal.push(format!(
                "error: `delivery.registry.{key}` must be a string, got {}",
                describe(other)
            )),
            None => {}
        }
    }
    // A registry is addressed by host; a pasted URL is the same host.
    let host = out
        .host
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_ascii_lowercase();
    if host.contains('/') || host.contains(char::is_whitespace) {
        fatal.push(format!(
            "error: `delivery.registry.host` must be a host name such as `myorg.azurecr.io`, got `{}`",
            crate::util::truncate(&out.host, 60)
        ));
    }
    out.host = host;
    out.namespace = out.namespace.trim_matches('/').to_string();
    if out.namespace.contains(char::is_whitespace) {
        fatal.push(format!(
            "error: `delivery.registry.namespace` must not contain spaces, got `{}`",
            crate::util::truncate(&out.namespace, 60)
        ));
    }
    if out.host.is_empty() {
        match out.kind.as_str() {
            "ghcr" => out.host = "ghcr.io".into(),
            "dockerhub" => out.host = "docker.io".into(),
            "acr" | "ecr" | "gar" | "generic" => fatal.push(format!(
                "error: `delivery.registry.host` is required for kind `{}`",
                out.kind
            )),
            _ => {}
        }
    }
    out
}

// What follows mirrors the execution service's own check of an environment
// (`SyncEnvironments` refuses the whole list on the first problem), so a
// file that parses cleanly here is one execution accepts. The two must
// change together.

/// Deployer settings each kind reads (the CONTRACT.md §22 table).
fn deployer_config_keys(kind: &str) -> &'static [&'static str] {
    match kind {
        "github_actions" => &["workflow", "ref"],
        // Flux alone can also own image automation (ImagePolicy and friends).
        "gitops_flux" => &[
            "repo",
            "branch",
            "path",
            "strategy",
            "yaml_path",
            "mode",
            "image_automation",
        ],
        "gitops_argocd" => &["repo", "branch", "path", "strategy", "yaml_path", "mode"],
        "jenkins" => &["url", "job", "user_secret", "token_secret"],
        "azure_devops" => &["organization", "project", "pipeline_id", "pat_secret"],
        "webhook" => &["url", "secret"],
        "docker_local" => &["port", "env"],
        _ => &[],
    }
}

/// Settings every deployer kind accepts on top of its own.
const COMMON_CONFIG_KEYS: &[&str] = &["timeout_seconds", "health_timeout_seconds"];

/// Settings a deployer cannot run without.
fn deployer_required_keys(kind: &str) -> &'static [&'static str] {
    match kind {
        "gitops_flux" | "gitops_argocd" => &["path"],
        "jenkins" => &["url", "job", "user_secret", "token_secret"],
        "azure_devops" => &["organization", "project", "pipeline_id", "pat_secret"],
        "webhook" => &["url", "secret"],
        _ => &[],
    }
}

/// Settings whose value is the *name* of a workspace secret.
fn deployer_secret_keys(kind: &str) -> &'static [&'static str] {
    match kind {
        "jenkins" => &["user_secret", "token_secret"],
        "azure_devops" => &["pat_secret"],
        "webhook" => &["secret"],
        _ => &[],
    }
}

/// Strategies that cannot run without `config.yaml_path`. `helm_values`
/// falls back to `image.tag`, the same default the execution service uses.
const YAML_PATH_STRATEGIES: &[&str] = &["yaml_path"];

/// A project holds at most this many environments.
const MAX_ENVIRONMENTS: usize = 50;
/// Longest a deployer may be given to finish (six hours); a webhook has
/// to answer within ten minutes.
const MAX_TIMEOUT_SECONDS: u64 = 6 * 60 * 60;
const MAX_WEBHOOK_TIMEOUT_SECONDS: u64 = 600;
/// Longest a health check may wait (one hour).
const MAX_HEALTH_TIMEOUT_SECONDS: u64 = 60 * 60;

/// `^[A-Z][A-Z0-9_]{1,63}$`, the identity service's rule for secret names.
fn is_secret_name(s: &str) -> bool {
    let bytes = s.as_bytes();
    (2..=64).contains(&bytes.len())
        && bytes[0].is_ascii_uppercase()
        && bytes[1..]
            .iter()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || *b == b'_')
}

/// An http(s) URL with a host and no credentials.
fn is_http_url(s: &str) -> bool {
    let Some(rest) = s
        .strip_prefix("https://")
        .or_else(|| s.strip_prefix("http://"))
    else {
        return false;
    };
    if s.contains(char::is_whitespace) {
        return false;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    // `user:password@host` belongs in a workspace secret, not in the file.
    if authority.is_empty() || authority.contains('@') {
        return false;
    }
    let (host, port, v6) = match authority.strip_prefix('[') {
        // `[::1]:8080`
        Some(rest) => match rest.split_once(']') {
            Some((host, "")) => (host, None, true),
            Some((host, port)) if port.starts_with(':') => (host, Some(&port[1..]), true),
            _ => return false,
        },
        None => match authority.rsplit_once(':') {
            Some((host, port)) => (host, Some(port), false),
            None => (authority, None, false),
        },
    };
    let host_ok = !host.is_empty()
        && host.chars().all(|c| {
            if v6 {
                c.is_ascii_hexdigit() || matches!(c, ':' | '.')
            } else {
                c.is_alphanumeric() || matches!(c, '.' | '-' | '_')
            }
        });
    host_ok && port.is_none_or(|p| p.is_empty() || p.parse::<u16>().is_ok())
}

/// A branch or tag name that is safe to hand to git and to a URL.
fn is_git_ref(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with(['-', '/'])
        && !s.contains("..")
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/'))
}

/// A file path inside a repository: relative, no `..`, no empty segment.
fn is_repo_path(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('/')
        && !s.contains(char::is_whitespace)
        && s.split('/')
            .all(|segment| !segment.is_empty() && segment != "..")
}

/// A workflow file name (`deploy.yml`) or a workflow's numeric id.
fn is_workflow_ref(s: &str) -> bool {
    let plain = s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    let file = s.ends_with(".yml") || s.ends_with(".yaml");
    let id = !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    plain && (file || id)
}

fn is_env_var_name(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with(|c: char| c.is_ascii_digit())
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn seconds_within(text: &str, max: u64) -> bool {
    text.parse::<u64>().is_ok_and(|n| (1..=max).contains(&n))
}

fn parse_environment(
    item: &Value,
    label: &str,
    warnings: &mut Vec<String>,
    fatal: &mut Vec<String>,
) -> DeliveryEnvironmentDraft {
    let mut env = DeliveryEnvironmentDraft::default();
    check_keys(
        item,
        &format!("{label}."),
        &[
            "name",
            "kind",
            "url",
            "health_path",
            "protected",
            "auto_promote",
            "registry",
            "config",
        ],
        warnings,
    );
    for (key, slot, required) in [
        ("name", &mut env.name, true),
        ("kind", &mut env.kind, true),
        ("url", &mut env.url, false),
        ("health_path", &mut env.health_path, false),
        ("registry", &mut env.registry, false),
    ] {
        match yaml::get(item, key) {
            Some(Value::String(s)) => *slot = s.trim().to_string(),
            Some(other) => fatal.push(format!(
                "error: `{label}.{key}` must be a string, got {}",
                describe(other)
            )),
            None if required => fatal.push(format!("error: `{label}.{key}` is required")),
            None => {}
        }
    }
    // The name becomes a GitHub environment, a path segment of generated
    // manifests and a pipeline parameter, so it stays a plain identifier.
    let name_ok = env.name.len() <= 63
        && env
            .name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric())
        && env
            .name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if !env.name.is_empty() && !name_ok {
        fatal.push(format!(
            "error: `{label}.name` may only use letters, digits, `-`, `_` and `.` (at most 63), got `{}`",
            crate::util::truncate(&env.name, 70)
        ));
    }
    env.kind = env.kind.to_ascii_lowercase();
    let kind_ok = DEPLOYER_KINDS.contains(&env.kind.as_str());
    if !env.kind.is_empty() && !kind_ok {
        fatal.push(format!(
            "error: `{label}.kind` must be one of {}, got `{}`",
            DEPLOYER_KINDS.join(" | "),
            env.kind
        ));
    }
    if !env.url.is_empty() && !is_http_url(&env.url) {
        fatal.push(format!(
            "error: `{label}.url` must be an http(s) URL with a host and no credentials, got `{}`",
            crate::util::truncate(&env.url, 60)
        ));
    }
    env.url = env.url.trim_end_matches('/').to_string();
    if !env.health_path.is_empty() && !env.health_path.starts_with('/') {
        fatal.push(format!(
            "error: `{label}.health_path` must start with `/`, got `{}`",
            crate::util::truncate(&env.health_path, 60)
        ));
    } else if env.health_path.len() > 200 || env.health_path.contains(char::is_whitespace) {
        fatal.push(format!(
            "error: `{label}.health_path` must be a URL path such as `/health` (at most 200 characters, no spaces), got `{}`",
            crate::util::truncate(&env.health_path, 60)
        ));
    }
    for (key, slot) in [
        ("protected", &mut env.protected),
        ("auto_promote", &mut env.auto_promote),
    ] {
        match yaml::get(item, key) {
            Some(Value::Bool(b)) => *slot = *b,
            Some(other) => fatal.push(format!(
                "error: `{label}.{key}` must be true or false, got {}",
                describe(other)
            )),
            None => {}
        }
    }
    if let Some(v) = yaml::get(item, "config") {
        if v.is_mapping() {
            env.config = parse_deployer_config(v, &env.kind, kind_ok, label, warnings, fatal);
        } else {
            fatal.push(format!(
                "error: `{label}.config` must be a mapping, got {}",
                describe(v)
            ));
        }
    }
    if kind_ok {
        for key in deployer_required_keys(&env.kind) {
            if !env.config.contains_key(*key) {
                fatal.push(format!(
                    "error: `{label}.config.{key}` is required for kind `{}`",
                    env.kind
                ));
            }
        }
        if let Some(strategy) = env.config.get("strategy")
            && YAML_PATH_STRATEGIES.contains(&strategy.as_str())
            && !env.config.contains_key("yaml_path")
        {
            fatal.push(format!(
                "error: `{label}.config.yaml_path` is required with strategy `{strategy}`"
            ));
        }
    }
    env
}

/// `config:` of one environment as strings. Scalars are written as they
/// are; `docker_local.env` may be a mapping (or a JSON object in a string)
/// and is stored as one JSON object.
fn parse_deployer_config(
    v: &Value,
    kind: &str,
    kind_ok: bool,
    label: &str,
    warnings: &mut Vec<String>,
    fatal: &mut Vec<String>,
) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    let known = deployer_config_keys(kind);
    for (key, value) in yaml::entries(Some(v)) {
        let path = format!("{label}.config.{key}");
        if kind_ok && !known.contains(&key.as_str()) && !COMMON_CONFIG_KEYS.contains(&key.as_str())
        {
            warnings.push(format!("warning: unknown key `{path}` was ignored"));
            continue;
        }
        let text = if kind == "docker_local" && key == "env" {
            match env_json(value) {
                Some(Ok(json)) => json,
                Some(Err(name)) => {
                    fatal.push(format!(
                        "error: `{path}`: `{}` is not a valid environment variable name",
                        crate::util::truncate(&name, 60)
                    ));
                    continue;
                }
                None => {
                    fatal.push(format!(
                        "error: `{path}` must be a mapping of variable names to values, got {}",
                        describe(value)
                    ));
                    continue;
                }
            }
        } else {
            match value {
                Value::String(s) => s.trim().to_string(),
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => b.to_string(),
                other => {
                    fatal.push(format!(
                        "error: `{path}` must be a string, number or boolean, got {}",
                        describe(other)
                    ));
                    continue;
                }
            }
        };
        // A secret is named, never written down: the value is not echoed
        // back, in case the file holds the secret itself.
        if deployer_secret_keys(kind).contains(&key.as_str()) {
            if is_secret_name(&text) {
                out.insert(key, text);
            } else {
                fatal.push(format!(
                    "error: `{path}` must name a workspace secret in upper snake case such as `DEPLOY_TOKEN` (2 to 64 of `A-Z`, `0-9` and `_`, starting with a letter), never hold the value itself"
                ));
            }
            continue;
        }
        // `deploy.yml` and `.github/workflows/deploy.yml` name the same file.
        let text = if key == "workflow" {
            text.trim_start_matches("./")
                .trim_start_matches(".github/workflows/")
                .to_string()
        } else {
            text
        };
        let timeout = if kind == "webhook" {
            MAX_WEBHOOK_TIMEOUT_SECONDS
        } else {
            MAX_TIMEOUT_SECONDS
        };
        let problem = match key.as_str() {
            "strategy" if !DELIVERY_STRATEGIES.contains(&text.as_str()) => {
                Some(format!("one of {}", DELIVERY_STRATEGIES.join(" | ")))
            }
            "mode" if !DELIVERY_MODES.contains(&text.as_str()) => {
                Some(format!("one of {}", DELIVERY_MODES.join(" | ")))
            }
            "image_automation" if !matches!(text.as_str(), "true" | "false") => {
                Some("true or false".to_string())
            }
            "repo" if !is_repo_name(&text) => Some("`owner/name`".to_string()),
            "branch" if !is_git_ref(&text) => Some("a branch name".to_string()),
            "ref" if !is_git_ref(&text) => Some("a branch or tag name".to_string()),
            "path" if !is_repo_path(&text) => {
                Some("a file path inside the repository, without a leading `/` or `..`".to_string())
            }
            "workflow" if !is_workflow_ref(&text) => {
                Some("a workflow file name such as `deploy.yml`".to_string())
            }
            "url" if !is_http_url(&text) => {
                Some("an http(s) URL with a host and no credentials".to_string())
            }
            "pipeline_id" if !text.parse::<u32>().is_ok_and(|n| n > 0) => {
                Some("a positive whole number".to_string())
            }
            "port" if !text.parse::<u16>().is_ok_and(|n| n > 0) => {
                Some("a port number between 1 and 65535".to_string())
            }
            "timeout_seconds" if !seconds_within(&text, timeout) => {
                Some(format!("a number of seconds between 1 and {timeout}"))
            }
            "health_timeout_seconds" if !seconds_within(&text, MAX_HEALTH_TIMEOUT_SECONDS) => Some(
                format!("a number of seconds between 1 and {MAX_HEALTH_TIMEOUT_SECONDS}"),
            ),
            _ => None,
        };
        if let Some(expected) = problem {
            fatal.push(format!(
                "error: `{path}` must be {expected}, got `{}`",
                crate::util::truncate(&text, 60)
            ));
            continue;
        }
        let text = if key == "url" {
            text.trim_end_matches('/').to_string()
        } else {
            text
        };
        if !text.is_empty() {
            out.insert(key, text);
        }
    }
    out
}

const DELIVERY_STRATEGIES: &[&str] = &["kustomize_image", "helm_values", "yaml_path"];
const DELIVERY_MODES: &[&str] = &["pull_request", "commit"];

fn is_repo_name(s: &str) -> bool {
    // Exactly `owner/name`; each part is as safe as a branch name.
    let ok = |part: &str| !part.contains('/') && is_git_ref(part);
    matches!(s.split_once('/'), Some((owner, name)) if ok(owner) && ok(name))
}

/// `docker_local.env` as one canonical JSON object of strings: `None` when
/// it is not a mapping of scalars, `Err` with the first key that is not a
/// variable name.
fn env_json(value: &Value) -> Option<Result<String, String>> {
    let scalar = |v: &Value| match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    };
    let map: std::collections::BTreeMap<String, String> = match value {
        Value::Mapping(_) => yaml::entries(Some(value))
            .into_iter()
            .map(|(k, v)| scalar(v).map(|v| (k, v)))
            .collect::<Option<_>>()?,
        Value::String(s) => {
            let parsed: std::collections::BTreeMap<String, serde_json::Value> =
                serde_json::from_str(s).ok()?;
            parsed
                .into_iter()
                .map(|(k, v)| match v {
                    serde_json::Value::String(s) => Some((k, s)),
                    serde_json::Value::Number(n) => Some((k, n.to_string())),
                    serde_json::Value::Bool(b) => Some((k, b.to_string())),
                    _ => None,
                })
                .collect::<Option<_>>()?
        }
        _ => return None,
    };
    if let Some(bad) = map.keys().find(|name| !is_env_var_name(name)) {
        return Some(Err(bad.clone()));
    }
    serde_json::to_string(&map).ok().map(Ok)
}

/// A config whose values may be used: present, and nothing fatal in it.
/// Fatal problems are reported as `error: …`, skipped keys as `warning: …`,
/// so the UI and this check read the same convention.
pub fn is_usable(cfg: &ProjectConfigDraft) -> bool {
    cfg.present && !cfg.errors.iter().any(|e| e.starts_with("error:"))
}

fn describe(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => format!("boolean {b}"),
        Value::Number(n) => format!("number {n}"),
        Value::String(s) => format!("string `{}`", crate::util::truncate(s, 40)),
        Value::Sequence(_) => "a list".into(),
        Value::Mapping(_) => "a mapping".into(),
        Value::Tagged(_) => "a tagged value".into(),
    }
}

fn require_mapping<'a>(v: &'a Value, name: &str, fatal: &mut Vec<String>) -> Option<&'a Value> {
    if v.is_mapping() {
        Some(v)
    } else {
        fatal.push(format!(
            "error: `{name}` must be a mapping, got {}",
            describe(v)
        ));
        None
    }
}

fn check_keys(v: &Value, prefix: &str, known: &[&str], warnings: &mut Vec<String>) {
    for (k, _) in yaml::entries(Some(v)) {
        if !known.contains(&k.as_str()) {
            warnings.push(format!("warning: unknown key `{prefix}{k}` was ignored"));
        }
    }
}

fn string_list(v: &Value) -> Option<Vec<String>> {
    match v {
        Value::Sequence(seq) => seq
            .iter()
            .map(|x| match x {
                Value::String(s) => Some(s.trim().to_string()),
                _ => None,
            })
            .collect::<Option<Vec<String>>>()
            .map(|l| l.into_iter().filter(|s| !s.is_empty()).collect()),
        Value::String(s) => Some(
            s.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
        ),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Detector: reads the file, records configured facts.
// ---------------------------------------------------------------------------

pub(crate) fn detect(ctx: &mut Ctx<'_>) {
    let Some(content) = ctx.file(CONFIG_PATH) else {
        return;
    };
    let cfg = parse_config(content);
    if is_usable(&cfg) {
        let mut facts: Vec<(&str, &str, String)> = Vec::new();
        if !cfg.deploy_target.is_empty() {
            facts.push(("deploy.target", "Deploy target", cfg.deploy_target.clone()));
        }
        if !cfg.environments.is_empty() {
            facts.push((
                "deploy.environments",
                "Environments",
                cfg.environments.join(", "),
            ));
        }
        if !cfg.region.is_empty() {
            facts.push(("deploy.region", "Region", cfg.region.clone()));
        }
        if cfg.budget_monthly_usd > 0.0 {
            facts.push((
                "budget.monthly_usd",
                "Monthly budget (USD)",
                format!("{}", cfg.budget_monthly_usd),
            ));
        }
        if !cfg.test_command.is_empty() {
            facts.push(("test.command", "Test command", cfg.test_command.clone()));
        }
        if !cfg.health_path.is_empty() {
            facts.push(("test.health_path", "Health path", cfg.health_path.clone()));
        }
        if cfg.port > 0 {
            facts.push(("test.port", "Port", cfg.port.to_string()));
        }
        if !cfg.scan_exclude.is_empty() {
            facts.push((
                "scan.exclude",
                "Scan exclusions",
                cfg.scan_exclude.join(", "),
            ));
        }
        if !cfg.review.rules.is_empty()
            || !cfg.review.ignore_paths.is_empty()
            || !cfg.review.severity.is_empty()
            || !cfg.review.require_tests_for.is_empty()
        {
            facts.push((
                "review.rules",
                "Review configuration",
                format!(
                    "{} custom rule{}, {} ignored path{}, {} severity override{}, {} test requirement{}",
                    cfg.review.rules.len(),
                    if cfg.review.rules.len() == 1 { "" } else { "s" },
                    cfg.review.ignore_paths.len(),
                    if cfg.review.ignore_paths.len() == 1 { "" } else { "s" },
                    cfg.review.severity.len(),
                    if cfg.review.severity.len() == 1 { "" } else { "s" },
                    cfg.review.require_tests_for.len(),
                    if cfg.review.require_tests_for.len() == 1 { "" } else { "s" },
                ),
            ));
        }
        for (path, label, value) in facts {
            let leaf = if path == "review.rules" {
                "review"
            } else {
                path.rsplit('.').next().unwrap_or(path)
            };
            let evidence = line_with(content, &format!("{leaf}:"))
                .unwrap_or(leaf)
                .to_string();
            ctx.configured(&format!("config:{path}"), label, &value, &evidence);
        }
        if let Some(delivery) = DeliveryConfigDraft::from_json(&cfg.delivery_json) {
            delivery_facts(ctx, &delivery, content);
        }
    }
    ctx.out.config = cfg;
}

/// The `delivery:` block of the file: from its key to the next top-level
/// key. Evidence is quoted from here so `environments:` never points at
/// `deploy.environments`.
fn delivery_section(content: &str) -> String {
    let mut out = Vec::new();
    let mut inside = false;
    for line in content.lines() {
        let top_level = !line.starts_with([' ', '\t', '#', '-']) && !line.trim().is_empty();
        if top_level {
            if inside {
                break;
            }
            inside = line.trim_start().starts_with("delivery:");
        }
        if inside {
            out.push(line);
        }
    }
    out.join("\n")
}

/// `ghcr.io`, `myorg.azurecr.io/team`: where a declared registry points.
fn registry_address(registry: &DeliveryRegistryDraft) -> String {
    if registry.namespace.is_empty() {
        registry.host.clone()
    } else {
        format!("{}/{}", registry.host, registry.namespace)
    }
}

fn delivery_facts(ctx: &mut Ctx<'_>, delivery: &DeliveryConfigDraft, content: &str) {
    let section = delivery_section(content);
    let evidence =
        |needle: &str, fallback: &str| line_with(&section, needle).unwrap_or(fallback).to_string();
    if !delivery.registry.kind.is_empty() {
        ctx.configured(
            "config:delivery.registry",
            "Image registry",
            &format!(
                "{} ({})",
                delivery.registry.kind,
                registry_address(&delivery.registry)
            ),
            &evidence("registry:", "registry"),
        );
    }
    if !delivery.build_workflow.is_empty() {
        ctx.configured(
            "config:delivery.build_workflow",
            "Build workflow",
            &delivery.build_workflow,
            &evidence("build_workflow:", "build_workflow"),
        );
    }
    if !delivery.environments.is_empty() {
        let value = delivery
            .environments
            .iter()
            .map(|e| {
                let mut traits = vec![e.kind.as_str()];
                if e.protected {
                    traits.push("protected");
                }
                if e.auto_promote {
                    traits.push("auto-promote");
                }
                format!("{} ({})", e.name, traits.join(", "))
            })
            .collect::<Vec<_>>()
            .join(", ");
        ctx.configured(
            "config:delivery.environments",
            "Delivery environments",
            &value,
            &evidence("name:", "environments"),
        );
    }
}

/// Fills in the questions the configuration answers. The question objects
/// stay (the UI shows what was asked and who answered); only `answer` and
/// `answered_by` change.
pub(crate) fn answer_questions(cfg: &ProjectConfigDraft, questions: &mut [QuestionDraft]) {
    if !is_usable(cfg) {
        return;
    }
    // `delivery.environments` names the stages just as well as
    // `deploy.environments` does; the explicit list wins when both exist.
    let delivery_environments: Vec<String> = DeliveryConfigDraft::from_json(&cfg.delivery_json)
        .map(|d| d.environments.into_iter().map(|e| e.name).collect())
        .unwrap_or_default();
    for q in questions.iter_mut() {
        let answer = match q.key.as_str() {
            "deployment_target" if !cfg.deploy_target.is_empty() => {
                Some(deploy_target_label(&cfg.deploy_target, &q.options))
            }
            "environments" if !cfg.environments.is_empty() => {
                Some(environments_label(&cfg.environments, &q.options))
            }
            "environments" if !delivery_environments.is_empty() => {
                Some(environments_label(&delivery_environments, &q.options))
            }
            _ => None,
        };
        if let Some(a) = answer {
            q.answer = a;
            q.answered_by = "config".into();
        }
    }
}

/// `azure` → `Azure` (an option), `fly` → `Fly.io` (free text).
fn deploy_target_label(target: &str, options: &[String]) -> String {
    let wanted = match target {
        "vm" => "VM",
        "azure" => "Azure",
        "aws" => "AWS",
        "gcp" => "GCP",
        "kubernetes" => "Kubernetes",
        "none" => "I don't deploy yet",
        "fly" => "Fly.io",
        "render" => "Render",
        other => other,
    };
    options
        .iter()
        .find(|o| o.eq_ignore_ascii_case(wanted))
        .cloned()
        .unwrap_or_else(|| wanted.to_string())
}

/// `[staging, production]` → `Staging and production`; anything the options
/// do not cover is answered with the list itself.
fn environments_label(envs: &[String], options: &[String]) -> String {
    let mut norm: Vec<String> = envs.iter().map(|e| e.trim().to_ascii_lowercase()).collect();
    norm.sort();
    norm.dedup();
    let wanted = match norm
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["production"] | ["prod"] => "Production only",
        ["production", "staging"] | ["prod", "staging"] => "Staging and production",
        ["development", "production", "staging"] | ["dev", "production", "staging"] => {
            "Development, staging and production"
        }
        [] => "None yet",
        _ => "",
    };
    options
        .iter()
        .find(|o| !wanted.is_empty() && o.eq_ignore_ascii_case(wanted))
        .cloned()
        .unwrap_or_else(|| envs.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_config() {
        let cfg = parse_config(
            "version: 1\ndeploy:\n  target: azure\n  environments: [staging, production]\n  region: westeurope\nbudget:\n  monthly_usd: 20\ntest:\n  command: cargo test\n  health_path: /health\n  port: 8080\nscan:\n  exclude: [\"docs/legacy/**\", \"vendor/**\"]\n",
        );
        assert!(cfg.present);
        assert!(cfg.errors.is_empty(), "{:?}", cfg.errors);
        assert_eq!(cfg.deploy_target, "azure");
        assert_eq!(cfg.environments, vec!["staging", "production"]);
        assert_eq!(cfg.region, "westeurope");
        assert_eq!(cfg.budget_monthly_usd, 20.0);
        assert_eq!(cfg.test_command, "cargo test");
        assert_eq!(cfg.health_path, "/health");
        assert_eq!(cfg.port, 8080);
        assert_eq!(cfg.scan_exclude, vec!["docs/legacy/**", "vendor/**"]);
        assert!(is_usable(&cfg));
    }

    #[test]
    fn unknown_keys_warn_but_apply() {
        let cfg = parse_config("version: 1\ndeploy:\n  target: vm\n  colour: blue\nowner: me\n");
        assert_eq!(cfg.deploy_target, "vm");
        assert_eq!(cfg.errors.len(), 2, "{:?}", cfg.errors);
        assert!(cfg.errors.iter().any(|e| e.contains("deploy.colour")));
        assert!(cfg.errors.iter().any(|e| e.contains("`owner`")));
        assert!(is_usable(&cfg));
    }

    #[test]
    fn wrong_types_or_version_ignore_the_file() {
        for content in [
            "version: 2\ndeploy:\n  target: vm\n",
            "deploy:\n  target: vm\n",
            "version: 1\ndeploy:\n  target: [vm]\n",
            "version: 1\ndeploy:\n  target: mainframe\n",
            "version: 1\nbudget:\n  monthly_usd: cheap\n",
            "version: 1\ntest:\n  port: eighty\n",
            "version: 1\nscan: nope\n",
            "- just\n- a list\n",
            "{{{ not yaml",
        ] {
            let cfg = parse_config(content);
            assert!(cfg.present);
            assert!(!cfg.errors.is_empty(), "{content}");
            assert!(cfg.deploy_target.is_empty(), "{content}");
            assert!(!is_usable(&cfg), "{content}");
        }
    }

    #[test]
    fn parses_the_review_section() {
        let cfg = parse_config(
            "version: 1\nreview:\n  ignore_paths: [\"gen/**\", \"vendor/**\"]\n  require_tests_for: [\"src/billing/**\"]\n  severity:\n    correctness.todo: high\n    docker.latest_tag: Info\n  rules:\n    - id: no-moment\n      pattern: \"from ['\\\"]moment['\\\"]\"\n      severity: medium\n      message: Use date-fns instead of moment\n      paths: [\"web/**\"]\n    - id: ticket_ref\n      pattern: \"JIRA-\\\\d+\"\n      severity: low\n",
        );
        assert!(cfg.errors.is_empty(), "{:?}", cfg.errors);
        assert!(is_usable(&cfg));
        assert_eq!(cfg.review.ignore_paths, vec!["gen/**", "vendor/**"]);
        assert_eq!(cfg.review.require_tests_for, vec!["src/billing/**"]);
        assert_eq!(
            cfg.review
                .severity
                .get("correctness.todo")
                .map(String::as_str),
            Some("high")
        );
        assert_eq!(
            cfg.review
                .severity
                .get("docker.latest_tag")
                .map(String::as_str),
            Some("info")
        );
        assert_eq!(cfg.review.rules.len(), 2);
        let first = &cfg.review.rules[0];
        assert_eq!(first.id, "no-moment");
        assert_eq!(first.severity, "medium");
        assert_eq!(first.message, "Use date-fns instead of moment");
        assert_eq!(first.paths, vec!["web/**"]);
        assert!(
            regex::Regex::new(&first.pattern)
                .unwrap()
                .is_match("import m from 'moment'")
        );
        let second = &cfg.review.rules[1];
        assert!(second.paths.is_empty());
        assert_eq!(second.message, "Matches review rule `ticket_ref`");
    }

    #[test]
    fn review_section_is_strict() {
        for content in [
            "version: 1\nreview: []\n",
            "version: 1\nreview:\n  ignore_paths: 7\n",
            "version: 1\nreview:\n  severity:\n    x.y: loud\n",
            "version: 1\nreview:\n  rules:\n    - id: a\n      pattern: \"(\"\n      severity: low\n",
            "version: 1\nreview:\n  rules:\n    - id: a\n      pattern: x\n      severity: huge\n",
            "version: 1\nreview:\n  rules:\n    - pattern: x\n      severity: low\n",
            "version: 1\nreview:\n  rules:\n    - id: a\n      pattern: x\n      severity: low\n    - id: a\n      pattern: y\n      severity: low\n",
            "version: 1\nreview:\n  rules:\n    - id: \"bad id\"\n      pattern: x\n      severity: low\n",
        ] {
            let cfg = parse_config(content);
            assert!(!is_usable(&cfg), "{content}");
            assert!(cfg.review.rules.is_empty(), "{content}");
        }
        let cfg = parse_config("version: 1\nreview:\n  colour: blue\n");
        assert!(is_usable(&cfg));
        assert!(cfg.errors.iter().any(|e| e.contains("review.colour")));
    }

    const DELIVERY: &str = r#"version: 1
deploy:
  environments: [qa]
delivery:
  registry: { kind: ACR, host: "https://MyOrg.azurecr.io/", namespace: /team/ }
  build_workflow: .github/workflows/ci.yml
  environments:
    - name: staging
      kind: github_actions
      url: https://staging.example.com/
      health_path: /health
      auto_promote: true
    - name: production
      kind: gitops_flux
      protected: true
      registry: fleet-registry
      config: { repo: acme/fleet, path: apps/api/kustomization.yaml, strategy: kustomize_image }
    - name: local
      kind: docker_local
      config:
        port: 8080
        env: { LOG_LEVEL: debug, WORKERS: 2 }
    - name: canary
      kind: gitops_argocd
      config: { path: deploy/canary/values.yaml, strategy: helm_values, yaml_path: image.tag, branch: release/2026, health_timeout_seconds: 900 }
    - name: legacy
      kind: jenkins
      config:
        url: https://ci.example.com/
        job: deploy-api
        user_secret: JENKINS_USER
        token_secret: JENKINS_TOKEN
        timeout_seconds: 21600
    - name: windows
      kind: azure_devops
      config: { organization: acme, project: shop, pipeline_id: 42, pat_secret: AZURE_DEVOPS_PAT, timeout_seconds: 1800 }
    - name: edge
      kind: webhook
      config: { url: "https://hooks.example.com/deploy", secret: DEPLOY_HOOK_SECRET, timeout_seconds: 600, health_timeout_seconds: 3600 }
"#;

    #[test]
    fn parses_the_delivery_section_into_canonical_json() {
        let cfg = parse_config(DELIVERY);
        assert!(cfg.errors.is_empty(), "{:?}", cfg.errors);
        assert!(is_usable(&cfg));
        let delivery = DeliveryConfigDraft::from_json(&cfg.delivery_json).expect("delivery json");
        assert_eq!(delivery.registry.kind, "acr");
        assert_eq!(delivery.registry.host, "myorg.azurecr.io");
        assert_eq!(delivery.registry.namespace, "team");
        assert_eq!(delivery.build_workflow, "ci.yml");
        assert_eq!(
            delivery
                .environments
                .iter()
                .map(|e| (e.name.as_str(), e.kind.as_str()))
                .collect::<Vec<_>>(),
            [
                ("staging", "github_actions"),
                ("production", "gitops_flux"),
                ("local", "docker_local"),
                ("canary", "gitops_argocd"),
                ("legacy", "jenkins"),
                ("windows", "azure_devops"),
                ("edge", "webhook")
            ]
        );
        let staging = &delivery.environments[0];
        assert_eq!(staging.url, "https://staging.example.com");
        assert_eq!(staging.health_path, "/health");
        assert!(staging.auto_promote && !staging.protected);
        assert!(staging.config.is_empty());
        let production = &delivery.environments[1];
        assert!(production.protected && !production.auto_promote);
        assert_eq!(production.registry, "fleet-registry");
        assert_eq!(production.config["repo"], "acme/fleet");
        assert_eq!(production.config["strategy"], "kustomize_image");
        assert_eq!(production.config["path"], "apps/api/kustomization.yaml");
        let local = &delivery.environments[2];
        assert_eq!(local.config["port"], "8080");
        assert_eq!(
            local.config["env"],
            r#"{"LOG_LEVEL":"debug","WORKERS":"2"}"#
        );
        // Every kind takes the two timeouts next to its own settings, and
        // the secrets are carried by name.
        let config = |name: &str| {
            &delivery
                .environments
                .iter()
                .find(|e| e.name == name)
                .unwrap()
                .config
        };
        assert_eq!(config("canary")["yaml_path"], "image.tag");
        assert_eq!(config("canary")["branch"], "release/2026");
        assert_eq!(config("canary")["health_timeout_seconds"], "900");
        assert_eq!(config("legacy")["url"], "https://ci.example.com");
        assert_eq!(config("legacy")["user_secret"], "JENKINS_USER");
        assert_eq!(config("legacy")["token_secret"], "JENKINS_TOKEN");
        assert_eq!(config("legacy")["timeout_seconds"], "21600");
        assert_eq!(config("windows")["pipeline_id"], "42");
        assert_eq!(config("windows")["pat_secret"], "AZURE_DEVOPS_PAT");
        assert_eq!(config("edge")["secret"], "DEPLOY_HOOK_SECRET");
        assert_eq!(config("edge")["timeout_seconds"], "600");
        // Canonical: the same declarations in another order and spelling
        // serialise to the same bytes, and the JSON round-trips.
        assert_eq!(delivery.to_json(), cfg.delivery_json);
        assert!(cfg.delivery_json.starts_with(
            r#"{"registry":{"kind":"acr","host":"myorg.azurecr.io","namespace":"team"},"build_workflow":"ci.yml","environments":[{"name":"staging","kind":"github_actions","#
        ));
        let reordered = parse_config(
            "version: 1\ndelivery:\n  environments:\n    - kind: gitops_argocd\n      config: { yaml_path: image.tag, strategy: helm_values, repo: acme/fleet, path: deploy/values.yaml }\n      name: prod\n",
        );
        let same = parse_config(
            "version: 1\ndelivery:\n  environments:\n    - name: prod\n      kind: GitOps_ArgoCD\n      config:\n        repo: acme/fleet\n        path: deploy/values.yaml\n        strategy: helm_values\n        yaml_path: image.tag\n",
        );
        assert!(is_usable(&reordered) && is_usable(&same));
        assert_eq!(reordered.delivery_json, same.delivery_json);
        // The strategy may be left out (execution takes `kustomize_image`),
        // and a workflow may be named by its path or its numeric id.
        let defaults = parse_config(
            "version: 1\ndelivery:\n  environments:\n    - { name: a, kind: gitops_flux, config: { path: k.yaml } }\n    - { name: b, kind: github_actions, config: { workflow: .github/workflows/deploy.yaml, ref: v1.2 } }\n    - { name: c, kind: github_actions, config: { workflow: 1234567 } }\n",
        );
        assert!(defaults.errors.is_empty(), "{:?}", defaults.errors);
        let defaults = DeliveryConfigDraft::from_json(&defaults.delivery_json).unwrap();
        assert!(!defaults.environments[0].config.contains_key("strategy"));
        assert_eq!(defaults.environments[1].config["workflow"], "deploy.yaml");
        assert_eq!(defaults.environments[2].config["workflow"], "1234567");
        // Defaults: GHCR and Docker Hub need no host.
        let ghcr = parse_config("version: 1\ndelivery:\n  registry: { kind: ghcr }\n");
        assert_eq!(
            DeliveryConfigDraft::from_json(&ghcr.delivery_json)
                .unwrap()
                .registry
                .host,
            "ghcr.io"
        );
        // No section, or an empty one, leaves the field empty.
        assert!(parse_config("version: 1\n").delivery_json.is_empty());
        assert!(
            parse_config("version: 1\ndelivery: {}\n")
                .delivery_json
                .is_empty()
        );
    }

    #[test]
    fn delivery_section_is_strict() {
        for (content, needle) in [
            ("delivery: staging", "`delivery` must be a mapping"),
            (
                "delivery:\n  registry: ghcr",
                "`delivery.registry` must be a mapping",
            ),
            (
                "delivery:\n  registry: { host: x.io }",
                "`delivery.registry.kind` is required",
            ),
            (
                "delivery:\n  registry: { kind: quay }",
                "`delivery.registry.kind` must be one of",
            ),
            (
                "delivery:\n  registry: { kind: acr }",
                "`delivery.registry.host` is required for kind `acr`",
            ),
            (
                "delivery:\n  registry: { kind: generic, host: \"registry.example.com/v2/path\" }",
                "`delivery.registry.host` must be a host name",
            ),
            (
                "delivery:\n  registry: { kind: ghcr, namespace: 7 }",
                "`delivery.registry.namespace` must be a string",
            ),
            (
                "delivery:\n  build_workflow: [ci.yml]",
                "`delivery.build_workflow` must be a string",
            ),
            (
                "delivery:\n  build_workflow: build the image",
                "`delivery.build_workflow` must be a workflow file name",
            ),
            (
                "delivery:\n  environments: staging",
                "`delivery.environments` must be a list",
            ),
            (
                "delivery:\n  environments: [staging]",
                "`delivery.environments[0]` must be a mapping",
            ),
            (
                "delivery:\n  environments:\n    - kind: webhook",
                "`delivery.environments[0].name` is required",
            ),
            (
                "delivery:\n  environments:\n    - name: staging",
                "`delivery.environments[0].kind` is required",
            ),
            (
                "delivery:\n  environments:\n    - { name: staging, kind: capistrano }",
                "`delivery.environments[0].kind` must be one of",
            ),
            (
                "delivery:\n  environments:\n    - { name: \"my stage\", kind: github_actions }",
                "`delivery.environments[0].name` may only use",
            ),
            (
                "delivery:\n  environments:\n    - { name: a, kind: github_actions }\n    - { name: A, kind: github_actions }",
                "`delivery.environments[1].name` `A` is used twice",
            ),
            (
                "delivery:\n  environments:\n    - { name: a, kind: github_actions, protected: \"yes\" }",
                "`delivery.environments[0].protected` must be true or false",
            ),
            (
                "delivery:\n  environments:\n    - { name: a, kind: github_actions, auto_promote: 1 }",
                "`delivery.environments[0].auto_promote` must be true or false",
            ),
            (
                "delivery:\n  environments:\n    - { name: a, kind: github_actions, url: staging.example.com }",
                "`delivery.environments[0].url` must be an http(s) URL",
            ),
            (
                "delivery:\n  environments:\n    - { name: a, kind: github_actions, health_path: health }",
                "`delivery.environments[0].health_path` must start with `/`",
            ),
            (
                "delivery:\n  environments:\n    - { name: a, kind: github_actions, registry: [x] }",
                "`delivery.environments[0].registry` must be a string",
            ),
            (
                "delivery:\n  environments:\n    - { name: a, kind: github_actions, config: deploy.yml }",
                "`delivery.environments[0].config` must be a mapping",
            ),
            (
                "delivery:\n  environments:\n    - { name: a, kind: github_actions, config: { workflow: [a] } }",
                "`delivery.environments[0].config.workflow` must be a string, number or boolean",
            ),
            (
                "delivery:\n  environments:\n    - { name: a, kind: gitops_flux, config: { strategy: sed } }",
                "`delivery.environments[0].config.strategy` must be one of",
            ),
            (
                "delivery:\n  environments:\n    - { name: a, kind: gitops_flux, config: { strategy: yaml_path } }",
                "`delivery.environments[0].config.yaml_path` is required with strategy `yaml_path`",
            ),
            (
                "delivery:\n  environments:\n    - { name: a, kind: gitops_argocd, config: { mode: push } }",
                "`delivery.environments[0].config.mode` must be one of",
            ),
            (
                "delivery:\n  environments:\n    - { name: a, kind: gitops_argocd, config: { repo: fleet } }",
                "`delivery.environments[0].config.repo` must be `owner/name`",
            ),
            (
                "delivery:\n  environments:\n    - { name: a, kind: jenkins, config: { url: \"https://ci.example.com\" } }",
                "`delivery.environments[0].config.job` is required for kind `jenkins`",
            ),
            (
                "delivery:\n  environments:\n    - { name: a, kind: jenkins, config: { url: ci.example.com, job: deploy } }",
                "`delivery.environments[0].config.url` must be an http(s) URL",
            ),
            (
                "delivery:\n  environments:\n    - { name: a, kind: azure_devops, config: { organization: acme, project: shop, pipeline_id: twelve } }",
                "`delivery.environments[0].config.pipeline_id` must be a positive whole number",
            ),
            (
                "delivery:\n  environments:\n    - { name: a, kind: webhook }",
                "`delivery.environments[0].config.url` is required for kind `webhook`",
            ),
            (
                "delivery:\n  environments:\n    - { name: a, kind: docker_local, config: { env: [A, B] } }",
                "`delivery.environments[0].config.env` must be a mapping of variable names",
            ),
            // What the execution service refuses at `SyncEnvironments` is
            // an error here, named by its `config.<key>`.
            (
                "delivery:\n  environments:\n    - { name: a, kind: gitops_flux }",
                "`delivery.environments[0].config.path` is required for kind `gitops_flux`",
            ),
            (
                "delivery:\n  environments:\n    - { name: a, kind: gitops_argocd, config: { repo: acme/fleet } }",
                "`delivery.environments[0].config.path` is required for kind `gitops_argocd`",
            ),
            (
                "delivery:\n  environments:\n    - { name: a, kind: gitops_argocd, config: { path: values.yaml, strategy: yaml_path } }",
                "`delivery.environments[0].config.yaml_path` is required with strategy `yaml_path`",
            ),
            (
                "delivery:\n  environments:\n    - { name: a, kind: gitops_flux, config: { path: /apps/k.yaml } }",
                "`delivery.environments[0].config.path` must be a file path inside the repository",
            ),
            (
                "delivery:\n  environments:\n    - { name: a, kind: gitops_flux, config: { path: apps/../../k.yaml } }",
                "`delivery.environments[0].config.path` must be a file path inside the repository",
            ),
            (
                "delivery:\n  environments:\n    - { name: a, kind: gitops_flux, config: { path: k.yaml, branch: \"-D main\" } }",
                "`delivery.environments[0].config.branch` must be a branch name",
            ),
            (
                "delivery:\n  environments:\n    - { name: a, kind: gitops_flux, config: { path: k.yaml, repo: acme/fleet/apps } }",
                "`delivery.environments[0].config.repo` must be `owner/name`",
            ),
            (
                "delivery:\n  environments:\n    - { name: a, kind: github_actions, config: { workflow: ci/deploy.yml } }",
                "`delivery.environments[0].config.workflow` must be a workflow file name",
            ),
            (
                "delivery:\n  environments:\n    - { name: a, kind: github_actions, config: { workflow: deploy } }",
                "`delivery.environments[0].config.workflow` must be a workflow file name",
            ),
            (
                "delivery:\n  environments:\n    - { name: a, kind: github_actions, config: { ref: \"main..next\" } }",
                "`delivery.environments[0].config.ref` must be a branch or tag name",
            ),
            (
                "delivery:\n  environments:\n    - { name: a, kind: jenkins, config: { url: \"https://ci.example.com\", job: deploy, token_secret: JENKINS_TOKEN } }",
                "`delivery.environments[0].config.user_secret` is required for kind `jenkins`",
            ),
            (
                "delivery:\n  environments:\n    - { name: a, kind: jenkins, config: { url: \"https://ci.example.com\", job: deploy, user_secret: JENKINS_USER } }",
                "`delivery.environments[0].config.token_secret` is required for kind `jenkins`",
            ),
            (
                "delivery:\n  environments:\n    - { name: a, kind: jenkins, config: { url: \"https://admin:hunter2@ci.example.com\", job: deploy, user_secret: JENKINS_USER, token_secret: JENKINS_TOKEN } }",
                "`delivery.environments[0].config.url` must be an http(s) URL with a host and no credentials",
            ),
            (
                "delivery:\n  environments:\n    - { name: a, kind: azure_devops, config: { organization: acme, project: shop, pipeline_id: 12 } }",
                "`delivery.environments[0].config.pat_secret` is required for kind `azure_devops`",
            ),
            (
                "delivery:\n  environments:\n    - { name: a, kind: azure_devops, config: { organization: acme, project: shop, pipeline_id: 12, pat_secret: azure-devops-pat } }",
                "`delivery.environments[0].config.pat_secret` must name a workspace secret",
            ),
            (
                "delivery:\n  environments:\n    - { name: a, kind: webhook, config: { url: \"https://hooks.example.com\" } }",
                "`delivery.environments[0].config.secret` is required for kind `webhook`",
            ),
            (
                "delivery:\n  environments:\n    - { name: a, kind: webhook, config: { url: \"https://hooks.example.com\", secret: S } }",
                "`delivery.environments[0].config.secret` must name a workspace secret",
            ),
            (
                "delivery:\n  environments:\n    - { name: a, kind: webhook, config: { url: \"https://hooks.example.com\", secret: HOOK_SECRET, timeout_seconds: 601 } }",
                "`delivery.environments[0].config.timeout_seconds` must be a number of seconds between 1 and 600",
            ),
            (
                "delivery:\n  environments:\n    - { name: a, kind: github_actions, config: { timeout_seconds: 21601 } }",
                "`delivery.environments[0].config.timeout_seconds` must be a number of seconds between 1 and 21600",
            ),
            (
                "delivery:\n  environments:\n    - { name: a, kind: docker_local, config: { timeout_seconds: soon } }",
                "`delivery.environments[0].config.timeout_seconds` must be a number of seconds",
            ),
            (
                "delivery:\n  environments:\n    - { name: a, kind: github_actions, config: { health_timeout_seconds: 0 } }",
                "`delivery.environments[0].config.health_timeout_seconds` must be a number of seconds between 1 and 3600",
            ),
            (
                "delivery:\n  environments:\n    - { name: a, kind: gitops_flux, config: { path: k.yaml, health_timeout_seconds: 3601 } }",
                "`delivery.environments[0].config.health_timeout_seconds` must be a number of seconds between 1 and 3600",
            ),
            (
                "delivery:\n  environments:\n    - { name: a, kind: docker_local, config: { port: 70000 } }",
                "`delivery.environments[0].config.port` must be a port number between 1 and 65535",
            ),
            (
                "delivery:\n  environments:\n    - { name: a, kind: docker_local, config: { env: { 2FAST: \"1\" } } }",
                "`delivery.environments[0].config.env`: `2FAST` is not a valid environment variable name",
            ),
            (
                "delivery:\n  environments:\n    - { name: a, kind: github_actions, url: \"https://deploy:pw@staging.example.com\" }",
                "`delivery.environments[0].url` must be an http(s) URL with a host and no credentials",
            ),
            (
                "delivery:\n  environments:\n    - { name: a, kind: github_actions, health_path: \"/health check\" }",
                "`delivery.environments[0].health_path` must be a URL path such as `/health`",
            ),
            (
                "delivery:\n  environments:\n    - { name: a123456789b123456789c123456789d123456789e123456789f123456789g123, kind: github_actions }",
                "`delivery.environments[0].name` may only use",
            ),
        ] {
            let cfg = parse_config(&format!("version: 1\n{content}\n"));
            assert!(!is_usable(&cfg), "{content}: {:?}", cfg.errors);
            assert!(
                cfg.errors
                    .iter()
                    .any(|e| e.starts_with("error:") && e.contains(needle)),
                "{content}: expected {needle:?} in {:?}",
                cfg.errors
            );
            assert!(cfg.delivery_json.is_empty(), "{content}");
        }
        // A secret is named, never written down: whatever the key holds
        // instead is not repeated in the error.
        let cfg = parse_config(
            "version: 1\ndelivery:\n  environments:\n    - { name: a, kind: webhook, config: { url: \"https://hooks.example.com\", secret: \"s3cr3t-value\" } }\n",
        );
        assert!(!is_usable(&cfg));
        assert!(
            cfg.errors.iter().all(|e| !e.contains("s3cr3t-value")),
            "{:?}",
            cfg.errors
        );
        // The longest names execution takes for an environment and a secret.
        let longest = parse_config(&format!(
            "version: 1\ndelivery:\n  environments:\n    - {{ name: {}, kind: webhook, config: {{ url: \"http://[::1]:8080/hook\", secret: {} }} }}\n",
            "e".repeat(63),
            "S".repeat(64)
        ));
        assert!(longest.errors.is_empty(), "{:?}", longest.errors);
        let too_long = parse_config(&format!(
            "version: 1\ndelivery:\n  environments:\n    - {{ name: a, kind: webhook, config: {{ url: \"https://hooks.example.com\", secret: {} }} }}\n",
            "S".repeat(65)
        ));
        assert!(!is_usable(&too_long));
        // A project holds at most fifty environments.
        let many = |count: usize| {
            let mut content = String::from("version: 1\ndelivery:\n  environments:\n");
            for index in 0..count {
                content.push_str(&format!(
                    "    - {{ name: env-{index}, kind: github_actions }}\n"
                ));
            }
            parse_config(&content)
        };
        assert!(is_usable(&many(50)));
        let cfg = many(51);
        assert!(!is_usable(&cfg));
        assert!(
            cfg.errors.iter().any(
                |e| e.contains("`delivery.environments` holds at most 50 environments, got 51")
            ),
            "{:?}",
            cfg.errors
        );
        // Keys nobody reads are warnings: reported, dropped, the rest applies.
        let cfg = parse_config(
            "version: 1\ndelivery:\n  colour: blue\n  registry: { kind: ghcr, mirror: true }\n  environments:\n    - name: staging\n      kind: github_actions\n      owner: me\n      config: { workflow: release.yml, timeout: 5, health_timeout_seconds: 300, timeout_seconds: 900 }\n    - name: production\n      kind: gitops_flux\n      config: { path: k.yaml, image_automation: true }\n",
        );
        assert!(is_usable(&cfg), "{:?}", cfg.errors);
        for key in [
            "delivery.colour",
            "delivery.registry.mirror",
            "delivery.environments[0].owner",
            "delivery.environments[0].config.timeout",
        ] {
            assert!(
                cfg.errors
                    .iter()
                    .any(|e| e == &format!("warning: unknown key `{key}` was ignored")),
                "{key}: {:?}",
                cfg.errors
            );
        }
        assert_eq!(cfg.errors.len(), 4, "{:?}", cfg.errors);
        let delivery = DeliveryConfigDraft::from_json(&cfg.delivery_json).unwrap();
        let config = &delivery.environments[0].config;
        assert_eq!(config["workflow"], "release.yml");
        assert_eq!(config["health_timeout_seconds"], "300");
        assert_eq!(config["timeout_seconds"], "900");
        assert!(!config.contains_key("timeout"));
        // Flux reads `image_automation`; it is a setting, not an unknown key.
        assert!(
            delivery.environments[1]
                .config
                .contains_key("image_automation")
        );
    }

    #[test]
    fn delivery_environments_answer_the_environments_question() {
        let options: Vec<String> = ["Production only", "Staging and production"]
            .map(String::from)
            .to_vec();
        let mut questions = vec![QuestionDraft {
            key: "environments".into(),
            prompt: String::new(),
            reason: String::new(),
            options,
            allow_free_text: true,
            answer: String::new(),
            answered_by: String::new(),
        }];
        let cfg = parse_config(
            "version: 1\ndelivery:\n  environments:\n    - { name: staging, kind: github_actions }\n    - { name: production, kind: github_actions, protected: true }\n",
        );
        answer_questions(&cfg, &mut questions);
        assert_eq!(questions[0].answer, "Staging and production");
        assert_eq!(questions[0].answered_by, "config");
        // `deploy.environments` is the explicit answer when both are declared.
        questions[0].answer.clear();
        answer_questions(&parse_config(DELIVERY), &mut questions);
        assert_eq!(questions[0].answer, "qa");
    }

    #[test]
    fn answers_map_to_option_labels() {
        let options: Vec<String> = [
            "Azure",
            "AWS",
            "GCP",
            "Kubernetes",
            "VM",
            "I don't deploy yet",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(deploy_target_label("vm", &options), "VM");
        assert_eq!(deploy_target_label("none", &options), "I don't deploy yet");
        assert_eq!(deploy_target_label("fly", &options), "Fly.io");
        let env_options: Vec<String> = [
            "Production only",
            "Staging and production",
            "Development, staging and production",
            "None yet",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(
            environments_label(&["production".into(), "staging".into()], &env_options),
            "Staging and production"
        );
        assert_eq!(
            environments_label(&["qa".into(), "prod".into()], &env_options),
            "qa, prod"
        );
    }
}

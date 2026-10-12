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
        &["version", "deploy", "budget", "test", "scan", "review"],
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
    }
    ctx.out.config = cfg;
}

/// Fills in the questions the configuration answers. The question objects
/// stay (the UI shows what was asked and who answered); only `answer` and
/// `answered_by` change.
pub(crate) fn answer_questions(cfg: &ProjectConfigDraft, questions: &mut [QuestionDraft]) {
    if !is_usable(cfg) {
        return;
    }
    for q in questions.iter_mut() {
        let answer = match q.key.as_str() {
            "deployment_target" if !cfg.deploy_target.is_empty() => {
                Some(deploy_target_label(&cfg.deploy_target, &q.options))
            }
            "environments" if !cfg.environments.is_empty() => {
                Some(environments_label(&cfg.environments, &q.options))
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

//! Keys, tokens and private keys in added lines. A match is reported with
//! the line it sits on but never with the value itself.

use std::sync::LazyLock;

use regex::Regex;

use super::{Finding, Severity};
use crate::domain::review::input::ReviewInput;

struct Pattern {
    check: &'static str,
    title: &'static str,
    regex: Regex,
    severity: Severity,
}

static PATTERNS: LazyLock<Vec<Pattern>> = LazyLock::new(|| {
    let p = |check, title, re: &str, severity| Pattern {
        check,
        title,
        regex: Regex::new(re).expect("secret pattern"),
        severity,
    };
    vec![
        p(
            "private_key",
            "Private key committed",
            r"-----BEGIN (?:RSA |EC |DSA |OPENSSH |PGP |ENCRYPTED )?PRIVATE KEY-----",
            Severity::Critical,
        ),
        p(
            "aws_key",
            "AWS access key id",
            r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b",
            Severity::Critical,
        ),
        p(
            "github_token",
            "GitHub token",
            r"\b(?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9]{36,}\b|\bgithub_pat_[A-Za-z0-9_]{22,}\b",
            Severity::Critical,
        ),
        p(
            "slack_token",
            "Slack token",
            r"\bxox[baprs]-[0-9A-Za-z-]{10,}\b",
            Severity::High,
        ),
        p(
            "stripe_key",
            "Stripe live secret key",
            r"\bsk_live_[0-9A-Za-z]{16,}\b",
            Severity::Critical,
        ),
        p(
            "google_api_key",
            "Google API key",
            r"\bAIza[0-9A-Za-z_-]{35}\b",
            Severity::High,
        ),
        p(
            "jwt",
            "Signed JWT committed",
            r"\beyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\b",
            Severity::Medium,
        ),
        p(
            "connection_string",
            "Connection string with a password",
            r"(?i)\b(?:postgres(?:ql)?|mysql|mongodb(?:\+srv)?|redis|amqp)://[^\s:/@]+:[^\s@]{4,}@",
            Severity::High,
        ),
    ]
});

/// `password = "..."`, `api_key: '...'`, `SECRET=...` with a value that
/// does not look like a placeholder or a reference.
static ASSIGNMENT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)\b(?:password|passwd|pwd|secret|api[_-]?key|access[_-]?key|auth[_-]?token|token|client[_-]?secret|private[_-]?key)\b\s*[:=]{1,2}\s*["']([^"']{8,})["']"#,
    )
    .expect("assignment pattern")
});

/// `password: hunter2`, `"token": "abcd1234"`, `API_KEY=...` in a
/// configuration file: the key names above with any literal value (quoted
/// or not) of four or more characters.
static CONFIG_KEY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)^\s*-?\s*["']?(?:[\w.-]*(?:password|passwd|pwd|secret|api[_-]?key|access[_-]?key|auth[_-]?token|token|client[_-]?secret|private[_-]?key|secret[_-]?key|credentials?))["']?\s*[:=]\s*["']?([^"'\s#][^"'#]*?)["']?\s*[,;]?\s*(?:#.*)?$"#,
    )
    .expect("config key pattern")
});

/// Configuration formats the config-credential check reads.
fn is_config_file(file: &crate::domain::review::input::ChangedFile) -> bool {
    let name = file.file_name();
    if name.ends_with(".example")
        || name.ends_with(".sample")
        || name.ends_with(".template")
        || name.ends_with(".dist")
        || name.contains("example")
    {
        return false;
    }
    config_format(file)
}

/// A file in a configuration format, example files included.
fn config_format(file: &crate::domain::review::input::ChangedFile) -> bool {
    let name = file.file_name();
    matches!(
        file.extension(),
        "yml" | "yaml" | "json" | "toml" | "ini" | "cfg" | "conf" | "properties" | "env"
    ) || name == ".env"
        || name.starts_with(".env.")
}

/// Values that reference a secret instead of holding one.
fn reference(value: &str) -> bool {
    let v = value.trim();
    v.starts_with('$')
        || v.starts_with('%')
        || v.contains("${")
        || v.contains("{{")
        || v.contains("<%")
        || v.starts_with("vault:")
        || v.starts_with("op://")
        || v.starts_with("sops")
        || v.starts_with("ENC[")
        || v.starts_with("AQI")
        || v.starts_with("projects/")
        || v.starts_with("arn:")
        || v.starts_with("@")
        || v.starts_with("file:")
        || v.starts_with('/')
        || v.contains("secretKeyRef")
        || v.contains("valueFrom")
        || v.eq_ignore_ascii_case("null")
        || v.eq_ignore_ascii_case("none")
        || v.eq_ignore_ascii_case("true")
        || v.eq_ignore_ascii_case("false")
        || v == "~"
        || v == "\"\""
        || v == "''"
        || v.starts_with('[')
        || v.starts_with('{')
        || v.chars().all(|c| c.is_ascii_digit())
}

fn placeholder(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.contains("example")
        || lower.contains("changeme")
        || lower.contains("change-me")
        || lower.contains("placeholder")
        || lower.contains("dummy")
        || lower.contains("your_")
        || lower.contains("your-")
        || lower.contains("<")
        || lower.contains("${")
        || lower.contains("{{")
        || lower.contains("process.env")
        || lower.contains("env(")
        || lower.contains("os.environ")
        || lower.contains("xxx")
        || lower.contains("redacted")
        || lower.contains("secret-name")
        || lower.chars().all(|c| c == '*' || c == 'x')
}

fn config_path(path: &str) -> bool {
    config_format(&crate::domain::review::input::ChangedFile {
        path: path.to_owned(),
        ..Default::default()
    })
}

/// Whether a line of `path` may hold a secret, for redaction before source
/// lines leave the service (CONTRACT.md §19, §23.2). Deliberately wider
/// than the findings: every line a secrets rule would report matches, and
/// so does a credential-looking assignment whose value only looks like a
/// placeholder. Hiding one line too many costs a little context; showing
/// one too many cannot be undone.
pub fn line_may_hold_secret(path: &str, line: &str) -> bool {
    let line = line.strip_suffix('\r').unwrap_or(line);
    PATTERNS.iter().any(|p| p.regex.is_match(line))
        || ASSIGNMENT.is_match(line)
        || (config_path(path)
            && CONFIG_KEY
                .captures(line)
                .is_some_and(|c| c[1].trim().len() >= 4 && !reference(&c[1])))
}

pub fn analyze(input: &ReviewInput) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in &input.files {
        if file.is_removed() {
            continue;
        }
        // Fixtures and docs still get flagged, but as something to check,
        // not as a leak.
        let downgrade = file.is_test() || file.is_docs();
        for (line_no, line) in file.added_lines() {
            for pattern in PATTERNS.iter() {
                if pattern.regex.is_match(line) {
                    let severity = if downgrade {
                        Severity::Low
                    } else {
                        pattern.severity
                    };
                    findings.push(Finding::new(
                        "secrets",
                        pattern.check,
                        severity,
                        &file.path,
                        line_no,
                        pattern.title,
                        format!(
                            "Line {line_no} of `{}` adds what looks like a {}. Even if it is revoked later it stays in the \
                             history: remove it from the change, rotate it, and load it from the environment or a secret store.",
                            file.path,
                            pattern.title.to_ascii_lowercase()
                        ),
                    ));
                }
            }
            if is_config_file(file)
                && let Some(captures) = CONFIG_KEY.captures(line)
                && captures[1].trim().len() >= 4
                && !placeholder(&captures[1])
                && !reference(&captures[1])
                && !PATTERNS.iter().any(|p| p.regex.is_match(line))
            {
                let severity = if downgrade {
                    Severity::Low
                } else {
                    Severity::High
                };
                findings.push(Finding::new(
                    "secrets",
                    "config_credential",
                    severity,
                    &file.path,
                    line_no,
                    "Credential written into configuration",
                    format!(
                        "Line {line_no} of `{}` sets a credential-looking key to a literal value. Configuration files \
                         travel with the repository and its images: reference the secret (`${{VAR}}`, a secret store, \
                         `secretKeyRef`) and keep only a placeholder in a committed example file.",
                        file.path
                    ),
                ));
                continue;
            }
            if let Some(captures) = ASSIGNMENT.captures(line)
                && !placeholder(&captures[1])
                && !PATTERNS.iter().any(|p| p.regex.is_match(line))
            {
                let severity = if downgrade {
                    Severity::Low
                } else {
                    Severity::High
                };
                findings.push(Finding::new(
                    "secrets",
                    "hardcoded_credential",
                    severity,
                    &file.path,
                    line_no,
                    "Credential assigned in code",
                    format!(
                        "Line {line_no} of `{}` assigns a literal value to a credential-looking name. Read it from the \
                         environment or a secret store and keep a placeholder in `.env.example` instead.",
                        file.path
                    ),
                ));
            }
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::*;
    use super::*;

    #[test]
    fn redaction_covers_every_reported_line_and_more() {
        // Whatever the rules report is hidden.
        let lines = [
            ("config/aws.py", "ACCESS = \"AKIAIOSFODNN7EXAMPLE\""),
            ("deploy/server.go", "password := \"hunter2hunter2\""),
            (
                "deploy/server.go",
                "const key = `-----BEGIN RSA PRIVATE KEY-----`",
            ),
            ("config/app.yaml", "db_password: s3cr3t-value"),
            (
                "src/db.ts",
                "const url = 'postgres://app:s3cretpass@db.internal/app'",
            ),
        ];
        for (path, line) in lines {
            let reported = !analyze(&input(vec![added(path, &[line])])).is_empty();
            assert!(reported, "{line} should be a finding");
            assert!(line_may_hold_secret(path, line), "{line}");
            assert!(
                line_may_hold_secret(path, &format!("{line}\r")),
                "CRLF {line}"
            );
        }
        // A value that only looks like a placeholder is hidden all the same.
        assert!(line_may_hold_secret(
            "src/a.py",
            "password = \"changeme-but-real\""
        ));
        // References and ordinary code are not.
        for (path, line) in [
            ("config/app.yaml", "db_password: ${DB_PASSWORD}"),
            ("config/app.yaml", "replicas: 3"),
            ("src/a.rs", "let token = read_token(path)?;"),
            ("src/a.rs", "fn main() {"),
            // The config rule only reads configuration files.
            ("src/a.rs", "password: hunter2"),
        ] {
            assert!(!line_may_hold_secret(path, line), "{path}: {line}");
        }
        // An example file is not reported, but its lines are still hidden.
        assert!(
            analyze(&input(vec![added(
                ".env.example",
                &["API_TOKEN=abcd1234efgh"]
            )]))
            .is_empty()
        );
        assert!(line_may_hold_secret(
            ".env.example",
            "API_TOKEN=abcd1234efgh"
        ));
    }

    #[test]
    fn flags_cloud_keys_and_tokens() {
        let findings = analyze(&input(vec![added(
            "config/aws.py",
            &[
                "ACCESS = \"AKIAIOSFODNN7EXAMPLE\"",
                "GH = 'ghp_abcdefghijklmnopqrstuvwxyz0123456789ABCD'",
                "url = 'postgres://app:s3cretpass@db.internal/app'",
            ],
        )]));
        assert_eq!(
            rules(&findings),
            vec![
                "secrets.aws_key",
                "secrets.github_token",
                "secrets.connection_string"
            ]
        );
        assert_eq!(findings[0].severity, Severity::Critical);
        assert_eq!(findings[0].line, 1);
        assert!(!findings[0].detail.contains("AKIA"));
    }

    #[test]
    fn flags_private_keys_and_hardcoded_credentials() {
        let findings = analyze(&input(vec![added(
            "deploy/server.go",
            &[
                "const key = `-----BEGIN RSA PRIVATE KEY-----`",
                "password := \"hunter2hunter2\"",
            ],
        )]));
        assert_eq!(
            rules(&findings),
            vec!["secrets.private_key", "secrets.hardcoded_credential"]
        );
        assert_eq!(findings[1].severity, Severity::High);
    }

    #[test]
    fn config_credentials_are_literal_values_only() {
        let findings = analyze(&input(vec![
            added(
                "config/production.yml",
                &[
                    "database:",
                    "  password: hunter2secret",
                    "  user: app",
                    "  token: ${DB_TOKEN}",
                    "  api_key: \"\"",
                    "  secret_name: db-secret",
                ],
            ),
            added(
                "appsettings.json",
                &[
                    "  \"ConnectionPassword\": \"P@ssw0rd!\",",
                    "  \"Debug\": true,",
                ],
            ),
            added(
                "settings.toml",
                &["secret = \"${VAULT_SECRET}\"", "pwd = 's3cr3t-value-here'"],
            ),
            added(".env.example", &["API_KEY=replace-me-please"]),
            added(".env", &["API_KEY=live-key-value-123", "PORT=8080"]),
            added(
                "k8s/secret.yml",
                &["  valueFrom:", "    secretKeyRef:", "      key: password"],
            ),
        ]));
        let hits: Vec<(&str, u32)> = findings
            .iter()
            .filter(|f| f.rule == "secrets.config_credential")
            .map(|f| (f.path.as_str(), f.line))
            .collect();
        assert_eq!(
            hits,
            vec![
                ("config/production.yml", 2),
                ("appsettings.json", 1),
                ("settings.toml", 2),
                (".env", 1),
            ],
            "{findings:?}"
        );
        assert!(findings.iter().all(|f| !f.detail.contains("hunter2")));
    }

    #[test]
    fn fixtures_are_downgraded_not_ignored() {
        let findings = analyze(&input(vec![added(
            "tests/fixtures/keys.json",
            &["\"slack\": \"xoxb-123456789012-abcdefghijk\""],
        )]));
        assert_eq!(rules(&findings), vec!["secrets.slack_token"]);
        assert_eq!(findings[0].severity, Severity::Low);
    }

    #[test]
    fn placeholders_references_and_removals_are_clean() {
        let findings = analyze(&input(vec![
            added(
                "src/config.rs",
                &[
                    "let password = \"${DB_PASSWORD}\";",
                    "api_key: \"<your-api-key-here>\"",
                    "token = \"changeme-please\"",
                    "let secret = std::env::var(\"SECRET\");",
                ],
            ),
            file(
                "old.py",
                "removed",
                &[],
                &["KEY = \"AKIAIOSFODNN7EXAMPLE\""],
            ),
        ]));
        assert!(findings.is_empty(), "{findings:?}");
    }
}

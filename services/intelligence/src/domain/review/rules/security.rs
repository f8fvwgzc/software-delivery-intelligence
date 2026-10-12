//! Shell injection, `eval`, SQL built by string formatting, disabled TLS
//! verification, world-writable permissions and hard-coded hosts in added
//! lines. Tests and docs are skipped: a fixture that shells out is not a
//! vulnerability.

use std::sync::LazyLock;

use regex::Regex;

use super::{Finding, Severity, evidence};
use crate::domain::review::input::{ChangedFile, ReviewInput};

static SHELL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"shell\s*=\s*True|\bos\.system\(|\bos\.popen\(|child_process\.exec(?:Sync)?\(|\bexecSync\(\s*[`"']?.*[\$+]|\bexec\(\s*`[^`]*\$\{|Command::new\(\s*"(?:sh|bash|zsh|cmd)"\s*\)\s*\.args?\(\s*(?:\[)?\s*"-c"|system\(\s*".*#\{|\bsh\s+-c\s+["'`].*[\$+]"#,
    )
    .expect("shell pattern")
});

static EVAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?:^|[^\w.])eval\s*\(|new\s+Function\s*\(|\bexec\s*\(\s*(?:f?['"]|[a-z_]+\s*\))"#)
        .expect("eval pattern")
});

static SQL_KEYWORD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:SELECT\s+.+\s+FROM|INSERT\s+INTO|UPDATE\s+\w+\s+SET|DELETE\s+FROM|DROP\s+TABLE)\b")
        .expect("sql keyword pattern")
});

static SQL_BUILT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"format!\(|\bf["']|\$\{|%s|["']\s*\+\s*\w|\w\s*\+\s*["']|\.format\(|#\{"#)
        .expect("sql built pattern")
});

static TLS_OFF: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"verify\s*=\s*False|rejectUnauthorized\s*:\s*false|NODE_TLS_REJECT_UNAUTHORIZED\s*=\s*['"]?0|danger_accept_invalid_certs\s*\(\s*true|InsecureSkipVerify\s*:\s*true|CURLOPT_SSL_VERIFYPEER\s*,\s*(?:false|0)|\bcurl\b[^\n]*\s(?:-k|--insecure)\b|sslmode\s*=\s*disable|verify_ssl\s*=\s*False|VERIFY_NONE|strict-ssl\s*=\s*false|GIT_SSL_NO_VERIFY"#,
    )
    .expect("tls pattern")
});

static WORLD_WRITABLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"chmod\s+(?:-R\s+)?(?:0?777|a\+rwx|o\+w)\b|0o777\b|\b0777\b|mode\s*=\s*['"]?0?777"#,
    )
    .expect("chmod pattern")
});

static HOST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"https?://(?:\d{1,3}\.){3}\d{1,3}(?::\d+)?|["'](?:\d{1,3}\.){3}\d{1,3}(?::\d+)?["']|https?://[a-z0-9-]+(?:\.[a-z0-9-]+)+(?::\d+)?/?"#)
        .expect("host pattern")
});

/// Hosts that are fine to spell out: loopback, unspecified, documentation
/// ranges, and the well-known public endpoints code legitimately targets.
fn benign_host(matched: &str) -> bool {
    let lower = matched.to_ascii_lowercase();
    lower.contains("127.0.0.1")
        || lower.contains("0.0.0.0")
        || lower.contains("localhost")
        || lower.contains("192.0.2.")
        || lower.contains("198.51.100.")
        || lower.contains("203.0.113.")
        || lower.contains("example.com")
        || lower.contains("example.org")
        || lower.contains("github.com")
        || lower.contains("githubusercontent.com")
        || lower.contains("schemas.")
        || lower.contains("w3.org")
        || lower.contains("json-schema.org")
        || lower.contains("docs.")
        || lower.contains("api.anthropic.com")
        || lower.contains("registry.npmjs.org")
        || lower.contains("crates.io")
        || lower.contains("pypi.org")
        || lower.contains("docker.io")
        || lower.contains("ghcr.io")
        || lower.contains("quay.io")
        || lower.contains("mcr.microsoft.com")
        || lower.contains("gcr.io")
        || lower.contains("amazonaws.com")
        || lower.contains("xmlns")
        || lower.contains("license")
        || lower.contains("apache.org")
        || lower.contains("opensource.org")
        || lower.contains("spdx.org")
}

fn is_code(file: &ChangedFile) -> bool {
    !file.is_test() && !file.is_docs() && !file.is_removed() && !file.is_lockfile()
}

fn is_comment(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("//") || t.starts_with('#') || t.starts_with("*") || t.starts_with("/*")
}

pub fn analyze(input: &ReviewInput) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in input.files.iter().filter(|f| is_code(f)) {
        let path = &file.path;
        for (line_no, line) in file.added_lines() {
            if is_comment(line) {
                continue;
            }
            if SHELL.is_match(line) {
                findings.push(Finding::new(
                    "security",
                    "shell_injection",
                    Severity::High,
                    path,
                    line_no,
                    "Command built for a shell",
                    format!(
                        "Line {line_no} runs a command through a shell with interpolated input: {}. Pass the program and \
                         its arguments as a list (no shell), and validate anything that comes from outside.",
                        evidence(line)
                    ),
                ));
            }
            if EVAL.is_match(line)
                && matches!(
                    file.extension(),
                    "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" | "py" | "rb" | "php"
                )
            {
                findings.push(Finding::new(
                    "security",
                    "eval",
                    Severity::High,
                    path,
                    line_no,
                    "Dynamic code evaluation",
                    format!(
                        "Line {line_no} evaluates a string as code: {}. Whatever reaches it runs with the program's \
                         rights; replace it with a parser, a lookup table or an explicit call.",
                        evidence(line)
                    ),
                ));
            }
            if SQL_KEYWORD.is_match(line) && SQL_BUILT.is_match(line) {
                findings.push(Finding::new(
                    "security",
                    "sql_format",
                    Severity::High,
                    path,
                    line_no,
                    "SQL built by string formatting",
                    format!(
                        "Line {line_no} assembles SQL from formatted or concatenated strings: {}. Use bound parameters \
                         (`$1`, `?`, `%s` with a parameter list) so values can never change the statement.",
                        evidence(line)
                    ),
                ));
            }
            if TLS_OFF.is_match(line) {
                findings.push(Finding::new(
                    "security",
                    "tls_disabled",
                    Severity::High,
                    path,
                    line_no,
                    "TLS verification disabled",
                    format!(
                        "Line {line_no} turns certificate verification off: {}. The connection then accepts any \
                         certificate; trust the right CA bundle instead, or scope the exception to a local test.",
                        evidence(line)
                    ),
                ));
            }
            if WORLD_WRITABLE.is_match(line) {
                findings.push(Finding::new(
                    "security",
                    "world_writable",
                    Severity::Medium,
                    path,
                    line_no,
                    "World-writable permissions",
                    format!(
                        "Line {line_no} makes a path writable by everyone: {}. Grant the owning user or group what it \
                         needs (`755`, `644`, or `770` for a shared group) and nothing more.",
                        evidence(line)
                    ),
                ));
            }
            if let Some(found) = HOST.find(line)
                && !benign_host(found.as_str())
                && !matches!(
                    file.extension(),
                    "json" | "lock" | "svg" | "html" | "xml" | "csv" | "yml" | "yaml" | "toml"
                )
            {
                findings.push(Finding::new(
                    "security",
                    "hardcoded_host",
                    Severity::Low,
                    path,
                    line_no,
                    "Hard-coded host",
                    format!(
                        "Line {line_no} names a host in code: {}. Environments differ; read the address from \
                         configuration so staging never talks to production.",
                        evidence(found.as_str())
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
    fn flags_shell_and_eval() {
        let findings = analyze(&input(vec![
            added(
                "app/run.py",
                &[
                    "subprocess.run(cmd, shell=True)",
                    "os.system(\"rm -rf \" + path)",
                    "result = eval(user_input)",
                ],
            ),
            added(
                "src/exec.rs",
                &["Command::new(\"sh\").arg(\"-c\").arg(format!(\"ls {dir}\"))"],
            ),
        ]));
        assert_eq!(
            rules(&findings),
            vec![
                "security.shell_injection",
                "security.shell_injection",
                "security.eval",
                "security.shell_injection"
            ]
        );
        assert!(findings.iter().all(|f| f.severity == Severity::High));
    }

    #[test]
    fn flags_sql_built_by_format_and_tls_off() {
        let findings = analyze(&input(vec![
            added(
                "src/db.rs",
                &["let q = format!(\"SELECT * FROM users WHERE id = {id}\");"],
            ),
            added(
                "src/client.ts",
                &["const agent = new https.Agent({ rejectUnauthorized: false });"],
            ),
            added("scripts/fetch.py", &["requests.get(url, verify=False)"]),
        ]));
        assert_eq!(
            rules(&findings),
            vec![
                "security.sql_format",
                "security.tls_disabled",
                "security.tls_disabled"
            ]
        );
    }

    #[test]
    fn flags_chmod_777_and_hardcoded_hosts() {
        let findings = analyze(&input(vec![
            added("deploy/setup.sh", &["chmod -R 777 /var/app"]),
            added(
                "src/api.ts",
                &["const base = \"http://10.0.12.4:8080/api\";"],
            ),
        ]));
        assert_eq!(
            rules(&findings),
            vec!["security.world_writable", "security.hardcoded_host"]
        );
        assert_eq!(findings[1].severity, Severity::Low);
    }

    #[test]
    fn parameterised_sql_tests_comments_and_benign_hosts_are_clean() {
        let findings = analyze(&input(vec![
            added(
                "src/db.rs",
                &[
                    "sqlx::query(\"SELECT * FROM users WHERE id = $1\").bind(id)",
                    "// os.system(\"legacy\")",
                    "let url = \"http://localhost:3000\";",
                    "const docs = \"https://github.com/o/r\";",
                ],
            ),
            added("tests/shell.py", &["subprocess.run(cmd, shell=True)"]),
            added("README.md", &["Run `curl -k https://10.0.0.1`"]),
        ]));
        assert!(findings.is_empty(), "{findings:?}");
    }
}

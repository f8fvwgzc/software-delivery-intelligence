//! Repository-defined rules from `.sdi.yml` `review.rules` (CONTRACT.md
//! §12): a regex over the added lines of the files each rule's `paths`
//! select. The pattern was validated when the config was parsed; one that
//! still fails to compile is skipped, never guessed at.

use regex::Regex;

use super::{Finding, Severity, evidence};
use crate::domain::review::input::ReviewInput;

pub fn severity_from_str(raw: &str) -> Option<Severity> {
    Some(match raw.trim().to_ascii_lowercase().as_str() {
        "info" => Severity::Info,
        "low" => Severity::Low,
        "medium" => Severity::Medium,
        "high" => Severity::High,
        "critical" => Severity::Critical,
        _ => return None,
    })
}

pub fn analyze(input: &ReviewInput) -> Vec<Finding> {
    let mut findings = Vec::new();
    for rule in &input.config.rules {
        if rule.id.is_empty() || rule.pattern.is_empty() {
            continue;
        }
        let Ok(regex) = Regex::new(&rule.pattern) else {
            continue;
        };
        let severity = severity_from_str(&rule.severity).unwrap_or(Severity::Low);
        for file in input.files.iter().filter(|f| f.is_reviewable()) {
            if !rule.paths.is_empty() && !sdi_analyzer::matches_any_glob(&file.path, &rule.paths) {
                continue;
            }
            for (line_no, line) in file.added_lines() {
                if regex.is_match(line) {
                    findings.push(Finding::new(
                        "custom",
                        &rule.id,
                        severity,
                        &file.path,
                        line_no,
                        rule.message.clone(),
                        format!(
                            "Line {line_no} of `{}` matches the repository rule `{}` from `.sdi.yml`: {}.",
                            file.path,
                            rule.id,
                            evidence(line)
                        ),
                    ));
                }
            }
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use sdi_analyzer::CustomRuleDraft;

    use super::super::fixtures::*;
    use super::*;
    use crate::domain::review::input::ReviewConfig;

    #[test]
    fn custom_rules_match_added_lines_of_selected_paths() {
        let mut input = input(vec![
            added(
                "web/src/a.ts",
                &["import moment from 'moment';", "const x = 1;"],
            ),
            added("api/src/b.ts", &["import moment from 'moment';"]),
            added("web/src/c.ts", &["// JIRA-123"]),
        ]);
        input.config = ReviewConfig {
            rules: vec![
                CustomRuleDraft {
                    id: "no-moment".into(),
                    pattern: "from ['\"]moment['\"]".into(),
                    severity: "high".into(),
                    message: "Use date-fns".into(),
                    paths: vec!["web/**".into()],
                },
                CustomRuleDraft {
                    id: "ticket".into(),
                    pattern: r"JIRA-\d+".into(),
                    severity: "nope".into(),
                    message: String::new(),
                    paths: vec![],
                },
                CustomRuleDraft {
                    id: "broken".into(),
                    pattern: "(".into(),
                    severity: "low".into(),
                    message: String::new(),
                    paths: vec![],
                },
            ],
            ..Default::default()
        };
        let findings = analyze(&input);
        let hits: Vec<(&str, &str, u32)> = findings
            .iter()
            .map(|f| (f.rule.as_str(), f.path.as_str(), f.line))
            .collect();
        assert_eq!(
            hits,
            vec![
                ("custom.no-moment", "web/src/a.ts", 1),
                ("custom.ticket", "web/src/c.ts", 1),
            ]
        );
        assert_eq!(findings[0].severity, Severity::High);
        assert_eq!(findings[0].title, "Use date-fns");
        assert_eq!(findings[1].severity, Severity::Low);
        assert_eq!(severity_from_str("Critical"), Some(Severity::Critical));
    }
}

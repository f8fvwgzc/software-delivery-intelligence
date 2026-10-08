//! Test frameworks from dependencies and test files from the tree. A test
//! suite becomes a node the service is TESTED_BY.

use crate::detectors::{Ctx, attrs};
use crate::model::*;
use crate::util::{basename, display, line_with};

/// (ecosystem, dependency, framework key, e2e?)
const FRAMEWORKS: &[(&str, &str, &str, bool)] = &[
    ("npm", "vitest", "vitest", false),
    ("npm", "jest", "jest", false),
    ("npm", "mocha", "mocha", false),
    ("npm", "@playwright/test", "playwright", true),
    ("npm", "cypress", "cypress", true),
    ("cargo", "proptest", "proptest", false),
    ("cargo", "criterion", "criterion", false),
    ("cargo", "insta", "insta", false),
    ("pip", "pytest", "pytest", false),
    ("pip", "playwright", "playwright", true),
    ("maven", "org.junit.jupiter:junit-jupiter", "junit", false),
    (
        "maven",
        "org.springframework.boot:spring-boot-starter-test",
        "junit",
        false,
    ),
    ("maven", "junit:junit", "junit", false),
    ("bundler", "rspec", "rspec", false),
    ("bundler", "rspec-rails", "rspec", false),
    ("composer", "phpunit/phpunit", "phpunit", false),
];

fn is_test_file(path: &str) -> bool {
    let name = basename(path);
    name.contains(".test.")
        || name.contains(".spec.")
        || (name.starts_with("test_") && name.ends_with(".py"))
        || name.ends_with("_test.go")
        || name.ends_with("_test.rs")
        || name.ends_with("_test.exs")
        || name.ends_with("_spec.rb")
        || name.ends_with("Test.java")
        || name.ends_with("Test.kt")
        || ((path.starts_with("tests/") || path.contains("/tests/")) && name.ends_with(".rs"))
}

pub(crate) fn detect(ctx: &mut Ctx<'_>) {
    let mut suites: Vec<(String, bool, String)> = Vec::new();
    for m in ctx.manifests.clone() {
        let content = ctx.file(&m.path).unwrap_or("");
        for (eco, dep, key, e2e) in FRAMEWORKS {
            if m.ecosystem != *eco || !m.has_dep(dep) {
                continue;
            }
            if !suites.iter().any(|(k, _, _)| k == key) {
                let evidence = line_with(content, dep).unwrap_or(dep).to_string();
                ctx.detected(
                    FactCategory::Testing,
                    &format!("testing:{key}"),
                    &display(key),
                    if *e2e { "end-to-end" } else { "unit" },
                    &m.path,
                    &evidence,
                );
                suites.push((key.to_string(), *e2e, m.path.clone()));
            }
        }
        // Built-in runners have no dependency: look at the layout instead.
        let builtin = match m.ecosystem.as_str() {
            "cargo" => Some(("cargo_test", "tests/ or *_test.rs")),
            "go" => Some(("go_test", "*_test.go")),
            "mix" => Some(("exunit", "test/*_test.exs")),
            _ => None,
        };
        if let Some((key, hint)) = builtin {
            let has = ctx.tree_has(|p| {
                is_test_file(p)
                    && (m.ecosystem != "cargo" || p.ends_with(".rs"))
                    && (m.ecosystem != "go" || p.ends_with(".go"))
                    && (m.ecosystem != "mix" || p.ends_with(".exs"))
            });
            if has && !suites.iter().any(|(k, _, _)| k == key) {
                ctx.detected(
                    FactCategory::Testing,
                    &format!("testing:{key}"),
                    &display(key),
                    "unit",
                    &m.path,
                    hint,
                );
                suites.push((key.to_string(), false, m.path.clone()));
            }
        }
        if let Some(script) = m.scripts.get("test")
            && !script.contains("no test specified")
            && !ctx.has_fact("testing:script")
        {
            ctx.detected(
                FactCategory::Testing,
                "testing:script",
                "Test script",
                script,
                &m.path,
                &format!("\"test\": \"{script}\""),
            );
        }
    }

    let test_files = ctx.tree_paths().filter(|p| is_test_file(p)).count();
    if test_files > 0 {
        let sample = ctx
            .tree_paths()
            .find(|p| is_test_file(p))
            .unwrap_or("")
            .to_string();
        ctx.detected(
            FactCategory::Testing,
            "testing:files",
            "Test files",
            &format!("{test_files} files"),
            "",
            &sample,
        );
    }

    let has_unit =
        suites.iter().any(|(_, e2e, _)| !e2e) || test_files > 0 || ctx.has_fact("testing:script");
    let has_e2e = suites.iter().any(|(_, e2e, _)| *e2e);
    if has_unit {
        ctx.detected(
            FactCategory::Testing,
            "tests:unit",
            "Unit tests",
            "present",
            "",
            &suites
                .iter()
                .filter(|(_, e2e, _)| !e2e)
                .map(|(k, _, _)| k.clone())
                .collect::<Vec<_>>()
                .join(", "),
        );
    }
    if has_e2e {
        ctx.detected(
            FactCategory::Testing,
            "tests:e2e",
            "End-to-end tests",
            "present",
            "",
            &suites
                .iter()
                .filter(|(_, e2e, _)| *e2e)
                .map(|(k, _, _)| k.clone())
                .collect::<Vec<_>>()
                .join(", "),
        );
    }

    for (key, e2e, manifest) in suites {
        let node_key = format!("test_suite:{key}");
        ctx.node(
            NodeKind::TestSuite,
            &node_key,
            &display(&key),
            &format!(
                "{} {} tests",
                display(&key),
                if e2e { "end-to-end" } else { "unit" }
            ),
            &manifest,
            attrs(&[
                ("framework", &key),
                ("kind", if e2e { "e2e" } else { "unit" }),
            ]),
        );
        let app = ctx
            .app_for_manifest(&manifest)
            .map(|a| a.key.clone())
            .or_else(|| ctx.primary_service_key());
        if let Some(app) = app {
            ctx.edge(&app, &node_key, "TESTED_BY", RelationCategory::Validation);
        }
    }
}

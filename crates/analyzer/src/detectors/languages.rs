//! Languages come from the provider's byte counts when present and from
//! manifests and file extensions otherwise, so fixtures without provider
//! metadata still get `lang:*` facts. A plain HTML/CSS/JS tree with no
//! manifest is a static site; a Vite build without a server framework is
//! one too, and `frameworks` adds that case once the frameworks are known.

use std::collections::BTreeMap;

use crate::detectors::Ctx;
use crate::model::*;
use crate::util::{basename, display, extension};

/// Provider language names → capability keys. Non-programming languages map to
/// nothing so they never become the primary language.
fn key_for_language(name: &str) -> Option<&'static str> {
    Some(match name.to_ascii_lowercase().as_str() {
        "rust" => "rust",
        "typescript" => "typescript",
        "javascript" => "javascript",
        "python" => "python",
        "go" => "go",
        "java" => "java",
        "kotlin" => "kotlin",
        "c#" | "csharp" => "csharp",
        "ruby" => "ruby",
        "php" => "php",
        "elixir" => "elixir",
        "swift" => "swift",
        "scala" => "scala",
        "dart" => "dart",
        "perl" => "perl",
        "c++" | "cpp" => "cpp",
        "c" => "c",
        "html" => "html",
        "css" | "scss" => "css",
        "shell" => "shell",
        "vue" => "vue",
        "svelte" => "svelte",
        _ => return None,
    })
}

fn key_for_extension(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "rs" => "rust",
        "ts" | "tsx" | "mts" | "cts" => "typescript",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "py" => "python",
        "go" => "go",
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "cs" => "csharp",
        "rb" => "ruby",
        "php" => "php",
        "ex" | "exs" => "elixir",
        "swift" => "swift",
        "scala" => "scala",
        "dart" => "dart",
        "pl" | "pm" => "perl",
        "cpp" | "cc" | "cxx" | "hpp" | "hh" => "cpp",
        "c" | "h" => "c",
        "html" | "htm" => "html",
        "css" | "scss" => "css",
        "sh" | "bash" => "shell",
        "vue" => "vue",
        "svelte" => "svelte",
        _ => return None,
    })
}

pub(crate) fn detect(ctx: &mut Ctx<'_>) {
    // Ordered by weight: provider bytes, else file counts.
    let mut weights: BTreeMap<&'static str, u64> = BTreeMap::new();
    let mut evidence: BTreeMap<&'static str, String> = BTreeMap::new();
    for (name, bytes) in &ctx.snap.languages {
        if let Some(k) = key_for_language(name) {
            *weights.entry(k).or_default() += *bytes;
            evidence.insert(k, format!("{name}: {bytes} bytes (provider)"));
        }
    }
    if weights.is_empty() {
        let mut counts: BTreeMap<&'static str, u64> = BTreeMap::new();
        for p in ctx.tree_paths() {
            if let Some(k) = key_for_extension(&extension(p)) {
                *counts.entry(k).or_default() += 1;
            }
        }
        for (k, n) in counts {
            weights.insert(k, n);
            evidence.insert(k, format!("{n} *.{k} files"));
        }
    }

    // Manifests settle ambiguous cases (a Vite app is TypeScript if tsconfig exists).
    for m in ctx.manifests.clone() {
        let (k, ev) = match m.ecosystem.as_str() {
            "cargo" => ("rust", "Cargo.toml"),
            "pip" => ("python", "pyproject.toml / requirements"),
            "go" => ("go", "go.mod"),
            "bundler" => ("ruby", "Gemfile"),
            "composer" => ("php", "composer.json"),
            "mix" => ("elixir", "mix.exs"),
            "nuget" => ("csharp", "*.csproj"),
            "cpan" => ("perl", "cpanfile"),
            "maven" => {
                if m.path.ends_with(".kts") || m.deps.iter().any(|d| d.name.contains("kotlin")) {
                    ("kotlin", "build.gradle.kts")
                } else {
                    ("java", "pom.xml / build.gradle")
                }
            }
            "npm" => {
                let ts = m.has_dep("typescript")
                    || ctx.tree_has(|p| basename(p).starts_with("tsconfig"));
                if ts {
                    ("typescript", "tsconfig.json / typescript dependency")
                } else {
                    ("javascript", "package.json")
                }
            }
            _ => continue,
        };
        weights.entry(k).or_insert(1);
        evidence
            .entry(k)
            .or_insert_with(|| format!("{} ({ev})", m.path));
    }

    let mut ordered: Vec<(&str, u64)> = weights.into_iter().collect();
    ordered.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    let primary = key_for_language(&ctx.snap.primary_language).or_else(|| {
        ordered
            .iter()
            .map(|(k, _)| *k)
            .find(|k| !matches!(*k, "html" | "css" | "shell"))
    });
    for (k, _) in &ordered {
        let label = display(k);
        let value = if Some(*k) == primary {
            "primary"
        } else {
            "secondary"
        };
        ctx.detected(
            FactCategory::Language,
            &format!("lang:{k}"),
            &label,
            value,
            "",
            evidence.get(k).map(String::as_str).unwrap_or(""),
        );
    }

    let only_web = !ordered.is_empty()
        && ordered
            .iter()
            .all(|(k, _)| matches!(*k, "html" | "css" | "javascript" | "shell"));
    let has_index = ctx.tree_has(|p| p == "index.html" || p == "public/index.html");
    if only_web
        && has_index
        && ctx
            .manifests
            .iter()
            .all(|m| m.ecosystem != "npm" || m.deps.is_empty())
    {
        ctx.detected(
            FactCategory::Repository,
            "repository:static_site",
            "Static site",
            "HTML/CSS/JS without a build step",
            "index.html",
            "index.html at the repository root",
        );
    }
}

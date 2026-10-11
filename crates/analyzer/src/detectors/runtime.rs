//! Runtime versions and their support status. Every version file the
//! ecosystems use is read (`.nvmrc`, `.tool-versions`, `engines`,
//! `runtime.txt`, `requires-python`, `maven.compiler.*`, `<TargetFramework>`,
//! `go.mod`, `Cargo.toml`, base images) and compared with the curated table
//! in [`crate::eol`] on the day the caller passed in.
//!
//! Facts: `runtime:<name>:<version>` for every version found and
//! `runtime:eol:<name>` when one of them is past its end of life, with a
//! sentence that names the file it was read from.

use std::collections::BTreeMap;

use regex::Regex;

use crate::detectors::Ctx;
use crate::eol::{self, format_day, label};
use crate::files::is_dockerfile;
use crate::model::*;
use crate::util::{basename, line_with};

/// One version read from one place.
struct Hit {
    runtime: &'static str,
    version: String,
    path: String,
    evidence: String,
}

fn hit(runtime: &'static str, version: Option<String>, path: &str, evidence: &str) -> Option<Hit> {
    version.map(|version| Hit {
        runtime,
        version,
        path: path.to_string(),
        evidence: evidence.to_string(),
    })
}

fn first_line(content: &str) -> &str {
    content
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#'))
        .unwrap_or("")
}

pub(crate) fn detect(ctx: &mut Ctx<'_>) {
    let mut hits: Vec<Hit> = Vec::new();

    for p in [".nvmrc", ".node-version"] {
        if let Some(c) = ctx.file(p) {
            let v = first_line(c);
            hits.extend(hit(eol::NODE, eol::node_major(v), p, &format!("{p}: {v}")));
        }
    }
    if let Some(c) = ctx.file(".tool-versions") {
        for line in c.lines() {
            let mut parts = line.split_whitespace();
            let (Some(tool), Some(v)) = (parts.next(), parts.next()) else {
                continue;
            };
            let ev = format!(".tool-versions: {tool} {v}");
            let h = match tool {
                "nodejs" | "node" => hit(eol::NODE, eol::node_major(v), ".tool-versions", &ev),
                "python" => hit(eol::PYTHON, eol::python_minor(v), ".tool-versions", &ev),
                "ruby" => hit(eol::RUBY, eol::ruby_minor(v), ".tool-versions", &ev),
                "golang" | "go" => hit(eol::GO, eol::go_minor(v), ".tool-versions", &ev),
                "java" => hit(eol::JAVA, eol::java_major(v), ".tool-versions", &ev),
                "php" => hit(eol::PHP, eol::php_minor(v), ".tool-versions", &ev),
                "rust" => hit(eol::RUST, eol::go_minor(v), ".tool-versions", &ev),
                "dotnet" | "dotnet-core" => {
                    let major =
                        eol::go_minor(v).and_then(|mm| mm.split('.').next().map(String::from));
                    match major.as_deref() {
                        Some("1" | "2" | "3") => {
                            hit(eol::DOTNET_CORE, eol::go_minor(v), ".tool-versions", &ev)
                        }
                        _ => hit(eol::DOTNET, major, ".tool-versions", &ev),
                    }
                }
                _ => None,
            };
            hits.extend(h);
        }
    }
    if let Some(c) = ctx.file("runtime.txt") {
        let v = first_line(c);
        if v.starts_with("python") {
            hits.extend(hit(
                eol::PYTHON,
                eol::python_minor(v),
                "runtime.txt",
                &format!("runtime.txt: {v}"),
            ));
        }
    }
    if let Some(c) = ctx.file(".python-version") {
        let v = first_line(c);
        hits.extend(hit(
            eol::PYTHON,
            eol::python_minor(v),
            ".python-version",
            &format!(".python-version: {v}"),
        ));
    }
    if let Some(c) = ctx.file(".ruby-version") {
        let v = first_line(c);
        hits.extend(hit(
            eol::RUBY,
            eol::ruby_minor(v),
            ".ruby-version",
            &format!(".ruby-version: {v}"),
        ));
    }
    if let Some(c) = ctx.file(".go-version") {
        let v = first_line(c);
        hits.extend(hit(
            eol::GO,
            eol::go_minor(v),
            ".go-version",
            &format!(".go-version: {v}"),
        ));
    }
    if let Some(c) = ctx.file(".java-version") {
        let v = first_line(c);
        hits.extend(hit(
            eol::JAVA,
            eol::java_major(v),
            ".java-version",
            &format!(".java-version: {v}"),
        ));
    }
    if let Some(c) = ctx.file(".sdkmanrc")
        && let Some(line) = line_with(c, "java=")
    {
        let v = line.trim_start_matches("java=").trim();
        hits.extend(hit(
            eol::JAVA,
            eol::java_major(v),
            ".sdkmanrc",
            &format!(".sdkmanrc: {line}"),
        ));
    }
    if let Some(c) = ctx.file("global.json")
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(c)
        && let Some(sdk) = v["sdk"]["version"].as_str()
    {
        let ev = format!("global.json: sdk {sdk}");
        let major = sdk.split('.').next().unwrap_or("");
        let h = match major {
            "1" | "2" | "3" => hit(eol::DOTNET_CORE, eol::go_minor(sdk), "global.json", &ev),
            _ => hit(eol::DOTNET, eol::node_major(sdk), "global.json", &ev),
        };
        hits.extend(h);
    }
    for (p, c) in ctx.files_where(|p| basename(p).starts_with("rust-toolchain")) {
        let v = if p.ends_with(".toml") {
            line_with(c, "channel")
                .and_then(|l| l.split('=').nth(1))
                .map(|s| s.trim().trim_matches('"').to_string())
                .unwrap_or_default()
        } else {
            first_line(c).to_string()
        };
        if v.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
            hits.extend(hit(eol::RUST, eol::go_minor(&v), p, &format!("{p}: {v}")));
        }
    }

    // Manifests: engines, requires-python, compiler level, TFMs, go directive, edition.
    let pom_tags = Regex::new(
        r"<(maven\.compiler\.(?:source|target|release)|java\.version|release|source)>\s*([^<]+?)\s*</",
    )
    .expect("static regex");
    let gradle_java = Regex::new(
        r"(?:sourceCompatibility|targetCompatibility)\s*=?\s*(?:JavaVersion\.)?(VERSION_[\d_]+|['\x22]?[\d.]+['\x22]?)|jvmToolchain\s*\(\s*(\d+)\s*\)|JavaLanguageVersion\.of\s*\(\s*(\d+)\s*\)",
    )
    .expect("static regex");
    let tfm = Regex::new(
        r"<TargetFrameworks?>\s*([^<]+?)\s*</TargetFrameworks?>|<TargetFrameworkVersion>\s*([^<]+?)\s*</TargetFrameworkVersion>",
    )
    .expect("static regex");
    let web_config_tfm = Regex::new(r#"targetFramework\s*=\s*"([^"]+)""#).expect("static regex");
    for m in ctx.manifests.clone() {
        let Some(content) = ctx.file(&m.path) else {
            continue;
        };
        match m.ecosystem.as_str() {
            "npm" => {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(content)
                    && let Some(node) = v["engines"]["node"].as_str()
                {
                    hits.extend(hit(
                        eol::NODE,
                        eol::node_major(node),
                        &m.path,
                        &format!("{}: engines.node {node}", m.path),
                    ));
                }
            }
            "pip" if basename(&m.path) == "pyproject.toml" => {
                if let Ok(t) = content.parse::<toml::Table>() {
                    let requires = t
                        .get("project")
                        .and_then(|p| p.get("requires-python"))
                        .and_then(|v| v.as_str())
                        .map(|s| ("requires-python", s.to_string()))
                        .or_else(|| {
                            t.get("tool")
                                .and_then(|t| t.get("poetry"))
                                .and_then(|p| p.get("dependencies"))
                                .and_then(|d| d.get("python"))
                                .and_then(|v| v.as_str())
                                .map(|s| ("tool.poetry.dependencies.python", s.to_string()))
                        });
                    if let Some((what, spec)) = requires {
                        hits.extend(hit(
                            eol::PYTHON,
                            eol::python_minor(&spec),
                            &m.path,
                            &format!("{}: {what} {spec}", m.path),
                        ));
                    }
                }
            }
            "maven" => {
                let mut found = false;
                for cap in pom_tags.captures_iter(content) {
                    let v = cap[2].trim();
                    if v.starts_with("${") {
                        continue;
                    }
                    if let Some(h) = hit(
                        eol::JAVA,
                        eol::java_major(v),
                        &m.path,
                        &format!("{}: <{}>{v}", m.path, &cap[1]),
                    ) {
                        hits.push(h);
                        found = true;
                        break;
                    }
                }
                if !found {
                    for cap in gradle_java.captures_iter(content) {
                        let raw = cap
                            .get(1)
                            .or_else(|| cap.get(2))
                            .or_else(|| cap.get(3))
                            .map(|v| v.as_str().trim_matches(['\'', '"']))
                            .unwrap_or("");
                        if let Some(h) = hit(
                            eol::JAVA,
                            eol::java_major(raw),
                            &m.path,
                            &format!("{}: {}", m.path, cap[0].trim()),
                        ) {
                            hits.push(h);
                            break;
                        }
                    }
                }
            }
            "nuget" => {
                for cap in tfm.captures_iter(content) {
                    let raw = cap
                        .get(1)
                        .or_else(|| cap.get(2))
                        .map(|v| v.as_str())
                        .unwrap_or("");
                    for one in raw.split(';') {
                        if let Some((rt, v)) = eol::dotnet_tfm(one) {
                            hits.push(Hit {
                                runtime: rt,
                                version: v,
                                path: m.path.clone(),
                                evidence: format!("{}: {}", m.path, one.trim()),
                            });
                        }
                    }
                }
            }
            "go" => {
                if let Some(line) = content
                    .lines()
                    .map(str::trim)
                    .find(|l| l.starts_with("go "))
                {
                    let v = line.trim_start_matches("go ").trim();
                    hits.extend(hit(
                        eol::GO,
                        eol::go_minor(v),
                        &m.path,
                        &format!("{}: {line}", m.path),
                    ));
                }
            }
            "cargo" => {
                if let Ok(t) = content.parse::<toml::Table>() {
                    let pkg = t.get("package");
                    let ws = t.get("workspace").and_then(|w| w.get("package"));
                    let get = |k: &str| {
                        pkg.and_then(|p| p.get(k))
                            .or_else(|| ws.and_then(|p| p.get(k)))
                            .and_then(|v| v.as_str())
                            .map(String::from)
                    };
                    if let Some(rv) = get("rust-version") {
                        hits.extend(hit(
                            eol::RUST,
                            eol::go_minor(&rv),
                            &m.path,
                            &format!("{}: rust-version = \"{rv}\"", m.path),
                        ));
                    }
                    // The edition is a separate fact from the MSRV: §13 keys
                    // runtime currency on it, and a workspace usually declares
                    // both, so one must never hide the other.
                    if let Some(ed) = get("edition") {
                        hits.push(Hit {
                            runtime: eol::RUST,
                            version: format!("edition-{ed}"),
                            path: m.path.clone(),
                            evidence: format!("{}: edition = \"{ed}\"", m.path),
                        });
                    }
                }
            }
            "bundler" => {
                if let Some(line) = line_with(content, "ruby ").filter(|l| l.starts_with("ruby ")) {
                    let v = line
                        .trim_start_matches("ruby ")
                        .trim()
                        .trim_matches(['\'', '"']);
                    hits.extend(hit(
                        eol::RUBY,
                        eol::ruby_minor(v),
                        &m.path,
                        &format!("{}: {line}", m.path),
                    ));
                }
            }
            "composer" => {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(content)
                    && let Some(php) = v["require"]["php"].as_str()
                {
                    hits.extend(hit(
                        eol::PHP,
                        eol::php_minor(php),
                        &m.path,
                        &format!("{}: require.php {php}", m.path),
                    ));
                }
            }
            _ => {}
        }
    }
    for (p, c) in ctx.files_where(|p| basename(p).eq_ignore_ascii_case("web.config")) {
        if let Some(cap) = web_config_tfm.captures(c)
            && let Some((rt, v)) = eol::dotnet_framework_version(&cap[1])
        {
            hits.push(Hit {
                runtime: rt,
                version: v,
                path: p.to_string(),
                evidence: format!("{p}: targetFramework=\"{}\"", &cap[1]),
            });
        }
    }

    // Base images: Dockerfiles and the images compose / Kubernetes run.
    for (p, c) in ctx.files_where(|p| is_dockerfile(basename(p))) {
        for line in c.lines().map(str::trim) {
            if line.to_ascii_uppercase().starts_with("FROM ") {
                let image = line[5..].split_whitespace().next().unwrap_or("");
                for (rt, v) in eol::image_runtimes(image) {
                    hits.push(Hit {
                        runtime: rt,
                        version: v,
                        path: p.to_string(),
                        evidence: format!("{p}: {line}"),
                    });
                }
            }
        }
    }
    for img in ctx.images.clone() {
        for (rt, v) in eol::image_runtimes(&img.image) {
            hits.push(Hit {
                runtime: rt,
                version: v,
                path: img.path.clone(),
                evidence: format!("{}: {}", img.path, img.line),
            });
        }
    }

    // One fact per (runtime, version); the first place it was read wins.
    let mut seen: BTreeMap<(&str, String), ()> = BTreeMap::new();
    let mut eol_by_runtime: BTreeMap<&str, (eol::Day, String, String, String)> = BTreeMap::new();
    for h in &hits {
        if seen.insert((h.runtime, h.version.clone()), ()).is_some() {
            continue;
        }
        let name = label(h.runtime, &h.version);
        ctx.detected(
            FactCategory::Runtime,
            &format!("runtime:{}:{}", h.runtime, h.version),
            &name,
            &h.version,
            &h.path,
            &h.evidence,
        );
        if let Some(day) = eol::is_eol(h.runtime, &h.version, ctx.today) {
            // The oldest end-of-life version is the one to report.
            let worse = eol_by_runtime
                .get(h.runtime)
                .is_none_or(|(existing, _, _, _)| day < *existing);
            if worse {
                eol_by_runtime.insert(
                    h.runtime,
                    (day, h.version.clone(), h.path.clone(), h.evidence.clone()),
                );
            }
        }
    }
    for (runtime, (day, version, path, evidence)) in eol_by_runtime {
        let name = label(runtime, &version);
        let date = format_day(day);
        ctx.detected(
            FactCategory::Runtime,
            &format!("runtime:eol:{runtime}"),
            &format!("{name} (end of life)"),
            &date,
            &path,
            &format!("{name} reached end of life {date} ({evidence})"),
        );
    }
}

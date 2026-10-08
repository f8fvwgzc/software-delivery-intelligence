//! Manifest parsers. Each one is lenient: a malformed manifest yields an
//! empty dependency list instead of aborting the whole analysis.

use std::collections::BTreeMap;

use regex::Regex;

use crate::util::basename;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Dep {
    pub name: String,
    pub version: String,
    /// `runtime` | `dev` | `build`
    pub scope: String,
    /// Cargo features or similar extras; drive datastore detection for sqlx & co.
    pub features: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Manifest {
    pub path: String,
    /// `npm` | `cargo` | `pip` | `go` | `maven` | `gradle` | `bundler` | `composer` | `mix`
    pub ecosystem: String,
    /// Package manager capability key, e.g. `pnpm`, `poetry`.
    pub package_manager: String,
    pub name: String,
    pub deps: Vec<Dep>,
    pub scripts: BTreeMap<String, String>,
    /// Declares workspace members → monorepo.
    pub workspace: bool,
    /// Cargo: has a `[lib]` and no binary; npm: library-ish (`main` + not private).
    pub library: bool,
}

impl Manifest {
    pub fn has_dep(&self, name: &str) -> bool {
        self.deps.iter().any(|d| d.name == name)
    }

    pub fn dep(&self, name: &str) -> Option<&Dep> {
        self.deps.iter().find(|d| d.name == name)
    }
}

fn dep(name: &str, version: &str, scope: &str) -> Dep {
    Dep {
        name: name.to_string(),
        version: version.to_string(),
        scope: scope.to_string(),
        features: Vec::new(),
    }
}

/// Dispatches on the file name. `tree_names` lets lock files pick the package manager.
pub(crate) fn parse(path: &str, content: &str, tree_names: &[&str]) -> Option<Manifest> {
    let name = basename(path);
    let mut m = match name {
        "package.json" => parse_package_json(content, tree_names),
        "Cargo.toml" => parse_cargo(content, tree_names),
        "pyproject.toml" => parse_pyproject(content, tree_names),
        "go.mod" => parse_go_mod(content),
        "pom.xml" => parse_pom(content),
        "build.gradle" | "build.gradle.kts" => parse_gradle(content),
        "Gemfile" => parse_gemfile(content),
        "composer.json" => parse_composer(content),
        "mix.exs" => parse_mix(content),
        n if n.starts_with("requirements") && n.ends_with(".txt") => parse_requirements(n, content),
        _ => return None,
    };
    m.path = path.to_string();
    Some(m)
}

fn has_name(tree_names: &[&str], name: &str) -> bool {
    tree_names.contains(&name)
}

fn parse_package_json(content: &str, tree_names: &[&str]) -> Manifest {
    let mut m = Manifest {
        ecosystem: "npm".into(),
        package_manager: "npm".into(),
        ..Default::default()
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(content) else {
        return m;
    };
    m.name = v["name"].as_str().unwrap_or("").to_string();
    for (field, scope) in [
        ("dependencies", "runtime"),
        ("devDependencies", "dev"),
        ("peerDependencies", "runtime"),
    ] {
        if let Some(obj) = v[field].as_object() {
            for (k, ver) in obj {
                m.deps.push(dep(k, ver.as_str().unwrap_or(""), scope));
            }
        }
    }
    if let Some(obj) = v["scripts"].as_object() {
        for (k, s) in obj {
            m.scripts
                .insert(k.clone(), s.as_str().unwrap_or("").to_string());
        }
    }
    m.workspace = !v["workspaces"].is_null() || has_name(tree_names, "pnpm-workspace.yaml");
    m.library = v["private"].as_bool() != Some(true)
        && (!v["main"].is_null() || !v["exports"].is_null())
        && v["bin"].is_null();
    m.package_manager = match v["packageManager"].as_str() {
        Some(pm) if pm.starts_with("pnpm") => "pnpm",
        Some(pm) if pm.starts_with("yarn") => "yarn",
        Some(pm) if pm.starts_with("bun") => "bun",
        Some(pm) if pm.starts_with("npm") => "npm",
        _ if has_name(tree_names, "pnpm-lock.yaml") => "pnpm",
        _ if has_name(tree_names, "yarn.lock") => "yarn",
        _ if has_name(tree_names, "bun.lockb") || has_name(tree_names, "bun.lock") => "bun",
        _ => "npm",
    }
    .into();
    m
}

fn toml_deps(table: Option<&toml::Value>, scope: &str, out: &mut Vec<Dep>) {
    let Some(table) = table.and_then(|t| t.as_table()) else {
        return;
    };
    for (name, spec) in table {
        let mut d = dep(name, "", scope);
        match spec {
            toml::Value::String(v) => d.version = v.clone(),
            toml::Value::Table(t) => {
                d.version = t
                    .get("version")
                    .and_then(|v| v.as_str())
                    .unwrap_or(if t.contains_key("path") { "path" } else { "" })
                    .to_string();
                if let Some(f) = t.get("features").and_then(|f| f.as_array()) {
                    d.features = f
                        .iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect();
                }
                if let Some(p) = t.get("package").and_then(|p| p.as_str()) {
                    d.name = p.to_string();
                }
            }
            _ => {}
        }
        out.push(d);
    }
}

fn parse_cargo(content: &str, tree_names: &[&str]) -> Manifest {
    let mut m = Manifest {
        ecosystem: "cargo".into(),
        package_manager: "cargo".into(),
        ..Default::default()
    };
    let Ok(v) = content.parse::<toml::Table>() else {
        return m;
    };
    m.name = v
        .get("package")
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .to_string();
    toml_deps(v.get("dependencies"), "runtime", &mut m.deps);
    toml_deps(v.get("dev-dependencies"), "dev", &mut m.deps);
    toml_deps(v.get("build-dependencies"), "build", &mut m.deps);
    if let Some(ws) = v.get("workspace") {
        m.workspace = ws.get("members").is_some();
        toml_deps(ws.get("dependencies"), "runtime", &mut m.deps);
    }
    // Workspace-inherited deps keep their features, which is all we need from them.
    let has_bin = v.get("bin").is_some() || has_name(tree_names, "main.rs");
    m.library = v.get("lib").is_some() && !has_bin || !has_bin && v.get("package").is_some();
    m
}

/// PEP 508 requirement → (name, version spec).
fn split_requirement(spec: &str) -> Option<(String, String)> {
    let spec = spec.trim();
    if spec.is_empty() || spec.starts_with('#') || spec.starts_with('-') {
        return None;
    }
    let end = spec
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'))
        .unwrap_or(spec.len());
    let name = spec[..end].to_ascii_lowercase();
    if name.is_empty() {
        return None;
    }
    let mut rest = &spec[end..];
    if rest.starts_with('[') {
        rest = rest.find(']').map(|i| &rest[i + 1..]).unwrap_or("");
    }
    let version = rest.split(';').next().unwrap_or("").trim().to_string();
    Some((name, version))
}

fn parse_pyproject(content: &str, tree_names: &[&str]) -> Manifest {
    let mut m = Manifest {
        ecosystem: "pip".into(),
        package_manager: "pip".into(),
        ..Default::default()
    };
    let Ok(v) = content.parse::<toml::Table>() else {
        return m;
    };
    if let Some(p) = v.get("project") {
        m.name = p
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("")
            .to_string();
        if let Some(arr) = p.get("dependencies").and_then(|d| d.as_array()) {
            for s in arr.iter().filter_map(|x| x.as_str()) {
                if let Some((n, ver)) = split_requirement(s) {
                    m.deps.push(dep(&n, &ver, "runtime"));
                }
            }
        }
        if let Some(groups) = p.get("optional-dependencies").and_then(|d| d.as_table()) {
            for (group, arr) in groups {
                let scope = if group.contains("dev") || group.contains("test") {
                    "dev"
                } else {
                    "runtime"
                };
                for s in arr
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|x| x.as_str())
                {
                    if let Some((n, ver)) = split_requirement(s) {
                        m.deps.push(dep(&n, &ver, scope));
                    }
                }
            }
        }
    }
    if let Some(groups) = v.get("dependency-groups").and_then(|d| d.as_table()) {
        for arr in groups.values() {
            for s in arr
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|x| x.as_str())
            {
                if let Some((n, ver)) = split_requirement(s) {
                    m.deps.push(dep(&n, &ver, "dev"));
                }
            }
        }
    }
    if let Some(poetry) = v.get("tool").and_then(|t| t.get("poetry")) {
        m.package_manager = "poetry".into();
        if m.name.is_empty() {
            m.name = poetry
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .to_string();
        }
        let mut runtime = Vec::new();
        toml_deps(poetry.get("dependencies"), "runtime", &mut runtime);
        m.deps
            .extend(runtime.into_iter().filter(|d| d.name != "python"));
        toml_deps(poetry.get("dev-dependencies"), "dev", &mut m.deps);
        if let Some(groups) = poetry.get("group").and_then(|g| g.as_table()) {
            for g in groups.values() {
                toml_deps(g.get("dependencies"), "dev", &mut m.deps);
            }
        }
    } else if has_name(tree_names, "uv.lock") {
        m.package_manager = "uv".into();
    }
    for d in &mut m.deps {
        d.name = d.name.to_ascii_lowercase();
    }
    m
}

fn parse_requirements(file_name: &str, content: &str) -> Manifest {
    let scope = if file_name.contains("dev") || file_name.contains("test") {
        "dev"
    } else {
        "runtime"
    };
    let mut m = Manifest {
        ecosystem: "pip".into(),
        package_manager: "pip".into(),
        ..Default::default()
    };
    for line in content.lines() {
        if let Some((n, ver)) = split_requirement(line) {
            m.deps.push(dep(&n, &ver, scope));
        }
    }
    m
}

fn parse_go_mod(content: &str) -> Manifest {
    let mut m = Manifest {
        ecosystem: "go".into(),
        package_manager: "go".into(),
        ..Default::default()
    };
    let mut in_require = false;
    for raw in content.lines() {
        let line = raw.split("//").next().unwrap_or("").trim();
        let indirect = raw.contains("// indirect");
        if let Some(rest) = line.strip_prefix("module ") {
            m.name = rest.trim().to_string();
        } else if line == "require (" {
            in_require = true;
        } else if line == ")" {
            in_require = false;
        } else if let Some(rest) = line.strip_prefix("require ") {
            push_go_dep(&mut m, rest, indirect);
        } else if in_require && !line.is_empty() {
            push_go_dep(&mut m, line, indirect);
        }
    }
    m
}

fn push_go_dep(m: &mut Manifest, spec: &str, indirect: bool) {
    let mut parts = spec.split_whitespace();
    if let (Some(name), Some(ver)) = (parts.next(), parts.next()) {
        let mut d = dep(name, ver, "runtime");
        if indirect {
            d.scope = "build".into();
        }
        m.deps.push(d);
    }
}

fn parse_pom(content: &str) -> Manifest {
    let mut m = Manifest {
        ecosystem: "maven".into(),
        package_manager: "maven".into(),
        ..Default::default()
    };
    let re = Regex::new(r"(?s)<dependency>(.*?)</dependency>").expect("static regex");
    let tag = |block: &str, t: &str| -> String {
        Regex::new(&format!(r"<{t}>\s*([^<]+?)\s*</{t}>"))
            .expect("static regex")
            .captures(block)
            .map(|c| c[1].to_string())
            .unwrap_or_default()
    };
    if let Some(a) = Regex::new(r"<artifactId>\s*([^<]+?)\s*</artifactId>")
        .expect("static regex")
        .captures(content)
    {
        m.name = a[1].to_string();
    }
    for cap in re.captures_iter(content) {
        let block = &cap[1];
        let group = tag(block, "groupId");
        let artifact = tag(block, "artifactId");
        if artifact.is_empty() {
            continue;
        }
        let scope = match tag(block, "scope").as_str() {
            "test" => "dev",
            "provided" => "build",
            _ => "runtime",
        };
        m.deps.push(dep(
            &format!("{group}:{artifact}"),
            &tag(block, "version"),
            scope,
        ));
    }
    m
}

fn parse_gradle(content: &str) -> Manifest {
    let mut m = Manifest {
        ecosystem: "maven".into(),
        package_manager: "gradle".into(),
        ..Default::default()
    };
    let re = Regex::new(
        r#"(?m)^\s*(implementation|api|compileOnly|runtimeOnly|testImplementation|testRuntimeOnly|kapt|annotationProcessor)\s*\(?\s*["']([^"']+)["']"#,
    )
    .expect("static regex");
    for cap in re.captures_iter(content) {
        let scope = if cap[1].starts_with("test") {
            "dev"
        } else {
            "runtime"
        };
        let coord = &cap[2];
        let mut parts = coord.split(':');
        let group = parts.next().unwrap_or("");
        let artifact = parts.next().unwrap_or("");
        let version = parts.next().unwrap_or("");
        let name = if artifact.is_empty() {
            group.to_string()
        } else {
            format!("{group}:{artifact}")
        };
        m.deps.push(dep(&name, version, scope));
    }
    let plugins = Regex::new(r#"id\s*\(?\s*["']([^"']+)["']"#).expect("static regex");
    for cap in plugins.captures_iter(content) {
        m.deps
            .push(dep(&format!("plugin:{}", &cap[1]), "", "build"));
    }
    m
}

fn parse_gemfile(content: &str) -> Manifest {
    let mut m = Manifest {
        ecosystem: "bundler".into(),
        package_manager: "bundler".into(),
        ..Default::default()
    };
    let gem = Regex::new(r#"^\s*gem\s+["']([^"']+)["'](?:\s*,\s*["']([^"']+)["'])?"#)
        .expect("static regex");
    let mut dev_depth = 0usize;
    for line in content.lines() {
        let t = line.trim();
        if t.starts_with("group") && (t.contains(":test") || t.contains(":development")) {
            dev_depth += 1;
            continue;
        }
        if t == "end" && dev_depth > 0 {
            dev_depth -= 1;
            continue;
        }
        if let Some(cap) = gem.captures(line) {
            let scope = if dev_depth > 0 { "dev" } else { "runtime" };
            m.deps.push(dep(
                &cap[1],
                cap.get(2).map(|v| v.as_str()).unwrap_or(""),
                scope,
            ));
        }
    }
    m
}

fn parse_composer(content: &str) -> Manifest {
    let mut m = Manifest {
        ecosystem: "composer".into(),
        package_manager: "composer".into(),
        ..Default::default()
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(content) else {
        return m;
    };
    m.name = v["name"].as_str().unwrap_or("").to_string();
    for (field, scope) in [("require", "runtime"), ("require-dev", "dev")] {
        if let Some(obj) = v[field].as_object() {
            for (k, ver) in obj {
                if k == "php" || k.starts_with("ext-") {
                    continue;
                }
                m.deps.push(dep(k, ver.as_str().unwrap_or(""), scope));
            }
        }
    }
    m
}

fn parse_mix(content: &str) -> Manifest {
    let mut m = Manifest {
        ecosystem: "mix".into(),
        package_manager: "mix".into(),
        ..Default::default()
    };
    let app = Regex::new(r"app:\s*:(\w+)").expect("static regex");
    if let Some(cap) = app.captures(content) {
        m.name = cap[1].to_string();
    }
    let re = Regex::new(r#"\{:(\w+),\s*"([^"]*)"([^}]*)\}"#).expect("static regex");
    for cap in re.captures_iter(content) {
        let scope =
            if cap[3].contains("only:") && (cap[3].contains(":test") || cap[3].contains(":dev")) {
                "dev"
            } else {
                "runtime"
            };
        m.deps.push(dep(&cap[1], &cap[2], scope));
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_json_scopes_and_manager() {
        let m = parse(
            "package.json",
            r#"{"name":"web","private":true,"dependencies":{"react":"^19"},"devDependencies":{"vite":"^6"},"workspaces":["packages/*"]}"#,
            &["pnpm-lock.yaml"],
        )
        .unwrap();
        assert_eq!(m.name, "web");
        assert_eq!(m.package_manager, "pnpm");
        assert!(m.workspace);
        assert_eq!(m.dep("vite").unwrap().scope, "dev");
        assert!(!m.library);
    }

    #[test]
    fn cargo_features_and_workspace_deps() {
        let m = parse(
            "Cargo.toml",
            r#"
[package]
name = "api"
[dependencies]
sqlx = { version = "0.8", features = ["postgres", "runtime-tokio"] }
axum = "0.8"
[dev-dependencies]
proptest = "1"
"#,
            &["main.rs"],
        )
        .unwrap();
        assert_eq!(
            m.dep("sqlx").unwrap().features,
            vec!["postgres", "runtime-tokio"]
        );
        assert_eq!(m.dep("proptest").unwrap().scope, "dev");
        assert!(!m.library);
    }

    #[test]
    fn pyproject_and_requirements() {
        let m = parse(
            "pyproject.toml",
            r#"
[project]
name = "svc"
dependencies = ["fastapi>=0.110", "psycopg[binary]==3.1", "uvicorn[standard]"]
[project.optional-dependencies]
dev = ["pytest"]
"#,
            &[],
        )
        .unwrap();
        assert!(m.has_dep("fastapi"));
        assert_eq!(m.dep("psycopg").unwrap().version, "==3.1");
        assert_eq!(m.dep("pytest").unwrap().scope, "dev");
        let r = parse(
            "requirements-dev.txt",
            "pytest==8.0\n# c\n-r base.txt\nhttpx",
            &[],
        )
        .unwrap();
        assert_eq!(r.deps.len(), 2);
        assert_eq!(r.deps[0].scope, "dev");
    }

    #[test]
    fn go_pom_gradle_gemfile_mix() {
        let g = parse(
            "go.mod",
            "module example.com/app\n\ngo 1.22\n\nrequire (\n\tgithub.com/gin-gonic/gin v1.9.1\n\tgolang.org/x/sys v0.1.0 // indirect\n)\n",
            &[],
        )
        .unwrap();
        assert_eq!(g.name, "example.com/app");
        assert_eq!(g.dep("github.com/gin-gonic/gin").unwrap().version, "v1.9.1");
        assert_eq!(g.dep("golang.org/x/sys").unwrap().scope, "build");

        let p = parse(
            "pom.xml",
            "<project><artifactId>demo</artifactId><dependencies><dependency><groupId>org.springframework.boot</groupId><artifactId>spring-boot-starter-web</artifactId><version>3.2.0</version></dependency><dependency><groupId>org.junit.jupiter</groupId><artifactId>junit-jupiter</artifactId><scope>test</scope></dependency></dependencies></project>",
            &[],
        )
        .unwrap();
        assert!(p.has_dep("org.springframework.boot:spring-boot-starter-web"));
        assert_eq!(
            p.dep("org.junit.jupiter:junit-jupiter").unwrap().scope,
            "dev"
        );

        let gr = parse(
            "build.gradle.kts",
            "plugins { id(\"org.springframework.boot\") }\ndependencies {\n implementation(\"org.postgresql:postgresql:42.7.0\")\n testImplementation(\"org.junit.jupiter:junit-jupiter\")\n}",
            &[],
        )
        .unwrap();
        assert_eq!(
            gr.dep("org.postgresql:postgresql").unwrap().version,
            "42.7.0"
        );
        assert!(gr.has_dep("plugin:org.springframework.boot"));

        let gem = parse(
            "Gemfile",
            "gem 'rails', '~> 7.1'\ngroup :test do\n  gem 'rspec-rails'\nend\n",
            &[],
        )
        .unwrap();
        assert_eq!(gem.dep("rails").unwrap().version, "~> 7.1");
        assert_eq!(gem.dep("rspec-rails").unwrap().scope, "dev");

        let mix = parse(
            "mix.exs",
            "def project, do: [app: :shop]\ndefp deps do\n[{:phoenix, \"~> 1.7\"}, {:postgrex, \">= 0.0.0\"}, {:ex_doc, \"~> 0.3\", only: :dev}]\nend",
            &[],
        )
        .unwrap();
        assert_eq!(mix.name, "shop");
        assert!(mix.has_dep("phoenix"));
        assert_eq!(mix.dep("ex_doc").unwrap().scope, "dev");
    }
}

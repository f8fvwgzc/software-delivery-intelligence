//! File-level import graph: every sampled file's imports, parsed per
//! language (Rust `mod`/`use`, TS/JS `import`/`require`, Python
//! `from`/`import`, Go package paths, Java/C# packages) and resolved against
//! the repository tree. Edges run importer → imported; a file that cannot be
//! resolved to a path in the tree (standard library, third-party) is dropped.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;

use super::catalog::Catalog;

/// Extensions of files that take part in the import graph.
pub const SOURCE_EXTENSIONS: [&str; 20] = [
    "rs", "ts", "tsx", "js", "jsx", "mjs", "cjs", "py", "go", "java", "kt", "cs", "rb", "php",
    "swift", "scala", "ex", "exs", "vue", "svelte",
];

/// Extensions that turn a path into a module file for TS/JS resolution.
const TS_EXTENSIONS: [&str; 7] = ["ts", "tsx", "js", "jsx", "mjs", "cjs", "d.ts"];

pub fn extension(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.').map(|(_, e)| e).unwrap_or("")
}

pub fn is_source(path: &str) -> bool {
    SOURCE_EXTENSIONS.contains(&extension(path))
}

/// Generated, vendored and minified files never count as modules.
pub fn is_generated(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    let lower = name.to_lowercase();
    lower.contains(".generated.")
        || lower.ends_with(".pb.go")
        || lower.ends_with("_pb2.py")
        || lower.ends_with("_pb2_grpc.py")
        || lower.ends_with(".pb.rs")
        || lower.ends_with(".min.js")
        || lower.ends_with(".d.ts")
        || lower.ends_with(".g.dart")
        || lower == "build.rs"
        || path.split('/').any(|p| {
            matches!(
                p,
                "generated" | "gen" | "__generated__" | "third_party" | "vendored" | "vendor"
            )
        })
}

pub fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

pub fn parent_dir(path: &str) -> &str {
    path.rsplit_once('/').map(|(d, _)| d).unwrap_or("")
}

/// `a/b/../c` → `a/c`; a path that climbs above the root is `None`.
pub fn normalize(path: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    Some(parts.join("/"))
}

pub fn join(dir: &str, rel: &str) -> Option<String> {
    if dir.is_empty() {
        normalize(rel)
    } else {
        normalize(&format!("{dir}/{rel}"))
    }
}

/// Resolves import specifiers to repository paths.
pub struct Resolver<'a> {
    catalog: &'a Catalog<'a>,
    /// Normalised package name (`-` → `_`, lowercase) → package directory.
    packages: Vec<(String, String)>,
    /// Workspace member directories (from the graph or manifests found in
    /// the tree), used for the import roots of absolute specifiers.
    package_dirs: Vec<String>,
    /// `(module directory, item)` → file, from `pub use module::{Item}` in
    /// sampled `lib.rs` / `mod.rs` files, so `crate_name::Item` reaches the
    /// module that defines it.
    reexports: HashMap<(String, String), String>,
}

static REEXPORT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?s)\bpub\s+use\s+(?:self::)?([a-z_][a-z0-9_]*)::(?:\{([^}]*)\}|([A-Za-z_]\w*))\s*;",
    )
    .expect("reexport")
});

static CARGO_NAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^\s*\[(package|lib)\]\s*$|^\s*name\s*=\s*"([^"]+)""#).expect("cargo name")
});
static NPM_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#""name"\s*:\s*"([^"]+)""#).expect("npm name"));

impl<'a> Resolver<'a> {
    pub fn new(catalog: &'a Catalog<'a>) -> Self {
        let mut packages: Vec<(String, String)> = Vec::new();
        let mut package_dirs: BTreeSet<String> = BTreeSet::new();
        for node in catalog.nodes().filter(|n| catalog.is_product_node(n)) {
            let dir = catalog.dir_of(node);
            packages.push((norm_name(&node.label), dir.clone()));
            package_dirs.insert(dir);
        }
        // Manifests in the sample add lib aliases (`[lib] name = "x"`) and
        // scoped npm names.
        for (path, text) in catalog.input.files {
            let dir = parent_dir(path).to_owned();
            let name = file_name(path);
            if name == "Cargo.toml" {
                let mut section = "";
                for cap in CARGO_NAME.captures_iter(text) {
                    if let Some(s) = cap.get(1) {
                        section = s.as_str();
                    } else if let Some(n) = cap.get(2)
                        && (section == "package" || section == "lib")
                    {
                        packages.push((norm_name(n.as_str()), dir.clone()));
                        package_dirs.insert(dir.clone());
                    }
                }
            } else if name == "package.json"
                && let Some(cap) = NPM_NAME.captures(text)
            {
                packages.push((norm_name(&cap[1]), dir.clone()));
                package_dirs.insert(dir.clone());
            }
        }
        // Manifest directories in the tree even when no graph node names them.
        for file in catalog.all_files() {
            if matches!(
                file_name(file),
                "Cargo.toml" | "package.json" | "pyproject.toml" | "go.mod"
            ) && !Catalog::is_excluded_path(file)
            {
                package_dirs.insert(parent_dir(file).to_owned());
            }
        }
        packages.sort();
        packages.dedup();
        let mut reexports = HashMap::new();
        for (path, text) in catalog.input.files {
            let name = file_name(path);
            if !matches!(name, "lib.rs" | "mod.rs" | "main.rs") {
                continue;
            }
            let dir = parent_dir(path).to_owned();
            for cap in REEXPORT.captures_iter(text) {
                let module = cap[1].to_owned();
                let file = [
                    format!("{dir}/{module}.rs"),
                    format!("{dir}/{module}/mod.rs"),
                ]
                .into_iter()
                .find(|p| catalog.file_exists(p));
                let Some(file) = file else { continue };
                let items: Vec<String> = match (cap.get(2), cap.get(3)) {
                    (Some(group), _) => group
                        .as_str()
                        .split(',')
                        .map(|i| i.split_whitespace().next().unwrap_or("").to_owned())
                        .collect(),
                    (None, Some(single)) => vec![single.as_str().to_owned()],
                    _ => Vec::new(),
                };
                for item in items.into_iter().filter(|i| !i.is_empty() && i != "self") {
                    reexports.insert((dir.clone(), item), file.clone());
                }
            }
        }
        Self {
            catalog,
            packages,
            package_dirs: package_dirs.into_iter().collect(),
            reexports,
        }
    }

    fn exists(&self, path: &str) -> bool {
        self.catalog.file_exists(path)
    }

    fn is_dir(&self, path: &str) -> bool {
        self.catalog.dir_exists(path)
    }

    /// Directory of the workspace package that holds `path` (the deepest).
    pub fn package_dir_of(&self, path: &str) -> String {
        self.package_dirs
            .iter()
            .filter(|d| super::catalog::within(path, d))
            .max_by_key(|d| d.len())
            .cloned()
            .unwrap_or_default()
    }

    fn package_dir_by_name(&self, name: &str) -> Option<&str> {
        let wanted = norm_name(name);
        self.packages
            .iter()
            .find(|(n, _)| *n == wanted)
            .map(|(_, d)| d.as_str())
    }

    /// Every path `text` (at `path`) imports that exists in the tree.
    pub fn imports_of(&self, path: &str, text: &str) -> Vec<String> {
        let mut out: Vec<String> = match extension(path) {
            "rs" => self.rust(path, text),
            "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "vue" | "svelte" => self.ts(path, text),
            "py" => self.python(path, text),
            "go" => self.go(path, text),
            "java" | "kt" | "scala" => self.java(path, text),
            "cs" => self.csharp(path, text),
            _ => Vec::new(),
        };
        out.retain(|p| p != path && !Catalog::is_excluded_path(p) && !is_generated(p));
        let mut seen = HashSet::new();
        out.retain(|p| seen.insert(p.clone()));
        out
    }

    // -- Rust ---------------------------------------------------------------

    /// Directory whose `*.rs` files are this file's child modules.
    fn rust_own_dir(path: &str) -> String {
        let dir = parent_dir(path);
        let name = file_name(path);
        if matches!(name, "mod.rs" | "lib.rs" | "main.rs") {
            dir.to_owned()
        } else {
            join(dir, name.trim_end_matches(".rs")).unwrap_or_default()
        }
    }

    /// Directory of the parent module's children.
    fn rust_super_dir(path: &str) -> String {
        let dir = parent_dir(path);
        let name = file_name(path);
        if matches!(name, "mod.rs" | "lib.rs" | "main.rs") {
            parent_dir(dir).to_owned()
        } else {
            dir.to_owned()
        }
    }

    /// Nearest ancestor directory holding `lib.rs` or `main.rs`.
    fn rust_crate_root(&self, path: &str) -> String {
        let mut dir = parent_dir(path).to_owned();
        loop {
            if self.exists(&format!("{dir}/lib.rs")) || self.exists(&format!("{dir}/main.rs")) {
                return dir;
            }
            if dir.is_empty() {
                return parent_dir(path).to_owned();
            }
            dir = parent_dir(&dir).to_owned();
        }
    }

    /// The deepest file a module path resolves to under `root`, else the
    /// root module file itself.
    fn rust_resolve(&self, root: &str, segments: &[&str]) -> Option<String> {
        let mut best: Option<String> = None;
        let mut cur = root.to_owned();
        for seg in segments {
            if seg.is_empty() {
                break;
            }
            if let Some(file) = self.reexports.get(&(cur.clone(), (*seg).to_owned())) {
                best = Some(file.clone());
                break;
            }
            if seg.chars().next().is_some_and(|c| c.is_uppercase()) {
                break;
            }
            let file = format!("{cur}/{seg}.rs");
            let modfile = format!("{cur}/{seg}/mod.rs");
            let dir = format!("{cur}/{seg}");
            if self.exists(&file) {
                best = Some(file);
            }
            if self.exists(&modfile) {
                best = Some(modfile);
            }
            if self.is_dir(&dir) {
                cur = dir;
            } else {
                break;
            }
        }
        best.or_else(|| {
            ["mod.rs", "lib.rs", "main.rs"]
                .iter()
                .map(|f| format!("{root}/{f}"))
                .find(|p| self.exists(p))
        })
    }

    fn rust(&self, path: &str, text: &str) -> Vec<String> {
        static USE: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"(?s)\b(?:pub(?:\([^)]*\))?\s+)?use\s+([^;]+);").expect("use")
        });
        static PATH: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(
                r"\b(crate|super|self|[a-z_][a-z0-9_]*)::([A-Za-z_]\w*)(?:::([A-Za-z_]\w*))?",
            )
            .expect("path")
        });
        let mut out = Vec::new();
        let own = Self::rust_own_dir(path);
        let crate_root = self.rust_crate_root(path);
        let super_dir = Self::rust_super_dir(path);
        let clean = strip_comments(text);
        // `use a::b::{c, d::e}` expands to `a::b::c` and `a::b::d::e`; inline
        // paths in code (`symphony_cli::app::main()`) count too. `mod x;`
        // declarations are structure, not calls, so they add no edge.
        let mut paths: Vec<String> = Vec::new();
        for cap in USE.captures_iter(&clean) {
            paths.extend(expand_use_tree(cap[1].trim()));
        }
        for cap in PATH.captures_iter(&clean) {
            let mut p = format!("{}::{}", &cap[1], &cap[2]);
            if let Some(s2) = cap.get(3) {
                p.push_str("::");
                p.push_str(s2.as_str());
            }
            paths.push(p);
        }
        for p in paths {
            let mut segments = p.split("::").filter(|s| !s.is_empty());
            let Some(head) = segments.next() else {
                continue;
            };
            let rest: Vec<&str> = segments.take(3).collect();
            if rest.is_empty() {
                continue;
            }
            let resolved = match head {
                "crate" => self.rust_resolve(&crate_root, &rest),
                "self" => self.rust_resolve(&own, &rest),
                "super" => {
                    // `super::x` is a sibling module or an item of the parent module.
                    self.rust_resolve(&super_dir, &rest).or_else(|| {
                        let parent_name = file_name(&super_dir);
                        let parent_file = format!("{}/{parent_name}.rs", parent_dir(&super_dir));
                        self.exists(&parent_file).then_some(parent_file)
                    })
                }
                "std" | "core" | "alloc" => None,
                other => self
                    .package_dir_by_name(other)
                    .and_then(|dir| {
                        let root = if self.is_dir(&format!("{dir}/src")) {
                            format!("{dir}/src")
                        } else {
                            dir.to_owned()
                        };
                        self.rust_resolve(&root, &rest)
                            .or_else(|| self.catalog.entry_point(dir))
                    })
                    .or_else(|| {
                        // `dispatch::ready(...)`: a module declared in this file or its parent.
                        let local = |dir: &str| {
                            (self.exists(&format!("{dir}/{other}.rs"))
                                || self.exists(&format!("{dir}/{other}/mod.rs")))
                            .then(|| {
                                let mut segments = vec![other];
                                segments.extend(rest.iter().copied());
                                self.rust_resolve(dir, &segments)
                            })
                            .flatten()
                        };
                        local(&own).or_else(|| local(&super_dir))
                    }),
            };
            if let Some(resolved) = resolved {
                out.push(resolved);
            }
        }
        out
    }

    // -- TypeScript / JavaScript ---------------------------------------------

    fn ts(&self, path: &str, text: &str) -> Vec<String> {
        static SPEC: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(
                r#"(?:\b(?:import|export)\b[^'"`;]*?\bfrom\s*['"]([^'"]+)['"])|(?:\bimport\s*['"]([^'"]+)['"])|(?:\brequire\(\s*['"]([^'"]+)['"]\s*\))|(?:\bimport\(\s*['"]([^'"]+)['"]\s*\))"#,
            )
            .expect("ts import")
        });
        let dir = parent_dir(path);
        let pkg = self.package_dir_of(path);
        let mut out = Vec::new();
        for cap in SPEC.captures_iter(text) {
            let spec = (1..=4)
                .filter_map(|i| cap.get(i))
                .map(|m| m.as_str())
                .next()
                .unwrap_or("");
            if spec.is_empty() {
                continue;
            }
            let base = if spec.starts_with("./") || spec.starts_with("../") {
                join(dir, spec)
            } else if let Some(rest) = spec
                .strip_prefix("@/")
                .or_else(|| spec.strip_prefix("~/"))
                .or_else(|| spec.strip_prefix("src/"))
            {
                join(&pkg, &format!("src/{rest}"))
            } else {
                // A workspace package by name.
                let name = spec.split('/').take(2).collect::<Vec<_>>().join("/");
                let short = name.rsplit('/').next().unwrap_or(&name);
                if let Some(target) = self
                    .package_dir_by_name(&name)
                    .or_else(|| self.package_dir_by_name(short))
                    .and_then(|d| self.catalog.entry_point(d))
                {
                    out.push(target);
                }
                continue;
            };
            let Some(base) = base else { continue };
            if let Some(found) = self.ts_candidate(&base) {
                out.push(found);
            }
        }
        out
    }

    fn ts_candidate(&self, base: &str) -> Option<String> {
        if self.exists(base) && is_source(base) {
            return Some(base.to_owned());
        }
        let stem = base
            .strip_suffix(".js")
            .or_else(|| base.strip_suffix(".jsx"))
            .or_else(|| base.strip_suffix(".mjs"))
            .unwrap_or(base);
        for ext in TS_EXTENSIONS {
            let candidate = format!("{stem}.{ext}");
            if self.exists(&candidate) {
                return Some(candidate);
            }
        }
        for ext in TS_EXTENSIONS {
            let candidate = format!("{base}/index.{ext}");
            if self.exists(&candidate) {
                return Some(candidate);
            }
        }
        None
    }

    // -- Python ---------------------------------------------------------------

    fn python(&self, path: &str, text: &str) -> Vec<String> {
        static FROM: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"(?m)^\s*from\s+(\.*)([\w.]*)\s+import\b").expect("py from")
        });
        static IMPORT: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"(?m)^\s*import\s+([\w.]+(?:\s*,\s*[\w.]+)*)").expect("py import")
        });
        let dir = parent_dir(path);
        let mut out = Vec::new();
        let mut absolute: Vec<String> = Vec::new();
        for cap in FROM.captures_iter(text) {
            let dots = cap[1].len();
            let module = cap[2].to_owned();
            if dots == 0 {
                absolute.push(module);
                continue;
            }
            let mut base = dir.to_owned();
            for _ in 1..dots {
                base = parent_dir(&base).to_owned();
            }
            if let Some(found) = self.python_resolve(&base, &module) {
                out.push(found);
            }
        }
        for cap in IMPORT.captures_iter(text) {
            for module in cap[1].split(',') {
                absolute.push(module.trim().to_owned());
            }
        }
        // Absolute modules: the file's own directory and its ancestors up to the root.
        let mut bases: Vec<String> = Vec::new();
        let mut cur = dir.to_owned();
        loop {
            bases.push(cur.clone());
            if cur.is_empty() {
                break;
            }
            cur = parent_dir(&cur).to_owned();
        }
        for module in absolute {
            if let Some(found) = bases.iter().find_map(|b| self.python_resolve(b, &module)) {
                out.push(found);
            }
        }
        out
    }

    fn python_resolve(&self, base: &str, module: &str) -> Option<String> {
        let mut best = None;
        let mut cur = base.to_owned();
        let segments: Vec<&str> = module.split('.').filter(|s| !s.is_empty()).collect();
        if segments.is_empty() {
            let init = format!("{base}/__init__.py");
            return self.exists(&init).then_some(init);
        }
        for seg in segments {
            let file = join(&cur, &format!("{seg}.py"))?;
            let pkg = join(&cur, &format!("{seg}/__init__.py"))?;
            if self.exists(&file) {
                best = Some(file);
            }
            if self.exists(&pkg) {
                best = Some(pkg);
            }
            let dir = join(&cur, seg)?;
            if self.is_dir(&dir) {
                cur = dir;
            } else {
                break;
            }
        }
        best
    }

    // -- Go -------------------------------------------------------------------

    fn go(&self, path: &str, text: &str) -> Vec<String> {
        static BLOCK: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"(?s)\bimport\s*\((.*?)\)").expect("go block"));
        static SINGLE: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r#"(?m)^\s*import\s+(?:\w+\s+)?"([^"]+)""#).expect("go single")
        });
        static QUOTED: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r#""([^"]+)""#).expect("quoted"));
        let mut specs: Vec<String> = Vec::new();
        for cap in BLOCK.captures_iter(text) {
            for q in QUOTED.captures_iter(&cap[1]) {
                specs.push(q[1].to_owned());
            }
        }
        for cap in SINGLE.captures_iter(text) {
            specs.push(cap[1].to_owned());
        }
        let pkg = self.package_dir_of(path);
        let mut out = Vec::new();
        for spec in specs {
            let segments: Vec<&str> = spec.split('/').collect();
            // The longest suffix that is a directory of the module.
            let found = (0..segments.len()).find_map(|i| {
                let suffix = segments[i..].join("/");
                [join(&pkg, &suffix), Some(suffix.clone())]
                    .into_iter()
                    .flatten()
                    .find(|d| self.is_dir(d))
            });
            if let Some(dir) = found {
                let mut files: Vec<&str> = self
                    .catalog
                    .files_under(&dir)
                    .filter(|f| {
                        parent_dir(f) == dir && f.ends_with(".go") && !f.ends_with("_test.go")
                    })
                    .collect();
                files.sort();
                out.extend(files.into_iter().take(4).map(str::to_owned));
            }
        }
        out
    }

    // -- Java / Kotlin / Scala -------------------------------------------------

    fn java(&self, _path: &str, text: &str) -> Vec<String> {
        static IMPORT: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"(?m)^\s*import\s+(?:static\s+)?([\w.]+?)(?:\.\*)?\s*;?\s*$")
                .expect("java import")
        });
        let mut out = Vec::new();
        for cap in IMPORT.captures_iter(text) {
            let dotted = cap[1].replace('.', "/");
            let suffix_class = format!("/{dotted}.");
            let mut hits: Vec<&str> = self
                .catalog
                .all_files()
                .filter(|f| {
                    let with_slash = format!("/{f}");
                    (with_slash.contains(&suffix_class)
                        && matches!(extension(f), "java" | "kt" | "scala"))
                        || (parent_dir(f).ends_with(&dotted)
                            && matches!(extension(f), "java" | "kt" | "scala"))
                })
                .collect();
            hits.sort();
            out.extend(hits.into_iter().take(4).map(str::to_owned));
        }
        out
    }

    // -- C# -------------------------------------------------------------------

    fn csharp(&self, _path: &str, text: &str) -> Vec<String> {
        static USING: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"(?m)^\s*using\s+([A-Z][\w.]*)\s*;").expect("using"));
        let mut out = Vec::new();
        for cap in USING.captures_iter(text) {
            let ns = &cap[1];
            let slashed = ns.replace('.', "/");
            let mut hits: Vec<&str> = self
                .catalog
                .all_files()
                .filter(|f| {
                    extension(f) == "cs"
                        && (parent_dir(f).ends_with(&slashed) || parent_dir(f).ends_with(ns))
                })
                .collect();
            hits.sort();
            out.extend(hits.into_iter().take(4).map(str::to_owned));
        }
        out
    }
}

/// `a::b::{c, d::{e, f}, self}` → `a::b::c`, `a::b::d::e`, `a::b::d::f`, `a::b`.
pub fn expand_use_tree(tree: &str) -> Vec<String> {
    fn walk(prefix: &str, text: &str, out: &mut Vec<String>) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        if let Some(open) = text.find('{') {
            if text.ends_with('}') {
                let head = text[..open].trim().trim_end_matches("::");
                let prefix = join_path(prefix, head);
                let inner = &text[open + 1..text.len() - 1];
                for item in split_items(inner) {
                    walk(&prefix, &item, out);
                }
            }
            return;
        }
        let item = text.split_whitespace().next().unwrap_or("");
        let item = item.trim_end_matches(',');
        if item == "self" || item.is_empty() {
            if !prefix.is_empty() {
                out.push(prefix.to_owned());
            }
        } else if item != "*" {
            out.push(join_path(prefix, item));
        }
    }
    fn join_path(prefix: &str, rest: &str) -> String {
        match (prefix.is_empty(), rest.is_empty()) {
            (true, _) => rest.to_owned(),
            (_, true) => prefix.to_owned(),
            _ => format!("{prefix}::{rest}"),
        }
    }
    fn split_items(text: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut depth = 0;
        let mut current = String::new();
        for c in text.chars() {
            match c {
                '{' => {
                    depth += 1;
                    current.push(c);
                }
                '}' => {
                    depth -= 1;
                    current.push(c);
                }
                ',' if depth == 0 => out.push(std::mem::take(&mut current)),
                _ => current.push(c),
            }
        }
        if !current.trim().is_empty() {
            out.push(current);
        }
        out
    }
    let mut out = Vec::new();
    walk("", tree.trim_start_matches("::"), &mut out);
    out
}

/// Removes `//` line comments and `/* */` blocks so paths in prose are ignored.
pub fn strip_comments(text: &str) -> String {
    static BLOCK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)/\*.*?\*/").expect("block"));
    static LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)//.*$").expect("line"));
    let no_block = BLOCK.replace_all(text, " ");
    LINE.replace_all(&no_block, " ").into_owned()
}

pub fn norm_name(name: &str) -> String {
    name.trim()
        .trim_start_matches('@')
        .to_lowercase()
        .replace('-', "_")
}

/// Importer → imported edges over the sampled files, deduplicated.
#[derive(Debug, Default, Clone)]
pub struct FileGraph {
    pub edges: Vec<(String, String)>,
}

impl FileGraph {
    pub fn build(catalog: &Catalog<'_>, resolver: &Resolver<'_>) -> Self {
        let mut edges = Vec::new();
        for (path, text) in catalog.input.files {
            if Catalog::is_excluded_path(path) || is_generated(path) || !is_source(path) {
                continue;
            }
            for target in resolver.imports_of(path, text) {
                edges.push((path.clone(), target));
            }
        }
        edges.sort();
        edges.dedup();
        Self { edges }
    }

    /// Distinct importers per file.
    pub fn fan_in(&self) -> BTreeMap<String, usize> {
        let mut out: BTreeMap<String, usize> = BTreeMap::new();
        for (_, target) in &self.edges {
            *out.entry(target.clone()).or_default() += 1;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::diagrams::fixtures::{self, input};

    #[test]
    fn rust_imports_resolve_mods_crate_super_and_sibling_crates() {
        let files = fixtures::symphony_files();
        let tree = fixtures::symphony_tree();
        let ctx = fixtures::symphony_context();
        let input = input(&ctx, &tree, &files, "");
        let catalog = Catalog::new(&input);
        let resolver = Resolver::new(&catalog);
        let app = resolver.imports_of(
            "crates/symphony/src/app.rs",
            &files["crates/symphony/src/app.rs"],
        );
        assert!(
            app.contains(&"crates/symphony-core/src/workflow_store.rs".to_owned()),
            "{app:?}"
        );
        assert!(
            app.contains(&"crates/symphony-runtime/src/runtime.rs".to_owned()),
            "{app:?}"
        );
        assert!(
            app.contains(&"crates/symphony-store/src/store.rs".to_owned()),
            "{app:?}"
        );
        assert!(
            app.contains(&"crates/symphony/src/dashboard/mod.rs".to_owned()),
            "{app:?}"
        );
        // main.rs reaches app.rs through the `[lib] name` alias.
        let main = resolver.imports_of(
            "crates/symphony/src/main.rs",
            &files["crates/symphony/src/main.rs"],
        );
        assert_eq!(main, vec!["crates/symphony/src/app.rs".to_owned()]);
        // `mod x;` and `use super::` inside a module directory.
        let orch = resolver.imports_of(
            "crates/symphony-runtime/src/orchestrator/mod.rs",
            &files["crates/symphony-runtime/src/orchestrator/mod.rs"],
        );
        assert!(orch.contains(&"crates/symphony-runtime/src/orchestrator/dispatch.rs".to_owned()));
        assert!(
            orch.contains(&"crates/symphony-runtime/src/worker.rs".to_owned()),
            "{orch:?}"
        );
        let dispatch = resolver.imports_of(
            "crates/symphony-runtime/src/orchestrator/dispatch.rs",
            &files["crates/symphony-runtime/src/orchestrator/dispatch.rs"],
        );
        assert!(dispatch.contains(&"crates/symphony-runtime/src/orchestrator/state.rs".to_owned()));
        assert!(!dispatch.iter().any(|p| p.contains("/tests/")));
    }

    #[test]
    fn ts_and_python_imports_resolve_relative_paths() {
        let files = fixtures::files();
        let tree = fixtures::tree();
        let ctx = fixtures::context();
        let input = input(&ctx, &tree, &files, "");
        let catalog = Catalog::new(&input);
        let resolver = Resolver::new(&catalog);
        let main = resolver.imports_of("web/src/main.tsx", &files["web/src/main.tsx"]);
        assert_eq!(main, vec!["web/src/App.tsx".to_owned()]);
        let app = resolver.imports_of("web/src/App.tsx", &files["web/src/App.tsx"]);
        assert!(app.contains(&"web/src/lib/api.ts".to_owned()), "{app:?}");
        assert!(
            app.contains(&"web/src/features/views/ViewList.tsx".to_owned()),
            "{app:?}"
        );
        let py = resolver.imports_of(
            "services/quant/app/main.py",
            &files["services/quant/app/main.py"],
        );
        assert_eq!(py, vec!["services/quant/app/pricing.py".to_owned()]);
        let graph = FileGraph::build(&catalog, &resolver);
        assert!(graph.fan_in()["web/src/lib/api.ts"] >= 3);
    }

    #[test]
    fn path_helpers() {
        assert_eq!(join("a/b", "../c.ts").as_deref(), Some("a/c.ts"));
        assert_eq!(join("", "./x").as_deref(), Some("x"));
        assert!(join("a", "../../x").is_none());
        assert_eq!(extension("a/b.test.ts"), "ts");
        assert!(is_generated("src/gen/x.rs"));
        assert!(is_generated("proto/x.pb.go"));
        assert!(!is_generated("src/main.rs"));
    }
}

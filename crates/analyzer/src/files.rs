//! Which files are worth downloading. Ranking matters because the scanner
//! stops at [`MAX_FILES`], so the root manifest must never lose to a doc.

use crate::model::TreeEntry;
use crate::util::{basename, depth, dirname, extension, has_dir, is_yaml};
use crate::{MAX_FILE_BYTES, MAX_FILES};

const VENDORED_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    "build",
    "vendor",
    ".git",
    ".next",
    ".nuxt",
    ".venv",
    "venv",
    "__pycache__",
    ".cache",
    "bower_components",
    ".terraform",
    "coverage",
];

const LOCK_FILES: &[&str] = &[
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "bun.lockb",
    "bun.lock",
    "Cargo.lock",
    "poetry.lock",
    "Pipfile.lock",
    "uv.lock",
    "go.sum",
    "composer.lock",
    "Gemfile.lock",
    "mix.lock",
    "flake.lock",
];

const BINARY_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "ico", "webp", "bmp", "tiff", "pdf", "zip", "tar", "gz", "tgz",
    "bz2", "xz", "7z", "woff", "woff2", "ttf", "otf", "eot", "mp4", "mp3", "wav", "wasm", "exe",
    "dll", "so", "dylib", "bin", "jar", "war", "class", "pyc", "o", "a", "lib", "db", "sqlite",
    "lockb", "psd", "ai",
];

const SECRET_EXTENSIONS: &[&str] = &["pem", "key", "p12", "pfx", "jks", "keystore", "gpg", "asc"];

const CODE_EXTENSIONS: &[&str] = &[
    "ts", "tsx", "js", "jsx", "mjs", "cjs", "rs", "py", "go", "java", "kt", "cs", "rb", "php",
    "ex", "exs", "swift", "vue", "svelte", "astro", "html",
];

/// Anything that may hold credentials. Checked by name only: content is
/// never inspected, so a false positive just means we skip a file.
pub(crate) fn looks_secret(path: &str) -> bool {
    let name = basename(path);
    let lower = name.to_ascii_lowercase();
    if lower == ".env" || (lower.starts_with(".env.") && !is_env_template(&lower)) {
        return true;
    }
    if SECRET_EXTENSIONS.contains(&extension(path).as_str()) {
        return true;
    }
    matches!(
        lower.as_str(),
        "id_rsa" | "id_dsa" | "id_ecdsa" | "id_ed25519" | "credentials.json" | "credentials"
    ) || lower.ends_with(".secret")
        || lower.ends_with(".secrets")
        || lower.starts_with("secrets.")
        || lower.starts_with("service-account")
        || (lower.contains("secret") && is_yaml(path) && has_dir(path, "k8s"))
}

pub(crate) fn is_env_template(lower_name: &str) -> bool {
    matches!(
        lower_name,
        ".env.example" | ".env.sample" | ".env.template" | ".env.dist" | ".env.defaults"
    )
}

pub(crate) fn is_lock_file(path: &str) -> bool {
    LOCK_FILES.contains(&basename(path))
}

fn is_vendored(path: &str) -> bool {
    let dir = dirname(path);
    !dir.is_empty() && dir.split('/').any(|c| VENDORED_DIRS.contains(&c))
}

pub(crate) fn is_manifest_name(name: &str) -> bool {
    matches!(
        name,
        "package.json"
            | "pnpm-workspace.yaml"
            | "Cargo.toml"
            | "pyproject.toml"
            | "Pipfile"
            | "setup.py"
            | "go.mod"
            | "pom.xml"
            | "build.gradle"
            | "build.gradle.kts"
            | "settings.gradle"
            | "settings.gradle.kts"
            | "Gemfile"
            | "composer.json"
            | "mix.exs"
    ) || name.starts_with("requirements") && name.ends_with(".txt")
}

pub(crate) fn is_dockerfile(name: &str) -> bool {
    name == "Dockerfile" || name.starts_with("Dockerfile.") || name.ends_with(".Dockerfile")
}

pub(crate) fn is_compose(name: &str) -> bool {
    (name.starts_with("docker-compose") || name.starts_with("compose"))
        && (name.ends_with(".yml") || name.ends_with(".yaml"))
}

pub(crate) fn is_workflow(path: &str) -> bool {
    path.starts_with(".github/workflows/") && is_yaml(path) && depth(path) == 2
}

pub(crate) fn is_k8s_dir(path: &str) -> bool {
    [
        "k8s",
        "kubernetes",
        "deploy",
        "deployment",
        "manifests",
        "charts",
        "helm",
        "kustomize",
    ]
    .iter()
    .any(|d| has_dir(path, d))
}

pub(crate) fn is_migration_path(path: &str) -> bool {
    has_dir(path, "migrations") || has_dir(path, "migrate") || has_dir(path, "versions")
}

fn is_frontend_config(name: &str) -> bool {
    name.starts_with("tsconfig")
        || name.starts_with("vite.config.")
        || name.starts_with("next.config.")
        || name.starts_with("nuxt.config.")
        || name.starts_with("svelte.config.")
        || name.starts_with("astro.config.")
        || name == "angular.json"
        || name == "schema.prisma"
}

fn is_entry_file(name: &str) -> bool {
    let (stem, ext) = match name.rfind('.') {
        Some(i) => (&name[..i], &name[i + 1..]),
        None => return false,
    };
    matches!(stem, "main" | "index" | "app" | "server" | "lib" | "mod")
        && CODE_EXTENSIONS.contains(&ext)
}

/// Lower is better. `None` means the file is not wanted at all.
fn priority(path: &str) -> Option<u8> {
    let name = basename(path);
    let d = depth(path);
    let lower = name.to_ascii_lowercase();
    if is_manifest_name(name) {
        return Some(if d == 0 { 0 } else { 6 });
    }
    if lower.starts_with("readme") && d == 0 {
        return Some(1);
    }
    if is_dockerfile(name) || is_compose(name) {
        return Some(if d == 0 { 2 } else { 5 });
    }
    if is_workflow(path) || path == ".gitlab-ci.yml" || path == ".github/dependabot.yml" {
        return Some(3);
    }
    let ext = extension(path);
    if ext == "tf" || ext == "bicep" || name == "Pulumi.yaml" || name.starts_with("Pulumi.") {
        return Some(4);
    }
    if is_yaml(path) && (is_k8s_dir(path) || name == "Chart.yaml" || name == "values.yaml") {
        return Some(4);
    }
    if lower.starts_with("architecture") || lower.starts_with("adr") || has_dir(path, "adr") {
        return Some(5);
    }
    if is_env_template(&lower) || is_frontend_config(name) {
        return Some(5);
    }
    if lower == "security.md" || path == ".github/CODEOWNERS" || path == "CODEOWNERS" {
        return Some(5);
    }
    if is_migration_path(path) {
        return Some(7);
    }
    if is_entry_file(name) && d <= 3 {
        return Some(8);
    }
    if ext == "md" && (path.starts_with("docs/") || path.starts_with("doc/")) {
        return Some(9);
    }
    None
}

/// Paths worth downloading, most valuable first.
pub fn wanted_files(tree: &[TreeEntry]) -> Vec<String> {
    let mut ranked: Vec<(u8, usize, &str)> = tree
        .iter()
        .filter(|e| !e.is_dir && e.size <= MAX_FILE_BYTES)
        .filter(|e| !is_vendored(&e.path) && !is_lock_file(&e.path) && !looks_secret(&e.path))
        .filter(|e| !BINARY_EXTENSIONS.contains(&extension(&e.path).as_str()))
        .filter_map(|e| {
            let mut p = priority(&e.path)?;
            // Migrations are wanted for their names; big ones cost budget for little gain.
            if p == 7 && e.size > 16_000 {
                p = 10;
            }
            Some((p, depth(&e.path), e.path.as_str()))
        })
        .collect();
    ranked.sort();
    let mut docs = 0usize;
    let mut out = Vec::new();
    for (p, _, path) in ranked {
        if p == 9 {
            docs += 1;
            if docs > 12 {
                continue;
            }
        }
        out.push(path.to_string());
        if out.len() >= MAX_FILES {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, size: u64) -> TreeEntry {
        TreeEntry {
            path: path.into(),
            size,
            is_dir: false,
        }
    }

    #[test]
    fn ranks_manifests_first_and_skips_junk() {
        let tree = vec![
            entry("docs/guide.md", 10),
            entry("package.json", 10),
            entry(".env", 10),
            entry("secrets/id_rsa", 10),
            entry("node_modules/x/package.json", 10),
            entry("package-lock.json", 10),
            entry("logo.png", 10),
            entry("README.md", 10),
            entry("huge/Cargo.toml", MAX_FILE_BYTES + 1),
            entry(".github/workflows/ci.yml", 10),
            entry("infra/main.tf", 10),
            entry("certs/server.pem", 10),
            entry(".env.example", 10),
        ];
        let wanted = wanted_files(&tree);
        assert_eq!(
            wanted,
            vec![
                "package.json",
                "README.md",
                ".github/workflows/ci.yml",
                "infra/main.tf",
                ".env.example",
                "docs/guide.md",
            ]
        );
    }

    #[test]
    fn caps_at_max_files() {
        let tree: Vec<TreeEntry> = (0..400)
            .map(|i| entry(&format!("pkg{i}/package.json"), 10))
            .collect();
        assert_eq!(wanted_files(&tree).len(), MAX_FILES);
    }
}

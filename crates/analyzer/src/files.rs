//! Which files are worth downloading. Ranking matters because the scanner
//! stops at [`MAX_FILES`], so the root manifest must never lose to a doc.

use crate::config::CONFIG_PATH;
use crate::model::TreeEntry;
use crate::util::{basename, depth, dirname, excluded, extension, has_dir, is_yaml};
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
            | "packages.config"
            | "cpanfile"
    ) || name.starts_with("requirements") && name.ends_with(".txt")
        || name.ends_with(".csproj")
        || name.ends_with(".vbproj")
        || name.ends_with(".fsproj")
}

/// Files that pin a runtime version (CONTRACT.md §13): cheap, tiny, and the
/// only evidence the end-of-life check has.
pub(crate) fn is_runtime_file(name: &str) -> bool {
    matches!(
        name,
        ".nvmrc"
            | ".node-version"
            | ".tool-versions"
            | "runtime.txt"
            | ".python-version"
            | ".ruby-version"
            | ".go-version"
            | ".java-version"
            | ".sdkmanrc"
            | "global.json"
            | "rust-toolchain"
            | "rust-toolchain.toml"
    )
}

/// Build systems and legacy configuration that have no manifest of their
/// own but say a lot about how old the project is.
pub(crate) fn is_legacy_signal_file(path: &str) -> bool {
    let name = basename(path);
    let lower = name.to_ascii_lowercase();
    matches!(
        name,
        "Makefile"
            | "GNUmakefile"
            | "CMakeLists.txt"
            | "configure.ac"
            | "configure.in"
            | "Makefile.am"
            | "Makefile.PL"
            | "Build.PL"
            | "Directory.Build.props"
    ) || lower == "web.config"
        || lower == "web.xml"
        || lower == "wp-config.php"
        || lower == "config.ru"
        || name.ends_with(".sln")
        || name.ends_with(".gemspec")
}

/// Hand-rolled deploy scripts are read to see whether they push over FTP.
pub(crate) fn is_deploy_script(path: &str) -> bool {
    let lower = basename(path).to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "deploy.sh"
            | "deploy.bat"
            | "deploy.ps1"
            | "deploy.cmd"
            | "publish.sh"
            | "upload.sh"
            | "release.sh"
            | "ftp.sh"
            | ".ftpconfig"
            | "sftp-config.json"
            | "deploy.php"
    ) || (lower.starts_with("ftp") && (lower.ends_with(".sh") || lower.ends_with(".txt")))
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

/// Deploy pipelines that live outside `.github/workflows` (CONTRACT.md §22):
/// a `Jenkinsfile` (also `Jenkinsfile.deploy`) anywhere in the tree.
pub(crate) fn is_jenkinsfile(path: &str) -> bool {
    let name = basename(path);
    name == "Jenkinsfile"
        || name.strip_prefix("Jenkinsfile.").is_some_and(|suffix| {
            !suffix.is_empty() && !matches!(suffix, "md" | "txt" | "bak" | "orig")
        })
}

/// `azure-pipelines.yml` at any depth, or a pipeline under `.azure-pipelines/`.
pub(crate) fn is_azure_pipelines(path: &str) -> bool {
    if !is_yaml(path) {
        return false;
    }
    let name = basename(path);
    name.starts_with("azure-pipelines.")
        || name.starts_with(".azure-pipelines.")
        || has_dir(path, ".azure-pipelines")
}

/// Directories that only a GitOps layout has: what Flux and Argo CD reconcile.
fn is_gitops_only_dir(path: &str) -> bool {
    [
        "clusters",
        "gitops",
        "flux",
        "flux-system",
        "argocd",
        "argo-cd",
    ]
    .iter()
    .any(|d| has_dir(path, d))
}

/// Where GitOps repositories keep their manifests. `apps/` is the Flux and
/// Argo CD convention for workloads, and also where monorepos keep code.
pub(crate) fn is_gitops_dir(path: &str) -> bool {
    is_gitops_only_dir(path) || has_dir(path, "apps")
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

// Priorities, lowest first. Named so the special cases below read as intent.
const P_CONFIG: u8 = 0;
const P_ROOT_MANIFEST: u8 = 1;
const P_README: u8 = 2;
const P_ROOT_CONTAINER: u8 = 3;
const P_PIPELINE: u8 = 4;
const P_IAC: u8 = 5;
const P_SUPPORTING: u8 = 6;
const P_NESTED_MANIFEST: u8 = 7;
const P_GITOPS: u8 = 8;
const P_GITOPS_APPS: u8 = 9;
const P_MIGRATION: u8 = 10;
const P_ENTRY: u8 = 11;
const P_DOC: u8 = 12;
const P_BIG_MIGRATION: u8 = 13;

/// YAML under `clusters/`, `gitops/` or `apps/` is read to recognise Flux
/// and Argo CD. A handful of files settles that, and `apps/` is also where
/// monorepos keep unrelated YAML, so those files rank after the
/// unambiguous directories and the budget for all of them is capped.
const MAX_GITOPS_FILES: usize = 40;

/// Lower is better. `None` means the file is not wanted at all.
fn priority(path: &str) -> Option<u8> {
    let name = basename(path);
    let d = depth(path);
    let lower = name.to_ascii_lowercase();
    if path == CONFIG_PATH {
        // The repository's own declarations outrank everything we could infer.
        return Some(P_CONFIG);
    }
    if is_manifest_name(name) || (is_runtime_file(name) && d == 0) {
        return Some(if d == 0 {
            P_ROOT_MANIFEST
        } else {
            P_NESTED_MANIFEST
        });
    }
    if lower.starts_with("readme") && d == 0 {
        return Some(P_README);
    }
    if is_dockerfile(name) || is_compose(name) {
        return Some(if d == 0 {
            P_ROOT_CONTAINER
        } else {
            P_SUPPORTING
        });
    }
    if is_workflow(path) || path == ".gitlab-ci.yml" || path == ".github/dependabot.yml" {
        return Some(P_PIPELINE);
    }
    if is_jenkinsfile(path) || is_azure_pipelines(path) {
        return Some(P_PIPELINE);
    }
    let ext = extension(path);
    if ext == "tf" || ext == "bicep" || name == "Pulumi.yaml" || name.starts_with("Pulumi.") {
        return Some(P_IAC);
    }
    if is_yaml(path) && (is_k8s_dir(path) || name == "Chart.yaml" || name == "values.yaml") {
        return Some(P_IAC);
    }
    if is_legacy_signal_file(path) || is_deploy_script(path) || is_runtime_file(name) {
        return Some(if d == 0 {
            P_ROOT_MANIFEST
        } else {
            P_SUPPORTING
        });
    }
    if lower.starts_with("architecture") || lower.starts_with("adr") || has_dir(path, "adr") {
        return Some(P_SUPPORTING);
    }
    if is_env_template(&lower) || is_frontend_config(name) {
        return Some(P_SUPPORTING);
    }
    if lower == "security.md" || path == ".github/CODEOWNERS" || path == "CODEOWNERS" {
        return Some(P_SUPPORTING);
    }
    if is_yaml(path) && is_gitops_dir(path) {
        return Some(if is_gitops_only_dir(path) {
            P_GITOPS
        } else {
            P_GITOPS_APPS
        });
    }
    if is_migration_path(path) {
        return Some(P_MIGRATION);
    }
    if is_entry_file(name) && d <= 3 {
        return Some(P_ENTRY);
    }
    if ext == "md" && (path.starts_with("docs/") || path.starts_with("doc/")) {
        return Some(P_DOC);
    }
    None
}

/// Paths worth downloading, most valuable first.
pub fn wanted_files(tree: &[TreeEntry]) -> Vec<String> {
    wanted_files_with(tree, &[])
}

/// [`wanted_files`] minus everything matching an `.sdi.yml` `scan.exclude`
/// glob. `.sdi.yml` itself can never be excluded, otherwise a typo in the
/// file would hide the file that carries the typo.
pub fn wanted_files_with(tree: &[TreeEntry], exclude: &[String]) -> Vec<String> {
    let mut ranked: Vec<(u8, usize, &str)> = tree
        .iter()
        .filter(|e| !e.is_dir && e.size <= MAX_FILE_BYTES)
        .filter(|e| !is_vendored(&e.path) && !is_lock_file(&e.path) && !looks_secret(&e.path))
        .filter(|e| !BINARY_EXTENSIONS.contains(&extension(&e.path).as_str()))
        .filter(|e| e.path == CONFIG_PATH || !excluded(&e.path, exclude))
        .filter_map(|e| {
            let mut p = priority(&e.path)?;
            // Migrations are wanted for their names; big ones cost budget for little gain.
            if p == P_MIGRATION && e.size > 16_000 {
                p = P_BIG_MIGRATION;
            }
            Some((p, depth(&e.path), e.path.as_str()))
        })
        .collect();
    ranked.sort();
    let mut docs = 0usize;
    let mut gitops = 0usize;
    let mut out = Vec::new();
    for (p, _, path) in ranked {
        if p == P_DOC {
            docs += 1;
            if docs > 12 {
                continue;
            }
        }
        if p == P_GITOPS || p == P_GITOPS_APPS {
            gitops += 1;
            if gitops > MAX_GITOPS_FILES {
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
    fn config_first_runtime_files_and_excludes() {
        let tree = vec![
            entry("package.json", 10),
            entry(".sdi.yml", 10),
            entry(".nvmrc", 3),
            entry("WebApp.sln", 10),
            entry("WebApp/WebApp.csproj", 10),
            entry("WebApp/packages.config", 10),
            entry("WebApp/Web.config", 10),
            entry("Makefile", 10),
            entry("CMakeLists.txt", 10),
            entry("deploy.sh", 10),
            entry("vendor-legacy/old.csproj", 10),
            entry("docs/legacy/plan.md", 10),
            entry("docs/current/plan.md", 10),
        ];
        let all = wanted_files(&tree);
        assert_eq!(all[0], ".sdi.yml");
        for p in [
            ".nvmrc",
            "WebApp.sln",
            "WebApp/WebApp.csproj",
            "WebApp/packages.config",
            "WebApp/Web.config",
            "Makefile",
            "CMakeLists.txt",
            "deploy.sh",
            "docs/legacy/plan.md",
        ] {
            assert!(all.iter().any(|w| w == p), "missing {p}: {all:?}");
        }
        let filtered = wanted_files_with(
            &tree,
            &[
                "docs/legacy/**".into(),
                "vendor-legacy".into(),
                ".sdi.yml".into(),
            ],
        );
        assert_eq!(
            filtered[0], ".sdi.yml",
            "the config can never exclude itself"
        );
        assert!(!filtered.iter().any(|w| w == "docs/legacy/plan.md"));
        assert!(!filtered.iter().any(|w| w.starts_with("vendor-legacy/")));
        assert!(filtered.iter().any(|w| w == "docs/current/plan.md"));
    }

    #[test]
    fn delivery_pipelines_and_gitops_manifests_are_wanted() {
        let mut tree = vec![
            entry("package.json", 10),
            entry("Jenkinsfile", 10),
            entry("ci/Jenkinsfile.deploy", 10),
            entry("azure-pipelines.yml", 10),
            entry(".azure-pipelines/release.yaml", 10),
            entry("clusters/production/flux-system/gotk-sync.yaml", 10),
            entry("apps/api/kustomization.yaml", 10),
            entry("gitops/argocd/application.yaml", 10),
            entry("apps/api/src/handler.ts", 10),
            entry("notes/Jenkinsfile.md", 10),
        ];
        let wanted = wanted_files(&tree);
        for p in [
            "Jenkinsfile",
            "ci/Jenkinsfile.deploy",
            "azure-pipelines.yml",
            ".azure-pipelines/release.yaml",
            "clusters/production/flux-system/gotk-sync.yaml",
            "apps/api/kustomization.yaml",
            "gitops/argocd/application.yaml",
        ] {
            assert!(wanted.iter().any(|w| w == p), "missing {p}: {wanted:?}");
        }
        assert!(!wanted.iter().any(|w| w == "apps/api/src/handler.ts"));
        assert!(!wanted.iter().any(|w| w == "notes/Jenkinsfile.md"));
        // Pipelines rank with the workflows, ahead of the GitOps manifests.
        let position = |p: &str| wanted.iter().position(|w| w == p).unwrap();
        assert!(position("Jenkinsfile") < position("apps/api/kustomization.yaml"));
        // A monorepo's `apps/` cannot spend the whole budget on YAML.
        tree.extend((0..200).map(|i| entry(&format!("apps/svc{i}/config.yaml"), 10)));
        let wanted = wanted_files(&tree);
        let gitops = wanted
            .iter()
            .filter(|w| is_yaml(w) && is_gitops_dir(w))
            .count();
        assert_eq!(gitops, MAX_GITOPS_FILES);
        for p in [
            "package.json",
            "clusters/production/flux-system/gotk-sync.yaml",
            "gitops/argocd/application.yaml",
        ] {
            assert!(wanted.iter().any(|w| w == p), "missing {p}");
        }
    }

    #[test]
    fn caps_at_max_files() {
        let tree: Vec<TreeEntry> = (0..400)
            .map(|i| entry(&format!("pkg{i}/package.json"), 10))
            .collect();
        assert_eq!(wanted_files(&tree).len(), MAX_FILES);
    }
}

//! Code browser (CONTRACT.md §15): the repository tree and single files at
//! the scanned commit, served from the `repo_trees` / `repo_files` caches
//! and fetched from GitHub on a miss.
//!
//! Data controls (§19) decide what a miss leaves behind: `none` returns the
//! file without storing it, `session` stores it with an expiry, `persistent`
//! stores it for good. Every file read is audited (`file.read`).
//!
//! `GetRepoFile` with a `ref` (§23) reads one file at another commit, a
//! pull request's head: straight from GitHub, with the same limits and the
//! same refusals, and never into the cache, whatever the data controls say.

use sdi_common::grpc::db_status;
use sdi_proto::knowledge::v1::*;
use tonic::Status;

use super::{KnowledgeState, github_status};
use crate::{
    auth::Caller,
    db::{
        self,
        code::{FileRow, NewFile, TreeEntryRecord},
        projects::ProjectRow,
    },
    github::split_full_name,
};

/// Largest file the browser shows in full; longer ones are cut here and
/// flagged `truncated`.
pub const MAX_FILE_BYTES: usize = 1 << 20;

pub(super) async fn get_repo_tree(
    state: &KnowledgeState,
    caller: &Caller,
    req: GetRepoTreeRequest,
) -> Result<RepoTree, Status> {
    let ws = caller.workspace_id;
    let project = state.project(caller, &req.project_id).await?;
    let commit_sha = scanned_commit(state, caller, &project).await?;

    let cached = db::code::get_tree(&state.pool, ws, project.id, &commit_sha)
        .await
        .map_err(db_status)?;
    let (mut entries, truncated) = match cached {
        Some(row) => (row.entries.0, row.truncated),
        None => {
            let (owner, name) = repo_parts(&project)?;
            let github = state.github(caller).await?;
            let tree = github
                .full_tree(owner, name, &commit_sha)
                .await
                .map_err(github_status)?;
            let entries: Vec<TreeEntryRecord> = tree
                .tree
                .into_iter()
                // Submodules have no content here and symlinks only a
                // target; the UI cannot open either.
                .filter(|node| node.is_file() || node.is_dir())
                .map(|node| TreeEntryRecord {
                    is_dir: node.is_dir(),
                    size: node.size.unwrap_or(0),
                    path: node.path,
                })
                .collect();
            db::code::upsert_tree(
                &state.pool,
                ws,
                project.id,
                &commit_sha,
                &entries,
                tree.truncated,
            )
            .await
            .map_err(db_status)?;
            (entries, tree.truncated)
        }
    };
    entries.sort_by(|a, b| a.path.cmp(&b.path));

    Ok(RepoTree {
        project_id: project.id.to_string(),
        commit_sha,
        entries: entries.into_iter().map(tree_entry).collect(),
        truncated,
    })
}

pub(super) async fn get_repo_file(
    state: &KnowledgeState,
    caller: &Caller,
    req: GetRepoFileRequest,
) -> Result<RepoFile, Status> {
    let ws = caller.workspace_id;
    let path = validate_path(&req.path)?;
    if looks_secret(path) {
        return Err(Status::permission_denied(
            "this file looks like a secret and is never displayed",
        ));
    }
    // Before the project lookup: a malformed ref is the caller's mistake
    // whatever project it names.
    let reference = commit_ref(&req.r#ref)?;
    let project = state.project(caller, &req.project_id).await?;
    let (row, fact_ids) = match reference {
        // Another commit than the scanned one: read, answer, keep nothing.
        // Facts describe the scanned snapshot, so none are claimed for it.
        Some(sha) => (
            fetch(state, caller, &project, &sha, path).await?,
            Vec::new(),
        ),
        None => {
            let commit_sha = scanned_commit(state, caller, &project).await?;
            let cached = db::code::get_file(&state.pool, ws, project.id, &commit_sha, path)
                .await
                .map_err(db_status)?;
            let row = match cached {
                Some(row) => row,
                None => fetch_and_maybe_store(state, caller, &project, &commit_sha, path).await?,
            };
            let fact_ids = db::facts::ids_for_path(&state.pool, ws, project.id, path)
                .await
                .map_err(db_status)?;
            (row, fact_ids)
        }
    };
    state.identity.audit_file_read(
        caller,
        &project.id.to_string(),
        &project.repo_full_name,
        &row.commit_sha,
        &row.path,
    );

    Ok(RepoFile {
        html_url: format!(
            "{}/blob/{}/{}",
            project.repo_url.trim_end_matches('/'),
            row.commit_sha,
            row.path
        ),
        path: row.path,
        commit_sha: row.commit_sha,
        content: row.content,
        binary: row.binary,
        truncated: row.truncated,
        size: row.size.max(0) as u64,
        language: row.language,
        fact_ids: fact_ids.into_iter().map(|id| id.to_string()).collect(),
    })
}

/// The `ref` of a `GetRepoFile` request: nothing (the scanned commit), or
/// a full commit SHA. Branch names, tags and abbreviated SHAs are refused:
/// what is read must be exactly the commit the caller reviewed, and a name
/// can move between two calls.
pub(crate) fn commit_ref(raw: &str) -> Result<Option<String>, Status> {
    let reference = raw.trim();
    if reference.is_empty() {
        return Ok(None);
    }
    if reference.len() != 40 || !reference.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Status::invalid_argument(
            "ref must be a full 40-character commit SHA",
        ));
    }
    Ok(Some(reference.to_ascii_lowercase()))
}

/// What GitHub holds for `path` at `commit_sha`, classified and bounded.
struct Fetched {
    content: String,
    binary: bool,
    truncated: bool,
    size: u64,
}

/// Reads the file from GitHub: at most [`MAX_FILE_BYTES`], binary files
/// without content.
async fn fetch_raw(
    state: &KnowledgeState,
    caller: &Caller,
    project: &ProjectRow,
    commit_sha: &str,
    path: &str,
) -> Result<Fetched, Status> {
    let ws = caller.workspace_id;
    let (owner, name) = repo_parts(project)?;
    let github = state.github(caller).await?;
    let raw = github
        .file_raw(owner, name, path, commit_sha, MAX_FILE_BYTES)
        .await
        .map_err(github_status)?;
    let classified = classify(path, &raw.bytes, raw.truncated);
    // A cut file's real size is what GitHub announced, else what the tree
    // said (only the scanned commit has one); as a last resort the bytes
    // read, a lower bound.
    let size = match (raw.truncated, raw.content_length) {
        (false, _) => raw.bytes.len() as u64,
        (true, Some(len)) => len,
        (true, None) => db::code::tree_entry_size(&state.pool, ws, project.id, commit_sha, path)
            .await
            .map_err(db_status)?
            .and_then(|size| u64::try_from(size).ok())
            .unwrap_or(raw.bytes.len() as u64),
    };
    Ok(Fetched {
        content: classified.content,
        binary: classified.binary,
        truncated: classified.truncated,
        size,
    })
}

impl Fetched {
    fn as_new<'a>(&'a self, commit_sha: &'a str, path: &'a str) -> NewFile<'a> {
        NewFile {
            commit_sha,
            path,
            content: &self.content,
            binary: self.binary,
            truncated: self.truncated,
            size: self.size,
            language: sdi_analyzer::language_for_path(path),
        }
    }
}

/// Fetches the file for the answer only: nothing is written.
async fn fetch(
    state: &KnowledgeState,
    caller: &Caller,
    project: &ProjectRow,
    commit_sha: &str,
    path: &str,
) -> Result<FileRow, Status> {
    let fetched = fetch_raw(state, caller, project, commit_sha, path).await?;
    Ok(unstored(
        caller.workspace_id,
        project.id,
        &fetched.as_new(commit_sha, path),
    ))
}

/// Fetches the file and, depending on the workspace's `code_cache`, stores
/// it with or without an expiry, or not at all.
async fn fetch_and_maybe_store(
    state: &KnowledgeState,
    caller: &Caller,
    project: &ProjectRow,
    commit_sha: &str,
    path: &str,
) -> Result<FileRow, Status> {
    let ws = caller.workspace_id;
    let fetched = fetch_raw(state, caller, project, commit_sha, path).await?;
    let file = fetched.as_new(commit_sha, path);
    let controls = state.controls.get(caller).await;
    if !controls.stores_files() {
        return Ok(unstored(ws, project.id, &file));
    }
    db::code::upsert_file(
        &state.pool,
        ws,
        project.id,
        &file,
        controls.file_expiry(chrono::Utc::now()),
    )
    .await
    .map_err(db_status)
}

/// The row that would have been stored, kept only for the answer: what a
/// `code_cache = none` workspace gets, and every read at a `ref`.
fn unstored(workspace_id: uuid::Uuid, project_id: uuid::Uuid, file: &NewFile<'_>) -> FileRow {
    FileRow {
        workspace_id,
        project_id,
        commit_sha: file.commit_sha.to_owned(),
        path: file.path.to_owned(),
        content: file.content.to_owned(),
        binary: file.binary,
        truncated: file.truncated,
        size: i64::try_from(file.size).unwrap_or(i64::MAX),
        language: file.language.to_owned(),
        fetched_at: chrono::Utc::now(),
        expires_at: None,
    }
}

/// The commit the project's last successful scan analysed; the browser
/// always shows that snapshot so facts and diagrams line up with the code.
async fn scanned_commit(
    state: &KnowledgeState,
    caller: &Caller,
    project: &ProjectRow,
) -> Result<String, Status> {
    let Some(scan_id) = project.last_scan_id else {
        return Err(Status::failed_precondition(
            "project has not been scanned yet",
        ));
    };
    let scan = db::scans::get(&state.pool, caller.workspace_id, scan_id)
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => {
                Status::failed_precondition("project has not been scanned yet")
            }
            other => db_status(other),
        })?;
    if scan.commit_sha.trim().is_empty() {
        return Err(Status::failed_precondition(
            "the last scan recorded no commit",
        ));
    }
    Ok(scan.commit_sha)
}

fn repo_parts(project: &ProjectRow) -> Result<(&str, &str), Status> {
    split_full_name(&project.repo_full_name).ok_or_else(|| {
        Status::internal(format!(
            "project has an invalid repository name: {}",
            project.repo_full_name
        ))
    })
}

fn tree_entry(record: TreeEntryRecord) -> RepoTreeEntry {
    let name = record
        .path
        .rsplit('/')
        .next()
        .unwrap_or(&record.path)
        .to_owned();
    let language = if record.is_dir {
        ""
    } else {
        sdi_analyzer::language_for_path(&record.path)
    };
    RepoTreeEntry {
        name,
        is_dir: record.is_dir,
        size: record.size,
        language: language.to_owned(),
        path: record.path,
    }
}

/// A repository-relative path: non-empty, no NUL, no leading `/`, and no
/// `.` / `..` / empty components, so nothing can point outside the tree or
/// at a different spelling of the same file (which would dodge the secret
/// check and split the cache).
pub(crate) fn validate_path(raw: &str) -> Result<&str, Status> {
    let path = raw.trim();
    if path.is_empty() {
        return Err(Status::invalid_argument("path is required"));
    }
    if path.contains('\0') {
        return Err(Status::invalid_argument("path must not contain NUL"));
    }
    if path.starts_with('/') || path.contains('\\') {
        return Err(Status::invalid_argument(
            "path must be relative to the repository root",
        ));
    }
    if path
        .split('/')
        .any(|component| matches!(component, "" | "." | ".."))
    {
        return Err(Status::invalid_argument(
            "path must not contain empty, `.` or `..` components",
        ));
    }
    Ok(path)
}

/// Files the platform never displays, whatever the repository holds:
/// environment files, private keys and certificates, credential stores.
/// Judged on the file name, in any directory.
pub(crate) fn looks_secret(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    let stem = name
        .rsplit_once('.')
        .map_or(name.as_str(), |(stem, _)| stem);
    name.starts_with(".env")
        || name.starts_with("id_rsa")
        || name.starts_with("id_dsa")
        || name.starts_with("id_ecdsa")
        || name.starts_with("id_ed25519")
        || name.starts_with("credentials")
        || stem == "credentials"
        || matches!(
            name.rsplit_once('.').map(|(_, ext)| ext),
            Some("pem" | "key" | "p12" | "pfx" | "jks" | "keystore" | "ppk")
        )
        || matches!(
            name.as_str(),
            ".netrc"
                | ".npmrc"
                | ".pypirc"
                | ".htpasswd"
                | ".git-credentials"
                | "secrets.yml"
                | "secrets.yaml"
                | "secrets.json"
        )
}

/// Extensions whose content is never text, so a file is binary before a
/// byte is inspected (an SVG or a lockfile is text and is not listed).
const BINARY_EXTENSIONS: &[&str] = &[
    "png",
    "jpg",
    "jpeg",
    "gif",
    "webp",
    "bmp",
    "ico",
    "icns",
    "tif",
    "tiff",
    "psd",
    "ai",
    "heic",
    "avif",
    "pdf",
    "zip",
    "gz",
    "tgz",
    "bz2",
    "xz",
    "zst",
    "7z",
    "rar",
    "tar",
    "jar",
    "war",
    "ear",
    "class",
    "exe",
    "dll",
    "so",
    "dylib",
    "o",
    "a",
    "lib",
    "obj",
    "bin",
    "wasm",
    "woff",
    "woff2",
    "ttf",
    "otf",
    "eot",
    "mp3",
    "mp4",
    "m4a",
    "mov",
    "avi",
    "mkv",
    "wav",
    "ogg",
    "flac",
    "webm",
    "sqlite",
    "sqlite3",
    "db",
    "pyc",
    "pyo",
    "dex",
    "apk",
    "ipa",
    "dmg",
    "iso",
    "img",
    "pkg",
    "deb",
    "rpm",
    "msi",
    "swf",
    "fla",
    "blend",
    "fbx",
    "glb",
    "parquet",
    "npy",
    "npz",
    "pb",
    "onnx",
    "h5",
    "pt",
    "pth",
    "safetensors",
];

struct Classified {
    content: String,
    binary: bool,
    truncated: bool,
}

/// Binary by extension or by a NUL byte; binary files carry no content. A
/// cut text file loses a possibly half-read last character rather than
/// showing a replacement glyph that was never in the repository.
fn classify(path: &str, bytes: &[u8], truncated: bool) -> Classified {
    let ext = path
        .rsplit('/')
        .next()
        .and_then(|name| name.rsplit_once('.'))
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default();
    let binary = BINARY_EXTENSIONS.contains(&ext.as_str()) || bytes.contains(&0);
    if binary {
        return Classified {
            content: String::new(),
            binary: true,
            truncated,
        };
    }
    let mut content = match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(error) if truncated && error.error_len().is_none() => {
            // Only the cut character is incomplete; keep the valid prefix.
            String::from_utf8_lossy(&bytes[..error.valid_up_to()]).into_owned()
        }
        Err(_) => String::from_utf8_lossy(bytes).into_owned(),
    };
    content.shrink_to_fit();
    Classified {
        content,
        binary: false,
        truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_validated() {
        assert_eq!(validate_path("src/main.rs").unwrap(), "src/main.rs");
        assert_eq!(validate_path("  README.md ").unwrap(), "README.md");
        for bad in [
            "",
            "   ",
            "/etc/passwd",
            "../secret",
            "src/../.env",
            "src/./a.rs",
            "src//a.rs",
            "a\0b",
            "src\\main.rs",
            "src/",
        ] {
            let error = validate_path(bad).unwrap_err();
            assert_eq!(error.code(), tonic::Code::InvalidArgument, "{bad:?}");
        }
    }

    #[test]
    fn a_ref_is_a_full_commit_sha_or_nothing() {
        assert_eq!(commit_ref("").unwrap(), None);
        assert_eq!(commit_ref("   ").unwrap(), None);
        let sha = "0123456789abcdef0123456789ABCDEF01234567";
        assert_eq!(
            commit_ref(&format!(" {sha} ")).unwrap().as_deref(),
            Some("0123456789abcdef0123456789abcdef01234567")
        );
        for bad in [
            "main",
            "refs/heads/main",
            "v1.2.3",
            "0123456",
            // 39 and 41 characters, and 40 that are not all hex.
            "0123456789abcdef0123456789abcdef0123456",
            "0123456789abcdef0123456789abcdef012345678",
            "0123456789abcdef0123456789abcdef0123456g",
            "0123456789abcdef0123456789abcdef0123456/",
            "../../../0123456789abcdef0123456789abcdef",
        ] {
            let error = commit_ref(bad).unwrap_err();
            assert_eq!(error.code(), tonic::Code::InvalidArgument, "{bad:?}");
            assert!(error.message().contains("40-character"), "{bad:?}");
        }
    }

    #[test]
    fn secret_looking_files_are_recognised() {
        for secret in [
            ".env",
            ".env.local",
            "config/.env.production",
            "certs/server.pem",
            "certs/server.key",
            "keys/id_rsa",
            "keys/id_rsa.pub",
            ".ssh/id_ed25519",
            "store.p12",
            "credentials",
            "credentials.json",
            "aws/credentials.ini",
            ".npmrc",
            "config/secrets.yml",
            // Prefix rules are deliberately broad: a stray sample is cheaper
            // to hide than a real secret is to show.
            ".env.example.md",
            "config/credentials-doc.md",
        ] {
            assert!(looks_secret(secret), "{secret}");
        }
        for plain in [
            "src/main.rs",
            "environment.ts",
            "docs/keys.md",
            "src/keyboard.rs",
            "README",
            "Cargo.lock",
            "docs/env.md",
        ] {
            assert!(!looks_secret(plain), "{plain}");
        }
    }

    #[test]
    fn classification_detects_binaries_and_keeps_text() {
        let text = classify("src/main.rs", b"fn main() {}\n", false);
        assert!(!text.binary);
        assert_eq!(text.content, "fn main() {}\n");

        let nul = classify("data/blob.txt", b"abc\0def", false);
        assert!(nul.binary);
        assert!(nul.content.is_empty());

        let png = classify("assets/logo.PNG", b"not really", false);
        assert!(png.binary);

        let svg = classify("assets/logo.svg", b"<svg/>", false);
        assert!(!svg.binary);
        assert_eq!(svg.content, "<svg/>");
    }

    #[test]
    fn a_cut_multibyte_character_is_dropped_not_replaced() {
        // "é" is two bytes; cut after the first one.
        let bytes = "ab\u{e9}".as_bytes();
        let cut = classify("notes.md", &bytes[..3], true);
        assert!(cut.truncated);
        assert_eq!(cut.content, "ab");

        // Genuinely invalid bytes in an untruncated file are replaced instead.
        let bad = classify("notes.md", b"ab\xffcd", false);
        assert!(!bad.binary);
        assert_eq!(bad.content, "ab\u{fffd}cd");
    }

    #[test]
    fn tree_entries_derive_name_and_language() {
        let file = tree_entry(TreeEntryRecord {
            path: "web/src/App.tsx".into(),
            is_dir: false,
            size: 12,
        });
        assert_eq!(file.name, "App.tsx");
        assert_eq!(file.language, "tsx");
        assert_eq!(file.size, 12);
        let dir = tree_entry(TreeEntryRecord {
            path: "web/src".into(),
            is_dir: true,
            size: 0,
        });
        assert_eq!(dir.name, "src");
        assert!(dir.is_dir);
        assert_eq!(dir.language, "");
    }
}

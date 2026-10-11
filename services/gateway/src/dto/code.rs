//! Code browser (CONTRACT.md §15): the repository tree and one file at the
//! scanned commit.

use sdi_proto::knowledge::v1 as proto;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

/// One file or directory of the repository tree.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RepoTreeEntry {
    /// Repository-relative path, e.g. `web/src/App.tsx`.
    pub path: String,
    /// Last path component.
    pub name: String,
    /// True for directories.
    pub is_dir: bool,
    /// Size in bytes; 0 for directories.
    pub size: u64,
    /// Lowercase language id from the extension, e.g. `typescript`; empty when unknown.
    pub language: String,
}

impl From<proto::RepoTreeEntry> for RepoTreeEntry {
    fn from(entry: proto::RepoTreeEntry) -> Self {
        Self {
            path: entry.path,
            name: entry.name,
            is_dir: entry.is_dir,
            size: entry.size,
            language: entry.language,
        }
    }
}

/// Every entry of the repository at the scanned commit, sorted by path.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RepoTree {
    /// Project id.
    pub project_id: String,
    /// Commit the last successful scan analysed.
    pub commit_sha: String,
    /// Files and directories, sorted by path.
    pub entries: Vec<RepoTreeEntry>,
    /// GitHub truncated the recursive tree (very large repositories).
    pub truncated: bool,
}

impl From<proto::RepoTree> for RepoTree {
    fn from(tree: proto::RepoTree) -> Self {
        Self {
            project_id: tree.project_id,
            commit_sha: tree.commit_sha,
            entries: tree.entries.into_iter().map(Into::into).collect(),
            truncated: tree.truncated,
        }
    }
}

/// One file's text at the scanned commit.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RepoFile {
    /// Repository-relative path.
    pub path: String,
    /// Commit the content belongs to.
    pub commit_sha: String,
    /// UTF-8 text; empty when binary.
    pub content: String,
    /// The file is not text (NUL bytes or a known binary extension).
    pub binary: bool,
    /// Content was cut at the 1 MiB limit; `size` is the real size.
    pub truncated: bool,
    /// Size in bytes on GitHub.
    pub size: u64,
    /// Lowercase language id from the extension; empty when unknown.
    pub language: String,
    /// `https://github.com/{owner}/{repo}/blob/{sha}/{path}`.
    pub html_url: String,
    /// Ids of the facts detected from this file.
    pub fact_ids: Vec<String>,
}

impl From<proto::RepoFile> for RepoFile {
    fn from(file: proto::RepoFile) -> Self {
        Self {
            path: file.path,
            commit_sha: file.commit_sha,
            content: file.content,
            binary: file.binary,
            truncated: file.truncated,
            size: file.size,
            language: file.language,
            html_url: file.html_url,
            fact_ids: file.fact_ids,
        }
    }
}

/// Query of `GET /projects/{id}/file`.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct GetRepoFileQuery {
    /// Repository-relative path of the file, e.g. `src/main.rs`.
    pub path: String,
}

//! Pure repository analysis: files in, facts, graph and understanding out.
//!
//! No network, no database, no clock. The knowledge service owns all I/O and
//! calls in here, which keeps every detector unit-testable with plain strings.

pub mod model;

pub use model::*;

/// Largest file the scanner should download, in bytes.
pub const MAX_FILE_BYTES: u64 = 200_000;

/// Upper bound on how many files one scan downloads.
pub const MAX_FILES: usize = 150;

/// Paths worth downloading for analysis, most valuable first.
///
/// The result is bounded by [`MAX_FILES`] and never includes files larger
/// than [`MAX_FILE_BYTES`], lock files, binaries or anything that looks like
/// a secret store.
pub fn wanted_files(_tree: &[TreeEntry]) -> Vec<String> {
    Vec::new()
}

/// Turns a snapshot into the project knowledge model.
pub fn analyze(_snapshot: &RepoSnapshot) -> Analysis {
    Analysis::default()
}

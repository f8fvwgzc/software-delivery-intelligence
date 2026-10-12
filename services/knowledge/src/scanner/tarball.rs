//! Unpacks a GitHub repository archive into the files the analyzer wants.
//!
//! One archive download replaces one `/contents` request per file, which is
//! what made v1 scans take tens of seconds on repositories with a hundred
//! manifests. The archive is `owner-repo-sha/<path>`; the first component is
//! stripped so paths match the tree the analyzer already selected from.

use std::{
    collections::{BTreeMap, HashSet},
    io::Read,
};

use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use tar::{Archive, EntryType};

/// Archives larger than this are not kept in memory; the scanner falls back
/// to per-file downloads, which only fetch what the analyzer wants.
pub const MAX_TARBALL_BYTES: usize = 50 * 1024 * 1024;

/// Keeps only regular files whose stripped path is in `wanted` and that are
/// at most `max_file_bytes`; bytes are decoded lossily because the analyzer
/// only reads text (a file of garbage bytes becomes replacement glyphs,
/// never a panic). Symlinks, hard links and anything that is not a regular
/// file are skipped. A damaged archive is an error: the caller then falls
/// back to per-file downloads. `source` is the gzipped archive: a byte
/// slice in tests, the streamed temporary file in a scan.
pub fn unpack<R: Read>(
    source: R,
    wanted: &HashSet<String>,
    max_file_bytes: usize,
) -> Result<BTreeMap<String, String>> {
    let mut archive = Archive::new(GzDecoder::new(source));
    let mut files = BTreeMap::new();
    for entry in archive.entries().context("tar entries")? {
        let mut entry = entry.context("tar entry")?;
        let kind = entry.header().entry_type();
        if !matches!(kind, EntryType::Regular | EntryType::Continuous) {
            continue;
        }
        let path = entry.path().context("tar entry path")?;
        let Some(stripped) = strip_root(&path.to_string_lossy()) else {
            continue;
        };
        if !wanted.contains(&stripped) {
            continue;
        }
        // Size is checked twice: the header is a hint, the read is the truth.
        if entry.header().size().unwrap_or(0) > max_file_bytes as u64 {
            continue;
        }
        let mut buf = Vec::with_capacity(entry.header().size().unwrap_or(0) as usize);
        entry
            .by_ref()
            .take(max_file_bytes as u64 + 1)
            .read_to_end(&mut buf)
            .with_context(|| format!("reading {stripped}"))?;
        if buf.len() > max_file_bytes {
            continue;
        }
        files.insert(stripped, String::from_utf8_lossy(&buf).into_owned());
        if files.len() == wanted.len() {
            break;
        }
    }
    Ok(files)
}

/// `owner-repo-sha/src/main.rs` → `src/main.rs`. The root directory entry
/// itself and anything without a root are skipped.
fn strip_root(path: &str) -> Option<String> {
    let rest = path.trim_start_matches("./").split_once('/')?.1;
    (!rest.is_empty()).then(|| rest.to_owned())
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::{Compression, write::GzEncoder};
    use tar::{Builder, Header};

    use super::*;

    fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut builder = Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
        let mut dir = Header::new_gnu();
        dir.set_entry_type(EntryType::Directory);
        dir.set_size(0);
        dir.set_mode(0o755);
        dir.set_cksum();
        builder
            .append_data(&mut dir, "owner-repo-abc123/", std::io::empty())
            .unwrap();
        for (path, body) in entries {
            let mut header = Header::new_gnu();
            header.set_size(body.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, format!("owner-repo-abc123/{path}"), *body)
                .unwrap();
        }
        let encoder = builder.into_inner().unwrap();
        let mut out = encoder.finish().unwrap();
        out.flush().unwrap();
        out
    }

    fn wanted(paths: &[&str]) -> HashSet<String> {
        paths.iter().map(|p| (*p).to_owned()).collect()
    }

    #[test]
    fn keeps_only_wanted_files_with_the_root_stripped() {
        let bytes = archive(&[
            ("Cargo.toml", b"[package]\nname = \"x\"\n"),
            ("src/main.rs", b"fn main() {}\n"),
            ("README.md", b"# x\n"),
            ("target/debug/junk.bin", b"\x00\x01\x02"),
        ]);
        let files = unpack(&bytes[..], &wanted(&["Cargo.toml", "src/main.rs"]), 1024).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files["Cargo.toml"], "[package]\nname = \"x\"\n");
        assert_eq!(files["src/main.rs"], "fn main() {}\n");
        assert!(!files.contains_key("README.md"));
    }

    #[test]
    fn oversized_and_binary_files_are_handled() {
        let big = vec![b'a'; 300];
        let bytes = archive(&[
            ("big.txt", &big),
            ("bin.dat", b"\xff\xfeok"),
            ("ok.txt", b"fine"),
        ]);
        let files = unpack(&bytes[..], &wanted(&["big.txt", "bin.dat", "ok.txt"]), 200).unwrap();
        assert_eq!(files.len(), 2, "the oversized file is dropped");
        assert_eq!(files["ok.txt"], "fine");
        assert!(
            files["bin.dat"].ends_with("ok"),
            "binary bytes decode lossily"
        );
    }

    #[test]
    fn garbage_is_an_error_and_root_stripping_is_strict() {
        assert!(unpack(&b"not a tarball"[..], &wanted(&["x"]), 10).is_err());
        assert_eq!(
            strip_root("owner-repo-sha/a/b.rs").as_deref(),
            Some("a/b.rs")
        );
        assert_eq!(strip_root("./owner-repo-sha/a.rs").as_deref(), Some("a.rs"));
        assert_eq!(strip_root("owner-repo-sha/"), None);
        assert_eq!(strip_root("pax_global_header"), None);
    }
}

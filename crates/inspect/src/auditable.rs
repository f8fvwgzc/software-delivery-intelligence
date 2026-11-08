//! Rust dependency lists embedded by `cargo auditable`: a zlib-compressed
//! JSON document in the `.dep-v0` section naming every crate the binary was
//! built from, with its version and source. A port of REA's
//! `src/rust/RustAuditableReader.ts` (`morluto/rea`, MIT), with its limits.

use std::io::Read;

use flate2::read::ZlibDecoder;
use serde::Deserialize;

use crate::elf::Elf;

/// Compressed section limit (REA: 1 MiB).
const MAX_SECTION_BYTES: usize = 1024 * 1024;
/// Inflated JSON limit (REA: 8 MiB, and at most 64× the compressed size).
const MAX_INFLATED_BYTES: u64 = 8 * 1024 * 1024;
const MAX_RATIO: u64 = 64;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Package {
    pub name: String,
    pub version: String,
    /// `crates.io`, `local`, `git`, `registry`, …
    pub source: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub root: bool,
}

#[derive(Debug, Deserialize)]
struct Document {
    packages: Vec<Package>,
}

/// `Ok(None)` when the binary carries no `.dep-v0`.
pub fn read(elf: &Elf<'_>) -> Result<Option<Vec<Package>>, String> {
    let Some(section) = elf.section(".dep-v0") else {
        return Ok(None);
    };
    let bytes = elf
        .section_bytes(section)
        .map_err(|e| format!("dependency section: {e}"))?;
    decode(bytes).map(Some)
}

pub fn decode(bytes: &[u8]) -> Result<Vec<Package>, String> {
    if bytes.is_empty() {
        return Err("dependency section is empty".into());
    }
    if bytes.len() > MAX_SECTION_BYTES {
        return Err("dependency section exceeds the 1 MiB compressed limit".into());
    }
    let limit = MAX_INFLATED_BYTES.min(bytes.len() as u64 * MAX_RATIO);
    let mut json = Vec::new();
    ZlibDecoder::new(bytes)
        .take(limit + 1)
        .read_to_end(&mut json)
        .map_err(|e| format!("dependency section does not inflate: {e}"))?;
    if json.len() as u64 > limit {
        return Err("dependency section inflates past its limit".into());
    }
    let document: Document =
        serde_json::from_slice(&json).map_err(|e| format!("dependency JSON: {e}"))?;
    Ok(document
        .packages
        .into_iter()
        .filter(|p| !p.name.is_empty() && !p.version.is_empty())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{ElfBuilder, auditable_section};

    #[test]
    fn reads_the_crates_a_binary_was_built_from() {
        let json = r#"{"packages":[
            {"name":"api","version":"0.1.0","source":"local","root":true,"dependencies":[1]},
            {"name":"hyper","version":"0.14.10","source":"crates.io","kind":"runtime"}
        ]}"#;
        let bytes = ElfBuilder::new()
            .section(".dep-v0", auditable_section(json))
            .build();
        let packages = read(&Elf::parse(&bytes).unwrap()).unwrap().unwrap();
        assert_eq!(packages.len(), 2);
        assert!(packages[0].root);
        assert_eq!(packages[1].name, "hyper");
        assert_eq!(packages[1].source, "crates.io");
    }

    #[test]
    fn refuses_bombs_and_garbage() {
        assert!(decode(&[]).is_err());
        assert!(decode(b"not zlib").is_err());
        // 64 KiB of spaces compress to almost nothing: over the ratio.
        let bomb = auditable_section(&" ".repeat(MAX_RATIO as usize * 1024));
        assert!(decode(&bomb).unwrap_err().contains("limit"));
        assert_eq!(
            read(&Elf::parse(&ElfBuilder::new().build()).unwrap()),
            Ok(None)
        );
    }
}

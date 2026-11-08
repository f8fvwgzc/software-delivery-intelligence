//! Go build information: the compiler version and the exact module
//! versions a Go binary was linked from, read from the bytes the Go
//! linker embeds (`runtime/debug.BuildInfo`). A port of REA's
//! `src/go/GoBinaryImage.ts`, `GoBuildInfoSearch.ts` and `GoModuleText.ts`
//! (`morluto/rea`, MIT).
//!
//! The header is 32 bytes at a 16-byte aligned address in `.go.buildinfo`
//! (else in a writable, non-executable `PT_LOAD`): the magic
//! `\xff Go buildinf:`, the pointer size, and flags. Flag bit 1 means the
//! two strings follow inline as varint-prefixed bytes (Go 1.18 and later);
//! without it the header holds pointers to two Go string headers (older
//! toolchains). The module string is framed by two 16-byte sentinels.

use crate::elf::{self, Elf};

const MAGIC: &[u8; 14] = b"\xff Go buildinf:";
const MODULE_START: [u8; 16] = [
    0x30, 0x77, 0xaf, 0x0c, 0x92, 0x74, 0x08, 0x02, 0x41, 0xe1, 0xc1, 0x07, 0xe6, 0xd6, 0x18, 0xe6,
];
const MODULE_END: [u8; 16] = [
    0xf9, 0x32, 0x43, 0x31, 0x86, 0x18, 0x20, 0x72, 0x00, 0x82, 0x42, 0x10, 0x41, 0x16, 0xd8, 0xf2,
];
/// REA's limits: build-info strings above 1 MiB together are refused.
const MAX_BUILD_INFO_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Module {
    pub path: String,
    pub version: String,
    pub sum: String,
    /// `=> path version` when a `replace` directive applied.
    pub replacement: Option<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BuildInfo {
    /// `go1.22.5`.
    pub go_version: String,
    pub path: String,
    pub main: Option<Module>,
    pub deps: Vec<Module>,
    pub settings: Vec<(String, String)>,
    /// `pointer` (before Go 1.18) or `inline`.
    pub encoding: &'static str,
    /// Lines that did not parse; absence of a module is not proven when
    /// this is not empty.
    pub unparsed: usize,
}

impl BuildInfo {
    pub fn setting(&self, key: &str) -> Option<&str> {
        self.settings
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

/// `Ok(None)`: no build information (not Go, or stripped of it). `Err`:
/// the header was found and is malformed.
pub fn read(elf: &Elf<'_>) -> Result<Option<BuildInfo>, String> {
    for (offset, size, vaddr) in search_ranges(elf) {
        let Some(header) = find_header(elf.bytes, offset, size, vaddr) else {
            continue;
        };
        return decode(elf, header, offset + size).map(Some);
    }
    Ok(None)
}

/// `.go.buildinfo` when named, else every writable non-executable load
/// segment (REA's fallback for images without section headers).
fn search_ranges(elf: &Elf<'_>) -> Vec<(usize, usize, u64)> {
    if let Some(section) = elf.section(".go.buildinfo")
        && section.kind != elf::SHT_NOBITS
        && section.flags & elf::SHF_COMPRESSED == 0
    {
        // Inline strings may run past the section into the rest of its
        // segment: search the section, read up to the segment's end.
        let end = elf
            .programs
            .iter()
            .filter(|p| p.kind == elf::PT_LOAD)
            .find(|p| section.offset >= p.offset && section.offset < p.offset + p.filesz)
            .map_or(section.offset + section.size, |p| p.offset + p.filesz);
        return vec![(
            section.offset as usize,
            (end.saturating_sub(section.offset)) as usize,
            section.addr,
        )];
    }
    elf.programs
        .iter()
        .filter(|p| p.kind == elf::PT_LOAD && p.flags & elf::PF_W != 0 && p.flags & elf::PF_X == 0)
        .map(|p| (p.offset as usize, p.filesz as usize, p.vaddr))
        .collect()
}

fn find_header(bytes: &[u8], offset: usize, size: usize, vaddr: u64) -> Option<usize> {
    let end = offset.checked_add(size)?.min(bytes.len());
    let window = bytes.get(offset..end)?;
    let mut from = 0;
    while let Some(found) = find(&window[from..], MAGIC) {
        let at = from + found;
        // The header sits at a 16-byte aligned *address*.
        if (vaddr + at as u64).is_multiple_of(16) && at + 32 <= window.len() {
            return Some(offset + at);
        }
        from = at + 1;
    }
    None
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn decode(elf: &Elf<'_>, header: usize, limit: usize) -> Result<BuildInfo, String> {
    let bytes = elf.bytes;
    let ptr_size = bytes[header + 14];
    let flags = bytes[header + 15];
    if flags & !3 != 0 {
        return Err("Go build-info flags contain an unsupported bit".into());
    }
    let (version, module, encoding) = if flags & 2 != 0 {
        let limit = limit.min(bytes.len());
        let (version, next) = inline_string(bytes, header + 32, limit)?;
        let (module, _) = inline_string(bytes, next, limit)?;
        (version, module, "inline")
    } else {
        if ptr_size != 4 && ptr_size != 8 {
            return Err("Go build-info pointer width must be 4 or 8".into());
        }
        let width = u64::from(ptr_size);
        let read_ptr = |at: usize| -> Result<u64, String> {
            let raw = bytes
                .get(at..at + width as usize)
                .ok_or("Go build-info header is truncated")?;
            Ok(word(raw, elf.little))
        };
        let version = pointer_string(elf, read_ptr(header + 16)?, width)?;
        let module = pointer_string(elf, read_ptr(header + 16 + width as usize)?, width)?;
        (version, module, "pointer")
    };
    if version.len() + module.len() > MAX_BUILD_INFO_BYTES {
        return Err("Go build-info strings exceed 1 MiB".into());
    }
    if version.is_empty() {
        return Err("Go build-info compiler version is empty".into());
    }
    let body = if module.is_empty() {
        module
    } else if module.len() >= 33
        && module[..16] == MODULE_START
        && module[module.len() - 16..] == MODULE_END
    {
        &module[16..module.len() - 16]
    } else {
        return Err("Go module string framing is invalid".into());
    };
    let mut info = parse_modules(&String::from_utf8_lossy(body));
    info.go_version = String::from_utf8_lossy(version).into_owned();
    info.encoding = encoding;
    Ok(info)
}

fn word(raw: &[u8], little: bool) -> u64 {
    let mut buf = [0u8; 8];
    if little {
        buf[..raw.len()].copy_from_slice(raw);
        u64::from_le_bytes(buf)
    } else {
        buf[8 - raw.len()..].copy_from_slice(raw);
        u64::from_be_bytes(buf)
    }
}

/// Go's `binary.Uvarint` length, then that many bytes.
fn inline_string(bytes: &[u8], at: usize, limit: usize) -> Result<(&[u8], usize), String> {
    let mut value: u64 = 0;
    let mut shift = 0u32;
    let mut cursor = at;
    loop {
        let byte = *bytes
            .get(cursor)
            .filter(|_| cursor < limit)
            .ok_or("Go build-info string length is truncated")?;
        cursor += 1;
        if shift >= 64 {
            return Err("Go build-info string length overflows".into());
        }
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
    }
    let len = usize::try_from(value).map_err(|_| "Go build-info string is too long")?;
    if len > MAX_BUILD_INFO_BYTES {
        return Err("Go build-info string exceeds 1 MiB".into());
    }
    let end = cursor
        .checked_add(len)
        .filter(|end| *end <= limit.min(bytes.len()))
        .ok_or("Go build-info string runs past its segment")?;
    Ok((&bytes[cursor..end], end))
}

/// A Go string header (`data`, `len`) at `vaddr`, then its bytes.
fn pointer_string<'a>(elf: &Elf<'a>, vaddr: u64, width: u64) -> Result<&'a [u8], String> {
    let header = elf
        .at_vaddr(vaddr, width * 2)
        .ok_or("Go build-info string header is not mapped from the file")?;
    let data = word(&header[..width as usize], elf.little);
    let len = word(&header[width as usize..], elf.little);
    if len == 0 {
        return Ok(&[]);
    }
    if len as usize > MAX_BUILD_INFO_BYTES {
        return Err("Go build-info string exceeds 1 MiB".into());
    }
    elf.at_vaddr(data, len)
        .ok_or_else(|| "Go build-info string is not mapped from the file".to_owned())
}

/// The module text `go version -m` prints:
///
/// ```text
/// path    github.com/acme/api
/// mod     github.com/acme/api     (devel)
/// dep     golang.org/x/net        v0.17.0 h1:…
/// =>      golang.org/x/net        v0.23.0 h1:…
/// build   vcs.revision=1a2b3c…
/// ```
pub fn parse_modules(text: &str) -> BuildInfo {
    let mut info = BuildInfo::default();
    // Index into `deps` (Some(i)) or the main module (Some(usize::MAX))
    // that a following `=>` line replaces.
    let mut last: Option<usize> = None;
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        if let Some(path) = line.strip_prefix("path\t") {
            if info.path.is_empty() {
                info.path = path.to_owned();
                continue;
            }
        } else if let Some(rest) = line
            .strip_prefix("mod\t")
            .or_else(|| line.strip_prefix("dep\t"))
        {
            last = None;
            if let Some(module) = module_line(rest) {
                if line.starts_with("mod\t") {
                    if info.main.is_none() {
                        info.main = Some(module);
                        last = Some(usize::MAX);
                        continue;
                    }
                } else {
                    info.deps.push(module);
                    last = Some(info.deps.len() - 1);
                    continue;
                }
            }
        } else if let Some(rest) = line.strip_prefix("=>\t") {
            if let (Some(index), Some(module)) = (last.take(), module_line(rest)) {
                let target = if index == usize::MAX {
                    info.main.as_mut()
                } else {
                    info.deps.get_mut(index)
                };
                if let Some(target) = target {
                    // A local replacement (`=> ../fork`) has no version.
                    target.replacement = Some((module.path, module.version));
                    continue;
                }
            }
        } else if let Some((key, value)) = line
            .strip_prefix("build\t")
            .and_then(|setting| setting.split_once('='))
        {
            let key = key.trim_matches('"');
            if !key.is_empty() {
                info.settings
                    .push((key.to_owned(), value.trim_matches('"').to_owned()));
                continue;
            }
        }
        info.unparsed += 1;
    }
    info
}

fn module_line(rest: &str) -> Option<Module> {
    let fields: Vec<&str> = rest.split('\t').collect();
    match fields.as_slice() {
        [path] if !path.is_empty() => Some(Module {
            path: (*path).to_owned(),
            ..Module::default()
        }),
        [path, version] | [path, version, ..] if !path.is_empty() => Some(Module {
            path: (*path).to_owned(),
            version: (*version).to_owned(),
            sum: fields.get(2).map(|s| (*s).to_owned()).unwrap_or_default(),
            replacement: None,
        }),
        _ => None,
    }
}

/// `go1.22.5` → `1.22.5`; `devel …` and other non-release strings → None.
pub fn release_version(go_version: &str) -> Option<String> {
    let v = go_version.trim().strip_prefix("go")?;
    let v = v.split([' ', '-', '+']).next()?;
    let numeric = v
        .split('.')
        .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
    numeric.then(|| v.to_owned())
}

/// The version OSV indexes a Go module under: without the `v`.
pub fn osv_version(version: &str) -> Option<String> {
    let v = version.trim();
    if v.is_empty() || v == "(devel)" {
        return None;
    }
    Some(v.strip_prefix('v').unwrap_or(v).to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{ElfBuilder, go_buildinfo_inline, go_buildinfo_pointer};

    const MODULES: &str = "path\tgithub.com/acme/api\n\
mod\tgithub.com/acme/api\t(devel)\t\n\
dep\tgolang.org/x/net\tv0.17.0\th1:abc=\n\
=>\tgolang.org/x/net\tv0.23.0\th1:def=\n\
dep\tgithub.com/gin-gonic/gin\tv1.9.1\th1:ghi=\n\
build\t-trimpath=true\n\
build\tCGO_ENABLED=0\n\
build\tvcs.revision=1a2b3c4d5e6f\n\
build\tvcs.modified=true\n";

    #[test]
    fn parses_the_module_text_go_version_m_prints() {
        let info = parse_modules(MODULES);
        assert_eq!(info.path, "github.com/acme/api");
        assert_eq!(info.main.as_ref().unwrap().version, "(devel)");
        assert_eq!(info.deps.len(), 2);
        assert_eq!(info.deps[0].path, "golang.org/x/net");
        assert_eq!(info.deps[0].version, "v0.17.0");
        assert_eq!(
            info.deps[0].replacement,
            Some(("golang.org/x/net".to_owned(), "v0.23.0".to_owned()))
        );
        assert_eq!(info.setting("vcs.modified"), Some("true"));
        assert_eq!(info.setting("CGO_ENABLED"), Some("0"));
        assert_eq!(info.unparsed, 0);
    }

    #[test]
    fn reads_inline_build_info_from_go_buildinfo() {
        let bytes = ElfBuilder::new()
            .section(".go.buildinfo", go_buildinfo_inline("go1.22.5", MODULES))
            .build();
        let info = read(&Elf::parse(&bytes).unwrap()).unwrap().unwrap();
        assert_eq!(info.encoding, "inline");
        assert_eq!(info.go_version, "go1.22.5");
        assert_eq!(info.deps[1].path, "github.com/gin-gonic/gin");
    }

    #[test]
    fn reads_pointer_build_info_of_older_toolchains() {
        let bytes = go_buildinfo_pointer("go1.16.15", MODULES);
        let info = read(&Elf::parse(&bytes).unwrap()).unwrap().unwrap();
        assert_eq!(info.encoding, "pointer");
        assert_eq!(info.go_version, "go1.16.15");
        assert_eq!(info.main.unwrap().path, "github.com/acme/api");
    }

    #[test]
    fn a_binary_without_build_info_is_none_and_broken_framing_is_an_error() {
        let plain = ElfBuilder::new().build();
        assert_eq!(read(&Elf::parse(&plain).unwrap()), Ok(None));
        let mut section = go_buildinfo_inline("go1.22.5", MODULES);
        let last = section.len() - 1;
        section[last] ^= 0xff; // corrupt the end sentinel
        let bytes = ElfBuilder::new().section(".go.buildinfo", section).build();
        assert!(read(&Elf::parse(&bytes).unwrap()).is_err());
    }

    #[test]
    fn versions_as_osv_wants_them() {
        assert_eq!(release_version("go1.21.3").as_deref(), Some("1.21.3"));
        assert_eq!(release_version("go1.22rc1"), None);
        assert_eq!(release_version("devel go1.23-abc"), None);
        assert_eq!(osv_version("v0.17.0").as_deref(), Some("0.17.0"));
        assert_eq!(osv_version("(devel)"), None);
    }
}

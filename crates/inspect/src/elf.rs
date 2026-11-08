//! A bounded ELF reader: the header, program headers, sections, the dynamic
//! table, symbol names and notes. Every offset is checked against the file
//! before it is read, so a truncated or hostile file is an error, never a
//! panic. Follows the layouts REA's native readers use (`morluto/rea`,
//! `src/native/metadata`, MIT).

use thiserror::Error;

pub const MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];

pub const ET_EXEC: u16 = 2;
pub const ET_DYN: u16 = 3;

pub const PT_LOAD: u32 = 1;
pub const PT_DYNAMIC: u32 = 2;
pub const PT_INTERP: u32 = 3;
pub const PT_NOTE: u32 = 4;
pub const PT_GNU_STACK: u32 = 0x6474_e551;
pub const PT_GNU_RELRO: u32 = 0x6474_e552;

pub const PF_X: u32 = 1;
pub const PF_W: u32 = 2;

pub const SHT_SYMTAB: u32 = 2;
pub const SHT_NOTE: u32 = 7;
pub const SHT_NOBITS: u32 = 8;
pub const SHT_DYNSYM: u32 = 11;
pub const SHF_COMPRESSED: u64 = 0x800;

pub const DT_NULL: u64 = 0;
pub const DT_NEEDED: u64 = 1;
pub const DT_STRTAB: u64 = 5;
pub const DT_STRSZ: u64 = 10;
pub const DT_RPATH: u64 = 15;
pub const DT_BIND_NOW: u64 = 24;
pub const DT_RUNPATH: u64 = 29;
pub const DT_FLAGS: u64 = 30;
pub const DT_FLAGS_1: u64 = 0x6fff_fffb;
pub const DF_BIND_NOW: u64 = 0x8;
pub const DF_1_NOW: u64 = 0x1;
pub const DF_1_PIE: u64 = 0x0800_0000;

pub const EM_386: u16 = 3;
pub const EM_PPC: u16 = 20;
pub const EM_PPC64: u16 = 21;
pub const EM_S390: u16 = 22;
pub const EM_ARM: u16 = 40;
pub const EM_X86_64: u16 = 62;
pub const EM_AARCH64: u16 = 183;
pub const EM_RISCV: u16 = 243;

/// Most entries of one table read; a larger table is malformed for our
/// purposes (REA's structural budget is the same order).
const MAX_TABLE_ENTRIES: usize = 65_536;
/// Bytes of symbol tables scanned for names.
const MAX_SYMBOL_SCAN_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ElfError {
    #[error("not an ELF file")]
    NotElf,
    #[error("malformed ELF: {0}")]
    Malformed(&'static str),
}

type Result<T> = std::result::Result<T, ElfError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    pub kind: u32,
    pub flags: u32,
    pub offset: u64,
    pub vaddr: u64,
    pub filesz: u64,
    pub memsz: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub name: String,
    pub kind: u32,
    pub flags: u64,
    pub addr: u64,
    pub offset: u64,
    pub size: u64,
    pub link: u32,
    pub entsize: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Dynamic {
    pub entries: Vec<(u64, u64)>,
    pub needed: Vec<String>,
    pub rpath: Vec<String>,
    pub runpath: Vec<String>,
}

impl Dynamic {
    pub fn value(&self, tag: u64) -> Option<u64> {
        self.entries
            .iter()
            .find(|(t, _)| *t == tag)
            .map(|(_, v)| *v)
    }
}

/// A parsed ELF image over borrowed bytes.
pub struct Elf<'a> {
    pub bytes: &'a [u8],
    pub bits: u8,
    pub little: bool,
    pub kind: u16,
    pub machine: u16,
    pub programs: Vec<Program>,
    pub sections: Vec<Section>,
}

/// Whether `bytes` start like an ELF file.
pub fn is_elf(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && bytes[..4] == MAGIC
}

impl<'a> Elf<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        if !is_elf(bytes) {
            return Err(ElfError::NotElf);
        }
        if bytes.len() < 52 {
            return Err(ElfError::Malformed("header is truncated"));
        }
        let bits = match bytes[4] {
            1 => 32,
            2 => 64,
            _ => return Err(ElfError::Malformed("unknown class")),
        };
        let little = match bytes[5] {
            1 => true,
            2 => false,
            _ => return Err(ElfError::Malformed("unknown byte order")),
        };
        if bits == 64 && bytes.len() < 64 {
            return Err(ElfError::Malformed("header is truncated"));
        }
        let mut elf = Elf {
            bytes,
            bits,
            little,
            kind: 0,
            machine: 0,
            programs: Vec::new(),
            sections: Vec::new(),
        };
        elf.kind = elf.u16(16)?;
        elf.machine = elf.u16(18)?;
        let (phoff, shoff, phentsize, phnum, shentsize, shnum, shstrndx) = if bits == 64 {
            (
                elf.u64(32)?,
                elf.u64(40)?,
                elf.u16(54)?,
                elf.u16(56)?,
                elf.u16(58)?,
                elf.u16(60)?,
                elf.u16(62)?,
            )
        } else {
            (
                u64::from(elf.u32(28)?),
                u64::from(elf.u32(32)?),
                elf.u16(42)?,
                elf.u16(44)?,
                elf.u16(46)?,
                elf.u16(48)?,
                elf.u16(50)?,
            )
        };
        elf.programs = elf.read_programs(phoff, phentsize, phnum)?;
        elf.sections = elf.read_sections(shoff, shentsize, shnum, shstrndx)?;
        Ok(elf)
    }

    fn range(&self, offset: u64, len: u64) -> Result<&'a [u8]> {
        let start = usize::try_from(offset).map_err(|_| ElfError::Malformed("offset"))?;
        let len = usize::try_from(len).map_err(|_| ElfError::Malformed("length"))?;
        let end = start
            .checked_add(len)
            .ok_or(ElfError::Malformed("range overflows"))?;
        self.bytes
            .get(start..end)
            .ok_or(ElfError::Malformed("range is outside the file"))
    }

    pub fn u16(&self, at: usize) -> Result<u16> {
        let b = self
            .bytes
            .get(at..at + 2)
            .ok_or(ElfError::Malformed("truncated"))?;
        let a = [b[0], b[1]];
        Ok(if self.little {
            u16::from_le_bytes(a)
        } else {
            u16::from_be_bytes(a)
        })
    }

    pub fn u32(&self, at: usize) -> Result<u32> {
        let b = self
            .bytes
            .get(at..at + 4)
            .ok_or(ElfError::Malformed("truncated"))?;
        let a = [b[0], b[1], b[2], b[3]];
        Ok(if self.little {
            u32::from_le_bytes(a)
        } else {
            u32::from_be_bytes(a)
        })
    }

    pub fn u64(&self, at: usize) -> Result<u64> {
        let b = self
            .bytes
            .get(at..at + 8)
            .ok_or(ElfError::Malformed("truncated"))?;
        let mut a = [0u8; 8];
        a.copy_from_slice(b);
        Ok(if self.little {
            u64::from_le_bytes(a)
        } else {
            u64::from_be_bytes(a)
        })
    }

    /// A pointer-sized word.
    pub fn word(&self, at: usize) -> Result<u64> {
        if self.bits == 64 {
            self.u64(at)
        } else {
            self.u32(at).map(u64::from)
        }
    }

    fn read_programs(&self, phoff: u64, entsize: u16, count: u16) -> Result<Vec<Program>> {
        let want = if self.bits == 64 { 56 } else { 32 };
        if count == 0 {
            return Ok(Vec::new());
        }
        if usize::from(entsize) < want {
            return Err(ElfError::Malformed("program header entry is too small"));
        }
        self.range(phoff, u64::from(entsize) * u64::from(count))?;
        // In range: checked just above.
        let base = phoff as usize;
        let mut out = Vec::with_capacity(usize::from(count));
        for i in 0..usize::from(count) {
            let at = base + i * usize::from(entsize);
            let program = if self.bits == 64 {
                Program {
                    kind: self.u32(at)?,
                    flags: self.u32(at + 4)?,
                    offset: self.u64(at + 8)?,
                    vaddr: self.u64(at + 16)?,
                    filesz: self.u64(at + 32)?,
                    memsz: self.u64(at + 40)?,
                }
            } else {
                Program {
                    kind: self.u32(at)?,
                    offset: u64::from(self.u32(at + 4)?),
                    vaddr: u64::from(self.u32(at + 8)?),
                    filesz: u64::from(self.u32(at + 16)?),
                    memsz: u64::from(self.u32(at + 20)?),
                    flags: self.u32(at + 24)?,
                }
            };
            out.push(program);
        }
        Ok(out)
    }

    fn read_sections(
        &self,
        shoff: u64,
        entsize: u16,
        count: u16,
        shstrndx: u16,
    ) -> Result<Vec<Section>> {
        let want = if self.bits == 64 { 64 } else { 40 };
        if count == 0 || shoff == 0 {
            // Stripped of its section table (`sstrip`): legal, nothing to read.
            return Ok(Vec::new());
        }
        if usize::from(entsize) < want {
            return Err(ElfError::Malformed("section header entry is too small"));
        }
        self.range(shoff, u64::from(entsize) * u64::from(count))?;
        let base = shoff as usize;
        let mut raw = Vec::with_capacity(usize::from(count));
        for i in 0..usize::from(count) {
            let at = base + i * usize::from(entsize);
            let (name, kind, flags, addr, offset, size, link, entsize) = if self.bits == 64 {
                (
                    self.u32(at)?,
                    self.u32(at + 4)?,
                    self.u64(at + 8)?,
                    self.u64(at + 16)?,
                    self.u64(at + 24)?,
                    self.u64(at + 32)?,
                    self.u32(at + 40)?,
                    self.u64(at + 56)?,
                )
            } else {
                (
                    self.u32(at)?,
                    self.u32(at + 4)?,
                    u64::from(self.u32(at + 8)?),
                    u64::from(self.u32(at + 12)?),
                    u64::from(self.u32(at + 16)?),
                    u64::from(self.u32(at + 20)?),
                    self.u32(at + 24)?,
                    u64::from(self.u32(at + 36)?),
                )
            };
            raw.push((name, kind, flags, addr, offset, size, link, entsize));
        }
        let names: &[u8] = raw
            .get(usize::from(shstrndx))
            .filter(|s| s.1 != SHT_NOBITS)
            .and_then(|s| self.range(s.4, s.5).ok())
            .unwrap_or(&[]);
        Ok(raw
            .into_iter()
            .map(
                |(name, kind, flags, addr, offset, size, link, entsize)| Section {
                    name: c_string(names, name as usize),
                    kind,
                    flags,
                    addr,
                    offset,
                    size,
                    link,
                    entsize,
                },
            )
            .collect())
    }

    pub fn section(&self, name: &str) -> Option<&Section> {
        self.sections.iter().find(|s| s.name == name)
    }

    /// The file bytes of a section (empty for `NOBITS`).
    pub fn section_bytes(&self, section: &Section) -> Result<&'a [u8]> {
        if section.kind == SHT_NOBITS {
            return Ok(&[]);
        }
        self.range(section.offset, section.size)
    }

    pub fn program(&self, kind: u32) -> Option<&Program> {
        self.programs.iter().find(|p| p.kind == kind)
    }

    /// File offset of `len` bytes at virtual address `vaddr`, through the
    /// `PT_LOAD` that maps them from the file.
    pub fn file_offset(&self, vaddr: u64, len: u64) -> Option<u64> {
        self.programs
            .iter()
            .filter(|p| p.kind == PT_LOAD)
            .find(|p| {
                vaddr >= p.vaddr
                    && vaddr
                        .checked_add(len)
                        .is_some_and(|end| end <= p.vaddr.saturating_add(p.filesz))
            })
            .map(|p| p.offset + (vaddr - p.vaddr))
    }

    /// Bytes at a virtual address.
    pub fn at_vaddr(&self, vaddr: u64, len: u64) -> Option<&'a [u8]> {
        let offset = self.file_offset(vaddr, len)?;
        self.range(offset, len).ok()
    }

    /// The dynamic table with its string-valued entries resolved.
    pub fn dynamic(&self) -> Result<Option<Dynamic>> {
        let Some(program) = self.program(PT_DYNAMIC) else {
            return Ok(None);
        };
        let table = self.range(program.offset, program.filesz)?;
        let width = if self.bits == 64 { 16 } else { 8 };
        let base = program.offset as usize;
        let mut dynamic = Dynamic::default();
        for i in 0..(table.len() / width).min(MAX_TABLE_ENTRIES) {
            let at = base + i * width;
            let (tag, value) = if self.bits == 64 {
                (self.u64(at)?, self.u64(at + 8)?)
            } else {
                (u64::from(self.u32(at)?), u64::from(self.u32(at + 4)?))
            };
            if tag == DT_NULL {
                break;
            }
            dynamic.entries.push((tag, value));
        }
        // DT_STRTAB is a virtual address; `.dynstr` is the same bytes.
        let strtab = dynamic.value(DT_STRTAB).and_then(|vaddr| {
            let size = dynamic.value(DT_STRSZ).unwrap_or(0).min(16 * 1024 * 1024);
            self.at_vaddr(vaddr, size)
        });
        let strtab = strtab.or_else(|| {
            self.section(".dynstr")
                .and_then(|s| self.section_bytes(s).ok())
        });
        if let Some(strings) = strtab {
            for (tag, value) in &dynamic.entries {
                let text = c_string(strings, *value as usize);
                match *tag {
                    DT_NEEDED => dynamic.needed.push(text),
                    DT_RPATH => dynamic.rpath.push(text),
                    DT_RUNPATH => dynamic.runpath.push(text),
                    _ => {}
                }
            }
        }
        Ok(Some(dynamic))
    }

    /// Names in `.symtab` and `.dynsym`, bounded.
    pub fn symbol_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        let mut scanned = 0usize;
        let width = if self.bits == 64 { 24 } else { 16 };
        for section in self
            .sections
            .iter()
            .filter(|s| s.kind == SHT_SYMTAB || s.kind == SHT_DYNSYM)
        {
            let Ok(table) = self.section_bytes(section) else {
                continue;
            };
            let Some(strings) = self
                .sections
                .get(section.link as usize)
                .and_then(|s| self.section_bytes(s).ok())
            else {
                continue;
            };
            if scanned + table.len() > MAX_SYMBOL_SCAN_BYTES {
                break;
            }
            scanned += table.len();
            for entry in table.chunks_exact(width).take(MAX_TABLE_ENTRIES) {
                let raw = [entry[0], entry[1], entry[2], entry[3]];
                let name = if self.little {
                    u32::from_le_bytes(raw)
                } else {
                    u32::from_be_bytes(raw)
                };
                if name != 0 {
                    let text = c_string(strings, name as usize);
                    if !text.is_empty() {
                        names.push(text);
                    }
                }
            }
        }
        names
    }

    /// `(name, type, desc)` of every note in `SHT_NOTE` sections, else in
    /// `PT_NOTE` segments.
    pub fn notes(&self) -> Vec<(String, u32, Vec<u8>)> {
        let mut blobs: Vec<&[u8]> = self
            .sections
            .iter()
            .filter(|s| s.kind == SHT_NOTE)
            .filter_map(|s| self.section_bytes(s).ok())
            .collect();
        if blobs.is_empty() {
            blobs = self
                .programs
                .iter()
                .filter(|p| p.kind == PT_NOTE)
                .filter_map(|p| self.range(p.offset, p.filesz).ok())
                .collect();
        }
        let mut out = Vec::new();
        for blob in blobs {
            let mut at = 0usize;
            while at + 12 <= blob.len() && out.len() < 256 {
                let read = |o: usize| {
                    let b = [blob[o], blob[o + 1], blob[o + 2], blob[o + 3]];
                    if self.little {
                        u32::from_le_bytes(b)
                    } else {
                        u32::from_be_bytes(b)
                    }
                };
                let namesz = read(at) as usize;
                let descsz = read(at + 4) as usize;
                let kind = read(at + 8);
                let name_start = at + 12;
                let desc_start = name_start + align4(namesz);
                let next = desc_start + align4(descsz);
                if next > blob.len() || desc_start + descsz > blob.len() {
                    break;
                }
                let name = c_string(&blob[name_start..name_start + namesz], 0);
                out.push((name, kind, blob[desc_start..desc_start + descsz].to_vec()));
                at = next;
            }
        }
        out
    }

    pub fn machine_name(&self) -> &'static str {
        match self.machine {
            EM_386 => "i386",
            EM_PPC => "ppc",
            EM_PPC64 => "ppc64",
            EM_S390 => "s390",
            EM_ARM => "arm",
            EM_X86_64 => "x86_64",
            EM_AARCH64 => "aarch64",
            EM_RISCV => "riscv",
            _ => "unknown",
        }
    }
}

fn align4(n: usize) -> usize {
    (n + 3) & !3
}

/// The NUL-terminated string at `at`, lossily decoded.
pub fn c_string(bytes: &[u8], at: usize) -> String {
    let Some(rest) = bytes.get(at..) else {
        return String::new();
    };
    let end = rest.iter().position(|b| *b == 0).unwrap_or(rest.len());
    String::from_utf8_lossy(&rest[..end.min(4096)]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::ElfBuilder;

    #[test]
    fn reads_header_programs_sections_dynamic_and_symbols() {
        let bytes = ElfBuilder::new()
            .pie()
            .relro()
            .bind_now()
            .stack(false)
            .runpath("$ORIGIN/../lib")
            .needed("libc.so.6")
            .symbols(&["main", "__stack_chk_fail", "__memcpy_chk"])
            .section(
                ".note.gnu.build-id",
                crate::testing::build_id_note(&[0xab, 0xcd, 0xef]),
            )
            .build();
        let elf = Elf::parse(&bytes).unwrap();
        assert_eq!(elf.bits, 64);
        assert!(elf.little);
        assert_eq!(elf.kind, ET_DYN);
        assert_eq!(elf.machine_name(), "x86_64");
        assert!(elf.program(PT_INTERP).is_some());
        assert!(elf.program(PT_GNU_RELRO).is_some());
        let dynamic = elf.dynamic().unwrap().unwrap();
        assert_eq!(dynamic.needed, ["libc.so.6"]);
        assert_eq!(dynamic.runpath, ["$ORIGIN/../lib"]);
        assert_eq!(dynamic.value(DT_FLAGS_1), Some(DF_1_NOW | DF_1_PIE));
        let names = elf.symbol_names();
        assert!(names.contains(&"__stack_chk_fail".to_owned()));
        let notes = elf.notes();
        assert_eq!(notes[0].0, "GNU");
        assert_eq!(notes[0].1, 3);
        assert_eq!(notes[0].2, vec![0xab, 0xcd, 0xef]);
    }

    #[test]
    fn hostile_input_is_an_error_never_a_panic() {
        assert_eq!(Elf::parse(b"MZ\x90\x00").err(), Some(ElfError::NotElf));
        assert!(Elf::parse(&MAGIC).is_err());
        let mut bytes = ElfBuilder::new().build();
        // Program headers claimed far outside the file.
        bytes[32..40].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(Elf::parse(&bytes).is_err());
        // Every prefix of a valid file parses or fails cleanly.
        let valid = ElfBuilder::new().pie().symbols(&["x"]).build();
        for cut in 0..valid.len() {
            let _ = Elf::parse(&valid[..cut])
                .map(|elf| (elf.dynamic(), elf.symbol_names(), elf.notes()));
        }
    }
}

//! Builders for the tests: small, valid ELF files with exactly the
//! headers, segments and sections a test asks for, Go build-info blobs in
//! both encodings, cargo-auditable sections and image layers. Every file
//! is laid out with `vaddr == file offset` in one `PT_LOAD`.

#![cfg(test)]

use std::io::Write;

use flate2::{Compression, write::ZlibEncoder};

use crate::elf;

const MODULE_START: [u8; 16] = [
    0x30, 0x77, 0xaf, 0x0c, 0x92, 0x74, 0x08, 0x02, 0x41, 0xe1, 0xc1, 0x07, 0xe6, 0xd6, 0x18, 0xe6,
];
const MODULE_END: [u8; 16] = [
    0xf9, 0x32, 0x43, 0x31, 0x86, 0x18, 0x20, 0x72, 0x00, 0x82, 0x42, 0x10, 0x41, 0x16, 0xd8, 0xf2,
];

#[derive(Default, Clone)]
pub struct ElfBuilder {
    kind: u16,
    machine: u16,
    interp: bool,
    relro: bool,
    bind_now: bool,
    flags_1: u64,
    stack: Option<bool>,
    needed: Vec<String>,
    rpath: Option<String>,
    runpath: Option<String>,
    symbols: Vec<String>,
    sections: Vec<(String, Vec<u8>)>,
}

struct Placed {
    name: String,
    kind: u32,
    offset: usize,
    size: usize,
    link: u32,
    entsize: u64,
}

#[allow(clippy::too_many_arguments)]
fn place(
    file: &mut Vec<u8>,
    placed: &mut Vec<Placed>,
    name: &str,
    kind: u32,
    bytes: &[u8],
    link: u32,
    entsize: u64,
) -> usize {
    while !file.len().is_multiple_of(16) {
        file.push(0);
    }
    let offset = file.len();
    file.extend_from_slice(bytes);
    placed.push(Placed {
        name: name.to_owned(),
        kind,
        offset,
        size: bytes.len(),
        link,
        entsize,
    });
    offset
}

fn put16(buf: &mut [u8], at: usize, v: u16) {
    buf[at..at + 2].copy_from_slice(&v.to_le_bytes());
}
fn put32(buf: &mut [u8], at: usize, v: u32) {
    buf[at..at + 4].copy_from_slice(&v.to_le_bytes());
}
fn put64(buf: &mut [u8], at: usize, v: u64) {
    buf[at..at + 8].copy_from_slice(&v.to_le_bytes());
}

impl ElfBuilder {
    pub fn new() -> Self {
        Self {
            kind: elf::ET_EXEC,
            machine: elf::EM_X86_64,
            ..Self::default()
        }
    }
    pub fn pie(mut self) -> Self {
        self.kind = elf::ET_DYN;
        self.interp = true;
        self.flags_1 |= elf::DF_1_PIE;
        self
    }
    pub fn shared_object(mut self) -> Self {
        self.kind = elf::ET_DYN;
        self.interp = false;
        self
    }
    pub fn machine(mut self, machine: u16) -> Self {
        self.machine = machine;
        self
    }
    pub fn relro(mut self) -> Self {
        self.relro = true;
        self
    }
    pub fn bind_now(mut self) -> Self {
        self.bind_now = true;
        self.flags_1 |= elf::DF_1_NOW;
        self
    }
    pub fn flags_1(mut self, flags: u64) -> Self {
        self.flags_1 |= flags;
        self
    }
    /// `Some(true)`: an executable `PT_GNU_STACK`.
    pub fn stack(mut self, executable: bool) -> Self {
        self.stack = Some(executable);
        self
    }
    pub fn needed(mut self, lib: &str) -> Self {
        self.needed.push(lib.to_owned());
        self
    }
    pub fn rpath(mut self, path: &str) -> Self {
        self.rpath = Some(path.to_owned());
        self
    }
    pub fn runpath(mut self, path: &str) -> Self {
        self.runpath = Some(path.to_owned());
        self
    }
    pub fn symbols(mut self, names: &[&str]) -> Self {
        self.symbols.extend(names.iter().map(|s| (*s).to_owned()));
        self
    }
    pub fn debug_info(self) -> Self {
        self.section(".debug_info", vec![0; 16])
    }
    pub fn section(mut self, name: &str, bytes: Vec<u8>) -> Self {
        self.sections.push((name.to_owned(), bytes));
        self
    }

    fn dynamic_wanted(&self) -> bool {
        !self.needed.is_empty()
            || self.rpath.is_some()
            || self.runpath.is_some()
            || self.flags_1 != 0
            || self.bind_now
    }

    pub fn build(&self) -> Vec<u8> {
        let mut file = vec![0u8; 64];
        let align = |file: &mut Vec<u8>| {
            while !file.len().is_multiple_of(16) {
                file.push(0);
            }
        };
        let mut placed: Vec<Placed> = Vec::new();
        // Index 0 is the null section; sections are numbered in placement order + 1.
        let mut interp_at = None;
        if self.interp {
            let path = b"/lib64/ld-linux-x86-64.so.2\0";
            interp_at = Some((
                place(&mut file, &mut placed, ".interp", 1, path, 0, 0),
                path.len(),
            ));
        }
        let mut dynamic_at = None;
        if self.dynamic_wanted() {
            let mut dynstr = vec![0u8];
            let mut add = |s: &str| {
                let at = dynstr.len() as u64;
                dynstr.extend_from_slice(s.as_bytes());
                dynstr.push(0);
                at
            };
            let mut entries: Vec<(u64, u64)> = Vec::new();
            for lib in &self.needed {
                entries.push((elf::DT_NEEDED, add(lib)));
            }
            if let Some(p) = &self.rpath {
                entries.push((elf::DT_RPATH, add(p)));
            }
            if let Some(p) = &self.runpath {
                entries.push((elf::DT_RUNPATH, add(p)));
            }
            let dynstr_index = placed.len() as u32 + 1;
            let dynstr_at = place(&mut file, &mut placed, ".dynstr", 3, &dynstr, 0, 0);
            entries.push((elf::DT_STRTAB, dynstr_at as u64));
            entries.push((elf::DT_STRSZ, dynstr.len() as u64));
            if self.bind_now {
                entries.push((elf::DT_BIND_NOW, 0));
            }
            if self.flags_1 != 0 {
                entries.push((elf::DT_FLAGS_1, self.flags_1));
            }
            entries.push((elf::DT_NULL, 0));
            let mut table = Vec::new();
            for (tag, value) in entries {
                table.extend_from_slice(&tag.to_le_bytes());
                table.extend_from_slice(&value.to_le_bytes());
            }
            dynamic_at = Some((
                place(
                    &mut file,
                    &mut placed,
                    ".dynamic",
                    6,
                    &table,
                    dynstr_index,
                    16,
                ),
                table.len(),
            ));
        }
        if !self.symbols.is_empty() {
            let mut strtab = vec![0u8];
            let mut symtab = vec![0u8; 24]; // the null symbol
            for name in &self.symbols {
                let at = strtab.len() as u32;
                strtab.extend_from_slice(name.as_bytes());
                strtab.push(0);
                let mut sym = [0u8; 24];
                put32(&mut sym, 0, at);
                sym[4] = 0x12; // GLOBAL FUNC
                symtab.extend_from_slice(&sym);
            }
            let strtab_index = placed.len() as u32 + 1;
            place(&mut file, &mut placed, ".strtab", 3, &strtab, 0, 0);
            place(
                &mut file,
                &mut placed,
                ".symtab",
                elf::SHT_SYMTAB,
                &symtab,
                strtab_index,
                24,
            );
        }
        for (name, bytes) in &self.sections {
            let kind = if name.starts_with(".note") {
                elf::SHT_NOTE
            } else {
                1
            };
            place(&mut file, &mut placed, name, kind, bytes, 0, 0);
        }
        // Section names.
        let mut shstrtab = vec![0u8];
        let mut name_at = Vec::new();
        for p in placed
            .iter()
            .map(|p| p.name.clone())
            .chain([".shstrtab".to_owned()])
        {
            name_at.push(shstrtab.len() as u32);
            shstrtab.extend_from_slice(p.as_bytes());
            shstrtab.push(0);
        }
        place(&mut file, &mut placed, ".shstrtab", 3, &shstrtab, 0, 0);

        // Program headers.
        let mut programs: Vec<(u32, u32, u64, u64)> = Vec::new(); // type, flags, offset, size
        if let Some((at, len)) = interp_at {
            programs.push((elf::PT_INTERP, 4, at as u64, len as u64));
        }
        if let Some((at, len)) = dynamic_at {
            programs.push((elf::PT_DYNAMIC, 6, at as u64, len as u64));
        }
        if let Some(executable) = self.stack {
            programs.push((elf::PT_GNU_STACK, if executable { 7 } else { 6 }, 0, 0));
        }
        if self.relro {
            programs.push((elf::PT_GNU_RELRO, 4, 0, 0));
        }
        align(&mut file);
        let phoff = file.len();
        let phnum = programs.len() + 1;
        file.resize(phoff + phnum * 56, 0);
        align(&mut file);
        let shoff = file.len();
        let shnum = placed.len() + 1;
        file.resize(shoff + shnum * 64, 0);
        let total = file.len() as u64;
        // PT_LOAD maps the whole file at vaddr == offset.
        let mut all = vec![(elf::PT_LOAD, 5u32, 0u64, total)];
        all.extend(programs);
        for (i, (kind, flags, offset, size)) in all.into_iter().enumerate() {
            let at = phoff + i * 56;
            put32(&mut file, at, kind);
            put32(&mut file, at + 4, flags);
            put64(&mut file, at + 8, offset);
            put64(&mut file, at + 16, offset);
            put64(&mut file, at + 24, offset);
            put64(&mut file, at + 32, size);
            put64(&mut file, at + 40, size);
            put64(&mut file, at + 48, 16);
        }
        for (i, p) in placed.iter().enumerate() {
            let at = shoff + (i + 1) * 64;
            put32(&mut file, at, name_at[i]);
            put32(&mut file, at + 4, p.kind);
            put64(&mut file, at + 16, p.offset as u64);
            put64(&mut file, at + 24, p.offset as u64);
            put64(&mut file, at + 32, p.size as u64);
            put32(&mut file, at + 40, p.link);
            put64(&mut file, at + 48, 16);
            put64(&mut file, at + 56, p.entsize);
        }
        // ELF header.
        file[..4].copy_from_slice(&elf::MAGIC);
        file[4] = 2; // 64-bit
        file[5] = 1; // little endian
        file[6] = 1;
        put16(&mut file, 16, self.kind);
        put16(&mut file, 18, self.machine);
        put32(&mut file, 20, 1);
        put64(&mut file, 32, phoff as u64);
        put64(&mut file, 40, shoff as u64);
        put16(&mut file, 52, 64);
        put16(&mut file, 54, 56);
        put16(&mut file, 56, phnum as u16);
        put16(&mut file, 58, 64);
        put16(&mut file, 60, shnum as u16);
        put16(&mut file, 62, placed.len() as u16); // .shstrtab is the last placed
        file
    }
}

fn uvarint(mut n: usize, out: &mut Vec<u8>) {
    loop {
        let byte = (n & 0x7f) as u8;
        n >>= 7;
        if n == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn framed(modules: &str) -> Vec<u8> {
    let mut out = MODULE_START.to_vec();
    out.extend_from_slice(modules.as_bytes());
    if !modules.ends_with('\n') {
        out.push(b'\n');
    }
    out.extend_from_slice(&MODULE_END);
    out
}

fn header(flags: u8) -> Vec<u8> {
    let mut out = b"\xff Go buildinf:".to_vec();
    out.push(8);
    out.push(flags);
    out.resize(32, 0);
    out
}

/// A `.go.buildinfo` section in the Go 1.18+ inline encoding.
pub fn go_buildinfo_inline(version: &str, modules: &str) -> Vec<u8> {
    let mut out = header(2);
    uvarint(version.len(), &mut out);
    out.extend_from_slice(version.as_bytes());
    let module = framed(modules);
    uvarint(module.len(), &mut out);
    out.extend_from_slice(&module);
    out
}

/// A whole ELF whose `.go.buildinfo` uses the pre-1.18 pointer encoding.
pub fn go_buildinfo_pointer(version: &str, modules: &str) -> Vec<u8> {
    let module = framed(modules);
    let size = 64 + version.len() + module.len();
    let mut bytes = ElfBuilder::new()
        .section(".go.buildinfo", vec![0; size])
        .build();
    let section = {
        let elf = elf::Elf::parse(&bytes).unwrap();
        elf.section(".go.buildinfo").unwrap().offset as usize
    };
    let s = section;
    bytes[s..s + 32].copy_from_slice(&header(0));
    let version_header = (s + 32) as u64;
    let module_header = (s + 48) as u64;
    let version_data = (s + 64) as u64;
    let module_data = version_data + version.len() as u64;
    put64(&mut bytes, s + 16, version_header);
    put64(&mut bytes, s + 24, module_header);
    put64(&mut bytes, s + 32, version_data);
    put64(&mut bytes, s + 40, version.len() as u64);
    put64(&mut bytes, s + 48, module_data);
    put64(&mut bytes, s + 56, module.len() as u64);
    bytes[s + 64..s + 64 + version.len()].copy_from_slice(version.as_bytes());
    let m = s + 64 + version.len();
    bytes[m..m + module.len()].copy_from_slice(&module);
    bytes
}

/// cargo-auditable's `.dep-v0`: the JSON, zlib-compressed.
pub fn auditable_section(json: &str) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(json.as_bytes()).unwrap();
    encoder.finish().unwrap()
}

/// A GNU build-id note.
pub fn build_id_note(desc: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&4u32.to_le_bytes());
    out.extend_from_slice(&(desc.len() as u32).to_le_bytes());
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(b"GNU\0");
    out.extend_from_slice(desc);
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
    out
}

/// An uncompressed layer tar of `(path, mode, content)` regular files.
pub fn tar_layer(entries: &[(&str, u32, Vec<u8>)]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for (path, mode, content) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(*mode);
        header.set_entry_type(tar::EntryType::Regular);
        header.set_cksum();
        builder
            .append_data(&mut header, path, &content[..])
            .unwrap();
    }
    builder.into_inner().unwrap()
}

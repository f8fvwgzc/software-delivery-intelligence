//! ELF hardening, the way REA reads it (`morluto/rea`,
//! `src/native/hardening/ElfHardening.ts`, MIT): each property is a row
//! with a state, how it is known and the bytes it was read from.
//!
//! | property | read from |
//! |---|---|
//! | `pie` | `e_type`, `PT_INTERP`, `DF_1_PIE` |
//! | `nx` | `PT_GNU_STACK` flags, else the machine's documented default |
//! | `relro` | `PT_GNU_RELRO` plus `DT_BIND_NOW` / `DF_BIND_NOW` / `DF_1_NOW` |
//! | `stack_protector` | `__stack_chk_fail` / `__stack_chk_guard` symbols |
//! | `fortify` | `__*_chk` symbols |
//! | `rpath`, `runpath` | `DT_RPATH`, `DT_RUNPATH` |
//! | `stripped` | `.symtab`, `.debug_*` sections |
//!
//! Go and Rust do not use the C stack protector or FORTIFY_SOURCE: those
//! rows are `not_applicable` for them, never `disabled`.

use crate::{
    elf::{self, Elf},
    report::{Confidence, HardeningRow, State},
};

/// Which toolchain produced a binary, as far as its bytes say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Go,
    Rust,
    Native,
}

impl Language {
    pub fn as_str(self) -> &'static str {
        match self {
            Language::Go => "go",
            Language::Rust => "rust",
            Language::Native => "native",
        }
    }
}

/// Go: `.go.buildinfo`, `.gopclntab` or the Go build-id note. Rust:
/// cargo-auditable's `.dep-v0`, `.rustc`, or a Rust runtime symbol.
pub fn language(elf: &Elf<'_>, symbols: &[String]) -> Language {
    let has = |name: &str| elf.section(name).is_some();
    if has(".go.buildinfo") || has(".gopclntab") || has(".note.go.buildid") {
        return Language::Go;
    }
    if has(".dep-v0")
        || has(".rustc")
        || symbols.iter().any(|s| {
            s == "rust_begin_unwind"
                || s.starts_with("_ZN3std")
                || s.starts_with("_RNv")
                || s.contains("rust_panic")
        })
    {
        return Language::Rust;
    }
    Language::Native
}

/// `executable`, `pie_executable`, `static_pie_executable` or
/// `shared_object`.
pub fn kind(elf: &Elf<'_>, flags_1: u64) -> &'static str {
    let interp = elf.program(elf::PT_INTERP).is_some();
    match elf.kind {
        elf::ET_EXEC => "executable",
        elf::ET_DYN if interp => "pie_executable",
        elf::ET_DYN if flags_1 & elf::DF_1_PIE != 0 => "static_pie_executable",
        elf::ET_DYN => "shared_object",
        _ => "other",
    }
}

fn row(
    property: &str,
    state: State,
    confidence: Confidence,
    basis: impl Into<String>,
) -> HardeningRow {
    HardeningRow {
        property: property.to_owned(),
        state,
        confidence,
        basis: basis.into(),
    }
}

pub fn rows(elf: &Elf<'_>, symbols: &[String], language: Language) -> Vec<HardeningRow> {
    use Confidence::{Derived, Observed};
    let dynamic = elf.dynamic().ok().flatten().unwrap_or_default();
    let flags = dynamic.value(elf::DT_FLAGS).unwrap_or(0);
    let flags_1 = dynamic.value(elf::DT_FLAGS_1).unwrap_or(0);
    let kind = kind(elf, flags_1);
    let mut out = Vec::new();

    // PIE: only an executable can lack it; a library is always relocatable.
    out.push(match kind {
        "pie_executable" => row(
            "pie",
            State::Enabled,
            Observed,
            "e_type ET_DYN with PT_INTERP",
        ),
        "static_pie_executable" => row(
            "pie",
            State::Enabled,
            Observed,
            "e_type ET_DYN with DF_1_PIE",
        ),
        "executable" => row(
            "pie",
            State::Disabled,
            Observed,
            "e_type ET_EXEC: loaded at a fixed address",
        ),
        "shared_object" => row(
            "pie",
            State::NotApplicable,
            Observed,
            "shared object: position independent by construction",
        ),
        _ => row(
            "pie",
            State::Unknown,
            Observed,
            format!("e_type {}", elf.kind),
        ),
    });

    // NX: PT_GNU_STACK says; without it Linux falls back per machine.
    out.push(match elf.program(elf::PT_GNU_STACK) {
        Some(p) if p.flags & elf::PF_X != 0 => row(
            "nx",
            State::Disabled,
            Observed,
            "PT_GNU_STACK is executable (RWE): the stack can run code",
        ),
        Some(_) => row("nx", State::Enabled, Observed, "PT_GNU_STACK without PF_X"),
        None => match elf.machine {
            elf::EM_386 | elf::EM_PPC | elf::EM_ARM => row(
                "nx",
                State::Disabled,
                Derived,
                format!(
                    "no PT_GNU_STACK: Linux treats {} images as read-implies-exec",
                    elf.machine_name()
                ),
            ),
            elf::EM_X86_64 | elf::EM_AARCH64 | elf::EM_RISCV | elf::EM_PPC64 => row(
                "nx",
                State::Enabled,
                Derived,
                format!(
                    "no PT_GNU_STACK: {} stacks are non-executable by default",
                    elf.machine_name()
                ),
            ),
            _ => row(
                "nx",
                State::Unknown,
                Derived,
                "no PT_GNU_STACK and no known default",
            ),
        },
    });

    // RELRO.
    let bind_now = dynamic.value(elf::DT_BIND_NOW).is_some()
        || flags & elf::DF_BIND_NOW != 0
        || flags_1 & elf::DF_1_NOW != 0;
    let has_dynamic = elf.program(elf::PT_DYNAMIC).is_some();
    out.push(match (elf.program(elf::PT_GNU_RELRO).is_some(), bind_now) {
        (true, true) => row(
            "relro",
            State::Enabled,
            Observed,
            "PT_GNU_RELRO with immediate binding (full RELRO)",
        ),
        (true, false) if !has_dynamic => row(
            "relro",
            State::Enabled,
            Observed,
            "PT_GNU_RELRO in a static image: nothing is bound lazily",
        ),
        (true, false) => row(
            "relro",
            State::Partial,
            Observed,
            "PT_GNU_RELRO without BIND_NOW: the GOT stays writable (partial RELRO)",
        ),
        (false, _) => row(
            "relro",
            State::Disabled,
            Observed,
            "no PT_GNU_RELRO segment",
        ),
    });

    let stripped = elf.section(".symtab").is_none();
    let symbol_basis = if stripped {
        "dynamic symbols (the image is stripped)"
    } else {
        "the symbol table"
    };
    // Stack protector and FORTIFY: C/C++ instrumentation.
    if language == Language::Native {
        let canary = symbols.iter().any(|s| {
            s == "__stack_chk_fail" || s == "__stack_chk_guard" || s == "__intel_security_cookie"
        });
        out.push(if canary {
            row(
                "stack_protector",
                State::Enabled,
                Observed,
                format!("__stack_chk_fail in {symbol_basis}"),
            )
        } else if symbols.is_empty() {
            row(
                "stack_protector",
                State::Unknown,
                Derived,
                "no symbols to read",
            )
        } else {
            row(
                "stack_protector",
                State::Disabled,
                Derived,
                format!("no __stack_chk_fail in {symbol_basis}"),
            )
        });
        let fortified = symbols
            .iter()
            .filter(|s| s.starts_with("__") && s.ends_with("_chk") && *s != "__stack_chk_fail")
            .count();
        out.push(if fortified > 0 {
            row(
                "fortify",
                State::Enabled,
                Observed,
                format!("{fortified} fortified __*_chk calls"),
            )
        } else if symbols.is_empty() {
            row("fortify", State::Unknown, Derived, "no symbols to read")
        } else {
            row("fortify", State::Disabled, Derived, "no __*_chk calls")
        });
    } else {
        let why = format!("{} does not use the C instrumentation", language.as_str());
        out.push(row(
            "stack_protector",
            State::NotApplicable,
            Derived,
            why.clone(),
        ));
        out.push(row("fortify", State::NotApplicable, Derived, why));
    }

    for (property, paths) in [("rpath", &dynamic.rpath), ("runpath", &dynamic.runpath)] {
        out.push(if paths.is_empty() {
            row(
                property,
                State::Disabled,
                Observed,
                format!("no DT_{}", property.to_ascii_uppercase()),
            )
        } else {
            row(
                property,
                State::Enabled,
                Observed,
                format!("DT_{} = {}", property.to_ascii_uppercase(), paths.join(":")),
            )
        });
    }

    let debug = elf.sections.iter().any(|s| s.name.starts_with(".debug_"));
    out.push(if stripped {
        row("stripped", State::Enabled, Observed, "no .symtab")
    } else if debug {
        row(
            "stripped",
            State::Disabled,
            Observed,
            ".symtab and .debug_* sections ship with the image",
        )
    } else {
        row(
            "stripped",
            State::Partial,
            Observed,
            ".symtab ships; no debug sections",
        )
    });
    out
}

/// RPATH/RUNPATH entries an attacker who can write files may control:
/// relative directories (resolved against the working directory) and
/// shared temporary ones. `$ORIGIN` and absolute system paths are fine.
pub fn risky_search_paths(rows: &[HardeningRow]) -> Vec<String> {
    rows.iter()
        .filter(|r| (r.property == "rpath" || r.property == "runpath") && r.state == State::Enabled)
        .filter_map(|r| r.basis.split_once(" = ").map(|(_, paths)| paths.to_owned()))
        .flat_map(|paths| paths.split(':').map(str::to_owned).collect::<Vec<_>>())
        .filter(|p| {
            p.is_empty()
                || !(p.starts_with('/') || p.starts_with("$ORIGIN") || p.starts_with("${ORIGIN}"))
                || ["/tmp", "/var/tmp", "/dev/shm"]
                    .iter()
                    .any(|t| p == t || p.starts_with(&format!("{t}/")))
        })
        .collect()
}

/// GNU build-id, hex.
pub fn build_id(elf: &Elf<'_>) -> String {
    elf.notes()
        .into_iter()
        .find(|(name, kind, _)| name == "GNU" && *kind == 3)
        .map(|(_, _, desc)| desc.iter().map(|b| format!("{b:02x}")).collect())
        .unwrap_or_default()
}

pub fn state_of<'r>(rows: &'r [HardeningRow], property: &str) -> Option<&'r HardeningRow> {
    rows.iter().find(|r| r.property == property)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::ElfBuilder;

    fn analyze(bytes: &[u8]) -> (Vec<HardeningRow>, Language) {
        let elf = Elf::parse(bytes).unwrap();
        let symbols = elf.symbol_names();
        let language = language(&elf, &symbols);
        (rows(&elf, &symbols, language), language)
    }

    fn state(rows: &[HardeningRow], property: &str) -> State {
        state_of(rows, property).unwrap().state
    }

    #[test]
    fn a_hardened_c_binary_reads_enabled_everywhere() {
        let bytes = ElfBuilder::new()
            .pie()
            .relro()
            .bind_now()
            .stack(false)
            .symbols(&["main", "__stack_chk_fail", "__memcpy_chk"])
            .build();
        let (rows, language) = analyze(&bytes);
        assert_eq!(language, Language::Native);
        assert_eq!(state(&rows, "pie"), State::Enabled);
        assert_eq!(state(&rows, "nx"), State::Enabled);
        assert_eq!(state(&rows, "relro"), State::Enabled);
        assert_eq!(state(&rows, "stack_protector"), State::Enabled);
        assert_eq!(state(&rows, "fortify"), State::Enabled);
        assert_eq!(state(&rows, "rpath"), State::Disabled);
        assert_eq!(state(&rows, "stripped"), State::Partial);
    }

    #[test]
    fn a_soft_binary_reads_disabled_with_its_basis() {
        let bytes = ElfBuilder::new()
            .stack(true)
            .relro()
            .rpath("lib:/tmp/x")
            .symbols(&["main", "memcpy"])
            .debug_info()
            .build();
        let (rows, _) = analyze(&bytes);
        assert_eq!(state(&rows, "pie"), State::Disabled);
        assert_eq!(state(&rows, "nx"), State::Disabled);
        assert!(state_of(&rows, "nx").unwrap().basis.contains("RWE"));
        assert_eq!(state(&rows, "relro"), State::Partial);
        assert_eq!(state(&rows, "stack_protector"), State::Disabled);
        assert_eq!(
            state_of(&rows, "stack_protector").unwrap().confidence,
            Confidence::Derived
        );
        assert_eq!(state(&rows, "fortify"), State::Disabled);
        assert_eq!(state(&rows, "stripped"), State::Disabled);
        assert_eq!(risky_search_paths(&rows), ["lib", "/tmp/x"]);
    }

    #[test]
    fn no_gnu_stack_falls_back_to_the_machine_default() {
        let x86_64 = ElfBuilder::new().build();
        let (rows, _) = analyze(&x86_64);
        let nx = state_of(&rows, "nx").unwrap();
        assert_eq!(
            (nx.state, nx.confidence),
            (State::Enabled, Confidence::Derived)
        );
        let i386 = ElfBuilder::new().machine(elf::EM_386).build();
        let (rows, _) = analyze(&i386);
        assert_eq!(state(&rows, "nx"), State::Disabled);
    }

    #[test]
    fn go_and_rust_are_not_judged_by_c_instrumentation() {
        let go = ElfBuilder::new()
            .section(".gopclntab", vec![0; 8])
            .symbols(&["main.main"])
            .build();
        let (rows, language) = analyze(&go);
        assert_eq!(language, Language::Go);
        assert_eq!(state(&rows, "stack_protector"), State::NotApplicable);
        assert_eq!(state(&rows, "fortify"), State::NotApplicable);
        let rust = ElfBuilder::new().symbols(&["rust_begin_unwind"]).build();
        assert_eq!(analyze(&rust).1, Language::Rust);
    }

    #[test]
    fn libraries_and_static_pie_are_told_apart() {
        let library = ElfBuilder::new().shared_object().build();
        let (rows, _) = analyze(&library);
        assert_eq!(state(&rows, "pie"), State::NotApplicable);
        let static_pie = ElfBuilder::new()
            .shared_object()
            .flags_1(elf::DF_1_PIE)
            .build();
        let elf = Elf::parse(&static_pie).unwrap();
        assert_eq!(kind(&elf, elf::DF_1_PIE), "static_pie_executable");
    }

    #[test]
    fn reads_the_gnu_build_id() {
        let bytes = ElfBuilder::new()
            .section(
                ".note.gnu.build-id",
                crate::testing::build_id_note(&[0x01, 0xfe]),
            )
            .build();
        assert_eq!(build_id(&Elf::parse(&bytes).unwrap()), "01fe");
    }
}

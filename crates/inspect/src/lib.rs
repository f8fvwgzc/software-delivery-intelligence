//! Reverse-engineers a shipped container image (CONTRACT.md §22.1).
//!
//! A repository review sees what the team *meant* to ship; this crate reads
//! what *was* shipped: the release image's layers, as the registry holds
//! them. It finds what only the built artifact can show:
//!
//! - the dependency versions compiled into Go binaries (build info) and
//!   Rust binaries (cargo-auditable), the Go toolchain that built them, and
//!   the packages the base image and `node_modules` / `site-packages`
//!   actually carry, ready for an OSV lookup ([`osv`]);
//! - binary hardening (PIE, NX, RELRO, stack protector, FORTIFY, RPATH,
//!   stripping) with REA's evidence model ([`hardening`]);
//! - provenance: a binary built from another commit than the release, or
//!   from uncommitted changes ([`rules`]);
//! - things that should never ship: private keys, `.env` files, source
//!   maps ([`scan`]); an end-of-life distribution.
//!
//! The static readers are Rust ports of REA, "Reverse Engineer Anything"
//! (`github.com/morluto/rea`, MIT License, Copyright (c) 2026 morluto; see
//! NOTICE): its Go build-info decoder, cargo-auditable reader and ELF
//! hardening rows, with their limits and their rule that what could not be
//! read is a limitation, never a clean bill of health.
//!
//! Pure like `sdi-analyzer`: bytes in, a [`Report`] out. No network (the
//! caller asks OSV), no clock (the caller passes the day), no disk.
//!
//! ```text
//! let mut scan = ImageScan::new(Limits::default());
//! for layer in layers { scan.layer(decompressed)?; }      // lowest first
//! let mut report = scan.finish();
//! rules::apply(&mut report, &Context { today, repo_full_name, commit_sha });
//! let asks = osv::queries(&report, 1000);                   // caller asks OSV
//! osv::add_findings(&mut report, &answers);
//! rules::finalize(&mut report, 500, 2000);
//! let blocking = Gate::parse("high", "binary.*").blocking(&report);
//! ```

pub mod auditable;
pub mod elf;
pub mod golang;
pub mod hardening;
pub mod osv;
pub mod packages;
pub mod report;
pub mod rules;
pub mod scan;
mod testing;

pub use report::{
    BinaryReport, Component, Confidence, Finding, HardeningRow, OsRelease, Report, SCHEMA,
    Severity, State, ToolRun,
};
pub use rules::{Context, Gate};
pub use scan::{ImageScan, Limits, ScanError};

/// `off`, `report` or `gate`: what an environment does with an inspection
/// (CONTRACT.md §22.1).
pub const MODE_OFF: &str = "off";
pub const MODE_REPORT: &str = "report";
pub const MODE_GATE: &str = "gate";
pub const MODES: [&str; 3] = [MODE_OFF, MODE_REPORT, MODE_GATE];

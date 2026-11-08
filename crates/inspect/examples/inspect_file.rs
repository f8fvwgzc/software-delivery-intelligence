//! Inspects one ELF file or one uncompressed layer tar and prints the
//! report as JSON: `cargo run -p sdi-inspect --example inspect_file -- <path> [owner/repo] [commit]`.

use std::{env, fs};

use sdi_inspect::{ImageScan, Limits, rules};

fn main() {
    let args: Vec<String> = env::args().collect();
    let Some(path) = args.get(1) else {
        eprintln!("usage: inspect_file <elf-or-layer.tar> [owner/repo] [commit]");
        std::process::exit(2);
    };
    let bytes = fs::read(path).expect("read the file");
    let mut report = if sdi_inspect::elf::is_elf(&bytes) {
        let mut limitations = Vec::new();
        let mut report = sdi_inspect::Report::default();
        if let Some((binary, components)) =
            sdi_inspect::scan::analyze_binary(path, &bytes, true, &mut limitations)
        {
            report.binaries.push(binary);
            report.components = components;
        }
        report.limitations = limitations;
        report
    } else {
        let mut scan = ImageScan::new(Limits::default());
        scan.layer(&bytes[..]).expect("an uncompressed tar");
        scan.finish()
    };
    let ctx = rules::Context {
        today: (2026, 11, 8),
        repo_full_name: args.get(2).map_or("", String::as_str),
        commit_sha: args.get(3).map_or("", String::as_str),
    };
    rules::apply(&mut report, &ctx);
    rules::finalize(&mut report, 500, 2000);
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
}

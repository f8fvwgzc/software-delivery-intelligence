use std::path::Path;

const PROTOS: [&str; 6] = [
    "sdi/common/v1/common.proto",
    "sdi/knowledge/v1/knowledge.proto",
    "sdi/intelligence/v1/intelligence.proto",
    "sdi/execution/v1/execution.proto",
    "sdi/identity/v1/identity.proto",
    "sdi/gitops/v1/gitops.proto",
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../proto");
    let files = PROTOS.map(|file| root.join(file));

    // Each file, not only the directory: a directory is fresh while no
    // entry is added or removed, so an edited contract would not rebuild.
    println!("cargo:rerun-if-changed={}", root.display());
    for file in &files {
        println!("cargo:rerun-if-changed={}", file.display());
    }

    tonic_prost_build::configure()
        .build_client(true)
        .build_server(true)
        .compile_protos(&files, &[root])?;

    Ok(())
}

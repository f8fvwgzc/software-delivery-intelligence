use std::path::Path;

const PROTOS: [&str; 5] = [
    "sdi/common/v1/common.proto",
    "sdi/knowledge/v1/knowledge.proto",
    "sdi/intelligence/v1/intelligence.proto",
    "sdi/execution/v1/execution.proto",
    "sdi/identity/v1/identity.proto",
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../proto");
    let files = PROTOS.map(|file| root.join(file));

    println!("cargo:rerun-if-changed={}", root.display());

    tonic_prost_build::configure()
        .build_client(true)
        .build_server(true)
        .compile_protos(&files, &[root])?;

    Ok(())
}

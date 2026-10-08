//! Pure repository analysis: files in, facts, graph and understanding out.
//!
//! No network, no database, no clock. The knowledge service owns all I/O and
//! calls in here, which keeps every detector unit-testable with plain strings.

mod capabilities;
mod detectors;
mod files;
mod graph;
mod manifests;
pub mod model;
mod patterns;
mod profile;
mod questions;
mod understanding;
mod util;
mod yaml;

pub use files::wanted_files;
pub use model::*;

/// Largest file the scanner should download, in bytes.
pub const MAX_FILE_BYTES: u64 = 200_000;

/// Upper bound on how many files one scan downloads.
pub const MAX_FILES: usize = 150;

/// Turns a snapshot into the project knowledge model.
///
/// Detectors append facts, nodes, edges and chunks; the passes afterwards
/// dedupe them, derive capabilities and patterns, decide what must be asked
/// and write the understanding. Nothing here can fail: a malformed file is
/// evidence of nothing, not an error.
pub fn analyze(snapshot: &RepoSnapshot) -> Analysis {
    let mut ctx = detectors::Ctx::new(snapshot);
    detectors::run_all(&mut ctx);

    ctx.out.facts = dedupe_facts(std::mem::take(&mut ctx.out.facts));
    ctx.out.technologies = technologies(&ctx.out.facts);
    ctx.out.capabilities = capabilities::derive(&ctx.out.facts);
    ctx.out.learned_patterns = patterns::derive(&ctx, &ctx.out.capabilities);
    ctx.out.questions = questions::derive(&ctx);
    ctx.out.understanding = understanding::derive(&ctx);
    ctx.out.profile_text = profile::derive(&ctx, &ctx.out.capabilities, &ctx.out.understanding);

    let u = &ctx.out.understanding;
    let architecture = format!(
        "{}\n\n{}\n\nRequest flow: {}",
        u.tldr,
        u.architecture,
        u.request_flow.join(" → ")
    );
    ctx.chunk(ChunkKind::Architecture, "", "Architecture", &architecture);

    let (nodes, edges) = graph::finalize(
        &ctx.repo_key,
        std::mem::take(&mut ctx.out.nodes),
        std::mem::take(&mut ctx.out.edges),
    );
    ctx.out.nodes = nodes;
    ctx.out.edges = edges;
    ctx.out
}

/// Keys are unique per analysis: the most confident fact wins, first-seen
/// order is kept so output stays deterministic.
fn dedupe_facts(facts: Vec<FactDraft>) -> Vec<FactDraft> {
    let mut out: Vec<FactDraft> = Vec::with_capacity(facts.len());
    for f in facts {
        match out.iter_mut().find(|x| x.key == f.key) {
            Some(existing) => {
                if f.confidence > existing.confidence {
                    *existing = f;
                }
            }
            None => out.push(f),
        }
    }
    out
}

/// Display names, most significant first: frameworks, languages, datastores,
/// then delivery tooling.
fn technologies(facts: &[FactDraft]) -> Vec<String> {
    let order = [
        FactCategory::Framework,
        FactCategory::Language,
        FactCategory::Datastore,
        FactCategory::Container,
        FactCategory::CiCd,
        FactCategory::Iac,
        FactCategory::Cloud,
    ];
    let mut out: Vec<String> = Vec::new();
    for cat in order {
        for f in facts
            .iter()
            .filter(|f| f.category == cat && f.confidence >= 0.8)
        {
            let name = match cat {
                FactCategory::Container if f.key.starts_with("container:dockerfile:") => {
                    "Docker".to_string()
                }
                FactCategory::Container if f.key.starts_with("container:compose:") => {
                    "Docker Compose".to_string()
                }
                FactCategory::Container if f.key == "container:nginx" => "nginx".to_string(),
                FactCategory::Container => continue,
                FactCategory::CiCd if f.key.starts_with("ci:workflow:") => continue,
                FactCategory::Iac if f.key.starts_with("iac:terraform:") => continue,
                FactCategory::Datastore => {
                    if f.key.starts_with("datastore:orm:")
                        || f.key.starts_with("datastore:configured:")
                        || f.key.starts_with("architecture:")
                    {
                        continue;
                    }
                    let engine = f
                        .key
                        .trim_start_matches("infra:")
                        .trim_start_matches("datastore:");
                    util::display(engine)
                }
                _ => f.label.clone(),
            };
            if !out.contains(&name) {
                out.push(name);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedupe_keeps_highest_confidence_and_order() {
        let mk = |key: &str, kind: FactKind, c: f64| FactDraft {
            kind,
            category: FactCategory::Cloud,
            key: key.into(),
            label: key.into(),
            value: String::new(),
            confidence: c,
            source_path: String::new(),
            evidence: String::new(),
        };
        let out = dedupe_facts(vec![
            mk("cloud:aws", FactKind::Inferred, 0.6),
            mk("cloud:azure", FactKind::Detected, 1.0),
            mk("cloud:aws", FactKind::Detected, 1.0),
        ]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].key, "cloud:aws");
        assert_eq!(out[0].kind, FactKind::Detected);
    }
}

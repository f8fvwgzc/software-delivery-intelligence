//! Pure repository analysis: files in, facts, graph and understanding out.
//!
//! No network, no database, no clock. The knowledge service owns all I/O and
//! calls in here, which keeps every detector unit-testable with plain strings.
//! The only time-dependent judgement, runtime end of life, takes `today` as
//! a parameter ([`analyze_at`]); there is no default date, so a service can
//! never analyse against a day baked into the binary.

mod capabilities;
mod config;
mod detectors;
pub mod eol;
mod files;
mod graph;
pub mod lang;
mod manifests;
pub mod maturity;
pub mod model;
mod patterns;
mod profile;
mod questions;
mod understanding;
mod util;
mod yaml;

pub use config::{CONFIG_PATH, is_usable as config_is_usable, parse_config};
pub use eol::Day;
pub use files::{wanted_files, wanted_files_with};
pub use lang::language_for_path;
pub use model::*;
pub use util::{glob_match, matches_any_glob};

/// Largest file the scanner should download, in bytes.
pub const MAX_FILE_BYTES: u64 = 200_000;

/// Upper bound on how many files one scan downloads.
pub const MAX_FILES: usize = 150;

/// Turns a snapshot into the project knowledge model as of `today`.
///
/// Detectors append facts, nodes, edges and chunks; the passes afterwards
/// dedupe them, derive capabilities and patterns, decide what must be asked,
/// score maturity and write the understanding. `today` only matters for the
/// end-of-life check and may be any tuple: nothing here can fail, because a
/// malformed file (or date) is evidence of nothing, not an error.
pub fn analyze_at(snapshot: &RepoSnapshot, today: Day) -> Analysis {
    let mut ctx = detectors::Ctx::new(snapshot, today);
    detectors::run_all(&mut ctx);

    ctx.out.facts = dedupe_facts(std::mem::take(&mut ctx.out.facts));
    ctx.out.technologies = technologies(&ctx.out.facts);
    ctx.out.capabilities = capabilities::derive(&ctx.out.facts);
    ctx.out.learned_patterns = patterns::derive(&ctx, &ctx.out.capabilities);
    let mut questions = questions::derive(&ctx);
    config::answer_questions(&ctx.out.config, &mut questions);
    ctx.out.questions = questions;
    ctx.out.maturity = maturity::score(&ctx.out.facts);
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
                // Per-workflow facts describe files, not tools.
                FactCategory::CiCd
                    if f.key.starts_with("ci:workflow:")
                        || f.key.starts_with("ci:build:")
                        || f.key.starts_with("ci:build_image:") =>
                {
                    continue;
                }
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

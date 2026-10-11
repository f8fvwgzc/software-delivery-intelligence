//! Repositories: `FromRow` structs plus plain async functions. Single-query
//! functions take any executor; multi-query ones take a connection so they
//! can run inside the caller's transaction.

pub mod chunks;
pub mod config;
pub mod delivery;
pub mod dependencies;
pub mod facts;
pub mod graph;
pub mod maturity;
pub mod patterns;
pub mod pipelines;
pub mod profiles;
pub mod projects;
pub mod questions;
pub mod scans;
pub mod settings;
pub mod stats;
pub mod understanding;

/// Parses a pgvector text literal (`[0.1,0.2]`) back into floats.
pub fn parse_pgvector(literal: &str) -> Vec<f32> {
    literal
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .filter_map(|v| v.trim().parse().ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pgvector_literal_round_trips() {
        let vector = vec![0.5_f32, -0.25, 1.0];
        let literal = sdi_common::embed::to_pgvector(&vector);
        assert_eq!(parse_pgvector(&literal), vector);
        assert!(parse_pgvector("").is_empty());
    }
}

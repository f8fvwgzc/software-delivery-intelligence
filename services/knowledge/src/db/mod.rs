//! Repositories: `FromRow` structs plus plain async functions. Single-query
//! functions take any executor; multi-query ones take a connection so they
//! can run inside the caller's transaction.
//!
//! Tenancy (CONTRACT.md §18): every function takes the caller's
//! `workspace_id` and every query filters on it, so an id alone never
//! addresses a row. Project lookups are `(workspace_id, id)`; child rows
//! carry the workspace too and are filtered on it as well, which makes a
//! cross-workspace read impossible even if a project id leaks.

pub mod chunks;
pub mod code;
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
pub mod pull_requests;
pub mod questions;
pub mod scans;
pub mod settings;
pub mod stats;
pub mod understanding;
pub mod workspace;

use uuid::Uuid;

/// The workspace every row created before tenancy belongs to (migration
/// `0004_tenancy`): the single-tenant installation that preceded
/// workspaces. Dev mode's personal workspace maps to it.
pub const BOOTSTRAP_WORKSPACE: Uuid = uuid::uuid!("00000000-0000-7000-8000-00000000dead");

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

    #[test]
    fn bootstrap_workspace_is_the_documented_id() {
        assert_eq!(
            BOOTSTRAP_WORKSPACE.to_string(),
            "00000000-0000-7000-8000-00000000dead"
        );
    }
}

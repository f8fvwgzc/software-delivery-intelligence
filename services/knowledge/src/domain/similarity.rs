//! Project-to-project similarity. Architecture is the cosine of the profile
//! embeddings; the other dimensions compare capability subsets by Jaccard.

use std::collections::BTreeSet;

use sdi_common::embed::cosine;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Profile {
    pub technologies: Vec<String>,
    pub capabilities: Vec<String>,
    pub embedding: Vec<f32>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Breakdown {
    pub architecture: f64,
    pub iac: f64,
    pub deployment: f64,
    pub cloud: f64,
    pub database: f64,
    pub overall: f64,
}

type Signal = fn(&Breakdown) -> f64;

const WEIGHTS: [(Signal, f64); 5] = [
    (|b| b.architecture, 0.40),
    (|b| b.deployment, 0.20),
    (|b| b.iac, 0.15),
    (|b| b.cloud, 0.15),
    (|b| b.database, 0.10),
];

fn subset<'a>(capabilities: &'a [String], prefixes: &[&str]) -> BTreeSet<&'a str> {
    capabilities
        .iter()
        .map(String::as_str)
        .filter(|c| {
            prefixes
                .iter()
                .any(|p| c == p || c.starts_with(&format!("{p}:")))
        })
        .collect()
}

/// Jaccard index. Two empty sets agree completely: neither project does the thing.
pub fn jaccard(a: &BTreeSet<&str>, b: &BTreeSet<&str>) -> f64 {
    let union = a.union(b).count();
    if union == 0 {
        return 1.0;
    }
    a.intersection(b).count() as f64 / union as f64
}

pub fn compare(a: &Profile, b: &Profile) -> Breakdown {
    let dim = |prefixes: &[&str]| {
        jaccard(
            &subset(&a.capabilities, prefixes),
            &subset(&b.capabilities, prefixes),
        )
    };
    let mut breakdown = Breakdown {
        architecture: if a.embedding.is_empty() || b.embedding.is_empty() {
            0.0
        } else {
            cosine(&a.embedding, &b.embedding)
        },
        iac: dim(&["iac", "k8s"]),
        deployment: dim(&["dockerfile", "docker_compose", "ci", "static_site"]),
        cloud: dim(&["cloud"]),
        database: dim(&["database"]),
        overall: 0.0,
    };
    breakdown.overall = WEIGHTS
        .iter()
        .map(|(get, weight)| get(&breakdown) * weight)
        .sum();
    breakdown
}

/// Technologies both projects list, case-insensitively, in `a`'s order.
pub fn shared_technologies(a: &[String], b: &[String]) -> Vec<String> {
    let lower: BTreeSet<String> = b.iter().map(|t| t.to_lowercase()).collect();
    let mut seen = BTreeSet::new();
    a.iter()
        .filter(|t| lower.contains(&t.to_lowercase()) && seen.insert(t.to_lowercase()))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sdi_common::embed::embed;

    fn caps(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn identical_profiles_score_one() {
        let p = Profile {
            technologies: caps(&["Rust", "PostgreSQL"]),
            capabilities: caps(&[
                "dockerfile",
                "iac:terraform",
                "cloud:aws",
                "database:postgres",
            ]),
            embedding: embed("rust axum postgres terraform aws"),
        };
        let b = compare(&p, &p);
        assert!((b.overall - 1.0).abs() < 1e-6);
        assert!((b.architecture - 1.0).abs() < 1e-4);
    }

    #[test]
    fn jaccard_handles_partial_overlap_and_empty() {
        let a = BTreeSet::from(["iac:terraform", "k8s:helm"]);
        let b = BTreeSet::from(["iac:terraform"]);
        assert!((jaccard(&a, &b) - 0.5).abs() < 1e-9);
        assert_eq!(jaccard(&BTreeSet::new(), &BTreeSet::new()), 1.0);
        assert_eq!(jaccard(&a, &BTreeSet::new()), 0.0);
    }

    #[test]
    fn overall_is_weighted_mean_of_dimensions() {
        let a = Profile {
            technologies: vec![],
            capabilities: caps(&["cloud:aws", "database:postgres"]),
            embedding: vec![],
        };
        let b = Profile {
            technologies: vec![],
            capabilities: caps(&["cloud:azure", "database:postgres"]),
            embedding: vec![],
        };
        let r = compare(&a, &b);
        assert_eq!(r.architecture, 0.0);
        assert_eq!(r.cloud, 0.0);
        assert_eq!(r.database, 1.0);
        assert_eq!(r.iac, 1.0);
        assert_eq!(r.deployment, 1.0);
        let expected = 0.20 + 0.15 + 0.10;
        assert!((r.overall - expected).abs() < 1e-9);
    }

    #[test]
    fn shared_technologies_is_case_insensitive_and_ordered() {
        let shared = shared_technologies(
            &caps(&["React", "Rust", "PostgreSQL", "Docker"]),
            &caps(&["docker", "postgresql", "Go"]),
        );
        assert_eq!(shared, vec!["PostgreSQL", "Docker"]);
    }
}

//! Per-option cost model (CONTRACT.md §5.6, §6 Budget). The catalog ships
//! every pattern's cost model as `Pattern.cost_lines` (compute, networking,
//! registry, CI minutes); those lines are the baseline and this module only
//! fills them in for the project: compute is multiplied by the replicas the
//! project needs, a free CI line is priced for a private repository, and the
//! project's own datastores add managed-service lines the template cannot
//! know about. A learned pattern has no seeded lines, so its
//! `estimated_monthly_cost_usd` stands in as a single baseline line. The
//! lines sum to the option's estimate, so the breakdown and the number never
//! drift.

use sdi_proto::knowledge::v1::{Pattern, ProjectContext};

use super::model::CostLineData;

/// GitHub Actions free tier is for public repositories; private repositories
/// pay per minute once the included minutes run out.
const CI_MINUTES_PRIVATE_USD: f64 = 4.0;

struct Signals {
    relational: Vec<String>,
    cache: bool,
    document: bool,
    is_private: bool,
    replicas: u32,
}

fn signals(ctx: &ProjectContext) -> Signals {
    let mut relational = Vec::new();
    let mut cache = false;
    let mut document = false;
    let engines = ctx
        .facts
        .iter()
        .filter(|f| f.confidence >= 0.8)
        .filter_map(|f| {
            f.key
                .strip_prefix("datastore:")
                .or_else(|| f.key.strip_prefix("infra:datastore:"))
        })
        .filter(|e| !e.contains(':'))
        .map(str::to_lowercase)
        .chain(
            ctx.capabilities
                .iter()
                .filter_map(|c| c.strip_prefix("database:"))
                .map(str::to_lowercase),
        );
    for engine in engines {
        match engine.as_str() {
            "postgres" | "mysql" => {
                if !relational.contains(&engine) {
                    relational.push(engine);
                }
            }
            "redis" => cache = true,
            "mongodb" => document = true,
            _ => {}
        }
    }
    Signals {
        relational,
        cache,
        document,
        is_private: ctx.project.as_ref().is_some_and(|p| p.is_private),
        replicas: if ctx.capabilities.iter().any(|c| c == "tests:e2e") {
            2
        } else {
            1
        },
    }
}

/// How a runtime family prices the add-ons a project's datastores need:
/// (managed database, managed cache).
fn addons(pattern: &Pattern) -> (f64, f64) {
    match (pattern.cloud.as_str(), pattern.runtime.as_str()) {
        ("azure", _) => (14.0, 16.0),
        ("aws", _) => (13.0, 12.0),
        ("gcp", _) => (10.0, 15.0),
        (_, "kubernetes") => (15.0, 10.0),
        (_, "paas") => (7.0, 3.0),
        // A VM runs its datastores in Compose on the same box: no extra lines.
        (_, "vm_docker_compose") => (0.0, 0.0),
        _ => (0.0, 0.0),
    }
}

/// The billing family a seeded line belongs to: the note's prefix
/// (`compute`, `ci: free tier` → `ci`).
fn kind(line: &sdi_proto::common::v1::CostLine) -> &str {
    line.note.split(':').next().unwrap_or_default().trim()
}

fn scales_with_replicas(pattern: &Pattern) -> bool {
    pattern.runtime != "serverless_container" && pattern.runtime != "serverless"
}

pub fn breakdown(pattern: &Pattern, ctx: &ProjectContext) -> Vec<CostLineData> {
    let s = signals(ctx);
    let mut lines = Vec::new();
    let deploys = pattern.category == "deployment" || pattern.category == "infrastructure";
    let base = pattern.estimated_monthly_cost_usd.max(0.0);
    let mut has_database = false;
    let mut has_ci = false;

    if pattern.cost_lines.is_empty() {
        // No seeded model: the estimate is the whole baseline.
        if base > 0.0 || deploys {
            let compute = if deploys && scales_with_replicas(pattern) {
                base * f64::from(s.replicas)
            } else {
                base
            };
            lines.push(CostLineData::new(
                if deploys { "compute" } else { "service" },
                compute,
                if compute > base {
                    format!(
                        "{} replica(s) of the pattern's baseline ${base:.0}",
                        s.replicas
                    )
                } else {
                    format!("{} baseline", pattern.name)
                },
            ));
        }
    } else {
        for line in &pattern.cost_lines {
            let family = kind(line);
            let mut monthly = line.monthly_usd.max(0.0);
            let mut note = line.note.clone();
            match family {
                "compute" if deploys && s.replicas > 1 && scales_with_replicas(pattern) => {
                    monthly *= f64::from(s.replicas);
                    note = format!("{note}: {} replicas", s.replicas);
                }
                "ci" if s.is_private && monthly == 0.0 => {
                    has_ci = true;
                    monthly = CI_MINUTES_PRIVATE_USD;
                    note = "ci: GitHub Actions minutes beyond the private-repository allowance"
                        .to_owned();
                }
                "ci" => has_ci = true,
                "database" => has_database = true,
                _ => {}
            }
            lines.push(CostLineData::new(line.item.clone(), monthly, note));
        }
    }

    // What the project brings that the template cannot know: its datastores
    // become managed services on a cloud (a VM keeps them in Compose).
    if deploys {
        let (db, cache) = addons(pattern);
        if !has_database {
            for engine in &s.relational {
                if db > 0.0 {
                    lines.push(CostLineData::new(
                        format!("database:{engine}"),
                        db,
                        format!("smallest managed {engine} instance"),
                    ));
                }
            }
            if s.document && db > 0.0 {
                lines.push(CostLineData::new(
                    "database:mongodb",
                    db,
                    "smallest managed document database",
                ));
            }
        }
        if s.cache && cache > 0.0 {
            lines.push(CostLineData::new(
                "cache:redis",
                cache,
                "smallest managed cache",
            ));
        }
    }
    if !has_ci
        && s.is_private
        && (pattern.category == "ci_cd"
            || deploys
            || pattern
                .technologies
                .iter()
                .any(|t| t.contains("GitHub Actions")))
    {
        lines.push(CostLineData::new(
            "ci_minutes",
            CI_MINUTES_PRIVATE_USD,
            "GitHub Actions minutes beyond the private-repository allowance",
        ));
    }
    lines
}

pub fn total(lines: &[CostLineData]) -> f64 {
    let sum: f64 = lines.iter().map(|l| l.monthly_usd).sum();
    (sum * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use sdi_proto::common::v1::CostLine;

    use super::*;
    use crate::domain::ranking::fixtures;

    fn seeded(lines: &[(&str, f64, &str)]) -> Vec<CostLine> {
        lines
            .iter()
            .map(|(item, monthly_usd, note)| CostLine {
                item: (*item).to_owned(),
                monthly_usd: *monthly_usd,
                note: (*note).to_owned(),
            })
            .collect()
    }

    #[test]
    fn seeded_lines_are_the_baseline_and_the_project_fills_them_in() {
        let mut ctx = fixtures::context();
        ctx.facts.push(fixtures::fact(
            "f-pg",
            "datastore:postgres",
            "PostgreSQL",
            "compose.yml",
        ));
        ctx.facts.push(fixtures::fact(
            "f-redis",
            "datastore:redis",
            "Redis",
            "compose.yml",
        ));
        ctx.project.as_mut().unwrap().is_private = true;
        ctx.capabilities.push("tests:e2e".into());

        // The catalog's aws-ecs-fargate model, as seeded.
        let mut aws =
            fixtures::pattern("aws-ecs-fargate", "aws", "serverless_container", &[], 75.0);
        aws.cost_lines = seeded(&[
            ("Fargate task 0.5 vCPU / 1 GB", 18.0, "compute"),
            ("Application Load Balancer", 22.0, "networking"),
            ("NAT gateway", 32.0, "networking"),
            ("ECR storage", 1.0, "registry"),
            ("GitHub Actions minutes", 2.0, "ci"),
        ]);
        let lines = breakdown(&aws, &ctx);
        let items: Vec<&str> = lines.iter().map(|l| l.item.as_str()).collect();
        assert_eq!(
            items,
            vec![
                "Fargate task 0.5 vCPU / 1 GB",
                "Application Load Balancer",
                "NAT gateway",
                "ECR storage",
                "GitHub Actions minutes",
                "database:postgres",
                "cache:redis",
            ]
        );
        // Serverless compute does not multiply by replicas; a priced CI line
        // is kept as seeded rather than doubled by the private-repo rule.
        assert!((total(&lines) - (75.0 + 13.0 + 12.0)).abs() < 1e-9);

        // A VM scales its compute with the replicas, keeps its datastores in
        // Compose, and its free CI line is priced for a private repository.
        let mut vm = fixtures::pattern("vm-docker-compose", "any", "vm_docker_compose", &[], 25.0);
        vm.cost_lines = seeded(&[
            ("Small VM (2 vCPU, 4 GB)", 20.0, "compute"),
            ("Static IP and egress", 3.0, "networking"),
            ("GitHub Actions minutes", 0.0, "ci: free tier"),
        ]);
        let vm_lines = breakdown(&vm, &ctx);
        assert_eq!(vm_lines.len(), 3);
        assert!((vm_lines[0].monthly_usd - 40.0).abs() < 1e-9);
        assert!(vm_lines[0].note.contains("2 replicas"));
        assert!((vm_lines[2].monthly_usd - CI_MINUTES_PRIVATE_USD).abs() < 1e-9);
        assert!((total(&vm_lines) - 47.0).abs() < 1e-9);

        // A seeded database line (managed-database) is not doubled by the
        // project's own postgres signal.
        let mut managed = fixtures::pattern("managed-database", "any", "", &[], 15.0);
        managed.category = "infrastructure".to_owned();
        managed.cost_lines = seeded(&[
            ("Smallest managed instance", 12.0, "database"),
            ("Automated backups and storage", 3.0, "database"),
        ]);
        let managed_lines = breakdown(&managed, &ctx);
        assert_eq!(managed_lines.len(), 3, "{managed_lines:?}");
        assert_eq!(managed_lines[2].item, "ci_minutes");
    }

    #[test]
    fn a_learned_pattern_falls_back_to_its_estimate() {
        let mut ctx = fixtures::context();
        ctx.facts.push(fixtures::fact(
            "f-pg",
            "datastore:postgres",
            "PostgreSQL",
            "compose.yml",
        ));
        let gcp = fixtures::pattern("gcp-cloud-run", "gcp", "serverless_container", &[], 20.0);
        let lines = breakdown(&gcp, &ctx);
        assert_eq!(
            lines.iter().map(|l| l.item.as_str()).collect::<Vec<_>>(),
            vec!["compute", "database:postgres"]
        );
        assert!((total(&lines) - 30.0).abs() < 1e-9);

        // Public repository, no datastore, non-deployment: the estimate alone.
        let mut docs = fixtures::pattern("readme-architecture-docs", "any", "", &[], 0.0);
        docs.category = "documentation".to_owned();
        assert!(breakdown(&docs, &fixtures::context()).is_empty());
    }
}

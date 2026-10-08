//! CONTRACT.md §6: deterministic ranking of candidate patterns for a project.
//! Pure: everything it needs arrives in `RankingInput`.

use chrono::{DateTime, Utc};
use sdi_common::embed;
use sdi_proto::{
    common::v1::KnowledgeScope,
    intelligence::v1::{Decision, DecisionScope, DecisionStatus, ProposalKind},
    knowledge::v1::{
        Fact, Pattern, PatternSource, PatternStatus, Project, ProjectContext, SimilarProject,
    },
};

use super::{
    model::{
        Breakdown, ConstraintData, EvidenceItem, EvidenceKind, PlanStepData, RankedOption, Weights,
    },
    policy,
};

pub const MAX_OPTIONS: usize = 5;
const CATALOG_RECENCY: f64 = 0.8;
const NO_DECISION_POLICY: f64 = 0.6;
const SUCCESS_PRIOR: f64 = 0.7;
const SUCCESS_SHRINK_BELOW: u32 = 5;
const COST_CEILING: f64 = 500.0;
const CONSTRAINT_PENALTY: f64 = 0.1;
const PREFERENCE_BONUS: f64 = 0.1;

pub struct RankingInput<'a> {
    pub context: &'a ProjectContext,
    pub patterns: &'a [Pattern],
    pub decisions: &'a [Decision],
    pub similar: &'a [SimilarProject],
    pub constraints: &'a [ConstraintData],
    pub weights: &'a Weights,
    pub now: DateTime<Utc>,
}

/// Pattern category a proposal kind draws its candidates from.
pub fn category_for(kind: ProposalKind) -> Option<&'static str> {
    match kind {
        ProposalKind::Deployment => Some("deployment"),
        ProposalKind::Infrastructure => Some("infrastructure"),
        ProposalKind::CiCd => Some("ci_cd"),
        ProposalKind::Testing => Some("testing"),
        ProposalKind::Security => Some("security"),
        ProposalKind::Documentation => Some("documentation"),
        ProposalKind::Dependency => Some("dependency"),
        ProposalKind::Unspecified => None,
    }
}

/// Scope filter: shared patterns are visible to everyone; private ones only
/// to the project that produced them.
pub fn visible(pattern: &Pattern, project: Option<&Project>) -> bool {
    if pattern.status() == PatternStatus::Deprecated {
        return false;
    }
    match pattern.scope() {
        KnowledgeScope::Private => project.is_some_and(|p| {
            pattern
                .used_by
                .iter()
                .any(|name| name == &p.name || name == &p.repo_full_name || name == &p.id)
        }),
        _ => true,
    }
}

/// Decisions that bind this project: its own plus every wider scope.
pub fn applicable_decisions(decisions: &[Decision], project_id: &str) -> Vec<Decision> {
    decisions
        .iter()
        .filter(|d| d.status() == DecisionStatus::Active)
        .filter(|d| {
            d.scope() != DecisionScope::Project
                || d.project_id == project_id
                || d.project_id.is_empty()
        })
        .cloned()
        .collect()
}

pub fn pattern_text(pattern: &Pattern) -> String {
    format!(
        "{} {} {}",
        pattern.name,
        pattern.summary,
        pattern.technologies.join(" ")
    )
}

/// `cloud:*`, `runtime:*`, `iac:*` tags the policy signal matches on.
pub fn pattern_tags(pattern: &Pattern) -> Vec<String> {
    [
        ("cloud", &pattern.cloud),
        ("runtime", &pattern.runtime),
        ("iac", &pattern.iac),
    ]
    .into_iter()
    .filter(|(_, value)| !value.is_empty() && *value != "any")
    .map(|(prefix, value)| format!("{prefix}:{}", value.to_lowercase()))
    .collect()
}

pub fn semantic_signal(profile: &[f32], pattern: &Pattern) -> f64 {
    embed::cosine(profile, &embed::embed(&pattern_text(pattern)))
}

/// CI systems a pattern commits to (`ci:*` requirement or a named technology),
/// as the `ci:<system>` decision vocabulary. Patterns name no CI field, so a
/// pattern that mentions none stays neutral to a `ci:*` decision.
fn pattern_ci_systems(pattern: &Pattern) -> Vec<String> {
    let mut systems: Vec<String> = pattern
        .requires
        .iter()
        .filter_map(|r| r.strip_prefix("ci:"))
        .map(str::to_owned)
        .collect();
    for tech in &pattern.technologies {
        let t = tech.to_lowercase();
        let system = if t.contains("github actions") {
            "github_actions"
        } else if t.contains("gitlab") {
            "gitlab_ci"
        } else {
            continue;
        };
        if !systems.iter().any(|s| s == system) {
            systems.push(system.to_owned());
        }
    }
    systems
}

/// One decision tag checked against the pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyMatch {
    pub decision_id: String,
    pub decision_title: String,
    pub tag: String,
    pub agrees: bool,
}

/// 1.0 when every applicable decision tag agrees, 0.0 when one contradicts,
/// 0.6 when no decision speaks to this pattern. A pattern that works on
/// "any" cloud never contradicts a cloud decision.
pub fn policy_signal(pattern: &Pattern, decisions: &[Decision]) -> (f64, Vec<PolicyMatch>) {
    let mut matches = Vec::new();
    for decision in decisions {
        for tag in &decision.tags {
            let Some((prefix, wanted)) = tag.split_once(':') else {
                continue;
            };
            let agrees = match prefix {
                "cloud" | "runtime" | "iac" => {
                    let actual = match prefix {
                        "cloud" => &pattern.cloud,
                        "runtime" => &pattern.runtime,
                        _ => &pattern.iac,
                    };
                    if actual.is_empty() || actual == "any" {
                        continue;
                    }
                    actual.eq_ignore_ascii_case(wanted)
                }
                "ci" => {
                    let systems = pattern_ci_systems(pattern);
                    if systems.is_empty() {
                        continue;
                    }
                    systems.iter().any(|s| s == wanted)
                }
                _ => continue,
            };
            matches.push(PolicyMatch {
                decision_id: decision.id.clone(),
                decision_title: decision.title.clone(),
                tag: tag.clone(),
                agrees,
            });
        }
    }
    let signal = if matches.is_empty() {
        NO_DECISION_POLICY
    } else if matches.iter().all(|m| m.agrees) {
        1.0
    } else {
        0.0
    };
    (signal, matches)
}

/// 1.0 up to 30 days, linear decay to 0.3 at two years. Catalog patterns are
/// evergreen templates and sit at 0.8.
pub fn recency_signal(pattern: &Pattern, now: DateTime<Utc>) -> f64 {
    if pattern.source() == PatternSource::Catalog {
        return CATALOG_RECENCY;
    }
    let Some(last) = pattern
        .last_used_at
        .as_ref()
        .and_then(sdi_common::time::from_proto)
    else {
        return 0.3;
    };
    let days = (now - last).num_days().max(0) as f64;
    if days <= 30.0 {
        1.0
    } else if days >= 730.0 {
        0.3
    } else {
        1.0 - 0.7 * ((days - 30.0) / 700.0)
    }
}

/// Few deployments mean little evidence: shrink the rate toward 0.7.
pub fn success_signal(pattern: &Pattern) -> f64 {
    let rate = pattern.success_rate.clamp(0.0, 1.0);
    let n = pattern.deployments;
    if n >= SUCCESS_SHRINK_BELOW {
        return rate;
    }
    let n = f64::from(n);
    let k = f64::from(SUCCESS_SHRINK_BELOW);
    (n * rate + (k - n) * SUCCESS_PRIOR) / k
}

/// Share of `requires` the project has, minus 0.1 per gap, floored at 0.
pub fn architecture_fit(requires: &[String], capabilities: &[String]) -> (f64, Vec<String>) {
    if requires.is_empty() {
        return (1.0, Vec::new());
    }
    let unmet: Vec<String> = requires
        .iter()
        .filter(|r| !capabilities.iter().any(|c| c == *r))
        .cloned()
        .collect();
    let met = (requires.len() - unmet.len()) as f64 / requires.len() as f64;
    let fit = (met - CONSTRAINT_PENALTY * unmet.len() as f64).max(0.0);
    (fit, unmet)
}

pub fn cost_signal(monthly_usd: f64) -> f64 {
    1.0 - monthly_usd.clamp(0.0, COST_CEILING) / COST_CEILING
}

pub fn security_signal(pattern: &Pattern) -> f64 {
    pattern.security_score.clamp(0.0, 1.0)
}

fn pattern_mentions(pattern: &Pattern, term: &str) -> bool {
    let term = term.to_lowercase();
    let haystack = format!(
        "{} {} {} {} {} {} {} {} {}",
        pattern.id,
        pattern.name,
        pattern.summary,
        pattern.technologies.join(" "),
        pattern.cloud,
        pattern.runtime,
        pattern.iac,
        pattern.delivery,
        pattern.pros.join(" ")
    )
    .to_lowercase();
    haystack.contains(&term)
}

/// Fact key prefixes the analyzer uses for a capability (CONTRACT.md §5.1):
/// the capability vocabulary is flat while fact keys carry their category.
fn fact_key_prefixes(requirement: &str) -> Vec<String> {
    let mut prefixes = vec![requirement.to_owned()];
    if let Some(engine) = requirement.strip_prefix("database:") {
        prefixes.push(format!("datastore:{engine}"));
        prefixes.push(format!("infra:datastore:{engine}"));
    }
    match requirement {
        "dockerfile" => prefixes.push("container:dockerfile:".into()),
        "docker_compose" => prefixes.push("container:compose:".into()),
        "lockfile" | "env_example" | "dependabot" => {
            prefixes.push(format!("security:{requirement}"));
        }
        "static_site" | "monorepo" => prefixes.push(format!("repository:{requirement}")),
        "migrations" => prefixes.push("migrations:".into()),
        _ => {}
    }
    prefixes
}

fn fact_for<'a>(facts: &'a [Fact], requirement: &str) -> Option<&'a Fact> {
    let prefixes = fact_key_prefixes(requirement);
    facts
        .iter()
        .find(|f| f.key == requirement)
        .or_else(|| {
            facts
                .iter()
                .find(|f| prefixes.iter().any(|p| f.key.starts_with(p.as_str())))
        })
        .or_else(|| {
            facts
                .iter()
                .find(|f| f.key.ends_with(&format!(":{requirement}")))
        })
}

/// Fills plan parameters from what the scan knows about the project.
pub fn instantiate_plan(pattern: &Pattern, project: Option<&Project>) -> Vec<PlanStepData> {
    let repo_name = project
        .map(|p| {
            p.repo_full_name
                .rsplit('/')
                .next()
                .unwrap_or(&p.name)
                .to_lowercase()
        })
        .unwrap_or_default();
    let repo_full = project
        .map(|p| p.repo_full_name.clone())
        .unwrap_or_default();
    let branch = project
        .map(|p| p.default_branch.clone())
        .filter(|b| !b.is_empty())
        .unwrap_or_else(|| "main".to_owned());
    let project_name = project.map(|p| p.name.clone()).unwrap_or_default();

    let fill = |text: &str| {
        text.replace("{{repo}}", &repo_name)
            .replace("{{repository}}", &repo_full)
            .replace("{{branch}}", &branch)
            .replace("{{project}}", &project_name)
    };

    pattern
        .plan
        .iter()
        .map(|step| {
            let mut params: std::collections::BTreeMap<String, String> = step
                .params
                .iter()
                .map(|(k, v)| (k.clone(), fill(v)))
                .collect();
            if !repo_full.is_empty() {
                params
                    .entry("repository".into())
                    .or_insert_with(|| repo_full.clone());
                params
                    .entry("branch".into())
                    .or_insert_with(|| branch.clone());
            }
            if step.action_type.starts_with("container.") && !repo_name.is_empty() {
                params
                    .entry("image".into())
                    .or_insert_with(|| repo_name.clone());
            }
            PlanStepData {
                key: step.key.clone(),
                title: fill(&step.title),
                description: fill(&step.description),
                action_type: step.action_type.clone(),
                adapter: step.adapter.clone(),
                depends_on: step.depends_on.clone(),
                required_permissions: step.required_permissions.clone(),
                params,
                verification: fill(&step.verification),
            }
        })
        .collect()
}

pub fn option_id(pattern_id: &str) -> String {
    format!("opt-{pattern_id}")
}

/// Ranks every visible candidate and returns the top `MAX_OPTIONS`.
pub fn rank(input: &RankingInput<'_>) -> Vec<RankedOption> {
    let project = input.context.project.as_ref();
    let profile = embed::embed(&input.context.profile_text);
    let mut options: Vec<RankedOption> = input
        .patterns
        .iter()
        .filter(|p| visible(p, project))
        .filter(|p| !excluded(p, input.constraints))
        .map(|pattern| score_pattern(pattern, input, &profile))
        .collect();

    options.sort_by(|a, b| {
        b.score
            .overall
            .partial_cmp(&a.score.overall)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.pattern_id.cmp(&b.pattern_id))
    });
    options.truncate(MAX_OPTIONS);
    for (index, option) in options.iter_mut().enumerate() {
        option.rank = index as u32 + 1;
    }
    options
}

fn excluded(pattern: &Pattern, constraints: &[ConstraintData]) -> bool {
    constraints.iter().any(|c| {
        let (kind, term) = c.split();
        kind == "exclude" && pattern_mentions(pattern, term)
    })
}

fn score_pattern(pattern: &Pattern, input: &RankingInput<'_>, profile: &[f32]) -> RankedOption {
    let ctx = input.context;
    let project = ctx.project.as_ref();
    let (policy_score, matches) = policy_signal(pattern, input.decisions);
    let (fit, mut unmet) = architecture_fit(&pattern.requires, &ctx.capabilities);
    let mut score = Breakdown {
        semantic_similarity: semantic_signal(profile, pattern),
        policy: policy_score,
        recency: recency_signal(pattern, input.now),
        success: success_signal(pattern),
        architecture_fit: fit,
        cost: cost_signal(pattern.estimated_monthly_cost_usd),
        security: security_signal(pattern),
        overall: 0.0,
    };
    score.overall = score.weighted(input.weights);

    let mut evidence = vec![EvidenceItem::new(
        EvidenceKind::Pattern,
        &pattern.id,
        &pattern.name,
        format!(
            "{} pattern, {} deployments, {:.0}% success, {} incidents",
            pattern
                .source()
                .as_str_name()
                .trim_start_matches("PATTERN_SOURCE_")
                .to_lowercase(),
            pattern.deployments,
            pattern.success_rate * 100.0,
            pattern.incidents
        ),
    )];

    for requirement in pattern.requires.iter().filter(|r| !unmet.contains(r)) {
        match fact_for(&ctx.facts, requirement) {
            Some(fact) => evidence.push(EvidenceItem::new(
                EvidenceKind::Fact,
                &fact.id,
                &fact.label,
                format!(
                    "satisfies `{requirement}` ({})",
                    if fact.source_path.is_empty() {
                        "detected"
                    } else {
                        &fact.source_path
                    }
                ),
            )),
            None => evidence.push(EvidenceItem::new(
                EvidenceKind::Fact,
                "",
                format!("capability {requirement}"),
                format!("project capability satisfies `{requirement}`"),
            )),
        }
    }

    for m in &matches {
        evidence.push(EvidenceItem::new(
            EvidenceKind::Decision,
            &m.decision_id,
            &m.decision_title,
            format!(
                "tag `{}` {}",
                m.tag,
                if m.agrees { "agrees" } else { "contradicts" }
            ),
        ));
    }

    if let Some(stats) = &ctx.pipeline_stats
        && stats.total > 0
    {
        evidence.push(EvidenceItem::new(
            EvidenceKind::DeliveryHistory,
            "pipelines",
            "Pipeline history",
            format!(
                "{} runs, {:.0}% success, avg {}s",
                stats.total,
                stats.success_rate * 100.0,
                stats.avg_duration_seconds
            ),
        ));
    }
    if let Some(stats) = &ctx.deployment_stats
        && stats.total > 0
    {
        evidence.push(EvidenceItem::new(
            EvidenceKind::DeliveryHistory,
            "deployments",
            "Deployment history",
            format!(
                "{} deployments, {:.0}% success",
                stats.total,
                stats.success_rate * 100.0
            ),
        ));
    }

    for similar in input.similar {
        let Some(other) = similar.project.as_ref() else {
            continue;
        };
        let uses_pattern = pattern
            .used_by
            .iter()
            .any(|name| name == &other.name || name == &other.repo_full_name);
        let shared: Vec<&String> = similar
            .shared_technologies
            .iter()
            .filter(|t| {
                pattern
                    .technologies
                    .iter()
                    .any(|pt| pt.eq_ignore_ascii_case(t))
            })
            .collect();
        if uses_pattern || !shared.is_empty() {
            let overall = similar
                .similarity
                .as_ref()
                .map(|s| s.overall)
                .unwrap_or_default();
            evidence.push(EvidenceItem::new(
                EvidenceKind::SimilarProject,
                &other.id,
                &other.name,
                if uses_pattern {
                    format!(
                        "already uses this pattern ({:.0}% similar)",
                        overall * 100.0
                    )
                } else {
                    format!(
                        "{:.0}% similar, shares {}",
                        overall * 100.0,
                        shared
                            .iter()
                            .map(|s| s.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                },
            ));
        }
    }

    let gated: Vec<String> = pattern
        .plan
        .iter()
        .filter(|s| policy::lookup(&s.action_type).gate.needs_human())
        .map(|s| s.key.clone())
        .collect();
    evidence.push(EvidenceItem::new(
        EvidenceKind::Policy,
        "",
        "Policy",
        match (matches.is_empty(), gated.is_empty()) {
            (true, true) => "no active decision applies; every step runs automatically".to_owned(),
            (true, false) => format!(
                "no active decision applies; approval needed for: {}",
                gated.join(", ")
            ),
            (false, true) => format!(
                "{} decision tag(s) checked; every step runs automatically",
                matches.len()
            ),
            (false, false) => format!(
                "{} decision tag(s) checked; approval needed for: {}",
                matches.len(),
                gated.join(", ")
            ),
        },
    ));

    for constraint in input.constraints {
        let (kind, term) = constraint.split();
        let mentions = pattern_mentions(pattern, term);
        match kind {
            "require" if !mentions => {
                unmet.push(format!("constraint: {}", constraint.description));
                score.overall = (score.overall - CONSTRAINT_PENALTY).max(0.0);
            }
            "prefer" if mentions => {
                score.overall = (score.overall + PREFERENCE_BONUS).min(1.0);
                evidence.push(EvidenceItem::new(
                    EvidenceKind::Answer,
                    &constraint.source_message_id,
                    "Preference",
                    format!("you asked for {term}; this option matches"),
                ));
            }
            _ => {}
        }
    }

    RankedOption {
        id: option_id(&pattern.id),
        title: pattern.name.clone(),
        summary: pattern.summary.clone(),
        pattern_id: pattern.id.clone(),
        rank: 0,
        score,
        pros: pattern.pros.clone(),
        cons: pattern.cons.clone(),
        evidence,
        estimated_monthly_cost_usd: pattern.estimated_monthly_cost_usd,
        unmet_requirements: unmet,
        plan: instantiate_plan(pattern, project),
        tags: pattern_tags(pattern),
    }
}

#[cfg(test)]
pub mod fixtures {
    use sdi_proto::{
        common::v1::{KnowledgeScope, PlanStep},
        intelligence::v1::{Decision, DecisionScope, DecisionSource, DecisionStatus},
        knowledge::v1::{
            Fact, FactCategory, FactKind, Pattern, PatternSource, PatternStatus, PipelineStats,
            Project, ProjectContext,
        },
    };

    pub fn pattern(id: &str, cloud: &str, runtime: &str, requires: &[&str], cost: f64) -> Pattern {
        Pattern {
            id: id.to_owned(),
            name: id.replace('-', " "),
            summary: format!("{id} deployment pattern"),
            category: "deployment".to_owned(),
            technologies: vec!["Docker".to_owned()],
            cloud: cloud.to_owned(),
            runtime: runtime.to_owned(),
            iac: String::new(),
            delivery: "GitHub Actions".to_owned(),
            requires: requires.iter().map(|r| (*r).to_owned()).collect(),
            scope: KnowledgeScope::PublicTemplate as i32,
            source: PatternSource::Catalog as i32,
            status: PatternStatus::Active as i32,
            used_by: vec![],
            deployments: 0,
            success_rate: 0.0,
            incidents: 0,
            last_used_at: None,
            estimated_monthly_cost_usd: cost,
            security_score: 0.7,
            complexity: 2,
            pros: vec!["simple".to_owned()],
            cons: vec!["manual scaling".to_owned()],
            plan: vec![
                PlanStep {
                    key: "build".to_owned(),
                    title: "Build image {{repo}}".to_owned(),
                    description: String::new(),
                    action_type: "container.build".to_owned(),
                    adapter: "docker".to_owned(),
                    depends_on: vec![],
                    required_permissions: vec!["container:build".to_owned()],
                    params: Default::default(),
                    verification: "image exists".to_owned(),
                },
                PlanStep {
                    key: "deploy".to_owned(),
                    title: "Deploy to staging".to_owned(),
                    description: String::new(),
                    action_type: "deploy.staging".to_owned(),
                    adapter: "cloud".to_owned(),
                    depends_on: vec!["build".to_owned()],
                    required_permissions: vec!["deploy:staging".to_owned()],
                    params: Default::default(),
                    verification: "health check passes".to_owned(),
                },
            ],
            created_at: None,
        }
    }

    pub fn catalog() -> Vec<Pattern> {
        vec![
            pattern(
                "vm-docker-compose",
                "any",
                "vm_docker_compose",
                &["dockerfile"],
                20.0,
            ),
            pattern(
                "azure-container-apps",
                "azure",
                "serverless_container",
                &["dockerfile", "ci:github_actions"],
                60.0,
            ),
            pattern(
                "aws-ecs-fargate",
                "aws",
                "serverless_container",
                &["dockerfile"],
                90.0,
            ),
            pattern(
                "kubernetes-helm",
                "any",
                "kubernetes",
                &["dockerfile", "k8s:manifests"],
                250.0,
            ),
        ]
    }

    pub fn fact(id: &str, key: &str, label: &str, path: &str) -> Fact {
        Fact {
            id: id.to_owned(),
            project_id: "proj-1".to_owned(),
            kind: FactKind::Detected as i32,
            category: FactCategory::Container as i32,
            key: key.to_owned(),
            label: label.to_owned(),
            value: String::new(),
            confidence: 1.0,
            source_path: path.to_owned(),
            evidence: String::new(),
            detected_at: None,
        }
    }

    pub fn context() -> ProjectContext {
        ProjectContext {
            project: Some(Project {
                id: "proj-1".to_owned(),
                name: "view-engine".to_owned(),
                repo_full_name: "f8fvwgzc/view-engine".to_owned(),
                default_branch: "main".to_owned(),
                primary_language: "TypeScript".to_owned(),
                ..Default::default()
            }),
            understanding: None,
            facts: vec![
                fact("f-docker", "dockerfile", "Dockerfile", "Dockerfile"),
                fact(
                    "f-gha",
                    "ci:github_actions",
                    "GitHub Actions",
                    ".github/workflows/ci.yml",
                ),
            ],
            questions: vec![],
            graph: None,
            dependabot: None,
            pipeline_stats: Some(PipelineStats {
                total: 12,
                succeeded: 10,
                failed: 2,
                success_rate: 0.83,
                avg_duration_seconds: 140,
            }),
            deployment_stats: None,
            technologies: vec!["TypeScript".to_owned(), "Docker".to_owned()],
            capabilities: vec![
                "lang:typescript".to_owned(),
                "dockerfile".to_owned(),
                "ci:github_actions".to_owned(),
            ],
            profile_text: "TypeScript web service with a Dockerfile and GitHub Actions CI"
                .to_owned(),
        }
    }

    pub fn decision(id: &str, scope: DecisionScope, tags: &[&str]) -> Decision {
        Decision {
            id: id.to_owned(),
            project_id: String::new(),
            scope: scope as i32,
            status: DecisionStatus::Active as i32,
            source: DecisionSource::Manual as i32,
            title: format!("decision {id}"),
            statement: String::new(),
            reasons: vec![],
            approved_by: String::new(),
            proposal_id: String::new(),
            tags: tags.iter().map(|t| (*t).to_owned()).collect(),
            created_at: None,
            superseded_at: None,
            superseded_by: String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{fixtures::*, *};
    use sdi_proto::intelligence::v1::DecisionScope;

    fn input<'a>(
        ctx: &'a ProjectContext,
        patterns: &'a [Pattern],
        decisions: &'a [Decision],
        constraints: &'a [ConstraintData],
        weights: &'a Weights,
    ) -> RankingInput<'a> {
        RankingInput {
            context: ctx,
            patterns,
            decisions,
            similar: &[],
            constraints,
            weights,
            now: Utc::now(),
        }
    }

    #[test]
    fn docker_plus_actions_ranks_container_options_above_kubernetes() {
        let ctx = context();
        let patterns = catalog();
        let weights = Weights::default();
        let options = rank(&input(&ctx, &patterns, &[], &[], &weights));

        let position = |id: &str| options.iter().position(|o| o.pattern_id == id).unwrap();
        let k8s = position("kubernetes-helm");
        assert!(position("azure-container-apps") < k8s || position("vm-docker-compose") < k8s);
        assert_eq!(options[k8s].unmet_requirements, vec!["k8s:manifests"]);
        assert_eq!(options[0].rank, 1);
        assert!(
            options
                .iter()
                .all(|o| o.score.overall > 0.0 && o.score.overall <= 1.0)
        );
    }

    #[test]
    fn aws_organisation_decision_zeroes_azure_policy() {
        let ctx = context();
        let patterns = catalog();
        let decisions = vec![decision(
            "d-aws",
            DecisionScope::Organization,
            &["cloud:aws"],
        )];
        let weights = Weights::default();
        let options = rank(&input(&ctx, &patterns, &decisions, &[], &weights));

        let by_id = |id: &str| options.iter().find(|o| o.pattern_id == id).unwrap();
        assert_eq!(by_id("azure-container-apps").score.policy, 0.0);
        assert_eq!(by_id("aws-ecs-fargate").score.policy, 1.0);
        assert_eq!(by_id("vm-docker-compose").score.policy, NO_DECISION_POLICY);
        assert!(
            by_id("azure-container-apps")
                .evidence
                .iter()
                .any(|e| e.kind == EvidenceKind::Decision && e.detail.contains("contradicts"))
        );
    }

    #[test]
    fn evidence_cites_facts_history_and_policy() {
        let ctx = context();
        let patterns = catalog();
        let weights = Weights::default();
        let options = rank(&input(&ctx, &patterns, &[], &[], &weights));
        let aca = options
            .iter()
            .find(|o| o.pattern_id == "azure-container-apps")
            .unwrap();

        assert!(
            aca.evidence
                .iter()
                .any(|e| e.kind == EvidenceKind::Fact && e.ref_id == "f-docker")
        );
        assert!(
            aca.evidence
                .iter()
                .any(|e| e.kind == EvidenceKind::DeliveryHistory)
        );
        assert!(
            aca.evidence
                .iter()
                .any(|e| e.kind == EvidenceKind::Policy && e.detail.contains("deploy"))
        );
        assert_eq!(
            aca.plan[0].params.get("image").map(String::as_str),
            Some("view-engine")
        );
        assert_eq!(aca.plan[0].title, "Build image view-engine");
        assert_eq!(
            aca.tags,
            vec!["cloud:azure", "runtime:serverless_container"]
        );
    }

    #[test]
    fn constraints_exclude_prefer_and_require() {
        let ctx = context();
        let patterns = catalog();
        let weights = Weights::default();
        let constraints = vec![
            ConstraintData {
                key: "exclude:kubernetes".into(),
                description: "not kubernetes".into(),
                source_message_id: String::new(),
            },
            ConstraintData {
                key: "prefer:aws".into(),
                description: "use aws".into(),
                source_message_id: String::new(),
            },
            ConstraintData {
                key: "require:private networking".into(),
                description: "need private networking".into(),
                source_message_id: String::new(),
            },
        ];
        let options = rank(&input(&ctx, &patterns, &[], &constraints, &weights));

        assert!(options.iter().all(|o| o.pattern_id != "kubernetes-helm"));
        assert_eq!(options[0].pattern_id, "aws-ecs-fargate");
        assert!(options.iter().all(|o| {
            o.unmet_requirements
                .iter()
                .any(|u| u.contains("private networking"))
        }));
    }

    #[test]
    fn signal_formulas_follow_the_contract() {
        let mut p = pattern("x", "any", "paas", &[], 250.0);
        assert!((cost_signal(250.0) - 0.5).abs() < 1e-9);
        assert_eq!(cost_signal(5000.0), 0.0);

        p.deployments = 2;
        p.success_rate = 1.0;
        assert!((success_signal(&p) - 0.82).abs() < 1e-9);
        p.deployments = 10;
        assert_eq!(success_signal(&p), 1.0);

        assert_eq!(recency_signal(&p, Utc::now()), CATALOG_RECENCY);
        p.source = PatternSource::Learned as i32;
        p.last_used_at = Some(sdi_common::time::to_proto(
            Utc::now() - chrono::Duration::days(10),
        ));
        assert_eq!(recency_signal(&p, Utc::now()), 1.0);
        p.last_used_at = Some(sdi_common::time::to_proto(
            Utc::now() - chrono::Duration::days(800),
        ));
        assert_eq!(recency_signal(&p, Utc::now()), 0.3);

        let (fit, unmet) = architecture_fit(&["a".into(), "b".into()], &["a".into()]);
        assert!((fit - 0.4).abs() < 1e-9);
        assert_eq!(unmet, vec!["b"]);
    }

    #[test]
    fn private_patterns_are_only_visible_to_their_project() {
        let ctx = context();
        let mut p = pattern("private-one", "any", "paas", &[], 0.0);
        p.scope = KnowledgeScope::Private as i32;
        assert!(!visible(&p, ctx.project.as_ref()));
        p.used_by = vec!["view-engine".to_owned()];
        assert!(visible(&p, ctx.project.as_ref()));
    }

    #[test]
    fn kinds_map_to_categories() {
        assert_eq!(category_for(ProposalKind::CiCd), Some("ci_cd"));
        assert_eq!(category_for(ProposalKind::Unspecified), None);
    }
}

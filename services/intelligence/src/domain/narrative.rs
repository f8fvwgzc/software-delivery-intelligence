//! Assistant text templated from structured data. Every sentence points at a
//! score, an effect or an `Evidence`; nothing here is invented.

use serde::Serialize;

use super::{
    model::{EvidenceItem, RankedOption, Signal, Weights},
    negotiation::{DecisionDraft, Effect, Question},
};

#[derive(Debug, Clone, PartialEq)]
pub struct ReplyDraft {
    pub text: String,
    pub references: Vec<EvidenceItem>,
    pub changed_recommendation: bool,
}

/// Everything the reply (and the optional LLM rewrite) may talk about.
#[derive(Debug, Serialize)]
pub struct ReplyContext<'a> {
    pub user_message: &'a str,
    pub effects: Vec<String>,
    pub recorded_decision: Option<&'a DecisionDraft>,
    pub weights_before: &'a Weights,
    pub weights_after: &'a Weights,
    pub before: &'a [RankedOption],
    pub after: &'a [RankedOption],
}

fn fmt_score(option: &RankedOption) -> String {
    format!(
        "{} ({:.2}: similarity {:.2}, policy {:.2}, recency {:.2}, success {:.2}, fit {:.2}, cost {:.2}, security {:.2})",
        option.title,
        option.score.overall,
        option.score.semantic_similarity,
        option.score.policy,
        option.score.recency,
        option.score.success,
        option.score.architecture_fit,
        option.score.cost,
        option.score.security
    )
}

fn evidence_line(e: &EvidenceItem) -> String {
    let kind = format!("{:?}", e.kind).to_lowercase();
    if e.detail.is_empty() {
        format!("[{kind}] {}", e.label)
    } else {
        format!("[{kind}] {}: {}", e.label, e.detail)
    }
}

pub fn describe_effect(effect: &Effect, before: &Weights, after: &Weights) -> String {
    match effect {
        Effect::WeightShift {
            signal, trigger, ..
        } => format!(
            "\"{trigger}\" raised the {} weight from {:.2} to {:.2} (other weights renormalised).",
            signal.name(),
            before.get(*signal),
            after.get(*signal)
        ),
        Effect::Constraint { key, description } => match key.split_once(':') {
            Some(("exclude", term)) => {
                format!("Excluded options mentioning {term} ({description}).")
            }
            Some(("prefer", term)) => {
                format!("Preferred options mentioning {term} ({description}).")
            }
            _ => format!(
                "Added constraint \"{description}\"; options that do not satisfy it are penalised."
            ),
        },
        Effect::Decision(d) => format!(
            "Recorded decision \"{}\" (organization scope, tags: {}); the policy signal was recomputed.",
            d.title,
            if d.tags.is_empty() {
                "none".to_owned()
            } else {
                d.tags.join(", ")
            }
        ),
        Effect::Question(Question::Why { subject }) => match subject {
            Some(s) => format!("You asked why {s}."),
            None => "You asked why the recommendation stands.".to_owned(),
        },
        Effect::Question(Question::WhatIf { subject }) => {
            format!("You asked what happens if {subject}.")
        }
        Effect::Question(Question::Compare) => "You asked for a comparison.".to_owned(),
        Effect::Budget(value) => format!(
            "Set the monthly budget to ${}; options above it lose 0.25 and are marked budget:{}.",
            super::model::fmt_usd(*value),
            super::model::fmt_usd(*value)
        ),
        Effect::Param { name, value } => {
            format!("Set {name} to {value}; generated files were regenerated with it.")
        }
    }
}

fn find_subject<'a>(options: &'a [RankedOption], subject: &str) -> Option<&'a RankedOption> {
    let subject = subject.to_lowercase();
    let tags = super::negotiation::technology_tags(&subject);
    options.iter().find(|o| {
        let text = format!("{} {} {}", o.title, o.summary, o.pattern_id).to_lowercase();
        text.contains(&subject) || (!tags.is_empty() && tags.iter().all(|t| o.tags.contains(t)))
    })
}

pub fn compose_reply(effects: &[Effect], ctx: &ReplyContext<'_>) -> ReplyDraft {
    let before_top = ctx.before.first();
    let after_top = ctx.after.first();
    let changed = match (before_top, after_top) {
        (Some(b), Some(a)) => b.id != a.id,
        _ => false,
    };

    let mut lines: Vec<String> = Vec::new();
    let mut references: Vec<EvidenceItem> = Vec::new();

    let state_changes: Vec<&Effect> = effects
        .iter()
        .filter(|e| !matches!(e, Effect::Question(_)))
        .collect();
    if state_changes.is_empty() && effects.is_empty() {
        lines.push("I could not map that to a constraint, a weight or a decision, so the ranking is unchanged. Try \"cheaper\", \"not kubernetes\", \"we need private networking\" or \"why <option>?\".".to_owned());
    }
    if !state_changes.is_empty() {
        lines.push("What changed:".to_owned());
        for effect in &state_changes {
            lines.push(format!(
                "- {}",
                describe_effect(effect, ctx.weights_before, ctx.weights_after)
            ));
        }
    }

    match (before_top, after_top) {
        (Some(b), Some(a)) if changed => {
            lines.push(format!(
                "Recommendation changed: {} -> {}. Before: {}. After: {}.",
                b.title,
                a.title,
                fmt_score(b),
                fmt_score(a)
            ));
        }
        (Some(b), Some(a)) => {
            lines.push(format!(
                "Recommendation unchanged: {} (overall {:.2} before, {:.2} now).",
                a.title, b.score.overall, a.score.overall
            ));
        }
        (_, Some(a)) => lines.push(format!("Recommendation: {}.", fmt_score(a))),
        _ => lines.push(
            "No option is left after applying the constraints; relax one to see options again."
                .to_owned(),
        ),
    }

    for effect in effects {
        let Effect::Question(question) = effect else {
            continue;
        };
        match question {
            Question::Why { subject } => {
                let target = subject
                    .as_deref()
                    .and_then(|s| find_subject(ctx.after, s))
                    .or(after_top);
                match target {
                    Some(option) => {
                        lines.push(format!("Why {}: {}", option.title, fmt_score(option)));
                        for e in &option.evidence {
                            lines.push(format!("- {}", evidence_line(e)));
                        }
                        if !option.unmet_requirements.is_empty() {
                            lines.push(format!("- unmet: {}", option.unmet_requirements.join(", ")));
                        }
                        references.extend(option.evidence.iter().cloned());
                    }
                    None => lines.push(format!(
                        "No option matches \"{}\" among the current candidates.",
                        subject.clone().unwrap_or_default()
                    )),
                }
            }
            Question::WhatIf { subject } => match find_subject(ctx.after, subject) {
                Some(option) => lines.push(format!(
                    "If you went with {}: {}; unmet requirements: {}.",
                    option.title,
                    fmt_score(option),
                    if option.unmet_requirements.is_empty() {
                        "none".to_owned()
                    } else {
                        option.unmet_requirements.join(", ")
                    }
                )),
                None => lines.push(format!(
                    "\"{subject}\" does not match a candidate; state it as a preference (\"use X\") or a constraint (\"we need X\") to re-rank."
                )),
            },
            Question::Compare => {
                lines.push("Comparison:".to_owned());
                for option in ctx.after {
                    lines.push(format!(
                        "{}. {} - pros: {}; cons: {}; est. ${:.0}/month",
                        option.rank,
                        fmt_score(option),
                        option.pros.join(", "),
                        option.cons.join(", "),
                        option.estimated_monthly_cost_usd
                    ));
                }
            }
        }
    }

    if !ctx.after.is_empty()
        && !effects
            .iter()
            .any(|e| matches!(e, Effect::Question(Question::Compare)))
    {
        lines.push("Current ranking:".to_owned());
        for option in ctx.after {
            lines.push(format!("{}. {}", option.rank, fmt_score(option)));
        }
    }

    if let Some(top) = after_top {
        if references.is_empty() {
            references.extend(top.evidence.iter().cloned());
        }
        lines.push("Evidence for the recommendation:".to_owned());
        for e in &top.evidence {
            lines.push(format!("- {}", evidence_line(e)));
        }
    }

    ReplyDraft {
        text: lines.join("\n"),
        references,
        changed_recommendation: changed,
    }
}

/// Proposal title and summary straight from the ranked options.
pub fn describe_proposal(
    kind_label: &str,
    project_name: &str,
    options: &[RankedOption],
) -> (String, String) {
    let title = format!("{kind_label} proposal for {project_name}");
    let summary = match options.first() {
        Some(top) => {
            let unmet = if top.unmet_requirements.is_empty() {
                String::new()
            } else {
                format!(" Unmet: {}.", top.unmet_requirements.join(", "))
            };
            format!(
                "Recommended: {} (overall {:.2}) out of {} candidate(s). {}{}",
                top.title,
                top.score.overall,
                options.len(),
                top.summary,
                unmet
            )
        }
        None => "No pattern of this kind is visible to the project yet.".to_owned(),
    };
    (title, summary)
}

/// Markdown for a system message that opens a proposal thread.
pub fn opening_message(options: &[RankedOption], weights: &Weights) -> String {
    let mut lines = vec![format!(
        "Ranked {} option(s) with weights {}.",
        options.len(),
        Signal::ALL
            .iter()
            .map(|s| format!("{} {:.2}", s.name(), weights.get(*s)))
            .collect::<Vec<_>>()
            .join(", ")
    )];
    for option in options {
        lines.push(format!("{}. {}", option.rank, fmt_score(option)));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{model::Breakdown, negotiation};

    fn option(id: &str, overall: f64) -> RankedOption {
        RankedOption {
            id: format!("opt-{id}"),
            title: id.to_owned(),
            summary: String::new(),
            pattern_id: id.to_owned(),
            rank: 1,
            score: Breakdown {
                overall,
                ..Default::default()
            },
            pros: vec!["p".into()],
            cons: vec!["c".into()],
            evidence: vec![EvidenceItem::new(
                super::super::model::EvidenceKind::Fact,
                "f1",
                "Dockerfile",
                "satisfies dockerfile",
            )],
            estimated_monthly_cost_usd: 10.0,
            unmet_requirements: vec![],
            plan: vec![],
            tags: negotiation::technology_tags(id),
            artifacts: vec![],
            cost_breakdown: vec![],
        }
    }

    #[test]
    fn reply_states_changes_and_switch() {
        let effects = negotiation::parse("cheaper");
        let before = vec![
            option("azure-container-apps", 0.7),
            option("aws-ecs-fargate", 0.6),
        ];
        let after = vec![
            option("aws-ecs-fargate", 0.75),
            option("azure-container-apps", 0.7),
        ];
        let mut after_w = Weights::default();
        after_w.shift(Signal::Cost, 0.15);
        let ctx = ReplyContext {
            user_message: "cheaper",
            effects: vec![],
            recorded_decision: None,
            weights_before: &Weights::default(),
            weights_after: &after_w,
            before: &before,
            after: &after,
        };
        let reply = compose_reply(&effects, &ctx);
        assert!(reply.changed_recommendation);
        assert!(reply.text.contains("raised the cost weight"));
        assert!(
            reply
                .text
                .contains("Recommendation changed: azure-container-apps -> aws-ecs-fargate")
        );
        assert!(reply.text.contains("0.70") && reply.text.contains("0.75"));
        assert_eq!(reply.references[0].ref_id, "f1");
    }

    #[test]
    fn why_answers_with_evidence_of_the_subject() {
        let effects = negotiation::parse("why aws?");
        let after = vec![
            option("azure-container-apps", 0.7),
            option("aws-ecs-fargate", 0.6),
        ];
        let ctx = ReplyContext {
            user_message: "why aws?",
            effects: vec![],
            recorded_decision: None,
            weights_before: &Weights::default(),
            weights_after: &Weights::default(),
            before: &after,
            after: &after,
        };
        let reply = compose_reply(&effects, &ctx);
        assert!(!reply.changed_recommendation);
        assert!(reply.text.contains("Why aws-ecs-fargate"));
        assert!(reply.text.contains("[fact] Dockerfile"));
        assert!(reply.text.contains("Recommendation unchanged"));
    }
}

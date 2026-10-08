//! Deterministic intent parsing of one negotiation turn. No model involved:
//! the same sentence always yields the same effects, so re-ranking stays
//! reproducible (CONTRACT.md §6).

use serde::Serialize;

use super::model::{ConstraintData, Signal, Weights};

const WEIGHT_STEP: f64 = 0.15;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DecisionDraft {
    pub title: String,
    pub statement: String,
    pub reasons: Vec<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Question {
    /// "why aks?" - explain the evidence behind one option (or the recommendation).
    Why {
        subject: Option<String>,
    },
    WhatIf {
        subject: String,
    },
    Compare,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    WeightShift {
        signal: Signal,
        delta: f64,
        trigger: String,
    },
    /// `require:` / `prefer:` / `exclude:` constraint.
    Constraint {
        key: String,
        description: String,
    },
    Decision(DecisionDraft),
    Question(Question),
}

impl Effect {
    fn constraint(kind: &str, term: &str, description: &str) -> Self {
        Effect::Constraint {
            key: format!("{kind}:{}", term.trim()),
            description: description.trim().to_owned(),
        }
    }
}

const STOP_WORDS: &[&str] = &[
    " instead", " because", " but ", " and ", " please", " rather", " since", " as ", " so ",
    " for ",
];

/// Text after the first matching phrase, cut at punctuation or a connective.
fn after_phrase<'a>(text: &'a str, phrases: &[&str]) -> Option<(String, &'a str)> {
    let mut best: Option<(usize, usize)> = None;
    for phrase in phrases {
        if let Some(start) = text.find(phrase) {
            let bounded = start == 0 || !text.as_bytes()[start - 1].is_ascii_alphanumeric();
            if bounded && best.is_none_or(|(s, _)| start < s) {
                best = Some((start, start + phrase.len()));
            }
        }
    }
    let (_, end) = best?;
    let rest = &text[end..];
    let mut cut = rest.len();
    for stop in [",", ".", "?", "!", ";", ":"] {
        if let Some(i) = rest.find(stop) {
            cut = cut.min(i);
        }
    }
    for stop in STOP_WORDS {
        if let Some(i) = rest.find(stop) {
            cut = cut.min(i);
        }
    }
    let term = rest[..cut]
        .trim()
        .trim_matches(|c: char| !c.is_alphanumeric() && c != ' ')
        .to_owned();
    if term.is_empty() {
        None
    } else {
        Some((term, rest))
    }
}

/// Machine tags for a technology mentioned in free text.
pub fn technology_tags(term: &str) -> Vec<String> {
    let t = term.to_lowercase();
    let mut tags: Vec<&str> = Vec::new();
    let mut push = |items: &[&'static str]| {
        for item in items {
            if !tags.contains(item) {
                tags.push(item);
            }
        }
    };
    let has = |needle: &str| t.contains(needle);

    if has("aks") {
        push(&["cloud:azure", "runtime:kubernetes"]);
    }
    if has("eks") {
        push(&["cloud:aws", "runtime:kubernetes"]);
    }
    if has("gke") {
        push(&["cloud:gcp", "runtime:kubernetes"]);
    }
    if has("container apps") {
        push(&["cloud:azure", "runtime:serverless_container"]);
    }
    if has("cloud run") {
        push(&["cloud:gcp", "runtime:serverless_container"]);
    }
    if has("fargate") || has("ecs") {
        push(&["cloud:aws", "runtime:serverless_container"]);
    }
    if has("aws") || has("amazon") {
        push(&["cloud:aws"]);
    }
    if has("azure") {
        push(&["cloud:azure"]);
    }
    if has("gcp") || has("google") {
        push(&["cloud:gcp"]);
    }
    if has("kubernetes") || has("k8s") || has("helm") {
        push(&["runtime:kubernetes"]);
    }
    if has("compose") || has("virtual machine") || t.split_whitespace().any(|w| w == "vm") {
        push(&["runtime:vm_docker_compose"]);
    }
    if has("fly.io") || has("fly ") || t == "fly" {
        push(&["runtime:paas"]);
    }
    if has("github pages") || has("static") {
        push(&["runtime:static_hosting"]);
    }
    if has("terraform") {
        push(&["iac:terraform"]);
    }
    if has("bicep") {
        push(&["iac:bicep"]);
    }
    if has("pulumi") {
        push(&["iac:pulumi"]);
    }
    if has("cloudformation") {
        push(&["iac:cloudformation"]);
    }
    if has("github actions") || has("github workflows") || t == "gha" {
        push(&["ci:github_actions"]);
    }
    if has("gitlab") {
        push(&["ci:gitlab_ci"]);
    }
    tags.into_iter().map(str::to_owned).collect()
}

fn weight_triggers(text: &str) -> Vec<(Signal, &'static str)> {
    const TABLE: &[(&str, Signal)] = &[
        ("cheaper", Signal::Cost),
        ("cheap", Signal::Cost),
        ("cost", Signal::Cost),
        ("budget", Signal::Cost),
        ("expensive", Signal::Cost),
        ("more secure", Signal::Security),
        ("secure", Signal::Security),
        ("security", Signal::Security),
        ("hardened", Signal::Security),
        ("compliance", Signal::Security),
        ("faster", Signal::ArchitectureFit),
        ("simpler", Signal::ArchitectureFit),
        ("simple", Signal::ArchitectureFit),
        ("quick", Signal::ArchitectureFit),
        ("lightweight", Signal::ArchitectureFit),
        ("proven", Signal::Success),
        ("reliable", Signal::Success),
        ("stable", Signal::Success),
        ("battle-tested", Signal::Success),
    ];
    let mut out: Vec<(Signal, &'static str)> = Vec::new();
    for (word, signal) in TABLE {
        if text.contains(word) && !out.iter().any(|(s, _)| s == signal) {
            out.push((*signal, word));
        }
    }
    out
}

pub fn parse(content: &str) -> Vec<Effect> {
    let text = content.trim().to_lowercase();
    let mut effects = Vec::new();
    if text.is_empty() {
        return effects;
    }

    // Standing decisions first: "we standardise on X because Y".
    let mut decision_subject: Option<String> = None;
    if let Some((subject, rest)) = after_phrase(
        &text,
        &[
            "we standardise on ",
            "we standardize on ",
            "standardise on ",
            "standardize on ",
            "we always use ",
            "always use ",
        ],
    )
    .or_else(|| after_phrase(&text, &["we prefer "]).filter(|(_, rest)| rest.contains("because")))
    {
        let reasons = rest
            .split_once("because")
            .map(|(_, why)| why.trim().trim_end_matches(['.', '!']).to_owned())
            .filter(|why| !why.is_empty())
            .into_iter()
            .collect();
        effects.push(Effect::Decision(DecisionDraft {
            title: format!("Standardise on {subject}"),
            statement: content.trim().to_owned(),
            reasons,
            tags: technology_tags(&subject),
        }));
        decision_subject = Some(subject);
    }

    // Questions never change state on their own.
    if let Some((subject, _)) = after_phrase(&text, &["why "]) {
        let subject = subject
            .trim_start_matches("not ")
            .trim_start_matches("is ")
            .to_owned();
        effects.push(Effect::Question(Question::Why {
            subject: (!matches!(subject.as_str(), "this" | "that" | "it")).then_some(subject),
        }));
    } else if text.starts_with("why") {
        effects.push(Effect::Question(Question::Why { subject: None }));
    }
    if let Some((subject, _)) = after_phrase(&text, &["what if "]) {
        effects.push(Effect::Question(Question::WhatIf { subject }));
    }
    if text.contains("compare") || text.contains(" vs ") || text.contains(" versus ") {
        effects.push(Effect::Question(Question::Compare));
    }
    let is_question = effects.iter().any(|e| matches!(e, Effect::Question(_)));

    for (signal, trigger) in weight_triggers(&text) {
        effects.push(Effect::WeightShift {
            signal,
            delta: WEIGHT_STEP,
            trigger: trigger.to_owned(),
        });
    }

    if is_question {
        return effects;
    }

    if let Some((term, _)) = after_phrase(
        &text,
        &[
            "not ",
            "no ",
            "avoid ",
            "without ",
            "never ",
            "don't want ",
            "do not want ",
            "cheaper than ",
            "other than ",
            "instead of ",
            "rather than ",
        ],
    ) && technology_tags(&term).len() + usize::from(term.split_whitespace().count() <= 3) > 0
        && decision_subject.as_deref() != Some(term.as_str())
    {
        effects.push(Effect::constraint("exclude", &term, &format!("not {term}")));
    }

    if let Some((term, _)) = after_phrase(
        &text,
        &[
            "switch to ",
            "go with ",
            "use ",
            "what about ",
            "how about ",
            "rather have ",
            "prefer ",
        ],
    ) && decision_subject.is_none()
        && weight_triggers(&term).is_empty()
        && !effects.iter().any(
            |e| matches!(e, Effect::Constraint { key, .. } if key == &format!("exclude:{term}")),
        )
    {
        effects.push(Effect::constraint("prefer", &term, &format!("use {term}")));
    }

    if let Some((term, _)) = after_phrase(
        &text,
        &[
            "keep ",
            "need ",
            "needs ",
            "must have ",
            "must ",
            "require ",
            "has to have ",
        ],
    ) && weight_triggers(&term).is_empty()
        && !term.starts_with("to ")
    {
        effects.push(Effect::constraint(
            "require",
            &term,
            &format!("keep {term}"),
        ));
    }

    effects
}

/// Applies the state-changing effects to a proposal's weights and constraints.
pub fn apply(
    effects: &[Effect],
    weights: &mut Weights,
    constraints: &mut Vec<ConstraintData>,
    message_id: &str,
) {
    for effect in effects {
        match effect {
            Effect::WeightShift { signal, delta, .. } => weights.shift(*signal, *delta),
            Effect::Constraint { key, description } => {
                if let Some(existing) = constraints.iter_mut().find(|c| &c.key == key) {
                    existing.description = description.clone();
                    existing.source_message_id = message_id.to_owned();
                } else {
                    constraints.push(ConstraintData {
                        key: key.clone(),
                        description: description.clone(),
                        source_message_id: message_id.to_owned(),
                    });
                }
            }
            Effect::Decision(_) | Effect::Question(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(effects: &[Effect]) -> Vec<String> {
        effects
            .iter()
            .map(|e| match e {
                Effect::WeightShift { signal, .. } => format!("weight:{}", signal.name()),
                Effect::Constraint { key, .. } => key.clone(),
                Effect::Decision(d) => format!("decision:{}", d.tags.join("+")),
                Effect::Question(Question::Why { subject }) => {
                    format!("why:{}", subject.clone().unwrap_or_default())
                }
                Effect::Question(Question::WhatIf { subject }) => format!("whatif:{subject}"),
                Effect::Question(Question::Compare) => "compare".to_owned(),
            })
            .collect()
    }

    #[test]
    fn parsing_table() {
        let cases: &[(&str, &[&str])] = &[
            ("cheaper please", &["weight:cost"]),
            ("make it more secure", &["weight:security"]),
            ("something faster and simpler", &["weight:architecture_fit"]),
            ("I want something proven and reliable", &["weight:success"]),
            ("use container apps instead", &["prefer:container apps"]),
            ("not kubernetes", &["exclude:kubernetes"]),
            ("why aks?", &["why:aks"]),
            ("why?", &["why:"]),
            ("what if we used aws", &["whatif:we used aws"]),
            ("compare the top two", &["compare"]),
            (
                "we need private networking",
                &["require:private networking"],
            ),
            (
                "keep the current networking",
                &["require:the current networking"],
            ),
            (
                "we standardise on AWS because the platform team supports it",
                &["decision:cloud:aws"],
            ),
            ("We always use Terraform", &["decision:iac:terraform"]),
            (
                "we prefer AKS because of the ops team",
                &["decision:cloud:azure+runtime:kubernetes"],
            ),
            (
                "no kubernetes, we need it cheaper",
                &["weight:cost", "exclude:kubernetes"],
            ),
            (
                "Can we use something cheaper than Kubernetes? We standardise on GitHub Actions because it is already set up.",
                &[
                    "decision:ci:github_actions",
                    "weight:cost",
                    "exclude:kubernetes",
                ],
            ),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(keys(&parse(input)), *expected, "input: {input:?}");
        }
    }

    #[test]
    fn decisions_keep_reasons_and_statement() {
        let effects = parse("we standardise on Azure because our contracts run there.");
        let Effect::Decision(d) = &effects[0] else {
            panic!()
        };
        assert_eq!(d.title, "Standardise on azure");
        assert_eq!(d.reasons, vec!["our contracts run there"]);
        assert_eq!(
            d.statement,
            "we standardise on Azure because our contracts run there."
        );
    }

    #[test]
    fn apply_updates_weights_and_constraints_once() {
        let mut weights = Weights::default();
        let mut constraints = Vec::new();
        let effects = parse("cheaper and not kubernetes");
        apply(&effects, &mut weights, &mut constraints, "m1");
        apply(&effects, &mut weights, &mut constraints, "m2");
        assert_eq!(constraints.len(), 1);
        assert_eq!(constraints[0].source_message_id, "m2");
        assert!(weights.cost > 0.3);
    }

    #[test]
    fn technology_tags_cover_the_vocabulary() {
        assert_eq!(
            technology_tags("AKS"),
            vec!["cloud:azure", "runtime:kubernetes"]
        );
        assert_eq!(
            technology_tags("docker compose on a vm"),
            vec!["runtime:vm_docker_compose"]
        );
        assert_eq!(technology_tags("pulumi"), vec!["iac:pulumi"]);
        assert!(technology_tags("blue").is_empty());
    }
}

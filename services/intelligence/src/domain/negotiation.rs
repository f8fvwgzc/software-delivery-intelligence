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
    /// Monthly budget in USD ("under $15 a month"). Replaces an earlier one.
    Budget(f64),
    /// A generator parameter: `region`, `port` or `runtime`.
    Param {
        name: String,
        value: String,
    },
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

/// Leading number of `rest`, after optional whitespace and a currency sign.
fn leading_number(rest: &str) -> Option<f64> {
    let rest = rest
        .trim_start()
        .trim_start_matches(['$', '€', '£'])
        .trim_start();
    let digits: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == ',')
        .collect();
    let digits = digits.replace(',', "");
    let value: f64 = digits.trim_end_matches('.').parse().ok()?;
    let after = &rest[digits.len().min(rest.len())..];
    // "under 10%" or "max 3 replicas" are not money.
    if after.trim_start().starts_with('%') {
        return None;
    }
    (value > 0.0).then_some(value)
}

fn mentions_money(text: &str) -> bool {
    ["$", "dollar", "usd", "€", "eur", "£", "gbp"]
        .iter()
        .any(|w| text.contains(w))
}

fn mentions_month(text: &str) -> bool {
    text.contains("month") || text.contains("/mo") || text.contains("budget")
}

/// "under $15 a month", "max 20 dollars", "budget 30", "no more than 40 usd",
/// "$25/month". A bare number only counts when money or a month is mentioned.
pub fn parse_budget(text: &str) -> Option<f64> {
    const TRIGGERS: &[&str] = &[
        "no more than ",
        "not more than ",
        "less than ",
        "at most ",
        "budget of ",
        "budget is ",
        "budget: ",
        "budget ",
        "under ",
        "below ",
        "within ",
        "max ",
        "maximum ",
        "capped at ",
        "cap at ",
        "cap of ",
        "up to ",
    ];
    let mut best: Option<(usize, f64)> = None;
    for trigger in TRIGGERS {
        let mut from = 0;
        while let Some(found) = text[from..].find(trigger) {
            let start = from + found;
            let bounded = start == 0 || !text.as_bytes()[start - 1].is_ascii_alphanumeric();
            if bounded && let Some(value) = leading_number(&text[start + trigger.len()..]) {
                // A bare "under 10" is ambiguous; money or a month makes it a budget.
                let explicit =
                    trigger.contains("budget") || mentions_money(text) || mentions_month(text);
                if explicit && best.is_none_or(|(s, _)| start < s) {
                    best = Some((start, value));
                }
            }
            from = start + trigger.len();
        }
    }
    if let Some((_, value)) = best {
        return Some(value);
    }
    // "$25/month", "25 dollars per month", "25 usd monthly"
    if mentions_month(text) {
        for (i, c) in text.char_indices() {
            if c == '$'
                && let Some(value) = leading_number(&text[i..])
            {
                return Some(value);
            }
        }
        for marker in [" dollars", " usd", " dollar"] {
            if let Some(end) = text.find(marker) {
                let before = text[..end].trim_end();
                let start = before
                    .rfind(|c: char| !(c.is_ascii_digit() || c == '.'))
                    .map(|i| i + 1)
                    .unwrap_or(0);
                if let Some(value) = leading_number(&before[start..]) {
                    return Some(value);
                }
            }
        }
    }
    None
}

const RUNTIMES: &[&str] = &[
    "node", "nodejs", "python", "java", "go", "golang", "dotnet", ".net", "ruby", "php", "rust",
];

/// "node 20", "python 3.12", "runtime java 21" -> ("runtime", "node 20").
fn find_runtime(text: &str) -> Option<String> {
    let words: Vec<&str> = text
        .split(|c: char| c.is_whitespace() || c == ',')
        .map(|w| w.trim_matches(|c: char| !(c.is_alphanumeric() || c == '.')))
        .filter(|w| !w.is_empty())
        .collect();
    for pair in words.windows(2) {
        let name = pair[0].to_lowercase();
        let version = pair[1].trim_start_matches('v');
        if RUNTIMES.contains(&name.as_str())
            && version.chars().next().is_some_and(|c| c.is_ascii_digit())
            && version.chars().all(|c| c.is_ascii_digit() || c == '.')
        {
            let name = match name.as_str() {
                "nodejs" => "node",
                "golang" => "go",
                ".net" => "dotnet",
                other => other,
            };
            return Some(format!("{name} {}", version.trim_end_matches('.')));
        }
    }
    None
}

/// Region ("region westeurope", "in west europe"), port ("port 3000") and
/// runtime ("node 20") parameters a turn may change.
pub fn parse_params(text: &str) -> Vec<(String, String)> {
    let mut params = Vec::new();
    if let Some(region) = parse_region(text) {
        params.push(("region".to_owned(), region));
    }
    if let Some((term, _)) = after_phrase(text, &["port to ", "port "]) {
        let digits: String = term.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.parse::<u16>().is_ok_and(|p| p > 0) {
            params.push(("port".to_owned(), digits));
        }
    }
    if let Some(runtime) = find_runtime(text) {
        params.push(("runtime".to_owned(), runtime));
    }
    params
}

const DIRECTIONS: &[&str] = &["west", "east", "north", "south", "central", "uk", "us"];

fn region_token(words: &[&str]) -> Option<String> {
    let region = match words {
        [first, second, ..] if DIRECTIONS.contains(first) => format!("{first}{second}"),
        [first, ..] => (*first).to_owned(),
        [] => return None,
    };
    let valid = region
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-')
        && region.chars().any(|c| c.is_ascii_alphabetic())
        && !matches!(
            region.as_str(),
            "the" | "a" | "in" | "to" | "same" | "another"
        );
    valid.then_some(region)
}

/// "region eastus", "deploy in region eastus", "use west europe region".
fn parse_region(text: &str) -> Option<String> {
    if let Some((term, _)) =
        after_phrase(text, &["region to ", "region ", "deploy in ", "deploy to "])
    {
        // "deploy in region eastus" matches the earlier phrase; drop the noun.
        let term = term
            .trim_start_matches("the ")
            .trim_start_matches("region ")
            .trim_start_matches("to ");
        let words: Vec<&str> = term.split_whitespace().collect();
        if let Some(region) = region_token(&words) {
            return Some(region);
        }
    }
    // "<x> region": the one or two words before the noun.
    let idx = text.find(" region")?;
    let before: Vec<&str> = text[..idx].split_whitespace().collect();
    let tail: Vec<&str> = before.iter().rev().take(2).rev().copied().collect();
    match tail.as_slice() {
        [first, second] if DIRECTIONS.contains(first) => region_token(&[first, second]),
        [.., last] => region_token(&[last]),
        [] => None,
    }
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

    let budget = parse_budget(&text);
    if let Some(value) = budget {
        effects.push(Effect::Budget(value));
    }
    let params = parse_params(&text);
    for (name, value) in &params {
        effects.push(Effect::Param {
            name: name.clone(),
            value: value.clone(),
        });
    }
    let numeric = |term: &str| term.chars().any(|c| c.is_ascii_digit());
    // "use west europe region" is a parameter, not a preference for "west europe".
    let consumed = |term: &str| {
        term.contains("region")
            || params
                .iter()
                .any(|(_, v)| term.replace(' ', "").contains(v.as_str()))
    };

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
        && !(budget.is_some() && numeric(&term))
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
        && !numeric(&term)
        && !consumed(&term)
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
        && !numeric(&term)
        && !consumed(&term)
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
            Effect::Budget(value) => {
                let constraint = ConstraintData {
                    source_message_id: message_id.to_owned(),
                    ..ConstraintData::budget(*value)
                };
                constraints.retain(|c| !c.same_family(&constraint));
                constraints.push(constraint);
            }
            Effect::Param { name, value } => {
                let constraint = ConstraintData {
                    source_message_id: message_id.to_owned(),
                    ..ConstraintData::param(name, value)
                };
                constraints.retain(|c| !c.same_family(&constraint));
                constraints.push(constraint);
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
                Effect::Budget(value) => format!("budget:{value}"),
                Effect::Param { name, value } => format!("param:{name}:{value}"),
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
            ("under $15 a month", &["budget:15"]),
            ("max 20 dollars", &["budget:20"]),
            ("budget 30", &["budget:30", "weight:cost"]),
            ("no more than 40 usd per month please", &["budget:40"]),
            ("keep it below $25/month", &["budget:25"]),
            ("we need it under 12.50 dollars a month", &["budget:12.5"]),
            ("under 10% failure rate", &[]),
            ("deploy in region eastus", &["param:region:eastus"]),
            ("use west europe region", &["param:region:westeurope"]),
            ("run on port 4000", &["param:port:4000"]),
            ("move to node 22", &["param:runtime:node 22"]),
            (
                "python 3.12 please, max $20 a month",
                &["budget:20", "param:runtime:python 3.12"],
            ),
        ];
        for (input, expected) in cases {
            assert_eq!(keys(&parse(input)), *expected, "input: {input:?}");
        }
    }

    #[test]
    fn budget_and_params_replace_their_family() {
        let mut weights = Weights::default();
        let mut constraints = Vec::new();
        apply(
            &parse("under $15 a month"),
            &mut weights,
            &mut constraints,
            "m1",
        );
        apply(
            &parse("ok, max 20 dollars"),
            &mut weights,
            &mut constraints,
            "m2",
        );
        apply(
            &parse("region westeurope"),
            &mut weights,
            &mut constraints,
            "m3",
        );
        apply(
            &parse("region eastus"),
            &mut weights,
            &mut constraints,
            "m4",
        );
        let keys: Vec<&str> = constraints.iter().map(|c| c.key.as_str()).collect();
        assert_eq!(keys, vec!["budget_monthly_usd:20", "param:region:eastus"]);
        assert_eq!(constraints[0].source_message_id, "m2");
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

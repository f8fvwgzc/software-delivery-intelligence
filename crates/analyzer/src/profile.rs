//! One paragraph the project embedding is built from. Dense with the words
//! patterns are described with, so similar projects land close together.

use crate::detectors::Ctx;
use crate::model::*;
use crate::util::display;

pub(crate) fn derive(
    ctx: &Ctx<'_>,
    capabilities: &[String],
    understanding: &UnderstandingDraft,
) -> String {
    let snap = ctx.snap;
    let mut parts: Vec<String> = Vec::new();
    let name = if snap.full_name.is_empty() {
        "This repository".to_string()
    } else {
        snap.full_name.clone()
    };
    let desc = snap.description.trim();
    parts.push(
        if desc.is_empty() {
            format!("{name}.")
        } else {
            format!("{name}: {}", desc.trim_end_matches('.'))
        } + ".",
    );
    parts.push(understanding.tldr.clone());
    let list = |prefix: &str| -> Vec<String> {
        capabilities
            .iter()
            .filter_map(|c| c.strip_prefix(prefix))
            .map(display)
            .collect()
    };
    let groups: &[(&str, &str)] = &[
        ("Languages", "lang:"),
        ("Frameworks", "framework:"),
        ("Databases", "database:"),
        ("Package managers", "package:"),
        ("Cloud", "cloud:"),
    ];
    for (label, prefix) in groups {
        let items = list(prefix);
        if !items.is_empty() {
            parts.push(format!("{label}: {}.", items.join(", ")));
        }
    }
    let tooling: Vec<String> = capabilities
        .iter()
        .filter(|c| {
            !c.contains(':')
                || c.starts_with("ci:")
                || c.starts_with("iac:")
                || c.starts_with("k8s:")
                || c.starts_with("tests:")
                || c.starts_with("docs:")
        })
        .map(|c| c.replace([':', '_'], " "))
        .collect();
    if !tooling.is_empty() {
        parts.push(format!("Practices: {}.", tooling.join(", ")));
    }
    if !snap.topics.is_empty() {
        parts.push(format!("Topics: {}.", snap.topics.join(", ")));
    }
    if !ctx.environments.is_empty() {
        parts.push(format!(
            "Environments: {}.",
            ctx.environments
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    parts.join(" ")
}

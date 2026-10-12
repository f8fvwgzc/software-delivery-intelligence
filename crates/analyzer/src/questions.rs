//! Open questions. Asked only when the repository genuinely cannot answer,
//! and each one says why so the human knows what evidence was missing.

use crate::detectors::Ctx;
use crate::model::*;
use crate::util::display;

pub(crate) fn derive(ctx: &Ctx<'_>) -> Vec<QuestionDraft> {
    let mut out = Vec::new();

    // Deployment target: IaC, a cloud fact, or a deploying pipeline all settle it.
    let deploy_evidence = ctx.has_fact_prefix("iac:")
        || ctx.has_fact_prefix("k8s:")
        || ctx
            .out
            .facts
            .iter()
            .any(|f| f.key.starts_with("cloud:") && f.kind == FactKind::Detected)
        || ctx.has_fact_prefix("deploy:pipeline:")
        || !ctx.deploy_targets.is_empty();
    if !deploy_evidence {
        let reason = if ctx.has_fact_prefix("ci:") || ctx.has_fact_prefix("cd:") {
            "CI runs but no job deploys, and no infrastructure code or cloud configuration names a target."
        } else if ctx.has_fact_prefix("container:dockerfile:") {
            "A Dockerfile exists but nothing shows where the image runs: no CI deploy job, no IaC, no cloud configuration."
        } else {
            "No CI pipeline, infrastructure code or cloud configuration was found, so the deployment target cannot be detected."
        };
        out.push(QuestionDraft {
            key: "deployment_target".into(),
            prompt: "Where should this project be deployed?".into(),
            reason: reason.into(),
            options: [
                "Azure",
                "AWS",
                "GCP",
                "Kubernetes",
                "VM",
                "I don't deploy yet",
            ]
            .map(String::from)
            .to_vec(),
            allow_free_text: true,
            answer: String::new(),
            answered_by: String::new(),
        });
    }

    // Production database: a driver without any infrastructure that provides it.
    let needed = ctx.fact_suffixes("datastore:");
    let needed: Vec<String> = needed
        .into_iter()
        .filter(|e| !e.starts_with("orm:") && !e.starts_with("configured:"))
        .collect();
    let provided = ctx.fact_suffixes("infra:datastore:");
    let missing: Vec<String> = needed
        .iter()
        .filter(|e| !provided.contains(e))
        .filter(|e| e.as_str() != "sqlite")
        .cloned()
        .collect();
    if !missing.is_empty() {
        let names: Vec<String> = missing.iter().map(|m| display(m)).collect();
        let drivers: Vec<String> = missing
            .iter()
            .filter_map(|m| ctx.fact(&format!("datastore:{m}")))
            .map(|f| format!("{} ({})", f.value, f.source_path))
            .collect();
        out.push(QuestionDraft {
            key: "production_database".into(),
            prompt: format!("Where does {} run in production?", names.join(" and ")),
            reason: format!(
                "The code uses {} but no compose file, Kubernetes manifest or IaC resource defines that database.",
                drivers.join(", ")
            ),
            options: [
                "Managed cloud database",
                "Container next to the app",
                "Existing shared server",
                "Not decided yet",
            ]
            .map(String::from)
            .to_vec(),
            allow_free_text: true,
            answer: String::new(),
            answered_by: String::new(),
        });
    }

    // Environments: nothing beyond a local compose stack was named anywhere.
    let real_envs: Vec<&String> = ctx
        .environments
        .iter()
        .filter(|e| e.as_str() != "local")
        .collect();
    if real_envs.is_empty() {
        let reason = if ctx.environments.contains("local") {
            "Only a local Docker Compose environment was found; no staging or production environment is named in CI, IaC or manifests."
        } else {
            "No environment is named in CI jobs, IaC variable files, Kubernetes namespaces or overlays."
        };
        out.push(QuestionDraft {
            key: "environments".into(),
            prompt: "Which environments does this project deploy to?".into(),
            reason: reason.into(),
            options: [
                "Production only",
                "Staging and production",
                "Development, staging and production",
                "None yet",
            ]
            .map(String::from)
            .to_vec(),
            allow_free_text: true,
            answer: String::new(),
            answered_by: String::new(),
        });
    }
    out
}

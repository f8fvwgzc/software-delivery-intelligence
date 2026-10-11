//! `docs/deployment.md` and `docs/runbook.md` written from the option's plan
//! and the default policy table, so the repository carries the same gates
//! the platform enforces. The Documents page reuses these bodies for the
//! deployment guide and runbook documents.

use super::{GenInput, Stack, artifact};
use crate::domain::{
    model::{ArtifactData, PlanStepData},
    policy,
};

pub fn deployment(input: &GenInput) -> Vec<ArtifactData> {
    vec![artifact(
        input,
        "docs/deployment.md",
        "markdown",
        "Deployment guide: the chosen pattern, prerequisites, every plan step with its policy gate and the permissions the pipeline needs.",
        deployment_markdown(input),
    )]
}

pub fn runbook(input: &GenInput) -> Vec<ArtifactData> {
    vec![artifact(
        input,
        "docs/runbook.md",
        "markdown",
        "Runbook: health checks, where the logs are, rollback order and escalation.",
        runbook_markdown(input),
    )]
}

pub fn uat(input: &GenInput) -> Vec<ArtifactData> {
    vec![artifact(
        input,
        "docs/uat.md",
        "markdown",
        "User acceptance checklist derived from the services, datastores and plan steps.",
        uat_markdown(input),
    )]
}

fn table(headers: &[&str], rows: &[Vec<String>]) -> String {
    let mut out = format!("| {} |\n", headers.join(" | "));
    out.push_str(&format!(
        "|{}|\n",
        headers.iter().map(|_| "---").collect::<Vec<_>>().join("|")
    ));
    for row in rows {
        out.push_str(&format!(
            "| {} |\n",
            row.iter()
                .map(|c| c.replace('|', "\\|"))
                .collect::<Vec<_>>()
                .join(" | ")
        ));
    }
    out
}

/// The step table and policy-gate list the deployment guide and the
/// generated `docs/deployment.md` share. Public so the document generator
/// can embed it without duplicating the policy wording.
pub fn plan_sections(plan: &[PlanStepData]) -> String {
    let mut out = String::from("## Steps\n\n");
    if plan.is_empty() {
        out.push_str("Not detected\n\n");
    } else {
        let rows: Vec<Vec<String>> = plan
            .iter()
            .enumerate()
            .map(|(i, step)| {
                let row = policy::lookup(&step.action_type);
                vec![
                    (i + 1).to_string(),
                    step.title.clone(),
                    step.action_type.clone(),
                    row.risk.to_owned(),
                    row.gate.label().to_owned(),
                    step.depends_on.join(", "),
                    step.verification.clone(),
                ]
            })
            .collect();
        out.push_str(&table(
            &[
                "#",
                "Step",
                "Action",
                "Risk",
                "Gate",
                "Depends on",
                "Verification",
            ],
            &rows,
        ));
        out.push('\n');
    }

    out.push_str("## Policy gates\n\n");
    let gated: Vec<&PlanStepData> = plan
        .iter()
        .filter(|s| policy::lookup(&s.action_type).gate.needs_human())
        .collect();
    let blocked: Vec<&PlanStepData> = plan
        .iter()
        .filter(|s| policy::lookup(&s.action_type).gate == policy::Gate::Blocked)
        .collect();
    if gated.is_empty() && blocked.is_empty() {
        out.push_str("Every step runs automatically under the default policy.\n\n");
    } else {
        for step in gated {
            let row = policy::lookup(&step.action_type);
            out.push_str(&format!(
                "- `{}` ({} - {}) stops for human approval: {} gate, {} risk\n",
                step.key,
                step.action_type,
                row.label,
                row.gate.label(),
                row.risk
            ));
        }
        for step in blocked {
            out.push_str(&format!(
                "- `{}` ({}) is blocked by policy and will never run\n",
                step.key, step.action_type
            ));
        }
        out.push('\n');
    }

    out.push_str("## Permissions\n\n");
    let mut permissions: Vec<String> = plan
        .iter()
        .flat_map(|s| s.required_permissions.clone())
        .collect();
    permissions.sort();
    permissions.dedup();
    if permissions.is_empty() {
        out.push_str("Not detected\n");
    } else {
        out.push_str(&format!(
            "The executing identity needs: {}. Anything beyond `repository:read`, `container:build` and `environment:write` must be granted with a TTL first.\n",
            permissions
                .iter()
                .map(|p| format!("`{p}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    out
}

pub fn deployment_markdown(input: &GenInput) -> String {
    let mut out = format!("# {} deployment guide\n\n", input.repo_name);
    if !input.option_title.is_empty() {
        out.push_str("## Pattern\n\n");
        out.push_str(&format!(
            "**{}** - {}\n\n",
            input.option_title, input.option_summary
        ));
    }
    out.push_str("## Prerequisites\n\n");
    out.push_str(&format!(
        "- Runtime: {} {}\n- Container port `{}`, health endpoint `{}`\n- Test command: `{}`\n",
        input.stack.label(),
        input.runtime_version,
        input.port,
        input.health_path,
        input.test_command
    ));
    if !input.cloud.is_empty() {
        out.push_str(&format!("- Cloud: {} in `{}`\n", input.cloud, input.region));
    }
    if !input.datastores.is_empty() {
        out.push_str(&format!("- Datastores: {}\n", input.datastores.join(", ")));
    }
    if !input.env_vars.is_empty() {
        out.push_str(&format!(
            "- Environment variables (see `.env.example`): {}\n",
            input
                .env_vars
                .iter()
                .map(|v| format!("`{v}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    out.push('\n');
    out.push_str(&plan_sections(&input.plan));
    out.push_str("\n## Verify\n\n");
    out.push_str(&format!(
        "1. Build the image: `docker build -t {image}:local .`\n2. Run it: `docker compose up -d`\n3. Check health: `curl -fsS http://localhost:{port}{health}`\n4. Run the tests against the running services: `docker compose --profile test run --rm test`\n",
        image = input.image,
        port = input.port,
        health = input.health_path,
    ));
    out
}

pub fn runbook_markdown(input: &GenInput) -> String {
    let mut out = format!("# {} runbook\n\n## Health\n\n", input.repo_name);
    if input.services.is_empty() {
        out.push_str(&format!(
            "- {}: confirm the process answers `GET {}` on port {} and the container is running\n\n",
            input.image, input.health_path, input.port
        ));
    } else {
        for service in &input.services {
            out.push_str(&format!(
                "- {service}: confirm the process answers its health endpoint and the container is running\n"
            ));
        }
        out.push('\n');
    }

    out.push_str("## Logs\n\n");
    out.push_str("- `docker logs --tail 200 <container>` (single container)\n- `docker compose logs -f app` (Compose)\n");
    match input.cloud.as_str() {
        "azure" => out.push_str(&format!(
            "- `az containerapp logs show --name {} --resource-group rg-{} --follow` (Azure)\n",
            input.image, input.image
        )),
        "aws" => out.push_str(&format!(
            "- `aws logs tail /ecs/{} --follow --region {}` (AWS)\n",
            input.image, input.region
        )),
        "gcp" => out.push_str(&format!(
            "- `gcloud run services logs read {} --region {}` (GCP)\n",
            input.image, input.region
        )),
        _ => {}
    }
    out.push('\n');

    out.push_str("## Rollback\n\n");
    if input.plan.is_empty() {
        out.push_str("Not detected\n\n");
    } else {
        out.push_str("Rollback order is the reverse of the plan; every step is undone from its last checkpoint:\n\n");
        for step in input.plan.iter().rev() {
            out.push_str(&format!("- Undo `{}` ({})\n", step.key, step.action_type));
        }
        out.push('\n');
    }
    out.push_str("Redeploying the previous image tag is always safe: the deploy workflow accepts any tag the registry still holds.\n\n");

    out.push_str("## Escalation\n\n");
    out.push_str("Every failure pauses the execution, blocks dependents and waits for a human to pick a remediation (CONTRACT.md §7).\n");
    out
}

fn uat_markdown(input: &GenInput) -> String {
    let mut out = format!(
        "# {} user acceptance checklist\n\n## Services\n\n",
        input.repo_name
    );
    if input.services.is_empty() {
        out.push_str(&format!(
            "- [ ] {} starts and answers `GET {}`\n",
            input.image, input.health_path
        ));
    } else {
        for service in &input.services {
            out.push_str(&format!(
                "- [ ] {service} starts and answers its health endpoint\n"
            ));
        }
    }
    out.push_str("\n## Datastores\n\n");
    if input.datastores.is_empty() {
        out.push_str("Not detected\n");
    } else {
        for engine in &input.datastores {
            out.push_str(&format!(
                "- [ ] {engine} accepts connections with the configured credentials\n"
            ));
        }
    }
    out.push_str("\n## Plan steps\n\n");
    if input.plan.is_empty() {
        out.push_str("Not detected\n");
    } else {
        for step in &input.plan {
            out.push_str(&format!(
                "- [ ] {}: {}\n",
                step.title,
                if step.verification.is_empty() {
                    "verified"
                } else {
                    &step.verification
                }
            ));
        }
    }
    out.push_str("\n## Smoke tests\n\n");
    out.push_str(&format!(
        "- [ ] `{}` passes against the deployed environment\n",
        input.test_command
    ));
    if input.stack == Stack::Static {
        out.push_str("- [ ] The home page renders and deep links fall back to `index.html`\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::{input::fixtures, snapshot::assert_snapshot};
    use super::*;

    #[test]
    fn deployment_and_runbook_snapshots() {
        let input = fixtures::input(&fixtures::node_context(), "azure");
        let guide = deployment(&input);
        assert_snapshot("docs_deployment_node_azure", &guide[0]);
        assert!(guide[0].content.contains("| 2 | Deploy to staging | deploy.staging | high | approval | build | health check passes |"));
        assert!(guide[0].content.contains("`deploy` (deploy.staging - Deploy to staging) stops for human approval: approval gate, high risk"));
        assert!(
            guide[0]
                .content
                .contains("`container:build`, `deploy:staging`")
        );
        let book = runbook(&input);
        assert!(book[0].content.contains("Undo `deploy` (deploy.staging)"));
        assert!(book[0].content.contains("az containerapp logs show"));
        let checklist = uat(&input);
        assert!(
            checklist[0]
                .content
                .contains("- [ ] postgres accepts connections")
        );
    }
}

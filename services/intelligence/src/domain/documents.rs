//! Markdown documents generated from the knowledge model. Every section is
//! derived from facts, the understanding, the graph, decisions or delivery
//! data; when the data is missing the section says "Not detected" instead of
//! guessing.

use std::collections::{BTreeMap, HashSet};

use chrono::{DateTime, Utc};
use sdi_proto::{
    execution::v1::{DeliveryStats, Execution},
    intelligence::v1::{Decision, DecisionStatus, DocumentKind},
    knowledge::v1::{
        Fact, FactCategory, GraphNode, NodeKind, PipelineRun, ProjectContext, RunStatus,
    },
};

use super::{model::RankedOption, policy};

pub const NOT_DETECTED: &str = "Not detected";
const MAX_GRAPH_NODES: usize = 60;

pub struct DocumentInput<'a> {
    pub context: &'a ProjectContext,
    pub decisions: &'a [Decision],
    /// Accepted option if there is one, else the current recommendation.
    pub plan_option: Option<&'a RankedOption>,
    pub pipeline_runs: &'a [PipelineRun],
    pub delivery: Option<&'a DeliveryStats>,
    pub execution: Option<&'a Execution>,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Generated {
    pub title: String,
    pub markdown: String,
    pub facts_used: u32,
}

pub fn kind_label(kind: DocumentKind) -> &'static str {
    match kind {
        DocumentKind::Readme => "README",
        DocumentKind::Architecture => "Architecture",
        DocumentKind::DeploymentGuide => "Deployment guide",
        DocumentKind::Runbook => "Runbook",
        DocumentKind::Adr => "Architecture decision records",
        DocumentKind::UatPlan => "UAT plan",
        DocumentKind::ExecutiveReport => "Executive report",
        DocumentKind::EngineeringReport => "Engineering report",
        DocumentKind::Unspecified => "Document",
    }
}

/// Markdown builder that remembers which facts it cited.
struct Doc {
    lines: Vec<String>,
    facts: HashSet<String>,
}

impl Doc {
    fn new(title: &str) -> Self {
        Self {
            lines: vec![format!("# {title}"), String::new()],
            facts: HashSet::new(),
        }
    }

    fn h2(&mut self, title: &str) {
        self.lines.push(format!("## {title}"));
        self.lines.push(String::new());
    }

    fn p(&mut self, text: impl Into<String>) {
        self.lines.push(text.into());
        self.lines.push(String::new());
    }

    fn bullet(&mut self, text: impl Into<String>) {
        self.lines.push(format!("- {}", text.into()));
    }

    fn end_list(&mut self) {
        self.lines.push(String::new());
    }

    fn not_detected(&mut self) {
        self.p(NOT_DETECTED);
    }

    fn cite(&mut self, fact: &Fact) -> String {
        self.facts.insert(fact.id.clone());
        let value = if fact.value.is_empty() {
            String::new()
        } else {
            format!(" `{}`", fact.value)
        };
        let path = if fact.source_path.is_empty() {
            String::new()
        } else {
            format!(" ({})", fact.source_path)
        };
        format!("{}{value}{path}", fact.label)
    }

    /// Lists facts of one category, or "Not detected".
    fn facts_section(&mut self, title: &str, facts: &[Fact], category: FactCategory) {
        self.h2(title);
        let picked: Vec<Fact> = facts
            .iter()
            .filter(|f| f.category() == category)
            .cloned()
            .collect();
        if picked.is_empty() {
            self.not_detected();
            return;
        }
        for fact in &picked {
            let line = self.cite(fact);
            self.bullet(line);
        }
        self.end_list();
    }

    fn table(&mut self, headers: &[&str], rows: &[Vec<String>]) {
        self.lines.push(format!("| {} |", headers.join(" | ")));
        self.lines.push(format!(
            "|{}|",
            headers.iter().map(|_| "---").collect::<Vec<_>>().join("|")
        ));
        for row in rows {
            self.lines.push(format!(
                "| {} |",
                row.iter()
                    .map(|c| c.replace('|', "\\|"))
                    .collect::<Vec<_>>()
                    .join(" | ")
            ));
        }
        self.lines.push(String::new());
    }

    fn finish(self, title: String) -> Generated {
        Generated {
            title,
            markdown: self.lines.join("\n").trim_end().to_owned() + "\n",
            facts_used: self.facts.len() as u32,
        }
    }
}

fn project_name(ctx: &ProjectContext) -> String {
    ctx.project
        .as_ref()
        .map(|p| p.name.clone())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "project".to_owned())
}

fn node_kind_label(kind: NodeKind) -> String {
    kind.as_str_name()
        .trim_start_matches("NODE_KIND_")
        .to_lowercase()
        .replace('_', " ")
}

fn mermaid_id(index: usize) -> String {
    format!("n{index}")
}

fn mermaid_label(node: &GraphNode) -> String {
    let label = if node.label.is_empty() {
        node.key.clone()
    } else {
        node.label.clone()
    };
    label
        .replace(['"', '[', ']', '(', ')', '{', '}'], " ")
        .trim()
        .to_owned()
}

pub fn generate(kind: DocumentKind, input: &DocumentInput<'_>) -> Option<Generated> {
    let name = project_name(input.context);
    let title = format!("{} - {name}", kind_label(kind));
    let doc = match kind {
        DocumentKind::Readme => readme(input, &name),
        DocumentKind::Architecture => architecture(input, &name),
        DocumentKind::DeploymentGuide => deployment_guide(input, &name),
        DocumentKind::Runbook => runbook(input, &name),
        DocumentKind::Adr => adr(input, &name),
        DocumentKind::UatPlan => uat_plan(input, &name),
        DocumentKind::ExecutiveReport => executive_report(input, &name),
        DocumentKind::EngineeringReport => engineering_report(input, &name),
        DocumentKind::Unspecified => return None,
    };
    Some(doc.finish(title))
}

fn readme(input: &DocumentInput<'_>, name: &str) -> Doc {
    let ctx = input.context;
    let mut doc = Doc::new(name);
    let tldr = ctx
        .understanding
        .as_ref()
        .map(|u| u.tldr.clone())
        .filter(|t| !t.is_empty())
        .or_else(|| {
            ctx.project
                .as_ref()
                .map(|p| p.tldr.clone())
                .filter(|t| !t.is_empty())
        });
    match tldr {
        Some(t) => doc.p(t),
        None => doc.p(format!("{NOT_DETECTED}: no scan summary is available yet.")),
    }

    doc.h2("Stack");
    let stack: Vec<String> = ctx
        .understanding
        .as_ref()
        .map(|u| u.stack.clone())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| ctx.technologies.clone());
    if stack.is_empty() {
        doc.not_detected();
    } else {
        doc.p(stack.join(", "));
    }
    doc.facts_section(
        "Languages and frameworks",
        &ctx.facts,
        FactCategory::Language,
    );
    doc.facts_section("Frameworks", &ctx.facts, FactCategory::Framework);

    doc.h2("How to run");
    let has = |cap: &str| ctx.capabilities.iter().any(|c| c == cap);
    let repo = ctx
        .project
        .as_ref()
        .map(|p| {
            p.repo_full_name
                .rsplit('/')
                .next()
                .unwrap_or("app")
                .to_lowercase()
        })
        .unwrap_or_else(|| "app".to_owned());
    let mut any = false;
    let package_cmd = [
        ("package:pnpm", "pnpm install && pnpm dev"),
        ("package:yarn", "yarn install && yarn dev"),
        ("package:npm", "npm install && npm run dev"),
        ("package:cargo", "cargo build && cargo run"),
        (
            "package:poetry",
            "poetry install && poetry run python -m <module>",
        ),
        ("package:pip", "pip install -r requirements.txt"),
        ("package:go", "go build ./... && go run ."),
        ("package:maven", "mvn package"),
        ("package:gradle", "./gradlew build"),
        ("package:composer", "composer install"),
        ("package:bundler", "bundle install"),
        ("package:mix", "mix deps.get && mix run"),
    ];
    for (cap, cmd) in package_cmd {
        if has(cap) {
            if let Some(fact) = ctx.facts.iter().find(|f| f.key == cap) {
                let cite = doc.cite(fact);
                doc.bullet(format!("Detected {cite}: `{cmd}`"));
            } else {
                doc.bullet(format!("Detected `{cap}`: `{cmd}`"));
            }
            any = true;
        }
    }
    if has("dockerfile") {
        if let Some(fact) = ctx
            .facts
            .iter()
            .find(|f| f.key == "dockerfile" || f.key.ends_with(":dockerfile"))
        {
            doc.cite(fact);
        }
        doc.bullet(format!(
            "Container: `docker build -t {repo} . && docker run --rm -p 8080:8080 {repo}`"
        ));
        any = true;
    }
    if has("docker_compose") {
        doc.bullet("Compose: `docker compose up --build`");
        any = true;
    }
    if has("env_example") {
        doc.bullet("Copy the example environment file before starting (`.env.example` detected).");
        any = true;
    }
    if any {
        doc.end_list();
    } else {
        doc.not_detected();
    }

    doc.facts_section("Continuous integration", &ctx.facts, FactCategory::CiCd);

    doc.h2("Deployment");
    let deploy_facts: Vec<Fact> = ctx
        .facts
        .iter()
        .filter(|f| {
            matches!(
                f.category(),
                FactCategory::Deployment | FactCategory::Cloud | FactCategory::Iac
            )
        })
        .cloned()
        .collect();
    if deploy_facts.is_empty() && input.plan_option.is_none() {
        doc.not_detected();
    } else {
        for fact in &deploy_facts {
            let line = doc.cite(fact);
            doc.bullet(line);
        }
        if let Some(option) = input.plan_option {
            doc.bullet(format!(
                "Proposed pattern: {} (est. ${:.0}/month)",
                option.title, option.estimated_monthly_cost_usd
            ));
        }
        doc.end_list();
    }
    doc
}

fn architecture(input: &DocumentInput<'_>, name: &str) -> Doc {
    let ctx = input.context;
    let mut doc = Doc::new(&format!("{name} architecture"));
    let understanding = ctx.understanding.as_ref();

    doc.h2("Overview");
    match understanding
        .map(|u| u.architecture.as_str())
        .filter(|a| !a.is_empty())
    {
        Some(a) => doc.p(a.to_owned()),
        None => doc.not_detected(),
    }

    if let Some(u) = understanding {
        for section in &u.sections {
            doc.h2(&section.title);
            if !section.summary.is_empty() {
                doc.p(section.summary.clone());
            }
            for item in &section.items {
                doc.bullet(item.clone());
            }
            if !section.items.is_empty() {
                doc.end_list();
            }
            if section.summary.is_empty() && section.items.is_empty() {
                doc.not_detected();
            }
        }
    }

    doc.h2("Request flow");
    match understanding
        .map(|u| u.request_flow.as_slice())
        .filter(|f| !f.is_empty())
    {
        Some(flow) => doc.p(flow.join(" -> ")),
        None => doc.not_detected(),
    }

    doc.facts_section("Datastores", &ctx.facts, FactCategory::Datastore);

    doc.h2("Graph summary");
    let graph = ctx.graph.as_ref();
    match graph.filter(|g| !g.nodes.is_empty()) {
        Some(g) => {
            let mut counts: BTreeMap<String, usize> = BTreeMap::new();
            for node in &g.nodes {
                *counts.entry(node_kind_label(node.kind())).or_default() += 1;
            }
            let rows: Vec<Vec<String>> = counts
                .into_iter()
                .map(|(k, n)| vec![k, n.to_string()])
                .collect();
            doc.table(&["Node kind", "Count"], &rows);
            doc.p(format!("{} nodes, {} edges.", g.nodes.len(), g.edges.len()));

            doc.h2("Graph");
            doc.lines.push("```mermaid".to_owned());
            doc.lines.push("flowchart LR".to_owned());
            let shown: Vec<&GraphNode> = g.nodes.iter().take(MAX_GRAPH_NODES).collect();
            let index: BTreeMap<&str, usize> = shown
                .iter()
                .enumerate()
                .map(|(i, n)| (n.id.as_str(), i))
                .collect();
            for (i, node) in shown.iter().enumerate() {
                doc.lines.push(format!(
                    "    {}[\"{}\"]",
                    mermaid_id(i),
                    mermaid_label(node)
                ));
            }
            for edge in &g.edges {
                if let (Some(s), Some(t)) = (
                    index.get(edge.source_id.as_str()),
                    index.get(edge.target_id.as_str()),
                ) {
                    doc.lines.push(format!(
                        "    {} -->|{}| {}",
                        mermaid_id(*s),
                        edge.relation,
                        mermaid_id(*t)
                    ));
                }
            }
            doc.lines.push("```".to_owned());
            if g.nodes.len() > MAX_GRAPH_NODES {
                doc.lines.push(format!(
                    "Showing the first {MAX_GRAPH_NODES} of {} nodes.",
                    g.nodes.len()
                ));
            }
            doc.lines.push(String::new());
        }
        None => doc.not_detected(),
    }
    doc
}

fn deployment_guide(input: &DocumentInput<'_>, name: &str) -> Doc {
    let mut doc = Doc::new(&format!("{name} deployment guide"));
    let Some(option) = input.plan_option else {
        doc.h2("Plan");
        doc.p(format!("{NOT_DETECTED}: no deployment proposal has been generated or accepted for this project."));
        return doc;
    };
    doc.h2("Pattern");
    doc.p(format!("**{}** - {}", option.title, option.summary));
    doc.bullet(format!(
        "Estimated cost: ${:.0}/month",
        option.estimated_monthly_cost_usd
    ));
    doc.bullet(format!("Overall score: {:.2}", option.score.overall));
    doc.end_list();

    doc.h2("Prerequisites");
    if option.unmet_requirements.is_empty() {
        doc.p("Every pattern requirement is satisfied by the current repository.");
    } else {
        for unmet in &option.unmet_requirements {
            doc.bullet(format!("Missing: {unmet}"));
        }
        doc.end_list();
    }

    doc.h2("Steps");
    let rows: Vec<Vec<String>> = option
        .plan
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
    if rows.is_empty() {
        doc.not_detected();
    } else {
        doc.table(
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
        );
    }

    doc.h2("Policy gates");
    let gated: Vec<&super::model::PlanStepData> = option
        .plan
        .iter()
        .filter(|s| policy::lookup(&s.action_type).gate.needs_human())
        .collect();
    let blocked: Vec<&super::model::PlanStepData> = option
        .plan
        .iter()
        .filter(|s| policy::lookup(&s.action_type).gate == policy::Gate::Blocked)
        .collect();
    if gated.is_empty() && blocked.is_empty() {
        doc.p("Every step runs automatically under the default policy.");
    } else {
        for step in gated {
            let row = policy::lookup(&step.action_type);
            doc.bullet(format!(
                "`{}` ({} - {}) stops for human approval: {} gate, {} risk",
                step.key,
                step.action_type,
                row.label,
                row.gate.label(),
                row.risk
            ));
        }
        for step in blocked {
            doc.bullet(format!(
                "`{}` ({}) is blocked by policy and will never run",
                step.key, step.action_type
            ));
        }
        doc.end_list();
    }

    doc.h2("Permissions");
    let mut permissions: Vec<String> = option
        .plan
        .iter()
        .flat_map(|s| s.required_permissions.clone())
        .collect();
    permissions.sort();
    permissions.dedup();
    if permissions.is_empty() {
        doc.not_detected();
    } else {
        doc.p(format!(
            "The executing identity needs: {}. Anything beyond `repository:read`, `container:build` and `environment:write` must be granted with a TTL first.",
            permissions.iter().map(|p| format!("`{p}`")).collect::<Vec<_>>().join(", ")
        ));
    }
    doc
}

fn runbook(input: &DocumentInput<'_>, name: &str) -> Doc {
    let ctx = input.context;
    let mut doc = Doc::new(&format!("{name} runbook"));

    doc.h2("Health");
    let services: Vec<&GraphNode> = ctx
        .graph
        .as_ref()
        .map(|g| {
            g.nodes
                .iter()
                .filter(|n| n.kind() == NodeKind::Service)
                .collect()
        })
        .unwrap_or_default();
    if services.is_empty() {
        doc.not_detected();
    } else {
        for service in services {
            doc.bullet(format!(
                "{}: confirm the process answers its health endpoint and the container is running",
                mermaid_label(service)
            ));
        }
        doc.end_list();
    }

    doc.h2("Logs");
    let has = |cap: &str| ctx.capabilities.iter().any(|c| c == cap);
    if has("dockerfile") || has("docker_compose") {
        doc.bullet("`docker logs --tail 200 <container>` (Dockerfile detected)");
        if has("docker_compose") {
            doc.bullet("`docker compose logs -f` (Compose detected)");
        }
        doc.end_list();
    } else {
        doc.not_detected();
    }

    doc.h2("Rollback");
    match input.execution {
        Some(execution) => {
            doc.p(format!(
                "Execution #{} ({}) - revision {}, last checkpoint: {}.",
                execution.number,
                execution
                    .status()
                    .as_str_name()
                    .trim_start_matches("EXECUTION_STATUS_")
                    .to_lowercase(),
                execution.revision,
                if execution.last_checkpoint.is_empty() {
                    "none"
                } else {
                    &execution.last_checkpoint
                }
            ));
            let checkpointed: Vec<_> = execution.steps.iter().filter(|s| s.checkpointed).collect();
            if checkpointed.is_empty() {
                doc.p("No step has been checkpointed yet; nothing to roll back.");
            } else {
                for step in checkpointed.iter().rev() {
                    doc.bullet(format!(
                        "Undo `{}` ({}): {}",
                        step.key, step.action_type, step.title
                    ));
                }
                doc.end_list();
            }
        }
        None => {
            match input.plan_option {
                Some(option) => {
                    doc.p("No execution has run yet. Rollback order once it does (reverse of the plan):");
                    for step in option.plan.iter().rev() {
                        doc.bullet(format!("Undo `{}` ({})", step.key, step.action_type));
                    }
                    doc.end_list();
                }
                None => doc.not_detected(),
            }
        }
    }

    doc.h2("Escalation");
    doc.p("Every failure pauses the execution, blocks dependents and waits for a human to pick a remediation (CONTRACT.md §7).");
    doc
}

fn adr(input: &DocumentInput<'_>, name: &str) -> Doc {
    let mut doc = Doc::new(&format!("{name} architecture decision records"));
    let active: Vec<&Decision> = input
        .decisions
        .iter()
        .filter(|d| d.status() == DecisionStatus::Active)
        .collect();
    if active.is_empty() {
        doc.p(format!(
            "{NOT_DETECTED}: no active decision is recorded for this project."
        ));
        return doc;
    }
    for (i, decision) in active.iter().enumerate() {
        doc.h2(&format!("ADR-{:03}: {}", i + 1, decision.title));
        doc.bullet(format!(
            "Status: active ({} scope, source {})",
            decision
                .scope()
                .as_str_name()
                .trim_start_matches("DECISION_SCOPE_")
                .to_lowercase(),
            decision
                .source()
                .as_str_name()
                .trim_start_matches("DECISION_SOURCE_")
                .to_lowercase()
        ));
        if let Some(at) = decision
            .created_at
            .as_ref()
            .and_then(sdi_common::time::from_proto)
        {
            doc.bullet(format!("Date: {}", at.format("%Y-%m-%d")));
        }
        if !decision.approved_by.is_empty() {
            doc.bullet(format!("Approved by: {}", decision.approved_by));
        }
        doc.end_list();
        doc.p(format!(
            "**Decision.** {}",
            if decision.statement.is_empty() {
                &decision.title
            } else {
                &decision.statement
            }
        ));
        doc.p(format!(
            "**Context.** {}",
            if decision.reasons.is_empty() {
                NOT_DETECTED.to_owned()
            } else {
                decision.reasons.join("; ")
            }
        ));
        doc.p(format!(
            "**Tags.** {}",
            if decision.tags.is_empty() {
                "none".to_owned()
            } else {
                decision.tags.join(", ")
            }
        ));
    }
    doc
}

fn uat_plan(input: &DocumentInput<'_>, name: &str) -> Doc {
    let ctx = input.context;
    let mut doc = Doc::new(&format!("{name} UAT plan"));
    let graph = ctx.graph.as_ref();
    let nodes = |kind: NodeKind| -> Vec<&GraphNode> {
        graph
            .map(|g| g.nodes.iter().filter(|n| n.kind() == kind).collect())
            .unwrap_or_default()
    };

    doc.h2("Services");
    let services = nodes(NodeKind::Service);
    if services.is_empty() {
        doc.not_detected();
    } else {
        for s in services {
            doc.bullet(format!(
                "[ ] {} starts and answers a basic request",
                mermaid_label(s)
            ));
        }
        doc.end_list();
    }

    doc.h2("External APIs");
    let apis = nodes(NodeKind::ExternalApi);
    if apis.is_empty() {
        doc.not_detected();
    } else {
        for a in apis {
            doc.bullet(format!(
                "[ ] {} reachable with the configured credentials",
                mermaid_label(a)
            ));
        }
        doc.end_list();
    }

    doc.h2("Datastores");
    let stores = nodes(NodeKind::Datastore);
    if stores.is_empty() {
        doc.not_detected();
    } else {
        for s in stores {
            doc.bullet(format!(
                "[ ] {} migrations applied and connection verified",
                mermaid_label(s)
            ));
        }
        doc.end_list();
    }

    doc.h2("Tests");
    let suites = nodes(NodeKind::TestSuite);
    let test_facts: Vec<Fact> = ctx
        .facts
        .iter()
        .filter(|f| f.category() == FactCategory::Testing)
        .cloned()
        .collect();
    if suites.is_empty() && test_facts.is_empty() {
        doc.not_detected();
    } else {
        for s in suites {
            doc.bullet(format!("[ ] {} passes", mermaid_label(s)));
        }
        for fact in &test_facts {
            let line = doc.cite(fact);
            doc.bullet(format!("[ ] {line} passes"));
        }
        doc.end_list();
    }

    doc.h2("Open questions to settle before sign-off");
    let open: Vec<_> = ctx
        .questions
        .iter()
        .filter(|q| q.answer.is_empty())
        .collect();
    if open.is_empty() {
        doc.p("None open.");
    } else {
        for q in open {
            doc.bullet(format!("[ ] {}", q.prompt));
        }
        doc.end_list();
    }
    doc
}

fn executive_report(input: &DocumentInput<'_>, name: &str) -> Doc {
    let ctx = input.context;
    let mut doc = Doc::new(&format!("{name} executive report"));
    doc.p(format!("Generated {}.", input.now.format("%Y-%m-%d")));

    doc.h2("Delivery");
    let mut rows: Vec<Vec<String>> = Vec::new();
    if let Some(p) = &ctx.pipeline_stats {
        rows.push(vec![
            "Pipeline runs".into(),
            p.total.to_string(),
            format!("{:.0}%", p.success_rate * 100.0),
        ]);
    }
    if let Some(d) = &ctx.deployment_stats {
        rows.push(vec![
            "Deployments".into(),
            d.total.to_string(),
            format!("{:.0}%", d.success_rate * 100.0),
        ]);
    }
    if let Some(e) = input.delivery {
        rows.push(vec![
            "Executions".into(),
            e.executions_total.to_string(),
            format!("{:.0}%", e.success_rate * 100.0),
        ]);
    }
    if rows.is_empty() {
        doc.not_detected();
    } else {
        doc.table(&["Metric", "Total", "Success rate"], &rows);
    }

    doc.h2("Risks");
    let open_questions = ctx.questions.iter().filter(|q| q.answer.is_empty()).count();
    let failed_runs = input
        .pipeline_runs
        .iter()
        .filter(|r| r.status() == RunStatus::Failed)
        .count();
    let unmet = input
        .plan_option
        .map(|o| o.unmet_requirements.len())
        .unwrap_or(0);
    let dependabot = ctx
        .dependabot
        .as_ref()
        .map(|d| d.configured)
        .unwrap_or(false);
    let security_facts = ctx
        .facts
        .iter()
        .filter(|f| f.category() == FactCategory::Security)
        .count();
    doc.table(
        &["Risk", "Count"],
        &[
            vec!["Open questions".into(), open_questions.to_string()],
            vec![
                "Failed pipeline runs (recent)".into(),
                failed_runs.to_string(),
            ],
            vec![
                "Unmet requirements of the recommended pattern".into(),
                unmet.to_string(),
            ],
            vec![
                "Dependabot".into(),
                if dependabot {
                    "configured".into()
                } else {
                    "not configured".into()
                },
            ],
            vec!["Security findings".into(), security_facts.to_string()],
        ],
    );

    doc.h2("Recommendation");
    match input.plan_option {
        Some(o) => doc.p(format!(
            "{} (score {:.2}, est. ${:.0}/month).",
            o.title, o.score.overall, o.estimated_monthly_cost_usd
        )),
        None => doc.not_detected(),
    }
    doc
}

fn engineering_report(input: &DocumentInput<'_>, name: &str) -> Doc {
    let ctx = input.context;
    let mut doc = Doc::new(&format!("{name} engineering report"));

    doc.h2("Pipeline failures");
    let failures: Vec<&PipelineRun> = input
        .pipeline_runs
        .iter()
        .filter(|r| r.status() == RunStatus::Failed)
        .collect();
    if failures.is_empty() {
        doc.p(if input.pipeline_runs.is_empty() {
            NOT_DETECTED.to_owned()
        } else {
            "No failed runs in the observed window.".to_owned()
        });
    } else {
        let rows: Vec<Vec<String>> = failures
            .iter()
            .map(|r| {
                let (job, step, fix) = r
                    .failure
                    .as_ref()
                    .map(|f| (f.job.clone(), f.step.clone(), f.suggested_fix.clone()))
                    .unwrap_or_default();
                vec![
                    format!("#{}", r.run_number),
                    r.pipeline_name.clone(),
                    r.branch.clone(),
                    job,
                    step,
                    fix,
                ]
            })
            .collect();
        doc.table(
            &["Run", "Pipeline", "Branch", "Job", "Step", "Suggested fix"],
            &rows,
        );
    }

    doc.h2("Dependabot");
    match &ctx.dependabot {
        Some(d) if d.configured => doc.p(format!(
            "Configured at `{}` for {}; {} open pull request(s).",
            d.config_path,
            if d.ecosystems.is_empty() {
                "no ecosystems".to_owned()
            } else {
                d.ecosystems.join(", ")
            },
            d.open_pull_requests
        )),
        Some(_) => doc.p("Not configured."),
        None => doc.not_detected(),
    }

    doc.h2("Open questions");
    let open: Vec<_> = ctx
        .questions
        .iter()
        .filter(|q| q.answer.is_empty())
        .collect();
    if open.is_empty() {
        doc.p("None open.");
    } else {
        for q in open {
            doc.bullet(format!("{} - {}", q.prompt, q.reason));
        }
        doc.end_list();
    }

    doc.facts_section("Security", &ctx.facts, FactCategory::Security);

    doc.h2("Recommendations");
    let mut any = false;
    if let Some(o) = input.plan_option {
        doc.bullet(format!("Adopt {} (score {:.2}).", o.title, o.score.overall));
        for unmet in &o.unmet_requirements {
            doc.bullet(format!("Close the gap: {unmet}"));
        }
        any = true;
    }
    if ctx.dependabot.as_ref().is_some_and(|d| !d.configured) {
        doc.bullet("Enable Dependabot so dependency updates arrive as reviewed pull requests.");
        any = true;
    }
    if !ctx.capabilities.iter().any(|c| c == "tests:unit")
        && !ctx
            .facts
            .iter()
            .any(|f| f.category() == FactCategory::Testing)
    {
        doc.bullet("No test suite detected; add one before enabling execute mode.");
        any = true;
    }
    if any {
        doc.end_list();
    } else {
        doc.p("Nothing to recommend from the current data.");
    }
    doc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        model::{Breakdown, PlanStepData, RankedOption},
        ranking::fixtures,
    };
    use sdi_proto::{
        intelligence::v1::DecisionScope,
        knowledge::v1::{
            Graph, GraphEdge, GraphNode, NodeKind, Understanding, UnderstandingSection,
        },
    };

    fn fake_context() -> ProjectContext {
        let mut ctx = fixtures::context();
        ctx.facts[1].category = FactCategory::CiCd as i32;
        ctx.understanding = Some(Understanding {
            project_id: "proj-1".into(),
            tldr: "A TypeScript view engine served from a container.".into(),
            architecture: "Single service with a Postgres datastore.".into(),
            stack: vec!["TypeScript".into(), "PostgreSQL".into()],
            sections: vec![UnderstandingSection {
                key: "services".into(),
                title: "Services".into(),
                summary: "One HTTP service.".into(),
                items: vec!["api".into()],
            }],
            request_flow: vec!["Browser".into(), "API".into(), "PostgreSQL".into()],
            commit_sha: "abc123".into(),
            generated_at: None,
        });
        ctx.graph = Some(Graph {
            nodes: vec![
                GraphNode {
                    id: "n-api".into(),
                    kind: NodeKind::Service as i32,
                    key: "service:api".into(),
                    label: "api".into(),
                    ..Default::default()
                },
                GraphNode {
                    id: "n-db".into(),
                    kind: NodeKind::Datastore as i32,
                    key: "datastore:postgres".into(),
                    label: "PostgreSQL".into(),
                    ..Default::default()
                },
            ],
            edges: vec![GraphEdge {
                id: "e1".into(),
                source_id: "n-api".into(),
                target_id: "n-db".into(),
                relation: "DEPENDS_ON".into(),
                ..Default::default()
            }],
        });
        ctx
    }

    fn option() -> RankedOption {
        RankedOption {
            id: "opt-aca".into(),
            title: "Azure Container Apps".into(),
            summary: "Serverless containers".into(),
            pattern_id: "azure-container-apps".into(),
            rank: 1,
            score: Breakdown {
                overall: 0.81,
                ..Default::default()
            },
            pros: vec![],
            cons: vec![],
            evidence: vec![],
            estimated_monthly_cost_usd: 60.0,
            unmet_requirements: vec![],
            plan: vec![
                PlanStepData {
                    key: "build".into(),
                    title: "Build image".into(),
                    action_type: "container.build".into(),
                    required_permissions: vec!["container:build".into()],
                    ..Default::default()
                },
                PlanStepData {
                    key: "deploy".into(),
                    title: "Deploy to production".into(),
                    action_type: "deploy.production".into(),
                    depends_on: vec!["build".into()],
                    required_permissions: vec!["deploy:production".into()],
                    ..Default::default()
                },
            ],
            tags: vec![],
        }
    }

    fn input<'a>(
        ctx: &'a ProjectContext,
        decisions: &'a [Decision],
        option: Option<&'a RankedOption>,
    ) -> DocumentInput<'a> {
        DocumentInput {
            context: ctx,
            decisions,
            plan_option: option,
            pipeline_runs: &[],
            delivery: None,
            execution: None,
            now: DateTime::parse_from_rfc3339("2026-10-08T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
        }
    }

    #[test]
    fn readme_snapshot() {
        let ctx = fake_context();
        let doc = generate(DocumentKind::Readme, &input(&ctx, &[], None)).unwrap();
        assert_eq!(doc.title, "README - view-engine");
        assert!(
            doc.markdown
                .starts_with("# view-engine\n\nA TypeScript view engine served from a container.")
        );
        assert!(doc.markdown.contains("## Stack\n\nTypeScript, PostgreSQL"));
        assert!(doc.markdown.contains("docker build -t view-engine ."));
        assert!(
            doc.markdown.contains(
                "## Continuous integration\n\n- GitHub Actions (.github/workflows/ci.yml)"
            )
        );
        assert!(doc.markdown.contains("## Deployment\n\nNot detected"));
        assert_eq!(doc.facts_used, 2);
    }

    #[test]
    fn architecture_has_flow_table_and_mermaid() {
        let ctx = fake_context();
        let doc = generate(DocumentKind::Architecture, &input(&ctx, &[], None)).unwrap();
        assert!(doc.markdown.contains("Browser -> API -> PostgreSQL"));
        assert!(doc.markdown.contains("| datastore | 1 |"));
        assert!(doc.markdown.contains("```mermaid\nflowchart LR\n    n0[\"api\"]\n    n1[\"PostgreSQL\"]\n    n0 -->|DEPENDS_ON| n1\n```"));
        assert!(doc.markdown.contains("## Datastores\n\nNot detected"));
    }

    #[test]
    fn deployment_guide_spells_out_gates() {
        let ctx = fake_context();
        let option = option();
        let doc = generate(
            DocumentKind::DeploymentGuide,
            &input(&ctx, &[], Some(&option)),
        )
        .unwrap();
        assert!(doc.markdown.contains(
            "| 2 | Deploy to production | deploy.production | high | required | build |"
        ));
        assert!(
            doc.markdown
                .contains("`deploy` (deploy.production - Deploy to production) stops for human approval: required gate")
        );
        assert!(
            doc.markdown
                .contains("`container:build`, `deploy:production`")
        );

        let empty = generate(DocumentKind::DeploymentGuide, &input(&ctx, &[], None)).unwrap();
        assert!(empty.markdown.contains("Not detected"));
    }

    #[test]
    fn adr_runbook_uat_and_reports_never_fabricate() {
        let ctx = fake_context();
        let decisions = vec![fixtures::decision(
            "d1",
            DecisionScope::Organization,
            &["cloud:aws"],
        )];
        let option = option();

        let adr = generate(DocumentKind::Adr, &input(&ctx, &decisions, None)).unwrap();
        assert!(adr.markdown.contains("## ADR-001: decision d1"));
        assert!(adr.markdown.contains("**Context.** Not detected"));
        let no_adr = generate(DocumentKind::Adr, &input(&ctx, &[], None)).unwrap();
        assert!(no_adr.markdown.contains("Not detected"));

        let runbook = generate(DocumentKind::Runbook, &input(&ctx, &[], Some(&option))).unwrap();
        assert!(runbook.markdown.contains("- api: confirm the process"));
        assert!(
            runbook
                .markdown
                .contains("Undo `deploy` (deploy.production)")
        );

        let uat = generate(DocumentKind::UatPlan, &input(&ctx, &[], None)).unwrap();
        assert!(uat.markdown.contains("[ ] api starts"));
        assert!(uat.markdown.contains("## External APIs\n\nNot detected"));

        let exec = generate(
            DocumentKind::ExecutiveReport,
            &input(&ctx, &[], Some(&option)),
        )
        .unwrap();
        assert!(exec.markdown.contains("| Pipeline runs | 12 | 83% |"));
        assert!(exec.markdown.contains("| Dependabot | not configured |"));

        let eng = generate(DocumentKind::EngineeringReport, &input(&ctx, &[], None)).unwrap();
        assert!(
            eng.markdown
                .contains("## Pipeline failures\n\nNot detected")
        );
        assert!(eng.markdown.contains("No test suite detected"));

        assert!(generate(DocumentKind::Unspecified, &input(&ctx, &[], None)).is_none());
    }
}

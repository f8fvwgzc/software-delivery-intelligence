//! End-to-end flow through the service with fake peers and a real Postgres
//! (the dev database, isolated in its own schema). Ignored by default: run
//! `cargo test -p sdi-intelligence -- --ignored` with the database up.

use std::{sync::Arc, time::Duration};

use sdi_common::principal::{Principal, Role};
use sdi_proto::{
    common::v1::WatchEventsRequest,
    identity::v1::ReasonerKey,
    intelligence::v1::{
        AcceptProposalRequest, CreateDecisionRequest, DecisionScope, DecisionSource,
        DeleteDiagramRequest, DeleteWorkspaceDataRequest, DiagramKind, DiagramSpec, DocumentKind,
        ExportWorkspaceDataRequest, GenerateDiagramRequest, GenerateDocumentRequest,
        GenerateProposalRequest, GetDiagramRequest, GetIntelligenceStatsRequest,
        GetProposalRequest, GetSettingsRequest, IntelligenceSettings, ListDecisionsRequest,
        ListDiagramsRequest, ListMessagesRequest, ListProposalsRequest, MessageRole,
        NegotiateRequest, ProposalKind, ProposalStatus, ScoreWeights, SupersedeDecisionRequest,
        UpdateDiagramRequest, UpdateSettingsRequest,
        intelligence_service_server::IntelligenceService,
    },
};
use tokio_stream::StreamExt;
use tonic::{Code, Request};

use super::{Caller, IntelligenceGrpc, ReasonerConfig};
use crate::{
    domain::ranking::fixtures,
    gateway::{
        self,
        fake::{FakeExecution, FakeIdentity, FakeKnowledge},
    },
};

/// The two workspaces the tests play in. Every call carries one of them as
/// metadata, the way the gateway forwards it (CONTRACT.md §18).
const WS_A: &str = "00000000-0000-7000-8000-00000000000a";
const WS_B: &str = "00000000-0000-7000-8000-00000000000b";

fn principal(role: Role, workspace: &str) -> Principal {
    Principal {
        user_id: format!("user-{}", role.as_str()),
        workspace_id: workspace.to_owned(),
        role,
    }
}

fn as_role<T>(role: Role, workspace: &str, message: T) -> Request<T> {
    gateway::outgoing(&principal(role, workspace), message)
}

/// The default caller: an admin of workspace A.
fn req<T>(message: T) -> Request<T> {
    as_role(Role::Admin, WS_A, message)
}

struct Peers {
    knowledge: Arc<FakeKnowledge>,
    execution: Arc<FakeExecution>,
    identity: Arc<FakeIdentity>,
}

/// One schema per test so they can run in parallel without truncating each other.
async fn service(schema: &str) -> (IntelligenceGrpc, Arc<FakeKnowledge>, Arc<FakeExecution>) {
    let (svc, peers) = service_with_peers(schema).await;
    (svc, peers.knowledge, peers.execution)
}

async fn service_with_peers(schema: &str) -> (IntelligenceGrpc, Peers) {
    let url = sdi_common::config::database_url();
    let pool = sdi_common::db::connect(&url, schema)
        .await
        .expect("dev database");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("migrations");
    for table in [
        "messages",
        "proposals",
        "decisions",
        "documents",
        "diagrams",
        "settings",
    ] {
        sqlx::query(sqlx::AssertSqlSafe(format!("TRUNCATE {table} CASCADE")))
            .execute(&pool)
            .await
            .unwrap();
    }
    let mut context = fixtures::context();
    context.understanding = Some(sdi_proto::knowledge::v1::Understanding {
        commit_sha: "0123456789abcdef".into(),
        ..Default::default()
    });
    let knowledge = Arc::new(FakeKnowledge {
        context: Some(context),
        patterns: fixtures::catalog(),
        ..Default::default()
    });
    let execution = Arc::new(FakeExecution::default());
    let identity = Arc::new(FakeIdentity::default());
    let svc = IntelligenceGrpc::new(
        pool,
        knowledge.clone(),
        execution.clone(),
        identity.clone(),
        ReasonerConfig {
            anthropic_api_key: None,
            anthropic_model: crate::config::DEFAULT_MODEL.to_owned(),
        },
    );
    (
        svc,
        Peers {
            knowledge,
            execution,
            identity,
        },
    )
}

#[tokio::test]
#[ignore = "needs the dev Postgres"]
async fn propose_negotiate_accept_document_flow() {
    let (svc, knowledge, execution) = service("intelligence_test_flow").await;

    let proposal = svc
        .generate_proposal(req(GenerateProposalRequest {
            project_id: "proj-1".into(),
            kind: ProposalKind::Deployment as i32,
            prompt: "deploy this".into(),
            budget_monthly_usd: 0.0,
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(proposal.status(), ProposalStatus::Proposed);
    assert_eq!(proposal.options.len(), 4);
    assert_eq!(proposal.options[0].rank, 1);
    assert_eq!(proposal.recommended_option_id, proposal.options[0].id);
    assert_eq!(proposal.reasoner, "heuristic");
    let before_top = proposal.options[0].pattern_id.clone();
    assert_ne!(before_top, "kubernetes-helm");

    let missing = svc
        .generate_proposal(req(GenerateProposalRequest {
            project_id: "nope".into(),
            kind: ProposalKind::Deployment as i32,
            prompt: String::new(),
            budget_monthly_usd: 0.0,
        }))
        .await
        .unwrap_err();
    assert_eq!(missing.code(), Code::NotFound);

    // A standing decision re-ranks: aws wins once azure contradicts the policy.
    let turn = svc
        .negotiate(req(NegotiateRequest {
            proposal_id: proposal.id.clone(),
            content: "we standardise on AWS because the platform team supports it".into(),
        }))
        .await
        .unwrap()
        .into_inner();
    let decision = turn.recorded_decision.expect("decision recorded");
    assert_eq!(decision.scope(), DecisionScope::Organization);
    assert_eq!(decision.source(), DecisionSource::Negotiation);
    assert_eq!(decision.tags, vec!["cloud:aws"]);
    let after = turn.proposal.unwrap();
    assert_eq!(after.status(), ProposalStatus::Negotiating);
    assert_eq!(after.revision, 2);
    let azure = after
        .options
        .iter()
        .find(|o| o.pattern_id == "azure-container-apps")
        .unwrap();
    assert_eq!(azure.score.as_ref().unwrap().policy, 0.0);
    let reply = turn.reply.unwrap();
    assert_eq!(reply.role(), MessageRole::Assistant);
    assert!(reply.content.contains("Recorded decision"));
    assert!(!reply.references.is_empty());

    let turn = svc
        .negotiate(req(NegotiateRequest {
            proposal_id: proposal.id.clone(),
            content: "cheaper".into(),
        }))
        .await
        .unwrap()
        .into_inner();
    let after = turn.proposal.unwrap();
    assert!(after.weights.as_ref().unwrap().cost > 0.2);
    assert!(
        turn.reply
            .unwrap()
            .content
            .contains("raised the cost weight")
    );

    let indexed = knowledge.indexed.lock().unwrap().len();
    assert_eq!(indexed, 3, "decision + two conversation turns indexed");

    let messages = svc
        .list_messages(req(ListMessagesRequest {
            proposal_id: proposal.id.clone(),
        }))
        .await
        .unwrap()
        .into_inner()
        .messages;
    assert_eq!(
        messages.len(),
        6,
        "system opener, prompt, 2 user turns, 2 replies"
    );

    let accepted = svc
        .accept_proposal(req(AcceptProposalRequest {
            id: proposal.id.clone(),
            option_id: String::new(),
            create_execution: true,
            adapter: String::new(),
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(accepted.status(), ProposalStatus::Accepted);
    assert_eq!(accepted.accepted_option_id, after.recommended_option_id);
    assert_eq!(accepted.execution_id, "exec-1");
    {
        let created = execution.created.lock().unwrap();
        assert_eq!(created[0].proposal_id, proposal.id);
        assert_eq!(created[0].steps.len(), 2);
        assert_eq!(
            created[0].steps[0].params.get("image").map(String::as_str),
            Some("view-engine")
        );
    }

    let again = svc
        .accept_proposal(req(AcceptProposalRequest {
            id: proposal.id.clone(),
            option_id: String::new(),
            create_execution: false,
            adapter: String::new(),
        }))
        .await
        .unwrap_err();
    assert_eq!(again.code(), Code::FailedPrecondition);

    let decisions = svc
        .list_decisions(req(ListDecisionsRequest {
            project_id: "proj-1".into(),
            include_superseded: false,
        }))
        .await
        .unwrap()
        .into_inner()
        .decisions;
    assert_eq!(decisions.len(), 2);
    let adopted = decisions
        .iter()
        .find(|d| d.source() == DecisionSource::ProposalAccepted)
        .unwrap();
    assert_eq!(adopted.proposal_id, proposal.id);
    assert!(adopted.tags.iter().any(|t| t.starts_with("pattern:")));

    let manual = svc
        .create_decision(req(CreateDecisionRequest {
            project_id: String::new(),
            scope: DecisionScope::Organization as i32,
            title: "Terraform everywhere".into(),
            statement: "All infrastructure is Terraform.".into(),
            reasons: vec!["one toolchain".into()],
            approved_by: "cto".into(),
            tags: vec!["IAC:terraform".into()],
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(manual.tags, vec!["iac:terraform"]);
    let superseded = svc
        .supersede_decision(req(SupersedeDecisionRequest {
            id: manual.id.clone(),
            superseded_by: String::new(),
        }))
        .await
        .unwrap()
        .into_inner();
    assert!(superseded.superseded_at.is_some());
    let twice = svc
        .supersede_decision(req(SupersedeDecisionRequest {
            id: manual.id,
            superseded_by: String::new(),
        }))
        .await
        .unwrap_err();
    assert_eq!(twice.code(), Code::FailedPrecondition);

    let guide = svc
        .generate_document(req(GenerateDocumentRequest {
            project_id: "proj-1".into(),
            kind: DocumentKind::DeploymentGuide as i32,
        }))
        .await
        .unwrap()
        .into_inner();
    assert!(guide.markdown.contains("## Policy gates"));
    assert!(guide.markdown.contains("deploy.staging"));
    let adr = svc
        .generate_document(req(GenerateDocumentRequest {
            project_id: "proj-1".into(),
            kind: DocumentKind::Adr as i32,
        }))
        .await
        .unwrap()
        .into_inner();
    assert!(adr.markdown.contains("Standardise on aws"));
    assert!(adr.markdown.contains("## ADR-002"));

    let stats = svc
        .get_intelligence_stats(req(GetIntelligenceStatsRequest {}))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(stats.proposals, 1);
    assert_eq!(stats.proposals_accepted, 1);
    assert_eq!(stats.proposals_open, 0);
    assert_eq!(stats.decisions_active, 2);
    assert_eq!(stats.documents, 2);
    assert_eq!(stats.messages, 7);
}

#[tokio::test]
#[ignore = "needs the dev Postgres"]
async fn validation_errors_are_invalid_argument() {
    let (svc, _, _) = service("intelligence_test_validation").await;
    let err = svc
        .generate_proposal(req(GenerateProposalRequest {
            project_id: "proj-1".into(),
            kind: ProposalKind::Unspecified as i32,
            prompt: String::new(),
            budget_monthly_usd: 0.0,
        }))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::InvalidArgument);
    let err = svc
        .negotiate(req(NegotiateRequest {
            proposal_id: "not-a-uuid".into(),
            content: "cheaper".into(),
        }))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::InvalidArgument);
    let err = svc
        .list_messages(req(ListMessagesRequest {
            proposal_id: uuid::Uuid::nil().to_string(),
        }))
        .await;
    assert!(err.is_ok_and(|r| r.into_inner().messages.is_empty()));
}

async fn propose(svc: &IntelligenceGrpc, budget: f64) -> sdi_proto::intelligence::v1::Proposal {
    svc.generate_proposal(req(GenerateProposalRequest {
        project_id: "proj-1".into(),
        kind: ProposalKind::Deployment as i32,
        prompt: String::new(),
        budget_monthly_usd: budget,
    }))
    .await
    .unwrap()
    .into_inner()
}

#[tokio::test]
#[ignore = "needs the dev Postgres"]
async fn accept_with_each_adapter_hands_the_right_plan_to_execution() {
    let (svc, _, execution) = service("intelligence_test_adapters").await;

    // docker: the local verification loop with image/port/health/test params.
    let proposal = propose(&svc, 0.0).await;
    let accepted = svc
        .accept_proposal(req(AcceptProposalRequest {
            id: proposal.id.clone(),
            option_id: String::new(),
            create_execution: true,
            adapter: "docker".into(),
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(accepted.status(), ProposalStatus::Accepted);
    {
        let created = execution.created.lock().unwrap();
        let request = created.last().unwrap();
        assert_eq!(request.adapter, "docker");
        // The option's generated files ride along so the checkout is overlaid
        // with them before `docker build`.
        assert!(request.artifacts.contains_key("Dockerfile"));
        let actions: Vec<&str> = request
            .steps
            .iter()
            .map(|s| s.action_type.as_str())
            .collect();
        assert_eq!(
            actions,
            vec![
                "repo.checkout",
                "container.build",
                "container.run",
                "tests.run_local",
                "env.destroy_temporary"
            ]
        );
        assert_eq!(
            request.steps[1].params.get("image").map(String::as_str),
            Some("view-engine:sdi-0123456")
        );
        assert_eq!(
            request.steps[2]
                .params
                .get("health_path")
                .map(String::as_str),
            Some("/health")
        );
        assert_eq!(
            request.steps[3]
                .params
                .get("test_command")
                .map(String::as_str),
            Some("npm test")
        );
        assert!(request.title.starts_with("Verify "));
    }

    // github: checkout → open_pull_request with the option's files.
    let proposal = propose(&svc, 0.0).await;
    let option = proposal.options[0].clone();
    assert!(
        !option.artifacts.is_empty(),
        "deployment options carry files"
    );
    let accepted = svc
        .accept_proposal(req(AcceptProposalRequest {
            id: proposal.id.clone(),
            option_id: option.id.clone(),
            create_execution: true,
            adapter: "GitHub".into(),
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(accepted.execution_id, "exec-2");
    {
        let created = execution.created.lock().unwrap();
        let request = created.last().unwrap();
        assert_eq!(request.adapter, "github");
        assert_eq!(request.steps.len(), 2);
        assert_eq!(request.steps[0].action_type, "repo.checkout");
        assert_eq!(request.steps[1].action_type, "repo.open_pull_request");
        let short: String = proposal.id.chars().take(8).collect();
        assert_eq!(
            request.steps[1].params.get("branch").map(String::as_str),
            Some(format!("sdi/{short}").as_str())
        );
        assert_eq!(
            request.steps[1].params.get("title").map(String::as_str),
            Some(option.title.as_str())
        );
        let body = request.steps[1].params.get("body").unwrap();
        assert!(body.contains("### Files"));
        assert!(body.contains("### Evidence"));
        assert_eq!(request.artifacts.len(), option.artifacts.len());
        for file in &option.artifacts {
            assert_eq!(request.artifacts.get(&file.path), Some(&file.content));
            assert!(body.contains(&format!("`{}`", file.path)));
        }
    }

    // simulated (default): the option's own plan.
    let proposal = propose(&svc, 0.0).await;
    svc.accept_proposal(req(AcceptProposalRequest {
        id: proposal.id.clone(),
        option_id: String::new(),
        create_execution: true,
        adapter: String::new(),
    }))
    .await
    .unwrap();
    {
        let created = execution.created.lock().unwrap();
        let request = created.last().unwrap();
        assert_eq!(request.adapter, "simulated");
        assert_eq!(request.steps.len(), 2);
        assert_eq!(request.steps[0].action_type, "container.build");
    }

    let proposal = propose(&svc, 0.0).await;
    let bad = svc
        .accept_proposal(req(AcceptProposalRequest {
            id: proposal.id,
            option_id: String::new(),
            create_execution: true,
            adapter: "cloud".into(),
        }))
        .await
        .unwrap_err();
    assert_eq!(bad.code(), Code::InvalidArgument);
}

#[tokio::test]
#[ignore = "needs the dev Postgres"]
async fn settings_drive_ranking_and_validate_input() {
    let (svc, _, _) = service("intelligence_test_settings").await;
    let defaults = svc
        .get_settings(req(GetSettingsRequest {}))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(defaults.reasoner, "claude");
    assert_eq!(defaults.model, "claude-sonnet-5-5", "env default until set");
    assert!(!defaults.api_key_configured);
    assert_eq!(defaults.cost_ceiling_usd, 500.0);
    let weights = defaults.default_weights.unwrap();
    assert!((weights.cost - 0.10).abs() < 1e-9);

    // Unnormalised weights are accepted and renormalised.
    let updated = svc
        .update_settings(req(UpdateSettingsRequest {
            settings: Some(IntelligenceSettings {
                default_weights: Some(ScoreWeights {
                    semantic_similarity: 10.0,
                    policy: 10.0,
                    recency: 10.0,
                    success: 10.0,
                    architecture_fit: 10.0,
                    cost: 40.0,
                    security: 10.0,
                }),
                reasoner: "heuristic".into(),
                model: "claude-opus-5".into(),
                api_key_configured: true,
                cost_ceiling_usd: 100.0,
            }),
        }))
        .await
        .unwrap()
        .into_inner();
    let w = updated.default_weights.unwrap();
    assert!((w.cost - 0.4).abs() < 1e-9);
    assert!((w.policy - 0.1).abs() < 1e-9);
    assert_eq!(updated.reasoner, "heuristic");
    assert_eq!(updated.model, "claude-opus-5");
    assert!(
        !updated.api_key_configured,
        "comes from the environment, never from the request"
    );
    assert_eq!(updated.cost_ceiling_usd, 100.0);

    // Ranking reads the stored defaults: the proposal's weights are the new ones.
    let proposal = propose(&svc, 0.0).await;
    assert!((proposal.weights.unwrap().cost - 0.4).abs() < 1e-9);
    assert_eq!(proposal.reasoner, "heuristic");
    // With a $100 ceiling the $250 Kubernetes option's cost signal is 0.
    let k8s = proposal
        .options
        .iter()
        .find(|o| o.pattern_id == "kubernetes-helm")
        .unwrap();
    assert_eq!(k8s.score.as_ref().unwrap().cost, 0.0);

    let bad = svc
        .update_settings(req(UpdateSettingsRequest {
            settings: Some(IntelligenceSettings {
                reasoner: "gpt".into(),
                ..Default::default()
            }),
        }))
        .await
        .unwrap_err();
    assert_eq!(bad.code(), Code::InvalidArgument);
    let bad = svc
        .update_settings(req(UpdateSettingsRequest {
            settings: Some(IntelligenceSettings {
                default_weights: Some(ScoreWeights::default()),
                ..Default::default()
            }),
        }))
        .await
        .unwrap_err();
    assert_eq!(bad.code(), Code::InvalidArgument);
}

#[tokio::test]
#[ignore = "needs the dev Postgres"]
async fn budget_artifacts_list_and_events() {
    let (svc, _, _) = service("intelligence_test_budget").await;
    let mut events = svc
        .watch_events(req(WatchEventsRequest {}))
        .await
        .unwrap()
        .into_inner();

    let proposal = propose(&svc, 30.0).await;
    assert!(
        proposal
            .constraints
            .iter()
            .any(|c| c.key == "budget_monthly_usd:30")
    );
    let k8s = proposal
        .options
        .iter()
        .find(|o| o.pattern_id == "kubernetes-helm")
        .unwrap();
    assert!(k8s.unmet_requirements.contains(&"budget:30".to_owned()));
    let vm = proposal
        .options
        .iter()
        .find(|o| o.pattern_id == "vm-docker-compose")
        .unwrap();
    assert!(
        !vm.unmet_requirements
            .iter()
            .any(|u| u.starts_with("budget:"))
    );
    assert!(
        proposal
            .options
            .iter()
            .all(|o| !o.cost_breakdown.is_empty())
    );
    assert!(proposal.options.iter().all(|o| !o.artifacts.is_empty()));

    let created = events.next().await.unwrap().unwrap();
    assert_eq!(
        (
            created.entity.as_str(),
            created.action.as_str(),
            created.status.as_str()
        ),
        ("proposal", "created", "proposed")
    );
    assert_eq!(created.entity_id, proposal.id);
    assert_eq!(created.data.get("revision").map(String::as_str), Some("1"));

    // A negotiation turn can replace the budget and change parameters; the
    // artifacts are regenerated with the new region.
    let turn = svc
        .negotiate(req(NegotiateRequest {
            proposal_id: proposal.id.clone(),
            content: "max 20 dollars a month, deploy in region eu-central-1".into(),
        }))
        .await
        .unwrap()
        .into_inner();
    let after = turn.proposal.unwrap();
    let keys: Vec<&str> = after.constraints.iter().map(|c| c.key.as_str()).collect();
    assert!(keys.contains(&"budget_monthly_usd:20"));
    assert!(keys.contains(&"param:region:eu-central-1"));
    assert!(!keys.contains(&"budget_monthly_usd:30"));
    assert!(
        turn.reply
            .unwrap()
            .content
            .contains("Set the monthly budget to $20")
    );
    let aws = after
        .options
        .iter()
        .find(|o| o.pattern_id == "aws-ecs-fargate")
        .unwrap();
    let variables = aws
        .artifacts
        .iter()
        .find(|a| a.path == "infra/aws/variables.tf")
        .unwrap();
    assert!(variables.content.contains("eu-central-1"));
    let updated = events.next().await.unwrap().unwrap();
    assert_eq!(
        (updated.action.as_str(), updated.status.as_str()),
        ("updated", "negotiating")
    );
    assert_eq!(updated.data.get("revision").map(String::as_str), Some("2"));

    // List responses omit artifacts; the single get carries them.
    let listed = svc
        .list_proposals(req(ListProposalsRequest {
            project_id: "proj-1".into(),
        }))
        .await
        .unwrap()
        .into_inner()
        .proposals;
    assert_eq!(listed.len(), 1);
    assert!(listed[0].options.iter().all(|o| o.artifacts.is_empty()));
    assert!(
        listed[0]
            .options
            .iter()
            .all(|o| !o.cost_breakdown.is_empty())
    );
    let got = svc
        .get_proposal(req(GetProposalRequest {
            id: proposal.id.clone(),
        }))
        .await
        .unwrap()
        .into_inner();
    assert!(got.options.iter().all(|o| !o.artifacts.is_empty()));
    assert_eq!(got.revision, 2);

    // A new proposal of the same kind supersedes the old one and says so.
    let newer = propose(&svc, 0.0).await;
    let created = events.next().await.unwrap().unwrap();
    assert_eq!(created.entity_id, newer.id);
    let superseded = events.next().await.unwrap().unwrap();
    assert_eq!(superseded.entity_id, proposal.id);
    assert_eq!(superseded.status, "superseded");

    let document = svc
        .generate_document(req(GenerateDocumentRequest {
            project_id: "proj-1".into(),
            kind: DocumentKind::Runbook as i32,
        }))
        .await
        .unwrap()
        .into_inner();
    assert!(document.markdown.contains("## Rollback"));
    let event = events.next().await.unwrap().unwrap();
    assert_eq!(
        (event.entity.as_str(), event.entity_id.as_str()),
        ("document", document.id.as_str())
    );
    assert_eq!(event.data.get("kind").map(String::as_str), Some("runbook"));
}

/// Diagrams: generate from the fake knowledge (context + tree + files),
/// list without spec/mermaid, edit the spec, override the Mermaid, delete.
/// Runs only when `DATABASE_URL` points at a database.
#[tokio::test]
async fn diagram_round_trip() {
    if std::env::var("DATABASE_URL").is_err() {
        eprintln!("DATABASE_URL not set; skipping diagram_round_trip");
        return;
    }
    use crate::domain::diagrams::fixtures as dia;
    let (svc, _, _) = service("intelligence_test_diagrams").await;
    let knowledge = Arc::new(FakeKnowledge {
        context: Some(dia::context()),
        tree: dia::tree(),
        files: dia::files(),
        ..Default::default()
    });
    let svc = IntelligenceGrpc::new(
        svc.pool.clone(),
        knowledge.clone(),
        Arc::new(FakeExecution::default()),
        Arc::new(FakeIdentity::default()),
        ReasonerConfig::default(),
    );
    let mut events = svc.events.watch(uuid::Uuid::parse_str(WS_A).unwrap());

    let missing = svc
        .generate_diagram(req(GenerateDiagramRequest {
            project_id: "nope".into(),
            kind: DiagramKind::Architecture as i32,
            prompt: String::new(),
        }))
        .await
        .unwrap_err();
    assert_eq!(missing.code(), Code::NotFound);
    let unspecified = svc
        .generate_diagram(req(GenerateDiagramRequest {
            project_id: dia::PROJECT_ID.into(),
            kind: 0,
            prompt: String::new(),
        }))
        .await
        .unwrap_err();
    assert_eq!(unspecified.code(), Code::InvalidArgument);

    let diagram = svc
        .generate_diagram(req(GenerateDiagramRequest {
            project_id: dia::PROJECT_ID.into(),
            kind: DiagramKind::Architecture as i32,
            prompt: String::new(),
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(diagram.kind(), DiagramKind::Architecture);
    assert_eq!(diagram.title, "view-engine architecture");
    assert_eq!(diagram.source, "generated");
    assert_eq!(diagram.reasoner, "heuristic");
    assert_eq!(diagram.revision, 1);
    assert_eq!(
        diagram.commit_sha,
        "0123456789abcdef0123456789abcdef01234567"
    );
    let spec = diagram.spec.clone().expect("spec");
    assert!(
        spec.nodes
            .iter()
            .any(|n| n.id == "service_api" && n.path == "services/api/src/main.rs")
    );
    assert!(diagram.mermaid.starts_with("flowchart LR\n"));
    assert!(diagram.mermaid.contains(&format!(
        "/projects/{}/code/services/api/src/main.rs",
        dia::PROJECT_ID
    )));
    let words = diagram.explanation.split_whitespace().count();
    assert!((60..=150).contains(&words));
    // Key files were fetched through the gateway (bounded).
    let read = knowledge.files_read.lock().unwrap().clone();
    assert!(!read.is_empty() && read.len() <= crate::domain::diagrams::MAX_KEY_FILES);
    assert!(read.contains(&"services/api/src/main.rs".to_owned()));
    let created = events.next().await.unwrap().unwrap();
    assert_eq!(
        (created.entity.as_str(), created.action.as_str()),
        ("diagram", "created")
    );
    assert_eq!(created.entity_id, diagram.id);

    // A Mermaid import is stored as is, with an empty spec.
    let imported = svc
        .generate_diagram(req(GenerateDiagramRequest {
            project_id: dia::PROJECT_ID.into(),
            kind: DiagramKind::Custom as i32,
            prompt: "flowchart TB\n  a[A] --> b[B]\n".into(),
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(imported.source, "mermaid");
    assert!(imported.spec.as_ref().unwrap().nodes.is_empty());
    assert!(imported.mermaid.starts_with("flowchart TB"));
    let _ = events.next().await;

    // List omits spec and mermaid, newest first.
    let listed = svc
        .list_diagrams(req(ListDiagramsRequest {
            project_id: dia::PROJECT_ID.into(),
        }))
        .await
        .unwrap()
        .into_inner()
        .diagrams;
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].id, imported.id);
    assert!(
        listed
            .iter()
            .all(|d| d.spec.is_none() && d.mermaid.is_empty())
    );
    assert_eq!(listed[1].title, diagram.title);

    // Spec edit: Mermaid regenerated, source "edited", revision bumped.
    let mut edited_spec = spec.clone();
    edited_spec
        .nodes
        .retain(|n| n.id == "service_api" || n.id == "datastore_postgres");
    edited_spec
        .edges
        .retain(|e| e.source == "service_api" && e.target == "datastore_postgres");
    edited_spec.nodes[0].label = "HTTP API".into();
    let edited = svc
        .update_diagram(req(UpdateDiagramRequest {
            id: diagram.id.clone(),
            title: "Trimmed".into(),
            spec: Some(edited_spec),
            mermaid: String::new(),
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(edited.title, "Trimmed");
    assert_eq!(edited.source, "edited");
    assert_eq!(edited.revision, 2);
    assert_eq!(edited.spec.as_ref().unwrap().nodes.len(), 2);
    assert!(edited.mermaid.contains("HTTP API"));
    assert!(!edited.mermaid.contains("n_service_quant"));
    let updated = events.next().await.unwrap().unwrap();
    assert_eq!(
        (
            updated.entity.as_str(),
            updated.action.as_str(),
            updated.status.as_str()
        ),
        ("diagram", "updated", "edited")
    );
    assert_eq!(updated.data.get("revision").map(String::as_str), Some("2"));

    // Mermaid edit: source authoritative, spec kept.
    let overridden = svc
        .update_diagram(req(UpdateDiagramRequest {
            id: diagram.id.clone(),
            title: String::new(),
            spec: None,
            mermaid: "flowchart LR\n  x[X] --> y[Y]\n".into(),
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(overridden.source, "mermaid");
    assert_eq!(overridden.revision, 3);
    assert_eq!(overridden.spec.as_ref().unwrap().nodes.len(), 2);
    assert!(overridden.mermaid.starts_with("flowchart LR\n  x[X]"));
    let _ = events.next().await;

    // Invalid edits are rejected.
    let bad_spec = svc
        .update_diagram(req(UpdateDiagramRequest {
            id: diagram.id.clone(),
            title: String::new(),
            spec: Some(DiagramSpec {
                direction: "LR".into(),
                groups: vec![],
                nodes: vec![],
                edges: vec![sdi_proto::intelligence::v1::DiagramEdge {
                    source: "a".into(),
                    target: "b".into(),
                    ..Default::default()
                }],
            }),
            mermaid: String::new(),
        }))
        .await
        .unwrap_err();
    assert_eq!(bad_spec.code(), Code::InvalidArgument);
    let bad_mermaid = svc
        .update_diagram(req(UpdateDiagramRequest {
            id: diagram.id.clone(),
            title: String::new(),
            spec: None,
            mermaid: "flowchart LR\n  subgraph s\n  a --> b\n".into(),
        }))
        .await
        .unwrap_err();
    assert_eq!(bad_mermaid.code(), Code::InvalidArgument);
    let nothing = svc
        .update_diagram(req(UpdateDiagramRequest {
            id: diagram.id.clone(),
            ..Default::default()
        }))
        .await
        .unwrap_err();
    assert_eq!(nothing.code(), Code::InvalidArgument);

    let fetched = svc
        .get_diagram(req(GetDiagramRequest {
            id: diagram.id.clone(),
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(fetched.revision, 3);
    assert_eq!(fetched.source, "mermaid");

    svc.delete_diagram(req(DeleteDiagramRequest {
        id: diagram.id.clone(),
    }))
    .await
    .unwrap();
    let deleted = events.next().await.unwrap().unwrap();
    assert_eq!(
        (deleted.entity.as_str(), deleted.action.as_str()),
        ("diagram", "deleted")
    );
    let gone = svc
        .get_diagram(req(GetDiagramRequest {
            id: diagram.id.clone(),
        }))
        .await
        .unwrap_err();
    assert_eq!(gone.code(), Code::NotFound);
    let twice = svc
        .delete_diagram(req(DeleteDiagramRequest {
            id: diagram.id.clone(),
        }))
        .await
        .unwrap_err();
    assert_eq!(twice.code(), Code::NotFound);
    let bad_id = svc
        .get_diagram(req(GetDiagramRequest {
            id: "not-a-uuid".into(),
        }))
        .await
        .unwrap_err();
    assert_eq!(bad_id.code(), Code::InvalidArgument);
}

/// Tenancy (CONTRACT.md §18): metadata is mandatory, roles gate writes,
/// one workspace never sees another's rows, peers get the caller forwarded,
/// events carry the workspace, and the export / delete RPCs are complete.
#[tokio::test]
#[ignore = "needs the dev Postgres"]
async fn workspaces_are_isolated_and_roles_enforced() {
    let (svc, peers) = service_with_peers("intelligence_test_tenancy").await;
    let generate = || GenerateProposalRequest {
        project_id: "proj-1".into(),
        kind: ProposalKind::Deployment as i32,
        prompt: String::new(),
        budget_monthly_usd: 0.0,
    };

    // No metadata: not even a list.
    let err = svc
        .list_proposals(Request::new(ListProposalsRequest::default()))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::Unauthenticated);
    let mut bad = Request::new(ListProposalsRequest::default());
    principal(Role::Admin, "not-a-uuid").apply(&mut bad);
    assert_eq!(
        svc.list_proposals(bad).await.unwrap_err().code(),
        Code::Unauthenticated
    );

    // Viewers read, members write, admins own the settings.
    let err = svc
        .generate_proposal(as_role(Role::Viewer, WS_A, generate()))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::PermissionDenied);
    let err = svc
        .update_settings(as_role(
            Role::Member,
            WS_A,
            UpdateSettingsRequest {
                settings: Some(IntelligenceSettings {
                    cost_ceiling_usd: 100.0,
                    ..Default::default()
                }),
            },
        ))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::PermissionDenied);
    assert!(
        svc.list_proposals(as_role(Role::Viewer, WS_A, ListProposalsRequest::default()))
            .await
            .is_ok()
    );

    // A member of A proposes; every knowledge call carried that member.
    let proposal = svc
        .generate_proposal(as_role(Role::Member, WS_A, generate()))
        .await
        .unwrap()
        .into_inner();
    {
        let callers = peers.knowledge.callers.lock().unwrap();
        assert!(!callers.is_empty());
        assert!(callers.iter().all(|p| p == &principal(Role::Member, WS_A)));
    }

    // B cannot see, list, negotiate or accept it: the id alone is nothing.
    let from_b = |id: &str| GetProposalRequest { id: id.to_owned() };
    let err = svc
        .get_proposal(as_role(Role::Admin, WS_B, from_b(&proposal.id)))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::NotFound);
    let err = svc
        .negotiate(as_role(
            Role::Admin,
            WS_B,
            NegotiateRequest {
                proposal_id: proposal.id.clone(),
                content: "cheaper".into(),
            },
        ))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::NotFound);
    let err = svc
        .accept_proposal(as_role(
            Role::Admin,
            WS_B,
            AcceptProposalRequest {
                id: proposal.id.clone(),
                ..Default::default()
            },
        ))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::NotFound);
    let listed = svc
        .list_proposals(as_role(Role::Admin, WS_B, ListProposalsRequest::default()))
        .await
        .unwrap()
        .into_inner();
    assert!(listed.proposals.is_empty());
    let messages = svc
        .list_messages(as_role(
            Role::Admin,
            WS_B,
            ListMessagesRequest {
                proposal_id: proposal.id.clone(),
            },
        ))
        .await
        .unwrap()
        .into_inner();
    assert!(messages.messages.is_empty());
    let stats = svc
        .get_intelligence_stats(as_role(Role::Viewer, WS_B, GetIntelligenceStatsRequest {}))
        .await
        .unwrap()
        .into_inner();
    assert_eq!((stats.proposals, stats.messages), (0, 0));

    // Settings are per workspace.
    svc.update_settings(req(UpdateSettingsRequest {
        settings: Some(IntelligenceSettings {
            cost_ceiling_usd: 100.0,
            ..Default::default()
        }),
    }))
    .await
    .unwrap();
    let a = svc
        .get_settings(req(GetSettingsRequest {}))
        .await
        .unwrap()
        .into_inner();
    let b = svc
        .get_settings(as_role(Role::Viewer, WS_B, GetSettingsRequest {}))
        .await
        .unwrap()
        .into_inner();
    assert_eq!((a.cost_ceiling_usd, b.cost_ceiling_usd), (100.0, 500.0));

    // Events are scoped: B's watch stays silent while A moves.
    let mut events_a = svc
        .watch_events(as_role(Role::Viewer, WS_A, WatchEventsRequest {}))
        .await
        .unwrap()
        .into_inner();
    let mut events_b = svc
        .watch_events(as_role(Role::Viewer, WS_B, WatchEventsRequest {}))
        .await
        .unwrap()
        .into_inner();

    // Accepting forwards the member to execution and audits both writes.
    let accepted = svc
        .accept_proposal(as_role(
            Role::Member,
            WS_A,
            AcceptProposalRequest {
                id: proposal.id.clone(),
                option_id: String::new(),
                create_execution: true,
                adapter: String::new(),
            },
        ))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(accepted.status(), ProposalStatus::Accepted);
    assert_eq!(
        peers.execution.callers.lock().unwrap().last(),
        Some(&principal(Role::Member, WS_A))
    );
    assert_eq!(
        peers.identity.actions(),
        vec!["decision.create".to_owned(), "proposal.accept".to_owned()]
    );
    {
        let audit = peers.identity.audit.lock().unwrap();
        assert!(audit.iter().all(|(p, e)| p.workspace_id == WS_A
            && e.workspace_id == WS_A
            && e.user_id == "user-member"
            && e.at.is_some()));
        let (_, accept) = audit
            .iter()
            .find(|(_, e)| e.action == "proposal.accept")
            .unwrap();
        assert_eq!(
            (accept.entity.as_str(), accept.entity_id.as_str()),
            ("proposal", proposal.id.as_str())
        );
        assert!(accept.detail.contains("exec-1"));
    }
    let event = events_a.next().await.unwrap().unwrap();
    assert_eq!(
        event.data.get("workspace_id").map(String::as_str),
        Some(WS_A)
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(200), events_b.next())
            .await
            .is_err(),
        "B saw A's event"
    );

    // Export is complete and scoped; delete is real.
    let mismatch = svc
        .export_workspace_data(req(ExportWorkspaceDataRequest {
            workspace_id: WS_B.into(),
        }))
        .await
        .unwrap_err();
    assert_eq!(mismatch.code(), Code::PermissionDenied);
    let export = |role: Role| {
        as_role(
            role,
            WS_A,
            ExportWorkspaceDataRequest {
                workspace_id: WS_A.into(),
            },
        )
    };
    assert_eq!(
        svc.export_workspace_data(export(Role::Member))
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
    let mut records = Vec::new();
    let mut stream = svc
        .export_workspace_data(export(Role::Admin))
        .await
        .unwrap()
        .into_inner();
    while let Some(record) = stream.next().await {
        records.push(record.unwrap());
    }
    let entities: std::collections::BTreeSet<&str> =
        records.iter().map(|r| r.entity.as_str()).collect();
    assert_eq!(
        entities.into_iter().collect::<Vec<_>>(),
        vec!["decision", "message", "proposal", "settings"]
    );
    assert_eq!(records.iter().filter(|r| r.entity == "proposal").count(), 1);
    for record in &records {
        let json: serde_json::Value = serde_json::from_str(&record.json).unwrap();
        assert_eq!(json["workspace_id"], WS_A, "{}", record.entity);
    }
    let exported_proposal = records.iter().find(|r| r.entity == "proposal").unwrap();
    let json: serde_json::Value = serde_json::from_str(&exported_proposal.json).unwrap();
    assert_eq!(json["id"], proposal.id);
    assert!(json["options"].as_array().is_some_and(|o| !o.is_empty()));

    let delete = |role: Role, workspace: &str| {
        as_role(
            role,
            WS_A,
            DeleteWorkspaceDataRequest {
                workspace_id: workspace.to_owned(),
            },
        )
    };
    assert_eq!(
        svc.delete_workspace_data(delete(Role::Admin, WS_A))
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
    assert_eq!(
        svc.delete_workspace_data(delete(Role::Owner, WS_B))
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
    let deleted = svc
        .delete_workspace_data(delete(Role::Owner, WS_A))
        .await
        .unwrap()
        .into_inner()
        .deleted;
    assert_eq!(deleted["proposal"], 1);
    assert!(deleted["message"] >= 2);
    assert_eq!(deleted["decision"], 1);
    assert_eq!(deleted["settings"], 1);
    assert!(
        svc.list_proposals(req(ListProposalsRequest::default()))
            .await
            .unwrap()
            .into_inner()
            .proposals
            .is_empty()
    );
    let mut stream = svc
        .export_workspace_data(export(Role::Admin))
        .await
        .unwrap()
        .into_inner();
    assert!(stream.next().await.is_none(), "nothing left to export");
}

/// BYOK (CONTRACT.md §19): the workspace's data controls decide whether
/// Claude may be used at all, its own key is preferred, and the environment
/// key is only a fallback for a workspace that allows the reasoner.
#[tokio::test]
#[ignore = "needs the dev Postgres"]
async fn reasoner_follows_the_workspace_data_controls() {
    let (svc, peers) = service_with_peers("intelligence_test_byok").await;
    let caller = Caller::from_request(&req(())).unwrap();
    let settings = svc.settings(&caller).await.unwrap();

    // Off (the default): nothing, not even with an environment key.
    let access = svc.reasoner_access(&caller).await;
    assert!(!access.allowed && access.key.is_none());
    assert_eq!(svc.reasoner_for(&settings, &access).name(), "heuristic");
    let with_env = IntelligenceGrpc::new(
        svc.pool.clone(),
        peers.knowledge.clone(),
        peers.execution.clone(),
        peers.identity.clone(),
        ReasonerConfig {
            anthropic_api_key: Some("env-key".into()),
            anthropic_model: "env-model".into(),
        },
    );
    let access = with_env.reasoner_access(&caller).await;
    assert!(!access.configured());
    assert_eq!(
        with_env.reasoner_for(&settings, &access).name(),
        "heuristic"
    );
    let shown = with_env
        .get_settings(req(GetSettingsRequest {}))
        .await
        .unwrap()
        .into_inner();
    assert!(!shown.api_key_configured);

    // Allowed without a workspace key: the environment fallback applies.
    peers.identity.set_reasoner(WS_A, "claude", None);
    let access = svc.reasoner_access(&caller).await;
    assert!(access.allowed && !access.configured());
    assert_eq!(svc.reasoner_for(&settings, &access).name(), "heuristic");
    let access = with_env.reasoner_access(&caller).await;
    assert!(access.configured());
    assert_eq!(with_env.reasoner_for(&settings, &access).name(), "claude");
    assert_eq!(with_env.effective_model(&settings, &access), "env-model");

    // The workspace's own key wins and brings its model along.
    peers.identity.set_reasoner(WS_A, "claude", Some("ws-key"));
    peers.identity.keys.lock().unwrap().insert(
        WS_A.to_owned(),
        ReasonerKey {
            api_key: "ws-key".into(),
            model: "ws-model".into(),
        },
    );
    let access = with_env.reasoner_access(&caller).await;
    assert_eq!(
        access.key.as_ref().map(|k| k.api_key.as_str()),
        Some("ws-key")
    );
    assert_eq!(with_env.effective_model(&settings, &access), "ws-model");
    let shown = svc
        .get_settings(req(GetSettingsRequest {}))
        .await
        .unwrap()
        .into_inner();
    assert!(shown.api_key_configured);
    assert_eq!(shown.model, "ws-model");
    // A model chosen on the settings page beats the key's.
    let chosen = svc
        .update_settings(req(UpdateSettingsRequest {
            settings: Some(IntelligenceSettings {
                model: "chosen-model".into(),
                ..Default::default()
            }),
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(chosen.model, "chosen-model");

    // Another workspace's controls do not leak.
    let other = Caller::from_request(&as_role(Role::Admin, WS_B, ())).unwrap();
    let access = with_env.reasoner_access(&other).await;
    assert!(!access.allowed);

    // The settings switch can still turn it off per workspace.
    svc.update_settings(req(UpdateSettingsRequest {
        settings: Some(IntelligenceSettings {
            reasoner: "heuristic".into(),
            ..Default::default()
        }),
    }))
    .await
    .unwrap();
    let settings = svc.settings(&caller).await.unwrap();
    let access = svc.reasoner_access(&caller).await;
    assert!(access.configured());
    assert_eq!(svc.reasoner_for(&settings, &access).name(), "heuristic");
}

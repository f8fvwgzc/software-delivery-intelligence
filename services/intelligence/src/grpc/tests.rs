//! End-to-end flow through the service with fake peers and a real Postgres
//! (the dev database, isolated in its own schema). Ignored by default: run
//! `cargo test -p sdi-intelligence -- --ignored` with the database up.

use std::{sync::Arc, time::Duration};

use sdi_common::principal::{Principal, Role};
use sdi_proto::{
    common::v1::{PlatformEvent, WatchEventsRequest},
    identity::v1::ReasonerKey,
    intelligence::v1::{
        AcceptProposalRequest, CreateDecisionRequest, DecideFindingRequest, DecisionScope,
        DecisionSource, DeleteDiagramRequest, DeleteWorkspaceDataRequest, DiagramKind, DiagramSpec,
        DocumentKind, ExportWorkspaceDataRequest, FindingDecision, GenerateDiagramRequest,
        GenerateDocumentRequest, GenerateProposalRequest, GetDiagramRequest,
        GetIntelligenceStatsRequest, GetProposalRequest, GetReviewRequest, GetSettingsRequest,
        IntelligenceSettings, ListDecisionsRequest, ListDiagramsRequest, ListMessagesRequest,
        ListProposalsRequest, ListReviewActivityRequest, ListReviewsRequest, MessageRole,
        NegotiateRequest, ProposalKind, ProposalStatus, PublishReviewRequest, ReviewStatus,
        ScoreWeights, StartReviewRequest, SupersedeDecisionRequest, UpdateDiagramRequest,
        UpdateSettingsRequest, intelligence_service_server::IntelligenceService,
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
        "review_learnings",
        "reasoner_usage",
        "osv_cache",
        "review_activity",
        "findings",
        "reviews",
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
            env: crate::config::ReasonerEnv {
                model: crate::config::DEFAULT_MODEL.to_owned(),
                ..Default::default()
            },
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

/// A deployment proposal with its artifacts filled in: `GenerateProposal`
/// answers before the files exist (CONTRACT.md §6 Realtime), so this waits
/// for the background step the way the web app does.
async fn propose(svc: &IntelligenceGrpc, budget: f64) -> sdi_proto::intelligence::v1::Proposal {
    let proposal = svc
        .generate_proposal(req(GenerateProposalRequest {
            project_id: "proj-1".into(),
            kind: ProposalKind::Deployment as i32,
            prompt: String::new(),
            budget_monthly_usd: budget,
        }))
        .await
        .unwrap()
        .into_inner();
    settled(svc, &proposal.id).await
}

/// The proposal once every option carries its generated files.
async fn settled(svc: &IntelligenceGrpc, id: &str) -> sdi_proto::intelligence::v1::Proposal {
    for _ in 0..200 {
        let proposal = svc
            .get_proposal(req(GetProposalRequest { id: id.to_owned() }))
            .await
            .unwrap()
            .into_inner();
        if proposal.options.iter().all(|o| !o.artifacts.is_empty()) {
            return proposal;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("proposal {id} never got its artifacts");
}

/// The next event that is not generation progress (`progress`, or the
/// `updated` that closes the artifact step), which the older flows below
/// do not care about.
async fn next_event(
    events: &mut tokio_stream::wrappers::ReceiverStream<Result<PlatformEvent, tonic::Status>>,
) -> PlatformEvent {
    loop {
        let event = tokio::time::timeout(Duration::from_secs(10), events.next())
            .await
            .expect("an event within 10 s")
            .unwrap()
            .unwrap();
        let step = event.data.get("step").map(String::as_str);
        if event.action == "progress" || (event.action == "updated" && step == Some("done")) {
            continue;
        }
        return event;
    }
}

async fn propose_raw(svc: &IntelligenceGrpc, budget: f64) -> sdi_proto::intelligence::v1::Proposal {
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
                auto_review: false,
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
                ..Default::default()
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

    let created = next_event(&mut events).await;
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
    let updated = next_event(&mut events).await;
    assert_eq!(
        (updated.action.as_str(), updated.status.as_str()),
        ("updated", "negotiating")
    );
    assert_eq!(updated.data.get("revision").map(String::as_str), Some("2"));

    // List responses omit artifacts; the single get carries them.
    let listed = svc
        .list_proposals(req(ListProposalsRequest {
            project_id: "proj-1".into(),
            ..Default::default()
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
    let created = next_event(&mut events).await;
    assert_eq!(created.entity_id, newer.id);
    let superseded = next_event(&mut events).await;
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
    let event = next_event(&mut events).await;
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
            ..Default::default()
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
    let event = next_event(&mut events_a).await;
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
    let access = svc.reasoner_access(&caller, &settings).await;
    assert!(!access.allowed && access.key.is_none());
    assert_eq!(
        svc.reasoner_for(&caller, &settings, &access).name(),
        "heuristic"
    );
    let with_env = IntelligenceGrpc::new(
        svc.pool.clone(),
        peers.knowledge.clone(),
        peers.execution.clone(),
        peers.identity.clone(),
        ReasonerConfig {
            env: crate::config::ReasonerEnv {
                api_key: Some("env-key".into()),
                model: "env-model".into(),
                ..Default::default()
            },
        },
    );
    let access = with_env.reasoner_access(&caller, &settings).await;
    assert!(!access.configured());
    assert_eq!(
        with_env.reasoner_for(&caller, &settings, &access).name(),
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
    let access = svc.reasoner_access(&caller, &settings).await;
    assert!(access.allowed && !access.configured());
    assert_eq!(
        svc.reasoner_for(&caller, &settings, &access).name(),
        "heuristic"
    );
    let access = with_env.reasoner_access(&caller, &settings).await;
    assert!(access.configured());
    assert_eq!(
        with_env.reasoner_for(&caller, &settings, &access).name(),
        "claude"
    );
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
    let access = with_env.reasoner_access(&caller, &settings).await;
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
    let access = with_env.reasoner_access(&other, &settings).await;
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
    let access = svc.reasoner_access(&caller, &settings).await;
    assert!(access.configured());
    assert_eq!(
        svc.reasoner_for(&caller, &settings, &access).name(),
        "heuristic"
    );
}

/// A pull request against the fixture project: a new Dockerfile with a
/// floating tag and a source change without tests.
fn pull_request_fixture(number: u64, head_sha: &str) -> sdi_proto::knowledge::v1::PullRequest {
    use sdi_proto::knowledge::v1::{ChangedFile, DiffHunk, PullRequestSummary};
    sdi_proto::knowledge::v1::PullRequest {
        summary: Some(PullRequestSummary {
            project_id: "proj-1".into(),
            number,
            title: "Containerise the API".into(),
            author: "ana".into(),
            state: "open".into(),
            base_ref: "main".into(),
            head_ref: "feat/docker".into(),
            head_sha: head_sha.into(),
            changed_files: 2,
            additions: 8,
            deletions: 0,
            html_url: format!("https://github.com/f8fvwgzc/view-engine/pull/{number}"),
            ..Default::default()
        }),
        body: "Adds a Dockerfile so the API can run in the cluster.".into(),
        files: vec![
            ChangedFile {
                path: "Dockerfile".into(),
                status: "added".into(),
                additions: 2,
                hunks: vec![DiffHunk {
                    new_start: 1,
                    new_lines: 2,
                    lines: vec![
                        "+FROM node:latest".into(),
                        "+CMD [\"node\", \"server.js\"]".into(),
                    ],
                    ..Default::default()
                }],
                ..Default::default()
            },
            ChangedFile {
                path: "src/server.ts".into(),
                status: "modified".into(),
                additions: 6,
                hunks: vec![DiffHunk {
                    new_start: 1,
                    new_lines: 6,
                    lines: (1..=6).map(|i| format!("+const v{i} = {i};")).collect(),
                    ..Default::default()
                }],
                ..Default::default()
            },
        ],
        touched_fact_ids: vec!["f-docker".into()],
        impacted_node_ids: vec![],
    }
}

#[tokio::test]
#[ignore = "needs the dev Postgres"]
async fn review_pipeline_proposes_decides_and_refuses_to_publish_without_github() {
    let (svc, peers) = service_with_peers("intelligence_test_review").await;
    // The fake knowledge needs the pull request; rebuild it with one.
    let mut context = fixtures::context();
    context.capabilities.push("tests:unit".into());
    let knowledge = Arc::new(FakeKnowledge {
        context: Some(context),
        patterns: fixtures::catalog(),
        pull_requests: [(7, pull_request_fixture(7, "abc123"))].into(),
        ..Default::default()
    });
    let svc = IntelligenceGrpc::new(
        svc.pool.clone(),
        knowledge.clone(),
        peers.execution.clone(),
        peers.identity.clone(),
        ReasonerConfig::default(),
    );

    // A viewer may not start one; a member may.
    let denied = svc
        .start_review(as_role(
            Role::Viewer,
            WS_A,
            StartReviewRequest {
                project_id: "proj-1".into(),
                pr_number: 7,
            },
        ))
        .await
        .unwrap_err();
    assert_eq!(denied.code(), Code::PermissionDenied);
    let started = svc
        .start_review(as_role(
            Role::Member,
            WS_A,
            StartReviewRequest {
                project_id: "proj-1".into(),
                pr_number: 7,
            },
        ))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(started.status(), ReviewStatus::Context);
    assert_eq!(started.pr_number, 7);
    assert_eq!(started.head_sha, "abc123");
    assert_eq!(started.requested_by, "user-member");
    assert_eq!(started.steps.len(), 5);

    // The pipeline runs in the background; wait for it to propose.
    let mut review = started.clone();
    for _ in 0..100 {
        review = svc
            .get_review(req(GetReviewRequest {
                id: started.id.clone(),
            }))
            .await
            .unwrap()
            .into_inner();
        if matches!(
            review.status(),
            ReviewStatus::Proposed | ReviewStatus::Failed
        ) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(review.status(), ReviewStatus::Proposed, "{}", review.error);
    assert_eq!(review.project_name, "view-engine");
    assert_eq!(review.reasoner, "heuristic");
    assert_eq!(review.files_reviewed, 2);
    assert!(review.summary.contains("## Intent"));
    assert!(review.summary.contains("docker.latest_tag"));
    let steps: Vec<(&str, &str)> = review
        .steps
        .iter()
        .map(|s| (s.key.as_str(), s.status.as_str()))
        .collect();
    assert_eq!(
        steps,
        vec![
            ("context", "done"),
            ("analyze", "done"),
            ("findings", "done"),
            ("summary", "done"),
            ("publish", "pending")
        ]
    );
    let rules: Vec<&str> = review.findings.iter().map(|f| f.rule.as_str()).collect();
    assert!(rules.contains(&"docker.latest_tag"), "{rules:?}");
    assert!(rules.contains(&"tests.missing"), "{rules:?}");
    assert!(rules.contains(&"docker.no_healthcheck"), "{rules:?}");
    assert_eq!(review.findings_total as usize, review.findings.len());

    // Starting again for the same head returns the same review.
    let again = svc
        .start_review(as_role(
            Role::Member,
            WS_A,
            StartReviewRequest {
                project_id: "proj-1".into(),
                pr_number: 7,
            },
        ))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(again.id, started.id);
    assert_eq!(again.status(), ReviewStatus::Proposed);

    // Lists omit the summary and findings; another workspace sees nothing.
    let listed = svc
        .list_reviews(req(ListReviewsRequest {
            project_id: String::new(),
            limit: 0,
        }))
        .await
        .unwrap()
        .into_inner()
        .reviews;
    assert_eq!(listed.len(), 1);
    assert!(listed[0].summary.is_empty());
    assert!(listed[0].findings.is_empty());
    assert!(
        svc.list_reviews(as_role(Role::Admin, WS_B, ListReviewsRequest::default()))
            .await
            .unwrap()
            .into_inner()
            .reviews
            .is_empty()
    );
    assert_eq!(
        svc.get_review(as_role(
            Role::Admin,
            WS_B,
            GetReviewRequest {
                id: started.id.clone()
            }
        ))
        .await
        .unwrap_err()
        .code(),
        Code::NotFound
    );

    // Decide: accept the docker finding, dismiss the tests one.
    let docker = review
        .findings
        .iter()
        .find(|f| f.rule == "docker.latest_tag")
        .unwrap();
    let decided = svc
        .decide_finding(as_role(
            Role::Member,
            WS_A,
            DecideFindingRequest {
                finding_id: docker.id.clone(),
                decision: FindingDecision::Accepted as i32,
                note: "pin it".into(),
            },
        ))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(decided.decision(), FindingDecision::Accepted);
    assert_eq!(decided.decided_by, "user-member");
    assert_eq!(decided.decision_note, "pin it");
    let tests_finding = review
        .findings
        .iter()
        .find(|f| f.rule == "tests.missing")
        .unwrap();
    svc.decide_finding(req(DecideFindingRequest {
        finding_id: tests_finding.id.clone(),
        decision: FindingDecision::Dismissed as i32,
        note: String::new(),
    }))
    .await
    .unwrap();
    assert_eq!(
        svc.decide_finding(req(DecideFindingRequest {
            finding_id: docker.id.clone(),
            decision: FindingDecision::Pending as i32,
            note: String::new(),
        }))
        .await
        .unwrap_err()
        .code(),
        Code::InvalidArgument
    );
    let review = svc
        .get_review(req(GetReviewRequest {
            id: started.id.clone(),
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(review.findings_accepted, 1);

    // Publishing needs an approver and GitHub access; the fake identity has
    // none, so it fails before anything is posted and the review stays proposed.
    assert_eq!(
        svc.publish_review(as_role(
            Role::Member,
            WS_A,
            PublishReviewRequest {
                review_id: started.id.clone(),
                event: String::new(),
            }
        ))
        .await
        .unwrap_err()
        .code(),
        Code::PermissionDenied
    );
    let refused = svc
        .publish_review(as_role(
            Role::Approver,
            WS_A,
            PublishReviewRequest {
                review_id: started.id.clone(),
                event: "approve".into(),
            },
        ))
        .await
        .unwrap_err();
    assert_eq!(refused.code(), Code::FailedPrecondition, "{refused}");
    assert_eq!(
        svc.publish_review(req(PublishReviewRequest {
            review_id: started.id.clone(),
            event: "merge".into(),
        }))
        .await
        .unwrap_err()
        .code(),
        Code::InvalidArgument
    );
    let review = svc
        .get_review(req(GetReviewRequest {
            id: started.id.clone(),
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(review.status(), ReviewStatus::Proposed);

    // The activity log tells the story, newest first, and pages by `before`.
    let activity = svc
        .list_review_activity(req(ListReviewActivityRequest {
            project_id: "proj-1".into(),
            limit: 0,
            before: None,
        }))
        .await
        .unwrap()
        .into_inner()
        .activity;
    let actions: Vec<&str> = activity.iter().map(|a| a.action.as_str()).collect();
    assert_eq!(
        actions,
        vec![
            "finding.dismissed",
            "finding.accepted",
            "review.proposed",
            "review.started"
        ]
    );
    assert_eq!(activity[1].actor, "user-member");
    assert!(activity[1].detail.contains("pin it"));
    let older = svc
        .list_review_activity(req(ListReviewActivityRequest {
            project_id: String::new(),
            limit: 10,
            before: activity[2].at,
        }))
        .await
        .unwrap()
        .into_inner()
        .activity;
    assert_eq!(older.len(), 1);
    assert_eq!(older[0].action, "review.started");

    // A new head commit is a new review that links the previous one.
    let knowledge2 = Arc::new(FakeKnowledge {
        context: knowledge.context.clone(),
        patterns: fixtures::catalog(),
        pull_requests: [(7, pull_request_fixture(7, "def456"))].into(),
        ..Default::default()
    });
    let svc2 = IntelligenceGrpc::new(
        svc.pool.clone(),
        knowledge2,
        peers.execution.clone(),
        peers.identity.clone(),
        ReasonerConfig::default(),
    );
    let next = svc2
        .start_review(req(StartReviewRequest {
            project_id: "proj-1".into(),
            pr_number: 7,
        }))
        .await
        .unwrap()
        .into_inner();
    assert_ne!(next.id, started.id);
    assert_eq!(next.head_sha, "def456");
    let listed = svc2
        .list_reviews(req(ListReviewsRequest {
            project_id: "proj-1".into(),
            limit: 1,
        }))
        .await
        .unwrap()
        .into_inner()
        .reviews;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, next.id);

    // Settings carry the auto_review switch.
    let settings = svc
        .update_settings(req(UpdateSettingsRequest {
            settings: Some(IntelligenceSettings {
                auto_review: true,
                ..Default::default()
            }),
        }))
        .await
        .unwrap()
        .into_inner();
    assert!(settings.auto_review);
    assert!(
        !svc.get_settings(as_role(Role::Viewer, WS_B, GetSettingsRequest {}))
            .await
            .unwrap()
            .into_inner()
            .auto_review
    );
}

/// Reviews until the pipeline has proposed (or failed).
async fn wait_for_review(svc: &IntelligenceGrpc, id: &str) -> sdi_proto::intelligence::v1::Review {
    for _ in 0..200 {
        let review = svc
            .get_review(req(GetReviewRequest { id: id.to_owned() }))
            .await
            .unwrap()
            .into_inner();
        if matches!(
            review.status(),
            ReviewStatus::Proposed | ReviewStatus::Failed
        ) {
            return review;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("review {id} never finished");
}

fn review_service(svc: &IntelligenceGrpc, peers: &Peers, shas: &[&str]) -> IntelligenceGrpc {
    let mut context = fixtures::context();
    context.capabilities.push("tests:unit".into());
    let knowledge = Arc::new(FakeKnowledge {
        context: Some(context),
        patterns: fixtures::catalog(),
        pull_requests: shas
            .iter()
            .enumerate()
            .map(|(i, sha)| (i as u64 + 1, pull_request_fixture(i as u64 + 1, sha)))
            .collect(),
        ..Default::default()
    });
    IntelligenceGrpc::new(
        svc.pool.clone(),
        knowledge,
        peers.execution.clone(),
        peers.identity.clone(),
        ReasonerConfig::default(),
    )
}

/// CONTRACT.md §20 learnings: a rule dismissed three times with no
/// acceptance is suppressed on the next review of the project; a rule the
/// project accepted orders above its peers; a changed verdict moves the
/// count instead of counting twice; the ledger is part of the export.
#[tokio::test]
#[ignore = "needs the dev Postgres"]
async fn review_learnings_suppress_dismissed_rules_and_lift_accepted_ones() {
    let (svc, peers) = service_with_peers("intelligence_test_learnings").await;
    let svc = review_service(&svc, &peers, &["sha1", "sha2", "sha3", "sha4"]);
    let start = |n: u64| {
        as_role(
            Role::Member,
            WS_A,
            StartReviewRequest {
                project_id: "proj-1".into(),
                pr_number: n,
            },
        )
    };
    let decide = |id: &str, decision: FindingDecision, note: &str| {
        as_role(
            Role::Member,
            WS_A,
            DecideFindingRequest {
                finding_id: id.to_owned(),
                decision: decision as i32,
                note: note.to_owned(),
            },
        )
    };
    let mut first_order: Option<Vec<String>> = None;
    for n in 1..=3u64 {
        let started = svc.start_review(start(n)).await.unwrap().into_inner();
        let review = wait_for_review(&svc, &started.id).await;
        assert_eq!(review.status(), ReviewStatus::Proposed, "{}", review.error);
        let latest = review
            .findings
            .iter()
            .find(|f| f.rule == "docker.latest_tag")
            .expect("latest tag finding");
        assert_eq!(
            latest.severity(),
            sdi_proto::intelligence::v1::FindingSeverity::Medium
        );
        assert!(!latest.detail.contains("Suppressed"));
        if first_order.is_none() {
            first_order = Some(review.findings.iter().map(|f| f.rule.clone()).collect());
        }
        // Dismiss it; the first time by way of an accept that is reversed,
        // which must leave accepted at 0.
        if n == 1 {
            svc.decide_finding(decide(&latest.id, FindingDecision::Accepted, "hm"))
                .await
                .unwrap();
        }
        svc.decide_finding(decide(
            &latest.id,
            FindingDecision::Dismissed,
            "we pin elsewhere",
        ))
        .await
        .unwrap();
        if n == 1 {
            let tests = review
                .findings
                .iter()
                .find(|f| f.rule == "tests.missing")
                .expect("tests finding");
            svc.decide_finding(decide(&tests.id, FindingDecision::Accepted, "yes"))
                .await
                .unwrap();
        }
    }
    let rows = crate::db::learnings::for_project(
        &svc.pool,
        uuid::Uuid::parse_str(WS_A).unwrap(),
        "proj-1",
    )
    .await
    .unwrap();
    let counts: Vec<(&str, i32, i32)> = rows
        .iter()
        .map(|r| (r.rule.as_str(), r.accepted, r.dismissed))
        .collect();
    assert_eq!(
        counts,
        vec![("docker.latest_tag", 0, 3), ("tests.missing", 1, 0)]
    );

    let started = svc.start_review(start(4)).await.unwrap().into_inner();
    let review = wait_for_review(&svc, &started.id).await;
    assert_eq!(review.status(), ReviewStatus::Proposed, "{}", review.error);
    let latest = review
        .findings
        .iter()
        .find(|f| f.rule == "docker.latest_tag")
        .expect("still emitted");
    assert_eq!(
        latest.severity(),
        sdi_proto::intelligence::v1::FindingSeverity::Info
    );
    assert!(
        latest.detail.contains("Suppressed by project learnings"),
        "{}",
        latest.detail
    );
    assert!(
        latest
            .detail
            .contains("dismissed `docker.latest_tag` 3 times")
    );
    // The accepted rule now leads, ahead of the medium docker finding it
    // used to follow.
    assert_eq!(review.findings[0].rule, "tests.missing");
    let before = first_order.unwrap();
    assert_ne!(before[0], "tests.missing");
    let analyze = review.steps.iter().find(|s| s.key == "analyze").unwrap();
    assert!(
        analyze.detail.contains("1 suppressed by project learnings"),
        "{}",
        analyze.detail
    );
    let context = review.steps.iter().find(|s| s.key == "context").unwrap();
    assert!(
        context.detail.contains("2 learned rules"),
        "{}",
        context.detail
    );

    // Part of the workspace's data: exported, and gone after a delete.
    let mut stream = svc
        .export_workspace_data(req(ExportWorkspaceDataRequest {
            workspace_id: WS_A.into(),
        }))
        .await
        .unwrap()
        .into_inner();
    let mut learnings = 0;
    while let Some(record) = stream.next().await {
        if record.unwrap().entity == "review_learning" {
            learnings += 1;
        }
    }
    assert_eq!(learnings, 2);
    let deleted = svc
        .delete_workspace_data(as_role(
            Role::Owner,
            WS_A,
            DeleteWorkspaceDataRequest {
                workspace_id: WS_A.into(),
            },
        ))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(deleted.deleted.get("review_learning"), Some(&2));
}

/// A recorded decision changes the next proposal's ranking with no rescan:
/// the fake knowledge never changes, only our decision memory does. And
/// what negotiation added as a constraint stays through later turns.
#[tokio::test]
#[ignore = "needs the dev Postgres"]
async fn decisions_rerank_without_a_rescan_and_constraints_persist() {
    let (svc, _, _) = service("intelligence_test_decisions").await;
    let before = propose_raw(&svc, 0.0).await;
    let policy_of = |p: &sdi_proto::intelligence::v1::Proposal, id: &str| {
        p.options
            .iter()
            .find(|o| o.pattern_id == id)
            .map(|o| o.score.as_ref().unwrap().policy)
    };
    assert_eq!(policy_of(&before, "azure-container-apps"), Some(0.6));
    assert_eq!(policy_of(&before, "aws-ecs-fargate"), Some(0.6));

    svc.create_decision(req(CreateDecisionRequest {
        project_id: String::new(),
        scope: DecisionScope::Organization as i32,
        title: "We run on AWS".into(),
        statement: "Everything deploys to AWS.".into(),
        reasons: vec!["platform team".into()],
        tags: vec!["cloud:aws".into()],
        approved_by: String::new(),
    }))
    .await
    .unwrap();

    let after = propose_raw(&svc, 0.0).await;
    assert_eq!(policy_of(&after, "azure-container-apps"), Some(0.0));
    assert_eq!(policy_of(&after, "aws-ecs-fargate"), Some(1.0));
    assert_ne!(
        before.options[0].pattern_id, after.options[0].pattern_id,
        "the recommendation follows the decision"
    );
    assert_eq!(after.options[0].pattern_id, "aws-ecs-fargate");

    // Constraints survive every later revision.
    let turn = svc
        .negotiate(req(NegotiateRequest {
            proposal_id: after.id.clone(),
            content: "we need private networking".into(),
        }))
        .await
        .unwrap()
        .into_inner();
    let keys = |p: &sdi_proto::intelligence::v1::Proposal| -> Vec<String> {
        p.constraints.iter().map(|c| c.key.clone()).collect()
    };
    let first = turn.proposal.unwrap();
    assert_eq!(first.revision, 2);
    assert!(
        keys(&first).iter().any(|k| k.starts_with("require:")),
        "{:?}",
        keys(&first)
    );
    let turn = svc
        .negotiate(req(NegotiateRequest {
            proposal_id: after.id.clone(),
            content: "cheaper, and not kubernetes".into(),
        }))
        .await
        .unwrap()
        .into_inner();
    let second = turn.proposal.unwrap();
    assert_eq!(second.revision, 3);
    let k = keys(&second);
    assert!(k.iter().any(|x| x.starts_with("require:")), "{k:?}");
    assert!(k.iter().any(|x| x.starts_with("exclude:")), "{k:?}");
    assert!(
        second
            .options
            .iter()
            .all(|o| o.pattern_id != "kubernetes-helm")
    );
    assert_eq!(policy_of(&second, "aws-ecs-fargate"), Some(1.0));
}

/// CONTRACT.md §6 Realtime: the answer comes back within two seconds for
/// fifty candidate patterns, progress is streamed, and the artifacts
/// arrive afterwards with an `updated` event.
#[tokio::test]
#[ignore = "needs the dev Postgres"]
async fn generate_proposal_answers_fast_and_fills_artifacts_in_the_background() {
    let (svc, peers) = service_with_peers("intelligence_test_realtime").await;
    let mut patterns = fixtures::catalog();
    for i in 0..50 {
        let cloud = ["aws", "azure", "gcp", "any"][i % 4];
        patterns.push(fixtures::pattern(
            &format!("candidate-{i:02}"),
            cloud,
            "serverless_container",
            &["dockerfile"],
            20.0 + i as f64,
        ));
    }
    assert!(patterns.len() >= 50);
    let mut context = fixtures::context();
    context.understanding = Some(sdi_proto::knowledge::v1::Understanding {
        commit_sha: "0123456789abcdef".into(),
        ..Default::default()
    });
    let knowledge = Arc::new(FakeKnowledge {
        context: Some(context),
        patterns,
        ..Default::default()
    });
    let svc = IntelligenceGrpc::new(
        svc.pool.clone(),
        knowledge,
        peers.execution.clone(),
        peers.identity.clone(),
        ReasonerConfig::default(),
    );
    let mut events = svc
        .watch_events(req(WatchEventsRequest {}))
        .await
        .unwrap()
        .into_inner();

    let started = std::time::Instant::now();
    let proposal = propose_raw(&svc, 0.0).await;
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(2),
        "GenerateProposal took {elapsed:?}"
    );
    assert_eq!(proposal.status(), ProposalStatus::Proposed);
    assert_eq!(proposal.options.len(), 5);
    assert!(
        proposal.options.iter().all(|o| o.artifacts.is_empty()),
        "files are generated after the answer"
    );
    assert!(
        proposal
            .options
            .iter()
            .all(|o| !o.cost_breakdown.is_empty())
    );

    let mut seen: Vec<(String, String, String)> = Vec::new();
    loop {
        let event = tokio::time::timeout(Duration::from_secs(10), events.next())
            .await
            .expect("events keep coming")
            .unwrap()
            .unwrap();
        assert_eq!(event.entity_id, proposal.id);
        let step = event.data.get("step").cloned().unwrap_or_default();
        seen.push((event.action.clone(), event.status.clone(), step.clone()));
        if event.action == "updated" && step == "done" {
            assert_eq!(event.status, "proposed");
            assert_eq!(event.data.get("revision").map(String::as_str), Some("1"));
            break;
        }
    }
    assert_eq!(
        seen,
        vec![
            (
                "progress".to_owned(),
                "generating".to_owned(),
                "retrieving_context".to_owned()
            ),
            (
                "progress".to_owned(),
                "generating".to_owned(),
                "ranking".to_owned()
            ),
            ("created".to_owned(), "proposed".to_owned(), String::new()),
            (
                "progress".to_owned(),
                "generating".to_owned(),
                "generating_artifacts".to_owned()
            ),
            (
                "updated".to_owned(),
                "proposed".to_owned(),
                "done".to_owned()
            ),
        ]
    );
    let filled = svc
        .get_proposal(req(GetProposalRequest {
            id: proposal.id.clone(),
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(filled.revision, 1);
    // The synthetic candidates have no generator; the catalog ones do.
    assert!(
        filled
            .options
            .iter()
            .filter(|o| !o.pattern_id.starts_with("candidate-"))
            .all(|o| !o.artifacts.is_empty())
    );
    assert!(filled.options.iter().any(|o| !o.artifacts.is_empty()));
    assert_eq!(
        filled.options.iter().map(|o| &o.id).collect::<Vec<_>>(),
        proposal.options.iter().map(|o| &o.id).collect::<Vec<_>>(),
        "the background step changes nothing but the files"
    );
}

/// The reasoner is any provider: the settings carry provider, endpoint,
/// model and the daily token cap; an OpenAI-compatible endpoint works
/// without a key; the stats show the day's spend against the cap.
#[tokio::test]
#[ignore = "needs the dev Postgres"]
async fn settings_carry_the_provider_and_the_token_cap() {
    let (svc, peers) = service_with_peers("intelligence_test_provider").await;
    let caller = Caller::from_request(&req(())).unwrap();
    let shown = svc
        .get_settings(req(GetSettingsRequest {}))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(shown.reasoner, "claude");
    assert_eq!(shown.reasoner_provider, "anthropic");
    assert_eq!(shown.reasoner_daily_token_cap, 200_000);
    assert_eq!(shown.reasoner_model, shown.model);

    for (provider, url, message) in [
        ("carrier-pigeon", "", "reasoner_provider"),
        ("openai_compatible", "ollama:11434", "http://"),
    ] {
        let error = svc
            .update_settings(req(UpdateSettingsRequest {
                settings: Some(IntelligenceSettings {
                    reasoner_provider: provider.into(),
                    reasoner_base_url: url.into(),
                    ..Default::default()
                }),
            }))
            .await
            .unwrap_err();
        assert_eq!(error.code(), Code::InvalidArgument);
        assert!(error.message().contains(message), "{error}");
    }

    let updated = svc
        .update_settings(req(UpdateSettingsRequest {
            settings: Some(IntelligenceSettings {
                reasoner: "llm".into(),
                reasoner_provider: "openai_compatible".into(),
                reasoner_base_url: "http://ollama:11434/v1/".into(),
                reasoner_model: "llama3".into(),
                reasoner_daily_token_cap: 5_000,
                ..Default::default()
            }),
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(updated.reasoner_provider, "openai_compatible");
    assert_eq!(updated.reasoner_base_url, "http://ollama:11434/v1");
    assert_eq!(
        (updated.model.as_str(), updated.reasoner_model.as_str()),
        ("llama3", "llama3")
    );
    assert_eq!(updated.reasoner_daily_token_cap, 5_000);
    // Not allowed by the data controls yet: heuristic, not configured.
    assert!(!updated.api_key_configured);
    let settings = svc.settings(&caller).await.unwrap();
    let access = svc.reasoner_access(&caller, &settings).await;
    assert_eq!(
        svc.reasoner_for(&caller, &settings, &access).name(),
        "heuristic"
    );

    // Allowed, no key: a local OpenAI-compatible endpoint needs none.
    peers.identity.set_reasoner(WS_A, "claude", None);
    let access = svc.reasoner_access(&caller, &settings).await;
    assert!(access.configured());
    assert_eq!(svc.reasoner_for(&caller, &settings, &access).name(), "llm");
    let shown = svc
        .get_settings(req(GetSettingsRequest {}))
        .await
        .unwrap()
        .into_inner();
    assert!(shown.api_key_configured);

    // Back to Anthropic: now a key is required again.
    svc.update_settings(req(UpdateSettingsRequest {
        settings: Some(IntelligenceSettings {
            reasoner_provider: "anthropic".into(),
            ..Default::default()
        }),
    }))
    .await
    .unwrap();
    let settings = svc.settings(&caller).await.unwrap();
    let access = svc.reasoner_access(&caller, &settings).await;
    assert!(!access.configured());
    assert_eq!(
        svc.reasoner_for(&caller, &settings, &access).name(),
        "heuristic"
    );
    peers.identity.set_reasoner(WS_A, "claude", Some("ws-key"));
    let access = svc.reasoner_access(&caller, &settings).await;
    assert_eq!(
        svc.reasoner_for(&caller, &settings, &access).name(),
        "claude"
    );

    // Usage shows up in the stats, per workspace.
    let ws = uuid::Uuid::parse_str(WS_A).unwrap();
    crate::db::usage::add(&svc.pool, ws, 1_200, 300)
        .await
        .unwrap();
    crate::db::usage::add(&svc.pool, ws, 100, 50).await.unwrap();
    let stats = svc
        .get_intelligence_stats(req(GetIntelligenceStatsRequest {}))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(stats.reasoner_tokens_today, 1_650);
    assert_eq!(stats.reasoner_calls_today, 2);
    assert_eq!(stats.reasoner_daily_token_cap, 5_000);
    let other = svc
        .get_intelligence_stats(as_role(Role::Viewer, WS_B, GetIntelligenceStatsRequest {}))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(other.reasoner_tokens_today, 0);
    assert_eq!(other.reasoner_daily_token_cap, 200_000);
    // The ledger the provider uses reads the same rows.
    let ledger = crate::db::usage::PgUsageLedger {
        pool: svc.pool.clone(),
    };
    use crate::db::usage::UsageLedger;
    assert_eq!(ledger.used_today(ws).await, 1_650);
}

/// OSV (CONTRACT.md §20): added exact pins are looked up in one batch, the
/// advisories' details fetched once, everything cached for a day, and a
/// broken endpoint costs nothing but the finding.
#[tokio::test]
#[ignore = "needs the dev Postgres"]
async fn osv_check_queries_once_caches_and_fails_soft() {
    use std::sync::Mutex;

    use axum::{
        Router,
        extract::State,
        routing::{get, post},
    };
    use serde_json::{Value, json};

    use crate::domain::review::rules::dependencies::AddedDependency;

    let (svc, _, _) = service("intelligence_test_osv").await;

    #[derive(Default)]
    struct Seen {
        batches: Vec<Value>,
        details: Vec<String>,
        broken: bool,
    }
    type Shared = Arc<Mutex<Seen>>;
    async fn batch(
        State(seen): State<Shared>,
        axum::Json(body): axum::Json<Value>,
    ) -> (axum::http::StatusCode, axum::Json<Value>) {
        let mut seen = seen.lock().unwrap();
        if seen.broken {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({})),
            );
        }
        let n = body["queries"].as_array().map(Vec::len).unwrap_or(0);
        seen.batches.push(body.clone());
        let results: Vec<Value> = (0..n)
            .map(|i| {
                let name = body["queries"][i]["package"]["name"].as_str().unwrap_or("");
                if name == "left-pad" {
                    json!({"vulns": [{"id": "GHSA-left", "modified": "2024-01-01T00:00:00Z"}]})
                } else {
                    json!({})
                }
            })
            .collect();
        (
            axum::http::StatusCode::OK,
            axum::Json(json!({"results": results})),
        )
    }
    async fn detail(
        State(seen): State<Shared>,
        axum::extract::Path(id): axum::extract::Path<String>,
    ) -> axum::Json<Value> {
        seen.lock().unwrap().details.push(id.clone());
        axum::Json(json!({
            "id": id,
            "summary": "Prototype pollution",
            "database_specific": {"severity": "HIGH"},
            "affected": [{"package": {"name": "left-pad", "ecosystem": "npm"},
                          "ranges": [{"type": "SEMVER", "events": [{"introduced": "0"}, {"fixed": "1.3.1"}]}]}]
        }))
    }
    let seen: Shared = Arc::default();
    let app = Router::new()
        .route("/v1/querybatch", post(batch))
        .route("/v1/vulns/{id}", get(detail))
        .with_state(seen.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let client = crate::osv::OsvClient::new(&format!("http://{addr}"), svc.pool.clone()).unwrap();
    let deps = vec![
        AddedDependency {
            ecosystem: "npm",
            name: "left-pad".into(),
            version: "1.3.0".into(),
            path: "package.json".into(),
            line: 4,
        },
        AddedDependency {
            ecosystem: "PyPI",
            name: "requests".into(),
            version: "2.32.0".into(),
            path: "requirements.txt".into(),
            line: 1,
        },
    ];
    let answers = client.check(&deps).await;
    assert_eq!(answers.len(), 2);
    let left = &answers[&("npm".to_owned(), "left-pad".to_owned(), "1.3.0".to_owned())];
    assert_eq!(left.len(), 1);
    assert_eq!(
        (
            left[0].id.as_str(),
            left[0].severity.as_str(),
            left[0].fixed.as_str()
        ),
        ("GHSA-left", "HIGH", "1.3.1")
    );
    assert!(
        answers[&(
            "PyPI".to_owned(),
            "requests".to_owned(),
            "2.32.0".to_owned()
        )]
            .is_empty()
    );
    {
        let seen = seen.lock().unwrap();
        assert_eq!(seen.batches.len(), 1);
        assert_eq!(seen.batches[0]["queries"].as_array().unwrap().len(), 2);
        assert_eq!(seen.batches[0]["queries"][0]["package"]["ecosystem"], "npm");
        assert_eq!(seen.details, vec!["GHSA-left"]);
    }
    let findings = crate::domain::review::rules::osv::findings(&deps, &answers);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule, "dependencies.vulnerable");
    assert_eq!(findings[0].line, 4);

    // Cached: the second check asks nothing, even with the server broken.
    seen.lock().unwrap().broken = true;
    let again = client.check(&deps).await;
    assert_eq!(again, answers);
    assert_eq!(seen.lock().unwrap().batches.len(), 1);

    // A new package with a broken endpoint: no answer, no failure.
    let unknown = vec![AddedDependency {
        ecosystem: "crates.io",
        name: "serde".into(),
        version: "1.0.0".into(),
        path: "Cargo.toml".into(),
        line: 9,
    }];
    assert!(client.check(&unknown).await.is_empty());

    // Stale cache rows are refreshed on the next lookup.
    sqlx::query("UPDATE osv_cache SET fetched_at = now() - interval '25 hours'")
        .execute(&svc.pool)
        .await
        .unwrap();
    seen.lock().unwrap().broken = false;
    client.check(&deps).await;
    assert_eq!(seen.lock().unwrap().batches.len(), 2);
}

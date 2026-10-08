//! End-to-end flow through the service with fake peers and a real Postgres
//! (the dev database, isolated in its own schema). Ignored by default: run
//! `cargo test -p sdi-intelligence -- --ignored` with the database up.

use std::sync::Arc;

use sdi_proto::intelligence::v1::{
    AcceptProposalRequest, CreateDecisionRequest, DecisionScope, DecisionSource, DocumentKind,
    GenerateDocumentRequest, GenerateProposalRequest, GetIntelligenceStatsRequest,
    ListDecisionsRequest, ListMessagesRequest, MessageRole, NegotiateRequest, ProposalKind,
    ProposalStatus, SupersedeDecisionRequest, intelligence_service_server::IntelligenceService,
};
use tonic::{Code, Request};

use super::IntelligenceGrpc;
use crate::{
    domain::ranking::fixtures,
    gateway::fake::{FakeExecution, FakeKnowledge},
    reasoner::HeuristicReasoner,
};

/// One schema per test so they can run in parallel without truncating each other.
async fn service(schema: &str) -> (IntelligenceGrpc, Arc<FakeKnowledge>, Arc<FakeExecution>) {
    let url = sdi_common::config::database_url();
    let pool = sdi_common::db::connect(&url, schema)
        .await
        .expect("dev database");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("migrations");
    for table in ["messages", "proposals", "decisions", "documents"] {
        sqlx::query(sqlx::AssertSqlSafe(format!("TRUNCATE {table} CASCADE")))
            .execute(&pool)
            .await
            .unwrap();
    }
    let knowledge = Arc::new(FakeKnowledge {
        context: Some(fixtures::context()),
        patterns: fixtures::catalog(),
        ..Default::default()
    });
    let execution = Arc::new(FakeExecution::default());
    let svc = IntelligenceGrpc::new(
        pool,
        knowledge.clone(),
        execution.clone(),
        Arc::new(HeuristicReasoner),
    );
    (svc, knowledge, execution)
}

#[tokio::test]
#[ignore = "needs the dev Postgres"]
async fn propose_negotiate_accept_document_flow() {
    let (svc, knowledge, execution) = service("intelligence_test_flow").await;

    let proposal = svc
        .generate_proposal(Request::new(GenerateProposalRequest {
            project_id: "proj-1".into(),
            kind: ProposalKind::Deployment as i32,
            prompt: "deploy this".into(),
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
        .generate_proposal(Request::new(GenerateProposalRequest {
            project_id: "nope".into(),
            kind: ProposalKind::Deployment as i32,
            prompt: String::new(),
        }))
        .await
        .unwrap_err();
    assert_eq!(missing.code(), Code::NotFound);

    // A standing decision re-ranks: aws wins once azure contradicts the policy.
    let turn = svc
        .negotiate(Request::new(NegotiateRequest {
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
        .negotiate(Request::new(NegotiateRequest {
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
        .list_messages(Request::new(ListMessagesRequest {
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
        .accept_proposal(Request::new(AcceptProposalRequest {
            id: proposal.id.clone(),
            option_id: String::new(),
            create_execution: true,
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
        .accept_proposal(Request::new(AcceptProposalRequest {
            id: proposal.id.clone(),
            option_id: String::new(),
            create_execution: false,
        }))
        .await
        .unwrap_err();
    assert_eq!(again.code(), Code::FailedPrecondition);

    let decisions = svc
        .list_decisions(Request::new(ListDecisionsRequest {
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
        .create_decision(Request::new(CreateDecisionRequest {
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
        .supersede_decision(Request::new(SupersedeDecisionRequest {
            id: manual.id.clone(),
            superseded_by: String::new(),
        }))
        .await
        .unwrap()
        .into_inner();
    assert!(superseded.superseded_at.is_some());
    let twice = svc
        .supersede_decision(Request::new(SupersedeDecisionRequest {
            id: manual.id,
            superseded_by: String::new(),
        }))
        .await
        .unwrap_err();
    assert_eq!(twice.code(), Code::FailedPrecondition);

    let guide = svc
        .generate_document(Request::new(GenerateDocumentRequest {
            project_id: "proj-1".into(),
            kind: DocumentKind::DeploymentGuide as i32,
        }))
        .await
        .unwrap()
        .into_inner();
    assert!(guide.markdown.contains("## Policy gates"));
    assert!(guide.markdown.contains("deploy.staging"));
    let adr = svc
        .generate_document(Request::new(GenerateDocumentRequest {
            project_id: "proj-1".into(),
            kind: DocumentKind::Adr as i32,
        }))
        .await
        .unwrap()
        .into_inner();
    assert!(adr.markdown.contains("Standardise on aws"));
    assert!(adr.markdown.contains("## ADR-002"));

    let stats = svc
        .get_intelligence_stats(Request::new(GetIntelligenceStatsRequest {}))
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
        .generate_proposal(Request::new(GenerateProposalRequest {
            project_id: "proj-1".into(),
            kind: ProposalKind::Unspecified as i32,
            prompt: String::new(),
        }))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::InvalidArgument);
    let err = svc
        .negotiate(Request::new(NegotiateRequest {
            proposal_id: "not-a-uuid".into(),
            content: "cheaper".into(),
        }))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::InvalidArgument);
    let err = svc
        .list_messages(Request::new(ListMessagesRequest {
            proposal_id: uuid::Uuid::nil().to_string(),
        }))
        .await;
    assert!(err.is_ok_and(|r| r.into_inner().messages.is_empty()));
}

use chrono::Utc;
use sdi_common::grpc::db_status;
use sdi_proto::{
    intelligence::v1::{
        DecisionScope, DecisionSource, ListMessagesRequest, ListMessagesResponse, NegotiateRequest,
        NegotiateResponse,
    },
    knowledge::v1::{IndexKnowledgeRequest, KnowledgeKind},
};
use sqlx::types::Json;
use tonic::Status;
use uuid::Uuid;

use super::{Caller, IntelligenceGrpc, parse_id, proposals, require};
use crate::{
    convert,
    db::{
        decisions::DecisionRow,
        messages::{self as messages_repo, MessageRow},
        proposals as proposals_repo,
    },
    domain::{
        narrative,
        negotiation::{self, Effect},
    },
};

pub(super) async fn negotiate(
    svc: &IntelligenceGrpc,
    caller: &Caller,
    request: NegotiateRequest,
) -> Result<NegotiateResponse, Status> {
    let content = require("content", &request.content)?;
    let mut row = proposals::load(svc, caller, &request.proposal_id).await?;
    if !row.is_open() {
        return Err(Status::failed_precondition(format!(
            "proposal is {}",
            row.status
        )));
    }
    let kind = convert::kind_from_str(&row.kind);

    let user_message = MessageRow::new(
        caller.workspace,
        row.id,
        "user",
        content.clone(),
        false,
        Vec::new(),
    );
    messages_repo::insert(&svc.pool, &user_message)
        .await
        .map_err(db_status)?;

    let effects = negotiation::parse(&content);
    let weights_before = row.weights.0.clone();
    let before = row.options.0.clone();

    let mut weights = weights_before.clone();
    let mut constraints = row.constraints.0.clone();
    negotiation::apply(
        &effects,
        &mut weights,
        &mut constraints,
        &user_message.id.to_string(),
    );

    let mut recorded: Option<DecisionRow> = None;
    if let Some(Effect::Decision(draft)) = effects.iter().find(|e| matches!(e, Effect::Decision(_)))
    {
        let decision = DecisionRow {
            id: Uuid::now_v7(),
            workspace_id: caller.workspace,
            project_id: String::new(),
            scope: convert::scope_to_str(DecisionScope::Organization),
            status: "active".to_owned(),
            source: convert::source_to_str(DecisionSource::Negotiation),
            title: draft.title.clone(),
            statement: draft.statement.clone(),
            reasons: draft.reasons.clone(),
            approved_by: caller.principal.user_id.clone(),
            proposal_id: row.id.to_string(),
            tags: draft.tags.clone(),
            created_at: Utc::now(),
            superseded_at: None,
            superseded_by: String::new(),
        };
        proposals::record_decision(svc, caller, &decision).await?;
        recorded = Some(decision);
    }

    let settings = svc.settings(caller).await?;
    let access = svc.reasoner_access(caller).await;
    let reasoner = svc.reasoner_for(&settings, &access);
    // Every revision re-ranks and regenerates artifacts, so a changed
    // region, port or runtime is reflected in the generated files.
    let (_, after) = proposals::rank_for(
        svc,
        caller,
        &row.project_id,
        kind,
        &constraints,
        &weights,
        settings.cost_ceiling_usd,
    )
    .await?;
    let draft_decision = recorded.as_ref().map(|d| negotiation::DecisionDraft {
        title: d.title.clone(),
        statement: d.statement.clone(),
        reasons: d.reasons.clone(),
        tags: d.tags.clone(),
    });
    let context = narrative::ReplyContext {
        user_message: &content,
        effects: effects
            .iter()
            .map(|e| narrative::describe_effect(e, &weights_before, &weights))
            .collect(),
        recorded_decision: draft_decision.as_ref(),
        weights_before: &weights_before,
        weights_after: &weights,
        before: &before,
        after: &after,
    };
    let draft = narrative::compose_reply(&effects, &context);
    let context_json = serde_json::to_value(&context).unwrap_or(serde_json::Value::Null);
    let text = reasoner.write_reply(&draft.text, &context_json).await;

    let reply = MessageRow::new(
        caller.workspace,
        row.id,
        "assistant",
        text,
        draft.changed_recommendation,
        draft.references.clone(),
    );
    messages_repo::insert(&svc.pool, &reply)
        .await
        .map_err(db_status)?;

    let project_name = row.title.clone();
    let (_, summary) =
        narrative::describe_proposal(&row.kind.replace('_', "/"), &project_name, &after);
    row.status = "negotiating".to_owned();
    row.summary = summary;
    row.recommended_option_id = after.first().map(|o| o.id.clone()).unwrap_or_default();
    row.options = Json(after);
    row.constraints = Json(constraints);
    row.weights = Json(weights);
    row.revision += 1;
    row.reasoner = reasoner.name().to_owned();
    proposals_repo::update_ranking(&svc.pool, &row)
        .await
        .map_err(db_status)?;
    proposals::emit_proposal(svc, &row, "updated");

    index_turn(
        svc,
        caller,
        &row.project_id,
        &row.id,
        &row.title,
        &user_message,
        &reply,
    )
    .await;

    let row = proposals::load(svc, caller, &request.proposal_id).await?;
    Ok(NegotiateResponse {
        proposal: Some(convert::proposal(&row)),
        user_message: Some(convert::message(&user_message)),
        reply: Some(convert::message(&reply)),
        recorded_decision: recorded.as_ref().map(convert::decision),
    })
}

pub(super) async fn list_messages(
    svc: &IntelligenceGrpc,
    caller: &Caller,
    request: ListMessagesRequest,
) -> Result<ListMessagesResponse, Status> {
    let id = parse_id("proposal_id", &request.proposal_id)?;
    let rows = messages_repo::list(&svc.pool, caller.workspace, id)
        .await
        .map_err(db_status)?;
    Ok(ListMessagesResponse {
        messages: rows.iter().map(convert::message).collect(),
    })
}

/// One conversation turn (question + answer) becomes retrievable memory.
async fn index_turn(
    svc: &IntelligenceGrpc,
    caller: &Caller,
    project_id: &str,
    proposal_id: &Uuid,
    title: &str,
    user: &MessageRow,
    reply: &MessageRow,
) {
    if let Err(error) = svc
        .knowledge
        .index_knowledge(
            &caller.principal,
            IndexKnowledgeRequest {
                kind: KnowledgeKind::Conversation as i32,
                project_id: project_id.to_owned(),
                ref_id: proposal_id.to_string(),
                title: format!("{title} - negotiation"),
                content: format!("User: {}\nAssistant: {}", user.content, reply.content),
                path: format!("proposals/{proposal_id}/messages/{}", user.id),
            },
        )
        .await
    {
        tracing::warn!(%error, proposal = %proposal_id, "could not index conversation turn");
    }
}

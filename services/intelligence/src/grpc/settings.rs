//! Ranking defaults and reasoner choice, one row per workspace. Weights are
//! renormalised so a settings page that sends 0.2/0.2/0.1/0.15/0.15/0.1/0.1
//! and one that sends 20/20/10/15/15/10/10 mean the same thing.
//! `api_key_configured` reports whether the workspace can actually reach
//! Claude: its data controls allow it and a key (its own, else the dev
//! fallback) exists.

use std::{sync::Arc, time::Instant};

use sdi_common::grpc::db_status;
use sdi_proto::intelligence::v1::{IntelligenceSettings, ReasonerTest, UpdateSettingsRequest};
use sqlx::types::Json;
use tonic::Status;

use super::{Caller, IntelligenceGrpc, reasoner_allowed};
use crate::{
    convert,
    db::{
        settings::{self as repo, DEFAULT_COST_CEILING_USD},
        usage::PgUsageLedger,
    },
    domain::model::Signal,
    reasoner::{CapReached, LlmReasoner, ReasonerRequest, without_body},
};

/// A provider or model that only repeats what the workspace would inherit
/// anyway (from the environment, or from its stored key) is not stored as
/// the workspace's own choice. `GetSettings` returns the effective values,
/// and a client that sends them back unchanged must not thereby make the
/// workspace deaf to a later change of `LLM_PROVIDER` or `LLM_MODEL`.
async fn release_inherited(svc: &IntelligenceGrpc, caller: &Caller, row: &mut repo::SettingsRow) {
    let inherited_provider =
        crate::reasoner::Provider::parse(&svc.reasoner.env.provider, &svc.reasoner.env.base_url);
    if row.reasoner_base_url.trim().is_empty()
        && row.reasoner_provider.trim() == inherited_provider.label()
    {
        row.reasoner_provider.clear();
    }
    if row.model.trim().is_empty() {
        return;
    }
    let mut without_choice = row.clone();
    without_choice.model.clear();
    let access = svc.reasoner_access(caller, &without_choice).await;
    if svc.effective_model(&without_choice, &access) == row.model.trim() {
        row.model.clear();
    }
}

/// What the connection test asks. The reply's content does not matter:
/// that one came back at all is the test.
const TEST_SYSTEM: &str = "You answer connection tests.";
const TEST_INSTRUCTION: &str = "Reply with the single word: ready";
/// Enough for one word from any model, small enough to cost nothing.
const TEST_MAX_TOKENS: u32 = 16;

/// One tiny call to the workspace's model, or the reason none can be made
/// (CONTRACT.md §19). Never an error for a misconfiguration: the answer
/// names the setting and where it lives, which is the point of asking.
pub(super) async fn test_reasoner(
    svc: &IntelligenceGrpc,
    caller: &Caller,
) -> Result<ReasonerTest, Status> {
    let row = svc.settings(caller).await?;
    let provider = svc.provider_for(&row);
    let answer = |status: &str, message: String, model: String| ReasonerTest {
        status: status.to_owned(),
        message,
        provider: provider.label().to_owned(),
        model,
        endpoint: provider.endpoint(),
        ..ReasonerTest::default()
    };

    let controls = match svc.identity.data_controls(&caller.principal).await {
        Ok(controls) => controls,
        Err(error) => {
            tracing::warn!(%error, "data controls unavailable for the reasoner test");
            return Ok(answer(
                "identity_unavailable",
                "The identity service did not answer, so the workspace's data controls could not be read. Nothing was sent to the model. Try again in a moment.".to_owned(),
                String::new(),
            ));
        }
    };
    if !reasoner_allowed(&controls) {
        return Ok(answer(
            "off",
            "The reasoner is off for this workspace, so nothing is sent to a model. Turn it on under Workspace, Data and privacy.".to_owned(),
            String::new(),
        ));
    }
    if !row.wants_llm() {
        return Ok(answer(
            "heuristic",
            "The settings choose the built-in reasoner, so the model is not called. Choose the model reasoner on this page to use it.".to_owned(),
            String::new(),
        ));
    }
    let access = svc.access_from_controls(caller, &row, &controls).await;
    let model = svc.effective_model(&row, &access);
    let Some(key) = access.key.as_ref() else {
        return Ok(answer(
            "no_key",
            "No model key is configured. Store one under Workspace, Data and privacy, or set LLM_API_KEY where the intelligence service runs.".to_owned(),
            model,
        ));
    };
    if model.trim().is_empty() {
        return Ok(answer(
            "no_model",
            "No model is named, and this provider has no default. Name one on this page, or set LLM_MODEL where the intelligence service runs.".to_owned(),
            model,
        ));
    }

    let reasoner = LlmReasoner::new(
        access.provider.clone(),
        key.api_key.clone(),
        model.clone(),
        caller.workspace,
        row.daily_token_cap(),
        Arc::new(PgUsageLedger {
            pool: svc.pool.clone(),
        }),
    );
    let started = Instant::now();
    let outcome = reasoner
        .complete(&ReasonerRequest {
            system: TEST_SYSTEM,
            context_json: serde_json::Value::Null,
            instruction: TEST_INSTRUCTION.to_owned(),
            max_tokens: TEST_MAX_TOKENS,
            json: false,
        })
        .await;
    let latency_ms = u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX);
    Ok(match outcome {
        Ok(completion) => ReasonerTest {
            latency_ms,
            input_tokens: u32::try_from(completion.usage.input_tokens).unwrap_or(u32::MAX),
            output_tokens: u32::try_from(completion.usage.output_tokens).unwrap_or(u32::MAX),
            ..answer(
                "ok",
                format!("The model answered in {latency_ms} ms."),
                model,
            )
        },
        Err(error) => match error.downcast_ref::<CapReached>() {
            Some(cap) => answer(
                "cap_reached",
                format!(
                    "Today's token cap is spent ({} of {} used), so no call was made. Raise the cap on this page or wait until tomorrow (UTC).",
                    cap.used, cap.cap
                ),
                model,
            ),
            // Provider and HTTP status only: a response body can echo the
            // request, and is never shown or logged.
            None => ReasonerTest {
                latency_ms,
                ..answer(
                    "provider_error",
                    format!(
                        "The provider did not accept the call: {}. A 401 or 403 means the key, a 404 usually the model name or the endpoint.",
                        without_body(&error.to_string())
                    ),
                    model,
                )
            },
        },
    })
}

pub(super) async fn get(
    svc: &IntelligenceGrpc,
    caller: &Caller,
) -> Result<IntelligenceSettings, Status> {
    let row = svc.settings(caller).await?;
    let access = svc.reasoner_access(caller, &row).await;
    let watch = crate::db::autopilot::watch_state(&svc.pool, caller.workspace)
        .await
        .map_err(db_status)?;
    Ok(convert::settings(
        &row,
        access.configured(),
        svc.effective_model(&row, &access),
        access.provider.label(),
        &convert::WatcherView {
            interval_seconds: svc.pr_watch_seconds,
            last_run_at: watch.last_run_at,
            last_error: watch.last_error,
        },
    ))
}

pub(super) async fn update(
    svc: &IntelligenceGrpc,
    caller: &Caller,
    request: UpdateSettingsRequest,
) -> Result<IntelligenceSettings, Status> {
    let Some(incoming) = request.settings else {
        return Err(Status::invalid_argument("settings is required"));
    };
    let mut row = svc.settings(caller).await?;

    if let Some(weights) = incoming.default_weights {
        let mut weights = convert::weights_from_proto(&weights);
        if Signal::ALL
            .iter()
            .any(|s| weights.get(*s) < 0.0 || !weights.get(*s).is_finite())
        {
            return Err(Status::invalid_argument(
                "weights must be finite and non-negative",
            ));
        }
        let total: f64 = Signal::ALL.iter().map(|s| weights.get(*s)).sum();
        if total <= f64::EPSILON {
            return Err(Status::invalid_argument("weights must not all be zero"));
        }
        weights.normalise();
        row.default_weights = Json(weights);
    }

    let reasoner = incoming.reasoner.trim().to_lowercase();
    if !reasoner.is_empty() {
        if !matches!(reasoner.as_str(), "heuristic" | "claude" | "llm") {
            return Err(Status::invalid_argument(
                "reasoner must be \"heuristic\" or \"llm\"",
            ));
        }
        row.reasoner = reasoner;
    }
    // `reasoner_model` is the explicit spelling of `model`; either sets it.
    let model = if incoming.reasoner_model.trim().is_empty() {
        incoming.model.trim()
    } else {
        incoming.reasoner_model.trim()
    };
    if !model.is_empty() {
        row.model = model.to_owned();
    }
    let provider = incoming.reasoner_provider.trim().to_lowercase();
    if !provider.is_empty() {
        if !matches!(
            provider.as_str(),
            "anthropic" | "openai_compatible" | "openai-compatible" | "openai"
        ) {
            return Err(Status::invalid_argument(
                "reasoner_provider must be \"anthropic\" or \"openai_compatible\"",
            ));
        }
        row.reasoner_provider = if provider == "anthropic" {
            provider
        } else {
            "openai_compatible".to_owned()
        };
    }
    let base_url = incoming.reasoner_base_url.trim();
    if !base_url.is_empty() {
        if !(base_url.starts_with("http://") || base_url.starts_with("https://")) {
            return Err(Status::invalid_argument(
                "reasoner_base_url must start with http:// or https://",
            ));
        }
        row.reasoner_base_url = base_url.trim_end_matches('/').to_owned();
    }
    if incoming.reasoner_daily_token_cap != 0 {
        row.reasoner_daily_token_cap =
            i64::try_from(incoming.reasoner_daily_token_cap).unwrap_or(i64::MAX);
    } else if row.reasoner_daily_token_cap <= 0 {
        row.reasoner_daily_token_cap = repo::DEFAULT_DAILY_TOKEN_CAP;
    }
    if incoming.cost_ceiling_usd != 0.0 {
        if incoming.cost_ceiling_usd <= 0.0 || !incoming.cost_ceiling_usd.is_finite() {
            return Err(Status::invalid_argument(
                "cost_ceiling_usd must be positive",
            ));
        }
        row.cost_ceiling_usd = incoming.cost_ceiling_usd;
    } else if row.cost_ceiling_usd <= 0.0 {
        row.cost_ceiling_usd = DEFAULT_COST_CEILING_USD;
    }
    // `auto_merge` is a word, so it can say "unset": empty keeps what is
    // stored. Anything but the two known words is refused.
    let auto_merge = incoming.auto_merge.trim().to_lowercase();
    if !auto_merge.is_empty() {
        if !matches!(
            auto_merge.as_str(),
            repo::AUTO_MERGE_OFF | repo::AUTO_MERGE_DEPENDABOT
        ) {
            return Err(Status::invalid_argument(
                "auto_merge must be \"off\" or \"dependabot\"",
            ));
        }
        row.auto_merge = auto_merge;
    }
    // A bool cannot say "unset"; the gateway sends the whole object back.
    // The read-only fields (`pr_watch_*`) are never taken from a request.
    row.auto_review = incoming.auto_review;
    row.auto_draft_fixes = incoming.auto_draft_fixes;
    row.merge_requires_review = incoming.merge_requires_review;

    release_inherited(svc, caller, &mut row).await;

    repo::upsert(&svc.pool, caller.workspace, &row)
        .await
        .map_err(db_status)?;
    get(svc, caller).await
}

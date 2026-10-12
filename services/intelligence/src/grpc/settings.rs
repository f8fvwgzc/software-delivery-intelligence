//! Ranking defaults and reasoner choice, one row per workspace. Weights are
//! renormalised so a settings page that sends 0.2/0.2/0.1/0.15/0.15/0.1/0.1
//! and one that sends 20/20/10/15/15/10/10 mean the same thing.
//! `api_key_configured` reports whether the workspace can actually reach
//! Claude: its data controls allow it and a key (its own, else the dev
//! fallback) exists.

use sdi_common::grpc::db_status;
use sdi_proto::intelligence::v1::{IntelligenceSettings, UpdateSettingsRequest};
use sqlx::types::Json;
use tonic::Status;

use super::{Caller, IntelligenceGrpc};
use crate::{
    convert,
    db::settings::{self as repo, DEFAULT_COST_CEILING_USD},
    domain::model::Signal,
};

pub(super) async fn get(
    svc: &IntelligenceGrpc,
    caller: &Caller,
) -> Result<IntelligenceSettings, Status> {
    let row = svc.settings(caller).await?;
    let access = svc.reasoner_access(caller, &row).await;
    Ok(convert::settings(
        &row,
        access.configured(),
        svc.effective_model(&row, &access),
        access.provider.label(),
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
    // A bool cannot say "unset"; the gateway sends the whole object back.
    row.auto_review = incoming.auto_review;

    repo::upsert(&svc.pool, caller.workspace, &row)
        .await
        .map_err(db_status)?;
    get(svc, caller).await
}

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
    let access = svc.reasoner_access(caller).await;
    Ok(convert::settings(
        &row,
        access.configured(),
        svc.effective_model(&row, &access),
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
        if !matches!(reasoner.as_str(), "heuristic" | "claude") {
            return Err(Status::invalid_argument(
                "reasoner must be \"heuristic\" or \"claude\"",
            ));
        }
        row.reasoner = reasoner;
    }
    let model = incoming.model.trim();
    if !model.is_empty() {
        row.model = model.to_owned();
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

    repo::upsert(&svc.pool, caller.workspace, &row)
        .await
        .map_err(db_status)?;
    get(svc, caller).await
}

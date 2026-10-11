//! Platform settings. The two services each own their half; the gateway
//! reads both in parallel and patches whichever half the body names, so the
//! settings page never has to know which service a knob belongs to.

use axum::{Json, extract::State};
use sdi_proto::{intelligence::v1 as intelligence, knowledge::v1 as knowledge};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::{
    dto::{
        intelligence::IntelligenceSettings,
        knowledge::KnowledgeSettings,
        settings::{Settings, UpdateSettingsRequest},
    },
    error::{ApiJson, ApiResult, ErrorResponses},
    state::AppState,
};

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(get_settings, update_settings))
}

async fn knowledge_settings(state: &AppState) -> ApiResult<KnowledgeSettings> {
    let settings = state
        .knowledge()
        .get_settings(knowledge::GetSettingsRequest {})
        .await?
        .into_inner();
    Ok(settings.into())
}

async fn intelligence_settings(state: &AppState) -> ApiResult<IntelligenceSettings> {
    let settings = state
        .intelligence()
        .get_settings(intelligence::GetSettingsRequest {})
        .await?
        .into_inner();
    Ok(settings.into())
}

/// Scanner limits and ranking defaults, composed from both services.
#[utoipa::path(
    get,
    path = "/settings",
    tag = "system",
    responses((status = 200, body = Settings), ErrorResponses),
)]
async fn get_settings(State(state): State<AppState>) -> ApiResult<Json<Settings>> {
    let (knowledge, intelligence) =
        tokio::try_join!(knowledge_settings(&state), intelligence_settings(&state))?;
    Ok(Json(Settings {
        knowledge,
        intelligence,
    }))
}

/// Changes either part (or both). Omitted fields keep their current value.
/// A part that is not in the body is read back unchanged.
#[utoipa::path(
    patch,
    path = "/settings",
    tag = "system",
    request_body = UpdateSettingsRequest,
    responses((status = 200, body = Settings), ErrorResponses),
)]
async fn update_settings(
    State(state): State<AppState>,
    ApiJson(body): ApiJson<UpdateSettingsRequest>,
) -> ApiResult<Json<Settings>> {
    // Read-modify-write per part: the services take the whole settings
    // object, so the merge over the current values happens here.
    let knowledge = async {
        let current = knowledge_settings(&state).await?;
        let Some(patch) = body.knowledge else {
            return Ok(current);
        };
        let updated = state
            .knowledge()
            .update_settings(knowledge::UpdateSettingsRequest {
                settings: Some(patch.apply(current).into()),
            })
            .await?
            .into_inner();
        Ok::<_, crate::error::ApiError>(updated.into())
    };
    let intelligence = async {
        let current = intelligence_settings(&state).await?;
        let Some(patch) = body.intelligence else {
            return Ok(current);
        };
        let updated = state
            .intelligence()
            .update_settings(intelligence::UpdateSettingsRequest {
                settings: Some(patch.apply(current).into()),
            })
            .await?
            .into_inner();
        Ok::<_, crate::error::ApiError>(updated.into())
    };
    let (knowledge, intelligence) = tokio::try_join!(knowledge, intelligence)?;
    Ok(Json(Settings {
        knowledge,
        intelligence,
    }))
}

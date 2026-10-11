//! Platform settings, composed by the gateway from the knowledge and
//! intelligence services so the settings page has one object to read and
//! one to patch (CONTRACT.md §8).

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::{
    intelligence::{IntelligenceSettings, IntelligenceSettingsPatch},
    knowledge::{KnowledgeSettings, KnowledgeSettingsPatch},
};

/// Response of `GET /settings` and `PATCH /settings`.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// Scanner limits (knowledge service).
    pub knowledge: KnowledgeSettings,
    /// Ranking defaults and the reasoner (intelligence service).
    pub intelligence: IntelligenceSettings,
}

/// Body of `PATCH /settings`. Either part may be omitted; within a part,
/// omitted fields keep their current value.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSettingsRequest {
    /// Changes to the scanner limits.
    pub knowledge: Option<KnowledgeSettingsPatch>,
    /// Changes to the ranking defaults and the reasoner.
    pub intelligence: Option<IntelligenceSettingsPatch>,
}

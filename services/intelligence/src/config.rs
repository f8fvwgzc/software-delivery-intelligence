//! Typed settings. Everything comes from the environment (CONTRACT.md §3).

use std::net::SocketAddr;

use anyhow::{Context, Result};
use sdi_common::config;

pub const DEFAULT_MODEL: &str = "claude-sonnet-5-5";

/// The reasoner's environment fallbacks (CONTRACT.md §3, §19): provider,
/// endpoint, key and model, used only for a workspace whose data controls
/// allow the reasoner and that configured nothing of its own. The
/// `ANTHROPIC_*` names are kept as aliases of `LLM_API_KEY` / `LLM_MODEL`.
#[derive(Clone, Default)]
pub struct ReasonerEnv {
    /// `LLM_PROVIDER`: "anthropic" (default) | "openai_compatible".
    pub provider: String,
    /// `LLM_BASE_URL`: OpenAI-compatible endpoint (Ollama, vLLM, Azure, ...).
    pub base_url: String,
    /// `LLM_API_KEY`, else `ANTHROPIC_API_KEY`.
    pub api_key: Option<String>,
    /// `LLM_MODEL`: the model for whichever provider is configured. Empty
    /// when unset.
    pub model: String,
    /// `ANTHROPIC_MODEL`: the older name. It names an Anthropic model, so
    /// it applies to the Anthropic provider only. Empty when unset.
    pub anthropic_model: String,
}

impl ReasonerEnv {
    pub fn from_env() -> Self {
        Self {
            provider: config::env_or("LLM_PROVIDER", ""),
            base_url: config::env_or("LLM_BASE_URL", ""),
            api_key: config::env_opt("LLM_API_KEY")
                .or_else(|| config::env_opt("ANTHROPIC_API_KEY")),
            model: config::env_opt("LLM_MODEL").unwrap_or_default(),
            anthropic_model: config::env_opt("ANTHROPIC_MODEL").unwrap_or_default(),
        }
    }

    /// The model the environment names for `provider`. Only Anthropic has a
    /// built-in default: a Claude model name sent to another provider's
    /// endpoint is a failed call, so there the answer is empty and the
    /// caller reports that a model must be named.
    pub fn model_for(&self, provider: &crate::reasoner::Provider) -> String {
        let named = self.model.trim();
        if !named.is_empty() {
            return named.to_owned();
        }
        match provider {
            crate::reasoner::Provider::Anthropic => {
                let older = self.anthropic_model.trim();
                if older.is_empty() {
                    DEFAULT_MODEL.to_owned()
                } else {
                    older.to_owned()
                }
            }
            crate::reasoner::Provider::OpenAiCompatible { .. } => String::new(),
        }
    }
}

/// Seconds between two looks of the pull request watcher when
/// `SDI_PR_WATCH_SECONDS` is unset (CONTRACT.md §23.1).
pub const DEFAULT_PR_WATCH_SECONDS: u32 = 60;
/// The shortest interval the watcher accepts; anything from 1 to 14 is raised to it.
pub const MIN_PR_WATCH_SECONDS: u32 = 15;

/// `SDI_PR_WATCH_SECONDS` as the watcher reads it: unset or empty is the
/// default, `0` turns the watcher off, 1–14 are raised to the minimum, and
/// anything that is not a whole number of seconds is a configuration error
/// (a typo must not silently become "every minute" or "never").
pub fn pr_watch_seconds(raw: Option<&str>) -> Result<u32> {
    let Some(raw) = raw.map(str::trim).filter(|r| !r.is_empty()) else {
        return Ok(DEFAULT_PR_WATCH_SECONDS);
    };
    let seconds: u32 = raw.parse().with_context(|| {
        format!("SDI_PR_WATCH_SECONDS must be a whole number of seconds (0 turns the watcher off): {raw}")
    })?;
    Ok(match seconds {
        0 => 0,
        s if s < MIN_PR_WATCH_SECONDS => MIN_PR_WATCH_SECONDS,
        s => s,
    })
}

#[derive(Clone)]
pub struct Settings {
    pub listen: SocketAddr,
    pub database_url: String,
    pub knowledge_url: String,
    pub execution_url: String,
    pub identity_url: String,
    /// Optional dev fallback for the LLM reasoner: used only for a
    /// workspace whose data controls allow the reasoner and that stored no
    /// key of its own (CONTRACT.md §19).
    pub reasoner: ReasonerEnv,
    /// Seconds between two looks at the open pull requests; 0 = off.
    pub pr_watch_seconds: u32,
}

impl Settings {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            listen: config::listen_addr("INTELLIGENCE_LISTEN", "0.0.0.0:50052")?,
            database_url: config::database_url(),
            knowledge_url: config::knowledge_url(),
            execution_url: config::execution_url(),
            identity_url: config::identity_url(),
            reasoner: ReasonerEnv::from_env(),
            pr_watch_seconds: pr_watch_seconds(config::env_opt("SDI_PR_WATCH_SECONDS").as_deref())?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reasoner::Provider;

    /// Only Anthropic has a built-in default model. A Claude model name
    /// sent to another provider's endpoint is a failed call, so there the
    /// answer is "none named" and nothing is called.
    #[test]
    fn the_environment_names_a_model_per_provider() {
        let openai = Provider::parse("openai", "");
        let nothing = ReasonerEnv::default();
        assert_eq!(nothing.model_for(&Provider::Anthropic), DEFAULT_MODEL);
        assert_eq!(nothing.model_for(&openai), "");

        // `ANTHROPIC_MODEL` names an Anthropic model and applies there only.
        let older = ReasonerEnv {
            anthropic_model: "claude-older".into(),
            ..Default::default()
        };
        assert_eq!(older.model_for(&Provider::Anthropic), "claude-older");
        assert_eq!(older.model_for(&openai), "");

        // `LLM_MODEL` is for whichever provider is configured and wins.
        let named = ReasonerEnv {
            model: " my-model ".into(),
            anthropic_model: "claude-older".into(),
            ..Default::default()
        };
        assert_eq!(named.model_for(&Provider::Anthropic), "my-model");
        assert_eq!(named.model_for(&openai), "my-model");
    }

    #[test]
    fn pr_watch_seconds_has_a_default_a_floor_and_an_off_switch() {
        assert_eq!(pr_watch_seconds(None).unwrap(), 60);
        assert_eq!(pr_watch_seconds(Some("")).unwrap(), 60);
        assert_eq!(pr_watch_seconds(Some("  ")).unwrap(), 60);
        assert_eq!(pr_watch_seconds(Some("0")).unwrap(), 0, "0 turns it off");
        for low in 1..=14 {
            assert_eq!(pr_watch_seconds(Some(&low.to_string())).unwrap(), 15);
        }
        assert_eq!(pr_watch_seconds(Some("15")).unwrap(), 15);
        assert_eq!(pr_watch_seconds(Some(" 300 ")).unwrap(), 300);
        for bad in ["-1", "1.5", "soon", "60s"] {
            let error = pr_watch_seconds(Some(bad)).unwrap_err();
            assert!(
                error.to_string().contains("SDI_PR_WATCH_SECONDS"),
                "{error}"
            );
        }
    }
}

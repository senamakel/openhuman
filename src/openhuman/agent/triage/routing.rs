//! Local-vs-remote provider resolver for triage turns.
//!
//! ## Commit 1 scope
//!
//! This file ships as a **remote-only stub**: it loads the persisted
//! [`Config`], builds a fresh routed backend provider the same way
//! `agent_chat_simple` does (see
//! [`crate::openhuman::local_ai::ops::agent_chat_simple`]), and hands
//! it back with `used_local = false`. Every triage call pays a config
//! load + provider construction — acceptable because the composio
//! subscriber is feature-flagged off by default and the work is
//! bounded by the event bus rate.
//!
//! ## Commit 2 (planned)
//!
//! - Check `LocalAiService::global().is_running().await` and current
//!   [`crate::openhuman::local_ai::ModelTier`] from config. Local is
//!   used when `tier >= Ram4To8Gb` per the plan (`linear-bouncing-lovelace.md`).
//! - Run a ~500 ms liveness probe on the effective chat model.
//! - Cache the decision for 60 s in a
//!   `tokio::sync::Mutex<Option<CachedDecision>>` with `Local` /
//!   `Remote` / `Degraded` states; `Degraded` is entered when a local
//!   turn fails mid-flight so subsequent triggers in the window skip
//!   local entirely.
//!
//! The public [`resolve_provider`] signature is already the commit-2
//! shape so `evaluator.rs` doesn't need to change.

use std::sync::Arc;

use anyhow::Context;

use crate::openhuman::config::Config;
use crate::openhuman::providers::{self, Provider, ProviderRuntimeOptions, INFERENCE_BACKEND_ID};

/// The concrete provider + metadata that [`evaluator::run_triage`]
/// should use for this particular triage turn.
///
/// Owning fields only — the caller may store this briefly while the
/// turn is in flight and across the `tokio::spawn` that the composio
/// subscriber uses. `provider` is an `Arc` so it can be cloned into
/// the [`crate::openhuman::agent::bus::AgentTurnRequest`] without a
/// deep copy.
pub struct ResolvedProvider {
    /// Ready-to-use provider, already constructed.
    pub provider: Arc<dyn Provider>,
    /// Provider name token — `"openhuman"` for the remote backend and
    /// `"local-ollama"` (commit 2) for the local path. Passed through
    /// unchanged into `AgentTurnRequest::provider_name`.
    pub provider_name: String,
    /// Model identifier — the concrete string `run_tool_call_loop`
    /// will hand to the provider.
    pub model: String,
    /// `true` if this turn is running on the local LLM. Published in
    /// `DomainEvent::TriggerEvaluated.used_local` for observability.
    pub used_local: bool,
}

/// Resolve a provider for a single triage turn. Commit 1 always
/// returns the default remote backend; commit 2 adds the local probe.
pub async fn resolve_provider() -> anyhow::Result<ResolvedProvider> {
    let config = Config::load_or_init()
        .await
        .context("loading config for triage provider resolution")?;
    build_remote_provider(&config)
}

/// Internal helper so the local fallback path in commit 2 can call
/// back into the remote builder with the same `config` it probed
/// with, instead of loading the config twice.
fn build_remote_provider(config: &Config) -> anyhow::Result<ResolvedProvider> {
    let default_model = config
        .default_model
        .clone()
        .unwrap_or_else(|| crate::openhuman::config::DEFAULT_MODEL.to_string());
    let options = ProviderRuntimeOptions {
        auth_profile_override: None,
        openhuman_dir: config.config_path.parent().map(std::path::PathBuf::from),
        secrets_encrypt: config.secrets.encrypt,
        reasoning_enabled: config.runtime.reasoning_enabled,
    };
    let provider_box = providers::create_routed_provider_with_options(
        config.api_key.as_deref(),
        config.api_url.as_deref(),
        &config.reliability,
        &config.model_routes,
        default_model.as_str(),
        &options,
    )
    .context("building routed remote provider for triage")?;
    // `Box<dyn Provider>` → `Arc<dyn Provider>` is a single reallocation
    // — the `Provider` trait is `Send + Sync` so this is type-safe.
    let provider: Arc<dyn Provider> = Arc::from(provider_box);
    tracing::debug!(
        provider = %INFERENCE_BACKEND_ID,
        model = %default_model,
        "[triage::routing] resolved remote provider (commit 1 stub)"
    );
    Ok(ResolvedProvider {
        provider,
        provider_name: INFERENCE_BACKEND_ID.to_string(),
        model: default_model,
        used_local: false,
    })
}

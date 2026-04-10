//! Event-bus handler that triggers the owner-discovery agent.
//!
//! Listens on [`DomainEvent::SkillOAuthCompleted`] and, when the
//! feature is enabled, spawns a background [`run_discovery`] call on
//! a `tokio` task.
//!
//! > **Status: Apify stubbed.** `config.discovery.enabled` defaults to
//! > `false` while the real Apify integration is on hold. With the
//! > default config the handler logs and no-ops, so Phase 1 (skill
//! > `memory.updateOwner`) is still the only active owner-identity
//! > path. Flip `OPENHUMAN_DISCOVERY_ENABLED=1` and the runner wakes
//! > up — with a [`StubApifyClient`](super::apify::StubApifyClient)
//! > that returns empty datasets, so the LLM can still persist
//! > anything it infers from seed facts alone.
//!
//! # Debounce
//!
//! When active, discovery stores its last-run unix timestamp in the
//! memory KV store under key `owner.discovery.last_run_at` (global
//! namespace). On each event the subscriber reads that value and skips
//! if the window is still open, so rapid skill reconnects don't
//! trigger a storm of duplicate runs.

use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::Mutex;

use super::apify::{ApifyClient, StubApifyClient};
use super::llm::BackendLlmClient;
use super::runner::{run_discovery, DiscoveryDeps};
use super::types::{DiscoveryJob, DiscoveryTrigger, SeedFact};
use crate::openhuman::event_bus::{DomainEvent, EventHandler, SubscriptionHandle};

/// Global KV key used for debounce tracking.
pub(crate) const DISCOVERY_LAST_RUN_KEY: &str = "owner.discovery.last_run_at";

static DISCOVERY_HANDLE: OnceLock<SubscriptionHandle> = OnceLock::new();

/// Process-local mutex serialising concurrent `trigger_discovery` calls.
///
/// The KV-based debounce check is a read-then-write against the memory
/// store; on its own it lets two rapid `SkillOAuthCompleted` events race
/// past the window and both launch runners. Wrapping the whole check +
/// run in this mutex means the second task re-reads `last_run_at` after
/// the first has stamped it, so it observes the fresh timestamp and
/// bounces. The mutex is intentionally process-local — this subscriber
/// only runs inside a single core process.
static TRIGGER_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn trigger_lock() -> &'static Mutex<()> {
    TRIGGER_LOCK.get_or_init(|| Mutex::new(()))
}

/// Register the discovery subscriber on the global event bus.
///
/// Idempotent — safe to call multiple times; only the first
/// registration wins.
pub fn register_discovery_subscriber() {
    if DISCOVERY_HANDLE.get().is_some() {
        return;
    }

    match crate::openhuman::event_bus::subscribe_global(Arc::new(DiscoverySubscriber)) {
        Some(handle) => {
            let _ = DISCOVERY_HANDLE.set(handle);
            log::info!("[discovery:bus] subscriber registered");
        }
        None => {
            log::warn!("[discovery:bus] failed to register subscriber — event bus not initialized");
        }
    }
}

pub struct DiscoverySubscriber;

#[async_trait]
impl EventHandler for DiscoverySubscriber {
    fn name(&self) -> &str {
        "agent::discovery"
    }

    fn domains(&self) -> Option<&[&str]> {
        Some(&["skill"])
    }

    async fn handle(&self, event: &DomainEvent) {
        let DomainEvent::SkillOAuthCompleted {
            skill_id,
            integration_id,
            state_snapshot,
        } = event
        else {
            return;
        };

        log::debug!(
            "[discovery:bus] SkillOAuthCompleted skill={} integration={}",
            skill_id,
            integration_id
        );

        // Snapshot the state for the background task; the borrowed
        // event reference dies when this function returns.
        let skill_id = skill_id.clone();
        let integration_id = integration_id.clone();
        let state_snapshot = state_snapshot.clone();

        tokio::spawn(async move {
            if let Err(e) = trigger_discovery(skill_id, integration_id, state_snapshot).await {
                log::warn!("[discovery:bus] discovery run failed: {e}");
            }
        });
    }
}

/// Build a [`DiscoveryJob`] and invoke [`run_discovery`] with the
/// global memory client + a freshly-constructed set of HTTP clients.
///
/// Respects `config.discovery.enabled` and the debounce window. The
/// debounce check-then-run is serialised under [`trigger_lock`] so
/// concurrent `SkillOAuthCompleted` events can't race past a stale
/// `last_run_at` and fire duplicate runs.
async fn trigger_discovery(
    skill_id: String,
    integration_id: String,
    state_snapshot: Value,
) -> Result<(), String> {
    // Load the current Config snapshot — gives us the enabled flag,
    // api_url, and session-token handle we need downstream.
    let config = match crate::openhuman::config::Config::load_or_init().await {
        Ok(c) => c,
        Err(e) => return Err(format!("Config::load_or_init: {e}")),
    };

    if !config.discovery.enabled {
        log::debug!("[discovery:bus] skipped — discovery disabled in config");
        return Ok(());
    }

    let memory = crate::openhuman::memory::global::client_if_ready()
        .ok_or_else(|| "memory global client not ready".to_string())?;

    // Serialise the debounce check + run. A second task queued behind
    // this mutex will observe the updated `last_run_at` from the first
    // task's successful completion and bounce on the KV re-read.
    let _claim = trigger_lock().lock().await;

    // Debounce: check KV for last-run timestamp (now under the lock).
    let now = unix_secs();
    if let Ok(Some(v)) = memory.kv_get(None, DISCOVERY_LAST_RUN_KEY).await {
        if let Some(last) = v.as_u64() {
            let elapsed = now.saturating_sub(last);
            if elapsed < config.discovery.debounce_secs {
                log::info!(
                    "[discovery:bus] debounced — last run {}s ago, window {}s",
                    elapsed,
                    config.discovery.debounce_secs
                );
                return Ok(());
            }
        }
    }

    // Stake the debounce marker *before* doing the expensive run so a
    // racing task that acquires the lock after us still observes a fresh
    // timestamp and bounces. We refresh it again on success below so a
    // partial run that crashes mid-flight doesn't permanently block
    // future attempts beyond the next debounce window.
    let _ = memory
        .kv_set(None, DISCOVERY_LAST_RUN_KEY, &json!(now))
        .await;

    let llm = BackendLlmClient::from_config(&config)
        .map_err(|e| format!("BackendLlmClient::from_config: {e}"))?;
    // Apify integration is stubbed for now — see apify::StubApifyClient.
    // When real wiring returns, swap this for the backend-proxy client.
    let apify: Arc<dyn ApifyClient> = Arc::new(StubApifyClient::new());

    // Pick a sensible default model — we reuse the core's default
    // model if set, otherwise fall back to the generic "default" hint
    // that the backend understands.
    let model = config
        .default_model
        .clone()
        .unwrap_or_else(|| "default".to_string());

    let seeds = extract_seeds(&state_snapshot);
    let job = DiscoveryJob {
        trigger: DiscoveryTrigger::SkillOAuthCompleted {
            skill_id: skill_id.clone(),
            integration_id,
        },
        seed_facts: seeds,
    };

    let deps = DiscoveryDeps {
        memory: memory.clone(),
        llm: Arc::new(llm),
        apify,
        config: config.discovery.clone(),
        model,
    };

    match run_discovery(job, deps).await {
        Ok(report) => {
            log::info!(
                "[discovery:bus] run complete skill={} facts={} docs={} rounds={} errors={}",
                skill_id,
                report.facts_written,
                report.docs_written,
                report.rounds_used,
                report.errors.len()
            );
            // Update the debounce marker on success so a partial run
            // still blocks the next spam click.
            let _ = memory
                .kv_set(None, DISCOVERY_LAST_RUN_KEY, &json!(now))
                .await;
            Ok(())
        }
        Err(e) => Err(format!("run_discovery: {e}")),
    }
}

/// Walk the skill's published-state JSON and pull out fields that are
/// likely to help the LLM find the owner. This is best-effort — we
/// prefer flat, string-valued keys at the top level of the object.
pub(crate) fn extract_seeds(state: &Value) -> Vec<SeedFact> {
    let mut out: Vec<SeedFact> = Vec::new();
    let obj = match state.as_object() {
        Some(o) => o,
        None => return out,
    };

    // Well-known keys worth surfacing directly. Order is priority —
    // the LLM sees them in the order we push.
    const PRIORITY_KEYS: &[(&str, &str)] = &[
        ("email", "email"),
        ("user_email", "email"),
        ("email_address", "email"),
        ("full_name", "full_name"),
        ("name", "name"),
        ("display_name", "display_name"),
        ("username", "username"),
        ("workspace_name", "workspace"),
        ("workspace", "workspace"),
        ("team_name", "team"),
        ("company", "company"),
        ("organization", "company"),
        ("location", "location"),
        ("timezone", "timezone"),
        ("title", "title"),
        ("role", "role"),
    ];

    for (source_key, display_label) in PRIORITY_KEYS {
        if let Some(val) = obj.get(*source_key).and_then(Value::as_str) {
            let trimmed = val.trim();
            if !trimmed.is_empty() && trimmed.len() <= 200 {
                out.push(SeedFact::new(*display_label, trimmed));
            }
        }
    }

    out
}

fn unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_seeds_pulls_known_keys() {
        let state = json!({
            "email": "ada@example.com",
            "full_name": "Ada Lovelace",
            "workspace_name": "Analytical Engines",
            "irrelevant": 42,
        });
        let seeds = extract_seeds(&state);
        let labels: Vec<&str> = seeds.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"email"));
        assert!(labels.contains(&"full_name"));
        assert!(labels.contains(&"workspace"));
    }

    #[test]
    fn extract_seeds_skips_empty_and_huge_values() {
        let state = json!({
            "email": "   ",
            "name": "x".repeat(500),
            "company": "Analytical Engines"
        });
        let seeds = extract_seeds(&state);
        assert_eq!(seeds.len(), 1);
        assert_eq!(seeds[0].label, "company");
    }

    #[test]
    fn extract_seeds_handles_non_object_state() {
        assert!(extract_seeds(&json!(null)).is_empty());
        assert!(extract_seeds(&json!([1, 2, 3])).is_empty());
        assert!(extract_seeds(&json!("scalar")).is_empty());
    }
}

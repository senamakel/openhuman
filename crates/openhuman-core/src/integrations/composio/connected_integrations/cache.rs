//! The process-wide connected-integrations cache: [`CachedIntegrations`],
//! [`INTEGRATIONS_CACHE`], its readers/invalidation
//! ([`cached_active_integrations`], [`cached_active_integrations_including_expired`],
//! [`invalidate_connected_integrations_cache`]), and the reconciliation
//! helpers ([`connected_set_hash`], [`sync_cache_with_connections`]) that
//! keep it in sync with a fresh backend `list_connections` response.

use crate::agent::context::prompt::ConnectedIntegration;
use crate::config::Config;

use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, RwLock};
use std::time::{Duration, Instant};

// ── Prompt integration discovery ────────────────────────────────────

/// Defensive TTL on the integrations cache.
///
/// Background: the primary invalidation path is the
/// `ComposioConnectionCreated` → `wait_for_connection_active` bus flow
/// (see [`crate::integrations::composio::bus::ComposioConnectionCreatedSubscriber`]), which
/// polls the backend for up to 60 s after `composio_authorize` returns
/// a `connectUrl`. On Windows the OAuth round-trip can exceed that
/// window (Defender SmartScreen, slower browser launch, extra consent
/// dialogs), so the invalidation call never fires and the chat
/// runtime's cache stays frozen on the pre-connect snapshot even
/// though the Settings UI polls `composio_list_connections` every 5 s
/// and shows the user as "Connected".
///
/// The cross-platform defenses we layer on top:
///   1. [`composio_list_connections`] diff-invalidates the cache whenever
///      the backend's active-toolkit set diverges from what's cached,
///      so a running UI keeps the chat cache in sync within one poll
///      interval.
///   2. This TTL caps worst-case staleness at 60 s regardless of
///      whether the UI is open, the bus fires, or the user reconnected
///      out-of-band.
pub(crate) const CACHE_TTL: Duration = Duration::from_secs(60);

/// Cached entry: the integrations list plus the timestamp we wrote it.
#[derive(Clone)]
pub(crate) struct CachedIntegrations {
    pub(crate) entries: Vec<ConnectedIntegration>,
    pub(crate) cached_at: Instant,
}

/// Process-wide cache for connected integrations, keyed by the config
/// identity (the `config_path` string) so different user contexts don't
/// collide. Each entry is populated on first fetch and returned on
/// subsequent calls until explicitly invalidated or the TTL expires.
pub(crate) static INTEGRATIONS_CACHE: LazyLock<RwLock<HashMap<String, CachedIntegrations>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Crate-wide test serialization lock for all tests that mutate or read
/// the process-global `INTEGRATIONS_CACHE`. Defined here so it is shared
/// by every `cfg(test)` module in this crate (ops_tests, tools_tests, …).
/// Poison-recovery (`unwrap_or_else`) keeps a panicking test from
/// permanently blocking later ones.
#[cfg(test)]
pub(crate) fn composio_cache_test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Derive a stable cache key from a [`Config`]. We use the stringified
/// `config_path` because it uniquely identifies a user context (it
/// resolves to the per-user openhuman dir).
pub(crate) fn cache_key(config: &Config) -> String {
    config.config_path.display().to_string()
}

/// Clear cached connected integrations so the next call to
/// [`fetch_connected_integrations`] hits the backend again.
///
/// Called by [`crate::integrations::composio::bus::ComposioConnectionCreatedSubscriber`] when a
/// new OAuth connection completes, by [`composio_list_connections`]
/// when it observes a divergence between the backend response and the
/// cached snapshot, and from tests. Clears the entire map because the
/// callers don't carry a config reference.
pub fn invalidate_connected_integrations_cache() {
    if let Ok(mut guard) = INTEGRATIONS_CACHE.write() {
        let entries = guard.len();
        guard.clear();
        tracing::info!(
            cached_keys = entries,
            "[composio][integrations] cache invalidated"
        );
    }
}

/// Read-only snapshot of the currently cached connected integrations for
/// the given config, or [`None`] when the cache is empty, expired, or
/// the lock is held by a writer.
///
/// Designed for hot-path callers that want a cheap "what does the cache
/// already say?" probe without triggering a backend fetch. The agent
/// harness uses this on every turn to detect mid-session connection
/// changes — it relies on the desktop UI's 5 s `composio_list_connections`
/// poll (which calls into [`fetch_connected_integrations`] and
/// repopulates this cache) plus the event-driven invalidation path to
/// keep the cache current.
///
/// `try_read` (not `read`) so a writer in progress — e.g. the UI poll
/// repopulating the cache — never blocks a turn. Worst case the agent
/// sees `None` for one turn while the writer holds the lock; the next
/// turn picks up the value naturally.
///
/// TTL is enforced defensively: entries older than [`CACHE_TTL`] are
/// treated as missing even though they're still in the map (a stale
/// entry would otherwise pin the agent to a frozen view if every
/// invalidation path silently failed).
pub fn cached_active_integrations(config: &Config) -> Option<Vec<ConnectedIntegration>> {
    read_cached_integrations(config, false)
}

/// Like [`cached_active_integrations`] but returns the last cached snapshot
/// even when it has aged past [`CACHE_TTL`].
///
/// Intended ONLY as a transient-failure fallback: when a live fetch reports
/// `Unavailable`/times out, preserving the last-known integrations is strictly
/// better than collapsing the delegation surface to an empty set (which drops
/// `delegate_to_integrations_agent` and silently disables channel tool-calling).
/// Without this, a backend blip that lands just after the 60 s TTL expiry still
/// wipes tool-calling despite having a perfectly good previous snapshot. A fresh
/// fetch repopulates the cache the moment the backend recovers, so the stale
/// window is bounded by the outage, not by this call.
pub fn cached_active_integrations_including_expired(
    config: &Config,
) -> Option<Vec<ConnectedIntegration>> {
    read_cached_integrations(config, true)
}

/// Shared reader for the integrations cache. `allow_expired` bypasses the
/// [`CACHE_TTL`] freshness check for the transient-failure fallback path.
fn read_cached_integrations(
    config: &Config,
    allow_expired: bool,
) -> Option<Vec<ConnectedIntegration>> {
    let key = cache_key(config);
    let guard = match INTEGRATIONS_CACHE.try_read() {
        Ok(g) => g,
        Err(_) => {
            tracing::trace!(
                key = %key,
                "[composio][integrations_cache] cached_active_integrations:lock_contended"
            );
            return None;
        }
    };
    let Some(cached) = guard.get(&key) else {
        tracing::trace!(
            key = %key,
            "[composio][integrations_cache] cached_active_integrations:miss"
        );
        return None;
    };
    let age = cached.cached_at.elapsed();
    if !allow_expired && age > CACHE_TTL {
        tracing::trace!(
            key = %key,
            age_ms = age.as_millis() as u64,
            ttl_ms = CACHE_TTL.as_millis() as u64,
            "[composio][integrations_cache] cached_active_integrations:expired"
        );
        return None;
    }
    // Surface a *very* stale fallback so an unusually long backend outage is
    // observable rather than silently pinning the agent to an ancient snapshot.
    if allow_expired && age > 5 * CACHE_TTL {
        tracing::warn!(
            key = %key,
            age_ms = age.as_millis() as u64,
            ttl_ms = CACHE_TTL.as_millis() as u64,
            "[composio][integrations_cache] serving a heavily-stale integrations snapshot on transient-failure fallback (backend outage?)"
        );
    }
    tracing::trace!(
        key = %key,
        entries = cached.entries.len(),
        age_ms = age.as_millis() as u64,
        allow_expired,
        "[composio][integrations_cache] cached_active_integrations:hit"
    );
    Some(cached.entries.clone())
}

/// Stable hash of the *routing-relevant* slice of a connected-integrations
/// snapshot.
///
/// Two snapshots produce the same hash iff they would synthesise the
/// same `delegate_<toolkit>` tool set in the orchestrator's
/// function-calling schema. The hash is:
///
///   - **Order-independent** — callers don't need to sort the input.
///   - **Description-insensitive** — Composio catalogue text edits don't
///     trigger a refresh. The schema's tool-description field still
///     picks up new text on the next *real* (membership-changing)
///     refresh, so descriptions are never permanently stale.
///   - **Process-local** — [`std::collections::hash_map::DefaultHasher`]
///     is randomly seeded per process. Fine because we only compare
///     hashes within one process lifetime.
///
/// Only `connected == true` entries contribute. Unconnected toolkits are
/// stripped by [`crate::tools::orchestrator_tools::collect_orchestrator_tools`]
/// anyway, so churn among the unconnected set never changes the agent's
/// surface and shouldn't trigger a refresh.
pub fn connected_set_hash(integrations: &[ConnectedIntegration]) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut pairs: Vec<(&str, Vec<&str>)> = integrations
        .iter()
        .filter(|i| i.connected)
        .map(|i| {
            let mut ids: Vec<&str> = i
                .connections
                .iter()
                .map(|c| c.connection_id.as_str())
                .collect();
            ids.sort();
            (i.toolkit.as_str(), ids)
        })
        .collect();
    pairs.sort_by(|a, b| a.0.cmp(b.0));

    let mut hasher = DefaultHasher::new();
    pairs.hash(&mut hasher);
    hasher.finish()
}

/// Collect the set of toolkit slugs marked `connected` in a snapshot.
///
/// Exposed to [`sync_cache_with_connections`] so it can diff the live
/// backend connection list against what the chat runtime currently
/// believes is connected.
fn connected_toolkit_set(integrations: &[ConnectedIntegration]) -> HashSet<String> {
    integrations
        .iter()
        .filter(|i| i.connected)
        .map(|i| i.toolkit.clone())
        .collect()
}

/// Reconcile the process-wide integrations cache with a fresh backend
/// `list_connections` response.
///
/// Called from [`composio_list_connections`], which the desktop UI
/// polls every 5 s (see `app/src/lib/composio/hooks.ts`). When the set
/// of ACTIVE/CONNECTED toolkits in the response differs from what's in
/// the cache, we invalidate so the chat runtime re-fetches on its next
/// `fetch_connected_integrations` call. This keeps tool availability
/// in chat in sync with the badge the user sees in Settings, even when
/// the primary event-bus invalidation path misses (e.g. Windows OAuth
/// flows that overrun the 60 s readiness poll).
pub(crate) fn sync_cache_with_connections(
    connections: &[crate::integrations::composio::types::ComposioConnection],
) {
    let live_active: HashSet<String> = connections
        .iter()
        .filter(|c| c.is_active())
        .map(|c| c.normalized_toolkit())
        .filter(|toolkit| !toolkit.is_empty())
        .collect();

    // Collect active connection IDs per toolkit to detect multi-account changes
    let live_ids: std::collections::HashMap<String, Vec<String>> = {
        let mut ids: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        for c in connections.iter().filter(|c| c.is_active()) {
            let tk = c.normalized_toolkit();
            if !tk.is_empty() {
                ids.entry(tk).or_default().push(c.id.clone());
            }
        }
        for v in ids.values_mut() {
            v.sort();
        }
        ids
    };

    // Read once to decide whether any cache entry is out of sync. We
    // clone out the keys + connected sets so we can release the read
    // lock before taking the write lock.
    let divergent_keys: Vec<(String, HashSet<String>, HashSet<String>)> = {
        let Ok(guard) = INTEGRATIONS_CACHE.read() else {
            return;
        };
        guard
            .iter()
            .filter_map(|(key, cached)| {
                let cached_set = connected_toolkit_set(&cached.entries);
                // Also check per-toolkit connection IDs (not just counts)
                let ids_match = cached.entries.iter().all(|i| {
                    let mut cached_ids: Vec<&str> = i
                        .connections
                        .iter()
                        .map(|c| c.connection_id.as_str())
                        .collect();
                    cached_ids.sort();
                    let empty = Vec::new();
                    let live = live_ids.get(&i.toolkit).unwrap_or(&empty);
                    cached_ids.len() == live.len()
                        && cached_ids
                            .iter()
                            .zip(live.iter())
                            .all(|(a, b)| *a == b.as_str())
                });
                if cached_set != live_active || !ids_match {
                    Some((key.clone(), cached_set, live_active.clone()))
                } else {
                    None
                }
            })
            .collect()
    };

    if divergent_keys.is_empty() {
        tracing::debug!(
            live_connected = live_active.len(),
            "[composio][integrations] list_connections matches cache — no invalidation needed"
        );
        return;
    }

    if let Ok(mut guard) = INTEGRATIONS_CACHE.write() {
        for (key, cached_set, live_set) in divergent_keys {
            // Diff logging — makes Windows-timing regressions easy to
            // catch in user-supplied debug dumps without leaking any
            // PII (toolkit slugs are public strings like "gmail").
            let added: Vec<&String> = live_set.difference(&cached_set).collect();
            let removed: Vec<&String> = cached_set.difference(&live_set).collect();
            tracing::info!(
                key = %key,
                ?added,
                ?removed,
                "[composio][integrations] cache diverges from backend — invalidating"
            );
            guard.remove(&key);
        }
    }
}

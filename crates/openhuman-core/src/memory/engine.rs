//! Binding the configured memory engine.
//!
//! [`resolve`] turns the `[memory]` config into either a live engine
//! ([`Binding::On`]) or the reason memory is off ([`Binding::Off`]):
//!
//! | Engine | Endpoint | Credential | Off when |
//! | --- | --- | --- | --- |
//! | `tinyhumans` | `[memory.engines.tinyhumans] endpoint`, else [`crate::backend::base_url`] | the host's backend credential, resolved per request through [`resolve_backend_credential`] | signed out, or no backend transport |
//! | `cortexdb` | `[memory.engines.cortexdb] endpoint`, else CortexDB's managed API | the API key stored as [`MEMORY_CORTEXDB_KEY_NAME`] | no key stored |
//!
//! Built engines are cached per config fingerprint (engine id, endpoint,
//! credential identity), so a config change, a sign-in or a new key rebuilds
//! the engine on the next call without any event plumbing, and repeated calls
//! reuse one HTTP client.
//!
//! An embedding host can bring its own engine instead ([`install_host_engine`]):
//! it then wins over the configured one for every config in the process — one
//! OpenHuman runtime per process — so a host with its own store, or a test
//! binary with an in-memory engine, runs the whole lifecycle without a
//! TinyHumans credential or a CortexDB key. It is wrapped in the same
//! scrubber as a configured engine.
//!
//! The core never holds a TinyHumans credential of its own: the bearer comes
//! from the host's credential seam on every request, so a refreshed session is
//! used at once and signing out turns memory off.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, RwLock};

use async_trait::async_trait;
use sha2::{Digest, Sha256};
use tinymemory_api::MemoryEngine;
use tinymemory_integrations::{BearerSource, EngineCredential, EngineSettings};

use crate::config::schema::MEMORY_CORTEXDB_KEY_NAME;
use crate::config::Config;
use crate::security::credentials::session_support::resolve_backend_credential;
use crate::security::credentials::{AuthService, DEFAULT_AUTH_PROFILE_NAME};

use super::error::{MemoryError, MemoryResult};

/// Engine id of CortexDB behind the TinyHumans backend.
pub const TINYHUMANS_ENGINE: &str = tinymemory_integrations::cortex::TINYHUMANS_ENGINE_ID;

/// Engine id of CortexDB reached directly.
pub const CORTEXDB_ENGINE: &str = tinymemory_integrations::cortex::CORTEXDB_ENGINE_ID;

/// A bound engine. Its writes are scrubbed ([`super::guard`]).
#[derive(Clone)]
pub struct BoundEngine {
    /// The engine.
    pub engine: Arc<dyn MemoryEngine>,
    /// Its id.
    pub id: String,
    /// The endpoint it talks to.
    pub endpoint: String,
}

impl std::fmt::Debug for BoundEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BoundEngine")
            .field("id", &self.id)
            .field("endpoint", &self.endpoint)
            .finish_non_exhaustive()
    }
}

/// Whether memory is on, and with what.
#[derive(Debug, Clone)]
pub enum Binding {
    /// An engine is bound.
    On(BoundEngine),
    /// Memory is off.
    Off {
        /// The configured engine id, when it names a known engine.
        engine: Option<String>,
        /// The configured endpoint, when known.
        endpoint: Option<String>,
        /// Why memory is off (no credential detail).
        reason: String,
    },
}

impl Binding {
    /// The bound engine, or [`MemoryError::Off`].
    pub fn engine(self) -> MemoryResult<BoundEngine> {
        match self {
            Self::On(bound) => Ok(bound),
            Self::Off { reason, .. } => Err(MemoryError::Off(reason)),
        }
    }

    /// Whether an engine is bound.
    #[must_use]
    pub fn is_on(&self) -> bool {
        matches!(self, Self::On(_))
    }
}

/// Built engines, keyed by fingerprint.
static CACHE: LazyLock<RwLock<HashMap<String, BoundEngine>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Test engines, keyed by workspace directory, so concurrent tests that each
/// own a temp workspace never see one another's engine.
#[cfg(test)]
static TEST_ENGINES: LazyLock<RwLock<HashMap<std::path::PathBuf, Arc<dyn MemoryEngine>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Binds `engine` for every config whose workspace is `workspace` (tests only).
#[cfg(test)]
pub(crate) fn install_test_engine(workspace: &std::path::Path, engine: Arc<dyn MemoryEngine>) {
    TEST_ENGINES
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(workspace.to_path_buf(), engine);
}

/// The engine an embedding host installed, if any ([`install_host_engine`]).
static HOST_ENGINE: LazyLock<RwLock<Option<BoundEngine>>> = LazyLock::new(|| RwLock::new(None));

/// Endpoint reported for a host-installed engine.
pub const HOST_ENGINE_ENDPOINT: &str = "host://engine";

/// Binds `engine` for every config in this process, ahead of the configured
/// one. For an embedding host that owns its memory store; the engine's writes
/// are scrubbed like any other's ([`super::guard`]). A later call replaces the
/// earlier engine.
pub fn install_host_engine(engine: Arc<dyn MemoryEngine>) {
    let id = engine.descriptor().id.to_string();
    tracing::info!(engine = %id, "[memory:engine] host engine installed");
    *HOST_ENGINE
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(BoundEngine {
        id,
        endpoint: HOST_ENGINE_ENDPOINT.to_string(),
        engine: super::guard::ScrubbingEngine::wrap(engine),
    });
}

/// Removes a host-installed engine; the configured engine applies again.
/// Returns whether one was installed.
pub fn clear_host_engine() -> bool {
    HOST_ENGINE
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
        .is_some()
}

fn host_engine() -> Option<BoundEngine> {
    HOST_ENGINE
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// Resolves the configured engine.
#[must_use]
pub fn resolve(config: &Config) -> Binding {
    #[cfg(test)]
    {
        let installed = TEST_ENGINES
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&config.workspace_dir)
            .cloned();
        if let Some(engine) = installed {
            return Binding::On(BoundEngine {
                id: engine.descriptor().id.to_string(),
                endpoint: "test://engine".to_string(),
                engine: super::guard::ScrubbingEngine::wrap(engine),
            });
        }
    }
    if let Some(bound) = host_engine() {
        return Binding::On(bound);
    }
    let engine_id = config.memory.engine.trim().to_string();
    match engine_id.as_str() {
        TINYHUMANS_ENGINE => resolve_tinyhumans(config),
        CORTEXDB_ENGINE => resolve_cortexdb(config),
        "" => {
            let reason = if config.memory.legacy_backend_unsupported
                || config.memory.legacy_backend.is_some()
            {
                "legacy memory backend is unsupported; select an explicit v2 memory engine"
                    .to_string()
            } else {
                "no memory engine selected".to_string()
            };
            off(None, None, &reason)
        }
        other => {
            tracing::warn!(engine = %other, "[memory:engine] unknown engine id in config");
            off(
                None,
                None,
                "the configured memory engine is not available in this build",
            )
        }
    }
}

/// Whether memory is on for `config`.
#[must_use]
pub fn is_on(config: &Config) -> bool {
    resolve(config).is_on()
}

fn off(engine: Option<&str>, endpoint: Option<String>, reason: &str) -> Binding {
    Binding::Off {
        engine: engine.map(str::to_string),
        endpoint,
        reason: reason.to_string(),
    }
}

fn resolve_tinyhumans(config: &Config) -> Binding {
    let endpoint = match config.memory.endpoint_for(TINYHUMANS_ENGINE) {
        Some(endpoint) => endpoint,
        None => match crate::backend::base_url(&config.api_url) {
            Ok(url) => url,
            Err(error) => {
                tracing::debug!(error = %error, "[memory:engine] no backend origin for tinyhumans");
                return off(
                    Some(TINYHUMANS_ENGINE),
                    None,
                    "no TinyHumans backend is available",
                );
            }
        },
    };
    if let Err(error) = resolve_backend_credential(config) {
        tracing::debug!(error = %error, "[memory:engine] no usable TinyHumans credential");
        return off(
            Some(TINYHUMANS_ENGINE),
            Some(endpoint),
            "sign in to use TinyHumans memory",
        );
    }
    // The backend expects the host's attribution (`x-sdk-name`, …) on every
    // request, as every other backend caller sends it; the transport owns it.
    let headers = attribution_headers();
    let fingerprint = format!(
        "{TINYHUMANS_ENGINE}|{endpoint}|{}|{headers:?}",
        config.config_path.display()
    );
    let source: Arc<dyn BearerSource> = Arc::new(HostBearer {
        config: Arc::new(config.clone()),
    });
    build_cached(
        TINYHUMANS_ENGINE,
        EngineSettings {
            endpoint: Some(endpoint),
            headers,
            ..EngineSettings::default()
        },
        fingerprint,
        EngineCredential::Dynamic(source),
    )
}

/// The installed transport's attribution headers, as engine settings carry
/// them. A value that is not text is left out (and logged by name only).
fn attribution_headers() -> std::collections::BTreeMap<String, String> {
    crate::backend::attribution_headers()
        .iter()
        .filter_map(|(name, value)| match value.to_str() {
            Ok(value) => Some((name.as_str().to_string(), value.to_string())),
            Err(_) => {
                tracing::debug!(header = %name, "[memory:engine] skipping a non-text attribution header");
                None
            }
        })
        .collect()
}

fn resolve_cortexdb(config: &Config) -> Binding {
    let configured = config.memory.endpoint_for(CORTEXDB_ENGINE);
    let endpoint = configured
        .clone()
        .unwrap_or_else(|| tinymemory_integrations::cortex::CORTEX_API_ENDPOINT.to_string());
    let key = match read_cortexdb_key(config) {
        Ok(Some(key)) => key,
        Ok(None) => {
            return off(
                Some(CORTEXDB_ENGINE),
                Some(endpoint),
                "add a CortexDB API key to use CortexDB memory",
            )
        }
        Err(error) => {
            tracing::warn!(error = %error, "[memory:engine] reading the CortexDB key failed");
            return off(
                Some(CORTEXDB_ENGINE),
                Some(endpoint),
                "the CortexDB API key could not be read",
            );
        }
    };
    let fingerprint = format!("{CORTEXDB_ENGINE}|{endpoint}|{}", key_digest(&key));
    // A third-party endpoint: no TinyHumans attribution headers.
    build_cached(
        CORTEXDB_ENGINE,
        EngineSettings {
            endpoint: Some(endpoint),
            ..EngineSettings::default()
        },
        fingerprint,
        EngineCredential::Static(key),
    )
}

fn build_cached(
    id: &str,
    settings: EngineSettings,
    fingerprint: String,
    credential: EngineCredential,
) -> Binding {
    let endpoint = settings.endpoint.clone().unwrap_or_default();
    if let Some(bound) = CACHE
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&fingerprint)
        .cloned()
    {
        return Binding::On(bound);
    }
    match tinymemory_integrations::build_engine(id, &settings, credential) {
        Ok(engine) => {
            tracing::info!(engine = %id, "[memory:engine] engine bound");
            let bound = BoundEngine {
                engine: super::guard::ScrubbingEngine::wrap(engine),
                id: id.to_string(),
                endpoint: endpoint.to_string(),
            };
            CACHE
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(fingerprint, bound.clone());
            Binding::On(bound)
        }
        Err(error) => {
            tracing::warn!(engine = %id, error = %error, "[memory:engine] engine build refused");
            off(Some(id), Some(endpoint.to_string()), &error.to_string())
        }
    }
}

/// A short, non-reversible identity of a key for the cache fingerprint.
fn key_digest(key: &str) -> String {
    let digest = Sha256::digest(key.as_bytes());
    digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Drops every cached engine, so the next [`resolve`] rebuilds.
pub fn invalidate() {
    CACHE
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
}

/// The stored CortexDB API key, if any.
pub fn read_cortexdb_key(config: &Config) -> anyhow::Result<Option<String>> {
    AuthService::from_config(config).get_provider_bearer_token(MEMORY_CORTEXDB_KEY_NAME, None)
}

/// Stores the CortexDB API key in the credential store (never in config).
pub fn store_cortexdb_key(config: &Config, key: &str) -> MemoryResult<()> {
    let key = key.trim();
    if key.is_empty() {
        return Err(MemoryError::invalid("the API key is blank"));
    }
    AuthService::from_config(config)
        .store_provider_token(
            MEMORY_CORTEXDB_KEY_NAME,
            DEFAULT_AUTH_PROFILE_NAME,
            key,
            HashMap::new(),
            true,
        )
        .map_err(|error| MemoryError::Engine(format!("storing the API key failed: {error}")))?;
    tracing::debug!("[memory:engine] cortexdb key stored");
    Ok(())
}

/// Removes the stored CortexDB API key.
pub fn clear_cortexdb_key(config: &Config) -> MemoryResult<bool> {
    AuthService::from_config(config)
        .remove_profile(MEMORY_CORTEXDB_KEY_NAME, DEFAULT_AUTH_PROFILE_NAME)
        .map_err(|error| MemoryError::Engine(format!("removing the API key failed: {error}")))
}

/// Whether the engine `id` has a credential available.
#[must_use]
pub fn has_key(config: &Config, id: &str) -> bool {
    match id {
        TINYHUMANS_ENGINE => resolve_backend_credential(config).is_ok(),
        CORTEXDB_ENGINE => matches!(read_cortexdb_key(config), Ok(Some(_))),
        _ => false,
    }
}

/// The host's backend credential as a per-request bearer.
struct HostBearer {
    config: Arc<Config>,
}

#[async_trait]
impl BearerSource for HostBearer {
    async fn bearer(&self) -> tinymemory_api::Result<String> {
        match resolve_backend_credential(&self.config) {
            Ok(credential) => Ok(credential.into_secret()),
            Err(error) => {
                tracing::debug!(error = %error, "[memory:engine] backend credential unavailable");
                Err(tinymemory_api::Error::Unauthorized(
                    "the backend credential is unavailable".to_string(),
                ))
            }
        }
    }
}

#[cfg(test)]
#[path = "engine_tests.rs"]
mod tests;

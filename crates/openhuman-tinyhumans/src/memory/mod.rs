//! The hosted `tinyhumans` memory engine and its free-period check.
//!
//! [`register`] hands the core's memory engine registry
//! (`openhuman_embed::__host::memory_engines`) the engine id `tinyhumans`:
//!
//! - [`engine`] binds TinyMemory's CortexDB engine to the TinyHumans backend
//!   (endpoint, attribution headers, per-request bearer), below the layout
//!   root the core passes in the [`EngineRequest`] (`memory::scope::user_root`).
//! - [`billing`] is the "background work is free" predicate: free only while
//!   the backend's memory free period is on, answered by a cached
//!   `GET /memory/free-period`.
//!
//! Generic engine resolution, scope and the `cortexdb` engine stay in the core.
//! With nothing registered the core answers `tinyhumans` as unavailable.

pub(crate) mod billing;
pub(crate) mod engine;

use openhuman_embed::__host::config::Config;
use openhuman_embed::__host::memory_engines::{
    EngineCredential, EngineEntry, EngineHealth, EngineSettings, ReadOnlyHealthProbe,
    TINYHUMANS_ENGINE, register_memory_engine, register_memory_health_probe,
};
use std::sync::Arc;

use crate::install::InstallOptions;

/// Registers this area's hooks with the core. Idempotent.
pub(crate) fn register(_options: &InstallOptions) {
    register_memory_engine(
        TINYHUMANS_ENGINE,
        EngineEntry {
            build: Arc::new(engine::bind),
            free_background: Some(Arc::new(|config, bound| {
                Box::pin(billing::free_period_active(config, bound))
            })),
        },
    );
    register_memory_health_probe(TINYHUMANS_ENGINE, Arc::new(read_only_health_probe));
    log::debug!("[tinyhumans] memory: hosted engine registered as '{TINYHUMANS_ENGINE}'");
}

/// Builds a throwaway engine for a read-only health request. It reads the
/// selected config's credential profile directly and never enters the normal
/// engine cache or publishes session-expiry state.
fn read_only_health_probe(
    config: Config,
) -> openhuman_embed::__host::memory_engines::EngineFuture<EngineHealth> {
    Box::pin(async move {
        let credential = if let Ok(Some(key)) =
            openhuman_embed::__host::security::credentials::api_key::get_api_key_read_only(&config)
        {
            Some(key)
        } else {
            read_only_session_token(&config)
        };
        let Some(credential) = credential else {
            return EngineHealth::Down(
                "No usable credential is available for the selected backend.".to_owned(),
            );
        };
        let endpoint = config
            .memory
            .endpoint_for(TINYHUMANS_ENGINE)
            .or_else(|| config.resolved_endpoints.control_plane_url.clone());
        let Some(endpoint) = endpoint else {
            return EngineHealth::Down("The selected backend endpoint is unavailable.".to_owned());
        };
        let headers = match crate::backend::headers::attribution_headers() {
            Ok(headers) => headers
                .iter()
                .filter_map(|(name, value)| {
                    value
                        .to_str()
                        .ok()
                        .map(|value| (name.as_str().to_owned(), value.to_owned()))
                })
                .collect(),
            Err(_) => {
                return EngineHealth::Down(
                    "Backend attribution headers are unavailable.".to_owned(),
                );
            }
        };
        let settings = EngineSettings {
            endpoint: Some(endpoint),
            headers,
            tenant_root: true,
            ..EngineSettings::default()
        };
        match tinymemory_integrations::build_engine(
            TINYHUMANS_ENGINE,
            &settings,
            EngineCredential::Static(credential),
        ) {
            Ok(engine) => engine.health().await,
            Err(_) => EngineHealth::Down(
                "The selected memory engine configuration is invalid.".to_owned(),
            ),
        }
    })
}

fn read_only_session_token(config: &Config) -> Option<String> {
    use openhuman_embed::__host::security::credentials::session_support::{
        SessionTokenCheck, check_session_backend, classify_session_token,
        load_app_session_profile_read_only,
    };

    let profile = load_app_session_profile_read_only(config).ok().flatten()?;
    check_session_backend(config, Some(&profile)).ok()?;
    match classify_session_token(Some(&profile), chrono::Utc::now()) {
        SessionTokenCheck::Live(token) => Some(token),
        SessionTokenCheck::Absent | SessionTokenCheck::Expired => None,
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;

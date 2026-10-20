//! Bounded inspection calls to an already-ready connector runtime.

use serde::de::DeserializeOwned;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use crate::config::Config;
use crate::modules::types::ModuleState;

const INSPECTION_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, PartialEq, Eq)]
struct ConfiguredBinding {
    context: [u8; 32],
    credential: [u8; 32],
}

/// Safe result boundary for the existing-only connector call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExistingCallError {
    /// The runtime is absent or its accepted config belongs to another scope.
    Unavailable,
    /// A matching ready runtime returned a failed or malformed response.
    Failed,
}

fn route_gate() -> &'static tokio::sync::Mutex<()> {
    static GATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    &GATE
}

fn configured_binding() -> &'static Mutex<Option<ConfiguredBinding>> {
    static BINDING: OnceLock<Mutex<Option<ConfiguredBinding>>> = OnceLock::new();
    BINDING.get_or_init(|| Mutex::new(None))
}

pub(super) fn route_lock() -> &'static tokio::sync::Mutex<()> {
    route_gate()
}

/// Capture the scope only after the module accepted its CONFIGURE request.
/// The ordinary route fingerprint intentionally omits state_dir and toolkit
/// policy; this separate identity prevents those omissions from authorizing a
/// doctor read against a different workspace or configuration.
pub(super) fn record_configured_route(config: &Config, route: &serde_json::Value) {
    let binding = (|| {
        let selected = selected_credential(config)?;
        let accepted_secret = route
            .get("auth_token")
            .or_else(|| route.get("api_key"))?
            .as_str()?;
        if selected.1 != accepted_secret {
            return None;
        }
        binding_for_selected(config, &selected)
    })();
    *configured_binding()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = binding;
}

/// Call a read-only member only when the currently serving module was already
/// configured for this exact selected scope and credential. This function
/// never starts the module runtime, loads the module, reconciles its route, or
/// imports legacy connector data. `None` means inspection is unavailable.
pub(crate) async fn call_existing_readonly<Request, Reply>(
    config: &Config,
    member: &str,
    request: Request,
) -> Result<Option<Reply>, ExistingCallError>
where
    Request: Serialize + Send,
    Reply: DeserializeOwned,
{
    let _route_guard = route_gate().lock().await;
    if !config.modules.enabled
        || !config.components.connectors
        || super::ops::state_of(super::MODULE_ID) != ModuleState::Ready
    {
        return Ok(None);
    }
    let Some(expected) = binding_for_config(config) else {
        return Ok(None);
    };
    if *configured_binding()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        != Some(expected)
    {
        return Ok(None);
    }
    let Some(runtime) = super::super::runtime::existing_runtime() else {
        return Ok(None);
    };
    let record = super::registry::find(super::MODULE_ID).ok_or(ExistingCallError::Unavailable)?;
    let proxy = runtime
        .proxy(record.bus_name, record.object_path)
        .map_err(|_| ExistingCallError::Unavailable)?
        .with_timeout(INSPECTION_TIMEOUT);
    call_proxy_readonly(&proxy, member, request).await.map(Some)
}

async fn call_proxy_readonly<Request, Reply>(
    proxy: &tinybus::Proxy,
    member: &str,
    request: Request,
) -> Result<Reply, ExistingCallError>
where
    Request: Serialize + Send,
    Reply: DeserializeOwned,
{
    match tokio::time::timeout(
        INSPECTION_TIMEOUT,
        proxy.call_confidential::<Reply>(member, (request,)),
    )
    .await
    {
        Ok(Ok(reply)) => Ok(reply),
        Ok(Err(_)) | Err(_) => Err(ExistingCallError::Failed),
    }
}

fn selected_credential(config: &Config) -> Option<(String, String)> {
    if let Some(pinned) = config.composio.host_credential.as_ref() {
        return Some(("host-pinned-direct".to_owned(), pinned.api_key().to_owned()));
    }

    match config.composio.mode.trim() {
        "" | crate::config::schema::COMPOSIO_MODE_BACKEND => {
            let api_key =
                crate::security::credentials::api_key::get_api_key_read_only(config).ok()?;
            if let Some(api_key) = api_key.filter(|key| !key.trim().is_empty()) {
                return Some(("backend-api-key".to_owned(), api_key.trim().to_owned()));
            }
            let profile =
                crate::security::credentials::session_support::load_app_session_profile_read_only(
                    config,
                )
                .ok()?;
            let token = match crate::security::credentials::session_support::classify_session_token(
                profile.as_ref(),
                chrono::Utc::now(),
            ) {
                crate::security::credentials::session_support::SessionTokenCheck::Live(token)
                    if !crate::security::credentials::session_support::is_local_session_token(
                        token.trim(),
                    ) =>
                {
                    token
                }
                _ => return None,
            };
            Some(("backend-session".to_owned(), token))
        }
        crate::config::schema::COMPOSIO_MODE_DIRECT => {
            let profile = crate::security::credentials::AuthService::get_profile_read_only(
                config,
                crate::security::credentials::COMPOSIO_DIRECT_PROVIDER,
                None,
            )
            .ok()?;
            let stored = profile.and_then(|profile| match profile.kind {
                crate::security::credentials::profiles::AuthProfileKind::Token => profile.token,
                crate::security::credentials::profiles::AuthProfileKind::OAuth => {
                    profile.token_set.map(|tokens| tokens.access_token)
                }
            });
            stored
                .or_else(|| config.composio.api_key.clone())
                .map(|key| ("direct-composio-key".to_owned(), key.trim().to_owned()))
                .filter(|(_, key)| !key.is_empty())
        }
        _ => None,
    }
}

fn binding_for_config(config: &Config) -> Option<ConfiguredBinding> {
    let selected = selected_credential(config)?;
    binding_for_selected(config, &selected)
}

fn binding_for_selected(config: &Config, selected: &(String, String)) -> Option<ConfiguredBinding> {
    Some(ConfiguredBinding {
        context: context_fingerprint(config)?,
        credential: credential_fingerprint(&selected.0, &selected.1),
    })
}

#[cfg(test)]
fn binding_matches_config(config: &Config) -> bool {
    let Some(expected) = binding_for_config(config) else {
        return false;
    };
    *configured_binding()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        == Some(expected)
}

fn context_fingerprint(config: &Config) -> Option<[u8; 32]> {
    let config_json = serde_json::to_value(config).ok()?;
    let env = [
        "BACKEND_URL",
        "VITE_BACKEND_URL",
        "OPENHUMAN_APP_ENV",
        "VITE_APP_ENV",
        "OPENHUMAN_COMPOSIO_DIRECT_BASE_V2",
        "OPENHUMAN_COMPOSIO_DIRECT_BASE_V3",
        "TZ",
    ]
    .into_iter()
    .map(|key| (key, std::env::var(key).ok()))
    .collect::<std::collections::BTreeMap<_, _>>();
    let context = serde_json::json!({
        "config": config_json,
        "config_path": config.config_path,
        "workspace_dir": config.workspace_dir,
        "state_dir": config.workspace_dir.join("state"),
        "offered_toolkits": super::offered_toolkits(config),
        "host_credential": config.composio.host_credential.as_ref().map(|credential| {
            serde_json::json!({
                "entity_id": credential.entity(),
                "base_urls": credential.direct_base_urls().map(|urls| [&urls.v2, &urls.v3]),
            })
        }),
        "env": env,
    });
    Some(Sha256::digest(serde_json::to_vec(&context).ok()?).into())
}

fn credential_fingerprint(kind: &str, secret: &str) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(kind.as_bytes());
    hash.update([0]);
    hash.update(secret.as_bytes());
    hash.finalize().into()
}

#[cfg(test)]
#[path = "inspection_tests.rs"]
mod tests;

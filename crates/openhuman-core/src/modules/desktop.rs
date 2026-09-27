//! Host calls to the attested tinydesktop module.

use std::hash::{Hash, Hasher};
use std::sync::OnceLock;

use serde::Serialize;
use tinybus::Proxy;
use tinydesktop_bus::{names, DesktopResponse, PermissionsRequest};

use crate::config::Config;

#[cfg(test)]
#[path = "desktop_tests.rs"]
mod tests;

pub const MODULE_ID: &str = "tinydesktop";

/// This confidential payload is sent only to the module lifecycle callback.
/// Missing or expired host credentials remove the old Jev client on refresh.
pub fn module_config(config: &Config) -> serde_json::Value {
    let credential =
        crate::security::credentials::session_support::resolve_backend_credential(config)
            .ok()
            .map(crate::security::credentials::session_support::BackendCredential::into_secret)
            .filter(|token| {
                !crate::security::credentials::session_support::is_local_session_token(token)
            });
    match credential {
        Some(api_key) => serde_json::json!({
            "jev": {
                "api_key": api_key,
                "provider": "tiny_humans_open_router",
                "sdk_name": crate::backend::product_identity()
            }
        }),
        None => {
            // A headless or BYOK host may have no TinyHumans session. Direct
            // OpenRouter Jev remains usable with its own scoped credential.
            let direct =
                crate::inference::provider::factory::lookup_key_for_slug("openrouter", config)
                    .ok()
                    .filter(|key| !key.trim().is_empty())
                    .or_else(|| {
                        std::env::var("OPENROUTER_API_KEY")
                            .ok()
                            .filter(|key| !key.trim().is_empty())
                    });
            direct.map_or_else(
                || serde_json::json!({}),
                |api_key| {
                    serde_json::json!({
                        "jev": { "api_key": api_key, "provider": "open_router" }
                    })
                },
            )
        }
    }
}

pub fn jev_ready(config: &Config) -> bool {
    module_config(config)["jev"].is_object()
}

fn last_config() -> &'static tokio::sync::Mutex<Option<u64>> {
    static LAST: OnceLock<tokio::sync::Mutex<Option<u64>>> = OnceLock::new();
    LAST.get_or_init(|| tokio::sync::Mutex::new(None))
}

fn fingerprint(value: &serde_json::Value) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.to_string().hash(&mut hasher);
    hasher.finish()
}

async fn proxy(config: &Config) -> Result<Proxy, String> {
    crate::modules::ops::ensure_loaded_within(
        config,
        MODULE_ID,
        Some(std::time::Duration::from_secs(8)),
    )
    .await
    .map_err(crate::modules::ops::LoadError::into_message)?;
    let runtime = crate::modules::host::runtime()
        .await
        .map_err(|error| format!("desktop module bus unavailable: {error}"))?;
    crate::modules::registry::find(MODULE_ID)
        .ok_or_else(|| "desktop module is not in the compiled registry".to_owned())?;

    // Reinitialization is private and retains no plaintext secret in this host.
    // The lock makes credential rotation atomic across concurrent tool calls.
    let configuration = module_config(config);
    let current = fingerprint(&configuration);
    let mut previous = last_config().lock().await;
    if *previous != Some(current) {
        runtime
            .connection()
            .reinitialize_module(MODULE_ID, configuration)
            .await
            .map_err(|error| format!("desktop module credential refresh failed: {error}"))?;
    }
    *previous = Some(current);
    drop(previous);
    runtime
        .proxy(names::INTERFACE, names::OBJECT_PATH)
        .map_err(|error| format!("desktop module proxy unavailable: {error}"))
}

/// Call one contract member, preserving the structured error envelope.
pub async fn call<Request: Serialize + Send>(
    config: &Config,
    member: &str,
    request: Request,
) -> Result<DesktopResponse, String> {
    let proxy = proxy(config).await?;
    call_with_proxy(&proxy, member, request).await
}

async fn call_with_proxy<Request: Serialize + Send>(
    proxy: &Proxy,
    member: &str,
    request: Request,
) -> Result<DesktopResponse, String> {
    // RunGoal enforces its own shorter elapsed-time budget (at most five
    // minutes). Keep the bus deadline beyond that budget so the caller gets a
    // structured stop and partial execution evidence instead of a transport
    // timeout after TinyBus's default 30 seconds.
    let goal_proxy = (member == names::methods::RUN_GOAL).then(|| {
        proxy
            .clone()
            .with_timeout(std::time::Duration::from_secs(330))
    });
    let proxy = goal_proxy.as_ref().unwrap_or(proxy);
    let response = if member == names::methods::RUN_GOAL || member == names::methods::RESOLVE_INTENT
    {
        proxy
            .call_confidential::<DesktopResponse>(member, (request,))
            .await
    } else {
        proxy.call::<DesktopResponse>(member, (request,)).await
    };
    response.map_err(|error| format!("desktop {member} failed: {error}"))
}

pub async fn permissions(config: &Config) -> Result<DesktopResponse, String> {
    call(
        config,
        names::methods::PERMISSIONS,
        PermissionsRequest::default(),
    )
    .await
}

pub fn state(config: &Config) -> (String, Option<String>) {
    crate::modules::ops::list(config)
        .into_iter()
        .find(|item| item.id == MODULE_ID)
        .map_or_else(
            || {
                (
                    "failed".to_owned(),
                    Some("desktop module is not registered".to_owned()),
                )
            },
            |item| (format!("{:?}", item.state).to_lowercase(), item.detail),
        )
}

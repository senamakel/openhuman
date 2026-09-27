//! The backend port: how the core reaches a hosted backend it knows nothing
//! about.
//!
//! The core holds no hosted-backend URL, header policy, product identity or
//! SDK. A host installs a [`BackendTransport`] (`openhuman-tinyhumans` installs
//! the TinyHumans one) and the core asks it for everything host-specific:
//!
//! - [`transport`] — the port itself: [`BackendTransport`], [`BackendRequest`],
//!   [`BackendTransportError`] and the process-wide install slot. The transport
//!   sends requests *and* answers [`base_url`] / [`inference_base_url`]
//!   (defaults, environment overrides, the inference-endpoint guard) and
//!   [`product_identity`].
//! - [`client`] — [`BackendClient`]: authenticated JSON calls over the port,
//!   the typed [`BackendApiError`] results domains recover from, and
//!   [`flatten_authed_error`], the chokepoint that turns them into the
//!   `SESSION_EXPIRED:` / `API_KEY_REJECTED:` / `BACKEND_UNAVAILABLE:`
//!   sentinels `core::observability` classifies.
//! - [`classify`] — backend budget-exhaustion body classification shared by
//!   inference, the agent loop, the scheduler and web chat.
//!
//! A core with no transport installed runs agents, memory, tools and RPC as
//! normal; every backend-touching call answers
//! [`BackendApiError::BackendUnavailable`] / `BACKEND_UNAVAILABLE:`, and the
//! URL helpers here return [`BackendTransportError::Unavailable`].

pub mod classify;
pub mod client;
pub mod transport;

pub use client::{flatten_authed_error, BackendApiError, BackendClient};
pub use transport::{
    install_backend_transport, installed_backend_transport, is_installed,
    resolve_backend_transport, BackendRequest, BackendTransport, BackendTransportError,
    BaseUrlPurpose, TransportProfile,
};

/// The operator's override, trimmed; `None` when unset or blank.
fn configured(api_url: &Option<String>) -> Option<&str> {
    api_url.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

/// The backend origin for control-plane calls (account, integrations,
/// channels, voice, sockets, telemetry), resolved by the installed transport
/// from the configured `api_url` override.
pub fn base_url(api_url: &Option<String>) -> Result<String, BackendTransportError> {
    let transport = resolve_backend_transport()?;
    Ok(transport.base_url(configured(api_url), BaseUrlPurpose::ControlPlane))
}

/// [`base_url`] for callers on the JSON-RPC `String` error channel: a missing
/// transport becomes the `BACKEND_UNAVAILABLE:` sentinel `core::observability`
/// demotes, so a core without a hosted backend never pages Sentry for it.
pub fn require_base_url(api_url: &Option<String>) -> Result<String, String> {
    base_url(api_url).map_err(|error| match error {
        BackendTransportError::Unavailable => format!(
            "{} no backend transport installed",
            crate::core::observability::BACKEND_UNAVAILABLE_PREFIX
        ),
        other => other.to_string(),
    })
}

/// The backend origin for the managed OpenAI-compatible inference proxy
/// (chat, embeddings). Unlike [`base_url`] this honours an `api_url` override
/// that points at an inference endpoint.
///
/// Managed inference talks to that URL directly rather than through the
/// transport, so a library host with no transport installed still reaches an
/// origin it configured explicitly (`openhuman_embed::Runtime::backend_url`).
/// Only the *default* origin is the host's to supply: with neither a
/// transport nor an override this is [`BackendTransportError::Unavailable`].
pub fn inference_base_url(api_url: &Option<String>) -> Result<String, BackendTransportError> {
    match resolve_backend_transport() {
        Ok(transport) => Ok(transport.base_url(configured(api_url), BaseUrlPurpose::Inference)),
        Err(BackendTransportError::Unavailable) => configured(api_url)
            .map(crate::util::url::normalize_api_base_url)
            .ok_or(BackendTransportError::Unavailable),
        Err(other) => Err(other),
    }
}

/// The product identity the installed host attributes backend traffic to
/// (the `x-sdk-name` value), or `None` when no transport is installed.
pub fn product_identity() -> Option<String> {
    resolve_backend_transport()
        .ok()
        .map(|transport| transport.product_identity())
}

/// The installed host's attribution headers (see
/// [`BackendTransport::attribution_headers`]); empty without a transport.
pub fn attribution_headers() -> reqwest::header::HeaderMap {
    resolve_backend_transport()
        .map(|transport| transport.attribution_headers())
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;

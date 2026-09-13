//! The network call behind the current-user cache: a shared, pooled HTTP
//! client for `GET /auth/me`, and the plain fetch-and-classify logic that
//! turns its response into a [`CurrentUserFetchError`] or a sanitized user.

use super::current_user::CurrentUserFetchError;
use super::LOG_PREFIX;
use crate::api::config::effective_backend_api_url;
use crate::api::jwt::bearer_authorization_value;
use crate::config::Config;
use log::{debug, warn};
use once_cell::sync::Lazy;
use reqwest::{header::AUTHORIZATION, Client, Method, Url};
use serde_json::Value;
use std::time::Duration;

pub(super) const AUTH_ME_REVALIDATION_TRANSIENT_STATUSES: &[u16] =
    &[408, 429, 500, 502, 503, 504, 520];

/// One process-wide client for `GET /auth/me`, so its pooled TCP+TLS
/// connection survives between snapshot polls instead of being handshaken
/// again on every one.
///
/// `app_state_snapshot` polls this endpoint for the life of the session. A
/// `Client` built per call gave each poll its own connection pool, so every one
/// paid a fresh TCP connect *and* TLS handshake before the request could go out
/// — two extra WAN round trips on top of the one the request itself costs.
/// Measured against the production backend over a ~250ms RTT link: ~780-1420ms
/// on a cold connection versus ~380-540ms on a reused one (#6180).
///
/// `reqwest::Client` is internally reference-counted and built to be shared;
/// holding one is the only way to keep its pool.
///
/// The product-identity header moved to the request rather than
/// `default_headers`, because this client now outlives
/// [`crate::api::product::set_product_identity`] — baking the header in here
/// would pin whichever identity happened to be installed when the first
/// snapshot ran. `MedullaClient` reads it per request for the same reason.
pub(super) static CURRENT_USER_CLIENT: Lazy<Result<Client, String>> = Lazy::new(|| {
    // Platform-appropriate TLS backend — see [`crate::util::tls`].
    crate::util::tls::tls_client_builder()
        .http1_only()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("failed to build HTTP client: {e}"))
});

pub(super) fn resolve_base(config: &Config) -> Result<Url, String> {
    let base = effective_backend_api_url(&config.api_url);
    let mut parsed =
        Url::parse(base.trim()).map_err(|e| format!("invalid api_url '{}': {e}", base))?;
    if !parsed.path().ends_with('/') && parsed.path() != "/" {
        let normalized = format!("{}/", parsed.path());
        parsed.set_path(&normalized);
    }
    Ok(parsed)
}

pub(super) async fn fetch_current_user(
    config: &Config,
    token: &str,
) -> Result<Option<Value>, CurrentUserFetchError> {
    let client = CURRENT_USER_CLIENT
        .as_ref()
        .map_err(|e| CurrentUserFetchError::FetchFailed(e.clone()))?;
    let base = resolve_base(config).map_err(CurrentUserFetchError::FetchFailed)?;
    let url = base
        .join("auth/me")
        .map_err(|e| CurrentUserFetchError::FetchFailed(format!("build URL failed: {e}")))?;
    let response = client
        .request(Method::GET, url.clone())
        // `GET /auth/me` is backend traffic like any other, so it carries the
        // product identity. This request is hand-rolled rather than issued
        // through `BackendOAuthClient`, so it inherits nothing from that path's
        // default headers — see [`crate::api::product`].
        .headers(crate::api::product::product_identity_headers())
        .header(AUTHORIZATION, bearer_authorization_value(token))
        .send()
        .await
        .map_err(|e| CurrentUserFetchError::FetchFailed(format!("request failed: {e}")))?;
    let status = response.status();
    let text = response.text().await.map_err(|e| {
        CurrentUserFetchError::FetchFailed(format!("failed to read backend response body: {e}"))
    })?;

    debug!("{LOG_PREFIX} GET /auth/me -> {}", status);

    if !status.is_success() {
        let message = format!("{status} {text}");
        warn!(
            "{LOG_PREFIX} current user fetch failed: {} {}",
            status, text
        );
        return if AUTH_ME_REVALIDATION_TRANSIENT_STATUSES.contains(&status.as_u16()) {
            Err(CurrentUserFetchError::TransientResponse(message))
        } else {
            Err(CurrentUserFetchError::Rejected(message))
        };
    }

    let raw: Value =
        serde_json::from_str(&text).unwrap_or_else(|_| Value::String(text.to_string()));
    let user = raw
        .as_object()
        .and_then(|obj| obj.get("data"))
        .cloned()
        .unwrap_or(raw);
    Ok(Some(user))
}

pub(super) fn sanitize_snapshot_user(user: Option<Value>) -> Option<Value> {
    match user {
        Some(Value::Object(map)) if map.is_empty() => None,
        Some(Value::Null) => None,
        other => other,
    }
}

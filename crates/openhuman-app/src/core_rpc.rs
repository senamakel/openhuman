//! Shared helpers for authenticated calls from the Tauri host to the local core RPC.

use reqwest::RequestBuilder;

pub(crate) use openhuman_rpc::{
    bearer_header as relay_bearer_header, redact_url_for_log, HttpRpcResponse as RelayHttpResponse,
};

const CORE_RPC_URL_ENV: &str = "OPENHUMAN_CORE_RPC_URL";
pub(crate) fn core_rpc_url_value() -> String {
    std::env::var(CORE_RPC_URL_ENV).unwrap_or_else(|_| {
        format!(
            "http://127.0.0.1:{}/rpc",
            crate::core_process::default_core_port()
        )
    })
}

pub(crate) fn apply_auth(builder: RequestBuilder) -> Result<RequestBuilder, String> {
    let token = crate::core_process::current_rpc_token()
        .ok_or_else(|| "core RPC token is not initialized".to_string())?;
    Ok(builder.header("Authorization", format!("Bearer {token}")))
}

/// POST a JSON-RPC body to an arbitrary self-hosted runtime URL from the Rust
/// host instead of the webview.
///
/// Why this exists (#3865): the desktop webview origin is `tauri://localhost`,
/// a *secure context*. Chromium treats `http://127.0.0.1` / `localhost` as
/// "potentially trustworthy", so browser `fetch()` to the embedded local core
/// works — but a self-hosted runtime on a LAN IP (e.g.
/// `http://192.168.1.74:7788`) is plain cleartext from a secure context, so the
/// fetch is blocked as mixed content before any request leaves the browser
/// ("Failed to fetch", and the runtime never logs a `/rpc` hit) even though the
/// endpoint is healthy and reachable from curl/Safari. Issuing the request from
/// the Rust host with `reqwest` bypasses the webview's mixed-content / CORS
/// restrictions entirely — the same way the shell already talks to the local
/// core.
///
/// Returns the upstream status + body verbatim (including JSON-RPC error
/// envelopes and any 4xx/5xx) so the renderer keeps its existing handling; only
/// transport-level failures (DNS, connect, timeout) surface as `Err`.
#[tauri::command]
pub(crate) async fn relay_http_rpc(
    url: String,
    token: Option<String>,
    body: String,
) -> Result<RelayHttpResponse, String> {
    post_json_rpc(&url, token.as_deref(), body).await
}

/// Transport core of [`relay_http_rpc`]: POST a JSON body to `url` with an
/// optional bearer, returning the upstream status + body verbatim. Factored out
/// so in-process shell callers (e.g. the desktop companion pipeline) can reach
/// the local core over the same path the renderer's relay uses, without going
/// through the Tauri command boundary.
pub(crate) async fn post_json_rpc(
    url: &str,
    token: Option<&str>,
    body: String,
) -> Result<RelayHttpResponse, String> {
    // Defense in depth behind `store::save`'s validation: never attach a
    // bearer over plain HTTP to a non-loopback host, whatever the caller is.
    // The local core (loopback) and any `https` endpoint keep working; a
    // Remote gateway that slipped past persistence is still rejected here.
    #[cfg(feature = "gateways")]
    if relay_bearer_header(token).is_some()
        && crate::gateway::types::validate_remote_transport(url, token).is_err()
    {
        return Err(format!(
            "refusing to send a bearer to {} over an insecure transport",
            redact_url_for_log(url)
        ));
    }

    openhuman_rpc::post_json_rpc(url, token, body).await
}

#[cfg(test)]
#[path = "core_rpc_tests.rs"]
mod tests;

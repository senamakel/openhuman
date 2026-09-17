//! Channel-link token issuance (an authed backend call; bearer only).

use crate::api::config::effective_backend_api_url;
use crate::api::jwt::get_session_token;
use crate::api::rest::BackendOAuthClient;
use crate::config::Config;
use crate::rpc::RpcOutcome;

pub async fn auth_create_channel_link_token(
    config: &Config,
    channel: &str,
) -> Result<RpcOutcome<serde_json::Value>, String> {
    let channel = channel.trim();
    if channel.is_empty() {
        return Err("channel is required".to_string());
    }
    let channel = channel.to_lowercase();
    if !matches!(channel.as_str(), "telegram" | "discord") {
        return Err(format!("unsupported channel: {channel}"));
    }

    let api_url = effective_backend_api_url(&config.api_url);
    let token = get_session_token(config)?.ok_or_else(|| "session JWT required".to_string())?;
    let client = BackendOAuthClient::new(&api_url).map_err(|e| e.to_string())?;
    let payload = client
        .create_channel_link_token(&channel, &token)
        .await
        // Keeps the typed 401 classifiable as session expiry while non-401s
        // keep their full anyhow chain.
        .map_err(crate::api::flatten_authed_error)?;

    Ok(RpcOutcome::single_log(
        payload,
        "channel link token created",
    ))
}

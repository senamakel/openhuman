use crate::openhuman::skills::global_engine;
use crate::openhuman::webhooks::{
    WebhookDebugLogListResult, WebhookDebugLogsClearedResult, WebhookDebugRegistrationsResult,
    WebhookRequest, WebhookResponseData,
};
use crate::openhuman::config::Config;
use crate::rpc::RpcOutcome;
use base64::Engine;
use log::debug;
use reqwest::{header::AUTHORIZATION, Client, Method, Url};
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;
use crate::api::config::effective_api_url;
use crate::api::jwt::{bearer_authorization_value, get_session_token};

const LOG_PREFIX: &str = "[webhooks]";

fn build_client() -> Result<Client, String> {
    Client::builder()
        .use_rustls_tls()
        .http1_only()
        .timeout(Duration::from_secs(120))
        .connect_timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| format!("failed to build HTTP client: {e}"))
}

fn resolve_base(config: &Config) -> Result<Url, String> {
    let base = effective_api_url(&config.api_url);
    Url::parse(base.trim()).map_err(|e| format!("invalid api_url '{}': {e}", base))
}

fn require_token(config: &Config) -> Result<String, String> {
    get_session_token(config)?
        .and_then(|v| {
            let t = v.trim().to_string();
            if t.is_empty() { None } else { Some(t) }
        })
        .ok_or_else(|| "no backend session token; run auth_store_session first".to_string())
}

async fn authed_request(
    client: &Client,
    base: &Url,
    token: &str,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> Result<Value, String> {
    let url = base
        .join(path.trim_start_matches('/'))
        .map_err(|e| format!("build URL failed: {e}"))?;

    let mut req = client
        .request(method.clone(), url.clone())
        .header(AUTHORIZATION, bearer_authorization_value(token));

    if let Some(body) = body {
        req = req.json(&body);
    }

    let resp = req.send().await.map_err(|e| format!("request failed: {e}"))?;
    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| format!("failed to read response body: {e}"))?;

    debug!("{LOG_PREFIX} {} {} -> {}", method, url, status);

    let raw: Value = serde_json::from_str(&text).unwrap_or_else(|_| Value::String(text.clone()));
    if !status.is_success() {
        let msg = raw
            .as_object()
            .and_then(|o| {
                o.get("message")
                    .or_else(|| o.get("error"))
                    .or_else(|| o.get("detail"))
                    .and_then(|v| v.as_str())
            })
            .unwrap_or(&text);
        return Err(format!(
            "backend responded with {} for {}: {}",
            status.as_u16(),
            url.path(),
            msg
        ));
    }

    if let Some(data) = raw.as_object().and_then(|o| o.get("data")) {
        return Ok(data.clone());
    }

    Ok(raw)
}

async fn get_authed_value(
    config: &Config,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> Result<Value, String> {
    let token = require_token(config)?;
    let client = build_client()?;
    let base = resolve_base(config)?;
    authed_request(&client, &base, &token, method, path, body).await
}

pub async fn list_registrations() -> Result<RpcOutcome<WebhookDebugRegistrationsResult>, String> {
    let engine = global_engine().ok_or_else(|| "skill runtime not initialized".to_string())?;
    let registrations = engine.webhook_router().list_all();
    let count = registrations.len();

    Ok(RpcOutcome::single_log(
        WebhookDebugRegistrationsResult { registrations },
        format!("webhooks.list_registrations returned {count} registration(s)"),
    ))
}

pub async fn list_logs(
    limit: Option<usize>,
) -> Result<RpcOutcome<WebhookDebugLogListResult>, String> {
    let engine = global_engine().ok_or_else(|| "skill runtime not initialized".to_string())?;
    let logs = engine.webhook_router().list_logs(limit);
    let count = logs.len();

    Ok(RpcOutcome::single_log(
        WebhookDebugLogListResult { logs },
        format!("webhooks.list_logs returned {count} log entrie(s)"),
    ))
}

pub async fn clear_logs() -> Result<RpcOutcome<WebhookDebugLogsClearedResult>, String> {
    let engine = global_engine().ok_or_else(|| "skill runtime not initialized".to_string())?;
    let cleared = engine.webhook_router().clear_logs();

    Ok(RpcOutcome::single_log(
        WebhookDebugLogsClearedResult { cleared },
        format!("webhooks.clear_logs removed {cleared} log entrie(s)"),
    ))
}

pub async fn register_echo(
    tunnel_uuid: &str,
    tunnel_name: Option<String>,
    backend_tunnel_id: Option<String>,
) -> Result<RpcOutcome<WebhookDebugRegistrationsResult>, String> {
    let engine = global_engine().ok_or_else(|| "skill runtime not initialized".to_string())?;
    let router = engine.webhook_router();
    router.register_echo(tunnel_uuid, tunnel_name, backend_tunnel_id)?;
    let registrations = router.list_all();

    Ok(RpcOutcome::single_log(
        WebhookDebugRegistrationsResult { registrations },
        format!("webhooks.register_echo registered tunnel {tunnel_uuid}"),
    ))
}

pub async fn unregister_echo(
    tunnel_uuid: &str,
) -> Result<RpcOutcome<WebhookDebugRegistrationsResult>, String> {
    let engine = global_engine().ok_or_else(|| "skill runtime not initialized".to_string())?;
    let router = engine.webhook_router();
    router.unregister(tunnel_uuid, "echo")?;
    let registrations = router.list_all();

    Ok(RpcOutcome::single_log(
        WebhookDebugRegistrationsResult { registrations },
        format!("webhooks.unregister_echo removed tunnel {tunnel_uuid}"),
    ))
}

pub fn build_echo_response(request: &WebhookRequest) -> WebhookResponseData {
    let response_body = serde_json::json!({
        "ok": true,
        "echo": {
            "correlationId": request.correlation_id,
            "tunnelId": request.tunnel_id,
            "tunnelUuid": request.tunnel_uuid,
            "tunnelName": request.tunnel_name,
            "method": request.method,
            "path": request.path,
            "query": request.query,
            "headers": request.headers,
            "bodyBase64": request.body,
        }
    });

    let mut headers = HashMap::new();
    headers.insert("content-type".to_string(), "application/json".to_string());
    headers.insert("x-openhuman-webhook-target".to_string(), "echo".to_string());

    WebhookResponseData {
        correlation_id: request.correlation_id.clone(),
        status_code: 200,
        headers,
        body: base64::engine::general_purpose::STANDARD.encode(response_body.to_string()),
    }
}

pub async fn list_tunnels(config: &Config) -> Result<RpcOutcome<Value>, String> {
    let data = get_authed_value(config, Method::GET, "/webhooks/core", None).await?;
    Ok(RpcOutcome::single_log(data, "webhook tunnels fetched"))
}

pub async fn create_tunnel(
    config: &Config,
    name: &str,
    description: Option<String>,
) -> Result<RpcOutcome<Value>, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("name is required".to_string());
    }
    let body = serde_json::json!({
        "name": name,
        "description": description.and_then(|v| {
            let t = v.trim().to_string();
            (!t.is_empty()).then_some(t)
        }),
    });
    let data = get_authed_value(config, Method::POST, "/webhooks/core", Some(body)).await?;
    Ok(RpcOutcome::single_log(data, "webhook tunnel created"))
}

pub async fn get_tunnel(config: &Config, id: &str) -> Result<RpcOutcome<Value>, String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("id is required".to_string());
    }
    let data = get_authed_value(config, Method::GET, &format!("/webhooks/core/{id}"), None).await?;
    Ok(RpcOutcome::single_log(data, "webhook tunnel fetched"))
}

pub async fn update_tunnel(
    config: &Config,
    id: &str,
    payload: Value,
) -> Result<RpcOutcome<Value>, String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("id is required".to_string());
    }
    let data = get_authed_value(config, Method::PATCH, &format!("/webhooks/core/{id}"), Some(payload)).await?;
    Ok(RpcOutcome::single_log(data, "webhook tunnel updated"))
}

pub async fn delete_tunnel(config: &Config, id: &str) -> Result<RpcOutcome<Value>, String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("id is required".to_string());
    }
    let data = get_authed_value(config, Method::DELETE, &format!("/webhooks/core/{id}"), None).await?;
    Ok(RpcOutcome::single_log(data, "webhook tunnel deleted"))
}

pub async fn get_bandwidth(config: &Config) -> Result<RpcOutcome<Value>, String> {
    let data = get_authed_value(config, Method::GET, "/webhooks/core/bandwidth", None).await?;
    Ok(RpcOutcome::single_log(data, "webhook bandwidth fetched"))
}

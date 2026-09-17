//! `CoreLink` over the shell's own RPC path: the same `(url, token)` the
//! renderer uses, resolved per call so a gateway switch or a core restart is
//! picked up without re-wiring, and the same transport guard
//! (`core_rpc::post_json_rpc`) that refuses a bearer over plain HTTP off
//! loopback.

use async_trait::async_trait;
use openhuman_session::CoreLink;
use serde_json::{json, Value};

use crate::core_process::CoreProcessHandle;

pub struct HttpCoreLink {
    desktop: CoreProcessHandle,
}

impl HttpCoreLink {
    pub fn new(desktop: CoreProcessHandle) -> Self {
        Self { desktop }
    }
}

/// Decode a JSON-RPC 2.0 response body into its `result`, or the error
/// message.
pub(crate) fn decode_rpc_response(status: u16, body: &str) -> Result<Value, String> {
    let parsed: Value = serde_json::from_str(body)
        .map_err(|e| format!("core rpc returned a non-JSON body (http {status}): {e}"))?;
    if let Some(error) = parsed.get("error").filter(|e| !e.is_null()) {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| error.to_string());
        return Err(message);
    }
    if !(200..300).contains(&status) {
        return Err(format!("core rpc failed with http {status}"));
    }
    Ok(parsed.get("result").cloned().unwrap_or(Value::Null))
}

#[async_trait]
impl CoreLink for HttpCoreLink {
    async fn invoke(&self, method: &str, params: Value) -> Result<Value, String> {
        let (url, token) = crate::active_rpc_endpoint(&self.desktop).await;
        let body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        })
        .to_string();
        log::debug!(
            "[session][link] {method} -> {}",
            openhuman_rpc::redact_url_for_log(&url)
        );
        let token = (!token.is_empty()).then_some(token);
        let response = crate::core_rpc::post_json_rpc(&url, token.as_deref(), body).await?;
        decode_rpc_response(response.status, &response.body)
    }
}

#[cfg(test)]
#[path = "link_tests.rs"]
mod tests;

//! Method dispatch for webview_apis requests.
//!
//! Maps a protocol method name (`"gmail.list_labels"`) to the Rust
//! function that handles it. Keep this file as the single place that
//! does the mapping — implementations live in their own connector
//! modules (`crate::gmail`, and future siblings).

use serde_json::{Map, Value};

use crate::gmail;

/// Dispatch a single webview_apis request to its handler. Returns the
/// `result` JSON on success or a string error that the server relays
/// back as `{ ok: false, error }`.
pub async fn dispatch(method: &str, params: Map<String, Value>) -> Result<Value, String> {
    log::debug!("[webview_apis] dispatch method={method}");
    match method {
        "gmail.list_labels" => serialize(
            gmail::cdp_list_labels(&read_string(&params, "account_id")?).await,
        ),
        "gmail.list_messages" => serialize(
            gmail::cdp_list_messages(
                &read_string(&params, "account_id")?,
                read_u32(&params, "limit")?,
                read_optional_string(&params, "label")?,
            )
            .await,
        ),
        "gmail.search" => serialize(
            gmail::cdp_search(
                &read_string(&params, "account_id")?,
                read_string(&params, "query")?,
                read_u32(&params, "limit")?,
            )
            .await,
        ),
        "gmail.get_message" => serialize(
            gmail::cdp_get_message(
                &read_string(&params, "account_id")?,
                read_string(&params, "message_id")?,
            )
            .await,
        ),
        "gmail.send" => {
            let account_id = read_string(&params, "account_id")?;
            let request: gmail::types::GmailSendRequest = serde_json::from_value(
                params
                    .get("request")
                    .cloned()
                    .ok_or_else(|| "missing required param 'request'".to_string())?,
            )
            .map_err(|e| format!("invalid 'request': {e}"))?;
            serialize(gmail::cdp_send(&account_id, request).await)
        }
        "gmail.trash" => serialize(
            gmail::cdp_trash(
                &read_string(&params, "account_id")?,
                read_string(&params, "message_id")?,
            )
            .await,
        ),
        "gmail.add_label" => serialize(
            gmail::cdp_add_label(
                &read_string(&params, "account_id")?,
                read_string(&params, "message_id")?,
                read_string(&params, "label")?,
            )
            .await,
        ),
        _ => Err(format!("unknown webview_apis method: {method}")),
    }
}

fn serialize<T: serde::Serialize>(res: Result<T, String>) -> Result<Value, String> {
    match res {
        Ok(v) => serde_json::to_value(v)
            .map_err(|e| format!("[webview_apis] serialize response failed: {e}")),
        Err(e) => Err(e),
    }
}

// ── param helpers ───────────────────────────────────────────────────────

fn read_string(params: &Map<String, Value>, key: &str) -> Result<String, String> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| format!("missing required string param '{key}'"))
}

fn read_optional_string(
    params: &Map<String, Value>,
    key: &str,
) -> Result<Option<String>, String> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(format!("invalid 'label': expected string")),
    }
}

fn read_u32(params: &Map<String, Value>, key: &str) -> Result<u32, String> {
    params
        .get(key)
        .and_then(|v| v.as_u64())
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| format!("missing or invalid u32 param '{key}'"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn unknown_method_is_rejected() {
        let err = dispatch("something.else", Map::new()).await.unwrap_err();
        assert!(err.contains("unknown webview_apis method"));
    }

    #[tokio::test]
    async fn missing_account_id_reports_clearly() {
        let err = dispatch("gmail.list_labels", Map::new()).await.unwrap_err();
        assert!(err.contains("account_id"), "got: {err}");
    }

    #[tokio::test]
    async fn list_messages_rejects_missing_limit() {
        let mut p = Map::new();
        p.insert("account_id".into(), json!("gmail"));
        let err = dispatch("gmail.list_messages", p).await.unwrap_err();
        assert!(err.contains("limit"), "got: {err}");
    }
}

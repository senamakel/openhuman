//! Checking a requested embedding model against the ids an OpenAI-compatible
//! endpoint reports on `/models`.

use crate::rpc::RpcOutcome;

/// GET `{endpoint}/models` (OpenAI-compatible) and return the served model ids.
/// Time-boxed and best-effort — any failure returns `Err` and the caller falls
/// back to the live test-embed probe (issue #3761).
pub(super) async fn fetch_served_model_ids(
    endpoint: &str,
    api_key: &str,
) -> Result<Vec<String>, String> {
    #[derive(serde::Deserialize)]
    struct ModelEntry {
        id: String,
    }
    #[derive(serde::Deserialize)]
    struct ModelsResponse {
        #[serde(default)]
        data: Vec<ModelEntry>,
    }

    let url = format!("{}/models", endpoint.trim_end_matches('/'));
    let client = reqwest::Client::new();
    let mut req = client.get(&url).timeout(std::time::Duration::from_secs(5));
    if !api_key.trim().is_empty() {
        req = req.bearer_auth(api_key.trim());
    }
    let resp = req
        .send()
        .await
        .map_err(|e| format!("models request failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("models request returned status {}", resp.status()));
    }
    let parsed: ModelsResponse = resp
        .json()
        .await
        .map_err(|e| format!("models parse failed: {e}"))?;
    Ok(parsed.data.into_iter().map(|m| m.id).collect())
}

/// Normalize an embedding model id for tolerant *suggestion* matching:
/// lowercase, drop a leading `text-embedding-`, drop a trailing `:tag`. Used
/// only to suggest the right served name — never to silently rewrite the id.
pub(super) fn normalize_embed_model_id(name: &str) -> String {
    let lower = name.trim().to_ascii_lowercase();
    let stripped = lower.strip_prefix("text-embedding-").unwrap_or(&lower);
    stripped.split(':').next().unwrap_or(stripped).to_string()
}

/// Decide whether the requested model is acceptable given the endpoint's served
/// list. Returns `Some(reject)` only when the endpoint reports a non-empty list
/// that does NOT contain the requested id — i.e. we have positive evidence the
/// model isn't loaded. An empty/unknown list returns `None` (defer to the live
/// test-embed probe) so we never block on a server that doesn't expose
/// `/models` (issue #3761).
pub(super) fn check_requested_model_served(
    requested: &str,
    served: &[String],
) -> Option<RpcOutcome<serde_json::Value>> {
    if served.is_empty() || served.iter().any(|m| m == requested) {
        return None;
    }
    Some(reject_model_not_served(requested, served))
}

/// Build the "model not served" rejection: names what the endpoint actually
/// serves and, when a normalized match exists, suggests the exact name to pick
/// (e.g. `bge-m3` → `text-embedding-bge-m3`). Reuses the
/// `EMBEDDINGS_NO_MODEL_LOADED` error code so the existing Embeddings setup
/// dialog surfaces `message` and keeps the config unsaved (issue #3761).
pub(super) fn reject_model_not_served(
    requested: &str,
    served: &[String],
) -> RpcOutcome<serde_json::Value> {
    let want = normalize_embed_model_id(requested);
    let suggestion = served
        .iter()
        .find(|m| normalize_embed_model_id(m) == want)
        .cloned();
    let served_list = served.join(", ");
    let message = match suggestion.as_deref() {
        Some(s) => format!(
            "`{requested}` isn't loaded on this embeddings server — but the same model appears to be served as `{s}`. Select `{s}` (the exact name your server reports), then save again. Available models: {served_list}."
        ),
        None => format!(
            "`{requested}` isn't loaded on this embeddings server. Select one of the loaded models (the exact name your server reports), then save again. Available models: {served_list}."
        ),
    };
    let mut body = serde_json::json!({
        "error": "EMBEDDINGS_NO_MODEL_LOADED",
        "message": message,
        "requested_model": requested,
        "available_models": served,
    });
    if let Some(s) = suggestion {
        body["suggested_model"] = serde_json::Value::String(s);
    }
    RpcOutcome::new(
        body,
        vec!["embedding model not served by endpoint — not saved".to_string()],
    )
}

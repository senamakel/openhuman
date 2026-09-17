//! Parsing the OpenAI-compatible (and ChatGPT Codex) `/models` response
//! envelope into typed [`ModelInfo`] entries.

use super::*;

/// Parse the OpenAI-compatible `/models` response envelope, or the ChatGPT
/// Codex backend's sibling `models` envelope, into typed [`ModelInfo`] entries.
///
/// Returns distinct errors for the three failure modes the wild has
/// produced in `inference_list_models` Sentry events:
///
/// 1. **Missing `data`/`models` field** — endpoint isn't `/models`-compatible
///    (user typo'd the base URL, pointed at a vector-DB host, etc.).
/// 2. **`data`/`models` field present but wrong type** — provider returned
///    `{"object":"error","data":{…}}` or similar non-array. The error names
///    the actual JSON type so triage knows what the provider sent.
/// 3. **Non-object top-level body** — provider returned a bare array,
///    string, etc. Caught explicitly so the parser doesn't silently
///    drop into the missing-data arm with a `<non-object>` keys list.
///
/// A **null** `data`/`models` field on a **success envelope** is NOT an
/// error — Ollama's OpenAI-compatible `/v1/models` null-encodes the catalog
/// (`{"object":"list","data":null}`) when no models are pulled, so it is
/// treated as an empty model list (TAURI-RUST-874 / TAURI-RUST-875). The
/// null-as-empty short-circuit is gated on `object` being absent or `"list"`:
/// a null `data` on an error envelope (`{"object":"error","data":null}`)
/// instead falls through to failure mode 2 so the provider error still
/// surfaces in the UI and Sentry.
///
/// Per-entry parsing ignores entries that don't have a usable string id/slug
/// (lax on purpose — many OpenAI-compatible servers include malformed rows for
/// capabilities they don't fully implement).
pub fn parse_models_response(body: &serde_json::Value) -> Result<Vec<ModelInfo>, String> {
    let obj = body.as_object().ok_or_else(|| {
        format!(
            "provider response is not a JSON object — endpoint is not OpenAI-compatible (got {} at top level)",
            json_value_kind(body)
        )
    })?;

    let (field_name, data_value) = obj
        .get("data")
        .map(|value| ("data", value))
        .or_else(|| obj.get("models").map(|value| ("models", value)))
        .ok_or_else(|| {
        let keys = obj.keys().cloned().collect::<Vec<_>>().join(", ");
        format!(
                "provider response missing `data` or `models` field — endpoint is not OpenAI-compatible (got keys: {})",
            keys
        )
    })?;

    // A null `data`/`models` field is a valid empty catalog ONLY on a success
    // envelope: Ollama's OpenAI-compatible `/v1/models` returns
    // `{"object":"list","data":null}` (object="list", the success marker) when
    // no models are pulled. Treat that as an empty model list so a healthy-but-
    // empty local runtime doesn't manufacture a hard error (TAURI-RUST-874 /
    // TAURI-RUST-875).
    //
    // An error body such as `{"object":"error","data":null}` ALSO null-encodes
    // `data`; swallowing it as an empty catalog would hide provider/endpoint
    // failures from the UI and Sentry. So gate on a success envelope: short-
    // circuit only when `object` is absent or "list". Any other `object` value
    // (e.g. "error") with null `data` falls through to the descriptive error
    // below, which surfaces the `object` value for triage. Non-array kinds
    // (object/string/number/bool) likewise fall through.
    let is_success_envelope = obj
        .get("object")
        .and_then(|value| value.as_str())
        .is_none_or(|object| object.eq_ignore_ascii_case("list"));

    if data_value.is_null() && is_success_envelope {
        log::info!(
            "[providers][list_models] `{field_name}` is null on a success envelope — provider returned an empty catalog (no models)"
        );
        return Ok(Vec::new());
    }

    let data = data_value.as_array().ok_or_else(|| {
        // Include the sibling `object` field if present — OpenAI-shaped
        // servers set it to `"list"` on success and `"error"` (or omit)
        // on failure, so its value is the fastest triage signal for
        // future Sentry events on the wrong-type arm.
        let object_field = obj
            .get("object")
            .map(|v| v.to_string())
            .unwrap_or_else(|| "<absent>".to_string());
        format!(
            "provider response has `{}` field but it is {}, expected array — endpoint may be returning an error envelope (\"object\" = {})",
            field_name,
            json_value_kind(data_value),
            object_field,
        )
    })?;

    Ok(data
        .iter()
        .filter_map(model_info_from_catalog_item)
        .collect())
}

/// Name the JSON value kind for use in `parse_models_response` error
/// messages. Mirrors `serde_json::Value::*` variants exactly so test
/// assertions on the rendered token (`object`/`string`/`null`/…) stay
/// in lock-step with the matcher.
fn json_value_kind(v: &serde_json::Value) -> &'static str {
    match v {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

pub fn merge_openai_codex_model_hints(models: &mut Vec<ModelInfo>) {
    let mut seen = models
        .iter()
        .map(|model| model.id.to_ascii_lowercase())
        .collect::<std::collections::HashSet<_>>();

    for id in OPENAI_CODEX_MODEL_HINTS {
        if seen.insert(id.to_ascii_lowercase()) {
            models.push(ModelInfo {
                id: (*id).to_string(),
                owned_by: Some("openai-codex".to_string()),
                context_window: None,
                display_name: None,
                input_per_1m: None,
                output_per_1m: None,
            });
        }
    }
}

#[allow(dead_code)]
pub fn model_items_from_body(body: &serde_json::Value) -> Option<Vec<serde_json::Value>> {
    body.get("data")
        .and_then(|d| d.as_array())
        .or_else(|| body.get("models").and_then(|d| d.as_array()))
        .cloned()
}

fn model_info_from_catalog_item(item: &serde_json::Value) -> Option<ModelInfo> {
    if let Some(id) = item.as_str().map(str::trim).filter(|id| !id.is_empty()) {
        return Some(ModelInfo {
            id: id.to_string(),
            owned_by: None,
            context_window: None,
            display_name: None,
            input_per_1m: None,
            output_per_1m: None,
        });
    }

    let id = item
        .get("id")
        .or_else(|| item.get("slug"))
        .or_else(|| item.get("name"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|id| !id.is_empty())?
        .to_string();
    let owned_by = item
        .get("owned_by")
        .or_else(|| item.get("owned_by_organization"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let context_window = item
        .get("context_length")
        .or_else(|| item.get("context_window"))
        .or_else(|| item.get("max_context_window"))
        .and_then(|v| v.as_u64());
    let display_name = item
        .get("display_name")
        .or_else(|| item.get("name"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    // `pricing` is already the CHARGED price per 1M tokens on the managed
    // catalog listing, so it can be surfaced verbatim — no margin math here.
    let pricing = item.get("pricing");
    let price = |key: &str| -> Option<f64> {
        pricing
            .and_then(|p| p.get(key))
            .and_then(|v| v.as_f64())
            .filter(|n| n.is_finite() && *n >= 0.0)
    };
    Some(ModelInfo {
        id,
        owned_by,
        context_window,
        display_name,
        input_per_1m: price("inputPer1M"),
        output_per_1m: price("outputPer1M"),
    })
}

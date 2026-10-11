//! Backend wire shapes for reply speech and hosted session bootstrap.

use serde_json::{json, Value};
use tinyvoice_bus::provider::{RealtimeSession, SynthesisRequest};

pub(super) fn speech_body(request: SynthesisRequest) -> Result<Value, String> {
    let text = request.text.trim();
    if text.is_empty() { return Err("text is required".into()); }
    let mut body = serde_json::Map::from_iter([("text".into(), json!(text)), ("with_visemes".into(), json!(true))]);
    for (key, value) in [("voice_id", request.voice_id), ("model_id", request.model_id), ("output_format", request.output_format)] {
        if let Some(value) = value.as_deref().map(str::trim).filter(|value| !value.is_empty()) { body.insert(key.into(), json!(value)); }
    }
    if let Some(settings) = request.voice_settings.filter(|value| !value.is_null()) { body.insert("voice_settings".into(), settings); }
    Ok(Value::Object(body))
}

pub(super) fn voice_agent_session(raw: &Value) -> Result<RealtimeSession, String> {
    let data = raw.get("data").unwrap_or(raw);
    let field = |key: &str| data.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
    let signed_url = field("signedUrl");
    if signed_url.is_empty() { return Err("backend returned no signed_url".into()); }
    Ok(RealtimeSession::VoiceAgent { signed_url, agent_id: field("agentId"), user_token: field("userToken") })
}

pub(super) fn gemini_session(raw: &Value) -> Result<RealtimeSession, String> {
    let data = raw.get("data").unwrap_or(raw);
    let field = |key: &str| data.get(key).and_then(Value::as_str).filter(|value| !value.is_empty()).unwrap_or_default().to_string();
    let ws_url = field("wsUrl");
    if ws_url.is_empty() { return Err("backend returned no wsUrl for the live session".into()); }
    Ok(RealtimeSession::GeminiLive { ws_url, session_id: field("sessionId"), model: field("model") })
}

#[cfg(test)]
#[path = "requests_tests.rs"]
mod tests;

//! Hosted STT request validation and native SDK multipart dispatch.

use base64::{engine::general_purpose::STANDARD, Engine};
use openhuman_embed::__host::{backend::BackendClient, security::credentials::session_support::BackendCredential};
use reqwest::multipart::{Form, Part};
use tinyvoice_bus::provider::{Transcription, TranscriptionRequest};

const MAX_AUDIO_BASE64_LEN: usize = 33_554_432;
const MAX_RESPONSE_BYTES: usize = 65_536;

pub(super) async fn transcribe(client: &BackendClient, credential: &BackendCredential, request: TranscriptionRequest) -> Result<Transcription, String> {
    let url = client.url_for("/openai/v1/audio/transcriptions").map_err(|error| error.to_string())?;
    ensure_endpoint(&url, credential.is_api_key())?;
    let form = transcription_form(request)?;
    // This OpenAI-compatible endpoint has historically accepted both the JWT
    // and TinyHumans API key as Bearer; keep that endpoint-specific contract.
    let sdk = tinyhumans_sdk::TinyHumansClient::new(client.url_for("/").map_err(|error| error.to_string())?)
        .with_http_client(client.raw_client().map_err(openhuman_embed::__host::backend::flatten_authed_error)?)
        .with_token(Some(credential.secret().to_string()));
    let raw = sdk.raw().post_multipart_with_response_limit("/openai/v1/audio/transcriptions", form, MAX_RESPONSE_BYTES).await
        .map_err(|error| classify_error(error, credential.is_api_key()))?;
    let text = raw.get("text").and_then(serde_json::Value::as_str)
        .ok_or_else(|| "transcription response missing string `text`".to_string())?.trim().to_string();
    Ok(Transcription { text })
}

fn ensure_endpoint(url: &url::Url, is_api_key: bool) -> Result<(), String> {
    let loopback = match url.host() {
        Some(url::Host::Domain("localhost")) => true,
        Some(url::Host::Ipv4(address)) => address.is_loopback(),
        Some(url::Host::Ipv6(address)) => address.is_loopback(),
        _ => false,
    };
    let secure = url.scheme() == "https" || (url.scheme() == "http" && loopback);
    let managed = (url.scheme() == "https" && matches!(url.host_str(), Some("api.tinyhumans.ai" | "staging-api.tinyhumans.ai"))) || loopback;
    if !secure || (is_api_key && !managed) {
        return Err("refusing to send the TinyHumans API key over a non-HTTPS, non-loopback endpoint".into());
    }
    Ok(())
}

fn transcription_form(request: TranscriptionRequest) -> Result<Form, String> {
    let audio = request.audio_base64.trim();
    if audio.is_empty() { return Err("audio_base64 is required".into()); }
    if audio.len() > MAX_AUDIO_BASE64_LEN { return Err("audio_base64 exceeds maximum size (32MB)".into()); }
    let bytes = STANDARD.decode(audio).map_err(|error| format!("invalid base64 audio: {error}"))?;
    if bytes.is_empty() { return Err("decoded audio is empty".into()); }
    let mime = nonempty(request.mime_type.as_deref()).unwrap_or("audio/webm");
    let part = Part::bytes(bytes).file_name(nonempty(request.file_name.as_deref()).unwrap_or("audio.webm").to_string())
        .mime_str(mime).map_err(|error| format!("invalid mime '{mime}': {error}"))?;
    let mut form = Form::new().part("file", part).text("model", nonempty(request.model.as_deref()).unwrap_or("whisper-v1").to_string());
    if let Some(language) = nonempty(request.language.as_deref()) { form = form.text("language", language.to_string()); }
    Ok(form)
}

fn nonempty(value: Option<&str>) -> Option<&str> { value.map(str::trim).filter(|value| !value.is_empty()) }

fn classify_error(error: tinyhumans_sdk::Error, is_api_key: bool) -> String {
    if matches!(error, tinyhumans_sdk::Error::Status { status: 401, .. }) {
        let prefix = if is_api_key { "API_KEY_REJECTED" } else { "SESSION_EXPIRED" };
        return format!("{prefix}: transcription request failed (401 Unauthorized): {error}");
    }
    match error {
        tinyhumans_sdk::Error::ResponseTooLarge { max_bytes } => format!("transcription response exceeds {max_bytes} bytes"),
        other => format!("transcription request failed: {other}"),
    }
}

#[cfg(test)]
#[path = "transcription_tests.rs"]
mod tests;

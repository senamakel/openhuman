//! TinyHumans cloud voice transport installed by host composition.

use std::sync::Arc;

use openhuman_embed::__host::{
    backend::{BackendApiError, BackendClient},
    config::Config,
    platform::cloud_voice_provider::{self, CloudVoiceProviderFactory},
    security::credentials::session_support::{resolve_backend_credential, BackendCredential},
};
use reqwest::Method;
use serde_json::Value;
use tinyvoice_bus::provider::{
    CloudVoiceProvider, ProviderError, ProviderErrorKind, ProviderFuture, RealtimeRequest,
    RealtimeSession, SynthesisRequest, Transcription, TranscriptionRequest,
};

mod requests;
mod transcription;

pub(crate) fn install() {
    cloud_voice_provider::install(Arc::new(Factory));
}

struct Factory;

impl CloudVoiceProviderFactory for Factory {
    fn for_config(&self, config: &Config) -> Result<Arc<dyn CloudVoiceProvider>, String> {
        // Each voice call constructs its provider from current credentials; a
        // process-level factory never captures a login token at installation.
        let credential = resolve_backend_credential(config)?;
        let client = BackendClient::from_config(config).map_err(|error| error.to_string())?;
        Ok(Arc::new(Provider { client, credential }))
    }
}

struct Provider {
    client: BackendClient,
    credential: BackendCredential,
}

impl CloudVoiceProvider for Provider {
    fn transcribe(&self, request: TranscriptionRequest) -> ProviderFuture<'_, Transcription> {
        Box::pin(transcription::transcribe(
            &self.client,
            &self.credential,
            request,
        ))
    }

    fn synthesize(&self, request: SynthesisRequest) -> ProviderFuture<'_, Value> {
        Box::pin(async move {
            let body = requests::speech_body(request)
                .map_err(|message| provider_error(ProviderErrorKind::InvalidInput, message))?;
            self.client
                .authed_json(
                    &self.credential,
                    Method::POST,
                    "/openai/v1/audio/speech",
                    Some(body),
                )
                .await
                .map_err(classify_backend_error)
        })
    }

    fn realtime(&self, request: RealtimeRequest) -> ProviderFuture<'_, RealtimeSession> {
        Box::pin(async move {
            match request {
                RealtimeRequest::VoiceAgent => {
                    let raw = self
                        .client
                        .authed_json(
                            &self.credential,
                            Method::GET,
                            "/voice-agent/get-signed-url",
                            None,
                        )
                        .await
                        .map_err(classify_backend_error)?;
                    requests::voice_agent_session(&raw).map_err(|message| {
                        provider_error(ProviderErrorKind::InvalidResponse, message)
                    })
                }
                RealtimeRequest::GeminiLive { options } => {
                    let raw = self
                        .client
                        .authed_json(
                            &self.credential,
                            Method::POST,
                            "/agent-integrations/gemini/live/sessions",
                            Some(options),
                        )
                        .await
                        .map_err(classify_backend_error)?;
                    requests::gemini_session(&raw).map_err(|message| {
                        provider_error(ProviderErrorKind::InvalidResponse, message)
                    })
                }
            }
        })
    }
}

fn provider_error(kind: ProviderErrorKind, message: String) -> ProviderError {
    ProviderError { kind, message }
}

fn classify_backend_error(error: anyhow::Error) -> ProviderError {
    let kind = match error.downcast_ref::<BackendApiError>() {
        Some(BackendApiError::Unauthorized { .. }) => ProviderErrorKind::SessionExpired,
        Some(BackendApiError::ApiKeyRejected { .. }) => ProviderErrorKind::ApiKeyRejected,
        Some(BackendApiError::BackendUnavailable { .. }) => ProviderErrorKind::Unavailable,
        _ => ProviderErrorKind::Transport,
    };
    provider_error(kind, error.to_string())
}

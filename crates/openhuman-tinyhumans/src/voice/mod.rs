//! TinyHumans cloud voice transport installed by host composition.

use std::sync::Arc;

use openhuman_embed::__host::{
    backend::{flatten_authed_error, BackendClient},
    config::Config,
    platform::cloud_voice_provider::{self, CloudVoiceProviderFactory},
    security::credentials::session_support::{resolve_backend_credential, BackendCredential},
};
use reqwest::Method;
use serde_json::Value;
use tinyvoice_bus::provider::{CloudVoiceProvider, ProviderFuture, RealtimeRequest, RealtimeSession, SynthesisRequest, Transcription, TranscriptionRequest};

mod transcription;
mod requests;

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
        Box::pin(transcription::transcribe(&self.client, &self.credential, request))
    }

    fn synthesize(&self, request: SynthesisRequest) -> ProviderFuture<'_, Value> {
        Box::pin(async move {
            let body = requests::speech_body(request)?;
            self.client.authed_json(&self.credential, Method::POST, "/openai/v1/audio/speech", Some(body)).await.map_err(flatten_authed_error)
        })
    }

    fn realtime(&self, request: RealtimeRequest) -> ProviderFuture<'_, RealtimeSession> {
        Box::pin(async move {
            match request {
                RealtimeRequest::VoiceAgent => {
                    let raw = self.client.authed_json(&self.credential, Method::GET, "/voice-agent/get-signed-url", None).await.map_err(flatten_authed_error)?;
                    requests::voice_agent_session(&raw)
                }
                RealtimeRequest::GeminiLive { options } => {
                    let raw = self.client.authed_json(&self.credential, Method::POST, "/agent-integrations/gemini/live/sessions", Some(options)).await.map_err(flatten_authed_error)?;
                    requests::gemini_session(&raw)
                }
            }
        })
    }
}

//! Realtime voice-agent bootstrap through the installed host provider.
//!
//! The voice domain preserves the renderer contract and internal approval
//! classification. Backend transport and credentials belong to the host layer.

use serde::{Deserialize, Serialize};
use crate::config::Config;
use crate::core::Outcome;
use crate::platform::cloud_voice_provider;
use tinyvoice_bus::provider::{RealtimeRequest, RealtimeSession};

/// A short-lived signed URL for a realtime voice-agent session plus the agent
/// it was minted for. Mirrors the backend `/voice-agent/get-signed-url` shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceAgentSignedUrl {
    pub signed_url: String,
    pub agent_id: String,
    /// Short-lived token binding this session to the signed-in user. The
    /// renderer echoes it back as the ElevenLabs `userId`, which the backend
    /// relay verifies — so the Custom-LLM relay never trusts a raw user id
    /// (#5399). Empty only against an older backend that predates the binding.
    #[serde(default)]
    pub user_token: String,
}

/// Mint a realtime voice-agent connection through the installed provider.
pub async fn mint_voice_agent_signed_url(
    config: &Config,
) -> Result<Outcome<VoiceAgentSignedUrl>, String> {
    let session = cloud_voice_provider::for_config(config)?
        .realtime(RealtimeRequest::VoiceAgent)
        .await
        .map_err(cloud_voice_provider::product_error)?;
    let RealtimeSession::VoiceAgent { signed_url, agent_id, user_token } = session else {
        return Err("voice provider returned the wrong realtime session kind".into());
    };
    Ok(Outcome::single_log(
        VoiceAgentSignedUrl { signed_url, agent_id, user_token },
        "voice agent connection minted through the installed provider",
    ))
}

#[cfg(test)]
#[path = "realtime_tests.rs"]
mod tests;

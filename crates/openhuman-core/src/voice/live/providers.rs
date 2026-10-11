//! Live provider catalogue, configuration and connection preparation.
//!
//! Four providers, two of them hosted by the TinyHumans backend:
//!
//! | id | reached through | credential |
//! | --- | --- | --- |
//! | `gemini-hosted` | backend Gemini Live relay ticket | the backend credential |
//! | `elevenlabs-hosted` | backend-signed ElevenLabs agent URL | the backend credential |
//! | `gemini` | Google directly | `provider:google` |
//! | `sarvam` | Sarvam directly | `provider:sarvam` |
//!
//! Ticket and signed URL requests dispatch through the installed host provider;
//! generic live configuration and connection preparation remain here.

use serde_json::json;
use tinyagents_live::tinyliveagents::elevenlabs::ElevenLabsConvai;
use tinyagents_live::tinyliveagents::gemini::{self, GeminiLive, GeminiRelay};
use tinyagents_live::tinyliveagents::sarvam::SarvamCascade;
use tinyagents_live::tinyliveagents::{AudioFormat, LiveConfig, LiveProvider};

use super::error::LiveVoiceError;
use super::types::{LiveProviderInfo, LiveProviderKind};
use crate::config::schema::voice_live::{
    LIVE_PROVIDERS, LIVE_PROVIDER_ELEVENLABS_HOSTED, LIVE_PROVIDER_GEMINI,
    LIVE_PROVIDER_GEMINI_HOSTED, LIVE_PROVIDER_SARVAM,
};
use crate::config::Config;
use crate::platform::cloud_voice_provider;
use tinyvoice_bus::provider::{RealtimeRequest, RealtimeSession};

const LOG_PREFIX: &str = "[voice-live]";

/// Gemini Live prebuilt voices.
const GEMINI_VOICES: &[&str] = &[
    "Puck", "Charon", "Kore", "Fenrir", "Aoede", "Leda", "Orus", "Zephyr",
];
/// Languages Gemini Live speaks well (it understands more).
const GEMINI_LANGUAGES: &[&str] = &[
    "en-US", "en-IN", "en-GB", "hi-IN", "es-US", "es-ES", "fr-FR", "de-DE", "it-IT", "pt-BR",
    "ja-JP", "ko-KR", "cmn-CN", "ar-XA", "id-ID", "ru-RU", "pl-PL", "bn-IN",
];
/// Sarvam `bulbul:v3` speakers offered in the UI.
const SARVAM_SPEAKERS: &[&str] = &[
    "shubh", "priya", "aditya", "ritu", "neha", "rahul", "pooja", "rohan", "simran", "kavya",
    "amit", "dev", "ishita", "shreya", "varun", "kabir",
];
/// Languages Sarvam can both hear and speak, plus automatic detection.
const SARVAM_LANGUAGES: &[&str] = &[
    "en-IN", "hi-IN", "bn-IN", "gu-IN", "kn-IN", "ml-IN", "mr-IN", "od-IN", "pa-IN", "ta-IN",
    "te-IN", "auto",
];

/// The BYOK key slug a provider reads, if any.
pub(crate) fn key_slug(provider: &str) -> Option<&'static str> {
    match provider {
        LIVE_PROVIDER_GEMINI => Some("google"),
        LIVE_PROVIDER_SARVAM => Some("sarvam"),
        _ => None,
    }
}

/// The stored key for `slug`, or `None` when none is stored.
pub(crate) fn stored_key(slug: &str, config: &Config) -> Option<String> {
    crate::inference::provider::factory::lookup_key_for_slug(slug, config)
        .ok()
        .filter(|key| !key.trim().is_empty())
}

/// Whether a backend credential is available for hosted providers.
fn backend_ready(config: &Config) -> bool {
    cloud_voice_provider::for_config(config).is_ok()
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_string()).collect()
}

/// The provider catalogue with each provider's readiness.
pub(crate) fn provider_infos(config: &Config) -> Vec<LiveProviderInfo> {
    let hosted = backend_ready(config);
    LIVE_PROVIDERS
        .iter()
        .map(|id| {
            let slug = key_slug(id);
            let (label, kind, voices, languages) = match *id {
                LIVE_PROVIDER_GEMINI_HOSTED => (
                    "Gemini Live (TinyHumans)",
                    LiveProviderKind::Hosted,
                    strings(GEMINI_VOICES),
                    strings(GEMINI_LANGUAGES),
                ),
                LIVE_PROVIDER_ELEVENLABS_HOSTED => (
                    "ElevenLabs Agent (TinyHumans)",
                    LiveProviderKind::Hosted,
                    Vec::new(),
                    Vec::new(),
                ),
                LIVE_PROVIDER_GEMINI => (
                    "Gemini Live (Google API key)",
                    LiveProviderKind::Byok,
                    strings(GEMINI_VOICES),
                    strings(GEMINI_LANGUAGES),
                ),
                _ => (
                    "Sarvam AI",
                    LiveProviderKind::Byok,
                    strings(SARVAM_SPEAKERS),
                    strings(SARVAM_LANGUAGES),
                ),
            };
            let configured = match slug {
                Some(slug) => stored_key(slug, config).is_some(),
                None => hosted,
            };
            LiveProviderInfo {
                id: (*id).to_string(),
                label: label.to_string(),
                kind,
                configured,
                key_slug: slug.map(str::to_string),
                voices,
                languages,
            }
        })
        .collect()
}

/// The session configuration for `provider`, from the user's settings.
pub(crate) fn live_config(
    config: &Config,
    provider: &str,
    system_instruction: &str,
    input_sample_rate: u32,
) -> LiveConfig {
    let settings = &config.voice_live;
    let mut live = LiveConfig::new();
    live.input_format = AudioFormat::pcm16(input_sample_rate);
    match provider {
        LIVE_PROVIDER_GEMINI_HOSTED | LIVE_PROVIDER_GEMINI => {
            live.system_instruction = Some(system_instruction.to_string());
            live.model = settings.gemini.model.clone();
            live.voice = settings.gemini.voice.clone();
            live.language = settings.gemini.language.clone();
            // Keep long conversations inside the context window.
            live.provider_options =
                json!({ "context_window_compression": { "slidingWindow": {} } });
        }
        LIVE_PROVIDER_SARVAM => {
            live.system_instruction = Some(system_instruction.to_string());
            live.model = settings.sarvam.model.clone();
            live.voice = settings.sarvam.speaker.clone();
            match settings.sarvam.language.as_deref() {
                Some("auto") => live.provider_options = json!({ "auto_language": true }),
                other => live.language = Some(other.unwrap_or("en-IN").to_string()),
            }
        }
        LIVE_PROVIDER_ELEVENLABS_HOSTED => {
            // The hosted agent owns its prompt and its brain (OpenHuman over
            // the Custom-LLM relay); only the voice may be overridden.
            live.voice = non_empty(settings.elevenlabs.voice_id.clone());
        }
        _ => {}
    }
    live
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|v| !v.trim().is_empty())
}

/// A provider ready to connect, with the configuration it must connect with.
pub(crate) struct PreparedProvider {
    pub(crate) provider: Box<dyn LiveProvider>,
    pub(crate) config: LiveConfig,
}

/// Resolves `provider` to something `tinyagents-live` can connect, minting a
/// backend ticket or signed URL for the hosted ones.
pub(crate) async fn prepare(
    config: &Config,
    provider: &str,
    mut live: LiveConfig,
) -> Result<PreparedProvider, LiveVoiceError> {
    match provider {
        LIVE_PROVIDER_GEMINI_HOSTED => {
            let ticket = mint_gemini_ticket(config, &live).await?;
            tracing::debug!(
                "{LOG_PREFIX} minted gemini live ticket session={} model={}",
                ticket.session_id,
                ticket.model
            );
            let relay = GeminiRelay::connect_url(ticket.ws_url)
                .with_session(ticket.session_id, ticket.model);
            Ok(PreparedProvider {
                provider: Box::new(relay),
                config: live,
            })
        }
        LIVE_PROVIDER_ELEVENLABS_HOSTED => {
            let signed = crate::voice::realtime::mint_voice_agent_signed_url(config)
                .await
                .map_err(LiveVoiceError::backend)?
                .value;
            // `user_token` is the identity the backend's Custom-LLM relay
            // verifies on every relayed turn (#5399).
            live.provider_options = json!({
                "user_id": signed.user_token,
                "custom_llm_extra_body": { "user": signed.user_token },
            });
            Ok(PreparedProvider {
                provider: Box::new(ElevenLabsConvai::connect_url(signed.signed_url)),
                config: live,
            })
        }
        LIVE_PROVIDER_GEMINI => {
            let key = stored_key("google", config).ok_or_else(|| {
                LiveVoiceError::not_configured("add a Google API key for Gemini Live")
            })?;
            Ok(PreparedProvider {
                provider: Box::new(GeminiLive::new(key)),
                config: live,
            })
        }
        LIVE_PROVIDER_SARVAM => {
            let key = stored_key("sarvam", config)
                .ok_or_else(|| LiveVoiceError::not_configured("add a Sarvam API key"))?;
            Ok(PreparedProvider {
                provider: Box::new(SarvamCascade::new(key)),
                config: live,
            })
        }
        other => Err(LiveVoiceError::invalid(format!(
            "unknown live provider `{other}`"
        ))),
    }
}

/// A minted relay ticket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GeminiTicket {
    pub(crate) ws_url: String,
    pub(crate) session_id: String,
    pub(crate) model: String,
}

async fn mint_gemini_ticket(
    config: &Config,
    live: &LiveConfig,
) -> Result<GeminiTicket, LiveVoiceError> {
    let session = cloud_voice_provider::for_config(config)
        .map_err(LiveVoiceError::not_configured)?
        .realtime(RealtimeRequest::GeminiLive {
            options: gemini::ticket_request(live),
        })
        .await
        .map_err(|error| LiveVoiceError::backend(cloud_voice_provider::product_error(error)))?;
    let RealtimeSession::GeminiLive {
        ws_url,
        session_id,
        model,
    } = session
    else {
        return Err(LiveVoiceError::backend(
            "voice provider returned the wrong realtime session kind",
        ));
    };
    Ok(GeminiTicket {
        ws_url,
        session_id,
        model,
    })
}

#[cfg(test)]
#[path = "providers_tests.rs"]
mod tests;

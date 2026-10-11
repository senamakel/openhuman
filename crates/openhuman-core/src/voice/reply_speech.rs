//! Reply-speech synthesis — proxies the hosted backend's
//! `/openai/v1/audio/speech` endpoint (ElevenLabs under the hood) so the
//! desktop UI does not have to talk to it directly. Returns base64-encoded
//! audio + an Oculus-15 viseme alignment timeline the mascot uses for
//! lip-sync.
//!
//! Lives in the voice domain because the response is consumed by the
//! mascot's lipsync pipeline (`useHumanMascot` → `findActiveFrame` →
//! `oculusVisemeToShape`).
//!
//! Approval gate (#1339) classification: **internal**. Reply-speech is
//! the user's own assistant speaking through the user's own speakers
//! — there is no outbound side effect visible to a third party.
//! Coordinate with #1206 voice work: if `reply_speech` is ever wrapped
//! in a `Tool` impl, the `external_effect()` method MUST stay `false`
//! (the trait's default) so the approval gate never prompts on TTS.

use log::{debug, warn};
use serde_json::Value;

use crate::config::Config;
use crate::core::Outcome;
use crate::platform::cloud_voice_provider;
use tinyvoice_bus::provider::SynthesisRequest;

const LOG_PREFIX: &str = "[voice_reply]";

/// Env var that activates the [`test_seam`] short-circuit at runtime. When
/// set to `1` / `true`, [`synthesize_reply`] records the requested text
/// into [`test_seam::OBSERVED_CALLS`] and returns a stub
/// [`ReplySpeechResult`] *without* contacting the hosted backend. Anything
/// else (unset, `0`, `false`, …) leaves the production code path
/// untouched.
///
/// The env-var gate (rather than a `#[cfg(test)]` gate) is deliberate:
/// integration tests in `tests/` are compiled against the production
/// `openhuman_core` crate, so a unit-only `cfg(test)` block would not be
/// visible from there. The observer module itself is always compiled,
/// but its only producer is this env-gated branch and its only consumer
/// is the test harness, so production callers never touch it.
pub const TEST_SEAM_ENV: &str = "OPENHUMAN_TEST_REPLY_SPEECH_SEAM";

fn test_seam_enabled() -> bool {
    matches!(
        std::env::var(TEST_SEAM_ENV).ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE")
    )
}

/// Test seam observation log. See [`TEST_SEAM_ENV`] for the activation
/// gate. Always compiled (the visibility lets `tests/json_rpc_e2e.rs`
/// inspect calls), but only written to when the env gate is on.
pub mod test_seam {
    use once_cell::sync::Lazy;
    use std::sync::Mutex;

    /// FIFO log of every `text` argument that flowed through the test-seam
    /// short-circuit in [`super::synthesize_reply`]. Cleared between tests
    /// with [`clear`].
    pub static OBSERVED_CALLS: Lazy<Mutex<Vec<String>>> = Lazy::new(|| Mutex::new(Vec::new()));

    /// Clear the observation log.
    pub fn clear() {
        OBSERVED_CALLS.lock().unwrap().clear();
    }

    /// Snapshot of the observation log.
    pub fn observed() -> Vec<String> {
        OBSERVED_CALLS.lock().unwrap().clone()
    }
}

// The response types and the tolerant-shape parsers live in
// `tinyinference_voice::reply`; the UI contract is unchanged.
use tinyinference_voice::reply::{normalize_response, ReplySpeech as ReplySpeechResult};

/// Caller-tunable knobs.
#[derive(Debug, Default, Clone)]
pub struct ReplySpeechOptions {
    pub voice_id: Option<String>,
    pub model_id: Option<String>,
    pub output_format: Option<String>,
    /// ElevenLabs `voice_settings` blob — passed through verbatim.
    /// Typical fields: `stability`, `similarity_boost`, `style`,
    /// `use_speaker_boost`. The backend forwards this to ElevenLabs;
    /// unknown keys are dropped server-side.
    pub voice_settings: Option<Value>,
}

/// Synthesize the agent's reply through the hosted backend.
///
/// The host provider owns backend authentication and transport.
pub async fn synthesize_reply(
    config: &Config,
    text: &str,
    opts: &ReplySpeechOptions,
) -> Result<Outcome<ReplySpeechResult>, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("text is required".to_string());
    }

    // Test seam: when OPENHUMAN_TEST_REPLY_SPEECH_SEAM is set (and only in
    // debug builds — the seam is structurally dead in release), record the
    // call and short-circuit before hitting the backend.
    // See `test_seam` module docs and `TEST_SEAM_ENV` for the activation gate.
    if cfg!(debug_assertions) && test_seam_enabled() {
        warn!(
            "[voice_reply] TEST SEAM ACTIVE — synthesize_reply short-circuited ({} is set); skipping backend call",
            TEST_SEAM_ENV
        );
        let _ = (config, opts);
        test_seam::OBSERVED_CALLS
            .lock()
            .unwrap()
            .push(trimmed.to_string());
        return Ok(Outcome::single_log(
            ReplySpeechResult {
                audio_base64: String::new(),
                audio_mime: "audio/mpeg".to_string(),
                visemes: Vec::new(),
                alignment: None,
            },
            "voice reply synthesized (test seam short-circuit)",
        ));
    }

    debug!(
        "{LOG_PREFIX} synthesize chars={} voice={}",
        trimmed.len(),
        opts.voice_id.as_deref().unwrap_or("default")
    );

    let raw = cloud_voice_provider::for_config(config)?
        .synthesize(SynthesisRequest {
            text: trimmed.to_string(),
            voice_id: opts.voice_id.clone(),
            model_id: opts.model_id.clone(),
            output_format: opts.output_format.clone(),
            voice_settings: opts.voice_settings.clone(),
        })
        .await
        .map_err(cloud_voice_provider::product_error)?;

    let result = normalize_response(&raw);
    debug!(
        "{LOG_PREFIX} synthesized audio_bytes={} visemes={} alignment={}",
        result.audio_base64.len(),
        result.visemes.len(),
        result.alignment.as_ref().map_or(0, Vec::len)
    );

    Ok(Outcome::single_log(
        result,
        "voice reply synthesized via POST /openai/v1/audio/speech",
    ))
}

#[cfg(test)]
#[path = "reply_speech_tests.rs"]
mod tests;

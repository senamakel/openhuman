//! Cloud speech-to-text dispatch through the installed host provider.

use crate::config::Config;
use crate::core::Outcome;
use crate::platform::cloud_voice_provider;
use tinyvoice_bus::provider::TranscriptionRequest;

pub use tinyvoice_bus::provider::Transcription as CloudTranscribeResult;

/// Caller-selected hints for the cloud transcription controller.
#[derive(Debug, Default, Clone)]
pub struct CloudTranscribeOptions {
    /// Optional transcription model.
    pub model: Option<String>,
    /// Optional language hint.
    pub language: Option<String>,
    /// Input audio MIME type.
    pub mime_type: Option<String>,
    /// Original file-name hint.
    pub file_name: Option<String>,
}

/// Transcribe renderer-supplied base64 audio through the installed provider.
pub async fn transcribe_cloud(
    config: &Config,
    audio_base64: &str,
    options: &CloudTranscribeOptions,
) -> Result<Outcome<CloudTranscribeResult>, String> {
    let result = cloud_voice_provider::for_config(config)?
        .transcribe(TranscriptionRequest {
            audio_base64: audio_base64.to_string(),
            model: options.model.clone(),
            language: options.language.clone(),
            mime_type: options.mime_type.clone(),
            file_name: options.file_name.clone(),
        })
        .await
        .map_err(cloud_voice_provider::product_error)?;
    Ok(Outcome::single_log(
        result,
        "cloud STT through the installed voice provider",
    ))
}

#[cfg(test)]
#[path = "cloud_transcribe_tests.rs"]
mod tests;

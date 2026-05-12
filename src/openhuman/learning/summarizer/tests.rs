//! Tests for the summarizer trait + ConfiguredSummarizer wrapper.

use super::*;
use crate::openhuman::providers::Provider;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Recording double that captures the model hint + last prompt and
/// returns a canned response.
#[derive(Default)]
struct RecordingProvider {
    last_prompt: Mutex<Option<String>>,
    last_model: Mutex<Option<String>>,
    response: Mutex<String>,
}

#[async_trait]
impl Provider for RecordingProvider {
    async fn chat_with_system(
        &self,
        _system_prompt: Option<&str>,
        message: &str,
        model: &str,
        _temperature: f64,
    ) -> anyhow::Result<String> {
        *self.last_prompt.lock().await = Some(message.to_string());
        *self.last_model.lock().await = Some(model.to_string());
        Ok(self.response.lock().await.clone())
    }
}

#[tokio::test]
async fn summarizer_dispatches_with_model_hint() {
    let provider = Arc::new(RecordingProvider::default());
    *provider.response.lock().await = "merged".into();
    let summarizer =
        ConfiguredSummarizer::new(provider.clone(), "hint:fast", 4_000, "test-summarizer");

    let out = summarizer.prompt("input").await.unwrap();
    assert_eq!(out, "merged");
    assert_eq!(provider.last_prompt.lock().await.as_deref(), Some("input"));
    assert_eq!(
        provider.last_model.lock().await.as_deref(),
        Some("hint:fast")
    );
    assert_eq!(summarizer.label(), "test-summarizer");
    assert_eq!(summarizer.context_window_chars(), 4_000);
}

#[tokio::test]
async fn context_window_floor_clamps_misconfigured_values() {
    let provider = Arc::new(RecordingProvider::default());
    // Anything below MIN_SUMMARIZER_CONTEXT_CHARS is bumped up so a
    // zero / accidentally-tiny config can't produce empty digests.
    let summarizer = ConfiguredSummarizer::new(provider, "hint:fast", 0, "tiny");
    assert_eq!(
        summarizer.context_window_chars(),
        MIN_SUMMARIZER_CONTEXT_CHARS
    );
}

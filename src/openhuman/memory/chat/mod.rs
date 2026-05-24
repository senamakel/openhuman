//! Memory-tree chat backend abstraction.
//!
//! The memory_tree's two LLM consumers (the entity extractor and the
//! summariser) both want a small, structured "give me JSON for this prompt"
//! call. Historically each built its own `reqwest::Client` and talked to a
//! local Ollama daemon directly. This module replaces that with a single
//! [`ChatProvider`] trait so the same call site can be served by either:
//!
//! - **Cloud** — `providers::router` against the OpenHuman backend with
//!   the `summarization-v1` model. No local daemon required. Default for new
//!   installs.
//! - **Local** — the legacy Ollama-direct path. Opt-in via
//!   `memory_tree.llm_backend = "local"` in config or
//!   `OPENHUMAN_MEMORY_TREE_LLM_BACKEND=local`.
//!
//! ## Why a memory-tree-local trait
//!
//! The existing top-level [`crate::openhuman::inference::provider::Provider`] trait
//! is rich (streaming, native tool calling, vision, …) and depends on the
//! agent's full conversation surface. The extractor and summariser only
//! need:
//!
//! 1. Send a (system, user) prompt pair.
//! 2. Get a JSON-shaped string back.
//!
//! Defining [`ChatProvider`] here keeps the memory_tree decoupled from
//! the agent's prompt/tool-calling stack, makes the extractor/summariser
//! trivial to mock in unit tests, and lets us route either the cloud or
//! the local backend through the same trait object.
//!
//! ## Soft-fallback contract
//!
//! Implementations of `chat_for_json` MUST NOT return `Err` for transient
//! upstream issues. Both memory_tree consumers fall back to a deterministic
//! no-op when the LLM is unavailable; bubbling the error up would abort
//! ingest cascades. Real bugs (e.g. malformed config) are still acceptable
//! `Err` cases — they should be rare and surfaced loudly.
//!
//! See [`local::OllamaChatProvider`] and [`cloud::CloudChatProvider`] for
//! the two production implementations.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use crate::openhuman::config::{Config, DEFAULT_CLOUD_LLM_MODEL};

pub mod cloud;
pub mod local;

/// One pair of prompt messages handed to the chat backend.
///
/// Keeps the surface deliberately tiny — the memory_tree's two consumers
/// both build a system prompt + a single user message. Multi-turn,
/// streaming, and tool calling are out of scope.
#[derive(Debug, Clone)]
pub struct ChatPrompt {
    /// System prompt anchoring the model's role and expected output schema.
    pub system: String,
    /// User prompt carrying the dynamic input (the chunk text, the inputs
    /// to summarise, etc.).
    pub user: String,
    /// Sampling temperature. Both consumers use 0.0 today (max determinism).
    pub temperature: f64,
    /// Diagnostic tag included in tracing logs so seal-time and admit-time
    /// calls are easy to disambiguate. Stable, lowercase, no PII.
    pub kind: &'static str,
}

/// Pluggable chat surface used by the memory_tree's extractor + summariser.
///
/// Returns the model's raw output as a string. Callers parse it themselves
/// (typically as JSON conforming to a schema embedded in the system prompt)
/// because the parsing logic is consumer-specific.
#[async_trait]
pub trait ChatProvider: Send + Sync {
    /// Stable, grep-friendly name for logs. e.g. `"cloud:summarization-v1"`.
    fn name(&self) -> &str;

    /// Run one chat completion and return the assistant's content,
    /// constraining the model to JSON output where the wire format
    /// supports it (Ollama's `format: "json"`).
    ///
    /// Implementations should log entry / exit at debug level under the
    /// `[memory::chat]` prefix.
    async fn chat_for_json(&self, prompt: &ChatPrompt) -> Result<String>;

    /// Run one chat completion and return the assistant's plain-text
    /// content. Unlike [`chat_for_json`], implementations MUST NOT
    /// enable any wire-level JSON-mode flag — used by the summariser
    /// which emits prose, not a structured envelope.
    ///
    /// Default impl forwards to `chat_for_json`; providers that gate
    /// JSON-mode at the wire (e.g. Ollama) override to skip it.
    async fn chat_for_text(&self, prompt: &ChatPrompt) -> Result<String> {
        self.chat_for_json(prompt).await
    }
}

/// Build the [`ChatProvider`] dictated by the unified
/// `Config::workload_local_model("memory")`.
///
/// - When that returns `None` (i.e. `memory_provider` is unset / `"cloud"`):
///   wires [`cloud::CloudChatProvider`] against the OpenHuman backend with
///   `cloud_llm_model` (defaulting to `summarization-v1`).
/// - When it returns `Some(model)` (i.e. `memory_provider = "ollama:<m>"`):
///   wires [`local::OllamaChatProvider`] against the legacy
///   `llm_extractor_endpoint` / `llm_summariser_endpoint` (the daemon
///   endpoints stay in the `memory_tree` block — only the cloud/local
///   routing decision moves to the unified `memory_provider`).
///
/// Memory always uses the summariser model for every LLM call (walking,
/// summarising, extracting). The previous per-consumer split (Extract vs
/// Summarise endpoint/model/timeout) was collapsed because every site
/// reaches for the same model in practice.
///
/// Local (Ollama): reads `memory_tree.llm_summariser_*`. Cloud: reads
/// `memory_tree.cloud_llm_model` (default `summarization-v1`).
pub fn build_chat_provider(config: &Config) -> Result<Arc<dyn ChatProvider>> {
    #[cfg(test)]
    if let Some(p) = test_override::current() {
        return Ok(p);
    }
    if let Some(routed_model) = config.workload_local_model("memory") {
        let endpoint = config.memory_tree.llm_summariser_endpoint.clone();
        let model = config
            .memory_tree
            .llm_summariser_model
            .clone()
            .or(Some(routed_model));
        let timeout_ms = config
            .memory_tree
            .llm_summariser_timeout_ms
            .unwrap_or(120_000);
        log::debug!(
            "[memory::chat] building Local (Ollama) provider \
             endpoint_set={} model_set={} timeout_ms={}",
            endpoint.is_some(),
            model.is_some(),
            timeout_ms
        );
        Ok(Arc::new(local::OllamaChatProvider::new(
            endpoint,
            model,
            std::time::Duration::from_millis(timeout_ms),
        )?))
    } else {
        let model = config
            .memory_tree
            .cloud_llm_model
            .clone()
            .unwrap_or_else(|| DEFAULT_CLOUD_LLM_MODEL.to_string());
        let openhuman_dir = config.config_path.parent().map(std::path::PathBuf::from);
        log::debug!(
            "[memory::chat] building Cloud provider model={} openhuman_dir={:?}",
            model,
            openhuman_dir
        );
        Ok(Arc::new(cloud::CloudChatProvider::new(
            config.api_url.clone(),
            model,
            openhuman_dir,
            config.secrets.encrypt,
        )))
    }
}


/// In-memory chat provider for unit tests. Returns a canned response
/// regardless of the prompt and counts invocations so tests can assert
/// they were exercised. Lifted out of `mod tests` so any test module in
/// the crate can reach it via `chat::StaticChatProvider`.
#[cfg(test)]
pub struct StaticChatProvider {
    pub response: String,
    pub calls: std::sync::atomic::AtomicUsize,
}

#[cfg(test)]
impl StaticChatProvider {
    pub fn new(response: impl Into<String>) -> Self {
        Self {
            response: response.into(),
            calls: std::sync::atomic::AtomicUsize::new(0),
        }
    }
}

#[cfg(test)]
#[async_trait]
impl ChatProvider for StaticChatProvider {
    fn name(&self) -> &str {
        "test:static"
    }
    async fn chat_for_json(&self, _prompt: &ChatPrompt) -> Result<String> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(self.response.clone())
    }
}

/// Test-only chat-provider override. Tests that need a deterministic
/// summariser/extractor wrap their body in
/// [`test_override::with_provider`] — the inner future sees
/// `build_chat_provider` return the injected `Arc<dyn ChatProvider>`
/// regardless of `Config`.
#[cfg(test)]
pub mod test_override {
    use super::ChatProvider;
    use std::sync::Arc;

    tokio::task_local! {
        static OVERRIDE: Arc<dyn ChatProvider>;
    }

    /// Returns the override visible in the current task, if any.
    pub fn current() -> Option<Arc<dyn ChatProvider>> {
        OVERRIDE.try_with(Arc::clone).ok()
    }

    /// Run `fut` with `provider` installed as the override for any
    /// `build_chat_provider` call made inside it (or any child task
    /// spawned within it that inherits the task-local).
    pub async fn with_provider<F, T>(provider: Arc<dyn ChatProvider>, fut: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        OVERRIDE.scope(provider, fut).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_provider_returns_cloud_when_default() {
        let cfg = Config::default();
        // Default is LlmBackend::Cloud — provider construction must succeed
        // without a configured local Ollama endpoint.
        let provider = build_chat_provider(&cfg).unwrap();
        assert!(provider.name().contains("cloud"));
    }

    #[test]
    fn build_provider_returns_local_when_configured() {
        // After #1710 the local-vs-cloud decision is driven by
        // `memory_provider` (via `Config::workload_uses_local("memory")`),
        // not the legacy `memory_tree.llm_backend` flag — so the test
        // needs to set the new workload field. Endpoint + model on
        // `memory_tree` are still consumed for endpoint/model resolution
        // inside the local branch.
        let mut cfg = Config::default();
        cfg.memory_provider = Some("ollama:qwen2.5:0.5b".into());
        cfg.memory_tree.llm_extractor_endpoint = Some("http://localhost:11434".into());
        cfg.memory_tree.llm_extractor_model = Some("qwen2.5:0.5b".into());
        let provider = build_chat_provider(&cfg).unwrap();
        assert!(provider.name().contains("ollama") || provider.name().contains("local"));
    }

    #[tokio::test]
    async fn static_chat_provider_returns_response_and_counts() {
        let p = StaticChatProvider::new("hello");
        let prompt = ChatPrompt {
            system: "sys".into(),
            user: "u".into(),
            temperature: 0.0,
            kind: "test",
        };
        assert_eq!(p.chat_for_json(&prompt).await.unwrap(), "hello");
        assert_eq!(p.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
}

//! Embedding providers for the OpenHuman memory system.
//!
//! Converts text into numerical vectors for semantic search. Providers:
//!
//! - **Managed** (default): Routes through the OpenHuman backend's
//!   `POST /openai/v1/embeddings` (Voyage-backed). The recommended path —
//!   works on a fresh install without requiring a local Ollama daemon.
//! - **Voyage**: Direct Voyage AI API with the user's own key.
//! - **OpenAI**: Cloud-based embeddings via the OpenAI API.
//! - **Cohere**: Cohere embed API with the user's own key.
//! - **Ollama**: Local Ollama server. Opt-in for offline-only setups.
//! - **Custom**: Any OpenAI-compatible endpoint.
//! - **Noop**: A fallback provider for keyword-only search.

pub mod catalog;
#[path = "cloud_adapter.rs"]
pub mod cloud;
mod factory;
pub mod noop;
mod provider_trait;
pub mod rate_limit {
    pub use tinyagents::harness::embeddings::{
        rate_limit as embedding_rate_limit, set_rate_limit as set_embedding_rate_limit,
        DEFAULT_REQUESTS_PER_MINUTE as DEFAULT_EMBEDDING_RATE_LIMIT_PER_MIN,
    };

    pub async fn acquire_embedding_slot(base_url: &str) {
        tinyagents::harness::embeddings::acquire(base_url).await;
    }
}
pub mod retry_after {
    pub use tinyagents::harness::embeddings::{
        backoff_ms_for_attempt, parse_retry_after_ms, BASE_BACKOFF_MS, MAX_BACKOFF_MS,
        MAX_RETRIES as MAX_429_RETRIES,
    };
}
mod rpc;
mod schemas;

pub use catalog::non_embedding_model_reason;
pub use cloud::{
    OpenHumanCloudEmbedding, DEFAULT_CLOUD_EMBEDDING_DIMENSIONS, DEFAULT_CLOUD_EMBEDDING_MODEL,
};
pub use factory::{
    create_embedding_provider, create_embedding_provider_with_config,
    create_embedding_provider_with_credentials, default_embedding_provider,
    default_embedding_provider_with_config, default_local_embedding_provider,
};
// `pub(crate)` helper — reused by the memory-tree OpenAI-compat adapter to gate
// configs whose dimension the fixed-1024 tree can't store (#4056). Not part of
// the public surface, so it can't ride the `pub use` above (E0364).
// Its sole caller is `memory::host_impls`, which is unconditional again.
pub(crate) use factory::{model_supports_dimensions, MODELS_SUPPORTING_DIMENSIONS};
// #002 FR-015: the memory-tree OpenAI-compat embedder reuses the same key
// resolution the embeddings RPC uses, so there is one source of truth.
pub use noop::NoopEmbedding;
pub use provider_trait::{
    format_embedding_signature, EmbeddingProvider, TinyAgentsEmbeddingProvider,
};
pub use rpc::provider_from_config;
pub(crate) use rpc::resolve_api_key;
pub use schemas::{
    all_controller_schemas as all_embeddings_controller_schemas,
    all_registered_controllers as all_embeddings_registered_controllers,
};
pub use tinyagents::harness::embeddings::{
    DEFAULT_OLLAMA_DIMENSIONS, DEFAULT_OLLAMA_MODEL, DEFAULT_OLLAMA_URL,
};

/// The **intended** embedding selection — `(provider, model, dimensions)`.
///
/// # Why this is the host's and not the engine's
///
/// Which embedder the operator meant is selection policy over the host's own
/// config — the same class of decision as the preference lanes and the event
/// heuristics before it. The engine kept an identical helper for its internal
/// pipelines; this host used to reach through the crate for it, which was an
/// engine link taken on for a ten-line precedence rule (#5560). Ported
/// verbatim: a configured local model wins over the `[memory]` section, and a
/// blank local value falls back to the Ollama default rather than shipping
/// whitespace to a daemon that will 404 it.
///
/// Note: this is the *intended* setting. It does not check whether the Ollama
/// daemon is actually running.
pub fn effective_embedding_settings(
    memory: &crate::openhuman::config::schema::MemoryConfig,
    local_embedding_model: Option<&str>,
) -> (String, String, usize) {
    if let Some(raw) = local_embedding_model {
        // Trim once and reuse — the emptiness check and the final model
        // string must agree, otherwise a value like "  bge-m3  " would pass
        // through to Ollama with surrounding whitespace and 404.
        let trimmed = raw.trim();
        let model = if trimmed.is_empty() {
            DEFAULT_OLLAMA_MODEL.to_string()
        } else {
            trimmed.to_string()
        };
        return ("ollama".to_string(), model, DEFAULT_OLLAMA_DIMENSIONS);
    }
    (
        memory.embedding_provider.clone(),
        memory.embedding_model.clone(),
        memory.embedding_dimensions,
    )
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;

//! The persisted [`Config`] document, its model-tier constants, and the
//! resolvers layered on top of it.
//!
//! Load/save and env overrides extend `Config` in `load/`.

mod config;
mod defaults;
mod model_ids;
mod output_language;
mod resolvers;

pub use config::{Config, CustomEmbeddingsConfig, ModelRegistryEntry};
pub use model_ids::{
    DEFAULT_MEMORY_SYNC_INTERVAL_SECS, DEFAULT_MODEL, MEMORY_SYNC_INTERVAL_PRESETS_SECS,
    MODEL_AGENTIC_V1, MODEL_BURST_V1, MODEL_CHAT_V1, MODEL_CODING_V1, MODEL_REASONING_QUICK_V1,
    MODEL_REASONING_V1, MODEL_SUMMARIZATION_V1, MODEL_VISION_V1,
};
pub use output_language::{normalize_output_language, output_language_directive};

#[cfg(test)]
use crate::config::schema::{CapabilityProviderTrustState, TeamModelConfig};

#[cfg(test)]
#[path = "types_model_pin_tests.rs"]
mod model_pin_tests;

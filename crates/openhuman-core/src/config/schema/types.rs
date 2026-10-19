//! The persisted [`Config`] document, its model-tier constants, and the
//! resolvers layered on top of it.
//!
//! Load/save and env overrides extend `Config` in `load/`.

mod config;
mod config_clone;
mod defaults;
mod model_ids;
mod output_language;
mod resolvers;
mod time_zone;

pub(crate) use config::ConfigDocumentSnapshot;
pub use config::{Config, ConfigRuntimeState, CustomEmbeddingsConfig, ModelRegistryEntry};
pub use model_ids::{
    is_legacy_tier_model, legacy_tier_role, DEFAULT_MODEL, LEGACY_TIER_MODELS,
    MANAGED_MULTIMODAL_MODELS, MODEL_IMAGE_GENERATION_AGENT, MODEL_MANAGED_DEFAULT,
    MODEL_MEDIA_UNDERSTANDING, MODEL_VIDEO_GENERATION_AGENT, WORKLOAD_ROLES,
};
pub use output_language::{normalize_output_language, output_language_directive};
pub use time_zone::{device_time_zone, normalize_time_zone};

#[cfg(test)]
use crate::config::schema::{CapabilityProviderTrustState, TeamModelConfig};

#[cfg(test)]
#[path = "types_model_pin_tests.rs"]
mod model_pin_tests;

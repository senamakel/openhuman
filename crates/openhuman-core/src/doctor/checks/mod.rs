//! Static configuration checks for inference and embedding routes.

mod embeddings;
mod inference;

use std::sync::Arc;

use super::DoctorCheck;

/// Built-in static checks owned by the inference configuration leaves.
pub(super) fn builtins() -> Vec<Arc<dyn DoctorCheck>> {
    vec![
        Arc::new(inference::ProviderRef),
        Arc::new(inference::Construct),
        Arc::new(inference::Temperature),
        Arc::new(embeddings::EmbeddingConfigCheck),
    ]
}

//! Built-in doctor checks for configuration, inference, and embeddings.

mod config_file;
mod config_provenance;
mod embeddings;
mod inference;

use std::sync::Arc;

use super::contract::DoctorCheck;

pub(super) fn builtins() -> Vec<Arc<dyn DoctorCheck>> {
    let mut checks = config_file::checks();
    checks.push(Arc::new(config_provenance::EnvironmentOverrideCheck));
    checks.push(Arc::new(config_provenance::ConfigSourceCheck));
    checks.push(Arc::new(inference::ProviderRef));
    checks.push(Arc::new(inference::Construct));
    checks.push(Arc::new(inference::Temperature));
    checks.push(Arc::new(embeddings::EmbeddingConfigCheck));
    checks
}
}

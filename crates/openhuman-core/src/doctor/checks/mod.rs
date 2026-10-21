//! Built-in doctor checks for config, inference, embeddings, semantics, and dynamic services.

mod config_file;
mod config_provenance;
mod config_semantics;
mod embeddings;
mod ids;
mod inference;
mod inference_live;
mod mcp;
mod modules;
mod registry;
mod sandbox;
mod skills;
pub(crate) mod inference_catalog;

use std::sync::Arc;

use super::contract::DoctorCheck;

pub(super) use ids::canonical_segment;
pub(super) use registry::for_context;

pub(super) fn builtins() -> Vec<Arc<dyn DoctorCheck>> {
    let mut checks = config_file::checks();
    checks.push(Arc::new(config_provenance::EnvironmentOverrideCheck));
    checks.push(Arc::new(config_provenance::ConfigSourceCheck));
    checks.push(Arc::new(inference::ProviderRef));
    checks.push(Arc::new(inference::Construct));
    checks.push(Arc::new(inference::Temperature));
    checks.push(Arc::new(embeddings::EmbeddingConfigCheck));
    checks.extend(config_semantics::checks());
    checks
}

#[cfg(test)]
#[path = "config_semantics_tests.rs"]
mod config_semantics_tests;

#[cfg(test)]
#[path = "inference_live_tests.rs"]
mod inference_live_tests;

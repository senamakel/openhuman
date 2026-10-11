//! Composition seam for host-supplied cloud voice transport.
//!
//! Core selects voice behavior without linking a backend implementation. The
//! host installs a factory that binds current configuration to the minimal
//! TinyVoice provider contract; its backend layer owns credential and HTTP work.

use std::sync::{Arc, OnceLock, RwLock};

use crate::config::Config;
use tinyvoice_bus::provider::CloudVoiceProvider;

/// Builds a cloud provider under the current call's configuration.
pub trait CloudVoiceProviderFactory: Send + Sync {
    /// Resolve a provider for this call.
    ///
    /// # Errors
    /// Returns the host's configuration or backend availability error.
    fn for_config(&self, config: &Config) -> Result<Arc<dyn CloudVoiceProvider>, String>;
}

static FACTORY: OnceLock<RwLock<Option<Arc<dyn CloudVoiceProviderFactory>>>> = OnceLock::new();

fn slot() -> &'static RwLock<Option<Arc<dyn CloudVoiceProviderFactory>>> {
    FACTORY.get_or_init(|| RwLock::new(None))
}

/// Install the host backend factory, returning the previous composition.
pub fn install(
    factory: Arc<dyn CloudVoiceProviderFactory>,
) -> Option<Arc<dyn CloudVoiceProviderFactory>> {
    slot()
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .replace(factory)
}

/// Resolve cloud transport without linking a backend implementation.
///
/// # Errors
/// Returns `BACKEND_UNAVAILABLE` when no host factory is installed, otherwise
/// the factory's configuration error.
pub fn for_config(config: &Config) -> Result<Arc<dyn CloudVoiceProvider>, String> {
    let factory = slot()
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .ok_or_else(|| "BACKEND_UNAVAILABLE: cloud voice provider is not installed".to_string())?;
    factory.for_config(config)
}

#[cfg(test)]
pub(crate) fn restore(factory: Option<Arc<dyn CloudVoiceProviderFactory>>) {
    *slot()
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = factory;
}

/// Project stable provider categories into the product's existing error contract.
pub fn product_error(error: tinyvoice_bus::Error) -> String {
    use tinyvoice_bus::Error;
    match error {
        Error::SessionExpired { message } => format!("SESSION_EXPIRED: {message}"),
        Error::ApiKeyRejected { message } => format!("API_KEY_REJECTED: {message}"),
        Error::Unavailable { message } => format!("BACKEND_UNAVAILABLE: {message}"),
        other => other.to_string(),
    }
}

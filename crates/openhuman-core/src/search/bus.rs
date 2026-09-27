//! Event subscriber that keeps the TinySearch module's managed routes in step
//! with the credential this core holds.
//!
//! The module receives the backend credential privately at (re)initialization.
//! Without this subscriber a login would only reach it on the next search
//! call, and a logout would leave the old credential in the module until then.

use std::sync::Arc;

use async_trait::async_trait;
use tinybus::EventHandler;

use crate::core::events::DomainEvent;

/// Refreshes a loaded TinySearch module on [`DomainEvent::CredentialChanged`].
#[derive(Debug, Default)]
pub struct CredentialRefreshSubscriber;

#[async_trait]
impl EventHandler<DomainEvent> for CredentialRefreshSubscriber {
    fn name(&self) -> &str {
        "search::credential_refresh"
    }

    fn domains(&self) -> Option<&[&str]> {
        Some(&["auth"])
    }

    async fn handle(&self, event: &DomainEvent) {
        let DomainEvent::CredentialChanged { kind } = event else {
            return;
        };
        let config = match crate::config::rpc::load_config_with_timeout().await {
            Ok(config) => config,
            Err(error) => {
                tracing::warn!(kind = %kind, %error, "[search][bus] config unavailable; skipping refresh");
                return;
            }
        };
        match crate::modules::search::refresh_loaded(&config).await {
            Ok(()) => {
                tracing::debug!(kind = %kind, "[search][bus] module refreshed after credential change")
            }
            Err(error) => {
                tracing::warn!(kind = %kind, %error, "[search][bus] module refresh failed")
            }
        }
    }
}

/// Register the subscriber on the process bus. Called once at startup.
pub fn register_credential_refresh_subscriber() {
    match crate::core::bus::BUS.subscribe(Arc::new(CredentialRefreshSubscriber)) {
        Some(handle) => std::mem::forget(handle),
        None => tracing::warn!(
            "[search][bus] failed to register credential refresh — bus not initialized"
        ),
    }
}

#[cfg(test)]
#[path = "bus_tests.rs"]
mod tests;

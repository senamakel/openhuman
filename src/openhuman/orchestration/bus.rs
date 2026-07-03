//! Event-bus subscriber that drives orchestration ingest off inbound tiny.place
//! DM stream messages.

use std::sync::{Arc, OnceLock};

use async_trait::async_trait;

use crate::core::event_bus::{subscribe_global, DomainEvent, EventHandler, SubscriptionHandle};

static INGEST_HANDLE: OnceLock<SubscriptionHandle> = OnceLock::new();
static WAKE_HANDLE: OnceLock<SubscriptionHandle> = OnceLock::new();

/// Register the orchestration ingest subscriber on the global event bus.
pub fn register_orchestration_ingest_subscriber() {
    if INGEST_HANDLE.get().is_some() {
        return;
    }
    match subscribe_global(Arc::new(OrchestrationIngestSubscriber)) {
        Some(handle) => {
            let _ = INGEST_HANDLE.set(handle);
        }
        None => {
            log::warn!(
                "[orchestration] failed to register ingest subscriber — bus not initialized"
            );
        }
    }
}

pub struct OrchestrationIngestSubscriber;

#[async_trait]
impl EventHandler for OrchestrationIngestSubscriber {
    fn name(&self) -> &str {
        "orchestration::ingest"
    }

    fn domains(&self) -> Option<&[&str]> {
        Some(&["tinyplace"])
    }

    async fn handle(&self, event: &DomainEvent) {
        let DomainEvent::TinyPlaceStreamMessage {
            stream_id,
            kind,
            message,
        } = event
        else {
            return;
        };
        let config = match crate::openhuman::config::Config::load_or_init().await {
            Ok(config) => config,
            Err(e) => {
                log::warn!("[orchestration] ingest.config_load_failed: {e}");
                return;
            }
        };
        super::ingest::ingest_stream_message(&config, kind, stream_id, message).await;
    }
}

/// Register the orchestration **wake** subscriber: on each persisted session DM
/// (`OrchestrationSessionMessage`, published by ingest), schedule a debounced
/// wake-graph run for that session (stage 4). Kept separate from the ingest
/// subscriber so the transport path stays independent of the graph path.
pub fn register_orchestration_wake_subscriber() {
    if WAKE_HANDLE.get().is_some() {
        return;
    }
    match subscribe_global(Arc::new(OrchestrationWakeSubscriber)) {
        Some(handle) => {
            let _ = WAKE_HANDLE.set(handle);
        }
        None => {
            log::warn!("[orchestration] failed to register wake subscriber — bus not initialized");
        }
    }
}

pub struct OrchestrationWakeSubscriber;

#[async_trait]
impl EventHandler for OrchestrationWakeSubscriber {
    fn name(&self) -> &str {
        "orchestration::wake"
    }

    fn domains(&self) -> Option<&[&str]> {
        Some(&["agent"])
    }

    async fn handle(&self, event: &DomainEvent) {
        let DomainEvent::OrchestrationSessionMessage {
            agent_id,
            session_id,
            chat_kind,
        } = event
        else {
            return;
        };
        super::ops::schedule_wake(agent_id.clone(), session_id.clone(), chat_kind.clone()).await;
    }
}

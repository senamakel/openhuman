//! Web-surface delivery for proactive messages.
//!
//! This subscriber belongs to `web_chat`, rather than the optional external
//! channels runtime, because web proactive messages are available even when
//! the `channels` feature is disabled.

use crate::core::events::DomainEvent;
use crate::core::runtime::DispatchCtx;
use crate::web_chat::{publish_web_channel_event_in, WebChannelEvent};
use async_trait::async_trait;
use std::sync::Arc;
use tinybus::EventHandler;

/// Register the web destination for proactive messages once the global bus is
/// ready. A pre-bootstrap attempt is deferred without consuming the `Once`.
pub(crate) fn register_proactive_subscriber() {
    if crate::core::bus::BUS.get().is_none() {
        tracing::warn!("[proactive] deferred web subscriber registration — bus not initialized");
        return;
    }

    static REGISTERED: std::sync::Once = std::sync::Once::new();
    REGISTERED.call_once(|| {
        if let Some(handle) = crate::core::bus::BUS.subscribe(Arc::new(WebProactiveSubscriber)) {
            std::mem::forget(handle);
            tracing::debug!("[proactive] web subscriber registered");
        } else {
            tracing::warn!("[proactive] failed to register web subscriber — bus not initialized");
        }
    });
}

pub(crate) struct WebProactiveSubscriber;

#[async_trait]
impl EventHandler<DomainEvent> for WebProactiveSubscriber {
    fn name(&self) -> &str {
        "web_chat::proactive"
    }

    fn domains(&self) -> Option<&[&str]> {
        Some(&["cron"])
    }

    async fn handle(&self, event: &DomainEvent) {
        let DomainEvent::ProactiveMessageRequested {
            source,
            message,
            job_name,
            tenant,
        } = event
        else {
            return;
        };

        let Some(ctx) = DispatchCtx::for_event(tenant.as_ref()) else {
            tracing::warn!(source = %source, "[proactive] skipping web delivery without a live tenant context");
            return;
        };

        let thread_id = format!("proactive:{}", job_name.as_deref().unwrap_or("system"));
        publish_web_channel_event_in(
            &ctx,
            WebChannelEvent {
                event: "proactive_message".to_string(),
                client_id: "system".to_string(),
                thread_id,
                request_id: uuid::Uuid::new_v4().to_string(),
                full_response: Some(message.clone()),
                message: None,
                error_type: None,
                error_source: None,
                error_retryable: None,
                error_retry_after_ms: None,
                error_provider: None,
                error_fallback_available: None,
                copy_key: None,
                copy_params: None,
                tool_name: None,
                skill_id: None,
                args: None,
                output: None,
                success: Some(true),
                round: None,
                segment_index: None,
                segment_total: None,
                delta: None,
                delta_kind: None,
                tool_call_id: None,
                failure: None,
                citations: None,
                subagent: None,
                tool_display_label: None,
                tool_display_detail: None,
                elapsed_ms: None,
                structured: None,
                usage: None,
                // This event is outside the seq-stamping progress bridge.
                seq: None,
                ..Default::default()
            },
        );

        tracing::debug!(
            source = %source,
            thread_id = %format!("proactive:{}", job_name.as_deref().unwrap_or("system")),
            message_len = message.len(),
            "[proactive] delivered to web surface"
        );
    }
}

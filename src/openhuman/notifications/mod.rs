//! Core-side notification bridge.
//!
//! Subscribes to selected [`DomainEvent`](crate::core::event_bus::DomainEvent)
//! variants (cron completions, webhook processed, sub-agent completions) and
//! republishes them as notification payloads on a broadcast channel consumed
//! by the Socket.IO bridge in `core::socketio::spawn_web_channel_bridge`. The
//! frontend listens on the `core_notification` / `core:notification` event
//! and funnels the payload into the in-app notification center.
//!
//! This module does not own any UI — it's purely a fan-out from the internal
//! event bus to the wire-level socket event. The shape is kept small and
//! frontend-agnostic so future event types can be added without changing
//! either side of the contract.

pub mod bus;
pub mod types;

pub use bus::{
    publish_core_notification, register_notification_bridge_subscriber,
    subscribe_core_notifications, NotificationBridgeSubscriber,
};
pub use types::{CoreNotificationCategory, CoreNotificationEvent};

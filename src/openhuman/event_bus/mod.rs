//! Cross-module event bus for decoupled pub/sub communication.
//!
//! Provides a typed, async event bus built on `tokio::sync::broadcast`.
//! Domain modules publish [`DomainEvent`] variants; subscribers react
//! without direct module dependencies.
//!
//! # Usage
//!
//! ```ignore
//! use crate::openhuman::event_bus::{EventBus, DomainEvent};
//!
//! let bus = EventBus::with_default_capacity();
//!
//! // Subscribe with a closure
//! let _handle = bus.on("my-handler", |event| Box::pin(async move {
//!     tracing::info!("got event: {:?}", event);
//! }));
//!
//! // Publish an event
//! bus.publish(DomainEvent::SystemStartup { component: "example".into() });
//! ```

mod bus;
mod events;
mod subscriber;
mod tracing;

pub use bus::{global, init_global, publish_global, EventBus};
pub use events::DomainEvent;
pub use subscriber::{EventHandler, SubscriptionHandle};
pub use tracing::TracingSubscriber;

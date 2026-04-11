//! Cross-module event bus for decoupled events and typed controller requests.
//!
//! The event bus is a **singleton** — one instance for the entire application.
//! Call [`init_global`] once at startup, then use [`publish_global`],
//! [`subscribe_global`], and [`request_global`] from any module.
//!
//! # Usage
//!
//! ```ignore
//! use crate::openhuman::event_bus::{publish_global, subscribe_global, DomainEvent};
//!
//! // Publish from anywhere
//! publish_global(DomainEvent::SystemStartup { component: "example".into() });
//!
//! // Subscribe from anywhere
//! let _handle = subscribe_global(Arc::new(MyHandler));
//! ```

mod bus;
mod events;
mod request;
mod subscriber;
mod tracing;

pub use bus::{
    global, init_global, publish_global, request_controller_global, request_global,
    subscribe_global, EventBus, DEFAULT_CAPACITY,
};
pub use events::DomainEvent;
pub use request::{ControllerCall, ControllerResponse, EventBusRequestError};
pub use subscriber::{EventHandler, SubscriptionHandle};
pub use tracing::TracingSubscriber;

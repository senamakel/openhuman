//! The [`SpanCollector`] state machine: data shape and construction
//! ([`state`]), turn/parent bootstrap and content capture ([`span_lifecycle`]),
//! generation-span folding ([`model_call`]), and the public event-fold +
//! finish API ([`events`]).

mod events;
mod model_call;
mod span_lifecycle;
mod state;

pub use state::SpanCollector;

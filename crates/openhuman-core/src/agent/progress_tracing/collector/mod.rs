//! The [`SpanCollector`] state machine: data shape and construction
//! ([`state`]), turn/parent bootstrap and content capture ([`span_lifecycle`]),
//! generation-span folding ([`model_call`], attribute shaping in
//! [`generation`]), the public event fold ([`events`]), and sealing the tree
//! with the turn's outcome ([`finish`]).

mod events;
mod finish;
mod generation;
mod model_call;
mod span_lifecycle;
mod state;

pub use state::{SpanCollector, TurnOutcome};

//! Bridge the `tinyagents` harness event stream onto openhuman's
//! [`AgentProgress`] + cost tracker (issue #4249).
//!
//! tinyagents emits a typed [`AgentEvent`] stream (model started/delta/completed,
//! tool started/completed, usage) through an [`EventSink`] that callers attach
//! to a [`RunContext`]. This listener translates those into the same
//! `AgentProgress` events the legacy `run_turn_engine` produced — restoring the
//! live tool timeline, streaming text, and the cost/token footer on the
//! tinyagents path — and feeds per-call usage into the global cost tracker.

mod cap_pauser;
mod event_bridge;
mod event_projection;
mod graph_tracing;

#[cfg(test)]
use std::sync::Arc;

#[cfg(test)]
use tinyagents_harness::events::AgentEvent;
#[cfg(test)]
use tinyinference::usage::Usage;

#[cfg(test)]
use crate::agent::progress::AgentProgress;

pub(crate) use cap_pauser::{
    CapPauser, IterationCursor, ProviderUsageCarry, SubagentScope, ToolFailureMap, ToolNameMap,
};
pub(crate) use event_bridge::OpenhumanEventBridge;
pub(crate) use event_projection::surface_cache_layout_events;
pub(crate) use graph_tracing::GraphTracingSink;

#[cfg(test)]
#[path = "observability_tests.rs"]
mod tests;

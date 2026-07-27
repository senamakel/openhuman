//! The Medulla event vocabulary: every library cycle event plus the
//! host-sourced rows (cycle framing, conversation turns, agent/session status,
//! effects).
//!
//! [`TuiEvent`] deserializes any JSON `{kind, ...}` shape and keeps unrecognized
//! kinds in [`TuiEvent::Unknown`], so a newer backend never drops rows on an
//! older host.
//!
//! # This module is an ungated type carve-out
//!
//! Unlike the rest of `medulla`, these types stay compiled whether or not the
//! `medulla` feature is on. They are inert serde/std definitions with no
//! coupling to their gated siblings, and `src/embed/` names them in public
//! signatures — gating them would take the facade down with them.
//!
//! This follows the rule `AGENTS.md` draws from the `skills` / `mcp_registry`
//! gates: put a domain's inert types in a dependency-free submodule and leave it
//! ungated; gate only behaviour. Both builds then share one definition, so
//! fields cannot drift between them.
//!
//! Split by responsibility: [`types`] is the data model, `serde_impl` the custom
//! compact-JSON codec, `derive` the read-only derivations.

mod derive;
mod serde_impl;
mod types;

#[cfg(test)]
mod tests;

pub use derive::{chat_transcript, describe_event, last_assistant_message};
pub use types::{EventEnvelope, NodeTrace, TaskDigest, ToolCall, TuiEvent, Usage};

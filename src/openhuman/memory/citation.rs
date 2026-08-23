//! Memory-recall provenance, as an inert type.
//!
//! # Why this is its own ungated module
//!
//! [`MemoryCitation`] is a plain serde struct and [`CROSS_CHAT_HEADER`] a
//! `&'static str`. Neither names a driver, a store, or the engine crate, and
//! neither can fail — but both appear in **always-compiled** signatures: the
//! agent's `last_turn_citations` field, its `pending_citations` join handle,
//! and the web-chat delivery path that renders source chips.
//!
//! So they follow `skills::types`' rule rather than `voice`'s: put a domain's
//! inert types in a dependency-free submodule and leave it **ungated**; stub
//! only the behaviour. Both builds then share ONE definition, and there is no
//! twin free to drift. The producer — `collect_recall_citations`, which is a
//! full recall — stays behind the gate in
//! [`memory_loader`](super::agent::memory_loader), which re-exports these two
//! so every historical path keeps resolving.
//!
//! With the family compiled out the vector is simply always empty: no recall
//! ran, so nothing cited anything.

use serde::{Deserialize, Serialize};

/// Prefix stamped on recalled context drawn from a *different* conversation.
///
/// The disclaimer is load-bearing rather than cosmetic: cross-chat context is
/// historical, and a capability the assistant had in the earlier conversation
/// may since have been removed.
pub const CROSS_CHAT_HEADER: &str =
    "[Cross-chat context — historical; capabilities may have changed since]\n";

/// Lightweight citation object derived from recalled memory entries.
///
/// Attached to agent responses so the UI can show provenance for
/// memory-informed answers without exposing full raw memory.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemoryCitation {
    pub id: String,
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    pub timestamp: String,
    pub snippet: String,
}

//! Subconscious trigger ingestion + gate front-end.
//!
//! This domain turns heterogeneous bus events (cron tick, user message,
//! Composio webhook, sub-agent conclusion) into a unified [`Trigger`],
//! collapses duplicates / rate-limits storms ([`TriggerRegistry`]), runs a
//! cheap LLM gate to decide *promote vs drop*, and enqueues promotions onto
//! the background orchestrator's [`OrchestratorQueue`].
//!
//! It is the *ingestion front-end* for the long-lived subconscious
//! orchestrator session; the session/engine itself lives in the
//! `subconscious` domain.
//!
//! Module shape follows the canonical layout:
//! - [`types`]    — `Trigger`, `TriggerSource`, `GateDecision`, …
//! - [`queue`]    — bounded priority queue of promoted triggers
//! - [`registry`] — dedupe + rate-limit admission front-end
//! - gate / bus / ops / schemas land in later slices.

pub mod gate;
pub mod normalize;
pub mod queue;
pub mod registry;
pub mod types;

pub use gate::{GatePass, PromotionBudget};
pub use normalize::{normalize, SUBCONSCIOUS_SENDER_MARKER};
pub use queue::{EnqueueOutcome, OrchestratorQueue};
pub use registry::{AdmitOutcome, DedupeWindow, RateLimiter, TriggerRegistry};
pub use types::{
    DedupeKey, GateDecision, Trigger, TriggerPayload, TriggerPriority, TriggerSource,
};

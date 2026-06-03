//! Active-run queue model for mid-turn steering and message queuing.
//!
//! When a user sends a message while an agent turn is already running,
//! the [`QueueMode`] determines what happens:
//!
//! - **Interrupt** — abort the in-flight turn and start fresh (default,
//!   preserves backward compatibility).
//! - **Steer** — inject the message into the running turn's history at
//!   the next model boundary (after the current tool-call batch
//!   completes).
//! - **Followup** — queue the message for delivery as a new turn after
//!   the current one completes naturally.
//! - **Collect** — queue additional context that is injected alongside
//!   steers at the next model boundary.
//!
//! The engine drains steers and collects at the top of each iteration
//! loop in [`super::engine::run_turn_engine`], preserving the
//! invariant that tool-call / tool-result message pairs are never
//! broken. Followups are drained by the web channel after the turn
//! completes.

mod ops;
pub(crate) mod types;

pub(crate) use ops::RunQueue;
pub(crate) use types::{QueueCounts, QueueEntry, QueueMode};

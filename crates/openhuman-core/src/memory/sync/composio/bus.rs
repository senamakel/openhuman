//! Event bus subscribers for the Composio domain.
//!
//! The backend emits `composio:trigger` over Socket.IO when a webhook
//! arrives and is HMAC-verified (see
//! `src/controllers/agentIntegrations/composio/handleWebhook.ts` in the
//! backend repo). The socket transport layer parses that payload and
//! publishes [`DomainEvent::ComposioTriggerReceived`], and this
//! subscriber is what actually does something with it.
//!
//! ## What it does today
//!
//! - **Always**: logs the trigger at `debug` level for grep-friendly
//!   audit trails.
//! - **When enabled**: runs the trigger through
//!   [`crate::agent::triage::run_triage`] to produce a
//!   [`TriageDecision`] and then
//!   [`crate::agent::triage::apply_decision`] to act on it.
//!   The classifier runs on the shared built-in
//!   [`trigger_triage`][trigger_triage] agent and its decisions are
//!   published as `TriggerEvaluated` / `TriggerEscalated` events on
//!   the bus.
//!
//! [trigger_triage]: crate::agent::registry::agents
//!
//! ## Feature flag
//!
//! The triage path is gated on `OPENHUMAN_TRIGGER_TRIAGE_DISABLED` (set
//! to `1`/`true`/`yes` to disable). The pipeline is on by default; the
//! env var is an opt-out escape hatch.
//!
//! There are two long-lived subscribers, both registered at startup:
//!
//!   * [`ComposioTriggerSubscriber`] — handles
//!     [`DomainEvent::ComposioTriggerReceived`]. The backend HMAC-verifies
//!     a Composio webhook, parses it, and emits `composio:trigger` over
//!     Socket.IO; the socket transport publishes that as a domain event.
//!     The subscriber routes it through the triage pipeline.
//!
//!   * [`ComposioConnectionCreatedSubscriber`] — handles
//!     [`DomainEvent::ComposioConnectionCreated`]. Fired by `composio_authorize`
//!     once the OAuth handoff has produced a `connectUrl` + `connectionId`.
//!     We look up the provider and call `on_connection_created`, which
//!     by default fetches the user profile and runs the initial sync.
//!
//! Both subscribers do their work in a `tokio::spawn`-ed task so the
//! event bus dispatch loop is never blocked by a long-running provider
//! call (sync can take seconds).
//!
//! ## Module layout
//!
//! Split by responsibility: [`trigger_subscriber`] (`ComposioTriggerSubscriber`
//! and the triage-disabled gate), [`connection_created_subscriber`]
//! (`ComposioConnectionCreatedSubscriber`, the toolkit-registrable predicate,
//! and connection-readiness polling), [`config_changed_subscriber`]
//! (`ComposioConfigChangedSubscriber`), and [`registration`] (wiring all
//! three onto the global bus at startup).

mod config_changed_subscriber;
mod connection_created_subscriber;
mod registration;
mod trigger_subscriber;

pub use config_changed_subscriber::ComposioConfigChangedSubscriber;
pub use connection_created_subscriber::ComposioConnectionCreatedSubscriber;
pub use registration::register_composio_trigger_subscriber;
pub use trigger_subscriber::ComposioTriggerSubscriber;

// Test-only visibility: `tests` below is declared directly under `bus` (not
// under the submodule that owns each helper) and reaches these through
// `use super::*;`. Private `use` is enough — a descendant module can see
// everything visible in its ancestors.

#[cfg(test)]
#[path = "bus_tests.rs"]
mod tests;

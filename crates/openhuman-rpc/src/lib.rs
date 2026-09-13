//! Shared JSON-RPC / CLI wire vocabulary for OpenHuman.
//!
//! This crate is linked by `openhuman_core` (re-exported there as
//! `openhuman_core::rpc`), the Tauri shell (`crates/openhuman-app`), and the
//! TUI (`crates/openhuman-tui`). It is a separate crate so the producer of
//! RPC envelopes (the core) and their decoders (the shell's HTTP relay, the
//! TUI) compile one definition of the wire shape, and that definition depends
//! on nothing in the core. It must stay free of domain types and runtime
//! dependencies.
//!
//! - [`RpcOutcome`] and [`apply_log_envelope`] define handler results and the
//!   log envelope rule. Domain `ops.rs` operations return `RpcOutcome<T>`.
//! - [`unwrap_rpc`] unwraps the client-side `result`/`data` envelopes.
//! - [`StructuredRpcError`] and [`STRUCTURED_RPC_ERROR_SENTINEL`] are the
//!   typed error envelope decoded at the transport boundary.
//! - Behind the `http-client` feature (default-on here, but disabled by the
//!   root workspace dependency so each consumer opts in): [`post_json_rpc`],
//!   [`bearer_header`], [`redact_url_for_log`], and [`HttpRpcResponse`].

use serde::Serialize;
use serde_json::json;

#[cfg(feature = "http-client")]
mod client;
mod structured_error;

#[cfg(feature = "http-client")]
pub use client::{bearer_header, post_json_rpc, redact_url_for_log, HttpRpcResponse};
pub use structured_error::{StructuredRpcError, STRUCTURED_RPC_ERROR_SENTINEL};

/// Unwrap the optional log and API envelopes used by OpenHuman RPC handlers.
pub fn unwrap_rpc(mut value: &serde_json::Value) -> &serde_json::Value {
    loop {
        if let Some(next) = value.get("result").or_else(|| value.get("data")) {
            value = next;
        } else {
            return value;
        }
    }
}

/// Successful RPC handler result: serialized JSON value plus optional log lines.
///
/// This type represents the result of a domain-specific RPC call, including
/// any log messages generated during execution.
#[derive(Debug)]
pub struct RpcOutcome<T> {
    /// The actual data returned by the RPC call.
    pub value: T,
    /// A collection of log messages for auditing or debugging.
    pub logs: Vec<String>,
}

impl<T> RpcOutcome<T> {
    /// Creates a new `RpcOutcome` with a value and a list of logs.
    pub fn new(value: T, logs: Vec<String>) -> Self {
        Self { value, logs }
    }
}

impl<T: Serialize> RpcOutcome<T> {
    /// Creates a new `RpcOutcome` with a value and a single log message.
    pub fn single_log(value: T, log: impl Into<String>) -> Self {
        Self {
            value,
            logs: vec![log.into()],
        }
    }

    /// Converts the outcome into a CLI-compatible JSON value.
    ///
    /// The shape is decided by [`apply_log_envelope`], which is the single
    /// definition of the rule — see its docs for the rule itself and for why
    /// having one definition matters (#6080).
    ///
    /// # Errors
    ///
    /// Returns an error if serialization to JSON fails.
    pub fn into_cli_compatible_json(self) -> Result<serde_json::Value, String> {
        let RpcOutcome { value, logs } = self;
        let value = serde_json::to_value(value).map_err(|e| e.to_string())?;
        Ok(apply_log_envelope(value, logs))
    }
}

/// Apply the log envelope to an already-serialized handler value.
///
/// **This is the one definition of the rule.** Both controller return paths go
/// through it:
///
///  * the registry path — every `RpcOutcome::into_cli_compatible_json` call
///    (152 call sites across the domains), and
///  * the dynamic-dispatch path — `core::types::invocation_to_rpc_json`, used
///    by `core::dispatch` for internal / legacy methods.
///
/// # The rule, and the defect it currently encodes (#6080)
///
/// ```text
/// logs.is_empty()  ->  value                            (bare)
/// otherwise        ->  { "result": value, "logs": … }   (wrapped)
/// ```
///
/// So a controller's **wire shape is decided by its log vector, not by its
/// schema**. Two methods in one namespace can answer differently, and a handler
/// that later gains a log line silently changes its own response shape with no
/// schema change — which is #6080. That defect is deliberately **preserved
/// byte-for-byte here**: normalising it is a wire change across every
/// controller and needs a maintainer's ruling, not a quiet fix inside a
/// refactor.
///
/// What this function buys today is that the rule exists **once**. Before it,
/// the same six lines were written independently in `rpc::RpcOutcome` and in
/// `core::types::invocation_to_rpc_json`; a fix applied to one would have left
/// the other on the old behaviour, and nothing linked them. When the ruling
/// lands, this is the only body that has to change.
#[must_use]
pub fn apply_log_envelope(value: serde_json::Value, logs: Vec<String>) -> serde_json::Value {
    if logs.is_empty() {
        value
    } else {
        json!({ "result": value, "logs": logs })
    }
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;

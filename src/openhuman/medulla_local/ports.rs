//! The host-side port surface a supervised serve child drives via reverse RPC.
//!
//! serve issues `call` frames for the ports it needs (§5); the supervisor
//! demultiplexes them and dispatches to a [`HostPorts`] implementation, then
//! writes the `ret`. This trait is the clean seam between the transport
//! (`server.rs`) and the openhuman capability surface (`host_ports.rs`): the
//! supervisor and its tests are generic over `Arc<dyn HostPorts>`.
//!
//! For this DRAFT milestone only two ports are answered — `inference` and
//! `tools` — matching plan §3.2's "the substance of the flavor". Every other
//! port serve might request is refused as `port_unavailable`, handled centrally
//! in `server.rs` so an implementer cannot forget to reject one.

use async_trait::async_trait;
use serde_json::Value;

use super::protocol::ServeError;
use super::types::{error_codes, InferenceCall, InferenceResult, ToolSpec};

/// A failure answering a port callback. Serialized into a `ret` error
/// envelope (§8) with one of the reserved [`error_codes`].
#[derive(Debug, Clone)]
pub struct PortError {
    pub code: &'static str,
    pub message: String,
}

impl PortError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// The host does not offer this port (or the tool/method is not on the
    /// curated allowlist for this draft).
    pub fn port_unavailable(message: impl Into<String>) -> Self {
        Self::new(error_codes::PORT_UNAVAILABLE, message)
    }

    /// The port is offered but the specific method is not implemented.
    pub fn unsupported_method(message: impl Into<String>) -> Self {
        Self::new(error_codes::UNSUPPORTED_METHOD, message)
    }

    /// A host-side failure while servicing an otherwise-valid callback.
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(error_codes::INTERNAL, message)
    }

    pub fn to_serve_error(&self) -> ServeError {
        ServeError::new(self.code, self.message.clone())
    }
}

/// The subset of medulla-v1 ports this host answers.
///
/// The wire ToolResult (`{content, isError}`, §5.2) is returned as a raw
/// [`Value`] so the concrete adapter can pass a tool's result through
/// **unchanged** (the draft's contract) without a lossy re-typing.
#[async_trait]
pub trait HostPorts: Send + Sync {
    /// The host tool specs advertised in the `hello` handshake (§3), which serve
    /// binds into the serve-side module registry so the model can see and emit
    /// `tools.invoke` calls for them.
    ///
    /// This MUST correspond to the set [`Self::invoke_tool`] will actually
    /// answer — an advertised tool the port later refuses is a phantom the model
    /// wastes a turn on, and a tool the port answers but never advertises can
    /// never be invoked at all. Both derive from the same curated allowlist in
    /// the concrete adapter to keep them in lock-step.
    fn tool_specs(&self) -> Vec<ToolSpec>;

    /// `inference.invoke` (§5.1) — route the call's tier onto the host's
    /// per-role model routing and return an `InferenceResult`.
    async fn invoke_inference(&self, call: InferenceCall) -> Result<InferenceResult, PortError>;

    /// `tools.invoke` (§5.2) — execute a host tool by name against the curated
    /// read-only allowlist and return its wire `ToolResult` unchanged.
    async fn invoke_tool(&self, name: &str, args: Value) -> Result<Value, PortError>;
}

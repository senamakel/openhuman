//! The error a [`BackendTransport`](super::BackendTransport) returns.
//!
//! This is the core-owned image of the variants the classification code in
//! [`crate::backend::client`] and `integrations/client/errors.rs` matches on. It
//! mirrors the vendored SDK's `Error` one-to-one for those arms so a transport
//! built on the SDK maps mechanically, while the core itself never names the
//! SDK type.

use serde_json::Value;

/// Failure of a single backend round-trip, as seen by the core.
#[derive(Debug, thiserror::Error)]
pub enum BackendTransportError {
    /// No transport is installed in this process, so the core cannot reach the
    /// hosted backend at all. This is the normal state of a core built and
    /// run without `openhuman-tinyhumans`; callers surface it as a typed
    /// "backend unavailable" condition, never as a bug.
    #[error("no backend transport installed")]
    Unavailable,
    /// The base URL and path did not compose into a valid URL.
    #[error("invalid url: {0}")]
    Url(String),
    /// The HTTP client failed before a response arrived (DNS, TLS, connect,
    /// timeout, body read). Kept as the `reqwest` error so the source chain
    /// walk in the classifiers still sees nested causes.
    #[error("http client error: {0}")]
    Http(#[from] reqwest::Error),
    /// The backend answered with a non-2xx status. `body` is the parsed JSON
    /// body when it parsed, otherwise the raw text as a JSON string.
    #[error("http {status}: {body}")]
    Status { status: u16, body: Value },
    /// The response carried a `{success:false, ...}` envelope on a 2xx status.
    #[error("backend reported failure: {error}")]
    Envelope {
        error: String,
        error_code: Option<String>,
        details: Value,
    },
    /// A header value was not representable on the wire.
    #[error("invalid header value: {0}")]
    Header(String),
    /// The response body could not be decoded into the expected shape.
    #[error("response decoding failed: {0}")]
    Decode(String),
    /// The transport refuses this route by policy (the SDK's unexposed-route
    /// registry, for example). `(method, path)`.
    #[error("route is intentionally not exposed by the backend transport: {0} {1}")]
    RouteNotExposed(String, String),
    /// Anything else the transport implementation reports.
    #[error("{0}")]
    Other(String),
}

impl BackendTransportError {
    /// The HTTP status when this error is a [`Self::Status`].
    pub fn status(&self) -> Option<u16> {
        match self {
            Self::Status { status, .. } => Some(*status),
            _ => None,
        }
    }
}

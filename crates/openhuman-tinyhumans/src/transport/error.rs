//! `tinyhumans_sdk::Error` → core [`BackendTransportError`], one arm per
//! variant the core classifies on so `backend::client::finish_authed_json` and
//! the integrations client see exactly the shapes they saw when they called
//! the SDK directly.

use openhuman_core::backend::transport::BackendTransportError;
use tinyhumans_sdk::Error as SdkError;

/// Map the SDK's error onto the core-owned transport error.
pub fn map_sdk_error(error: SdkError) -> BackendTransportError {
    match error {
        SdkError::Url(e) => BackendTransportError::Url(e.to_string()),
        SdkError::Http(e) => BackendTransportError::Http(e),
        SdkError::Status { status, body } => BackendTransportError::Status { status, body },
        SdkError::Header(e) => BackendTransportError::Header(e.to_string()),
        SdkError::Decode(e) => BackendTransportError::Decode(e.to_string()),
        SdkError::RouteNotExposed(method, path) => {
            BackendTransportError::RouteNotExposed(method, path)
        }
        SdkError::Envelope {
            error,
            error_code,
            details,
        } => BackendTransportError::Envelope {
            error,
            error_code,
            details,
        },
        // Socket.IO variants only exist with the SDK's `socket` feature, which
        // this crate turns off; kept as a catch-all so a future SDK variant
        // maps to something rather than failing the build.
        #[allow(unreachable_patterns)]
        other => BackendTransportError::Other(other.to_string()),
    }
}

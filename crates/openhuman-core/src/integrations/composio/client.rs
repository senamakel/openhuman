//! Thin HTTP wrapper over the openhuman backend's
//! `/agent-integrations/composio/*` routes.
//!
//! All calls go through the shared
//! [`crate::integrations::IntegrationClient`] so they inherit
//! the same Bearer JWT auth, timeout, envelope parsing, and proxy behavior
//! as the other backend-proxied integrations.
//!
//! Logging uses the `[composio]` grep-prefix so all sidecar output for
//! this domain can be filtered in one shot.

mod connections;
mod direct;
mod execute;
mod factory;
mod triggers;

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;

pub use connections::ComposioClient;
pub(crate) use direct::{direct_authorize, direct_list_tools};
pub use direct::{direct_execute, direct_list_connections};
pub(crate) use factory::{build_composio_client, create_direct_composio_tool_for_api_key};
pub use factory::{create_composio_client, ComposioClientKind};

// Brought into this module's own namespace (private `use`, not `pub use`)
// so `client_tests.rs` — declared as a direct child module of `client`
// above — can still reach these via a plain `use super::*;`, exactly as
// when this was one un-split file. See each item's `pub(super)` in its
// owning submodule.
#[cfg(test)]
use super::types::ComposioExecuteResponse;
#[cfg(test)]
use execute::is_post_oauth_auth_readiness_error;
#[cfg(test)]
use std::sync::Arc;

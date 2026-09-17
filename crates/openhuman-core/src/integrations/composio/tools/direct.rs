// Composio Tool Provider — optional managed tool surface with 1000+ OAuth integrations.
//
// When enabled, OpenHuman can execute actions on Gmail, Notion, GitHub, Slack, etc.
// through Composio's API without storing raw OAuth tokens locally.
//
// This is opt-in. Users who prefer sovereign/local-only mode skip this entirely.
// The Composio API key is stored in the encrypted secret store.

#[cfg(test)]
#[path = "direct_tests.rs"]
mod tests;

mod connections;
mod construction;
mod discovery;
mod execution;
mod http_errors;
mod tool_impl;
mod types;

pub use connections::ComposioConnectedAccount;
pub use discovery::ComposioAction;
pub use types::ComposioTool;

// Test-only bridges: the flat `direct_tests.rs` module (kept as a rename-only
// group per the unsplit policy) still expects these internal helpers to be
// reachable unqualified via `use super::*`, mirroring the single-scope shape
// `include!` gave it before the split into responsibility-based submodules.
#[cfg(test)]
use crate::tools::traits::Tool;
#[cfg(test)]
use connections::ComposioAuthConfig;
#[cfg(test)]
use construction::normalize_entity_id;
#[cfg(test)]
use discovery::{
    map_v3_tools_to_actions, ComposioActionsResponse, ComposioToolkitRef, ComposioToolsResponse,
    ComposioV3Tool,
};
#[cfg(test)]
use http_errors::{extract_api_error_message, extract_redirect_url, sanitize_error_message};
#[cfg(test)]
use types::{ensure_https, is_loopback_http_url, COMPOSIO_API_BASE_V3};

//! `composio` compiled out: no Composio agent tools and no per-action tools.
//!
//! Signatures MUST match `registry.rs`; the trimmed build
//! (`cargo check -p openhuman --no-default-features`) is what catches drift.

use tinytools::Tool;

/// No Composio agent tools in this build, signed in or not.
pub fn all_composio_agent_tools(_config: &crate::config::Config) -> Vec<Box<dyn Tool>> {
    tracing::debug!("[composio] agent tools not registered — the `composio` feature is compiled out");
    Vec::new()
}

/// No per-action `TOOLKIT_ACTION` tool in this build.
pub fn deferred_action_tool(
    toolkit: &str,
    action: String,
    _description: String,
    _parameters: Option<serde_json::Value>,
) -> Option<Box<dyn Tool>> {
    tracing::debug!(
        toolkit,
        action = %action,
        "[composio] action tool not built — the `composio` feature is compiled out"
    );
    None
}

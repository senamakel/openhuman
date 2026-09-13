//! Browser automation tool with pluggable backends.
//!
//! By default this uses Vercel's `agent-browser` tool for automation.
//! Playwright is also supported as a local Node-backed backend. Optionally, a
//! Rust-native backend can be enabled at build time via `--features
//! browser-native` and selected through config.
//! Computer-use (OS-level) actions are supported via an optional sidecar endpoint.

#[path = "action_parser.rs"]
mod action_parser;
#[cfg(feature = "browser-native")]
#[path = "native_backend.rs"]
mod native_backend;
#[path = "security.rs"]
mod security;
#[path = "types.rs"]
mod types;

#[cfg(test)]
#[path = "browser_tests.rs"]
mod tests;

#[path = "browser_backend_resolution.rs"]
mod backend_resolution;
#[path = "browser_computer_use.rs"]
mod computer_use_actions;
#[path = "browser_construction.rs"]
mod construction;
#[path = "browser_execution.rs"]
mod execution;
#[path = "browser_tool_impl.rs"]
mod tool_impl;

use super::playwright_backend;
#[allow(unused_imports)]
pub(super) use action_parser::{
    backend_name, is_computer_use_only_action, is_supported_browser_action, parse_browser_action,
    unavailable_action_for_backend_error,
};
pub(super) use security::{
    allow_all_browser_domains, endpoint_reachable, extract_host, host_matches_allowlist,
    is_private_host, normalize_domains,
};
pub(super) use types::{
    AgentBrowserResponse, BrowserBackendKind, ComputerUseResponse, ResolvedBackend,
};
pub use types::{BrowserAction, ComputerUseConfig};

use crate::security::SecurityPolicy;
use crate::tools::traits::{Tool, ToolResult};
use anyhow::Context;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;
use tracing::debug;

/// Browser automation tool using pluggable backends.
pub struct BrowserTool {
    security: Arc<SecurityPolicy>,
    allowed_domains: Vec<String>,
    session_name: Option<String>,
    backend: String,
    native_headless: bool,
    native_webdriver_url: String,
    native_chrome_path: Option<String>,
    computer_use: ComputerUseConfig,
    playwright_state: tokio::sync::Mutex<playwright_backend::PlaywrightBrowserState>,
    #[cfg(feature = "browser-native")]
    native_state: tokio::sync::Mutex<native_backend::NativeBrowserState>,
}

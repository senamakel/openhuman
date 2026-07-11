//! Core library for the OpenHuman platform.
//!
//! This crate provides the central logic for the OpenHuman core binary, including:
//! - API and RPC handlers for external interactions.
//! - Core system services (CLI, configuration, monitoring).
//! - Domain-specific logic for the OpenHuman agent runtime.

pub mod api;
pub mod core;
pub mod openhuman;
pub mod rpc;

pub use openhuman::config::DaemonConfig;
pub use openhuman::memory_store::{MemoryClient, MemoryState};

/// Embeddable core composition API. Host the OpenHuman core in any process —
/// the Tauri shell, a CLI, a stdio MCP server, or a cloud/team server — via
/// [`CoreBuilder`] → [`CoreRuntime`]. See `docs/plans/pluggable-core/`.
pub use core::runtime::{CoreBuilder, CoreRuntime, ServiceSet, TokenSource};
pub use core::types::HostKind;

/// Runs the core logic based on the provided command-line arguments.
///
/// This is the primary entry point for the OpenHuman binary, delegating to the
/// CLI module for argument parsing and command dispatch.
///
/// # Arguments
///
/// * `args` - A slice of strings containing the command-line arguments.
///
/// # Errors
///
/// Returns an error if command execution fails.
pub fn run_core_from_args(args: &[String]) -> anyhow::Result<()> {
    core::cli::load_dotenv_for_cli()?;
    openhuman::service::apply_startup_restart_delay_from_env();
    openhuman::keyring::init_master_key();
    core::cli::run_from_cli_args(args)
}

//! Shared HTTP client for all integration tools.

mod construct;
mod download;
mod errors;
mod pricing;
mod requests;

pub use construct::IntegrationClient;
pub use pricing::{build_client, pricing_for_config};

pub(crate) use errors::{extract_error_detail, MAX_ERROR_BODY_LEN};

// Brought into scope here (rather than only inside each submodule) purely so
// `#[cfg(test)] mod tests` below — and the `super::*` glob each test file
// does — can see them, mirroring what direct declaration in this file used
// to provide before the `include!` split was replaced with real submodules.
#[cfg(test)]
use construct::sanitize_backend_url;
#[cfg(test)]
use errors::is_composio_soft_auth_path;
#[cfg(test)]
use requests::{backend_egress_descriptor, enforce_backend_egress, managed_budget_applies_to_path};

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;

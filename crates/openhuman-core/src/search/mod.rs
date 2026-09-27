//! Web search: host policy over the TinySearch module.
//!
//! Provider implementations, tool schemas and role dispatch live in
//! `vendor/tinysearch`. This domain owns what OpenHuman decides: which
//! providers are usable for this process (`providers`), how a response is shown
//! to the model and the chat UI (`render`), the agent tool bridge (`tools`),
//! and refreshing the module when the credential changes (`bus`).

pub mod providers;
pub mod render;

#[cfg(feature = "modules")]
pub mod bus;
#[cfg(feature = "modules")]
pub mod tools;

#[cfg(feature = "modules")]
pub use tools::{build_search_tools, TinySearchTool};

/// Without the module loader there is no search provider to call.
#[cfg(not(feature = "modules"))]
pub fn build_search_tools(_config: &crate::config::Config) -> Vec<Box<dyn tinytools::Tool>> {
    tracing::debug!("[search][tool] modules feature disabled; no search tools");
    Vec::new()
}

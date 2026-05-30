//! Unified search domain.
//!
//! This is the canonical home for web search selection and agent-facing search
//! tool registration. Engine implementations may still live in integration
//! modules when they own provider-specific HTTP details, but the active search
//! surface is assembled here.

pub mod registry;
pub mod tools;

pub(crate) mod engines;

pub use registry::build_search_tools;
pub use tools::WebSearchTool;

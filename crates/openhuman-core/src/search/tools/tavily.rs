//! Tavily Search + Extract integration — direct API (BYOK, not backend-proxied).
//!
//! **Scope**: Agent + CLI/RPC.
//!
//! **Endpoints**: `POST https://api.tavily.com/search`,
//! `POST https://api.tavily.com/extract`.
//!
//! **Auth**: `Authorization: Bearer <api key>`.
//!
//! When the user selects `tavily` as their search engine and has saved their own
//! Tavily API key, every call in this family goes straight from the desktop
//! client to `api.tavily.com` — the OpenHuman managed backend is never
//! involved. The managed (`engine = "managed"`) path is untouched by this module.

mod client;
mod extract_tool;
mod search_tool;
mod types;

#[cfg(test)]
#[path = "tavily_tests.rs"]
mod tests;

pub use extract_tool::TavilyExtractTool;
pub use search_tool::TavilySearchTool;
pub use types::{
    TavilyExtractResponse, TavilyExtractResult, TavilyImage, TavilyResultItem, TavilySearchResponse,
};

// Re-exported only for the test module (`super::*`), which builds real
// `TavilyClient`s and result rows to exercise the tools' HTTP-adjacent
// formatting directly.
#[cfg(test)]
use crate::tools::traits::{Tool, ToolCallOptions};
#[cfg(test)]
pub(crate) use client::TavilyClient;
#[cfg(test)]
use serde_json::{json, Value};

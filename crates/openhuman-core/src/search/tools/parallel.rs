//! Parallel web search and content extraction integration tools.
//!
//! **Scope**: All (agent loop + CLI/RPC).
//!
//! **Endpoints**:
//!   - `POST /agent-integrations/parallel/search`
//!   - `POST /agent-integrations/parallel/extract`
//!   - `POST /agent-integrations/parallel/chat`
//!   - `POST /agent-integrations/parallel/research` (async; we always wait inline)
//!   - `POST /agent-integrations/parallel/enrich`
//!   - `POST /agent-integrations/parallel/dataset`  (FindAll, async)
//!
//! **Pricing** (fetched from backend):
//!   - Search:  ~$0.01/request
//!   - Extract: ~$0.002/URL
//!   - Chat / research / enrich: per-model or per-processor (see backend `/pricing`)
//!   - Dataset: pre-charged at `match_limit × per-match`
//!
//! The backend handles Parallel API keys, billing, and rate limiting.

mod chat;
mod dataset;
mod enrich;
mod extract;
mod research;
mod search;

#[cfg(test)]
#[path = "parallel_tests.rs"]
mod tests;

pub use chat::ParallelChatTool;
pub use dataset::ParallelDatasetTool;
pub use enrich::ParallelEnrichTool;
pub use extract::ParallelExtractTool;
pub use research::ParallelResearchTool;
pub use search::{ParallelSearchTool, SearchResponse, SearchResultItem};

// Re-exported only for the test module (`super::*`), which asserts on the
// raw backend response shapes and formatting helpers directly.
#[cfg(test)]
use crate::integrations::IntegrationClient;
#[cfg(test)]
use crate::tools::traits::Tool;
#[cfg(test)]
use enrich::{enrich_payload, format_enrich_response, EnrichResponse};
#[cfg(test)]
use extract::ExtractResponse;
#[cfg(test)]
use research::{format_research_response, research_payload, ResearchResponse};
#[cfg(test)]
use serde_json::json;
#[cfg(test)]
use std::sync::Arc;

//! RPC handler functions for the memory tree layer.
//!
//! Public JSON-RPC surface:
//! - `openhuman.memory_tree_ingest` — one unified ingest. Caller supplies
//!   `source_kind` + generic JSON `payload` (adapter-specific). Chat and
//!   document are canonicalised into contract items and handed to the bound
//!   driver's `Ingest` family; mail still canonicalises in process, for the
//!   reasons on [`ingest_rpc`].
//! - `openhuman.memory_tree_list_chunks` — listing with filters.
//! - `openhuman.memory_tree_get_chunk` — single chunk fetch.
//!
//! ## Module layout
//!
//! Split by responsibility: [`ingest`] (canonicalisation + the unified
//! ingest RPC), [`chunks`] (list_chunks / get_chunk), [`backfill`]
//! (store/queue stats + the re-embed backfill status), [`stall`] (the pure
//! disk-size walk, #5324 stall verdict, and status-precedence derivation),
//! [`retry_failed`] (retry_failed + the typed blocking-cause rule),
//! [`pipeline_status`] (the aggregate status snapshot + doctor), and
//! [`set_enabled`] (the scheduler-gate toggle). Every handler keeps its
//! original `rpc::<name>` path through the re-exports below.

mod backfill;
mod chunks;
mod ingest;
mod pipeline_status;
mod retry_failed;
mod set_enabled;
mod stall;

pub use backfill::{backfill_status_rpc, BackfillStatusResponse};
pub use chunks::{
    get_chunk_rpc, list_chunks_rpc, GetChunkRequest, GetChunkResponse, ListChunksRequest,
    ListChunksResponse,
};
// `ingest` also re-exports the canonical payload shapes from
// `canonicalize_types.rs`; this glob keeps every existing `rpc::<Type>` path
// resolving.
pub use ingest::*;
pub use pipeline_status::{
    doctor_rpc, pipeline_status_rpc, PipelineJobCounts, PipelineStatusResponse, QuarantineStatus,
};
pub use retry_failed::{retry_failed_rpc, RetryFailedResponse};
pub use set_enabled::{set_enabled_rpc, SetEnabledRequest, SetEnabledResponse};

// Test-only visibility: `tests` below is declared directly under `rpc` (not
// under the submodule that owns each helper) and reaches these through
// `use super::*;`. Private `use` is enough — a descendant module can see
// everything visible in its ancestors.

#[cfg(test)]
#[path = "rpc_tests.rs"]
mod tests;

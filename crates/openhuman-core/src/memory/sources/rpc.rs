//! RPC handler implementations for memory sources.
//!
//! # How this file reaches memory (#5560)
//!
//! Every memory call below goes through the bound driver — never through an
//! in-process engine handle. The four surfaces this file used to reach into
//! `tinymemory_core::tinycortex` for became contract members in tinymemory
//! v1.7.0, and the mapping is one-for-one:
//!
//! | What a handler needs | Contract member |
//! |---|---|
//! | coding-session discovery | `MemoryCodingSessions::coding_session_status` |
//! | coding-session ingestion | `MemoryCodingSessions::ingest_coding_sessions` |
//! | raw-archive coverage, and its repair | `MemorySourceSync::raw_archive_coverage` / `rebuild_from_raw_archive` |
//! | the sync audit log | `MemorySourceSync::sync_audit_log` |
//! | the sync price | `MemorySourceSync::estimate_sync_cost_usd` |
//!
//! Two of those look shortcuttable and are not, so the reasons are recorded
//! here rather than left to be re-derived:
//!
//! - **The audit log is not read through the `memory::sync::audit` host shim.**
//!   That path resolves, and taking it would move this file off the
//!   direct-engine-reference scanner while removing no coupling whatsoever —
//!   precisely the edit the ratchet's own docs call the one that would make the
//!   lint lie.
//! - **The price is asked, never computed here.** The same constants behind
//!   `estimate_sync_cost_usd` stamped `estimated_cost_usd` onto every audit row
//!   the driver has already written, and `monthly_cost_summary_rpc` below totals
//!   those very rows. A host-side copy of the arithmetic becomes a second price
//!   the moment either side is retuned, and one screen would then show a
//!   projection and a history priced differently with nothing to say which.
//!
//! ## Refusing when the driver does not serve the family
//!
//! `as_source_sync()` / `as_coding_sessions()` answering `None` means the bound
//! driver serves no such family, and every handler here **refuses, naming the
//! driver** (see [`unserved`]). None of them reports an empty log, a zero cost
//! or an empty source list instead.
//!
//! That is a deliberately different trade from the tree read handlers, which do
//! degrade to empty. There, "no hits" is a true statement about a driver that
//! keeps no summary tree. Here the family *is* the entire subject of the call,
//! so an empty success is indistinguishable from "nothing has ever synced" or
//! "no coding agent is installed" — a wrong answer the caller cannot tell from
//! a right one, on the two screens where the number is a promise about money
//! and about how long an import will take.
//!
//! ## Module layout
//!
//! Split by responsibility rather than kept as one file: [`coding_sessions`]
//! (discovery + ingestion), [`registry_crud`] (list/get/add/update/remove and
//! the per-source item browse — talks only to `registry` and `readers` and
//! never resolves a binding), [`source_sync`] (the row-level Sync button and
//! raw-archive reconciliation), [`status_toolkits`] (ingest status and the
//! supported-toolkit catalog), [`cost_reporting`] (the audit log, per-source
//! cost estimate, and the monthly summary), and [`apply_all`] (the "apply all
//! in" sweep). Every handler keeps its original `rpc::<name>` path through the
//! re-exports below.

mod apply_all;
mod coding_sessions;
mod cost_reporting;
mod registry_crud;
mod source_sync;
mod status_toolkits;

pub use apply_all::{apply_all_in_rpc, AllInResponse};
pub(crate) use coding_sessions::ingest_budget;
pub use coding_sessions::{
    coding_session_status_rpc, ingest_coding_sessions_rpc, CodingSessionIngestRequest,
    CodingSessionStatusResponse,
};
pub use cost_reporting::{
    estimate_sync_cost_rpc, monthly_cost_summary_rpc, sync_audit_log_rpc, EstimateSyncCostRequest,
    EstimateSyncCostResponse, MonthlyCostSummaryResponse, SyncAuditLogResponse,
};
pub use registry_crud::{
    add_rpc, get_rpc, list_items_rpc, list_rpc, read_item_rpc, remove_rpc, update_rpc, AddRequest,
    AddResponse, GetRequest, GetResponse, ListItemsRequest, ListItemsResponse, ListResponse,
    ReadItemRequest, ReadItemResponse, RemoveRequest, RemoveResponse, UpdateRequest,
    UpdateResponse,
};
pub use source_sync::{
    reconcile_rpc, sync_rpc, ReconcileRequest, ReconcileResponse, ReconcileScopeReport,
    SyncRequest, SyncResponse,
};
pub use status_toolkits::{
    status_list_rpc, supported_toolkits_rpc, StatusListResponse, SupportedToolkitsResponse,
};

// Test-only visibility: the sibling test modules below are declared directly
// under `rpc` (not under the submodule that owns each helper) and reach these
// through `use super::*;`. Private `use` is enough — a descendant module can
// see everything visible in its ancestors, `unserved` and friends included.

#[cfg(test)]
#[path = "rpc_filter_tests_tests.rs"]
mod filter_tests;

#[cfg(test)]
#[path = "rpc_supported_toolkits_tests_tests.rs"]
mod supported_toolkits_tests;

#[cfg(test)]
#[path = "rpc_budget_tests_tests.rs"]
mod budget_tests;

#[cfg(test)]
#[path = "rpc_monthly_summary_tests_tests.rs"]
mod monthly_summary_tests;

#[cfg(test)]
#[path = "rpc_tests.rs"]
mod rpc_tests;

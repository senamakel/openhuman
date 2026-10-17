//! Host-specific prompt construction, tool policy refresh, and the model graph.
//!
//! Generic turn lifecycle is intentionally absent: it is owned by
//! `tinyagents_runtime::Session` in `runtime_session`.

mod context;

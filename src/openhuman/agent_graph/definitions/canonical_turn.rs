//! Node ids of the canonical agent turn, for `agent_graph_definition_list`.
//!
//! The runnable canonical turn lives in
//! [`super::tinyagents_graphs::build_canonical_turn`] (executed on the
//! `tinyagents` engine). This module now only exposes the node-id list the RPC
//! definition registry advertises.

/// Node ids of the canonical turn, in topological reading order.
pub const NODES: &[&str] = &[
    "dispatch",
    "parse",
    "stop_check",
    "tools",
    "compact",
    "finalize",
];

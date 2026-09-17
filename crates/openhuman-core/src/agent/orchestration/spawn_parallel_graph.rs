//! Structure-only graph scaffold for `spawn_parallel_agents`.
//!
//! The tool wrapper still owns `ToolResult` translation. This module owns
//! request parsing, parent-context validation, graph-side request validation,
//! worktree preflight, progress/event projection, worker fanout, final JSON
//! formatting, and the topology surface.
//!
//! **Write safety.** Whether a worker *needs* a claim on the shared workspace is
//! an OpenHuman decision — it reads sandbox mode, tool permissions and the
//! isolation request. Whether the claims of a whole batch can be granted
//! together is not, and goes through
//! [`plan_shared_workspace_dispatch`](tinyagents_graph::parallel::plan_shared_workspace_dispatch).
//! The rejection sentences stay here, which is why the crate reports conflicts
//! as data.
//!
//! ## Module layout
//!
//! - [`request`] — request decoding and structural validation.
//! - [`types`] — shared worker/result/lineage types.
//! - [`staging`] — OpenHuman policy admission and shared-workspace arbitration.
//! - [`dispatch`] — worktree preflight and progress projection for the
//!   `dispatch` phase, turning admitted tasks into staged workers.
//! - [`workers`] — the serial and `map_reduce` worker fanout.
//! - [`collect`] — collecting fanned-out results into the tool's final shape.
//! - [`graph`] — the fixed phase-graph scaffold and topology export.
//! - [`run`] — the public entry points that tie the above together.

mod collect;
mod dispatch;
mod graph;
mod request;
mod run;
mod staging;
mod types;
mod workers;

pub(crate) use collect::{format_spawn_parallel_success, SpawnParallelGraphOutcome};
pub(crate) use graph::spawn_parallel_graph_topology;
pub(crate) use request::SpawnParallelTaskValidationError;
pub(crate) use run::run_spawn_parallel_graph_with_cancellation_and_workspace;

#[cfg(test)]
pub(crate) use request::ParallelAgentTask;
#[cfg(test)]
pub(crate) use staging::{
    prepare_spawn_parallel_tasks_from_defs, with_ownership_boundary, ParallelTaskRejectionKind,
    SpawnParallelTaskPreflight, WorkerDispatchMode,
};
#[cfg(test)]
pub(crate) use types::{ParallelAgentLineage, ParallelAgentResult};

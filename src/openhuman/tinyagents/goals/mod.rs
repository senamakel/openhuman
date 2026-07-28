//! OpenHuman integration for tinyagents' thread-scoped goals.
//!
//! The `tinyagents::graph::goals` module owns the data model, lifecycle rules,
//! concurrency, and persistence. This module adds OpenHuman's stable
//! `thread_goals.*` RPC surface, agent tools, domain events, prompt injection,
//! autonomous continuation, and legacy-store migration.

pub mod continuation;
pub mod migration;
pub mod ops;
pub mod runtime;
mod schemas;
pub mod store;
pub mod tools;

pub use ::tinyagents::graph::goals::{ThreadGoal, ThreadGoalStatus};
pub use schemas::all_thread_goals_registered_controllers;
pub use tools::{GoalCompleteTool, GoalGetTool, GoalSetTool};

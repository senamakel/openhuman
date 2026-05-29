//! Task sources — proactive ingestion of work items from external tools.
//!
//! A [`TaskSource`] is a user-configured pull from an external tool
//! (GitHub, Notion, Linear, ClickUp) with a per-provider [`FilterSpec`].
//! The periodic poll ([`periodic`]) fetches matching items through the
//! existing Composio providers' [`fetch_tasks`] surface, the pipeline
//! ([`pipeline`]) dedups + enriches them, and [`route`] drops a todo
//! card onto the dedicated `task-sources` thread board and (for
//! proactive sources) dispatches a triage turn so an agent can start
//! working.
//!
//! Layering mirrors the `cron` domain: `mod.rs` is export-only, business
//! logic lives in the sibling modules, persistence is SQLite in
//! `store.rs`, and the RPC surface is wired through `schemas.rs`.
//!
//! [`fetch_tasks`]: crate::openhuman::memory_sync::composio::providers::ComposioProvider::fetch_tasks

pub mod filter;
pub mod store;
pub mod types;

pub use crate::openhuman::memory_sync::composio::providers::{NormalizedTask, TaskFetchFilter};
pub use types::{
    EnrichedTask, FetchOutcome, FetchReason, FilterSpec, ProviderSlug, SourceTarget, TaskSource,
    TaskSourcePatch,
};

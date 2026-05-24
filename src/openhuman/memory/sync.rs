//! High-level memory sync orchestration.
//!
//! This module owns the user-facing "sync my memory" workflow:
//!
//! 1. accept a manual or scheduled sync request
//! 2. emit coarse lifecycle events for UI visibility
//! 3. dispatch into [`crate::openhuman::memory_sync`] backends
//! 4. rely on `memory_store` + `memory_queue` + `memory_tree` backends to
//!    persist, enqueue, ingest, and seal the resulting data
//!
//! The low-level provider implementations live in `memory_sync/*`; this module
//! is the orchestration seam the `memory` domain presents to RPC/tools/UI.

use serde::{Deserialize, Serialize};
use crate::core::event_bus::{publish_global, DomainEvent};

/// Why a sync run was requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemorySyncTrigger {
    Manual,
    Cron,
}

impl MemorySyncTrigger {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Cron => "cron",
        }
    }
}

/// Coarse orchestration stages surfaced to the frontend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemorySyncStage {
    Requested,
    Fetching,
    Stored,
    Queued,
    Ingesting,
    Completed,
    Failed,
}

impl MemorySyncStage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Requested => "requested",
            Self::Fetching => "fetching",
            Self::Stored => "stored",
            Self::Queued => "queued",
            Self::Ingesting => "ingesting",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

/// Publish a coarse sync lifecycle event for UI subscribers.
pub fn emit_sync_stage(
    trigger: MemorySyncTrigger,
    stage: MemorySyncStage,
    provider: Option<&str>,
    connection_id: Option<&str>,
    detail: Option<String>,
) {
    publish_global(DomainEvent::MemorySyncStageChanged {
        trigger: trigger.as_str().to_string(),
        stage: stage.as_str().to_string(),
        provider: provider.map(str::to_string),
        connection_id: connection_id.map(str::to_string),
        detail,
    });
}

//! Memory orchestration.
//!
//! This module is the high-level routing + policy layer over the memory
//! stack. Owns the ingest pipeline, background job handlers, scoring,
//! tree-building policy (tree_global / tree_topic), recall ranking, and
//! the RPC surface. Storage primitives all live in sibling memory_*
//! modules — see [`README.md`](README.md) for the full map.
//!
//! No SQLite, no on-disk md, no vector tables here — those belong one
//! layer down in [`memory_store`](crate::openhuman::memory_store).

// Legacy memory modules
pub mod conversations;
pub mod global;
pub mod ingestion;
pub mod ops;
pub mod preferences;
pub mod rpc_models;
pub mod schemas;
pub mod stm_recall;
pub mod sync_status;
pub mod traits;

// Tool-scoped memory moved to top-level `memory_tools`. Re-exported here so
// existing `memory::tool_memory::*` paths still resolve during the migration.
pub use crate::openhuman::memory_tools as tool_memory;

// Modules moved from memory_tree (Phase 3)
pub mod canonicalize;
pub mod chat;
pub mod ingest_pipeline;
pub mod jobs;
pub mod read_rpc;
pub mod retrieval;
pub mod schema;
pub mod score;
pub mod summarizer;
pub mod tree_global;
pub mod tree_rpc;
pub mod tree_topic;
pub mod util;

pub use ingestion::{
    ExtractedEntity, ExtractedRelation, ExtractionMode, IngestionJob, IngestionQueue,
    IngestionState, IngestionStatusSnapshot, MemoryIngestionConfig, MemoryIngestionRequest,
    MemoryIngestionResult, DEFAULT_MEMORY_EXTRACTION_MODEL,
};
pub use ops as rpc;
pub use ops::*;
pub use rpc_models::*;
pub use schemas::{
    all_controller_schemas as all_memory_controller_schemas,
    all_registered_controllers as all_memory_registered_controllers,
};
pub use crate::openhuman::memory_store::{
    create_memory, create_memory_for_migration, create_memory_with_local_ai,
    effective_embedding_settings, effective_memory_backend_name, MemoryClient, MemoryClientRef,
    MemoryItemKind, MemoryState, NamespaceDocumentInput, NamespaceMemoryHit, NamespaceQueryResult,
    NamespaceRetrievalContext, RetrievalScoreBreakdown, UnifiedMemory,
};
pub use sync_status::{
    all_memory_sync_status_controller_schemas, all_memory_sync_status_registered_controllers,
    FreshnessLabel, MemorySyncStatus,
};
pub use tool_memory::{
    render_tool_memory_rules, tool_memory_namespace, ToolMemoryCaptureHook, ToolMemoryPriority,
    ToolMemoryRule, ToolMemoryRulesSection, ToolMemorySource, ToolMemoryStore, TOOL_MEMORY_HEADING,
    TOOL_MEMORY_PROMPT_CAP,
};
pub use traits::{Memory, MemoryCategory, MemoryEntry, NamespaceSummary, RecallOpts};

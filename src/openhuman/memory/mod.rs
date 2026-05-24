//! Memory system for OpenHuman.
//!
//! This module provides the core abstractions and implementations for the memory system,
//! including semantic search, ingestion pipelines, document management, and knowledge graph
//! operations. It integrates vector search, keyword search, and relational data to provide
//! a unified memory interface for AI agents.
//!
//! Storage is consolidated in `memory_store`:
//! - `memory_store::unified`  — SQLite unified memory
//! - `memory_store::chunks`   — SQLite chunk + type layer
//! - `memory_store::content`  — on-disk .md content store
//! - `memory_store::vectors`  — local vector/embedding store
//! - `memory_store::trees*`   — summary-tree persistence

// Legacy memory modules
pub mod chunker;
pub mod conversations;
pub mod global;
pub mod ingestion;
pub mod ops;
pub mod preferences;
pub mod rpc_models;
pub mod safety;
pub mod schemas;
pub mod stm_recall;
pub mod sync_status;
pub mod tool_memory;
pub mod traits;

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
    create_memory_with_storage, create_memory_with_storage_and_routes,
    effective_embedding_settings, effective_embedding_settings_probed,
    effective_memory_backend_name, MemoryClient, MemoryClientRef, MemoryItemKind, MemoryState,
    NamespaceDocumentInput, NamespaceMemoryHit, NamespaceQueryResult, NamespaceRetrievalContext,
    RetrievalScoreBreakdown, UnifiedMemory,
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

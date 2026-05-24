//! Memory system for OpenHuman.
//!
//! This module provides the core abstractions and implementations for the memory system,
//! including semantic search, ingestion pipelines, document management, and knowledge graph
//! operations. It integrates vector search, keyword search, and relational data to provide
//! a unified memory interface for AI agents.
//!
//! Phase 3 of the memory_tree cleanup: the following modules were moved from
//! `memory_tree` into this module:
//! `canonicalize`, `chat`, `content_store`, `ingest_pipeline`, `jobs`,
//! `read_rpc`, `retrieval`, `tree_rpc`, `schema`, `score`, `chunk_store`,
//! `summarizer`, `chunk_types`, `util`.
//! The tree-specific `chunker`, `tree/*`, `sources/`, `summarise`, and the
//! `tools/` (including `ingest_document`) stayed under `memory_tree`.

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
pub mod store;
pub mod sync_status;
pub mod tool_memory;
pub mod traits;

// Modules moved from memory_tree (Phase 3)
pub mod canonicalize;
pub mod chat;
pub mod chunk_store;
pub mod chunk_types;
pub mod content_store;
pub mod ingest_pipeline;
pub mod jobs;
pub mod read_rpc;
pub mod retrieval;
pub mod schema;
pub mod score;
pub mod summarizer;
pub mod tree_rpc;
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
pub use store::{
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

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
pub mod global;
pub mod ingestion;
pub mod ops;
pub mod preferences;
pub mod rpc_models;
pub mod schemas;
pub mod traits;

// Modules moved from memory_tree (Phase 3)
pub mod chat;
pub mod ingest_pipeline;
pub mod query;
pub mod read_rpc;
pub mod remember;
pub mod schema;
pub mod sync;
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
pub use traits::{Memory, MemoryCategory, MemoryEntry, NamespaceSummary, RecallOpts};

//! Memory tools — write-path tools for the memory pipeline.
//!
//! The tree-iteration (read-path) tools remain in `memory_tree::tools`.
//! This module holds the write-path ingest tool that was moved from
//! `memory_tree::tools` in Phase 3 of the memory_tree cleanup.

pub mod ingest_document;

pub use ingest_document::MemoryTreeIngestDocumentTool;

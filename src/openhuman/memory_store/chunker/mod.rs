//! Markdown chunking — storage primitive owned by memory_store.
//!
//! Two flavors live here because they cover different ingest paths:
//!
//! - [`source_kind`] — the Phase B source-kind-dispatch chunker used by
//!   the memory ingest pipeline (chat / email / document). Produces stable
//!   sequence numbers per source and bounded chunks ready for the L0
//!   bucket-seal.
//! - [`semantic`]    — heading- and paragraph-aware chunker used by the
//!   unified memory writer (`unified::helpers`) to split large documents
//!   into LLM-context-sized pieces while preserving heading context.
//!
//! Both produce string-shaped chunks (the unit of memory_store persistence)
//! so they live next to chunks/ and content/ rather than in any consumer
//! module.

pub mod semantic;
pub mod source_kind;

pub use semantic::chunk_markdown as chunk_semantic;
pub use source_kind::{chunk_markdown, ChunkerInput, ChunkerOptions};

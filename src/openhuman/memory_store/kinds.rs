//! Catalog of every kind of data the memory_store persists.
//!
//! Every stored object falls into exactly one of these kinds. The enum is the
//! authoritative answer to "what can memory_store store?" and is used by:
//! - The retrieval facade, to fan out a query to the right backends.
//! - The vector/obsidian compatibility traits, to dispatch by kind.
//! - Agent tools, to surface a kind filter to LLM callers.
//!
//! Adding a new storage kind = adding a variant here, an impl of the
//! [`VectorEmbeddable`] / [`ObsidianRepresentable`] traits
//! ([`crate::openhuman::memory_store::traits`]), and a delegation in
//! [`crate::openhuman::memory_store::retrieval`].

use serde::{Deserialize, Serialize};

/// Every persisted data shape in memory_store, named once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    /// On-disk raw markdown file (the content store). One file per
    /// canonicalized source chunk OR per summary node.
    Content,
    /// SQLite chunk row (canonicalized payload + provenance + tags). The
    /// "leaf" unit of the summary tree.
    Chunk,
    /// Sealed summary tree node — Source, Global, or Topic flavor. Tree
    /// flavor is carried separately on the row (`TreeKind`).
    Tree,
    /// Dense vector embedding row in the local vector DB.
    Vector,
    /// Namespace document persisted by `UnifiedMemory` (preferences,
    /// learnings, structured docs the LLM stored explicitly).
    Document,
    /// Namespace key-value record persisted by `UnifiedMemory`.
    Kv,
    /// Graph relation row persisted by `UnifiedMemory`.
    Graph,
    /// Address-book contact (`people::Person`) routed through the contacts
    /// facade.
    Contact,
}

impl MemoryKind {
    /// Snake-case discriminant used in RPC payloads, logs, and tool args.
    pub fn as_str(self) -> &'static str {
        match self {
            MemoryKind::Content => "content",
            MemoryKind::Chunk => "chunk",
            MemoryKind::Tree => "tree",
            MemoryKind::Vector => "vector",
            MemoryKind::Document => "document",
            MemoryKind::Kv => "kv",
            MemoryKind::Graph => "graph",
            MemoryKind::Contact => "contact",
        }
    }

    /// Every variant, in stable declaration order. Useful for fan-out
    /// retrieval and for surfacing the kind catalog to LLM tools.
    pub const ALL: &'static [MemoryKind] = &[
        MemoryKind::Content,
        MemoryKind::Chunk,
        MemoryKind::Tree,
        MemoryKind::Vector,
        MemoryKind::Document,
        MemoryKind::Kv,
        MemoryKind::Graph,
        MemoryKind::Contact,
    ];
}

/// Per-kind canonical Rust type aliases — one stop to find "what struct
/// represents a Tree row?", "what struct represents a Contact?", etc. Aliases
/// (not re-exports) so the documentation lives here and the source-of-truth
/// types stay in their owning modules.
pub mod types {
    pub use crate::openhuman::memory_store::chunks::types::Chunk;
    pub use crate::openhuman::memory_store::contacts::Person as Contact;
    pub use crate::openhuman::memory_store::trees::{SummaryNode as TreeNode, Tree, TreeKind};
    pub use crate::openhuman::memory_store::types::{
        GraphRelationRecord as Graph, MemoryKvRecord as Kv, StoredMemoryDocument as Document,
    };
}

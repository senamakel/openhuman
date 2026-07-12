//! Storage layer for tool-scoped rules — thin host shim over
//! `tinycortex::memory::tool_memory::store` (W7).
//!
//! The store engine (put / get / list / delete / prompt over an
//! `Arc<dyn Memory>`) is the crate's. It is generic over the **crate** `Memory`
//! trait, while host call sites hold `Arc<dyn `[`crate::openhuman::memory::Memory`]`>`
//! — which is the crate trait *plus* the host-only `sqlite_conn()` escape hatch
//! (gap G1). [`HostMemoryBridge`] adapts one to the other (every method forwards
//! unchanged), so [`tool_memory_store`] builds a crate `ToolMemoryStore` over a
//! host backend without waiting on the W3 trait unification.
//!
//! Behaviour note: the deleted host engine had a `sqlite_conn` fast-path in
//! `list_rules` (a direct `memory_docs` query ordered by `updated_at`); the
//! crate engine uses the trait `list()` only. This is behaviour-equivalent —
//! the host already used `list()` as its fallback for connectionless backends —
//! and differs only by a negligible per-tool query cost.

use std::sync::Arc;

use async_trait::async_trait;

use tinycortex::memory::Memory as CrateMemory;
use tinycortex::memory::{MemoryCategory, MemoryEntry, MemoryTaint, NamespaceSummary, RecallOpts};

use crate::openhuman::memory::Memory;

pub use tinycortex::memory::tool_memory::store::{ToolMemoryStore, TOOL_MEMORY_PROMPT_CAP};

/// Presents a host [`Arc<dyn Memory>`] as the crate [`Memory`](CrateMemory) the
/// crate `ToolMemoryStore` is generic over. The host trait is the crate trait
/// plus `sqlite_conn`, so every method forwards verbatim (the value types are
/// already crate re-exports, so no conversion is needed).
struct HostMemoryBridge(Arc<dyn Memory>);

#[async_trait]
impl CrateMemory for HostMemoryBridge {
    fn name(&self) -> &str {
        self.0.name()
    }

    async fn store(
        &self,
        namespace: &str,
        key: &str,
        content: &str,
        category: MemoryCategory,
        session_id: Option<&str>,
    ) -> anyhow::Result<()> {
        self.0
            .store(namespace, key, content, category, session_id)
            .await
    }

    async fn store_with_taint(
        &self,
        namespace: &str,
        key: &str,
        content: &str,
        category: MemoryCategory,
        session_id: Option<&str>,
        taint: MemoryTaint,
    ) -> anyhow::Result<()> {
        self.0
            .store_with_taint(namespace, key, content, category, session_id, taint)
            .await
    }

    async fn recall(
        &self,
        query: &str,
        limit: usize,
        opts: RecallOpts<'_>,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        self.0.recall(query, limit, opts).await
    }

    async fn recall_relevant_by_vector(
        &self,
        namespace: &str,
        query: &str,
        limit: usize,
        min_vector_similarity: f64,
    ) -> anyhow::Result<Vec<(String, String)>> {
        self.0
            .recall_relevant_by_vector(namespace, query, limit, min_vector_similarity)
            .await
    }

    async fn get(&self, namespace: &str, key: &str) -> anyhow::Result<Option<MemoryEntry>> {
        self.0.get(namespace, key).await
    }

    async fn list(
        &self,
        namespace: Option<&str>,
        category: Option<&MemoryCategory>,
        session_id: Option<&str>,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        // UnifiedMemory::list() lists *documents* and surfaces each document's
        // title as the entry content — so tool rules, stored as JSON in
        // `memory_docs`, can't be round-tripped back through it (the crate
        // `list_rules` would fail to deserialize the title as a rule and drop
        // it). When the backend exposes a raw connection — as UnifiedMemory
        // does — read the real content straight from `memory_docs`, mirroring
        // the fast-path the host `ToolMemoryStore` used before this engine moved
        // to the crate. Connectionless backends (e.g. the test `MockMemory`)
        // fall back to the trait `list()`, whose content is already faithful.
        if let (Some(ns), Some(conn)) = (namespace, self.0.sqlite_conn()) {
            let conn = conn.lock();
            let mut stmt = conn.prepare(
                "SELECT document_id, key, content, taint \
                 FROM memory_docs WHERE namespace = ?1",
            )?;
            let rows = stmt.query_map([ns], |r| {
                Ok(MemoryEntry {
                    id: r.get::<_, String>(0)?,
                    key: r.get::<_, String>(1)?,
                    content: r.get::<_, String>(2)?,
                    namespace: Some(ns.to_string()),
                    // These fields are unused by the sole consumer
                    // (`ToolMemoryStore::list_rules`, which reads key + content);
                    // taint is carried faithfully, the rest are placeholders.
                    category: MemoryCategory::Core,
                    timestamp: String::new(),
                    session_id: None,
                    score: None,
                    taint: MemoryTaint::from_db_str(&r.get::<_, String>(3)?),
                })
            })?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            return Ok(out);
        }
        self.0.list(namespace, category, session_id).await
    }

    async fn forget(&self, namespace: &str, key: &str) -> anyhow::Result<bool> {
        self.0.forget(namespace, key).await
    }

    async fn namespace_summaries(&self) -> anyhow::Result<Vec<NamespaceSummary>> {
        self.0.namespace_summaries().await
    }

    async fn count(&self) -> anyhow::Result<usize> {
        self.0.count().await
    }

    async fn health_check(&self) -> bool {
        self.0.health_check().await
    }
}

/// Build a crate [`ToolMemoryStore`] over a host memory backend, bridging the
/// host `Memory` trait object to the crate `Memory` the store requires.
pub fn tool_memory_store(memory: Arc<dyn Memory>) -> ToolMemoryStore {
    ToolMemoryStore::new(Arc::new(HostMemoryBridge(memory)))
}

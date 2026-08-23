//! An in-memory [`MemoryProvider`] that actually stores things.
//!
//! # Why this exists
//!
//! The module port keeps meeting the same test problem. A consumer used to be
//! handed an `Arc<dyn Memory>` and tests handed it a real `UnifiedMemory` over
//! a temp dir, then asserted a genuine round trip: write, read back, prune.
//! Converting the consumer to the guard breaks those tests, and the cheap
//! answer — `#[ignore]` behind `OPENHUMAN_MODULE_PATH` — pays for each
//! conversion with real coverage.
//!
//! [`super::test_support::RecordingProvider`] cannot stand in: it records calls
//! and answers empty, which proves a call was *made* but never that the data
//! came back. Round-trip assertions need storage.
//!
//! So this is storage: a `HashMap` keyed by `(namespace, key)` behind a mutex,
//! implementing the mandatory three so it can be wrapped in a real
//! [`MemoryGuard`](super::MemoryGuard) and dropped in wherever a consumer now
//! wants a guard.
//!
//! # It is deliberately not `#[cfg(test)]`
//!
//! Integration tests under `tests/` link the library compiled without
//! `cfg(test)`, so a test-gated helper is invisible to them — the trap
//! `ProfileStore::for_tests` documents and this port has already fallen into
//! once. `#[doc(hidden)]` keeps it off the public docs instead.
//!
//! # What it does not pretend to be
//!
//! `recall` is a substring match over content, not a ranked hybrid search. That
//! is enough for "did the write land and come back", which is what these tests
//! assert; it is **not** enough to test ranking, and a test about ordering
//! should use the real engine rather than this.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;

use crate::openhuman::memory::api::capabilities::{Capabilities, Capability};
use crate::openhuman::memory::api::chunks::{Chunk, Metadata, SourceKind};
use crate::openhuman::memory::api::error::MemoryError;
use crate::openhuman::memory::api::health::MemoryHealth;
use crate::openhuman::memory::api::provider::chunks::{ChunkDetail, ChunkEmbedding, ChunkQuery};
use crate::openhuman::memory::api::provider::types::SourceScope;
use crate::openhuman::memory::api::provider::types::{ExportPage, ExportRecord, ImportOutcome};
use crate::openhuman::memory::api::provider::types::{IngestItem, IngestOutcome};
use crate::openhuman::memory::api::provider::{MemoryChunks, MemoryIngest};
use crate::openhuman::memory::api::provider::{
    MemoryCore, MemoryPortability, MemoryProvider, MemoryRecall,
};
use crate::openhuman::memory::api::recall::OwnedRecallOpts;
use crate::openhuman::memory::api::types::{
    MemoryCategory, MemoryEntry, MemoryTaint, NamespaceSummary,
};

/// Entries held in memory, keyed by `(namespace, key)`.
#[derive(Default)]
pub struct InMemoryProvider {
    entries: Mutex<HashMap<(String, String), MemoryEntry>>,
}

impl InMemoryProvider {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many entries are stored, for assertions about pruning.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.lock().len()
    }

    /// Whether the store holds nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.lock().is_empty()
    }
}

#[async_trait]
impl MemoryCore for InMemoryProvider {
    async fn store(
        &self,
        namespace: &str,
        key: &str,
        content: &str,
        category: MemoryCategory,
        session_id: Option<&str>,
        taint: MemoryTaint,
    ) -> Result<(), MemoryError> {
        let entry = MemoryEntry {
            id: format!("{namespace}/{key}"),
            key: key.to_string(),
            content: content.to_string(),
            namespace: Some(namespace.to_string()),
            category,
            // Monotonic enough for "oldest first" pruning assertions, and
            // stable to render.
            timestamp: chrono::Utc::now().to_rfc3339(),
            session_id: session_id.map(str::to_string),
            score: None,
            taint,
        };
        self.entries
            .lock()
            .insert((namespace.to_string(), key.to_string()), entry);
        Ok(())
    }

    async fn get(&self, namespace: &str, key: &str) -> Result<Option<MemoryEntry>, MemoryError> {
        Ok(self
            .entries
            .lock()
            .get(&(namespace.to_string(), key.to_string()))
            .cloned())
    }

    async fn forget(&self, namespace: &str, key: &str) -> Result<bool, MemoryError> {
        Ok(self
            .entries
            .lock()
            .remove(&(namespace.to_string(), key.to_string()))
            .is_some())
    }

    async fn list(
        &self,
        namespace: Option<&str>,
        category: Option<&MemoryCategory>,
        session_id: Option<&str>,
    ) -> Result<Vec<MemoryEntry>, MemoryError> {
        let entries = self.entries.lock();
        let mut out: Vec<MemoryEntry> = entries
            .values()
            .filter(|e| namespace.is_none_or(|ns| e.namespace.as_deref() == Some(ns)))
            .filter(|e| category.is_none_or(|c| &e.category == c))
            .filter(|e| session_id.is_none_or(|s| e.session_id.as_deref() == Some(s)))
            .cloned()
            .collect();
        // Deterministic order — a `HashMap`'s iteration order varies per
        // process and would make an otherwise-identical assertion flaky.
        out.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(out)
    }

    async fn namespaces(&self) -> Result<Vec<NamespaceSummary>, MemoryError> {
        let entries = self.entries.lock();
        let mut counts: HashMap<String, usize> = HashMap::new();
        for entry in entries.values() {
            if let Some(ns) = &entry.namespace {
                *counts.entry(ns.clone()).or_default() += 1;
            }
        }
        let mut out: Vec<NamespaceSummary> = counts
            .into_iter()
            .map(|(namespace, count)| NamespaceSummary {
                namespace,
                count,
                last_updated: None,
            })
            .collect();
        out.sort_by(|a, b| a.namespace.cmp(&b.namespace));
        Ok(out)
    }
}

#[async_trait]
impl MemoryRecall for InMemoryProvider {
    /// Substring match, not ranking — see the module docs.
    async fn recall(
        &self,
        query: &str,
        limit: usize,
        opts: &OwnedRecallOpts,
        _scope: Option<&SourceScope>,
    ) -> Result<Vec<MemoryEntry>, MemoryError> {
        let needle = query.to_lowercase();
        let entries = self.entries.lock();
        let mut out: Vec<MemoryEntry> = entries
            .values()
            .filter(|e| {
                opts.namespace
                    .as_deref()
                    .is_none_or(|ns| e.namespace.as_deref() == Some(ns))
            })
            .filter(|e| e.content.to_lowercase().contains(&needle))
            .cloned()
            .collect();
        out.sort_by(|a, b| a.key.cmp(&b.key));
        out.truncate(limit);
        Ok(out)
    }
}

#[async_trait]
impl MemoryPortability for InMemoryProvider {
    async fn export_page(
        &self,
        _cursor: Option<&str>,
        _limit: usize,
    ) -> Result<ExportPage, MemoryError> {
        Err(MemoryError::Other(anyhow::anyhow!(
            "InMemoryProvider does not implement export"
        )))
    }

    async fn import_records(
        &self,
        _records: Vec<ExportRecord>,
    ) -> Result<ImportOutcome, MemoryError> {
        Err(MemoryError::Other(anyhow::anyhow!(
            "InMemoryProvider does not implement import"
        )))
    }
}

#[async_trait]
impl MemoryProvider for InMemoryProvider {
    fn driver_id(&self) -> &str {
        "in-memory"
    }

    /// Only the mandatory three. Advertising more would fail
    /// `audit_provider`, since no optional accessor is overridden.
    fn capabilities(&self) -> Capabilities {
        Capabilities::mandatory()
    }

    async fn health(&self) -> MemoryHealth {
        MemoryHealth::Ready
    }
}

/// A real [`MemoryGuard`](super::MemoryGuard) over a fresh in-memory store,
/// plus the store itself for direct assertions.
#[must_use]
pub fn guarded_in_memory() -> (Arc<InMemoryProvider>, Arc<super::MemoryGuard>) {
    let provider = Arc::new(InMemoryProvider::new());
    let guard = guard_over(Arc::clone(&provider) as Arc<dyn MemoryProvider>);
    (provider, guard)
}

/// Wrap any provider in a real [`MemoryGuard`](super::MemoryGuard) at the
/// trusted tier.
///
/// Split out of [`guarded_in_memory`] so a test that needs an optional family
/// — retrieval, say — can supply its own provider and still be exercised
/// through the same policy decorator production uses, rather than calling the
/// provider directly and skipping the guard entirely.
#[must_use]
pub fn guard_over(provider: Arc<dyn MemoryProvider>) -> Arc<super::MemoryGuard> {
    let policy = Arc::new(super::GuardPolicy::new(
        "in-memory",
        crate::core::subsystem::DriverClass::Embedded,
        crate::openhuman::config::schema::MemoryHooksConfig::default(),
        super::policy::TRUSTED,
    ));
    Arc::new(super::MemoryGuard::new(provider, policy))
}

/// A provider whose `recall` answers with a fixed entry list, whatever the
/// query.
///
/// # Why this is not [`InMemoryProvider`]
///
/// That one substring-matches, which is right for a round-trip test and wrong
/// for the several channel/context tests that script a specific result set —
/// scored entries, an over-long entry, ten entries to overflow a budget — and
/// assert on what the *caller* does with it. Those tests are about rendering
/// and filtering downstream of recall, so recall itself has to be a constant.
///
/// Everything else is inert: writes are accepted and dropped, reads answer
/// empty. A test needing real storage wants [`InMemoryProvider`].
pub struct FixedRecallProvider {
    entries: Vec<MemoryEntry>,
}

impl FixedRecallProvider {
    #[must_use]
    pub fn new(entries: Vec<MemoryEntry>) -> Self {
        Self { entries }
    }

    /// The provider wrapped in a real guard, ready to drop into a context.
    #[must_use]
    pub fn guarded(entries: Vec<MemoryEntry>) -> Arc<super::MemoryGuard> {
        guard_over(Arc::new(Self::new(entries)) as Arc<dyn MemoryProvider>)
    }
}

#[async_trait]
impl MemoryCore for FixedRecallProvider {
    async fn store(
        &self,
        _namespace: &str,
        _key: &str,
        _content: &str,
        _category: MemoryCategory,
        _session_id: Option<&str>,
        _taint: MemoryTaint,
    ) -> Result<(), MemoryError> {
        Ok(())
    }

    async fn get(&self, _namespace: &str, _key: &str) -> Result<Option<MemoryEntry>, MemoryError> {
        Ok(None)
    }

    async fn forget(&self, _namespace: &str, _key: &str) -> Result<bool, MemoryError> {
        Ok(false)
    }

    async fn list(
        &self,
        _namespace: Option<&str>,
        _category: Option<&MemoryCategory>,
        _session_id: Option<&str>,
    ) -> Result<Vec<MemoryEntry>, MemoryError> {
        Ok(Vec::new())
    }

    async fn namespaces(&self) -> Result<Vec<NamespaceSummary>, MemoryError> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl MemoryRecall for FixedRecallProvider {
    async fn recall(
        &self,
        _query: &str,
        _limit: usize,
        _opts: &OwnedRecallOpts,
        _scope: Option<&SourceScope>,
    ) -> Result<Vec<MemoryEntry>, MemoryError> {
        Ok(self.entries.clone())
    }
}

#[async_trait]
impl MemoryPortability for FixedRecallProvider {
    async fn export_page(
        &self,
        _cursor: Option<&str>,
        _limit: usize,
    ) -> Result<ExportPage, MemoryError> {
        Err(MemoryError::Other(anyhow::anyhow!(
            "FixedRecallProvider does not implement export"
        )))
    }

    async fn import_records(
        &self,
        _records: Vec<ExportRecord>,
    ) -> Result<ImportOutcome, MemoryError> {
        Err(MemoryError::Other(anyhow::anyhow!(
            "FixedRecallProvider does not implement import"
        )))
    }
}

#[async_trait]
impl MemoryProvider for FixedRecallProvider {
    fn driver_id(&self) -> &str {
        "fixed-recall"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::mandatory()
    }

    async fn health(&self) -> MemoryHealth {
        MemoryHealth::Ready
    }
}

/// An in-memory provider that stores **chunks**, for round trips that span
/// ingest and chunk listing.
///
/// # Why this is not [`InMemoryProvider`]
///
/// That one stores `MemoryEntry` behind the mandatory three and says why it
/// stops there: "advertising more would fail `audit_provider`, since no
/// optional accessor is overridden". The chunk handlers need two optional
/// families instead — [`MemoryIngest`] to write and [`MemoryChunks`] to read —
/// and a round trip is only meaningful when the same object serves both, so
/// they live together here rather than as two providers a test would have to
/// keep in sync.
///
/// # What it does not pretend to be
///
/// Ingest stores the item as **one** chunk. The real pipeline splits on token
/// budget, scores entities, and derives a deterministic id from
/// `(source_kind, source_id, seq_in_source, content)`. This is enough for "the
/// write reached the driver and the read brought it back through the guard",
/// which is what a handler test asserts; it is **not** enough to test chunking,
/// and an assertion about chunk counts or ids belongs against
/// `ingest_pipeline` where that logic actually lives.
///
/// Dedupe is by `source_id`, matching the pipeline's `already_ingested`
/// behaviour, because the idempotency handler test turns on exactly that.
#[derive(Default)]
pub struct InMemoryChunks {
    chunks: Mutex<Vec<Chunk>>,
}

impl InMemoryChunks {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many chunks are stored, for assertions that bypass the guard.
    #[must_use]
    pub fn len(&self) -> usize {
        self.chunks.lock().len()
    }

    /// Whether the store holds nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.chunks.lock().is_empty()
    }
}

#[async_trait]
impl MemoryCore for InMemoryChunks {
    async fn store(
        &self,
        _namespace: &str,
        _key: &str,
        _content: &str,
        _category: MemoryCategory,
        _session_id: Option<&str>,
        _taint: MemoryTaint,
    ) -> Result<(), MemoryError> {
        Err(MemoryError::Other(anyhow::anyhow!(
            "InMemoryChunks stores chunks, not entries"
        )))
    }

    async fn get(&self, _namespace: &str, _key: &str) -> Result<Option<MemoryEntry>, MemoryError> {
        Ok(None)
    }

    async fn forget(&self, _namespace: &str, _key: &str) -> Result<bool, MemoryError> {
        Ok(false)
    }

    async fn list(
        &self,
        _namespace: Option<&str>,
        _category: Option<&MemoryCategory>,
        _session_id: Option<&str>,
    ) -> Result<Vec<MemoryEntry>, MemoryError> {
        Ok(Vec::new())
    }

    async fn namespaces(&self) -> Result<Vec<NamespaceSummary>, MemoryError> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl MemoryIngest for InMemoryChunks {
    async fn ingest_document(&self, item: IngestItem) -> Result<IngestOutcome, MemoryError> {
        let mut chunks = self.chunks.lock();
        if chunks
            .iter()
            .any(|chunk| chunk.metadata.source_id == item.source_id)
        {
            // Matches the pipeline's `already_ingested` arm, which the adapter
            // in `TinycortexProvider::ingest_document` reports as `skipped: 1`.
            return Ok(IngestOutcome {
                written: 0,
                skipped: 1,
                ids: Vec::new(),
            });
        }
        let timestamp = item.timestamp.unwrap_or_else(chrono::Utc::now);
        let id = format!("{}::0", item.source_id);
        let chunk = Chunk {
            id: id.clone(),
            content: item.content,
            metadata: Metadata {
                source_kind: SourceKind::Document,
                source_id: item.source_id,
                owner: item.owner,
                timestamp,
                time_range: (timestamp, timestamp),
                tags: item.tags,
                source_ref: item.source_ref,
                path_scope: item.path_scope,
            },
            token_count: 0,
            seq_in_source: 0,
            created_at: timestamp,
            partial_message: false,
        };
        chunks.push(chunk);
        Ok(IngestOutcome {
            written: 1,
            skipped: 0,
            ids: vec![id],
        })
    }

    async fn ingest_chat(&self, messages: Vec<IngestItem>) -> Result<IngestOutcome, MemoryError> {
        let mut outcome = IngestOutcome::default();
        for message in messages {
            let one = self.ingest_document(message).await?;
            outcome.written += one.written;
            outcome.skipped += one.skipped;
            outcome.ids.extend(one.ids);
        }
        Ok(outcome)
    }
}

#[async_trait]
impl MemoryChunks for InMemoryChunks {
    async fn list_chunks(
        &self,
        query: &ChunkQuery,
        scope: Option<&SourceScope>,
    ) -> Result<Vec<Chunk>, MemoryError> {
        let chunks = self.chunks.lock();
        let mut rows: Vec<Chunk> = chunks
            .iter()
            .filter(|chunk| {
                let m = &chunk.metadata;
                query.source_kind.is_none_or(|kind| kind == m.source_kind)
                    && query.source_id.as_ref().is_none_or(|id| *id == m.source_id)
                    && query.owner.as_ref().is_none_or(|owner| *owner == m.owner)
                    && query
                        .since_ms
                        .is_none_or(|since| m.timestamp.timestamp_millis() >= since)
                    && query
                        .until_ms
                        .is_none_or(|until| m.timestamp.timestamp_millis() <= until)
            })
            // Scope is applied before the limit, as the trait requires: a
            // disallowed source must not starve permitted ones out of the page.
            .filter(|chunk| scope.is_none_or(|s| s.allows_source_id(&chunk.metadata.source_id)))
            .cloned()
            .collect();
        rows.sort_by_key(|chunk| std::cmp::Reverse(chunk.created_at));
        if let Some(offset) = query.offset {
            rows = rows.into_iter().skip(offset).collect();
        }
        if let Some(limit) = query.limit {
            rows.truncate(limit);
        }
        Ok(rows)
    }

    async fn get_chunk(&self, chunk_id: &str) -> Result<Option<Chunk>, MemoryError> {
        Ok(self
            .chunks
            .lock()
            .iter()
            .find(|chunk| chunk.id == chunk_id)
            .cloned())
    }

    async fn chunk_detail(&self, _chunk_id: &str) -> Result<Option<ChunkDetail>, MemoryError> {
        Err(MemoryError::Other(anyhow::anyhow!(
            "InMemoryChunks does not store chunk detail"
        )))
    }

    async fn storage_kinds(&self) -> Result<Vec<String>, MemoryError> {
        Ok(vec!["document".to_string()])
    }

    async fn chunk_embeddings(
        &self,
        _chunk_ids: &[String],
        _model_signature: &str,
    ) -> Result<Vec<ChunkEmbedding>, MemoryError> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl MemoryRecall for InMemoryChunks {
    async fn recall(
        &self,
        _query: &str,
        _limit: usize,
        _opts: &OwnedRecallOpts,
        _scope: Option<&SourceScope>,
    ) -> Result<Vec<MemoryEntry>, MemoryError> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl MemoryPortability for InMemoryChunks {
    async fn export_page(
        &self,
        _cursor: Option<&str>,
        _limit: usize,
    ) -> Result<ExportPage, MemoryError> {
        Err(MemoryError::Other(anyhow::anyhow!(
            "InMemoryChunks does not implement export"
        )))
    }

    async fn import_records(
        &self,
        _records: Vec<ExportRecord>,
    ) -> Result<ImportOutcome, MemoryError> {
        Err(MemoryError::Other(anyhow::anyhow!(
            "InMemoryChunks does not implement import"
        )))
    }
}

#[async_trait]
impl MemoryProvider for InMemoryChunks {
    fn driver_id(&self) -> &str {
        "in-memory-chunks"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::mandatory()
            .with(Capability::Chunks)
            .with(Capability::Ingest)
    }

    async fn health(&self) -> MemoryHealth {
        MemoryHealth::Ready
    }

    fn as_chunks(&self) -> Option<&dyn MemoryChunks> {
        Some(self)
    }

    fn as_ingest(&self) -> Option<&dyn MemoryIngest> {
        Some(self)
    }
}

/// A real [`MemoryGuard`](super::MemoryGuard) over a fresh chunk store, plus
/// the store itself for assertions that bypass the guard.
#[must_use]
pub fn guarded_in_memory_chunks() -> (Arc<InMemoryChunks>, Arc<super::MemoryGuard>) {
    let provider = Arc::new(InMemoryChunks::new());
    let guard = guard_over(Arc::clone(&provider) as Arc<dyn MemoryProvider>);
    (provider, guard)
}

#[cfg(test)]
mod in_memory_chunks_tests {
    use super::*;
    use crate::openhuman::memory::api::chunks::DataSource;

    fn document(source_id: &str, owner: &str, content: &str) -> IngestItem {
        IngestItem {
            namespace: None,
            source: DataSource::Upload,
            source_id: source_id.to_string(),
            owner: owner.to_string(),
            source_ref: None,
            content: content.to_string(),
            mime: None,
            timestamp: None,
            tags: Vec::new(),
            taint: MemoryTaint::default(),
            path_scope: None,
        }
    }

    /// The point of this provider: a write and a read that both cross a real
    /// [`MemoryGuard`] reach the same store. `InMemoryProvider` cannot serve
    /// this — `as_chunks()` is `None` there, so the guard answers `Unsupported`.
    #[tokio::test]
    async fn a_document_written_through_the_guard_is_read_back_through_it() {
        let (store, guard) = guarded_in_memory_chunks();

        let outcome = guard
            .as_ingest()
            .expect("guard exposes ingest")
            .ingest_document(document("doc-launch", "alice", "Phoenix launch canary"))
            .await
            .expect("ingest");
        assert_eq!(outcome.written, 1);
        assert_eq!(store.len(), 1);

        let listed = guard
            .as_chunks()
            .expect("guard exposes chunks")
            .list_chunks(
                &ChunkQuery {
                    source_id: Some("doc-launch".to_string()),
                    ..ChunkQuery::default()
                },
                None,
            )
            .await
            .expect("list_chunks");
        assert_eq!(listed.len(), 1);
        assert!(listed[0].content.contains("Phoenix launch canary"));

        let fetched = guard
            .as_chunks()
            .expect("guard exposes chunks")
            .get_chunk(&outcome.ids[0])
            .await
            .expect("get_chunk")
            .expect("chunk exists");
        assert_eq!(fetched.metadata.owner, "alice");
    }

    /// Dedupe is by `source_id`, mirroring the pipeline's `already_ingested`
    /// arm that `TinycortexProvider::ingest_document` maps to `skipped: 1`.
    #[tokio::test]
    async fn a_repeated_source_id_is_skipped_rather_than_duplicated() {
        let (store, guard) = guarded_in_memory_chunks();
        let ingest = guard.as_ingest().expect("guard exposes ingest");

        ingest
            .ingest_document(document("doc-launch", "alice", "first"))
            .await
            .expect("first ingest");
        let second = ingest
            .ingest_document(document("doc-launch", "alice", "second"))
            .await
            .expect("second ingest");

        assert_eq!(second.written, 0);
        assert_eq!(second.skipped, 1);
        assert_eq!(store.len(), 1, "the duplicate must not add a chunk");
    }

    /// The trait requires scope to be applied *before* the limit, so a
    /// forbidden source cannot starve permitted ones out of the page.
    #[tokio::test]
    async fn a_scope_excludes_sources_outside_it() {
        let (_store, guard) = guarded_in_memory_chunks();
        let ingest = guard.as_ingest().expect("guard exposes ingest");
        ingest
            .ingest_document(document("allowed", "alice", "in scope"))
            .await
            .expect("ingest allowed");
        ingest
            .ingest_document(document("denied", "alice", "out of scope"))
            .await
            .expect("ingest denied");

        let scope = SourceScope::new(["allowed"]);
        let listed = guard
            .as_chunks()
            .expect("guard exposes chunks")
            .list_chunks(&ChunkQuery::default(), Some(&scope))
            .await
            .expect("list_chunks");

        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].metadata.source_id, "allowed");
    }
}

//! The ten optional-family decorators — the load-bearing half of the guard.
//!
//! ## Why these exist at all
//!
//! [`MemoryProvider::as_tree`] and its nine siblings return a **borrow** of a
//! family trait object. If [`MemoryGuard`]'s override simply forwarded
//! `self.inner.as_tree()`, every caller that reached memory through a family
//! accessor would hold a raw, unguarded driver handle — and the guard's whole
//! reason to exist ("the only handle product code receives") would be
//! bypassable by one method call. Nine of the thirteen families are *only*
//! reachable that way.
//!
//! So each family gets its own decorator, and the accessor hands back a borrow
//! of that. Because the accessor returns a reference, the decorators cannot be
//! constructed on demand inside it — a reference to a temporary does not
//! outlive the call — so they are **fields on the guard, built once at
//! construction**. That is also what makes their presence mirror the inner
//! driver's exactly: a field exists iff `inner.provides(...)` said so, which is
//! what keeps `audit_provider` happy.
//!
//! ## Why each decorator holds the provider, not the family
//!
//! A `GuardedTree { inner: &dyn MemoryTree }` borrowed out of an
//! `Arc<dyn MemoryProvider>` the same struct owns is self-referential, and Rust
//! has no way to express that without unsafe pinning. Holding
//! `Arc<dyn MemoryProvider>` and re-deriving the family per call sidesteps it
//! entirely, at the cost of one `Option` unwrap that is structurally
//! unreachable — see [`family`](GuardedTree::family).
//!
//! [`MemoryGuard`]: super::MemoryGuard

use std::sync::Arc;

use crate::openhuman::memory::api::capabilities::Capability;
use crate::openhuman::memory::api::chunks::Chunk;
use crate::openhuman::memory::api::error::MemoryError;
use crate::openhuman::memory::api::goals::GoalsDoc;
use crate::openhuman::memory::api::provider::chunks::{
    ChunkDetail, ChunkEmbedding, ChunkListRow, ChunkQuery, MemoryChunks, SourceTotal,
};
use crate::openhuman::memory::api::provider::episodic::{
    ConversationSegment, EpisodicTurn, MemoryEpisodic,
};
use crate::openhuman::memory::api::provider::people::{
    AddressBookSeedOutcome, MemoryPeople, PersonHandle, PersonInteraction, PersonRecord,
    PersonScore, RankedPerson, ResolvedPerson,
};
use crate::openhuman::memory::api::provider::profile::{
    FacetType, MemoryProfile, ProfileFacet, UserState,
};
use crate::openhuman::memory::api::provider::retrieval::{
    CoverWindowQuery, EntityMatch, FastRetrieveQuery, MemoryRetrieval, RetrievalHit,
    RetrievalResponse, SourceRetrievalQuery,
};
use crate::openhuman::memory::api::provider::scoring::MemoryScoring;
use crate::openhuman::memory::api::provider::sessions::{
    CodingSessionIngestReport, CodingSessionIngestRequest, CodingSessionSource,
};
use crate::openhuman::memory::api::provider::sync::{
    RawArchiveCoverage, RawRebuildOutcome, SourceSyncState, SourceSyncStatus, SyncAuditEntry,
    SyncRunOutcome,
};
use crate::openhuman::memory::api::provider::types::{
    ChunkEntityOccurrence, DiffReport, EntityHit, EntityOccurrence, ForgetOutcome, ForgetSelector,
    IngestItem, IngestOutcome, MaintenanceReport, PurgeOutcome, SnapshotRef, SourceItem,
    SourceScope,
};
use crate::openhuman::memory::api::provider::{
    EpisodicEvent, MemoryCodingSessions, MemoryDiff, MemoryDocuments, MemoryEntities, MemoryGoals,
    MemoryGraph, MemoryIngest, MemoryMaintenance, MemoryProvider, MemorySourceSink,
    MemorySourceSync, MemoryToolMemory, MemoryTree,
};
use crate::openhuman::memory::api::tool_memory::ToolMemoryRule;
use crate::openhuman::memory::api::tree::{
    IngestRequest, QueryResult, SummaryForest, TreeLeaf, TreeStatus,
};
use crate::openhuman::memory::api::types::NamespaceMemoryHit;
use crate::openhuman::memory::api::types::{
    GraphRelationRecord, MemoryKvRecord, MemoryTaint, NamespaceDocumentInput,
    NamespaceRetrievalContext, StoredMemoryDocument,
};
use async_trait::async_trait;

use super::audit::{trace_allowed, NO_NAMESPACE};
use super::policy::GuardPolicy;

/// Declares one decorator: the two shared fields, a constructor, and the
/// `family()` re-derivation.
macro_rules! decorator {
    ($(#[$meta:meta])* $name:ident, $fam:ty, $accessor:ident, $cap:ident) => {
        $(#[$meta])*
        pub struct $name {
            inner: Arc<dyn MemoryProvider>,
            policy: Arc<GuardPolicy>,
        }

        impl $name {
            pub(super) fn new(inner: Arc<dyn MemoryProvider>, policy: Arc<GuardPolicy>) -> Self {
                Self { inner, policy }
            }

            /// The underlying family handle.
            ///
            /// The `Err` arm is **structurally unreachable**: `MemoryGuard::new`
            /// only builds this decorator when the inner provider answered
            /// `provides(Capability::$cap)`, and the contract documents the
            /// capability set as fixed at bind time. It is written as a real
            /// error rather than `.expect(...)` because a panic inside a memory
            /// call is a strictly worse failure than an `Unsupported` a caller
            /// can already handle.
            fn family(&self) -> Result<&$fam, MemoryError> {
                self.inner
                    .$accessor()
                    .ok_or_else(|| MemoryError::unsupported(Capability::$cap))
            }
        }
    };
}

decorator!(
    /// Guarded [`MemoryIngest`].
    GuardedIngest,
    dyn MemoryIngest,
    as_ingest,
    Ingest
);
decorator!(
    /// Guarded [`MemoryDocuments`].
    GuardedDocuments,
    dyn MemoryDocuments,
    as_documents,
    Documents
);
decorator!(
    /// Guarded [`MemoryTree`] — the one family that carries step 2.
    GuardedTree,
    dyn MemoryTree,
    as_tree,
    Tree
);
decorator!(
    /// Guarded [`MemoryEntities`].
    GuardedEntities,
    dyn MemoryEntities,
    as_entities,
    Entities
);
decorator!(
    /// Guarded [`MemoryGraph`].
    GuardedGraph,
    dyn MemoryGraph,
    as_graph,
    Graph
);
decorator!(
    /// Guarded [`MemoryDiff`].
    GuardedDiff,
    dyn MemoryDiff,
    as_diff,
    Diff
);
decorator!(
    /// Guarded [`MemoryGoals`].
    GuardedGoals,
    dyn MemoryGoals,
    as_goals,
    Goals
);
decorator!(
    /// Guarded [`MemoryToolMemory`].
    GuardedToolMemory,
    dyn MemoryToolMemory,
    as_tool_memory,
    ToolMemory
);
decorator!(
    /// Guarded [`MemorySourceSink`].
    GuardedSources,
    dyn MemorySourceSink,
    as_sources,
    Sources
);
decorator!(
    /// Guarded [`MemoryMaintenance`].
    GuardedMaintenance,
    dyn MemoryMaintenance,
    as_maintenance,
    Maintenance
);
decorator!(
    /// Guarded [`MemoryPeople`].
    GuardedPeople,
    dyn MemoryPeople,
    as_people,
    People
);
decorator!(
    /// Guarded [`MemoryChunks`].
    GuardedChunks,
    dyn MemoryChunks,
    as_chunks,
    Chunks
);
decorator!(
    /// Guarded [`MemoryRetrieval`].
    GuardedRetrieval,
    dyn MemoryRetrieval,
    as_retrieval,
    Retrieval
);
decorator!(
    /// Guarded [`MemoryEpisodic`].
    GuardedEpisodic,
    dyn MemoryEpisodic,
    as_episodic,
    Episodic
);
decorator!(
    /// Guarded [`MemorySourceSync`].
    GuardedSourceSync,
    dyn MemorySourceSync,
    as_source_sync,
    SourceSync
);
decorator!(
    /// Guarded [`MemoryCodingSessions`].
    GuardedCodingSessions,
    dyn MemoryCodingSessions,
    as_coding_sessions,
    CodingSessions
);
decorator!(
    /// Guarded [`MemoryProfile`].
    GuardedProfile,
    dyn MemoryProfile,
    as_profile,
    Profile
);
decorator!(
    /// Guarded [`MemoryScoring`].
    GuardedScoring,
    dyn MemoryScoring,
    as_scoring,
    Scoring
);

// ── Ingest ───────────────────────────────────────────────────────────────────

impl GuardedIngest {
    /// Steps 3 + 4 over one ingest item: stamp provenance, redact on egress.
    fn admit(&self, mut item: IngestItem) -> IngestItem {
        item.taint = self.policy.stamp_taint(item.taint);
        item.content = self.policy.redact_outbound(&item.content).into_owned();
        item
    }
}

#[async_trait]
impl MemoryIngest for GuardedIngest {
    async fn ingest_document(&self, item: IngestItem) -> Result<IngestOutcome, MemoryError> {
        let namespace = item.namespace.clone().unwrap_or_else(|| "-".to_string());
        self.policy.admit_write(
            Capability::Ingest,
            "ingest.ingest_document",
            &namespace,
            true,
        )?;
        let item = self.admit(item);
        trace_allowed(
            &self.policy,
            "ingest.ingest_document",
            &namespace,
            item.content.chars().count(),
        );
        self.family()?.ingest_document(item).await
    }

    async fn ingest_chat(&self, messages: Vec<IngestItem>) -> Result<IngestOutcome, MemoryError> {
        self.policy
            .admit_write(Capability::Ingest, "ingest.ingest_chat", NO_NAMESPACE, true)?;
        let messages: Vec<IngestItem> = messages.into_iter().map(|m| self.admit(m)).collect();
        trace_allowed(
            &self.policy,
            "ingest.ingest_chat",
            NO_NAMESPACE,
            messages.iter().map(|m| m.content.chars().count()).sum(),
        );
        self.family()?.ingest_chat(messages).await
    }

    async fn ingest_email(&self, messages: Vec<IngestItem>) -> Result<IngestOutcome, MemoryError> {
        // Admitted exactly like chat: a thread is one conversation with no
        // namespace of its own, and every message is taint-stamped and
        // redacted before it reaches the driver.
        self.policy.admit_write(
            Capability::Ingest,
            "ingest.ingest_email",
            NO_NAMESPACE,
            true,
        )?;
        let messages: Vec<IngestItem> = messages.into_iter().map(|m| self.admit(m)).collect();
        trace_allowed(
            &self.policy,
            "ingest.ingest_email",
            NO_NAMESPACE,
            messages.iter().map(|m| m.content.chars().count()).sum(),
        );
        self.family()?.ingest_email(messages).await
    }
}

// ── Documents ────────────────────────────────────────────────────────────────

#[async_trait]
impl MemoryDocuments for GuardedDocuments {
    async fn put_document(&self, mut input: NamespaceDocumentInput) -> Result<String, MemoryError> {
        self.policy.admit_write(
            Capability::Documents,
            "documents.put_document",
            &input.namespace,
            true,
        )?;
        input.taint = self.policy.stamp_taint(input.taint);
        input.title = self.policy.redact_outbound(&input.title).into_owned();
        input.content = self.policy.redact_outbound(&input.content).into_owned();
        input.metadata = self.policy.redact_outbound_json(input.metadata);
        trace_allowed(
            &self.policy,
            "documents.put_document",
            &input.namespace,
            input.content.chars().count(),
        );
        self.family()?.put_document(input).await
    }

    async fn get_document(
        &self,
        namespace: &str,
        key: &str,
    ) -> Result<Option<StoredMemoryDocument>, MemoryError> {
        self.policy.admit_read(
            Capability::Documents,
            "documents.get_document",
            namespace,
            false,
        )?;
        self.family()?.get_document(namespace, key).await
    }

    async fn list_documents(
        &self,
        namespace: Option<&str>,
    ) -> Result<serde_json::Value, MemoryError> {
        self.policy.admit_read(
            Capability::Documents,
            "documents.list_documents",
            namespace.unwrap_or(NO_NAMESPACE),
            false,
        )?;
        self.family()?.list_documents(namespace).await
    }

    async fn list_namespaces(&self) -> Result<Vec<String>, MemoryError> {
        self.policy.admit_read(
            Capability::Documents,
            "documents.list_namespaces",
            NO_NAMESPACE,
            false,
        )?;
        self.family()?.list_namespaces().await
    }

    async fn delete_document(
        &self,
        namespace: &str,
        document_id: &str,
    ) -> Result<serde_json::Value, MemoryError> {
        self.policy.admit_write(
            Capability::Documents,
            "documents.delete_document",
            namespace,
            false,
        )?;
        self.family()?.delete_document(namespace, document_id).await
    }

    async fn clear_namespace(&self, namespace: &str) -> Result<(), MemoryError> {
        self.policy.admit_write(
            Capability::Documents,
            "documents.clear_namespace",
            namespace,
            false,
        )?;
        self.family()?.clear_namespace(namespace).await
    }

    async fn query_documents(
        &self,
        namespace: &str,
        query: &str,
        limit: usize,
    ) -> Result<NamespaceRetrievalContext, MemoryError> {
        // The query text itself crosses the boundary on an external driver.
        self.policy.admit_read(
            Capability::Documents,
            "documents.query_documents",
            namespace,
            true,
        )?;
        let query = self.policy.redact_outbound(query).into_owned();
        self.family()?
            .query_documents(namespace, &query, limit)
            .await
    }

    async fn recall_documents(
        &self,
        namespace: &str,
        limit: usize,
    ) -> Result<NamespaceRetrievalContext, MemoryError> {
        self.policy.admit_read(
            Capability::Documents,
            "documents.recall_documents",
            namespace,
            false,
        )?;
        self.family()?.recall_documents(namespace, limit).await
    }
}

// ── Tree ─────────────────────────────────────────────────────────────────────

#[async_trait]
impl MemoryTree for GuardedTree {
    async fn append(&self, mut request: IngestRequest) -> Result<(), MemoryError> {
        self.policy
            .admit_write(Capability::Tree, "tree.append", &request.namespace, true)?;
        request.content = self.policy.redact_outbound(&request.content).into_owned();
        trace_allowed(
            &self.policy,
            "tree.append",
            &request.namespace,
            request.content.chars().count(),
        );
        self.family()?.append(request).await
    }

    /// **Step 2 lives here.** This is the only contract method in the tree
    /// today that both takes a [`SourceScope`] and applies it as a real query
    /// predicate: the embedded driver pushes `scope.allow` into
    /// `ListChunksQuery.source_scope`, which reaches SQL *before* `LIMIT`.
    ///
    /// The ambient allowlist
    /// ([`source_scope::current_source_scope`](crate::openhuman::memory::source_scope::current_source_scope))
    /// is therefore read at this boundary and passed down, rather than being
    /// applied to the returned rows. An explicit `scope` argument may only
    /// *narrow* it: the two are intersected by
    /// [`GuardPolicy::narrow_scope`](crate::openhuman::memory::guard::GuardPolicy::narrow_scope),
    /// so a caller that computed a tighter scope than the task-local still wins,
    /// while one that names a collection outside the ambient allowlist cannot
    /// widen the turn back out.
    ///
    /// There is **no double application**: the embedded `query_source` does not
    /// itself read the task-local (only the deeper `tree::retrieval` and
    /// `list_chunks` paths do, and the guard does not sit in front of those),
    /// so this fills a predicate that would otherwise be `None`.
    async fn query_source(
        &self,
        namespace: &str,
        source_id: &str,
        limit: usize,
        scope: Option<&SourceScope>,
    ) -> Result<Vec<Chunk>, MemoryError> {
        self.policy
            .admit_read(Capability::Tree, "tree.query_source", namespace, false)?;
        let ambient = self.policy.ambient_scope();
        let effective = self.policy.narrow_scope(scope);
        log::debug!(
            "[memory:guard] tree.query_source namespace={namespace} limit={limit} \
             scoped={} scope_from={}",
            effective.is_some(),
            match (scope.is_some(), ambient.is_some()) {
                (true, true) => "argument∩ambient",
                (true, false) => "argument",
                (false, true) => "ambient",
                (false, false) => "none",
            }
        );
        self.family()?
            .query_source(namespace, source_id, limit, effective.as_ref())
            .await
    }

    async fn drill_down(&self, namespace: &str, node_id: &str) -> Result<QueryResult, MemoryError> {
        self.policy
            .admit_read(Capability::Tree, "tree.drill_down", namespace, false)?;
        self.family()?.drill_down(namespace, node_id).await
    }

    async fn seal(&self, namespace: &str) -> Result<TreeStatus, MemoryError> {
        self.policy
            .admit_write(Capability::Tree, "tree.seal", namespace, false)?;
        self.family()?.seal(namespace).await
    }

    async fn cascade(&self, namespace: &str) -> Result<TreeStatus, MemoryError> {
        self.policy
            .admit_write(Capability::Tree, "tree.cascade", namespace, false)?;
        self.family()?.cascade(namespace).await
    }

    /// Enumerates the sealed forest, so it narrows by the ambient scope for the
    /// same reason [`Self::query_source`] does: a summary is derived from the
    /// chunks beneath it, and handing back a node built from sources the caller
    /// may not read discloses their contents in condensed form.
    async fn summary_forest(
        &self,
        limit: usize,
        scope: Option<&SourceScope>,
    ) -> Result<SummaryForest, MemoryError> {
        self.policy
            .admit_read(Capability::Tree, "tree.summary_forest", NO_NAMESPACE, false)?;
        let effective = self.policy.narrow_scope(scope);
        self.family()?
            .summary_forest(limit, effective.as_ref())
            .await
    }

    /// Sealing one source's tree is a write, and it names the scope it acts on
    /// — so unlike the reads above it is admitted against that scope rather
    /// than `NO_NAMESPACE`. `carries_content: false`: the caller supplies a
    /// scope label, never prose, and the seals it fires write content the
    /// driver already holds.
    async fn flush_source_tree(&self, source_scope: &str) -> Result<u64, MemoryError> {
        self.policy.admit_write(
            Capability::Tree,
            "tree.flush_source_tree",
            source_scope,
            false,
        )?;
        self.family()?.flush_source_tree(source_scope).await
    }

    /// Leaves are chunks, so this is the same disclosure as
    /// [`Self::query_source`] with a different ordering, and takes the same
    /// intersection.
    async fn recent_leaves(
        &self,
        limit: usize,
        scope: Option<&SourceScope>,
    ) -> Result<Vec<TreeLeaf>, MemoryError> {
        self.policy
            .admit_read(Capability::Tree, "tree.recent_leaves", NO_NAMESPACE, false)?;
        let effective = self.policy.narrow_scope(scope);
        self.family()?
            .recent_leaves(limit, effective.as_ref())
            .await
    }
}

// ── Entities ─────────────────────────────────────────────────────────────────

#[async_trait]
impl MemoryEntities for GuardedEntities {
    async fn entities(
        &self,
        namespace: &str,
        query: Option<&str>,
        limit: usize,
    ) -> Result<Vec<EntityHit>, MemoryError> {
        self.policy.admit_read(
            Capability::Entities,
            "entities.entities",
            namespace,
            query.is_some(),
        )?;
        let redacted = query.map(|q| self.policy.redact_outbound(q).into_owned());
        self.family()?
            .entities(namespace, redacted.as_deref(), limit)
            .await
    }

    async fn entity_edges(
        &self,
        namespace: &str,
        entity_id: &str,
        limit: usize,
    ) -> Result<Vec<GraphRelationRecord>, MemoryError> {
        self.policy.admit_read(
            Capability::Entities,
            "entities.entity_edges",
            namespace,
            false,
        )?;
        self.family()?
            .entity_edges(namespace, entity_id, limit)
            .await
    }

    async fn touch_entities(
        &self,
        namespace: &str,
        entity_ids: &[String],
    ) -> Result<(), MemoryError> {
        self.policy.admit_write(
            Capability::Entities,
            "entities.touch_entities",
            namespace,
            false,
        )?;
        self.family()?.touch_entities(namespace, entity_ids).await
    }

    /// The occurrence index has no namespace and the contract gives this member
    /// no scope argument, so there is nothing to intersect — the tier check is
    /// the whole gate. Worth stating rather than leaving as an apparent
    /// omission beside the scoped members above.
    async fn top_entities(
        &self,
        kind: Option<&str>,
        limit: usize,
    ) -> Result<Vec<EntityOccurrence>, MemoryError> {
        self.policy.admit_read(
            Capability::Entities,
            "entities.top_entities",
            NO_NAMESPACE,
            false,
        )?;
        self.family()?.top_entities(kind, limit).await
    }

    /// Scoped by the chunk ids the caller already holds: it can only name
    /// chunks a previous, scoped read handed it, so this adds no reach beyond
    /// the read that produced them.
    async fn chunk_entities(
        &self,
        chunk_ids: &[String],
        kinds: Option<&[String]>,
    ) -> Result<Vec<ChunkEntityOccurrence>, MemoryError> {
        self.policy.admit_read(
            Capability::Entities,
            "entities.chunk_entities",
            NO_NAMESPACE,
            false,
        )?;
        self.family()?.chunk_entities(chunk_ids, kinds).await
    }

    /// Returns ids only, never content. A caller still has to read those chunks
    /// through [`MemoryChunks`] to see anything, and that path applies the
    /// scope intersection.
    async fn entity_chunk_ids(
        &self,
        entity_id: &str,
        limit: usize,
    ) -> Result<Vec<String>, MemoryError> {
        self.policy.admit_read(
            Capability::Entities,
            "entities.entity_chunk_ids",
            NO_NAMESPACE,
            false,
        )?;
        self.family()?.entity_chunk_ids(entity_id, limit).await
    }
}

// ── Graph ────────────────────────────────────────────────────────────────────

/// Namespace label for the graph family's `Option<&str>` namespace — `None`
/// addresses the global, namespace-less slice.
fn graph_ns(namespace: Option<&str>) -> &str {
    namespace.unwrap_or(NO_NAMESPACE)
}

#[async_trait]
impl MemoryGraph for GuardedGraph {
    async fn kv_get(
        &self,
        namespace: Option<&str>,
        key: &str,
    ) -> Result<Option<MemoryKvRecord>, MemoryError> {
        self.policy.admit_read(
            Capability::Graph,
            "graph.kv_get",
            graph_ns(namespace),
            false,
        )?;
        self.family()?.kv_get(namespace, key).await
    }

    async fn kv_put(
        &self,
        namespace: Option<&str>,
        key: &str,
        value: serde_json::Value,
    ) -> Result<(), MemoryError> {
        self.policy
            .admit_write(Capability::Graph, "graph.kv_put", graph_ns(namespace), true)?;
        let value = self.policy.redact_outbound_json(value);
        self.family()?.kv_put(namespace, key, value).await
    }

    async fn kv_delete(&self, namespace: Option<&str>, key: &str) -> Result<bool, MemoryError> {
        self.policy.admit_write(
            Capability::Graph,
            "graph.kv_delete",
            graph_ns(namespace),
            false,
        )?;
        self.family()?.kv_delete(namespace, key).await
    }

    async fn kv_list(
        &self,
        namespace: Option<&str>,
        prefix: Option<&str>,
        limit: usize,
    ) -> Result<Vec<MemoryKvRecord>, MemoryError> {
        self.policy.admit_read(
            Capability::Graph,
            "graph.kv_list",
            graph_ns(namespace),
            false,
        )?;
        self.family()?.kv_list(namespace, prefix, limit).await
    }

    async fn relations(
        &self,
        namespace: Option<&str>,
        subject: Option<&str>,
        predicate: Option<&str>,
        limit: usize,
    ) -> Result<Vec<GraphRelationRecord>, MemoryError> {
        self.policy.admit_read(
            Capability::Graph,
            "graph.relations",
            graph_ns(namespace),
            false,
        )?;
        self.family()?
            .relations(namespace, subject, predicate, limit)
            .await
    }

    async fn put_relation(&self, relation: GraphRelationRecord) -> Result<(), MemoryError> {
        self.policy.admit_write(
            Capability::Graph,
            "graph.put_relation",
            graph_ns(relation.namespace.as_deref()),
            true,
        )?;
        self.family()?.put_relation(relation).await
    }
}

// ── Diff ─────────────────────────────────────────────────────────────────────

#[async_trait]
impl MemoryDiff for GuardedDiff {
    async fn capture_snapshot(&self, source_id: &str) -> Result<SnapshotRef, MemoryError> {
        self.policy.admit_write(
            Capability::Diff,
            "diff.capture_snapshot",
            NO_NAMESPACE,
            false,
        )?;
        self.family()?.capture_snapshot(source_id).await
    }

    async fn snapshots(
        &self,
        source_id: &str,
        limit: usize,
    ) -> Result<Vec<SnapshotRef>, MemoryError> {
        self.policy
            .admit_read(Capability::Diff, "diff.snapshots", NO_NAMESPACE, false)?;
        self.family()?.snapshots(source_id, limit).await
    }

    async fn diff(
        &self,
        source_id: &str,
        from: Option<&str>,
        to: &str,
    ) -> Result<DiffReport, MemoryError> {
        self.policy
            .admit_read(Capability::Diff, "diff.diff", NO_NAMESPACE, false)?;
        self.family()?.diff(source_id, from, to).await
    }
}

// ── Goals ────────────────────────────────────────────────────────────────────

#[async_trait]
impl MemoryGoals for GuardedGoals {
    async fn goals(&self) -> Result<GoalsDoc, MemoryError> {
        self.policy
            .admit_read(Capability::Goals, "goals.goals", NO_NAMESPACE, false)?;
        self.family()?.goals().await
    }

    async fn set_goals(&self, goals: GoalsDoc) -> Result<(), MemoryError> {
        self.policy
            .admit_write(Capability::Goals, "goals.set_goals", NO_NAMESPACE, true)?;
        // The goals document's own validating mutation surface (the PII and
        // secret predicates) is host policy that already runs in
        // `memory::goals` before a document reaches the contract, so the guard
        // does not re-scrub item text here. If an external driver ever binds,
        // M6 must decide whether that upstream scrub is sufficient for egress
        // or whether item bodies need the same `redact_outbound` treatment the
        // document and ingest paths get.
        self.family()?.set_goals(goals).await
    }
}

// ── Tool memory ──────────────────────────────────────────────────────────────

#[async_trait]
impl MemoryToolMemory for GuardedToolMemory {
    async fn tool_rules(&self, tool_name: &str) -> Result<Vec<ToolMemoryRule>, MemoryError> {
        self.policy.admit_read(
            Capability::ToolMemory,
            "tool_memory.tool_rules",
            NO_NAMESPACE,
            false,
        )?;
        self.family()?.tool_rules(tool_name).await
    }

    async fn put_tool_rule(&self, rule: ToolMemoryRule) -> Result<(), MemoryError> {
        self.policy.admit_write(
            Capability::ToolMemory,
            "tool_memory.put_tool_rule",
            NO_NAMESPACE,
            true,
        )?;
        self.family()?.put_tool_rule(rule).await
    }

    async fn delete_tool_rule(&self, tool_name: &str, rule_id: &str) -> Result<bool, MemoryError> {
        self.policy.admit_write(
            Capability::ToolMemory,
            "tool_memory.delete_tool_rule",
            NO_NAMESPACE,
            false,
        )?;
        self.family()?.delete_tool_rule(tool_name, rule_id).await
    }
}

// ── Sources ──────────────────────────────────────────────────────────────────

#[async_trait]
impl MemorySourceSink for GuardedSources {
    async fn accept_source_items(
        &self,
        source_id: &str,
        source_kind: &str,
        items: Vec<SourceItem>,
        taint: MemoryTaint,
    ) -> Result<IngestOutcome, MemoryError> {
        self.policy.admit_write(
            Capability::Sources,
            "sources.accept_source_items",
            NO_NAMESPACE,
            true,
        )?;
        // Step 3: the batch taint is the guard's to decide. `stamp_taint` never
        // downgrades, so a sync path that already asked for `ExternalSync` keeps
        // it whether or not a source scope is active.
        let taint = self.policy.stamp_taint(taint);
        let items: Vec<SourceItem> = items
            .into_iter()
            .map(|mut item| {
                item.title = self.policy.redact_outbound(&item.title).into_owned();
                item.content = self.policy.redact_outbound(&item.content).into_owned();
                item
            })
            .collect();
        trace_allowed(
            &self.policy,
            "sources.accept_source_items",
            NO_NAMESPACE,
            items.iter().map(|i| i.content.chars().count()).sum(),
        );
        self.family()?
            .accept_source_items(source_id, source_kind, items, taint)
            .await
    }

    async fn forget_source(&self, source_id: &str) -> Result<u64, MemoryError> {
        self.policy.admit_write(
            Capability::Sources,
            "sources.forget_source",
            NO_NAMESPACE,
            false,
        )?;
        self.family()?.forget_source(source_id).await
    }

    /// The one door for every scoped forget, so it takes the same write tier as
    /// [`Self::forget_source`]. The selector names what to remove rather than
    /// carrying content, which is why the egress flag is `false` — the same
    /// reading its single-source sibling makes.
    async fn forget_matching(
        &self,
        selector: &ForgetSelector,
    ) -> Result<ForgetOutcome, MemoryError> {
        self.policy.admit_write(
            Capability::Sources,
            "sources.forget_matching",
            NO_NAMESPACE,
            false,
        )?;
        self.family()?.forget_matching(selector).await
    }
}

// ── Maintenance ──────────────────────────────────────────────────────────────

#[async_trait]
impl MemoryMaintenance for GuardedMaintenance {
    async fn reembed(&self) -> Result<MaintenanceReport, MemoryError> {
        self.policy.admit_write(
            Capability::Maintenance,
            "maintenance.reembed",
            NO_NAMESPACE,
            false,
        )?;
        self.family()?.reembed().await
    }

    async fn compact(&self) -> Result<MaintenanceReport, MemoryError> {
        self.policy.admit_write(
            Capability::Maintenance,
            "maintenance.compact",
            NO_NAMESPACE,
            false,
        )?;
        self.family()?.compact().await
    }

    async fn consolidate(&self) -> Result<MaintenanceReport, MemoryError> {
        self.policy.admit_write(
            Capability::Maintenance,
            "maintenance.consolidate",
            NO_NAMESPACE,
            false,
        )?;
        self.family()?.consolidate().await
    }

    /// Read-only by contract, so this takes the **read** tier check: a
    /// `readonly` operator must still be able to run `doctor`, which is exactly
    /// the tier where diagnosing without mutating matters most.
    async fn doctor(&self) -> Result<MaintenanceReport, MemoryError> {
        self.policy.admit_read(
            Capability::Maintenance,
            "maintenance.doctor",
            NO_NAMESPACE,
            false,
        )?;
        self.family()?.doctor().await
    }

    /// Empties the whole store, so it takes the write tier rather than
    /// `doctor`'s read one — and deliberately carries no scope, because there
    /// is no scoped reading of "purge everything". A source-restricted caller
    /// that reached this would be destroying rows it is not even allowed to
    /// read; the write tier is what stops it.
    async fn purge_all(&self) -> Result<PurgeOutcome, MemoryError> {
        self.policy.admit_write(
            Capability::Maintenance,
            "maintenance.purge_all",
            NO_NAMESPACE,
            false,
        )?;
        self.family()?.purge_all().await
    }
}

// ── People ───────────────────────────────────────────────────────────────────

#[async_trait]
impl MemoryPeople for GuardedPeople {
    async fn list_people(&self, limit: Option<usize>) -> Result<Vec<RankedPerson>, MemoryError> {
        self.policy.admit_read(
            Capability::People,
            "people.list_people",
            NO_NAMESPACE,
            false,
        )?;
        self.family()?.list_people(limit).await
    }

    async fn get_person(&self, person_id: &str) -> Result<Option<PersonRecord>, MemoryError> {
        self.policy
            .admit_read(Capability::People, "people.get_person", NO_NAMESPACE, false)?;
        self.family()?.get_person(person_id).await
    }

    /// A read *unless* it may mint a person, which is a write.
    ///
    /// The tier check follows what the call can actually do rather than what it
    /// is named: with `create_if_missing` set this inserts a row, so a
    /// `readonly` operator must be refused. Classifying the whole method as a
    /// read would have handed `readonly` a working insert through the back
    /// door.
    async fn resolve_handle(
        &self,
        handle: &PersonHandle,
        create_if_missing: bool,
    ) -> Result<Option<ResolvedPerson>, MemoryError> {
        if create_if_missing {
            self.policy.admit_write(
                Capability::People,
                "people.resolve_handle",
                NO_NAMESPACE,
                true,
            )?;
        } else {
            self.policy.admit_read(
                Capability::People,
                "people.resolve_handle",
                NO_NAMESPACE,
                false,
            )?;
        }
        self.family()?
            .resolve_handle(handle, create_if_missing)
            .await
    }

    async fn add_handle_alias(
        &self,
        person_id: &str,
        handle: &PersonHandle,
    ) -> Result<(), MemoryError> {
        self.policy.admit_write(
            Capability::People,
            "people.add_handle_alias",
            NO_NAMESPACE,
            true,
        )?;
        self.family()?.add_handle_alias(person_id, handle).await
    }

    async fn score_person(&self, person_id: &str) -> Result<Option<PersonScore>, MemoryError> {
        self.policy.admit_read(
            Capability::People,
            "people.score_person",
            NO_NAMESPACE,
            false,
        )?;
        self.family()?.score_person(person_id).await
    }

    async fn record_interaction(&self, interaction: &PersonInteraction) -> Result<(), MemoryError> {
        self.policy.admit_write(
            Capability::People,
            "people.record_interaction",
            NO_NAMESPACE,
            true,
        )?;
        self.family()?.record_interaction(interaction).await
    }

    /// A write: it reads the platform address book and inserts what it finds.
    async fn seed_from_address_book(&self) -> Result<AddressBookSeedOutcome, MemoryError> {
        self.policy.admit_write(
            Capability::People,
            "people.seed_from_address_book",
            NO_NAMESPACE,
            true,
        )?;
        self.family()?.seed_from_address_book().await
    }
}

// ── Chunks ───────────────────────────────────────────────────────────────────

#[async_trait]
impl MemoryChunks for GuardedChunks {
    async fn list_chunks(
        &self,
        query: &ChunkQuery,
        scope: Option<&SourceScope>,
    ) -> Result<Vec<Chunk>, MemoryError> {
        self.policy.admit_read(
            Capability::Chunks,
            "chunks.list_chunks",
            NO_NAMESPACE,
            false,
        )?;
        // Intersected with the ambient allowlist, never passed through. The
        // ambient scope is an upper bound: forwarding the caller's scope
        // unchanged would let a source-restricted turn widen itself back out by
        // naming a collection the restriction excluded. See
        // `GuardPolicy::narrow_scope`.
        let effective = self.policy.narrow_scope(scope);
        self.family()?.list_chunks(query, effective.as_ref()).await
    }

    /// The count that labels a [`Self::list_chunks`] page, and it must be
    /// narrowed by exactly the same rule. A total computed against a wider
    /// scope than the page it labels leaks the existence of rows the caller may
    /// not read — "showing 20 of 4000" tells a source-restricted turn how much
    /// it is not being shown.
    async fn count_chunks(
        &self,
        query: &ChunkQuery,
        scope: Option<&SourceScope>,
    ) -> Result<u64, MemoryError> {
        self.policy.admit_read(
            Capability::Chunks,
            "chunks.count_chunks",
            NO_NAMESPACE,
            false,
        )?;
        let effective = self.policy.narrow_scope(scope);
        self.family()?.count_chunks(query, effective.as_ref()).await
    }

    /// Same rows as [`Self::list_chunks`] with the stored facts beside them, so
    /// the same intersection applies for the same reason.
    async fn list_chunk_details(
        &self,
        query: &ChunkQuery,
        scope: Option<&SourceScope>,
    ) -> Result<Vec<ChunkListRow>, MemoryError> {
        self.policy.admit_read(
            Capability::Chunks,
            "chunks.list_chunk_details",
            NO_NAMESPACE,
            false,
        )?;
        let effective = self.policy.narrow_scope(scope);
        self.family()?
            .list_chunk_details(query, effective.as_ref())
            .await
    }

    /// Per-source totals are computed from the chunks the scope admits, not
    /// filtered afterwards — so a restricted caller must not learn that a
    /// forbidden source exists by seeing its row, nor see a permitted source
    /// carrying a count that includes rows it cannot read.
    async fn source_totals(
        &self,
        limit: usize,
        scope: Option<&SourceScope>,
    ) -> Result<Vec<SourceTotal>, MemoryError> {
        self.policy.admit_read(
            Capability::Chunks,
            "chunks.source_totals",
            NO_NAMESPACE,
            false,
        )?;
        let effective = self.policy.narrow_scope(scope);
        self.family()?
            .source_totals(limit, effective.as_ref())
            .await
    }

    async fn get_chunk(&self, chunk_id: &str) -> Result<Option<Chunk>, MemoryError> {
        self.policy
            .admit_read(Capability::Chunks, "chunks.get_chunk", NO_NAMESPACE, false)?;
        self.family()?.get_chunk(chunk_id).await
    }

    async fn chunk_detail(&self, chunk_id: &str) -> Result<Option<ChunkDetail>, MemoryError> {
        self.policy.admit_read(
            Capability::Chunks,
            "chunks.chunk_detail",
            NO_NAMESPACE,
            false,
        )?;
        self.family()?.chunk_detail(chunk_id).await
    }

    /// The catalog is not user content, so it takes no namespace and the
    /// lightest read check — refusing it under `readonly` would stop an
    /// operator finding out what the store can even hold.
    async fn storage_kinds(&self) -> Result<Vec<String>, MemoryError> {
        self.policy.admit_read(
            Capability::Chunks,
            "chunks.storage_kinds",
            NO_NAMESPACE,
            false,
        )?;
        self.family()?.storage_kinds().await
    }

    /// Vectors, not content — but still a read of stored material, so it takes
    /// the same tier check rather than being waved through as metadata.
    async fn chunk_embeddings(
        &self,
        chunk_ids: &[String],
        model_signature: &str,
    ) -> Result<Vec<ChunkEmbedding>, MemoryError> {
        self.policy.admit_read(
            Capability::Chunks,
            "chunks.chunk_embeddings",
            NO_NAMESPACE,
            false,
        )?;
        self.family()?
            .chunk_embeddings(chunk_ids, model_signature)
            .await
    }
}

// ── Retrieval ────────────────────────────────────────────────────────────────

#[async_trait]
impl MemoryRetrieval for GuardedRetrieval {
    async fn fast_retrieve(
        &self,
        query: &str,
        options: FastRetrieveQuery,
        scope: Option<&SourceScope>,
    ) -> Result<RetrievalResponse, MemoryError> {
        self.policy.admit_read(
            Capability::Retrieval,
            "retrieval.fast_retrieve",
            NO_NAMESPACE,
            false,
        )?;
        let effective = self.policy.narrow_scope(scope);
        self.family()?
            .fast_retrieve(query, options, effective.as_ref())
            .await
    }

    async fn cover_window(
        &self,
        window: &CoverWindowQuery,
        scope: Option<&SourceScope>,
    ) -> Result<RetrievalResponse, MemoryError> {
        self.policy.admit_read(
            Capability::Retrieval,
            "retrieval.cover_window",
            NO_NAMESPACE,
            false,
        )?;
        let effective = self.policy.narrow_scope(scope);
        self.family()?
            .cover_window(window, effective.as_ref())
            .await
    }

    async fn retrieve_source(
        &self,
        query: &SourceRetrievalQuery,
        scope: Option<&SourceScope>,
    ) -> Result<RetrievalResponse, MemoryError> {
        self.policy.admit_read(
            Capability::Retrieval,
            "retrieval.retrieve_source",
            NO_NAMESPACE,
            false,
        )?;
        let effective = self.policy.narrow_scope(scope);
        self.family()?
            .retrieve_source(query, effective.as_ref())
            .await
    }

    async fn retrieve_children(
        &self,
        node_id: &str,
        max_depth: u32,
        query: Option<&str>,
        limit: Option<usize>,
        scope: Option<&SourceScope>,
    ) -> Result<Vec<RetrievalHit>, MemoryError> {
        self.policy.admit_read(
            Capability::Retrieval,
            "retrieval.retrieve_children",
            NO_NAMESPACE,
            false,
        )?;
        // Intersected with the ambient allowlist, never passed through — same
        // rule as `list_chunks`. See `GuardPolicy::narrow_scope`.
        let effective = self.policy.narrow_scope(scope);
        self.family()?
            .retrieve_children(node_id, max_depth, query, limit, effective.as_ref())
            .await
    }

    async fn retrieve_leaves(
        &self,
        chunk_ids: &[String],
        scope: Option<&SourceScope>,
    ) -> Result<Vec<RetrievalHit>, MemoryError> {
        self.policy.admit_read(
            Capability::Retrieval,
            "retrieval.retrieve_leaves",
            NO_NAMESPACE,
            false,
        )?;
        let effective = self.policy.narrow_scope(scope);
        self.family()?
            .retrieve_leaves(chunk_ids, effective.as_ref())
            .await
    }

    /// Namespace-scoped, so the namespace reaches the tier check — unlike the
    /// other retrieval primitives, which span the store.
    async fn recall_namespace_scored(
        &self,
        namespace: &str,
        query: &str,
        limit: usize,
        exclude_session_id: Option<&str>,
    ) -> Result<Vec<NamespaceMemoryHit>, MemoryError> {
        self.policy.admit_read(
            Capability::Retrieval,
            "retrieval.recall_namespace_scored",
            namespace,
            false,
        )?;
        self.family()?
            .recall_namespace_scored(namespace, query, limit, exclude_session_id)
            .await
    }

    /// Namespace-scoped like its scored sibling, and admitted under the same
    /// capability: recency versus ranking is a retrieval mode, not a policy
    /// boundary.
    async fn recall_namespace_recent(
        &self,
        namespace: &str,
        limit: usize,
    ) -> Result<Vec<NamespaceMemoryHit>, MemoryError> {
        self.policy.admit_read(
            Capability::Retrieval,
            "retrieval.recall_namespace_recent",
            namespace,
            false,
        )?;
        self.family()?
            .recall_namespace_recent(namespace, limit)
            .await
    }

    async fn search_entities(
        &self,
        query: &str,
        kinds: Option<&[String]>,
        limit: usize,
    ) -> Result<Vec<EntityMatch>, MemoryError> {
        self.policy.admit_read(
            Capability::Retrieval,
            "retrieval.search_entities",
            NO_NAMESPACE,
            false,
        )?;
        self.family()?.search_entities(query, kinds, limit).await
    }
}

// ── Profile ──────────────────────────────────────────────────────────────────

#[async_trait]
impl MemoryEpisodic for GuardedEpisodic {
    async fn insert_turn(&self, turn: &EpisodicTurn) -> Result<i64, MemoryError> {
        // A recorded turn is user-authored conversation content, so this is a
        // write and is admitted as one — the read/write split here is about
        // what the tier permits, not about how much data moves.
        // `carries_content: true`, unlike the tier note above, which is about
        // what the tier permits rather than how much data moves. This flag is a
        // different question: it decides whether the egress record classifies
        // the transfer as `FileContent` or `Metadata`. A turn IS the user's
        // prose, and an audit trail that calls a transcript "metadata"
        // understates what left the process — the one thing that record exists
        // to get right. `tree.append` has always passed `true` for this shape.
        self.policy.admit_write(
            Capability::Episodic,
            "episodic.insert_turn",
            NO_NAMESPACE,
            true,
        )?;
        let mut turn = turn.clone();
        turn.content = self.policy.redact_outbound(&turn.content).into_owned();
        self.family()?.insert_turn(&turn).await
    }

    async fn session_turns(&self, session_id: &str) -> Result<Vec<EpisodicTurn>, MemoryError> {
        self.policy.admit_read(
            Capability::Episodic,
            "episodic.session_turns",
            NO_NAMESPACE,
            false,
        )?;
        self.family()?.session_turns(session_id).await
    }

    async fn open_segment(
        &self,
        session_id: &str,
    ) -> Result<Option<ConversationSegment>, MemoryError> {
        self.policy.admit_read(
            Capability::Episodic,
            "episodic.open_segment",
            NO_NAMESPACE,
            false,
        )?;
        self.family()?.open_segment(session_id).await
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "trait signature; see the contract's rationale"
    )]
    async fn create_segment(
        &self,
        segment_id: &str,
        session_id: &str,
        namespace: &str,
        start_episodic_id: i64,
        start_seq: Option<u32>,
        start_timestamp: f64,
        now: f64,
    ) -> Result<(), MemoryError> {
        // One of the two episodic calls that names a namespace — `insert_event`
        // is the other — so it is admitted against that namespace rather than
        // `NO_NAMESPACE`. The rest of this family addresses a segment by id and
        // has no namespace to check.
        self.policy.admit_write(
            Capability::Episodic,
            "episodic.create_segment",
            namespace,
            false,
        )?;
        self.family()?
            .create_segment(
                segment_id,
                session_id,
                namespace,
                start_episodic_id,
                start_seq,
                start_timestamp,
                now,
            )
            .await
    }

    async fn append_turn(
        &self,
        segment_id: &str,
        episodic_id: i64,
        seq: Option<u32>,
        timestamp: f64,
        now: f64,
    ) -> Result<(), MemoryError> {
        self.policy.admit_write(
            Capability::Episodic,
            "episodic.append_turn",
            NO_NAMESPACE,
            false,
        )?;
        self.family()?
            .append_turn(segment_id, episodic_id, seq, timestamp, now)
            .await
    }

    /// Admitted like its sibling writes; the event's namespace is the
    /// admission subject, since it is the one the record is scoped to.
    async fn insert_event(&self, event: &EpisodicEvent) -> Result<(), MemoryError> {
        // `carries_content: true` for the same reason as `insert_turn`: an
        // extracted event is the user's prose, not a descriptor of it.
        self.policy.admit_write(
            Capability::Episodic,
            "episodic.insert_event",
            &event.namespace,
            true,
        )?;
        let mut event = event.clone();
        event.content = self.policy.redact_outbound(&event.content).into_owned();
        self.family()?.insert_event(&event).await
    }

    async fn close_segment(&self, segment_id: &str, now: f64) -> Result<(), MemoryError> {
        self.policy.admit_write(
            Capability::Episodic,
            "episodic.close_segment",
            NO_NAMESPACE,
            false,
        )?;
        self.family()?.close_segment(segment_id, now).await
    }

    async fn set_segment_summary(
        &self,
        segment_id: &str,
        summary: &str,
        now: f64,
    ) -> Result<(), MemoryError> {
        self.policy.admit_write(
            Capability::Episodic,
            "episodic.set_segment_summary",
            NO_NAMESPACE,
            false,
        )?;
        self.family()?
            .set_segment_summary(segment_id, summary, now)
            .await
    }

    async fn upsert_segment_embedding(
        &self,
        segment_id: &str,
        model_signature: &str,
        embedding: &[f32],
        created_at: f64,
    ) -> Result<(), MemoryError> {
        self.policy.admit_write(
            Capability::Episodic,
            "episodic.upsert_segment_embedding",
            NO_NAMESPACE,
            false,
        )?;
        self.family()?
            .upsert_segment_embedding(segment_id, model_signature, embedding, created_at)
            .await
    }
}

#[async_trait]
impl MemoryProfile for GuardedProfile {
    async fn list_active_facets(&self) -> Result<Vec<ProfileFacet>, MemoryError> {
        self.policy.admit_read(
            Capability::Profile,
            "profile.list_active_facets",
            NO_NAMESPACE,
            false,
        )?;
        self.family()?.list_active_facets().await
    }

    async fn list_all_facets(&self) -> Result<Vec<ProfileFacet>, MemoryError> {
        self.policy.admit_read(
            Capability::Profile,
            "profile.list_all_facets",
            NO_NAMESPACE,
            false,
        )?;
        self.family()?.list_all_facets().await
    }

    async fn get_facet(&self, key: &str) -> Result<Option<ProfileFacet>, MemoryError> {
        self.policy.admit_read(
            Capability::Profile,
            "profile.get_facet",
            NO_NAMESPACE,
            false,
        )?;
        self.family()?.get_facet(key).await
    }

    async fn facets_by_type(
        &self,
        facet_type: FacetType,
    ) -> Result<Vec<ProfileFacet>, MemoryError> {
        self.policy.admit_read(
            Capability::Profile,
            "profile.facets_by_type",
            NO_NAMESPACE,
            false,
        )?;
        self.family()?.facets_by_type(facet_type).await
    }

    async fn upsert_facet(&self, facet: &ProfileFacet) -> Result<(), MemoryError> {
        self.policy.admit_write(
            Capability::Profile,
            "profile.upsert_facet",
            NO_NAMESPACE,
            true,
        )?;
        self.family()?.upsert_facet(facet).await
    }

    async fn upsert_provider_facet(
        &self,
        facet_id: &str,
        facet_type: FacetType,
        key: &str,
        value: &str,
        confidence: f64,
        segment_id: Option<&str>,
        observed_at: f64,
    ) -> Result<(), MemoryError> {
        self.policy.admit_write(
            Capability::Profile,
            "profile.upsert_provider_facet",
            NO_NAMESPACE,
            true,
        )?;
        self.family()?
            .upsert_provider_facet(
                facet_id,
                facet_type,
                key,
                value,
                confidence,
                segment_id,
                observed_at,
            )
            .await
    }

    async fn set_facet_user_state(
        &self,
        key: &str,
        user_state: UserState,
    ) -> Result<bool, MemoryError> {
        self.policy.admit_write(
            Capability::Profile,
            "profile.set_facet_user_state",
            NO_NAMESPACE,
            true,
        )?;
        self.family()?.set_facet_user_state(key, user_state).await
    }

    async fn delete_facet(&self, key: &str) -> Result<bool, MemoryError> {
        self.policy.admit_write(
            Capability::Profile,
            "profile.delete_facet",
            NO_NAMESPACE,
            true,
        )?;
        self.family()?.delete_facet(key).await
    }

    async fn delete_facet_by_id(&self, facet_id: &str) -> Result<bool, MemoryError> {
        self.policy.admit_write(
            Capability::Profile,
            "profile.delete_facet_by_id",
            NO_NAMESPACE,
            true,
        )?;
        self.family()?.delete_facet_by_id(facet_id).await
    }

    async fn drop_facets_below(&self, threshold: f64) -> Result<usize, MemoryError> {
        self.policy.admit_write(
            Capability::Profile,
            "profile.drop_facets_below",
            NO_NAMESPACE,
            true,
        )?;
        self.family()?.drop_facets_below(threshold).await
    }

    /// Refused reads answer `false`, matching the trait's "an error reads as
    /// no". A tier refusal is not evidence that the row matches.
    async fn workflow_identity_matches(&self, key_pattern: &str, canonical_value: &str) -> bool {
        if self
            .policy
            .admit_read(
                Capability::Profile,
                "profile.workflow_identity_matches",
                NO_NAMESPACE,
                false,
            )
            .is_err()
        {
            return false;
        }
        match self.family() {
            Ok(family) => {
                family
                    .workflow_identity_matches(key_pattern, canonical_value)
                    .await
            }
            Err(_) => false,
        }
    }
}

#[cfg(test)]
#[path = "families_tests.rs"]
mod tests;

// ── Source sync ──────────────────────────────────────────────────────────────

#[async_trait]
impl MemorySourceSync for GuardedSourceSync {
    /// A write: it fetches from an upstream connector and ingests what it finds.
    /// The tier check is what stops a `readonly` operator triggering one.
    async fn run_connection_sync(
        &self,
        toolkit: &str,
        connection_id: &str,
    ) -> Result<SyncRunOutcome, MemoryError> {
        self.policy.admit_write(
            Capability::SourceSync,
            "source_sync.run_connection_sync",
            NO_NAMESPACE,
            false,
        )?;
        self.family()?
            .run_connection_sync(toolkit, connection_id)
            .await
    }

    async fn source_sync_state(
        &self,
        toolkit: &str,
        connection_id: &str,
    ) -> Result<Option<SourceSyncState>, MemoryError> {
        self.policy.admit_read(
            Capability::SourceSync,
            "source_sync.source_sync_state",
            NO_NAMESPACE,
            false,
        )?;
        self.family()?
            .source_sync_state(toolkit, connection_id)
            .await
    }

    async fn sync_audit_log(
        &self,
        limit: Option<usize>,
    ) -> Result<Vec<SyncAuditEntry>, MemoryError> {
        self.policy.admit_read(
            Capability::SourceSync,
            "source_sync.sync_audit_log",
            NO_NAMESPACE,
            false,
        )?;
        self.family()?.sync_audit_log(limit).await
    }

    /// Arithmetic over the driver's own price table — no stored content is read,
    /// so this is the lightest check in the family.
    async fn estimate_sync_cost_usd(
        &self,
        input_tokens: u64,
        output_tokens: u64,
    ) -> Result<f64, MemoryError> {
        self.policy.admit_read(
            Capability::SourceSync,
            "source_sync.estimate_sync_cost_usd",
            NO_NAMESPACE,
            false,
        )?;
        self.family()?
            .estimate_sync_cost_usd(input_tokens, output_tokens)
            .await
    }

    async fn sync_statuses(&self) -> Result<Vec<SourceSyncStatus>, MemoryError> {
        self.policy.admit_read(
            Capability::SourceSync,
            "source_sync.sync_statuses",
            NO_NAMESPACE,
            false,
        )?;
        self.family()?.sync_statuses().await
    }

    async fn raw_archive_coverage(
        &self,
        tree_scope: &str,
        archive_source_id: &str,
    ) -> Result<RawArchiveCoverage, MemoryError> {
        self.policy.admit_read(
            Capability::SourceSync,
            "source_sync.raw_archive_coverage",
            NO_NAMESPACE,
            false,
        )?;
        self.family()?
            .raw_archive_coverage(tree_scope, archive_source_id)
            .await
    }

    /// Rebuilds a summary tree from the raw archive, so it writes.
    async fn rebuild_from_raw_archive(
        &self,
        tree_scope: &str,
        archive_source_id: &str,
    ) -> Result<RawRebuildOutcome, MemoryError> {
        self.policy.admit_write(
            Capability::SourceSync,
            "source_sync.rebuild_from_raw_archive",
            NO_NAMESPACE,
            false,
        )?;
        self.family()?
            .rebuild_from_raw_archive(tree_scope, archive_source_id)
            .await
    }
}

// ── Scoring ──────────────────────────────────────────────────────────────────

#[async_trait]
impl MemoryScoring for GuardedScoring {
    async fn extract_entities(&self, query: &str) -> Result<Vec<String>, MemoryError> {
        self.policy.admit_read(
            Capability::Scoring,
            "scoring.extract_entities",
            NO_NAMESPACE,
            true,
        )?;
        self.family()?.extract_entities(query).await
    }

    async fn embed_text(&self, text: &str) -> Result<Vec<f32>, MemoryError> {
        self.policy.admit_read(
            Capability::Scoring,
            "scoring.embed_text",
            NO_NAMESPACE,
            true,
        )?;
        self.family()?.embed_text(text).await
    }

    async fn embedder_slug(&self) -> Result<String, MemoryError> {
        self.policy.admit_read(
            Capability::Scoring,
            "scoring.embedder_slug",
            NO_NAMESPACE,
            false,
        )?;
        self.family()?.embedder_slug().await
    }
}

// ── Coding sessions ──────────────────────────────────────────────────────────

#[async_trait]
impl MemoryCodingSessions for GuardedCodingSessions {
    async fn coding_session_status(&self) -> Result<Vec<CodingSessionSource>, MemoryError> {
        self.policy.admit_read(
            Capability::CodingSessions,
            "coding_sessions.coding_session_status",
            NO_NAMESPACE,
            false,
        )?;
        self.family()?.coding_session_status().await
    }

    /// `carries_content: true` — the request carries the session transcripts
    /// themselves, which is the case the egress record exists to classify
    /// correctly.
    async fn ingest_coding_sessions(
        &self,
        request: CodingSessionIngestRequest,
    ) -> Result<CodingSessionIngestReport, MemoryError> {
        self.policy.admit_write(
            Capability::CodingSessions,
            "coding_sessions.ingest_coding_sessions",
            NO_NAMESPACE,
            true,
        )?;
        self.family()?.ingest_coding_sessions(request).await
    }
}

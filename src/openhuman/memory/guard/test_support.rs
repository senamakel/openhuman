//! A recording fake [`MemoryProvider`] for the guard's tests.
//!
//! `NullMemoryProvider` cannot serve here: it advertises only the mandatory
//! three and returns `None` from every `as_*` accessor, so a guard built over
//! it would have no family decorators at all — which is precisely what the
//! interesting tests are about. This fake implements **all thirteen** families
//! and records what actually reached it, so a test can assert both "the driver
//! saw the value the guard rewrote" and "the driver saw nothing at all".

#![cfg(test)]

use std::sync::{Arc, Mutex};

use crate::openhuman::memory::api::capabilities::Capabilities;
use crate::openhuman::memory::api::chunks::Chunk;
use crate::openhuman::memory::api::error::MemoryError;
use crate::openhuman::memory::api::goals::GoalsDoc;
use crate::openhuman::memory::api::health::MemoryHealth;
use crate::openhuman::memory::api::provider::sessions::{
    CodingSessionIngestReport, CodingSessionIngestRequest, CodingSessionSource,
};
use crate::openhuman::memory::api::provider::sync::{
    RawArchiveCoverage, RawRebuildOutcome, SourceSyncState, SourceSyncStatus, SyncAuditEntry,
    SyncRunOutcome,
};
use crate::openhuman::memory::api::provider::types::{
    DiffReport, EntityHit, ExportPage, ExportRecord, ImportOutcome, IngestItem, IngestOutcome,
    MaintenanceReport, SnapshotRef, SourceItem, SourceScope,
};
use crate::openhuman::memory::api::provider::{
    AddressBookSeedOutcome, ChunkDetail, ChunkEmbedding, ChunkQuery, CoverWindowQuery, EntityMatch,
    EpisodicEvent, FacetType, FastRetrieveQuery, MemoryChunks, MemoryCodingSessions, MemoryCore,
    MemoryDiff, MemoryDocuments, MemoryEntities, MemoryEpisodic, MemoryGoals, MemoryGraph,
    MemoryIngest, MemoryMaintenance, MemoryPeople, MemoryPortability, MemoryProfile,
    MemoryProvider, MemoryRecall, MemoryRetrieval, MemoryScoring, MemorySourceSink,
    MemorySourceSync, MemoryToolMemory, MemoryTree, PersonHandle, PersonInteraction, PersonRecord,
    PersonScore, ProfileFacet, RankedPerson, ResolvedPerson, RetrievalHit, RetrievalResponse,
    SourceRetrievalQuery, UserState,
};
use crate::openhuman::memory::api::recall::OwnedRecallOpts;
use crate::openhuman::memory::api::tool_memory::ToolMemoryRule;
use crate::openhuman::memory::api::tree::{IngestRequest, QueryResult, TreeStatus};
use crate::openhuman::memory::api::types::{
    GraphRelationRecord, MemoryCategory, MemoryEntry, MemoryKvRecord, MemoryTaint,
    NamespaceDocumentInput, NamespaceMemoryHit, NamespaceRetrievalContext, NamespaceSummary,
    StoredMemoryDocument,
};
use async_trait::async_trait;

/// One call that reached the driver.
#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    pub method: String,
    /// Content the driver was handed, when the method carries any.
    pub content: Option<String>,
    /// Provenance the driver was handed, when the method carries any.
    pub taint: Option<MemoryTaint>,
    /// Whether the method received a `Some(scope)`.
    pub scoped: Option<bool>,
}

/// The scope's allow list rendered for assertions, sorted for determinism.
fn rendered_scope(scope: Option<&SourceScope>) -> Option<String> {
    scope.map(|s| {
        let mut allow = s.allow.clone();
        allow.sort();
        allow.join(",")
    })
}

impl Call {
    fn plain(method: &str) -> Self {
        Self {
            method: method.into(),
            content: None,
            taint: None,
            scoped: None,
        }
    }
}

/// A provider that records and answers with empties.
pub struct RecordingProvider {
    calls: Mutex<Vec<Call>>,
    /// What `recall` returns, so budget tests can drive a known result set.
    recall_result: Mutex<Vec<MemoryEntry>>,
}

impl Default for RecordingProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl RecordingProvider {
    pub fn new() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            recall_result: Mutex::new(Vec::new()),
        }
    }

    pub fn with_recall_result(self, entries: Vec<MemoryEntry>) -> Self {
        *self.recall_result.lock().unwrap() = entries;
        self
    }

    fn record(&self, call: Call) {
        self.calls.lock().unwrap().push(call);
    }

    pub fn calls(&self) -> Vec<Call> {
        self.calls.lock().unwrap().clone()
    }

    pub fn call_count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }

    /// The single recorded call, panicking when there is not exactly one.
    pub fn only_call(&self) -> Call {
        let calls = self.calls();
        assert_eq!(
            calls.len(),
            1,
            "expected exactly one driver call: {calls:?}"
        );
        calls.into_iter().next().unwrap()
    }
}

/// A [`GuardPolicy`](super::GuardPolicy) over an embedded driver with default
/// budgets — the shipped configuration.
pub fn embedded_policy() -> super::GuardPolicy {
    super::GuardPolicy::new(
        "recording",
        crate::core::subsystem::DriverClass::Embedded,
        crate::openhuman::config::schema::MemoryHooksConfig::default(),
        super::policy::TRUSTED,
    )
}

/// A policy over an *external* driver. No such driver can bind today
/// (`binding::admit` refuses them), so this is the only way to reach the class
/// branches that land for real in M6.
pub fn external_policy(trust_state: &str) -> super::GuardPolicy {
    super::GuardPolicy::new(
        "supermemory",
        crate::core::subsystem::DriverClass::External,
        crate::openhuman::config::schema::MemoryHooksConfig::default(),
        trust_state,
    )
}

/// An [`ExportRecord`] fixture.
pub fn export_record(taint: MemoryTaint) -> ExportRecord {
    ExportRecord {
        kind: "entry".into(),
        id: "r1".into(),
        namespace: Some("ns".into()),
        taint,
        payload: serde_json::Value::Null,
    }
}

/// A guard over a fresh recording provider, plus a handle on that provider.
pub fn guarded(policy: super::GuardPolicy) -> (Arc<RecordingProvider>, super::MemoryGuard) {
    guarded_with(RecordingProvider::new(), policy)
}

/// As [`guarded`], over a caller-configured provider.
pub fn guarded_with(
    provider: RecordingProvider,
    policy: super::GuardPolicy,
) -> (Arc<RecordingProvider>, super::MemoryGuard) {
    let provider = Arc::new(provider);
    let guard = super::MemoryGuard::new(
        Arc::clone(&provider) as Arc<dyn MemoryProvider>,
        Arc::new(policy),
    );
    (provider, guard)
}

/// A [`MemoryEntry`] fixture.
pub fn entry(content: &str) -> MemoryEntry {
    MemoryEntry {
        id: "id".into(),
        key: "key".into(),
        content: content.into(),
        namespace: Some("ns".into()),
        category: MemoryCategory::Core,
        timestamp: "2026-01-01T00:00:00Z".into(),
        session_id: None,
        score: None,
        taint: MemoryTaint::Internal,
    }
}

/// A [`TreeStatus`] fixture.
fn tree_status(namespace: &str) -> TreeStatus {
    TreeStatus {
        namespace: namespace.to_string(),
        total_nodes: 0,
        depth: 0,
        oldest_entry: None,
        newest_entry: None,
        last_run_at: None,
    }
}

/// A [`NamespaceDocumentInput`] fixture.
pub fn document(content: &str, taint: MemoryTaint) -> NamespaceDocumentInput {
    NamespaceDocumentInput {
        namespace: "ns".into(),
        key: "k".into(),
        title: "t".into(),
        content: content.into(),
        source_type: "chat".into(),
        priority: "normal".into(),
        tags: vec![],
        metadata: serde_json::Value::Null,
        category: "core".into(),
        session_id: None,
        document_id: None,
        taint,
    }
}

#[async_trait]
impl MemoryCore for RecordingProvider {
    async fn store(
        &self,
        _namespace: &str,
        _key: &str,
        content: &str,
        _category: MemoryCategory,
        _session_id: Option<&str>,
        taint: MemoryTaint,
    ) -> Result<(), MemoryError> {
        self.record(Call {
            method: "core.store".into(),
            content: Some(content.to_string()),
            taint: Some(taint),
            scoped: None,
        });
        Ok(())
    }

    async fn get(&self, _namespace: &str, _key: &str) -> Result<Option<MemoryEntry>, MemoryError> {
        self.record(Call::plain("core.get"));
        Ok(None)
    }

    async fn forget(&self, _namespace: &str, _key: &str) -> Result<bool, MemoryError> {
        self.record(Call::plain("core.forget"));
        Ok(false)
    }

    async fn list(
        &self,
        _namespace: Option<&str>,
        _category: Option<&MemoryCategory>,
        _session_id: Option<&str>,
    ) -> Result<Vec<MemoryEntry>, MemoryError> {
        self.record(Call::plain("core.list"));
        Ok(vec![])
    }

    async fn namespaces(&self) -> Result<Vec<NamespaceSummary>, MemoryError> {
        self.record(Call::plain("core.namespaces"));
        Ok(vec![])
    }
}

#[async_trait]
impl MemoryRecall for RecordingProvider {
    async fn recall(
        &self,
        query: &str,
        _limit: usize,
        _opts: &OwnedRecallOpts,
        scope: Option<&SourceScope>,
    ) -> Result<Vec<MemoryEntry>, MemoryError> {
        self.record(Call {
            method: "recall.recall".into(),
            content: Some(query.to_string()),
            taint: None,
            scoped: Some(scope.is_some()),
        });
        Ok(self.recall_result.lock().unwrap().clone())
    }
}

#[async_trait]
impl MemoryPortability for RecordingProvider {
    async fn export_page(
        &self,
        _cursor: Option<&str>,
        _limit: usize,
    ) -> Result<ExportPage, MemoryError> {
        self.record(Call::plain("portability.export_page"));
        Ok(ExportPage::default())
    }

    async fn import_records(
        &self,
        records: Vec<ExportRecord>,
    ) -> Result<ImportOutcome, MemoryError> {
        self.record(Call {
            method: "portability.import_records".into(),
            content: None,
            taint: records.first().map(|r| r.taint),
            scoped: None,
        });
        Ok(ImportOutcome::default())
    }
}

#[async_trait]
impl MemoryIngest for RecordingProvider {
    async fn ingest_document(&self, item: IngestItem) -> Result<IngestOutcome, MemoryError> {
        self.record(Call {
            method: "ingest.ingest_document".into(),
            content: Some(item.content),
            taint: Some(item.taint),
            scoped: None,
        });
        Ok(IngestOutcome::default())
    }

    async fn ingest_chat(&self, messages: Vec<IngestItem>) -> Result<IngestOutcome, MemoryError> {
        self.record(Call {
            method: "ingest.ingest_chat".into(),
            content: messages.first().map(|m| m.content.clone()),
            taint: messages.first().map(|m| m.taint),
            scoped: None,
        });
        Ok(IngestOutcome::default())
    }
}

#[async_trait]
impl MemoryDocuments for RecordingProvider {
    async fn put_document(&self, input: NamespaceDocumentInput) -> Result<String, MemoryError> {
        self.record(Call {
            method: "documents.put_document".into(),
            content: Some(input.content),
            taint: Some(input.taint),
            scoped: None,
        });
        Ok("doc".into())
    }

    async fn get_document(
        &self,
        _namespace: &str,
        _key: &str,
    ) -> Result<Option<StoredMemoryDocument>, MemoryError> {
        self.record(Call::plain("documents.get_document"));
        Ok(None)
    }

    async fn list_documents(
        &self,
        _namespace: Option<&str>,
    ) -> Result<serde_json::Value, MemoryError> {
        self.record(Call::plain("documents.list_documents"));
        Ok(serde_json::json!({"documents": []}))
    }

    async fn list_namespaces(&self) -> Result<Vec<String>, MemoryError> {
        self.record(Call::plain("documents.list_namespaces"));
        Ok(vec![])
    }

    async fn delete_document(
        &self,
        _namespace: &str,
        _document_id: &str,
    ) -> Result<serde_json::Value, MemoryError> {
        self.record(Call::plain("documents.delete_document"));
        Ok(serde_json::json!({"deleted": false}))
    }

    async fn clear_namespace(&self, _namespace: &str) -> Result<(), MemoryError> {
        self.record(Call::plain("documents.clear_namespace"));
        Ok(())
    }

    async fn query_documents(
        &self,
        namespace: &str,
        query: &str,
        _limit: usize,
    ) -> Result<NamespaceRetrievalContext, MemoryError> {
        self.record(Call {
            method: "documents.query_documents".into(),
            content: Some(query.to_string()),
            taint: None,
            scoped: None,
        });
        Ok(NamespaceRetrievalContext {
            namespace: namespace.to_string(),
            query: Some(query.to_string()),
            context_text: String::new(),
            hits: vec![],
        })
    }

    async fn recall_documents(
        &self,
        namespace: &str,
        _limit: usize,
    ) -> Result<NamespaceRetrievalContext, MemoryError> {
        self.record(Call::plain("documents.recall_documents"));
        Ok(NamespaceRetrievalContext {
            namespace: namespace.to_string(),
            query: None,
            context_text: String::new(),
            hits: vec![],
        })
    }
}

#[async_trait]
impl MemoryTree for RecordingProvider {
    async fn append(&self, request: IngestRequest) -> Result<(), MemoryError> {
        self.record(Call {
            method: "tree.append".into(),
            content: Some(request.content),
            taint: None,
            scoped: None,
        });
        Ok(())
    }

    async fn query_source(
        &self,
        _namespace: &str,
        _source_id: &str,
        _limit: usize,
        scope: Option<&SourceScope>,
    ) -> Result<Vec<Chunk>, MemoryError> {
        self.record(Call {
            method: "tree.query_source".into(),
            // The scope's allow list, rendered so a test can assert which one
            // arrived. Sorted because it comes from a `HashSet`.
            content: scope.map(|s| {
                let mut allow = s.allow.clone();
                allow.sort();
                allow.join(",")
            }),
            taint: None,
            scoped: Some(scope.is_some()),
        });
        Ok(vec![])
    }

    async fn drill_down(
        &self,
        _namespace: &str,
        _node_id: &str,
    ) -> Result<QueryResult, MemoryError> {
        self.record(Call::plain("tree.drill_down"));
        Err(MemoryError::NotFound("node".into()))
    }

    async fn seal(&self, namespace: &str) -> Result<TreeStatus, MemoryError> {
        self.record(Call::plain("tree.seal"));
        Ok(tree_status(namespace))
    }

    async fn cascade(&self, namespace: &str) -> Result<TreeStatus, MemoryError> {
        self.record(Call::plain("tree.cascade"));
        Ok(tree_status(namespace))
    }
}

#[async_trait]
impl MemoryEntities for RecordingProvider {
    async fn entities(
        &self,
        _namespace: &str,
        _query: Option<&str>,
        _limit: usize,
    ) -> Result<Vec<EntityHit>, MemoryError> {
        self.record(Call::plain("entities.entities"));
        Ok(vec![])
    }

    async fn entity_edges(
        &self,
        _namespace: &str,
        _entity_id: &str,
        _limit: usize,
    ) -> Result<Vec<GraphRelationRecord>, MemoryError> {
        self.record(Call::plain("entities.entity_edges"));
        Ok(vec![])
    }

    async fn touch_entities(
        &self,
        _namespace: &str,
        _entity_ids: &[String],
    ) -> Result<(), MemoryError> {
        self.record(Call::plain("entities.touch_entities"));
        Ok(())
    }
}

#[async_trait]
impl MemoryGraph for RecordingProvider {
    async fn kv_get(
        &self,
        _namespace: Option<&str>,
        _key: &str,
    ) -> Result<Option<MemoryKvRecord>, MemoryError> {
        self.record(Call::plain("graph.kv_get"));
        Ok(None)
    }

    async fn kv_put(
        &self,
        _namespace: Option<&str>,
        _key: &str,
        value: serde_json::Value,
    ) -> Result<(), MemoryError> {
        self.record(Call {
            method: "graph.kv_put".into(),
            content: Some(value.to_string()),
            taint: None,
            scoped: None,
        });
        Ok(())
    }

    async fn kv_delete(&self, _namespace: Option<&str>, _key: &str) -> Result<bool, MemoryError> {
        self.record(Call::plain("graph.kv_delete"));
        Ok(false)
    }

    async fn kv_list(
        &self,
        _namespace: Option<&str>,
        _prefix: Option<&str>,
        _limit: usize,
    ) -> Result<Vec<MemoryKvRecord>, MemoryError> {
        self.record(Call::plain("graph.kv_list"));
        Ok(vec![])
    }

    async fn relations(
        &self,
        _namespace: Option<&str>,
        _subject: Option<&str>,
        _predicate: Option<&str>,
        _limit: usize,
    ) -> Result<Vec<GraphRelationRecord>, MemoryError> {
        self.record(Call::plain("graph.relations"));
        Ok(vec![])
    }

    async fn put_relation(&self, _relation: GraphRelationRecord) -> Result<(), MemoryError> {
        self.record(Call::plain("graph.put_relation"));
        Ok(())
    }
}

#[async_trait]
impl MemoryDiff for RecordingProvider {
    async fn capture_snapshot(&self, _source_id: &str) -> Result<SnapshotRef, MemoryError> {
        self.record(Call::plain("diff.capture_snapshot"));
        Err(MemoryError::NotFound("source".into()))
    }

    async fn snapshots(
        &self,
        _source_id: &str,
        _limit: usize,
    ) -> Result<Vec<SnapshotRef>, MemoryError> {
        self.record(Call::plain("diff.snapshots"));
        Ok(vec![])
    }

    async fn diff(
        &self,
        _source_id: &str,
        _from: Option<&str>,
        _to: &str,
    ) -> Result<DiffReport, MemoryError> {
        self.record(Call::plain("diff.diff"));
        Err(MemoryError::NotFound("snapshot".into()))
    }
}

#[async_trait]
impl MemoryGoals for RecordingProvider {
    async fn goals(&self) -> Result<GoalsDoc, MemoryError> {
        self.record(Call::plain("goals.goals"));
        Ok(GoalsDoc::default())
    }

    async fn set_goals(&self, _goals: GoalsDoc) -> Result<(), MemoryError> {
        self.record(Call::plain("goals.set_goals"));
        Ok(())
    }
}

#[async_trait]
impl MemoryToolMemory for RecordingProvider {
    async fn tool_rules(&self, _tool_name: &str) -> Result<Vec<ToolMemoryRule>, MemoryError> {
        self.record(Call::plain("tool_memory.tool_rules"));
        Ok(vec![])
    }

    async fn put_tool_rule(&self, _rule: ToolMemoryRule) -> Result<(), MemoryError> {
        self.record(Call::plain("tool_memory.put_tool_rule"));
        Ok(())
    }

    async fn delete_tool_rule(
        &self,
        _tool_name: &str,
        _rule_id: &str,
    ) -> Result<bool, MemoryError> {
        self.record(Call::plain("tool_memory.delete_tool_rule"));
        Ok(false)
    }
}

#[async_trait]
impl MemorySourceSink for RecordingProvider {
    async fn accept_source_items(
        &self,
        _source_id: &str,
        _source_kind: &str,
        items: Vec<SourceItem>,
        taint: MemoryTaint,
    ) -> Result<IngestOutcome, MemoryError> {
        self.record(Call {
            method: "sources.accept_source_items".into(),
            content: items.first().map(|i| i.content.clone()),
            taint: Some(taint),
            scoped: None,
        });
        Ok(IngestOutcome::default())
    }

    async fn forget_source(&self, _source_id: &str) -> Result<u64, MemoryError> {
        self.record(Call::plain("sources.forget_source"));
        Ok(0)
    }
}

#[async_trait]
impl MemoryMaintenance for RecordingProvider {
    async fn reembed(&self) -> Result<MaintenanceReport, MemoryError> {
        self.record(Call::plain("maintenance.reembed"));
        Ok(MaintenanceReport::default())
    }

    async fn compact(&self) -> Result<MaintenanceReport, MemoryError> {
        self.record(Call::plain("maintenance.compact"));
        Ok(MaintenanceReport::default())
    }

    async fn consolidate(&self) -> Result<MaintenanceReport, MemoryError> {
        self.record(Call::plain("maintenance.consolidate"));
        Ok(MaintenanceReport::default())
    }

    async fn doctor(&self) -> Result<MaintenanceReport, MemoryError> {
        self.record(Call::plain("maintenance.doctor"));
        Ok(MaintenanceReport::default())
    }
}

#[async_trait]
impl MemoryProvider for RecordingProvider {
    fn driver_id(&self) -> &str {
        "recording"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::all()
    }

    async fn health(&self) -> MemoryHealth {
        MemoryHealth::Ready
    }

    fn as_ingest(&self) -> Option<&dyn MemoryIngest> {
        Some(self)
    }
    fn as_documents(&self) -> Option<&dyn MemoryDocuments> {
        Some(self)
    }
    fn as_tree(&self) -> Option<&dyn MemoryTree> {
        Some(self)
    }
    fn as_entities(&self) -> Option<&dyn MemoryEntities> {
        Some(self)
    }
    fn as_graph(&self) -> Option<&dyn MemoryGraph> {
        Some(self)
    }
    fn as_diff(&self) -> Option<&dyn MemoryDiff> {
        Some(self)
    }
    fn as_goals(&self) -> Option<&dyn MemoryGoals> {
        Some(self)
    }
    fn as_tool_memory(&self) -> Option<&dyn MemoryToolMemory> {
        Some(self)
    }
    fn as_sources(&self) -> Option<&dyn MemorySourceSink> {
        Some(self)
    }
    fn as_maintenance(&self) -> Option<&dyn MemoryMaintenance> {
        Some(self)
    }
    fn as_people(&self) -> Option<&dyn MemoryPeople> {
        Some(self)
    }
    fn as_chunks(&self) -> Option<&dyn MemoryChunks> {
        Some(self)
    }
    fn as_retrieval(&self) -> Option<&dyn MemoryRetrieval> {
        Some(self)
    }
    fn as_profile(&self) -> Option<&dyn MemoryProfile> {
        Some(self)
    }
    fn as_episodic(&self) -> Option<&dyn MemoryEpisodic> {
        Some(self)
    }
    fn as_source_sync(&self) -> Option<&dyn MemorySourceSync> {
        Some(self)
    }
    fn as_coding_sessions(&self) -> Option<&dyn MemoryCodingSessions> {
        Some(self)
    }
    fn as_scoring(&self) -> Option<&dyn MemoryScoring> {
        Some(self)
    }
}

// The two families tinymemory v1.7.0 added. `capabilities()` above answers
// `Capabilities::all()`, so a driver that advertises them and then hands back
// `None` from the accessor is exactly the inconsistency `audit_provider`
// exists to catch — the recorder has to serve them to stay honest.

#[async_trait]
impl MemorySourceSync for RecordingProvider {
    async fn run_connection_sync(
        &self,
        toolkit: &str,
        connection_id: &str,
    ) -> Result<SyncRunOutcome, MemoryError> {
        self.record(Call::plain("source_sync.run_connection_sync"));
        let _ = (toolkit, connection_id);
        Ok(SyncRunOutcome::default())
    }
    async fn source_sync_state(
        &self,
        toolkit: &str,
        connection_id: &str,
    ) -> Result<Option<SourceSyncState>, MemoryError> {
        self.record(Call::plain("source_sync.source_sync_state"));
        let _ = (toolkit, connection_id);
        Ok(None)
    }
    async fn sync_audit_log(
        &self,
        _limit: Option<usize>,
    ) -> Result<Vec<SyncAuditEntry>, MemoryError> {
        self.record(Call::plain("source_sync.sync_audit_log"));
        Ok(Vec::new())
    }
    async fn estimate_sync_cost_usd(
        &self,
        _input_tokens: u64,
        _output_tokens: u64,
    ) -> Result<f64, MemoryError> {
        self.record(Call::plain("source_sync.estimate_sync_cost_usd"));
        Ok(0.0)
    }
    async fn sync_statuses(&self) -> Result<Vec<SourceSyncStatus>, MemoryError> {
        self.record(Call::plain("source_sync.sync_statuses"));
        Ok(Vec::new())
    }
    async fn raw_archive_coverage(
        &self,
        tree_scope: &str,
        archive_source_id: &str,
    ) -> Result<RawArchiveCoverage, MemoryError> {
        self.record(Call::plain("source_sync.raw_archive_coverage"));
        let _ = (tree_scope, archive_source_id);
        Ok(RawArchiveCoverage::default())
    }
    async fn rebuild_from_raw_archive(
        &self,
        tree_scope: &str,
        archive_source_id: &str,
    ) -> Result<RawRebuildOutcome, MemoryError> {
        self.record(Call::plain("source_sync.rebuild_from_raw_archive"));
        let _ = (tree_scope, archive_source_id);
        Ok(RawRebuildOutcome::default())
    }
}

#[async_trait]
impl MemoryCodingSessions for RecordingProvider {
    async fn coding_session_status(&self) -> Result<Vec<CodingSessionSource>, MemoryError> {
        self.record(Call::plain("coding_sessions.coding_session_status"));
        Ok(Vec::new())
    }
    async fn ingest_coding_sessions(
        &self,
        _request: CodingSessionIngestRequest,
    ) -> Result<CodingSessionIngestReport, MemoryError> {
        self.record(Call::plain("coding_sessions.ingest_coding_sessions"));
        Ok(CodingSessionIngestReport::default())
    }
}

#[async_trait]
impl MemoryEpisodic for RecordingProvider {
    async fn insert_turn(
        &self,
        turn: &crate::openhuman::memory::api::provider::episodic::EpisodicTurn,
    ) -> Result<i64, MemoryError> {
        // Records the turn text, so a guard that failed to redact one would be
        // visible here rather than only in a live store.
        self.record(Call {
            method: "episodic.insert_turn".into(),
            content: Some(turn.content.clone()),
            taint: None,
            scoped: None,
        });
        Ok(1)
    }

    async fn session_turns(
        &self,
        _session_id: &str,
    ) -> Result<Vec<crate::openhuman::memory::api::provider::episodic::EpisodicTurn>, MemoryError>
    {
        self.record(Call::plain("episodic.session_turns"));
        Ok(vec![])
    }

    async fn open_segment(
        &self,
        _session_id: &str,
    ) -> Result<
        Option<crate::openhuman::memory::api::provider::episodic::ConversationSegment>,
        MemoryError,
    > {
        self.record(Call::plain("episodic.open_segment"));
        Ok(None)
    }

    async fn create_segment(
        &self,
        _segment_id: &str,
        _session_id: &str,
        _namespace: &str,
        _start_episodic_id: i64,
        _start_seq: Option<u32>,
        _start_timestamp: f64,
        _now: f64,
    ) -> Result<(), MemoryError> {
        self.record(Call::plain("episodic.create_segment"));
        Ok(())
    }

    async fn append_turn(
        &self,
        _segment_id: &str,
        _episodic_id: i64,
        _seq: Option<u32>,
        _timestamp: f64,
        _now: f64,
    ) -> Result<(), MemoryError> {
        self.record(Call::plain("episodic.append_turn"));
        Ok(())
    }

    async fn close_segment(&self, _segment_id: &str, _now: f64) -> Result<(), MemoryError> {
        self.record(Call::plain("episodic.close_segment"));
        Ok(())
    }

    async fn insert_event(&self, event: &EpisodicEvent) -> Result<(), MemoryError> {
        // Records the event text for the same reason `insert_turn` does: a guard
        // that stopped redacting one would otherwise be invisible to every test,
        // and the redaction on this path has already been missing once.
        self.record(Call {
            method: "episodic.insert_event".into(),
            content: Some(event.content.clone()),
            taint: None,
            scoped: None,
        });
        Ok(())
    }

    async fn set_segment_summary(
        &self,
        _segment_id: &str,
        summary: &str,
        _now: f64,
    ) -> Result<(), MemoryError> {
        self.record(Call {
            method: "episodic.set_segment_summary".into(),
            content: Some(summary.to_string()),
            taint: None,
            scoped: None,
        });
        Ok(())
    }

    async fn upsert_segment_embedding(
        &self,
        _segment_id: &str,
        _model_signature: &str,
        _embedding: &[f32],
        _created_at: f64,
    ) -> Result<(), MemoryError> {
        self.record(Call::plain("episodic.upsert_segment_embedding"));
        Ok(())
    }
}
#[async_trait]
impl MemoryProfile for RecordingProvider {
    async fn list_active_facets(&self) -> Result<Vec<ProfileFacet>, MemoryError> {
        self.record(Call::plain("profile.list_active_facets"));
        Ok(vec![])
    }
    async fn list_all_facets(&self) -> Result<Vec<ProfileFacet>, MemoryError> {
        self.record(Call::plain("profile.list_all_facets"));
        Ok(vec![])
    }
    async fn get_facet(&self, _key: &str) -> Result<Option<ProfileFacet>, MemoryError> {
        self.record(Call::plain("profile.get_facet"));
        Ok(None)
    }
    async fn facets_by_type(
        &self,
        _facet_type: FacetType,
    ) -> Result<Vec<ProfileFacet>, MemoryError> {
        self.record(Call::plain("profile.facets_by_type"));
        Ok(vec![])
    }
    async fn upsert_facet(&self, _facet: &ProfileFacet) -> Result<(), MemoryError> {
        self.record(Call::plain("profile.upsert_facet"));
        Ok(())
    }
    async fn upsert_provider_facet(
        &self,
        _facet_id: &str,
        _facet_type: FacetType,
        _key: &str,
        _value: &str,
        _confidence: f64,
        _segment_id: Option<&str>,
        _observed_at: f64,
    ) -> Result<(), MemoryError> {
        self.record(Call::plain("profile.upsert_provider_facet"));
        Ok(())
    }
    async fn set_facet_user_state(
        &self,
        _key: &str,
        _user_state: UserState,
    ) -> Result<bool, MemoryError> {
        self.record(Call::plain("profile.set_facet_user_state"));
        Ok(false)
    }
    async fn delete_facet(&self, _key: &str) -> Result<bool, MemoryError> {
        self.record(Call::plain("profile.delete_facet"));
        Ok(false)
    }
    async fn delete_facet_by_id(&self, _facet_id: &str) -> Result<bool, MemoryError> {
        self.record(Call::plain("profile.delete_facet_by_id"));
        Ok(false)
    }
    async fn drop_facets_below(&self, _threshold: f64) -> Result<usize, MemoryError> {
        self.record(Call::plain("profile.drop_facets_below"));
        Ok(0)
    }
    async fn workflow_identity_matches(&self, _pattern: &str, _value: &str) -> bool {
        self.record(Call::plain("profile.workflow_identity_matches"));
        false
    }
}

#[async_trait]
impl MemoryChunks for RecordingProvider {
    async fn list_chunks(
        &self,
        _query: &ChunkQuery,
        scope: Option<&SourceScope>,
    ) -> Result<Vec<Chunk>, MemoryError> {
        self.record(Call {
            method: "chunks.list_chunks".into(),
            content: rendered_scope(scope),
            taint: None,
            scoped: Some(scope.is_some()),
        });
        Ok(vec![])
    }

    async fn get_chunk(&self, _chunk_id: &str) -> Result<Option<Chunk>, MemoryError> {
        self.record(Call::plain("chunks.get_chunk"));
        Ok(None)
    }

    async fn chunk_detail(&self, _chunk_id: &str) -> Result<Option<ChunkDetail>, MemoryError> {
        self.record(Call::plain("chunks.chunk_detail"));
        Ok(None)
    }

    async fn storage_kinds(&self) -> Result<Vec<String>, MemoryError> {
        self.record(Call::plain("chunks.storage_kinds"));
        Ok(vec![])
    }

    async fn chunk_embeddings(
        &self,
        _chunk_ids: &[String],
        _model_signature: &str,
    ) -> Result<Vec<ChunkEmbedding>, MemoryError> {
        self.record(Call::plain("chunks.chunk_embeddings"));
        Ok(vec![])
    }
}

#[async_trait]
impl MemoryRetrieval for RecordingProvider {
    async fn fast_retrieve(
        &self,
        _query: &str,
        _options: FastRetrieveQuery,
        scope: Option<&SourceScope>,
    ) -> Result<RetrievalResponse, MemoryError> {
        self.record(Call {
            method: "retrieval.fast_retrieve".into(),
            content: rendered_scope(scope),
            taint: None,
            scoped: Some(scope.is_some()),
        });
        Ok(RetrievalResponse::default())
    }

    async fn cover_window(
        &self,
        _window: &CoverWindowQuery,
        scope: Option<&SourceScope>,
    ) -> Result<RetrievalResponse, MemoryError> {
        self.record(Call {
            method: "retrieval.cover_window".into(),
            content: rendered_scope(scope),
            taint: None,
            scoped: Some(scope.is_some()),
        });
        Ok(RetrievalResponse::default())
    }

    async fn retrieve_source(
        &self,
        _query: &SourceRetrievalQuery,
        scope: Option<&SourceScope>,
    ) -> Result<RetrievalResponse, MemoryError> {
        self.record(Call {
            method: "retrieval.retrieve_source".into(),
            content: rendered_scope(scope),
            taint: None,
            scoped: Some(scope.is_some()),
        });
        Ok(RetrievalResponse::default())
    }

    async fn retrieve_children(
        &self,
        _node_id: &str,
        _max_depth: u32,
        _query: Option<&str>,
        _limit: Option<usize>,
        scope: Option<&SourceScope>,
    ) -> Result<Vec<RetrievalHit>, MemoryError> {
        self.record(Call {
            method: "retrieval.retrieve_children".into(),
            content: rendered_scope(scope),
            taint: None,
            scoped: Some(scope.is_some()),
        });
        Ok(vec![])
    }

    async fn retrieve_leaves(
        &self,
        _chunk_ids: &[String],
        scope: Option<&SourceScope>,
    ) -> Result<Vec<RetrievalHit>, MemoryError> {
        self.record(Call {
            method: "retrieval.retrieve_leaves".into(),
            content: rendered_scope(scope),
            taint: None,
            scoped: Some(scope.is_some()),
        });
        Ok(vec![])
    }

    async fn recall_namespace_scored(
        &self,
        _namespace: &str,
        _query: &str,
        _limit: usize,
        _exclude_session_id: Option<&str>,
    ) -> Result<Vec<NamespaceMemoryHit>, MemoryError> {
        self.record(Call::plain("retrieval.recall_namespace_scored"));
        Ok(vec![])
    }

    async fn recall_namespace_recent(
        &self,
        _namespace: &str,
        _limit: usize,
    ) -> Result<Vec<NamespaceMemoryHit>, MemoryError> {
        self.record(Call::plain("retrieval.recall_namespace_recent"));
        Ok(vec![])
    }

    async fn search_entities(
        &self,
        _query: &str,
        _kinds: Option<&[String]>,
        _limit: usize,
    ) -> Result<Vec<EntityMatch>, MemoryError> {
        self.record(Call::plain("retrieval.search_entities"));
        Ok(vec![])
    }
}

#[async_trait]
impl MemoryPeople for RecordingProvider {
    async fn list_people(&self, _limit: Option<usize>) -> Result<Vec<RankedPerson>, MemoryError> {
        self.record(Call::plain("people.list_people"));
        Ok(vec![])
    }

    async fn get_person(&self, _person_id: &str) -> Result<Option<PersonRecord>, MemoryError> {
        self.record(Call::plain("people.get_person"));
        Ok(None)
    }

    async fn resolve_handle(
        &self,
        _handle: &PersonHandle,
        _create_if_missing: bool,
    ) -> Result<Option<ResolvedPerson>, MemoryError> {
        self.record(Call::plain("people.resolve_handle"));
        Ok(None)
    }

    async fn add_handle_alias(
        &self,
        _person_id: &str,
        _handle: &PersonHandle,
    ) -> Result<(), MemoryError> {
        self.record(Call::plain("people.add_handle_alias"));
        Ok(())
    }

    async fn score_person(&self, _person_id: &str) -> Result<Option<PersonScore>, MemoryError> {
        self.record(Call::plain("people.score_person"));
        Ok(None)
    }

    async fn record_interaction(
        &self,
        _interaction: &PersonInteraction,
    ) -> Result<(), MemoryError> {
        self.record(Call::plain("people.record_interaction"));
        Ok(())
    }

    async fn seed_from_address_book(&self) -> Result<AddressBookSeedOutcome, MemoryError> {
        self.record(Call::plain("people.seed_from_address_book"));
        Ok(AddressBookSeedOutcome::default())
    }
}

#[async_trait]
impl MemoryScoring for RecordingProvider {
    async fn extract_entities(&self, query: &str) -> Result<Vec<String>, MemoryError> {
        self.record(Call {
            method: "scoring.extract_entities".into(),
            content: Some(query.to_string()),
            taint: None,
            scoped: None,
        });
        Ok(Vec::new())
    }

    async fn embed_text(&self, text: &str) -> Result<Vec<f32>, MemoryError> {
        self.record(Call {
            method: "scoring.embed_text".into(),
            content: Some(text.to_string()),
            taint: None,
            scoped: None,
        });
        Ok(Vec::new())
    }

    async fn embedder_slug(&self) -> Result<String, MemoryError> {
        self.record(Call::plain("scoring.embedder_slug"));
        Ok(String::new())
    }
}

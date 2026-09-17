//! `RecordingProvider`'s `SourceSink`, `Maintenance`, `SourceSync`, and
//! `CodingSessions` family implementations, plus the top-level
//! `MemoryProvider` impl that advertises every family through its `as_*`
//! accessors.
//!
//! Split out of the original `test_support.rs`; see [`super::fixtures`] for
//! `RecordingProvider`'s type and constructors. The read-heavy families
//! (`Retrieval`, `Episodic`, `Profile`, ...) and the typed-ingestion round
//! live in [`super::retrieval_and_ingest_impls`].

use crate::memory::api::capabilities::Capabilities;
use crate::memory::api::error::MemoryError;
use crate::memory::api::health::MemoryHealth;
use crate::memory::api::provider::operations::{
    MemoryAnswer, MemoryConversationIngest, MemoryDocumentIngest, MemoryEventIngest,
    MemoryLearningIngest,
};
use crate::memory::api::provider::sessions::{
    CodingSessionIngestReport, CodingSessionIngestRequest, CodingSessionSource,
};
use crate::memory::api::provider::sync::{
    RawArchiveCoverage, RawRebuildOutcome, SourceSyncState, SourceSyncStatus, SyncAuditEntry,
    SyncRunOutcome,
};
use crate::memory::api::provider::types::{IngestOutcome, MaintenanceReport, SourceItem};
use crate::memory::api::provider::{
    MemoryChunks, MemoryCodingSessions, MemoryDiff, MemoryDocuments, MemoryEntities,
    MemoryEpisodic, MemoryGoals, MemoryGraph, MemoryIngest, MemoryMaintenance, MemoryPeople,
    MemoryProfile, MemoryProvider, MemoryRetrieval, MemoryScoring, MemorySourceSink,
    MemorySourceSync, MemoryToolMemory, MemoryTree,
};
use crate::memory::api::types::MemoryTaint;
use async_trait::async_trait;

use super::fixtures::{Call, RecordingProvider};

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

    async fn diagnose(
        &self,
    ) -> Result<crate::memory::api::provider::diagnosis::Diagnosis, MemoryError> {
        self.record(Call::plain("maintenance.diagnose"));
        Ok(Default::default())
    }

    async fn degraded_state(
        &self,
    ) -> Result<crate::memory::api::provider::diagnosis::DegradedCapabilities, MemoryError> {
        self.record(Call::plain("maintenance.degraded_state"));
        Ok(Default::default())
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
    fn as_document_ingest(&self) -> Option<&dyn MemoryDocumentIngest> {
        Some(self)
    }
    fn as_conversation_ingest(&self) -> Option<&dyn MemoryConversationIngest> {
        Some(self)
    }
    fn as_learning_ingest(&self) -> Option<&dyn MemoryLearningIngest> {
        Some(self)
    }
    fn as_event_ingest(&self) -> Option<&dyn MemoryEventIngest> {
        Some(self)
    }
    fn as_answer(&self) -> Option<&dyn MemoryAnswer> {
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

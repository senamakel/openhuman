//! `MemoryGoals`, `MemoryToolMemory`, `MemorySourceSink`, and
//! `MemoryMaintenance` forwarding for [`ModuleMemoryProvider`].

use async_trait::async_trait;
use tinymemory_api::error::MemoryError;
use tinymemory_api::goals::GoalsDoc;
use tinymemory_api::provider::types::{
    BackfillTreesOutcome, BackfillTreesRequest, FlushOutcome, ForgetOutcome, ForgetSelector,
    IngestOutcome, MaintenanceReport, PurgeOutcome, QueueFailure, QueueStats, ResetOutcome,
    SourceItem, StoreStats,
};
use tinymemory_api::provider::{
    DegradedCapabilities, Diagnosis, MemoryGoals, MemoryMaintenance, MemorySourceSink,
    MemoryToolMemory,
};
use tinymemory_api::tool_memory::ToolMemoryRule;
use tinymemory_api::types::MemoryTaint;
use tinymemory_bus::names::methods;

use super::provider::{module_call, module_call_slow, ModuleMemoryProvider};

#[async_trait]
impl MemoryGoals for ModuleMemoryProvider {
    async fn goals(&self) -> Result<GoalsDoc, MemoryError> {
        module_call!(self, "goals", methods::GOALS, ())
    }
    async fn set_goals(&self, goals: GoalsDoc) -> Result<(), MemoryError> {
        module_call!(self, "set_goals", methods::SET_GOALS, (goals,))
    }
}

#[async_trait]
impl MemoryToolMemory for ModuleMemoryProvider {
    async fn tool_rules(&self, tool_name: &str) -> Result<Vec<ToolMemoryRule>, MemoryError> {
        module_call!(self, "tool_rules", methods::TOOL_RULES, (tool_name,))
    }
    async fn put_tool_rule(&self, rule: ToolMemoryRule) -> Result<(), MemoryError> {
        module_call!(self, "put_tool_rule", methods::PUT_TOOL_RULE, (rule,))
    }
    async fn delete_tool_rule(&self, tool_name: &str, rule_id: &str) -> Result<bool, MemoryError> {
        module_call!(
            self,
            "delete_tool_rule",
            methods::DELETE_TOOL_RULE,
            (tool_name, rule_id)
        )
    }
}

#[async_trait]
impl MemorySourceSink for ModuleMemoryProvider {
    async fn accept_source_items(
        &self,
        source_id: &str,
        source_kind: &str,
        items: Vec<SourceItem>,
        taint: MemoryTaint,
    ) -> Result<IngestOutcome, MemoryError> {
        module_call_slow!(
            self,
            "accept_source_items",
            methods::ACCEPT_SOURCE_ITEMS,
            (source_id, source_kind, items, taint)
        )
    }
    async fn forget_source(&self, source_id: &str) -> Result<u64, MemoryError> {
        module_call!(self, "forget_source", methods::FORGET_SOURCE, (source_id,))
    }
    async fn forget_matching(
        &self,
        selector: &ForgetSelector,
    ) -> Result<ForgetOutcome, MemoryError> {
        module_call!(
            self,
            "forget_matching",
            methods::FORGET_MATCHING,
            (selector,)
        )
    }
}

#[async_trait]
impl MemoryMaintenance for ModuleMemoryProvider {
    async fn reembed(&self) -> Result<MaintenanceReport, MemoryError> {
        module_call!(self, "reembed", methods::REEMBED, ())
    }
    async fn compact(&self) -> Result<MaintenanceReport, MemoryError> {
        module_call!(self, "compact", methods::COMPACT, ())
    }
    async fn consolidate(&self) -> Result<MaintenanceReport, MemoryError> {
        module_call!(self, "consolidate", methods::CONSOLIDATE, ())
    }
    async fn doctor(&self) -> Result<MaintenanceReport, MemoryError> {
        module_call!(self, "doctor", methods::DOCTOR, ())
    }
    async fn retry_failed(&self) -> Result<MaintenanceReport, MemoryError> {
        module_call!(self, "retry_failed", methods::RETRY_FAILED, ())
    }
    async fn store_stats(&self) -> Result<StoreStats, MemoryError> {
        module_call!(self, "store_stats", methods::STORE_STATS, ())
    }
    async fn queue_stats(&self, kind: Option<&str>) -> Result<QueueStats, MemoryError> {
        module_call!(self, "queue_stats", methods::QUEUE_STATS, (kind,))
    }
    async fn latest_queue_failure(&self) -> Result<Option<QueueFailure>, MemoryError> {
        module_call!(
            self,
            "latest_queue_failure",
            methods::LATEST_QUEUE_FAILURE,
            ()
        )
    }
    async fn backfill_in_progress(&self) -> Result<bool, MemoryError> {
        module_call!(
            self,
            "backfill_in_progress",
            methods::BACKFILL_IN_PROGRESS,
            ()
        )
    }
    async fn flush_pending(&self) -> Result<FlushOutcome, MemoryError> {
        module_call!(self, "flush_pending", methods::FLUSH_PENDING, ())
    }
    /// Long-running by nature — a pass reads and re-embeds up to its whole
    /// limit of documents — so this takes the bulk deadline rather than the
    /// default 30s one. `AcceptSourceItems` is here for the same reason: a call
    /// that outruns the deadline while the module goes on working is the
    /// pathology that made the connector sync retry a finished handoff forever.
    async fn backfill_connector_trees(
        &self,
        request: BackfillTreesRequest,
    ) -> Result<BackfillTreesOutcome, MemoryError> {
        module_call_slow!(
            self,
            "backfill_connector_trees",
            methods::BACKFILL_CONNECTOR_TREES,
            (request,)
        )
    }
    async fn reset_derived_index(&self) -> Result<ResetOutcome, MemoryError> {
        module_call!(
            self,
            "reset_derived_index",
            methods::RESET_DERIVED_INDEX,
            ()
        )
    }
    async fn purge_all(&self) -> Result<PurgeOutcome, MemoryError> {
        module_call!(self, "purge_all", methods::PURGE_ALL, ())
    }
    async fn diagnose(&self) -> Result<Diagnosis, MemoryError> {
        module_call!(self, "diagnose", methods::DIAGNOSE, ())
    }
    /// The degradation flags alone — three booleans and at most one cause,
    /// read from the atomics the module's own embed/extract/storage stages set.
    /// Deliberately not answered from [`Self::diagnose`]'s payload: a status
    /// light polls this, and `Diagnose` runs an aggregate scan of the chunk
    /// table.
    async fn degraded_state(&self) -> Result<DegradedCapabilities, MemoryError> {
        module_call!(self, "degraded_state", methods::DEGRADED_STATE, ())
    }
}

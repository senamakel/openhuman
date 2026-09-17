//! `MemorySourceSync`, `MemoryCodingSessions`, and `MemoryEpisodic`
//! forwarding for [`ModuleMemoryProvider`].

use async_trait::async_trait;
use tinymemory_api::error::MemoryError;
use tinymemory_api::provider::sessions::{
    CodingSessionIngestReport, CodingSessionIngestRequest, CodingSessionSource,
};
use tinymemory_api::provider::sync::{
    RawArchiveCoverage, RawRebuildOutcome, SourceSyncState, SourceSyncStatus, SyncAuditEntry,
    SyncRunOutcome,
};
use tinymemory_api::provider::{
    ConversationSegment, EpisodicEvent, EpisodicTurn, MemoryCodingSessions, MemoryEpisodic,
    MemorySourceSync,
};
use tinymemory_bus::names::methods;

use super::provider::{from_bus, module_call, ModuleMemoryProvider};

/// Bus deadline for the three calls that run a whole source sync inside the
/// module: `RunConnectionSync`, `RunSourceSync` and `BootstrapConnection`.
///
/// tinybus gives every call a 30 s default deadline if nobody sets one, and a
/// sync is routinely longer than that: one Gmail page is ~31 s end to end, an
/// initial bootstrap of a connection is minutes. With the default, the caller
/// was released with "call to `RunSourceSync` timed out after 30000ms" while
/// the module kept fetching and ingesting, and finished; the UI reported a
/// failure for work that succeeded (openhuman#5820). Same failure class, same
/// fix as `IngestCodingSessions` above: the deadline here is the wedged-forever
/// backstop tinybus requires, not a ceiling anyone is meant to hit.
///
/// Sized from the frontend's clamp, `PER_CALL_TIMEOUT_MAX_MS = 600 s`
/// (`app/src/services/coreRpcClient.ts`): that is the longest wait any RPC
/// caller can observe, so the bus must outlast it, plus [`INGEST_BUS_GRACE`]
/// so the client's own abort, with its clean message, is the one that fires
/// first when a run really does wedge.
const SOURCE_SYNC_BUS_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(600).saturating_add(INGEST_BUS_GRACE);

#[async_trait]
impl MemorySourceSync for ModuleMemoryProvider {
    async fn run_connection_sync(
        &self,
        toolkit: &str,
        connection_id: &str,
    ) -> Result<SyncRunOutcome, MemoryError> {
        self.proxy("run_connection_sync")
            .await?
            .with_timeout(SOURCE_SYNC_BUS_TIMEOUT)
            .call(methods::RUN_CONNECTION_SYNC, (toolkit, connection_id))
            .await
            .map_err(|error| from_bus(&error))
    }
    async fn run_source_sync(&self, source_id: &str) -> Result<SyncRunOutcome, MemoryError> {
        self.proxy("run_source_sync")
            .await?
            .with_timeout(SOURCE_SYNC_BUS_TIMEOUT)
            .call(methods::RUN_SOURCE_SYNC, (source_id,))
            .await
            .map_err(|error| from_bus(&error))
    }
    async fn bootstrap_connection(
        &self,
        toolkit: &str,
        connection_id: &str,
    ) -> Result<(), MemoryError> {
        self.proxy("bootstrap_connection")
            .await?
            .with_timeout(SOURCE_SYNC_BUS_TIMEOUT)
            .call(methods::BOOTSTRAP_CONNECTION, (toolkit, connection_id))
            .await
            .map_err(|error| from_bus(&error))
    }
    async fn is_toolkit_syncable(&self, toolkit: &str) -> Result<bool, MemoryError> {
        module_call!(
            self,
            "is_toolkit_syncable",
            methods::IS_TOOLKIT_SYNCABLE,
            (toolkit,)
        )
    }
    async fn source_sync_state(
        &self,
        toolkit: &str,
        connection_id: &str,
    ) -> Result<Option<SourceSyncState>, MemoryError> {
        module_call!(
            self,
            "source_sync_state",
            methods::SOURCE_SYNC_STATE,
            (toolkit, connection_id)
        )
    }
    async fn sync_audit_log(
        &self,
        limit: Option<usize>,
    ) -> Result<Vec<SyncAuditEntry>, MemoryError> {
        module_call!(self, "sync_audit_log", methods::SYNC_AUDIT_LOG, (limit,))
    }
    async fn estimate_sync_cost_usd(
        &self,
        input_tokens: u64,
        output_tokens: u64,
    ) -> Result<f64, MemoryError> {
        module_call!(
            self,
            "estimate_sync_cost_usd",
            methods::ESTIMATE_SYNC_COST_USD,
            (input_tokens, output_tokens)
        )
    }
    async fn sync_statuses(&self) -> Result<Vec<SourceSyncStatus>, MemoryError> {
        module_call!(self, "sync_statuses", methods::SYNC_STATUSES, ())
    }
    async fn raw_archive_coverage(
        &self,
        tree_scope: &str,
        archive_source_id: &str,
    ) -> Result<RawArchiveCoverage, MemoryError> {
        module_call!(
            self,
            "raw_archive_coverage",
            methods::RAW_ARCHIVE_COVERAGE,
            (tree_scope, archive_source_id)
        )
    }
    async fn rebuild_from_raw_archive(
        &self,
        tree_scope: &str,
        archive_source_id: &str,
    ) -> Result<RawRebuildOutcome, MemoryError> {
        module_call!(
            self,
            "rebuild_from_raw_archive",
            methods::REBUILD_FROM_RAW_ARCHIVE,
            (tree_scope, archive_source_id)
        )
    }
}

#[async_trait]
impl MemoryCodingSessions for ModuleMemoryProvider {
    async fn coding_session_status(&self) -> Result<Vec<CodingSessionSource>, MemoryError> {
        module_call!(
            self,
            "coding_session_status",
            methods::CODING_SESSION_STATUS,
            ()
        )
    }
    /// # Why this one call sets its own bus deadline
    ///
    /// Every other member here takes tinybus' `DEFAULT_TIMEOUT`
    /// (`vendor/tinybus/crates/tinybus/src/connection.rs:56`) — a flat 30 s,
    /// applied by `Proxy::new` (`proxy.rs:59`) whenever nobody says otherwise.
    /// That is the right default for a memory read. It is the wrong one here:
    /// distilling a coding session is several *sequential* model calls, and the
    /// RPC above it already computes a budget sized to the work
    /// (`memory::sources::rpc::ingest_budget`, 120 s + 90 s per session, capped
    /// at 600 s).
    ///
    /// So there were two deadlines and the tighter one was the one nobody
    /// chose. A real 35 s import tripped the 30 s default; the caller was
    /// released with an error while the module kept working and finished
    /// seconds later, having imported everything. The UI reported a failure for
    /// work that had succeeded, and invited a retry that would redo it
    /// (#5802).
    ///
    /// tinybus is explicit that this is the caller's problem to size: *"A
    /// timeout does not cancel the remote work — tinybus cannot — it stops
    /// waiting and frees the caller"* (`connection.rs:22-23`). Abandoning the
    /// call early therefore does not save anything; it only loses the report.
    ///
    /// The budget is taken from `ingest_budget` rather than restated, so the
    /// two layers cannot drift, plus [`INGEST_BUS_GRACE`]. The grace makes the
    /// ordering deterministic instead of a race between two equal deadlines:
    /// the RPC's own `tokio::time::timeout` fires first and reports its clean
    /// structured message, and this deadline survives only as the
    /// wedged-forever backstop tinybus requires. Same shape as the client's
    /// `CODING_SESSION_RPC_GRACE_MS` sitting above the server budget.
    async fn ingest_coding_sessions(
        &self,
        request: CodingSessionIngestRequest,
    ) -> Result<CodingSessionIngestReport, MemoryError> {
        let deadline =
            crate::memory::sources::rpc::ingest_budget(request.max_sessions) + INGEST_BUS_GRACE;
        self.proxy("ingest_coding_sessions")
            .await?
            .with_timeout(deadline)
            .call(methods::INGEST_CODING_SESSIONS, (request,))
            .await
            .map_err(|error| from_bus(&error))
    }
}

/// Head-room added to [`ingest_budget`](crate::memory::sources::rpc::ingest_budget)
/// for the bus deadline on `IngestCodingSessions`.
///
/// Exists to order two deadlines, not to allow more work: the RPC's own
/// wall-clock ceiling must be the one that fires, because its message names the
/// budget rather than the wire member. Anything comfortably longer than the
/// scheduling jitter between the two `tokio::time::timeout` arms would do.
pub(super) const INGEST_BUS_GRACE: std::time::Duration = std::time::Duration::from_secs(30);

#[async_trait]
impl MemoryEpisodic for ModuleMemoryProvider {
    async fn insert_turn(&self, turn: &EpisodicTurn) -> Result<i64, MemoryError> {
        module_call!(self, "insert_turn", methods::INSERT_TURN, (turn,))
    }
    async fn session_turns(&self, session_id: &str) -> Result<Vec<EpisodicTurn>, MemoryError> {
        module_call!(self, "session_turns", methods::SESSION_TURNS, (session_id,))
    }
    async fn open_segment(
        &self,
        session_id: &str,
    ) -> Result<Option<ConversationSegment>, MemoryError> {
        module_call!(self, "open_segment", methods::OPEN_SEGMENT, (session_id,))
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
        module_call!(
            self,
            "create_segment",
            methods::CREATE_SEGMENT,
            (
                segment_id,
                session_id,
                namespace,
                start_episodic_id,
                start_seq,
                start_timestamp,
                now
            )
        )
    }
    async fn append_turn(
        &self,
        segment_id: &str,
        episodic_id: i64,
        seq: Option<u32>,
        timestamp: f64,
        now: f64,
    ) -> Result<(), MemoryError> {
        module_call!(
            self,
            "append_turn",
            methods::APPEND_TURN,
            (segment_id, episodic_id, seq, timestamp, now)
        )
    }
    async fn insert_event(&self, event: &EpisodicEvent) -> Result<(), MemoryError> {
        module_call!(self, "insert_event", methods::INSERT_EVENT, (event,))
    }
    async fn close_segment(&self, segment_id: &str, now: f64) -> Result<(), MemoryError> {
        module_call!(
            self,
            "close_segment",
            methods::CLOSE_SEGMENT,
            (segment_id, now)
        )
    }
    async fn set_segment_summary(
        &self,
        segment_id: &str,
        summary: &str,
        now: f64,
    ) -> Result<(), MemoryError> {
        module_call!(
            self,
            "set_segment_summary",
            methods::SET_SEGMENT_SUMMARY,
            (segment_id, summary, now)
        )
    }
    /// Forwarded rather than left to the trait default (#6186). The default
    /// answers an empty list, which here would read as "no segment needs
    /// re-summarising" — indistinguishable from a healthy store, and silent.
    async fn segments_pending_summary(
        &self,
        limit: u32,
    ) -> Result<Vec<ConversationSegment>, MemoryError> {
        module_call!(
            self,
            "segments_pending_summary",
            methods::SEGMENTS_PENDING_SUMMARY,
            (limit,)
        )
    }
    async fn upsert_segment_embedding(
        &self,
        segment_id: &str,
        model_signature: &str,
        embedding: &[f32],
        created_at: f64,
    ) -> Result<(), MemoryError> {
        module_call!(
            self,
            "upsert_segment_embedding",
            methods::UPSERT_SEGMENT_EMBEDDING,
            (segment_id, model_signature, embedding, created_at)
        )
    }
}

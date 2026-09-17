//! Fire-and-forget background tasks kicked off after a turn settles:
//! archivist session-memory extraction and transcript ingestion.

use crate::agent::context::ARCHIVIST_EXTRACTION_PROMPT;
use crate::agent::harness;
use crate::agent::harness::session::types::Agent;

impl Agent {
    // ─────────────────────────────────────────────────────────────────
    // Session-memory extraction.
    // ─────────────────────────────────────────────────────────────────

    /// Spawn a background archivist sub-agent to extract durable facts
    /// from the recent conversation into `MEMORY.md`. Fire-and-forget.
    ///
    /// Gated by [`context_pipeline::SessionMemoryState::should_extract`]
    /// — see its docs for the threshold invariants. Safe to call from
    /// inside `turn()` after the turn body has settled.
    pub(in super::super) async fn spawn_session_memory_extraction(
        &mut self,
        parent_ctx: harness::ParentExecutionContext,
    ) {
        // ── Flush the trailing open segment before the session winds down ──
        //
        // The ArchivistHook manages per-turn segment lifecycle but cannot
        // force-close the *last* open segment because there is no explicit
        // "session end" event in the turn loop. `spawn_session_memory_extraction`
        // is the closest available signal: it fires when the context manager
        // decides the session has accumulated enough material to archive.
        //
        // GUARANTEE: the flush is *awaited* here (not fire-and-forget) so
        // the trailing segment always receives its recap + embedding + tree
        // ingest before the function returns, even during runtime wind-down.
        // This honours the doc-comment guarantee on `flush_open_segment` in
        // `archivist.rs`. No deadlock risk: no mutex guard is held across
        // this await point.
        if let Some(ref archivist) = self.archivist_hook {
            let session_id = self.event_session_id.clone();
            log::debug!(
                "[archivist] awaiting flush_open_segment for session={session_id} at session wind-down"
            );
            archivist.flush_open_segment(&session_id).await;
        }

        let Some(registry) = harness::AgentDefinitionRegistry::global() else {
            log::debug!("[session_memory] registry not initialised — skipping extraction spawn");
            return;
        };
        let Some(definition) = registry.get("archivist").cloned() else {
            log::debug!(
                "[session_memory] archivist definition not found — skipping extraction spawn"
            );
            return;
        };

        let extraction_prompt = ARCHIVIST_EXTRACTION_PROMPT.to_string();

        // Flip the extraction state to "in-progress" so future
        // should_extract checks return false until the archivist
        // finishes. We then hand a shared handle to the spawned task
        // so it can mark the extraction complete (resets deltas) on
        // success, or failed (keeps deltas intact for retry) on error.
        // This replaces the old optimistic `mark_complete` that
        // silently dropped the retry window when extractions failed.
        let stats_snapshot = self.context.stats();
        self.context.mark_session_memory_started();
        let sm_handle = self.context.session_memory_handle();

        log::info!(
            "[session_memory] spawning background archivist extraction (turn={}, tokens={})",
            stats_snapshot.session_memory_current_turn,
            stats_snapshot.session_memory_total_tokens
        );

        tokio::spawn(async move {
            let options = harness::SubagentRunOptions::default();
            let fut = harness::run_subagent(&definition, &extraction_prompt, options);
            let result = harness::with_parent_context(parent_ctx, fut).await;
            match result {
                Ok(outcome) => {
                    tracing::info!(
                        agent_id = %outcome.agent_id,
                        task_id = %outcome.task_id,
                        iterations = outcome.iterations,
                        output_chars = outcome.output.chars().count(),
                        "[session_memory] archivist extraction completed"
                    );
                    if let Ok(mut sm) = sm_handle.lock() {
                        sm.mark_extraction_complete();
                    }
                }
                Err(err) => {
                    tracing::warn!(
                        error = %err,
                        "[session_memory] archivist extraction failed — will retry after next threshold crossing"
                    );
                    // Leave the deltas intact so the next threshold
                    // crossing schedules another attempt. Clearing
                    // `extraction_in_progress` lets the retry
                    // actually fire.
                    if let Ok(mut sm) = sm_handle.lock() {
                        sm.mark_extraction_failed();
                    }
                }
            }
        });
    }

    /// Spawn a background task that ingests the current session
    /// transcript into the conversational-memory store.
    ///
    /// Issue #1399: complements `spawn_session_memory_extraction`. The
    /// archivist path writes dense bullets into `MEMORY.md`; this path
    /// extracts importance-tagged, provenance-bearing memories via the
    /// heuristic [`crate::agent::learning::transcript_ingest`]
    /// pipeline. The two are deliberately independent so the prompt
    /// retrieval layer can pull from `conversation_memory` without
    /// needing the archivist's extraction to have fired this session.
    ///
    /// Fire-and-forget: failures are logged, never propagated.
    pub(in super::super) fn spawn_transcript_ingestion(&self) {
        let Some(path) = self.session_transcript_path.clone() else {
            log::debug!("[transcript_ingest] no session transcript path yet — skipping spawn");
            return;
        };
        let memory = std::sync::Arc::clone(&self.memory);

        tokio::spawn(async move {
            match crate::agent::learning::transcript_ingest::ingest_transcript_path(
                memory.as_ref(),
                &path,
            )
            .await
            {
                Ok(report) => tracing::info!(
                    transcript = %path.display(),
                    extracted = report.extracted,
                    stored = report.stored,
                    deduped = report.deduped,
                    reflections_stored = report.reflections_stored,
                    "[transcript_ingest] background ingest complete"
                ),
                Err(err) => tracing::warn!(
                    transcript = %path.display(),
                    error = %err,
                    "[transcript_ingest] background ingest failed — will retry next threshold window"
                ),
            }
        });
    }
}

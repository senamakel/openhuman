//! Summarization and rolling recap logic for `ArchivistHook`.

use super::types::ArchivistHook;
use crate::openhuman::memory::api::provider::{ConversationSegment, EpisodicTurn};
use crate::openhuman::memory::tree::summarise::{summarise, SummaryContext, SummaryInput};
#[cfg(test)]
use std::sync::Arc;
// `SummaryContext::tree_kind` is TinyCortex's own enum, and
// `tinymemory_core::store::trees::types::TreeKind` was a re-export of exactly
// this item. Naming the owning crate changes no type — only which crate alias
// holds it (#5560).
use tinycortex::memory::tree::store::TreeKind;

/// An episodic entry paired with the stable identity exposed by its backing
/// store. The md archivist uses a per-session sequence while the legacy FTS5
/// store uses a row id.
pub(super) struct SessionEntry {
    pub(super) turn: EpisodicTurn,
    sequence: Option<u32>,
}

impl SessionEntry {
    /// Whether this entry belongs to a closed segment.
    ///
    /// Segment endpoints identify user turns. Each user turn is immediately
    /// followed by its assistant entry, so the inclusive span ends one entry
    /// after the recorded end user turn.
    pub(super) fn is_in_segment(&self, segment: &ConversationSegment) -> bool {
        if let (Some(sequence), Some(start)) = (self.sequence, segment.start_seq) {
            let end = segment.end_seq.unwrap_or(start).saturating_add(1);
            return sequence >= start && sequence <= end;
        }

        if let Some(id) = self.turn.id {
            let start = segment.start_episodic_id;
            let end = segment.end_episodic_id.unwrap_or(start).saturating_add(1);
            return id >= start && id <= end;
        }

        self.turn.timestamp >= segment.start_timestamp
            && segment
                .end_timestamp
                .map(|end| self.turn.timestamp <= end)
                .unwrap_or(true)
    }

    /// Whether this entry belongs to the open segment or a later turn.
    pub(super) fn is_at_or_after_segment_start(&self, segment: &ConversationSegment) -> bool {
        if let (Some(sequence), Some(start)) = (self.sequence, segment.start_seq) {
            return sequence >= start;
        }

        if let Some(id) = self.turn.id {
            return id >= segment.start_episodic_id;
        }

        self.turn.timestamp >= segment.start_timestamp
    }
}

impl ArchivistHook {
    /// Read every entry recorded for `session_id`, preferring the
    /// crate-owned md-backed archivist store when `self.config` is set and
    /// falling back to the legacy FTS5 episodic table otherwise.
    ///
    /// Each entry retains the stable sequence or row identity needed for
    /// segment selection. Timestamps are only a fallback for legacy records:
    /// the md store records epoch milliseconds and therefore cannot preserve
    /// the sub-millisecond timestamps used when a segment is opened.
    pub(super) async fn read_session_entries(&self, session_id: &str) -> Vec<SessionEntry> {
        if let Some(cfg) = self.config.as_ref() {
            // Workspace-rooted and nothing else, for the same reason as the
            // write side in `hook_impl.rs`: `session_entries` resolves its
            // directory through the archivist store's own private
            // `<workspace>/memory_tree/content` root and reads neither
            // `content_root` nor `embedding`. See that call site for why this
            // replaced `tinymemory_core::tinycortex::memory_config_from`.
            let engine_config = tinycortex::memory::MemoryConfig::new(cfg.workspace_dir.clone());
            match tinycortex::memory::archivist::store::session_entries(&engine_config, session_id)
            {
                Ok(turns) => {
                    return turns
                        .into_iter()
                        .map(|t| SessionEntry {
                            sequence: Some(t.seq),
                            turn: EpisodicTurn {
                                id: None,
                                session_id: t.session_id,
                                // ArchivedTurn stores epoch-ms; the contract
                                // turn takes epoch-seconds as f64.
                                timestamp: (t.timestamp_ms as f64) / 1000.0,
                                role: t.role,
                                content: t.content,
                                lesson: t.lesson,
                                tool_calls_json: t.tool_calls_json,
                                // The engine column is unsigned; the wire is a
                                // plain signed number.
                                cost_microdollars: i64::try_from(t.cost_microdollars)
                                    .unwrap_or(i64::MAX),
                            },
                        })
                        .collect();
                }
                Err(e) => {
                    tracing::warn!(
                        "[archivist] memory_archivist read failed (falling back to FTS5): {e}"
                    );
                }
            }
        }
        let Some(episodic) = self.episodic() else {
            return Vec::new();
        };
        episodic
            .session_turns(session_id)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|turn| SessionEntry {
                turn,
                sequence: None,
            })
            .collect()
    }

    /// Shared summarize helper — the **single LLM summarizer** used by both
    /// the finalize path (`on_segment_closed`) and the rolling-recap path
    /// (`rolling_segment_recap`).
    ///
    /// Builds a prose corpus from `entries`, calls the `LlmSummariser` when a
    /// summariser is available for this workspace, and falls back to the
    /// heuristic `segments::fallback_summary` on any failure or when none is.
    /// Always returns a non-empty string.
    ///
    /// Invariants:
    /// - NEVER mutates DB state (no `segment_set_summary`, no embedding).
    /// - NEVER closes a segment.
    /// - Safe to call on both open and closed segments.
    ///
    /// Summarize a set of episodic entries into a recap string.
    ///
    /// Returns `(text, produced_by_llm)`. `produced_by_llm == false` means the
    /// LLM was unavailable / failed / returned empty and `text` is the shallow
    /// heuristic `fallback_summary` bookend stub. That stub is an acceptable
    /// durable last-resort on the *finalize* path, but callers driving the
    /// **live prompt** (rolling recap → compaction) must treat
    /// `produced_by_llm == false` as "no real recap" and fall back to their
    /// own strategy — the stub must never become live compaction text.
    pub(super) async fn summarize_entries(
        &self,
        entries: &[&EpisodicTurn],
        segment_id: &str,
        turn_count: i32,
    ) -> (String, bool) {
        if entries.is_empty() {
            tracing::debug!(
                "[archivist] summarize_entries: no entries for segment={segment_id} — \
                 returning empty fallback"
            );
            return (super::boundary::fallback_summary("", "", turn_count), false);
        }

        // Build a full prose corpus from ALL entries (user + assistant prose;
        // tool-call JSON is already excluded because the archivist stores
        // stripped prose in the `content` column).
        let corpus_inputs: Vec<SummaryInput> = entries
            .iter()
            .filter(|e| !e.content.trim().is_empty())
            .map(|e| {
                use tinymemory_api::chunks::approx_token_count;
                let content = e.content.clone();
                let token_count = approx_token_count(&content);
                let ts = chrono::DateTime::from_timestamp(e.timestamp as i64, 0)
                    .unwrap_or_else(chrono::Utc::now);
                SummaryInput {
                    id: format!("{}-{}", e.role, e.timestamp as u64),
                    content,
                    token_count,
                    entities: Vec::new(),
                    topics: Vec::new(),
                    time_range_start: ts,
                    time_range_end: ts,
                    score: 0.5,
                }
            })
            .collect();

        let summary_ctx = SummaryContext {
            tree_id: segment_id,
            tree_kind: TreeKind::Source,
            target_level: 0,
            token_budget: 2_000,
            input_token_budget: tinycortex::memory::config::INPUT_TOKEN_BUDGET,
            overhead_reserve_tokens: tinycortex::memory::config::SUMMARY_OVERHEAD_RESERVE_TOKENS,
            ask: None,
        };

        let first = entries.first().map(|e| e.content.as_str()).unwrap_or("");
        let last = entries.last().map(|e| e.content.as_str()).unwrap_or(first);

        // Was `self.chat_provider.is_some()`. Same predicate — the archivist
        // never called that handle, it only asked whether one could be built —
        // recorded as the boolean it always was. See `lifecycle::with_config`.
        if self.summariser_available {
            if let Some(ref config) = self.config {
                tracing::debug!(
                    "[archivist] summarize_entries: LLM recap segment={segment_id} entries={}",
                    entries.len()
                );
                // Test-only: `summarise` builds its own chat provider, and
                // `build_chat_runtime` consults this task-local before building
                // one. Scoping the call is what keeps the recap tests off the
                // network. Production has no such override and never names the
                // engine's chat module (#5560).
                #[cfg(test)]
                let summary_result = if let Some(provider) = self.chat_provider.as_ref() {
                    tinymemory_core::chat::test_override::with_provider(
                        Arc::clone(provider),
                        summarise(config, &corpus_inputs, &summary_ctx),
                    )
                    .await
                } else {
                    summarise(config, &corpus_inputs, &summary_ctx).await
                };
                #[cfg(not(test))]
                let summary_result = summarise(config, &corpus_inputs, &summary_ctx).await;

                match summary_result {
                    Ok(output) if !output.content.is_empty() => {
                        tracing::debug!(
                            "[archivist] summarize_entries: LLM recap ok segment={segment_id} \
                             chars={}",
                            output.content.len()
                        );
                        return (output.content, true);
                    }
                    Ok(_) => {
                        tracing::debug!(
                            "[archivist] summarize_entries: LLM returned empty — \
                             heuristic fallback segment={segment_id}"
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            "[archivist] summarize_entries: LLM recap failed (non-fatal) \
                             segment={segment_id}: {e} — heuristic fallback"
                        );
                    }
                }
            } else {
                tracing::debug!(
                    "[archivist] summarize_entries: no config — \
                     heuristic fallback segment={segment_id}"
                );
            }
        } else {
            tracing::debug!(
                "[archivist] summarize_entries: no summariser available — \
                 heuristic fallback segment={segment_id}"
            );
        }
        (
            super::boundary::fallback_summary(first, last, turn_count),
            false,
        )
    }

    /// Produce a rolling recap of the **currently-open** segment for
    /// `session_id` WITHOUT closing it, writing `segment_set_summary`, or
    /// embedding.
    ///
    /// This is the Phase 1.5 "one summarizer" entry point. Both
    /// `on_segment_closed` (finalize) and this function delegate to the same
    /// [`Self::summarize_entries`] helper so the same LLM path is used in both
    /// cases. The distinction is purely in what happens *after* the summary
    /// string is produced:
    ///
    /// - **Finalize** (`on_segment_closed`): persists the summary via
    ///   `segment_set_summary`, embeds it, extracts events, pipes tree ingest.
    /// - **Rolling** (this function): returns the summary string and does
    ///   nothing else — segment stays open, DB is untouched.
    ///
    /// Returns `None` when:
    /// - The archivist is disabled or has no connection.
    /// - There is no open segment for `session_id`.
    /// - The open segment has no episodic entries.
    /// - No real LLM recap was produced (LLM unavailable / failed / empty, so
    ///   only the heuristic bookend stub is available). The shallow stub is
    ///   deliberately NOT used as live compaction text.
    ///
    /// Callers must treat `None` as "recap unavailable" and fall back to
    /// their own compaction strategy (e.g. `ProviderSummarizer`).
    pub async fn rolling_segment_recap(&self, session_id: &str) -> Option<String> {
        if !self.enabled {
            tracing::debug!(
                "[archivist] rolling_segment_recap: archivist disabled \
                 session={session_id} — returning None"
            );
            return None;
        }
        let episodic = self.episodic()?;

        // Find the currently-open segment for this session.
        let open_segment = match episodic.open_segment(session_id).await {
            Ok(Some(seg)) => seg,
            Ok(None) => {
                tracing::debug!(
                    "[archivist] rolling_segment_recap: no open segment for \
                     session={session_id} — returning None"
                );
                return None;
            }
            Err(e) => {
                tracing::warn!(
                    "[archivist] rolling_segment_recap: failed to query open segment \
                     session={session_id}: {e} — returning None"
                );
                return None;
            }
        };

        // Gather the episodic entries for this session so far.
        let all_entries = self.read_session_entries(session_id).await;

        // Keep only entries belonging to the open segment. Prefer stable
        // sequence/row identity because the md store rounds timestamps to ms.
        let segment_entries: Vec<&EpisodicTurn> = all_entries
            .iter()
            .filter(|record| record.is_at_or_after_segment_start(&open_segment))
            .map(|record| &record.turn)
            .collect();

        if segment_entries.is_empty() {
            tracing::debug!(
                "[archivist] rolling_segment_recap: no entries in open segment={} \
                 session={session_id} — returning None",
                open_segment.segment_id
            );
            return None;
        }

        tracing::debug!(
            "[archivist] rolling_segment_recap: summarizing open segment={} \
             entries={} session={session_id}",
            open_segment.segment_id,
            segment_entries.len()
        );

        let (recap, from_llm) = self
            .summarize_entries(
                &segment_entries,
                &open_segment.segment_id,
                open_segment.turn_count,
            )
            .await;

        if !from_llm {
            tracing::debug!(
                "[archivist] rolling_segment_recap: only heuristic bookend stub \
                 available (no real LLM recap) session={session_id} segment={} — \
                 returning None",
                open_segment.segment_id
            );
            return None;
        }

        if recap.is_empty() {
            tracing::debug!(
                "[archivist] rolling_segment_recap: summarize_entries returned empty \
                 session={session_id} segment={} — returning None",
                open_segment.segment_id
            );
            return None;
        }

        tracing::debug!(
            "[archivist] rolling_segment_recap: produced LLM recap chars={} \
             session={session_id} segment={}",
            recap.len(),
            open_segment.segment_id
        );
        Some(recap)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment() -> ConversationSegment {
        ConversationSegment {
            segment_id: "segment".into(),
            session_id: "session".into(),
            namespace: "global".into(),
            start_episodic_id: 20,
            end_episodic_id: Some(24),
            start_timestamp: 100.000_9,
            end_timestamp: Some(100.001_1),
            turn_count: 3,
            summary: None,
            embedding: None,
            open: false,
            start_seq: Some(10),
            end_seq: Some(14),
        }
    }

    fn entry(sequence: Option<u32>, id: Option<i64>, timestamp: f64) -> SessionEntry {
        SessionEntry {
            sequence,
            turn: EpisodicTurn {
                id,
                session_id: "session".into(),
                timestamp,
                role: "user".into(),
                content: "content".into(),
                lesson: None,
                tool_calls_json: None,
                cost_microdollars: 0,
            },
        }
    }

    #[test]
    fn segment_membership_uses_sequence_instead_of_rounded_timestamp() {
        let segment = segment();

        assert!(!entry(Some(9), None, 100.001).is_in_segment(&segment));
        assert!(entry(Some(10), None, 100.000).is_in_segment(&segment));
        assert!(entry(Some(15), None, 101.0).is_in_segment(&segment));
        assert!(!entry(Some(16), None, 100.001).is_in_segment(&segment));
    }

    #[test]
    fn segment_membership_falls_back_to_episodic_id() {
        let mut segment = segment();
        segment.start_seq = None;
        segment.end_seq = None;

        assert!(!entry(None, Some(19), 100.001).is_in_segment(&segment));
        assert!(entry(None, Some(20), 100.000).is_in_segment(&segment));
        assert!(entry(None, Some(25), 101.0).is_in_segment(&segment));
        assert!(!entry(None, Some(26), 100.001).is_in_segment(&segment));
    }
}

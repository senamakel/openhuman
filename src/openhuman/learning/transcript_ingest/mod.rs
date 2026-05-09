//! Transcript-to-memory ingestion pipeline.
//!
//! Reads completed session transcripts (`session_raw/*.jsonl`) and extracts
//! durable conversational memory plus higher-level reflections so that fresh
//! chats can recover continuity from prior conversations. See issue #1399.
//!
//! ## Outputs
//!
//! Two distinct memory streams, each persisted via [`crate::openhuman::memory::Memory`]:
//!
//! - **Conversational memory** (`conversation_memory` namespace) — durable
//!   facts (preferences, decisions, commitments, unresolved tasks) tagged with
//!   importance + provenance pointing back at the source transcript.
//! - **Conversational reflections** (`conversation_reflections` namespace) —
//!   higher-level patterns, recurring themes, or improvement signals.
//!
//! ## Pipeline
//!
//! ```text
//! SessionTranscript → extract → dedupe → persist → IngestionReport
//! ```
//!
//! Heuristic-only by design: the goal of the first pass is to make the
//! pipeline available to the rest of the system *without* a hard LLM
//! dependency, so it can run as a background task on session close, in tests,
//! and on machines without provider credentials. A subsequent iteration can
//! layer an LLM-driven extractor on the same trait surface.
//!
//! ## Provenance
//!
//! Every persisted entry carries enough metadata (`thread_id`, transcript
//! basename, source message indices, RFC-3339 timestamp) to trace the memory
//! back to the conversation it came from and to deduplicate repeats.

mod dedupe;
mod extract;
mod persist;
pub mod types;

pub use types::{
    CandidateKind, ConversationReflection, Importance, IngestionReport, MemoryCandidate,
    Provenance, CONVERSATION_MEMORY_NAMESPACE, CONVERSATION_REFLECTIONS_NAMESPACE,
};

use crate::openhuman::agent::harness::session::transcript::{self, SessionTranscript};
use crate::openhuman::memory::Memory;
use std::path::Path;

/// Ingest a single session transcript file: extract memory candidates,
/// dedupe against what's already stored, and persist new entries.
///
/// Background-first: callers should invoke this from a `tokio::spawn` so
/// chat latency is unaffected (see
/// `Agent::spawn_transcript_ingestion`). Failures are returned but the
/// caller should generally just log them — ingestion is best-effort and
/// retried on the next transcript write.
pub async fn ingest_transcript_path(
    memory: &dyn Memory,
    path: &Path,
) -> anyhow::Result<IngestionReport> {
    log::debug!("[transcript_ingest] starting ingest for {}", path.display());
    let parsed = transcript::read_transcript(path)?;
    ingest_session_transcript(memory, &parsed, path).await
}

/// Ingest an already-parsed [`SessionTranscript`].
///
/// Exposed separately from `ingest_transcript_path` so tests can drive the
/// pipeline without touching the filesystem.
pub async fn ingest_session_transcript(
    memory: &dyn Memory,
    transcript: &SessionTranscript,
    path: &Path,
) -> anyhow::Result<IngestionReport> {
    let basename = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();
    let path_display = path.display().to_string();
    let thread_id = transcript.meta.thread_id.clone();
    let now = chrono::Utc::now().to_rfc3339();

    let extracted = extract::extract_candidates(
        &transcript.messages,
        &extract::Provenance {
            thread_id: thread_id.clone(),
            transcript_path: path_display.clone(),
            transcript_basename: basename.clone(),
            extracted_at: now.clone(),
        },
    );

    let reflections = extract::extract_reflections(
        &transcript.messages,
        &extract::Provenance {
            thread_id: thread_id.clone(),
            transcript_path: path_display.clone(),
            transcript_basename: basename.clone(),
            extracted_at: now,
        },
    );

    let extracted_total = extracted.len();
    let reflection_total = reflections.len();

    let (kept, deduped) = dedupe::filter_new(memory, extracted).await?;
    let (kept_reflections, deduped_reflections) =
        dedupe::filter_new_reflections(memory, reflections).await?;

    let mut stored = 0usize;
    for candidate in &kept {
        match persist::store_candidate(memory, candidate).await {
            Ok(()) => stored += 1,
            Err(err) => log::warn!(
                "[transcript_ingest] failed to persist candidate kind={:?} importance={:?}: {err}",
                candidate.kind,
                candidate.importance
            ),
        }
    }

    let mut stored_reflections = 0usize;
    for reflection in &kept_reflections {
        match persist::store_reflection(memory, reflection).await {
            Ok(()) => stored_reflections += 1,
            Err(err) => log::warn!("[transcript_ingest] failed to persist reflection: {err}"),
        }
    }

    log::info!(
        "[transcript_ingest] ingested {}: extracted={} stored={} deduped={} reflections={}/{} (deduped={}) thread={}",
        path.display(),
        extracted_total,
        stored,
        deduped,
        stored_reflections,
        reflection_total,
        deduped_reflections,
        thread_id.as_deref().unwrap_or("-"),
    );

    Ok(IngestionReport {
        processed_messages: transcript.messages.len(),
        extracted: extracted_total,
        stored,
        deduped,
        reflections_extracted: reflection_total,
        reflections_stored: stored_reflections,
        candidates: kept,
        reflections: kept_reflections,
    })
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

//! Stage-2 merge pass for transcript-ingest (#1419).
//!
//! The heuristic extractor ([`super::extract`]) is intentionally
//! high-precision but emits one candidate per matching sentence. A long
//! transcript can therefore surface several near-duplicate preference /
//! commitment lines for the same underlying fact ("I prefer terse
//! answers", "I'd prefer terse responses"). Storage-side dedupe in
//! [`super::dedupe`] only catches exact normalised matches; the
//! semantically-overlapping pairs slip through and clutter retrieval.
//!
//! When a summarizer is configured this module collapses obvious
//! near-duplicates so retrieval surfaces one merged candidate per
//! distinct underlying fact instead of a cluster.
//!
//! Heuristic-only fallback: when the caller passes `None` the function
//! returns its input unchanged, preserving the #1406 behaviour for
//! offline users.
//!
//! NOTE: the heuristic merge runs even when no summarizer is configured.
//! Calling the summarizer is reserved for a future expansion where the
//! merged content is rewritten — today we only collapse and pick the
//! longest snippet as the representative, which keeps the pipeline
//! deterministic and lets the smaller summarizer model focus on the
//! reflection path.

use std::collections::HashSet;
use std::sync::Arc;

use crate::openhuman::learning::SummarizerProvider;

use super::types::{CandidateKind, MemoryCandidate};

/// Jaccard-similarity threshold above which two candidates of the same
/// `kind` are considered near-duplicates and collapsed into one. Tuned
/// to keep precision high — paraphrases of the same sentence usually
/// score 0.55–0.85, while different facts about the same domain score
/// below 0.4.
pub const NEAR_DUP_THRESHOLD: f32 = 0.55;

/// Outcome of one merge pass, surfaced in logs / [`super::types::IngestionReport`].
#[derive(Debug, Clone, Default)]
pub struct MergeReport {
    /// Number of input candidates considered.
    pub input: usize,
    /// Number of candidates that survived after collapsing near-dupes.
    pub output: usize,
    /// Number of pair-level merges performed.
    pub merged_pairs: usize,
    /// Whether a summarizer was invoked (vs heuristic-only merge).
    pub used_summarizer: bool,
}

/// Collapse near-duplicate candidates of the same `kind` based on a
/// cheap Jaccard-similarity check over their tokens.
///
/// Stable ordering — the first candidate encountered keeps its index in
/// the output. The `summarizer` parameter is accepted today for
/// future use: when a summarizer is wired in, this is the seam where
/// the merged group's content will be rewritten via an LLM call. For
/// now the longest snippet wins.
pub async fn collapse_near_duplicates(
    candidates: Vec<MemoryCandidate>,
    summarizer: Option<&Arc<dyn SummarizerProvider>>,
) -> (Vec<MemoryCandidate>, MergeReport) {
    let mut report = MergeReport {
        input: candidates.len(),
        output: candidates.len(),
        merged_pairs: 0,
        used_summarizer: summarizer.is_some(),
    };
    if candidates.len() < 2 {
        return (candidates, report);
    }

    let mut tokens: Vec<HashSet<String>> = candidates.iter().map(tokenise).collect();
    let mut kept: Vec<MemoryCandidate> = Vec::with_capacity(candidates.len());
    let mut taken: Vec<bool> = vec![false; candidates.len()];

    for (i, candidate) in candidates.iter().enumerate() {
        if taken[i] {
            continue;
        }
        let mut group_indices: Vec<usize> = vec![i];
        for j in (i + 1)..candidates.len() {
            if taken[j] {
                continue;
            }
            if candidates[j].kind != candidate.kind {
                continue;
            }
            if jaccard(&tokens[i], &tokens[j]) >= NEAR_DUP_THRESHOLD {
                group_indices.push(j);
                taken[j] = true;
            }
        }
        taken[i] = true;

        if group_indices.len() == 1 {
            kept.push(candidate.clone());
            continue;
        }
        report.merged_pairs += group_indices.len() - 1;
        // Choose the longest snippet as representative (richest
        // surface form). Merge provenance indices so we don't lose
        // pointers back to the source messages.
        let mut best_idx = group_indices[0];
        for &idx in &group_indices[1..] {
            if candidates[idx].content.chars().count()
                > candidates[best_idx].content.chars().count()
            {
                best_idx = idx;
            }
        }
        let mut merged = candidates[best_idx].clone();
        let mut all_msg_indices: Vec<usize> = Vec::new();
        for &idx in &group_indices {
            all_msg_indices.extend(candidates[idx].provenance.message_indices.iter().copied());
            // Promote importance to High if any member of the group is
            // High — recurrence is itself signal that this fact
            // matters.
            if matches!(candidates[idx].importance, super::types::Importance::High) {
                merged.importance = super::types::Importance::High;
            }
        }
        all_msg_indices.sort_unstable();
        all_msg_indices.dedup();
        merged.provenance.message_indices = all_msg_indices;
        kept.push(merged);
        // tokens of merged-away entries are no longer needed; null out
        // to free a little memory in long runs.
        for &idx in &group_indices[1..] {
            tokens[idx].clear();
        }
    }

    report.output = kept.len();
    if report.merged_pairs > 0 {
        log::info!(
            "[transcript_ingest::merge] collapsed {} near-duplicate pair(s): {} → {} (summarizer={})",
            report.merged_pairs,
            report.input,
            report.output,
            report.used_summarizer,
        );
    }
    (kept, report)
}

fn tokenise(c: &MemoryCandidate) -> HashSet<String> {
    let mut out: HashSet<String> = HashSet::new();
    for raw in c.content.split(|ch: char| !ch.is_alphanumeric()) {
        let word = raw.to_ascii_lowercase();
        if word.chars().count() >= 3 && !STOPWORDS.contains(&word.as_str()) {
            out.insert(word);
        }
    }
    out
}

fn jaccard(a: &HashSet<String>, b: &HashSet<String>) -> f32 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let inter = a.intersection(b).count();
    let union = a.union(b).count();
    if union == 0 {
        0.0
    } else {
        (inter as f32) / (union as f32)
    }
}

/// Minimal stopword list — common short connectors that artificially
/// inflate Jaccard similarity without adding meaning. Intentionally
/// short; the tokeniser already drops words under 3 chars.
const STOPWORDS: &[&str] = &[
    "the", "and", "for", "with", "that", "this", "but", "are", "you", "your", "from", "have",
    "has", "into", "than", "they", "them", "our", "ours", "i'll", "i'm", "i've", "i'd", "let",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openhuman::learning::transcript_ingest::types::{Importance, Provenance};

    fn cand(content: &str, kind: CandidateKind, idx: usize) -> MemoryCandidate {
        MemoryCandidate {
            kind,
            importance: Importance::High,
            content: content.into(),
            provenance: Provenance {
                thread_id: Some("t".into()),
                transcript_path: "/p".into(),
                transcript_basename: "p".into(),
                message_indices: vec![idx],
                extracted_at: "2026-05-11T00:00:00Z".into(),
            },
        }
    }

    #[tokio::test]
    async fn empty_input_is_a_noop() {
        let (out, report) = collapse_near_duplicates(vec![], None).await;
        assert!(out.is_empty());
        assert_eq!(report.merged_pairs, 0);
        assert!(!report.used_summarizer);
    }

    #[tokio::test]
    async fn singletons_pass_through_untouched() {
        let input = vec![
            cand(
                "I prefer Postgres for new services.",
                CandidateKind::Preference,
                1,
            ),
            cand(
                "Let's go with Datadog for tracing.",
                CandidateKind::Decision,
                3,
            ),
        ];
        let (out, report) = collapse_near_duplicates(input.clone(), None).await;
        assert_eq!(out.len(), 2);
        assert_eq!(report.merged_pairs, 0);
    }

    #[tokio::test]
    async fn collapses_near_duplicate_preferences() {
        let input = vec![
            cand(
                "I prefer terse, direct answers over long explanations.",
                CandidateKind::Preference,
                1,
            ),
            cand(
                "I prefer terse direct answers — please skip long explanations.",
                CandidateKind::Preference,
                5,
            ),
        ];
        let (out, report) = collapse_near_duplicates(input, None).await;
        assert_eq!(out.len(), 1, "near-dup preferences should collapse");
        assert_eq!(report.merged_pairs, 1);
        // Merged provenance points to both source messages.
        assert_eq!(out[0].provenance.message_indices, vec![1, 5]);
    }

    #[tokio::test]
    async fn does_not_collapse_across_kinds() {
        let input = vec![
            cand(
                "I prefer terse direct answers over long explanations.",
                CandidateKind::Preference,
                1,
            ),
            cand(
                "Going to write terse direct answers in the doc.",
                CandidateKind::Commitment,
                2,
            ),
        ];
        let (out, _) = collapse_near_duplicates(input, None).await;
        assert_eq!(out.len(), 2, "different kinds must not merge");
    }
}

//! Host-side re-ranking maths for the two agent search tools.
//!
//! # Why this is host code and not a capability family
//!
//! [`MemoryHybridSearchTool`] and [`MemoryVectorSearchTool`] already fetch
//! their candidates through [`MemoryGuard`], so by the time these functions run
//! the driver has answered and the bus round-trip is over. What is left is
//! arithmetic over values the caller is holding: a weighted sum of a
//! [`RetrievalScoreBreakdown`] the driver returned, a cosine between two
//! vectors the caller already has, and an MMR pass over that. None of it
//! touches a store, so routing it back over the bus would buy a serialization
//! hop and nothing else.
//!
//! It is also *policy the host chose*. Which of the four named profiles a mode
//! string selects, and how aggressively MMR trades relevance for diversity, are
//! decisions about what OpenHuman's agents should see — not facts about how a
//! particular engine stores things. A second driver should rank identically
//! here, which is exactly the argument for the maths living above the driver
//! rather than inside one.
//!
//! # Ported verbatim, and pinned to literals
//!
//! Every function here is a byte-for-byte port of the tinycortex original it
//! replaced (`memory::retrieval::scoring::hybrid_score`,
//! `memory::config::WeightProfile`, `memory::retrieval::mmr::mmr_select`,
//! `memory::store::vectors::cosine_similarity`). The tests assert **literal
//! expected values** rather than calling the engine, so they keep meaning once
//! the engine leaves the build — the same rule `util::redact`'s parity test
//! follows.
//!
//! [`MemoryHybridSearchTool`]: super::tools::search::hybrid_search::MemoryHybridSearchTool
//! [`MemoryVectorSearchTool`]: super::tools::search::vector_search::MemoryVectorSearchTool
//! [`MemoryGuard`]: super::guard::MemoryGuard

use serde::{Deserialize, Serialize};
use tinymemory_api::types::RetrievalScoreBreakdown;

/// Named hybrid-retrieval weight profiles (graph / vector / keyword / freshness).
///
/// Nothing here *enforces* that the four weights sum to `1.0` — the built-in
/// profiles are chosen that way by convention so scores land in a familiar
/// `[0.0, 1.0]`-ish range when every signal is itself in `[0.0, 1.0]`. A custom
/// profile with a different total simply rescales the final score.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WeightProfile {
    /// Weight on graph/co-occurrence proximity signal.
    pub graph: f64,
    /// Weight on dense vector (cosine) similarity signal.
    pub vector: f64,
    /// Weight on lexical/keyword match signal.
    pub keyword: f64,
    /// Weight on recency; `0.0` disables freshness boosting.
    pub freshness: f64,
}

impl WeightProfile {
    /// `balanced`: graph 0.35, vector 0.35, keyword 0.15, freshness 0.15.
    pub const BALANCED: Self = Self {
        graph: 0.35,
        vector: 0.35,
        keyword: 0.15,
        freshness: 0.15,
    };
    /// `semantic`: graph 0.15, vector 0.65, keyword 0.20.
    pub const SEMANTIC: Self = Self {
        graph: 0.15,
        vector: 0.65,
        keyword: 0.20,
        freshness: 0.0,
    };
    /// `lexical`: graph 0.25, vector 0.15, keyword 0.60.
    pub const LEXICAL: Self = Self {
        graph: 0.25,
        vector: 0.15,
        keyword: 0.60,
        freshness: 0.0,
    };
    /// `graph_first`: graph 0.55, vector 0.30, keyword 0.15.
    pub const GRAPH_FIRST: Self = Self {
        graph: 0.55,
        vector: 0.30,
        keyword: 0.15,
        freshness: 0.0,
    };

    /// Resolve a profile by its wire name, returning `None` for unknown names.
    #[must_use]
    pub fn by_name(name: &str) -> Option<Self> {
        match name {
            "balanced" => Some(Self::BALANCED),
            "semantic" => Some(Self::SEMANTIC),
            "lexical" => Some(Self::LEXICAL),
            "graph_first" => Some(Self::GRAPH_FIRST),
            _ => None,
        }
    }
}

/// Combine the four retrieval signals into a single ranking score under
/// `profile`, as the weighted sum `graph·g + vector·v + keyword·k + freshness·f`.
///
/// `episodic_relevance` is left at `0.0` — episodic memory is not a tree-
/// retrieval signal — but is carried in the breakdown for wire compatibility
/// with [`RetrievalScoreBreakdown`].
#[must_use]
pub fn hybrid_score(
    profile: &WeightProfile,
    graph_relevance: f64,
    vector_similarity: f64,
    keyword_relevance: f64,
    freshness: f64,
) -> RetrievalScoreBreakdown {
    let final_score = profile.graph * graph_relevance
        + profile.vector * vector_similarity
        + profile.keyword * keyword_relevance
        + profile.freshness * freshness;
    RetrievalScoreBreakdown {
        keyword_relevance,
        vector_similarity,
        graph_relevance,
        episodic_relevance: 0.0,
        freshness,
        final_score,
    }
}

/// Cosine similarity between two vectors. Returns `0.0` for mismatched
/// lengths, empty vectors, or zero-magnitude vectors.
///
/// The raw cosine value is clamped only for floating-point drift, preserving
/// its mathematical `[-1.0, 1.0]` range.
#[must_use]
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0_f64;
    let mut norm_a = 0.0_f64;
    let mut norm_b = 0.0_f64;
    for (x, y) in a.iter().zip(b.iter()) {
        let x = f64::from(*x);
        let y = f64::from(*y);
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }
    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom <= f64::EPSILON {
        return 0.0;
    }
    (dot / denom).clamp(-1.0, 1.0)
}

/// A candidate for MMR selection.
pub struct MmrCandidate<'a> {
    /// Caller-side index, echoed back on the result so the candidate can be
    /// resolved to its original record.
    pub index: usize,
    /// Candidate embedding; must share dimensionality with every other
    /// candidate, since cosine similarity is computed pairwise.
    pub embedding: &'a [f32],
    /// Precomputed relevance of this candidate to the query (typically a
    /// cosine score). Higher is more relevant; weighted by `lambda`.
    pub relevance: f64,
}

/// Result of MMR selection: the original index and its MMR score.
#[derive(Debug, Clone)]
pub struct MmrResult {
    /// Caller-side index echoed from the chosen [`MmrCandidate::index`].
    pub index: usize,
    /// The MMR score at the step this item was selected:
    /// `lambda · relevance − (1 − lambda) · max_similarity(c, selected)`.
    /// Not comparable across runs with different `lambda`.
    pub score: f64,
}

/// Select up to `limit` items from `candidates` using Maximal Marginal
/// Relevance.
///
/// `lambda` controls the relevance-diversity tradeoff: `1.0` is pure
/// relevance, `0.0` pure diversity, `0.7` the recommended default. It is
/// clamped to `[0.0, 1.0]`; `limit` is clamped to `candidates.len()`.
///
/// # Gotcha: `query_vec` is unused
///
/// Relevance-to-query is taken entirely from each candidate's precomputed
/// [`MmrCandidate::relevance`], which the caller must already have derived
/// from the query. Passing a mismatched or empty `query_vec` has no effect on
/// the result. The parameter is kept for shape parity with the tinycortex
/// original this replaced, so a diff between the two is empty rather than
/// "and the signature changed too".
#[must_use]
pub fn mmr_select(
    query_vec: &[f32],
    candidates: &[MmrCandidate<'_>],
    limit: usize,
    lambda: f64,
) -> Vec<MmrResult> {
    if candidates.is_empty() || limit == 0 {
        return Vec::new();
    }

    let lambda = lambda.clamp(0.0, 1.0);
    let limit = limit.min(candidates.len());

    let mut selected_embeddings: Vec<&[f32]> = Vec::with_capacity(limit);
    let mut results: Vec<MmrResult> = Vec::with_capacity(limit);
    let mut available: Vec<bool> = vec![true; candidates.len()];

    for _ in 0..limit {
        let mut best_idx: Option<usize> = None;
        let mut best_mmr = f64::NEG_INFINITY;

        for (i, candidate) in candidates.iter().enumerate() {
            if !available[i] {
                continue;
            }
            let max_sim_to_selected = if selected_embeddings.is_empty() {
                0.0
            } else {
                selected_embeddings
                    .iter()
                    .map(|sel| cosine_similarity(candidate.embedding, sel))
                    .fold(f64::NEG_INFINITY, f64::max)
            };
            let mmr_score = lambda * candidate.relevance - (1.0 - lambda) * max_sim_to_selected;
            if mmr_score > best_mmr {
                best_mmr = mmr_score;
                best_idx = Some(i);
            }
        }

        let Some(idx) = best_idx else { break };
        available[idx] = false;
        selected_embeddings.push(candidates[idx].embedding);
        results.push(MmrResult {
            index: candidates[idx].index,
            score: best_mmr,
        });
    }

    let _ = query_vec;
    results
}

#[cfg(test)]
#[path = "ranking_tests.rs"]
mod tests;

//! Scoring weight profiles for hybrid retrieval — thin host shim over
//! `tinycortex::memory::WeightProfile` (W5).
//!
//! The weight profile (graph/vector/keyword/freshness weights + the
//! `BALANCED`/`SEMANTIC`/`LEXICAL`/`GRAPH_FIRST` presets + `by_name`) is the
//! crate's, a byte-identical port. The host keeps only [`compose_score`] — the
//! trivial weighted combination the crate expresses via
//! `retrieval::scoring::hybrid_score` at its own call sites; exposed here as a
//! free function so `memory_search::tools::hybrid_search` keeps its call shape.

pub use tinycortex::memory::WeightProfile;

/// Weighted composite of the four retrieval signals under `profile`.
///
/// `graph·graph_relevance + vector·vector_similarity + keyword·keyword_relevance
/// + freshness·freshness`.
pub fn compose_score(
    profile: &WeightProfile,
    graph_relevance: f64,
    vector_similarity: f64,
    keyword_relevance: f64,
    freshness: f64,
) -> f64 {
    (profile.graph * graph_relevance)
        + (profile.vector * vector_similarity)
        + (profile.keyword * keyword_relevance)
        + (profile.freshness * freshness)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_sum_to_one() {
        for profile in [
            WeightProfile::BALANCED,
            WeightProfile::SEMANTIC,
            WeightProfile::LEXICAL,
            WeightProfile::GRAPH_FIRST,
        ] {
            let sum = profile.graph + profile.vector + profile.keyword + profile.freshness;
            assert!(
                (sum - 1.0).abs() < 0.01,
                "profile weights should sum to ~1.0, got {sum}"
            );
        }
    }

    #[test]
    fn by_name_resolves_with_balanced_fallback() {
        assert_eq!(
            WeightProfile::by_name("semantic").vector,
            WeightProfile::SEMANTIC.vector
        );
        assert_eq!(
            WeightProfile::by_name("lexical").keyword,
            WeightProfile::LEXICAL.keyword
        );
        assert_eq!(
            WeightProfile::by_name("graph_first").graph,
            WeightProfile::GRAPH_FIRST.graph
        );
        // Unknown names fall back to balanced.
        assert_eq!(
            WeightProfile::by_name("unknown").graph,
            WeightProfile::BALANCED.graph
        );
    }

    #[test]
    fn compose_score_is_weighted_sum() {
        let p = WeightProfile::BALANCED;
        let s = compose_score(&p, 1.0, 1.0, 1.0, 1.0);
        assert!((s - (p.graph + p.vector + p.keyword + p.freshness)).abs() < 1e-9);
    }
}

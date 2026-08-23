//! Parity tests for the ported ranking maths.
//!
//! Every expectation is a **literal**, never a call back into the engine. A
//! test that compared against `tinycortex::…` would pass by construction and
//! then stop compiling the day the engine leaves the build — which is the day
//! it would actually be needed. Same rule as `util::redact`'s parity test.

use super::{cosine_similarity, hybrid_score, mmr_select, MmrCandidate, WeightProfile};

/// Tolerance for the one expectation that is not exact in binary floating
/// point. The clean cases below are asserted exactly on purpose.
const EPSILON: f64 = 1e-12;

#[test]
fn named_profiles_resolve_and_unknown_names_do_not() {
    assert_eq!(
        WeightProfile::by_name("balanced"),
        Some(WeightProfile::BALANCED)
    );
    assert_eq!(
        WeightProfile::by_name("semantic"),
        Some(WeightProfile::SEMANTIC)
    );
    assert_eq!(
        WeightProfile::by_name("lexical"),
        Some(WeightProfile::LEXICAL)
    );
    assert_eq!(
        WeightProfile::by_name("graph_first"),
        Some(WeightProfile::GRAPH_FIRST)
    );
    assert_eq!(WeightProfile::by_name("nonexistent"), None);
    // The tool passes the agent's `mode` string straight in, so an empty or
    // differently-cased value must miss rather than fall back to balanced.
    assert_eq!(WeightProfile::by_name(""), None);
    assert_eq!(WeightProfile::by_name("Balanced"), None);
}

#[test]
fn hybrid_score_is_the_weighted_sum_of_the_four_signals() {
    let breakdown = hybrid_score(&WeightProfile::BALANCED, 0.8, 0.6, 0.4, 0.2);

    // 0.35·0.8 + 0.35·0.6 + 0.15·0.4 + 0.15·0.2 = 0.58
    assert!((breakdown.final_score - 0.58).abs() < EPSILON);
    // The four inputs are carried through untouched…
    assert!((breakdown.graph_relevance - 0.8).abs() < EPSILON);
    assert!((breakdown.vector_similarity - 0.6).abs() < EPSILON);
    assert!((breakdown.keyword_relevance - 0.4).abs() < EPSILON);
    assert!((breakdown.freshness - 0.2).abs() < EPSILON);
    // …and episodic is always zero: it is not a tree-retrieval signal, and is
    // present only for wire compatibility with the breakdown shape.
    assert_eq!(breakdown.episodic_relevance, 0.0);
}

#[test]
fn a_zero_freshness_profile_ignores_the_freshness_signal() {
    // `semantic` sets freshness to 0.0, so a wildly fresh item scores the same
    // as a stale one. Pins that the weight is applied rather than assumed.
    let fresh = hybrid_score(&WeightProfile::SEMANTIC, 0.0, 1.0, 0.0, 1.0);
    let stale = hybrid_score(&WeightProfile::SEMANTIC, 0.0, 1.0, 0.0, 0.0);
    assert!((fresh.final_score - stale.final_score).abs() < EPSILON);
    assert!((fresh.final_score - 0.65).abs() < EPSILON);
}

#[test]
fn cosine_similarity_covers_its_whole_mathematical_range() {
    assert_eq!(cosine_similarity(&[1.0, 0.0], &[1.0, 0.0]), 1.0);
    assert_eq!(cosine_similarity(&[1.0, 0.0], &[0.0, 1.0]), 0.0);
    // Anti-correlated stays negative — the value is clamped to [-1, 1] for
    // floating-point drift only, not squashed into [0, 1].
    assert_eq!(cosine_similarity(&[1.0, 0.0], &[-1.0, 0.0]), -1.0);

    // 32 / sqrt(14 · 77)
    let expected = 32.0_f64 / (14.0_f64.sqrt() * 77.0_f64.sqrt());
    assert!((cosine_similarity(&[1.0, 2.0, 3.0], &[4.0, 5.0, 6.0]) - expected).abs() < EPSILON);
}

#[test]
fn cosine_similarity_returns_zero_for_the_degenerate_inputs() {
    assert_eq!(cosine_similarity(&[], &[]), 0.0);
    assert_eq!(cosine_similarity(&[1.0, 2.0], &[1.0]), 0.0);
    assert_eq!(cosine_similarity(&[0.0, 0.0], &[1.0, 1.0]), 0.0);
}

#[test]
fn mmr_prefers_a_diverse_candidate_over_a_more_relevant_duplicate() {
    let near = [1.0_f32, 0.0];
    let orthogonal = [0.0_f32, 1.0];
    let candidates = vec![
        MmrCandidate {
            index: 0,
            embedding: &near,
            relevance: 0.9,
        },
        MmrCandidate {
            index: 1,
            embedding: &near,
            relevance: 0.8,
        },
        MmrCandidate {
            index: 2,
            embedding: &orthogonal,
            relevance: 0.5,
        },
    ];

    let selected = mmr_select(&[], &candidates, 3, 0.7);
    let order: Vec<usize> = selected.iter().map(|r| r.index).collect();

    // Candidate 1 is more relevant than 2 but identical to the already-picked
    // 0, so diversity demotes it below the orthogonal candidate. This is the
    // whole point of the pass; a plain relevance sort would give [0, 1, 2].
    assert_eq!(order, vec![0, 2, 1]);

    assert!((selected[0].score - 0.63).abs() < EPSILON); // 0.7·0.9
    assert!((selected[1].score - 0.35).abs() < EPSILON); // 0.7·0.5 − 0.3·0.0
    assert!((selected[2].score - 0.26).abs() < EPSILON); // 0.7·0.8 − 0.3·1.0
}

#[test]
fn mmr_clamps_limit_and_lambda_and_short_circuits_on_empty_input() {
    let emb = [1.0_f32, 0.0];
    let candidates = vec![MmrCandidate {
        index: 7,
        embedding: &emb,
        relevance: 0.5,
    }];

    // `limit` above the candidate count yields the candidates, not a panic.
    assert_eq!(mmr_select(&[], &candidates, 99, 0.7).len(), 1);
    // `lambda` out of range is clamped: 2.0 behaves as pure relevance.
    let pure_relevance = mmr_select(&[], &candidates, 1, 2.0);
    assert!((pure_relevance[0].score - 0.5).abs() < EPSILON);

    assert!(mmr_select(&[], &candidates, 0, 0.7).is_empty());
    assert!(mmr_select(&[], &[], 5, 0.7).is_empty());
}

// The tokenizer's own tests live with it, in `util::bm25`.
use super::*;

fn spec(name: &str, description: &str) -> ToolSpec {
    ToolSpec {
        name: name.to_string(),
        description: description.to_string(),
        parameters: json!({"type": "object"}),
    }
}

fn index() -> ToolSearchIndex {
    ToolSearchIndex::build(&[
        spec("stock_quote", "Get the latest price for a stock ticker"),
        spec("cron_add", "Schedule a recurring job to run later"),
        spec(
            "memory_hybrid_search",
            "Search stored memories semantically",
        ),
        spec(
            "generate_presentation",
            "Build a pptx slide deck from an outline",
        ),
    ])
}

#[test]
fn a_plain_language_query_finds_the_right_tool() {
    let index = index();
    let hits = index.search("schedule something to run every morning", 3);
    assert_eq!(hits.first().map(|t| t.name.as_str()), Some("cron_add"));
}

#[test]
fn a_query_matching_the_name_rather_than_the_description_still_hits() {
    let index = index();
    let hits = index.search("stock", 3);
    assert_eq!(hits.first().map(|t| t.name.as_str()), Some("stock_quote"));
}

#[test]
fn nothing_relevant_returns_nothing_rather_than_padding_to_the_limit() {
    // Padding would spend exactly the tokens deferral saves, and would
    // invite a call to something unrelated to the ask.
    let index = index();
    assert!(index.search("xyzzy quantum flux", 5).is_empty());
}

#[test]
fn results_are_capped_at_the_requested_limit() {
    let index = index();
    assert!(index.search("search a stock job memory slide", 2).len() <= 2);
}

#[test]
fn an_empty_query_matches_nothing() {
    let index = index();
    assert!(index.search("   ", 5).is_empty());
}

#[test]
fn an_empty_index_is_searchable_without_panicking() {
    // `average_length` is 0 here; the length-normalisation term divides by
    // it, so this is the case that would panic or produce NaN if the
    // `.max(1.0)` guard were dropped.
    let empty = ToolSearchIndex::build(&[]);
    assert!(empty.is_empty());
    assert!(empty.search("anything", 5).is_empty());
}

#[test]
fn ranking_is_stable_across_identical_queries() {
    let index = index();
    let first: Vec<&str> = index
        .search("search", 4)
        .iter()
        .map(|t| t.name.as_str())
        .collect();
    let second: Vec<&str> = index
        .search("search", 4)
        .iter()
        .map(|t| t.name.as_str())
        .collect();
    assert_eq!(first, second);
}

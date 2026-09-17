use super::*;

#[test]
fn snake_case_and_camel_case_both_split() {
    assert_eq!(
        tokenize("memory_hybrid_search"),
        ["memory", "hybrid", "search"]
    );
    assert_eq!(
        tokenize("readWorkflowResource"),
        ["read", "workflow", "resource"]
    );
    assert_eq!(tokenize("HTTPServer2"), ["httpserver2"]);
}

fn corpus() -> Bm25Index {
    Bm25Index::build([
        ("alpha", "send an email to a colleague"),
        ("beta", "look up a stock quote by ticker symbol"),
        ("gamma", "read a file from the workspace"),
    ])
}

#[test]
fn a_plain_language_query_finds_the_right_document() {
    assert_eq!(corpus().search("stock ticker", 3), vec![1]);
    assert_eq!(corpus().search("email a colleague", 3)[0], 0);
    assert_eq!(corpus().search("read from the workspace", 3)[0], 2);
}

#[test]
fn nothing_relevant_returns_nothing_rather_than_padding() {
    // The load-bearing property. A ranker that always returns `limit`
    // results turns a miss into a confident wrong answer.
    assert!(corpus().search("photosynthesis", 3).is_empty());
}

#[test]
fn a_query_matching_only_on_stopwords_is_a_miss() {
    // The regression, and it took two attempts to fix. "a" appears in
    // exactly ONE of these three documents, so by document frequency it is
    // the most distinguishing term in the query — the df threshold cannot
    // catch it, and the first fix that only had the threshold still ranked
    // a changelog skill as the match for provisioning a cluster.
    assert!(corpus()
        .search("provision a kubernetes cluster", 3)
        .is_empty());
    assert!(corpus().search("a", 3).is_empty());
    assert!(corpus()
        .search("what is it that you will do for me", 3)
        .is_empty());
}

#[test]
fn a_single_document_corpus_stays_searchable() {
    // The reason the threshold has a floor of 2 rather than being a plain
    // ratio. With one document every term is in every document, so a ratio
    // rule would filter the entire query and make the only installed skill
    // permanently unfindable.
    let one = Bm25Index::build([("solo", "post a message to discord")]);
    assert_eq!(one.search("discord", 1), vec![0]);
    assert_eq!(one.search("post a message", 1), vec![0]);
}

#[test]
fn a_real_term_still_matches_even_alongside_stopwords() {
    // The filter must drop the noise, not the query.
    assert_eq!(corpus().search("look up a stock", 3), vec![1]);
}

#[test]
fn an_empty_query_and_an_empty_index_are_both_safe() {
    assert!(corpus().search("", 3).is_empty());
    assert!(Bm25Index::build([]).search("anything", 3).is_empty());
}

#[test]
fn ranking_is_stable_across_identical_queries() {
    let index = corpus();
    let first = index.search("a", 3);
    for _ in 0..5 {
        assert_eq!(index.search("a", 3), first);
    }
}

#[test]
fn ties_break_on_the_sort_key_not_on_insertion_order() {
    // Two documents with identical text score identically; the sort key
    // decides. Built in reverse order so insertion order would give the
    // opposite answer.
    // Three documents, two identical: with only two, every term would be
    // in every document and `significant` would filter the query away.
    let index = Bm25Index::build([
        ("zulu", "same words here"),
        ("alpha", "same words here"),
        ("other", "entirely different text"),
    ]);
    assert_eq!(index.search("same words", 2), vec![1, 0]);
}

#[test]
fn the_limit_is_honoured() {
    // "read email stock" hits all three documents on a real term each, so
    // the unlimited search returns three — which is what makes this a test
    // of the cap rather than a query that happened to match once.
    assert_eq!(corpus().search("read email stock", 10).len(), 3);
    assert_eq!(corpus().search("read email stock", 1).len(), 1);
}

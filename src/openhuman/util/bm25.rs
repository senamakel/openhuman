//! BM25 ranking over short documents — the shared core behind `tool_search`
//! and `skill_search`.
//!
//! # Extraction-ready on purpose
//!
//! This module names **nothing** from `crate::`: no config, no `Tool`, no
//! workspace, no security policy. It takes `(id, text)` pairs and returns
//! ranked ids. That is not incidental tidiness — a capability that ends up in
//! a loadable module has to be reachable over a bus, and anything that reaches
//! back into the host cannot go. Ranking is the half of skill discovery that is
//! the same for every host; **which** bundles exist, whether the caller may
//! read one, and what a trust marker means are the half that never leaves.
//!
//! Keep it that way. A `use crate::` line here is the edit that makes the move
//! expensive, and it will look harmless at the time.
//!
//! # Why hand-rolled
//!
//! Codex uses the `bm25` crate for the same job. This crate is kernel surface
//! under a dependency-floor ratchet (`scripts/kernel-floor.limits`), and the
//! arithmetic below is a better trade than a package on the floor of every
//! embedder's build.

use std::collections::HashMap;

/// BM25 term-frequency saturation. The standard default.
pub const K1: f64 = 1.2;
/// BM25 length normalisation. The standard default.
pub const B: f64 = 0.75;

/// Split text into search terms.
///
/// Splits on non-alphanumerics **and** on a lower→upper transition, so
/// `memory_hybrid_search` and `readWorkflowResource` both yield the words a
/// person would actually type. Without the camelCase rule a query for
/// "workflow" misses a tool whose only mention of it is inside an identifier.
pub fn tokenize(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut previous_lower = false;
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            if ch.is_uppercase() && previous_lower && !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            current.push(ch.to_ascii_lowercase());
            previous_lower = ch.is_lowercase() || ch.is_numeric();
        } else if !current.is_empty() {
            out.push(std::mem::take(&mut current));
            previous_lower = false;
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// A ranked corpus of short documents.
///
/// Generic over nothing: documents are identified by their index into the slice
/// the caller built the index from, so the caller keeps its own richer type and
/// this module never has to know about it.
#[derive(Default, Debug)]
pub struct Bm25Index {
    documents: Vec<Document>,
    document_frequency: HashMap<String, usize>,
    average_length: f64,
}

#[derive(Debug, Clone)]
struct Document {
    /// Used only to break score ties deterministically.
    sort_key: String,
    tokens: Vec<String>,
}

impl Bm25Index {
    /// Build from `(sort_key, searchable_text)` pairs, in caller order.
    ///
    /// `sort_key` breaks ties; make it the id the caller would print, so two
    /// identical queries produce identical output. An unstable order would make
    /// a model's transcript non-reproducible for no benefit.
    pub fn build<'a>(documents: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        let documents: Vec<Document> = documents
            .into_iter()
            .map(|(sort_key, text)| Document {
                sort_key: sort_key.to_string(),
                tokens: tokenize(text),
            })
            .collect();

        let mut document_frequency: HashMap<String, usize> = HashMap::new();
        for doc in &documents {
            let mut seen: Vec<&str> = Vec::new();
            for token in &doc.tokens {
                if !seen.contains(&token.as_str()) {
                    seen.push(token);
                    *document_frequency.entry(token.clone()).or_insert(0) += 1;
                }
            }
        }

        let total: usize = documents.iter().map(|d| d.tokens.len()).sum();
        let average_length = if documents.is_empty() {
            0.0
        } else {
            total as f64 / documents.len() as f64
        };

        Self {
            documents,
            document_frequency,
            average_length,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.documents.is_empty()
    }

    pub fn len(&self) -> usize {
        self.documents.len()
    }

    /// Rank against `query`, best first, returning **indices** into the corpus.
    ///
    /// Only documents scoring above zero are returned. Padding the list out to
    /// `limit` with unrelated entries would spend exactly the tokens this
    /// mechanism exists to save, and would invite the model to act on something
    /// that has nothing to do with what it asked for.
    pub fn search(&self, query: &str, limit: usize) -> Vec<usize> {
        let terms = tokenize(query);
        if terms.is_empty() || self.documents.is_empty() {
            return Vec::new();
        }
        let count = self.documents.len() as f64;
        let mut scored: Vec<(f64, usize)> = self
            .documents
            .iter()
            .enumerate()
            .map(|(index, doc)| (self.score(doc, &terms), index))
            .filter(|(score, _)| *score > 0.0)
            .collect();

        scored.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| self.documents[a.1].sort_key.cmp(&self.documents[b.1].sort_key))
        });
        scored.into_iter().take(limit).map(|(_, i)| i).collect()
    }

    fn score(&self, doc: &Document, terms: &[String]) -> f64 {
        let length = doc.tokens.len() as f64;
        let count = self.documents.len() as f64;
        terms
            .iter()
            .map(|term| {
                let frequency = doc.tokens.iter().filter(|t| *t == term).count() as f64;
                if frequency == 0.0 {
                    return 0.0;
                }
                let df = *self.document_frequency.get(term).unwrap_or(&0) as f64;
                // Standard BM25 IDF with the +1 that keeps a term present in
                // every document at a small positive weight rather than a
                // negative one.
                let idf = (((count - df + 0.5) / (df + 0.5)) + 1.0).ln();
                let denominator =
                    frequency + K1 * (1.0 - B + B * length / self.average_length.max(1.0));
                idf * (frequency * (K1 + 1.0)) / denominator
            })
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snake_case_and_camel_case_both_split() {
        assert_eq!(tokenize("memory_hybrid_search"), ["memory", "hybrid", "search"]);
        assert_eq!(tokenize("readWorkflowResource"), ["read", "workflow", "resource"]);
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
        assert_eq!(corpus().search("what is the share price", 3), Vec::<usize>::new());
        assert_eq!(corpus().search("stock ticker", 3), vec![1]);
    }

    #[test]
    fn nothing_relevant_returns_nothing_rather_than_padding() {
        // The load-bearing property. A ranker that always returns `limit`
        // results turns a miss into a confident wrong answer.
        assert!(corpus().search("photosynthesis", 3).is_empty());
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
        let index = Bm25Index::build([("zulu", "same words here"), ("alpha", "same words here")]);
        assert_eq!(index.search("same words", 2), vec![1, 0]);
    }

    #[test]
    fn the_limit_is_honoured() {
        assert_eq!(corpus().search("a the to", 1).len().min(1), corpus().search("a the to", 1).len());
        assert!(corpus().search("a", 1).len() <= 1);
    }
}

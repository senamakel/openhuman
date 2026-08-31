//! `tool_search` — look up a capability whose schema is not on the wire.
//!
//! # Why this exists
//!
//! Tool schemas are a fixed cost paid on every request. Measured on the
//! orchestrator with an empty workspace, they were 45,199 bytes against 33,808
//! bytes of system prompt; on a signed-in workspace with Composio connected
//! they reached ~112 KB. Most of that is tools the model reaches for on a
//! handful of turns a week.
//!
//! [`ToolExposure::Deferred`] takes such a tool off the wire without removing
//! the capability, and this is the other half of that bargain: the model can
//! find it again. A capability the model can neither see nor look up is simply
//! gone, which is a far worse regression than the tokens it saves.
//!
//! # Why not the toolpack mechanism
//!
//! Packs answer a different question. A pack is a *group* withheld by a config
//! posture, recovered through `load_skill`, and its membership is compiled in
//! precisely so config cannot move a dangerous tool out of the reviewed
//! surface. That is the right shape for compressing a belt an agent owns.
//!
//! Deferral is per-tool and is a property of the tool: `stock_quote` is rarely
//! needed whoever is running the host. The two compose — a deferred tool inside
//! a withheld pack is simply absent twice — and neither can widen the surface,
//! because both only ever subtract from a set the belt and the security policy
//! already decided.
//!
//! # Ranking
//!
//! A hand-rolled BM25. Codex uses the `bm25` crate for the same job; this crate
//! is kernel surface under a dependency-floor ratchet
//! (`scripts/kernel-floor.limits`), and ~70 lines of arithmetic is a better
//! trade than a package on the floor of every embedder's build.

use std::collections::HashMap;
use std::sync::{Arc, RwLock, Weak};

use anyhow::Result;
use serde_json::{json, Value};

use crate::openhuman::tools::{PermissionLevel, Tool, ToolCategory, ToolResult, ToolSpec};

/// How many matches a search returns when the caller does not say.
const DEFAULT_LIMIT: usize = 5;
/// Ceiling on `limit`, so one call cannot undo the saving by asking for
/// everything.
const MAX_LIMIT: usize = 20;

/// BM25 term-frequency saturation. The standard default.
const BM25_K1: f64 = 1.2;
/// BM25 length normalisation. The standard default.
const BM25_B: f64 = 0.75;

/// One deferred tool, as the index sees it.
#[derive(Clone, Debug)]
pub struct SearchableTool {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    /// Lowercased `name` + `description` tokens, precomputed.
    tokens: Vec<String>,
}

impl SearchableTool {
    pub fn from_spec(spec: &ToolSpec) -> Self {
        let tokens = tokenize(&format!("{} {}", spec.name, spec.description));
        Self {
            name: spec.name.clone(),
            description: spec.description.clone(),
            parameters: spec.parameters.clone(),
            tokens,
        }
    }
}

/// Split on anything that is not alphanumeric, and additionally split
/// `snake_case` and `camelCase` runs.
///
/// Tool names are the highest-signal field in the index and they are almost all
/// `snake_case`, so a tokenizer that treats `memory_hybrid_search` as one term
/// matches the query "search memory" at zero. Splitting on `_` fixes that; the
/// camel split costs little and covers the handful of MCP-imported names that
/// arrive in that shape.
fn tokenize(text: &str) -> Vec<String> {
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

/// A BM25 index over the deferred tools.
#[derive(Default, Debug)]
pub struct ToolSearchIndex {
    tools: Vec<SearchableTool>,
    /// Document frequency per term.
    document_frequency: HashMap<String, usize>,
    average_length: f64,
}

impl ToolSearchIndex {
    pub fn build(specs: &[ToolSpec]) -> Self {
        let tools: Vec<SearchableTool> = specs.iter().map(SearchableTool::from_spec).collect();
        let mut document_frequency: HashMap<String, usize> = HashMap::new();
        for tool in &tools {
            let mut seen: Vec<&str> = Vec::new();
            for token in &tool.tokens {
                if !seen.contains(&token.as_str()) {
                    seen.push(token);
                    *document_frequency.entry(token.clone()).or_insert(0) += 1;
                }
            }
        }
        let total: usize = tools.iter().map(|t| t.tokens.len()).sum();
        let average_length = if tools.is_empty() {
            0.0
        } else {
            total as f64 / tools.len() as f64
        };
        Self {
            tools,
            document_frequency,
            average_length,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Rank the index against `query`, best first.
    ///
    /// Only tools scoring above zero are returned. Padding the list out to
    /// `limit` with unrelated tools would spend exactly the tokens this
    /// mechanism exists to save, and would invite the model to call something
    /// that has nothing to do with what it asked for.
    pub fn search(&self, query: &str, limit: usize) -> Vec<&SearchableTool> {
        let terms = tokenize(query);
        if terms.is_empty() || self.tools.is_empty() {
            return Vec::new();
        }
        let count = self.tools.len() as f64;
        let mut scored: Vec<(f64, usize)> = self
            .tools
            .iter()
            .enumerate()
            .map(|(index, tool)| {
                let length = tool.tokens.len() as f64;
                let score: f64 = terms
                    .iter()
                    .map(|term| {
                        let frequency =
                            tool.tokens.iter().filter(|t| *t == term).count() as f64;
                        if frequency == 0.0 {
                            return 0.0;
                        }
                        let df = *self.document_frequency.get(term).unwrap_or(&0) as f64;
                        // Standard BM25 IDF with the +1 that keeps a term
                        // present in every document at a small positive weight
                        // rather than a negative one.
                        let idf = (((count - df + 0.5) / (df + 0.5)) + 1.0).ln();
                        let denominator = frequency
                            + BM25_K1
                                * (1.0 - BM25_B
                                    + BM25_B * length / self.average_length.max(1.0));
                        idf * (frequency * (BM25_K1 + 1.0)) / denominator
                    })
                    .sum();
                (score, index)
            })
            .filter(|(score, _)| *score > 0.0)
            .collect();
        // Ties break on name so repeated identical queries give identical
        // results; an unstable order would make the model's transcript
        // non-reproducible for no benefit.
        scored.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| self.tools[a.1].name.cmp(&self.tools[b.1].name))
        });
        scored
            .into_iter()
            .take(limit)
            .map(|(_, index)| &self.tools[index])
            .collect()
    }
}

/// Shared handle to the index, so the tool can be constructed before the
/// registry it searches exists.
///
/// Same shape and same reason as `toolpacks::PackRegistryHandle`: the tool is
/// built during registry assembly and cannot be handed a finished registry at
/// that point.
pub type ToolSearchHandle = Arc<RwLock<ToolSearchIndex>>;

/// Look up a deferred capability by description.
pub struct ToolSearchTool {
    index: Weak<RwLock<ToolSearchIndex>>,
}

impl ToolSearchTool {
    pub fn new(index: &ToolSearchHandle) -> Self {
        Self {
            index: Arc::downgrade(index),
        }
    }
}

#[async_trait::async_trait]
impl Tool for ToolSearchTool {
    fn name(&self) -> &str {
        TOOL_SEARCH_NAME
    }

    fn description(&self) -> &str {
        "Find a tool that is not in your tool list. Not every capability this \
         host has is advertised up front; describe what you need in plain words \
         (\"send a calendar invite\", \"read a PDF\") and this returns the \
         matching tools with their full argument schemas, which you can then \
         call directly. Use it before telling the user something is impossible."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "What you need to do, in plain words."
                },
                "limit": {
                    "type": "integer",
                    "description": format!(
                        "How many matches to return (default {DEFAULT_LIMIT}, max {MAX_LIMIT})."
                    ),
                    "minimum": 1,
                    "maximum": MAX_LIMIT
                }
            },
            "required": ["query"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::System
    }

    fn permission_level(&self) -> PermissionLevel {
        // Reads a static in-memory index. Calling it cannot change anything,
        // and gating it would put an approval prompt in front of the model
        // merely asking what it is allowed to do.
        PermissionLevel::None
    }

    async fn execute(&self, args: Value) -> Result<ToolResult> {
        let query = args
            .get("query")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();
        if query.is_empty() {
            return Ok(ToolResult::error(
                "tool_search needs a `query` describing what you want to do.",
            ));
        }
        let limit = args
            .get("limit")
            .and_then(Value::as_u64)
            .map(|n| (n as usize).clamp(1, MAX_LIMIT))
            .unwrap_or(DEFAULT_LIMIT);

        let Some(index) = self.index.upgrade() else {
            // The registry outlives every tool in it, so this is a programmer
            // error rather than a user-visible condition. Report it as one
            // instead of pretending nothing matched, which would send the model
            // off to tell the user a capability does not exist.
            tracing::error!("[tool_search] index handle is dangling; registry dropped early");
            return Ok(ToolResult::error(
                "tool search is unavailable in this session (internal error)",
            ));
        };
        let index = index
            .read()
            .map_err(|_| anyhow::anyhow!("tool search index lock poisoned"))?;

        let matches = index.search(&query, limit);
        if matches.is_empty() {
            return Ok(ToolResult::success(format!(
                "No deferred tool matches \"{query}\". {} tools are searchable; \
                 everything else you can use is already in your tool list.",
                index.len()
            )));
        }
        let payload: Vec<Value> = matches
            .iter()
            .map(|tool| {
                json!({
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.parameters,
                })
            })
            .collect();
        Ok(ToolResult::success(format!(
            "{} match(es). Call any of these by name, using the parameters shown.\n{}",
            payload.len(),
            serde_json::to_string_pretty(&payload)?
        )))
    }
}

#[cfg(test)]
mod tests {
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
            spec("memory_hybrid_search", "Search stored memories semantically"),
            spec("generate_presentation", "Build a pptx slide deck from an outline"),
        ])
    }

    #[test]
    fn snake_case_names_are_split_into_searchable_terms() {
        // The whole point: `memory_hybrid_search` as one opaque term would
        // score zero against "search memory", which is how a user would ask.
        assert_eq!(
            tokenize("memory_hybrid_search"),
            vec!["memory", "hybrid", "search"]
        );
    }

    #[test]
    fn camel_case_is_split_too() {
        assert_eq!(tokenize("getUserProfile"), vec!["get", "user", "profile"]);
    }

    #[test]
    fn a_plain_language_query_finds_the_right_tool() {
        let hits = index().search("schedule something to run every morning", 3);
        assert_eq!(hits.first().map(|t| t.name.as_str()), Some("cron_add"));
    }

    #[test]
    fn a_query_matching_the_name_rather_than_the_description_still_hits() {
        let hits = index().search("stock", 3);
        assert_eq!(hits.first().map(|t| t.name.as_str()), Some("stock_quote"));
    }

    #[test]
    fn nothing_relevant_returns_nothing_rather_than_padding_to_the_limit() {
        // Padding would spend exactly the tokens deferral saves, and would
        // invite a call to something unrelated to the ask.
        assert!(index().search("xyzzy quantum flux", 5).is_empty());
    }

    #[test]
    fn results_are_capped_at_the_requested_limit() {
        assert!(index().search("search a stock job memory slide", 2).len() <= 2);
    }

    #[test]
    fn an_empty_query_matches_nothing() {
        assert!(index().search("   ", 5).is_empty());
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
        let first: Vec<&str> = index.search("search", 4).iter().map(|t| t.name.as_str()).collect();
        let second: Vec<&str> = index.search("search", 4).iter().map(|t| t.name.as_str()).collect();
        assert_eq!(first, second);
    }
}

/// Remove every [`ToolExposure::Deferred`] and [`ToolExposure::Hidden`] tool
/// from an agent's advertised set, returning the specs of the deferred ones so
/// the caller can index them.
///
/// Hidden tools are dropped and **not** returned: they are not searchable
/// either, by definition.
///
/// `tool_search` itself is never removed, whatever it declares. A search tool
/// the model cannot see is the one failure mode this whole mechanism cannot
/// recover from — every deferred capability would be unreachable, silently.
pub fn strip_deferred_from_visible(
    visible: &mut std::collections::HashSet<String>,
    tools: &[Box<dyn Tool>],
) -> Vec<ToolSpec> {
    use crate::openhuman::tools::ToolExposure;

    let mut deferred = Vec::new();
    for tool in tools {
        let name = tool.name();
        if name == TOOL_SEARCH_NAME || !visible.contains(name) {
            continue;
        }
        match tool.exposure() {
            ToolExposure::Direct => {}
            ToolExposure::Deferred => {
                visible.remove(name);
                deferred.push(tool.spec());
            }
            ToolExposure::Hidden => {
                visible.remove(name);
            }
        }
    }
    deferred
}

/// The advertised name of [`ToolSearchTool`], as a constant so the carve-out
/// above and the registration site cannot disagree about it.
pub const TOOL_SEARCH_NAME: &str = "tool_search";

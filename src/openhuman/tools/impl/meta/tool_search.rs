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

use std::sync::{Arc, RwLock};

use anyhow::Result;
use serde_json::{json, Value};

use crate::openhuman::tools::{PermissionLevel, Tool, ToolCategory, ToolResult, ToolSpec};
use crate::openhuman::util::bm25::Bm25Index;

/// How many matches a search returns when the caller does not say.
const DEFAULT_LIMIT: usize = 5;
/// Ceiling on `limit`, so one call cannot undo the saving by asking for
/// everything.
const MAX_LIMIT: usize = 20;

/// One deferred tool, as the index sees it.
#[derive(Clone, Debug)]
pub struct SearchableTool {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    /// `name` + `description`, the text the ranker sees.
    searchable: String,
}

impl SearchableTool {
    pub fn from_spec(spec: &ToolSpec) -> Self {
        Self {
            name: spec.name.clone(),
            description: spec.description.clone(),
            parameters: spec.parameters.clone(),
            searchable: format!("{} {}", spec.name, spec.description),
        }
    }
}

/// A BM25 index over the deferred tools.
///
/// A thin adapter: the ranking lives in [`crate::openhuman::util::bm25`], which
/// knows nothing about tools, and this type owns the part that is about tools —
/// what text is searchable and what a result looks like.
#[derive(Default, Debug)]
pub struct ToolSearchIndex {
    tools: Vec<SearchableTool>,
    index: Bm25Index,
}

impl ToolSearchIndex {
    pub fn build(specs: &[ToolSpec]) -> Self {
        let tools: Vec<SearchableTool> = specs.iter().map(SearchableTool::from_spec).collect();
        let index = Bm25Index::build(
            tools
                .iter()
                .map(|t| (t.name.as_str(), t.searchable.as_str()))
                .collect::<Vec<_>>(),
        );
        Self { tools, index }
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
        self.index
            .search(query, limit)
            .into_iter()
            .map(|i| &self.tools[i])
            .collect()
    }
}

/// Shared handle to the index.
///
/// The tool is constructed during registry assembly, before anything knows
/// which tools will end up deferred — that depends on the agent's belt, which
/// is resolved later in the session builder. So the tool owns an empty index
/// and the builder fills it in through [`bind_tool_search_index`], the same
/// two-step shape `toolpacks::bind_pack_registry` already uses.
pub type ToolSearchHandle = Arc<RwLock<ToolSearchIndex>>;

/// Look up a deferred capability by description.
pub struct ToolSearchTool {
    index: ToolSearchHandle,
}

impl Default for ToolSearchTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolSearchTool {
    pub fn new() -> Self {
        Self {
            index: Arc::new(RwLock::new(ToolSearchIndex::default())),
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

    /// Exposes the index so [`bind_tool_search_index`] can populate it after
    /// the agent's belt is resolved.
    ///
    /// Erased for the same reason `PackRegistryHandle` is: a `ToolSearchIndex`
    /// is this host's concept, and a vocabulary shared with other hosts has no
    /// business naming it.
    fn host_extension(&self) -> Option<&(dyn std::any::Any + Send + Sync)> {
        Some(&self.index)
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

        let index = self
            .index
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
            spec("memory_hybrid_search", "Search stored memories semantically"),
            spec("generate_presentation", "Build a pptx slide deck from an outline"),
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

/// Populate the registry's `tool_search` index with the deferred specs.
///
/// Returns `false` when the registry has no `tool_search` — which is not an
/// error: an agent whose belt defers nothing does not need one, and a build
/// with the tool compiled out is a legitimate configuration. It **is** worth a
/// warning when specs were deferred and there is nowhere to index them, because
/// that combination makes capabilities unreachable.
pub fn bind_tool_search_index(tools: &[Box<dyn Tool>], deferred: Vec<ToolSpec>) -> bool {
    let handle = tools.iter().find_map(|tool| {
        if tool.name() != TOOL_SEARCH_NAME {
            return None;
        }
        tool.host_extension()
            .and_then(|any| any.downcast_ref::<ToolSearchHandle>())
    });
    let Some(handle) = handle else {
        if !deferred.is_empty() {
            tracing::warn!(
                deferred = deferred.len(),
                "[tool_search] tools were deferred but no tool_search is registered; \
                 they are unreachable this session"
            );
        }
        return false;
    };
    match handle.write() {
        Ok(mut index) => {
            *index = ToolSearchIndex::build(&deferred);
            true
        }
        Err(_) => {
            tracing::error!("[tool_search] index lock poisoned; leaving it empty");
            false
        }
    }
}

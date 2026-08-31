//! `memory_vector_search` — direct semantic search over chunk embeddings.
//!
//! Pure cosine similarity over stored chunk embeddings. No graph scoring,
//! no LLM loop. Fast, single embedding call. Supports metadata filtering,
//! cross-namespace search, similarity threshold, and MMR diversity.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::fmt::Write;

use crate::openhuman::config::rpc as config_rpc;
use crate::openhuman::inference::embeddings::provider_from_config;
use crate::openhuman::memory::api::chunks::SourceKind;
use crate::openhuman::memory::api::provider::ChunkQuery;
use crate::openhuman::memory::api::provider::MemoryProvider;
use crate::openhuman::memory::ops::guard::active_memory_guard;
use crate::openhuman::tools::traits::{Tool, ToolExposure, ToolResult};
use tinycortex::memory::retrieval::mmr::{mmr_select, MmrCandidate};
use tinycortex::memory::store::vectors::cosine_similarity;

pub struct MemoryVectorSearchTool;

#[derive(Debug, Deserialize)]
struct Args {
    query: String,
    #[serde(default)]
    namespace: Option<String>,
    #[serde(default)]
    source_kind: Option<String>,
    #[serde(default)]
    time_window_days: Option<u32>,
    #[serde(default)]
    min_score: Option<f64>,
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default)]
    diverse: bool,
}

fn default_limit() -> usize {
    10
}

#[async_trait]
impl Tool for MemoryVectorSearchTool {
    /// Superseded by the `memory` tool, which dispatches every memory
    /// operation on one `action` field. Kept registered and dispatchable so a
    /// replayed transcript or a saved skill naming `memory_*` keeps working;
    /// hidden from the wire so eleven schemas do not ship where one does.
    fn exposure(&self) -> ToolExposure {
        ToolExposure::Hidden
    }

    fn name(&self) -> &str {
        "memory_vector_search"
    }

    fn description(&self) -> &str {
        "Direct semantic vector search over memory chunks. Embeds the query \
         and finds the most similar stored content by cosine similarity. \
         Fast (single embedding call, no LLM). Use for semantic lookup when \
         you know roughly what you're looking for. Returns chunk-level results \
         with scores."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "required": ["query"],
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Natural-language query to embed and search against stored memory chunks."
                },
                "source_kind": {
                    "type": "string",
                    "enum": ["chat", "email", "document"],
                    "description": "Filter to a specific source type."
                },
                "time_window_days": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Only include chunks from the last N days."
                },
                "min_score": {
                    "type": "number",
                    "minimum": 0.0,
                    "maximum": 1.0,
                    "description": "Minimum cosine similarity threshold (default 0.3)."
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 50,
                    "description": "Max results to return (default 10)."
                },
                "diverse": {
                    "type": "boolean",
                    "description": "Apply MMR diversity to reduce redundancy among results (default false)."
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let parsed: Args = serde_json::from_value(args)
            .map_err(|e| anyhow::anyhow!("invalid arguments for memory_vector_search: {e}"))?;

        if parsed.query.trim().is_empty() {
            return Err(anyhow::anyhow!(
                "memory_vector_search: query cannot be empty"
            ));
        }

        let limit = parsed.limit.clamp(1, 50);
        let min_score = parsed.min_score.unwrap_or(0.3);

        log::debug!(
            "[tool][memory_vector_search] query_len={} source_kind={:?} window={:?} min_score={} limit={} diverse={}",
            parsed.query.len(),
            parsed.source_kind,
            parsed.time_window_days,
            min_score,
            limit,
            parsed.diverse,
        );

        let config = config_rpc::load_config_with_timeout()
            .await
            .map_err(|e| anyhow::anyhow!("memory_vector_search: load config failed: {e}"))?;

        // Chunks are read through the bound driver, not by opening the store
        // in this process. Before the module port this called
        // `list_chunks(&config, …)` directly, which resolved the workspace path
        // and opened the same SQLite database the loaded module already had
        // open — two engine instances over one file, with the module not
        // authoritative. See `docs/specs/2026-08-13-memory-module-port.md` §2.1.
        let guard = active_memory_guard()
            .await
            .map_err(|e| anyhow::anyhow!("memory_vector_search: {e}"))?;
        let chunk_reader = guard.as_chunks().ok_or_else(|| {
            anyhow::anyhow!("memory_vector_search: memory driver does not support the chunk family")
        })?;

        let embedder = provider_from_config(&config)
            .map_err(|e| anyhow::anyhow!("memory_vector_search: embedding provider failed: {e}"))?;

        let query_vec = embedder
            .embed_one(&parsed.query)
            .await
            .map_err(|e| anyhow::anyhow!("memory_vector_search: embedding query failed: {e}"))?;

        let source_kind = match parsed.source_kind.as_deref() {
            Some(s) => Some(
                SourceKind::parse(s).map_err(|e| anyhow::anyhow!("memory_vector_search: {e}"))?,
            ),
            None => None,
        };

        let since_ms = parsed.time_window_days.map(|days| {
            let now_ms = chrono::Utc::now().timestamp_millis();
            now_ms - (i64::from(days) * 86_400_000)
        });

        // Fetch candidate chunks with metadata filters. The per-profile
        // memory-source gate is applied inside the driver's query (before the
        // row limit), so disallowed-source chunks can't starve permitted ones.
        //
        // `None` for the scope is not "unrestricted": the guard intersects it
        // with the ambient per-turn allowlist and passes the result down, so
        // naming a scope here could only ever *narrow* what the turn may see.
        let query = ChunkQuery {
            source_kind,
            source_id: None,
            owner: None,
            since_ms,
            until_ms: None,
            limit: Some(1000),
            offset: None,
            exclude_dropped: false,
            // The filtered-listing predicates this caller does not use. An
            // empty predicate is unfiltered, so the defaults leave the query
            // exactly as narrow as the fields above already make it.
            ..Default::default()
        };

        let chunks = chunk_reader
            .list_chunks(&query, None)
            .await
            .map_err(|e| anyhow::anyhow!("memory_vector_search: list chunks failed: {e}"))?;

        if chunks.is_empty() {
            return Ok(ToolResult::success("No chunks found matching filters."));
        }

        // Get embeddings for these chunks
        let chunk_ids: Vec<String> = chunks.iter().map(|c| c.id.clone()).collect();
        let model_sig = embedder.signature();
        let embeddings: std::collections::HashMap<String, Vec<f32>> = chunk_reader
            .chunk_embeddings(&chunk_ids, &model_sig)
            .await
            .map_err(|e| anyhow::anyhow!("memory_vector_search: load embeddings failed: {e}"))?
            .into_iter()
            .map(|embedding| (embedding.chunk_id, embedding.vector))
            .collect();

        // Score each chunk
        let mut scored: Vec<(usize, f64, &[f32])> = Vec::new();

        for (idx, chunk) in chunks.iter().enumerate() {
            let Some(emb) = embeddings.get(&chunk.id) else {
                continue;
            };
            if emb.len() != query_vec.len() {
                continue;
            }
            let score = cosine_similarity(&query_vec, emb);
            if score >= min_score {
                scored.push((idx, score, emb.as_slice()));
            }
        }

        if scored.is_empty() {
            return Ok(ToolResult::success(
                "No chunks scored above the similarity threshold.",
            ));
        }

        let results = if parsed.diverse && scored.len() > limit {
            let candidates: Vec<MmrCandidate<'_>> = scored
                .iter()
                .map(|(idx, score, emb)| MmrCandidate {
                    index: *idx,
                    embedding: emb,
                    relevance: *score,
                })
                .collect();
            let mmr_results = mmr_select(&query_vec, &candidates, limit, 0.7);
            mmr_results
                .into_iter()
                .map(|r| {
                    (
                        r.index,
                        scored.iter().find(|(i, _, _)| *i == r.index).unwrap().1,
                    )
                })
                .collect::<Vec<_>>()
        } else {
            scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            scored.truncate(limit);
            scored
                .iter()
                .map(|(idx, score, _)| (*idx, *score))
                .collect()
        };

        let mut output = format!("Found {} results:\n\n", results.len());
        for (chunk_idx, score) in &results {
            let chunk = &chunks[*chunk_idx];
            let preview: String = chunk.content.chars().take(300).collect();
            let truncated = if chunk.content.chars().count() > 300 {
                "..."
            } else {
                ""
            };
            let _ = writeln!(
                output,
                "- [{:.0}%] source={}:{} id={}\n  {}{}",
                score * 100.0,
                chunk.metadata.source_kind.as_str(),
                chunk.metadata.source_id,
                chunk.id,
                preview,
                truncated,
            );
        }

        log::debug!(
            "[tool][memory_vector_search] returning {} results from {} candidates",
            results.len(),
            chunks.len(),
        );

        Ok(ToolResult::success(output))
    }
}

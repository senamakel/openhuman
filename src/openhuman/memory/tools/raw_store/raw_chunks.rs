//! `memory_store_raw_chunks` — structured chunk filter.
//!
//! Bypasses ranking entirely. Returns chunks (timestamp DESC) matching the
//! supplied source/owner/time/tag filters. Use when the agent knows the
//! exact subset of memory it wants to inspect.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use crate::openhuman::memory::api::chunks::SourceKind;
use crate::openhuman::memory::api::provider::{ChunkQuery, MemoryProvider};
use crate::openhuman::memory::ops::guard::active_memory_guard;
use crate::openhuman::tools::traits::{Tool, ToolResult};

pub struct MemoryStoreRawChunksTool;

#[derive(Debug, Deserialize)]
struct Args {
    #[serde(default)]
    source_kind: Option<String>,
    #[serde(default)]
    source_id: Option<String>,
    #[serde(default)]
    owner: Option<String>,
    #[serde(default)]
    since_ms: Option<i64>,
    #[serde(default)]
    until_ms: Option<i64>,
    #[serde(default)]
    tags_all_of: Option<Vec<String>>,
    #[serde(default)]
    limit: Option<usize>,
}

#[async_trait]
impl Tool for MemoryStoreRawChunksTool {
    fn name(&self) -> &str {
        "memory_store_raw_chunks"
    }

    fn description(&self) -> &str {
        "List raw memory_store chunks (timestamp DESC) matching structured \
         filters: source kind, source id, owner, time range, required tags. \
         No scoring or rerank — use for exact-subset inspection, not search. \
         Returns full Chunk rows with metadata and content."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "source_kind": { "type": "string", "enum": ["chat", "email", "document"] },
                "source_id":   { "type": "string", "description": "Exact source id." },
                "owner":       { "type": "string", "description": "Owner / account filter." },
                "since_ms":    { "type": "integer", "description": "Inclusive lower bound on timestamp_ms." },
                "until_ms":    { "type": "integer", "description": "Inclusive upper bound on timestamp_ms." },
                "tags_all_of": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Post-filter: chunk.metadata.tags must contain every tag listed."
                },
                "limit":       { "type": "integer", "minimum": 1, "maximum": 1000, "description": "Default 100." }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let parsed: Args = serde_json::from_value(args)
            .map_err(|e| anyhow::anyhow!("invalid arguments for memory_store_raw_chunks: {e}"))?;
        log::debug!(
            "[tool][memory_store] raw_chunks source_kind={:?} owner={:?} tags={:?} limit={:?}",
            parsed.source_kind,
            parsed.owner,
            parsed.tags_all_of,
            parsed.limit
        );
        let source_kind = match parsed.source_kind.as_deref() {
            Some(s) => Some(
                SourceKind::parse(s)
                    .map_err(|e| anyhow::anyhow!("memory_store_raw_chunks: {e}"))?,
            ),
            None => None,
        };
        if let Some(limit) = parsed.limit {
            if !(1..=1000).contains(&limit) {
                return Err(anyhow::anyhow!(
                    "memory_store_raw_chunks: limit must be between 1 and 1000"
                ));
            }
        }
        // The per-profile memory-source gate is applied inside `list_chunks`
        // (before the row limit). None = unrestricted.
        let query = ChunkQuery {
            source_kind,
            source_id: parsed.source_id,
            owner: parsed.owner,
            since_ms: parsed.since_ms,
            until_ms: parsed.until_ms,
            limit: parsed.limit,
            offset: None,
            exclude_dropped: false,
            // The filtered-listing predicates this caller does not use. An
            // empty predicate is unfiltered, so the defaults leave the query
            // exactly as narrow as the fields above already make it.
            ..Default::default()
        };
        let guard = active_memory_guard()
            .await
            .map_err(|e| anyhow::anyhow!("memory_store_raw_chunks: {e}"))?;
        let mut rows = guard
            .as_chunks()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "memory_store_raw_chunks: memory driver does not support the chunk family"
                )
            })?
            .list_chunks(&query, None)
            .await?;
        if let Some(required) = parsed.tags_all_of.as_ref() {
            if !required.is_empty() {
                rows.retain(|c| {
                    required
                        .iter()
                        .all(|t| c.metadata.tags.iter().any(|ct| ct == t))
                });
            }
        }
        log::debug!(
            "[tool][memory_store] raw_chunks returning rows={}",
            rows.len()
        );
        let json = serde_json::to_string(&rows)?;
        Ok(ToolResult::success(json))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    use tempfile::TempDir;

    use crate::openhuman::config::Config;
    use crate::openhuman::config::TEST_ENV_LOCK;
    use crate::openhuman::tools::traits::Tool;
    use serde_json::json;

    struct WorkspaceEnvGuard {
        _lock: std::sync::MutexGuard<'static, ()>,
        previous: Option<OsString>,
    }

    impl WorkspaceEnvGuard {
        fn set(path: &std::path::Path) -> Self {
            let lock = TEST_ENV_LOCK.lock().unwrap_or_else(|err| err.into_inner());
            let previous = std::env::var_os("OPENHUMAN_WORKSPACE");
            unsafe {
                std::env::set_var("OPENHUMAN_WORKSPACE", path);
            }
            Self {
                _lock: lock,
                previous,
            }
        }
    }

    impl Drop for WorkspaceEnvGuard {
        fn drop(&mut self) {
            unsafe {
                if let Some(previous) = self.previous.as_ref() {
                    std::env::set_var("OPENHUMAN_WORKSPACE", previous);
                } else {
                    std::env::remove_var("OPENHUMAN_WORKSPACE");
                }
            }
        }
    }

    async fn isolated_config(tmp: &TempDir) -> (WorkspaceEnvGuard, Config) {
        let guard = WorkspaceEnvGuard::set(tmp.path());
        let config = Config::load_or_init().await.expect("load config");
        (guard, config)
    }

    #[test]
    fn args_deserialize_optional_filters() {
        let args: Args = serde_json::from_value(json!({
            "source_kind": "chat",
            "source_id": "slack:#eng",
            "owner": "alice",
            "since_ms": 10,
            "until_ms": 20,
            "tags_all_of": ["person:alice"],
            "limit": 25
        }))
        .unwrap();

        assert_eq!(args.source_kind.as_deref(), Some("chat"));
        assert_eq!(args.source_id.as_deref(), Some("slack:#eng"));
        assert_eq!(args.owner.as_deref(), Some("alice"));
        assert_eq!(args.since_ms, Some(10));
        assert_eq!(args.until_ms, Some(20));
        assert_eq!(args.tags_all_of, Some(vec!["person:alice".to_string()]));
        assert_eq!(args.limit, Some(25));
    }

    #[test]
    fn parameters_schema_exposes_supported_source_kinds() {
        let tool = MemoryStoreRawChunksTool;
        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(
            schema["properties"]["source_kind"]["enum"],
            json!(["chat", "email", "document"])
        );
        assert_eq!(schema["properties"]["limit"]["maximum"], 1000);
    }

    #[tokio::test]
    async fn execute_rejects_invalid_source_kind() {
        let tool = MemoryStoreRawChunksTool;
        let err = tool
            .execute(json!({
                "source_kind": "not-real"
            }))
            .await
            .expect_err("invalid source kind should fail");
        assert!(err.to_string().contains("memory_store_raw_chunks:"));
    }

    #[tokio::test]
    async fn execute_rejects_wrong_type_for_limit() {
        let tool = MemoryStoreRawChunksTool;
        let err = tool
            .execute(json!({
                "limit": "ten"
            }))
            .await
            .expect_err("wrong limit type should fail");
        assert!(err
            .to_string()
            .contains("invalid arguments for memory_store_raw_chunks"));
    }

    #[tokio::test]
    #[ignore = "needs a built tinymemory module (OPENHUMAN_MODULE_PATH) and its own process: \
the module bus belongs to the runtime that creates it, so run this test alone"]
    // Was a pure-SQLite test: it opened the workspace store in-process and read
    // an empty table. That is the split brain this port removes — the tool now
    // reads chunks through the bound driver, so the success path needs a driver
    // that advertises the chunk family. With no module artifact the binding
    // falls back to the null driver and the tool refuses, which is the correct
    // answer rather than a regression.
    async fn execute_success_path_returns_json_array() {
        let tmp = TempDir::new().expect("tempdir");
        let (_workspace, _config) = isolated_config(&tmp).await;
        let tool = MemoryStoreRawChunksTool;
        let result = tool
            .execute(json!({
                "source_kind": "document",
                "limit": 2
            }))
            .await
            .expect("valid raw_chunks request should succeed");
        assert!(!result.is_error);
        let parsed: serde_json::Value =
            serde_json::from_str(&result.text()).expect("tool result should be json");
        assert!(
            parsed.is_array(),
            "raw_chunks should serialize a JSON array"
        );
    }
}

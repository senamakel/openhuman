use crate::openhuman::memory::api::provider::MemoryRecall;
use crate::openhuman::memory::ops::guard::active_memory_guard;
use crate::openhuman::tools::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;
use std::fmt::Write;

/// Let the agent search its own memory.
///
/// Holds no memory handle: it resolves the guarded driver per call, like every
/// other memory tool in this port. That is what lets the session builder stop
/// threading an `Arc<dyn Memory>` through tool construction.
pub struct MemoryRecallTool;

impl MemoryRecallTool {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for MemoryRecallTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for MemoryRecallTool {
    fn name(&self) -> &str {
        "memory_recall"
    }

    fn description(&self) -> &str {
        "Search memory for relevant facts in a namespace. Returns scored results ranked by relevance."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Keywords or phrase to search for in memory"
                },
                "namespace": {
                    "type": "string",
                    "description": "Namespace to search (e.g. 'global', 'background', 'autocomplete', or 'skill-{id}')"
                },
                "limit": {
                    "type": "integer",
                    "description": "Max results to return (default: 5)"
                }
            },
            "required": ["namespace", "query"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let namespace = args
            .get("namespace")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'namespace' parameter"))?
            .trim();
        if namespace.is_empty() {
            return Err(anyhow::anyhow!("namespace cannot be empty"));
        }
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'query' parameter"))?
            .trim();
        if query.is_empty() {
            return Err(anyhow::anyhow!("query cannot be empty"));
        }

        #[allow(clippy::cast_possible_truncation)]
        let limit = args
            .get("limit")
            .and_then(serde_json::Value::as_u64)
            .map_or(5, |v| v as usize);

        // Search with the user query only. Prefixing `namespace` into the query
        // string would add a redundant token matching almost every row. Instead,
        // namespace scoping belongs in RecallOpts so the backend restricts the
        // search to the correct namespace column.
        let recall_opts = crate::openhuman::memory::api::recall::OwnedRecallOpts {
            namespace: Some(namespace.to_string()),
            ..Default::default()
        };
        let guard = active_memory_guard()
            .await
            .map_err(|e| anyhow::anyhow!("memory_recall: {e}"))?;
        // `None` scope: the guard intersects it with the ambient per-turn
        // allowlist, so this can only ever be narrowed, never widened.
        match guard.recall(query, limit, &recall_opts, None).await {
            Ok(entries) if entries.is_empty() => Ok(ToolResult::success(
                "No memories found matching that query.",
            )),
            Ok(entries) => {
                let mut output = format!("Found {} memories:\n", entries.len());
                for entry in &entries {
                    let score = entry
                        .score
                        .map_or_else(String::new, |s| format!(" [{s:.0}%]"));
                    let _ = writeln!(
                        output,
                        "- [{}] {}: {}{score}",
                        entry.category, entry.key, entry.content
                    );
                }
                Ok(ToolResult::success(output))
            }
            Err(e) => Ok(ToolResult::error(format!("Memory recall failed: {e}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openhuman::memory::api::provider::MemoryCore;
    use crate::openhuman::memory::api::types::MemoryTaint;
    use crate::openhuman::memory::MemoryCategory;

    /// A namespace nothing else writes to.
    ///
    /// The tool holds no handle — it recalls through the *bound* driver, which
    /// under a real module is the process-global test workspace every other
    /// test shares. A fixed namespace would make the `Found N` counts below
    /// depend on whatever else ran, so each test owns its own.
    fn unique_namespace() -> String {
        format!(
            "recall{}",
            &uuid::Uuid::new_v4().as_simple().to_string()[..12]
        )
    }

    /// Seed through the guard — the door the tool itself recalls through.
    ///
    /// A fixture store built here would be a different store entirely, so
    /// seeding into one would leave `recall_finds_match` passing only because
    /// it found nothing.
    async fn seed(namespace: &str, key: &str, content: &str) {
        active_memory_guard()
            .await
            .expect("a bound memory guard")
            .store(
                namespace,
                key,
                content,
                MemoryCategory::Core,
                None,
                MemoryTaint::default(),
            )
            .await
            .expect("seed through the guard");
    }

    #[tokio::test]
    #[ignore = "needs a built tinymemory module (OPENHUMAN_MODULE_PATH) and its own process: \
the tool resolves the bound driver rather than being handed a memory handle"]
    async fn recall_empty() {
        let namespace = unique_namespace();
        let tool = MemoryRecallTool::new();
        let result = tool
            .execute(json!({"namespace": namespace, "query": "anything"}))
            .await
            .unwrap();
        assert!(!result.is_error);
        assert!(result.output().contains("No memories found"));
    }

    #[tokio::test]
    #[ignore = "needs a built tinymemory module (OPENHUMAN_MODULE_PATH) and its own process: \
the tool resolves the bound driver rather than being handed a memory handle"]
    async fn recall_finds_match() {
        let namespace = unique_namespace();
        seed(&namespace, "lang", "User prefers Rust").await;
        seed(&namespace, "tz", "Timezone is EST").await;

        let tool = MemoryRecallTool::new();
        let result = tool
            .execute(json!({"namespace": namespace, "query": "Rust"}))
            .await
            .unwrap();
        assert!(!result.is_error);
        assert!(result.output().contains("Rust"));
        assert!(result.output().contains("Found 1"));
    }

    #[tokio::test]
    #[ignore = "needs a built tinymemory module (OPENHUMAN_MODULE_PATH) and its own process: \
the tool resolves the bound driver rather than being handed a memory handle"]
    async fn recall_respects_limit() {
        let namespace = unique_namespace();
        for i in 0..10 {
            seed(&namespace, &format!("k{i}"), &format!("Rust fact {i}")).await;
        }

        let tool = MemoryRecallTool::new();
        let result = tool
            .execute(json!({"namespace": namespace, "query": "Rust", "limit": 3}))
            .await
            .unwrap();
        assert!(!result.is_error);
        assert!(result.output().contains("Found 3"));
    }

    #[tokio::test]
    #[ignore = "needs a built tinymemory module (OPENHUMAN_MODULE_PATH) and its own process: \
the tool resolves the bound driver rather than being handed a memory handle"]
    async fn recall_missing_query() {
        let tool = MemoryRecallTool::new();
        let result = tool.execute(json!({})).await;
        assert!(result.is_err());
    }

    /// Pure schema assertion — needs no store at all now that the tool holds
    /// no handle.
    #[test]
    fn name_and_schema() {
        let tool = MemoryRecallTool::new();
        assert_eq!(tool.name(), "memory_recall");
        assert!(tool.parameters_schema()["properties"]["query"].is_object());
    }
}

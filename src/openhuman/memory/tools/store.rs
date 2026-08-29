use crate::openhuman::memory::api::provider::MemoryCore;
use crate::openhuman::memory::api::types::{MemoryCategory, MemoryTaint};
use crate::openhuman::memory::ops::guard::active_memory_guard;
use crate::openhuman::memory::safety;
use crate::openhuman::security::policy::ToolOperation;
use crate::openhuman::security::SecurityPolicy;
use crate::openhuman::tools::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

/// Let the agent store memories — its own brain writes
pub struct MemoryStoreTool {
    security: Arc<SecurityPolicy>,
}

impl MemoryStoreTool {
    /// Holds no memory handle — the guarded driver is resolved per call.
    #[must_use]
    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        Self { security }
    }
}

#[async_trait]
impl Tool for MemoryStoreTool {
    fn name(&self) -> &str {
        "memory_store"
    }

    fn description(&self) -> &str {
        "Store a general fact or note in an explicit namespace (e.g. global, background, autocomplete, skill-{id}). NOT for preferences — those go to `save_preference`, which writes the store the assistant actually reads. Check `memory_recall` for a near-duplicate first, and call `update_memory_md` afterwards, when you have those tools."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "key": {
                    "type": "string",
                    "description": "Unique key for this memory (e.g. 'user_lang', 'project_stack')"
                },
                "namespace": {
                    "type": "string",
                    "description": "Target namespace (e.g. 'global', 'background', 'autocomplete', or 'skill-{id}')"
                },
                "content": {
                    "type": "string",
                    "description": "The information to remember"
                },
                "category": {
                    "type": "string",
                    "description": "Memory category: 'core' (permanent), 'daily' (session), 'conversation' (chat), or a custom category name. Defaults to 'core'."
                }
            },
            "required": ["namespace", "key", "content"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let namespace = args
            .get("namespace")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'namespace' parameter"))?;
        let key = args
            .get("key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'key' parameter"))?;

        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'content' parameter"))?;

        let category = match args.get("category").and_then(|v| v.as_str()) {
            Some("core") | None => MemoryCategory::Core,
            Some("daily") => MemoryCategory::Daily,
            Some("conversation") => MemoryCategory::Conversation,
            // Route custom categories through `FromStr` so a `custom:<name>`
            // wire value — the form `memory_recall`/`Display` now emit — resolves
            // back to `Custom("<name>")` instead of `Custom("custom:<name>")`
            // (which would `Display` as `custom:custom:<name>` and stop matching
            // the original category on recall/filter). Legacy bare names still
            // parse to the same `Custom(name)`; an unparseable value falls back
            // to the raw string. (review: prefixed-custom round-trip)
            Some(other) => other
                .parse()
                .unwrap_or_else(|_| MemoryCategory::Custom(other.to_string())),
        };

        if let Err(error) = self
            .security
            .enforce_tool_operation(ToolOperation::Act, "memory_store")
        {
            return Ok(ToolResult::error(error));
        }

        let namespace = namespace.trim();
        if namespace.is_empty() {
            return Ok(ToolResult::error("namespace cannot be empty".to_string()));
        }
        let key = key.trim();
        if key.is_empty() {
            return Ok(ToolResult::error("key cannot be empty".to_string()));
        }

        if safety::has_likely_secret(content) {
            log::warn!(
                "[memory:safety] memory_store rejected secret-like content namespace_chars={} key_chars={} content_chars={}",
                namespace.chars().count(),
                key.chars().count(),
                content.chars().count()
            );
            return Ok(ToolResult::error(
                "Refusing to store content that looks like a secret. Remove credentials or tokens and try again.".to_string(),
            ));
        }

        let display_key = format!("{namespace}/{key}");
        let guard = active_memory_guard()
            .await
            .map_err(|e| anyhow::anyhow!("memory_store: {e}"))?;
        match guard
            .store(
                namespace,
                key,
                content,
                category,
                None,
                // Requested provenance; the guard stamps the effective value.
                MemoryTaint::default(),
            )
            .await
        {
            Ok(()) => Ok(ToolResult::success(format!("Stored memory: {display_key}"))),
            Err(e) => Ok(ToolResult::error(format!("Failed to store memory: {e}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openhuman::security::{AutonomyLevel, SecurityPolicy};

    use crate::openhuman::memory::api::types::MemoryEntry;

    fn test_security() -> Arc<SecurityPolicy> {
        Arc::new(SecurityPolicy::default())
    }

    /// Read a memory back through the seam the tool wrote it through.
    ///
    /// The tool holds no handle at all — it resolves the *bound* driver per
    /// call — so a fixture store built here could never see the write: under a
    /// real module the bound driver is the process-global test workspace, a
    /// different store entirely. That is why no fixture handle exists in this
    /// module, and why an absence assertion must not be made against one; it
    /// would hold whether or not the refusal under test worked, which is worse
    /// than no assertion at all.
    ///
    /// The guard is the tool's own door, so a read through it proves the write
    /// landed where a caller would look for it.
    async fn stored(namespace: &str, key: &str) -> Option<MemoryEntry> {
        active_memory_guard()
            .await
            .expect("a bound memory guard")
            .get(namespace, key)
            .await
            .expect("read back through the guard")
    }

    #[test]
    fn name_and_schema() {
        let tool = MemoryStoreTool::new(test_security());
        assert_eq!(tool.name(), "memory_store");
        let schema = tool.parameters_schema();
        assert!(schema["properties"]["key"].is_object());
        assert!(schema["properties"]["content"].is_object());
        // The memory protocol (#4116) must be stated up front so the model recalls
        // for dedupe before writing and reconciles the index after.
        let desc = tool.description();
        assert!(
            desc.contains("memory_recall") && desc.contains("update_memory_md"),
            "memory_store description must state the read→dedupe→write→update contract: {desc}"
        );
    }

    #[tokio::test]
    #[ignore = "needs a built tinymemory module (OPENHUMAN_MODULE_PATH) and its own process: \
the tool resolves the bound driver rather than being handed a memory handle"]
    async fn store_core() {
        let tool = MemoryStoreTool::new(test_security());
        let result = tool
            .execute(json!({"namespace": "global", "key": "lang", "content": "Prefers Rust"}))
            .await
            .unwrap();
        assert!(!result.is_error);
        assert!(result.output().contains("lang"));

        let entry = stored("global", "lang").await;
        assert!(
            entry.is_some(),
            "the write is visible through the tool's own seam"
        );
        assert_eq!(entry.unwrap().content, "Prefers Rust");
    }

    #[tokio::test]
    #[ignore = "needs a built tinymemory module (OPENHUMAN_MODULE_PATH) and its own process: \
the tool resolves the bound driver rather than being handed a memory handle"]
    async fn store_with_category() {
        let tool = MemoryStoreTool::new(test_security());
        let result = tool
            .execute(
                json!({"namespace": "global", "key": "note", "content": "Fixed bug", "category": "daily"}),
            )
            .await
            .unwrap();
        assert!(!result.is_error);
    }

    #[tokio::test]
    #[ignore = "needs a built tinymemory module (OPENHUMAN_MODULE_PATH) and its own process: \
the tool resolves the bound driver rather than being handed a memory handle"]
    async fn store_with_custom_category() {
        let tool = MemoryStoreTool::new(test_security());
        let result = tool
            .execute(
                json!({"namespace": "global", "key": "proj_note", "content": "Uses async runtime", "category": "project"}),
            )
            .await
            .unwrap();
        assert!(!result.is_error);

        let entry = stored("global", "proj_note")
            .await
            .expect("the stored memory is readable through the guard");
        assert_eq!(entry.content, "Uses async runtime");
        assert_eq!(entry.category, MemoryCategory::Custom("project".into()));
    }

    /// Regression: a `custom:<name>` wire value (the form `memory_recall` and
    /// `Display` now emit) must store as `Custom("<name>")`, not the
    /// double-prefixed `Custom("custom:<name>")` — otherwise it would `Display`
    /// as `custom:custom:<name>` and stop matching the original category.
    #[tokio::test]
    #[ignore = "needs a built tinymemory module (OPENHUMAN_MODULE_PATH) and its own process: \
the tool resolves the bound driver rather than being handed a memory handle"]
    async fn store_strips_custom_prefix_from_wire_category() {
        let tool = MemoryStoreTool::new(test_security());
        let result = tool
            .execute(json!({
                "namespace": "global",
                "key": "wire_prefixed_note",
                "content": "Uses async runtime",
                "category": "custom:project"
            }))
            .await
            .unwrap();
        assert!(!result.is_error);

        let entry = stored("global", "wire_prefixed_note")
            .await
            .expect("the stored memory is readable through the guard");
        assert_eq!(
            entry.category,
            MemoryCategory::Custom("project".into()),
            "the `custom:` wire prefix must be stripped, not double-stored"
        );
    }

    #[tokio::test]
    #[ignore = "needs a built tinymemory module (OPENHUMAN_MODULE_PATH) and its own process: \
the tool resolves the bound driver rather than being handed a memory handle"]
    async fn store_rejects_secret_like_content() {
        let tool = MemoryStoreTool::new(test_security());
        let result = tool
            .execute(json!({
                "namespace": "global",
                "key": "api",
                "content": "api_key=sk-123456789012345678901234567890"
            }))
            .await
            .unwrap();
        assert!(result.is_error);
        assert!(result.output().contains("looks like a secret"));
        // Through the guard — see `stored`: an absence assertion against a
        // store the tool never writes to holds whether or not the refusal
        // worked, which makes it worse than no assertion at all.
        assert!(stored("global", "api").await.is_none());
    }

    #[tokio::test]
    #[ignore = "needs a built tinymemory module (OPENHUMAN_MODULE_PATH) and its own process: \
the tool resolves the bound driver rather than being handed a memory handle"]
    async fn store_missing_key() {
        let tool = MemoryStoreTool::new(test_security());
        let result = tool.execute(json!({"content": "no key"})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    #[ignore = "needs a built tinymemory module (OPENHUMAN_MODULE_PATH) and its own process: \
the tool resolves the bound driver rather than being handed a memory handle"]
    async fn store_missing_content() {
        let tool = MemoryStoreTool::new(test_security());
        let result = tool.execute(json!({"key": "no_content"})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    #[ignore = "needs a built tinymemory module (OPENHUMAN_MODULE_PATH) and its own process: \
the tool resolves the bound driver rather than being handed a memory handle"]
    async fn store_blocked_in_readonly_mode() {
        let readonly = Arc::new(SecurityPolicy {
            autonomy: AutonomyLevel::ReadOnly,
            ..SecurityPolicy::default()
        });
        let tool = MemoryStoreTool::new(readonly);
        let result = tool
            .execute(
                json!({"namespace": "global", "key": "readonly_lang", "content": "Prefers Rust"}),
            )
            .await
            .unwrap();
        assert!(result.is_error);
        assert!(result.output().contains("read-only mode"));
        assert!(stored("global", "readonly_lang").await.is_none());
    }

    #[tokio::test]
    #[ignore = "needs a built tinymemory module (OPENHUMAN_MODULE_PATH) and its own process: \
the tool resolves the bound driver rather than being handed a memory handle"]
    async fn store_blocked_when_rate_limited() {
        let limited = Arc::new(SecurityPolicy {
            max_actions_per_hour: 0,
            ..SecurityPolicy::default()
        });
        let tool = MemoryStoreTool::new(limited);
        let result = tool
            .execute(json!({"namespace": "global", "key": "ratelimited_lang", "content": "Prefers Rust"}))
            .await
            .unwrap();
        assert!(result.is_error);
        assert!(result.output().contains("Rate limit exceeded"));
        assert!(stored("global", "ratelimited_lang").await.is_none());
    }
}

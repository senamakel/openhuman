//! `memory_store_kinds` — introspection. Enumerate every storage shape the
//! bound driver persists, so an agent can plan a fan-out without hard-coding.
//!
//! The catalog comes from the driver rather than from a compiled-in list: it is
//! the engine's own vocabulary, and a host-side copy drifts. This one had —
//! the description below used to advertise `content`, `document` and `graph`,
//! none of which exist, while omitting `raw` and `entity`, which do.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::openhuman::memory::api::provider::MemoryProvider;
use crate::openhuman::memory::ops::guard::active_memory_guard;
use crate::openhuman::tools::traits::{Tool, ToolExposure, ToolResult};

pub struct MemoryStoreKindsTool;

#[async_trait]
impl Tool for MemoryStoreKindsTool {
    /// Superseded by the `memory` tool, which dispatches every memory
    /// operation on one `action` field. Kept registered and dispatchable so a
    /// replayed transcript or a saved skill naming `memory_*` keeps working;
    /// hidden from the wire so eleven schemas do not ship where one does.
    fn exposure(&self) -> ToolExposure {
        ToolExposure::Hidden
    }

    fn name(&self) -> &str {
        "memory_store_kinds"
    }

    fn description(&self) -> &str {
        "Return the catalog of memory_store storage kinds the active memory \
         driver persists. No arguments. Use when planning a multi-kind \
         retrieval fan-out."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({ "type": "object", "properties": {} })
    }

    async fn execute(&self, _args: Value) -> anyhow::Result<ToolResult> {
        log::debug!("[tool][memory_store] kinds start");
        let guard = active_memory_guard()
            .await
            .map_err(|e| anyhow::anyhow!("memory_store_kinds: {e}"))?;
        let kinds = guard
            .as_chunks()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "memory_store_kinds: memory driver does not support the chunk family"
                )
            })?
            .storage_kinds()
            .await
            .map_err(|e| anyhow::anyhow!("memory_store_kinds: {e}"))?;
        log::debug!("[tool][memory_store] kinds success count={}", kinds.len());
        Ok(ToolResult::success(serde_json::to_string(
            &json!({ "kinds": kinds }),
        )?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameters_schema_is_empty_object() {
        let tool = MemoryStoreKindsTool;
        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"], json!({}));
    }

    /// The catalog is the driver's now, so this needs one bound.
    ///
    /// It used to assert against `MemoryKind::ALL` compiled into this crate,
    /// which is exactly the host-side copy that had drifted from the engine.
    #[tokio::test]
    #[ignore = "needs a built tinymemory module (OPENHUMAN_MODULE_PATH) and its own process: \
the catalog is read from the bound driver, not a compiled-in list"]
    async fn execute_returns_the_drivers_storage_kinds() {
        let tool = MemoryStoreKindsTool;
        let result = tool.execute(Value::Null).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result.output()).unwrap();
        assert!(
            parsed["kinds"].as_array().is_some_and(|k| !k.is_empty()),
            "a bound driver must report a non-empty catalog"
        );
    }
}

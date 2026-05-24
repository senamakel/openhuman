//! `memory_store_kinds` — introspection. Enumerate every supported
//! [`MemoryKind`] so an agent can plan a fan-out without hard-coding.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::openhuman::memory_store::MemoryKind;
use crate::openhuman::tools::traits::{Tool, ToolResult};

pub struct MemoryStoreKindsTool;

#[async_trait]
impl Tool for MemoryStoreKindsTool {
    fn name(&self) -> &str {
        "memory_store_kinds"
    }

    fn description(&self) -> &str {
        "Return the catalog of memory_store storage kinds (content, chunk, \
         tree, vector, document, kv, graph, contact). No arguments. Use \
         when planning a multi-kind retrieval fan-out."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({ "type": "object", "properties": {} })
    }

    async fn execute(&self, _args: Value) -> anyhow::Result<ToolResult> {
        let kinds: Vec<&'static str> = MemoryKind::ALL.iter().map(|k| k.as_str()).collect();
        let json = serde_json::to_string(&json!({ "kinds": kinds }))?;
        Ok(ToolResult::success(json))
    }
}

//! `memory` — the memory surface as one action-dispatched tool.
//!
//! Replaces eleven advertised schemas (`memory_store`, `memory_recall`,
//! `memory_forget`, `memory_doctor`, `memory_flavour`, `memory_vector_search`,
//! `memory_chunk_context`, `memory_hybrid_search`, `memory_store_raw_search`,
//! `memory_store_raw_chunks`, `memory_store_kinds`) with one. Between them they
//! were 7,879 bytes on every request, and three of the eleven are variations on
//! "search this index with a query and a limit".
//!
//! Hermes' whole memory surface is a single `memory` tool for the same reason.
//!
//! # `memory_tree` is deliberately NOT folded in
//!
//! It is already a collapsed tool: it dispatches eight operations on a `mode`
//! field over the ingested email/chat/document tree, which is a different
//! subsystem with a different storage model. Folding it in would produce
//! two-level dispatch — `action: "tree"` plus `mode: "drill_down"` — which is
//! harder for a model to get right than two tools, and would put its 3 KB of
//! schema behind an action most turns never take. Two tools that each dispatch
//! once beat one tool that dispatches twice.
//!
//! # Permissions
//!
//! The members disagree: `memory_recall` reads, `memory_store` writes, and
//! `memory_forget` destroys. `permission_level_with_args` resolves the real one
//! from the action; the argument-free `permission_level` reports the strictest
//! so an argument-less caller over-restricts. See
//! `tools::implementations::meta::collapse`.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use super::doctor::MemoryDoctorTool;
use super::flavour::MemoryFlavourTool;
use super::forget::MemoryForgetTool;
use super::raw_store::{MemoryStoreKindsTool, MemoryStoreRawChunksTool, MemoryStoreRawSearchTool};
use super::recall::MemoryRecallTool;
use super::search::{MemoryChunkContextTool, MemoryHybridSearchTool, MemoryVectorSearchTool};
use super::store::MemoryStoreTool;
use crate::openhuman::config::Config;
use crate::openhuman::security::policy::SecurityPolicy;
use crate::openhuman::tools::implementations::meta::collapse::{
    any_external_effect, args_without_action, merge_action_schemas, resolve, strictest_permission,
    unknown_action_message, CollapsedAction,
};
use crate::openhuman::tools::traits::{PermissionLevel, Tool, ToolCallOptions, ToolResult};

#[cfg(test)]
use crate::openhuman::tools::traits::ToolExposure;

/// The advertised name.
pub const MEMORY_TOOL_NAME: &str = "memory";

pub struct MemoryTool {
    store: MemoryStoreTool,
    recall: MemoryRecallTool,
    forget: MemoryForgetTool,
    doctor: MemoryDoctorTool,
    flavour: MemoryFlavourTool,
    hybrid_search: MemoryHybridSearchTool,
    vector_search: MemoryVectorSearchTool,
    chunk_context: MemoryChunkContextTool,
    raw_search: MemoryStoreRawSearchTool,
    raw_chunks: MemoryStoreRawChunksTool,
    kinds: MemoryStoreKindsTool,
}

impl MemoryTool {
    pub fn new(config: Arc<Config>, security: Arc<SecurityPolicy>) -> Self {
        Self {
            store: MemoryStoreTool::new(Arc::clone(&security)),
            recall: MemoryRecallTool::new(),
            forget: MemoryForgetTool::new(security),
            doctor: MemoryDoctorTool::new(Arc::clone(&config)),
            flavour: MemoryFlavourTool::new(config),
            hybrid_search: MemoryHybridSearchTool,
            vector_search: MemoryVectorSearchTool,
            chunk_context: MemoryChunkContextTool,
            raw_search: MemoryStoreRawSearchTool,
            raw_chunks: MemoryStoreRawChunksTool,
            kinds: MemoryStoreKindsTool,
        }
    }

    /// The action table, in the order it is advertised.
    ///
    /// Ordered by how often a turn needs it — `recall` and `store` first — so
    /// the enum reads as a recommendation as well as a list.
    ///
    /// **Filtered by memory capability.** The eleven members span five of them
    /// (`Core`, `Recall`, `Tree`, `Entities`, `Maintenance`), and the registry
    /// drops a tool whose capability the active memory driver does not serve —
    /// on the stated principle that absence beats a registered tool that
    /// fails. Collapsing would have quietly broken that: one tool cannot be
    /// dropped for one capability, so an unavailable action would sit in the
    /// enum inviting a call that always errors. Filtering here keeps the
    /// original behaviour, one action at a time.
    fn actions(&self) -> Vec<CollapsedAction<'_>> {
        self.all_actions()
            .into_iter()
            .filter(|entry| {
                crate::core::all::capability_allowed(
                    crate::openhuman::tools::ops::tool_capability(entry.tool.name()),
                )
            })
            .collect()
    }

    /// Every action this tool can serve, before capability filtering.
    fn all_actions(&self) -> Vec<CollapsedAction<'_>> {
        vec![
            CollapsedAction { action: "recall", tool: &self.recall },
            CollapsedAction { action: "store", tool: &self.store },
            CollapsedAction { action: "forget", tool: &self.forget },
            CollapsedAction { action: "hybrid_search", tool: &self.hybrid_search },
            CollapsedAction { action: "vector_search", tool: &self.vector_search },
            CollapsedAction { action: "chunk_context", tool: &self.chunk_context },
            CollapsedAction { action: "raw_search", tool: &self.raw_search },
            CollapsedAction { action: "raw_chunks", tool: &self.raw_chunks },
            CollapsedAction { action: "kinds", tool: &self.kinds },
            CollapsedAction { action: "flavour", tool: &self.flavour },
            CollapsedAction { action: "doctor", tool: &self.doctor },
        ]
    }
}

#[async_trait]
impl Tool for MemoryTool {
    fn name(&self) -> &str {
        MEMORY_TOOL_NAME
    }

    fn description(&self) -> &str {
        "Read and write the user's long-term memory. `action`: `recall` \
         (retrieve memories for a query — start here), `store` (save a durable \
         fact), `forget` (delete one), `hybrid_search` (keyword + semantic over \
         stored chunks), `vector_search` (semantic only), `chunk_context` \
         (surrounding text for a chunk you already have), `raw_search` / \
         `raw_chunks` / `kinds` (the raw ingest store and what source kinds it \
         holds), `flavour` (the compiled persona profile: communication style, \
         stack, workflow, directives), `doctor` (diagnose an empty or stalled \
         memory pipeline). For ingested email, chat and documents use the \
         separate `memory_tree` tool instead."
    }

    fn parameters_schema(&self) -> Value {
        merge_action_schemas(&self.actions())
    }

    fn permission_level(&self) -> PermissionLevel {
        strictest_permission(&self.actions())
    }

    fn permission_level_with_args(&self, args: &Value) -> PermissionLevel {
        // `forget` is destructive and `recall` is read-only; reporting one
        // level for both would either gate every read or let a delete through
        // on a read's clearance. Unknown/missing actions take the strictest.
        let actions = self.actions();
        args.get("action")
            .and_then(Value::as_str)
            .and_then(|action| resolve(&actions, action))
            .map(|entry| entry.tool.permission_level_with_args(args))
            .unwrap_or_else(|| strictest_permission(&actions))
    }

    fn external_effect(&self) -> bool {
        any_external_effect(&self.actions())
    }

    fn supports_markdown(&self) -> bool {
        true
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        self.execute_with_options(args, ToolCallOptions::default())
            .await
    }

    async fn execute_with_options(
        &self,
        args: Value,
        options: ToolCallOptions,
    ) -> anyhow::Result<ToolResult> {
        let actions = self.actions();
        let requested = args.get("action").and_then(Value::as_str);
        let Some(entry) = requested.and_then(|action| resolve(&actions, action)) else {
            return Ok(ToolResult::error(unknown_action_message(
                &actions, requested,
            )));
        };
        tracing::debug!(action = %entry.action, "[tool][memory] dispatch");
        entry
            .tool
            .execute_with_options(args_without_action(&args), options)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool() -> MemoryTool {
        MemoryTool::new(
            Arc::new(Config::default()),
            Arc::new(SecurityPolicy::default()),
        )
    }

    #[test]
    fn every_member_is_hidden_so_the_collapse_actually_saves_something() {
        for entry in tool().actions() {
            assert_eq!(
                entry.tool.exposure(),
                ToolExposure::Hidden,
                "`{}` is still advertised alongside the collapsed `memory` tool",
                entry.tool.name()
            );
        }
    }

    #[test]
    fn the_schema_advertises_every_action() {
        let schema = tool().parameters_schema();
        let listed = schema["properties"]["action"]["enum"]
            .as_array()
            .expect("enum")
            .len();
        assert_eq!(listed, 11);
    }

    #[test]
    fn a_read_is_not_gated_at_the_destructive_level() {
        let tool = tool();
        let recall = serde_json::json!({"action": "recall", "query": "x"});
        assert!(
            tool.permission_level_with_args(&recall) < tool.permission_level(),
            "recall must resolve below the family's strictest level"
        );
    }

    #[test]
    fn forget_is_gated_at_the_familys_strictest_level() {
        // The inverse of the test above, and the one that would catch a
        // dispatch bug quietly downgrading a delete.
        let tool = tool();
        let forget = serde_json::json!({"action": "forget", "id": "m1"});
        assert_eq!(
            tool.permission_level_with_args(&forget),
            tool.forget.permission_level(),
            "forget must resolve to exactly what the member requires"
        );
    }

    #[test]
    fn an_unknown_action_falls_back_to_the_strictest_level() {
        let tool = tool();
        assert_eq!(
            tool.permission_level_with_args(&serde_json::json!({"action": "nope"})),
            tool.permission_level()
        );
    }

    #[tokio::test]
    async fn an_unknown_action_is_an_error_result_naming_the_valid_ones() {
        let result = tool()
            .execute(serde_json::json!({"action": "recal"}))
            .await
            .expect("dispatch does not fail the call");
        assert!(result.is_error);
        let text = format!("{result:?}");
        assert!(text.contains("recal"));
        assert!(text.contains("recall|store|forget"));
    }

    #[test]
    fn the_memory_tree_tool_is_not_a_member() {
        // Pinning the decision in the module docs: `memory_tree` dispatches on
        // its own `mode`, and folding it in would make this two-level.
        assert!(
            !tool().actions().iter().any(|e| e.tool.name() == "memory_tree"),
            "memory_tree stays a separate tool"
        );
    }
}

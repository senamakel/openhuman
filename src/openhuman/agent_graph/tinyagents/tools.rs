//! `tinyagents` [`Tool`] adapter over an openhuman [`Tool`] (issue #4249).
//!
//! Wraps `Arc<dyn openhuman::tools::Tool>` so the harness agent-loop can invoke
//! the exact same tools the legacy loop runs. The harness calls `call` with a
//! validated [`TaToolCall`] (parsed JSON arguments + correlation id); we execute
//! the underlying tool and render the [`ToolResult`] the way the LLM should see
//! it — mirroring [`crate::openhuman::agent_graph::live::HarnessToolExecutor`].

use std::sync::Arc;

use async_trait::async_trait;
use tinyagents::harness::tool::{
    Tool, ToolCall as TaToolCall, ToolResult as TaToolResult, ToolSchema,
};

/// A harness tool backed by an openhuman [`Tool`].
pub struct ToolAdapter {
    inner: Arc<dyn crate::openhuman::tools::Tool>,
}

impl ToolAdapter {
    /// Wrap a resolved openhuman tool.
    pub fn new(inner: Arc<dyn crate::openhuman::tools::Tool>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl Tool<()> for ToolAdapter {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn description(&self) -> &str {
        self.inner.description()
    }

    fn schema(&self) -> ToolSchema {
        super::convert::spec_to_schema(&self.inner.spec())
    }

    async fn call(&self, _state: &(), call: TaToolCall) -> tinyagents::Result<TaToolResult> {
        tracing::debug!(
            tool = %call.name,
            call_id = %call.id,
            "[agent_graph::tinyagents] executing openhuman tool via harness adapter"
        );
        match self.inner.execute(call.arguments.clone()).await {
            Ok(result) => {
                let content = result.output_for_llm(true);
                let error = if result.is_error {
                    Some(content.clone())
                } else {
                    None
                };
                Ok(TaToolResult {
                    call_id: call.id,
                    name: call.name,
                    content,
                    raw: None,
                    error,
                    elapsed_ms: 0,
                })
            }
            Err(e) => {
                tracing::warn!(tool = %call.name, error = %e, "[agent_graph::tinyagents] tool failed");
                Ok(TaToolResult {
                    call_id: call.id,
                    name: call.name.clone(),
                    content: format!("Error executing '{}': {e}", call.name),
                    raw: None,
                    error: Some(e.to_string()),
                    elapsed_ms: 0,
                })
            }
        }
    }
}

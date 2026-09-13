//! [`CliRpcOnlyMiddleware`]: refuse CLI/RPC-only tools inside the autonomous
//! agent loop.

use std::sync::Arc;

use async_trait::async_trait;

use tinyagents_harness::context::RunContext;
use tinyagents_harness::error::Result as TaResult;
use tinyagents_harness::middleware::{MiddlewareToolOutcome, ToolHandler, ToolMiddleware};
use tinyagents_harness::tool::ToolResult as TaToolResult;
use tinyinference::tool::ToolCall as TaToolCall;

use crate::tools::Tool;

/// `wrap_tool`: refuse a tool whose scope is
/// [`ToolScope::CliRpcOnly`](crate::tools::ToolScope) inside the
/// autonomous agent loop (issue #4249). The in-house engine ran this gate in
/// `engine::tools`; the tinyagents path dropped it, so a CLI/RPC-only tool
/// (e.g. phone calls) would execute from the model loop. Applies on every path
/// (channel, session, sub-agent) since the restriction is intrinsic to the tool,
/// not the session — installed unconditionally.
pub(crate) struct CliRpcOnlyMiddleware {
    tool_sets: Vec<Arc<Vec<Box<dyn Tool>>>>,
}

impl CliRpcOnlyMiddleware {
    pub(crate) fn new(tool_sets: Vec<Arc<Vec<Box<dyn Tool>>>>) -> Self {
        Self { tool_sets }
    }

    fn is_cli_rpc_only(&self, name: &str) -> bool {
        self.tool_sets
            .iter()
            .flat_map(|set| set.iter())
            .find(|t| t.name() == name)
            .map(|t| t.scope() == crate::tools::ToolScope::CliRpcOnly)
            .unwrap_or(false)
    }
}

#[async_trait]
impl ToolMiddleware<()> for CliRpcOnlyMiddleware {
    fn name(&self) -> &str {
        "cli_rpc_only"
    }

    async fn wrap_tool(
        &self,
        ctx: &mut RunContext<()>,
        state: &(),
        call: TaToolCall,
        next: ToolHandler<'_, (), ()>,
    ) -> TaResult<MiddlewareToolOutcome> {
        if self.is_cli_rpc_only(&call.name) {
            tracing::warn!(
                tool = call.name.as_str(),
                "[tinyagents::mw] tool scope is CliRpcOnly — denied in agent loop"
            );
            let content = format!(
                "Tool '{}' is only available via explicit CLI/RPC invocation, not in the autonomous agent loop.",
                call.name
            );
            return Ok(MiddlewareToolOutcome::Result(TaToolResult {
                call_id: call.id,
                name: call.name,
                content: content.clone(),
                raw: None,
                error: Some(content),
                elapsed_ms: 0,
            }));
        }
        next.run(ctx, state, call).await
    }
}

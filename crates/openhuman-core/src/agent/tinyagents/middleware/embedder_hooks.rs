//! [`EmbedderToolHooksMiddleware`]: deliver tool lifecycle events to the hooks
//! an embedding host installed.

use async_trait::async_trait;

use tinyagents_harness::context::RunContext;
use tinyagents_harness::error::Result as TaResult;
use tinyagents_harness::middleware::Middleware;
use tinyagents_harness::tool::ToolResult as TaToolResult;
use tinyinference::tool::ToolCall as TaToolCall;

/// Delivers tool lifecycle events to hooks installed by an embedding host.
pub(crate) struct EmbedderToolHooksMiddleware {
    hooks: Vec<std::sync::Arc<dyn crate::agent::hooks::ToolHook>>,
    /// Normalized pre-call arguments keyed by provider `call_id`, so the
    /// `PostToolUse` context can hand an embedding host the same arguments its
    /// `PreToolUse` hook saw. The crate's `ToolResult` does not carry the
    /// original call arguments, so without this cache every post-use event would
    /// report `Null` and an auditing/correlating host could not match inputs to
    /// outcomes.
    pub(super) arguments_by_call_id:
        std::sync::Mutex<std::collections::HashMap<String, serde_json::Value>>,
}

impl EmbedderToolHooksMiddleware {
    pub(crate) fn new(hooks: Vec<std::sync::Arc<dyn crate::agent::hooks::ToolHook>>) -> Self {
        Self {
            hooks,
            arguments_by_call_id: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }
}

#[async_trait]
impl Middleware<()> for EmbedderToolHooksMiddleware {
    fn name(&self) -> &str {
        "embedder_tool_hooks"
    }

    async fn before_tool(
        &self,
        _ctx: &mut RunContext<()>,
        _state: &(),
        call: &mut TaToolCall,
    ) -> TaResult<()> {
        let mut context = crate::agent::hooks::ToolHookContext {
            event: crate::agent::hooks::ToolHookEvent::PreToolUse,
            call_id: call.id.clone(),
            tool_name: call.name.clone(),
            arguments: call.arguments.clone(),
            success: None,
            duration_ms: None,
            output: None,
            error: None,
            session_id: None,
            agent_id: None,
        };
        for hook in &self.hooks {
            match hook.before_tool_decision(&context).await {
                crate::agent::hooks::ToolHookDecision::Proceed => {}
                // A rewrite is applied to the live call *and* to the context
                // handed to later hooks, so a chain of hooks each narrowing the
                // arguments composes instead of the last one silently winning.
                crate::agent::hooks::ToolHookDecision::ProceedWith(arguments) => {
                    tracing::debug!(
                        hook = hook.name(),
                        tool = context.tool_name,
                        "[tinyagents::mw] tool hook rewrote call arguments"
                    );
                    call.arguments = arguments.clone();
                    context.arguments = arguments;
                }
                crate::agent::hooks::ToolHookDecision::Deny(reason) => {
                    return Err(tinyagents_harness::error::TinyAgentsError::Tool(format!(
                        "tool hook '{}' denied {}: {reason}",
                        hook.name(),
                        context.tool_name
                    )));
                }
                // There is no approval channel inside a middleware, and a hook
                // that asks for a human is asking for something stricter than
                // "proceed" — so an unresolvable `Ask` denies rather than
                // quietly allowing. The approval-gate path in
                // `security::approval` is where an interactive host resolves it.
                crate::agent::hooks::ToolHookDecision::Ask(reason) => {
                    tracing::info!(
                        hook = hook.name(),
                        tool = context.tool_name,
                        "[tinyagents::mw] tool hook requested approval; denying in a \
                         non-interactive middleware"
                    );
                    return Err(tinyagents_harness::error::TinyAgentsError::Tool(format!(
                        "tool hook '{}' requires approval for {}: {reason}",
                        hook.name(),
                        context.tool_name
                    )));
                }
            }
        }
        // Cache the (already-recovered) arguments only once every hook approved
        // the call: a vetoed call never reaches `after_tool`, so storing it here
        // would leak a cache entry for the turn.
        self.arguments_by_call_id
            .lock()
            .expect("embedder tool-hook arguments poisoned")
            .insert(call.id.clone(), call.arguments.clone());
        Ok(())
    }

    async fn after_tool(
        &self,
        _ctx: &mut RunContext<()>,
        _state: &(),
        result: &mut TaToolResult,
    ) -> TaResult<()> {
        let arguments = self
            .arguments_by_call_id
            .lock()
            .expect("embedder tool-hook arguments poisoned")
            .remove(&result.call_id)
            .unwrap_or(serde_json::Value::Null);
        let context = crate::agent::hooks::ToolHookContext {
            event: crate::agent::hooks::ToolHookEvent::PostToolUse,
            call_id: result.call_id.clone(),
            tool_name: result.name.clone(),
            arguments,
            success: Some(result.error.is_none()),
            duration_ms: Some(result.elapsed_ms),
            output: Some(result.content.clone()),
            error: result.error.clone(),
            session_id: None,
            agent_id: None,
        };
        for hook in &self.hooks {
            // Text a hook returns is appended to the result the model reads —
            // the seam a "you edited a file, here is the linter output" hook
            // needs. It is appended rather than substituted so a hook cannot
            // erase what the tool actually said.
            if let Some(additional) = hook.after_tool_context(&context).await {
                if !additional.trim().is_empty() {
                    tracing::debug!(
                        hook = hook.name(),
                        tool = context.tool_name,
                        chars = additional.chars().count(),
                        "[tinyagents::mw] tool hook appended context to the result"
                    );
                    result.content.push_str("\n\n");
                    result.content.push_str(additional.trim_end());
                }
            }
        }
        Ok(())
    }
}

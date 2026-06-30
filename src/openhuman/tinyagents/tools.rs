//! `tinyagents` [`Tool`] adapter over an openhuman [`Tool`] (issue #4249).
//!
//! Wraps `Arc<dyn openhuman::tools::Tool>` so the harness agent-loop can invoke
//! the exact same tools the legacy loop runs. The harness calls `call` with a
//! validated [`TaToolCall`] (parsed JSON arguments + correlation id); we execute
//! the underlying tool and render the [`ToolResult`] the way the LLM should see
//! it (rendered via `output_for_llm`, matching the legacy tool loop).

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tinyagents::harness::steering::{SteeringCommand, SteeringHandle};
use tinyagents::harness::tool::{
    Tool, ToolCall as TaToolCall, ToolResult as TaToolResult, ToolSchema,
};

/// A captured early-exit: a sub-agent invoked an early-exit tool (e.g.
/// `ask_user_clarification`), so the loop should pause and surface `question`
/// to the user. Mirrors the legacy `run_turn_engine` `early_exit_tool` seam.
#[derive(Debug, Clone)]
pub struct EarlyExit {
    pub tool: String,
    pub question: String,
}

/// Shared early-exit hook handed to the adapters for the early-exit tool names.
/// On a successful call to one of those tools it records the [`EarlyExit`] and
/// sends a [`SteeringCommand::Pause`] so the harness loop short-circuits at the
/// next checkpoint (before the next model call) — the tinyagents analogue of the
/// legacy loop's "break on early-exit tool" behavior.
#[derive(Clone)]
pub struct EarlyExitHook {
    handle: SteeringHandle,
    slot: Arc<Mutex<Option<EarlyExit>>>,
}

impl EarlyExitHook {
    /// Build a hook that pauses `handle` and records into a fresh slot.
    pub fn new(handle: SteeringHandle) -> Self {
        Self {
            handle,
            slot: Arc::new(Mutex::new(None)),
        }
    }

    /// The captured early-exit, if one fired during the run.
    pub fn take(&self) -> Option<EarlyExit> {
        self.slot.lock().unwrap().take()
    }

    /// Record an early-exit and request a cooperative pause. Only the first
    /// early-exit in a run is kept (matching the legacy "halt on first").
    fn trigger(&self, tool: &str, question: String) {
        {
            let mut slot = self.slot.lock().unwrap();
            if slot.is_none() {
                *slot = Some(EarlyExit {
                    tool: tool.to_string(),
                    question,
                });
            }
        }
        tracing::info!(tool, "[tinyagents] early-exit tool — requesting pause");
        self.handle.send(SteeringCommand::Pause);
    }
}

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
        Ok(execute_openhuman_tool(self.inner.as_ref(), call).await)
    }
}

/// Execute an openhuman [`Tool`](crate::openhuman::tools::Tool) for a harness
/// [`TaToolCall`] and render the [`TaToolResult`] the way the LLM should see it
/// (mirrors the live-path `HarnessToolExecutor`).
async fn execute_openhuman_tool(
    tool: &dyn crate::openhuman::tools::Tool,
    call: TaToolCall,
) -> TaToolResult {
    tracing::debug!(
        tool = %call.name,
        call_id = %call.id,
        "[tinyagents] executing openhuman tool via harness adapter"
    );
    match tool.execute(call.arguments.clone()).await {
        Ok(result) => {
            let content = result.output_for_llm(true);
            let error = if result.is_error {
                Some(content.clone())
            } else {
                None
            };
            TaToolResult {
                call_id: call.id,
                name: call.name,
                content,
                raw: None,
                error,
                elapsed_ms: 0,
            }
        }
        Err(e) => {
            tracing::warn!(tool = %call.name, error = %e, "[tinyagents] tool failed");
            TaToolResult {
                call_id: call.id,
                name: call.name.clone(),
                content: format!("Error executing '{}': {e}", call.name),
                raw: None,
                error: Some(e.to_string()),
                elapsed_ms: 0,
            }
        }
    }
}

/// A harness tool backed by the routes' shared, `Arc`-owned tool registry sets
/// (`Arc<Vec<Box<dyn Tool>>>`). One adapter is registered per advertised tool
/// name; on call it locates the named tool across the shared sets and executes
/// it — the tinyagents analogue of the live path's `SharedToolExecutor`, which
/// lets a route reuse the same `Arc`-shared tools the legacy loop runs without
/// cloning them.
pub struct SharedToolAdapter {
    sets: Vec<Arc<Vec<Box<dyn crate::openhuman::tools::Tool>>>>,
    name: String,
    description: String,
    schema: ToolSchema,
    /// When set, a successful call records an [`EarlyExit`] and pauses the loop.
    early_exit: Option<EarlyExitHook>,
}

impl SharedToolAdapter {
    /// Build an adapter for the tool named `name`, locating it across `sets` to
    /// capture its advertised spec. Returns `None` when no set contains it.
    pub fn for_name(
        sets: Vec<Arc<Vec<Box<dyn crate::openhuman::tools::Tool>>>>,
        name: &str,
    ) -> Option<Self> {
        let spec = sets
            .iter()
            .flat_map(|set| set.iter())
            .find(|t| t.name() == name)
            .map(|t| t.spec())?;
        Some(Self {
            sets,
            name: spec.name.clone(),
            description: spec.description.clone(),
            schema: super::convert::spec_to_schema(&spec),
            early_exit: None,
        })
    }

    /// Treat this tool as an early-exit tool: a successful call records the
    /// question and pauses the run via `hook`.
    pub fn with_early_exit(mut self, hook: EarlyExitHook) -> Self {
        self.early_exit = Some(hook);
        self
    }
}

#[async_trait]
impl Tool<()> for SharedToolAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn schema(&self) -> ToolSchema {
        self.schema.clone()
    }

    async fn call(&self, _state: &(), call: TaToolCall) -> tinyagents::Result<TaToolResult> {
        let found = self
            .sets
            .iter()
            .flat_map(|set| set.iter())
            .find(|t| t.name() == self.name);
        match found {
            Some(tool) => {
                let result = execute_openhuman_tool(tool.as_ref(), call).await;
                // Early-exit (e.g. `ask_user_clarification`): on a successful
                // call, record the question and pause so the runner can
                // checkpoint and surface the prompt — matching the legacy seam.
                if let Some(hook) = &self.early_exit {
                    if result.error.is_none() {
                        hook.trigger(&self.name, result.content.clone());
                    }
                }
                Ok(result)
            }
            None => {
                tracing::warn!(tool = %self.name, "[tinyagents] shared tool not found");
                Ok(TaToolResult {
                    call_id: call.id,
                    name: call.name,
                    content: format!("Error: unknown tool '{}'", self.name),
                    raw: None,
                    error: Some("unknown tool".to_string()),
                    elapsed_ms: 0,
                })
            }
        }
    }
}

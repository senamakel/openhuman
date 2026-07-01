//! openhuman context concerns expressed as tinyagents graph middlewares
//! (issue #4249).
//!
//! Historically these ran in the in-house engine's tool/prompt plumbing
//! (`agent_tool_exec`, `ContextManager`). The tinyagents turn path bypassed
//! them, so they were effectively dead on the live loop. Re-expressing them as
//! [`Middleware`] hooks restores the behaviour and makes the graph the single
//! place cross-cutting context concerns live:
//!
//! - [`CacheAlignMiddleware`] (`before_model`) — warn on volatile tokens in the
//!   system prompt that would bust the provider KV-cache prefix. Warn-only.
//! - [`MicrocompactMiddleware`] (`before_model`) — clear the bodies of older
//!   tool-result messages (keeping the N most recent) so a long tool-heavy
//!   thread stays cheap without dropping chat history.
//! - [`ToolOutputMiddleware`] (`after_tool`) — apply the per-tool-result byte
//!   cap and (optionally) the semantic payload summarizer to each tool result
//!   as it returns, before it enters the transcript.
//!
//! [`TurnContextMiddleware`] bundles the config and installs whichever hooks are
//! enabled onto a harness.

use std::sync::Arc;

use async_trait::async_trait;

use tinyagents::error::Result as TaResult;
use tinyagents::harness::context::RunContext;
use tinyagents::harness::message::Message as TaMessage;
use tinyagents::harness::middleware::{
    Middleware, MiddlewareToolOutcome, ToolHandler, ToolMiddleware,
};
use tinyagents::harness::model::ModelRequest;
use tinyagents::harness::runtime::AgentHarness;
use tinyagents::harness::tool::{ToolCall as TaToolCall, ToolResult as TaToolResult};

use crate::openhuman::agent::harness::payload_summarizer::PayloadSummarizer;
use crate::openhuman::approval::{
    redact_args, summarize_action, ApprovalGate, ExecutionOutcome, GateOutcome,
};
use crate::openhuman::context::tool_result_budget::apply_tool_result_budget;
use crate::openhuman::context::CLEARED_PLACEHOLDER;
use crate::openhuman::tools::Tool;

/// Default per-tool-result byte cap for the channel / sub-agent paths, which do
/// not carry a session `ContextManager` to source the configured budget from.
/// Mirrors the `ContextConfig::tool_result_budget_bytes` default (16 KiB).
pub const DEFAULT_TOOL_RESULT_BUDGET_BYTES: usize = 16 * 1024;

/// Config bundle for the openhuman context middlewares installed on a turn.
///
/// Cheap to clone (the summarizer is an `Arc`). An all-default value installs
/// nothing — [`install`](Self::install) is a no-op.
#[derive(Clone, Default)]
pub struct TurnContextMiddleware {
    /// Per-tool-result byte cap. `0` disables the cap.
    pub tool_result_budget_bytes: usize,
    /// Optional semantic tool-output summarizer (progressive disclosure).
    pub payload_summarizer: Option<Arc<dyn PayloadSummarizer>>,
    /// Warn on volatile tokens in the system prompt (KV-cache diagnostic).
    pub cache_align: bool,
    /// Keep-recent count for microcompact tool-body clearing. `0` disables it.
    pub microcompact_keep_recent: usize,
}

impl TurnContextMiddleware {
    /// A sensible default for turn paths without a session `ContextManager`
    /// (channel / sub-agent): cache-align warnings on and the default tool-result
    /// byte cap, no summarizer or microcompact.
    pub fn defaults() -> Self {
        Self {
            tool_result_budget_bytes: DEFAULT_TOOL_RESULT_BUDGET_BYTES,
            payload_summarizer: None,
            cache_align: true,
            microcompact_keep_recent: 0,
        }
    }

    /// `true` when no middleware would be installed.
    pub fn is_empty(&self) -> bool {
        self.tool_result_budget_bytes == 0
            && self.payload_summarizer.is_none()
            && !self.cache_align
            && self.microcompact_keep_recent == 0
    }

    /// Push the enabled middlewares onto `harness`.
    ///
    /// `before_model` hooks run in registration order, so cache-align (warn) and
    /// microcompact (clear tool bodies) are installed **before** the caller's
    /// summarization / trim middlewares — microcompact frees cheap tokens first,
    /// then summarization/trim handle the rest.
    pub fn install(self, harness: &mut AgentHarness<()>) {
        if self.cache_align {
            harness.push_middleware(Arc::new(CacheAlignMiddleware));
        }
        if self.microcompact_keep_recent > 0 {
            harness.push_middleware(Arc::new(MicrocompactMiddleware {
                keep_recent: self.microcompact_keep_recent,
            }));
        }
        if self.tool_result_budget_bytes > 0 || self.payload_summarizer.is_some() {
            harness.push_middleware(Arc::new(ToolOutputMiddleware {
                budget_bytes: self.tool_result_budget_bytes,
                payload_summarizer: self.payload_summarizer,
            }));
        }
    }
}

/// `before_model`: flag volatile tokens (UUIDs, timestamps, JWTs, …) in the
/// system prompt that silently break the provider KV-cache prefix. Warn-only —
/// never mutates the request. The graph analogue of the former
/// `ContextManager::warn_if_cache_unstable`.
struct CacheAlignMiddleware;

#[async_trait]
impl Middleware<()> for CacheAlignMiddleware {
    fn name(&self) -> &str {
        "cache_align"
    }

    async fn before_model(
        &self,
        _ctx: &mut RunContext<()>,
        _state: &(),
        request: &mut ModelRequest,
    ) -> TaResult<()> {
        if let Some(sys) = request
            .messages
            .iter()
            .find(|m| matches!(m, TaMessage::System(_)))
        {
            crate::openhuman::agent::harness::compaction::cache_align::warn_if_volatile(&sys.text());
        }
        Ok(())
    }
}

/// `before_model`: clear the bodies of older tool-result messages, keeping the
/// `keep_recent` most recent verbatim. The graph analogue of
/// `context::microcompact` — bounds a tool-heavy thread's cost without dropping
/// any chat turns. Idempotent: an already-cleared body is left as the
/// placeholder.
struct MicrocompactMiddleware {
    keep_recent: usize,
}

#[async_trait]
impl Middleware<()> for MicrocompactMiddleware {
    fn name(&self) -> &str {
        "microcompact"
    }

    async fn before_model(
        &self,
        _ctx: &mut RunContext<()>,
        _state: &(),
        request: &mut ModelRequest,
    ) -> TaResult<()> {
        let tool_idxs: Vec<usize> = request
            .messages
            .iter()
            .enumerate()
            .filter(|(_, m)| matches!(m, TaMessage::Tool(_)))
            .map(|(i, _)| i)
            .collect();
        if tool_idxs.len() <= self.keep_recent {
            return Ok(());
        }
        let cut = tool_idxs.len() - self.keep_recent;
        for &i in &tool_idxs[..cut] {
            // Skip messages already reduced to the placeholder; otherwise swap the
            // body for it (idempotent, preserves the tool_call_id).
            if request.messages[i].text() == CLEARED_PLACEHOLDER {
                continue;
            }
            if let TaMessage::Tool(t) = &request.messages[i] {
                let id = t.tool_call_id.clone();
                request.messages[i] = TaMessage::tool(id, CLEARED_PLACEHOLDER);
            }
        }
        Ok(())
    }
}

/// `after_tool`: apply the semantic payload summarizer (when configured) and
/// then the hard per-tool-result byte cap to each tool result's model-facing
/// content, before it enters the transcript. The graph analogue of the byte cap
/// + `payload_summarizer` interception the in-house `agent_tool_exec` ran.
struct ToolOutputMiddleware {
    budget_bytes: usize,
    payload_summarizer: Option<Arc<dyn PayloadSummarizer>>,
}

#[async_trait]
impl Middleware<()> for ToolOutputMiddleware {
    fn name(&self) -> &str {
        "tool_output_budget"
    }

    async fn after_tool(
        &self,
        _ctx: &mut RunContext<()>,
        _state: &(),
        result: &mut TaToolResult,
    ) -> TaResult<()> {
        // 1. Semantic summarization (progressive disclosure) — swap the raw
        //    payload for a compressed summary when the summarizer opts in.
        //    Failures never break the tool call (the trait swallows them).
        if let Some(ps) = &self.payload_summarizer {
            if let Ok(Some(payload)) = ps
                .maybe_summarize(&result.name, None, &result.content)
                .await
            {
                tracing::info!(
                    tool = %result.name,
                    from_bytes = payload.original_bytes,
                    to_bytes = payload.summary_bytes,
                    "[tinyagents::mw] payload_summarizer compressed tool output"
                );
                result.content = payload.summary;
            }
        }

        // 2. Hard byte cap backstop — truncate at a UTF-8 boundary with a marker.
        if self.budget_bytes > 0 {
            let (capped, outcome) =
                apply_tool_result_budget(std::mem::take(&mut result.content), self.budget_bytes);
            if outcome.truncated {
                tracing::debug!(
                    tool = %result.name,
                    from_bytes = outcome.original_bytes,
                    to_bytes = outcome.final_bytes,
                    "[tinyagents::mw] tool_result_budget truncated tool output"
                );
            }
            result.content = capped;
        }
        Ok(())
    }
}

/// `wrap_tool`: route OpenHuman's human-in-the-loop **approval gate** through a
/// named tinyagents tool middleware (issue #4249, Phase 1). A tool with an
/// external effect intercepts through the global [`ApprovalGate`]; a denial
/// short-circuits with the reason as a model-consumable [`TaToolResult`]
/// (`next` is never called), and an allowed call records a terminal audit row
/// once the tool resolves.
///
/// This replaces the inline approval block that used to live in
/// `execute_openhuman_tool`, giving approval a stable middleware name and
/// letting it short-circuit cleanly. Tool-*internal* security (path/command
/// policy via `live_policy`) stays inside each tool — it needs tool-specific
/// operation semantics the harness boundary can't reconstruct generically.
pub struct ApprovalSecurityMiddleware {
    /// The same `Arc`-shared tool sets the runner registers, used to resolve a
    /// call's OpenHuman `Tool` by name so `external_effect_with_args` can gate.
    tool_sets: Vec<Arc<Vec<Box<dyn Tool>>>>,
}

impl ApprovalSecurityMiddleware {
    /// Build the middleware over the runner's shared tool sets.
    pub fn new(tool_sets: Vec<Arc<Vec<Box<dyn Tool>>>>) -> Self {
        Self { tool_sets }
    }

    /// Whether the named tool declares an external effect for these args.
    fn has_external_effect(&self, name: &str, args: &serde_json::Value) -> bool {
        self.tool_sets
            .iter()
            .flat_map(|set| set.iter())
            .find(|t| t.name() == name)
            .map(|t| t.external_effect_with_args(args))
            .unwrap_or(false)
    }
}

#[async_trait]
impl ToolMiddleware<()> for ApprovalSecurityMiddleware {
    fn name(&self) -> &str {
        "approval_security"
    }

    async fn wrap_tool(
        &self,
        ctx: &mut RunContext<()>,
        state: &(),
        call: TaToolCall,
        next: ToolHandler<'_, (), ()>,
    ) -> TaResult<MiddlewareToolOutcome> {
        // Resolve external-effect up front so no tool borrow is held across the
        // approval await.
        let mut audit_id: Option<String> = None;
        if self.has_external_effect(&call.name, &call.arguments) {
            if let Some(gate) = ApprovalGate::try_global() {
                let summary = summarize_action(&call.name, &call.arguments);
                let redacted = redact_args(&call.arguments);
                let (outcome, request_id) =
                    gate.intercept_audited(&call.name, &summary, redacted).await;
                match outcome {
                    GateOutcome::Deny { reason } => {
                        tracing::warn!(
                            tool = %call.name,
                            reason = %reason,
                            "[tinyagents::mw] approval gate denied tool call"
                        );
                        return Ok(MiddlewareToolOutcome::Result(TaToolResult {
                            call_id: call.id,
                            name: call.name,
                            content: reason.clone(),
                            raw: None,
                            error: Some(reason),
                            elapsed_ms: 0,
                        }));
                    }
                    GateOutcome::Allow => audit_id = request_id,
                }
            }
        }

        let outcome = next.run(ctx, state, call).await?;

        // Record the terminal audit row for an approved external-effect call
        // (idempotent; a no-op when the id is unknown).
        if let Some(id) = audit_id {
            if let Some(gate) = ApprovalGate::try_global() {
                if let MiddlewareToolOutcome::Result(res) = &outcome {
                    let exec = if res.error.is_some() {
                        ExecutionOutcome::Failure
                    } else {
                        ExecutionOutcome::Success
                    };
                    gate.record_execution(&id, exec, res.error.as_deref());
                }
            }
        }
        Ok(outcome)
    }
}

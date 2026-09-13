//! [`TurnContextMiddleware`]: the per-turn config bundle that installs the
//! context middlewares, plus the small observation/handoff hooks it owns
//! (transcript snapshot, progressive-disclosure handoff).

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

use tinyagents_harness::context::RunContext;
use tinyagents_harness::error::Result as TaResult;
use tinyagents_harness::middleware::Middleware;
use tinyagents_harness::runtime::AgentHarness;
use tinyagents_harness::tool::{ToolPolicy as TaToolPolicy, ToolResult as TaToolResult};
use tinyinference::model::ModelRequest;

use crate::agent::harness::tool_result_artifacts::ToolResultArtifactStore;
use crate::agent::tinyagents::payload_summarizer::PayloadSummarizer;
use crate::inference::tokenjuice::AgentTokenjuiceCompression;

use super::tool_output::ToolOutputMiddleware;

/// Default per-tool-result byte cap for the channel / sub-agent paths, which do
/// not carry a session `ContextManager` to source the configured budget from.
/// Mirrors the `ContextConfig::tool_result_budget_bytes` default (16 KiB).
pub(crate) const DEFAULT_TOOL_RESULT_BUDGET_BYTES: usize = 16 * 1024;

/// Config bundle for the openhuman context middlewares installed on a turn.
///
/// Cheap to clone (the summarizer is an `Arc`). An all-default value installs
/// nothing — [`install`](Self::install) is a no-op.
#[derive(Clone, Default)]
pub(crate) struct TurnContextMiddleware {
    /// Per-tool-result byte cap. `0` disables the cap.
    pub(crate) tool_result_budget_bytes: usize,
    /// Optional semantic tool-output summarizer (progressive disclosure).
    pub(crate) payload_summarizer: Option<Arc<dyn PayloadSummarizer>>,
    /// Optional action-workspace artifact sink for oversized tool results.
    pub(crate) artifact_store: Option<ToolResultArtifactStore>,
    /// Whether TokenJuice content-aware compaction runs before output caps.
    pub(crate) tokenjuice_compaction_enabled: bool,
    /// Agent-level TokenJuice profile for tool-result compaction.
    pub(crate) tokenjuice_compression: AgentTokenjuiceCompression,
    /// Keep-recent count for microcompact tool-body clearing. `0` disables it.
    pub(crate) microcompact_keep_recent: usize,
    /// Whether the LLM summarization step (`ContextCompressionMiddleware`) may be
    /// installed on this turn. `false` when `[context].enabled` or
    /// `autocompact_enabled` is off, so a diagnostic/test opt-out doesn't spend
    /// summarizer tokens or rewrite history. The deterministic hard-trim backstop
    /// still installs regardless. Defaults to `true` (see [`defaults`](Self::defaults)).
    pub(crate) autocompact_enabled: bool,
    /// Progressive-disclosure handoff: when set (integrations_agent with a
    /// resolved toolkit), oversized tool results are stashed in the shared
    /// [`ResultHandoffCache`] and replaced with an `extract_from_result` drill-in
    /// placeholder. `None` everywhere else.
    pub(crate) handoff: Option<HandoffConfig>,
    /// Live transcript snapshot sink (#4466). When set, a
    /// [`TranscriptSnapshotMiddleware`] mirrors the running conversation (as
    /// openhuman [`ChatMessage`]s) into this shared buffer before every model
    /// call. Only the sub-agent path sets it, so an erroring run can persist the
    /// rounds completed before the failure (the harness drops its partial
    /// transcript on `Err`). `None` everywhere else (chat persists post-run).
    pub(crate) transcript_snapshot: Option<TranscriptSnapshotSink>,
}

/// Shared buffer a [`TranscriptSnapshotMiddleware`] mirrors the live sub-agent
/// conversation into, so the caller can persist completed rounds even when the
/// harness run ends in `Err` (#4466).
pub(crate) type TranscriptSnapshotSink =
    Arc<std::sync::Mutex<Vec<crate::agent::messages::ChatMessage>>>;

/// Observation-only middleware that snapshots the running transcript into a
/// shared [`TranscriptSnapshotSink`] before each model call (#4466).
///
/// The tinyagents harness owns the working message vector and only hands it back
/// inside a successful `AgentRun`; on a mid-run error it is dropped. The
/// sub-agent runner persists a per-child `session_raw` transcript so
/// `learning/transcript_ingest` can read it — but a failed run used to persist
/// nothing. This middleware mirrors each `before_model` request's messages
/// (which include every prior completed assistant/tool round) into an
/// openhuman-owned buffer, so the runner's error path can still write the rounds
/// that completed before the failure. Converts to [`ChatMessage`] eagerly so the
/// caller does not need access to the private `convert` module.
pub(crate) struct TranscriptSnapshotMiddleware {
    sink: TranscriptSnapshotSink,
}

#[async_trait]
impl Middleware<()> for TranscriptSnapshotMiddleware {
    fn name(&self) -> &str {
        "openhuman.transcript_snapshot"
    }

    async fn before_model(
        &self,
        _ctx: &mut RunContext<()>,
        _state: &(),
        request: &mut ModelRequest,
    ) -> TaResult<()> {
        let history = crate::agent::message_convert::messages_to_history(&request.messages);
        if let Ok(mut guard) = self.sink.lock() {
            *guard = history;
        }
        Ok(())
    }
}

/// Config for the [`HandoffMiddleware`]: the per-spawn cache (shared with the
/// `extract_from_result` tool) plus the ids used in handoff log lines.
#[derive(Clone)]
pub(crate) struct HandoffConfig {
    pub(crate) cache: Arc<crate::agent::harness::subagent_runner::ResultHandoffCache>,
    pub(crate) agent_id: String,
    pub(crate) task_id: String,
}

impl TurnContextMiddleware {
    /// A sensible default for turn paths without a session `ContextManager`
    /// (channel / sub-agent): the default tool-result byte cap, no summarizer or
    /// microcompact.
    pub(crate) fn defaults() -> Self {
        Self {
            tool_result_budget_bytes: DEFAULT_TOOL_RESULT_BUDGET_BYTES,
            payload_summarizer: None,
            artifact_store: None,
            tokenjuice_compaction_enabled: false,
            tokenjuice_compression: AgentTokenjuiceCompression::Off,
            microcompact_keep_recent: 0,
            autocompact_enabled: true,
            handoff: None,
            transcript_snapshot: None,
        }
    }

    /// `true` when no middleware would be installed.
    pub(crate) fn is_empty(&self) -> bool {
        self.tool_result_budget_bytes == 0
            && self.payload_summarizer.is_none()
            && !self.tokenjuice_compaction_enabled
            && self.microcompact_keep_recent == 0
            && self.handoff.is_none()
            && self.transcript_snapshot.is_none()
    }

    /// Push the enabled middlewares onto `harness`.
    ///
    /// `before_model` hooks run in registration order, so microcompact (clear
    /// tool bodies) is installed **before** the caller's summarization / trim
    /// middlewares — microcompact frees cheap tokens first, then
    /// summarization/trim handle the rest.
    pub(crate) fn install(
        self,
        harness: &mut AgentHarness<()>,
        tool_policies: HashMap<String, TaToolPolicy>,
    ) {
        // Transcript snapshot (#4466) runs first among before_model hooks so it
        // mirrors the *incoming* request transcript (every prior completed round)
        // before microcompact/summarization rewrite it — the caller's error path
        // persists exactly what the model was about to see.
        if let Some(sink) = self.transcript_snapshot {
            harness.push_middleware(Arc::new(TranscriptSnapshotMiddleware { sink }));
        }
        // Microcompact is NOT registered here any more (issue #6014). It used to
        // be, which put its `before_model` ahead of the summarization step the
        // caller installs later — so by the time the task-aware summarizer ran,
        // every tool body past `keep_recent` had already been replaced with
        // `CLEARED_PLACEHOLDER` and it was summarizing placeholders. The one
        // component able to preserve those results in condensed form never saw
        // them. The caller now sites it AFTER compression, so the ladder reads
        // summarize → blank → evict. See `assemble_turn_harness`.
        // REVERSE-ORDER RULE (issue #4464): the crate runs `after_tool` hooks in
        // REVERSE registration order (`MiddlewareStack::run_after_tool` iterates
        // `self.middlewares.iter().rev()`, tinyagents src/harness/middleware/mod.rs).
        // So the LAST-pushed middleware's `after_tool` runs FIRST. To make the
        // effective `after_tool` chain be handoff(raw) → tool-output budget/caps,
        // the handoff MUST be pushed AFTER the tool-output budget.
        //
        // Push the tool-output budget FIRST (so its `after_tool` runs SECOND):
        // it truncates the oversized payload to the 16 KiB byte cap.
        if self.tool_result_budget_bytes > 0
            || self.payload_summarizer.is_some()
            || self.tokenjuice_compaction_enabled
        {
            harness.push_middleware(Arc::new(ToolOutputMiddleware {
                budget_bytes: self.tool_result_budget_bytes,
                payload_summarizer: self.payload_summarizer,
                artifact_store: self.artifact_store,
                tokenjuice_compaction_enabled: self.tokenjuice_compaction_enabled,
                tokenjuice_compression: self.tokenjuice_compression,
                tool_policies,
            }));
        }
        // Push the handoff LAST (so its `after_tool` runs FIRST): it observes the
        // RAW, uncapped payload, stashes an oversized result into the
        // `ResultHandoffCache`, and swaps in a short pointer BEFORE the tool-output
        // budget can shrink it below the 50k-token handoff threshold and defeat the
        // drill-in.
        if let Some(handoff) = self.handoff {
            harness.push_middleware(Arc::new(HandoffMiddleware {
                cache: handoff.cache,
                agent_id: handoff.agent_id,
                task_id: handoff.task_id,
            }));
        }
    }
}

/// `after_tool`: progressive-disclosure handoff (issue #4249 1b). An oversized
/// sub-agent tool result is stashed in the shared [`ResultHandoffCache`] and its
/// content replaced with a short placeholder naming a `result_id` the model can
/// drill into via `extract_from_result`. Restores the seam the legacy
/// `SubagentToolSource` ran on every tool result (via `apply_handoff`), which the
/// agent_graph rewrite dropped. Errors and `extract_from_result`'s own output
/// pass through unchanged (handled inside `apply_handoff`).
pub(crate) struct HandoffMiddleware {
    cache: Arc<crate::agent::harness::subagent_runner::ResultHandoffCache>,
    agent_id: String,
    task_id: String,
}

#[async_trait]
impl Middleware<()> for HandoffMiddleware {
    fn name(&self) -> &str {
        "result_handoff"
    }

    async fn after_tool(
        &self,
        _ctx: &mut RunContext<()>,
        _state: &(),
        result: &mut TaToolResult,
    ) -> TaResult<()> {
        result.content = crate::agent::harness::subagent_runner::apply_handoff(
            &self.cache,
            &result.name,
            &self.task_id,
            &self.agent_id,
            std::mem::take(&mut result.content),
        );
        Ok(())
    }
}

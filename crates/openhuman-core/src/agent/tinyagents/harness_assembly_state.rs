//! Shared harness state and handles consumed after a turn.

use super::*;
use tinyagents_harness::steering::SteeringHandle;
use tinyagents_registry::{RegistryDiagnostic, RegistrySnapshot};

/// Everything [`assemble_turn_harness`] wires up for one turn: the configured
/// harness plus the shared slots/handles the run loop reads after the drive
/// future returns.
pub(in crate::agent::tinyagents) struct AssembledTurnHarness {
    /// The fully assembled harness: model, tools, and middleware registered in
    /// the intended order.
    pub(in crate::agent::tinyagents) harness: AgentHarness<(), OpenHumanRunContext>,
    /// Shared 1-based model-call cursor (event bridge advances, model adapter
    /// reads for out-of-band thinking attribution).
    pub(in crate::agent::tinyagents) cursor: IterationCursor,
    /// Shared `call_id → tool_name` map used by the event bridge to label
    /// tool-argument fragments projected off the crate stream.
    pub(in crate::agent::tinyagents) tool_names: ToolNameMap,
    /// Shared `call_id → (success, failure, elapsed_ms, output_chars)` side-channel:
    /// middleware records each outcome for the event bridge's `ToolCallCompleted`.
    pub(in crate::agent::tinyagents) failure_map: ToolFailureMap,
    /// Shared FIFO carry of per-call provider `BilledUsage`; the event bridge
    /// reads it when recording usage to preserve charged-USD precedence (#4467).
    pub(in crate::agent::tinyagents) provider_usage_carry: ProviderUsageCarry,
    /// Recovers the original (downcastable) provider error on run failure.
    pub(in crate::agent::tinyagents) error_slot: crate::agent::tinyagents::model::ModelErrorSlot,
    /// Root-cause summary recorded by the repeated-tool-failure breaker.
    pub(in crate::agent::tinyagents) halt_summary: HaltSummarySlot,
    /// Per-call tool success/content capture for honest `ToolCallRecord`s.
    pub(in crate::agent::tinyagents) tool_outcome_sink: ToolOutcomeSink,
    /// The shared steering handle (mid-flight steer, early-exit, cap, stop-hook
    /// pauses).
    pub(in crate::agent::tinyagents) handle: Option<SteeringHandle>,
    /// Records the first early-exit tool round, when early-exit tools exist.
    pub(in crate::agent::tinyagents) early_exit_hook: Option<EarlyExitHook>,
    /// Set by [`FinalCallWrapUpMiddleware`] when it turned the last permitted
    /// model call into the turn's conclusion (issue #6014). `None` when the
    /// middleware is not installed (a run that does not pause at its cap).
    ///
    /// A flag rather than an inference off the run, because this turn now ends
    /// the way a finished one does — the model returns text and requests no
    /// tools — so `final_response.is_none()` no longer tells the two apart.
    pub(in crate::agent::tinyagents) wrap_up_fired:
        Option<Arc<tinyagents_harness::middleware::FinalCallWrapUpMiddleware>>,
    /// Number of callable tools registered.
    pub(in crate::agent::tinyagents) tool_count: usize,
    /// TinyAgents named-capability projection for this turn. The live run still
    /// uses the harness registries above; this snapshot makes the projected
    /// model/tool/graph inventory inspectable without changing dispatch.
    pub(in crate::agent::tinyagents) registry_snapshot: RegistrySnapshot,
    /// Health diagnostics from the projected registry.
    pub(in crate::agent::tinyagents) registry_diagnostics: Vec<RegistryDiagnostic>,
    /// TinyAgents store index for OpenHuman action-dir tool-result artifacts.
    pub(in crate::agent::tinyagents) tool_result_artifact_index:
        Option<Arc<ToolResultArtifactIndexStore>>,
    /// Concrete handle to the installed [`ContextCompressionMiddleware`], when
    /// summarization is active. Drained after the run to surface each compaction's
    /// [`CompressionProvenance`][tinyagents_harness::summarization::CompressionProvenance]
    /// via the observability path.
    pub(in crate::agent::tinyagents) compression_mw: Option<Arc<ContextCompressionMiddleware>>,
    /// Crate prompt-cache guard (issue #4249, 03.2). Records a `CacheLayoutEvent`
    /// whenever the cacheable prompt prefix (system prompt + tool set) changes
    /// across model calls. Drained after the run and surfaced via
    /// [`observability::surface_cache_layout_events`](crate::agent::tinyagents::observability::surface_cache_layout_events) —
    /// the crate-native replacement for the deleted `CacheAlignMiddleware`
    /// warn-log (C3).
    pub(in crate::agent::tinyagents) prompt_cache_guard: Arc<PromptCacheGuardMiddleware>,
}

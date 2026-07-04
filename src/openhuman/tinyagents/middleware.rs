//! openhuman context concerns expressed as tinyagents graph middlewares
//! (issue #4249).
//!
//! Historically these ran in the in-house engine's tool/prompt plumbing
//! (`agent_tool_exec`, `ContextManager`). The tinyagents turn path bypassed
//! them, so they were effectively dead on the live loop. Re-expressing them as
//! [`Middleware`] hooks restores the behaviour and makes the graph the single
//! place cross-cutting context concerns live:
//!
//! - [`MicrocompactMiddleware`] (`before_model`) — clear the bodies of older
//!   tool-result messages (keeping the N most recent) so a long tool-heavy
//!   thread stays cheap without dropping chat history. This is now the crate
//!   [`tinyagents::harness::middleware::MicrocompactMiddleware`], constructed
//!   with OpenHuman's [`CLEARED_PLACEHOLDER`] wording; the in-house copy was
//!   upstreamed (see `99-deletion-ledger.md`).
//! - [`ToolOutputMiddleware`] (`after_tool`) — apply the per-tool-result byte
//!   cap and (optionally) the semantic payload summarizer to each tool result
//!   as it returns, before it enters the transcript.
//!
//! [`TurnContextMiddleware`] bundles the config and installs whichever hooks are
//! enabled onto a harness.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;

use tinyagents::error::Result as TaResult;
use tinyagents::harness::context::RunContext;
use tinyagents::harness::events::AgentEvent;
use tinyagents::harness::message::{ContentBlock, Message as TaMessage};
use tinyagents::harness::middleware::{
    AgentRun, BudgetTracker, ContextualToolSelectionMiddleware, MicrocompactMiddleware, Middleware,
    MiddlewareToolOutcome, ToolAllowlistMiddleware, ToolHandler, ToolMiddleware,
};
use tinyagents::harness::model::{ModelRequest, PromptSegment, SegmentRole};
use tinyagents::harness::no_progress::{NoProgress, NoProgressTracker, ToolAttempt};
use tinyagents::harness::runtime::AgentHarness;
use tinyagents::harness::steering::{SteeringCommand, SteeringHandle};
use tinyagents::harness::tool::{
    ToolCall as TaToolCall, ToolPolicy as TaToolPolicy, ToolResult as TaToolResult, ToolSchema,
};

use crate::openhuman::agent::harness::tool_result_artifacts::{
    apply_per_result_persistence, ToolResultArtifactStore, TINYAGENTS_TOOL_RESULT_ARTIFACT_STORE,
};
use crate::openhuman::approval::{
    redact_args, summarize_action, ApprovalGate, ExecutionOutcome, GateOutcome,
};
use crate::openhuman::context::CLEARED_PLACEHOLDER;
use crate::openhuman::tinyagents::payload_summarizer::PayloadSummarizer;
use crate::openhuman::tokenjuice::AgentTokenjuiceCompression;
use crate::openhuman::tools::Tool;

use super::policy_denial::PolicyDenial;

/// Default per-tool-result byte cap for the channel / sub-agent paths, which do
/// not carry a session `ContextManager` to source the configured budget from.
/// Mirrors the `ContextConfig::tool_result_budget_bytes` default (16 KiB).
const DEFAULT_TOOL_RESULT_BUDGET_BYTES: usize = 16 * 1024;

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
    /// "Super context" first-turn context collection. `Some` installs the
    /// [`SuperContextMiddleware`] graph node; `None` (the default, and every
    /// non-chat path) skips it. Only the chat turn sets this — and only when its
    /// gate (`should_run_super_context`) passes.
    pub(crate) super_context: Option<SuperContextConfig>,
    /// Progressive-disclosure handoff: when set (integrations_agent with a
    /// resolved toolkit), oversized tool results are stashed in the shared
    /// [`ResultHandoffCache`] and replaced with an `extract_from_result` drill-in
    /// placeholder. `None` everywhere else.
    pub(crate) handoff: Option<HandoffConfig>,
}

/// Config for the [`HandoffMiddleware`]: the per-spawn cache (shared with the
/// `extract_from_result` tool) plus the ids used in handoff log lines.
#[derive(Clone)]
pub(crate) struct HandoffConfig {
    pub(crate) cache: Arc<crate::openhuman::agent::harness::subagent_runner::ResultHandoffCache>,
    pub(crate) agent_id: String,
    pub(crate) task_id: String,
}

/// SHADOW tool-exposure middleware (issue #4249, 01.3 — dynamic exposure).
///
/// This is the **adapter-first landing** of the crate-native tool-selection
/// layer. It expresses OpenHuman's exposure policy (agent
/// `tool_allowlist`/`tool_denylist` + sub-agent scope + MCP visibility + channel
/// permission ceiling — all already collapsed by the precompute path into the
/// single `allowed` visible set handed to `assemble_turn_harness`) as a composed
/// crate selection layer:
///
/// - a [`ToolAllowlistMiddleware`] for the static allow guard, and
/// - one [`ContextualToolSelectionMiddleware`] built via
///   [`ContextualToolSelectionMiddleware::inheriting`] so a delegated child can
///   only ever *narrow* the parent's exposure (sub-agent-cannot-exceed-parent).
///
/// It runs in **SHADOW**: on the first model call it drives the composed crate
/// selection over a scratch [`ModelRequest`] built from the **broad candidate
/// set** (not the live request, whose `tools` OpenHuman already narrowed), so it
/// (a) makes the exposure decision **event-native** via the crate selection's own
/// [`AgentEvent::ToolsFiltered`] emit, and (b) logs any DIVERGENCE (grep-friendly
/// `[tool-exposure]`) between what the crate layer would expose and the set
/// OpenHuman actually registered as callable. It **never** mutates the live
/// `ModelRequest::tools`, so the model's actually-callable tool set is
/// byte-identical to today (zero behavior risk). Exposure is fail-closed in the
/// COMPUTATION (a candidate absent from `allowed` is excluded), but that decision
/// is only logged/emitted — not enforced — this slice.
///
/// Ownership flip (making this crate selection the sole authority + deleting
/// `agent/harness/tool_filter.rs` and `subagent_runner/tool_prep.rs`) is the
/// GATED follow-up, once the `[tool-exposure]` divergence logs show parity.
pub(super) struct OpenHumanToolExposureShadowMiddleware {
    /// Static allow guard (crate). Held for the fail-closed parity cross-check;
    /// NOT installed as a live `before_tool` execution guard this slice —
    /// OpenHuman already registers only the `allowed` set, so the model can never
    /// call a hidden tool.
    allowlist: ToolAllowlistMiddleware,
    /// The composed contextual selection layer, built via `inheriting(...)`:
    /// parent ceiling = broad candidate set, child = precomputed visible set. Its
    /// `before_model` drives the shadow retain + emits `ToolsFiltered`.
    selection: ContextualToolSelectionMiddleware,
    /// Broad candidate tool set (names before the precompute narrowed it), as
    /// scratch schemas the shadow selection filters over.
    candidates: Vec<ToolSchema>,
    /// The set OpenHuman actually registered as callable this turn — the
    /// divergence reference.
    registered: std::collections::HashSet<String>,
    /// agent id / task kind / security tier / channel encoded as selection tags
    /// (carried onto the scratch request + surfaced in the divergence log). The
    /// `inheriting`/`from_lists` predicate is name-based today, so these tags are
    /// documentary context for the ownership-flip follow-up.
    tags: Vec<String>,
    /// One-shot latch — `before_model` fires on every model call, but the shadow
    /// exposure decision is a once-per-run computation.
    ran: AtomicBool,
}

impl OpenHumanToolExposureShadowMiddleware {
    /// Build the shadow layer from the SAME inputs the precompute path feeds the
    /// runner: the broad `candidate_names` and the narrowed `allowed` visible set.
    /// An empty `allowed` means "all candidates visible" (OpenHuman convention).
    pub(super) fn new(
        candidate_names: &[String],
        allowed: &std::collections::HashSet<String>,
        tags: Vec<String>,
    ) -> Self {
        // Effective visible set = the OpenHuman-precomputed `allowed` (or every
        // candidate when `allowed` is empty). Fail-closed: a candidate absent from
        // `allowed` is treated as excluded (unclassified -> not exposed).
        let registered: std::collections::HashSet<String> = if allowed.is_empty() {
            candidate_names.iter().cloned().collect()
        } else {
            candidate_names
                .iter()
                .filter(|name| allowed.contains(*name))
                .cloned()
                .collect()
        };
        let excluded: Vec<String> = candidate_names
            .iter()
            .filter(|name| !registered.contains(*name))
            .cloned()
            .collect();
        // Compose the crate selection via `inheriting` so a child can only narrow:
        // parent ceiling = the broad candidate set (deny none), child = the
        // precomputed visible set (deny the withheld candidates). The effective
        // allow is `candidates ∩ registered == registered ⊆ candidates`, so the
        // decision can never widen beyond what the parent candidate context could
        // grant — the sub-agent-cannot-exceed-parent invariant, computed.
        let selection = ContextualToolSelectionMiddleware::inheriting(
            Some(candidate_names.to_vec()),
            Vec::<String>::new(),
            Some(registered.iter().cloned().collect::<Vec<_>>()),
            excluded,
        );
        let allowlist = ToolAllowlistMiddleware::new(registered.iter().cloned());
        let candidates = candidate_names
            .iter()
            .map(|name| ToolSchema::new(name.clone(), String::new(), serde_json::json!({})))
            .collect();
        Self {
            allowlist,
            selection,
            candidates,
            registered,
            tags,
            ran: AtomicBool::new(false),
        }
    }
}

/// Inputs the [`SuperContextMiddleware`] node needs to run its first-turn
/// read-only context-collection pass.
#[derive(Clone)]
pub(crate) struct SuperContextConfig {
    /// The raw user ask, used as the context scout's query.
    pub(crate) user_message: String,
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
            super_context: None,
            handoff: None,
        }
    }

    /// `true` when no middleware would be installed.
    pub(crate) fn is_empty(&self) -> bool {
        self.tool_result_budget_bytes == 0
            && self.payload_summarizer.is_none()
            && !self.tokenjuice_compaction_enabled
            && self.microcompact_keep_recent == 0
            && self.super_context.is_none()
            && self.handoff.is_none()
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
        // Super context runs first: it prepares the read-only context bundle and
        // folds it into the first model call's user message before any other
        // before_model hook inspects the request.
        if let Some(sc) = self.super_context {
            harness.push_middleware(Arc::new(SuperContextMiddleware {
                user_message: sc.user_message,
                ran: AtomicBool::new(false),
            }));
        }
        if self.microcompact_keep_recent > 0 {
            // Crate middleware (upstreamed from the in-house copy). Constructed
            // with OpenHuman's model-facing placeholder so behavior is
            // byte-identical to the deleted local version. Events stay off (the
            // default) to preserve the prior silent-rewrite behavior.
            harness.push_middleware(Arc::new(MicrocompactMiddleware::new(
                self.microcompact_keep_recent,
                CLEARED_PLACEHOLDER,
            )));
        }
        // Handoff runs BEFORE the tool-output budget so an oversized payload is
        // stashed + replaced with a short placeholder first; the byte cap would
        // otherwise shrink it below the handoff threshold and defeat the drill-in.
        if let Some(handoff) = self.handoff {
            harness.push_middleware(Arc::new(HandoffMiddleware {
                cache: handoff.cache,
                agent_id: handoff.agent_id,
                task_id: handoff.task_id,
            }));
        }
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
    }
}

fn estimate_output_tokens(bytes: usize) -> u64 {
    bytes.div_ceil(4) as u64
}

#[async_trait]
impl Middleware<()> for OpenHumanToolExposureShadowMiddleware {
    fn name(&self) -> &str {
        "openhuman_tool_exposure_shadow"
    }

    async fn before_model(
        &self,
        ctx: &mut RunContext<()>,
        state: &(),
        request: &mut ModelRequest,
    ) -> TaResult<()> {
        // Once-per-run: the exposure decision is stable for the turn.
        if self.ran.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        // SHADOW: drive the composed crate selection over a SCRATCH request built
        // from the BROAD candidate set — deliberately NOT the live `request`,
        // whose `tools` OpenHuman already narrowed to the visible set. This lets
        // the crate layer compute the exposure decision over the full candidate
        // context and emit it event-native (the crate
        // `ContextualToolSelectionMiddleware::before_model` emits
        // `AgentEvent::ToolsFiltered` on `ctx` for the withheld candidates) —
        // without ever dropping a tool the model can actually call. The live
        // `request.tools` is left untouched.
        let mut scratch = ModelRequest {
            tools: self.candidates.clone(),
            model: request.model.clone(),
            tags: self.tags.clone(),
            ..Default::default()
        };
        // Reuse the crate selection's own retain + `ToolsFiltered` emit verbatim.
        self.selection
            .before_model(ctx, state, &mut scratch)
            .await?;
        let shadow_exposed: std::collections::HashSet<String> =
            scratch.tools.iter().map(|s| s.name.clone()).collect();

        // Divergence vs what OpenHuman actually registered as callable this turn.
        let mut missing_from_shadow: Vec<&String> = self
            .registered
            .iter()
            .filter(|name| !shadow_exposed.contains(*name))
            .collect();
        let mut extra_in_shadow: Vec<&String> = shadow_exposed
            .iter()
            .filter(|name| !self.registered.contains(*name))
            .collect();
        // Fail-closed cross-check: every shadow-exposed name must also pass the
        // static allow guard (they are built from the same set, so this should be
        // vacuously true; a mismatch would flag a policy-composition bug).
        let mut allowlist_disagree: Vec<&String> = shadow_exposed
            .iter()
            .filter(|name| !self.allowlist.allows(name))
            .collect();
        missing_from_shadow.sort();
        extra_in_shadow.sort();
        allowlist_disagree.sort();

        if missing_from_shadow.is_empty()
            && extra_in_shadow.is_empty()
            && allowlist_disagree.is_empty()
        {
            tracing::debug!(
                exposed = shadow_exposed.len(),
                candidates = self.candidates.len(),
                registered = self.registered.len(),
                tags = ?self.tags,
                "[tool-exposure] shadow crate selection agrees with OpenHuman precompute (parity)"
            );
        } else {
            tracing::warn!(
                ?missing_from_shadow,
                ?extra_in_shadow,
                ?allowlist_disagree,
                registered = self.registered.len(),
                shadow_exposed = shadow_exposed.len(),
                candidates = self.candidates.len(),
                tags = ?self.tags,
                "[tool-exposure] DIVERGENCE: shadow crate selection differs from OpenHuman precompute — NOT enforced (SHADOW; ownership flip is the gated follow-up)"
            );
        }
        Ok(())
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
    cache: Arc<crate::openhuman::agent::harness::subagent_runner::ResultHandoffCache>,
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
        result.content = crate::openhuman::agent::harness::subagent_runner::apply_handoff(
            &self.cache,
            &result.name,
            &self.task_id,
            &self.agent_id,
            std::mem::take(&mut result.content),
        );
        Ok(())
    }
}

/// `before_model` (first call only): "super context" — the graph node analogue
/// of the harness-driven first-turn context collection that used to run
/// imperatively in `session/turn/core.rs`. On the first model call it runs the
/// read-only `context_scout` sub-agent against the raw user ask, folds the
/// resulting `[context_bundle]` into the user message, and registers a
/// prepared-context source so a later `agent_prepare_context` call in the same
/// turn self-suppresses.
///
/// Best-effort: any scout error leaves the turn to proceed with the
/// un-augmented message rather than blocking the user. Runs inside the parent
/// context scope the chat turn already installs (`with_parent_context`), which
/// the scout reads via `current_parent()`.
struct SuperContextMiddleware {
    /// The raw user ask, used as the scout's query (not the enriched message).
    user_message: String,
    /// One-shot latch — `before_model` fires on every model call, but super
    /// context is a first-turn, once-per-run pass.
    ran: AtomicBool,
}

#[async_trait]
impl Middleware<()> for SuperContextMiddleware {
    fn name(&self) -> &str {
        "super_context"
    }

    async fn before_model(
        &self,
        _ctx: &mut RunContext<()>,
        _state: &(),
        request: &mut ModelRequest,
    ) -> TaResult<()> {
        if self.ran.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        let scout = crate::openhuman::agent_orchestration::tools::run_context_scout(
            &self.user_message,
            None,
        )
        .await;
        match scout {
            Ok(result) if !result.is_error => {
                let bundle = result.output();
                // Register the source live so `agent_prepare_context` (which reads
                // `current_agent_context_prepared_sources()`) self-suppresses for
                // the rest of the turn. Only on success — a failed scout must not
                // block a legitimate retry.
                crate::openhuman::agent::harness::push_agent_context_prepared_source(
                    crate::openhuman::agent::harness::AgentContextPreparedSource {
                        source: "super context preparation".to_string(),
                        has_enough_context: parse_context_bundle_has_enough_context(&bundle),
                    },
                );
                tracing::info!(
                    bundle_chars = bundle.chars().count(),
                    "[tinyagents::mw] super_context bundle collected — folding into user message"
                );
                let block = format!(
                    "## Agent context status\n\nAgent context retrieval/preparation has already \
                     run once for this turn in code via super context preparation. Do not call \
                     `agent_prepare_context` again for general context preparation. Use the \
                     prepared context below, and call only specific follow-up tools if a concrete \
                     missing detail is required.\n\n\
                     ## Prepared context (super context)\n\nThe following context was collected \
                     up-front by a read-only context scout before this turn. Use it to ground your \
                     response; do not call `agent_prepare_context` again for general preparation.\n\n\
                     {bundle}\n\n---\n\n"
                );
                prepend_text_to_last_user(&mut request.messages, block);
            }
            Ok(_) => {
                tracing::warn!(
                    "[tinyagents::mw] super_context scout returned an error — proceeding without bundle"
                );
            }
            Err(err) => {
                tracing::warn!(
                    %err,
                    "[tinyagents::mw] super_context collection failed — proceeding without bundle"
                );
            }
        }
        Ok(())
    }
}

/// Prepend a text block to the most recent user message, preserving its existing
/// content blocks (multimodal image blocks survive — the bundle rides in front
/// as a new leading text block). No-op if there is no user message.
fn prepend_text_to_last_user(messages: &mut [TaMessage], block: String) {
    if let Some(TaMessage::User(m)) = messages
        .iter_mut()
        .rev()
        .find(|m| matches!(m, TaMessage::User(_)))
    {
        m.content.insert(0, ContentBlock::Text(block));
    }
}

/// Parse the `has_enough_context: true|false` marker line the context scout
/// emits inside its `[context_bundle]`. Mirrors the former core.rs helper so the
/// prepared-source record carries the same signal. Returns `None` when absent or
/// unparseable.
fn parse_context_bundle_has_enough_context(bundle: &str) -> Option<bool> {
    const PREFIX: &str = "has_enough_context:";
    let line = bundle.lines().map(str::trim).find(|line| {
        line.get(..PREFIX.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(PREFIX))
    })?;
    let value = line[PREFIX.len()..].trim();
    if value.eq_ignore_ascii_case("true") {
        Some(true)
    } else if value.eq_ignore_ascii_case("false") {
        Some(false)
    } else {
        None
    }
}

/// Seed-free FNV-1a fingerprint (matches the crate's own prompt-layout hash
/// approach) so a segment id is stable across process restarts — unlike Rust's
/// randomly-seeded `SipHash`. Used to build content-fingerprinted prompt-cache
/// segment ids: an unchanged system prompt / tool set keeps the same id (stable
/// prefix), while injected volatile content flips it and surfaces as a
/// `CacheLayoutEvent`.
fn stable_prefix_fingerprint(data: &str) -> String {
    const OFFSET_BASIS: u64 = 14_695_981_039_346_656_037;
    const PRIME: u64 = 1_099_511_628_211;
    let mut hash = OFFSET_BASIS;
    for &byte in data.as_bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(PRIME);
    }
    format!("{hash:016x}")
}

/// `before_model`: declare the turn's stable prompt prefix (system prompt + tool
/// schemas) as [`PromptSegment`]s on the [`ModelRequest`] (issue #4249, 03.2).
///
/// OpenHuman assembles the request's messages/tools directly rather than through
/// the crate prompt builder, so `cache_segments` would otherwise stay empty and
/// the crate `PromptCacheGuardMiddleware` (installed immediately after this) would
/// have no prefix to protect. This stamps the segments with **content-fingerprint
/// ids**: an unchanged system prompt + tool set yields a stable prefix, while an
/// injected timestamp/uuid/etc. changes the fingerprint and the guard records a
/// [`CacheLayoutEvent`](tinyagents::harness::cache::CacheLayoutEvent). This is
/// the structured, crate-native replacement for the deleted warn-only
/// `CacheAlignMiddleware` volatile-token scan (C3): the crate
/// `PromptCacheGuardMiddleware` now owns KV-cache-prefix drift detection via
/// recorded `CacheLayoutEvent`s. Read-only w.r.t. the transcript — only sets
/// `cache_segments` / `prompt_fingerprint`.
pub(crate) struct PromptCacheSegmentMiddleware;

#[async_trait]
impl Middleware<()> for PromptCacheSegmentMiddleware {
    fn name(&self) -> &str {
        "prompt_cache_segments"
    }

    async fn before_model(
        &self,
        _ctx: &mut RunContext<()>,
        _state: &(),
        request: &mut ModelRequest,
    ) -> TaResult<()> {
        let mut segments: Vec<PromptSegment> = Vec::new();
        // 1. System prompt — the cache-hottest stable prefix segment.
        if let Some(sys) = request
            .messages
            .iter()
            .find(|m| matches!(m, TaMessage::System(_)))
        {
            let fp = stable_prefix_fingerprint(&sys.text());
            segments.push(PromptSegment {
                id: format!("system:{fp}"),
                role: SegmentRole::System,
                cacheable: true,
            });
        }
        // 2. Tool schemas — advertised tool *set* identity (names, in registration
        //    order) forms the next stable prefix segment. A changed tool surface
        //    legitimately busts the prefix; an unchanged one keeps it stable.
        if !request.tools.is_empty() {
            let joined = request
                .tools
                .iter()
                .map(|t| t.name.as_str())
                .collect::<Vec<_>>()
                .join(",");
            let fp = stable_prefix_fingerprint(&joined);
            segments.push(PromptSegment {
                id: format!("tools:{fp}"),
                role: SegmentRole::Tools,
                cacheable: true,
            });
        }
        if !segments.is_empty() {
            let joined_ids = segments
                .iter()
                .map(|s| s.id.as_str())
                .collect::<Vec<_>>()
                .join("|");
            request.prompt_fingerprint = Some(stable_prefix_fingerprint(&joined_ids));
            tracing::debug!(
                segment_count = segments.len(),
                fingerprint = request.prompt_fingerprint.as_deref().unwrap_or(""),
                "[cache] declared stable prompt-prefix segments for KV-cache guard"
            );
            request.cache_segments = segments;
        }
        Ok(())
    }
}

/// `after_tool`: apply the semantic payload summarizer (when configured) and
/// then the hard per-tool-result byte cap to each tool result's model-facing
/// content, before it enters the transcript. The graph analogue of the byte cap
/// + `payload_summarizer` interception the in-house `agent_tool_exec` ran.
struct ToolOutputMiddleware {
    /// Fallback per-tool-result byte cap for tools that don't declare their own.
    budget_bytes: usize,
    payload_summarizer: Option<Arc<dyn PayloadSummarizer>>,
    artifact_store: Option<ToolResultArtifactStore>,
    tokenjuice_compaction_enabled: bool,
    tokenjuice_compression: AgentTokenjuiceCompression,
    /// SDK policy snapshot keyed by tool name. Used to honor the adapter-mapped
    /// `max_result_size_chars()` cap without re-querying the OpenHuman tool
    /// trait from `after_tool`.
    tool_policies: HashMap<String, TaToolPolicy>,
}

impl ToolOutputMiddleware {
    /// The tool's own declared cap, if any. The adapter maps OpenHuman's
    /// `max_result_size_chars()` into `ToolRuntime.max_result_bytes`; preserving
    /// char-based truncation here keeps the existing model-facing marker stable.
    fn tool_char_cap(&self, name: &str) -> Option<usize> {
        self.tool_policies
            .get(name)
            .and_then(|policy| policy.runtime.max_result_bytes)
    }
}

#[async_trait]
impl Middleware<()> for ToolOutputMiddleware {
    fn name(&self) -> &str {
        "tool_output_budget"
    }

    async fn after_tool(
        &self,
        ctx: &mut RunContext<()>,
        _state: &(),
        result: &mut TaToolResult,
    ) -> TaResult<()> {
        // 1. Semantic summarization (progressive disclosure) — swap the raw
        //    payload for a compressed summary when the summarizer opts in.
        //    Failures never break the tool call (the trait swallows them).
        if let Some(ps) = &self.payload_summarizer {
            if let Ok(Some(payload)) = ps
                .maybe_summarize_in_parent(ctx, &result.name, None, &result.content)
                .await
            {
                tracing::info!(
                    tool = %result.name,
                    from_bytes = payload.original_bytes,
                    to_bytes = payload.summary_bytes,
                    "[tinyagents::mw] payload_summarizer compressed tool output"
                );
                ctx.emit(AgentEvent::Compressed {
                    from_tokens: estimate_output_tokens(payload.original_bytes),
                    to_tokens: estimate_output_tokens(payload.summary_bytes),
                });
                result.content = payload.summary;
            }
        }

        // 2. TokenJuice content-aware compaction. This mirrors the legacy
        //    `agent_tool_exec` stage that ran after semantic summarization and
        //    before the hard output caps.
        let before_tokenjuice_bytes = result.content.len();
        let compacted = crate::openhuman::tokenjuice::compact_output_with_policy(
            std::mem::take(&mut result.content),
            &result.name,
            self.tokenjuice_compaction_enabled,
            self.tokenjuice_compression,
        )
        .await;
        result.content = compacted;
        let after_tokenjuice_bytes = result.content.len();
        if after_tokenjuice_bytes < before_tokenjuice_bytes {
            ctx.emit(AgentEvent::Compressed {
                from_tokens: estimate_output_tokens(before_tokenjuice_bytes),
                to_tokens: estimate_output_tokens(after_tokenjuice_bytes),
            });
        }

        // 3. Per-tool **char** cap — a tool that declares `max_result_size_chars`
        //    caps its own output in characters, with the tool-cap marker the model
        //    was taught to read (legacy engine parity). Distinct from the generic
        //    byte budget below: the tool cap is the tool's own contract.
        let tool_cap = self.tool_char_cap(&result.name);
        if let Some(cap) = tool_cap {
            let char_count = result.content.chars().count();
            if char_count > cap {
                let truncated: String = result.content.chars().take(cap).collect();
                let dropped = char_count - cap;
                tracing::debug!(
                    tool = %result.name,
                    cap,
                    char_count,
                    dropped,
                    "[tinyagents::mw] per-tool char cap applied"
                );
                result.content = format!(
                    "{truncated}\n\n[truncated by tool cap: {dropped} more chars not shown]"
                );
            }
        }

        // 4. Shared byte-cap backstop — truncate at a UTF-8 boundary with a marker.
        //    Only for tools with no cap of their own (a capped tool already bounded
        //    itself above; stacking the two markers would double-truncate).
        if tool_cap.is_none() && self.budget_bytes > 0 {
            let (capped, outcome) = apply_per_result_persistence(
                std::mem::take(&mut result.content),
                self.artifact_store.as_ref(),
                &result.name,
                Some(&result.call_id),
                self.budget_bytes,
            )
            .await;
            if outcome.persisted {
                tracing::info!(
                    tool = %result.name,
                    from_bytes = outcome.original_bytes,
                    to_bytes = outcome.final_bytes,
                    "[tinyagents::mw] tool_result_artifact persisted oversized output"
                );
                if let Some(path) = outcome.artifact_path.as_deref() {
                    if let Some(store) = ctx.stores.get(TINYAGENTS_TOOL_RESULT_ARTIFACT_STORE) {
                        let key = result.call_id.clone();
                        let mut fields = serde_json::Map::new();
                        fields.insert("tool".to_string(), result.name.clone().into());
                        fields.insert("call_id".to_string(), result.call_id.clone().into());
                        fields.insert("artifact_path".to_string(), path.to_string().into());
                        fields.insert(
                            "original_bytes".to_string(),
                            serde_json::Value::from(outcome.original_bytes as u64),
                        );
                        fields.insert(
                            "preview_bytes".to_string(),
                            serde_json::Value::from(outcome.final_bytes as u64),
                        );
                        let index_result: tinyagents::Result<()> =
                            store.put("tool_results", &key, fields.into()).await;
                        if let Err(err) = index_result {
                            tracing::warn!(
                                tool = %result.name,
                                call_id = %result.call_id,
                                error = %err,
                                "[tinyagents::mw] failed to index tool_result_artifact"
                            );
                        } else {
                            tracing::debug!(
                                tool = %result.name,
                                call_id = %result.call_id,
                                artifact_path = %path,
                                "[tinyagents::mw] indexed tool_result_artifact in run store"
                            );
                        }
                    }
                }
            } else if outcome.original_bytes != outcome.final_bytes {
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
pub(super) struct ApprovalSecurityMiddleware {
    /// The same `Arc`-shared tool sets the runner registers, used to resolve a
    /// call's OpenHuman `Tool` by name so `external_effect_with_args` can gate.
    tool_sets: Vec<Arc<Vec<Box<dyn Tool>>>>,
}

impl ApprovalSecurityMiddleware {
    /// Build the middleware over the runner's shared tool sets.
    pub(super) fn new(tool_sets: Vec<Arc<Vec<Box<dyn Tool>>>>) -> Self {
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

/// `wrap_tool`: refuse a tool whose scope is
/// [`ToolScope::CliRpcOnly`](crate::openhuman::tools::ToolScope) inside the
/// autonomous agent loop (issue #4249). The in-house engine ran this gate in
/// `engine::tools`; the tinyagents path dropped it, so a CLI/RPC-only tool
/// (e.g. phone calls) would execute from the model loop. Applies on every path
/// (channel, session, sub-agent) since the restriction is intrinsic to the tool,
/// not the session — installed unconditionally.
pub(super) struct CliRpcOnlyMiddleware {
    tool_sets: Vec<Arc<Vec<Box<dyn Tool>>>>,
}

impl CliRpcOnlyMiddleware {
    pub(super) fn new(tool_sets: Vec<Arc<Vec<Box<dyn Tool>>>>) -> Self {
        Self { tool_sets }
    }

    fn is_cli_rpc_only(&self, name: &str) -> bool {
        self.tool_sets
            .iter()
            .flat_map(|set| set.iter())
            .find(|t| t.name() == name)
            .map(|t| t.scope() == crate::openhuman::tools::ToolScope::CliRpcOnly)
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

/// `wrap_tool`: enforce the agent's builder-configured [`ToolPolicy`] at the tool
/// boundary (issue #4249). The in-house engine ran this check in
/// `agent_tool_exec` (`ctx.tool_policy.check(...)`); the tinyagents path bypassed
/// it, so a `.tool_policy()` deny/require-approval silently no-opped and the tool
/// executed anyway — a security regression. This middleware restores it: a
/// blocking decision short-circuits with a model-consumable result carrying the
/// same `"Tool '<name>' <denied|requires approval> by policy '<policy>': <reason>"`
/// wording the engine produced.
pub(super) struct ToolPolicyMiddleware {
    policy: Arc<dyn crate::openhuman::agent::tool_policy::ToolPolicy>,
    /// The session's channel-permission snapshot — enforces the per-channel deny
    /// + per-call permission-level ceiling the engine ran in `agent_tool_exec`.
    session: crate::openhuman::agent_tool_policy::ToolPolicySession,
    /// Shared tool sets (same `Arc`s the runner registers) so a call's OpenHuman
    /// `Tool` can be resolved for its generated-tool runtime context and its
    /// per-call permission level.
    tool_sets: Vec<Arc<Vec<Box<dyn Tool>>>>,
    session_id: String,
    channel: String,
    agent_definition_id: String,
}

impl ToolPolicyMiddleware {
    pub(super) fn new(
        policy: Arc<dyn crate::openhuman::agent::tool_policy::ToolPolicy>,
        session: crate::openhuman::agent_tool_policy::ToolPolicySession,
        tool_sets: Vec<Arc<Vec<Box<dyn Tool>>>>,
        session_id: String,
        channel: String,
        agent_definition_id: String,
    ) -> Self {
        Self {
            policy,
            session,
            tool_sets,
            session_id,
            channel,
            agent_definition_id,
        }
    }

    fn resolve_tool(&self, name: &str) -> Option<&Box<dyn Tool>> {
        self.tool_sets
            .iter()
            .flat_map(|set| set.iter())
            .find(|t| t.name() == name)
    }

    /// The channel-permission gate the engine ran before the builder policy: a
    /// session-level deny, then a per-call permission-level ceiling check. Returns
    /// the blocking message when the call must not execute.
    fn channel_permission_block(&self, call: &TaToolCall) -> Option<String> {
        let decision = self.session.decision_for(&call.name);
        if decision.is_denied() {
            return Some(
                PolicyDenial::SessionForbidden {
                    tool: &call.name,
                    required: decision.required_permission,
                    allowed: decision.allowed_permission,
                    channel: &self.channel,
                }
                .render(),
            );
        }
        let tool = self.resolve_tool(&call.name)?;
        let call_required = tool.permission_level_with_args(&call.arguments);
        if call_required > decision.allowed_permission {
            return Some(
                PolicyDenial::PermissionTooLow {
                    tool: &call.name,
                    required: call_required,
                    allowed: decision.allowed_permission,
                    channel: &self.channel,
                }
                .render(),
            );
        }
        None
    }

    fn generated_context(
        &self,
        name: &str,
        args: &serde_json::Value,
    ) -> Option<crate::openhuman::agent::tool_policy::GeneratedToolRuntimeContext> {
        self.tool_sets
            .iter()
            .flat_map(|set| set.iter())
            .find(|t| t.name() == name)
            .and_then(|t| t.generated_runtime_context(args))
    }
}

#[async_trait]
impl ToolMiddleware<()> for ToolPolicyMiddleware {
    fn name(&self) -> &str {
        "tool_policy"
    }

    async fn wrap_tool(
        &self,
        ctx: &mut RunContext<()>,
        state: &(),
        call: TaToolCall,
        next: ToolHandler<'_, (), ()>,
    ) -> TaResult<MiddlewareToolOutcome> {
        use crate::openhuman::agent::tool_policy::{
            ToolCallContext, ToolPolicyDecision, ToolPolicyRequest,
        };

        // Channel-permission ceiling first (session deny + per-call permission
        // level), mirroring the engine order in `agent_tool_exec`.
        if let Some(message) = self.channel_permission_block(&call) {
            tracing::debug!(
                tool = call.name.as_str(),
                channel = self.channel.as_str(),
                "[tinyagents::mw] tool blocked by channel permission ceiling"
            );
            return Ok(MiddlewareToolOutcome::Result(TaToolResult {
                call_id: call.id,
                name: call.name,
                content: message.clone(),
                raw: None,
                error: Some(message),
                elapsed_ms: 0,
            }));
        }

        let context = ToolCallContext::session(
            self.session_id.clone(),
            self.channel.clone(),
            self.agent_definition_id.clone(),
            call.id.clone(),
            1,
        );
        let mut request =
            ToolPolicyRequest::new(call.name.clone(), call.arguments.clone(), context);
        if let Some(generated) = self.generated_context(&call.name, &call.arguments) {
            request = request.with_generated_tool_context(generated);
        }

        let decision = self.policy.check(&request).await;
        if let Some(reason) = decision.blocking_reason() {
            let blocked_action = match &decision {
                ToolPolicyDecision::RequireApproval { .. } => "requires approval",
                ToolPolicyDecision::Deny { .. } => "denied",
                ToolPolicyDecision::Allow => "allowed",
            };
            crate::openhuman::tool_registry::denials::record(
                call.name.as_str(),
                self.policy.name(),
                blocked_action,
                reason,
            );
            tracing::debug!(
                tool = call.name.as_str(),
                policy = self.policy.name(),
                action = blocked_action,
                reason = %reason,
                "[tinyagents::mw] tool blocked by policy"
            );
            let content = match &decision {
                ToolPolicyDecision::RequireApproval { .. } => PolicyDenial::ApprovalRequired {
                    tool: &call.name,
                    policy: self.policy.name(),
                    reason,
                },
                _ => PolicyDenial::PolicyDenied {
                    tool: &call.name,
                    policy: self.policy.name(),
                    reason,
                },
            }
            .render();
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

/// `after_tool`: capture each tool call's execution outcome (success + content)
/// into a shared sink before the harness folds the result into a `Message::tool`
/// that drops the `error` flag (issue #4249). Without this, a post-turn
/// `ToolCallRecord` could only report every call as an optimistic success — the
/// in-house engine tracked real per-call success. Runs last in the `after_tool`
/// chain so it records the final (summarized/capped) content the transcript keeps.
pub(crate) struct ToolOutcomeCaptureMiddleware {
    sink: super::ToolOutcomeSink,
    /// `call_id → (success, classified failure)` side-channel read by the event
    /// bridge when projecting `ToolCallCompleted` (the crate event lacks the
    /// success/error the failure UI needs).
    failure_map: super::observability::ToolFailureMap,
}

impl ToolOutcomeCaptureMiddleware {
    pub(crate) fn new(
        sink: super::ToolOutcomeSink,
        failure_map: super::observability::ToolFailureMap,
    ) -> Self {
        Self { sink, failure_map }
    }
}

#[async_trait]
impl Middleware<()> for ToolOutcomeCaptureMiddleware {
    fn name(&self) -> &str {
        "tool_outcome_capture"
    }

    async fn after_tool(
        &self,
        _ctx: &mut RunContext<()>,
        _state: &(),
        result: &mut TaToolResult,
    ) -> TaResult<()> {
        // Enrich a raw security-policy / autonomy block (issue #4094): the ~20
        // `[policy-blocked]` denials emitted deep in `SecurityPolicy` / the tools
        // return a bare marker line with no workaround and no relay directive, so
        // the agent dead-ends. Rewrite the content into the structured
        // `Blocked / Reason / Workaround / relay` shape here — the last `after_tool`
        // hook, so the enriched text is what the transcript keeps. The marker is
        // preserved, and already-structured `ToolPolicyMiddleware` denials (which
        // carry a `Workaround:` suffix) are left untouched. This runs before
        // classification below, which still recognises the preserved marker.
        if let Some(enriched) =
            super::policy_denial::maybe_enrich_policy_block(&result.name, &result.content)
        {
            tracing::debug!(
                tool = result.name.as_str(),
                "[tinyagents::mw] enriched raw security-policy block with workaround + relay"
            );
            result.content = enriched;
        }

        let success = result.error.is_none();
        // Classify the failure so the live `ToolCallCompleted` event and the
        // persisted timeline can explain it in plain language. A hard
        // policy/permission denial is its own class; otherwise heuristics over
        // the error text (`timed_out` detected from the timeout branch's phrase).
        let failure = if success {
            None
        } else {
            let text = result.error.as_deref().unwrap_or(result.content.as_str());
            if result
                .content
                .contains(crate::openhuman::security::POLICY_BLOCKED_MARKER)
            {
                Some(crate::openhuman::tool_status::describe(
                    crate::openhuman::tool_status::ToolFailureClass::BlockedByPolicy,
                ))
            } else {
                let timed_out = result.content.contains("timed out");
                Some(crate::openhuman::tool_status::classify(text, timed_out))
            }
        };
        if let Ok(mut map) = self.failure_map.lock() {
            map.insert(result.call_id.clone(), (success, failure));
        }
        if let Ok(mut sink) = self.sink.lock() {
            sink.push(super::ToolCallOutcome {
                call_id: result.call_id.clone(),
                name: result.name.clone(),
                success,
                content: result.content.clone(),
            });
        }
        Ok(())
    }
}

/// `before_tool`: coerce a tool call's arguments to an empty object when they
/// are not a JSON object (issue #4249). A model can emit malformed native
/// arguments (invalid JSON, or a bare scalar/array); the model adapter parses
/// those to `Value::Null`, which the harness then rejects against an object
/// schema and aborts the whole turn. The in-house engine recovered such a call by
/// running the tool with `{}`; restore that so a single bad tool call is
/// recoverable rather than fatal.
pub(crate) struct ArgRecoveryMiddleware;

#[async_trait]
impl Middleware<()> for ArgRecoveryMiddleware {
    fn name(&self) -> &str {
        "arg_recovery"
    }

    async fn before_tool(
        &self,
        _ctx: &mut RunContext<()>,
        _state: &(),
        call: &mut TaToolCall,
    ) -> TaResult<()> {
        if !call.arguments.is_object() {
            tracing::debug!(
                tool = call.name.as_str(),
                "[tinyagents::mw] recovering non-object tool arguments to {{}}"
            );
            call.arguments = serde_json::json!({});
        }
        Ok(())
    }
}

/// Agents are told to follow a **read-index → dedupe → write → update-index**
/// cycle around durable memory, but the contract was never enforced, so it was
/// followed inconsistently: writes landed without a dedupe read (duplicating
/// entries) and `update_memory_md` was skipped (so `MEMORY.md` drifted from the
/// store). This middleware observes the ordered sequence of *successful* memory
/// tool calls via [`MemoryProtocolTracker`] and, on each memory write, appends a
/// corrective note to the tool result so the model is nudged back onto the
/// protocol — the same "structured correction surfaced to the model" pattern the
/// unknown-tool recovery (#4118) uses. At run end it warns when a write was never
/// followed by an index update (the index is left stale).
///
/// Only *successful* ops advance the state machine — a failed `memory_store`
/// neither creates an entry nor obliges an index update. Non-memory tools are
/// ignored, so this is a no-op on turns that never touch memory.
pub struct MemoryProtocolMiddleware {
    tracker:
        std::sync::Mutex<crate::openhuman::agent::harness::memory_protocol::MemoryProtocolTracker>,
    /// call_id → classified op, captured in `before_tool` (the tool result carries
    /// no arguments, yet `update_memory_md` and `memory_tree` can only be
    /// classified from their `file` / `mode` argument). Correlated back by
    /// `result.call_id` in `after_tool`.
    pending_ops: std::sync::Mutex<
        std::collections::HashMap<
            String,
            crate::openhuman::agent::harness::memory_protocol::MemoryOp,
        >,
    >,
}

impl MemoryProtocolMiddleware {
    pub fn new() -> Self {
        Self {
            tracker: std::sync::Mutex::new(
                crate::openhuman::agent::harness::memory_protocol::MemoryProtocolTracker::new(),
            ),
            pending_ops: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }
}

impl Default for MemoryProtocolMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Middleware<()> for MemoryProtocolMiddleware {
    fn name(&self) -> &str {
        "memory_protocol"
    }

    async fn before_tool(
        &self,
        _ctx: &mut RunContext<()>,
        _state: &(),
        call: &mut TaToolCall,
    ) -> TaResult<()> {
        // Classify with the arguments in hand (the result won't carry them) and
        // stash the op keyed by call id. Only memory-relevant ops are stored, so
        // the map stays empty on turns that never touch memory.
        let op = crate::openhuman::agent::harness::memory_protocol::classify_memory_op(
            &call.name,
            &call.arguments,
        );
        if op != crate::openhuman::agent::harness::memory_protocol::MemoryOp::Other {
            if let Ok(mut ops) = self.pending_ops.lock() {
                ops.insert(call.id.clone(), op);
            }
        }
        Ok(())
    }

    async fn after_tool(
        &self,
        _ctx: &mut RunContext<()>,
        _state: &(),
        result: &mut TaToolResult,
    ) -> TaResult<()> {
        // Consume the op captured for this call (removing it so the map can't
        // grow unbounded). Absent → a non-memory tool: nothing to enforce.
        let op = self
            .pending_ops
            .lock()
            .ok()
            .and_then(|mut ops| ops.remove(&result.call_id));
        let Some(op) = op else {
            return Ok(());
        };
        // Only successful memory ops advance the protocol — a failed write did
        // not mutate memory and must not demand an index update.
        if result.error.is_some() {
            return Ok(());
        }
        let observation = {
            let mut tracker = match self.tracker.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            tracker.observe(op)
        };
        if let Some(note) = observation.guidance(&result.name) {
            tracing::debug!(
                tool = result.name.as_str(),
                missing_index_read = observation.missing_index_read,
                index_drift = observation.index_drift,
                "[tinyagents::mw] memory-protocol guidance appended to tool result"
            );
            if !result.content.is_empty() {
                result.content.push_str("\n\n");
            }
            result.content.push_str(&note);
        }
        Ok(())
    }

    async fn after_agent(
        &self,
        _ctx: &mut RunContext<()>,
        _state: &(),
        _run: &mut AgentRun,
    ) -> TaResult<()> {
        let pending = self
            .tracker
            .lock()
            .map(|tracker| tracker.pending_index_update())
            .unwrap_or(false);
        if pending {
            tracing::warn!(
                "[tinyagents::mw] memory-protocol: run ended with a memory write that was never \
                 followed by update_memory_md — the MEMORY.md index is left stale"
            );
        }
        Ok(())
    }
}

/// `before_model`: enforce OpenHuman's daily/monthly cost budgets **before** a
/// model call spends (issue #4249, Phase 5). Reads the global
/// [`CostTracker`](crate::openhuman::cost) and, when cost budgets are configured
/// and already exceeded, fails the run before the provider call; a warning
/// threshold logs but proceeds. This enforcement path stays **authoritative**.
///
/// Self-gating: a no-op unless a global tracker exists and `config.enabled` with
/// a limit is set (`check_budget` returns `Allowed` otherwise). Complements the
/// post-call `StopHookMiddleware` per-turn USD cap. Projecting the *next* call's
/// cost pre-spend (vs the already-exceeded check here) needs an input-token
/// estimate — a follow-up.
///
/// # Shadow role (W2-budget-dedupe)
///
/// When built with [`with_shadow`](Self::with_shadow), this middleware is ALSO a
/// divergence-logging shadow over the observe-only crate
/// [`BudgetMiddleware`](tinyagents::harness::middleware::BudgetMiddleware). It
/// keeps enforcing exactly as before, but at `after_agent` it compares the
/// crate `BudgetMiddleware`'s shared [`BudgetTracker`] accumulation against the
/// authoritative runtime [`AgentRun::usage`] and logs `[budget_shadow]` parity
/// or divergence (compact numeric summary; no PII). Both accumulate the same
/// per-call `response.usage`, so token totals must match once the crate
/// middleware is on the path — this is the parity signal that must be clean
/// before enforcement can flip to the crate owner (see the flip-criteria comment
/// at the registration site in `tinyagents/mod.rs`). Cost is intentionally NOT
/// compared: the observe-only crate middleware has no pricing table, so its cost
/// stays zero while the local path prices via `cost::catalog` — cost parity is a
/// flip-criteria follow-up.
pub(crate) struct CostBudgetMiddleware {
    /// Observe-only crate `BudgetMiddleware`'s shared tracker handle, for the
    /// end-of-run `[budget_shadow]` comparison. `None` when the shadow is not
    /// installed (isolated unit tests of the enforcement gate).
    shadow_tracker: Option<BudgetTracker>,
}

impl CostBudgetMiddleware {
    /// Enforcement-only gate with no shadow comparison (isolated unit tests).
    pub(crate) fn new() -> Self {
        Self {
            shadow_tracker: None,
        }
    }

    /// Enforcement gate that ALSO compares its per-run token accounting against
    /// the observe-only crate `BudgetMiddleware`'s shared `tracker` at end of run
    /// and logs `[budget_shadow]` parity/divergence.
    pub(crate) fn with_shadow(tracker: BudgetTracker) -> Self {
        Self {
            shadow_tracker: Some(tracker),
        }
    }
}

#[async_trait]
impl Middleware<()> for CostBudgetMiddleware {
    fn name(&self) -> &str {
        "cost_budget"
    }

    async fn before_model(
        &self,
        _ctx: &mut RunContext<()>,
        _state: &(),
        _request: &mut ModelRequest,
    ) -> TaResult<()> {
        use crate::openhuman::cost::types::BudgetCheck;
        let Some(tracker) = crate::openhuman::cost::try_global() else {
            return Ok(());
        };
        // Pass 0.0 to test whether we are *already* over budget before spending
        // more (rather than projecting this call's cost, which needs a token
        // estimate).
        match tracker.check_budget(0.0) {
            Ok(BudgetCheck::Exceeded {
                current_usd,
                limit_usd,
                period,
            }) => {
                tracing::warn!(
                    %current_usd, %limit_usd, ?period,
                    "[tinyagents::mw] cost budget exceeded — failing before model call"
                );
                Err(tinyagents::TinyAgentsError::LimitExceeded(format!(
                    "cost budget exceeded: {period:?} spend ${current_usd:.4} \u{2265} limit ${limit_usd:.4}"
                )))
            }
            Ok(BudgetCheck::Warning {
                current_usd,
                limit_usd,
                period,
            }) => {
                tracing::warn!(
                    %current_usd, %limit_usd, ?period,
                    "[tinyagents::mw] cost budget warning threshold reached"
                );
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Shadow parity check (W2-budget-dedupe). Enforcement already happened per
    /// call in `before_model`; here we only observe. Compares the observe-only
    /// crate `BudgetMiddleware`'s accumulated token spend against the runtime's
    /// authoritative `AgentRun::usage` and logs `[budget_shadow]` divergence.
    /// Never fails the run.
    async fn after_agent(
        &self,
        _ctx: &mut RunContext<()>,
        _state: &(),
        run: &mut AgentRun,
    ) -> TaResult<()> {
        let Some(tracker) = &self.shadow_tracker else {
            return Ok(());
        };
        let crate_usage = tracker.snapshot().usage; // UsageTotals (crate shadow)
        let local = run.usage; // UsageTotals (runtime authoritative)
        let l = &local.usage;
        let c = &crate_usage.usage;
        let diverged = l.input_tokens != c.input_tokens
            || l.output_tokens != c.output_tokens
            || l.cache_read_tokens != c.cache_read_tokens
            || l.total_tokens != c.total_tokens
            || local.calls != crate_usage.calls;
        if diverged {
            tracing::warn!(
                local_calls = local.calls,
                crate_calls = crate_usage.calls,
                local_in = l.input_tokens,
                crate_in = c.input_tokens,
                local_out = l.output_tokens,
                crate_out = c.output_tokens,
                local_cached = l.cache_read_tokens,
                crate_cached = c.cache_read_tokens,
                local_total = l.total_tokens,
                crate_total = c.total_tokens,
                "[budget_shadow] divergence: crate BudgetMiddleware token accounting differs from authoritative AgentRun.usage"
            );
        } else {
            tracing::debug!(
                calls = local.calls,
                input = l.input_tokens,
                output = l.output_tokens,
                cached = l.cache_read_tokens,
                total = l.total_tokens,
                "[budget_shadow] parity: crate BudgetMiddleware token accounting matches AgentRun.usage"
            );
        }
        Ok(())
    }
}

/// `after_tool`: stop (or nudge) the run when tool calls keep failing with no
/// progress (issue #4249). The legacy tool loop's progress guard surfaced a
/// root-cause halt summary — a security/approval denial re-issued unchanged, an
/// identical error retried, or *different* commands all failing — instead of
/// burning the whole iteration budget and ending on a generic cap error. The
/// tinyagents path kept only the model/tool call caps, so this reinstates the
/// guard as a graph middleware.
///
/// As of tinyagents 1.5.0 the escalation ladder itself lives in the crate
/// ([`NoProgressTracker`], extracted upstream from OpenHuman #4389). This
/// middleware is now a **thin driver**: it captures the per-call argument
/// fingerprint (the tool result carries no arguments), feeds each outcome into
/// [`NoProgressTracker::record`], and lowers the returned [`NoProgress`] verdict
/// into OpenHuman steering. It owns only the OpenHuman-side policy:
///
/// - [`NoProgress::Continue`] — do nothing.
/// - [`NoProgress::Nudge`] — inject the crate's structured "no progress since
///   step X" corrective into the working transcript via
///   [`SteeringCommand::Redirect`] so the next model call sees it and changes
///   strategy *before* the same-strategy retry cap trips.
/// - [`NoProgress::Halt`] — record the crate's root-cause summary into the shared
///   [`HaltSummarySlot`](super::HaltSummarySlot) (the turn overrides its final
///   text with it) and pause the run via the shared steering handle (same
///   mechanism as the stop-hook / cap pausers), then [`reset`](NoProgressTracker::reset)
///   so a resumed run does not immediately re-pause on the latched state.
pub(crate) struct RepeatedToolFailureMiddleware {
    handle: SteeringHandle,
    halt_summary: super::HaltSummarySlot,
    /// Crate no-progress escalation ladder — the single source of the
    /// identical-failure / varied-failure / hard-reject logic (tinyagents 1.5.0).
    tracker: NoProgressTracker,
    /// Monotonic tool-outcome counter, used only for the crate's "no progress
    /// since step X" nudge wording. Not the model-call count, but a stable,
    /// increasing marker is all the wording needs.
    step: AtomicUsize,
    /// call_id → argument fingerprint, captured in `before_tool` (the tool result
    /// carries no arguments). Folded into the identical-repeat signature so the
    /// "identical arguments" halt only trips on the *same* args — two different
    /// argument sets that happen to share a first error line don't count as a
    /// repeat and can't pre-empt the generic no-progress backstop.
    arg_sigs: std::sync::Mutex<std::collections::HashMap<String, String>>,
}

impl RepeatedToolFailureMiddleware {
    /// Build the breaker. `identical_threshold` (the identical-signature retry
    /// ceiling) is handed straight to [`NoProgressTracker::new`], which clamps it
    /// so a nudge always precedes a halt (a single failure is never a loop).
    pub(crate) fn new(
        handle: SteeringHandle,
        identical_threshold: usize,
        halt_summary: super::HaltSummarySlot,
    ) -> Self {
        Self {
            handle,
            halt_summary,
            tracker: NoProgressTracker::new(identical_threshold),
            step: AtomicUsize::new(0),
            arg_sigs: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }
}

/// A stable, bounded fingerprint of a tool call's arguments for the identical-
/// repeat signature (hashed so a huge payload doesn't bloat the map/comparison).
fn args_fingerprint(arguments: &serde_json::Value) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    arguments.to_string().hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

#[async_trait]
impl Middleware<()> for RepeatedToolFailureMiddleware {
    fn name(&self) -> &str {
        "repeated_tool_failure"
    }

    async fn before_tool(
        &self,
        _ctx: &mut RunContext<()>,
        _state: &(),
        call: &mut TaToolCall,
    ) -> TaResult<()> {
        // The tool result carries no arguments, so capture a fingerprint here and
        // correlate it by call_id in `after_tool`.
        if let Ok(mut sigs) = self.arg_sigs.lock() {
            sigs.insert(call.id.clone(), args_fingerprint(&call.arguments));
        }
        Ok(())
    }

    async fn after_tool(
        &self,
        _ctx: &mut RunContext<()>,
        _state: &(),
        result: &mut TaToolResult,
    ) -> TaResult<()> {
        let arg_fp = self
            .arg_sigs
            .lock()
            .ok()
            .and_then(|mut sigs| sigs.remove(&result.call_id))
            .unwrap_or_default();
        let step = self.step.fetch_add(1, Ordering::SeqCst) + 1;

        // A hard policy rejection is marked in the tool output; it can never
        // succeed when re-issued unchanged, so the crate ladder trips it faster.
        let hard_reject = result
            .content
            .contains(crate::openhuman::security::POLICY_BLOCKED_MARKER)
            || result
                .error
                .as_deref()
                .is_some_and(|err| err.contains(crate::openhuman::security::POLICY_BLOCKED_MARKER));

        let attempt = ToolAttempt {
            tool: &result.name,
            arg_fingerprint: &arg_fp,
            error: result.error.as_deref(),
            hard_reject,
            // The unknown-tool recovery sentinel is a C3 concern; today every
            // failure feeds the generic backstop exactly as the legacy ladder did.
            recoverable_miss: false,
        };

        match self.tracker.record(step, &attempt) {
            NoProgress::Continue => {}
            NoProgress::Nudge(instruction) => {
                tracing::warn!(
                    tool = %result.name,
                    step,
                    hard_reject,
                    "[tinyagents::mw] no-progress nudge — steering the model to change strategy before the retry cap"
                );
                // Inject the crate's structured corrective as a system message via
                // the `InjectMessage` steering lane. This runs on *every* turn,
                // including the user's live interactive turn, whose steering policy
                // permits only `InjectMessage`/`Pause` — `Redirect` is Background
                // (sub-agent) only, so sending it here aborted every interactive
                // turn that hit the nudge with `steering command redirect is not
                // permitted by the run policy` (a #4473 migration regression). The
                // corrective is trusted, system-generated advisory text, so the
                // `InjectMessage` lane is both permitted and semantically correct.
                self.handle
                    .send(SteeringCommand::InjectMessage(TaMessage::system(
                        instruction,
                    )));
            }
            NoProgress::Halt(summary) => {
                tracing::warn!(
                    tool = %result.name,
                    step,
                    hard_reject,
                    "[tinyagents::mw] repeated tool failure — halting run so the root cause surfaces"
                );
                if let Ok(mut slot) = self.halt_summary.lock() {
                    *slot = Some(summary);
                }
                // Pause at the top of the next iteration (before the next model
                // call), matching the stop-hook / cap pause path. Reset so a
                // resumed run does not immediately re-pause on the latched state
                // (the crate also resets internally on a halt; this is explicit
                // and idempotent).
                self.handle.send(SteeringCommand::Pause);
                self.tracker.reset();
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tinyagents::harness::context::{RunConfig, RunContext};
    use tinyagents::harness::model::ModelRequest;

    fn ctx() -> RunContext<()> {
        RunContext::new(RunConfig::new("mw-test"), ())
    }

    /// A minimal openhuman [`Tool`] for the tool-set–backed middlewares. Its
    /// `max_result_size_chars` and `external_effect` are configurable so the
    /// budget/approval resolution paths can be exercised.
    struct FakeTool {
        name: &'static str,
        cap: Option<usize>,
        external: bool,
    }

    #[async_trait]
    impl Tool for FakeTool {
        fn name(&self) -> &str {
            self.name
        }
        fn description(&self) -> &str {
            "fake"
        }
        fn parameters_schema(&self) -> serde_json::Value {
            json!({ "type": "object" })
        }
        async fn execute(
            &self,
            _args: serde_json::Value,
        ) -> anyhow::Result<crate::openhuman::tools::ToolResult> {
            Ok(crate::openhuman::tools::ToolResult::success("ok"))
        }
        fn max_result_size_chars(&self) -> Option<usize> {
            self.cap
        }
        fn external_effect_with_args(&self, _args: &serde_json::Value) -> bool {
            self.external
        }
    }

    fn tool_result(name: &str, content: &str) -> TaToolResult {
        TaToolResult {
            call_id: "c1".into(),
            name: name.into(),
            content: content.into(),
            raw: None,
            error: None,
            elapsed_ms: 0,
        }
    }

    // ── ToolOutcomeCaptureMiddleware policy-block enrichment (issue #4094) ───

    fn outcome_capture_mw() -> ToolOutcomeCaptureMiddleware {
        ToolOutcomeCaptureMiddleware::new(
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        )
    }

    #[tokio::test]
    async fn raw_security_policy_block_is_enriched_with_workaround_and_relay() {
        let mw = outcome_capture_mw();
        let mut result = tool_result(
            "run_command",
            "[policy-blocked] Security policy: read-only mode — only read commands are allowed",
        );
        result.error = Some(result.content.clone());
        mw.after_tool(&mut ctx(), &(), &mut result).await.unwrap();
        // The bare denial now carries a workaround + relay directive, and keeps the
        // marker so classification / the loop-breaker still recognise it.
        assert!(result.content.contains("Workaround:"), "{}", result.content);
        assert!(result.content.contains("Relay this to the user"));
        assert!(result
            .content
            .contains(crate::openhuman::security::POLICY_BLOCKED_MARKER));
        assert!(result.content.contains("read-only mode"));
    }

    #[tokio::test]
    async fn already_structured_denial_is_not_double_wrapped() {
        // A ToolPolicyMiddleware-style denial already has "Workaround:"; the capture
        // middleware must leave it untouched (no second Workaround block).
        let mw = outcome_capture_mw();
        let structured =
            "Blocked: Tool 'x' denied. Reason: nope. Workaround: do y. Relay this to the user: ...";
        let mut result = tool_result("x", structured);
        result.error = Some(result.content.clone());
        mw.after_tool(&mut ctx(), &(), &mut result).await.unwrap();
        assert_eq!(
            result.content.matches("Workaround:").count(),
            1,
            "must not double-wrap: {}",
            result.content
        );
    }

    // ── TurnContextMiddleware config ────────────────────────────────────────

    #[test]
    fn defaults_enable_the_byte_cap_only() {
        let mw = TurnContextMiddleware::defaults();
        assert_eq!(
            mw.tool_result_budget_bytes,
            DEFAULT_TOOL_RESULT_BUDGET_BYTES
        );
        assert!(mw.payload_summarizer.is_none());
        assert_eq!(mw.microcompact_keep_recent, 0);
        // Autocompaction defaults on (channel/sub-agent); the chat path overrides
        // it from config.
        assert!(mw.autocompact_enabled);
        // The byte cap alone is enough to make the bundle non-empty (CacheAlign
        // was deleted in C3, so it no longer contributes here).
        assert!(!mw.is_empty());
    }

    #[test]
    fn an_all_default_bundle_installs_nothing() {
        assert!(TurnContextMiddleware::default().is_empty());
    }

    #[test]
    fn tokenjuice_only_bundle_is_not_empty() {
        let mw = TurnContextMiddleware {
            tokenjuice_compaction_enabled: true,
            tokenjuice_compression: AgentTokenjuiceCompression::Light,
            ..Default::default()
        };
        assert!(!mw.is_empty());
    }

    // ── SuperContextMiddleware helpers ──────────────────────────────────────

    #[test]
    fn super_context_is_off_by_default() {
        assert!(TurnContextMiddleware::defaults().super_context.is_none());
        assert!(TurnContextMiddleware::default().super_context.is_none());
    }

    #[test]
    fn parse_bundle_sufficiency_reads_the_marker_case_insensitively() {
        assert_eq!(
            parse_context_bundle_has_enough_context(
                "[context_bundle]\nhas_enough_context: true\n[/context_bundle]"
            ),
            Some(true)
        );
        assert_eq!(
            parse_context_bundle_has_enough_context("HAS_ENOUGH_CONTEXT: false"),
            Some(false)
        );
        assert_eq!(
            parse_context_bundle_has_enough_context("[context_bundle]\nsummary: ok"),
            None
        );
    }

    #[test]
    fn prepend_folds_bundle_ahead_of_the_last_user_message_keeping_images() {
        use tinyagents::harness::message::ImageRef;
        let mut msgs = vec![TaMessage::system("sys"), {
            // A multimodal user turn: text + an image block.
            let mut u = TaMessage::user("original ask");
            if let TaMessage::User(m) = &mut u {
                m.content.push(ContentBlock::Image(ImageRef {
                    url: "data:image/png;base64,AAAA".into(),
                    mime_type: None,
                }));
            }
            u
        }];

        prepend_text_to_last_user(&mut msgs, "BUNDLE_BLOCK\n\n".to_string());

        let TaMessage::User(m) = &msgs[1] else {
            panic!("expected a user message");
        };
        // Bundle rides in front as a new leading text block.
        assert!(
            matches!(&m.content[0], ContentBlock::Text(t) if t.starts_with("BUNDLE_BLOCK")),
            "bundle should be the leading text block"
        );
        // Original text and the image both survive.
        assert!(m
            .content
            .iter()
            .any(|b| matches!(b, ContentBlock::Text(t) if t.contains("original ask"))));
        assert!(
            m.content
                .iter()
                .any(|b| matches!(b, ContentBlock::Image(_))),
            "the image block must survive the fold"
        );
        // System message untouched.
        assert_eq!(msgs[0].text(), "sys");
    }

    #[test]
    fn prepend_is_a_noop_without_a_user_message() {
        let mut msgs = vec![TaMessage::system("only system")];
        prepend_text_to_last_user(&mut msgs, "IGNORED".to_string());
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].text(), "only system");
    }

    // ── MicrocompactMiddleware (crate) ──────────────────────────────────────
    //
    // These assert the crate `MicrocompactMiddleware`, constructed with
    // OpenHuman's `CLEARED_PLACEHOLDER`, reproduces the deleted in-house
    // middleware byte-for-byte — the parity contract for the upstream swap.

    #[tokio::test]
    async fn microcompact_clears_older_tool_bodies_and_keeps_recent() {
        let mw = MicrocompactMiddleware::new(1, CLEARED_PLACEHOLDER);
        let mut req = ModelRequest::new(vec![
            TaMessage::system("sys"),
            TaMessage::user("hello"),
            TaMessage::tool("t1", "FIRST_BODY"),
            TaMessage::assistant("thinking"),
            TaMessage::tool("t2", "SECOND_BODY"),
            TaMessage::tool("t3", "THIRD_BODY"),
        ]);

        mw.before_model(&mut ctx(), &(), &mut req).await.unwrap();

        // 3 tool messages, keep_recent=1 → the two oldest cleared, newest kept.
        assert_eq!(req.messages[2].text(), CLEARED_PLACEHOLDER);
        assert_eq!(req.messages[4].text(), CLEARED_PLACEHOLDER);
        assert_eq!(req.messages[5].text(), "THIRD_BODY");
        // Non-tool messages are never touched.
        assert_eq!(req.messages[0].text(), "sys");
        assert_eq!(req.messages[1].text(), "hello");
        assert_eq!(req.messages[3].text(), "thinking");
    }

    #[tokio::test]
    async fn microcompact_is_a_noop_when_within_keep_recent() {
        let mw = MicrocompactMiddleware::new(5, CLEARED_PLACEHOLDER);
        let mut req =
            ModelRequest::new(vec![TaMessage::tool("t1", "A"), TaMessage::tool("t2", "B")]);
        mw.before_model(&mut ctx(), &(), &mut req).await.unwrap();
        assert_eq!(req.messages[0].text(), "A");
        assert_eq!(req.messages[1].text(), "B");
    }

    #[tokio::test]
    async fn microcompact_is_idempotent() {
        let mw = MicrocompactMiddleware::new(1, CLEARED_PLACEHOLDER);
        let mut req = ModelRequest::new(vec![
            TaMessage::tool("t1", "FIRST"),
            TaMessage::tool("t2", "SECOND"),
        ]);
        mw.before_model(&mut ctx(), &(), &mut req).await.unwrap();
        let after_first = req.messages[0].text();
        assert_eq!(after_first, CLEARED_PLACEHOLDER);
        // Second pass leaves the already-cleared body as the placeholder.
        mw.before_model(&mut ctx(), &(), &mut req).await.unwrap();
        assert_eq!(req.messages[0].text(), CLEARED_PLACEHOLDER);
        assert_eq!(req.messages[1].text(), "SECOND");
    }

    // ── ToolOutputMiddleware ────────────────────────────────────────────────

    #[tokio::test]
    async fn tool_output_truncates_over_the_flat_budget() {
        let mw = ToolOutputMiddleware {
            budget_bytes: 100,
            payload_summarizer: None,
            artifact_store: None,
            tokenjuice_compaction_enabled: false,
            tokenjuice_compression: AgentTokenjuiceCompression::Off,
            tool_policies: HashMap::new(),
        };
        let mut result = tool_result("echo", &"x".repeat(5_000));
        mw.after_tool(&mut ctx(), &(), &mut result).await.unwrap();
        assert!(result.content.len() < 5_000, "content should be capped");
        assert!(
            result.content.contains("truncated by tool_result_budget"),
            "a truncation marker should be appended: {}",
            result.content
        );
    }

    #[tokio::test]
    async fn tool_output_leaves_small_results_untouched() {
        let mw = ToolOutputMiddleware {
            budget_bytes: 1_000,
            payload_summarizer: None,
            artifact_store: None,
            tokenjuice_compaction_enabled: false,
            tokenjuice_compression: AgentTokenjuiceCompression::Off,
            tool_policies: HashMap::new(),
        };
        let mut result = tool_result("echo", "tiny");
        mw.after_tool(&mut ctx(), &(), &mut result).await.unwrap();
        assert_eq!(result.content, "tiny");
    }

    #[test]
    fn tool_char_cap_reads_the_tools_own_declared_cap() {
        let mut tool_policies = HashMap::new();
        tool_policies.insert(
            "big".to_string(),
            TaToolPolicy::classified().with_runtime(tinyagents::harness::tool::ToolRuntime {
                timeout_ms: None,
                max_retries: None,
                idempotent: false,
                cancelable: true,
                sandbox: tinyagents::harness::tool::SandboxMode::Inherit,
                max_result_bytes: Some(10),
                streaming: false,
            }),
        );
        let mw = ToolOutputMiddleware {
            budget_bytes: 1_000,
            payload_summarizer: None,
            artifact_store: None,
            tokenjuice_compaction_enabled: false,
            tokenjuice_compression: AgentTokenjuiceCompression::Off,
            tool_policies,
        };
        // Tool declares its own char cap → surfaced for the per-tool truncation.
        assert_eq!(mw.tool_char_cap("big"), Some(10));
        // Unknown tool → no per-tool cap (the flat byte budget applies instead).
        assert_eq!(mw.tool_char_cap("other"), None);
    }

    #[tokio::test]
    async fn tool_output_honors_a_tools_own_cap() {
        let mut tool_policies = HashMap::new();
        tool_policies.insert(
            "capped".to_string(),
            TaToolPolicy::classified().with_runtime(tinyagents::harness::tool::ToolRuntime {
                timeout_ms: None,
                max_retries: None,
                idempotent: false,
                cancelable: true,
                sandbox: tinyagents::harness::tool::SandboxMode::Inherit,
                max_result_bytes: Some(20),
                streaming: false,
            }),
        );
        let mw = ToolOutputMiddleware {
            budget_bytes: 100_000,
            payload_summarizer: None,
            artifact_store: None,
            tokenjuice_compaction_enabled: false,
            tokenjuice_compression: AgentTokenjuiceCompression::Off,
            tool_policies,
        };
        let mut result = tool_result("capped", &"y".repeat(500));
        mw.after_tool(&mut ctx(), &(), &mut result).await.unwrap();
        assert!(
            result
                .content
                .contains("truncated by tool cap: 480 more chars not shown"),
            "the tool's own 20-char cap should truncate with the tool-cap marker: {}",
            result.content
        );
    }

    // ── CostBudgetMiddleware ────────────────────────────────────────────────

    #[tokio::test]
    async fn cost_budget_is_a_noop_without_a_global_tracker() {
        // No global CostTracker is installed in the unit-test process, so the
        // gate self-disables and the model call proceeds.
        let mw = CostBudgetMiddleware::new();
        let mut req = ModelRequest::new(vec![TaMessage::user("hi")]);
        assert!(mw.before_model(&mut ctx(), &(), &mut req).await.is_ok());
    }

    // ── CostBudgetMiddleware shadow (W2-budget-dedupe) ──────────────────────

    /// The shadow comparison at `after_agent` logs parity when the crate
    /// `BudgetMiddleware`'s tracker matches the runtime `AgentRun.usage`, and
    /// never fails the run — in both the matching and diverging cases. It also
    /// must be inert (no panic, `Ok`) when no shadow tracker is installed.
    #[tokio::test]
    async fn cost_budget_shadow_after_agent_never_fails_the_run() {
        use tinyagents::harness::usage::Usage;

        // No shadow tracker: after_agent is a silent no-op.
        let plain = CostBudgetMiddleware::new();
        let mut run = AgentRun::new();
        run.usage.record(Usage::new(100, 40));
        assert!(plain.after_agent(&mut ctx(), &(), &mut run).await.is_ok());

        // Matching tracker (parity): the crate tracker accumulated the same
        // single call's usage the runtime recorded into `run.usage`.
        let tracker = BudgetTracker::new();
        tracker.record(Usage::new(100, 40), Default::default());
        let shadow = CostBudgetMiddleware::with_shadow(tracker.clone());
        let mut run = AgentRun::new();
        run.usage.record(Usage::new(100, 40));
        assert!(shadow.after_agent(&mut ctx(), &(), &mut run).await.is_ok());

        // Diverging tracker (crate missed a call): still only logs, never fails.
        let mut diverged_run = AgentRun::new();
        diverged_run.usage.record(Usage::new(100, 40));
        diverged_run.usage.record(Usage::new(10, 5));
        assert!(shadow
            .after_agent(&mut ctx(), &(), &mut diverged_run)
            .await
            .is_ok());
    }

    // ── RepeatedToolFailureMiddleware ───────────────────────────────────────

    fn failing_result(name: &str, err: &str) -> TaToolResult {
        let mut r = tool_result(name, err);
        r.error = Some(err.to_string());
        r
    }

    /// Count how many of the steering commands drained from `handle` are
    /// `Pause` (the halt signal). The tracker-driven breaker now also emits a
    /// `Redirect` **nudge** below the retry cap, so a raw `pending()` count no
    /// longer isolates the halt — the tests classify by command kind instead.
    fn drain_pause_count(handle: &SteeringHandle) -> usize {
        handle
            .drain()
            .into_iter()
            .filter(|c| matches!(c, SteeringCommand::Pause))
            .count()
    }

    #[tokio::test]
    async fn repeated_tool_failure_pauses_only_after_the_threshold() {
        let handle = SteeringHandle::allow_all();
        let mw = RepeatedToolFailureMiddleware::new(
            handle.clone(),
            3,
            std::sync::Arc::new(std::sync::Mutex::new(None)),
        );
        // Two identical failures: below the halt threshold. The crate ladder
        // nudges (Redirect) on the second, but must NOT pause (halt) yet.
        for _ in 0..2 {
            let mut r = failing_result("flaky", "boom");
            mw.after_tool(&mut ctx(), &(), &mut r).await.unwrap();
        }
        assert_eq!(
            drain_pause_count(&handle),
            0,
            "no halt before the threshold"
        );
        // Third identical failure exhausts the same-strategy retries → halt.
        let mut r = failing_result("flaky", "boom");
        mw.after_tool(&mut ctx(), &(), &mut r).await.unwrap();
        assert_eq!(
            drain_pause_count(&handle),
            1,
            "the third identical failure should pause (halt) the run"
        );
    }

    #[tokio::test]
    async fn repeated_tool_failure_resets_on_a_success() {
        let handle = SteeringHandle::allow_all();
        let mw = RepeatedToolFailureMiddleware::new(
            handle.clone(),
            3,
            std::sync::Arc::new(std::sync::Mutex::new(None)),
        );
        // Two failures, then a success clears the counter.
        for _ in 0..2 {
            let mut r = failing_result("t", "boom");
            mw.after_tool(&mut ctx(), &(), &mut r).await.unwrap();
        }
        let mut ok = tool_result("t", "fine"); // error = None
        mw.after_tool(&mut ctx(), &(), &mut ok).await.unwrap();
        // Two more failures — still below the halt threshold because the counter
        // reset, so the ladder never reaches the third identical repeat.
        for _ in 0..2 {
            let mut r = failing_result("t", "boom");
            mw.after_tool(&mut ctx(), &(), &mut r).await.unwrap();
        }
        assert_eq!(
            drain_pause_count(&handle),
            0,
            "a success should reset the breaker so it never halts"
        );
    }

    #[tokio::test]
    async fn repeated_tool_failure_ignores_distinct_errors() {
        let handle = SteeringHandle::allow_all();
        let mw = RepeatedToolFailureMiddleware::new(
            handle.clone(),
            3,
            std::sync::Arc::new(std::sync::Mutex::new(None)),
        );
        // Three *different* errors never trip the breaker — only an identical,
        // deterministic failure loop does (and the varied-failure backstop nudges
        // at 4 / halts at 6, both above this count).
        for err in ["e1", "e2", "e3"] {
            let mut r = failing_result("t", err);
            mw.after_tool(&mut ctx(), &(), &mut r).await.unwrap();
        }
        assert_eq!(
            handle.pending(),
            0,
            "distinct errors below the backstop must not steer the run"
        );
    }

    /// Collect the nudge system-message texts drained from `handle`. The nudge
    /// rides the `InjectMessage` lane (not `Redirect`) so it is permitted on the
    /// user's interactive turn — see the test below.
    fn drain_nudge_messages(handle: &SteeringHandle) -> Vec<String> {
        handle
            .drain()
            .into_iter()
            .filter_map(|c| match c {
                SteeringCommand::InjectMessage(message) => Some(message.text()),
                _ => None,
            })
            .collect()
    }

    #[tokio::test]
    async fn repeated_tool_failure_nudges_change_of_strategy_before_the_halt() {
        use crate::openhuman::tinyagents::orchestration::{
            openhuman_steering_handle, SteeringRunClass,
        };
        use tinyagents::harness::steering::SteeringCommandKind;

        // #4089: before the same-strategy retry cap, the breaker must feed a
        // structured "no progress since step X" corrective back into the loop so
        // the model changes approach rather than retrying the identical failing
        // call — and it must do so *without* pausing yet.
        let handle = SteeringHandle::allow_all();
        let mw = RepeatedToolFailureMiddleware::new(
            handle.clone(),
            3,
            std::sync::Arc::new(std::sync::Mutex::new(None)),
        );
        // First identical failure: not a loop yet — no steering.
        let mut r = failing_result("read_file", "file not found");
        mw.after_tool(&mut ctx(), &(), &mut r).await.unwrap();
        assert!(
            handle.drain().is_empty(),
            "a single failure is never a loop"
        );
        // Second identical failure: the nudge fires, still no halt.
        let mut r = failing_result("read_file", "file not found");
        mw.after_tool(&mut ctx(), &(), &mut r).await.unwrap();
        let nudges = drain_nudge_messages(&handle);
        assert_eq!(
            nudges.len(),
            1,
            "the repeat should steer the model to change strategy before the retry cap"
        );
        let nudge = &nudges[0];
        assert!(
            nudge.contains("no progress"),
            "the nudge carries the structured no-progress signal: {nudge}"
        );
        assert!(
            nudge.to_lowercase().contains("read_file"),
            "the nudge names the failing call so the model knows what not to repeat: {nudge}"
        );

        // Regression for the #4473 crash: the nudge must ride a steering lane the
        // user's *interactive* turn permits. `Redirect` is Background-only, so a
        // Redirect nudge aborted interactive turns; `InjectMessage` is permitted
        // on both classes. Assert the interactive policy accepts the lane we use.
        let interactive = openhuman_steering_handle(SteeringRunClass::Interactive);
        assert!(
            interactive
                .policy()
                .is_allowed(SteeringCommandKind::InjectMessage),
            "the no-progress nudge must use a lane the interactive turn permits"
        );
        assert!(
            !interactive
                .policy()
                .is_allowed(SteeringCommandKind::Redirect),
            "sanity: interactive still refuses Redirect (the lane that crashed it)"
        );
    }

    // ── ApprovalSecurityMiddleware ──────────────────────────────────────────

    #[test]
    fn approval_external_effect_resolution_walks_the_tool_sets() {
        let tools: Arc<Vec<Box<dyn Tool>>> = Arc::new(vec![
            Box::new(FakeTool {
                name: "send_email",
                cap: None,
                external: true,
            }),
            Box::new(FakeTool {
                name: "read_file",
                cap: None,
                external: false,
            }),
        ]);
        let mw = ApprovalSecurityMiddleware::new(vec![tools]);
        assert!(mw.has_external_effect("send_email", &json!({})));
        assert!(!mw.has_external_effect("read_file", &json!({})));
        // Unknown tool defaults to no external effect (nothing to gate).
        assert!(!mw.has_external_effect("missing", &json!({})));
    }

    // ── MemoryProtocolMiddleware (issue #4116) ──────────────────────────────

    use crate::openhuman::agent::harness::memory_protocol::MEMORY_PROTOCOL_MARKER;

    /// Drive one full tool cycle through the middleware: `before_tool` (captures
    /// the arguments the result won't carry) then `after_tool`, correlated by a
    /// shared call id. Returns the (possibly annotated) result.
    async fn run_cycle(
        mw: &MemoryProtocolMiddleware,
        name: &str,
        args: serde_json::Value,
        content: &str,
        error: Option<&str>,
    ) -> TaToolResult {
        let mut call = TaToolCall {
            id: "c1".into(),
            name: name.into(),
            arguments: args,
        };
        mw.before_tool(&mut ctx(), &(), &mut call).await.unwrap();
        let mut result = tool_result(name, content); // call_id "c1" matches
        result.error = error.map(|e| e.to_string());
        mw.after_tool(&mut ctx(), &(), &mut result).await.unwrap();
        result
    }

    #[tokio::test]
    async fn memory_write_without_index_read_gets_a_corrective_note() {
        let mw = MemoryProtocolMiddleware::new();
        let result = run_cycle(&mw, "memory_store", json!({}), "stored entry 42", None).await;
        assert!(
            result.content.contains(MEMORY_PROTOCOL_MARKER),
            "a write with no preceding dedupe read should be annotated: {}",
            result.content
        );
        assert!(result
            .content
            .contains("without first reading the memory index"));
        assert!(result.content.contains("update_memory_md"));
        // The original tool output is preserved, guidance is appended.
        assert!(result.content.starts_with("stored entry 42"));
    }

    #[tokio::test]
    async fn full_cycle_read_then_write_then_update_only_reminds_on_the_write() {
        let mw = MemoryProtocolMiddleware::new();

        let read = run_cycle(&mw, "memory_recall", json!({}), "no dupes", None).await;
        assert!(
            !read.content.contains(MEMORY_PROTOCOL_MARKER),
            "a read is not annotated"
        );

        let write = run_cycle(&mw, "memory_store", json!({}), "stored", None).await;
        assert!(write.content.contains(MEMORY_PROTOCOL_MARKER));
        // The read preceded the write, so no missing-read complaint — just the
        // forward "sync the index" reminder.
        assert!(!write
            .content
            .contains("without first reading the memory index"));

        let update = run_cycle(
            &mw,
            "update_memory_md",
            json!({ "file": "MEMORY.md" }),
            "index updated",
            None,
        )
        .await;
        assert!(
            !update.content.contains(MEMORY_PROTOCOL_MARKER),
            "closing the cycle needs no guidance"
        );
    }

    #[tokio::test]
    async fn skill_md_update_does_not_close_the_memory_cycle() {
        let mw = MemoryProtocolMiddleware::new();
        run_cycle(&mw, "memory_recall", json!({}), "checked", None).await;
        run_cycle(&mw, "memory_store", json!({}), "stored", None).await;
        // update_memory_md targeting SKILL.md must NOT reconcile the MEMORY.md
        // index, so the stale-index warning is still owed at run end.
        run_cycle(
            &mw,
            "update_memory_md",
            json!({ "file": "SKILL.md" }),
            "skill updated",
            None,
        )
        .await;
        let mut run = AgentRun::new();
        // Still pending → after_agent takes its warn path without erroring.
        mw.after_agent(&mut ctx(), &(), &mut run).await.unwrap();
        // A following write reports drift, proving pending was not cleared.
        let next = run_cycle(&mw, "memory_store", json!({}), "again", None).await;
        assert!(
            next.content.contains("drifting"),
            "SKILL.md update must not mask the stale MEMORY.md index: {}",
            next.content
        );
    }

    #[tokio::test]
    async fn consolidated_memory_tree_ingest_is_treated_as_a_write() {
        let mw = MemoryProtocolMiddleware::new();
        let ingest = run_cycle(
            &mw,
            "memory_tree",
            json!({ "mode": "ingest_document" }),
            "ingested",
            None,
        )
        .await;
        assert!(
            ingest.content.contains(MEMORY_PROTOCOL_MARKER),
            "memory_tree ingest_document is a write and must be annotated: {}",
            ingest.content
        );
    }

    #[tokio::test]
    async fn failed_memory_write_does_not_advance_the_protocol() {
        let mw = MemoryProtocolMiddleware::new();
        let failed = run_cycle(
            &mw,
            "memory_store",
            json!({}),
            "disk full",
            Some("disk full"),
        )
        .await;
        // A failed write is not annotated and leaves nothing pending, so a later
        // run-end sweep must not warn about a stale index.
        assert!(!failed.content.contains(MEMORY_PROTOCOL_MARKER));
        let mut run = AgentRun::new();
        // after_agent is a no-op warn path; it must not error.
        mw.after_agent(&mut ctx(), &(), &mut run).await.unwrap();
    }

    #[tokio::test]
    async fn second_write_without_an_update_flags_index_drift() {
        let mw = MemoryProtocolMiddleware::new();
        run_cycle(&mw, "memory_recall", json!({}), "checked", None).await;
        let first = run_cycle(&mw, "memory_store", json!({}), "a", None).await;
        assert!(!first.content.contains("drifting"));

        // No update_memory_md between the two writes → the index is drifting.
        let second = run_cycle(&mw, "memory_store", json!({}), "b", None).await;
        assert!(
            second.content.contains("drifting"),
            "a second unsynced write should flag index drift: {}",
            second.content
        );
    }
}

//! SHADOW tool-exposure middleware: drives the crate-native tool-selection
//! layer over the broad candidate set and logs any divergence from the set
//! OpenHuman actually registered (never enforced).

use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;

use tinyagents_harness::context::RunContext;
use tinyagents_harness::error::Result as TaResult;
use tinyagents_harness::middleware::{
    ContextualToolSelectionMiddleware, Middleware, ToolAllowlistMiddleware,
};
use tinyinference::model::ModelRequest;
use tinyinference::tool::ToolSchema;

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
pub(crate) struct OpenHumanToolExposureShadowMiddleware {
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
    /// Allowlist semantics are **fail-closed** (issue #4452): `None` means "no
    /// filter supplied → all candidates visible"; `Some(set)` means "exactly the
    /// named tools", so `Some(empty)` is a genuine deny-all. This mirrors the
    /// registration loop in `assemble_turn_harness`, keeping the shadow divergence
    /// reference in step with what OpenHuman actually registers as callable.
    pub(crate) fn new(
        candidate_names: &[String],
        allowed: Option<&std::collections::HashSet<String>>,
        tags: Vec<String>,
    ) -> Self {
        // Effective visible set: `None` → every candidate; `Some(set)` → exactly
        // the candidates named in `set` (empty set → none). Fail-closed: a
        // candidate absent from a supplied `allowed` is excluded (not exposed).
        let registered: std::collections::HashSet<String> = match allowed {
            None => candidate_names.iter().cloned().collect(),
            Some(set) => candidate_names
                .iter()
                .filter(|name| set.contains(*name))
                .cloned()
                .collect(),
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

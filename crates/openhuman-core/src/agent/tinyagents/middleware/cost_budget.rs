//! [`CostBudgetMiddleware`]: enforce daily/monthly cost budgets before a model
//! call spends, and shadow-compare token accounting against the crate
//! `BudgetMiddleware`.

use async_trait::async_trait;

use tinyagents_harness::context::RunContext;
use tinyagents_harness::error::Result as TaResult;
use tinyagents_harness::middleware::{AgentRun, BudgetTracker, Middleware};
use tinyinference::model::ModelRequest;

/// `before_model`: enforce OpenHuman's daily/monthly cost budgets **before** a
/// model call spends (issue #4249, Phase 5). Reads the global
/// [`CostTracker`](crate::platform::cost) and, when cost budgets are configured
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
/// [`BudgetMiddleware`](tinyagents_harness::middleware::BudgetMiddleware). It
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
        request: &mut ModelRequest,
    ) -> TaResult<()> {
        use crate::platform::cost::types::BudgetCheck;
        let Some(tracker) = crate::platform::cost::try_global() else {
            return Ok(());
        };

        // #5016: exempt the CURRENT request when it is BYOK, not just BYOK
        // history. Excluding BYOK from the managed totals alone still refuses a
        // mixed-route user's own-key calls once their managed spend has
        // legitimately crossed the cap — managed exhaustion would disable the
        // provider OpenHuman never bills for, which is the whole bug. Classify
        // this call's route and skip the gate when OpenHuman is not the biller.
        if let Some(model) = request.model.as_deref() {
            let route = crate::platform::cost::route::route_for_model(model);
            if !route.counts_toward_budget() {
                tracing::debug!(
                    %model,
                    ?route,
                    "[tinyagents::mw] BYOK/local route — skipping the managed budget gate (#5016)"
                );
                return Ok(());
            }
        }

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
                Err(tinyagents_harness::TinyAgentsError::LimitExceeded(format!(
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

//! Construction and run-local state helpers for the failure driver.
use super::*;

impl RepeatedToolFailureMiddleware {
    /// Build the breaker. `identical_threshold` (the identical-signature retry
    /// ceiling) is handed straight to [`NoProgressTracker::new`], which clamps it
    /// so a nudge always precedes a halt (a single failure is never a loop).
    pub(crate) fn new(
        handle: SteeringHandle,
        identical_threshold: usize,
        halt_summary: crate::agent::tinyagents::HaltSummarySlot,
    ) -> Self {
        Self {
            handle,
            halt_summary,
            tracker: NoProgressTracker::new(identical_threshold),
            classified: ClassifiedFailureTracker::default(),
            last_exit_report: std::sync::Mutex::default(),
            step: AtomicUsize::new(0),
            arg_sigs: std::sync::Mutex::new(std::collections::HashMap::new()),
            target_scopes: std::sync::Mutex::new(std::collections::HashMap::new()),
            recoverable_sig_counts: std::sync::Mutex::new(std::collections::HashMap::new()),
            recoverable_consecutive: AtomicU32::new(0),
            pending_nudges: Arc::new(Mutex::new(Vec::new())),
            call_effects: std::sync::Mutex::new(std::collections::HashMap::new()),
            tool_facts: None,
            recovery: None,
            recovery_shapes: Mutex::default(),
        }
    }

    /// Snapshot recovery settings and the host evaluator once for this run.
    pub(crate) fn with_recovery(mut self, config: &crate::config::Config) -> Self {
        self.recovery = Some(super::super::recovery_advice::RunRecovery::new(config));
        self
    }

    #[cfg(test)]
    pub(crate) fn with_recovery_evaluator(
        mut self,
        config: crate::config::schema::RecoveryConfig,
        evaluator: Arc<dyn tinytools_jev::recovery::RecoveryEvaluator>,
    ) -> Self {
        self.recovery = Some(super::super::recovery_advice::RunRecovery::with_evaluator(
            config, evaluator,
        ));
        self
    }

    pub(crate) fn set_recovery_registry(
        &self,
        registry: &tinyagents_harness::tool::ToolRegistry<
            (),
            crate::agent::tinyagents::host::OpenHumanRunContext,
        >,
        session: Option<&crate::tools::agent_policy::ToolPolicySession>,
    ) {
        self.set_recovery_candidates(super::super::recovery_advice::admitted_candidates(
            registry, session,
        ));
    }

    /// Attach only the assembly's admitted model-callable candidate snapshot.
    pub(crate) fn set_recovery_candidates(&self, candidates: Vec<tinytools_jev::JevOption>) {
        if let Some(recovery) = &self.recovery {
            recovery.set_candidates(candidates);
        }
    }

    /// Judge side effects from host registry declarations.
    pub(crate) fn with_tool_facts(mut self, lookup: ToolFactsLookup) -> Self {
        self.tool_facts = Some(lookup);
        self
    }

    /// The request-scoped half of this breaker. Register it **last**: its
    /// `before_model` must run after the transcript snapshot (which a failed
    /// turn persists) and after every reduction step.
    pub(crate) fn nudge_injector(&self) -> PendingNudgeInjector {
        PendingNudgeInjector {
            pending: self.pending_nudges.clone(),
        }
    }

    pub(super) fn queue_nudge(&self, instruction: impl Into<String>) {
        if let Ok(mut pending) = self.pending_nudges.lock() {
            pending.push(instruction.into());
        }
    }

    /// Drain the queued nudges (what the injector does before a request).
    #[cfg(test)]
    pub(crate) fn take_pending_nudges(&self) -> Vec<String> {
        self.pending_nudges
            .lock()
            .map(|mut pending| std::mem::take(&mut *pending))
            .unwrap_or_default()
    }

    /// Clear the consecutive recoverable-failure streak. Called on any success or
    /// non-recoverable failure (the per-signature identical counts persist across
    /// the turn, matching the legacy guard). Idempotent.
    pub(super) fn reset_recoverable_streak(&self) {
        self.recoverable_consecutive.store(0, Ordering::SeqCst);
    }

    /// Record one recoverable failure and return a root-cause halt summary once
    /// its extended headroom is exhausted (identical `>=` [`RECOVERABLE_REPEAT_FAILURE_THRESHOLD`]
    /// or consecutive `>=` [`RECOVERABLE_NO_PROGRESS_FAILURE_THRESHOLD`]).
    pub(super) fn record_recoverable(
        &self,
        tool: &str,
        arg_fp: &str,
        failure_text: &str,
    ) -> Option<String> {
        let key = format!("{tool}\u{1f}{arg_fp}");
        let count = self
            .recoverable_sig_counts
            .lock()
            .ok()
            .map(|mut counts| {
                let c = counts.entry(key).or_insert(0);
                *c += 1;
                *c
            })
            .unwrap_or(0);
        let consecutive = self.recoverable_consecutive.fetch_add(1, Ordering::SeqCst) + 1;
        tracing::debug!(
            tool,
            count,
            consecutive,
            "[tinyagents::mw] recoverable tool failure recorded with extended circuit-breaker headroom"
        );
        if count >= RECOVERABLE_REPEAT_FAILURE_THRESHOLD {
            return Some(recoverable_identical_halt_summary(
                tool,
                count,
                failure_text,
            ));
        }
        if consecutive >= RECOVERABLE_NO_PROGRESS_FAILURE_THRESHOLD {
            return Some(recoverable_no_progress_halt_summary(
                consecutive,
                tool,
                failure_text,
            ));
        }
        None
    }
}

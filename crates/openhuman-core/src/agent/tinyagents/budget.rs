//! Host budget policy, handed to the turn once at admission and carried
//! explicitly to children (`HostTurnEnv::model_budget` -> `OpenHumanRunContext`).

use std::sync::Arc;

use tinyinference_llm::model::ChatModel;

/// Product recursion ceiling shared by native and MCP delegation.
pub(crate) const MAX_SPAWN_DEPTH: usize = 3;
pub use tinyagents_harness::cost::{
    ControlledDispatchError, DispatchModelOptions, DispatchRefusal, DispatchRefusalReason,
    PhysicalDispatchAdmission, PhysicalDispatchPermit,
};
pub use tinyinference_llm::model::budget::{
    Budget, BudgetExceeded, BudgetSnapshot, CallBudget, Spend, SpendLimits,
};

/// OpenHuman's source-compatible carrier for the shared ledger and physical
/// call reservation bounds. The owner harness performs the wrapping engine.
#[derive(Debug, Clone)]
pub struct ModelBudget {
    /// Run/turn ledger. Children of a turn share it.
    pub ledger: Budget,
    /// Per-call input, output and charge upper bounds for all allowed routes.
    pub call: CallBudget,
}

impl ModelBudget {
    /// Wait for live reservations when settled spend still affords admission.
    /// Clones and child turns inherit this opt-in; internal retries stay fail-fast.
    pub fn wait_for_capacity(mut self) -> Self {
        self.ledger = self.ledger.wait_for_capacity();
        self
    }

    /// Wrap the model with TinyAgents' shared physical-call budget engine.
    pub(crate) fn wrap(&self, model: Arc<dyn ChatModel<()>>) -> Arc<dyn ChatModel<()>> {
        self.wrap_with_observer(model, None)
    }

    /// Wrap the model with budget accounting and an optional physical observer.
    pub(crate) fn wrap_with_observer(
        &self,
        model: Arc<dyn ChatModel<()>>,
        observer: Option<Arc<dyn tinyagents_harness::observability::PhysicalDispatchObserver>>,
    ) -> Arc<dyn ChatModel<()>> {
        tinyagents_harness::cost::ModelBudget::new(self.ledger.clone(), self.call)
            .wrap_model_with_observer(model, observer)
    }

    pub(crate) fn control(
        model: Arc<dyn ChatModel<()>>,
        budget: Option<&Self>,
        admission: Option<Arc<dyn PhysicalDispatchAdmission>>,
    ) -> Arc<dyn ChatModel<()>> {
        let monetary_budget = budget.map(|budget| {
            tinyagents_harness::cost::ModelBudget::new(budget.ledger.clone(), budget.call)
        });
        Arc::new(tinyagents_harness::cost::ControlledChatModel::new(
            model,
            DispatchModelOptions::new()
                .monetary_budget(monetary_budget)
                .admission(admission),
        ))
    }
}

/// Effective host depth ceiling, never wider than product policy.
pub(crate) fn depth(context: &super::host::OpenHumanRunContext) -> usize {
    context
        .max_spawn_depth
        .unwrap_or(crate::agent::tinyagents::budget::MAX_SPAWN_DEPTH)
        .min(crate::agent::tinyagents::budget::MAX_SPAWN_DEPTH)
}
pub(super) fn install_depth(
    harness: &mut tinyagents_harness::runtime::AgentHarness<(), super::host::OpenHumanRunContext>,
    context: &super::host::OpenHumanRunContext,
) {
    if let Some(max_depth) = context.max_spawn_depth {
        let mut policy = harness.policy().clone();
        policy.limits.max_depth = policy.limits.max_depth.min(max_depth);
        harness.with_policy(policy);
    }
}

impl crate::inference::turn_models::TurnModels {
    /// Wrap every concrete route and summarizer below harness retries/fallback.
    pub(crate) fn with_host_controls(
        mut self,
        budget: Option<&ModelBudget>,
        admission: Option<Arc<dyn PhysicalDispatchAdmission>>,
    ) -> Self {
        if budget.is_none() && admission.is_none() {
            return self;
        }
        self.primary = ModelBudget::control(self.primary, budget, admission.clone());
        self.summarizer = ModelBudget::control(self.summarizer, budget, admission.clone());
        for (_, route) in &mut self.routes {
            *route = ModelBudget::control(route.clone(), budget, admission.clone());
        }
        self
    }
}

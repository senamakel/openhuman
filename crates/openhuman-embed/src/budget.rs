//! Enforced shared budgets for completion calls, tool loops and synchronous children.
//!
//! Attach [`ModelBudget`] to a turn or completer. Use [`Budget::child`] for a
//! per-turn ledger charged to a shared review/run ledger. Each concrete route
//! and summarizer reserves before calling its provider, including every retry.
//! Unknown usage, errors and cancellation consume the full reservation. Cost
//! bounds are host-supplied upper bounds for every allowed route, not billing
//! estimates; providers cannot be forced to honour a monetary cap locally.
//!
//! Budgeted calls currently accept text only. Input bounds use serialized-byte
//! counts conservatively; multimodal requests fail before dispatch. Choose
//! bounds that include model framing and the maximum price on allowed routes.
pub use openhuman_core::agent::tinyagents::budget::{
    Budget, BudgetExceeded, BudgetSnapshot, CallBudget, ModelBudget, Spend, SpendLimits,
};

# 06 — Usage, cost, budgets

Finish moving accounting onto crate primitives; OpenHuman keeps the tracker
DB, dashboard RPC, and pricing policy.

Target SDK surface: `Usage { input, output, total, cache_read,
cache_creation, reasoning_tokens }`, `UsageTotals`, `CostTotals`,
`UsageAccountingMiddleware`, `BudgetMiddleware`/`BudgetLimits`/
`BudgetTracker`, `AgentEvent::{UsageRecorded, CostRecorded, BudgetWarning,
BudgetExceeded}`, `ChildRun.usage`/`RunTree` lineage rollup,
`ModelPricing` (catalog, incl. cache/reasoning rates).

The $0-cost bug and the unobserved-turn tracker gap are already fixed
(memory, sessions e/2026-07-01). Remaining:

## Steps

1. **Normalize records:** carry `reasoning_tokens` (crate `Usage` has it),
   image/audio/embedding usage where providers report; add
   `cost_source: estimated | provider_charged` to `TokenUsage`; keep
   provider-charged USD via an out-of-band carry (crate `Usage` has no USD
   field — residual upstream gap).
2. **Crate budget middleware:** replace bespoke `CostBudgetMiddleware`
   daily/monthly pre-checks with `BudgetMiddleware` where limits map; add
   per-run and per-thread budgets (new `CostConfig` fields + thread-id
   threading). Budget stop = `BudgetExceeded` event + typed run failure.
3. **Lineage rollup:** stamp cost records with `run_id`/`root_run_id` from
   the observation stream (needs `TokenUsage` schema fields); parent totals
   via `UsageTotals`/`ChildRun` instead of the `turn_subagent_usage`
   task-local where lineage suffices; keep the task-local only for detached
   children the tree can't see (or move them to TaskStore rollup — 07.2).
4. **Embedding usage:** embedding calls record usage/cost with provider,
   model, dimensions, vector count (ties into 09).

## Deletions

- `CostBudgetMiddleware` (if fully mapped), `agent/cost.rs` TurnCost
  aggregation where `UsageTotals` covers it, `turn_subagent_usage.rs`
  task-local (only after 07.2 rollup parity).

## Acceptance

- Dashboard totals identical before/after on a scripted multi-child turn
  (no double count); budget-exceeded stops a recursive run pre-spend.

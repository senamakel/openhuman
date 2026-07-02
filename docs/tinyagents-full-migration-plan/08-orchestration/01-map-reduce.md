# 08.1 — `run_parallel_fanout` → `graph::parallel::map_reduce`

The crate now has the ordered/bounded map-reduce helper the OpenHuman
helper was written to fill (`ParallelOptions`, `FailurePolicy`,
per-item `ItemOutcome`, cancellation).

Current status (2026-07-02): live fanout callers now invoke
`tinyagents::graph::parallel::map_reduce` directly, and
`tinyagents/orchestration.rs::run_parallel_fanout` has been removed. This is
compile-clean under the root, all-features root, and Tauri manifest checks.
Acceptance tests and usage-rollup parity are still pending.

## Steps

1. Compare `tinyagents/orchestration.rs::run_parallel_fanout` semantics
   (input-order results, bounded concurrency, take-once payload cell,
   graph events) with `map_reduce` — write a small equivalence test first.
2. Re-point callers: `spawn_parallel_agents` (until 08.2 lands),
   workflow phase intra-phase fanout (`workflow_runs/graph.rs`),
   model_council fanout.
3. Preserve the task-local usage-collector behavior: confirm `map_reduce`
   executes workers on the same task (`join_all`-style) or thread the
   06-cost lineage rollup instead — this is the one silent-regression risk
   (memory: collectors propagate because fanout is join_all on one task).
4. Choose `FailurePolicy` per caller (council = collect-all; workflow
   phase = fail-fast or per-phase config). Use the 1.3.0 options:
   `with_item_timeout`/`with_total_timeout`/`with_cancellation`
   (a per-item timeout is behavior the old helper never had — set it
   deliberately per caller, not by default).

## Deletions

- `run_parallel_fanout` + the move-once cell machinery in
  `tinyagents/orchestration.rs` (file keeps the TaskStore re-exports until
  07.2 relocates them).

## Acceptance

- Deterministic input-order results with out-of-order completion (test);
  council 24 + workflow_runs 17 suites green; usage rollup parity on a
  fanout turn.

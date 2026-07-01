# 08.1 — `run_parallel_fanout` → `graph::parallel::map_reduce`

The crate now has the ordered/bounded map-reduce helper the OpenHuman
helper was written to fill (`ParallelOptions`, `FailurePolicy`,
per-item `ItemOutcome`, cancellation).

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
   phase = fail-fast or per-phase config).

## Deletions

- `run_parallel_fanout` + the move-once cell machinery in
  `tinyagents/orchestration.rs` (file keeps the TaskStore re-exports until
  07.2 relocates them).

## Acceptance

- Deterministic input-order results with out-of-order completion (test);
  council 24 + workflow_runs 17 suites green; usage rollup parity on a
  fanout turn.

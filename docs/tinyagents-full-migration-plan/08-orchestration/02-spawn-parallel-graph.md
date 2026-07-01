# 08.2 — `spawn_parallel_agents` as a graph tool

The spec's Phase 6 node shape stands; land it on `Send` + reducers.

## Nodes

- `validate`: parse tasks, min/max count, parent context, `subagents.
  allowlist`, toolkit requirements, worktree preflight.
- `dispatch`: `Send` one worker invocation per task (payload-specific
  fanout — no static per-task nodes).
- `worker`: the 07.1 subagent pipeline subgraph with inherited policy,
  optional `worktree_action_dir`, child task id, bounded budgets
  (`SubAgentBudget`).
- `collect` (reducer/`ChannelSet`): successes, failures, elapsed,
  iterations, worktree status, changed files, stale-read markers —
  deterministic task order.
- `finalize`: compatibility `DomainEvent`/`AgentProgress` projections,
  overlap warnings, existing `parallel_agents` JSON shape.

## Steps

1. `build_spawn_parallel_graph` + topology export (established pattern).
2. Tool wrapper in `agent_orchestration/tools/spawn_parallel_agents.rs`
   shrinks to: parse args → run graph → format JSON.
3. Policy (spec "ownership and scheduling"): disjoint write ownership or
   reject/serial-fallback; read-only workers may share workspace;
   write-capable request worktree isolation (08.5); children share
   root run id, own task ids; no widening of tools/model/sandbox/budget;
   cancellation at graph boundaries.
4. Checkpointing optional (fanout = Async durability).

## Deletions

- Orchestration mechanics inside `spawn_parallel_agents.rs` (809 → thin
  wrapper); overlap-detection helpers move into `collect`.

## Acceptance

- spawn_parallel suite (14) green against identical JSON output;
  cancellable mid-fanout; graph status shows per-task lineage.

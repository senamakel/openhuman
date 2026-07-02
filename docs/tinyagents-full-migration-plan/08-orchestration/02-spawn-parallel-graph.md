# 08.2 — `spawn_parallel_agents` as a graph tool

The spec's Phase 6 node shape stands; land it on `Send` + reducers.

Current status (2026-07-02): `spawn_parallel_agents` already fans prepared
workers through `tinyagents::graph::parallel::map_reduce`, exports the fixed
`validate -> dispatch -> worker -> collect -> finalize` topology, and the graph
entrypoint now owns `Config.action_dir` resolution for worktree-isolated workers.
The graph now owns the live `validate`/`dispatch`/`worker`/`collect`/`finalize`
phases: the graph entrypoint resolves parent context and registry state,
validate parses task requests and enforces the parent `max_parallel_tools`
limit, dispatch validates/preflights workers from an owned agent-definition
snapshot, worker fanout still uses the SDK `map_reduce` helper, collect still
projects compatibility `DomainEvent`/`AgentProgress`, and finalize still
returns the existing JSON shape. The tool wrapper still owns `ToolResult`
translation so malformed-argument and public error shapes stay unchanged. The
unused pre-graph public wrappers have been removed, internal graph helpers have
been narrowed, and the remaining shrink target is the 1281-line graph
implementation.

## Nodes

- `validate`: parse tasks, enforce min/max count, and preserve malformed-args
  error ordering before parent-context lookup.
- `dispatch`: resolve per-task agent definitions, `subagents.allowlist`,
  toolkit requirements, ownership prompt boundaries, and worktree preflight.
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
2. Tool wrapper in `agent_orchestration/tools/spawn_parallel_agents.rs` is now
   a 128-line thin shell: schema → run graph → translate `ToolResult`.
3. Policy (spec "ownership and scheduling"): disjoint write ownership or
   reject/serial-fallback; read-only workers may share workspace;
   write-capable request worktree isolation (08.5); children share
   root run id, own task ids; no widening of tools/model/sandbox/budget;
   cancellation at graph boundaries.
4. Checkpointing optional (fanout = Async durability).

## Deletions

- Orchestration mechanics inside `spawn_parallel_agents.rs` have moved into
  `agent_orchestration/spawn_parallel_graph.rs`; overlap detection lives in the
  collect/finalize graph path.

## Acceptance

- Required before completion: spawn_parallel suite (14) green against identical JSON output;
  cancellable mid-fanout; graph status shows per-task lineage.

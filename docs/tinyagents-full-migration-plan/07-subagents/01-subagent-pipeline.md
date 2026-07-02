# 07.1 — Sub-agent pipeline as a subgraph

Convert `subagent_runner/` "prepare prompt → filter tools → run child →
checkpoint/handback → mirror transcript" into an explicit graph.

Current status (2026-07-02): `tinyagents/subagent_graph.rs` defines and exports
the fixed pipeline topology, and `run_typed_mode` now runs that graph as a
best-effort diagnostic skeleton before continuing through the procedural runner.
The skeleton records the named phases with `GraphTracingSink`; the node effects
remain pending and can be moved over one phase at a time without changing the
external sub-agent behavior.

## Steps

1. Define `build_subagent_pipeline_graph` (new
   `tinyagents/subagent_graph.rs` or under `subagent_runner/`): nodes
   `resolve_definition` (registry lookup + allowlist) → `prepare_context`
   (parent ctx, memory, action root, sandbox) → `assemble_prompt` →
   `expose_tools` (uses 01.3 selection middleware) → `run_child`
   (harness leaf via `subagent_node` or direct
   `run_turn_via_tinyagents_shared`) → `finalize` (checkpoint/
   awaiting-user handback, worker-thread mirror, handoff cache).
   Follow the established `build_*_graph` + `*_topology()` pattern; export
   in `all_graph_topologies()`.
2. `run_typed_mode` (`ops/runner.rs`, the single chokepoint) invokes the
   compiled subgraph; `AgentGraph::Custom` runners plug in as alternate
   `run_child` leaves.
3. Child lineage: run with parent depth/events (`invoke_in_parent`
   semantics) so `root_run_id` rollup + `SubAgentStarted/Completed/Reused`
   events are native; usage rollup via `ChildRun.usage` (feeds 06.3).
4. Fold `ops/{provider,prompt,checkpoint,handoff_helper,usage}.rs`
   plumbing into node implementations; `ops/graph.rs` shrinks to the leaf.
5. Reusable child sessions: map the follow-up/continue flows onto
   `SubAgentSession` (`send`, `transcript()`, `reset()`).

## Deletions

- `subagent_runner/ops/{usage,handoff_helper}.rs` glue; parts of
  `ops/runner.rs`/`ops/graph.rs` absorbed by nodes (target: dir shrinks
  from ~6.1k to policy nodes + tests).

## Acceptance

- Sub-agent e2e parity (ops_tests 1685-line suite green or rewritten
  against graph events); topology export validates; checkpoint/handback
  and worker-thread mirroring unchanged from the outside.

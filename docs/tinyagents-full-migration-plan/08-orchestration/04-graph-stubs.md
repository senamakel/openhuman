# 08.4 — Per-agent graphs: stubs out, bespoke in

Default-only `agent_registry/agents/*/graph.rs` stubs return
`AgentGraph::Default` (~13 lines each). Delete the boilerplate; keep the
seam; land at least one real custom graph before declaring the migration
complete so the cleanup does not remove an unproven seam.

Current status (2026-07-02): `BuiltinAgent.graph_fn` is optional and the
loader supplies `AgentGraph::Default` when it is absent. All 32
`agent_registry/agents/*/graph.rs` default stubs are gone, along with the five
default-only non-registry graph modules (`tinyplace_agent`, skill setup,
skill executor, agent memory, subconscious). A first bespoke production graph is
still pending specifically as a per-agent `AgentGraph::Custom` runner; other
production orchestration graph topologies (delegation/workflow/team/
spawn-parallel) already exist.

## Steps

1. Land the first bespoke per-agent graph — `orchestrator` (explicit plan/delegate/
   parallelize routing) or `researcher` (search → read → synthesize → cite,
   bounded + checkpointed) — as `AgentGraph::Custom` over a compiled graph
   with topology export. tool_maker (generate → validate → expose with
   review gates) is the third candidate.
2. Done: make `graph_fn` optional on `BuiltinAgent` (default =
   `AgentGraph::Default` supplied by the loader/registry); delete every default
   stub `graph.rs` whose agent has no custom graph.
3. Done: `agent.graph_topologies` returns an `agents` array showing which graph
   (`default`/`custom`) each built-in resolves to.

## Deletions

- Deleted: all default-only `agent_registry/agents/*/graph.rs` files and the
  `graph_fn` boilerplate wiring for them.
- Deleted: default-only graph modules for `tinyplace_agent`, `skill_setup`,
  `skill_executor`, `agent_memory`, and `subconscious`.

## Acceptance

- Registry loads all agents with correct graph resolution (test);
  at least one bespoke graph in production with route tests + topology
  snapshot.

# 08.4 — Per-agent graphs: stubs out, bespoke in

Default-only `agent_registry/agents/*/graph.rs` stubs return
`AgentGraph::Default` (~13 lines each). Delete the boilerplate; keep the
seam; land at least one real custom graph before declaring the migration
complete so the cleanup does not remove an unproven seam.

Current status (2026-07-02): `BuiltinAgent.graph_fn` is optional and the
loader supplies `AgentGraph::Default` when it is absent. All 32
`agent_registry/agents/*/graph.rs` default stubs are gone. Non-registry default
graph modules remain outside this row (`tinyplace_agent`, skill/agent-memory,
subconscious). A first bespoke production graph is still pending.

## Steps

1. Land the first bespoke graph — `orchestrator` (explicit plan/delegate/
   parallelize routing) or `researcher` (search → read → synthesize → cite,
   bounded + checkpointed) — as `AgentGraph::Custom` over a compiled graph
   with topology export. tool_maker (generate → validate → expose with
   review gates) is the third candidate.
2. Done: make `graph_fn` optional on `BuiltinAgent` (default =
   `AgentGraph::Default` supplied by the loader/registry); delete every
   registry stub `graph.rs` whose agent has no custom graph.
3. Registry diagnostics: `agent.graph_topologies` RPC already exists —
   extend it to show which graph (default/custom) each agent resolves to.

## Deletions

- Deleted: all default-only `agent_registry/agents/*/graph.rs` files and the
  `graph_fn` boilerplate wiring for them.

## Acceptance

- Registry loads all agents with correct graph resolution (test);
  at least one bespoke graph in production with route tests + topology
  snapshot.
